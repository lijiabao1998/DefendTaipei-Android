//! Stable, render-only architecture for the existing Wanhua footprints.
//!
//! The planner never changes a lot, door, collision segment, room fixture or world RNG.
//! A townhouse has one narrow stacked balcony; an old walkup has separated apartment
//! bays and exposed service stairs where there is room; a shophouse repeats owner bays
//! above its existing arcade. Main masses survive the Near -> Mid transition. Thin
//! rails are added only at Near, in three shared material batches. The existing
//! WindowKit owns masonry window surrounds so it cannot duplicate this layer.
//!
//! Service stairs are visual upper-floor escape assemblies, not traversable new floors.
//! Their lowest landing is at GF and their folded lower ladder stops above 2.6 metres.
//! Consequently none of this layer changes the ground-level navigation contract.

use bevy::prelude::*;

use super::build::{Lod, Mat, Parts};
use super::data::{Bld, FH, GF, RoofClass, Win, WorldData, pip, seg_dist};
use crate::geom::{self, MeshBuf, hex, place};

/// Architectural families, independent of the gameplay shop/loot classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Typology {
    Townhouse,
    Walkup,
    Shophouse,
    Other,
}

/// Existing upper facade. Local +X is `u`, local +Z is outward `n`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frontage {
    pub a: Vec2,
    pub u: Vec2,
    pub n: Vec2,
    pub width: f32,
}

impl Frontage {
    pub fn centre(self) -> Vec2 {
        self.a + self.u * (self.width * 0.5)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExteriorSpec {
    pub typology: Typology,
    pub front: Option<Frontage>,
    pub palette: usize,
    /// Stable owner/apartment rhythm; it does not create new buildings or saved IDs.
    pub bays: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Balcony {
    /// Midpoint at the existing wall plane, before the outward projection.
    pub centre: Vec2,
    pub u: Vec2,
    pub n: Vec2,
    pub width: f32,
    pub depth: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServiceStair {
    /// Midpoint on the existing rear/service wall.
    pub centre: Vec2,
    pub u: Vec2,
    pub n: Vec2,
    pub width: f32,
    pub depth: f32,
    /// Lowest landing. The visibly folded lower ladder ends above 2.6 m.
    pub bottom: f32,
    /// Highest landing, below the roof; no new walkable storeys are implied.
    pub top: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExteriorPlan {
    pub spec: ExteriorSpec,
    pub balconies: Vec<Balcony>,
    pub stair: Option<ServiceStair>,
}

/// CPU/upload limits apply to the new structural layer, not pre-existing window detail.
pub const MAX_NEAR_VERTICES: usize = 10_000;
pub const MAX_MID_VERTICES: usize = 5_000;
pub const LOWEST_STRUCTURE_Y: f32 = 2.6;

fn stable_hash(b: &Bld) -> u32 {
    // A private integer stream: adding facade variation cannot shift WorldRng, seeds,
    // shop kinds, loot, interior IDs or a neighbouring building's appearance.
    let mut h = b.id.wrapping_mul(0x9e37_79b9) ^ b.seed7 as u32 ^ 0x57a1_41d3;
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

fn edge_frame(b: &Bld, index: usize) -> Option<Frontage> {
    let a = b.pts[index];
    let c = b.pts[(index + 1) % b.pts.len()];
    let width = a.distance(c);
    if width < 0.1 {
        return None;
    }
    let edge = (c - a) / width;
    let mut n = Vec2::new(-edge.y, edge.x);
    let centre = (a + c) * 0.5;
    if pip(centre + n * 0.4, &b.pts) {
        n = -n;
    }
    let u = Vec2::new(n.y, -n.x);
    Some(Frontage {
        a: centre - u * (width * 0.5),
        u,
        n,
        width,
    })
}

/// Locate the actual existing door-facing upper wall, including arcade recess depth.
/// A narrow lot's longest edge can run along its side; it is never a new front here.
fn front(b: &Bld) -> Option<Frontage> {
    let door = b.door?;
    let recess = if b.arcade.is_some() {
        super::data::ARC
    } else {
        0.0
    };
    (0..b.pts.len())
        .filter_map(|i| edge_frame(b, i))
        .filter(|f| {
            f.n.dot(door.n) > 0.98 && ((f.centre() - door.p).dot(door.n) - recess).abs() < 0.08
        })
        .max_by(|a, c| a.width.total_cmp(&c.width))
}

pub fn classify(b: &Bld) -> Typology {
    let Some(f) = front(b) else {
        return Typology::Other;
    };
    if b.lv < 2 || b.lv > 6 || b.area > 700.0 || f.width > 34.0 || b.rcls == RoofClass::None {
        Typology::Other
    } else if b.arcade.is_some() || (b.sign.is_some() && matches!(b.kind, "store" | "pharmacy")) {
        Typology::Shophouse
    } else if f.width <= 8.5 && b.area <= 175.0 && b.lv <= 4 {
        Typology::Townhouse
    } else {
        Typology::Walkup
    }
}

pub fn specification(b: &Bld) -> ExteriorSpec {
    let front = front(b);
    let typology = classify(b);
    let width = front.map_or(0.0, |f| f.width);
    let bays = match typology {
        Typology::Townhouse => 1,
        Typology::Walkup => ((width / 4.8).round() as usize).clamp(1, 3),
        Typology::Shophouse => ((width / 4.6).round() as usize).clamp(1, 4),
        Typology::Other => 0,
    };
    ExteriorSpec {
        typology,
        front,
        palette: stable_hash(b) as usize % 4,
        bays,
    }
}

fn rect(centre: Vec2, u: Vec2, n: Vec2, width: f32, near: f32, far: f32) -> [Vec2; 4] {
    [
        centre - u * (width * 0.5) + n * near,
        centre + u * (width * 0.5) + n * near,
        centre + u * (width * 0.5) + n * far,
        centre - u * (width * 0.5) + n * far,
    ]
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let ab = b - a;
    let cd = d - c;
    let det = cross(ab, cd);
    if det.abs() < 1e-6 {
        return seg_dist(a, c, d) < 0.001
            || seg_dist(b, c, d) < 0.001
            || seg_dist(c, a, b) < 0.001
            || seg_dist(d, a, b) < 0.001;
    }
    let t = cross(c - a, cd) / det;
    let s = cross(c - a, ab) / det;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&s)
}

fn polygons_overlap(a: &[Vec2], b: &[Vec2]) -> bool {
    a.iter().any(|p| pip(*p, b))
        || b.iter().any(|p| pip(*p, a))
        || (0..a.len()).any(|i| {
            (0..b.len())
                .any(|j| segments_intersect(a[i], a[(i + 1) % a.len()], b[j], b[(j + 1) % b.len()]))
        })
}

/// Test the whole projection with a wall-thickness margin, including concave neighbour
/// footprints and road points inside long edges. A centre-only test misses both cases.
fn envelope_clear(b: &Bld, w: &WorldData, f: Frontage, width: f32, depth: f32) -> bool {
    let corners = rect(f.centre(), f.u, f.n, width + 0.28, 0.11, depth + 0.14);
    let (lo, hi) = corners.iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    for id in w.blds_in(lo - 0.2, hi + 0.2) {
        if id == b.id {
            continue;
        }
        let other = &w.blds[id as usize];
        if polygons_overlap(&corners, &other.pts)
            || other
                .pts
                .iter()
                .any(|p| (0..4).any(|i| seg_dist(*p, corners[i], corners[(i + 1) % 4]) < 0.14))
        {
            return false;
        }
    }
    let columns = (width / 0.45).ceil() as usize;
    let rows = (depth / 0.45).ceil() as usize;
    for col in 0..=columns {
        for row in 0..=rows {
            let p = f.centre()
                + f.u * (-width * 0.5 + width * col as f32 / columns.max(1) as f32)
                + f.n * (0.11 + depth * row as f32 / rows.max(1) as f32);
            if w.road_dist(p) < 0.12 {
                return false;
            }
            // Concave own-lot corners must not swallow an outward balcony or staircase.
            if row > 0 && pip(p, &b.pts) {
                return false;
            }
        }
    }
    true
}

fn service_stair(b: &Bld, w: &WorldData, spec: ExteriorSpec) -> Option<ServiceStair> {
    if spec.typology != Typology::Walkup || b.lv < 3 {
        return None;
    }
    let front = spec.front?;
    let mut edges: Vec<_> = (0..b.pts.len())
        .filter_map(|i| edge_frame(b, i).map(|edge| (i, edge)))
        .filter(|(_, f)| f.width >= 4.8 && f.n.dot(front.n) < -0.3)
        .collect();
    edges.sort_by(|(ia, a), (ib, b)| {
        a.n.dot(front.n)
            .total_cmp(&b.n.dot(front.n))
            .then_with(|| b.width.total_cmp(&a.width))
            .then_with(|| ia.cmp(ib))
    });
    for (_, edge) in edges {
        let width = 4.2;
        let depth = 2.35;
        let offset = (edge.width - width - 0.45).max(0.0) * 0.35;
        for shift in [0.0, -offset, offset] {
            let centre = edge.centre() + edge.u * shift;
            let candidate = Frontage {
                a: centre - edge.u * (width * 0.5),
                width,
                ..edge
            };
            if envelope_clear(b, w, candidate, width, depth) {
                return Some(ServiceStair {
                    centre,
                    u: edge.u,
                    n: edge.n,
                    width,
                    depth,
                    bottom: GF,
                    top: (b.h - FH).min(GF + 4.0 * FH),
                });
            }
        }
    }
    None
}

/// Accepted geometry only: QA can select a real balcony/stair fixture from this plan.
pub fn plan(b: &Bld, w: &WorldData) -> ExteriorPlan {
    let spec = specification(b);
    let stair = service_stair(b, w, spec);
    let mut balconies = Vec::new();
    if let Some(f) = spec.front
        && spec.typology != Typology::Other
        && f.width >= 3.0
        && balcony_floors(b, spec).next().is_some()
    {
        let pitch = f.width / spec.bays as f32;
        for bay in 0..spec.bays {
            let width = match spec.typology {
                Typology::Townhouse => (pitch - 0.8).min(5.6),
                Typology::Walkup => (pitch - 1.0).min(3.6),
                Typology::Shophouse => (pitch - 0.55).min(4.4),
                Typology::Other => 0.0,
            };
            if width < 2.0 {
                continue;
            }
            let centre = f.a + f.u * (pitch * (bay as f32 + 0.5));
            let candidate = Frontage {
                a: centre - f.u * (width * 0.5),
                width,
                ..f
            };
            let desired_depth = match spec.typology {
                Typology::Townhouse => 0.95,
                Typology::Walkup => 0.85,
                Typology::Shophouse => 0.75,
                Typology::Other => 0.0,
            };
            for depth in [desired_depth, 0.58, 0.38] {
                if envelope_clear(b, w, candidate, width, depth) {
                    balconies.push(Balcony {
                        centre,
                        u: f.u,
                        n: f.n,
                        width,
                        depth,
                    });
                    break;
                }
            }
        }
    }
    if stair.is_some() && balconies.len() > 2 {
        // Reserve a fixed geometry budget for the rear service core without dropping
        // its silhouette at Mid. Two apartment stacks still flank the broad facade.
        balconies.remove(1);
    }
    ExteriorPlan {
        spec,
        balconies,
        stair,
    }
}

/// A rear escape landing replaces windows within its reserved service bay. Retain all
/// RNG draws in the caller before discarding its mesh, so other windows keep their state.
pub fn window_obscured(plan: &ExteriorPlan, win: &Win) -> bool {
    let Some(stair) = plan.stair else {
        return false;
    };
    let n = Vec2::new(win.ry.sin(), win.ry.cos());
    let relative = win.p.xz() - stair.centre;
    n.dot(stair.n) > 0.98
        && relative.dot(stair.n).abs() < 0.3
        && relative.dot(stair.u).abs() < stair.width * 0.5 + 1.65
        && win.p.y > stair.bottom - 0.1
        && win.p.y < stair.top + 2.4
}

fn intersects_box(lo: Vec3, hi: Vec3, other_lo: Vec3, other_hi: Vec3) -> bool {
    lo.x < other_hi.x
        && hi.x > other_lo.x
        && lo.y < other_hi.y
        && hi.y > other_lo.y
        && lo.z < other_hi.z
        && hi.z > other_lo.z
}

/// Keep existing wall-mounted signs/AC out of newly admitted opaque balcony pieces.
/// `centre` and `normal` are world-space; `half_size` is in the attachment's local frame.
/// This admits an AC safely mounted on the wall behind a deep balcony, while rejecting
/// a blade sign or shallow-balcony compressor that actually penetrates a panel.
pub fn attachment_conflicts(
    b: &Bld,
    plan: &ExteriorPlan,
    centre: Vec3,
    half_size: Vec3,
    normal: Vec2,
) -> bool {
    let tangent = Vec2::new(normal.y, -normal.x);
    let local_bounds = |origin: Vec2, u: Vec2, n: Vec2| {
        let relative = centre.xz() - origin;
        let local = Vec3::new(relative.dot(u), centre.y, relative.dot(n));
        let half = Vec3::new(
            half_size.x * tangent.dot(u).abs() + half_size.z * normal.dot(u).abs(),
            half_size.y,
            half_size.x * tangent.dot(n).abs() + half_size.z * normal.dot(n).abs(),
        );
        (local - half, local + half)
    };
    for balcony in &plan.balconies {
        let (lo, hi) = local_bounds(balcony.centre, balcony.u, balcony.n);
        let hw = balcony.width * 0.5;
        let depth = balcony.depth;
        let wall_h = if plan.spec.typology == Typology::Walkup {
            0.54
        } else {
            0.76
        };
        for y in balcony_floors(b, plan.spec) {
            let volumes = [
                (Vec3::new(-hw, y - 0.18, 0.0), Vec3::new(hw, y, depth)),
                (
                    Vec3::new(-hw, y, depth - 0.13),
                    Vec3::new(hw, y + wall_h, depth),
                ),
                (
                    Vec3::new(-hw, y, 0.04),
                    Vec3::new(-hw + 0.13, y + wall_h, depth - 0.04),
                ),
                (
                    Vec3::new(hw - 0.13, y, 0.04),
                    Vec3::new(hw, y + wall_h, depth - 0.04),
                ),
                (
                    Vec3::new(-hw, y + 0.96, depth - 0.10),
                    Vec3::new(hw, y + 1.04, depth - 0.03),
                ),
            ];
            if volumes
                .iter()
                .any(|&(other_lo, other_hi)| intersects_box(lo, hi, other_lo, other_hi))
            {
                return true;
            }
        }
    }
    if let Some(stair) = plan.stair {
        let (lo, hi) = local_bounds(stair.centre, stair.u, stair.n);
        if intersects_box(
            lo,
            hi,
            Vec3::new(-stair.width * 0.5, LOWEST_STRUCTURE_Y, 0.1),
            Vec3::new(stair.width * 0.5, stair.top + 2.2, stair.depth),
        ) {
            return true;
        }
    }
    false
}

/// Move an owner divider into the closest actual masonry strip, rather than laying
/// a 4.6 m ownership grid over the existing 3.2 m window grid. Reserve deep WindowKit
/// surrounds as well as the glass. A crowded end bay can simply omit its pilaster.
fn divider_position(b: &Bld, f: Frontage, desired: f32, width: f32) -> Option<f32> {
    let lo = -f.width * 0.5 + width * 0.5 + 0.08;
    let hi = f.width * 0.5 - width * 0.5 - 0.08;
    if hi <= lo {
        return None;
    }
    let mut forbidden = Vec::new();
    let margin = 1.12 + width * 0.5 + 0.02;
    for win in &b.wins {
        let n = Vec2::new(win.ry.sin(), win.ry.cos());
        if n.dot(f.n) < 0.98 || win.p.y + 0.86 < GF {
            continue;
        }
        let relative = win.p.xz() - f.centre();
        if relative.dot(f.n).abs() > 0.3 {
            continue;
        }
        let x = relative.dot(f.u);
        forbidden.push((x - margin, x + margin));
    }
    let mut candidates = vec![desired.clamp(lo, hi), lo, hi];
    for &(left, right) in &forbidden {
        candidates.push(left - 0.01);
        candidates.push(right + 0.01);
    }
    candidates.retain(|x| {
        *x >= lo
            && *x <= hi
            && (*x - desired).abs() <= 1.5
            && !forbidden
                .iter()
                .any(|&(left, right)| *x > left && *x < right)
    });
    candidates.sort_by(|a, b| {
        (a - desired)
            .abs()
            .total_cmp(&(b - desired).abs())
            .then_with(|| a.total_cmp(b))
    });
    candidates.first().copied()
}

fn tile_box(w: f32, h: f32, d: f32) -> MeshBuf {
    let mut mesh = geom::cuboid(w, h, d);
    for i in 0..mesh.pos.len() {
        let p = mesh.pos[i];
        let n = mesh.nrm[i];
        mesh.uv[i] = if n[1].abs() > 0.5 {
            [p[0], p[2]]
        } else if n[0].abs() > 0.5 {
            [p[2], -p[1]]
        } else {
            [p[0], -p[1]]
        };
    }
    mesh
}

#[allow(clippy::too_many_arguments)]
fn box_at(out: &mut Parts, mat: Mat, t: Mat4, size: Vec3, p: Vec3, color: u32) {
    let mesh = if mat == Mat::FacadeTile {
        tile_box(size.x, size.y, size.z)
    } else {
        geom::cuboid(size.x, size.y, size.z)
    };
    out.add(mat, &mesh, t * Mat4::from_translation(p), hex(color));
}

fn bar(out: &mut Parts, t: Mat4, a: Vec3, b: Vec3, width: f32, color: u32) {
    let delta = b - a;
    let length = delta.length();
    if length < 0.001 {
        return;
    }
    let transform = Mat4::from_translation((a + b) * 0.5)
        * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, delta / length));
    out.add(
        Mat::Metal,
        &geom::cuboid(width, length, width),
        t * transform,
        hex(color),
    );
}

fn palette(spec: ExteriorSpec) -> (u32, u32, u32) {
    let tile = match spec.typology {
        Typology::Townhouse => [0xcbbda4, 0xbba49a, 0xb6c1b2, 0xd0cbbb],
        Typology::Walkup => [0xbdbdaf, 0xaab9aa, 0xb5a99a, 0xc6bdb2],
        Typology::Shophouse => [0xb3c1b4, 0xbba397, 0xcbc3aa, 0xa8b6b4],
        Typology::Other => [0xbdb8ab; 4],
    }[spec.palette];
    let rail = [0x656c68, 0x78645a, 0x586b60, 0x727575][spec.palette];
    (tile, 0xb6b1a5, rail)
}

fn balcony_floors(b: &Bld, spec: ExteriorSpec) -> impl Iterator<Item = f32> + '_ {
    (1..b.lv.min(6)).filter_map(move |floor| {
        let y = GF + (floor - 1) as f32 * FH;
        // The existing sign sits at GF+0.75 on an arcade. Leave that fascia visible.
        let overlaps_sign = b
            .sign
            .is_some_and(|sign| (sign.p.y - (y + 0.5)).abs() < 1.1);
        (y + 1.1 < b.h && !(spec.typology == Typology::Shophouse && overlaps_sign)).then_some(y)
    })
}

fn facade_rhythm(b: &Bld, spec: ExteriorSpec, lod: Lod, out: &mut Parts) {
    let Some(f) = spec.front else { return };
    let (tile, concrete, _) = palette(spec);
    let t = place(f.centre().x, 0.0, f.centre().y, f.n.x.atan2(f.n.y));
    let width = (f.width - 0.16).max(0.1);
    let upper = b.h - GF;
    let band_h = if spec.typology == Typology::Walkup {
        0.24
    } else {
        0.16
    };
    for floor in 0..b.lv.min(6) {
        let y = GF + floor as f32 * FH;
        if y > b.h + 0.01 {
            break;
        }
        box_at(
            out,
            Mat::VCol,
            t,
            Vec3::new(width, band_h, 0.24),
            Vec3::new(0.0, y - band_h * 0.5, 0.12),
            concrete,
        );
        if lod == Lod::Near {
            // Thin drip edge gives a readable dark underside without real-time shadows.
            box_at(
                out,
                Mat::VCol,
                t,
                Vec3::new(width, 0.045, 0.255),
                Vec3::new(0.0, y - band_h - 0.014, 0.13),
                0x777970,
            );
        }
    }
    let divider_w = match spec.typology {
        Typology::Townhouse => 0.28,
        Typology::Walkup => 0.22,
        Typology::Shophouse => 0.32,
        Typology::Other => return,
    };
    let columns = if spec.typology == Typology::Walkup {
        1
    } else {
        spec.bays
    };
    let mut placed = Vec::<f32>::new();
    for bay in 0..=columns {
        let desired =
            -width * 0.5 + divider_w * 0.5 + (width - divider_w) * bay as f32 / columns as f32;
        let Some(x) = divider_position(b, f, desired, divider_w) else {
            continue;
        };
        if placed
            .iter()
            .any(|other| (*other - x).abs() < divider_w + 0.1)
        {
            continue;
        }
        placed.push(x);
        box_at(
            out,
            Mat::FacadeTile,
            t,
            Vec3::new(divider_w, upper + 0.06, 0.16),
            Vec3::new(x, GF + upper * 0.5, 0.16),
            tile,
        );
    }
    if spec.typology == Typology::Townhouse {
        // One narrow owner cornice, deliberately different from continuous shop bays.
        box_at(
            out,
            Mat::FacadeTile,
            t,
            Vec3::new(width, 0.28, 0.36),
            Vec3::new(0.0, b.h - 0.06, 0.16),
            tile,
        );
    } else if spec.typology == Typology::Shophouse {
        // Each proprietor has a separate slightly stepped parapet/coping band.
        let pitch = width / spec.bays as f32;
        for bay in 0..spec.bays {
            let rise = if (bay + spec.palette).is_multiple_of(2) {
                0.18
            } else {
                0.06
            };
            box_at(
                out,
                Mat::FacadeTile,
                t,
                Vec3::new(pitch - 0.1, 0.32, 0.28),
                Vec3::new(-width * 0.5 + pitch * (bay as f32 + 0.5), b.h + rise, 0.13),
                tile,
            );
        }
    }
}

fn balconies(b: &Bld, plan: &ExteriorPlan, lod: Lod, out: &mut Parts) {
    let (tile, concrete, rail) = palette(plan.spec);
    for balcony in &plan.balconies {
        let t = place(
            balcony.centre.x,
            0.0,
            balcony.centre.y,
            balcony.n.x.atan2(balcony.n.y),
        );
        let (width, depth) = (balcony.width, balcony.depth);
        for y in balcony_floors(b, plan.spec) {
            box_at(
                out,
                Mat::VCol,
                t,
                Vec3::new(width, 0.18, depth),
                Vec3::new(0.0, y - 0.09, depth * 0.5),
                concrete,
            );
            let wall_h = if plan.spec.typology == Typology::Walkup {
                0.54
            } else {
                0.76
            };
            box_at(
                out,
                Mat::FacadeTile,
                t,
                Vec3::new(width, wall_h, 0.13),
                Vec3::new(0.0, y + wall_h * 0.5, depth - 0.065),
                tile,
            );
            for side in [-1.0, 1.0] {
                box_at(
                    out,
                    Mat::FacadeTile,
                    t,
                    Vec3::new(0.13, wall_h, (depth - 0.09).max(0.1)),
                    Vec3::new(side * (width * 0.5 - 0.065), y + wall_h * 0.5, depth * 0.5),
                    tile,
                );
            }
            // The top rail remains at Mid to preserve the balcony's open silhouette.
            box_at(
                out,
                Mat::Metal,
                t,
                Vec3::new(width, 0.055, 0.055),
                Vec3::new(0.0, y + 1.0, depth - 0.065),
                rail,
            );
            let posts = if lod == Lod::Near {
                ((width / 0.65).ceil() as usize).clamp(3, 7)
            } else {
                2
            };
            for post in 0..posts {
                let x = -width * 0.5 + 0.065 + (width - 0.13) * post as f32 / (posts - 1) as f32;
                box_at(
                    out,
                    Mat::Metal,
                    t,
                    Vec3::new(0.04, 1.0 - wall_h, 0.04),
                    Vec3::new(x, y + (wall_h + 1.0) * 0.5, depth - 0.065),
                    rail,
                );
            }
        }
    }
}

fn stair_geometry(stair: ServiceStair, lod: Lod, rail: u32, out: &mut Parts) {
    let t = place(
        stair.centre.x,
        0.0,
        stair.centre.y,
        stair.n.x.atan2(stair.n.y),
    );
    let left = -stair.width * 0.5 + 0.45;
    let right = stair.width * 0.5 - 0.45;
    let run = right - left;
    let lanes = [0.68, 1.72];
    let stories = ((stair.top - stair.bottom) / FH).round() as usize;
    for level in 0..=stories {
        let y = stair.bottom + level as f32 * FH;
        box_at(
            out,
            Mat::VCol,
            t,
            Vec3::new(0.85, 0.16, stair.depth - 0.14),
            Vec3::new(left - 0.02, y - 0.08, stair.depth * 0.5),
            0xb9b5a9,
        );
        // Closed service access makes each landing visibly connect to its owner.
        // These upper doors are part of the scenery, never dynamic ground doors.
        let door_x = left + 0.08;
        out.add(
            Mat::VCol,
            &geom::plane(0.92, 2.08),
            t * geom::at(door_x, y + 1.04, 0.14),
            hex(0x343d39),
        );
        out.add(
            Mat::VCol,
            &geom::plane(0.76, 1.94),
            t * geom::at(door_x, y + 1.01, 0.153),
            hex(0x67736a),
        );
        if lod == Lod::Near {
            for side in [-1.0, 1.0] {
                box_at(
                    out,
                    Mat::Metal,
                    t,
                    Vec3::new(0.055, 2.08, 0.07),
                    Vec3::new(door_x + side * 0.43, y + 1.04, 0.17),
                    0x8b9186,
                );
            }
            box_at(
                out,
                Mat::Metal,
                t,
                Vec3::new(0.92, 0.055, 0.07),
                Vec3::new(door_x, y + 2.05, 0.17),
                0x8b9186,
            );
            box_at(
                out,
                Mat::Metal,
                t,
                Vec3::new(0.11, 0.035, 0.07),
                Vec3::new(door_x + 0.24, y + 0.96, 0.21),
                0xc0bba7,
            );
        }
        // Outer guard on the full-height landing.
        bar(
            out,
            t,
            Vec3::new(left - 0.40, y + 0.9, 0.14),
            Vec3::new(left - 0.40, y + 0.9, stair.depth - 0.08),
            0.045,
            rail,
        );
        for z in [0.16, stair.depth - 0.1] {
            bar(
                out,
                t,
                Vec3::new(left - 0.40, y, z),
                Vec3::new(left - 0.40, y + 0.9, z),
                0.04,
                rail,
            );
        }
        if level == stories {
            break;
        }
        let mid = y + FH * 0.5;
        box_at(
            out,
            Mat::VCol,
            t,
            Vec3::new(0.85, 0.16, stair.depth - 0.14),
            Vec3::new(right + 0.02, mid - 0.08, stair.depth * 0.5),
            0xb9b5a9,
        );
        bar(
            out,
            t,
            Vec3::new(right + 0.4, mid + 0.9, 0.14),
            Vec3::new(right + 0.4, mid + 0.9, stair.depth - 0.08),
            0.045,
            rail,
        );
        for z in [0.16, stair.depth - 0.1] {
            bar(
                out,
                t,
                Vec3::new(right + 0.4, mid, z),
                Vec3::new(right + 0.4, mid + 0.9, z),
                0.04,
                rail,
            );
        }
        for (flight, &z) in lanes.iter().enumerate() {
            let (start, end, base) = if flight == 0 {
                (left, right, y)
            } else {
                (right, left, mid)
            };
            let steps = if lod == Lod::Near { 10 } else { 4 };
            for step in 0..steps {
                let fraction = (step as f32 + 0.5) / steps as f32;
                box_at(
                    out,
                    Mat::VCol,
                    t,
                    Vec3::new(run / steps as f32 + 0.025, 0.13, 0.91),
                    Vec3::new(
                        start + (end - start) * fraction,
                        base + (step + 1) as f32 / steps as f32 * FH * 0.5 - 0.065,
                        z,
                    ),
                    0xa5a69e,
                );
            }
            for edge in [-0.42, 0.42] {
                bar(
                    out,
                    t,
                    Vec3::new(start, base - 0.12, z + edge),
                    Vec3::new(end, base + FH * 0.5 - 0.12, z + edge),
                    0.085,
                    0x70776f,
                );
            }
            let guard_z = z + 0.47;
            bar(
                out,
                t,
                Vec3::new(start, base + 0.92, guard_z),
                Vec3::new(end, base + FH * 0.5 + 0.92, guard_z),
                0.045,
                rail,
            );
            let posts = if lod == Lod::Near { 4 } else { 2 };
            for post in 0..posts {
                let f = post as f32 / (posts - 1) as f32;
                let p = Vec3::new(start + (end - start) * f, base + FH * 0.5 * f, guard_z);
                bar(out, t, p, p + Vec3::Y * 0.92, 0.04, rail);
            }
        }
    }
    // A short folded escape ladder is attached under the lowest landing. It visibly
    // stops in the air and is never a stair-shaped obstacle on the ground route.
    let low = Vec3::new(left - 0.05, 2.7, 0.35);
    let high = Vec3::new(left - 0.05, stair.bottom - 0.10, 0.85);
    for side in [-0.25, 0.25] {
        bar(
            out,
            t,
            low + Vec3::X * side,
            high + Vec3::X * side,
            0.045,
            rail,
        );
    }
    for rung in 0..5 {
        let p = low.lerp(high, rung as f32 / 4.0);
        bar(out, t, p - Vec3::X * 0.25, p + Vec3::X * 0.25, 0.04, rail);
    }
}

/// Shared structural facade layer, hidden with `BldDetail` during a Near cutaway.
pub fn structure(b: &Bld, world: &WorldData, lod: Lod) -> Parts {
    let mut out = Parts::default();
    if lod == Lod::Far {
        return out;
    }
    let plan = plan(b, world);
    if plan.spec.typology == Typology::Other {
        return out;
    }
    facade_rhythm(b, plan.spec, lod, &mut out);
    balconies(b, &plan, lod, &mut out);
    if let Some(stair) = plan.stair {
        stair_geometry(stair, lod, palette(plan.spec).2, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same_meshes(a: &Parts, b: &Parts) -> bool {
        a.0.len() == b.0.len()
            && a.0.iter().all(|(mat, mesh)| {
                b.0.get(mat).is_some_and(|other| {
                    mesh.pos == other.pos
                        && mesh.nrm == other.nrm
                        && mesh.uv == other.uv
                        && mesh.col == other.col
                        && mesh.idx == other.idx
                })
            })
    }

    #[test]
    fn real_world_has_all_three_architectural_families_and_admitted_features() {
        let world = super::super::world();
        let mut families = [0; 3];
        let mut balcony_families = [0; 3];
        let mut stairs = 0;
        for b in &world.blds {
            let plan = plan(b, world);
            let index = match plan.spec.typology {
                Typology::Townhouse => 0,
                Typology::Walkup => 1,
                Typology::Shophouse => 2,
                Typology::Other => continue,
            };
            families[index] += 1;
            if !plan.balconies.is_empty() && balcony_floors(b, plan.spec).next().is_some() {
                balcony_families[index] += 1;
            }
            stairs += usize::from(plan.stair.is_some());
            let f = plan.spec.front.unwrap();
            assert!((f.u.dot(f.n)).abs() < 1e-4);
            assert!((f.u.length() - 1.0).abs() < 1e-4);
            assert!(f.n.dot(b.door.unwrap().n) > 0.98);
        }
        assert!(families.iter().all(|n| *n >= 10), "families {families:?}");
        assert!(
            balcony_families.iter().all(|n| *n >= 5),
            "balconies {balcony_families:?}"
        );
        assert!(stairs >= 5, "only {stairs} service stair fixtures");
    }

    #[test]
    fn every_real_projection_has_clear_full_footprint_and_no_ground_obstruction() {
        let world = super::super::world();
        for b in &world.blds {
            let plan = plan(b, world);
            for balcony in &plan.balconies {
                let f = Frontage {
                    a: balcony.centre - balcony.u * (balcony.width * 0.5),
                    u: balcony.u,
                    n: balcony.n,
                    width: balcony.width,
                };
                assert!(
                    envelope_clear(b, world, f, balcony.width, balcony.depth),
                    "balcony {}",
                    b.id
                );
            }
            if let Some(stair) = plan.stair {
                let f = Frontage {
                    a: stair.centre - stair.u * (stair.width * 0.5),
                    u: stair.u,
                    n: stair.n,
                    width: stair.width,
                };
                assert!(
                    envelope_clear(b, world, f, stair.width, stair.depth),
                    "stair {}",
                    b.id
                );
                assert!(stair.n.dot(plan.spec.front.unwrap().n) < -0.3);
                assert!(stair.bottom >= GF && stair.top <= b.h - FH + 0.01);
            }
            let mesh = structure(b, world, Lod::Near);
            for part in mesh.0.values() {
                assert!(
                    part.pos.iter().all(|p| p[1] >= LOWEST_STRUCTURE_Y),
                    "ground route {}",
                    b.id
                );
            }
        }
    }

    #[test]
    fn balcony_slab_corners_survive_mid_lod_and_stairs_do_not_mask_front_windows() {
        let world = super::super::world();
        let contains = |parts: &Parts, expected: Vec3| {
            parts.0.get(&Mat::VCol).is_some_and(|mesh| {
                mesh.pos
                    .iter()
                    .any(|p| Vec3::from(*p).distance_squared(expected) < 1e-6)
            })
        };
        let mut checked = 0;
        for b in &world.blds {
            let plan = plan(b, world);
            if plan.balconies.is_empty() {
                continue;
            }
            let near = structure(b, world, Lod::Near);
            let mid = structure(b, world, Lod::Mid);
            for balcony in &plan.balconies {
                for y in balcony_floors(b, plan.spec) {
                    for side in [-1.0, 1.0] {
                        let xz = balcony.centre
                            + balcony.u * (side * balcony.width * 0.5)
                            + balcony.n * balcony.depth;
                        for height in [y, y - 0.18] {
                            let corner = Vec3::new(xz.x, height, xz.y);
                            assert!(contains(&near, corner), "missing Near slab {}", b.id);
                            assert!(contains(&mid, corner), "missing Mid slab {}", b.id);
                        }
                    }
                    checked += 1;
                }
            }
            let f = plan.spec.front.unwrap();
            for win in &b.wins {
                let normal = Vec2::new(win.ry.sin(), win.ry.cos());
                if normal.dot(f.n) > 0.98 {
                    assert!(!window_obscured(&plan, win));
                }
            }
        }
        assert!(checked > 50);
    }

    #[test]
    fn tiled_owner_dividers_leave_real_window_openings_and_surrounds_clear() {
        let world = super::super::world();
        let mut checked = 0;
        for b in &world.blds {
            let Some(front) = specification(b).front else {
                continue;
            };
            for lod in [Lod::Near, Lod::Mid] {
                let parts = structure(b, world, lod);
                let Some(tile) = parts.0.get(&Mat::FacadeTile) else {
                    continue;
                };
                for win in &b.wins {
                    let normal = Vec2::new(win.ry.sin(), win.ry.cos());
                    if normal.dot(front.n) < 0.98 {
                        continue;
                    }
                    let inverse = place(win.p.x, win.p.y, win.p.z, win.ry).inverse();
                    let points: Vec<_> = tile
                        .pos
                        .iter()
                        .map(|p| inverse.transform_point3(Vec3::from(*p)))
                        .collect();
                    for triangle in tile.idx.as_chunks::<3>().0 {
                        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                        for &index in triangle {
                            lo = lo.min(points[index as usize]);
                            hi = hi.max(points[index as usize]);
                        }
                        // Independently examine emitted triangles in the actual window
                        // frame. This catches a solid pilaster through a painted pane
                        // even when the facade planner and its own keepout agree.
                        let blocks_opening = lo.x < 1.02
                            && hi.x > -1.02
                            && lo.y < 0.85
                            && hi.y > -0.85
                            && lo.z < 0.24
                            && hi.z > -0.01;
                        assert!(
                            !blocks_opening,
                            "opaque tiled facade crosses window {} {lod:?}: {lo:?}..{hi:?}",
                            b.id,
                        );
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 100);
    }

    #[test]
    fn attachment_keepout_distinguishes_panel_collision_from_clear_balcony_interior() {
        let world = super::super::world();
        let mut checked = 0;
        for b in &world.blds {
            let plan = plan(b, world);
            let Some(y) = balcony_floors(b, plan.spec).next() else {
                continue;
            };
            for balcony in plan.balconies.iter().filter(|p| p.depth > 0.7) {
                let point = |z: f32| {
                    let xz = balcony.centre + balcony.n * z;
                    Vec3::new(xz.x, y + 0.3, xz.y)
                };
                assert!(attachment_conflicts(
                    b,
                    &plan,
                    point(balcony.depth - 0.06),
                    Vec3::splat(0.07),
                    balcony.n,
                ));
                assert!(!attachment_conflicts(
                    b,
                    &plan,
                    point(0.27),
                    Vec3::splat(0.07),
                    balcony.n,
                ));
                checked += 1;
            }
        }
        assert!(checked > 10);
    }

    #[test]
    fn structural_meshes_are_bounded_finite_deterministic_and_lod_safe() {
        let world = super::super::world();
        let mut strict_reductions = 0;
        for b in &world.blds {
            let near = structure(b, world, Lod::Near);
            let mid = structure(b, world, Lod::Mid);
            assert!(
                near.vcount() <= MAX_NEAR_VERTICES,
                "Near building {}: {}",
                b.id,
                near.vcount()
            );
            assert!(
                mid.vcount() <= MAX_MID_VERTICES,
                "Mid building {}: {}",
                b.id,
                mid.vcount()
            );
            assert!(mid.vcount() <= near.vcount());
            strict_reductions += usize::from(mid.vcount() < near.vcount());
            assert_eq!(structure(b, world, Lod::Far).vcount(), 0);
            assert!(near.0.len() <= 3 && mid.0.len() <= 3);
            for (material, mesh) in near.0.iter().chain(mid.0.iter()) {
                assert!(matches!(material, Mat::VCol | Mat::Metal | Mat::FacadeTile));
                assert_eq!(mesh.pos.len(), mesh.nrm.len());
                assert_eq!(mesh.pos.len(), mesh.uv.len());
                assert_eq!(mesh.pos.len(), mesh.col.len());
                assert!(mesh.pos.iter().all(|p| Vec3::from(*p).is_finite()));
                assert!(mesh.nrm.iter().all(|p| Vec3::from(*p).is_finite()));
                assert!(mesh.idx.iter().all(|i| (*i as usize) < mesh.pos.len()));
            }
            // Re-generate representative complete fixtures, independent of HashMap order.
            if b.id % 31 == 0 && near.vcount() > 0 {
                assert_eq!(plan(b, world), plan(b, world));
                assert!(same_meshes(&near, &structure(b, world, Lod::Near)));
            }
        }
        assert!(strict_reductions > 30);
    }

    #[test]
    fn overlap_test_rejects_crossed_edges_and_contained_concave_neighbours() {
        let a = [
            Vec2::new(-2.0, -0.5),
            Vec2::new(2.0, -0.5),
            Vec2::new(2.0, 0.5),
            Vec2::new(-2.0, 0.5),
        ];
        let crossed = [
            Vec2::new(-0.3, -2.0),
            Vec2::new(0.3, -2.0),
            Vec2::new(0.3, 2.0),
            Vec2::new(-0.3, 2.0),
        ];
        assert!(polygons_overlap(&a, &crossed));
        let outside = [
            Vec2::new(3.0, 2.0),
            Vec2::new(4.0, 2.0),
            Vec2::new(4.0, 3.0),
            Vec2::new(3.0, 3.0),
        ];
        assert!(!polygons_overlap(&a, &outside));
    }
}
