//! Colored LOD quads. Compiled in, spawned only for `--chunk-debug`,
//! `DEFEND_CHUNK_DEBUG=1`, or `DEFEND_CHUNK_SHOT`. A normal launch never
//! installs this provider and never shows the quads.

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::PrimaryWindow;

use super::config::{ChunkConfig, requested_resolution};
use super::grid::{TILE, TileCoord, tile_center};
use super::lod::Lod;
use super::provider::{
    ChunkFrameStats, StreamFocus, TileCtx, TileHandle, TileLoadError, TileProvider,
    TileProviderSlot,
};
use crate::camera::GameCamera;
use crate::player::Player;

const COLOR_NEAR: Color = Color::srgb(0.18, 0.78, 0.32);
const COLOR_MID: Color = Color::srgb(0.95, 0.62, 0.12);
const COLOR_FAR: Color = Color::srgb(0.25, 0.48, 0.92);
const CLEAR: Color = Color::srgb(0.07, 0.08, 0.11);

#[derive(Component)]
pub(super) struct ChunkDebugCamera;

#[derive(Component)]
pub(super) struct ChunkLegend;

struct DebugTiles {
    quad: Handle<Mesh>,
    near: Handle<StandardMaterial>,
    mid: Handle<StandardMaterial>,
    far: Handle<StandardMaterial>,
}

impl TileProvider for DebugTiles {
    fn load(
        &mut self,
        coord: TileCoord,
        lod: Lod,
        ctx: &mut TileCtx<'_, '_, '_>,
    ) -> Result<TileHandle, TileLoadError> {
        let [cx, cz] = tile_center(coord);
        let material = match lod {
            Lod::Near => self.near.clone(),
            Lod::Mid => self.mid.clone(),
            Lod::Far => self.far.clone(),
        };
        let root = ctx
            .commands
            .spawn((
                Mesh3d(self.quad.clone()),
                MeshMaterial3d(material),
                Transform::from_xyz(cx, 0.35, cz).with_scale(Vec3::new(
                    TILE * 0.92,
                    1.0,
                    TILE * 0.92,
                )),
            ))
            .id();
        Ok(TileHandle::entity(root))
    }

    fn unload(&mut self, _coord: TileCoord, handle: TileHandle, ctx: &mut TileCtx<'_, '_, '_>) {
        if let Some(root) = handle.root {
            ctx.commands.entity(root).despawn();
        }
    }
}

pub(super) fn setup_debug_view(
    mut commands: Commands,
    config: Res<ChunkConfig>,
    mut slot: ResMut<TileProviderSlot>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if let Some((width, height)) = requested_resolution()
        && let Ok(mut window) = windows.single_mut()
    {
        window.resolution.set(width as f32, height as f32);
    }
    if !config.debug_view {
        return;
    }
    if slot.provider.is_none() {
        let mut mat = |color: Color| {
            materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                cull_mode: None,
                ..default()
            })
        };
        slot.provider = Some(Box::new(DebugTiles {
            quad: meshes.add(xz_quad()),
            near: mat(COLOR_NEAR),
            mid: mat(COLOR_MID),
            far: mat(COLOR_FAR),
        }));
    }

    let target = crate::player::spawn_point();
    commands.spawn((
        ChunkDebugCamera,
        Camera3d::default(),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::Custom(CLEAR),
            ..default()
        },
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: overview_height(1.0, &config),
            },
            near: -5000.0,
            far: 5000.0,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(target.x, 800.0, target.z).looking_at(target, Vec3::Z),
    ));

    commands.spawn((
        ChunkLegend,
        Text::new("tile stream"),
        TextFont {
            font_size: FontSize::Px(20.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            bottom: px(12),
            padding: UiRect::all(px(10)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
    ));
}

pub(super) fn suppress_game_camera(
    config: Res<ChunkConfig>,
    mut cameras: Query<&mut Camera, With<GameCamera>>,
) {
    if !config.debug_view {
        return;
    }
    for mut camera in &mut cameras {
        camera.is_active = false;
    }
}

pub(super) fn fit_debug_camera(
    config: Res<ChunkConfig>,
    window: Query<&Window, With<PrimaryWindow>>,
    focus: Res<StreamFocus>,
    player: Query<&Transform, (With<Player>, Without<ChunkDebugCamera>)>,
    mut camera: Query<(&mut Transform, &mut Projection), With<ChunkDebugCamera>>,
) {
    if !config.debug_view {
        return;
    }
    let Ok((mut transform, mut projection)) = camera.single_mut() else {
        return;
    };
    let Some(xz) = focus
        .xz
        .or_else(|| player.iter().next().map(|t| t.translation.xz()))
    else {
        return;
    };
    let target = Vec3::new(xz.x, 0.0, xz.y);
    transform.translation = target + Vec3::Y * 800.0;
    transform.look_at(target, Vec3::Z);
    let aspect = window
        .iter()
        .next()
        .map(|w| {
            let size = w.size();
            if size.y <= 0.0 { 1.0 } else { size.x / size.y }
        })
        .unwrap_or(1.0);
    if let Projection::Orthographic(ortho) = projection.as_mut() {
        ortho.scaling_mode = ScalingMode::FixedVertical {
            viewport_height: overview_height(aspect, &config),
        };
    }
}

pub(super) fn update_legend(
    config: Res<ChunkConfig>,
    stats: Res<ChunkFrameStats>,
    mut text: Query<&mut Text, With<ChunkLegend>>,
) {
    if !config.debug_view {
        return;
    }
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let settle = if stats.settled {
        format!("resident after {} updates", stats.updates_to_settle)
    } else {
        format!("loading… {} tiles", stats.tiles)
    };
    text.0 = format!(
        "TILE STREAM   {TILE:.0} m\n\
         green  near    0–{near_m:.0} m      {near}\n\
         amber  mid     {near_m:.0}–{mid_m:.0} m   {mid}\n\
         blue   far     {mid_m:.0}–{drop_m:.0} m  {far}\n\
         budget {budget}    frame {frame} µs    peak {peak} µs\n\
         {settle}",
        near_m = config.near,
        mid_m = config.mid,
        drop_m = config.drop,
        near = stats.near,
        mid = stats.mid,
        far = stats.far,
        budget = stats.budget,
        frame = stats.last_us,
        peak = stats.peak_us,
    );
}

pub(super) fn capture_debug_shot(
    mut commands: Commands,
    config: Res<ChunkConfig>,
    stats: Res<ChunkFrameStats>,
    mut armed: Local<bool>,
) {
    let Some(path) = config.shot_path.clone() else {
        return;
    };
    if *armed || !stats.settled {
        return;
    }
    *armed = true;
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path))
        .observe(quit_after_shot);
}

fn quit_after_shot(_shot: On<ScreenshotCaptured>, mut exit: MessageWriter<bevy::app::AppExit>) {
    exit.write(bevy::app::AppExit::Success);
}

fn overview_height(aspect: f32, config: &ChunkConfig) -> f32 {
    let radius = ((config.drop + config.hyst) / TILE).ceil();
    let diameter = 2.0 * (radius + 1.5) * TILE;
    if aspect < 1.0 {
        diameter / aspect.max(0.2)
    } else {
        diameter
    }
}

fn xz_quad() -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5, 0.0, -0.5],
            [0.5, 0.0, -0.5],
            [0.5, 0.0, 0.5],
            [-0.5, 0.0, 0.5],
        ],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4]);
    mesh.insert_indices(bevy::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}
