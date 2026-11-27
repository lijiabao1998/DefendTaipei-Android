use super::*;
use crate::i18n::Language;
use crate::inventory::Inventory;

#[test]
fn all_ground_specs_and_motion_match_executed_pinned_typescript() {
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/vehicle-reference.json")).unwrap();
    assert_eq!(
        reference["source"],
        "lijiabao1998/defendtaipei@b9b0a2736642b37dd45925a4719dfef08b8f36b4"
    );
    for (name, expected) in reference["spec"].as_object().unwrap() {
        let kind: VehicleKind =
            serde_json::from_value(serde_json::Value::String(name.clone())).unwrap();
        let spec = kind.spec();
        assert_eq!(spec.name_en, expected["name"]);
        for (key, actual) in [
            ("L", spec.length),
            ("W", spec.width),
            ("vmax", spec.vmax),
            ("acc", spec.acceleration),
            ("turn", spec.turn),
            ("noise", spec.noise),
            ("fuelCap", spec.fuel_capacity),
            ("burn", spec.burn),
            ("cargo", spec.cargo),
            ("seatY", spec.seat_y),
        ] {
            let wanted = expected[key].as_f64().unwrap() as f32;
            assert!(
                (actual - wanted).abs() < 1e-6,
                "{name} {key}: {actual} != {wanted}"
            );
        }
        for (key, actual) in [
            ("drive", spec.drive),
            ("two", spec.two),
            ("elec", spec.electric),
            ("pedal", spec.pedal),
            ("key", spec.key),
            ("armored", spec.armored),
        ] {
            assert_eq!(
                actual,
                expected[key].as_bool().unwrap_or(false),
                "{name} {key}"
            );
        }
    }
    let mut snapshots = 0;
    for scenario in reference["cases"].as_array().unwrap() {
        let kind: VehicleKind = serde_json::from_value(scenario["kind"].clone()).unwrap();
        let mut v = vehicle(kind);
        let initial = &scenario["initial"];
        v.fuel = initial["fuel"].as_f64().unwrap() as f32;
        v.engine = initial["eng"].as_f64().unwrap() as f32;
        v.tyre = initial["tyre"].as_f64().unwrap() as f32;
        v.heading = initial["hd"].as_f64().unwrap() as f32;
        v.speed = initial["v"].as_f64().unwrap() as f32;
        let mut frame = 0;
        for step in scenario["steps"].as_array().unwrap() {
            for _ in 0..step["repeat"].as_u64().unwrap() {
                let want = Vec2::new(
                    step["want"][0].as_f64().unwrap() as f32,
                    step["want"][1].as_f64().unwrap() as f32,
                );
                drive_step(
                    &mut v,
                    want,
                    step["dt"].as_f64().unwrap() as f32,
                    scenario["ride"].as_f64().unwrap() as f32,
                    |_| scenario["blocked"].as_bool().unwrap_or(false),
                );
                let expected = &scenario["snapshots"][frame];
                for (key, actual) in [
                    ("heading", v.heading),
                    ("speed", v.speed),
                    ("fuel", v.fuel),
                    ("engine", v.engine),
                    ("tyre", v.tyre),
                ] {
                    let wanted = expected[key].as_f64().unwrap() as f32;
                    assert!(
                        (actual - wanted).abs() < 0.0002,
                        "{} frame{frame} {key}: {actual} != {wanted}",
                        scenario["name"]
                    );
                }
                for i in 0..2 {
                    assert!(
                        (v.pos[i] - expected["pos"][i].as_f64().unwrap() as f32).abs() < 0.0002
                    );
                }
                frame += 1;
                snapshots += 1;
            }
        }
    }
    assert_eq!(snapshots, 141);
}

#[test]
fn live_hulls_share_collision_visibility_cover_and_safe_exit_without_self_blocking() {
    let obstacles = crate::map::Obstacles::new(crate::map::world().clone());
    let p = crate::map::world()
        .roads
        .iter()
        .filter(|r| r.w >= 12.0)
        .flat_map(|r| r.p.windows(2))
        .map(|ab| (ab[0] + ab[1]) * 0.5)
        .find(|&p| !obstacles.blocked(p, 6.0))
        .expect("An open main-road fixture");
    let mut v = vehicle(VehicleKind::Sedan);
    v.pos = p.into();
    let before = obstacles.clone();
    obstacles.replace_dynamic(1, vec![v.collider()]);
    assert!(before.blocked(p, 0.4));
    assert!(!before.blocked_except(p, 0.4, Some(v.id as u64)));
    assert!(!before.los(p - Vec2::X * 4.0, p + Vec2::X * 4.0));
    assert!(
        before
            .segment_hit(p - Vec2::X * 4.0, p + Vec2::X * 4.0, 2.0, 2.0)
            .is_none()
    );
    assert!(before.segment_hit(p, p + Vec2::X * 4.0, 1.0, 1.0).is_none());
    assert_eq!(before.shot_ray(p, p + Vec2::X * 4.0, 1.0, 1.0, 0.0), 1.0);
    assert_eq!(before.shot_ray(p - Vec2::X * 4.0, p, 1.0, 1.0, 0.0), 1.0);
    assert!(before.shot_ray(p - Vec2::X * 4.0, p + Vec2::X * 4.0, 1.0, 1.0, 0.0) < 1.0);
    assert!(before.find_cover(p + Vec2::X * 1.0, 0.4).is_some());
    let walked = before.move_circle(p - Vec2::X * 4.0, p + Vec2::X * 4.0, 0.45);
    assert!(walked.x < p.x);
    let exit = exit_position(&v, &before).expect("A safe side exit");
    assert!(!before.blocked(exit, 0.45));
    let shifted = crate::map::collide::DynamicCollider {
        id: 0x1_0000_0000,
        corners: footprint(p + Vec2::Y * 5.0, 0.0, 2.0, 2.0),
        height: 2.5,
    };
    obstacles.replace_dynamic(2, vec![shifted]);
    obstacles.replace_dynamic(1, vec![]);
    assert!(!before.blocked(p, 0.4));
    assert!(before.blocked(p + Vec2::Y * 5.0, 0.4));
}
use bevy::prelude::*;

fn vehicle(kind: VehicleKind) -> VehicleState {
    VehicleState::new(9, kind, Vec2::ZERO, 0.0, &mut || 0.5)
}

#[test]
fn planned_fleet_is_stable_valid_and_does_not_change_static_world() {
    let world = crate::map::world();
    let before = (world.props.len(), world.segs.len(), world.blds.len());
    let a = VehicleFleet::planned(world);
    let b = VehicleFleet::planned(world);
    assert_eq!(
        serde_json::to_vec(&a).unwrap(),
        serde_json::to_vec(&b).unwrap()
    );
    assert!(a.validate());
    assert!(a.vehicles.len() > 20);
    assert_eq!(a.vehicles.iter().filter(|v| v.spec().armored).count(), 6);
    assert_eq!(
        before,
        (world.props.len(), world.segs.len(), world.blds.len())
    );
}

#[test]
fn steering_acceleration_reverse_and_no_energy_coast_follow_drive_rules() {
    let mut v = vehicle(VehicleKind::Scooter);
    v.engine = 1.0;
    v.tyre = 1.0;
    drive_step(&mut v, Vec2::Y, 0.1, 0.0, |_| false);
    assert!((v.speed - 0.6).abs() < 1e-6);
    assert!((v.pos[1] - 0.06).abs() < 1e-6);
    v.speed = 0.0;
    drive_step(&mut v, Vec2::NEG_Y, 0.1, 0.0, |_| false);
    assert!(v.speed < 0.0);
    assert!(v.heading.abs() < 1e-5);
    v.fuel = 0.0;
    let speed = v.speed.abs();
    drive_step(&mut v, Vec2::Y, 0.1, 0.0, |_| false);
    assert!(v.speed.abs() < speed);
}

#[test]
fn high_speed_collision_damages_vehicle_and_does_not_cross_hull() {
    let mut v = vehicle(VehicleKind::Sedan);
    v.engine = 1.0;
    v.tyre = 1.0;
    v.speed = 12.0;
    let hit = drive_step(&mut v, Vec2::Y, 0.05, 1.0, |_| true);
    assert!(hit > 6.0);
    assert_eq!(v.pos, [0.0, 0.0]);
    assert!(v.engine < 1.0 && v.tyre < 1.0);
    assert!(v.speed < 0.0);
}

#[test]
fn refuel_and_siphon_conserve_whole_litres_and_capacity() {
    let mut v = vehicle(VehicleKind::Scooter);
    v.fuel = 1.4;
    let mut bag = Inventory::default();
    bag.add("gas", 10);
    bag.add("hose", 1);
    let total = v.fuel + bag.count("gas") as f32;
    assert!(refuel(&mut v, &mut bag, Language::En).changed);
    assert_eq!(v.fuel, 4.4);
    assert!((v.fuel + bag.count("gas") as f32 - total).abs() < 1e-5);
    assert!(!refuel(&mut v, &mut bag, Language::En).changed);
    assert!(v.fuel <= 5.0);
    assert!(siphon(&mut v, &mut bag, Language::En).changed);
    assert!((v.fuel + bag.count("gas") as f32 - total).abs() < 1e-5);
}

#[test]
fn explicit_patch_does_not_spend_battery_or_gas() {
    let mut v = vehicle(VehicleKind::Sedan);
    v.tyre = 0.3;
    v.battery = 0.1;
    let mut bag = Inventory::default();
    bag.add("patch", 2);
    bag.add("carbat", 1);
    bag.add("gas", 1);
    let skills = crate::roles::role_by_id(Some("tech")).skills;
    assert!(use_item(&mut v, &mut bag, "patch", skills, Language::En).changed);
    assert_eq!(v.tyre, 1.0);
    assert_eq!(v.battery, 0.1);
    assert_eq!(bag.count("carbat"), 1);
    assert_eq!(bag.count("gas"), 1);
    assert!(!use_item(&mut v, &mut bag, "patch", skills, Language::En).changed);
    assert_eq!(bag.count("patch"), 1);
}

#[test]
fn locked_vehicle_requires_completed_wiring_and_bikes_require_tool() {
    let mut v = vehicle(VehicleKind::Scooter);
    v.key = false;
    let mut fleet = VehicleFleet {
        vehicles: vec![v],
        current: None,
    };
    let mut runtime = VehicleRuntime::default();
    let mut bag = Inventory::default();
    let ordinary = crate::roles::role_by_id(None).skills;
    let tech = crate::roles::role_by_id(Some("tech")).skills;
    assert!(!enter(&mut fleet, &mut runtime, 9, &bag, ordinary, Language::En).changed);
    assert!(!enter(&mut fleet, &mut runtime, 9, &bag, tech, Language::En).changed);
    assert_eq!(runtime.hotwire.unwrap().remaining, 2.5);
    assert!(!enter(&mut fleet, &mut runtime, 9, &bag, tech, Language::En).changed);
    runtime.hotwire.as_mut().unwrap().remaining = 0.0;
    assert!(enter(&mut fleet, &mut runtime, 9, &bag, tech, Language::En).changed);
    assert_eq!(fleet.current, Some(9));
    let mut bike = vehicle(VehicleKind::Ubike);
    bike.docked = true;
    fleet.vehicles = vec![bike];
    fleet.current = None;
    assert!(!enter(&mut fleet, &mut runtime, 9, &bag, ordinary, Language::En).changed);
    bag.add("pipe", 1);
    let result = enter(&mut fleet, &mut runtime, 9, &bag, ordinary, Language::En);
    assert!(result.changed);
    assert_eq!(result.noise, 26.0);
    assert_eq!(bag.count("pipe"), 1);
}

#[test]
fn persistence_rejects_duplicates_invalid_occupancy_and_nonfinite_values() {
    let mut fleet = VehicleFleet::default();
    assert!(fleet.validate());
    let json = serde_json::to_vec(&fleet).unwrap();
    let loaded: VehicleFleet = serde_json::from_slice(&json).unwrap();
    assert!(loaded.validate());
    fleet.vehicles.push(fleet.vehicles[0].clone());
    assert!(!fleet.validate());
    fleet.vehicles.pop();
    fleet.current = Some(u32::MAX);
    assert!(!fleet.validate());
    fleet.current = None;
    fleet.vehicles[0].heading = f32::NAN;
    assert!(!fleet.validate());
}
