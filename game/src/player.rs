//! Player (a 白面人 rig, see `human`): keyboard + virtual-joystick movement, building collision.

use bevy::prelude::*;

use crate::city::Obstacles;
use crate::combat::GameState;

pub const PLAYER_RADIUS: f32 = 0.45;
const SPEED: f32 = 3.4;

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
            .configure_sets(Update, MoveSet.after(crate::game_flow::FlowSet))
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

fn keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    modal: Option<Res<crate::ui::UiModal>>,
    pause: Option<Res<crate::ui::PauseOpen>>,
    mut input: ResMut<MoveInput>,
) {
    if modal.is_some_and(|m| m.blocks_input()) || pause.is_some_and(|p| p.0) {
        input.keys = Vec2::ZERO;
        input.stick = Vec2::ZERO;
        return;
    }
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

/// Sweeps a circle through walls, cover and live doors without skipping a thin
/// obstacle on a long frame, then keeps it within the playable map.
pub fn resolve_collisions(from: Vec2, to: Vec2, radius: f32, obstacles: &Obstacles) -> Vec2 {
    obstacles.move_circle(from, to, radius)
}

fn move_player(
    time: Res<Time>,
    input: Res<MoveInput>,
    state: Res<GameState>,
    obstacles: Res<Obstacles>,
    modal: Option<Res<crate::ui::UiModal>>,
    pause: Option<Res<crate::ui::PauseOpen>>,
    campaign: Option<Res<crate::game_flow::Campaign>>,
    keys: Res<ButtonInput<KeyCode>>,
    command: Option<Res<crate::input::PlayerInput>>,
    mut needs: Option<ResMut<crate::worldplay::PlayerNeeds>>,
    inventory: Option<Res<crate::inventory::Inventory>>,
    selected: Option<Res<crate::roles::SelectedRole>>,
    upstairs: Option<Res<crate::upstairs::Upstairs>>,
    mut player: Single<&mut Transform, With<Player>>,
) {
    if state.over
        || modal.is_some_and(|m| m.blocks_input())
        || pause.is_some_and(|p| p.0)
        || campaign.is_some_and(|c| c.sleeping)
    {
        return;
    }
    let dir = screen_to_ground(input.combined());
    if dir.length_squared() < 1e-4 {
        return;
    }
    let fatigue = needs.as_ref().map_or(10.0, |n| n.fatigue);
    let crouch = command.as_ref().is_some_and(|c| c.crouch);
    let running = !crouch
        && fatigue < 95.0
        && (keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
            || command
                .as_ref()
                .is_some_and(|c| c.stick > 0.93 || c.retreat > 0.0));
    let mut speed = if crouch {
        1.6
    } else if running {
        6.0
    } else {
        SPEED
    };
    if fatigue > 80.0 {
        speed *= 0.7;
    }
    let carry = selected.as_ref().map_or(1.0, |s| s.role().skills.carry);
    if inventory
        .as_ref()
        .is_some_and(|i| i.weight() > 12.0 * carry)
    {
        speed *= 0.8;
    }
    if state.stun > 0.0 {
        speed *= 0.25;
    }
    if running && let Some(ref mut needs) = needs {
        needs.fatigue = (needs.fatigue + time.delta_secs() * 0.25).min(100.0);
    }
    let pos = player.translation.xz() + dir * speed * time.delta_secs();
    // v23.4 an upper floor has its own walls and furniture
    let pos = match upstairs.as_deref().filter(|u| u.active()) {
        Some(up) => up.move_circle(player.translation.xz(), pos, PLAYER_RADIUS),
        None => resolve_collisions(player.translation.xz(), pos, PLAYER_RADIUS, &obstacles),
    };
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
            .insert_resource(Obstacles::new(crate::map::world().clone()))
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, (keyboard_input, move_player).chain());
        let e = app.world_mut().spawn((Player, Transform::default())).id();
        for (key, screen) in [
            (KeyCode::KeyD, Vec2::X),
            (KeyCode::ArrowUp, Vec2::Y),
            (KeyCode::KeyA, -Vec2::X),
            (KeyCode::ArrowDown, -Vec2::Y),
        ] {
            app.world_mut().get_mut::<Transform>(e).unwrap().translation = spawn_point();
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
            let moved =
                app.world().get::<Transform>(e).unwrap().translation.xz() - spawn_point().xz();
            let want = screen_to_ground(screen) * SPEED * 0.1;
            assert!(
                moved.distance(want) < 1e-3,
                "{key:?}: {moved:?} vs {want:?}"
            );
        }
    }
}
