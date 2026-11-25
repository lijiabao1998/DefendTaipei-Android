//! Player (a 白面人 rig, see `human`): keyboard + virtual-joystick movement, building collision.

use bevy::prelude::*;

use crate::city::Obstacles;
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

/// On the street in front of the door of the original's start home.
pub fn spawn_point() -> Vec3 {
    let p = crate::map::world().spawn;
    Vec3::new(p.x, 0.0, p.y)
}

fn spawn_player(mut commands: Commands) {
    commands.spawn((
        Player,
        crate::human::Humanoid::player(),
        Transform::from_translation(spawn_point()),
        Visibility::default(),
    ));
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

/// Pushes a circle out of every wall / cover segment and keeps it on the map (main.ts `collide`).
pub fn resolve_collisions(p: Vec2, radius: f32, obstacles: &Obstacles) -> Vec2 {
    obstacles.collide(p, radius)
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
