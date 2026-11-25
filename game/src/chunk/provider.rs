//! Integration point for the map module.
//!
//! Two ways to supply tile content. Use one of them.
//!
//! 1. [`TileProvider`] — synchronous. `load` may spawn entities through
//!    [`TileCtx`] and returns a [`TileHandle`]. [`ChunkAppExt::register_tile_provider`]
//!    installs it. The debug view uses this.
//! 2. [`TileMailbox`] — asynchronous. When no provider is registered and
//!    streaming is enabled, each budget slot pushes a [`TileLoadRequest`]. The
//!    map builds the tile (a worker first, then the main thread) and pushes
//!    [`TileLoadFinished`] with the same `generation`. Until that arrives the
//!    previous LOD stays visible. A finish whose generation is no longer current
//!    becomes a [`TileUnloadRequest`]. The streamer does not despawn on this
//!    path, so colliders stay the map's to remove.
//!
//! Run the map system after [`super::ChunkSet`] in `Update`. Requests pushed
//! during a frame are visible to systems ordered after the streamer. Finishes
//! pushed during a frame are applied on the next streamer run.
//!
//! [`TileLoaded`] / [`TileUnloaded`] mirror the web bus (`lod` is 0 near / 1 mid
//! / 2 far). They fire only after a load has finished, never for a tile whose
//! first load failed or was dropped while still pending.
//!
//! The streamer does not build streets, buildings, or collision.
//! [`super::mid_buildings`] and [`super::far_terrain`] are helpers for the mid
//! and far meshes `mid.ts` builds from data the map already owns.

use std::any::Any;

use bevy::ecs::message::{Message, MessageWriter};
use bevy::prelude::*;

use super::compact::{CompactMaterial, CompactOptions, CompactStats, ReleaseStats, compact_mesh};
use super::config::ChunkConfig;
use super::grid::{TILE, TileCoord};
use super::lod::Lod;
use super::manager::{LoadResult, TileEvent, TileHooks, TileManager};

/// Opaque content for one loaded tile. The streamer stores it and hands it
/// back. `root`, when set, is the entity the provider spawned. The provider,
/// or the unload handler, is what despawns it.
pub struct TileHandle {
    pub root: Option<Entity>,
    pub data: Option<Box<dyn Any + Send + Sync>>,
}

impl TileHandle {
    pub fn entity(root: Entity) -> Self {
        Self {
            root: Some(root),
            data: None,
        }
    }
}

/// Synchronous failure inside `load` (the web `throw`).
#[derive(Debug)]
pub struct TileLoadError;

/// World access a provider gets while a tile is built or torn down.
///
/// The borrows last for that call. A provider that needs more than a slice of
/// the frame should do the heavy work before `load`, or use [`TileMailbox`]
/// and finish on a later frame.
pub struct TileCtx<'w, 's, 'a> {
    pub commands: &'a mut Commands<'w, 's>,
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub compact: &'a mut CompactStats,
    pub release: &'a mut ReleaseStats,
    pub compact_enabled: bool,
    pub release_cpu: bool,
    cookie: u64,
}

impl<'w, 's, 'a> TileCtx<'w, 's, 'a> {
    /// Insert `mesh`, packing it first when compact is on and the mesh is large
    /// enough. See [`compact_mesh`]. Stock `StandardMaterial` does not read
    /// packed normals or colours.
    pub fn add_mesh(&mut self, mut mesh: Mesh, material: &CompactMaterial) -> Handle<Mesh> {
        if self.compact_enabled {
            self.cookie = self.cookie.wrapping_add(1).max(1);
            let opts = CompactOptions {
                enabled: true,
                release: self.release_cpu,
                ..CompactOptions::default()
            };
            compact_mesh(
                &mut mesh,
                material,
                &opts,
                self.compact,
                self.release,
                self.cookie,
            );
        }
        self.meshes.add(mesh)
    }
}

/// What the map implements. Registered with [`ChunkAppExt::register_tile_provider`].
pub trait TileProvider: Send + Sync + 'static {
    /// `TileHooks.has`. Cells that return false are never requested.
    fn has(&self, _coord: TileCoord) -> bool {
        true
    }

    /// Build the tile at `lod`. Spawn with `ctx.commands` and return the handle.
    fn load(
        &mut self,
        coord: TileCoord,
        lod: Lod,
        ctx: &mut TileCtx<'_, '_, '_>,
    ) -> Result<TileHandle, TileLoadError>;

    /// Drop whatever [`Self::load`] created, including a stale finish.
    fn unload(&mut self, coord: TileCoord, handle: TileHandle, ctx: &mut TileCtx<'_, '_, '_>);
}

/// Holds the registered provider. Empty means "use [`TileMailbox`]".
#[derive(Resource, Default)]
pub struct TileProviderSlot {
    pub provider: Option<Box<dyn TileProvider>>,
}

pub trait ChunkAppExt {
    /// Install the map's tile provider. Replaces one registered earlier.
    /// The debug view fills the slot only when it is still empty.
    fn register_tile_provider<P: TileProvider>(&mut self, provider: P) -> &mut Self;
}

impl ChunkAppExt for App {
    fn register_tile_provider<P: TileProvider>(&mut self, provider: P) -> &mut Self {
        self.insert_resource(TileProviderSlot {
            provider: Some(Box::new(provider)),
        });
        self
    }
}

/// Optional focus override. `None` follows the player, or the spawn point
/// before the player exists.
#[derive(Resource, Default)]
pub struct StreamFocus {
    pub xz: Option<Vec2>,
}

/// Live streamer. Drive it through a provider or [`TileMailbox`].
#[derive(Resource, Default)]
pub struct StreamState {
    pub manager: TileManager<TileHandle>,
}

/// Per-update cost and how many tiles each LOD currently holds.
#[derive(Resource, Default, Clone, Debug)]
pub struct ChunkFrameStats {
    /// Duration of the latest manager update, in microseconds, including up to
    /// `budget` provider `load` calls.
    pub last_us: u128,
    /// Slowest update since launch.
    pub peak_us: u128,
    pub tiles: usize,
    pub near: usize,
    pub mid: usize,
    pub far: usize,
    pub last_starts: u32,
    pub pending: usize,
    /// Manager updates from the first request until the radius was resident.
    pub updates_to_settle: u32,
    pub settled: bool,
    pub budget: u32,
}

/// `tileLoaded`. `lod` is 0 / 1 / 2.
#[derive(Message, Clone, Copy, Debug)]
pub struct TileLoaded {
    pub tx: i32,
    pub tz: i32,
    pub lod: u8,
}

/// `tileUnloaded`.
#[derive(Message, Clone, Copy, Debug)]
pub struct TileUnloaded {
    pub tx: i32,
    pub tz: i32,
}

/// A budget slot wants this cell built. Push a [`TileLoadFinished`] with the
/// same `generation`. Used only when no [`TileProvider`] is registered.
#[derive(Clone, Copy, Debug)]
pub struct TileLoadRequest {
    pub tx: i32,
    pub tz: i32,
    pub lod: Lod,
    pub generation: u64,
}

/// Completion of a [`TileLoadRequest`]. `None` is a failed load.
pub struct TileLoadFinished {
    pub tx: i32,
    pub tz: i32,
    pub generation: u64,
    pub handle: Option<TileHandle>,
}

/// The streamer dropped this cell, or a stale finish came back. Remove its
/// entities and any collision records stored in the handle.
pub struct TileUnloadRequest {
    pub tx: i32,
    pub tz: i32,
    pub handle: TileHandle,
}

/// Queue between the streamer and a map that does not use [`TileProvider`].
///
/// The streamer drains `finished` and appends to `requests` / `unloads`.
/// The map drains `requests` / `unloads` and appends to `finished`.
#[derive(Resource, Default)]
pub struct TileMailbox {
    pub requests: Vec<TileLoadRequest>,
    pub unloads: Vec<TileUnloadRequest>,
    pub finished: Vec<TileLoadFinished>,
}

struct Effects {
    requests: Vec<TileLoadRequest>,
    unloads: Vec<TileUnloadRequest>,
}

struct Adapter<'w, 's, 'a> {
    provider: &'a mut Option<Box<dyn TileProvider>>,
    ctx: &'a mut TileCtx<'w, 's, 'a>,
    effects: &'a mut Effects,
}

impl TileHooks<TileHandle> for Adapter<'_, '_, '_> {
    fn has(&self, tx: i32, tz: i32) -> bool {
        match &self.provider {
            Some(provider) => provider.has(TileCoord::new(tx, tz)),
            None => true,
        }
    }

    fn load(&mut self, tx: i32, tz: i32, lod: Lod, generation: u64) -> LoadResult<TileHandle> {
        let Some(provider) = self.provider.as_mut() else {
            self.effects.requests.push(TileLoadRequest {
                tx,
                tz,
                lod,
                generation,
            });
            return LoadResult::Pending;
        };
        match provider.load(TileCoord::new(tx, tz), lod, self.ctx) {
            Ok(handle) => LoadResult::Ready(handle),
            Err(TileLoadError) => LoadResult::Failed,
        }
    }

    fn unload(&mut self, tx: i32, tz: i32, handle: TileHandle) {
        if let Some(provider) = self.provider.as_mut() {
            provider.unload(TileCoord::new(tx, tz), handle, self.ctx);
        } else {
            self.effects
                .unloads
                .push(TileUnloadRequest { tx, tz, handle });
        }
    }
}

pub(super) fn apply_env_config(mut config: ResMut<ChunkConfig>) {
    *config = ChunkConfig::from_env();
}

pub(super) fn stream_tiles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut slot: ResMut<TileProviderSlot>,
    mut state: ResMut<StreamState>,
    mut frame: ResMut<ChunkFrameStats>,
    mut compact: ResMut<CompactStats>,
    mut release: ResMut<ReleaseStats>,
    mut mailbox: ResMut<TileMailbox>,
    config: Res<ChunkConfig>,
    focus: Res<StreamFocus>,
    player: Query<&Transform, With<crate::player::Player>>,
    mut loaded_out: MessageWriter<TileLoaded>,
    mut unloaded_out: MessageWriter<TileUnloaded>,
) {
    if !config.enabled {
        return;
    }
    state.manager.budget = config.budget;
    state.manager.thresholds = config.thresholds();
    let xz = focus.xz.unwrap_or_else(|| {
        player
            .iter()
            .next()
            .map(|transform| transform.translation.xz())
            .unwrap_or_else(|| {
                let p = crate::player::spawn_point();
                Vec2::new(p.x, p.z)
            })
    });

    let mut effects = Effects {
        requests: Vec::new(),
        unloads: Vec::new(),
    };
    let incoming = std::mem::take(&mut mailbox.finished);
    {
        let mut ctx = context(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut compact,
            &mut release,
            &config,
        );
        let mut adapter = Adapter {
            provider: &mut slot.provider,
            ctx: &mut ctx,
            effects: &mut effects,
        };
        for fin in incoming {
            state.manager.resolve(
                fin.tx,
                fin.tz,
                fin.generation,
                fin.handle.ok_or(()),
                &mut adapter,
            );
        }
    }

    // The screenshot harness fills the radius before the first present so the
    // picture is the settled rings. Gameplay stays at one update per frame.
    let catch_up = config.shot_path.is_some() && !frame.settled;
    let reach = config.drop + config.hyst;
    let radius = (reach / TILE).ceil() as u32;
    let span = radius.saturating_mul(2).saturating_add(1);
    let cap = span.saturating_mul(span) / config.budget.max(1) + 2;
    let mut guard = 0u32;
    loop {
        let us = {
            let mut ctx = context(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut compact,
                &mut release,
                &config,
            );
            let mut adapter = Adapter {
                provider: &mut slot.provider,
                ctx: &mut ctx,
                effects: &mut effects,
            };
            let started = std::time::Instant::now();
            state.manager.update(xz.x, xz.y, &mut adapter);
            started.elapsed().as_micros()
        };
        frame.last_us = us;
        frame.peak_us = frame.peak_us.max(us);
        if !frame.settled {
            frame.updates_to_settle += 1;
        }
        guard += 1;
        if !catch_up || state.manager.last_starts == 0 || guard > cap {
            break;
        }
    }

    for event in state.manager.events.drain(..) {
        match event {
            TileEvent::Loaded { tx, tz, lod } => {
                loaded_out.write(TileLoaded { tx, tz, lod });
            }
            TileEvent::Unloaded { tx, tz } => {
                unloaded_out.write(TileUnloaded { tx, tz });
            }
        }
    }
    mailbox.requests.append(&mut effects.requests);
    mailbox.unloads.append(&mut effects.unloads);

    let mut near = 0;
    let mut mid = 0;
    let mut far = 0;
    let mut pending = 0;
    for entry in state.manager.tiles.values() {
        if entry.pending && entry.obj.is_none() {
            pending += 1;
        }
        if entry.obj.is_none() {
            continue;
        }
        match entry.lod {
            Lod::Near => near += 1,
            Lod::Mid => mid += 1,
            Lod::Far => far += 1,
        }
    }
    frame.tiles = near + mid + far;
    frame.near = near;
    frame.mid = mid;
    frame.far = far;
    frame.pending = pending;
    frame.last_starts = state.manager.last_starts;
    frame.budget = config.budget;
    if !frame.settled && frame.tiles > 0 && pending == 0 && state.manager.last_starts == 0 {
        frame.settled = true;
        eprintln!(
            "chunk-lod settled tiles={} near={} mid={} far={} updates={} last_us={} peak_us={} budget={}",
            frame.tiles,
            frame.near,
            frame.mid,
            frame.far,
            frame.updates_to_settle,
            frame.last_us,
            frame.peak_us,
            frame.budget
        );
    }
}

fn context<'w, 's, 'a>(
    commands: &'a mut Commands<'w, 's>,
    meshes: &'a mut Assets<Mesh>,
    materials: &'a mut Assets<StandardMaterial>,
    compact: &'a mut CompactStats,
    release: &'a mut ReleaseStats,
    config: &ChunkConfig,
) -> TileCtx<'w, 's, 'a> {
    TileCtx {
        commands,
        meshes,
        materials,
        compact,
        release,
        compact_enabled: config.compact,
        release_cpu: config.release_cpu,
        cookie: 0,
    }
}
