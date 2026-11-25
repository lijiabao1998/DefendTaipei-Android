//! Colored LOD quads. Compiled in, spawned only for `--chunk-debug`,
//! `DEFEND_CHUNK_DEBUG=1`, or `DEFEND_CHUNK_SHOT`. A normal launch never
//! installs this provider and never shows the quads.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::PrimaryWindow;

use super::config::{ChunkConfig, requested_resolution};
use super::grid::{TILE, TileCoord, tile_center};
use super::lod::Lod;
use super::provider::{
    ChunkFrameStats, TileCtx, TileHandle, TileLoadError, TileProvider, TileProviderSlot,
};
use crate::game_flow::{GameFlow, SkipPrologue, StartGame};

const COLOR_NEAR: Color = Color::srgb(0.18, 0.78, 0.32);
const COLOR_MID: Color = Color::srgb(0.95, 0.62, 0.12);
const COLOR_FAR: Color = Color::srgb(0.25, 0.48, 0.92);

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
                Transform::from_xyz(cx, 0.4, cz).with_scale(Vec3::new(
                    TILE * 0.94,
                    1.0,
                    TILE * 0.94,
                )),
                NoFrustumCulling,
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

/// Widen the gameplay camera after its own fit, so the LOD rings fill the window.
/// The follow offset stays; only the orthographic height changes. A 256 m tile is
/// then large enough to read, and the close isometric look is unchanged.
/// `--chunk-debug` is a streaming view, not the title screen. Uses the public
/// start / skip messages so the menu is not edited from here.
pub(super) fn skip_menu_for_debug(
    config: Res<ChunkConfig>,
    state: Res<State<GameFlow>>,
    mut starts: MessageWriter<StartGame>,
    mut skips: MessageWriter<SkipPrologue>,
) {
    if !config.debug_view {
        return;
    }
    match state.get() {
        GameFlow::Title => {
            starts.write(StartGame);
        }
        GameFlow::Prologue => {
            skips.write(SkipPrologue);
        }
        _ => {}
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
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    );
    mesh.insert_indices(bevy::mesh::Indices::U32(vec![0, 2, 1, 0, 3, 2]));
    mesh
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
        format!("loading {} tiles", stats.tiles)
    };
    text.0 = format!(
        "TILE STREAM   {TILE:.0} m\n\
         green  near    0–{near_m:.0} m      {near}\n\
         amber  mid     {near_m:.0}–{mid_m:.0} m   {mid}\n\
         blue   far     {mid_m:.0}–{drop_m:.0} m  {far}\n\
         budget {budget}    frame {frame} us    peak {peak} us\n\
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
    mut frames_ready: Local<u32>,
) {
    let Some(path) = config.shot_path.clone() else {
        return;
    };
    if *armed {
        return;
    }
    // The first presented frame can still be the empty swapchain. Wait until
    // the radius is resident and a few frames have actually been drawn.
    if !stats.settled {
        return;
    }
    *frames_ready += 1;
    if *frames_ready < 15 {
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
