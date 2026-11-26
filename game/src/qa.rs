//! Desktop-only end-to-end visual QA through the real Bevy renderer.
//!
//! `--qa` uses an offscreen texture, with no OS window or display-server socket.
//! All game plugins still run. The virtual primary window provides the same
//! viewport inputs as a real window. Screenshots come from GPU readback, and a
//! save/menu/continue cycle must preserve the live run before QA can succeed.

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
    /// v24.3 a ROC remnant position in the street.
    Remnant,
    /// The squad leader's conversation card.
    RemnantTalk,
    /// v24.14 an area taken back by actual kills.
    Reclaim,
    /// v24.4 a C-2 airdrop called on the JSDF radio, after it lands.
    Airdrop,
    Shelter,
    Inventory,
    Trade,
    SaveMenu,
    Continue,
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
        name: "04k-remnant-position-landscape",
        size: [1280, 720],
        action: Action::Remnant,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04l-remnant-talk-portrait",
        size: [390, 844],
        action: Action::RemnantTalk,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04m-reclaim-title-landscape",
        size: [1280, 720],
        action: Action::Reclaim,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04n-jsdf-airdrop-landscape",
        size: [1280, 720],
        action: Action::Airdrop,
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
}

#[derive(Serialize)]
struct QaReport {
    success: bool,
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
    target: Option<Handle<Image>>,
    stage: usize,
    entered: bool,
    frame: u32,
    ready_frames: u32,
    scene_chunk: Option<crate::map::ChunkKey>,
    entrance_building: Option<u32>,
    interior_building: Option<u32>,
    door_test_id: Option<u32>,
    barricade_planks: Option<u32>,
    loot_fixture: Option<String>,
    loot_expected: Option<Stash>,
    loot_inventory: Option<Inventory>,
    /// The area the Reclaim stage takes back, and the bag before its gift.
    reclaim: Option<(usize, Inventory)>,
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
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .insert_resource(QaRun {
            output,
            target: None,
            stage: 0,
            entered: false,
            frame: 0,
            ready_frames: 0,
            scene_chunk: None,
            entrance_building: None,
            interior_building: None,
            door_test_id: None,
            barricade_planks: None,
            loot_fixture: None,
            loot_expected: None,
            loot_inventory: None,
            reclaim: None,
            waiting_capture: false,
            captured: false,
            checkpoint: None,
            continuation_checked: false,
            started: Instant::now(),
            report: QaReport {
                success: false,
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
    run.scene_chunk = None;
    run.entrance_building = None;
    run.interior_building = None;
    println!("DTQA entering {}", stage.name);
    if run.stage > 0 && matches!(STAGES[run.stage - 1].action, Action::PerformanceStreet) {
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
        Action::Remnant => {
            world.write_message(OpenPanel(PanelKind::Close));
            // daylight on day 2; the needs fixture of the warnings stage is lifted
            world.resource_mut::<Campaign>().game_min = 1440.0 + 600.0;
            world.resource_mut::<Campaign>().notice_t = 0.0;
            world
                .resource_mut::<crate::worldplay::WorldNotice>()
                .text
                .clear();
            *world.resource_mut::<crate::worldplay::PlayerNeeds>() = default();
            world.resource_mut::<crate::combat::GameState>().bleed = false;
            let site = *crate::map::world()
                .remnants
                .first()
                .ok_or("No ROC remnant position")?;
            move_view(world, site.p - Vec2::from_angle(site.ang) * 0.9, 1.6)?;
        }
        Action::RemnantTalk => {
            world.write_message(crate::dialogue::TalkTo(crate::dialogue::NpcKey::Remnant(0)));
        }
        Action::Reclaim => {
            world.resource_mut::<crate::dialogue::Dialogue>().0 = None;
            world.write_message(OpenPanel(PanelKind::Close));
            // The core area with the fewest live soldiers, taken back by real kill messages.
            let roster = world.resource::<crate::sectors::PatrolRoster>().clone();
            let dead = world
                .resource::<crate::worldplay::ScavengeState>()
                .dead
                .clone();
            let counts = crate::sectors::count_sectors(&roster, &dead);
            let target = (0..crate::sectors::SECTORS.len())
                .filter(|&i| !crate::sectors::outer_sector(i) && counts[i].alive > 0)
                .min_by_key(|&i| counts[i].alive)
                .ok_or("No occupied core area")?;
            run.reclaim = Some((target, world.resource::<Inventory>().clone()));
            let ids: Vec<u32> = roster
                .0
                .iter()
                .filter(|(_, at)| crate::sectors::sector_of(*at) == target)
                .map(|(id, _)| *id)
                .collect();
            let mut killed = Vec::new();
            let mut query = world.query::<(Entity, &mut crate::combat::Enemy, &Transform)>();
            for (entity, mut enemy, tf) in query.iter_mut(world) {
                if ids.contains(&enemy.0.id) && enemy.0.alive() {
                    enemy.0.hp = 0.0;
                    enemy.0.state = crate::combat::SolState::Dead;
                    killed.push((entity, enemy.0.id, enemy.0.kind, tf.translation));
                }
            }
            for (entity, id, kind, pos) in killed {
                world
                    .entity_mut(entity)
                    .insert(crate::combat::Body::new(kind));
                world.write_message(crate::combat::EnemyDied { id, kind, pos });
            }
            let p = player_position(world)?.xz();
            move_view(world, p, 1.0)?;
        }
        Action::Airdrop => {
            // The liaison's first meeting hands over the radio and two batteries.
            world.write_message(crate::dialogue::TalkTo(crate::dialogue::NpcKey::Liaison));
            world.write_message(crate::japan::CallSupport(crate::japan::JpKind::Drop));
            let p = player_position(world)?.xz();
            move_view(world, p, 0.8)?;
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
        && run.report.captures.len() == STAGES.len()
        && run.continuation_checked;
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
    if run.report.error.is_some() || run.stage >= STAGES.len() {
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
    // Fifty-one captures include performance controls and scaled 3D rendering, with room for
    // software Vulkan runner variance without removing any existing stage.
    // CI keeps the 25-minute bound; a slower local software renderer can extend it.
    let deadline = std::env::var("DEFEND_TAIPEI_QA_DEADLINE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1500);
    if run.started.elapsed() > Duration::from_secs(deadline) {
        return Err(format!("Visual QA exceeded its {deadline}-second deadline"));
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
        if run.stage == STAGES.len() {
            finish(world, run);
            return Ok(());
        }
    }
    let stage = &STAGES[run.stage];
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
            Action::Rain | Action::Night | Action::Blackout
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
            | Action::Remnant
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
        if !chunks_ready || !detail_ready || !interior_ready {
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
    if matches!(stage.action, Action::Remnant) {
        let built = world
            .query::<(
                &crate::remnant::RemnantMan,
                &crate::human::RigParts,
                &Visibility,
            )>()
            .iter(world)
            .filter(|(man, _, vis)| man.squad == 0 && **vis != Visibility::Hidden)
            .count();
        let p = player_position(world)?.xz();
        let reach = world
            .resource::<crate::dialogue::NpcSpots>()
            .nearest(p, |_| true)
            .map(|s| s.key);
        if built < 4 || reach != Some(crate::dialogue::NpcKey::Remnant(0)) {
            run.ready_frames = 0;
            return Ok(());
        }
        let hint = world
            .resource::<crate::worldplay::InteractionHint>()
            .detail
            .clone();
        run.report.assertions.push(format!(
            "ROC remnant position 0: four rigged soldiers visible, sandbags in the world's cover, Interact reaches the leader ({hint})"
        ));
    }
    if matches!(stage.action, Action::RemnantTalk) {
        let Some(card) = world.resource::<crate::dialogue::Dialogue>().0.clone() else {
            return Ok(());
        };
        let keys: Vec<_> = card.options.iter().map(|o| o.key).collect();
        if !keys.contains(&crate::dialogue::DialogueKey::RemnantSupply(0))
            || !keys.contains(&crate::dialogue::DialogueKey::RemnantIntel(0))
            || card.body.is_empty()
        {
            return Err(format!("Remnant card lacks its options: {keys:?}"));
        }
        run.report.assertions.push(format!(
            "Talking to {} opened the leader's card with {} options",
            card.title,
            card.options.len()
        ));
    }
    if matches!(stage.action, Action::Reclaim) {
        let (target, before) = run.reclaim.clone().ok_or("Missing reclaim fixture")?;
        let name = crate::sectors::SECTORS[target].zh;
        let taken = world
            .resource::<Campaign>()
            .reclaimed
            .iter()
            .any(|n| n == name);
        let title = world.resource::<crate::sectors::ReclaimTitle>().clone();
        if !taken || title.age.is_none() {
            return Ok(());
        }
        let bag = world.resource::<Inventory>();
        let gift = crate::sectors::reclaim_gift(target);
        if gift
            .iter()
            .any(|(key, n)| bag.count(key) != before.count(key) + n)
        {
            return Err(format!(
                "Reclaiming {name} did not hand over its gift exactly"
            ));
        }
        run.report.assertions.push(format!(
            "Killing every soldier assigned to {name} reclaimed it: title '{}', gift {gift:?} added to the bag",
            title.title
        ));
    }
    if matches!(stage.action, Action::Airdrop) {
        // close the liaison's card once it has been shown
        world.resource_mut::<crate::dialogue::Dialogue>().0 = None;
        if world.resource::<crate::japan::JapanStats>().landed == 0 {
            run.ready_frames = 0;
            return Ok(());
        }
        // let the crate marker and the red smoke render before the capture
        run.ready_frames += 1;
        if run.ready_frames < 45 {
            return Ok(());
        }
        let progress = world.resource::<crate::worldplay::ScavengeState>();
        let Some((id, at)) = progress
            .drops
            .iter()
            .find(|(id, _)| id.starts_with("jpdrop"))
            .cloned()
        else {
            return Err("Airdrop landed without a crate".into());
        };
        let stock = progress
            .loot
            .iter()
            .find(|(key, _)| *key == id)
            .map(|(_, s)| s.clone())
            .ok_or("Airdrop crate has no stock")?;
        if stock.count("type20") + stock.count("type89") != 1 || stock.count("a556n") != 60 {
            return Err(format!(
                "Airdrop crate contents are wrong: {:?}",
                stock.stacks()
            ));
        }
        let state = world.resource::<crate::japan::JapanState>();
        if !state.met || state.ready(crate::japan::JpKind::Drop) {
            return Err("Airdrop did not spend its sortie".into());
        }
        let p = player_position(world)?.xz();
        run.report.assertions.push(format!(
            "JSDF radio: the C-2 crate landed {:.0} m from the player with {:?}; the call spent a battery and started the 6-minute cooldown",
            Vec2::from_array(at).distance(p),
            stock.stacks()
        ));
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
    let image = run.target.clone().ok_or("No render target")?;
    world
        .spawn((
            Screenshot::image(image),
            CaptureTicket {
                name: stage.name.into(),
                path: run.output.join(format!("{}.png", stage.name)),
                size: stage.size,
                settings_pixels,
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
