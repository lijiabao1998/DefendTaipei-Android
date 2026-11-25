//! Thin adapter from lijiabao3's [`PlayerInput`] (`crate::input`, PR #6) to the weapon.
//!
//! [`PlayerInput`] is the one source of fire / crouch / aim for every platform
//! (touch 打 / 蹲 buttons, desktop mouse and keys). Combat works on the ground plane,
//! so [`player_input_to_weapon`] copies `fire` and `crouch` and converts the
//! screen-space `aim` (+x right, +y up) into a world (x, z) direction. That result is
//! [`WeaponInput`], which only combat reads.
//!
//! Desktop extra: while the left mouse button fires (and is not on a touch control),
//! the aim is the cursor projected onto the ground (original `aimPt`). F toggles
//! [`AutoFire`].

use bevy::prelude::*;

use crate::input::{PlayerInput, TouchControl};
use crate::player::screen_to_ground;

/// Ground-plane view of [`PlayerInput`] for the weapon. Written only by the adapter
/// (and by tests); everything else should write [`PlayerInput`].
pub type WeaponInput = CombatInput;

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct CombatInput {
    /// Manual aim direction on the ground plane (world x, z). `Vec2::ZERO` = no manual aim.
    pub aim: Vec2,
    /// Trigger held (original: mouse click / the 打 button). With `aim` set, fires along it;
    /// without, at the nearest soldier in sight within 25 m, else straight ahead.
    pub fire: bool,
    /// 蹲 toggle: copied to `PlayerStance.sneak` (quieter, harder to spot).
    pub crouch: bool,
}

/// Testing aid, OFF by default (the original has no auto-fire): fire back at the nearest
/// soldier in sight within 25 m that is already in combat with the player. F toggles it on
/// desktop. Not part of `PlayerInput`.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct AutoFire(pub bool);

/// Aim direction from a *screen-space* vector (x = right, y = up), as in
/// `PlayerInput::aim`; converts it for the isometric camera.
pub fn aim_from_screen(v: Vec2) -> Vec2 {
    screen_to_ground(v).normalize_or_zero()
}

/// The whole mapping from [`PlayerInput`] to [`WeaponInput`].
pub fn weapon_from_player(p: &PlayerInput) -> WeaponInput {
    CombatInput {
        aim: aim_from_screen(p.aim),
        fire: p.fire,
        crouch: p.crouch,
    }
}

/// Systems that write [`WeaponInput`]; the weapon reads it after this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatInputSet;

/// Runs after `crate::input::InputSet` (via `MoveSet`), on every platform.
pub fn player_input_to_weapon(player: Res<PlayerInput>, mut weapon: ResMut<WeaponInput>) {
    let next = weapon_from_player(&player);
    if *weapon != next {
        *weapon = next;
    }
}

/// Pointer id `crate::input` uses for the mouse when it drives the touch controls.
#[cfg_attr(target_os = "android", allow(dead_code))]
const MOUSE_POINTER: u64 = u64::MAX;

#[cfg_attr(target_os = "android", allow(dead_code))]
/// Desktop: cursor aim while the left mouse button fires; F toggles auto-fire.
pub(super) fn desktop_mouse_aim(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    hands: Res<TouchControl>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::GameCamera>>,
    player: Single<&Transform, With<crate::player::Player>>,
    mut input: ResMut<WeaponInput>,
    mut auto: ResMut<AutoFire>,
) {
    if keys.just_pressed(KeyCode::KeyF) {
        auto.0 = !auto.0;
    }
    // Only a free mouse click aims; a click on 打 / 蹲 / the pad keeps PlayerInput's aim.
    let click = mouse.pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Left);
    if !input.fire || !click || hands.claims(MOUSE_POINTER) {
        return;
    }
    let (cam, cam_gt) = *camera;
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(ray) = cam.viewport_to_world(cam_gt, cursor) else {
        return;
    };
    // original: ray onto the ground plane (aimPt)
    if let Some(d) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)) {
        let hit = ray.get_point(d).xz();
        input.aim = (hit - player.translation.xz()).normalize_or_zero();
    }
}

/// `crouch` → `PlayerStance.sneak`.
pub(super) fn sync_stance(input: Res<WeaponInput>, mut stance: ResMut<super::PlayerStance>) {
    if stance.sneak != input.crouch {
        stance.sneak = input.crouch;
    }
}
