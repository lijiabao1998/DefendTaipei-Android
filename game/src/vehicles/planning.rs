//! Native deterministic placement of the separate drive.ts fleet. The native
//! road/building stream is already intentionally independent of Three.js;
//! scenery from vehicles.ts remains untouched, including every static prop ID.
use bevy::prelude::*;
use std::sync::Arc;

use super::{VehicleFleet, VehicleKind, VehicleState};
use crate::map::Obstacles;
use crate::map::data::{Park, WorldData};

#[derive(Clone, Copy)]
struct RoadSpot {
    a: Vec2,
    b: Vec2,
    width: f32,
}

impl VehicleFleet {
    pub fn planned(world: &Arc<WorldData>) -> Self {
        let mut fleet = Self {
            vehicles: Vec::new(),
            current: None,
        };
        let obstacles = Obstacles::new(world.clone());
        let mut rng = Park(977);
        let mut segments = Vec::new();
        for road in &world.roads {
            if road.w < 4.0 {
                continue;
            }
            for ab in road.p.windows(2) {
                if ((ab[0] + ab[1]) * 0.5).distance(world.spawn) < 190.0
                    && ab[0].distance(ab[1]) > 8.0
                {
                    segments.push(RoadSpot {
                        a: ab[0],
                        b: ab[1],
                        width: road.w,
                    });
                }
            }
        }
        // A specified Fisher-Yates shuffle avoids JS comparator-sort dependence.
        for i in (1..segments.len()).rev() {
            let j = (rng.f() * (i + 1) as f32) as usize;
            segments.swap(i, j.min(i));
        }
        // Source reserves a wide-road bus before normal randomized placement.
        for road in &segments {
            if road.width < 9.0 || road.a.distance(road.b) < 16.0 {
                continue;
            }
            let u = (road.b - road.a).normalize();
            let n = Vec2::new(-u.y, u.x);
            let p = (road.a + road.b) * 0.5 + n * (road.width * 0.5 - 1.6);
            if fleet.try_spawn(
                world,
                &obstacles,
                VehicleKind::Bus,
                p,
                u.x.atan2(u.y),
                false,
                &mut rng,
            ) {
                break;
            }
        }
        let mut garbage = false;
        let mut docks = false;
        for road in &segments {
            if fleet.vehicles.len() >= 48 {
                break;
            }
            let u = (road.b - road.a).normalize();
            let n = Vec2::new(-u.y, u.x);
            let length = road.a.distance(road.b);
            let hd = u.x.atan2(u.y);
            let big = road.width >= 9.0;
            let wide = [
                VehicleKind::Taxi,
                VehicleKind::Sedan,
                VehicleKind::Sedan,
                VehicleKind::Van,
                VehicleKind::Kei,
                VehicleKind::Scooter,
                VehicleKind::Moto,
                VehicleKind::Delivery,
            ];
            let small = [
                VehicleKind::Scooter,
                VehicleKind::Scooter,
                VehicleKind::Escooter,
                VehicleKind::Moto,
                VehicleKind::Delivery,
                VehicleKind::Basket,
                VehicleKind::Kei,
            ];
            let mut kind = if big {
                wide[(rng.f() * 8.0) as usize]
            } else {
                small[(rng.f() * 7.0) as usize]
            };
            if !garbage && big && rng.f() < 0.15 {
                kind = VehicleKind::Garbage;
            }
            let t = 0.25 + rng.f() * 0.5;
            let side = if rng.f() < 0.5 { 1.0 } else { -1.0 };
            if big && !docks && rng.f() < 0.25 {
                let mut added = 0;
                for k in 0..4 {
                    let p = road.a
                        + u * (length * t + k as f32 * 0.9)
                        + n * (road.width * 0.5 + 0.6) * side;
                    if fleet.try_spawn(
                        world,
                        &obstacles,
                        VehicleKind::Ubike,
                        p,
                        hd + std::f32::consts::FRAC_PI_2,
                        true,
                        &mut rng,
                    ) {
                        added += 1;
                    }
                }
                docks = added > 0;
                if docks {
                    continue;
                }
            }
            let two = kind.spec().two;
            let p = road.a
                + u * length * t
                + n * (road.width * 0.5 - if two { 0.5 } else { 1.3 }) * side;
            let heading = hd
                + if two {
                    if rng.f() < 0.7 {
                        std::f32::consts::FRAC_PI_2
                    } else {
                        0.0
                    }
                } else if rng.f() < 0.5 {
                    std::f32::consts::PI
                } else {
                    0.0
                };
            if fleet.try_spawn(world, &obstacles, kind, p, heading, false, &mut rng)
                && kind == VehicleKind::Garbage
            {
                garbage = true;
            }
        }
        fleet.plan_abandoned(world, &obstacles);
        fleet
    }

    #[allow(clippy::too_many_arguments)]
    fn try_spawn(
        &mut self,
        world: &WorldData,
        obstacles: &Obstacles,
        kind: VehicleKind,
        p: Vec2,
        heading: f32,
        docked: bool,
        rng: &mut Park,
    ) -> bool {
        let mut v = VehicleState::new(self.vehicles.len() as u32, kind, p, heading, &mut || {
            rng.f()
        });
        v.docked = docked;
        let corners = v.corners_at(p, heading);
        if p.distance(world.spawn) < 4.0 || obstacles.footprint_blocked(corners, None) {
            return false;
        }
        // Preserve original doorway access and don't overlap the small decorative
        // scooters, whose original planner intentionally omitted collision.
        if world.blds.iter().filter_map(|b| b.door).any(|d| {
            let q = d.p + d.n * 1.1;
            corners.iter().any(|c| c.distance(q) < 1.6)
                || p.distance(q) < kind.spec().width * 0.5 + 1.3
        }) {
            return false;
        }
        if world.props.iter().any(|prop| {
            matches!(prop.kind, crate::map::street::PropKind::Scooter)
                && point_in_vehicle(
                    prop.p,
                    p,
                    heading,
                    kind.spec().length + 0.8,
                    kind.spec().width + 0.55,
                )
        }) {
            return false;
        }
        self.vehicles.push(v);
        self.publish_colliders(obstacles);
        true
    }

    fn plan_abandoned(&mut self, world: &WorldData, obstacles: &Obstacles) {
        let mut rng = Park(4177);
        let mut spots = Vec::new();
        for road in &world.roads {
            if road.w < 9.0 {
                continue;
            }
            for ab in road.p.windows(2) {
                if ab[0].distance(ab[1]) < 18.0 {
                    continue;
                }
                let u = (ab[1] - ab[0]).normalize();
                let p = (ab[0] + ab[1]) * 0.5 + Vec2::new(-u.y, u.x) * (road.w * 0.5 - 2.0);
                let distance = p.distance(world.spawn);
                if distance > 130.0 && distance < 560.0 && world.bld_at(p).is_none() {
                    spots.push((p, u.x.atan2(u.y), distance));
                }
            }
        }
        spots.sort_by(|a, b| a.2.total_cmp(&b.2));
        let kinds = [
            VehicleKind::Mengshi,
            VehicleKind::Zbl08,
            VehicleKind::Zbd05,
            VehicleKind::Ztd05,
            VehicleKind::Mengshi,
            VehicleKind::Zbl08,
        ];
        for (i, kind) in kinds.into_iter().enumerate() {
            if spots.is_empty() {
                break;
            }
            let start = (((i as f32 + rng.f() * 0.8) / 6.0) * spots.len() as f32) as usize;
            // Same radial bands as the source, with an explicit bounded fallback
            // for native static scenery that differs from Three's random stream.
            for offset in 0..spots.len() {
                let (p, heading, _) = spots[(start + offset) % spots.len()];
                if self.try_spawn(world, obstacles, kind, p, heading, false, &mut rng) {
                    break;
                }
            }
        }
    }
}

fn point_in_vehicle(point: Vec2, p: Vec2, heading: f32, length: f32, width: f32) -> bool {
    let d = point - p;
    d.dot(Vec2::new(heading.sin(), heading.cos())).abs() < length * 0.5
        && d.dot(Vec2::new(heading.cos(), -heading.sin())).abs() < width * 0.5
}
