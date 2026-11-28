//! The world renders to one reusable texture; UI and touch coordinates remain
//! in the full-resolution window. Resizing never changes the camera's framing.
use super::EffectivePerformance;
use super::upscale::SgsrMaterial;
use crate::camera::GameCamera;
use bevy::{
    camera::{RenderTarget, ShadowLodOrigin, visibility::RenderLayers},
    prelude::*,
    render::render_resource::{Extent3d, TextureFormat},
    window::PrimaryWindow,
};
use serde::Serialize;

#[derive(Component)]
pub(crate) struct PresentationCamera;
#[derive(Resource)]
struct SceneTarget(Handle<Image>);
#[derive(Resource)]
struct SceneUpscaler(Handle<SgsrMaterial>);
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RenderDimensions {
    pub ui: [u32; 2],
    pub scene: [u32; 2],
    pub scale: u8,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<RenderDimensions>()
        .add_systems(PostStartup, setup)
        .add_systems(PreUpdate, resize.after(crate::device::DeviceSet));
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut upscalers: ResMut<Assets<SgsrMaterial>>,
    cameras: Query<Entity, With<GameCamera>>,
) {
    let mut image = Image::new_target_texture(16, 16, TextureFormat::Rgba8UnormSrgb, None);
    // GPU rendering supplies every pixel. Retain the descriptor for resizes,
    // without retaining/uploading a full CPU zero buffer on every rotation.
    image.data = None;
    // The next world pass clears/redraws the entire target. Copying old pixels
    // would need COPY_SRC and is both wasted work and invalid for this target.
    image.copy_on_resize = false;
    let image = images.add(image);
    for entity in &cameras {
        commands.entity(entity).insert((
            RenderTarget::Image(image.clone().into()),
            ShadowLodOrigin,
            Camera {
                order: -1,
                ..default()
            },
        ));
    }
    commands.spawn((
        PresentationCamera,
        Camera2d,
        Camera {
            order: 0,
            ..default()
        },
        IsDefaultUiCamera,
        RenderLayers::layer(31),
        Msaa::Off,
    ));
    let upscaler = upscalers.add(SgsrMaterial {
        params: Vec4::new(1.0 / 16.0, 1.0 / 16.0, 0.55, 0.0),
        scene: image.clone(),
    });
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
            ..default()
        },
        MaterialNode(upscaler.clone()),
        GlobalZIndex(-1000),
        Pickable::IGNORE,
    ));
    commands.insert_resource(SceneTarget(image));
    commands.insert_resource(SceneUpscaler(upscaler));
}

pub fn scaled_dimensions(ui: UVec2, scale: u8) -> UVec2 {
    // Rounded to actual pixels. No viewport stretch or change to orthographic FOV.
    (ui.as_vec2() * scale.clamp(50, 100) as f32 / 100.0)
        .round()
        .as_uvec2()
        .max(UVec2::ONE)
}

fn resize(
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    target: Option<Res<SceneTarget>>,
    upscaler: Option<Res<SceneUpscaler>>,
    effective: Res<EffectivePerformance>,
    mut images: ResMut<Assets<Image>>,
    mut upscalers: ResMut<Assets<SgsrMaterial>>,
    mut dimensions: ResMut<RenderDimensions>,
) {
    let (Some(window), Some(target)) = (window, target) else {
        return;
    };
    let ui = UVec2::new(window.physical_width(), window.physical_height());
    if ui.x == 0 || ui.y == 0 {
        return;
    }
    let scene = scaled_dimensions(ui, effective.render_scale);
    let next = RenderDimensions {
        ui: ui.to_array(),
        scene: scene.to_array(),
        scale: effective.render_scale,
    };
    if *dimensions == next {
        return;
    }
    if let Some(mut image) = images.get_mut(&target.0) {
        image.resize(Extent3d {
            width: scene.x,
            height: scene.y,
            depth_or_array_layers: 1,
        });
        if let Some(upscaler) = upscaler
            && let Some(mut mat) = upscalers.get_mut(&upscaler.0)
        {
            mat.params = Vec4::new(
                1.0 / scene.x.max(1) as f32,
                1.0 / scene.y.max(1) as f32,
                0.55,
                if effective.render_scale < 100 {
                    1.0
                } else {
                    0.0
                },
            );
        }
        *dimensions = next;
        println!(
            "DTRESOLUTION ui={}x{} scene={}x{} scale={}",
            ui.x, ui.y, scene.x, scene.y, effective.render_scale
        );
    }
}

/// Mouse aiming uses the world camera's target coordinates, not window pixels.
/// Touch controls continue to use the full-resolution UI layout as before.
pub(crate) fn scene_pointer(cursor: Vec2, window_size: Vec2, camera_size: Vec2) -> Vec2 {
    cursor / window_size.max(Vec2::ONE) * camera_size
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_and_scaling_preserve_scene_pointer_alignment() {
        for size in [
            UVec2::new(1080, 2400),
            UVec2::new(2400, 1080),
            UVec2::new(390, 844),
        ] {
            for scale in [60, 70, 85, 100] {
                let scene = scaled_dimensions(size, scale);
                let center = scene_pointer(size.as_vec2() * 0.5, size.as_vec2(), scene.as_vec2());
                assert!(center.distance(scene.as_vec2() * 0.5) < 0.01);
                assert!(scene.x <= size.x && scene.y <= size.y);
            }
        }
    }
}
