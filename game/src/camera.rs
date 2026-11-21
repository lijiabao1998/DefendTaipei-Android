//! Isometric orthographic follow camera, matching the web build's feel:
//! fixed offset (40, 56, 40) from the player, ~44 m of street visible vertically
//! in landscape, a tighter framing in portrait, wheel / pinch zoom 0.5×–2.2×.

use bevy::{
    camera::ScalingMode,
    input::mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    light::NotShadowCaster,
    prelude::*,
    window::PrimaryWindow,
};

use crate::city::{Building, Obstacles};
use crate::player::Player;

pub const CAM_OFFSET: Vec3 = Vec3::new(40.0, 56.0, 40.0);

/// Current zoom factor (1 = default). Touch pinch writes to it from `touch.rs`.
#[derive(Resource)]
pub struct CameraZoom(pub f32);

impl Default for CameraZoom {
    fn default() -> Self {
        Self(1.0)
    }
}

#[derive(Component)]
pub struct GameCamera;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraZoom>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                PostUpdate,
                (wheel_zoom, fit_projection, follow_player, fade_occluders)
                    .chain()
                    .before(TransformSystems::Propagate),
            );
    }
}

fn spawn_camera(mut commands: Commands) {
    let target = crate::player::spawn_point();
    commands.spawn((
        GameCamera,
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 44.0,
            },
            near: -200.0,
            far: 400.0,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(target + CAM_OFFSET).looking_at(target, Vec3::Y),
        // MSAA panics on some Android GPUs (bevyengine/bevy#8229)
        #[cfg(target_os = "android")]
        Msaa::Off,
    ));
}

fn wheel_zoom(scroll: Res<AccumulatedMouseScroll>, mut zoom: ResMut<CameraZoom>) {
    let dy = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    if dy != 0.0 {
        zoom.0 = (zoom.0 * 1.1f32.powf(dy)).clamp(0.5, 2.2);
    }
}

/// Visible half-height in world units for a given aspect ratio (web build formula).
pub fn half_height(aspect: f32, zoom: f32) -> f32 {
    let s = if aspect < 1.0 {
        (16.0 / aspect).max(22.0) * 0.62
    } else {
        22.0
    };
    s / zoom
}

/// Re-fits the projection whenever the window is resized or rotated.
fn fit_projection(
    window: Single<&Window, With<PrimaryWindow>>,
    zoom: Res<CameraZoom>,
    mut projection: Single<&mut Projection, With<GameCamera>>,
) {
    let size = window.size();
    if size.y <= 0.0 {
        return;
    }
    let want = half_height(size.x / size.y, zoom.0) * 2.0;
    if let Projection::Orthographic(ortho) = projection.as_mut()
        && !matches!(ortho.scaling_mode, ScalingMode::FixedVertical { viewport_height } if (viewport_height - want).abs() < 1e-3)
    {
        ortho.scaling_mode = ScalingMode::FixedVertical {
            viewport_height: want,
        };
    }
}

fn follow_player(
    player: Single<&Transform, (With<Player>, Without<GameCamera>)>,
    mut cam: Single<&mut Transform, With<GameCamera>>,
) {
    let target = player.translation;
    cam.translation = target + CAM_OFFSET;
    cam.look_at(target, Vec3::Y);
}

/// Buildings standing between the camera and the player turn see-through.
fn fade_occluders(
    mut commands: Commands,
    player: Single<&Transform, With<Player>>,
    obstacles: Res<Obstacles>,
    mut buildings: Query<(Entity, &Building, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let origin = player.translation + Vec3::Y * 1.0;
    let dir = CAM_OFFSET.normalize();
    for (entity, b, mut mat) in &mut buildings {
        let o = obstacles.0[b.index];
        let hit = ray_hits_box(
            origin,
            dir,
            Vec3::new(o.min.x - 0.6, 0.0, o.min.y - 0.6),
            Vec3::new(o.max.x + 0.6, o.height, o.max.y + 0.6),
        );
        let want = if hit { &b.faded } else { &b.opaque };
        if mat.0 != *want {
            mat.0 = want.clone();
            // A see-through building should not throw a dark shadow over the player either.
            if hit {
                commands.entity(entity).insert(NotShadowCaster);
            } else {
                commands.entity(entity).remove::<NotShadowCaster>();
            }
        }
    }
}

fn ray_hits_box(o: Vec3, d: Vec3, min: Vec3, max: Vec3) -> bool {
    let inv = d.recip();
    let t1 = (min - o) * inv;
    let t2 = (max - o) * inv;
    let tmin = t1.min(t2).max_element();
    let tmax = t1.max(t2).min_element();
    tmax >= tmin.max(0.0)
}
