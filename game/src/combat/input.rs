//! Abstract aim / fire input for the weapon.
//!
//! Combat only reads [`CombatInput`]. Producers (desktop mouse here, the touch fire
//! button / aim stick from `touch.rs` later) write it in [`CombatInputSet`].
//! To plug in lijiabao3's `PlayerInput`, add one system in `CombatInputSet` that
//! copies it into `CombatInput` (or swap this resource for it) — nothing else in
//! combat needs to change.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::camera::GameCamera;
use crate::player::{Player, screen_to_ground};

#[derive(Resource, Debug, Clone, PartialEq)]
pub struct CombatInput {
    /// Manual aim direction on the ground plane (world x, z). `None` = no manual aim.
    pub aim: Option<Vec2>,
    /// Trigger held (original: mouse click / the 打 button). With `aim` set, fires along it;
    /// without, at the nearest soldier in sight within 25 m, else straight ahead.
    pub fire: bool,
    /// TEMPORARY until the touch fire button lands: fire back at the nearest soldier in
    /// sight within 25 m that is already in combat with the player. F toggles it on desktop.
    pub auto_fire: bool,
}

impl Default for CombatInput {
    fn default() -> Self {
        Self {
            aim: None,
            fire: false,
            auto_fire: true,
        }
    }
}

impl CombatInput {
    /// Sets the aim from a *screen-space* stick vector (x = right, y = up), e.g. a
    /// right-hand virtual aim stick; converts it for the isometric camera.
    pub fn set_aim_screen(&mut self, v: Vec2) {
        let g = screen_to_ground(v);
        self.aim = (g.length_squared() > 1e-4).then(|| g.normalize());
    }
}

/// Systems that write [`CombatInput`]; the weapon reads it after this set.
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
    mut input: ResMut<CombatInput>,
) {
    if keys.just_pressed(KeyCode::KeyF) {
        input.auto_fire = !input.auto_fire;
    }
    input.fire = mouse.pressed(MouseButton::Left);
    input.aim = None;
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
    let plane_y = Vec3::ZERO; // original: ray onto the ground plane (aimPt)
    if let Some(d) = ray.intersect_plane(plane_y, InfinitePlane3d::new(Vec3::Y)) {
        let hit = ray.get_point(d).xz();
        let dir = hit - player.translation.xz();
        input.aim = (dir.length_squared() > 1e-4).then(|| dir.normalize());
    }
}
