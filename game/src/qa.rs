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
    camera::{RenderTarget, ShadowLodOrigin},
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
    ChooseSurvivor,
    ReviewDeparture,
    Depart,
    Skip,
    Street,
    Entrance,
    DoorOpen,
    DoorClose,
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
        name: "02a-title-small-portrait",
        size: [320, 640],
        action: Action::None,
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
    door_test_id: Option<u32>,
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
            door_test_id: None,
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
    let camera = world
        .query_filtered::<Entity, With<GameCamera>>()
        .iter(world)
        .next()
        .ok_or("Game camera did not spawn")?;
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
        ShadowLodOrigin,
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

fn enter_stage(world: &mut World, run: &mut QaRun, stage: &Stage) -> Result<(), String> {
    set_target(world, run, stage.size)?;
    run.ready_frames = 0;
    run.scene_chunk = None;
    run.entrance_building = None;
    println!("DTQA entering {}", stage.name);
    match stage.action {
        Action::None => {}
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
            world.resource_mut::<crate::ui::PauseOpen>().0 = false;
            // Known authored barter location. This fixture avoids turning a UI
            // regression run into several minutes of walking across the map.
            let pos = TRADE_SPOTS[0].pos;
            let mut query = world.query_filtered::<&mut Transform, With<Player>>();
            let mut player = query.iter_mut(world).next().ok_or("Player did not spawn")?;
            player.translation.x = pos[0];
            player.translation.z = pos[1];
            world.write_message(OpenPanel(PanelKind::Trade));
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
    run.report
        .assertions
        .push("Continue preserved day, stockpile, inventory, survivor, position, clock, map notes/exploration and closed doors".into());
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
    if run.started.elapsed() > Duration::from_secs(900) {
        return Err("Visual QA exceeded its 15-minute deadline".into());
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
        Action::Street | Action::Entrance | Action::DoorOpen | Action::DoorClose | Action::Rain
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
        if !chunks_ready || !detail_ready {
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
    let image = run.target.clone().ok_or("No render target")?;
    world
        .spawn((
            Screenshot::image(image),
            CaptureTicket {
                name: stage.name.into(),
                path: run.output.join(format!("{}.png", stage.name)),
                size: stage.size,
            },
        ))
        .observe(capture_finished);
    run.waiting_capture = true;
    Ok(())
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
