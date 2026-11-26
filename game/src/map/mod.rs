//! 萬華 — the city map, a port of the web game's `buildMap` world: the OSM footprints and ways
//! of Wanhua (© OpenStreetMap contributors, ODbL — see `ATTRIBUTION` / NOTICE), the infill
//! frontages, the procedural facades (arcades, iron window cages, shop signs, rooftop tin
//! shacks and water tanks), streets, trees and parked scooters.
//!
//! The world is cut into 256 m chunks. [`build_chunk`] turns one chunk into CPU-side meshes at a
//! [`Lod`]; [`spawn_chunk`] uploads them. The default streaming system keeps the chunks around the
//! player at the right LOD (built on the async compute pool, so nothing blocks a frame); a chunk
//! loader can switch it off with [`MapStreaming`] and call the two functions itself.

pub mod atmosphere;
pub mod blocks;
pub mod build;
pub mod canvas;
pub mod collide;
pub mod data;
pub mod detail;
pub mod evac;
pub mod exterior;
pub mod interiors;
pub mod roofs;
pub mod street;
pub mod textures;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
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
/// Reuse spatial work while standing still without changing any LOD boundary.
const CHUNK_ORDER_REFRESH_METRES: f32 = 4.0;
const DETAIL_QUERY_REFRESH_METRES: f32 = 1.0;

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
        let mut m = build::building(b, lod);
        m.structure = exterior::structure(b, w, lod);
        if lod == Lod::Near {
            buildings.push((b.id, m));
        } else {
            merged.merge(m.walls);
            merged.merge(m.roof);
            merged.merge(m.base);
            merged.merge(m.structure);
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

/// Exact CPU mesh metadata retained at upload. Mesh vertex arrays are released
/// after GPU extraction; QA must not call `Mesh::count_vertices()` on them later.
#[derive(Component, Clone, Copy)]
pub(crate) struct MapMeshGeometry {
    pub vertices: usize,
    pub min: Vec3,
    pub max: Vec3,
}

fn material(m: &MapMaterials, k: Mat) -> Handle<StandardMaterial> {
    match k {
        Mat::Wall(i) => m.walls[i as usize % m.walls.len()].clone(),
        Mat::Roof(i) => m.roofs[i as usize % m.roofs.len()].clone(),
        Mat::Tin => m.tin.clone(),
        Mat::Floor => m.floor.clone(),
        Mat::FacadeTile => m.facade_tile.clone(),
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
        let (min, max) = m.aabb();
        let geometry = MapMeshGeometry {
            vertices: m.vcount(),
            min,
            max,
        };
        c.spawn((
            Mesh3d(meshes.add(m.into_mesh())),
            MeshMaterial3d(material(mats, k)),
            geometry,
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
                        spawn_parts(
                            bp,
                            meshes,
                            mats,
                            b.structure,
                            (BldDetail(id), Visibility::default()),
                        );
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

#[derive(Resource)]
struct Streamer {
    loaded: HashMap<ChunkKey, (Lod, Entity)>,
    pending: HashMap<ChunkKey, Task<ChunkMeshes>>,
    /// Map extent is immutable. Reuse this allocation and near-first ordering.
    keys: Vec<ChunkKey>,
    order_anchor: Option<Vec2>,
}

impl Default for Streamer {
    fn default() -> Self {
        Self {
            loaded: HashMap::new(),
            pending: HashMap::new(),
            keys: ChunkKey::all(world()),
            order_anchor: None,
        }
    }
}

#[derive(Resource, Default)]
struct DetailState {
    kit: Option<Arc<WindowKit>>,
    live: HashMap<u32, Entity>,
    candidates: VecDeque<u32>,
    query_anchor: Option<Vec2>,
    last_position: Option<Vec2>,
}

/// Local renderer workload counts, not GPU time or memory estimates.
#[derive(Resource, Clone, Debug, Default, Serialize)]
pub struct MapRenderStats {
    pub resident_chunks: usize,
    pub pending_chunks: usize,
    pub detail_buildings: usize,
    pub chunk_uploads: usize,
    pub detail_uploads: usize,
    pub chunk_order_rebuilds: u64,
    pub detail_candidate_queries: u64,
}

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        let w = world().clone();
        app.add_plugins(interiors::InteriorPlugin)
            .insert_resource(Obstacles::new(w.clone()))
            .init_resource::<MapStreaming>()
            .init_resource::<MapReady>()
            .init_resource::<Streamer>()
            .init_resource::<DetailState>()
            .init_resource::<MapRenderStats>()
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

fn spatial_refresh_due(anchor: Option<Vec2>, position: Vec2, distance: f32) -> bool {
    anchor.is_none_or(|old| old.distance_squared(position) >= distance * distance)
}

/// The extra metre discovers candidates that can cross the original 70 m
/// boundary before the next spatial query. Admission still checks exactly
/// DETAIL_IN; pending candidates outside it remain available for small moves.
fn detail_candidates(w: &WorldData, p: Vec2, live: &HashMap<u32, Entity>) -> VecDeque<u32> {
    let discovery_radius = DETAIL_IN + DETAIL_QUERY_REFRESH_METRES;
    let mut candidates: Vec<_> = w
        .blds_in(p - discovery_radius, p + discovery_radius)
        .into_iter()
        .filter(|id| {
            !live.contains_key(id)
                && w.blds[*id as usize].c.distance_squared(p) <= discovery_radius * discovery_radius
        })
        .collect();
    candidates.sort_by(|a, b| {
        w.blds[*a as usize]
            .c
            .distance_squared(p)
            .total_cmp(&w.blds[*b as usize].c.distance_squared(p))
            .then_with(|| a.cmp(b))
    });
    candidates.into()
}

fn next_detail_candidate(candidates: &mut VecDeque<u32>, w: &WorldData, p: Vec2) -> Option<u32> {
    // A fixed traversal bound prevents an outside-only backlog from spinning.
    for _ in 0..candidates.len() {
        let id = candidates.pop_front()?;
        if w.blds[id as usize].c.distance_squared(p) < DETAIL_IN * DETAIL_IN {
            return Some(id);
        }
        candidates.push_back(id);
    }
    None
}

/// Keep every chunk at the LOD its distance asks for; build off-thread, swap in when done.
fn stream_chunks(
    mut commands: Commands,
    cfg: Res<MapStreaming>,
    mut st: ResMut<Streamer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mats: Option<Res<MapMaterials>>,
    player: Option<Single<&Transform, With<Player>>>,
    mut stats: ResMut<MapRenderStats>,
) {
    stats.chunk_uploads = 0;
    stats.resident_chunks = st.loaded.len();
    stats.pending_chunks = st.pending.len();
    let Some(mats) = mats else { return };
    if !cfg.enabled {
        return;
    }
    let p = player.map_or(world().spawn, |t| t.translation.xz());
    // finished builds
    let mut done: Vec<ChunkKey> = st.pending.keys().copied().collect();
    done.sort_by(|a, b| a.dist(p).total_cmp(&b.dist(p)).then_with(|| a.cmp(b)));
    let upload_budget = if cfg!(target_os = "android") { 1 } else { 2 };
    for k in done {
        if stats.chunk_uploads >= upload_budget {
            break;
        }
        let task = st.pending.get_mut(&k).unwrap();
        if let Some(c) = check_ready(task) {
            st.pending.remove(&k);
            let lod = c.lod;
            let e = spawn_chunk(&mut commands, &mut meshes, &mats, c);
            if let Some((_, old)) = st.loaded.insert(k, (lod, e)) {
                commands.entity(old).despawn();
            }
            stats.chunk_uploads += 1;
        }
    }
    // nearest first, a couple of builds in flight at a time
    if spatial_refresh_due(st.order_anchor, p, CHUNK_ORDER_REFRESH_METRES) {
        st.keys
            .sort_by(|a, b| a.dist(p).total_cmp(&b.dist(p)).then_with(|| a.cmp(b)));
        st.order_anchor = Some(p);
        stats.chunk_order_rebuilds += 1;
    }
    for index in 0..st.keys.len() {
        if st.pending.len() >= 2 {
            break;
        }
        let k = st.keys[index];
        // Cached priority does not cache LOD: crossing a boundary still starts
        // the right build immediately, including a movement smaller than 4 m.
        let lod = want_lod(k, p);
        if st.loaded.get(&k).is_some_and(|(l, _)| *l == lod) || st.pending.contains_key(&k) {
            continue;
        }
        let w = world().clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { build_chunk(&w, k, lod) });
        st.pending.insert(k, task);
    }
    stats.resident_chunks = st.loaded.len();
    stats.pending_chunks = st.pending.len();
}

/// Lazy near layer: window units, wall clutter and rooftop shacks for buildings within ~70 m.
fn near_detail(
    mut commands: Commands,
    mut ds: ResMut<DetailState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mats: Option<Res<MapMaterials>>,
    player: Option<Single<&Transform, With<Player>>>,
    mut stats: ResMut<MapRenderStats>,
    performance: Option<Res<crate::performance::EffectivePerformance>>,
) {
    stats.detail_uploads = 0;
    stats.detail_buildings = ds.live.len();
    let (Some(mats), Some(player)) = (mats, player) else {
        return;
    };
    let p = player.translation.xz();
    let w = world();
    let kit = ds
        .kit
        .get_or_insert_with(|| Arc::new(WindowKit::new()))
        .clone();
    if ds.last_position != Some(p) {
        let drop: Vec<u32> = ds
            .live
            .keys()
            .copied()
            .filter(|&id| w.blds[id as usize].c.distance_squared(p) > DETAIL_OUT * DETAIL_OUT)
            .collect();
        for id in drop {
            if let Some(e) = ds.live.remove(&id) {
                commands.entity(e).despawn();
            }
        }
        ds.last_position = Some(p);
    }
    if spatial_refresh_due(ds.query_anchor, p, DETAIL_QUERY_REFRESH_METRES) {
        ds.candidates = detail_candidates(w, p, &ds.live);
        ds.query_anchor = Some(p);
        stats.detail_candidate_queries += 1;
    }
    // Use the controller's effective app cadence. Keep
    // uploads bounded while still draining the queue when the player stands
    // still; the final street/facade detail is identical at either budget.
    let target_fps = performance.as_deref().map_or(60, |value| value.target_fps);
    let budget = if (1..=30).contains(&target_fps) { 1 } else { 2 };
    for _ in 0..budget {
        let Some(id) = next_detail_candidate(&mut ds.candidates, w, p) else {
            break;
        };
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
        stats.detail_uploads += 1;
    }
    stats.detail_buildings = ds.live.len();
}

#[cfg(test)]
mod streaming_work_tests {
    use super::*;

    #[test]
    fn architectural_materials_and_roofs_follow_cutaway_without_wall_material_swaps() {
        let data = world();
        let b = data
            .blds
            .iter()
            .find(|b| {
                exterior::classify(b) == exterior::Typology::Walkup
                    && !exterior::plan(b, data).balconies.is_empty()
                    && data.bld_at(b.c).is_some_and(|inside| inside.id == b.id)
            })
            .expect("real walkup with a clear centre and balconies");
        let mut building = build::building(b, Lod::Near);
        building.structure = exterior::structure(b, data, Lod::Near);
        assert!(building.structure.0.contains_key(&Mat::FacadeTile));
        assert!(building.structure.0.contains_key(&Mat::Metal));
        let expected_vertices = building.structure.vcount();

        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mats = textures::build_materials(&mut images, &mut materials);
        let mut meshes = Assets::<Mesh>::default();
        let mut app = App::new();
        app.insert_resource(mats.clone())
            .add_systems(Update, cutaway);
        spawn_chunk(
            &mut app.world_mut().commands(),
            &mut meshes,
            &mats,
            ChunkMeshes {
                key: ChunkKey::of(b.c),
                lod: Lod::Near,
                merged: Parts::default(),
                buildings: vec![(b.id, building)],
            },
        );
        app.world_mut().flush();
        let outside = data.max + Vec2::splat(100.0);
        let player = app
            .world_mut()
            .spawn((Player, Transform::from_xyz(outside.x, 0.0, outside.y)))
            .id();
        let architecture: HashMap<_, _> = app
            .world_mut()
            .query_filtered::<(Entity, &MeshMaterial3d<StandardMaterial>), With<BldDetail>>()
            .iter(app.world())
            .map(|(entity, material)| (entity, material.0.clone()))
            .collect();
        assert_eq!(architecture.len(), 3);
        let uploaded_vertices: usize = app
            .world_mut()
            .query_filtered::<&MapMeshGeometry, With<BldDetail>>()
            .iter(app.world())
            .map(|geometry| geometry.vertices)
            .sum();
        assert_eq!(uploaded_vertices, expected_vertices);

        for (position, hidden) in [(outside, false), (b.c, true), (outside, false)] {
            app.world_mut()
                .get_mut::<Transform>(player)
                .unwrap()
                .translation = Vec3::new(position.x, 0.0, position.y);
            app.update();
            for (&entity, material) in &architecture {
                let world = app.world();
                assert_eq!(
                    &world
                        .get::<MeshMaterial3d<StandardMaterial>>(entity)
                        .unwrap()
                        .0,
                    material,
                    "ceramic and metal must not become the faded wall texture"
                );
                assert_eq!(
                    *world.get::<Visibility>(entity).unwrap() == Visibility::Hidden,
                    hidden,
                    "balconies and stairs must disappear and return with the cutaway"
                );
            }
            for (roof, visibility) in app
                .world_mut()
                .query::<(&BldRoof, &Visibility)>()
                .iter(app.world())
            {
                assert_eq!(roof.0, b.id);
                assert_eq!(*visibility == Visibility::Hidden, hidden);
            }
            for (wall, transform) in app
                .world_mut()
                .query::<(&BldWalls, &Transform)>()
                .iter(app.world())
            {
                assert_eq!(wall.faded, hidden);
                assert_eq!(transform.scale.y, if hidden { 0.12 } else { 1.0 });
            }
        }
    }

    #[test]
    fn priority_cache_does_not_delay_a_lod_boundary_crossing() {
        let key = ChunkKey(0, 0);
        let before = Vec2::new(CHUNK + NEAR_R + 0.4, CHUNK * 0.5);
        let after = before - Vec2::X;
        assert!(!spatial_refresh_due(
            Some(before),
            after,
            CHUNK_ORDER_REFRESH_METRES,
        ));
        assert_eq!(want_lod(key, before), Lod::Mid);
        assert_eq!(want_lod(key, after), Lod::Near);
        assert!(spatial_refresh_due(
            Some(before),
            before + Vec2::X * 4.0,
            CHUNK_ORDER_REFRESH_METRES,
        ));
    }

    #[test]
    fn sub_metre_move_keeps_boundary_buildings_available_without_requery() {
        let w = world();
        let building = &w.blds[0];
        let before = building.c + Vec2::X * (DETAIL_IN + 0.6);
        let after = before - Vec2::X * 0.7;
        let candidates = detail_candidates(w, before, &HashMap::new());
        assert!(candidates.contains(&building.id));
        assert!(!spatial_refresh_due(
            Some(before),
            after,
            DETAIL_QUERY_REFRESH_METRES,
        ));
        let mut pending = VecDeque::from([building.id]);
        assert_eq!(next_detail_candidate(&mut pending, w, before), None);
        assert_eq!(pending.len(), 1);
        assert_eq!(
            next_detail_candidate(&mut pending, w, after),
            Some(building.id)
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn stationary_backlog_drains_once_without_recreating_live_details() {
        let w = world();
        let p = w.spawn;
        let mut pending = detail_candidates(w, p, &HashMap::new());
        let expected: HashSet<_> = pending
            .iter()
            .copied()
            .filter(|id| w.blds[*id as usize].c.distance(p) < DETAIL_IN)
            .collect();
        assert!(!expected.is_empty());
        let mut admitted = HashSet::new();
        while let Some(id) = next_detail_candidate(&mut pending, w, p) {
            assert!(admitted.insert(id), "a detail was uploaded twice");
        }
        assert_eq!(admitted, expected);
        assert!(!spatial_refresh_due(
            Some(p),
            p,
            DETAIL_QUERY_REFRESH_METRES
        ));
        assert_eq!(next_detail_candidate(&mut pending, w, p), None);
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
