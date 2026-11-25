//! Live street entrances, following Web v24.42.0 `map/doors.ts`.
//!
//! Door frames belong to the streamed building meshes. A bounded near layer adds
//! hinged red home gates / green shop leaves and an interaction ring. The saved
//! closed building IDs update the same collision snapshot used by movement and
//! combat; unloading a mesh never opens its door. Locks and barricades are not
//! part of this native subset.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, RwLock};

use bevy::prelude::*;

use crate::geom::{self, MeshBuf, hex};
use crate::map::data::{CELL, DOOR_H, DOOR_W, Door, Seg, WorldData, seg_dist};
use crate::map::{BldWalls, Obstacles};
use crate::player::{PLAYER_RADIUS, Player};

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
    closed: RwLock<BTreeSet<u32>>,
    grid: HashMap<(i32, i32), Vec<Seg>>,
    ids: BTreeSet<u32>,
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
            closed: RwLock::new(BTreeSet::new()),
            grid,
            ids,
        }))
    }

    pub fn is_open(&self, id: u32) -> bool {
        !self.0.closed.read().unwrap().contains(&id)
    }

    pub fn set_open(&self, id: u32, open: bool) -> bool {
        if !self.0.ids.contains(&id) {
            return false;
        }
        let mut closed = self.0.closed.write().unwrap();
        if open {
            closed.remove(&id)
        } else {
            closed.insert(id)
        }
    }

    pub fn replace_closed(&self, ids: &[u32]) {
        let mut closed = self.0.closed.write().unwrap();
        closed.clear();
        closed.extend(ids.iter().copied().filter(|id| self.0.ids.contains(id)));
    }

    pub fn closed_ids(&self) -> Vec<u32> {
        self.0.closed.read().unwrap().iter().copied().collect()
    }

    pub fn all_ids(&self) -> Vec<u32> {
        self.0.ids.iter().copied().collect()
    }

    /// Indexed by the same 8 m cells as walls; no full-city per-ray door scan.
    pub(crate) fn along(&self, p: Vec2, q: Vec2, mut visit: impl FnMut(&Seg)) {
        let closed = self.0.closed.read().unwrap();
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
        let closed = self.0.closed.read().unwrap();
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
pub fn protect_position(obstacles: &Obstacles, pos: Vec2) {
    let mut ids = Vec::new();
    obstacles.doors().near(pos, PLAYER_RADIUS + 0.15, |seg| {
        if seg_dist(pos, seg.a, seg.b) < PLAYER_RADIUS + 0.15 {
            ids.push(seg.bld.unwrap());
        }
    });
    for id in ids {
        obstacles.doors().set_open(id, true);
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

#[derive(Resource)]
struct DoorArt {
    // Home/shop × left/right, shared by every visible entrance.
    meshes: [[Handle<Mesh>; 2]; 2],
    solid: Handle<StandardMaterial>,
    faded: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
struct DoorViews(HashMap<u32, [Entity; 2]>);

pub struct DoorPlugin;

impl Plugin for DoorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DoorPrompt>()
            .init_resource::<DoorViews>()
            .add_systems(Startup, setup)
            .add_systems(Update, show_doors.after(crate::worldplay::WorldSet));
    }
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
    for (mut leaf, mut tf, mut mat) in &mut leaves {
        let d = world.blds[leaf.id as usize].door.unwrap();
        let open = obstacles.doors().is_open(leaf.id);
        let in_building = inside == Some(leaf.id);
        *tf = leaf_transform(d, &mut leaf, open, time.delta_secs(), in_building);
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
}
