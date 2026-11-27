//! Render-only road marking plans. They use the original OSM segment and parked-prop
//! identities; none of these painted surfaces enters collision or saved world state.

use std::collections::HashMap;
use std::sync::OnceLock;

use bevy::prelude::*;

use super::{PropKind, curb_style};
use crate::geom::{self, hex, place};
use crate::map::build::{Mat, Parts, flat_poly};
use crate::map::data::{ARC, Door, WorldData, pip};

#[derive(Clone, Debug)]
pub struct RoadMarkingPlan {
    pub road_index: usize,
    pub segment_index: usize,
    pub a: Vec2,
    pub b: Vec2,
    pub width: f32,
    /// Distance from each source endpoint to the first ordinary lane marking.
    pub clear_start: f32,
    pub clear_end: f32,
    pub approaches: Vec<JunctionApproach>,
    /// Negative/positive road-normal side: no edge stripe across an admitted bay.
    pub edge_gaps: [Vec<[f32; 2]>; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct JunctionApproach {
    pub road_index: usize,
    pub segment_index: usize,
    pub end: u8,
    pub junction: Vec2,
    /// Points away from the junction, along the existing source segment.
    pub inward: Vec2,
    pub width: f32,
    /// Connected-road envelope, measured along this approach.
    pub clearance: f32,
    pub has_arrow: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ParkingPlan {
    pub prop_index: usize,
    pub road_index: usize,
    pub segment_index: usize,
    pub center: Vec2,
    pub angle: f32,
    pub width: f32,
    pub length: f32,
}

#[derive(Clone, Copy)]
struct Arm {
    direction: Vec2,
    width: f32,
}

type Junctions = HashMap<(i32, i32), Vec<Arm>>;

fn node_key(p: Vec2) -> (i32, i32) {
    ((p.x * 10.0).round() as i32, (p.y * 10.0).round() as i32)
}

fn junction_arms(w: &WorldData) -> Junctions {
    let mut nodes = Junctions::new();
    for road in &w.roads {
        for segment in road.p.windows(2) {
            let direction = (segment[1] - segment[0]).normalize_or_zero();
            if direction == Vec2::ZERO {
                continue;
            }
            for (point, direction) in [(segment[0], direction), (segment[1], -direction)] {
                let arms = nodes.entry(node_key(point)).or_default();
                // A road can be split into several OSM ways at the same junction.
                // Coincident arms describe one physical approach, not extra crossings.
                if let Some(arm) = arms
                    .iter_mut()
                    .find(|a| a.direction.dot(direction) > 0.9998)
                {
                    arm.width = arm.width.max(road.w);
                } else {
                    arms.push(Arm {
                        direction,
                        width: road.w,
                    });
                }
            }
        }
    }
    nodes
}

fn connected_clearance(nodes: &Junctions, point: Vec2, inward: Vec2, width: f32) -> Option<f32> {
    let arms = nodes.get(&node_key(point))?;
    if arms.len() < 3 {
        return None;
    }
    // The whole painted width must clear each intersecting ribbon. Using only the
    // centreline would leave the outside crosswalk bars inside an oblique junction.
    arms.iter()
        .filter_map(|arm| {
            let sin = inward.perp_dot(arm.direction).abs();
            (sin > 0.01)
                .then(|| (arm.width * 0.5 + width * 0.5 * inward.dot(arm.direction).abs()) / sin)
        })
        .reduce(f32::max)
        .map(|distance| distance + 0.35)
}

fn in_rect(point: Vec2, min: Vec2, max: Vec2) -> bool {
    point.x >= min.x && point.y >= min.y && point.x < max.x && point.y < max.y
}

fn rectangle(center: Vec2, across: Vec2, along: Vec2, width: f32, length: f32) -> [Vec2; 4] {
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .map(|(x, z)| center + across * (x * width * 0.5) + along * (z * length * 0.5))
}

fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let ab = b - a;
    let cd = d - c;
    let denom = ab.perp_dot(cd);
    if denom.abs() < 1e-5 {
        return false;
    }
    let t = (c - a).perp_dot(cd) / denom;
    let u = (c - a).perp_dot(ab) / denom;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
}

fn polygons_overlap(a: &[Vec2], b: &[Vec2]) -> bool {
    a.iter().any(|p| pip(*p, b))
        || b.iter().any(|p| pip(*p, a))
        || (0..a.len()).any(|i| {
            (0..b.len())
                .any(|j| segments_intersect(a[i], a[(i + 1) % a.len()], b[j], b[(j + 1) % b.len()]))
        })
}

fn clear_of_buildings(w: &WorldData, footprint: &[Vec2]) -> bool {
    let min = footprint
        .iter()
        .copied()
        .fold(Vec2::splat(f32::MAX), Vec2::min);
    let max = footprint
        .iter()
        .copied()
        .fold(Vec2::splat(f32::MIN), Vec2::max);
    w.blds.iter().all(|building| {
        building.min.x > max.x
            || building.max.x < min.x
            || building.min.y > max.y
            || building.max.y < min.y
            || !polygons_overlap(footprint, &building.pts)
    })
}

fn clear_of_entrances(footprint: &[Vec2], doors: &[Door]) -> bool {
    doors.iter().all(|door| {
        let length = ARC + 4.3;
        let center = door.p + door.n * (length * 0.5 - 0.8);
        let corridor = rectangle(center, door.tangent(), door.n, 2.5, length);
        !polygons_overlap(footprint, &corridor)
    })
}

fn segment_plan(
    w: &WorldData,
    nodes: &Junctions,
    road_index: usize,
    segment_index: usize,
) -> Option<RoadMarkingPlan> {
    let road = &w.roads[road_index];
    let (a, b, width) = (road.p[segment_index], road.p[segment_index + 1], road.w);
    let length = a.distance(b);
    if width < 9.0 || length < 2.0 {
        return None;
    }
    let u = (b - a) / length;
    let clearances = [
        connected_clearance(nodes, a, u, width),
        connected_clearance(nodes, b, -u, width),
    ];
    let available = length - clearances[0].unwrap_or(0.0) - clearances[1].unwrap_or(0.0);
    let junction_count = clearances.iter().flatten().count();
    let enough_crossing = available >= if junction_count == 2 { 10.5 } else { 5.5 };
    let enough_arrow = available >= if junction_count == 2 { 26.0 } else { 14.0 };
    let mut approaches = Vec::new();
    let mut clears = [0.0, 0.0];
    for (end, (junction, inward)) in [(a, u), (b, -u)].into_iter().enumerate() {
        let Some(clearance) = clearances[end] else {
            continue;
        };
        clears[end] = clearance + 0.4;
        if !enough_crossing {
            continue;
        }
        let n = Vec2::new(-inward.y, inward.x);
        let footprint = rectangle(
            junction + inward * (clearance + 2.0),
            n,
            inward,
            width - 1.0,
            3.0,
        );
        let stop_footprint = rectangle(
            junction + inward * (clearance + 4.7) + n * (width * 0.25),
            n,
            inward,
            width * 0.5 - 0.7,
            0.35,
        );
        if !clear_of_buildings(w, &footprint) || !clear_of_buildings(w, &stop_footprint) {
            continue;
        }
        let arrow_footprint = rectangle(
            junction + inward * (clearance + 8.9) + n * (width * 0.25),
            n,
            inward,
            1.2,
            3.0,
        );
        approaches.push(JunctionApproach {
            road_index,
            segment_index,
            end: end as u8,
            junction,
            inward,
            width,
            clearance,
            has_arrow: enough_arrow && clear_of_buildings(w, &arrow_footprint),
        });
        clears[end] = clearance + 3.85;
    }
    Some(RoadMarkingPlan {
        road_index,
        segment_index,
        a,
        b,
        width,
        clear_start: clears[0],
        clear_end: clears[1],
        approaches,
        edge_gaps: [Vec::new(), Vec::new()],
    })
}

fn build_road_marking_plans(w: &WorldData, nodes: &Junctions) -> Vec<RoadMarkingPlan> {
    let mut plans = Vec::new();
    for (road_index, road) in w.roads.iter().enumerate() {
        for segment_index in 0..road.p.len().saturating_sub(1) {
            if let Some(plan) = segment_plan(w, nodes, road_index, segment_index) {
                plans.push(plan);
            }
        }
    }
    plans
}

struct StreetPlans {
    roads: Vec<RoadMarkingPlan>,
    parking: Vec<ParkingPlan>,
}

impl StreetPlans {
    fn build(w: &WorldData) -> Self {
        let nodes = junction_arms(w);
        let mut roads = build_road_marking_plans(w, &nodes);
        let parking = build_parking_plans(w, &nodes);
        for bay in &parking {
            let Some(road) = roads.iter_mut().find(|plan| {
                plan.road_index == bay.road_index && plan.segment_index == bay.segment_index
            }) else {
                continue;
            };
            let u = (road.b - road.a).normalize();
            let n = Vec2::new(-u.y, u.x);
            let along = Vec2::new(bay.angle.sin(), bay.angle.cos());
            let across = Vec2::new(bay.angle.cos(), -bay.angle.sin());
            let corners = rectangle(
                bay.center,
                across,
                along,
                bay.width + 0.06,
                bay.length + 0.06,
            );
            let min = corners
                .iter()
                .map(|p| (*p - road.a).dot(u))
                .fold(f32::MAX, f32::min);
            let max = corners
                .iter()
                .map(|p| (*p - road.a).dot(u))
                .fold(f32::MIN, f32::max);
            let side = usize::from((bay.center - road.a).dot(n) >= 0.0);
            let stripe = (if side == 0 { -1.0 } else { 1.0 }) * (road.width * 0.5 - 0.45);
            let lateral_min = corners
                .iter()
                .map(|p| (*p - road.a).dot(n))
                .fold(f32::MAX, f32::min);
            let lateral_max = corners
                .iter()
                .map(|p| (*p - road.a).dot(n))
                .fold(f32::MIN, f32::max);
            if lateral_min < stripe + 0.1 && lateral_max > stripe - 0.1 {
                road.edge_gaps[side].push([min - 0.12, max + 0.12]);
            }
        }
        for road in &mut roads {
            for gaps in &mut road.edge_gaps {
                gaps.sort_by(|a, b| a[0].total_cmp(&b[0]));
            }
        }
        Self { roads, parking }
    }
}

fn with_plans<T>(w: &WorldData, f: impl FnOnce(&StreetPlans) -> T) -> T {
    // Only the immutable global world uses the cache. Tests or independent callers
    // with any other WorldData regenerate plans, even when their coordinates match.
    // A raw address is never retained as a reusable key for a dropped test world.
    static PLANS: OnceLock<StreetPlans> = OnceLock::new();
    if std::ptr::eq(w, crate::map::world().as_ref()) {
        f(PLANS.get_or_init(|| StreetPlans::build(w)))
    } else {
        f(&StreetPlans::build(w))
    }
}

pub(super) fn road_marking_plans_in(w: &WorldData, min: Vec2, max: Vec2) -> Vec<RoadMarkingPlan> {
    with_plans(w, |plans| {
        plans
            .roads
            .iter()
            .filter(|plan| in_rect((plan.a + plan.b) * 0.5, min, max))
            .cloned()
            .collect()
    })
}

/// Complete deterministic marking inventory, also used by the geometry and native QA gates.
pub fn road_marking_plans(w: &WorldData) -> Vec<RoadMarkingPlan> {
    with_plans(w, |plans| plans.roads.clone())
}

fn painted_rect(
    out: &mut Parts,
    center: Vec2,
    along: Vec2,
    length: f32,
    width: f32,
    y: f32,
    color: u32,
) {
    let angle = -along.y.atan2(along.x);
    out.add(
        Mat::Marking,
        &geom::plane(length, width),
        place(center.x, y, center.y, angle) * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        hex(color),
    );
}

/// One actual junction approach, independently regenerable for camera framing and upload checks.
pub fn approach_geometry(plan: &JunctionApproach) -> Parts {
    let mut out = Parts::default();
    let d = plan.inward;
    let n = Vec2::new(-d.y, d.x);
    let y = 0.035 + plan.width * 0.0005;
    let crossing = plan.junction + d * (plan.clearance + 2.0);
    let mut across = -plan.width * 0.5 + 0.72;
    while across + 0.23 < plan.width * 0.5 - 0.5 {
        painted_rect(&mut out, crossing + n * across, d, 3.0, 0.46, y, 0xecece6);
        across += 0.9;
    }
    // Right-hand traffic: when approaching the junction (-d), the inbound lane is
    // to its right, +n. Reversing source-segment order therefore preserves direction.
    let lane = n * (plan.width * 0.25);
    painted_rect(
        &mut out,
        plan.junction + d * (plan.clearance + 4.7) + lane,
        d,
        0.35,
        plan.width * 0.5 - 0.7,
        y,
        0xecece6,
    );
    if plan.has_arrow {
        let center = plan.junction + d * (plan.clearance + 8.9) + lane;
        let points = [
            center - d * 1.5,
            center - d * 0.35 + n * 0.6,
            center - d * 0.35 + n * 0.18,
            center + d * 1.5 + n * 0.18,
            center + d * 1.5 - n * 0.18,
            center - d * 0.35 - n * 0.18,
            center - d * 0.35 - n * 0.6,
        ];
        out.add(
            Mat::Marking,
            &flat_poly(&points, y, true, 1.0),
            Mat4::IDENTITY,
            hex(0xe7e7dd),
        );
    }
    out
}

pub fn road_marking_geometry(plan: &RoadMarkingPlan) -> Parts {
    let mut out = Parts::default();
    let length = plan.a.distance(plan.b);
    let u = (plan.b - plan.a) / length;
    let n = Vec2::new(-u.y, u.x);
    let start = plan.clear_start;
    let end = length - plan.clear_end;
    let y = 0.035 + plan.width * 0.0005;
    if end > start {
        let center = plan.a + u * ((start + end) * 0.5);
        if plan.width >= 12.0 {
            for offset in [-0.18, 0.18] {
                painted_rect(
                    &mut out,
                    center + n * offset,
                    u,
                    end - start,
                    0.12,
                    y,
                    0xd8b040,
                );
            }
        } else {
            let mut distance = start;
            while distance < end - 0.15 {
                let dash = 2.0f32.min(end - distance);
                painted_rect(
                    &mut out,
                    plan.a + u * (distance + dash * 0.5),
                    u,
                    dash,
                    0.12,
                    y,
                    0xe8e8e0,
                );
                distance += 6.0;
            }
        }
        for (index, side) in [-1.0, 1.0].into_iter().enumerate() {
            let mut cursor = start;
            for gap in &plan.edge_gaps[index] {
                let from = gap[0].clamp(start, end);
                let to = gap[1].clamp(start, end);
                if from > cursor {
                    let point =
                        plan.a + u * ((cursor + from) * 0.5) + n * side * (plan.width * 0.5 - 0.45);
                    painted_rect(&mut out, point, u, from - cursor, 0.1, y, 0xe8e8e0);
                }
                cursor = cursor.max(to);
            }
            if end > cursor {
                let point =
                    plan.a + u * ((cursor + end) * 0.5) + n * side * (plan.width * 0.5 - 0.45);
                painted_rect(&mut out, point, u, end - cursor, 0.1, y, 0xe8e8e0);
            }
        }
    }
    for approach in &plan.approaches {
        out.merge(approach_geometry(approach));
    }
    out
}

fn build_parking_plans(w: &WorldData, nodes: &Junctions) -> Vec<ParkingPlan> {
    let doors: Vec<_> = w.blds.iter().filter_map(|building| building.door).collect();
    let mut plans = Vec::new();
    for (prop_index, prop) in w.props.iter().enumerate() {
        let (width, length, shift) = match prop.kind {
            PropKind::Scooter => (0.69, 1.65, 0.25),
            PropKind::Car | PropKind::Taxi => (2.05, 4.7, 0.0),
            PropKind::Truck => (2.55, 6.9, 0.0),
            _ => continue,
        };
        let along = Vec2::new(prop.ang.sin(), prop.ang.cos());
        let across = Vec2::new(prop.ang.cos(), -prop.ang.sin());
        let center = prop.p + along * shift;
        let footprint = rectangle(center, across, along, width + 0.06, length + 0.06);
        let mut nearest = None;
        let mut distance = f32::MAX;
        for (road_index, road) in w.roads.iter().enumerate() {
            if road.w < 6.0 {
                continue;
            }
            for (segment_index, pair) in road.p.windows(2).enumerate() {
                let (a, b) = (pair[0], pair[1]);
                let segment_length = a.distance(b);
                if segment_length < 4.0 {
                    continue;
                }
                let u = (b - a) / segment_length;
                let n = Vec2::new(-u.y, u.x);
                let lateral = (center - a).dot(n);
                let start = connected_clearance(nodes, a, u, road.w).map_or(0.4, |c| c + 11.0);
                let end = segment_length
                    - connected_clearance(nodes, b, -u, road.w).map_or(0.4, |c| c + 11.0);
                if lateral.abs() < road.w * 0.2
                    || !footprint.iter().all(|point| {
                        let p = *point - a;
                        p.dot(u) > start && p.dot(u) < end && p.dot(n).abs() < road.w * 0.5 - 0.08
                    })
                {
                    continue;
                }
                if road.w >= 9.0 && curb_style(a, b, road.w, lateral.signum()).1 < 0.72 {
                    continue;
                }
                let aligned = if prop.kind == PropKind::Scooter {
                    along.dot(n).abs() > 0.97
                } else {
                    along.dot(u).abs() > 0.97
                };
                if !aligned {
                    continue;
                }
                let edge_distance = (lateral.abs() - road.w * 0.5).abs();
                if edge_distance < distance {
                    distance = edge_distance;
                    nearest = Some((road_index, segment_index));
                }
            }
        }
        let Some((road_index, segment_index)) = nearest else {
            continue;
        };
        if !clear_of_buildings(w, &footprint) || !clear_of_entrances(&footprint, &doors) {
            continue;
        }
        // A junction can meet the middle of a long source segment too. Reject the
        // full bay envelope if any other road ribbon overlaps it, including a parallel way.
        let crossed = w.roads.iter().enumerate().any(|(other_road, road)| {
            road.p.windows(2).enumerate().any(|(other_segment, pair)| {
                if other_road == road_index && other_segment == segment_index {
                    return false;
                }
                let delta = pair[1] - pair[0];
                let road_length = delta.length();
                if road_length < 0.1 {
                    return false;
                }
                let road_u = delta / road_length;
                let road_n = Vec2::new(-road_u.y, road_u.x);
                let road_box = rectangle(
                    (pair[0] + pair[1]) * 0.5,
                    road_n,
                    road_u,
                    road.w + 0.5,
                    road_length,
                );
                polygons_overlap(&footprint, &road_box)
            })
        });
        if !crossed {
            plans.push(ParkingPlan {
                prop_index,
                road_index,
                segment_index,
                center,
                angle: prop.ang,
                width,
                length,
            });
        }
    }
    plans
}

pub(super) fn parking_plans_in(w: &WorldData, min: Vec2, max: Vec2) -> Vec<ParkingPlan> {
    with_plans(w, |plans| {
        plans
            .parking
            .iter()
            .filter(|plan| in_rect(w.props[plan.prop_index].p, min, max))
            .copied()
            .collect()
    })
}

/// Only existing parked props with a fully clear, unpainted road edge receive a bay.
pub fn parking_plans(w: &WorldData) -> Vec<ParkingPlan> {
    with_plans(w, |plans| plans.parking.clone())
}

pub fn parking_geometry(plan: &ParkingPlan) -> Parts {
    let mut out = Parts::default();
    let along = Vec2::new(plan.angle.sin(), plan.angle.cos());
    let across = Vec2::new(plan.angle.cos(), -plan.angle.sin());
    // Painted bays remain below raised walks and slightly above every asphalt
    // ribbon, and use the existing non-emissive marking material.
    let y = 0.052;
    for side in [-1.0, 1.0] {
        painted_rect(
            &mut out,
            plan.center + across * side * plan.width * 0.5,
            along,
            plan.length,
            0.06,
            y,
            0xdcded5,
        );
        painted_rect(
            &mut out,
            plan.center + along * side * plan.length * 0.5,
            across,
            plan.width,
            0.06,
            y,
            0xdcded5,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::data::Road;

    #[test]
    fn oblique_and_narrow_forks_clear_the_entire_crosswalk_width() {
        for angle in [std::f32::consts::FRAC_PI_2, 0.55f32, 0.08] {
            let branch = Vec2::new(angle.sin(), angle.cos());
            let mut nodes = Junctions::new();
            nodes.insert(
                node_key(Vec2::ZERO),
                vec![
                    Arm {
                        direction: Vec2::Y,
                        width: 9.0,
                    },
                    Arm {
                        direction: -Vec2::Y,
                        width: 9.0,
                    },
                    Arm {
                        direction: branch,
                        width: 12.0,
                    },
                ],
            );
            let clear = connected_clearance(&nodes, Vec2::ZERO, Vec2::Y, 9.0).unwrap();
            let corners = rectangle(Vec2::Y * (clear + 2.0), Vec2::X, Vec2::Y, 8.0, 3.0);
            for corner in corners {
                assert!(
                    corner.perp_dot(branch).abs() > 6.0,
                    "a full crossing must clear a branch even at an acute angle"
                );
            }
            assert!(clear.is_finite());
        }
    }

    #[test]
    fn approach_direction_is_independent_of_source_order_and_stays_in_inbound_lane() {
        for inward in [Vec2::X, Vec2::Y, Vec2::new(0.6, 0.8)] {
            let approach = JunctionApproach {
                road_index: 0,
                segment_index: 0,
                end: 0,
                junction: Vec2::new(5.0, -8.0),
                inward,
                width: 12.0,
                clearance: 7.0,
                has_arrow: true,
            };
            let reverse = JunctionApproach { end: 1, ..approach };
            let mesh = approach_geometry(&approach);
            let reverse_mesh = approach_geometry(&reverse);
            assert_eq!(mesh.0[&Mat::Marking].pos, reverse_mesh.0[&Mat::Marking].pos);
            let right_when_inbound = Vec2::new(-inward.y, inward.x);
            let arrow = &mesh.0[&Mat::Marking].pos[mesh.vcount() - 7..];
            for vertex in arrow {
                let offset = Vec2::new(vertex[0], vertex[2]) - approach.junction;
                assert!(offset.dot(right_when_inbound) > 0.0);
                assert!(offset.dot(inward) > approach.clearance + 7.0);
            }
            let tip = Vec2::new(arrow[0][0], arrow[0][2]);
            let tail = Vec2::new(arrow[3][0], arrow[3][2]);
            assert!((tip - tail).dot(inward) < -2.9);
        }
    }

    #[test]
    fn short_segments_between_two_junctions_do_not_get_overlapping_crossings() {
        let mut world = WorldData::load();
        world.blds.clear();
        world.props.clear();
        let road = |points: &[Vec2]| Road {
            p: points.to_vec(),
            t: "residential".into(),
            w: 12.0,
            name: "fixture".into(),
        };
        world.roads = vec![
            road(&[Vec2::ZERO, Vec2::new(0.0, 14.0)]),
            road(&[Vec2::new(-30.0, 0.0), Vec2::ZERO, Vec2::new(30.0, 0.0)]),
            road(&[
                Vec2::new(-30.0, 14.0),
                Vec2::new(0.0, 14.0),
                Vec2::new(30.0, 14.0),
            ]),
        ];
        let nodes = junction_arms(&world);
        let plan = segment_plan(&world, &nodes, 0, 0).unwrap();
        assert!(plan.approaches.is_empty());
        assert!(plan.clear_start >= 6.7 && plan.clear_end >= 6.7);
        let vertices = road_marking_geometry(&plan).vcount();
        assert!(
            vertices <= 16,
            "at most the small non-junction lane remnant"
        );
        // The global cache must never become the plan for this different test world.
        let before = road_marking_plans(&world);
        world.roads[0].p[1].y = 40.0;
        let after = road_marking_plans(&world);
        assert_ne!(before[0].b, after[0].b);
        // An ordinary OSM polyline node is not a road junction. Keep its yellow
        // lines continuous instead of inserting a new 1.4 m gap at every node.
        world.roads = vec![road(&[
            Vec2::ZERO,
            Vec2::new(20.0, 0.0),
            Vec2::new(40.0, 0.0),
        ])];
        let continuation = road_marking_plans(&world);
        assert_eq!(continuation.len(), 2);
        assert!(continuation.iter().all(|plan| {
            plan.clear_start == 0.0 && plan.clear_end == 0.0 && plan.approaches.is_empty()
        }));
    }

    #[test]
    fn clearance_checks_full_footprints_not_just_centers_or_corners() {
        let horizontal = rectangle(Vec2::ZERO, Vec2::X, Vec2::Y, 10.0, 0.3);
        let vertical = rectangle(Vec2::ZERO, Vec2::X, Vec2::Y, 0.3, 10.0);
        assert!(!horizontal.iter().any(|p| pip(*p, &vertical)));
        assert!(!vertical.iter().any(|p| pip(*p, &horizontal)));
        assert!(polygons_overlap(&horizontal, &vertical));
        let door = Door {
            p: Vec2::ZERO,
            n: Vec2::Y,
        };
        assert!(!clear_of_entrances(&horizontal, &[door]));
        let clear = rectangle(Vec2::new(6.0, 0.0), Vec2::X, Vec2::Y, 2.0, 4.7);
        assert!(clear_of_entrances(&clear, &[door]));
    }

    #[test]
    fn real_parking_bays_fit_source_roads_and_avoid_painted_curbs_and_entrances() {
        let world = crate::map::world();
        let doors: Vec<_> = world
            .blds
            .iter()
            .filter_map(|building| building.door)
            .collect();
        let plans = parking_plans(world);
        assert!(!plans.is_empty());
        for plan in &plans {
            let source = &world.roads[plan.road_index];
            let a = source.p[plan.segment_index];
            let b = source.p[plan.segment_index + 1];
            let u = (b - a).normalize();
            let n = Vec2::new(-u.y, u.x);
            let along = Vec2::new(plan.angle.sin(), plan.angle.cos());
            let across = Vec2::new(plan.angle.cos(), -plan.angle.sin());
            let footprint = rectangle(
                plan.center,
                across,
                along,
                plan.width + 0.06,
                plan.length + 0.06,
            );
            assert!(clear_of_entrances(&footprint, &doors));
            assert!(clear_of_buildings(world, &footprint));
            assert!(
                footprint
                    .iter()
                    .all(|p| (*p - a).dot(n).abs() < source.w * 0.5 - 0.08)
            );
            if source.w >= 9.0 {
                assert!(curb_style(a, b, source.w, (plan.center - a).dot(n).signum()).1 >= 0.72);
            }
            let mesh = parking_geometry(plan);
            assert_eq!(mesh.0.len(), 1);
            assert_eq!(mesh.vcount(), 16);
            assert!(mesh.0[&Mat::Marking].pos.iter().all(|p| p[1] < 0.06));
        }
    }

    #[test]
    fn edge_lines_leave_the_complete_admitted_scooter_bay_clear() {
        let plan = RoadMarkingPlan {
            road_index: 0,
            segment_index: 0,
            a: Vec2::ZERO,
            b: Vec2::new(40.0, 0.0),
            width: 12.0,
            clear_start: 1.0,
            clear_end: 1.0,
            approaches: Vec::new(),
            edge_gaps: [vec![[5.0, 7.0], [6.0, 9.0]], Vec::new()],
        };
        let parts = road_marking_geometry(&plan);
        for triangle in parts.0[&Mat::Marking].idx.chunks(3) {
            let points: Vec<_> = triangle
                .iter()
                .map(|index| parts.0[&Mat::Marking].pos[*index as usize])
                .collect();
            if points.iter().all(|p| p[2] < -5.0) {
                assert!(
                    points.iter().all(|p| p[0] <= 5.001) || points.iter().all(|p| p[0] >= 8.999)
                );
            }
        }
    }
}
