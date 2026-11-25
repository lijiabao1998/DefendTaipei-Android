//! Abstract aim / fire input for the weapon.
//!
//! Combat only reads [`WeaponInput`], whose fields (`aim: Vec2`, `fire`, `crouch`) have
//! the same names and types as lijiabao3's `PlayerInput` (PR #6, `crate::input`).
//! When #6 is on main, switching is one line:
//!
//! ```ignore
//! pub type WeaponInput = crate::input::PlayerInput;
//! ```
//!
//! and [`CombatInput`] can be deleted. Until then producers (desktop mouse here) write
//! [`CombatInput`] in [`CombatInputSet`].

use bevy::{prelude::*, window::PrimaryWindow};

use crate::camera::GameCamera;
use crate::player::{Player, screen_to_ground};

/// The resource combat reads. Swap for `crate::input::PlayerInput` once PR #6 lands.
pub type WeaponInput = CombatInput;

/// Same field subset as lijiabao3's `PlayerInput` (PR #6).
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

/// TEMPORARY until the touch fire button is used for this: fire back at the nearest
/// soldier in sight within 25 m that is already in combat with the player.
/// F toggles it on desktop. Not part of `PlayerInput`, so it stays separate.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct AutoFire(pub bool);

impl Default for AutoFire {
    fn default() -> Self {
        Self(true)
    }
}

/// Aim direction from a *screen-space* stick vector (x = right, y = up), e.g. a
/// right-hand virtual aim stick; converts it for the isometric camera.
pub fn aim_from_screen(v: Vec2) -> Vec2 {
    screen_to_ground(v).normalize_or_zero()
}

/// Systems that write [`WeaponInput`]; the weapon reads it after this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatInputSet;

#[cfg_attr(target_os = "android", allow(dead_code))]
/// Desktop testing: hold left mouse to aim at the cursor and fire; F toggles auto-fire.
pub(super) fn desktop_mouse_input(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<GameCamera>>,
    player: Single<&Transform, With<Player>>,
    mut input: ResMut<WeaponInput>,
    mut auto: ResMut<AutoFire>,
) {
    if keys.just_pressed(KeyCode::KeyF) {
        auto.0 = !auto.0;
    }
    input.fire = mouse.pressed(MouseButton::Left);
    input.aim = Vec2::ZERO;
    if !input.fire {
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
