//! Player capsule: keyboard + virtual-joystick movement, building collision.

use bevy::prelude::*;

use crate::city::{MAP_HALF, Obstacles};
use crate::combat::GameState;

pub const PLAYER_RADIUS: f32 = 0.45;
const SPEED: f32 = 7.0;

#[derive(Component)]
pub struct Player;

/// Movement intent in *screen* space: x = right, y = up, length ≤ 1.
/// Written by keyboard and the touch joystick, read by `move_player`.
#[derive(Resource, Default)]
pub struct MoveInput {
    pub keys: Vec2,
    pub stick: Vec2,
}

impl MoveInput {
    pub fn combined(&self) -> Vec2 {
        let v = self.keys + self.stick;
        if v.length() > 1.0 { v.normalize() } else { v }
    }
}

/// Screen axes projected onto the ground for the isometric camera
/// (same mapping as the web build: up = (-1, -1), right = (1, -1) on x/z).
pub fn screen_to_ground(v: Vec2) -> Vec2 {
    let up = Vec2::new(-1.0, -1.0).normalize();
    let right = Vec2::new(1.0, -1.0).normalize();
    right * v.x + up * v.y
}

/// Systems that consume [`MoveInput`]; input producers run before this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MoveSet;

pub fn move_system_set() -> MoveSet {
    MoveSet
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MoveInput>()
            .add_systems(Startup, spawn_player)
            .add_systems(
                Update,
                (keyboard_input, move_player).chain().in_set(MoveSet),
            );
    }
}

pub fn spawn_point() -> Vec3 {
    // Middle of the central street crossing
    let starts = crate::city::block_starts();
    let mid = starts.len() / 2;
    let c = starts[mid] - 3.5;
    Vec3::new(c, 0.0, c)
}

fn spawn_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let body = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.55, 0.95), // bright jacket so the player reads at a glance
        perceptual_roughness: 0.8,
        ..default()
    });
    let accent = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.80, 0.25),
        emissive: LinearRgba::rgb(0.4, 0.3, 0.05),
        ..default()
    });
    commands
        .spawn((
            Player,
            Transform::from_translation(spawn_point()),
            Visibility::default(),
        ))
        .with_children(|p| {
            p.spawn((
                Mesh3d(meshes.add(Capsule3d::new(PLAYER_RADIUS, 0.9))),
                MeshMaterial3d(body),
                Transform::from_xyz(0.0, 0.9, 0.0),
            ));
            // Unlit ground ring: keeps the player findable behind faded buildings
            p.spawn((
                Mesh3d(meshes.add(Torus::new(0.62, 0.78))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.85, 0.2),
                    unlit: true,
                    ..default()
                })),
                Transform::from_xyz(0.0, 0.06, 0.0).with_scale(Vec3::new(1.0, 0.2, 1.0)),
                bevy::light::NotShadowCaster,
            ));
            // "Rifle" stub shows the facing direction (local -Z is forward)
            p.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.14, 0.14, 0.8))),
                MeshMaterial3d(accent),
                Transform::from_xyz(0.25, 1.05, -0.45),
            ));
        });
}

fn keyboard_input(keys: Res<ButtonInput<KeyCode>>, mut input: ResMut<MoveInput>) {
    let mut v = Vec2::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        v.y += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        v.y -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        v.x += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        v.x -= 1.0;
    }
    input.keys = v.normalize_or_zero();
}

/// Pushes a circle out of every building footprint and keeps it on the map.
pub fn resolve_collisions(mut p: Vec2, radius: f32, obstacles: &Obstacles) -> Vec2 {
    for o in &obstacles.0 {
        if !o.contains(p, radius) {
            continue;
        }
        let closest = p.clamp(o.min, o.max);
        let d = p - closest;
        let len = d.length();
        if len > 1e-4 {
            if len < radius {
                p = closest + d / len * radius;
            }
        } else {
            // Centre is inside the box: exit through the nearest face
            let exits = [
                (p.x - o.min.x, Vec2::new(o.min.x - radius, p.y)),
                (o.max.x - p.x, Vec2::new(o.max.x + radius, p.y)),
                (p.y - o.min.y, Vec2::new(p.x, o.min.y - radius)),
                (o.max.y - p.y, Vec2::new(p.x, o.max.y + radius)),
            ];
            p = exits
                .iter()
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|e| e.1)
                .unwrap_or(p);
        }
    }
    let lim = MAP_HALF - radius;
    p.clamp(Vec2::splat(-lim), Vec2::splat(lim))
}

fn move_player(
    time: Res<Time>,
    input: Res<MoveInput>,
    state: Res<GameState>,
    obstacles: Res<Obstacles>,
    mut player: Single<&mut Transform, With<Player>>,
) {
    if state.over {
        return;
    }
    let dir = screen_to_ground(input.combined());
    if dir.length_squared() < 1e-4 {
        return;
    }
    let pos = player.translation.xz() + dir * SPEED * time.delta_secs();
    let pos = resolve_collisions(pos, PLAYER_RADIUS, &obstacles);
    player.translation.x = pos.x;
    player.translation.z = pos.y;
    // Face the movement direction (local -Z forward)
    let target = player.translation + Vec3::new(dir.x, 0.0, dir.y);
    player.look_at(target, Vec3::Y);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Playtest report: WASD / arrows did nothing. Keys reach `MoveInput.keys` and move the player
    /// (the report did not reproduce under Xvfb either; most likely the window had no focus).
    #[test]
    fn keys_move_the_player() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<MoveInput>()
            .init_resource::<GameState>()
            .init_resource::<Obstacles>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, (keyboard_input, move_player).chain());
        let e = app.world_mut().spawn((Player, Transform::default())).id();
        for (key, screen) in [
            (KeyCode::KeyD, Vec2::X),
            (KeyCode::ArrowUp, Vec2::Y),
            (KeyCode::KeyA, -Vec2::X),
            (KeyCode::ArrowDown, -Vec2::Y),
        ] {
            app.world_mut().get_mut::<Transform>(e).unwrap().translation = Vec3::ZERO;
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(100));
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(key);
            let moved = app.world().get::<Transform>(e).unwrap().translation.xz();
            let want = screen_to_ground(screen) * SPEED * 0.1;
            assert!(
                moved.distance(want) < 1e-3,
                "{key:?}: {moved:?} vs {want:?}"
            );
        }
    }
}
