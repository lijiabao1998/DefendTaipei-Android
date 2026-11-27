//! Deterministic, walkable ground-floor furnishings inspired by Web v24.42.0
//! `map/interiors.ts`. Furniture, collision and scavenging share one catalogue.
//! A bounded near layer batches each room into one body and one stock draw.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use bevy::prelude::*;

use super::data::{Bld, CELL, Seg, WorldData, pip, seg_dist};
use crate::game_flow::Campaign;
use crate::geom::{self, MeshBuf, hex};
use crate::player::{PLAYER_RADIUS, Player};
use crate::survival::Stash;
use crate::worldplay::ScavengeState;

const MAX_ROOM_FIXTURES: usize = 7;
const MAX_LIVE_ROOMS: usize = 10;
const LIVE_RANGE: f32 = 44.0;
const MAX_PRODUCTS: usize = 18;
const FRONT_CLEARANCE: f32 = 0.64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FixtureKind {
    Shelf,
    Fridge,
    Counter,
    Medicine,
    Locker,
    Wardrobe,
    Kitchen,
    Altar,
    Bed,
    Sofa,
    Table,
}

impl FixtureKind {
    /// Width, height, depth in metres; local +Z is the accessible front.
    pub fn size(self) -> Vec3 {
        match self {
            Self::Shelf => Vec3::new(1.8, 1.8, 0.55),
            Self::Fridge => Vec3::new(1.5, 2.05, 0.72),
            Self::Counter => Vec3::new(1.6, 1.15, 0.7),
            Self::Medicine => Vec3::new(1.4, 1.85, 0.48),
            Self::Locker => Vec3::new(1.35, 1.9, 0.6),
            Self::Wardrobe => Vec3::new(1.35, 1.95, 0.6),
            Self::Kitchen => Vec3::new(1.7, 1.0, 0.65),
            Self::Altar => Vec3::new(1.3, 1.9, 0.85),
            Self::Bed => Vec3::new(1.45, 0.72, 2.0),
            Self::Sofa => Vec3::new(1.9, 0.95, 0.72),
            Self::Table => Vec3::new(1.25, 0.85, 1.1),
        }
    }

    fn loot(self) -> Option<&'static str> {
        Some(match self {
            Self::Shelf => "shelf",
            Self::Fridge => "fridge",
            Self::Counter => "counter",
            Self::Medicine => "medshelf",
            Self::Locker => "armory",
            Self::Wardrobe => "wardrobe",
            Self::Kitchen => "kitchen",
            Self::Altar => "altar",
            Self::Bed | Self::Sofa | Self::Table => return None,
        })
    }

    fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Shelf => ("泡麵零食貨架", "Noodles and snacks shelf"),
            Self::Fridge => ("飲料冷藏櫃", "Drinks refrigerator"),
            Self::Counter => ("結帳櫃台", "Checkout counter"),
            Self::Medicine => ("藥品保健櫃", "Medicine cabinet"),
            Self::Locker => ("警用裝備櫃", "Police equipment locker"),
            Self::Wardrobe => ("木製衣櫃", "Wooden wardrobe"),
            Self::Kitchen => ("流理臺與廚櫃", "Kitchen cupboards"),
            Self::Altar => ("神明桌與公媽龕", "Family shrine"),
            Self::Bed => ("床鋪", "Bed"),
            Self::Sofa => ("木沙發", "Wooden sofa"),
            Self::Table => ("餐桌與紅板凳", "Dining table and stools"),
        }
    }
}

/// The ID and approach are shared with the loot panel. First loot fixture keeps
/// native legacy `c{id}`; later IDs use a fixed layout slot, never a live entity.
#[derive(Clone, Debug)]
pub struct Fixture {
    pub id: String,
    pub building: u32,
    pub pos: Vec2,
    pub approach: Vec2,
    pub front: Vec2,
    pub kind: FixtureKind,
    pub loot: Option<&'static str>,
    pub label_zh: &'static str,
    pub label_en: &'static str,
}

impl Fixture {
    pub fn tangent(&self) -> Vec2 {
        Vec2::new(self.front.y, -self.front.x)
    }

    pub fn corners(&self) -> [Vec2; 4] {
        let s = self.kind.size();
        let u = self.tangent() * (s.x * 0.5);
        let n = self.front * (s.z * 0.5);
        [
            self.pos - u - n,
            self.pos + u - n,
            self.pos + u + n,
            self.pos - u + n,
        ]
    }

    fn contains(&self, p: Vec2, margin: f32) -> bool {
        let d = p - self.pos;
        let size = self.kind.size();
        d.dot(self.tangent()).abs() < size.x * 0.5 + margin
            && d.dot(self.front).abs() < size.z * 0.5 + margin
    }

    pub fn segments(&self) -> impl Iterator<Item = Seg> + '_ {
        let cs = self.corners();
        (0..4).map(move |i| Seg {
            a: cs[i],
            b: cs[(i + 1) % 4],
            h: Some(self.kind.size().y),
            // Furniture is cover, not an exterior wall / window aperture.
            bld: None,
        })
    }

    fn transform(&self) -> Mat4 {
        geom::place(
            self.pos.x,
            0.04,
            self.pos.y,
            self.front.x.atan2(self.front.y),
        )
    }
}

fn circle_inside(b: &Bld, p: Vec2, r: f32) -> bool {
    pip(p, &b.pts)
        && b.pts
            .iter()
            .enumerate()
            .all(|(i, a)| seg_dist(p, *a, b.pts[(i + 1) % b.pts.len()]) >= r)
}

// Full indexed building/arcade/prop geometry, excluding furniture itself when
// re-planning an already built world in tests. Ownership also prevents nested
// or overlapping OSM footprints from furnishing the same strip of floor twice.
fn usable_floor(world: &WorldData, b: &Bld, p: Vec2, radius: f32) -> bool {
    if !circle_inside(b, p, radius)
        || world.bld_at(p).is_none_or(|owner| owner.id != b.id)
        || p.x < world.min.x + radius
        || p.y < world.min.y + radius
        || p.x > world.max.x - radius
        || p.y > world.max.y - radius
    {
        return false;
    }
    let cell = (p / CELL).floor().as_ivec2();
    for x in cell.x - 1..=cell.x + 1 {
        for y in cell.y - 1..=cell.y + 1 {
            for &id in world.segs_in_cell((x, y)) {
                if id as usize >= world.interior_seg_start {
                    continue;
                }
                let seg = &world.segs[id as usize];
                if seg_dist(p, seg.a, seg.b) < radius {
                    return false;
                }
            }
        }
    }
    true
}

fn route_hits(fixture: &Fixture, from: Vec2, to: Vec2, radius: f32) -> bool {
    let delta = to - from;
    let size = fixture.kind.size();
    let mut entry = 0.0_f32;
    let mut exit = 1.0_f32;
    for (axis, half) in [
        (fixture.tangent(), size.x * 0.5 + radius),
        (fixture.front, size.z * 0.5 + radius),
    ] {
        let start = (from - fixture.pos).dot(axis);
        let step = delta.dot(axis);
        if step.abs() < 1e-6 {
            if start.abs() >= half {
                return false;
            }
        } else {
            let a = (-half - start) / step;
            let b = (half - start) / step;
            entry = entry.max(a.min(b));
            exit = exit.min(a.max(b));
            if entry >= exit {
                return false;
            }
        }
    }
    entry < exit && exit > 0.0 && entry < 1.0
}

fn clear_route(
    world: &WorldData,
    b: &Bld,
    from: Vec2,
    to: Vec2,
    fixtures: &[Fixture],
    candidate: &Fixture,
) -> bool {
    let radius = PLAYER_RADIUS + 0.1;
    if route_hits(candidate, from, to, radius)
        || fixtures.iter().any(|f| route_hits(f, from, to, radius))
    {
        return false;
    }
    // 10 cm samples + 10 cm padding retain at least 5 cm between the
    // player's capsule and even a narrow real wall corner between samples.
    let steps = (from.distance(to) / 0.1).ceil().max(1.0) as usize;
    (0..=steps).all(|i| usable_floor(world, b, from.lerp(to, i as f32 / steps as f32), radius))
}

fn overlaps(a: &Fixture, b: &Fixture, gap: f32) -> bool {
    let ac = a.corners();
    let bc = b.corners();
    for axis in [a.front, a.tangent(), b.front, b.tangent()] {
        let range = |cs: &[Vec2; 4]| {
            cs.iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                    let d = p.dot(axis);
                    (lo.min(d), hi.max(d))
                })
        };
        let (alo, ahi) = range(&ac);
        let (blo, bhi) = range(&bc);
        if ahi + gap <= blo || bhi + gap <= alo {
            return false;
        }
    }
    true
}

fn fits(world: &WorldData, b: &Bld, f: &Fixture, existing: &[Fixture]) -> bool {
    let cs = f.corners();
    // Check complete edges, not just corners: OSM footprints may be concave.
    let contained = (0..4).all(|edge| {
        let a = cs[edge];
        let z = cs[(edge + 1) % 4];
        let n = (a.distance(z) / 0.2).ceil().max(1.0) as usize;
        (0..=n).all(|i| usable_floor(world, b, a.lerp(z, i as f32 / n as f32), 0.16))
    });
    if !contained || existing.iter().any(|e| overlaps(f, e, 0.24)) {
        return false;
    }
    let door = b.door.unwrap();
    let anchor = door.p - door.n * 1.4;
    // Every approach retains a direct capsule-wide route to the entrance.
    // This also protects the home tap and both swinging door leaves.
    clear_route(world, b, anchor, f.approach, existing, f)
        && existing
            .iter()
            .all(|e| clear_route(world, b, anchor, e.approach, existing, f))
        && !f.contains(door.p, 1.1)
}

/// A fixed layout seed does not depend on render order, stream distance or loot.
/// Narrow/irregular buildings that cannot fit safe furniture retain the legacy
/// fallback crate through worldplay instead of adding unreachable containers.
pub fn plan_room(world: &WorldData, b: &Bld) -> Vec<Fixture> {
    let Some(door) = b.door else {
        return Vec::new();
    };
    let anchor = door.p - door.n * 1.4;
    if !usable_floor(world, b, anchor, PLAYER_RADIUS + 0.06) {
        return Vec::new();
    }
    let u = door.tangent();
    let inward = -door.n;
    let (mut left, mut right, mut depth) = (0.0_f32, 0.0_f32, 0.0_f32);
    for p in &b.pts {
        let d = *p - door.p;
        left = left.min(d.dot(u));
        right = right.max(d.dot(u));
        depth = depth.max(d.dot(inward));
    }
    let kinds: &[FixtureKind] = match b.kind {
        "store" | "convenience" | "market" => &[
            FixtureKind::Shelf,
            FixtureKind::Fridge,
            FixtureKind::Counter,
            FixtureKind::Shelf,
            FixtureKind::Fridge,
            FixtureKind::Shelf,
        ],
        "pharmacy" | "hospital" | "clinic" => &[
            FixtureKind::Medicine,
            FixtureKind::Medicine,
            FixtureKind::Counter,
            FixtureKind::Medicine,
            FixtureKind::Fridge,
        ],
        "police" => &[
            FixtureKind::Locker,
            FixtureKind::Counter,
            FixtureKind::Locker,
            FixtureKind::Medicine,
            FixtureKind::Table,
        ],
        "food" | "restaurant" | "breakfast" => &[
            FixtureKind::Kitchen,
            FixtureKind::Fridge,
            FixtureKind::Counter,
            FixtureKind::Table,
            FixtureKind::Table,
        ],
        _ => &[
            FixtureKind::Wardrobe,
            FixtureKind::Kitchen,
            FixtureKind::Altar,
            FixtureKind::Sofa,
            FixtureKind::Bed,
            FixtureKind::Table,
        ],
    };
    let mut out = Vec::new();
    let mut has_legacy = false;
    for (slot, &kind) in kinds.iter().enumerate().take(MAX_ROOM_FIXTURES) {
        let size = kind.size();
        let mut candidates = Vec::new();
        for side in [-1.0_f32, 1.0] {
            let bound = if side < 0.0 { -left } else { right };
            let min_x = 1.02 + size.z * 0.5;
            let max_x = (bound - size.z * 0.5 - 0.22).min(9.0);
            if max_x < min_x {
                continue;
            }
            for xi in 0..=4 {
                let x = side * (max_x - (max_x - min_x) * xi as f32 / 4.0);
                let first_v = (size.x * 0.5 + 1.0).max(2.2);
                let last_v = (depth - size.x * 0.5 - 0.25).min(15.0);
                let mut v = first_v;
                while v <= last_v {
                    let pos = door.p + u * x + inward * v;
                    let front = u * -side;
                    let (label_zh, label_en) = kind.labels();
                    let loot = kind.loot();
                    let id = if loot.is_some() && !has_legacy {
                        format!("c{}", b.id)
                    } else {
                        format!("x{}_{}", b.id, slot)
                    };
                    let fixture = Fixture {
                        id,
                        building: b.id,
                        pos,
                        approach: pos + front * (size.z * 0.5 + FRONT_CLEARANCE),
                        front,
                        kind,
                        loot,
                        label_zh,
                        label_en,
                    };
                    let preferred_side = if slot % 2 == 0 { -1.0 } else { 1.0 };
                    let preferred_depth = 2.4 + (slot / 2) as f32 * 2.5;
                    let score = (v - preferred_depth).abs()
                        + if side == preferred_side { 0.0 } else { 1.4 }
                        + xi as f32 * 0.28;
                    candidates.push((score, fixture));
                    v += 0.65;
                }
            }
        }
        candidates.sort_by(|a, z| a.0.total_cmp(&z.0));
        if let Some((_, fixture)) = candidates
            .into_iter()
            .find(|(_, f)| fits(world, b, f, &out))
        {
            has_legacy |= fixture.loot.is_some();
            out.push(fixture);
        }
    }
    out
}

/// Add static solid furniture to exactly the collision index used by players,
/// civilians, soldiers, melee and height-aware firearm rays.
pub fn install(w: &mut WorldData) {
    let fixtures: Vec<_> = w.blds.iter().flat_map(|b| plan_room(w, b)).collect();
    w.interior_seg_start = w.segs.len();
    for fixture in &fixtures {
        for segment in fixture.segments() {
            w.add_seg(segment);
        }
    }
    w.interiors = fixtures;
}

/// Preserve a valid save coordinate exactly. Only an overlap with newly added
/// furniture is relocated, to the nearest clear point of the same room. This
/// runs before door threshold protection during Continue.
pub fn restore_position(world: &WorldData, position: Vec2) -> Vec2 {
    let radius = PLAYER_RADIUS + 0.025;
    let intersecting: Vec<_> = world
        .interiors
        .iter()
        .filter(|f| f.contains(position, radius))
        .collect();
    if intersecting.is_empty() {
        return position;
    }
    let mut candidates = Vec::new();
    for fixture in &intersecting {
        let Some(building) = world.blds.iter().find(|b| b.id == fixture.building) else {
            continue;
        };
        let size = fixture.kind.size();
        let tangent = fixture.tangent();
        // Front is guaranteed connected to the entrance by the placement rule.
        candidates.push((fixture.approach, building));
        for axis in [fixture.front, -fixture.front, tangent, -tangent] {
            let half = if axis.dot(fixture.front).abs() > 0.5 {
                size.z * 0.5
            } else {
                size.x * 0.5
            };
            candidates.push((fixture.pos + axis * (half + FRONT_CLEARANCE), building));
        }
    }
    candidates.sort_by(|(a, _), (b, _)| {
        a.distance_squared(position)
            .total_cmp(&b.distance_squared(position))
    });
    candidates
        .into_iter()
        .find_map(|(candidate, building)| {
            (circle_inside(building, candidate, radius)
                && world.free(candidate, radius)
                && world
                    .interiors
                    .iter()
                    .all(|f| !f.contains(candidate, radius)))
            .then_some(candidate)
        })
        .unwrap_or(position)
}

fn box_part(out: &mut MeshBuf, size: Vec3, p: Vec3, color: u32) {
    out.add(
        &geom::cuboid(size.x, size.y, size.z),
        Mat4::from_translation(p),
        hex(color),
    );
}

fn cylinder_part(out: &mut MeshBuf, r: f32, height: f32, p: Vec3, color: u32) {
    out.add(
        &geom::cylinder(r, r, height, 8),
        Mat4::from_translation(p),
        hex(color),
    );
}

fn body_mesh(kind: FixtureKind) -> MeshBuf {
    let mut m = MeshBuf::default();
    let s = kind.size();
    let (w, d, h) = (s.x, s.z, s.y);
    let wood = 0x805837;
    let dark = 0x452f22;
    let steel = 0xa3aca6;
    match kind {
        FixtureKind::Shelf | FixtureKind::Medicine | FixtureKind::Fridge => {
            let fridge = kind == FixtureKind::Fridge;
            let pharmacy = kind == FixtureKind::Medicine;
            let frame = if fridge {
                0x35443e
            } else if pharmacy {
                0xd6d8ca
            } else {
                0xc0c7bd
            };
            box_part(
                &mut m,
                Vec3::new(w, h, 0.06),
                Vec3::new(0.0, h * 0.5, -d * 0.5 + 0.03),
                if fridge { 0xd2d9cc } else { 0xd4ccb3 },
            );
            for x in [-w * 0.5 + 0.035, w * 0.5 - 0.035] {
                box_part(
                    &mut m,
                    Vec3::new(0.07, h, d),
                    Vec3::new(x, h * 0.5, 0.0),
                    frame,
                );
            }
            for y in [0.14, 0.57, 1.0, 1.43] {
                box_part(
                    &mut m,
                    Vec3::new(w, 0.045, d),
                    Vec3::new(0.0, y, 0.0),
                    frame,
                );
                box_part(
                    &mut m,
                    Vec3::new(w - 0.1, 0.07, 0.025),
                    Vec3::new(0.0, y - 0.01, d * 0.5),
                    if pharmacy { 0x277d65 } else { 0xa43e30 },
                );
                for i in 0..4 {
                    box_part(
                        &mut m,
                        Vec3::new(0.14, 0.045, 0.008),
                        Vec3::new(-w * 0.37 + i as f32 * w * 0.245, y - 0.005, d * 0.5 + 0.016),
                        0xece0b5,
                    );
                }
            }
            box_part(
                &mut m,
                Vec3::new(w, 0.14, d),
                Vec3::new(0.0, h - 0.07, 0.0),
                if pharmacy { 0x277d65 } else { frame },
            );
            if fridge {
                box_part(
                    &mut m,
                    Vec3::new(0.035, h - 0.2, 0.045),
                    Vec3::new(0.0, h * 0.5, d * 0.5),
                    steel,
                );
                for x in [-0.08, 0.08] {
                    box_part(
                        &mut m,
                        Vec3::new(0.025, 0.35, 0.06),
                        Vec3::new(x, 1.1, d * 0.5 + 0.02),
                        0xe1e0d3,
                    );
                }
                for x in [-0.5, -0.25, 0.0, 0.25, 0.5] {
                    box_part(
                        &mut m,
                        Vec3::new(0.15, 0.025, 0.012),
                        Vec3::new(x, 0.055, d * 0.5 + 0.01),
                        0x1e2927,
                    );
                }
            }
        }
        FixtureKind::Wardrobe | FixtureKind::Locker => {
            let locker = kind == FixtureKind::Locker;
            let paint = if locker { 0x52615b } else { wood };
            box_part(
                &mut m,
                Vec3::new(w, h, d),
                Vec3::new(0.0, h * 0.5, 0.0),
                paint,
            );
            box_part(
                &mut m,
                Vec3::new(0.025, h - 0.08, 0.018),
                Vec3::new(0.0, h * 0.5, d * 0.5 + 0.012),
                dark,
            );
            for x in [-w * 0.25, w * 0.25] {
                box_part(
                    &mut m,
                    Vec3::new(w * 0.43, h - 0.17, 0.025),
                    Vec3::new(x, h * 0.5, d * 0.5 + 0.025),
                    if locker { 0x657066 } else { 0x9b7048 },
                );
                box_part(
                    &mut m,
                    Vec3::new(0.035, 0.16, 0.07),
                    Vec3::new(x * 0.32, 1.0, d * 0.5 + 0.07),
                    0xb89a55,
                );
                if locker {
                    for y in [1.48, 1.57, 1.66] {
                        box_part(
                            &mut m,
                            Vec3::new(w * 0.3, 0.025, 0.008),
                            Vec3::new(x, y, d * 0.5 + 0.043),
                            0x27352f,
                        );
                    }
                }
            }
        }
        FixtureKind::Counter => {
            box_part(
                &mut m,
                Vec3::new(w, 0.82, d),
                Vec3::new(0.0, 0.41, 0.0),
                0xb5b5a2,
            );
            box_part(
                &mut m,
                Vec3::new(w + 0.04, 0.065, d + 0.02),
                Vec3::new(0.0, 0.84, 0.0),
                0x344a3a,
            );
            box_part(
                &mut m,
                Vec3::new(0.42, 0.09, 0.32),
                Vec3::new(w * 0.25, 0.91, 0.02),
                0xbbb9a6,
            );
            box_part(
                &mut m,
                Vec3::new(0.32, 0.2, 0.16),
                Vec3::new(w * 0.25, 1.04, -0.07),
                0x3b443e,
            );
            box_part(
                &mut m,
                Vec3::new(0.26, 0.13, 0.015),
                Vec3::new(w * 0.25, 1.05, 0.018),
                0x1c2d29,
            );
            box_part(
                &mut m,
                Vec3::new(w * 0.68, 0.2, 0.015),
                Vec3::new(-0.08, 0.52, d * 0.5 + 0.013),
                0x8e3d29,
            );
            for x in [-0.25, -0.08, 0.09] {
                box_part(
                    &mut m,
                    Vec3::new(0.07, 0.025, 0.015),
                    Vec3::new(x, 0.52, d * 0.5 + 0.025),
                    0xe1d5ae,
                );
            }
        }
        FixtureKind::Kitchen => {
            box_part(
                &mut m,
                Vec3::new(w, 0.82, d),
                Vec3::new(0.0, 0.41, 0.0),
                0xa4b9ad,
            );
            box_part(
                &mut m,
                Vec3::new(w + 0.03, 0.04, d),
                Vec3::new(0.0, 0.86, 0.0),
                steel,
            );
            for x in [-w * 0.33, 0.0, w * 0.33] {
                box_part(
                    &mut m,
                    Vec3::new(w * 0.29, 0.66, 0.02),
                    Vec3::new(x, 0.43, d * 0.5 + 0.012),
                    0x607e70,
                );
                box_part(
                    &mut m,
                    Vec3::new(0.11, 0.03, 0.035),
                    Vec3::new(x, 0.68, d * 0.5 + 0.035),
                    0xd8d6c1,
                );
            }
            box_part(
                &mut m,
                Vec3::new(0.55, 0.025, 0.4),
                Vec3::new(-0.42, 0.89, 0.01),
                0x505f5a,
            );
            box_part(
                &mut m,
                Vec3::new(0.42, 0.028, 0.29),
                Vec3::new(-0.42, 0.9, 0.01),
                0x85958b,
            );
            box_part(
                &mut m,
                Vec3::new(0.035, 0.25, 0.035),
                Vec3::new(-0.42, 0.98, -0.21),
                steel,
            );
            box_part(
                &mut m,
                Vec3::new(0.035, 0.035, 0.16),
                Vec3::new(-0.42, 1.1, -0.14),
                steel,
            );
            for x in [0.25, 0.62] {
                cylinder_part(&mut m, 0.13, 0.025, Vec3::new(x, 0.9, 0.0), 0x2e3430);
                cylinder_part(&mut m, 0.08, 0.026, Vec3::new(x, 0.92, 0.0), 0x61746a);
            }
        }
        FixtureKind::Altar => {
            box_part(
                &mut m,
                Vec3::new(w, 1.02, d * 0.62),
                Vec3::new(0.0, 0.51, -0.12),
                dark,
            );
            box_part(
                &mut m,
                Vec3::new(w + 0.04, 0.06, d * 0.72),
                Vec3::new(0.0, 1.05, -0.12),
                0x742e25,
            );
            box_part(
                &mut m,
                Vec3::new(w * 0.9, 0.72, 0.075),
                Vec3::new(0.0, 1.52, -d * 0.43),
                0x721e1b,
            );
            for x in [-w * 0.4, w * 0.4] {
                box_part(
                    &mut m,
                    Vec3::new(0.025, 0.64, 0.018),
                    Vec3::new(x, 1.53, -d * 0.37),
                    0xc8a457,
                );
            }
            box_part(
                &mut m,
                Vec3::new(w * 0.78, 0.64, d * 0.52),
                Vec3::new(0.0, 0.32, d * 0.2),
                0x9a6541,
            );
            box_part(
                &mut m,
                Vec3::new(w * 0.82, 0.04, d * 0.56),
                Vec3::new(0.0, 0.66, d * 0.2),
                dark,
            );
            cylinder_part(&mut m, 0.11, 0.12, Vec3::new(-0.1, 1.14, -0.03), 0xb29147);
            for x in [-0.13, -0.1, -0.07] {
                box_part(
                    &mut m,
                    Vec3::new(0.009, 0.22, 0.009),
                    Vec3::new(x, 1.3, -0.03),
                    0xb88145,
                );
            }
            box_part(
                &mut m,
                Vec3::new(0.21, 0.42, 0.18),
                Vec3::new(w * 0.27, 1.29, -0.18),
                0x3b2217,
            );
            box_part(
                &mut m,
                Vec3::new(0.14, 0.3, 0.018),
                Vec3::new(w * 0.27, 1.3, -0.08),
                0xb89b52,
            );
            for x in [-w * 0.35, w * 0.35] {
                cylinder_part(&mut m, 0.055, 0.16, Vec3::new(x, 1.19, 0.01), 0x98271e);
            }
        }
        FixtureKind::Bed => {
            box_part(
                &mut m,
                Vec3::new(w, 0.31, d),
                Vec3::new(0.0, 0.2, 0.0),
                wood,
            );
            box_part(
                &mut m,
                Vec3::new(w, 0.15, d - 0.05),
                Vec3::new(0.0, 0.43, 0.0),
                0xc5c4b1,
            );
            box_part(
                &mut m,
                Vec3::new(w - 0.08, 0.06, d * 0.64),
                Vec3::new(0.0, 0.54, d * 0.14),
                0x557a89,
            );
            box_part(
                &mut m,
                Vec3::new(w, 0.66, 0.07),
                Vec3::new(0.0, 0.35, -d * 0.5),
                dark,
            );
            for x in [-w * 0.23, w * 0.23] {
                box_part(
                    &mut m,
                    Vec3::new(0.5, 0.11, 0.34),
                    Vec3::new(x, 0.56, -d * 0.33),
                    0xe0dac4,
                );
            }
        }
        FixtureKind::Sofa => {
            box_part(
                &mut m,
                Vec3::new(w, 0.38, d),
                Vec3::new(0.0, 0.25, 0.0),
                dark,
            );
            box_part(
                &mut m,
                Vec3::new(w, 0.56, 0.08),
                Vec3::new(0.0, 0.66, -d * 0.43),
                wood,
            );
            for x in [-w * 0.46, w * 0.46] {
                box_part(
                    &mut m,
                    Vec3::new(0.12, 0.34, d),
                    Vec3::new(x, 0.59, 0.0),
                    wood,
                );
            }
            for x in [-0.57, 0.0, 0.57] {
                box_part(
                    &mut m,
                    Vec3::new(0.53, 0.09, d * 0.8),
                    Vec3::new(x, 0.48, 0.05),
                    0xb9a075,
                );
                box_part(
                    &mut m,
                    Vec3::new(0.48, 0.34, 0.045),
                    Vec3::new(x, 0.7, -d * 0.35),
                    0xa98a59,
                );
            }
        }
        FixtureKind::Table => {
            box_part(
                &mut m,
                Vec3::new(0.9, 0.05, 0.66),
                Vec3::new(0.0, 0.76, 0.0),
                0xc6b786,
            );
            for x in [-0.36, 0.36] {
                for z in [-0.24, 0.24] {
                    box_part(
                        &mut m,
                        Vec3::new(0.055, 0.73, 0.055),
                        Vec3::new(x, 0.37, z),
                        0x455146,
                    );
                }
            }
            for x in [-0.47, 0.47] {
                cylinder_part(&mut m, 0.14, 0.38, Vec3::new(x, 0.2, 0.32), 0xa63b28);
            }
            cylinder_part(&mut m, 0.1, 0.025, Vec3::new(0.1, 0.8, 0.0), 0xe6e0c3);
            cylinder_part(&mut m, 0.05, 0.11, Vec3::new(-0.18, 0.83, 0.0), 0x897452);
        }
    }
    m.jitter()
}

/// A render treatment of the existing, collision-backed kitchen fixture. A
/// signed food shop may have native legacy `home` loot classification; this
/// never changes that classification, the fixture ID, stock or interaction.
pub fn market_cart_fixture(b: &Bld, kind: FixtureKind) -> bool {
    kind == FixtureKind::Kitchen
        && b.sign
            .is_some_and(|sign| matches!(sign.idx, 0 | 2 | 6 | 14))
}

pub fn market_cart_mesh() -> MeshBuf {
    let mut m = MeshBuf::default();
    let (w, d) = (1.7, 0.65);
    // Stainless pushcart silhouette, inside the old kitchen's 1.7 × 1.0 × 0.65
    // collision box. Shelved crates and a recessed refuse bucket add no solids.
    for x in [-0.66, 0.66] {
        for z in [-0.215, 0.215] {
            m.add(
                &geom::cylinder(0.105, 0.105, 0.055, 8),
                geom::at(x, 0.115, z) * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2),
                hex(0x262d2a),
            );
            box_part(
                &mut m,
                Vec3::new(0.045, 0.61, 0.045),
                Vec3::new(x, 0.475, z),
                0x949d96,
            );
        }
    }
    box_part(
        &mut m,
        Vec3::new(w - 0.05, 0.05, d - 0.03),
        Vec3::new(0.0, 0.78, 0.0),
        0xb6beb5,
    );
    box_part(
        &mut m,
        Vec3::new(w - 0.05, 0.045, d - 0.06),
        Vec3::new(0.0, 0.24, 0.0),
        0x657b6b,
    );
    for z in [-0.29, 0.29] {
        box_part(
            &mut m,
            Vec3::new(w - 0.09, 0.11, 0.025),
            Vec3::new(0.0, 0.68, z),
            0x607c70,
        );
        box_part(
            &mut m,
            Vec3::new(w - 0.03, 0.035, 0.04),
            Vec3::new(0.0, 0.817, z),
            0xc4c8b6,
        );
    }
    // Separate rectangular crate shells, deliberately empty. Actual food stocks
    // continue to come exclusively from the saved container ledger below.
    for (x, paint) in [(-0.43, 0x8d653f), (0.07, 0x496c60)] {
        for y in [0.39, 0.55] {
            box_part(
                &mut m,
                Vec3::new(0.42, 0.025, 0.43),
                Vec3::new(x, y - 0.065, 0.0),
                paint,
            );
            for z in [-0.205, 0.205] {
                for dy in [-0.025, 0.035] {
                    box_part(
                        &mut m,
                        Vec3::new(0.42, 0.028, 0.025),
                        Vec3::new(x, y + dy, z),
                        paint,
                    );
                }
            }
            for dx in [-0.195, 0.195] {
                box_part(
                    &mut m,
                    Vec3::new(0.025, 0.13, 0.43),
                    Vec3::new(x + dx, y, 0.0),
                    paint,
                );
            }
        }
    }
    cylinder_part(&mut m, 0.115, 0.33, Vec3::new(0.57, 0.43, 0.0), 0x37664f);
    cylinder_part(&mut m, 0.12, 0.03, Vec3::new(0.57, 0.61, 0.0), 0x263e35);
    box_part(
        &mut m,
        Vec3::new(0.36, 0.11, 0.2),
        Vec3::new(0.49, 0.87, -0.12),
        0xb4b7a9,
    );
    box_part(
        &mut m,
        Vec3::new(0.28, 0.035, 0.16),
        Vec3::new(0.49, 0.941, -0.12),
        0x424c45,
    );
    m.jitter()
}

#[derive(Resource)]
struct InteriorArt {
    material: Handle<StandardMaterial>,
    fixtures: HashMap<FixtureKind, MeshBuf>,
}

#[derive(Component)]
pub(crate) struct InteriorView {
    pub(crate) building: u32,
    pub(crate) stock: Entity,
    fingerprint: u64,
}

/// Retained at the actual upload, since GPU-only meshes release CPU arrays.
#[derive(Component, Clone, Copy)]
pub(crate) struct InteriorGeometry {
    pub(crate) vertices: usize,
    pub(crate) min: Vec3,
    pub(crate) max: Vec3,
}

#[derive(Resource, Default)]
struct InteriorTimer(f32);

/// MapPlugin installs this plugin; no extra platform-specific setup is needed.
pub struct InteriorPlugin;

impl Plugin for InteriorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InteriorTimer>()
            .add_systems(Startup, setup)
            .add_systems(Update, stream.after(crate::worldplay::WorldSet));
    }
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    let fixtures = [
        FixtureKind::Shelf,
        FixtureKind::Fridge,
        FixtureKind::Counter,
        FixtureKind::Medicine,
        FixtureKind::Locker,
        FixtureKind::Wardrobe,
        FixtureKind::Kitchen,
        FixtureKind::Altar,
        FixtureKind::Bed,
        FixtureKind::Sofa,
        FixtureKind::Table,
    ]
    .into_iter()
    .map(|kind| (kind, body_mesh(kind)))
    .collect();
    commands.insert_resource(InteriorArt {
        material: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.88,
            ..default()
        }),
        fixtures,
    });
}

fn product_mesh(item: &str) -> MeshBuf {
    let mut m = MeshBuf::default();
    let hash = crate::worldplay::id_hash(item);
    let color = [0xa7472f, 0xc6ae65, 0x527a4c, 0x3e7190, 0xa87941, 0xc7c6ae][hash as usize % 6];
    if matches!(
        item,
        "water"
            | "tea"
            | "boiled"
            | "rainwater"
            | "cola"
            | "sportsdrink"
            | "milk"
            | "soymilk"
            | "oil"
    ) {
        cylinder_part(
            &mut m,
            0.045,
            0.18,
            Vec3::new(0.0, 0.09, 0.0),
            if item == "water" { 0x77a6ae } else { color },
        );
        cylinder_part(&mut m, 0.025, 0.055, Vec3::new(0.0, 0.205, 0.0), 0xc7d3bf);
        cylinder_part(&mut m, 0.028, 0.025, Vec3::new(0.0, 0.24, 0.0), 0x346176);
        box_part(
            &mut m,
            Vec3::new(0.07, 0.06, 0.012),
            Vec3::new(0.0, 0.1, 0.04),
            0xddd9bf,
        );
    } else if matches!(
        item,
        "can" | "peanutcan" | "canfruit" | "pill" | "vitamin" | "antibiotic"
    ) {
        cylinder_part(&mut m, 0.055, 0.14, Vec3::new(0.0, 0.07, 0.0), color);
        cylinder_part(&mut m, 0.057, 0.015, Vec3::new(0.0, 0.148, 0.0), 0xbec3b8);
    } else {
        box_part(
            &mut m,
            Vec3::new(0.13, 0.19, 0.085),
            Vec3::new(0.0, 0.095, 0.0),
            color,
        );
        box_part(
            &mut m,
            Vec3::new(0.09, 0.055, 0.012),
            Vec3::new(0.0, 0.11, 0.043),
            0xded9be,
        );
    }
    m
}

/// Product shapes are a bounded display of actual stacks, not a second stock
/// ledger. Empty records always clear the display; unopened previews use the
/// same deterministic generator and night bonus as the scavenging panel.
fn display_units(stock: &Stash) -> Vec<&str> {
    let mut units = Vec::new();
    // Round-robin keeps each remaining type visible before extra quantities.
    for copy in 0..4 {
        for (id, count) in stock.stacks() {
            if *count > copy && units.len() < MAX_PRODUCTS {
                units.push(id.as_str());
            }
        }
    }
    units
}

fn stock_mesh(fixtures: &[&Fixture], stocks: &[Stash]) -> MeshBuf {
    let mut m = MeshBuf::default();
    for (f, stock) in fixtures.iter().zip(stocks) {
        let cart = super::world()
            .blds
            .get(f.building as usize)
            .is_some_and(|b| market_cart_fixture(b, f.kind));
        if !matches!(
            f.kind,
            FixtureKind::Shelf | FixtureKind::Fridge | FixtureKind::Medicine
        ) && !cart
        {
            continue;
        }
        let size = f.kind.size();
        let transform = f.transform();
        for (i, id) in display_units(stock).into_iter().enumerate() {
            if cart && i >= 5 {
                break;
            }
            let row = i / 5;
            let col = i % 5;
            let p = if cart {
                Vec3::new(-0.61 + i as f32 * 0.21, 0.81, 0.08)
            } else {
                Vec3::new(
                    (col as f32 - 2.0) * (size.x - 0.25) / 5.0,
                    0.163 + row as f32 * 0.43,
                    size.z * 0.24,
                )
            };
            let scale = if cart { Vec3::splat(0.7) } else { Vec3::ONE };
            m.add_white(
                &product_mesh(id),
                transform * Mat4::from_translation(p) * Mat4::from_scale(scale),
            );
        }
    }
    m
}

pub(crate) fn stock_visual_geometry(fixtures: &[&Fixture], stocks: &[Stash]) -> MeshBuf {
    stock_mesh(fixtures, stocks)
}

fn room_body(b: &Bld, fixtures: &[&Fixture], art: &HashMap<FixtureKind, MeshBuf>) -> MeshBuf {
    let mut mesh = MeshBuf::default();
    for f in fixtures {
        if market_cart_fixture(b, f.kind) {
            mesh.add_white(&market_cart_mesh(), f.transform());
        } else {
            mesh.add_white(&art[&f.kind], f.transform());
        }
    }
    if b.kind == "home"
        && let Some(door) = b.door
    {
        let transform = geom::place(door.p.x, 0.06, door.p.y, door.n.x.atan2(door.n.y));
        // Blue-and-white slippers, scuffed threshold mat: flat, non-obstructing.
        for x in [-0.84, -0.64] {
            mesh.add(
                &geom::cuboid(0.12, 0.024, 0.28),
                transform * geom::at(x, 0.0, -0.65),
                hex(0x3c5d99),
            );
            mesh.add(
                &geom::cuboid(0.12, 0.026, 0.07),
                transform * geom::at(x, 0.025, -0.64),
                hex(0xc7ccbf),
            );
        }
        mesh.add(
            &geom::cuboid(0.75, 0.013, 0.42),
            transform * geom::at(0.0, 0.0, -0.68),
            hex(0x7a7058),
        );
    }
    mesh
}

/// Regenerate the exact production room body for geometry audits; this is not
/// an alternate display mesh and does not insert entities or change fixtures.
pub(crate) fn room_visual_geometry(b: &Bld, fixtures: &[&Fixture]) -> MeshBuf {
    let art = fixtures
        .iter()
        .map(|fixture| (fixture.kind, body_mesh(fixture.kind)))
        .collect();
    room_body(b, fixtures, &art)
}

fn stock_fingerprint(stocks: &[Stash]) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for stock in stocks {
        stock.stacks().hash(&mut hash);
    }
    hash.finish()
}

/// Keep the room's stock entity/parent stable while dropping empty render data.
/// Empty home cupboards and depleted shelves must not upload vertex-less meshes.
fn replace_stock_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    entity: Entity,
    stock: MeshBuf,
) {
    if stock.is_empty() {
        commands
            .entity(entity)
            .remove::<(Mesh3d, InteriorGeometry)>()
            .insert(Visibility::Hidden);
    } else {
        let (min, max) = stock.aabb();
        let geometry = InteriorGeometry {
            vertices: stock.vcount(),
            min,
            max,
        };
        commands.entity(entity).insert((
            Mesh3d(meshes.add(stock.into_mesh())),
            geometry,
            Visibility::Inherited,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn stream(
    mut commands: Commands,
    time: Res<Time>,
    mut timer: ResMut<InteriorTimer>,
    art: Res<InteriorArt>,
    progress: Res<ScavengeState>,
    campaign: Res<Campaign>,
    player: Query<&Transform, With<Player>>,
    mut views: Query<(Entity, &mut InteriorView)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    timer.0 -= time.delta_secs();
    if timer.0 > 0.0 {
        return;
    }
    timer.0 = 0.25;
    let Ok(player) = player.single() else {
        return;
    };
    let pos = player.translation.xz();
    let world = super::world();
    let mut nearest: Vec<_> = world
        .interiors
        .iter()
        .map(|f| (f.building, f.pos.distance_squared(pos)))
        .filter(|(_, d)| *d < LIVE_RANGE * LIVE_RANGE)
        .collect();
    nearest.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut desired = Vec::new();
    for (id, _) in nearest {
        if !desired.contains(&id) {
            desired.push(id);
        }
        if desired.len() == MAX_LIVE_ROOMS {
            break;
        }
    }
    let mut existing = HashSet::new();
    for (entity, mut view) in &mut views {
        if !desired.contains(&view.building) {
            commands.entity(entity).despawn();
            continue;
        }
        existing.insert(view.building);
        let fixtures: Vec<_> = world
            .interiors
            .iter()
            .filter(|f| f.building == view.building)
            .collect();
        let stocks: Vec<_> = fixtures
            .iter()
            .map(|f| {
                f.loot.map_or_else(Stash::default, |kind| {
                    crate::worldplay::preview_container_loot(
                        &f.id,
                        kind,
                        campaign.game_min,
                        &progress,
                    )
                })
            })
            .collect();
        let fingerprint = stock_fingerprint(&stocks);
        if view.fingerprint != fingerprint {
            view.fingerprint = fingerprint;
            let stock = stock_mesh(&fixtures, &stocks);
            replace_stock_mesh(&mut commands, &mut meshes, view.stock, stock);
        }
    }
    for id in desired.into_iter().filter(|id| !existing.contains(id)) {
        let Some(building) = world.blds.iter().find(|b| b.id == id) else {
            continue;
        };
        let fixtures: Vec<_> = world
            .interiors
            .iter()
            .filter(|f| f.building == id)
            .collect();
        let stocks: Vec<_> = fixtures
            .iter()
            .map(|f| {
                f.loot.map_or_else(Stash::default, |kind| {
                    crate::worldplay::preview_container_loot(
                        &f.id,
                        kind,
                        campaign.game_min,
                        &progress,
                    )
                })
            })
            .collect();
        let stock = commands
            .spawn((
                MeshMaterial3d(art.material.clone()),
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();
        replace_stock_mesh(
            &mut commands,
            &mut meshes,
            stock,
            stock_mesh(&fixtures, &stocks),
        );
        let body = room_body(building, &fixtures, &art.fixtures);
        let (min, max) = body.aabb();
        let geometry = InteriorGeometry {
            vertices: body.vcount(),
            min,
            max,
        };
        commands
            .spawn((
                InteriorView {
                    building: id,
                    stock,
                    fingerprint: stock_fingerprint(&stocks),
                },
                geometry,
                Mesh3d(meshes.add(body.into_mesh())),
                MeshMaterial3d(art.material.clone()),
                Transform::default(),
                Visibility::default(),
            ))
            .add_child(stock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planned_rooms_keep_entrances_and_every_fixture_reachable() {
        let world = super::super::world();
        assert!(world.interiors.len() > 500, "catalogue must be substantial");
        let mut rooms: HashMap<u32, Vec<&Fixture>> = HashMap::new();
        for f in &world.interiors {
            rooms.entry(f.building).or_default().push(f);
        }
        let mut kinds = HashSet::new();
        for (id, fixtures) in rooms {
            let b = world.blds.iter().find(|b| b.id == id).unwrap();
            assert!(fixtures.len() <= MAX_ROOM_FIXTURES);
            assert_eq!(
                fixtures.iter().filter(|f| f.id == format!("c{id}")).count(),
                1
            );
            let door = b.door.unwrap();
            let anchor = door.p - door.n * 1.4;
            for f in &fixtures {
                kinds.insert(f.kind);
                for corner in f.corners() {
                    assert!(circle_inside(b, corner, 0.13));
                }
                let steps = (anchor.distance(f.approach) / 0.15).ceil().max(1.0) as usize;
                for i in 0..=steps {
                    let p = anchor.lerp(f.approach, i as f32 / steps as f32);
                    assert!(
                        world.free(p, PLAYER_RADIUS + 0.05),
                        "{} route hits real collision at {:?}",
                        f.id,
                        p
                    );
                    assert!(
                        circle_inside(b, p, PLAYER_RADIUS + 0.05),
                        "{} route leaves footprint",
                        f.id
                    );
                    assert!(
                        fixtures
                            .iter()
                            .all(|other| !other.contains(p, PLAYER_RADIUS + 0.05)),
                        "{} unreachable",
                        f.id
                    );
                }
            }
            let repeated = plan_room(world, b);
            assert_eq!(
                fixtures
                    .iter()
                    .map(|f| (&f.id, f.pos, f.approach))
                    .collect::<Vec<_>>(),
                repeated
                    .iter()
                    .map(|f| (&f.id, f.pos, f.approach))
                    .collect::<Vec<_>>()
            );
        }
        assert_eq!(kinds.len(), 11);
        eprintln!(
            "INTERIORS fixtures={} rooms={} kinds={}",
            world.interiors.len(),
            world
                .interiors
                .iter()
                .map(|f| f.building)
                .collect::<HashSet<_>>()
                .len(),
            kinds.len()
        );
        for kind in [
            FixtureKind::Shelf,
            FixtureKind::Medicine,
            FixtureKind::Wardrobe,
            FixtureKind::Altar,
        ] {
            let f = world
                .interiors
                .iter()
                .filter(|f| f.kind == kind)
                .min_by(|a, b| {
                    a.pos
                        .distance_squared(world.spawn)
                        .total_cmp(&b.pos.distance_squared(world.spawn))
                })
                .unwrap();
            eprintln!(
                "INTERIOR_QA {:?} id={} building={} pos={:?} approach={:?}",
                kind, f.id, f.building, f.pos, f.approach
            );
        }
    }

    #[test]
    fn solid_furniture_stops_walkers_but_low_furniture_can_be_shot_over() {
        let world = super::super::world();
        let obstacles = super::super::Obstacles::new(world.clone());
        for kind in [FixtureKind::Shelf, FixtureKind::Bed, FixtureKind::Table] {
            let f = world.interiors.iter().find(|f| f.kind == kind).unwrap();
            let behind = f.pos - f.front * (f.kind.size().z * 0.5 + 0.8);
            let moved = obstacles.move_circle(f.approach, behind, PLAYER_RADIUS);
            assert!(!f.contains(moved, PLAYER_RADIUS - 0.01));
            assert!(!obstacles.los(f.approach, f.pos));
            // The nearby-own-wall exception must not erase solid furniture.
            assert!(obstacles.shot_ray(f.approach, f.pos, 0.3, 0.3, 10.0) < 1.0);
            let hit = obstacles.segment_hit(f.approach, f.pos, 0.3, 0.3).unwrap();
            assert_eq!(hit.h, Some(f.kind.size().y));
            if kind != FixtureKind::Shelf {
                assert!(obstacles.segment_hit(f.approach, f.pos, 1.7, 1.7).is_none());
            }
        }
    }

    #[test]
    fn legacy_save_inside_new_furniture_is_unstuck_without_moving_clear_positions() {
        let world = super::super::world();
        for f in world
            .interiors
            .iter()
            .filter(|f| f.loot.is_some())
            .take(200)
        {
            assert_eq!(restore_position(world, f.approach), f.approach);
            let restored = restore_position(world, f.pos);
            assert_ne!(restored, f.pos, "{} remained inside furniture", f.id);
            assert!(world.free(restored, PLAYER_RADIUS));
            assert!(
                world
                    .interiors
                    .iter()
                    .all(|other| !other.contains(restored, PLAYER_RADIUS))
            );
            let b = world.blds.iter().find(|b| b.id == f.building).unwrap();
            assert!(pip(restored, &b.pts));
        }
    }

    #[test]
    fn furniture_and_stock_have_bounded_meshes_and_empty_stock_disappears() {
        for kind in [
            FixtureKind::Shelf,
            FixtureKind::Fridge,
            FixtureKind::Medicine,
            FixtureKind::Altar,
            FixtureKind::Table,
        ] {
            assert!(body_mesh(kind).vcount() < 2300);
        }
        let mut stock = Stash::default();
        stock.add("water", 1000);
        stock.add("can", 1000);
        stock.add("pill", 3);
        assert!(display_units(&stock).len() <= MAX_PRODUCTS);
        assert!(display_units(&stock).contains(&"pill"));
        let f = super::super::world()
            .interiors
            .iter()
            .find(|f| f.kind == FixtureKind::Shelf)
            .unwrap();
        let full = stock_mesh(&[f], &[stock]);
        assert!(full.vcount() > 0 && full.vcount() < 8000);
        assert!(stock_mesh(&[f], &[Stash::default()]).is_empty());
        assert_ne!(
            stock_fingerprint(&[Stash::default()]),
            stock_fingerprint(&[{
                let mut s = Stash::default();
                s.add("water", 1);
                s
            }])
        );
    }

    #[test]
    fn market_cart_treatment_preserves_the_existing_fixture_and_live_stock() {
        let world = super::super::world();
        let fixture = world
            .interiors
            .iter()
            .find(|f| market_cart_fixture(&world.blds[f.building as usize], f.kind))
            .expect("at least one original signed food frontage has a kitchen fixture");
        let mesh = market_cart_mesh();
        assert!(mesh.vcount() < 1_500);
        let size = fixture.kind.size();
        for vertex in &mesh.pos {
            assert!(vertex[0].abs() <= size.x * 0.5 + 0.001);
            assert!(vertex[2].abs() <= size.z * 0.5 + 0.001);
            assert!((0.0..=size.y).contains(&vertex[1]));
        }
        let mut actual = Stash::default();
        actual.add("water", 2);
        actual.add("can", 3);
        let displayed = stock_mesh(&[fixture], &[actual]);
        assert!(!displayed.is_empty());
        let inverse = fixture.transform().inverse();
        for vertex in &displayed.pos {
            let local = inverse.transform_point3(Vec3::from(*vertex));
            assert!(local.x.abs() <= size.x * 0.5);
            assert!(local.z.abs() <= size.z * 0.5);
            assert!((0.80..=size.y).contains(&local.y));
        }
        assert!(stock_mesh(&[fixture], &[Stash::default()]).is_empty());
    }
    #[test]
    fn stock_renderer_removes_empty_meshes_and_restores_visible_products() {
        let mut world = World::new();
        let mut assets = Assets::<Mesh>::default();
        let stock = world
            .spawn((Transform::from_xyz(1.0, 2.0, 3.0), Visibility::Hidden))
            .id();
        let fixture = super::super::world()
            .interiors
            .iter()
            .find(|f| f.kind == FixtureKind::Shelf)
            .unwrap();
        let mut inventory = Stash::default();
        inventory.add("water", 3);
        replace_stock_mesh(
            &mut world.commands(),
            &mut assets,
            stock,
            MeshBuf::default(),
        );
        world.flush();
        assert!(world.get::<Mesh3d>(stock).is_none());
        assert_eq!(assets.len(), 0, "empty stock must not allocate a GPU mesh");
        for _ in 0..2 {
            replace_stock_mesh(
                &mut world.commands(),
                &mut assets,
                stock,
                stock_mesh(&[fixture], std::slice::from_ref(&inventory)),
            );
            world.flush();
            let handle = world.get::<Mesh3d>(stock).unwrap();
            assert!(assets.get(&handle.0).unwrap().count_vertices() > 0);
            assert_eq!(
                *world.get::<Visibility>(stock).unwrap(),
                Visibility::Inherited
            );
            let allocated = assets.len();
            replace_stock_mesh(
                &mut world.commands(),
                &mut assets,
                stock,
                MeshBuf::default(),
            );
            world.flush();
            assert!(world.get::<Mesh3d>(stock).is_none());
            assert_eq!(*world.get::<Visibility>(stock).unwrap(), Visibility::Hidden);
            assert_eq!(
                assets.len(),
                allocated,
                "depletion must not create an empty replacement"
            );
            assert_eq!(
                world.get::<Transform>(stock).unwrap().translation,
                Vec3::new(1.0, 2.0, 3.0)
            );
        }
    }

    #[test]
    fn android_smoke_descriptor_targets_real_reachable_interactions() {
        #[derive(serde::Deserialize)]
        struct Descriptor {
            version: u8,
            shelf: Shelf,
            door: Entrance,
        }
        #[derive(serde::Deserialize)]
        struct Shelf {
            id: String,
            building: u32,
            approach: [f32; 2],
        }
        #[derive(serde::Deserialize)]
        struct Entrance {
            id: u32,
            position: [f32; 2],
        }
        let descriptor: Descriptor = serde_json::from_str(include_str!(
            "../../tests/fixtures/android-indoor-smoke.json"
        ))
        .unwrap();
        assert_eq!(descriptor.version, 1);
        let world = super::super::world();
        let shelf = world
            .interiors
            .iter()
            .find(|f| f.id == descriptor.shelf.id)
            .unwrap();
        let shelf_position = Vec2::from_array(descriptor.shelf.approach);
        assert_eq!(shelf.kind, FixtureKind::Shelf);
        assert_eq!(shelf.building, descriptor.shelf.building);
        assert!(shelf.approach.distance(shelf_position) < 0.01);
        assert!(world.free(shelf_position, PLAYER_RADIUS));
        assert_eq!(restore_position(world, shelf_position), shelf_position);
        assert_eq!(world.bld_at(shelf_position).unwrap().id, shelf.building);
        let nearest = world
            .interiors
            .iter()
            .filter(|f| f.loot.is_some())
            .min_by(|a, b| {
                a.approach
                    .distance_squared(shelf_position)
                    .total_cmp(&b.approach.distance_squared(shelf_position))
            })
            .unwrap();
        assert_eq!(nearest.id, shelf.id);
        assert!(
            world.blds[shelf.building as usize]
                .door
                .unwrap()
                .p
                .distance(shelf_position)
                > crate::doors::INTERACT_RANGE
        );

        let entrance = world
            .blds
            .iter()
            .find(|b| b.id == descriptor.door.id)
            .unwrap()
            .door
            .unwrap();
        let door_position = Vec2::from_array(descriptor.door.position);
        assert!(door_position.distance(entrance.p + entrance.n * 1.5) < 0.01);
        assert!(world.free(door_position, PLAYER_RADIUS));
        let obstacles = super::super::Obstacles::new(world.clone());
        obstacles.doors().set_open(descriptor.door.id, false);
        assert!(
            obstacles
                .collide(door_position, PLAYER_RADIUS)
                .distance(door_position)
                < 0.01
        );
        assert!(obstacles.los(door_position, entrance.p + entrance.n * 0.22));
        assert_eq!(
            crate::doors::nearest_door(door_position, &obstacles)
                .unwrap()
                .id,
            descriptor.door.id
        );
    }
}
