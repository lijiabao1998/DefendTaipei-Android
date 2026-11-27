//! Bounded street utility dressing from Web `decay.ts` and `roads.ts`.
//!
//! The source map has no native collision-backed utility-pole catalogue. Reuse
//! existing curb lamp anchors instead of creating phantom ground obstacles. All
//! added assemblies sit above 3.7 m; cables only join admitted anchors along the
//! same curb and are rejected when they intersect a building. Original prop,
//! road, collision and saved identities are never rewritten by this layer.

use std::{collections::HashSet, sync::OnceLock};

use bevy::prelude::*;

use super::{Prop, PropKind, road_marking_plans};
use crate::{
    game_flow::GameFlow,
    geom::{self, hex, place},
    map::{
        build::{Mat, Parts, WHITE},
        data::WorldData,
        textures::MapMaterials,
    },
    survival::ShelterEnvironment,
    ui::PauseOpen,
};

pub const MAX_UTILITY_VERTICES: usize = 2_400;
pub const MAX_SIGNAL_VERTICES: usize = 700;
pub const LOWEST_EQUIPMENT_Y: f32 = 3.7;
const CABLE_HEIGHT: f32 = 8.05;
const MAX_SPAN: f32 = 35.0;

#[derive(Clone, Debug, PartialEq)]
pub struct UtilitySite {
    pub prop_index: usize,
    pub position: Vec2,
    pub angle: f32,
    pub transformer: bool,
    /// Each span belongs to one anchor and is emitted exactly once.
    pub cable_to: Option<Vec2>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalSite {
    pub prop_index: usize,
    pub position: Vec2,
    /// Local +Z faces approaching road traffic.
    pub direction: Vec2,
    pub axis: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FurnishingPlans {
    pub utilities: Vec<UtilitySite>,
    pub signals: Vec<SignalSite>,
}

fn stable_hash(index: usize, p: Vec2) -> u32 {
    let mut x = (index as u32).wrapping_mul(0x9e37_79b9)
        ^ p.x.to_bits().rotate_left(7)
        ^ p.y.to_bits().rotate_right(9)
        ^ 0x5e17_2026;
    x = (x ^ (x >> 16)).wrapping_mul(0x7feb_352d);
    x ^ (x >> 15)
}

fn overhead_clear(world: &WorldData, p: Vec2, radius: f32, bottom: f32) -> bool {
    let lo = p - radius;
    let hi = p + radius;
    world.blds_in(lo, hi).into_iter().all(|id| {
        let b = &world.blds[id as usize];
        // Include the existing rooftop-shack/tank envelope, not just the wall.
        b.h + 4.0 < bottom
            || b.pts.iter().enumerate().all(|(i, a)| {
                !crate::map::data::pip(p, &b.pts)
                    && crate::map::data::seg_dist(p, *a, b.pts[(i + 1) % b.pts.len()]) > radius
            })
    })
}

fn cable_clear(world: &WorldData, a: Vec2, b: Vec2) -> bool {
    let steps = (a.distance(b) / 0.4).ceil() as usize;
    (0..=steps).all(|i| {
        let t = i as f32 / steps.max(1) as f32;
        overhead_clear(
            world,
            a.lerp(b, t),
            0.22,
            CABLE_HEIGHT - 0.6 - (t * std::f32::consts::PI).sin() * 0.65,
        )
    })
}

impl FurnishingPlans {
    pub fn build(world: &WorldData) -> Self {
        let lamps: Vec<(usize, &Prop)> = world
            .props
            .iter()
            .enumerate()
            .filter(|(_, p)| p.kind == PropKind::Lamp)
            .collect();
        let mut selected = HashSet::new();
        let mut spans = Vec::new();
        for &(index, lamp) in &lamps {
            if !stable_hash(index, lamp.p).is_multiple_of(7)
                || !overhead_clear(world, lamp.p, 0.98, 5.5)
            {
                continue;
            }
            selected.insert(index);
            let facing = Vec2::new(lamp.ang.sin(), lamp.ang.cos());
            let along = Vec2::new(facing.y, -facing.x);
            let neighbour = lamps
                .iter()
                .filter(|(other_index, other)| {
                    let delta = other.p - lamp.p;
                    *other_index != index
                        && delta.dot(along) > 8.0
                        && delta.dot(facing).abs() < 1.4
                        && delta.length() <= MAX_SPAN
                        && (other.ang - lamp.ang).cos() > 0.995
                        && overhead_clear(world, other.p, 0.98, 5.5)
                })
                .min_by(|(_, a), (_, b)| {
                    a.p.distance_squared(lamp.p)
                        .total_cmp(&b.p.distance_squared(lamp.p))
                });
            if let Some((other_index, other)) = neighbour
                && cable_clear(world, lamp.p, other.p)
            {
                selected.insert(*other_index);
                spans.push((index, other.p));
            }
        }
        let utilities = lamps
            .iter()
            .filter(|(index, _)| selected.contains(index))
            .map(|&(prop_index, p)| UtilitySite {
                prop_index,
                position: p.p,
                angle: p.ang,
                transformer: stable_hash(prop_index, p.p).is_multiple_of(3),
                cable_to: spans
                    .iter()
                    .find_map(|(id, point)| (*id == prop_index).then_some(*point)),
            })
            .collect();
        let mut signals = Vec::new();
        let mut used = HashSet::new();
        for approach in road_marking_plans(world)
            .iter()
            .flat_map(|plan| &plan.approaches)
        {
            // Reuse a real curb pole close to the existing crossing. A missing
            // safe pole means no signalled approach, never an extra ground pole.
            let n = Vec2::new(-approach.inward.y, approach.inward.x);
            let target = approach.junction
                + approach.inward * (approach.clearance + 0.6)
                + n * (approach.width * 0.5 + 0.4);
            let site = lamps
                .iter()
                .filter(|(index, p)| {
                    !used.contains(index)
                        && p.p.distance(target) < 8.0
                        && overhead_clear(world, p.p, 1.1, LOWEST_EQUIPMENT_Y)
                })
                .min_by(|(_, a), (_, b)| {
                    a.p.distance_squared(target)
                        .total_cmp(&b.p.distance_squared(target))
                });
            if let Some(&(prop_index, prop)) = site {
                used.insert(prop_index);
                signals.push(SignalSite {
                    prop_index,
                    position: prop.p,
                    direction: approach.inward,
                    axis: u8::from(approach.inward.x.abs() < approach.inward.y.abs()),
                });
            }
        }
        Self { utilities, signals }
    }
}

fn with_plans<T>(world: &WorldData, f: impl FnOnce(&FurnishingPlans) -> T) -> T {
    static PLANS: OnceLock<FurnishingPlans> = OnceLock::new();
    if std::ptr::eq(world, crate::map::world().as_ref()) {
        f(PLANS.get_or_init(|| FurnishingPlans::build(world)))
    } else {
        f(&FurnishingPlans::build(world))
    }
}

pub fn plans(world: &WorldData) -> FurnishingPlans {
    with_plans(world, Clone::clone)
}

fn bar(out: &mut Parts, a: Vec3, b: Vec3, width: f32, color: u32) {
    let delta = b - a;
    out.add(
        Mat::Metal,
        &geom::cuboid(width, delta.length(), width),
        Mat4::from_translation((a + b) * 0.5)
            * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, delta.normalize())),
        hex(color),
    );
}

pub fn utility_geometry(site: &UtilitySite) -> Parts {
    let mut out = Parts::default();
    let t = place(site.position.x, 0.0, site.position.y, site.angle);
    // Continue the original 5 m lamp shaft; there is no second ground obstacle.
    out.add(
        Mat::VCol,
        &geom::cylinder(0.075, 0.09, 3.6, 6),
        t * geom::at(0.0, 6.8, 0.0),
        hex(0x949488),
    );
    for y in [7.4, 8.0] {
        out.add(
            Mat::Metal,
            &geom::cuboid(1.8, 0.08, 0.1),
            t * geom::at(0.0, y, 0.0),
            hex(0x62635a),
        );
        for x in [-0.76, -0.27, 0.27, 0.76] {
            out.add(
                Mat::VCol,
                &geom::cylinder(0.05, 0.07, 0.16, 6),
                t * geom::at(x, y + 0.1, 0.0),
                hex(0xdbd8c5),
            );
        }
    }
    if site.transformer {
        out.add(
            Mat::Metal,
            &geom::cuboid(0.57, 0.75, 0.4),
            t * geom::at(0.0, 5.95, -0.3),
            hex(0x778779),
        );
        for y in [5.67, 6.23] {
            out.add(
                Mat::Metal,
                &geom::cuboid(0.64, 0.055, 0.46),
                t * geom::at(0.0, y, -0.3),
                hex(0x4f5c54),
            );
        }
        for x in [-0.2, 0.0, 0.2] {
            out.add(
                Mat::VCol,
                &geom::cuboid(0.035, 0.42, 0.016),
                t * geom::at(x, 5.95, -0.507),
                hex(0x40594e),
            );
        }
    }
    if let Some(end) = site.cable_to {
        for wire in 0..3 {
            let offset = (wire as f32 - 1.0) * 0.2;
            let cross = Vec2::new(site.angle.cos(), -site.angle.sin()) * offset;
            for segment in 0..8 {
                let point = |i: usize| {
                    let phase = i as f32 / 8.0;
                    let p = site.position.lerp(end, phase) + cross;
                    Vec3::new(
                        p.x,
                        CABLE_HEIGHT - (phase * std::f32::consts::PI).sin() * 0.65,
                        p.y,
                    )
                };
                bar(
                    &mut out,
                    point(segment),
                    point(segment + 1),
                    0.025,
                    0x303632,
                );
            }
        }
    }
    out
}

pub fn signal_geometry(site: &SignalSite) -> Parts {
    let mut out = Parts::default();
    let t = place(
        site.position.x,
        0.0,
        site.position.y,
        site.direction.x.atan2(site.direction.y),
    );
    out.add(
        Mat::Metal,
        &geom::cuboid(0.08, 0.08, 0.65),
        t * geom::at(0.0, 4.55, 0.28),
        hex(0x5a5e62),
    );
    out.add(
        Mat::Metal,
        &geom::cuboid(1.12, 0.46, 0.26),
        t * geom::at(0.0, 4.32, 0.56),
        hex(0x24282a),
    );
    for (aspect, x) in [-0.34, 0.0, 0.34].into_iter().enumerate() {
        out.add(
            Mat::Metal,
            &geom::cuboid(0.30, 0.045, 0.2),
            t * geom::at(x, 4.52, 0.76),
            hex(0x24282a),
        );
        out.add(
            Mat::Signal(site.axis * 3 + aspect as u8),
            &geom::cylinder(0.13, 0.13, 0.016, 12),
            t * geom::at(x, 4.32, 0.701) * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
            WHITE,
        );
    }
    out
}

pub(super) fn build_chunk(world: &WorldData, min: Vec2, max: Vec2, out: &mut Parts) {
    let inside = |p: Vec2| p.x >= min.x && p.y >= min.y && p.x < max.x && p.y < max.y;
    with_plans(world, |plans| {
        for utility in plans.utilities.iter().filter(|site| inside(site.position)) {
            out.merge(utility_geometry(utility));
        }
        for signal in plans.signals.iter().filter(|site| inside(site.position)) {
            out.merge(signal_geometry(signal));
        }
    });
}

/// Exact Web `RoadTiles.update` visual clock; this is not an extra gameplay timer.
#[derive(Resource, Default, Debug)]
pub struct SignalPhase {
    pub seconds: f32,
    /// Renderer QA can freeze the phase without changing production power/time.
    #[cfg(not(target_os = "android"))]
    pub qa_fixed: Option<f32>,
}

impl SignalPhase {
    pub fn value(&self) -> f32 {
        #[cfg(not(target_os = "android"))]
        if let Some(fixed) = self.qa_fixed {
            return fixed.rem_euclid(30.0);
        }
        self.seconds.rem_euclid(30.0)
    }

    pub fn aspects(&self, power: bool) -> [bool; 6] {
        if !power {
            return [false; 6];
        }
        let phase = self.value();
        [
            phase >= 15.0,
            (12.0..15.0).contains(&phase),
            phase < 12.0,
            phase < 15.0,
            phase >= 27.0,
            (15.0..27.0).contains(&phase),
        ]
    }

    /// The original vehicle permission overlaps amber by one second.
    pub fn green(&self, direction: Vec2, power: bool) -> bool {
        if !power {
            return true;
        }
        let phase = self.value();
        if direction.x.abs() >= direction.y.abs() {
            phase < 13.0
        } else {
            (15.0..28.0).contains(&phase)
        }
    }
}

pub(crate) fn update_signals(
    time: Res<Time>,
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    environment: Res<ShelterEnvironment>,
    mut phase: ResMut<SignalPhase>,
    city: Res<MapMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut previous: Local<Option<[bool; 6]>>,
) {
    let live = matches!(flow.get(), GameFlow::Prologue | GameFlow::Playing);
    if live && !pause.0 {
        phase.seconds = (phase.seconds + time.delta_secs()).rem_euclid(30.0);
    }
    let active = phase.aspects(live && environment.power);
    if *previous == Some(active) {
        return;
    }
    *previous = Some(active);
    for (index, handle) in city.signals.iter().enumerate() {
        if let Some(mut material) = materials.get_mut(handle) {
            let rgb = hex([0xff2010, 0xffb000, 0x20ff80][index % 3]);
            let strength = if active[index] { 2.2 } else { 0.0 };
            material.emissive =
                LinearRgba::rgb(rgb[0] * strength, rgb[1] * strength, rgb[2] * strength);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_signal_clock_keeps_axes_exclusive_and_blackout_dark() {
        for (phase, expected) in [
            (0.0, [2, 3]),
            (11.999, [2, 3]),
            (12.0, [1, 3]),
            (15.0, [0, 5]),
            (27.0, [0, 4]),
            (30.0, [2, 3]),
        ] {
            let active = SignalPhase {
                seconds: phase,
                ..default()
            }
            .aspects(true);
            assert_eq!(active.iter().filter(|on| **on).count(), 2);
            for index in expected {
                assert!(active[index]);
            }
            assert_eq!(
                SignalPhase {
                    seconds: phase,
                    ..default()
                }
                .aspects(false),
                [false; 6]
            );
        }
        assert!(
            SignalPhase {
                seconds: 12.9,
                ..default()
            }
            .green(Vec2::X, true)
        );
        assert!(
            !SignalPhase {
                seconds: 13.0,
                ..default()
            }
            .green(Vec2::X, true)
        );
        assert!(
            SignalPhase {
                seconds: 27.9,
                ..default()
            }
            .green(Vec2::Y, true)
        );
        assert!(
            !SignalPhase {
                seconds: 28.0,
                ..default()
            }
            .green(Vec2::Y, true)
        );
        assert!(
            SignalPhase {
                seconds: 14.0,
                ..default()
            }
            .green(Vec2::X, false)
        );
    }

    #[test]
    fn street_equipment_reuses_original_anchors_with_bounded_overhead_geometry() {
        let world = crate::map::world();
        let plan = plans(world);
        assert!(!plan.utilities.is_empty());
        assert!(!plan.signals.is_empty());
        assert!(plan.utilities.iter().any(|site| site.cable_to.is_some()));
        assert_eq!(plan, FurnishingPlans::build(world));
        for site in &plan.utilities {
            assert_eq!(world.props[site.prop_index].p, site.position);
            assert_eq!(world.props[site.prop_index].kind, PropKind::Lamp);
            let geometry = utility_geometry(site);
            assert!(geometry.vcount() <= MAX_UTILITY_VERTICES);
            assert!(geometry.0.len() <= 2);
            for mesh in geometry.0.values() {
                assert!(
                    mesh.pos
                        .iter()
                        .all(|p| p.iter().all(|x| x.is_finite()) && p[1] >= LOWEST_EQUIPMENT_Y)
                );
            }
            if let Some(end) = site.cable_to {
                assert!(site.position.distance(end) <= MAX_SPAN);
                assert!(cable_clear(world, site.position, end));
                assert!(plan.utilities.iter().any(|other| other.position == end));
            }
        }
        let mut ids = HashSet::new();
        for site in &plan.signals {
            assert!(ids.insert(site.prop_index));
            assert_eq!(world.props[site.prop_index].p, site.position);
            let geometry = signal_geometry(site);
            assert!(geometry.vcount() <= MAX_SIGNAL_VERTICES);
            assert!(
                geometry
                    .0
                    .values()
                    .flat_map(|mesh| &mesh.pos)
                    .all(|p| p[1] >= LOWEST_EQUIPMENT_Y)
            );
        }
    }
}
