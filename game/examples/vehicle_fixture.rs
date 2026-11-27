//! Reproducible, explicitly seeded scenarios for native-renderer and disposable
//! Android touch QA. Uses the real map, vehicle planner and collision provider.
use bevy::prelude::*;
use defend_taipei::{
    combat::armor::{ArmorFleet, ArmorState},
    map,
    vehicles::{VehicleFleet, VehicleKind, VehicleState},
};
use serde_json::json;

fn scenario(armored: bool) -> serde_json::Value {
    let world = map::world();
    let obstacles = map::Obstacles::new(world.clone());
    let mut fleet = VehicleFleet::default();
    fleet.publish_colliders(&obstacles);
    let kind = if armored {
        VehicleKind::Ztd05
    } else {
        VehicleKind::Kei
    };
    let mut selected = None;
    'roads: for road in &world.roads {
        if road.w < if armored { 12.0 } else { 9.0 } {
            continue;
        }
        for segment in road.p.windows(2) {
            let length = segment[0].distance(segment[1]);
            if length < if armored { 50.0 } else { 28.0 } {
                continue;
            }
            let direction = (segment[1] - segment[0]).normalize();
            let side = Vec2::new(direction.y, -direction.x);
            for fraction in [0.3, 0.5, 0.65] {
                let p = segment[0].lerp(segment[1], fraction);
                let heading = direction.x.atan2(direction.y);
                let mut vehicle = VehicleState::new(50_000, kind, p, heading, &mut || 0.5);
                vehicle.key = true;
                vehicle.fuel = if armored { 120.0 } else { 10.0 };
                vehicle.engine = 1.0;
                vehicle.tyre = 1.0;
                vehicle.battery = 1.0;
                // The real hull protects a waiting on-foot player from the
                // active patrol during a slow Android startup. Its rear exit
                // remains inside the original 2.6 m body-adjusted reach.
                let approach = if armored {
                    p - direction * (vehicle.spec().length * 0.5 + 0.6)
                } else {
                    p + side * (vehicle.spec().width * 0.5 + 0.8)
                };
                let target = p + direction * 24.0;
                if obstacles.blocked(approach, 0.45)
                    || [0.0, 2.0, 5.0, 8.0, 12.0].iter().any(|distance| {
                        obstacles.footprint_blocked(
                            vehicle.corners_at(p + direction * *distance, heading),
                            None,
                        )
                    })
                {
                    continue;
                }
                if armored {
                    let target_vehicle =
                        VehicleState::new(50_001, VehicleKind::Zbl08, target, heading, &mut || 0.5);
                    let muzzle = p + direction * (vehicle.spec().length * 0.5 + 0.15);
                    // Validate the entire live patrol, not only its spawn.
                    // The emulator may need many real seconds to board/fire.
                    if (0..=40).any(|step| {
                        let destination = target + direction * (step as f32 * 0.5);
                        obstacles.footprint_blocked(
                            target_vehicle.corners_at(destination, heading),
                            None,
                        ) || obstacles
                            .segment_hit_except(muzzle, destination, 1.9, 1.2, None)
                            .is_some()
                    }) {
                        continue;
                    }
                    obstacles.replace_dynamic(9, vec![vehicle.collider()]);
                    let protected_approach = (0..=40).all(|step| {
                        let destination = target + direction * (step as f32 * 0.5);
                        obstacles.shot_ray(destination, approach, 2.8, 1.2, 4.0) < 1.0
                    });
                    let reachable = obstacles
                        .segment_hit_except(approach, p, 1.0, 1.0, Some(vehicle.id as u64))
                        .is_none();
                    obstacles.replace_dynamic(9, vec![]);
                    if !protected_approach || !reachable {
                        continue;
                    }
                }
                // Do not select a point whose normal isometric view is buried
                // under a tall roof immediately toward the camera.
                if world
                    .blds
                    .iter()
                    .any(|b| b.h > 12.0 && b.c.distance(p + Vec2::splat(8.0)) < 9.0)
                {
                    continue;
                }
                selected = Some((vehicle, approach, direction, target));
                break 'roads;
            }
        }
    }
    let (vehicle, approach, direction, target) = selected.expect("real clear road fixture");
    let id = vehicle.id;
    fleet.vehicles.push(vehicle);
    let armor = ArmorFleet {
        units: if armored {
            vec![ArmorState::new(
                50_000,
                VehicleKind::Zbl08,
                vec![target, target + direction * 20.0],
                0,
            )]
        } else {
            vec![]
        },
    };
    assert!(fleet.validate());
    assert!(armor.validate());
    let right = Vec2::new(1.0, -1.0).normalize();
    let up = Vec2::new(-1.0, -1.0).normalize();
    json!({"player":[approach.x,0.0,approach.y],"fleet":fleet,"armor":armor,"vehicle_id":id,"target_id":armored.then_some(50_000),"drive_screen":[direction.dot(right),direction.dot(up)],"heading":direction.x.atan2(direction.y),"source":"controlled QA state on an unobstructed original road; production vehicle models, input, collision and save systems"})
}

fn main() {
    let fixture = json!({"version":1,"source_commit":"b9b0a2736642b37dd45925a4719dfef08b8f36b4","civilian":scenario(false),"armored":scenario(true)});
    let path = "game/tests/fixtures/android-vehicles-smoke.json";
    std::fs::write(path, serde_json::to_string_pretty(&fixture).unwrap() + "\n").unwrap();
    println!(
        "Vehicle QA fixtures written: civilian={:?}, armored={:?}",
        fixture["civilian"]["player"], fixture["armored"]["player"]
    );
}
