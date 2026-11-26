//! Desktop-only end-to-end visual QA through the real Bevy renderer.
//!
//! `--qa` uses an offscreen texture, with no OS window or display-server socket.
//! All game plugins still run. The virtual primary window provides the same
//! viewport inputs as a real window. Screenshots come from GPU readback, and a
//! save/menu/continue cycle must preserve the live run before QA can succeed.

use std::any::TypeId;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::{
    app::AppExit,
    camera::RenderTarget,
    prelude::*,
    render::{
        render_resource::{TextureFormat, TextureUsages},
        renderer::RenderAdapterInfo,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    time::TimeUpdateStrategy,
    window::PrimaryWindow,
};
use serde::Serialize;

use crate::camera::{CameraZoom, GameCamera};
use crate::game_flow::{Campaign, GameFlow, SkipPrologue};
use crate::i18n::UiFont;
use crate::inventory::Inventory;
use crate::player::{PLAYER_RADIUS, Player};
use crate::roles::SelectedRole;
use crate::save::{SaveAndMenu, SaveStatus};
use crate::survival::{Shelter, Stash, TRADE_SPOTS};
use crate::ui::{OpenPanel, PanelKind, TitleAction, TitleMenu, TitlePage};

const SETTLE_FRAMES: u32 = 60;
const MAX_STAGE_FRAMES: u32 = 1800;

#[derive(Clone, Copy)]
enum Action {
    None,
    SettingsAudio,
    SettingsGame,
    SettingsClose,
    SettingsPause,
    SettingsPerformance,
    SettingsPerformanceLandscape,
    SettingsDiagnostics,
    PerformanceStreet,
    Exterior(ExteriorScene),
    Commercial(CommercialScene),
    ChooseSurvivor,
    ReviewDeparture,
    Depart,
    Skip,
    Street,
    Entrance,
    DoorOpen,
    DoorClose,
    Barricade,
    BarricadeBlocked,
    Unbarricade,
    BarricadeInventory,
    ShopInterior,
    Scavenge,
    ScavengeOne,
    ScavengeAll,
    ShopDepleted,
    HomeInterior,
    Map,
    Rain,
    Night,
    Blackout,
    Journal,
    Pause,
    Warnings,
    Shelter,
    Inventory,
    Trade,
    SaveMenu,
    Continue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExteriorScene {
    Townhouse,
    Walkup,
    Shophouse,
    ServiceStair,
    Rooftop,
    Cutaway,
}

/// Commercial scenes share the normal fixed isometric camera. The final two
/// deliberately repeat the exact Food lot, position and zoom with power on/off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommercialScene {
    Arcade,
    Food,
    Essentials,
    Night,
    Blackout,
}

#[derive(Clone, Copy)]
struct CommercialFixture {
    building: u32,
    position: Vec2,
    zoom: f32,
    scene: CommercialScene,
}

#[derive(Clone, Copy)]
enum ExpectedFlow {
    Title,
    Survivor,
    Briefing,
    Prologue,
    Playing,
}

struct Stage {
    name: &'static str,
    size: [u32; 2],
    action: Action,
    flow: ExpectedFlow,
}

const STAGES: &[Stage] = &[
    Stage {
        name: "01-title-landscape",
        size: [1280, 720],
        action: Action::None,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-title-portrait",
        size: [720, 1280],
        action: Action::None,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-audio-portrait",
        size: [390, 844],
        action: Action::SettingsAudio,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-audio-small-landscape",
        size: [800, 360],
        action: Action::None,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-game-portrait",
        size: [390, 844],
        action: Action::SettingsGame,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-performance-portrait",
        size: [390, 844],
        action: Action::SettingsPerformance,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-performance-small-landscape",
        size: [800, 360],
        action: Action::SettingsPerformanceLandscape,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-diagnostics-portrait",
        size: [390, 844],
        action: Action::SettingsDiagnostics,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02a-title-small-portrait",
        size: [320, 640],
        action: Action::SettingsClose,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02b-survivor-portrait",
        size: [390, 844],
        action: Action::ChooseSurvivor,
        flow: ExpectedFlow::Survivor,
    },
    Stage {
        name: "02c-survivor-small-landscape",
        size: [800, 360],
        action: Action::None,
        flow: ExpectedFlow::Survivor,
    },
    Stage {
        name: "02d-briefing-small-landscape",
        size: [800, 360],
        action: Action::ReviewDeparture,
        flow: ExpectedFlow::Briefing,
    },
    Stage {
        name: "02e-briefing-portrait",
        size: [390, 844],
        action: Action::None,
        flow: ExpectedFlow::Briefing,
    },
    Stage {
        name: "03-prologue",
        size: [1280, 720],
        action: Action::Depart,
        flow: ExpectedFlow::Prologue,
    },
    Stage {
        name: "04-playing",
        size: [1280, 720],
        action: Action::Skip,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04a-streetscape-portrait",
        size: [390, 844],
        action: Action::Street,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04a1-streetscape-scale70-portrait",
        size: [390, 844],
        action: Action::PerformanceStreet,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04b-entrance-landscape",
        size: [1280, 720],
        action: Action::Entrance,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04c-door-open-landscape",
        size: [1280, 720],
        action: Action::DoorOpen,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d-door-closed-landscape",
        size: [1280, 720],
        action: Action::DoorClose,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d1-barricaded-portrait",
        size: [390, 844],
        action: Action::Barricade,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d2-barricade-blocked-portrait",
        size: [390, 844],
        action: Action::BarricadeBlocked,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d3-barricade-removed-landscape",
        size: [1280, 720],
        action: Action::Unbarricade,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d4-barricade-inventory-portrait",
        size: [320, 640],
        action: Action::BarricadeInventory,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d5-shop-interior-landscape",
        size: [1280, 720],
        action: Action::ShopInterior,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d6-scavenge-portrait",
        size: [390, 844],
        action: Action::Scavenge,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d7-scavenge-partial-portrait",
        size: [320, 640],
        action: Action::ScavengeOne,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d8-scavenge-small-landscape",
        size: [800, 360],
        action: Action::None,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d9-scavenge-empty-portrait",
        size: [390, 844],
        action: Action::ScavengeAll,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d10-shop-depleted-landscape",
        size: [1280, 720],
        action: Action::ShopDepleted,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d11-home-interior-portrait",
        size: [390, 844],
        action: Action::HomeInterior,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d12-exterior-townhouse-landscape",
        size: [1280, 720],
        action: Action::Exterior(ExteriorScene::Townhouse),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d13-exterior-walkup-landscape",
        size: [1280, 720],
        action: Action::Exterior(ExteriorScene::Walkup),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d14-exterior-shophouse-landscape",
        size: [1280, 720],
        action: Action::Exterior(ExteriorScene::Shophouse),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d15-exterior-service-stair-landscape",
        size: [1280, 720],
        action: Action::Exterior(ExteriorScene::ServiceStair),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d16-exterior-rooftop-landscape",
        size: [1280, 720],
        action: Action::Exterior(ExteriorScene::Rooftop),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d17-exterior-cutaway-portrait",
        size: [390, 844],
        action: Action::Exterior(ExteriorScene::Cutaway),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d18-commercial-arcade-landscape",
        size: [1280, 720],
        action: Action::Commercial(CommercialScene::Arcade),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d19-commercial-food-landscape",
        size: [1280, 720],
        action: Action::Commercial(CommercialScene::Food),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d20-commercial-essentials-landscape",
        size: [1280, 720],
        action: Action::Commercial(CommercialScene::Essentials),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d21-commercial-night-landscape",
        size: [1280, 720],
        action: Action::Commercial(CommercialScene::Night),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d22-commercial-blackout-landscape",
        size: [1280, 720],
        action: Action::Commercial(CommercialScene::Blackout),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04e-survivor-map-portrait",
        size: [390, 844],
        action: Action::Map,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04f-survivor-map-small-landscape",
        size: [800, 360],
        action: Action::None,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04g-rain-portrait",
        size: [390, 844],
        action: Action::Rain,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04h-night-portrait",
        size: [390, 844],
        action: Action::Night,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04i-blackout-portrait",
        size: [390, 844],
        action: Action::Blackout,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04j-survival-warnings-portrait",
        size: [320, 640],
        action: Action::Warnings,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "05-shelter-landscape",
        size: [1280, 720],
        action: Action::Shelter,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "06-shelter-small-landscape",
        size: [800, 360],
        action: Action::None,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "07-shelter-portrait",
        size: [720, 1280],
        action: Action::None,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "08-inventory-portrait",
        size: [720, 1280],
        action: Action::Inventory,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "08a-radio-journal-portrait",
        size: [390, 844],
        action: Action::Journal,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "08b-pause-small-landscape",
        size: [800, 360],
        action: Action::Pause,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "08c-settings-paused-small-landscape",
        size: [800, 360],
        action: Action::SettingsPause,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09-barter-portrait",
        size: [720, 1280],
        action: Action::Trade,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "10-title-with-save",
        size: [1280, 720],
        action: Action::SaveMenu,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "11-continued",
        size: [1280, 720],
        action: Action::Continue,
        flow: ExpectedFlow::Playing,
    },
];

#[derive(Serialize)]
struct Capture {
    stage: String,
    file: String,
    width: u32,
    height: u32,
    sampled_colors: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    exterior: Option<ExteriorEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commercial: Option<CommercialEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    map_workload: Option<MapWorkload>,
}

/// Counts of map geometry retained at upload, with the real game camera's
/// visibility list. Excludes characters, dynamic doors, interiors and UI.
#[derive(Clone, Serialize)]
struct MapWorkload {
    measurement: &'static str,
    resident_meshes: usize,
    resident_vertices: usize,
    main_view_meshes: usize,
    main_view_vertices: usize,
}

/// Mutually exclusive uploaded groups. A persistent shell contains both the
/// coarse facade and roof, merged by material; it is never counted as roof fittings.
#[derive(Clone, Copy, Default, Serialize)]
struct ExteriorMeshGroup {
    meshes: usize,
    vertices: usize,
    in_main_view: usize,
}

/// The selected lot and actual renderer state travel with its GPU readback.
/// Geometry counts describe workload, never measured device performance.
#[derive(Clone, Serialize)]
struct ExteriorEvidence {
    building_id: u32,
    typology: String,
    scene: String,
    player: [f32; 3],
    camera: [f32; 3],
    zoom: f32,
    projected_bounds: [[f32; 2]; 2],
    balconies: usize,
    service_stair: bool,
    rooftop_shacks: usize,
    roof_access: bool,
    persistent_shell: ExteriorMeshGroup,
    facade_detail: ExteriorMeshGroup,
    roof_fittings: ExteriorMeshGroup,
    total_meshes: usize,
    total_vertices: usize,
    cutaway_hidden: bool,
    streaming: crate::map::MapRenderStats,
}

/// Every feature count is render-derived from the selected existing lot. The
/// uploaded groups include its other shared-material base/shell geometry, so
/// their vertex totals are explicitly not attributed solely to the storefront.
#[derive(Clone, Serialize)]
struct CommercialEvidence {
    visibility_measurement: &'static str,
    building_id: u32,
    family: String,
    sign_index: usize,
    scene: String,
    arcade: bool,
    frontage_width: f32,
    building_height: f32,
    storefront_panels: usize,
    secondary_fascia: bool,
    canopy: bool,
    blade: bool,
    player: [f32; 3],
    camera: [f32; 3],
    zoom: f32,
    projected_bounds: [[f32; 2]; 2],
    commercial_vertices: usize,
    base_batches: ExteriorMeshGroup,
    persistent_shell_batches: ExteriorMeshGroup,
    night: f32,
    power: bool,
    active_lights: usize,
    sign_emissive: [f32; 3],
    streaming: crate::map::MapRenderStats,
}

#[derive(Clone, Copy)]
struct ExteriorFixture {
    building: u32,
    position: Vec2,
    zoom: f32,
    scene: ExteriorScene,
}

#[derive(Serialize)]
struct QaReport {
    success: bool,
    scope: &'static str,
    expected_captures: usize,
    renderer: String,
    captures: Vec<Capture>,
    assertions: Vec<String>,
    error: Option<String>,
}

struct Checkpoint {
    day: u32,
    stash: Stash,
    inventory: Inventory,
    role: SelectedRole,
    position: Vec3,
    game_min: f32,
    map: crate::worldmap::MapProgress,
    closed_doors: Vec<u32>,
    barricaded_doors: Vec<u32>,
    loot: Vec<(String, Stash)>,
    fixtures_version: u8,
}

#[derive(Resource)]
struct QaRun {
    output: PathBuf,
    stages: Vec<usize>,
    target: Option<Handle<Image>>,
    stage: usize,
    entered: bool,
    frame: u32,
    ready_frames: u32,
    wait_logged: Instant,
    scene_chunk: Option<crate::map::ChunkKey>,
    entrance_building: Option<u32>,
    interior_building: Option<u32>,
    exterior: Option<ExteriorFixture>,
    commercial: Option<CommercialFixture>,
    door_test_id: Option<u32>,
    barricade_planks: Option<u32>,
    loot_fixture: Option<String>,
    loot_expected: Option<Stash>,
    loot_inventory: Option<Inventory>,
    waiting_capture: bool,
    captured: bool,
    checkpoint: Option<Checkpoint>,
    continuation_checked: bool,
    started: Instant,
    report: QaReport,
    finished: bool,
}

#[derive(Component)]
struct CaptureTicket {
    name: String,
    path: PathBuf,
    size: [u32; 2],
    settings_pixels: Option<SettingsPixels>,
    exterior: Option<ExteriorEvidence>,
    commercial: Option<CommercialEvidence>,
    map_workload: Option<MapWorkload>,
}

struct SettingsPixels {
    panel: Vec<[u32; 2]>,
    selected_border: Vec<[u32; 2]>,
}

pub struct QaPlugin;

impl Plugin for QaPlugin {
    fn build(&self, app: &mut App) {
        let output = std::env::var_os("DEFEND_TAIPEI_QA_OUT")
            .map_or_else(|| PathBuf::from("qa-artifacts"), PathBuf::from);
        // Development subsets cannot satisfy CI's full capture/save gate.
        let exterior_only =
            std::env::var_os("DEFEND_TAIPEI_QA_EXTERIOR_ONLY").is_some_and(|value| value == "1");
        let commercial_only =
            std::env::var_os("DEFEND_TAIPEI_QA_COMMERCIAL_ONLY").is_some_and(|value| value == "1");
        let stages: Vec<_> = STAGES
            .iter()
            .enumerate()
            .filter_map(|(index, stage)| {
                ((!exterior_only && !commercial_only)
                    || index == 0
                    || matches!(
                        stage.action,
                        Action::ChooseSurvivor
                            | Action::ReviewDeparture
                            | Action::Depart
                            | Action::Skip
                    )
                    || (commercial_only && matches!(stage.action, Action::Commercial(_)))
                    || (!commercial_only
                        && exterior_only
                        && matches!(stage.action, Action::Exterior(_))))
                .then_some(index)
            })
            .collect();
        let scope = if commercial_only {
            "commercial_subset"
        } else if exterior_only {
            "exterior_subset"
        } else {
            "full"
        };
        let expected_captures = stages.len();
        println!("DTQA scope={scope} expected_captures={expected_captures}");
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .insert_resource(QaRun {
            output,
            stages,
            target: None,
            stage: 0,
            entered: false,
            frame: 0,
            ready_frames: 0,
            wait_logged: Instant::now(),
            scene_chunk: None,
            entrance_building: None,
            interior_building: None,
            exterior: None,
            commercial: None,
            door_test_id: None,
            barricade_planks: None,
            loot_fixture: None,
            loot_expected: None,
            loot_inventory: None,
            waiting_capture: false,
            captured: false,
            checkpoint: None,
            continuation_checked: false,
            started: Instant::now(),
            report: QaReport {
                success: false,
                scope,
                expected_captures,
                renderer: String::new(),
                captures: Vec::new(),
                assertions: Vec::new(),
                error: None,
            },
            finished: false,
        })
        .add_systems(Last, drive.after(crate::save::service_requests));
    }
}

fn set_target(world: &mut World, run: &mut QaRun, size: [u32; 2]) -> Result<(), String> {
    let world_camera = world
        .query_filtered::<Entity, With<GameCamera>>()
        .iter(world)
        .next()
        .ok_or("Game camera did not spawn")?;
    // Keep the existing QA renderer contract on the actual world camera.
    world.entity_mut(world_camera).insert(Msaa::Off);
    let camera = world
        .query_filtered::<Entity, With<crate::performance::render::PresentationCamera>>()
        .iter(world)
        .next()
        .ok_or("Presentation camera did not spawn")?;
    let window_entity = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .ok_or("Virtual primary window did not spawn")?;
    {
        let mut window = world
            .get_mut::<Window>(window_entity)
            .ok_or("Missing window dimensions")?;
        window.resolution.set_scale_factor_override(Some(1.0));
        window.resolution.set_physical_resolution(size[0], size[1]);
    }
    let mut image =
        Image::new_target_texture(size[0], size[1], TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let handle = world.resource_mut::<Assets<Image>>().add(image);
    world.entity_mut(camera).insert((
        RenderTarget::Image(handle.clone().into()),
        IsDefaultUiCamera,
        Msaa::Off,
    ));
    run.target = Some(handle);
    Ok(())
}

fn flow_matches(world: &World, expected: ExpectedFlow) -> bool {
    match (world.resource::<State<GameFlow>>().get(), expected) {
        (GameFlow::Title, ExpectedFlow::Title) => {
            world.resource::<TitleMenu>().page == TitlePage::Home
        }
        (GameFlow::Title, ExpectedFlow::Survivor) => {
            world.resource::<TitleMenu>().page == TitlePage::Survivor
        }
        (GameFlow::Title, ExpectedFlow::Briefing) => {
            world.resource::<TitleMenu>().page == TitlePage::Briefing
        }
        (GameFlow::Prologue, ExpectedFlow::Prologue)
        | (GameFlow::Playing, ExpectedFlow::Playing) => true,
        _ => false,
    }
}

fn player_position(world: &mut World) -> Result<Vec3, String> {
    world
        .query_filtered::<&Transform, With<Player>>()
        .iter(world)
        .next()
        .map(|t| t.translation)
        .ok_or_else(|| "Player did not spawn".into())
}

fn reset_and_close_settings(world: &mut World) {
    use crate::ui::settings::{SettingsAction as A, SettingsPage};
    world.write_message(A::Page(SettingsPage::Performance));
    world.write_message(A::Reset);
    world.write_message(A::Page(SettingsPage::Game));
    world.write_message(A::Reset);
    world.write_message(A::Page(SettingsPage::Audio));
    world.write_message(A::Reset);
    world.write_message(A::Close);
}

fn enter_stage(world: &mut World, run: &mut QaRun, stage: &Stage) -> Result<(), String> {
    set_target(world, run, stage.size)?;
    run.ready_frames = 0;
    run.wait_logged = Instant::now();
    run.scene_chunk = None;
    run.entrance_building = None;
    run.interior_building = None;
    run.exterior = None;
    run.commercial = None;
    println!("DTQA entering {}", stage.name);
    if run.stage > 0
        && matches!(
            STAGES[run.stages[run.stage - 1]].action,
            Action::PerformanceStreet
        )
    {
        // Restore only this explicit QA fixture before continuing the original
        // gameplay route. Runtime preferences never change themselves on close.
        *world.resource_mut::<crate::performance::PerformancePreferences>() =
            crate::performance::PerformancePreferences::default();
    }
    match stage.action {
        Action::None => {}
        Action::SettingsAudio => {
            use crate::ui::settings::SettingsAction as A;
            world.write_message(A::Open);
            for (index, value) in [0.65, 0.6, 0.5, 0.4].into_iter().enumerate() {
                world.write_message(A::Volume(index, value));
            }
        }
        Action::SettingsGame => {
            use crate::ui::settings::{SettingsAction as A, SettingsPage};
            world.write_message(A::Page(SettingsPage::Game));
            world.write_message(A::DayHours(2));
            world.write_message(A::Language(crate::i18n::Language::En));
        }
        Action::SettingsPerformance | Action::SettingsPerformanceLandscape => {
            use crate::ui::settings::{PerformanceSection, SettingsAction as A, SettingsPage};
            world.write_message(A::Page(SettingsPage::Performance));
            world.write_message(A::PerformanceSection(PerformanceSection::Display));
            world.write_message(A::PerformanceProfile(
                crate::performance::PerformanceProfile::Balanced,
            ));
            world.write_message(A::FrameCap(30));
            world.write_message(A::RenderScale(70));
            world.write_message(A::Language(
                if matches!(stage.action, Action::SettingsPerformanceLandscape) {
                    crate::i18n::Language::En
                } else {
                    crate::i18n::Language::ZhTw
                },
            ));
        }
        Action::SettingsDiagnostics => {
            use crate::ui::settings::{PerformanceSection, SettingsAction as A};
            world.write_message(A::PerformanceSection(PerformanceSection::Diagnostics));
            world.write_message(A::PerformanceOverlay);
            world.write_message(A::Language(crate::i18n::Language::ZhTw));
        }
        Action::PerformanceStreet => {
            use crate::ui::settings::{SettingsAction as A, SettingsPage};
            // Exercise the real UI actions while paused, then prove closing
            // preserves both the selected preferences and the paused run.
            world.resource_mut::<crate::ui::PauseOpen>().0 = true;
            world.write_message(A::Open);
            world.write_message(A::Page(SettingsPage::Performance));
            world.write_message(A::PerformanceProfile(
                crate::performance::PerformanceProfile::Balanced,
            ));
            world.write_message(A::FrameCap(30));
            world.write_message(A::RenderScale(70));
            world.write_message(A::PerformanceOverlay);
            world.write_message(A::Close);
        }
        Action::SettingsClose => {
            reset_and_close_settings(world);
        }
        Action::SettingsPause => {
            use crate::ui::settings::{SettingsAction as A, SettingsPage};
            world.write_message(A::Open);
            world.write_message(A::Page(SettingsPage::Game));
            world.write_message(A::DayHours(4));
        }
        Action::ChooseSurvivor => {
            world.write_message(TitleAction::NewGame);
        }
        Action::ReviewDeparture => {
            world.write_message(TitleAction::Next);
        }
        Action::Depart => {
            world.write_message(TitleAction::Depart);
        }
        Action::Skip => {
            world.write_message(SkipPrologue);
        }
        Action::Street => {
            // A real street-side camera, with gameplay and the streamed map active.
            // Geometry/material regression evidence, not a synthetic scene.
            let data = crate::map::world();
            let (position, owner) =
                street_view(data).ok_or("No clear main-road sidewalk for QA")?;
            move_view(world, position, 1.35)?;
            run.scene_chunk = Some(owner);
            run.report.assertions.push(format!(
                "Street capture uses a clear sidewalk at ({:.2}, {:.2})",
                position.x, position.y
            ));
        }
        Action::Exterior(scene) => {
            // Actual immutable map lots, ordinary gameplay camera and the same
            // streaming/cutaway systems used on Android. Only location, zoom and
            // daylight are explicit fixtures; no demonstration meshes are spawned.
            let fixture = exterior_fixture(crate::map::world(), scene, stage.size)?;
            println!(
                "DTQA exterior fixture scene={scene:?} lot={} player={:?} zoom={:.3}",
                fixture.building, fixture.position, fixture.zoom
            );
            move_view(world, fixture.position, fixture.zoom)?;
            run.scene_chunk = Some(crate::map::ChunkKey::of(
                crate::map::world().blds[fixture.building as usize].c,
            ));
            run.exterior = Some(fixture);
            world.write_message(OpenPanel(PanelKind::Close));
            world.resource_mut::<crate::ui::PauseOpen>().0 = false;
            world.resource_mut::<Campaign>().game_min = 720.0;
            world
                .resource_mut::<crate::survival::ShelterEnvironment>()
                .power = true;
            let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
            progress.weather.next = 5000.0;
            progress.weather.until = 0.0;
            progress.weather.amount = 0.0;
            progress.weather.intensity = 0.0;
            progress.weather.wet = 0.0;
        }
        Action::Commercial(scene) => {
            let fixture = commercial_fixture(crate::map::world(), scene, stage.size)?;
            println!(
                "DTQA commercial fixture scene={scene:?} lot={} player={:?} zoom={:.3}",
                fixture.building, fixture.position, fixture.zoom
            );
            move_view(world, fixture.position, fixture.zoom)?;
            run.scene_chunk = Some(crate::map::ChunkKey::of(
                crate::map::world().blds[fixture.building as usize].c,
            ));
            run.commercial = Some(fixture);
            world.write_message(OpenPanel(PanelKind::Close));
            world.resource_mut::<crate::ui::PauseOpen>().0 = false;
            world.resource_mut::<Campaign>().game_min =
                if matches!(scene, CommercialScene::Night | CommercialScene::Blackout) {
                    1320.0
                } else {
                    720.0
                };
            world
                .resource_mut::<crate::survival::ShelterEnvironment>()
                .power = scene != CommercialScene::Blackout;
            let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
            progress.weather.next = 5000.0;
            progress.weather.until = 0.0;
            progress.weather.amount = 0.0;
            progress.weather.intensity = 0.0;
            progress.weather.wet = 0.0;
        }
        Action::Entrance => {
            let data = crate::map::world();
            let (building, position) = data
                .blds
                .iter()
                .filter(|building| !building.fill && building.h < 20.0)
                .filter(|building| {
                    !world
                        .resource::<crate::map::Obstacles>()
                        .doors()
                        .is_open(building.id)
                })
                .filter_map(|building| {
                    let door = building.door?;
                    // Our isometric camera looks from +X/+Z: choose an entrance
                    // facing the camera, with a clear place to stand outside it.
                    if door.n.dot(Vec2::ONE.normalize()) < 0.65 {
                        return None;
                    }
                    let position = door.p + door.n * 5.5;
                    (position.cmpge(data.min + 1.0).all()
                        && position.cmple(data.max - 1.0).all()
                        && building.c.distance(position) < 70.0
                        && data.bld_at(position).is_none()
                        && data.free(position, 0.6)
                        && (-5..=55).all(|step| {
                            data.free(door.p + door.n * step as f32 * 0.1, PLAYER_RADIUS)
                        }))
                    .then_some((building, position))
                })
                .min_by(|a, b| {
                    a.1.distance_squared(data.spawn)
                        .total_cmp(&b.1.distance_squared(data.spawn))
                })
                .ok_or("No camera-facing reachable entrance for QA")?;
            move_view(world, position, 1.9)?;
            run.scene_chunk = Some(crate::map::ChunkKey::of(building.c));
            run.entrance_building = Some(building.id);
            run.door_test_id = Some(building.id);
            run.report.assertions.push(format!(
                "Entrance capture uses reachable building {} at ({:.2}, {:.2})",
                building.id, position.x, position.y
            ));
        }
        Action::DoorOpen | Action::DoorClose => {
            let id = run.door_test_id.ok_or("Missing tested entrance")?;
            let building = &crate::map::world().blds[id as usize];
            let door = building.door.ok_or("Tested building has no door")?;
            move_view(world, door.p + door.n * 1.5, 1.9)?;
            world.write_message(crate::worldplay::WorldAction::Interact);
            run.scene_chunk = Some(crate::map::ChunkKey::of(building.c));
            run.entrance_building = Some(id);
        }
        Action::Barricade | Action::Unbarricade | Action::BarricadeInventory => {
            let id = run.door_test_id.ok_or("Missing barricade entrance")?;
            let building = &crate::map::world().blds[id as usize];
            let door = building.door.ok_or("Missing barricade door")?;
            move_view(world, door.p + door.n * 1.5, 2.4)?;
            if matches!(stage.action, Action::Barricade) {
                let mut bag = world.resource_mut::<Inventory>();
                if bag.count("hammer") == 0 {
                    bag.add("hammer", 1);
                }
                bag.add("plank", 2);
                run.barricade_planks = Some(bag.count("plank"));
                run.report.assertions.push("Barricade QA adds an explicit hammer / two-plank fixture, then exercises actual WorldAction handlers".into());
            }
            world.write_message(crate::worldplay::WorldAction::Barricade);
            if matches!(stage.action, Action::BarricadeInventory) {
                world.write_message(OpenPanel(PanelKind::Inventory));
            }
        }
        Action::BarricadeBlocked => {
            world.write_message(crate::worldplay::WorldAction::Interact);
        }
        Action::ShopInterior => {
            world.write_message(OpenPanel(PanelKind::Close));
            world
                .resource_mut::<crate::worldplay::WorldNotice>()
                .text
                .clear();
            world.resource_mut::<Campaign>().notice_t = 0.0;
            let data = crate::map::world();
            let minute = world.resource::<Campaign>().game_min;
            let progress = world.resource::<crate::worldplay::ScavengeState>();
            let fixture = data
                .interiors
                .iter()
                .filter(|f| f.kind == crate::map::interiors::FixtureKind::Shelf)
                .filter(|f| {
                    data.interiors
                        .iter()
                        .filter(|other| other.building == f.building)
                        .count()
                        >= 4
                })
                .filter(|f| {
                    data.blds[f.building as usize]
                        .door
                        .is_some_and(|d| d.p.distance(f.approach) > 3.2)
                })
                .filter(|f| {
                    let stock = crate::worldplay::preview_container_loot(
                        &f.id,
                        f.loot.unwrap(),
                        minute,
                        progress,
                    );
                    stock.stacks().len() >= 2
                        && stock.stacks().iter().map(|(_, n)| *n).sum::<u32>() >= 3
                        && stock
                            .stacks()
                            .iter()
                            .map(|(id, n)| crate::inventory::item(id).unwrap().weight * *n as f32)
                            .sum::<f32>()
                            < 6.0
                })
                .min_by(|a, b| {
                    a.approach
                        .distance_squared(data.spawn)
                        .total_cmp(&b.approach.distance_squared(data.spawn))
                })
                .cloned()
                .ok_or("No reachable furnished shop with a small multi-item shelf")?;
            let stock = crate::worldplay::preview_container_loot(
                &fixture.id,
                fixture.loot.unwrap(),
                minute,
                progress,
            );
            run.loot_expected = Some(stock);
            run.loot_inventory = Some(world.resource::<Inventory>().clone());
            run.loot_fixture = Some(fixture.id.clone());
            run.interior_building = Some(fixture.building);
            move_view(world, fixture.approach, 3.2)?;
            run.scene_chunk = Some(crate::map::ChunkKey::of(fixture.approach));
            run.report.assertions.push(format!("Shop fixture {} at ({:.3}, {:.3}) uses deterministic real stock and shared furniture collision", fixture.id, fixture.approach.x, fixture.approach.y));
        }
        Action::Scavenge => {
            world.write_message(crate::worldplay::WorldAction::Interact);
        }
        Action::ScavengeOne | Action::ScavengeAll => {
            let id = run.loot_fixture.clone().ok_or("Missing shop fixture")?;
            let stock = run.loot_expected.as_mut().ok_or("Missing expected stock")?;
            let bag = run
                .loot_inventory
                .as_mut()
                .ok_or("Missing expected inventory")?;
            let take = if matches!(stage.action, Action::ScavengeOne) {
                let item = stock
                    .stacks()
                    .first()
                    .ok_or("Shelf already empty")?
                    .0
                    .clone();
                if crate::worldplay::pickable_count(bag, &item, 1) != 1 {
                    return Err("QA bag has no room for one shelf item".into());
                }
                stock.take(&item, 1);
                stock.remove_empty();
                bag.add(&item, 1);
                crate::worldplay::LootTake::One(item)
            } else {
                for (item, count) in stock.stacks() {
                    if crate::worldplay::pickable_count(bag, item, *count) != *count {
                        return Err(
                            "QA bag has insufficient capacity for the small shelf fixture".into(),
                        );
                    }
                    bag.add(item, *count);
                }
                *stock = Stash::default();
                crate::worldplay::LootTake::All
            };
            world.write_message(crate::worldplay::WorldAction::TakeLoot {
                container: id,
                take,
            });
        }
        Action::ShopDepleted => {
            world.write_message(OpenPanel(PanelKind::Close));
        }
        Action::HomeInterior => {
            world
                .resource_mut::<crate::worldplay::WorldNotice>()
                .text
                .clear();
            world.resource_mut::<Campaign>().notice_t = 0.0;
            let data = crate::map::world();
            let fixture = data
                .interiors
                .iter()
                .filter(|f| f.kind == crate::map::interiors::FixtureKind::Altar)
                .filter(|f| f.front.dot(crate::camera::CAM_OFFSET.xz().normalize()) > 0.3)
                .filter(|f| {
                    data.interiors
                        .iter()
                        .filter(|other| other.building == f.building)
                        .count()
                        >= 5
                })
                .min_by(|a, b| {
                    a.approach
                        .distance_squared(data.spawn)
                        .total_cmp(&b.approach.distance_squared(data.spawn))
                })
                .ok_or("No furnished home with a family shrine")?;
            move_view(world, fixture.approach, 2.8)?;
            run.interior_building = Some(fixture.building);
            run.scene_chunk = Some(crate::map::ChunkKey::of(fixture.approach));
            run.report.assertions.push(format!(
                "Home interior capture uses building {} with collision-clear furniture approaches",
                fixture.building
            ));
        }
        Action::Map => {
            // Resume the pre-existing route at its exterior-cutaway camera,
            // time, power and weather after the commercial day/night sequence.
            // Exploration from the extra real-world fixture positions remains;
            // these stages are retained, not claimed to have identical pixels.
            if run.stage > 0
                && matches!(
                    STAGES[run.stages[run.stage - 1]].action,
                    Action::Commercial(_)
                )
            {
                let previous =
                    exterior_fixture(crate::map::world(), ExteriorScene::Cutaway, [390, 844])?;
                move_view(world, previous.position, previous.zoom)?;
                world.resource_mut::<Campaign>().game_min = 720.0;
                world
                    .resource_mut::<crate::survival::ShelterEnvironment>()
                    .power = true;
                let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
                progress.weather.next = 5000.0;
                progress.weather.until = 0.0;
                progress.weather.amount = 0.0;
                progress.weather.intensity = 0.0;
                progress.weather.wet = 0.0;
            }
            let p = player_position(world)?.xz();
            let mut progress = world.resource_mut::<crate::worldmap::MapProgress>();
            progress.add_note(
                p + Vec2::new(-20.0, 10.0),
                crate::worldmap::NoteKind::Supplies,
            );
            progress.add_note(
                p + Vec2::new(24.0, -12.0),
                crate::worldmap::NoteKind::Danger,
            );
            world.write_message(crate::worldmap::ToggleMap);
        }
        Action::Rain => {
            world.write_message(OpenPanel(PanelKind::Close));
            let (p, owner) = street_view(crate::map::world()).ok_or("No rain street view")?;
            move_view(world, p, 1.35)?;
            run.scene_chunk = Some(owner);
            world.resource_mut::<Campaign>().game_min = 720.0;
            let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
            progress.weather.next = 5000.0;
            progress.weather.until = 900.0;
            progress.weather.amount = 0.85;
            progress.weather.intensity = 0.85;
            progress.weather.wet = 1.0;
            run.report.assertions.push("Rain/night captures use explicit visual weather fixtures in the real running world".into());
        }
        Action::Night => {
            world.resource_mut::<Campaign>().game_min = 1320.0;
            let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
            progress.weather.until = 0.0;
            progress.weather.amount = 0.0;
        }
        Action::Blackout => {
            world
                .resource_mut::<crate::survival::ShelterEnvironment>()
                .power = false;
        }
        Action::Warnings => {
            let mut needs = world.resource_mut::<crate::worldplay::PlayerNeeds>();
            needs.hunger = 22.0;
            needs.thirst = 18.0;
            needs.fatigue = 82.0;
            world.resource_mut::<crate::combat::GameState>().bleed = true;
            world.resource_mut::<Campaign>().notice_t = 12.0;
            world.resource_mut::<crate::worldplay::WorldNotice>().text = "流血，需要止血。".into();
        }
        Action::Journal => {
            world.write_message(OpenPanel(PanelKind::Journal));
        }
        Action::Pause => {
            world.write_message(OpenPanel(PanelKind::Close));
            world.write_message(crate::ui::TogglePause);
        }
        Action::Shelter => {
            world.resource_mut::<Campaign>().game_min = 1440.0 + 720.0;
            world
                .resource_mut::<crate::survival::ShelterEnvironment>()
                .power = true;
            *world.resource_mut::<crate::worldplay::PlayerNeeds>() = default();
            let mut game = world.resource_mut::<crate::combat::GameState>();
            game.bleed = false;
            game.hp = 100.0;
            move_view(world, crate::player::spawn_point().xz(), 1.0)?;
            world.write_message(OpenPanel(PanelKind::Shelter));
        }
        Action::Inventory => {
            world.write_message(OpenPanel(PanelKind::Inventory));
        }
        Action::Trade => {
            reset_and_close_settings(world);
            // Keep the game paused until the queued settings resets and close
            // have run. Unpausing here would invalidate the open overlay before
            // its action handler can apply either reset.
            // Known authored barter location. This fixture avoids turning a UI
            // regression run into several minutes of walking across the map.
            let pos = TRADE_SPOTS[0].pos;
            let mut query = world.query_filtered::<&mut Transform, With<Player>>();
            let mut player = query.iter_mut(world).next().ok_or("Player did not spawn")?;
            player.translation.x = pos[0];
            player.translation.z = pos[1];
            // Open after the settings close pointer has cleared modal capture.
        }
        Action::SaveMenu => {
            let position = player_position(world)?;
            run.checkpoint = Some(Checkpoint {
                day: world.resource::<Shelter>().day,
                stash: world.resource::<Shelter>().stash.clone(),
                inventory: world.resource::<Inventory>().clone(),
                role: *world.resource::<SelectedRole>(),
                position,
                game_min: world.resource::<Campaign>().game_min,
                map: world.resource::<crate::worldmap::MapProgress>().clone(),
                closed_doors: world
                    .resource::<crate::worldplay::ScavengeState>()
                    .closed_doors
                    .clone(),
                barricaded_doors: world
                    .resource::<crate::worldplay::ScavengeState>()
                    .barricaded_doors
                    .clone(),
                loot: world
                    .resource::<crate::worldplay::ScavengeState>()
                    .loot
                    .clone(),
                fixtures_version: world
                    .resource::<crate::worldplay::ScavengeState>()
                    .fixtures_version,
            });
            world.write_message(OpenPanel(PanelKind::Close));
            world.write_message(SaveAndMenu);
        }
        Action::Continue => {
            let status = world.resource::<SaveStatus>();
            if !status.available || status.error.is_some() {
                return Err(format!("Save/menu failed: {}", status.message()));
            }
            run.report
                .assertions
                .push("SaveAndMenu wrote a valid save before resetting the run".into());
            world.write_message(TitleAction::Continue);
        }
    }
    run.entered = true;
    run.frame = 0;
    Ok(())
}

fn move_view(world: &mut World, position: Vec2, zoom: f32) -> Result<(), String> {
    let mut query = world.query_filtered::<&mut Transform, With<Player>>();
    let mut player = query.iter_mut(world).next().ok_or("Player did not spawn")?;
    player.translation.x = position.x;
    player.translation.z = position.y;
    world.resource_mut::<CameraZoom>().0 = zoom;
    Ok(())
}

/// Put the actual player on the far sidewalk, looking across the carriageway.
/// A merely walkable point can still put a roof or tree crown in front of the
/// camera; that would produce a valid image without useful pavement evidence.
fn street_view(data: &crate::map::WorldData) -> Option<(Vec2, crate::map::ChunkKey)> {
    let camera = Vec2::ONE.normalize();
    let mut candidates = Vec::new();
    for road in data
        .roads
        .iter()
        .filter(|road| road.w >= 12.0 && road.w <= 22.0)
    {
        for pair in road.p.windows(2) {
            let length = pair[0].distance(pair[1]);
            if length < 30.0 {
                continue;
            }
            let direction = (pair[1] - pair[0]) / length;
            let normal = Vec2::new(-direction.y, direction.x);
            for side in [-1.0, 1.0] {
                let outward = normal * side;
                if outward.dot(camera) >= -0.35 {
                    continue;
                }
                for along in [0.3, 0.45, 0.6, 0.75] {
                    if (along * length).min((1.0 - along) * length) < road.w / 2.0 + 5.0 {
                        continue;
                    }
                    let middle = pair[0].lerp(pair[1], along);
                    let p = middle + outward * (road.w / 2.0 + 1.1);
                    if !p.cmpge(data.min + 1.0).all()
                        || !p.cmple(data.max - 1.0).all()
                        || !data.free(p, 0.6)
                        || data.bld_at(middle).is_some()
                        || ![-8.0, -4.0, 0.0, 4.0, 8.0].into_iter().all(|distance| {
                            let q = p + direction * distance;
                            data.bld_at(q).is_none() && data.road_dist(q) > -0.05
                        })
                        || data.props.iter().any(|prop| {
                            matches!(prop.kind, crate::map::street::PropKind::Tree(_))
                                && prop.p.distance(p) < 5.0
                        })
                        || (2..=18).any(|distance| {
                            data.bld_at(p + camera * distance as f32)
                                .is_some_and(|building| {
                                    building.h + 3.0 > distance as f32 * 56.0 / 40.0_f32.hypot(40.0)
                                })
                        })
                    {
                        continue;
                    }
                    candidates.push((p, crate::map::ChunkKey::of((pair[0] + pair[1]) / 2.0)));
                }
            }
        }
    }
    candidates.into_iter().min_by(|a, b| {
        a.0.distance_squared(data.spawn)
            .total_cmp(&b.0.distance_squared(data.spawn))
    })
}

fn exterior_bounds(building: &crate::map::data::Bld) -> Vec<Vec3> {
    building
        .pts
        .iter()
        .flat_map(|point| {
            [
                Vec3::new(point.x, 0.0, point.y),
                Vec3::new(point.x, building.h, point.y),
            ]
        })
        .collect()
}

/// Fit real generated geometry, not a large generic padding cube. This keeps a
/// full-building view close enough to inspect its facade while retaining every
/// balcony, stair, sign, roof and antenna that the normal renderer will upload.
fn exterior_geometry_bounds(
    building: &crate::map::data::Bld,
    data: &crate::map::WorldData,
    kit: &crate::map::detail::WindowKit,
) -> Option<Vec<Vec3>> {
    let mut base = crate::map::build::building(building, crate::map::Lod::Near);
    let structure = crate::map::exterior::structure(building, data, crate::map::Lod::Near);
    let detail = crate::map::detail::near_detail(building, kit);
    let fittings = crate::map::detail::near_roof(building);
    if structure.vcount() == 0 || detail.vcount() < 100 || fittings.vcount() < 20 {
        return None;
    }
    // Match runtime material batching before computing AABBs. A combined roof
    // and projecting facade has corner combinations absent from separate AABBs;
    // fitting them separately cropped the first walkup fixture at lot854.
    base.structure = structure;
    base.merge_structure_into_roof();
    let mut bounds = exterior_bounds(building);
    for parts in [base.walls, base.roof, base.base, detail, fittings] {
        for mesh in parts.0.values() {
            let (min, max) = mesh.aabb();
            bounds.extend(box_corners(min, max));
        }
    }
    Some(bounds)
}

fn box_corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(max.x, max.y, max.z),
    ]
}

/// Fit the entire real lot, including rooftop additions, with a useful margin.
/// This changes normal gameplay zoom, not the camera direction or LOD distance.
fn exterior_zoom(bounds: &[Vec3], position: Vec2, size: [u32; 2]) -> Option<f32> {
    let right = Vec3::new(1.0, 0.0, -1.0).normalize();
    let up = crate::camera::CAM_OFFSET.normalize().cross(right);
    let focus = position.extend(0.0).xzy();
    let (mut horizontal, mut vertical) = (0.0_f32, 0.0_f32);
    for point in bounds {
        horizontal = horizontal.max((*point - focus).dot(right).abs());
        vertical = vertical.max((*point - focus).dot(up).abs());
    }
    let aspect = size[0] as f32 / size[1] as f32;
    let half = crate::camera::half_height(aspect, 1.0);
    let zoom = (half * aspect * 0.89 / horizontal.max(0.1))
        .min(half * 0.89 / vertical.max(0.1))
        .min(*crate::camera::ZOOM_RANGE.end());
    (zoom >= *crate::camera::ZOOM_RANGE.start()).then_some(zoom)
}

fn exterior_camera_clear(data: &crate::map::WorldData, building: u32, target: Vec3) -> bool {
    let camera = Vec2::ONE.normalize();
    // A traversable player point alone does not prove the facade can be seen:
    // reject taller lots along the camera-to-subject ray, including roof clutter.
    (2..=32).step_by(2).all(|step| {
        let distance = step as f32;
        let p = target.xz() + camera * distance;
        let height = target.y + distance * crate::camera::CAM_OFFSET.y / 40.0_f32.hypot(40.0);
        data.bld_at(p)
            .is_none_or(|other| other.id == building || other.h + 4.0 < height)
            && !data.props.iter().any(|prop| {
                matches!(prop.kind, crate::map::street::PropKind::Tree(_))
                    && prop.p.distance(p) < 2.0
                    && height < 8.0
            })
    })
}

fn exterior_fixture(
    data: &crate::map::WorldData,
    scene: ExteriorScene,
    size: [u32; 2],
) -> Result<ExteriorFixture, String> {
    use crate::map::exterior::{Typology, classify, plan};

    if scene == ExteriorScene::Cutaway {
        // Use the same walkup as its full facade view, then actually put the
        // player inside its ground floor. The shared cutaway must hide every
        // architecture and rooftop mesh, including the newly added structures.
        let outside = exterior_fixture(data, ExteriorScene::Walkup, [1280, 720])?;
        let b = &data.blds[outside.building as usize];
        let mut candidates = vec![b.c];
        for x in 1..10 {
            for z in 1..10 {
                candidates.push(b.min + (b.max - b.min) * Vec2::new(x as f32, z as f32) / 10.0);
            }
        }
        candidates.sort_by(|a, b0| a.distance_squared(b.c).total_cmp(&b0.distance_squared(b.c)));
        let position = candidates
            .into_iter()
            .find(|p| data.bld_at(*p).is_some_and(|inside| inside.id == b.id) && data.free(*p, 0.6))
            .ok_or_else(|| format!("Exterior cutaway lot {} has no clear interior", b.id))?;
        return Ok(ExteriorFixture {
            building: b.id,
            position,
            zoom: 1.2,
            scene,
        });
    }

    let camera = Vec2::ONE.normalize();
    let mut candidates = Vec::new();
    for b in &data.blds {
        let typology = classify(b);
        let matches = match scene {
            ExteriorScene::Townhouse => typology == Typology::Townhouse,
            ExteriorScene::Walkup | ExteriorScene::ServiceStair => typology == Typology::Walkup,
            ExteriorScene::Shophouse => typology == Typology::Shophouse,
            ExteriorScene::Rooftop => b.shack && typology != Typology::Other,
            ExteriorScene::Cutaway => unreachable!(),
        };
        if !matches
            || b.h > 17.0
            || (b.max - b.min).max_element() > 32.0
            || (scene == ExteriorScene::Walkup && b.lv < 3)
        {
            continue;
        }
        if scene == ExteriorScene::Rooftop {
            let roof = crate::map::roofs::layout(b);
            if roof.shacks.is_empty() || roof.access.is_none() {
                continue;
            }
        }
        let layout = plan(b, data);
        let (centre, normal) = if scene == ExteriorScene::ServiceStair {
            let Some(stair) = layout.stair else { continue };
            (stair.centre, stair.n)
        } else {
            let Some(front) = layout.spec.front else {
                continue;
            };
            if scene != ExteriorScene::Rooftop && layout.balconies.is_empty() {
                continue;
            }
            (front.centre(), front.n)
        };
        if normal.dot(camera) < 0.55
            || !exterior_camera_clear(data, b.id, Vec3::new(centre.x, b.h * 0.55, centre.y))
        {
            continue;
        }
        for distance in [3.5, 5.5, 8.0, 10.0] {
            let position = centre + normal * distance;
            if !position.cmpge(data.min + 1.0).all()
                || !position.cmple(data.max - 1.0).all()
                || data.bld_at(position).is_some()
                || !data.free(position, 0.6)
                || b.c.distance(position) >= 65.0
                || crate::map::exterior_blocks_camera(b, position)
            {
                continue;
            }
            let Some(zoom) = exterior_zoom(&exterior_bounds(b), position, size) else {
                continue;
            };
            candidates.push(ExteriorFixture {
                building: b.id,
                position,
                zoom,
                scene,
            });
            break;
        }
    }
    // Whole-lot readability comes before proximity to spawn. The closest infill
    // lot previously forced a wide, crowded view and could hide its own rear stair.
    candidates.sort_by(|a, b| {
        b.zoom
            .total_cmp(&a.zoom)
            .then_with(|| {
                data.blds[a.building as usize]
                    .fill
                    .cmp(&data.blds[b.building as usize].fill)
            })
            .then_with(|| {
                a.position
                    .distance_squared(data.spawn)
                    .total_cmp(&b.position.distance_squared(data.spawn))
            })
            .then(a.building.cmp(&b.building))
    });
    let kit = crate::map::detail::WindowKit::new();
    let mut fitted = Vec::new();
    for mut fixture in candidates.into_iter().take(24) {
        let b = &data.blds[fixture.building as usize];
        let Some(bounds) = exterior_geometry_bounds(b, data, &kit) else {
            continue;
        };
        let Some(zoom) = exterior_zoom(&bounds, fixture.position, size) else {
            continue;
        };
        fixture.zoom = zoom;
        fitted.push(fixture);
    }
    fitted
        .into_iter()
        .max_by(|a, b| {
            a.zoom
                .total_cmp(&b.zoom)
                .then_with(|| {
                    data.blds[b.building as usize]
                        .fill
                        .cmp(&data.blds[a.building as usize].fill)
                })
                .then_with(|| {
                    b.position
                        .distance_squared(data.spawn)
                        .total_cmp(&a.position.distance_squared(data.spawn))
                })
                .then(b.building.cmp(&a.building))
        })
        .ok_or_else(|| format!("No clear, fully framed real-world {scene:?} exterior fixture"))
}

fn exterior_ready(world: &mut World, fixture: ExteriorFixture) -> bool {
    let id = fixture.building;
    world
        .query::<&crate::map::DetailRoot>()
        .iter(world)
        .any(|root| root.0 == id)
        && world
            .query::<&crate::map::BldWalls>()
            .iter(world)
            .any(|walls| walls.id == id && walls.faded == (fixture.scene == ExteriorScene::Cutaway))
        && exterior_visibility(world, fixture).is_some()
}

fn main_camera_meshes(world: &mut World) -> Option<HashSet<Entity>> {
    world
        .query_filtered::<&bevy::camera::visibility::VisibleEntities, With<GameCamera>>()
        .iter(world)
        .next()
        .map(|visible| visible.iter(TypeId::of::<Mesh3d>()).copied().collect())
}

/// One entity belongs to exactly one evidence group. Persistent shells carry the
/// roof tag for cutaway plus BldStructure for ownership; they never count twice.
fn exterior_group(
    id: u32,
    shell: Option<&crate::map::BldStructure>,
    detail: Option<&crate::map::BldDetail>,
    roof: Option<&crate::map::BldRoof>,
) -> Result<Option<usize>, &'static str> {
    if !shell.is_some_and(|tag| tag.0 == id)
        && !detail.is_some_and(|tag| tag.0 == id)
        && !roof.is_some_and(|tag| tag.0 == id)
    {
        return Ok(None);
    }
    match (shell, detail, roof) {
        (Some(shell), None, Some(roof)) if shell.0 == id && roof.0 == id => Ok(Some(0)),
        (None, Some(detail), None) if detail.0 == id => Ok(Some(1)),
        (None, None, Some(roof)) if roof.0 == id => Ok(Some(2)),
        _ => Err("conflicting exterior ownership/cutaway tags"),
    }
}

/// Wait for propagated visibility and membership of the actual game-camera list.
/// All three groups must exist; the combined shell cannot stand in for fine detail.
fn exterior_visibility(world: &mut World, fixture: ExteriorFixture) -> Option<[usize; 3]> {
    let main_view = main_camera_meshes(world)?;
    let hidden = fixture.scene == ExteriorScene::Cutaway;
    let (mut total, mut visible) = ([0_usize; 3], [0_usize; 3]);
    for (entity, shell, detail, roof, local, inherited, view) in world
        .query_filtered::<(
            Entity,
            Option<&crate::map::BldStructure>,
            Option<&crate::map::BldDetail>,
            Option<&crate::map::BldRoof>,
            &Visibility,
            &InheritedVisibility,
            &ViewVisibility,
        ), Or<(With<crate::map::BldDetail>, With<crate::map::BldRoof>)>>()
        .iter(world)
    {
        let Some(group) = exterior_group(fixture.building, shell, detail, roof).ok()? else {
            continue;
        };
        let in_main_view = main_view.contains(&entity);
        if hidden {
            if *local != Visibility::Hidden || inherited.get() || view.get() || in_main_view {
                return None;
            }
        } else if *local == Visibility::Hidden || !inherited.get() || !view.get() {
            return None;
        }
        total[group] += 1;
        visible[group] += usize::from(in_main_view);
    }
    (total.iter().all(|count| *count > 0) && (hidden || visible.iter().all(|count| *count > 0)))
        .then_some(visible)
}

/// One bounded diagnostic at most every five seconds while a fixture is waiting.
/// Keep the exact reason in the log instead of silently exhausting the QA deadline.
fn exterior_wait_status(world: &mut World, fixture: ExteriorFixture) -> String {
    let id = fixture.building;
    let main_view = main_camera_meshes(world).unwrap_or_default();
    let (mut total, mut local_hidden, mut inherited, mut view, mut main) =
        ([0; 3], [0; 3], [0; 3], [0; 3], [0; 3]);
    let mut points = Vec::new();
    let mut invalid_tags = 0;
    for (
        entity,
        shell,
        detail,
        roof,
        local,
        inherited_visibility,
        view_visibility,
        geometry,
        transform,
    ) in world
        .query_filtered::<(
            Entity,
            Option<&crate::map::BldStructure>,
            Option<&crate::map::BldDetail>,
            Option<&crate::map::BldRoof>,
            &Visibility,
            &InheritedVisibility,
            &ViewVisibility,
            &crate::map::MapMeshGeometry,
            &GlobalTransform,
        ), Or<(With<crate::map::BldDetail>, With<crate::map::BldRoof>)>>()
        .iter(world)
    {
        let group = match exterior_group(id, shell, detail, roof) {
            Ok(Some(group)) => group,
            Ok(None) => continue,
            Err(_) => {
                invalid_tags += 1;
                continue;
            }
        };
        total[group] += 1;
        local_hidden[group] += usize::from(*local == Visibility::Hidden);
        inherited[group] += usize::from(inherited_visibility.get());
        view[group] += usize::from(view_visibility.get());
        main[group] += usize::from(main_view.contains(&entity));
        points.extend(
            box_corners(geometry.min, geometry.max).map(|point| transform.transform_point(point)),
        );
    }
    let walls: Vec<_> = world
        .query::<&crate::map::BldWalls>()
        .iter(world)
        .filter(|walls| walls.id == id)
        .map(|walls| walls.faded)
        .collect();
    let player = player_position(world).ok();
    let blocks = player.map(|player| {
        crate::map::exterior_blocks_camera(&crate::map::world().blds[id as usize], player.xz())
    });
    let bounds = world
        .query_filtered::<(&Camera, &GlobalTransform), With<GameCamera>>()
        .iter(world)
        .next()
        .map(|(camera, transform)| {
            let projected: Vec<_> = points
                .into_iter()
                .filter_map(|point| camera.world_to_viewport(transform, point).ok())
                .collect();
            if projected.is_empty() {
                return "none".to_string();
            }
            let (lo, hi) = projected.into_iter().fold(
                (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
                |(lo, hi), point| (lo.min(point), hi.max(point)),
            );
            format!("{lo:?}..{hi:?}")
        })
        .unwrap_or_else(|| "no-camera".to_string());
    format!(
        "lot={id} cutaway_front={blocks:?} walls_faded={walls:?} groups(shell/facade/fittings)={total:?} hidden={local_hidden:?} inherited={inherited:?} view={view:?} main_view={main:?} invalid_tags={invalid_tags} projected={bounds}"
    )
}

fn map_workload(world: &mut World) -> Option<MapWorkload> {
    let visible = main_camera_meshes(world)?;
    let mut workload = MapWorkload {
        measurement: "uploaded_map_mesh_counts_not_gpu_time_or_device_performance",
        resident_meshes: 0,
        resident_vertices: 0,
        main_view_meshes: 0,
        main_view_vertices: 0,
    };
    for (entity, geometry) in world
        .query_filtered::<(Entity, &crate::map::MapMeshGeometry), With<Mesh3d>>()
        .iter(world)
    {
        workload.resident_meshes += 1;
        workload.resident_vertices += geometry.vertices;
        if visible.contains(&entity) {
            workload.main_view_meshes += 1;
            workload.main_view_vertices += geometry.vertices;
        }
    }
    Some(workload)
}

fn exterior_evidence(
    world: &mut World,
    fixture: ExteriorFixture,
    size: [u32; 2],
) -> Result<ExteriorEvidence, String> {
    let id = fixture.building;
    let b = &crate::map::world().blds[id as usize];
    let layout = crate::map::exterior::plan(b, crate::map::world());
    let roof_layout = crate::map::roofs::layout(b);
    let hidden = fixture.scene == ExteriorScene::Cutaway;
    let visible = exterior_visibility(world, fixture).ok_or_else(|| {
        format!("Exterior {id}: hierarchy or game-camera visibility changed before capture")
    })?;
    let mut groups = [ExteriorMeshGroup::default(); 3];
    let mut uploaded_bounds = Vec::new();
    let mut handles = Vec::new();
    for (shell, detail, roof, mesh, geometry, transform) in world
        .query_filtered::<(
            Option<&crate::map::BldStructure>,
            Option<&crate::map::BldDetail>,
            Option<&crate::map::BldRoof>,
            &Mesh3d,
            &crate::map::MapMeshGeometry,
            &GlobalTransform,
        ), Or<(With<crate::map::BldDetail>, With<crate::map::BldRoof>)>>()
        .iter(world)
    {
        let Some(group) = exterior_group(id, shell, detail, roof)? else {
            continue;
        };
        groups[group].meshes += 1;
        groups[group].vertices += geometry.vertices;
        handles.push(mesh.0.clone());
        uploaded_bounds.extend(
            box_corners(geometry.min, geometry.max).map(|point| transform.transform_point(point)),
        );
    }
    for (group, visible) in groups.iter_mut().zip(visible) {
        group.in_main_view = visible;
    }
    let meshes = world.resource::<Assets<Mesh>>();
    if handles.iter().any(|handle| !meshes.contains(handle)) {
        return Err(format!("Exterior {id}: missing uploaded mesh"));
    }
    if groups
        .iter()
        .any(|group| group.meshes == 0 || group.vertices < 20)
    {
        return Err(format!(
            "Exterior {id}: missing persistent shell, facade detail or roof fittings"
        ));
    }
    let position = player_position(world)?;
    let (camera, transform) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<GameCamera>>()
        .iter(world)
        .next()
        .ok_or("Exterior camera is missing")?;
    let camera_position = transform.translation();
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for point in exterior_bounds(b).into_iter().chain(uploaded_bounds) {
        let projected = camera
            .world_to_viewport(transform, point)
            .map_err(|err| format!("Exterior {id}: cannot project target: {err:?}"))?;
        lo = lo.min(projected);
        hi = hi.max(projected);
    }
    if !hidden
        && (lo.x < size[0] as f32 * 0.04
            || lo.y < size[1] as f32 * 0.04
            || hi.x > size[0] as f32 * 0.96
            || hi.y > size[1] as f32 * 0.96)
    {
        return Err(format!(
            "Exterior {id}: full building is cropped: {lo:?}..{hi:?} in {size:?}"
        ));
    }
    if hidden
        && (crate::map::world()
            .bld_at(position.xz())
            .is_none_or(|inside| inside.id != id)
            || !world
                .query::<(&crate::map::BldWalls, &Transform)>()
                .iter(world)
                .any(|(walls, transform)| {
                    walls.id == id && walls.faded && (transform.scale.y - 0.12).abs() < 0.001
                }))
    {
        return Err(format!(
            "Exterior {id}: cutaway did not follow actual interior player position"
        ));
    }
    Ok(ExteriorEvidence {
        building_id: id,
        typology: format!("{:?}", layout.spec.typology),
        scene: format!("{:?}", fixture.scene),
        player: position.to_array(),
        camera: camera_position.to_array(),
        zoom: world.resource::<CameraZoom>().0,
        projected_bounds: [lo.to_array(), hi.to_array()],
        balconies: layout.balconies.len(),
        service_stair: layout.stair.is_some(),
        rooftop_shacks: roof_layout.shacks.len(),
        roof_access: roof_layout.access.is_some(),
        persistent_shell: groups[0],
        facade_detail: groups[1],
        roof_fittings: groups[2],
        total_meshes: groups.iter().map(|group| group.meshes).sum(),
        total_vertices: groups.iter().map(|group| group.vertices).sum(),
        cutaway_hidden: hidden,
        streaming: world.resource::<crate::map::MapRenderStats>().clone(),
    })
}

/// Focus the actual commercial assembly rather than zooming out to an entire
/// tall roof. The camera offset, projection and gameplay zoom limits stay normal.
fn commercial_zoom(bounds: &[Vec3], position: Vec2, size: [u32; 2]) -> Option<f32> {
    let right = Vec3::new(1.0, 0.0, -1.0).normalize();
    let up = crate::camera::CAM_OFFSET.normalize().cross(right);
    let focus = position.extend(0.0).xzy();
    let (mut horizontal, mut vertical) = (0.0_f32, 0.0_f32);
    for point in bounds {
        horizontal = horizontal.max((*point - focus).dot(right).abs());
        vertical = vertical.max((*point - focus).dot(up).abs());
    }
    let aspect = size[0] as f32 / size[1] as f32;
    let half = crate::camera::half_height(aspect, 1.0);
    // Leave room for the ordinary HUD while keeping the storefront legible.
    let zoom = (half * aspect * 0.76 / horizontal.max(0.1))
        .min(half * 0.68 / vertical.max(0.1))
        .min(*crate::camera::ZOOM_RANGE.end());
    (zoom >= *crate::camera::ZOOM_RANGE.start()).then_some(zoom)
}

fn commercial_fixture(
    data: &crate::map::WorldData,
    scene: CommercialScene,
    size: [u32; 2],
) -> Result<CommercialFixture, String> {
    use crate::map::commercial::{ShopFamily, geometry, plan};

    // This canonical selector makes all three lighting states use the identical
    // lot and ordinary camera. It is independent of mutable game/weather state.
    let kind = match scene {
        CommercialScene::Night | CommercialScene::Blackout => CommercialScene::Food,
        other => other,
    };
    let mut candidates = Vec::new();
    for b in &data.blds {
        let Some(layout) = plan(b, data) else {
            continue;
        };
        let eligible = match kind {
            CommercialScene::Arcade => {
                b.arcade.is_some()
                    && layout.panels.len() >= 2
                    && layout.secondary_fascia
                    && (8.0..=16.0).contains(&layout.frontage.width)
                    && b.h <= 12.0
            }
            CommercialScene::Food => {
                b.arcade.is_none()
                    && layout.family == ShopFamily::Food
                    && layout.canopy.is_some()
                    && layout.blade.is_some()
                    // Both the fascia and the blade's printed side must face
                    // the fixed camera; a front-on blade would be edge-on.
                    && layout.frontage.u.dot(Vec2::ONE.normalize()).abs() >= 0.35
                    && !layout.panels.is_empty()
            }
            CommercialScene::Essentials => {
                matches!(
                    layout.family,
                    ShopFamily::Pharmacy | ShopFamily::Convenience
                ) && !layout.panels.is_empty()
            }
            _ => unreachable!(),
        };
        let front = layout.frontage;
        if !eligible
            || front.n.dot(Vec2::ONE.normalize()) < 0.6
            || front.width > 24.0
            || !exterior_camera_clear(
                data,
                b.id,
                Vec3::new(front.centre().x, 2.4, front.centre().y),
            )
        {
            continue;
        }
        let generated = geometry(b, &layout, crate::map::Lod::Near);
        let bounds: Vec<Vec3> = generated
            .base
            .0
            .values()
            .chain(generated.shell.0.values())
            .flat_map(|mesh| mesh.pos.iter().copied().map(Vec3::from))
            .collect();
        if bounds.len() < 40 {
            continue;
        }
        for distance in [2.0, 3.0, 4.5, 6.0] {
            let position = front.centre() + front.n * distance;
            if !position.cmpge(data.min + 1.0).all()
                || !position.cmple(data.max - 1.0).all()
                || data.bld_at(position).is_some()
                || !data.free(position, 0.6)
                || b.c.distance(position) >= 65.0
                || crate::map::exterior_blocks_camera(b, position)
            {
                continue;
            }
            let Some(zoom) = commercial_zoom(&bounds, position, size) else {
                continue;
            };
            candidates.push(CommercialFixture {
                building: b.id,
                position,
                zoom,
                scene,
            });
            break;
        }
    }
    candidates
        .into_iter()
        .max_by(|a, b| {
            a.zoom
                .total_cmp(&b.zoom)
                .then_with(|| {
                    b.position
                        .distance_squared(data.spawn)
                        .total_cmp(&a.position.distance_squared(data.spawn))
                })
                .then(b.building.cmp(&a.building))
        })
        .ok_or_else(|| {
            format!("No visible, collision-clear real-world {scene:?} commercial fixture")
        })
}

fn commercial_group(
    id: u32,
    base: Option<&crate::map::BldBase>,
    shell: Option<&crate::map::BldStructure>,
) -> Option<usize> {
    match (base, shell) {
        (Some(base), None) if base.0 == id => Some(0),
        (None, Some(shell)) if shell.0 == id => Some(1),
        _ => None,
    }
}

fn commercial_ready(world: &mut World, fixture: CommercialFixture) -> bool {
    let id = fixture.building;
    if !world
        .query::<&crate::map::DetailRoot>()
        .iter(world)
        .any(|root| root.0 == id)
        || !world
            .query::<&crate::map::BldWalls>()
            .iter(world)
            .any(|walls| walls.id == id && !walls.faded)
    {
        return false;
    }
    let Some(main_view) = main_camera_meshes(world) else {
        return false;
    };
    let mut visible = [0_usize; 2];
    let mut sign_visible = false;
    for (entity, base, shell, material, local, inherited, view) in world
        .query_filtered::<(
            Entity,
            Option<&crate::map::BldBase>,
            Option<&crate::map::BldStructure>,
            &crate::map::MapMeshMaterial,
            &Visibility,
            &InheritedVisibility,
            &ViewVisibility,
        ), Or<(With<crate::map::BldBase>, With<crate::map::BldStructure>)>>()
        .iter(world)
    {
        let Some(group) = commercial_group(id, base, shell) else {
            continue;
        };
        if *local == Visibility::Hidden || !inherited.get() {
            return false;
        }
        if view.get() && main_view.contains(&entity) {
            visible[group] += 1;
            sign_visible |= material.0 == crate::map::build::Mat::Sign;
        }
    }
    visible.into_iter().all(|count| count > 0) && sign_visible
}

/// Explain missing base/sign batches and visibility while asynchronous chunks
/// settle; a blocked fixture must not silently consume the full QA deadline.
fn commercial_wait_status(world: &mut World, fixture: CommercialFixture) -> String {
    let id = fixture.building;
    let main_view = main_camera_meshes(world).unwrap_or_default();
    let (mut total, mut hidden, mut inherited, mut view, mut main) = (
        [0_usize; 2],
        [0_usize; 2],
        [0_usize; 2],
        [0_usize; 2],
        [0_usize; 2],
    );
    let mut signs = Vec::new();
    for (entity, base, shell, material, local, inherited_visibility, view_visibility) in world
        .query_filtered::<(
            Entity,
            Option<&crate::map::BldBase>,
            Option<&crate::map::BldStructure>,
            &crate::map::MapMeshMaterial,
            &Visibility,
            &InheritedVisibility,
            &ViewVisibility,
        ), Or<(With<crate::map::BldBase>, With<crate::map::BldStructure>)>>()
        .iter(world)
    {
        let Some(group) = commercial_group(id, base, shell) else {
            continue;
        };
        total[group] += 1;
        hidden[group] += usize::from(*local == Visibility::Hidden);
        inherited[group] += usize::from(inherited_visibility.get());
        view[group] += usize::from(view_visibility.get());
        main[group] += usize::from(main_view.contains(&entity));
        if material.0 == crate::map::build::Mat::Sign {
            signs.push((group, view_visibility.get(), main_view.contains(&entity)));
        }
    }
    let walls: Vec<_> = world
        .query::<&crate::map::BldWalls>()
        .iter(world)
        .filter(|wall| wall.id == id)
        .map(|wall| wall.faded)
        .collect();
    let detail = world
        .query::<&crate::map::DetailRoot>()
        .iter(world)
        .any(|root| root.0 == id);
    let position = player_position(world).ok();
    let blocks = position.map(|p| {
        crate::map::exterior_blocks_camera(&crate::map::world().blds[id as usize], p.xz())
    });
    format!(
        "commercial lot={id} player={position:?} cutaway_front={blocks:?} walls_faded={walls:?} detail_root={detail} groups(base/shell)={total:?} hidden={hidden:?} inherited={inherited:?} view={view:?} main_view={main:?} sign(group/view/main)={signs:?}"
    )
}

fn commercial_evidence(
    world: &mut World,
    fixture: CommercialFixture,
    size: [u32; 2],
) -> Result<CommercialEvidence, String> {
    use crate::map::build::{Lod, Mat};

    let id = fixture.building;
    let data = crate::map::world();
    let b = &data.blds[id as usize];
    let layout = crate::map::commercial::plan(b, data)
        .ok_or_else(|| format!("Commercial {id}: plan disappeared"))?;
    let generated = crate::map::commercial::geometry(b, &layout, Lod::Near);
    let features = [generated.base, generated.shell];
    let commercial_vertices = features.iter().map(|parts| parts.vcount()).sum();
    if commercial_vertices < 40 || layout.panels.is_empty() {
        return Err(format!("Commercial {id}: empty promised storefront"));
    }
    // Recreate exactly the runtime material batching. Comparing the full batch
    // counts and bounds catches omitted/misrouted storefront uploads without
    // allocating separate per-shop materials or pretending an AABB is a draw.
    let mut all = crate::map::build::building(b, Lod::Near);
    all.structure = crate::map::exterior::structure(b, data, Lod::Near);
    all.merge_structure_into_roof();
    let expected = [all.base, all.roof];
    let main_view =
        main_camera_meshes(world).ok_or("Commercial camera has no visible mesh list")?;
    let mut groups = [ExteriorMeshGroup::default(); 2];
    let mut found = [HashSet::new(), HashSet::new()];
    let mut handles = Vec::new();
    for (entity, base, shell, material, mesh, uploaded, transform, local, inherited, view) in world
        .query_filtered::<(
            Entity,
            Option<&crate::map::BldBase>,
            Option<&crate::map::BldStructure>,
            &crate::map::MapMeshMaterial,
            &Mesh3d,
            &crate::map::MapMeshGeometry,
            &GlobalTransform,
            &Visibility,
            &InheritedVisibility,
            &ViewVisibility,
        ), Or<(With<crate::map::BldBase>, With<crate::map::BldStructure>)>>()
        .iter(world)
    {
        let Some(group) = commercial_group(id, base, shell) else {
            continue;
        };
        let key = material.0;
        let planned = expected[group]
            .0
            .get(&key)
            .ok_or_else(|| format!("Commercial {id}: unexpected {key:?} batch in group {group}"))?;
        let (min, max) = planned.aabb();
        if !found[group].insert(key)
            || uploaded.vertices != planned.vcount()
            || uploaded.min.distance(min) > 0.001
            || uploaded.max.distance(max) > 0.001
            || transform.translation().length() > 0.001
            || *local == Visibility::Hidden
            || !inherited.get()
        {
            return Err(format!(
                "Commercial {id}: actual {key:?} upload differs from generated batch"
            ));
        }
        let visible = view.get() && main_view.contains(&entity);
        if features[group].0.contains_key(&key) && !visible {
            return Err(format!(
                "Commercial {id}: {key:?} storefront material absent from main view"
            ));
        }
        groups[group].meshes += 1;
        groups[group].vertices += uploaded.vertices;
        groups[group].in_main_view += usize::from(visible);
        handles.push(mesh.0.clone());
    }
    for (group, (full, feature)) in expected.iter().zip(&features).enumerate() {
        let expected_materials: HashSet<_> = full
            .0
            .iter()
            .filter_map(|(material, mesh)| (!mesh.is_empty()).then_some(*material))
            .collect();
        if found[group] != expected_materials {
            return Err(format!(
                "Commercial {id}: incomplete uploaded group {group}"
            ));
        }
        for (material, mesh) in &feature.0 {
            let full = full.0.get(material).ok_or_else(|| {
                format!("Commercial {id}: {material:?} commercial part not integrated")
            })?;
            // Every feature vertex must survive the material merge. Checking this
            // against the runtime builder detects a valid but unused planner.
            let key = |p: [f32; 3]| p.map(|value| (value * 1000.0).round() as i32);
            let vertices: HashSet<_> = full.pos.iter().copied().map(key).collect();
            if mesh
                .pos
                .iter()
                .copied()
                .any(|point| !vertices.contains(&key(point)))
            {
                return Err(format!(
                    "Commercial {id}: feature geometry missing from {material:?} runtime batch"
                ));
            }
        }
    }
    if handles
        .iter()
        .any(|handle| !world.resource::<Assets<Mesh>>().contains(handle))
    {
        return Err(format!("Commercial {id}: referenced mesh asset is missing"));
    }
    if !features[1].0.contains_key(&Mat::Sign) {
        return Err(format!(
            "Commercial {id}: original typed sign texture is missing"
        ));
    }
    let position = player_position(world)?;
    if position.xz().distance(fixture.position) > 0.001
        || data.bld_at(position.xz()).is_some()
        || !data.free(position.xz(), 0.6)
        || crate::map::exterior_blocks_camera(b, position.xz())
    {
        return Err(format!(
            "Commercial {id}: storefront camera is inside/blocked or moved"
        ));
    }
    let (camera, transform) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<GameCamera>>()
        .iter(world)
        .next()
        .ok_or("Commercial camera is missing")?;
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for point in features
        .iter()
        .flat_map(|parts| parts.0.values())
        .flat_map(|mesh| mesh.pos.iter().copied().map(Vec3::from))
    {
        let point = camera
            .world_to_viewport(transform, point)
            .map_err(|err| format!("Commercial {id}: cannot project feature: {err:?}"))?;
        lo = lo.min(point);
        hi = hi.max(point);
    }
    if lo.x < size[0] as f32 * 0.08
        || lo.y < size[1] as f32 * 0.08
        || hi.x > size[0] as f32 * 0.92
        || hi.y > size[1] as f32 * 0.92
        || (hi - lo).min_element() < 35.0
    {
        return Err(format!(
            "Commercial {id}: cropped or unreadably small storefront {lo:?}..{hi:?}"
        ));
    }
    let camera_position = transform.translation().to_array();
    let atmosphere = world.resource::<crate::map::atmosphere::AtmosphereState>();
    let signs = &world.resource::<crate::map::textures::MapMaterials>().signs;
    let emissive = world
        .resource::<Assets<StandardMaterial>>()
        .get(signs)
        .ok_or("Commercial sign material is missing")?
        .emissive;
    let sign_emissive = [emissive.red, emissive.green, emissive.blue];
    let lighting_ok = match fixture.scene {
        CommercialScene::Night => {
            atmosphere.night > 0.9
                && atmosphere.power
                && atmosphere.active_lights > 0
                && atmosphere.active_lights <= 6
                && sign_emissive.iter().all(|value| *value > 0.8)
        }
        CommercialScene::Blackout => {
            atmosphere.night > 0.9
                && !atmosphere.power
                && atmosphere.active_lights == 0
                && sign_emissive.iter().all(|value| *value == 0.0)
        }
        _ => atmosphere.night < 0.1 && atmosphere.power,
    };
    if !lighting_ok {
        return Err(format!(
            "Commercial {id}: lighting/power state mismatch: {atmosphere:?} emissive={sign_emissive:?}"
        ));
    }
    Ok(CommercialEvidence {
        visibility_measurement: "uploaded_batches_and_main_view_membership_not_pixel_occlusion",
        building_id: id,
        family: format!("{:?}", layout.family),
        sign_index: layout.sign_index,
        scene: format!("{:?}", fixture.scene),
        arcade: b.arcade.is_some(),
        frontage_width: layout.frontage.width,
        building_height: b.h,
        storefront_panels: layout.panels.len(),
        secondary_fascia: layout.secondary_fascia,
        canopy: layout.canopy.is_some(),
        blade: layout.blade.is_some(),
        player: position.to_array(),
        camera: camera_position,
        zoom: world.resource::<CameraZoom>().0,
        projected_bounds: [lo.to_array(), hi.to_array()],
        commercial_vertices,
        base_batches: groups[0],
        persistent_shell_batches: groups[1],
        night: atmosphere.night,
        power: atmosphere.power,
        active_lights: atmosphere.active_lights,
        sign_emissive,
        streaming: world.resource::<crate::map::MapRenderStats>().clone(),
    })
}

fn check_continuation(world: &mut World, run: &mut QaRun) -> Result<(), String> {
    let checkpoint = run.checkpoint.as_ref().ok_or("Missing checkpoint")?;
    let position = player_position(world)?;
    let shelter = world.resource::<Shelter>();
    if shelter.day != checkpoint.day || shelter.stash != checkpoint.stash {
        return Err("Continue reset the shelter day or stockpile".into());
    }
    if world.resource::<Inventory>() != &checkpoint.inventory {
        return Err("Continue changed the carried inventory".into());
    }
    if world.resource::<SelectedRole>() != &checkpoint.role {
        return Err("Continue changed the selected survivor".into());
    }
    if position.distance(checkpoint.position) > 0.1 {
        return Err("Continue changed the saved player position".into());
    }
    if world.resource::<Campaign>().game_min < checkpoint.game_min {
        return Err("Continue reset the campaign clock".into());
    }
    let map = world.resource::<crate::worldmap::MapProgress>();
    if map.notes != checkpoint.map.notes || !map.revealed.is_superset(&checkpoint.map.revealed) {
        return Err("Continue lost survivor map notes or exploration".into());
    }
    if world
        .resource::<crate::worldplay::ScavengeState>()
        .closed_doors
        != checkpoint.closed_doors
    {
        return Err("Continue changed closed door state".into());
    }
    let progress = world.resource::<crate::worldplay::ScavengeState>();
    if progress.barricaded_doors != checkpoint.barricaded_doors
        || progress.loot != checkpoint.loot
        || progress.fixtures_version != checkpoint.fixtures_version
    {
        return Err(
            "Continue lost barricades, shelf leftovers or fixture migration version".into(),
        );
    }
    run.report
        .assertions
        .push("Continue preserved day, stockpile, inventory, survivor, position, clock, map notes/exploration, closed/barricaded doors and exact shelf leftovers".into());
    run.continuation_checked = true;
    Ok(())
}

fn finish(world: &mut World, run: &mut QaRun) {
    run.finished = true;
    run.report.success = run.report.error.is_none()
        && run.report.captures.len() == run.stages.len()
        && (matches!(run.report.scope, "exterior_subset" | "commercial_subset")
            || run.continuation_checked);
    let report_path = run.output.join("qa-report.json");
    let result = serde_json::to_vec_pretty(&run.report)
        .map_err(|err| err.to_string())
        .and_then(|bytes| fs::write(&report_path, bytes).map_err(|err| err.to_string()));
    if let Err(err) = result {
        run.report.success = false;
        eprintln!("DTQA could not write report: {err}");
    }
    println!(
        "DTQA result={} screenshots={} report={}",
        run.report.success,
        run.report.captures.len(),
        report_path.display()
    );
    if let Some(error) = &run.report.error {
        eprintln!("DTQA failure: {error}");
    }
    world.write_message(if run.report.success {
        AppExit::Success
    } else {
        AppExit::error()
    });
}

fn drive(world: &mut World) {
    let Some(mut run) = world.remove_resource::<QaRun>() else {
        return;
    };
    if run.finished {
        world.insert_resource(run);
        return;
    }
    if run.report.error.is_some() || run.stage >= run.stages.len() {
        finish(world, &mut run);
        world.insert_resource(run);
        return;
    }
    let result = step(world, &mut run);
    if let Err(error) = result {
        run.report.error = Some(error);
        finish(world, &mut run);
    }
    world.insert_resource(run);
}

fn step(world: &mut World, run: &mut QaRun) -> Result<(), String> {
    // Retain all original gameplay, performance and architecture captures, then
    // add commercial fronts with paired day/night/power-loss evidence.
    if run.started.elapsed() > Duration::from_secs(1500) {
        return Err("Visual QA exceeded its 25-minute deadline".into());
    }
    if run.report.renderer.is_empty() {
        fs::create_dir_all(&run.output).map_err(|err| err.to_string())?;
        run.report.renderer = world.get_resource::<RenderAdapterInfo>().map_or_else(
            || "Bevy offscreen renderer".into(),
            |adapter| {
                format!(
                    "{} ({:?}, {:?})",
                    adapter.name, adapter.backend, adapter.device_type
                )
            },
        );
    }
    if run.captured {
        run.captured = false;
        run.waiting_capture = false;
        run.entered = false;
        run.stage += 1;
        if run.stage == run.stages.len() {
            finish(world, run);
            return Ok(());
        }
    }
    let stage = &STAGES[run.stages[run.stage]];
    if !run.entered {
        enter_stage(world, run, stage)?;
    }
    run.frame += 1;
    if run.frame == SETTLE_FRAMES / 2 && matches!(stage.action, Action::PerformanceStreet) {
        let settings = world.resource::<crate::ui::settings::SettingsUi>();
        let preferences = world.resource::<crate::performance::PerformancePreferences>();
        if settings.open
            || settings.captured
            || !world.resource::<crate::ui::PauseOpen>().0
            || preferences.profile != crate::performance::PerformanceProfile::Balanced
            || preferences.frame_cap != 30
            || preferences.render_scale != 70
            || !preferences.overlay
            || world.resource::<Campaign>().day_hours != 1
        {
            return Err(
                "Performance settings close changed preferences, resumed pause or altered this run"
                    .into(),
            );
        }
        run.report.assertions.push("Closing performance settings preserved the selected profile, 30 FPS request, 70% 3D resolution and overlay, kept the run paused and did not change its day length".into());
        world.resource_mut::<crate::ui::PauseOpen>().0 = false;
    }
    if run.frame == SETTLE_FRAMES / 2 && matches!(stage.action, Action::Trade) {
        let settings = world.resource::<crate::ui::settings::SettingsUi>();
        if settings.open
            || settings.captured
            || !world.resource::<crate::ui::PauseOpen>().0
            || world
                .resource::<crate::settings::GamePreferences>()
                .day_hours
                != 1
            || *world.resource::<crate::performance::PerformancePreferences>()
                != crate::performance::PerformancePreferences::default()
            || *world.resource::<crate::audio::AudioSettings>()
                != crate::audio::AudioSettings::default()
            || *world.resource::<crate::i18n::Language>() != crate::i18n::Language::ZhTw
            || world.resource::<Campaign>().day_hours != 1
            || world
                .resource::<crate::settings::SettingsStatus>()
                .error
                .is_some()
        {
            return Err(
                "Paused settings did not reset all pages and release capture before trade".into(),
            );
        }
        run.report.assertions.push(
            "Paused settings reset performance, audio, next-run day length and language; closing released capture while preserving pause and the current run".into(),
        );
        world.resource_mut::<crate::ui::PauseOpen>().0 = false;
        world.write_message(OpenPanel(PanelKind::Trade));
    }
    if run.frame == SETTLE_FRAMES / 2
        && matches!(
            stage.action,
            Action::Rain
                | Action::Night
                | Action::Blackout
                | Action::Exterior(_)
                | Action::Commercial(_)
        )
    {
        // The explicit clock/weather fixture can dispatch several catch-up
        // broadcasts at once. Preserve their received-history flags, but clear
        // their overlays so the weather/lighting evidence shows the street.
        world.resource_mut::<Campaign>().notice_t = 0.0;
        world
            .resource_mut::<crate::worldplay::WorldNotice>()
            .text
            .clear();
    }
    if run.frame > MAX_STAGE_FRAMES {
        return Err(format!(
            "Stage {} did not complete: flow={:?}",
            stage.name,
            world.resource::<State<GameFlow>>().get()
        ));
    }
    if run.waiting_capture || run.frame < SETTLE_FRAMES || !flow_matches(world, stage.flow) {
        return Ok(());
    }
    if let Some(error) = &world.resource::<SaveStatus>().error {
        return Err(format!(
            "Save service failed during {}: {error}",
            stage.name
        ));
    }
    let font = world.resource::<UiFont>().0.clone();
    if world.resource::<Assets<Font>>().get(&font).is_none() {
        return Ok(());
    }
    if world
        .query_filtered::<Entity, With<crate::map::Chunk>>()
        .iter(world)
        .next()
        .is_none()
    {
        return Ok(());
    }
    if matches!(
        stage.action,
        Action::Street
            | Action::PerformanceStreet
            | Action::Entrance
            | Action::DoorOpen
            | Action::DoorClose
            | Action::Rain
            | Action::ShopInterior
            | Action::HomeInterior
            | Action::Exterior(_)
            | Action::Commercial(_)
    ) {
        let key = crate::map::ChunkKey::of(player_position(world)?.xz());
        let keys = [Some(key), run.scene_chunk];
        let chunks_ready = keys.into_iter().flatten().all(|key| {
            world
                .query::<&crate::map::Chunk>()
                .iter(world)
                .any(|chunk| chunk.key == key && chunk.lod == crate::map::Lod::Near)
        });
        let detail_ready = run.entrance_building.is_none_or(|id| {
            world
                .query::<&crate::map::BldWalls>()
                .iter(world)
                .any(|walls| walls.id == id && !walls.faded)
                && world
                    .query::<&crate::map::DetailRoot>()
                    .iter(world)
                    .any(|root| root.0 == id)
                && world
                    .query::<(&crate::map::BldDetail, &Visibility)>()
                    .iter(world)
                    .any(|(detail, visibility)| detail.0 == id && *visibility != Visibility::Hidden)
        });
        let interior_ready = run.interior_building.is_none_or(|id| {
            world
                .query::<&crate::map::interiors::InteriorView>()
                .iter(world)
                .any(|room| room.building == id)
        });
        let exterior_loaded = run
            .exterior
            .is_none_or(|fixture| exterior_ready(world, fixture));
        let commercial_loaded = run
            .commercial
            .is_none_or(|fixture| commercial_ready(world, fixture));
        if !chunks_ready
            || !detail_ready
            || !interior_ready
            || !exterior_loaded
            || !commercial_loaded
        {
            if run.wait_logged.elapsed() >= Duration::from_secs(5) {
                let exterior = run
                    .exterior
                    .map(|fixture| exterior_wait_status(world, fixture))
                    .unwrap_or_default();
                let commercial = run
                    .commercial
                    .map(|fixture| commercial_wait_status(world, fixture))
                    .unwrap_or_default();
                println!(
                    "DTQA waiting stage={} frame={} chunks={} detail={} interior={} exterior={} commercial={} {} {}",
                    stage.name,
                    run.frame,
                    chunks_ready,
                    detail_ready,
                    interior_ready,
                    exterior_loaded,
                    commercial_loaded,
                    exterior,
                    commercial
                );
                run.wait_logged = Instant::now();
            }
            run.ready_frames = 0;
            return Ok(());
        }
        // Chunk entities appear before their GPU upload. Leave several complete
        // render intervals after all target geometry and details become ready.
        run.ready_frames += 1;
        if run.ready_frames < 4 {
            return Ok(());
        }
    }
    if matches!(stage.action, Action::DoorOpen | Action::DoorClose) {
        let id = run.door_test_id.ok_or("Missing door interaction test")?;
        let open = matches!(stage.action, Action::DoorOpen);
        let door = crate::map::world().blds[id as usize].door.unwrap();
        let obstacles = world.resource::<crate::map::Obstacles>();
        if obstacles.doors().is_open(id) != open || obstacles.blocked(door.p, PLAYER_RADIUS) == open
        {
            return Err(format!(
                "Door {id} interaction did not synchronize visible state and collision (expected open={open})"
            ));
        }
        if !world
            .query::<&crate::doors::DoorLeaf>()
            .iter(world)
            .any(|leaf| leaf.id == id)
        {
            return Err("Door collider has no visible streamed leaf".into());
        }
        run.report.assertions.push(format!(
            "Real Interact set door {id} open={open}; collision and streamed leaf match"
        ));
    }
    if matches!(
        stage.action,
        Action::Barricade
            | Action::BarricadeBlocked
            | Action::Unbarricade
            | Action::BarricadeInventory
    ) {
        let id = run.door_test_id.ok_or("Missing barricade check")?;
        let expected = !matches!(stage.action, Action::Unbarricade);
        let obstacles = world.resource::<crate::map::Obstacles>();
        let door = crate::map::world().blds[id as usize].door.unwrap();
        let base = run.barricade_planks.ok_or("Missing plank baseline")?;
        if obstacles.doors().is_barricaded(id) != expected
            || obstacles.doors().is_open(id)
            || !obstacles.blocked(door.p, PLAYER_RADIUS)
            || world.resource::<Inventory>().count("plank") != base - u32::from(expected)
            || world.resource::<Inventory>().count("hammer") == 0
        {
            return Err(format!(
                "Barricade action failed collision or material conservation at {id}: expected barricaded={expected}"
            ));
        }
        run.report.assertions.push(format!("{}: door {id} barricaded={expected}, cannot open while boarded, plank count conserved and hammer retained", stage.name));
    }
    if matches!(
        stage.action,
        Action::Scavenge | Action::ScavengeOne | Action::ScavengeAll | Action::ShopDepleted
    ) {
        let id = run.loot_fixture.as_ref().ok_or("Missing shelf check")?;
        let stock = world
            .resource::<crate::worldplay::ScavengeState>()
            .loot
            .iter()
            .find(|(key, _)| key == id)
            .map(|(_, stock)| stock);
        if stock != run.loot_expected.as_ref()
            || Some(world.resource::<Inventory>()) != run.loot_inventory.as_ref()
        {
            return Err(format!(
                "Selective loot did not conserve exact shelf and bag contents during {}",
                stage.name
            ));
        }
        if !matches!(stage.action, Action::ShopDepleted) {
            let session = world.resource::<crate::worldplay::ScavengeSession>();
            if !session.reachable
                || session
                    .target
                    .as_ref()
                    .is_none_or(|target| &target.id != id)
                || !world.resource::<crate::ui::UiModal>().open
            {
                return Err("Selective loot panel is not attached to its reachable fixture".into());
            }
        }
        run.report.assertions.push(format!(
            "{}: exact shelf/bag conservation verified for {id}",
            stage.name
        ));
    }
    if matches!(stage.action, Action::Map) {
        if !world.resource::<crate::worldmap::MapOpen>().0
            || !world.resource::<crate::ui::UiModal>().open
            || world.resource::<crate::worldmap::MapProgress>().notes.len() != 2
        {
            return Err(
                "Survivor map did not open with input capture and two saved annotations".into(),
            );
        }
        run.report.assertions.push(
            "Survivor map opens with real exploration, two annotations and modal input capture"
                .into(),
        );
    }
    if matches!(
        stage.action,
        Action::SettingsAudio
            | Action::SettingsGame
            | Action::SettingsClose
            | Action::SettingsPause
    ) {
        use crate::ui::settings::{SettingsPage, SettingsUi};
        let settings = world.resource::<SettingsUi>();
        let preferences = world.resource::<crate::settings::GamePreferences>();
        let audio = world.resource::<crate::audio::AudioSettings>();
        let correct = match stage.action {
            Action::SettingsAudio => {
                settings.open
                    && settings.page == SettingsPage::Audio
                    && (audio.master - 0.65).abs() < 0.001
                    && (audio.effects - 0.6).abs() < 0.001
                    && (audio.ambience - 0.5).abs() < 0.001
                    && (audio.radio - 0.4).abs() < 0.001
            }
            Action::SettingsGame => {
                settings.open
                    && settings.page == SettingsPage::Game
                    && preferences.day_hours == 2
                    && *world.resource::<crate::i18n::Language>() == crate::i18n::Language::En
            }
            Action::SettingsClose => {
                !settings.open
                    && !settings.captured
                    && preferences.day_hours == 1
                    && *audio == crate::audio::AudioSettings::default()
                    && *world.resource::<crate::performance::PerformancePreferences>()
                        == crate::performance::PerformancePreferences::default()
                    && *world.resource::<crate::i18n::Language>() == crate::i18n::Language::ZhTw
            }
            Action::SettingsPause => {
                settings.open
                    && preferences.day_hours == 4
                    && world.resource::<Campaign>().day_hours == 1
                    && world.resource::<crate::ui::PauseOpen>().0
            }
            _ => false,
        };
        if !correct
            || world
                .resource::<crate::settings::SettingsStatus>()
                .error
                .is_some()
        {
            return Err(format!(
                "Settings actions, persistence or current-run isolation failed in {}",
                stage.name
            ));
        }
        run.report.assertions.push(format!("{}: settings actions, independent preferences, modal state and current-run day length verified", stage.name));
    }
    if matches!(
        stage.action,
        Action::SettingsPerformance
            | Action::SettingsPerformanceLandscape
            | Action::SettingsDiagnostics
            | Action::PerformanceStreet
    ) {
        use crate::performance::{
            EffectivePerformance, PerformancePreferences, PerformanceProfile,
        };
        use crate::ui::settings::{PerformanceSection, SettingsPage, SettingsUi};
        let preferences = world.resource::<PerformancePreferences>();
        let effective = world.resource::<EffectivePerformance>();
        let dimensions = world.resource::<crate::performance::render::RenderDimensions>();
        let settings = world.resource::<SettingsUi>();
        let diagnostics = matches!(stage.action, Action::SettingsDiagnostics);
        let closed = matches!(stage.action, Action::PerformanceStreet);
        if preferences.profile != PerformanceProfile::Balanced
            || preferences.frame_cap != 30
            || preferences.render_scale != 70
            || effective.render_scale != 70
            || effective.target_fps != 30
            || dimensions.ui != stage.size
            || dimensions.scale != 70
            || (!closed && (!settings.open || settings.page != SettingsPage::Performance))
            || (!closed
                && settings.performance_section
                    != if diagnostics {
                        PerformanceSection::Diagnostics
                    } else {
                        PerformanceSection::Display
                    })
            || (closed
                && (settings.open
                    || settings.captured
                    || world.resource::<crate::ui::PauseOpen>().0))
        {
            return Err(format!(
                "Performance settings, render policy or modal state mismatch in {}",
                stage.name
            ));
        }
        let scene = dimensions.scene;
        for (actual, full) in scene.into_iter().zip(stage.size) {
            let expected = full as f32 * 0.7;
            if (actual as f32 - expected).abs() > 2.0 || actual >= full {
                return Err(format!(
                    "{}: 70% scene extent {scene:?} does not match native UI {:?}",
                    stage.name, stage.size
                ));
            }
        }
        let target = world
            .query_filtered::<&RenderTarget, With<GameCamera>>()
            .iter(world)
            .next()
            .ok_or("Game camera has no render target")?;
        let RenderTarget::Image(target) = target else {
            return Err("Game camera bypassed the scaled scene image".into());
        };
        let image = world
            .resource::<Assets<Image>>()
            .get(&target.handle)
            .ok_or("Scaled scene image is missing")?;
        if image.width() != scene[0] || image.height() != scene[1] {
            return Err("Reported scene extent does not match actual render texture".into());
        }
        if world
            .resource::<crate::settings::SettingsStatus>()
            .error
            .is_some()
        {
            return Err("Performance settings could not be saved".into());
        }
        run.report.assertions.push(format!("{}: requested 30 FPS and 70% scene image {}x{} with independent native {}x{} UI, actual render texture and settings state verified; fixed-timestep visual QA is not a device benchmark", stage.name, scene[0], scene[1], stage.size[0], stage.size[1]));
    }
    if matches!(stage.action, Action::Trade) {
        let settings = world.resource::<crate::ui::settings::SettingsUi>();
        if world.resource::<crate::ui::PauseOpen>().0
            || settings.open
            || settings.captured
            || !world.resource::<crate::ui::UiModal>().open
        {
            return Err("Deferred trade panel did not open after closing paused settings".into());
        }
        run.report.assertions.push(
            "Deferred trade panel acquired modal input after settings closed and pause was released"
                .into(),
        );
    }
    if matches!(
        stage.action,
        Action::Rain | Action::Night | Action::Blackout
    ) {
        let atmosphere = world.resource::<crate::map::atmosphere::AtmosphereState>();
        let ok = match stage.action {
            Action::Rain => {
                atmosphere.rain_streaks > 0
                    && atmosphere.rain_streaks <= 240
                    && atmosphere.wetness > 0.5
            }
            Action::Night => {
                atmosphere.night > 0.9
                    && atmosphere.power
                    && atmosphere.active_lights > 0
                    && atmosphere.active_lights <= 6
            }
            Action::Blackout => {
                atmosphere.night > 0.9 && !atmosphere.power && atmosphere.active_lights == 0
            }
            _ => unreachable!(),
        };
        if !ok {
            return Err(format!(
                "Atmosphere mismatch for {}: {atmosphere:?}",
                stage.name
            ));
        }
        run.report.assertions.push(format!(
            "{}: night={:.2}, power={}, wetness={:.2}, lights={}, rain={}",
            stage.name,
            atmosphere.night,
            atmosphere.power,
            atmosphere.wetness,
            atmosphere.active_lights,
            atmosphere.rain_streaks
        ));
    }
    if matches!(stage.action, Action::Continue) && !run.continuation_checked {
        check_continuation(world, run)?;
    }
    // State alone is insufficient: a repeatedly rebuilt subtree can have
    // open=true while its extracted UI is blank. Check real layout/visibility,
    // then also verify opaque panel and selected-control pixels after readback.
    let settings_pixels = settings_pixels(world, stage.size)?;
    let exterior = run
        .exterior
        .map(|fixture| exterior_evidence(world, fixture, stage.size))
        .transpose()?;
    let commercial = run
        .commercial
        .map(|fixture| commercial_evidence(world, fixture, stage.size))
        .transpose()?;
    if let Some(evidence) = &commercial {
        if matches!(
            stage.action,
            Action::Commercial(CommercialScene::Night | CommercialScene::Blackout)
        ) {
            let day = run
                .report
                .captures
                .iter()
                .find_map(|capture| {
                    capture
                        .commercial
                        .as_ref()
                        .filter(|value| value.scene == "Food")
                })
                .ok_or("Commercial night comparison is missing its daylight capture")?;
            if evidence.building_id != day.building_id
                || evidence.player != day.player
                || evidence.camera != day.camera
                || evidence.zoom != day.zoom
                || evidence.projected_bounds != day.projected_bounds
            {
                return Err(
                    "Commercial day/night/blackout camera or feature bounds drifted".into(),
                );
            }
        }
        run.report.assertions.push(format!(
            "{}: real lot {} {} sign atlas cell {}, {} solid-flank panels, canopy={}, blade={}, {} commercial vertices within verified uploaded base/shell batches; night={:.2} power={} sign_emissive={:?}; actual main-camera visibility and storefront framing checked",
            stage.name, evidence.building_id, evidence.family, evidence.sign_index,
            evidence.storefront_panels, evidence.canopy, evidence.blade, evidence.commercial_vertices,
            evidence.night, evidence.power, evidence.sign_emissive
        ));
    }
    let map_workload = matches!(stage.flow, ExpectedFlow::Playing)
        .then(|| map_workload(world))
        .flatten();
    if let Some(evidence) = &exterior {
        run.report.assertions.push(format!(
            "{}: actual lot {} {}, persistent shell {} vertices, facade detail {} vertices, roof fittings {} vertices across {} unique material meshes; cutaway_hidden={}, full_exterior_framing={}; merged shell includes coarse facade and roof; real GPU readback, workload counts are not a device benchmark",
            stage.name, evidence.building_id, evidence.typology,
            evidence.persistent_shell.vertices, evidence.facade_detail.vertices, evidence.roof_fittings.vertices,
            evidence.total_meshes, evidence.cutaway_hidden, !evidence.cutaway_hidden
        ));
    }
    let image = run.target.clone().ok_or("No render target")?;
    world
        .spawn((
            Screenshot::image(image),
            CaptureTicket {
                name: stage.name.into(),
                path: run.output.join(format!("{}.png", stage.name)),
                size: stage.size,
                settings_pixels,
                exterior,
                commercial,
                map_workload,
            },
        ))
        .observe(capture_finished);
    run.waiting_capture = true;
    Ok(())
}

fn settings_pixels(world: &mut World, size: [u32; 2]) -> Result<Option<SettingsPixels>, String> {
    use crate::ui::settings::{PerformanceSection, SettingsPage, SettingsRoot, SettingsUi};
    let settings = world.resource::<SettingsUi>();
    if !settings.open {
        return Ok(None);
    }
    let page = settings.page;
    let section = settings.performance_section;
    let profile = world
        .resource::<crate::performance::PerformancePreferences>()
        .profile;
    let presentation = world
        .query_filtered::<Entity, With<crate::performance::render::PresentationCamera>>()
        .single(world)
        .map_err(|_| "Missing unique UI presentation camera")?;
    let mut roots = world.query_filtered::<(
        &InheritedVisibility,
        &ComputedNode,
        &ComputedUiTargetCamera,
        &Children,
    ), With<SettingsRoot>>();
    let (visibility, node, target, children) = roots
        .single(world)
        .map_err(|_| "Open settings did not have exactly one rendered root")?;
    if !visibility.get()
        || node
            .size()
            .distance(Vec2::new(size[0] as f32, size[1] as f32))
            > 1.0
        || target.get() != Some(presentation)
        || children.len() < 6
    {
        return Err(format!(
            "Open settings root was not visible, laid out at full viewport, or attached to presentation camera: visible={} size={:?} children={}",
            visibility.get(),
            node.size(),
            children.len()
        ));
    }
    let layout = crate::ui::settings::settings_layout(Vec2::new(size[0] as f32, size[1] as f32));
    let selected = match page {
        SettingsPage::Audio => layout.tabs[0],
        SettingsPage::Game => layout.tabs[1],
        SettingsPage::Performance if section == PerformanceSection::Diagnostics => {
            layout.performance_sections[1]
        }
        SettingsPage::Performance => {
            layout.profiles[match profile {
                crate::performance::PerformanceProfile::Auto => 0,
                crate::performance::PerformanceProfile::Battery => 1,
                crate::performance::PerformanceProfile::Balanced => 2,
                crate::performance::PerformanceProfile::Quality => 3,
            }]
        }
    };
    let mut panel = Vec::with_capacity(6);
    let mut selected_border = Vec::with_capacity(6);
    for fraction in [0.25, 0.5, 0.75] {
        let y = (layout.frame.min.y + layout.frame.height() * fraction).round() as u32;
        panel.push([(layout.frame.min.x + 4.0).round() as u32, y]);
        panel.push([(layout.frame.max.x - 4.0).round() as u32, y]);
        let x = (selected.min.x + selected.width() * fraction).round() as u32;
        selected_border.push([x, selected.min.y.ceil() as u32]);
        selected_border.push([x, selected.max.y.ceil() as u32 - 1]);
    }
    Ok(Some(SettingsPixels {
        panel,
        selected_border,
    }))
}

fn capture_finished(
    event: On<ScreenshotCaptured>,
    tickets: Query<&CaptureTicket>,
    mut run: ResMut<QaRun>,
) {
    let Ok(ticket) = tickets.get(event.entity) else {
        return;
    };
    let result = (|| -> Result<Capture, String> {
        let image = event
            .image
            .clone()
            .try_into_dynamic()
            .map_err(|err| err.to_string())?
            .to_rgba8();
        if image.width() != ticket.size[0] || image.height() != ticket.size[1] {
            return Err(format!("{} has unexpected dimensions", ticket.name));
        }
        if let Some(probes) = &ticket.settings_pixels {
            let matches = |point: [u32; 2], expected: [u8; 3], tolerance: u8| {
                let pixel = image.get_pixel(point[0], point[1]).0;
                (0..3).all(|channel| pixel[channel].abs_diff(expected[channel]) <= tolerance)
            };
            // These clear interior margins and straight selected-button edges
            // distinguish a real settings overlay from its underlying title.
            // Merely counting colors accepted the wrong screen in this bug.
            if !probes
                .panel
                .iter()
                .all(|point| matches(*point, [15, 19, 18], 2))
                || !probes
                    .selected_border
                    .iter()
                    .all(|point| matches(*point, [209, 173, 102], 8))
            {
                return Err(format!(
                    "{}: captured pixels do not contain the opaque settings panel and selected control border",
                    ticket.name
                ));
            }
        }
        let colors: HashSet<_> = image
            .pixels()
            .step_by(37)
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect();
        if colors.len() < 8 {
            return Err(format!(
                "{} appears blank ({} sampled colors)",
                ticket.name,
                colors.len()
            ));
        }
        image.save(&ticket.path).map_err(|err| err.to_string())?;
        Ok(Capture {
            stage: ticket.name.clone(),
            file: ticket.path.display().to_string(),
            width: image.width(),
            height: image.height(),
            sampled_colors: colors.len(),
            exterior: ticket.exterior.clone(),
            commercial: ticket.commercial.clone(),
            map_workload: ticket.map_workload.clone(),
        })
    })();
    match result {
        Ok(capture) => {
            println!(
                "DTQA captured {} {}x{} colors={}",
                capture.stage, capture.width, capture.height, capture.sampled_colors
            );
            run.report.captures.push(capture);
            run.captured = true;
        }
        Err(error) => run.report.error = Some(error),
    }
}

#[cfg(test)]
mod exterior_tests {
    use super::*;

    #[test]
    fn commercial_views_use_real_features_clear_positions_and_identical_lighting_cameras() {
        use crate::map::commercial::{ShopFamily, plan};

        let data = crate::map::world();
        let size = [1280, 720];
        let food = commercial_fixture(data, CommercialScene::Food, size).unwrap();
        for scene in [
            CommercialScene::Arcade,
            CommercialScene::Food,
            CommercialScene::Essentials,
            CommercialScene::Night,
            CommercialScene::Blackout,
        ] {
            let fixture = commercial_fixture(data, scene, size).unwrap();
            let building = &data.blds[fixture.building as usize];
            let layout = plan(building, data).unwrap();
            assert!(data.bld_at(fixture.position).is_none());
            assert!(data.free(fixture.position, 0.6));
            assert!(!crate::map::exterior_blocks_camera(
                building,
                fixture.position
            ));
            assert!(crate::camera::ZOOM_RANGE.contains(&fixture.zoom));
            assert!(building.c.distance(fixture.position) < 65.0);
            assert!(!layout.panels.is_empty());
            assert_eq!(layout.sign_index, building.sign.unwrap().idx);
            match scene {
                CommercialScene::Arcade => {
                    assert!(building.arcade.is_some());
                    assert!(layout.panels.len() >= 2);
                    assert!(layout.secondary_fascia);
                    assert!((8.0..=16.0).contains(&layout.frontage.width));
                    assert!(building.h <= 12.0);
                }
                CommercialScene::Essentials => {
                    assert!(matches!(
                        layout.family,
                        ShopFamily::Pharmacy | ShopFamily::Convenience
                    ));
                }
                _ => {
                    assert_eq!(layout.family, ShopFamily::Food);
                    assert!(building.arcade.is_none());
                    assert!(layout.canopy.is_some());
                    assert!(layout.blade.is_some());
                    assert!(layout.frontage.u.dot(Vec2::ONE.normalize()).abs() >= 0.35);
                    assert_eq!(fixture.building, food.building);
                    assert_eq!(fixture.position, food.position);
                    assert_eq!(fixture.zoom, food.zoom);
                }
            }
            eprintln!(
                "Commercial fixture {scene:?}: lot={} family={:?} panels={} canopy={} blade={} position={:?} zoom={:.3} frontage={:.2} height={:.2}",
                building.id,
                layout.family,
                layout.panels.len(),
                layout.canopy.is_some(),
                layout.blade.is_some(),
                fixture.position,
                fixture.zoom,
                layout.frontage.width,
                building.h
            );
        }
    }

    #[test]
    fn exterior_views_select_real_reachable_lots_with_the_promised_features() {
        use crate::map::exterior::{Typology, classify, plan};

        let data = crate::map::world();
        let cases = [
            (ExteriorScene::Townhouse, Some(Typology::Townhouse)),
            (ExteriorScene::Walkup, Some(Typology::Walkup)),
            (ExteriorScene::Shophouse, Some(Typology::Shophouse)),
            (ExteriorScene::ServiceStair, Some(Typology::Walkup)),
            (ExteriorScene::Rooftop, None),
            (ExteriorScene::Cutaway, Some(Typology::Walkup)),
        ];
        for (scene, expected) in cases {
            let size = if scene == ExteriorScene::Cutaway {
                [390, 844]
            } else {
                [1280, 720]
            };
            let fixture = exterior_fixture(data, scene, size).unwrap();
            let b = &data.blds[fixture.building as usize];
            let layout = plan(b, data);
            if let Some(expected) = expected {
                assert_eq!(classify(b), expected);
            }
            assert!(
                data.free(fixture.position, 0.6),
                "{scene:?}: player intersects collision"
            );
            assert!(crate::camera::ZOOM_RANGE.contains(&fixture.zoom));
            if scene != ExteriorScene::Cutaway {
                assert!(
                    !crate::map::exterior_blocks_camera(b, fixture.position),
                    "{scene:?}: an outdoor fixture must not hide its own subject"
                );
            }
            assert!(
                b.c.distance(fixture.position) < 65.0,
                "target is outside near-detail radius"
            );
            match scene {
                ExteriorScene::Townhouse | ExteriorScene::Walkup | ExteriorScene::Shophouse => {
                    assert!(
                        !layout.balconies.is_empty(),
                        "{scene:?}: no actual balcony layout"
                    );
                    assert!(data.bld_at(fixture.position).is_none());
                }
                ExteriorScene::ServiceStair => assert!(layout.stair.is_some()),
                ExteriorScene::Rooftop => {
                    let roof = crate::map::roofs::layout(b);
                    assert!(!roof.shacks.is_empty());
                    assert!(roof.access.is_some());
                }
                ExteriorScene::Cutaway => {
                    assert_eq!(
                        data.bld_at(fixture.position).map(|inside| inside.id),
                        Some(b.id)
                    );
                }
            }
            eprintln!(
                "Exterior fixture {scene:?}: lot={} position={:?} zoom={:.3}",
                b.id, fixture.position, fixture.zoom
            );
        }
    }
}
