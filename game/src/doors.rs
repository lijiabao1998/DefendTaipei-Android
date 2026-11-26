//! Live street entrances, following Web v24.42.0 `map/doors.ts`.
//!
//! Door frames belong to the streamed building meshes. A bounded near layer adds
//! hinged red home gates / green shop leaves and an interaction ring. The saved
//! closed building IDs update the same collision snapshot used by movement and
//! combat; unloading a mesh never opens its door. Barricades follow the Web's
//! hammer + one plank rule, including removal refunds and patrol-attracting noise.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, RwLock};

use bevy::prelude::*;

use crate::audio::SfxEvent;
use crate::combat::{CombatEnv, Enemy, GameState};
use crate::game_flow::GameFlow;
use crate::geom::{self, MeshBuf, hex};
use crate::i18n::{Language, tr};
use crate::inventory::Inventory;
use crate::map::data::{CELL, DOOR_H, DOOR_W, Door, Seg, WorldData, seg_dist};
use crate::map::{BldWalls, Obstacles};
use crate::player::{PLAYER_RADIUS, Player};
use crate::ui::PauseOpen;
use crate::worldplay::{ScavengeState, WorldAction, WorldNotice};

pub const INTERACT_RANGE: f32 = 3.0;
const LEAF_WIDTH: f32 = DOOR_W / 2.0 - 0.08;
const LEAF_HEIGHT: f32 = 2.24;
const LIVE_RANGE: f32 = 48.0;
const MAX_LIVE: usize = 24;

/// Shared with cloned `Obstacles`, including the combat world's boxed copy.
/// The geometry index is immutable; only the small set of closed IDs changes.
#[derive(Clone)]
pub struct DoorRuntime(Arc<DoorCollision>);

struct DoorCollision {
    state: RwLock<DoorState>,
    grid: HashMap<(i32, i32), Vec<Seg>>,
    ids: BTreeSet<u32>,
}

#[derive(Default)]
struct DoorState {
    closed: BTreeSet<u32>,
    barricaded: BTreeSet<u32>,
}

impl DoorRuntime {
    /// A bare map starts with open passages, matching pre-door native saves.
    /// New-game setup explicitly installs its initially closed entrances.
    pub fn new(world: &WorldData) -> Self {
        let mut grid: HashMap<(i32, i32), Vec<Seg>> = HashMap::new();
        let mut ids = BTreeSet::new();
        for b in &world.blds {
            let Some(door) = b.door else { continue };
            let seg = door_segment(b.id, door);
            // Expand the index by one sampling interval so a diagonal ray that
            // clips a cell corner cannot skip a short doorway segment.
            let lo = ((seg.a.min(seg.b) - CELL * 0.5) / CELL).floor().as_ivec2();
            let hi = ((seg.a.max(seg.b) + CELL * 0.5) / CELL).floor().as_ivec2();
            for x in lo.x..=hi.x {
                for y in lo.y..=hi.y {
                    grid.entry((x, y)).or_default().push(seg);
                }
            }
            ids.insert(b.id);
        }
        Self(Arc::new(DoorCollision {
            state: RwLock::new(DoorState::default()),
            grid,
            ids,
        }))
    }

    pub fn is_open(&self, id: u32) -> bool {
        !self.0.state.read().unwrap().closed.contains(&id)
    }

    pub fn is_barricaded(&self, id: u32) -> bool {
        self.0.state.read().unwrap().barricaded.contains(&id)
    }

    pub fn set_open(&self, id: u32, open: bool) -> bool {
        if !self.0.ids.contains(&id) {
            return false;
        }
        let mut state = self.0.state.write().unwrap();
        if open {
            if state.barricaded.contains(&id) {
                return false;
            }
            state.closed.remove(&id)
        } else {
            state.closed.insert(id)
        }
    }

    pub fn replace_closed(&self, ids: &[u32]) {
        self.replace_state(ids, &[]);
    }

    /// Restore both sets atomically: a barricade always implies a closed door.
    pub fn replace_state(&self, closed: &[u32], barricaded: &[u32]) {
        let mut state = self.0.state.write().unwrap();
        state.closed = closed
            .iter()
            .copied()
            .filter(|id| self.0.ids.contains(id))
            .collect();
        state.barricaded = barricaded
            .iter()
            .copied()
            .filter(|id| self.0.ids.contains(id))
            .collect();
        let barricaded = state.barricaded.clone();
        state.closed.extend(barricaded);
    }

    /// Mutating physical state alone never grants or spends inventory. Gameplay
    /// goes through `toggle_barricade`, which validates before changing either.
    pub fn set_barricaded(&self, id: u32, barricaded: bool) -> bool {
        if !self.0.ids.contains(&id) {
            return false;
        }
        let mut state = self.0.state.write().unwrap();
        if barricaded {
            state.closed.insert(id);
            state.barricaded.insert(id)
        } else {
            state.barricaded.remove(&id)
        }
    }

    pub fn closed_ids(&self) -> Vec<u32> {
        self.0
            .state
            .read()
            .unwrap()
            .closed
            .iter()
            .copied()
            .collect()
    }

    pub fn barricaded_ids(&self) -> Vec<u32> {
        self.0
            .state
            .read()
            .unwrap()
            .barricaded
            .iter()
            .copied()
            .collect()
    }

    pub fn all_ids(&self) -> Vec<u32> {
        self.0.ids.iter().copied().collect()
    }

    /// Indexed by the same 8 m cells as walls; no full-city per-ray door scan.
    pub(crate) fn along(&self, p: Vec2, q: Vec2, mut visit: impl FnMut(&Seg)) {
        let state = self.0.state.read().unwrap();
        let closed = &state.closed;
        if closed.is_empty() {
            return;
        }
        let d = q - p;
        let steps = (d.length() / (CELL * 0.5)).ceil() as i32 + 1;
        let mut seen = HashSet::new();
        for i in 0..=steps {
            let cell = ((p + d * (i as f32 / steps as f32)) / CELL)
                .floor()
                .as_ivec2();
            if let Some(segments) = self.0.grid.get(&(cell.x, cell.y)) {
                for seg in segments {
                    let id = seg.bld.unwrap();
                    if closed.contains(&id) && seen.insert(id) {
                        visit(seg);
                    }
                }
            }
        }
    }

    pub(crate) fn near(&self, p: Vec2, radius: f32, mut visit: impl FnMut(&Seg)) {
        let state = self.0.state.read().unwrap();
        let closed = &state.closed;
        if closed.is_empty() {
            return;
        }
        let lo = ((p - radius) / CELL).floor().as_ivec2();
        let hi = ((p + radius) / CELL).floor().as_ivec2();
        let mut seen = HashSet::new();
        for x in lo.x..=hi.x {
            for y in lo.y..=hi.y {
                if let Some(segments) = self.0.grid.get(&(x, y)) {
                    for seg in segments {
                        let id = seg.bld.unwrap();
                        if closed.contains(&id) && seen.insert(id) {
                            visit(seg);
                        }
                    }
                }
            }
        }
    }
}

pub fn door_segment(id: u32, door: Door) -> Seg {
    let half = door.tangent() * (DOOR_W / 2.0);
    Seg {
        a: door.p - half,
        b: door.p + half,
        h: Some(DOOR_H),
        bld: Some(id),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DoorTarget {
    pub id: u32,
    pub position: Vec2,
    pub open: bool,
    pub distance: f32,
}

/// Set by the same target selection as the Interact button, not a second guess.
#[derive(Resource, Default)]
pub struct DoorPrompt(pub Option<DoorTarget>);

pub fn nearest_door(pos: Vec2, obstacles: &Obstacles) -> Option<DoorTarget> {
    obstacles
        .world()
        .blds_in(pos - INTERACT_RANGE, pos + INTERACT_RANGE)
        .into_iter()
        .filter_map(|id| {
            let door = obstacles.world().blds[id as usize].door?;
            let distance = pos.distance(door.p);
            if distance >= INTERACT_RANGE {
                return None;
            }
            // Test a point on the player's side of the leaf. The target door
            // itself must not make its interaction ray appear obstructed.
            let side = if (pos - door.p).dot(door.n) < 0.0 {
                -1.0
            } else {
                1.0
            };
            let approach = door.p + door.n * side * 0.22;
            if !obstacles.los(pos, approach) {
                return None;
            }
            Some(DoorTarget {
                id,
                position: door.p,
                open: obstacles.doors().is_open(id),
                distance,
            })
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance).then(a.id.cmp(&b.id)))
}

/// Do not shut a leaf through a player or NPC standing in the opening. The
/// visual swing is cosmetic; the closed aperture is the collision boundary.
pub fn can_close(door: Door, occupants: impl IntoIterator<Item = (Vec2, f32)>) -> bool {
    let seg = door_segment(0, door);
    occupants
        .into_iter()
        .all(|(p, r)| seg_dist(p, seg.a, seg.b) >= r + 0.12)
}

/// Legacy saves may have been made in the formerly open aperture. Preserve the
/// restored position by opening just those overlapping doors, never teleporting.
pub fn protect_position(obstacles: &Obstacles, pos: Vec2) -> u32 {
    let mut ids = Vec::new();
    let barricaded: BTreeSet<_> = obstacles.doors().barricaded_ids().into_iter().collect();
    obstacles.doors().near(pos, PLAYER_RADIUS + 0.15, |seg| {
        let id = seg.bld.unwrap();
        // Standing against paid boards is a valid saved position. Do not
        // dismantle them just because the legacy leaf repair has extra slack.
        let threshold = if barricaded.contains(&id) {
            PLAYER_RADIUS - 0.001
        } else {
            PLAYER_RADIUS + 0.15
        };
        if seg_dist(pos, seg.a, seg.b) < threshold {
            ids.push(id);
        }
    });
    let mut recovered = 0;
    for id in ids {
        // Only corrupt/old manually positioned snapshots can put a player in
        // a new barricade. Keep the no-teleport guarantee and return its plank
        // to the save adapter so this repair never destroys paid materials.
        recovered += u32::from(obstacles.doors().set_barricaded(id, false));
        obstacles.doors().set_open(id, true);
    }
    recovered
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarricadeError {
    NoDoor,
    MissingMaterials,
    Occupied,
}

impl BarricadeError {
    pub fn message(self, lang: Language) -> &'static str {
        match self {
            Self::NoDoor => tr(
                lang,
                "走近可觸及的門口再封門。",
                "Move within reach of an entrance.",
            ),
            Self::MissingMaterials => tr(
                lang,
                "封門需要鐵鎚和一塊木板。",
                "Barricading requires a hammer and one plank.",
            ),
            Self::Occupied => tr(
                lang,
                "門口有人，先退開再封門。",
                "The doorway is occupied. Step away before barricading it.",
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarricadeOutcome {
    pub id: u32,
    pub added: bool,
    pub noise: f32,
}

impl BarricadeOutcome {
    pub fn message(self, lang: Language) -> &'static str {
        if self.added {
            tr(
                lang,
                "門封好了；敲打聲可能引來巡邏。",
                "Door barricaded; hammering may attract patrols.",
            )
        } else {
            tr(
                lang,
                "拆下封門木板，已放回背包；門仍關著。",
                "Recovered the barricade plank. The door is still closed.",
            )
        }
    }
}

/// The same reachable 3 m target as the door prompt, rechecked when the action
/// arrives. Validation happens before spending a plank or changing collision.
pub fn toggle_barricade(
    obstacles: &Obstacles,
    pos: Vec2,
    inventory: &mut Inventory,
    occupants: impl IntoIterator<Item = (Vec2, f32)>,
) -> Result<BarricadeOutcome, BarricadeError> {
    let target = nearest_door(pos, obstacles).ok_or(BarricadeError::NoDoor)?;
    let doors = obstacles.doors();
    if doors.is_barricaded(target.id) {
        doors.set_barricaded(target.id, false);
        // Web addItem always returns the paid plank, including when the bag
        // has since filled. We preserve it rather than silently discarding it.
        inventory.add("plank", 1);
        return Ok(BarricadeOutcome {
            id: target.id,
            added: false,
            noise: 10.0,
        });
    }
    let door = obstacles.world().blds[target.id as usize].door.unwrap();
    if !can_close(door, occupants) {
        return Err(BarricadeError::Occupied);
    }
    if inventory.count("hammer") == 0 || inventory.count("plank") == 0 {
        return Err(BarricadeError::MissingMaterials);
    }
    inventory.take("plank", 1);
    doors.set_barricaded(target.id, true);
    Ok(BarricadeOutcome {
        id: target.id,
        added: true,
        noise: 18.0,
    })
}

struct DoorNoiseRng(crate::city::Lcg);

impl Default for DoorNoiseRng {
    fn default() -> Self {
        Self(crate::city::Lcg(0x6261727269636164))
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_barricades(
    mut actions: MessageReader<WorldAction>,
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    game: Res<GameState>,
    player: Query<&Transform, With<Player>>,
    obstacles: Res<Obstacles>,
    mut inventory: ResMut<Inventory>,
    mut progress: ResMut<ScavengeState>,
    mut notice: ResMut<WorldNotice>,
    mut enemies: Query<&mut Enemy>,
    civilians: Query<&Transform, With<crate::human::Civilian>>,
    env: Res<CombatEnv>,
    lang: Res<Language>,
    mut sound: MessageWriter<SfxEvent>,
    mut rng: Local<DoorNoiseRng>,
) {
    if actions
        .read()
        .filter(|a| matches!(a, WorldAction::Barricade))
        .count()
        == 0
        || flow.get() != &GameFlow::Playing
        || pause.0
        || game.hp <= 0.0
        || game.over
    {
        return;
    }
    let Ok(player) = player.single() else { return };
    let pos = player.translation.xz();
    let occupants = std::iter::once((pos, PLAYER_RADIUS))
        .chain(
            enemies
                .iter()
                .filter(|e| e.0.alive())
                .map(|e| (e.0.pos, 0.4)),
        )
        .chain(civilians.iter().map(|c| (c.translation.xz(), 0.35)));
    match toggle_barricade(&obstacles, pos, &mut inventory, occupants) {
        Ok(result) => {
            progress.closed_doors = obstacles.doors().closed_ids();
            progress.barricaded_doors = obstacles.doors().barricaded_ids();
            notice.text = result.message(*lang).into();
            crate::combat::soldier::noise(
                enemies.iter_mut().map(|e| &mut e.into_inner().0),
                pos,
                result.noise,
                env.rain,
                env.night,
                &mut rng.0,
            );
            sound.write(SfxEvent::Clink { dist: 0.0 });
            println!(
                "DTDOOR barricaded={} id={} plank={}",
                result.added,
                result.id,
                inventory.count("plank")
            );
        }
        Err(error) => notice.text = error.message(*lang).into(),
    }
}

#[derive(Component)]
pub struct DoorLeaf {
    pub id: u32,
    side: f32,
    angle: f32,
}

#[derive(Component)]
struct DoorRing;

#[derive(Component)]
struct BarricadeView(u32);

#[derive(Resource)]
struct DoorArt {
    // Home/shop × left/right, shared by every visible entrance.
    meshes: [[Handle<Mesh>; 2]; 2],
    barricade: Handle<Mesh>,
    solid: Handle<StandardMaterial>,
    faded: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
struct DoorViews(HashMap<u32, [Entity; 2]>);

#[derive(Resource, Default)]
struct BarricadeViews(HashMap<u32, Entity>);

pub struct DoorPlugin;

impl Plugin for DoorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DoorPrompt>()
            .init_resource::<DoorViews>()
            .init_resource::<BarricadeViews>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (apply_barricades, show_doors, show_barricades)
                    .chain()
                    .after(crate::worldplay::WorldSet),
            );
    }
}

fn barricade_mesh() -> MeshBuf {
    let mut mesh = MeshBuf::default();
    for i in 0..3 {
        let local = Mat4::from_translation(Vec3::new(0.0, 0.6 + i as f32 * 0.6, 0.0))
            * Mat4::from_rotation_z((i as f32 - 1.0) * 0.15);
        mesh.add(
            &geom::cuboid(DOOR_W + 0.3, 0.25, 0.08),
            local,
            hex(0x9a7040),
        );
        for x in [-0.65, 0.65] {
            mesh.add(
                &geom::cuboid(0.035, 0.035, 0.011),
                local * Mat4::from_translation(Vec3::new(x, 0.0, 0.045)),
                hex(0x403a33),
            );
            mesh.add(
                &geom::cuboid(0.035, 0.035, 0.011),
                local * Mat4::from_translation(Vec3::new(x, 0.0, -0.045)),
                hex(0x403a33),
            );
        }
    }
    mesh
}

fn leaf_mesh(home: bool, side: f32) -> MeshBuf {
    let mut mesh = MeshBuf::default();
    let paint = hex(if home { 0x963b2e } else { 0x315b46 });
    let rail = hex(if home { 0x442923 } else { 0x253d31 });
    let mirror = Mat4::from_scale(Vec3::new(-side, 1.0, 1.0));
    let mut add = |size: Vec3, at: Vec3, color| {
        mesh.add(
            &geom::cuboid(size.x, size.y, size.z),
            mirror * Mat4::from_translation(at),
            color,
        );
    };
    add(
        Vec3::new(LEAF_WIDTH, LEAF_HEIGHT, 0.055),
        Vec3::new(LEAF_WIDTH / 2.0, 1.18, 0.0),
        paint,
    );
    for y in [0.35, 1.95] {
        add(
            Vec3::new(LEAF_WIDTH, 0.055, 0.07),
            Vec3::new(LEAF_WIDTH / 2.0, y, 0.015),
            rail,
        );
    }
    for i in 1..=4 {
        add(
            Vec3::new(0.025, 1.97, 0.07),
            Vec3::new(LEAF_WIDTH * i as f32 / 5.0, 1.18, 0.015),
            rail,
        );
    }
    add(
        Vec3::new(0.05, 0.2, 0.11),
        Vec3::new(LEAF_WIDTH - 0.12, 1.08, 0.035),
        hex(0xbcab80),
    );
    for (x, y, w) in [(0.17, 0.17, 0.14), (0.47, 0.21, 0.10)] {
        add(
            Vec3::new(w, 0.06, 0.006),
            Vec3::new(x, y, 0.031),
            hex(0x645447),
        );
    }
    mesh
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let make = |home, meshes: &mut Assets<Mesh>| {
        [-1.0, 1.0].map(|side| meshes.add(leaf_mesh(home, side).into_mesh()))
    };
    let art = DoorArt {
        meshes: [make(true, &mut meshes), make(false, &mut meshes)],
        barricade: meshes.add(barricade_mesh().into_mesh()),
        solid: materials.add(StandardMaterial {
            perceptual_roughness: 0.72,
            metallic: 0.25,
            ..default()
        }),
        faded: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 1.0, 1.0, 0.27),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.72,
            ..default()
        }),
    };
    let mut ring = MeshBuf::default();
    // A flat broken brass ring is readable on paving without another light.
    for i in 0..24 {
        let a = i as f32 * std::f32::consts::TAU / 24.0;
        let p = Vec3::new(a.cos() * 0.94, 0.0, a.sin() * 0.94);
        ring.add(
            &geom::cuboid(0.13, 0.012, 0.045),
            Mat4::from_translation(p) * Mat4::from_rotation_y(-a + std::f32::consts::FRAC_PI_2),
            hex(0xd8b469),
        );
    }
    commands.spawn((
        DoorRing,
        Mesh3d(meshes.add(ring.into_mesh())),
        MeshMaterial3d(materials.add(StandardMaterial {
            unlit: true,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.insert_resource(art);
}

#[allow(clippy::too_many_arguments)]
fn show_doors(
    mut commands: Commands,
    time: Res<Time>,
    obstacles: Res<Obstacles>,
    art: Res<DoorArt>,
    mut views: ResMut<DoorViews>,
    prompt: Res<DoorPrompt>,
    player: Single<&Transform, (With<Player>, Without<DoorLeaf>, Without<DoorRing>)>,
    walls: Query<&BldWalls>,
    chunks: Query<&crate::map::Chunk>,
    mut leaves: Query<
        (
            &mut DoorLeaf,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
            &mut Visibility,
        ),
        (Without<Player>, Without<DoorRing>),
    >,
    mut ring: Single<
        (&mut Transform, &mut Visibility),
        (With<DoorRing>, Without<Player>, Without<DoorLeaf>),
    >,
) {
    let pos = player.translation.xz();
    let world = obstacles.world();
    let visible: HashMap<_, _> = walls.iter().map(|wall| (wall.id, wall.faded)).collect();
    let loaded: HashSet<_> = chunks
        .iter()
        .filter(|chunk| chunk.lod != crate::map::Lod::Far)
        .map(|chunk| chunk.key)
        .collect();
    let inside = world.bld_at(pos).map(|b| b.id);
    let mut close: Vec<_> = world
        .blds_in(pos - LIVE_RANGE, pos + LIVE_RANGE)
        .into_iter()
        .filter_map(|id| {
            let b = &world.blds[id as usize];
            let d = b.door?;
            let dist = d.p.distance_squared(pos);
            // Large footprints can put their centroid in a Mid owner chunk
            // even while an entrance is close to the player. Mid still has
            // the complete frame; it also needs a visible closed leaf.
            (dist < LIVE_RANGE * LIVE_RANGE && loaded.contains(&crate::map::ChunkKey::of(b.c)))
                .then_some((id, dist))
        })
        .collect();
    close.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    close.truncate(MAX_LIVE);
    let wanted: HashSet<_> = close.iter().map(|(id, _)| *id).collect();
    views.0.retain(|id, pair| {
        if wanted.contains(id) {
            true
        } else {
            for &e in pair.iter() {
                commands.entity(e).despawn();
            }
            false
        }
    });
    for (id, _) in close {
        views.0.entry(id).or_insert_with(|| {
            let b = &world.blds[id as usize];
            let is_open = obstacles.doors().is_open(id);
            let d = b.door.unwrap();
            [-1.0, 1.0].map(|side| {
                let mut leaf = DoorLeaf {
                    id,
                    side,
                    angle: if is_open { 1.0 } else { 0.0 },
                };
                let tf = leaf_transform(d, &mut leaf, is_open, 0.0, inside == Some(id));
                commands
                    .spawn((
                        leaf,
                        Mesh3d(
                            art.meshes[usize::from(b.kind != "home")][usize::from(side > 0.0)]
                                .clone(),
                        ),
                        MeshMaterial3d(if visible.get(&id).copied().unwrap_or(false) {
                            art.faded.clone()
                        } else {
                            art.solid.clone()
                        }),
                        tf,
                        Visibility::default(),
                    ))
                    .id()
            })
        });
    }
    for (mut leaf, mut tf, mut mat, mut visibility) in &mut leaves {
        let d = world.blds[leaf.id as usize].door.unwrap();
        let open = obstacles.doors().is_open(leaf.id);
        let in_building = inside == Some(leaf.id);
        *tf = leaf_transform(d, &mut leaf, open, time.delta_secs(), in_building);
        *visibility = if obstacles.doors().is_barricaded(leaf.id) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        mat.0 = if visible.get(&leaf.id).copied().unwrap_or(false) {
            art.faded.clone()
        } else {
            art.solid.clone()
        };
    }
    let (ref mut tf, ref mut visibility) = *ring;
    if let Some(target) = prompt.0 {
        **visibility = Visibility::Inherited;
        tf.translation = Vec3::new(target.position.x, 0.105, target.position.y);
        let pulse = 1.0 + (time.elapsed_secs() * 4.0).sin() * 0.04;
        tf.scale = Vec3::splat(pulse);
    } else {
        **visibility = Visibility::Hidden;
    }
}

#[allow(clippy::too_many_arguments)]
fn show_barricades(
    mut commands: Commands,
    obstacles: Res<Obstacles>,
    art: Res<DoorArt>,
    doors: Res<DoorViews>,
    mut views: ResMut<BarricadeViews>,
    player: Single<&Transform, (With<Player>, Without<BarricadeView>)>,
    walls: Query<&BldWalls>,
    mut markers: Query<
        (
            &BarricadeView,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<Player>,
    >,
) {
    let world = obstacles.world();
    let inside = world.bld_at(player.translation.xz()).map(|b| b.id);
    let faded: HashSet<_> = walls.iter().filter(|w| w.faded).map(|w| w.id).collect();
    let wanted: HashSet<_> = doors
        .0
        .keys()
        .copied()
        .filter(|id| obstacles.doors().is_barricaded(*id))
        .collect();
    views.0.retain(|id, e| {
        if wanted.contains(id) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
    let transform = |id| {
        let door = world.blds[id as usize].door.unwrap();
        Transform::from_xyz(door.p.x, 0.0, door.p.y)
            .with_rotation(Quat::from_rotation_y(door.n.x.atan2(door.n.y)))
            .with_scale(Vec3::new(
                1.0,
                if inside == Some(id) { 0.32 } else { 1.0 },
                1.0,
            ))
    };
    let material = |id| {
        if faded.contains(&id) {
            art.faded.clone()
        } else {
            art.solid.clone()
        }
    };
    for id in wanted {
        views.0.entry(id).or_insert_with(|| {
            commands
                .spawn((
                    BarricadeView(id),
                    Mesh3d(art.barricade.clone()),
                    MeshMaterial3d(material(id)),
                    transform(id),
                ))
                .id()
        });
    }
    for (marker, mut tf, mut mat) in &mut markers {
        *tf = transform(marker.0);
        mat.0 = material(marker.0);
    }
}

fn leaf_transform(door: Door, leaf: &mut DoorLeaf, open: bool, dt: f32, inside: bool) -> Transform {
    let wanted = if open { 1.0 } else { 0.0 };
    leaf.angle += (wanted - leaf.angle).clamp(-dt * 2.5, dt * 2.5);
    let eased = leaf.angle * leaf.angle * (3.0 - 2.0 * leaf.angle);
    let hinge = door.p + door.tangent() * leaf.side * LEAF_WIDTH;
    Transform::from_xyz(hinge.x, 0.0, hinge.y)
        .with_rotation(Quat::from_rotation_y(
            door.n.x.atan2(door.n.y) + leaf.side * eased * 1.75,
        ))
        .with_scale(Vec3::new(1.0, if inside { 0.32 } else { 1.0 }, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (Obstacles, u32, Door) {
        let world = crate::map::world().clone();
        let b = world
            .blds
            .iter()
            .filter(|b| b.kind == "home" && !b.fill && b.door.is_some())
            .min_by(|a, b| {
                a.door
                    .unwrap()
                    .p
                    .distance_squared(world.spawn)
                    .total_cmp(&b.door.unwrap().p.distance_squared(world.spawn))
            })
            .unwrap();
        let id = b.id;
        let door = b.door.unwrap();
        (Obstacles::new(world), id, door)
    }

    #[test]
    fn closed_state_is_shared_by_movement_combat_and_clones() {
        let (obstacles, id, door) = home();
        let cloned = obstacles.clone();
        let a = door.p + door.n * 0.6;
        let b = door.p - door.n * 0.6;
        assert!(obstacles.los(a, b));
        obstacles.doors().set_open(id, false);
        assert!(!cloned.los(a, b));
        assert!(cloned.shot_ray(a, b, 1.5, 1.5, 0.0) < 0.6);
        assert!(
            cloned.shot_ray(a, b, 1.5, 1.5, 1.0) < 0.6,
            "own-cover forgiveness must not let a nearby closed door be shot through"
        );
        assert!(
            cloned
                .collide(door.p + door.n * 0.1, PLAYER_RADIUS)
                .distance(door.p)
                >= PLAYER_RADIUS - 0.001
        );
        let stopped = cloned.move_circle(a, b - door.n, PLAYER_RADIUS);
        assert!(
            (stopped - door.p).dot(door.n) >= PLAYER_RADIUS - 0.001,
            "a long movement frame crossed the closed leaf: {stopped:?}"
        );
        assert!(
            cloned.segment_hit(a, b, 2.8, 2.8).is_none(),
            "a door must not become an infinite-height wall"
        );
        obstacles.doors().set_open(id, true);
        assert!(cloned.los(a, b));
        assert!(
            cloned
                .collide(door.p + door.n * 0.1, PLAYER_RADIUS)
                .distance(door.p + door.n * 0.1)
                < 0.001
        );
    }

    #[test]
    fn closing_refuses_an_occupied_threshold_and_restore_keeps_position_safe() {
        let (obstacles, id, door) = home();
        assert!(!can_close(door, [(door.p, PLAYER_RADIUS)]));
        assert!(!can_close(door, [(door.p + door.n * 0.2, 0.35)]));
        assert!(can_close(door, [(door.p + door.n * 1.0, PLAYER_RADIUS)]));
        obstacles.doors().set_open(id, false);
        protect_position(&obstacles, door.p);
        assert!(obstacles.doors().is_open(id));
    }

    #[test]
    fn nearest_closed_door_is_reachable_from_both_sides_and_respects_range() {
        let (obstacles, id, door) = home();
        obstacles.doors().set_open(id, false);
        for side in [-1.0, 1.0] {
            let target = nearest_door(door.p + door.n * side, &obstacles).unwrap();
            assert_eq!(target.id, id);
            assert!(!target.open);
        }
        assert!(nearest_door(door.p + door.n * 4.0, &obstacles).is_none_or(|d| d.id != id));
    }

    #[test]
    fn open_leaves_clear_the_capsule_and_mesh_budget_is_bounded() {
        let door = Door {
            p: Vec2::ZERO,
            n: Vec2::Y,
        };
        for side in [-1.0, 1.0] {
            let mesh = leaf_mesh(true, side);
            assert!(mesh.vcount() <= 300);
            let mut leaf = DoorLeaf {
                id: 0,
                side,
                angle: 1.0,
            };
            let tf = leaf_transform(door, &mut leaf, true, 0.0, false).to_matrix();
            for p in &mesh.pos {
                let p = tf.transform_point3(Vec3::from(*p));
                assert!(
                    p.x.abs() > PLAYER_RADIUS + 0.15,
                    "open leaf intersects passage: {p:?}"
                );
            }
        }
    }

    #[test]
    fn saved_closed_ids_are_independent_of_live_visual_slots() {
        let (obstacles, id, _) = home();
        let mut saved = obstacles.doors().all_ids();
        saved.retain(|other| *other != id);
        obstacles.doors().replace_closed(&saved);
        assert!(saved.len() > MAX_LIVE * 10);
        assert_eq!(obstacles.doors().closed_ids(), saved);
        assert!(obstacles.doors().is_open(id));
        assert!(!obstacles.doors().set_open(u32::MAX, false));
        assert_eq!(obstacles.doors().closed_ids(), saved);
    }

    #[test]
    fn barricade_spends_one_plank_keeps_hammer_blocks_all_clones_and_refunds_once() {
        let (obstacles, id, door) = home();
        let cloned = obstacles.clone();
        let pos = door.p + door.n;
        let mut bag = Inventory::default();
        bag.add("hammer", 1);
        bag.add("plank", 2);
        let built = toggle_barricade(&obstacles, pos, &mut bag, [(pos, PLAYER_RADIUS)]).unwrap();
        assert_eq!(
            built,
            BarricadeOutcome {
                id,
                added: true,
                noise: 18.0
            }
        );
        assert_eq!(bag.count("plank"), 1);
        assert_eq!(bag.count("hammer"), 1);
        assert!(cloned.doors().is_barricaded(id));
        assert!(!cloned.doors().is_open(id));
        assert!(
            !cloned.doors().set_open(id, true),
            "ordinary Interact must not bypass boards"
        );
        assert!(!cloned.los(pos, door.p - door.n));
        let stopped = cloned.move_circle(pos, door.p - door.n, PLAYER_RADIUS);
        assert!((stopped - door.p).dot(door.n) >= PLAYER_RADIUS - 0.001);
        assert!(cloned.shot_ray(pos, door.p - door.n, 1.5, 1.5, 1.0) < 1.0);
        // Removing has no hammer requirement, matching the Web's paid-plank refund.
        bag.take("hammer", 1);
        // A bag filled by subsequent scavenging must still recover its paid
        // plank. The 15 kg ceiling governs pickups, not material refunds.
        bag.add("water", 20);
        bag.add("can", 5);
        let full_weight = bag.weight();
        assert!((full_weight - crate::worldplay::LOOT_CAPACITY_KG).abs() < 0.001);
        let removed = toggle_barricade(&cloned, pos, &mut bag, []).unwrap();
        assert_eq!(
            removed,
            BarricadeOutcome {
                id,
                added: false,
                noise: 10.0
            }
        );
        assert_eq!(bag.count("plank"), 2);
        assert_eq!(bag.count("water"), 20);
        assert_eq!(bag.count("can"), 5);
        assert!((bag.weight() - full_weight - 1.0).abs() < 0.001);
        assert!(!obstacles.doors().is_barricaded(id));
        assert!(
            !obstacles.doors().is_open(id),
            "removal must preserve a closed normal leaf"
        );
        assert_eq!(
            toggle_barricade(&obstacles, pos, &mut bag, []),
            Err(BarricadeError::MissingMaterials)
        );
        assert_eq!(
            bag.count("plank"),
            2,
            "repeated remove requests cannot mint planks"
        );
        assert!(obstacles.doors().set_open(id, true));
        assert!(cloned.los(pos, door.p - door.n));
    }

    #[test]
    fn barricade_validates_occupancy_materials_and_reach_before_spending() {
        let (obstacles, id, door) = home();
        let pos = door.p + door.n;
        let mut bag = Inventory::default();
        bag.add("plank", 2);
        assert_eq!(
            toggle_barricade(&obstacles, pos, &mut bag, []),
            Err(BarricadeError::MissingMaterials)
        );
        bag.add("hammer", 1);
        assert_eq!(
            toggle_barricade(&obstacles, pos, &mut bag, [(door.p, 0.35)]),
            Err(BarricadeError::Occupied)
        );
        assert_eq!(
            toggle_barricade(&obstacles, Vec2::splat(1_000_000.0), &mut bag, []),
            Err(BarricadeError::NoDoor)
        );
        assert_eq!(bag.count("plank"), 2);
        assert_eq!(bag.count("hammer"), 1);
        assert!(obstacles.doors().is_open(id));
        assert!(obstacles.doors().barricaded_ids().is_empty());
    }

    #[test]
    fn barricade_restore_normalizes_ids_and_threshold_repair_accounts_for_materials() {
        let (obstacles, id, door) = home();
        obstacles.doors().replace_state(&[], &[id, id, u32::MAX]);
        assert_eq!(obstacles.doors().closed_ids(), vec![id]);
        assert_eq!(obstacles.doors().barricaded_ids(), vec![id]);
        assert_eq!(
            protect_position(&obstacles, door.p + door.n * PLAYER_RADIUS),
            0
        );
        assert!(
            obstacles.doors().is_barricaded(id),
            "Continue beside boards must not dismantle them"
        );
        assert_eq!(protect_position(&obstacles, door.p), 1);
        assert_eq!(protect_position(&obstacles, door.p), 0);
        assert!(obstacles.doors().is_open(id));
        assert!(obstacles.doors().barricaded_ids().is_empty());
        obstacles.doors().replace_state(&[id], &[id]);
        obstacles.doors().replace_closed(&[id]);
        assert!(!obstacles.doors().is_open(id));
        assert!(
            !obstacles.doors().is_barricaded(id),
            "legacy restoration clears stale boards from the previous session"
        );
    }

    #[test]
    fn barricade_mesh_stays_within_shared_near_layer_budget() {
        let mesh = barricade_mesh();
        assert!(mesh.vcount() <= 360);
        assert!(mesh.pos.iter().all(|p| p.iter().all(|v| v.is_finite())));
        assert!(mesh.pos.iter().map(|p| p[0].abs()).fold(0.0, f32::max) <= DOOR_W / 2.0 + 0.2);
    }

    #[test]
    fn barricade_message_updates_save_delta_and_alerts_patrol_without_duplicate_toggle() {
        use crate::combat::{EnemyKind, SolState, Soldier};
        use bevy::state::app::StatesPlugin;
        let (obstacles, id, door) = home();
        let pos = door.p + door.n;
        let mut bag = Inventory::default();
        bag.add("hammer", 1);
        bag.add("plank", 2);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(GameFlow::Playing)
            .insert_resource(obstacles)
            .insert_resource(bag)
            .init_resource::<PauseOpen>()
            .init_resource::<GameState>()
            .init_resource::<ScavengeState>()
            .init_resource::<WorldNotice>()
            .init_resource::<CombatEnv>()
            .init_resource::<Language>()
            .add_message::<WorldAction>()
            .add_message::<SfxEvent>()
            .add_systems(Update, apply_barricades);
        app.world_mut()
            .spawn((Player, Transform::from_xyz(pos.x, 0.0, pos.y)));
        let soldier = app
            .world_mut()
            .spawn(Enemy(Soldier::new(
                1234,
                EnemyKind::Patrol,
                pos + Vec2::X * 7.0,
                vec![],
            )))
            .id();
        app.world_mut().write_message(WorldAction::Barricade);
        app.world_mut().write_message(WorldAction::Barricade);
        app.update();
        assert_eq!(app.world().resource::<Inventory>().count("plank"), 1);
        assert_eq!(
            app.world().resource::<ScavengeState>().barricaded_doors,
            vec![id]
        );
        assert!(
            app.world()
                .resource::<ScavengeState>()
                .closed_doors
                .contains(&id)
        );
        assert_eq!(
            app.world().get::<Enemy>(soldier).unwrap().0.state,
            SolState::Investigate
        );
        app.update();
        assert_eq!(
            app.world().resource::<Inventory>().count("plank"),
            1,
            "a coalesced duplicate must not execute in the next frame"
        );
        app.world_mut().write_message(WorldAction::Barricade);
        app.update();
        assert_eq!(app.world().resource::<Inventory>().count("plank"), 2);
        assert!(
            app.world()
                .resource::<ScavengeState>()
                .barricaded_doors
                .is_empty()
        );
    }
}
