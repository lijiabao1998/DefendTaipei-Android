//! 萬華 — the city map, a port of the web game's `buildMap` world: the OSM footprints and ways
//! of Wanhua (© OpenStreetMap contributors, ODbL — see `ATTRIBUTION` / NOTICE), the infill
//! frontages, the procedural facades (arcades, iron window cages, shop signs, rooftop tin
//! shacks and water tanks), streets, trees and parked scooters.
//!
//! The world is cut into 256 m chunks. [`build_chunk`] turns one chunk into CPU-side meshes at a
//! [`Lod`]; [`spawn_chunk`] uploads them. The default streaming system keeps the chunks around the
//! player at the right LOD (built on the async compute pool, so nothing blocks a frame); a chunk
//! loader can switch it off with [`MapStreaming`] and call the two functions itself.

pub mod blocks;
pub mod build;
pub mod canvas;
pub mod collide;
pub mod data;
pub mod detail;
pub mod evac;
pub mod street;
pub mod textures;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

pub use build::Lod;
use build::{BuildingMeshes, Mat, Parts};
pub use collide::{Obstacles, SegmentHit};
pub use data::{ATTRIBUTION, WorldData};
use detail::WindowKit;
pub use evac::{BoatSpot, EvacMarker, EvacSites};
use textures::MapMaterials;

use crate::player::Player;

/// Chunk edge, metres.
pub const CHUNK: f32 = 256.0;
/// Near detail (window units, rooftop shacks) is built for buildings this close…
const DETAIL_IN: f32 = 70.0;
/// …and dropped again beyond this.
const DETAIL_OUT: f32 = 95.0;
/// A chunk whose rect is this close to the player is Near (per-building, fadeable)…
const NEAR_R: f32 = 60.0;
/// …this close Mid, else Far.
const MID_R: f32 = 260.0;

static WORLD: OnceLock<Arc<WorldData>> = OnceLock::new();

/// The planned Wanhua world (loaded once, shared by the map, collision and spawn code).
pub fn world() -> &'static Arc<WorldData> {
    WORLD.get_or_init(|| Arc::new(WorldData::load()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkKey(pub i32, pub i32);

impl ChunkKey {
    pub fn of(p: Vec2) -> Self {
        Self((p.x / CHUNK).floor() as i32, (p.y / CHUNK).floor() as i32)
    }
    pub fn rect(self) -> (Vec2, Vec2) {
        let min = Vec2::new(self.0 as f32, self.1 as f32) * CHUNK;
        (min, min + CHUNK)
    }
    pub fn dist(self, p: Vec2) -> f32 {
        let (a, b) = self.rect();
        (p - p.clamp(a, b)).length()
    }
    /// Every chunk that holds part of the map.
    pub fn all(w: &WorldData) -> Vec<ChunkKey> {
        let (a, b) = (Self::of(w.min - 40.0), Self::of(w.max + 40.0));
        (a.0..=b.0)
            .flat_map(|x| (a.1..=b.1).map(move |z| ChunkKey(x, z)))
            .collect()
    }
}

/// CPU-side geometry of one chunk.
pub struct ChunkMeshes {
    pub key: ChunkKey,
    pub lod: Lod,
    /// Ground, roads, props — and, at Mid / Far, every building merged in.
    pub merged: Parts,
    /// Near only: buildings kept separate so they can fade / lose their roof.
    pub buildings: Vec<(u32, BuildingMeshes)>,
}

/// Build one chunk. Buildings belong to the chunk holding their centroid; roads to the chunk
/// holding each segment's midpoint.
pub fn build_chunk(w: &WorldData, key: ChunkKey, lod: Lod) -> ChunkMeshes {
    let (min, max) = key.rect();
    let mut merged = Parts::default();
    street::ground(min, max, &mut merged);
    let junc = street::junctions(&w.roads);
    street::roads(w, &junc, min, max, lod == Lod::Far, &mut merged);
    street::props(&w.props, min, max, lod == Lod::Far, &mut merged);
    let mut buildings = Vec::new();
    for b in &w.blds {
        if b.c.x < min.x || b.c.y < min.y || b.c.x >= max.x || b.c.y >= max.y {
            continue;
        }
        let m = build::building(b, lod);
        if lod == Lod::Near {
            buildings.push((b.id, m));
        } else {
            merged.merge(m.walls);
            merged.merge(m.roof);
            merged.merge(m.base);
        }
    }
    ChunkMeshes {
        key,
        lod,
        merged,
        buildings,
    }
}

/// Tags the root entity of a spawned chunk.
#[derive(Component)]
pub struct Chunk {
    pub key: ChunkKey,
    pub lod: Lod,
}

/// Walls of one near building: fade to see-through when they hide the player (wallMatsT).
#[derive(Component, Clone)]
pub struct BldWalls {
    pub id: u32,
    pub mat: usize,
    pub faded: bool,
}

/// Roof / rooftop parts of a near building: hidden when it hides the player or holds them.
#[derive(Component, Clone)]
pub struct BldRoof(pub u32);

/// Window units and wall clutter of a near building.
#[derive(Component, Clone)]
pub struct BldDetail(pub u32);

/// Root of a building's near-detail entities.
#[derive(Component)]
pub struct DetailRoot(pub u32);

fn material(m: &MapMaterials, k: Mat) -> Handle<StandardMaterial> {
    match k {
        Mat::Wall(i) => m.walls[i as usize % m.walls.len()].clone(),
        Mat::Roof(i) => m.roofs[i as usize % m.roofs.len()].clone(),
        Mat::Tin => m.tin.clone(),
        Mat::Floor => m.floor.clone(),
        Mat::ArcadeFloor(i) => m.arcade_floor[i as usize % m.arcade_floor.len()].clone(),
        Mat::Column => m.column.clone(),
        Mat::Shutter => m.shutter.clone(),
        Mat::Ceiling => m.ceiling.clone(),
        Mat::Lamp => m.lamp.clone(),
        Mat::VCol => m.vcol.clone(),
        Mat::Metal => m.metal.clone(),
        Mat::Glass => m.glass.clone(),
        Mat::Sign => m.signs.clone(),
        Mat::VSign => m.vsigns.clone(),
        Mat::Brick => m.brick.clone(),
        Mat::Road => m.road.clone(),
        Mat::Ground => m.ground.clone(),
        Mat::Foliage => m.foliage.clone(),
        Mat::Marking => m.marking.clone(),
        Mat::Walk => m.walk.clone(),
        Mat::Curb => m.curb.clone(),
    }
}

/// Spawn one entity per material of `parts` under `parent`, each with `extra`.
fn spawn_parts<B: Bundle + Clone>(
    c: &mut ChildSpawnerCommands,
    meshes: &mut Assets<Mesh>,
    mats: &MapMaterials,
    parts: Parts,
    extra: B,
) {
    let mut v: Vec<_> = parts.0.into_iter().filter(|(_, m)| !m.is_empty()).collect();
    v.sort_by_key(|(k, _)| *k);
    for (k, m) in v {
        c.spawn((
            Mesh3d(meshes.add(m.into_mesh())),
            MeshMaterial3d(material(mats, k)),
            extra.clone(),
        ));
    }
}

/// Upload a built chunk. Returns the chunk's root entity.
pub fn spawn_chunk(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &MapMaterials,
    c: ChunkMeshes,
) -> Entity {
    commands
        .spawn((
            Chunk {
                key: c.key,
                lod: c.lod,
            },
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|root| {
            spawn_parts(root, meshes, mats, c.merged, ());
            for (id, b) in c.buildings {
                root.spawn((Transform::default(), Visibility::default()))
                    .with_children(|bp| {
                        let wall_mat = b.walls.0.keys().find_map(|k| match k {
                            Mat::Wall(i) => Some(*i as usize),
                            _ => None,
                        });
                        spawn_parts(
                            bp,
                            meshes,
                            mats,
                            b.walls,
                            (
                                BldWalls {
                                    id,
                                    mat: wall_mat.unwrap_or(0),
                                    faded: false,
                                },
                                Transform::default(),
                            ),
                        );
                        spawn_parts(
                            bp,
                            meshes,
                            mats,
                            b.roof,
                            (BldRoof(id), Visibility::default()),
                        );
                        spawn_parts(bp, meshes, mats, b.base, ());
                    });
            }
        })
        .id()
}

/// Turn the built-in streaming on / off (a dedicated chunk loader switches it off).
#[derive(Resource)]
pub struct MapStreaming {
    pub enabled: bool,
}

impl Default for MapStreaming {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// The map has its first chunk on screen (the loading cover can go).
#[derive(Resource, Default)]
pub struct MapReady {
    pub ready: bool,
    frames: u32,
}

#[derive(Resource, Default)]
struct Streamer {
    loaded: HashMap<ChunkKey, (Lod, Entity)>,
    pending: HashMap<ChunkKey, Task<ChunkMeshes>>,
}

#[derive(Resource, Default)]
struct DetailState {
    kit: Option<Arc<WindowKit>>,
    live: HashMap<u32, Entity>,
}

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        let w = world().clone();
        app.insert_resource(Obstacles(w.clone()))
            .init_resource::<MapStreaming>()
            .init_resource::<MapReady>()
            .init_resource::<Streamer>()
            .init_resource::<DetailState>()
            .add_systems(
                Startup,
                (
                    setup_map,
                    install_combat_world,
                    spawn_cover,
                    evac::spawn_evac,
                ),
            )
            .add_systems(
                Update,
                (
                    stream_chunks,
                    near_detail,
                    ready_check,
                    lift_cover,
                    evac::update_evac,
                ),
            )
            .add_systems(PostUpdate, cutaway.after(TransformSystems::Propagate));
    }
}

fn install_combat_world(
    obstacles: Res<Obstacles>,
    mut cw: ResMut<crate::combat::world::CombatWorldRes>,
) {
    cw.set(obstacles.clone());
}

fn setup_map(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(textures::build_materials(&mut images, &mut mats));
    // Sun. Shadows are off on Android: some GPUs crash with Bevy shadow maps
    // (bevyengine/bevy#8214), and it saves a lot of fill rate on phones.
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.93, 0.8),
            illuminance: 8_000.0,
            shadow_maps_enabled: !cfg!(target_os = "android"),
            ..default()
        },
        Transform::from_xyz(-40.0, 80.0, 25.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn want_lod(k: ChunkKey, p: Vec2) -> Lod {
    let d = k.dist(p);
    if d < NEAR_R {
        Lod::Near
    } else if d < MID_R {
        Lod::Mid
    } else {
        Lod::Far
    }
}

/// Keep every chunk at the LOD its distance asks for; build off-thread, swap in when done.
fn stream_chunks(
    mut commands: Commands,
    cfg: Res<MapStreaming>,
    mut st: ResMut<Streamer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mats: Option<Res<MapMaterials>>,
    player: Option<Single<&Transform, With<Player>>>,
) {
    let Some(mats) = mats else { return };
    if !cfg.enabled {
        return;
    }
    let p = player.map_or(world().spawn, |t| t.translation.xz());
    // finished builds
    let done: Vec<ChunkKey> = st.pending.keys().copied().collect();
    for k in done {
        let task = st.pending.get_mut(&k).unwrap();
        if let Some(c) = check_ready(task) {
            st.pending.remove(&k);
            let lod = c.lod;
            let e = spawn_chunk(&mut commands, &mut meshes, &mats, c);
            if let Some((_, old)) = st.loaded.insert(k, (lod, e)) {
                commands.entity(old).despawn();
            }
        }
    }
    // nearest first, a couple of builds in flight at a time
    let mut keys = ChunkKey::all(world());
    keys.sort_by(|a, b| a.dist(p).total_cmp(&b.dist(p)));
    for k in keys {
        if st.pending.len() >= 2 {
            break;
        }
        let lod = want_lod(k, p);
        if st.loaded.get(&k).is_some_and(|(l, _)| *l == lod) || st.pending.contains_key(&k) {
            continue;
        }
        let w = world().clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { build_chunk(&w, k, lod) });
        st.pending.insert(k, task);
    }
}

/// Lazy near layer: window units, wall clutter and rooftop shacks for buildings within ~70 m.
fn near_detail(
    mut commands: Commands,
    mut ds: ResMut<DetailState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mats: Option<Res<MapMaterials>>,
    player: Option<Single<&Transform, With<Player>>>,
) {
    let (Some(mats), Some(player)) = (mats, player) else {
        return;
    };
    let p = player.translation.xz();
    let w = world();
    let kit = ds
        .kit
        .get_or_insert_with(|| Arc::new(WindowKit::new()))
        .clone();
    let drop: Vec<u32> = ds
        .live
        .keys()
        .copied()
        .filter(|&id| w.blds[id as usize].c.distance(p) > DETAIL_OUT)
        .collect();
    for id in drop {
        if let Some(e) = ds.live.remove(&id) {
            commands.entity(e).despawn();
        }
    }
    let mut cand: Vec<u32> = w
        .blds_in(p - DETAIL_IN, p + DETAIL_IN)
        .into_iter()
        .filter(|id| !ds.live.contains_key(id) && w.blds[*id as usize].c.distance(p) < DETAIL_IN)
        .collect();
    cand.sort_by(|a, b| {
        w.blds[*a as usize]
            .c
            .distance(p)
            .total_cmp(&w.blds[*b as usize].c.distance(p))
    });
    for id in cand.into_iter().take(4) {
        let b = &w.blds[id as usize];
        let det = detail::near_detail(b, &kit);
        let roof = detail::near_roof(b);
        let e = commands
            .spawn((DetailRoot(id), Transform::default(), Visibility::default()))
            .with_children(|c| {
                spawn_parts(
                    c,
                    &mut meshes,
                    &mats,
                    det,
                    (BldDetail(id), Visibility::default()),
                );
                spawn_parts(
                    c,
                    &mut meshes,
                    &mats,
                    roof,
                    (BldRoof(id), Visibility::default()),
                );
            })
            .id();
        ds.live.insert(id, e);
    }
}

/// The building cutaway of main.ts: a building between the camera and the player (the camera
/// sits at +x +z) turns its walls see-through and drops its roof; the building the player is
/// in does too, and its walls shrink to a low ring.
fn cutaway(
    player: Option<Single<&Transform, With<Player>>>,
    mats: Option<Res<MapMaterials>>,
    mut walls: Query<
        (
            &mut BldWalls,
            &mut MeshMaterial3d<StandardMaterial>,
            &mut Transform,
        ),
        Without<Player>,
    >,
    mut roofs: Query<(&BldRoof, &mut Visibility), Without<BldDetail>>,
    mut details: Query<(&BldDetail, &mut Visibility), Without<BldRoof>>,
) {
    let (Some(player), Some(mats)) = (player, mats) else {
        return;
    };
    let p = player.translation.xz();
    let w = world();
    let inside = w.bld_at(p).map(|b| b.id);
    let mut front: HashSet<u32> = HashSet::new();
    for id in w.blds_in(p - 40.0, p + 40.0) {
        let b = &w.blds[id as usize];
        let d = b.c - p;
        if d.x.abs() < 40.0
            && d.y.abs() < 40.0
            && d.x + d.y > -4.0
            && d.x + d.y < 30.0
            && (d.x - d.y).abs() < 22.0
            && b.h > 4.0
        {
            front.insert(id);
        }
    }
    for (mut bw, mut mat, mut tf) in &mut walls {
        let f = front.contains(&bw.id) || inside == Some(bw.id);
        if f != bw.faded {
            bw.faded = f;
            mat.0 = if f {
                mats.walls_faded[bw.mat].clone()
            } else {
                mats.walls[bw.mat].clone()
            };
        }
        let sy = if inside == Some(bw.id) { 0.12 } else { 1.0 };
        if tf.scale.y != sy {
            tf.scale.y = sy;
        }
    }
    for (r, mut v) in &mut roofs {
        let hide = front.contains(&r.0) || inside == Some(r.0);
        v.set_if_neq(if hide {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
    for (d, mut v) in &mut details {
        let hide = front.contains(&d.0) || inside == Some(d.0);
        v.set_if_neq(if hide {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}

fn ready_check(
    mut ready: ResMut<MapReady>,
    st: Res<Streamer>,
    player: Option<Single<&Transform, With<Player>>>,
) {
    if ready.ready {
        return;
    }
    let p = player.map_or(world().spawn, |t| t.translation.xz());
    if st.loaded.contains_key(&ChunkKey::of(p)) {
        ready.frames += 1;
        if ready.frames > 8 {
            ready.ready = true;
        }
    }
}

/// Opaque cover shown from the very first frame until the streets around the player are on
/// screen: the launch used to show 1–3 s of bare clear colour while the city was built and
/// the pipelines compiled.
#[derive(Component)]
struct LoadingCover(f32);

fn spawn_cover(mut commands: Commands) {
    commands
        .spawn((
            LoadingCover(1.0),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.075, 0.07)),
            GlobalZIndex(100),
        ))
        .with_children(|c| {
            c.spawn((
                Text::new("保衛台北"),
                TextFont {
                    font_size: FontSize::Px(40.0),
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.78, 0.3)),
            ));
            c.spawn((
                Text::new("萬華"),
                TextFont {
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(Color::srgb(0.8, 0.78, 0.72)),
            ));
        });
}

fn lift_cover(
    mut commands: Commands,
    time: Res<Time>,
    ready: Res<MapReady>,
    mut cover: Query<(Entity, &mut LoadingCover, &mut BackgroundColor, &Children)>,
    mut texts: Query<&mut TextColor>,
) {
    if !ready.ready {
        return;
    }
    for (e, mut c, mut bg, kids) in &mut cover {
        c.0 -= time.delta_secs() / 0.35;
        if c.0 <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        bg.0.set_alpha(c.0);
        for k in kids.iter() {
            if let Ok(mut t) = texts.get_mut(k) {
                t.0.set_alpha(c.0);
            }
        }
    }
}
