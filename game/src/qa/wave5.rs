//! Explicit controlled transport scenarios, driven through production messages,
//! input adapters, rendering and save services. Never enabled in the Android app.
use crate::{
    combat::armor::{ArmorFleet, ArmorQaFreeze},
    input::{ControlAction, ControlPreferences},
    vehicles::{VehicleAction, VehicleFleet, VehicleRuntime},
    worldplay::WorldAction,
};
use bevy::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scene {
    Controls,
    Inspect,
    Board,
    Drive,
    Service,
    Rider,
    Armor,
    Capture,
    Mounted,
}

#[derive(Deserialize)]
struct Fixture {
    player: [f32; 3],
    fleet: VehicleFleet,
    armor: ArmorFleet,
    vehicle_id: u32,
    drive_screen: [f32; 2],
}

fn fixture(armored: bool) -> Fixture {
    let value: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/android-vehicles-smoke.json"
    ))
    .unwrap();
    serde_json::from_value(value[if armored { "armored" } else { "civilian" }].clone()).unwrap()
}

#[derive(Resource, Default)]
pub(super) struct Progress {
    pub vehicle: u32,
    start: Vec2,
    fuel: f32,
    drive_screen: Vec2,
    pub captured: Option<u32>,
    reload_started: bool,
    switched: bool,
}

fn release(world: &mut World) {
    world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
    world.resource_mut::<ButtonInput<MouseButton>>().reset_all();
}

fn install(world: &mut World, armored: bool) -> Result<Vec2, String> {
    let fixture = fixture(armored);
    let v = fixture
        .fleet
        .get(fixture.vehicle_id)
        .ok_or("Vehicle fixture missing")?;
    let progress = Progress {
        vehicle: fixture.vehicle_id,
        start: v.position(),
        fuel: v.fuel,
        drive_screen: Vec2::from_array(fixture.drive_screen),
        captured: None,
        reload_started: false,
        switched: false,
    };
    fixture
        .fleet
        .publish_colliders(world.resource::<crate::map::Obstacles>());
    world
        .resource::<crate::map::Obstacles>()
        .replace_dynamic(2, vec![]);
    world.insert_resource(fixture.fleet);
    world.insert_resource(fixture.armor);
    world.insert_resource(VehicleRuntime::default());
    world.insert_resource(progress);
    let p = Vec3::from_array(fixture.player).xz();
    super::move_view(world, p, if armored { 1.0 } else { 1.7 })?;
    Ok(p)
}

pub(super) fn enter(world: &mut World, scene: Scene) -> Result<Vec2, String> {
    release(world);
    world.write_message(crate::ui::OpenPanel(crate::ui::PanelKind::Close));
    world.resource_mut::<crate::ui::PauseOpen>().0 = false;
    world.resource_mut::<crate::game_flow::Campaign>().sleeping = false;
    world.resource_mut::<crate::game_flow::Campaign>().speed = 1;
    world.resource_mut::<crate::game_flow::Campaign>().game_min = 720.0;
    world.resource_mut::<crate::game_flow::Campaign>().notice_t = 0.0;
    *world.resource_mut::<crate::worldplay::PlayerNeeds>() = default();
    world.resource_mut::<crate::combat::GameState>().hp = 100.0;
    world.resource_mut::<crate::combat::GameState>().bleed = false;
    world.resource_mut::<crate::combat::GameState>().over = false;
    world.resource_mut::<ArmorQaFreeze>().0 = true;
    let enemies: Vec<_> = world
        .query_filtered::<Entity, With<crate::combat::Enemy>>()
        .iter(world)
        .collect();
    for enemy in enemies {
        world.despawn(enemy);
    }
    {
        let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
        progress.weather.next = 5000.0;
        progress.weather.until = 0.0;
        progress.weather.amount = 0.0;
        progress.weather.intensity = 0.0;
        progress.weather.wet = 0.0;
    }
    use crate::ui::settings::{ControlsSection, SettingsAction as A, SettingsPage};
    match scene {
        Scene::Controls => {
            world.resource_mut::<crate::ui::PauseOpen>().0 = true;
            world.write_message(A::Open);
            world.write_message(A::Page(SettingsPage::Controls));
            world.write_message(A::ControlsSection(ControlsSection::Touch));
            world.write_message(A::ButtonSize(110));
        }
        Scene::Inspect => {
            world.write_message(A::Close);
            install(world, false)?;
        }
        Scene::Board => {
            world.write_message(WorldAction::Interact);
        }
        Scene::Drive => {
            let p = world
                .resource::<VehicleFleet>()
                .current()
                .ok_or("Boarding failed before drive")?
                .position();
            super::move_view(world, p, 1.65)?;
        }
        Scene::Service => {
            world.write_message(VehicleAction::Exit);
        }
        Scene::Rider => {
            install(world, false)?;
            let mut fleet = world.resource_mut::<VehicleFleet>();
            let v = fleet.get_mut(50_000).unwrap();
            v.kind = crate::vehicles::VehicleKind::Delivery;
            v.fuel = 3.0;
            world.write_message(VehicleAction::Enter(50_000));
        }
        Scene::Armor => {
            install(world, true)?;
            world.write_message(WorldAction::Interact);
        }
        Scene::Capture => {}
        Scene::Mounted => {
            let id = world
                .resource::<ArmorFleet>()
                .units
                .iter()
                .find_map(|a| a.captured_id)
                .ok_or("Armour crew did not abandon damaged vehicle")?;
            world.resource_mut::<Progress>().captured = Some(id);
            world.write_message(VehicleAction::ReloadGun);
        }
    }
    Ok(super::player_position(world)?.xz())
}

pub(super) fn frame(world: &mut World, scene: Scene, frame: u32) -> Result<(), String> {
    world.resource_mut::<crate::game_flow::Campaign>().notice_t = 0.0;
    match scene {
        Scene::Controls if frame == 5 => {
            use crate::ui::settings::{ControlsSection, SettingsAction as A, SettingsPage};
            world.write_message(A::Open);
            world.write_message(A::Page(SettingsPage::Controls));
            world.write_message(A::ControlsSection(ControlsSection::Touch));
            world.write_message(A::ButtonSize(110));
        }
        Scene::Drive => {
            if frame <= 40 {
                let p = world.resource::<Progress>().drive_screen;
                let action = if p.x.abs() > p.y.abs() {
                    if p.x > 0.0 {
                        ControlAction::Right
                    } else {
                        ControlAction::Left
                    }
                } else if p.y > 0.0 {
                    ControlAction::Up
                } else {
                    ControlAction::Down
                };
                let key = world.resource::<ControlPreferences>().key(action);
                world.resource_mut::<ButtonInput<KeyCode>>().press(key);
            } else if frame == 41 {
                release(world);
                world.write_message(VehicleAction::Brake);
            }
        }
        Scene::Service if frame == 5 => {
            if world.resource::<VehicleFleet>().current.is_some() {
                return Err("Vehicle exit failed on clear original road".into());
            }
            let id = world.resource::<Progress>().vehicle;
            {
                let mut fleet = world.resource_mut::<VehicleFleet>();
                let v = fleet.get_mut(id).unwrap();
                v.tyre = 0.3;
                v.battery = 0.1;
                v.fuel = 5.0;
            }
            let mut bag = world.resource_mut::<crate::inventory::Inventory>();
            bag.add("patch", 1);
            bag.add("carbat", 1);
            bag.add("gas", 2);
            bag.add("hose", 1);
            world.write_message(crate::ui::OpenPanel(crate::ui::PanelKind::Inventory));
        }
        Scene::Service if frame == 10 => {
            for item in ["patch", "carbat", "gas"] {
                world.write_message(WorldAction::Use(item.into()));
            }
        }
        Scene::Service if frame == 15 => {
            world.write_message(crate::ui::OpenPanel(crate::ui::PanelKind::Close));
        }
        Scene::Capture if frame == 10 => {
            let point = world.resource::<ArmorFleet>().units[0].position();
            let screen = {
                let (camera, transform) = world
                    .query_filtered::<(&Camera, &GlobalTransform), With<crate::camera::GameCamera>>(
                    )
                    .single(world)
                    .map_err(|e| e.to_string())?;
                camera
                    .world_to_viewport(transform, Vec3::new(point.x, 0.0, point.y))
                    .map_err(|e| e.to_string())?
            };
            world
                .query_filtered::<&mut Window, With<bevy::window::PrimaryWindow>>()
                .single_mut(world)
                .map_err(|e| e.to_string())?
                .set_cursor_position(Some(screen));
            world
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
        }
        Scene::Capture if frame == 12 => release(world),
        Scene::Mounted if frame == 3 => {
            let original = fixture(true);
            let original_gun = &original
                .fleet
                .get(50_000)
                .unwrap()
                .gun
                .as_ref()
                .unwrap()
                .guns[0];
            let expected_rounds = original_gun.ready + original_gun.reserve - 1;
            let gun = world
                .resource::<VehicleFleet>()
                .get(50_000)
                .unwrap()
                .gun
                .as_ref()
                .unwrap();
            // The source returns ready ammunition to reserve at reload start.
            // It fills the magazine only after the complete reload duration.
            if gun.selected != 0
                || gun.guns[0].ready != 0
                || gun.guns[0].reserve != expected_rounds
                || gun.guns[0].reload_remaining <= 0.0
            {
                return Err(
                    "Mounted reload action did not start a real reload of the fired 105 mm gun"
                        .into(),
                );
            }
            world.resource_mut::<Progress>().reload_started = true;
            world.write_message(VehicleAction::SwitchGun);
        }
        Scene::Mounted if frame == 5 => {
            if world
                .resource::<VehicleFleet>()
                .current()
                .unwrap()
                .gun
                .as_ref()
                .unwrap()
                .selected
                != 1
            {
                return Err("Mounted gun switch did not apply before exit".into());
            }
            world.resource_mut::<Progress>().switched = true;
            world.write_message(VehicleAction::Exit);
        }
        Scene::Mounted if frame == 8 => {
            if world.resource::<VehicleFleet>().current.is_some() {
                return Err(
                    "Could not exit the original armour before boarding the captured vehicle"
                        .into(),
                );
            }
            let id = world.resource::<Progress>().captured.unwrap();
            let vehicle = world.resource::<VehicleFleet>().get(id).unwrap();
            let approach =
                crate::vehicles::exit_position(vehicle, world.resource::<crate::map::Obstacles>())
                    .ok_or("Captured vehicle has no collision-safe approach")?;
            super::move_view(world, approach, 1.2)?;
            world.resource_mut::<Progress>().vehicle = id;
            world.write_message(VehicleAction::Enter(id));
        }
        Scene::Mounted if frame == 11 => {
            if world.resource::<VehicleFleet>().current != world.resource::<Progress>().captured {
                return Err(
                    "Captured vehicle could not be boarded through the production action".into(),
                );
            }
            world.write_message(VehicleAction::SwitchGun);
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn evidence(world: &mut World, scene: Scene) -> Result<Value, String> {
    if scene == Scene::Controls {
        let state = world.resource::<crate::ui::settings::SettingsUi>();
        if !state.open
            || state.page != crate::ui::settings::SettingsPage::Controls
            || world.resource::<ControlPreferences>().button_percent != 110
        {
            return Err("Controls settings failed to open/apply button size".into());
        }
        return Ok(
            json!({"scene":"Controls","button_percent":110,"vibration":world.resource::<ControlPreferences>().vibration,"bindings":world.resource::<ControlPreferences>().bindings,"measurement":"actual native settings UI and preferences; screenshot is GPU readback"}),
        );
    }
    let id = world.resource::<Progress>().vehicle;
    let fleet = world.resource::<VehicleFleet>();
    let v = fleet.get(id).ok_or("QA vehicle disappeared")?.clone();
    let current = fleet.current;
    let runtime = world.resource::<VehicleRuntime>();
    let distance = runtime.distance_driven;
    let consumed = runtime.fuel_used;
    match scene {
        Scene::Inspect if current.is_some() => {
            return Err("Inspection unexpectedly boarded the vehicle".into());
        }
        Scene::Board
        | Scene::Drive
        | Scene::Rider
        | Scene::Armor
        | Scene::Capture
        | Scene::Mounted
            if current != Some(id) =>
        {
            return Err(format!("{scene:?}: current vehicle {current:?} != {id}"));
        }
        Scene::Service
            if current.is_some()
                || v.tyre != 1.0
                || v.battery != 1.0
                || (v.fuel - 7.0).abs() > 0.001 =>
        {
            return Err(format!(
                "Inventory maintenance failed: tire={} battery={} fuel={} current={current:?}",
                v.tyre, v.battery, v.fuel
            ));
        }
        _ => {}
    }
    if scene == Scene::Drive
        && (distance < 0.2
            || consumed <= 0.0
            || v.position().distance(world.resource::<Progress>().start) < 0.2
            || v.fuel >= world.resource::<Progress>().fuel
            || v.speed != 0.0)
    {
        return Err(format!(
            "Real drive/brake/fuel failed: distance={distance} consumed={consumed} speed={}",
            v.speed
        ));
    }
    if scene == Scene::Capture {
        let armor = world.resource::<ArmorFleet>();
        let unit = &armor.units[0];
        if !unit.abandoned
            || unit.hp != 30.0
            || unit.captured_id.is_none()
            || v.gun.as_ref().unwrap().guns[0].ready != 7
        {
            return Err(format!(
                "Mounted cannon did not produce 100→30 HP capture with one round: hp={} abandoned={} capture={:?} ready={}",
                unit.hp,
                unit.abandoned,
                unit.captured_id,
                v.gun.as_ref().unwrap().guns[0].ready
            ));
        }
    }
    if scene == Scene::Mounted {
        let progress = world.resource::<Progress>();
        if v.gun.as_ref().unwrap().selected != 1
            || current != progress.captured
            || !progress.reload_started
            || !progress.switched
            || v.kind != crate::vehicles::VehicleKind::Zbl08
            || v.engine != 0.4
        {
            return Err(
                "Captured vehicle boarding, source reload start or mounted switching did not apply"
                    .into(),
            );
        }
    }
    let visible = super::main_camera_meshes(world).ok_or("No main camera visibility list")?;
    let model = world
        .query::<(
            Entity,
            &crate::vehicles::visual::VehicleVisual,
            &crate::vehicles::visual::VehicleGeometry,
            &GlobalTransform,
        )>()
        .iter(world)
        .find(|(_, tag, _, _)| tag.0 == id)
        .map(|(entity, _, geometry, transform)| (entity, *geometry, *transform))
        .ok_or("Vehicle model was not uploaded")?;
    if !visible.contains(&model.0) || model.1.vertices < 100 {
        return Err(format!(
            "{scene:?}: selected vehicle body is not in main-camera render list"
        ));
    }
    let model_position = model.2.translation();
    if model_position.xz().distance(v.position()) > 0.01 {
        return Err("Rendered vehicle did not follow its saved simulation position".into());
    }
    Ok(
        json!({"scene":format!("{scene:?}"),"measurement":"production vehicle body uploaded and present in main-camera visibility list; actual actions/keyboard/mouse pipeline; controlled state on original road", "vehicle":v,"current":current,"distance_driven":distance,"fuel_used":consumed,"body_vertices":model.1.vertices,"body_indices":model.1.indices,"body_in_main_view":true,"rendered_position":model_position.to_array(),"player":super::player_position(world)?.to_array(),"armor":world.resource::<ArmorFleet>(),"notice":world.resource::<crate::worldplay::WorldNotice>().text,"captured_vehicle_id":world.resource::<Progress>().captured,"captured_vehicle_boarded":world.resource::<Progress>().captured.is_some_and(|id| current==Some(id)),"source_gun_reload_started":world.resource::<Progress>().reload_started,"source_gun_switch_verified":world.resource::<Progress>().switched}),
    )
}
