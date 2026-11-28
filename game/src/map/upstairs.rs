//! v23.4 / v24.1 上樓 — stairwells and the furnished upper floors of every
//! multi-storey building (Web `src/map/upstairs.ts`, `clutter.ts` `plan.stair`).
//!
//! A building with two or more storeys and a door gets a stairwell at the back
//! of its ground floor when one fits clear of the furniture and with a route to
//! the door. Every upper floor uses the footprint of the building and its own
//! deterministic layout: the second floor is a living floor (sofa, kitchen,
//! fridge, table, bed, wardrobe), middle floors are bedrooms and the top floor of
//! a three-storey or taller building is the shrine hall and storeroom. Each floor
//! has its own containers, `u<building>_<floor>_<slot>`. Floors go up to the
//! top storey (at most 14), with the flight up beside the landing.

use bevy::math::{Vec2, Vec3};

use super::data::{Bld, WorldData};
use super::interiors::{Fixture, FixtureKind, overlaps, route_hits, usable_floor};
use crate::player::PLAYER_RADIUS;

/// The highest floor anyone climbs to.
pub const MAX_FLOOR: u8 = 14;

/// A stairwell: the landing on every floor and the direction the flight rises.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stair {
    pub building: u32,
    /// Where the flight starts on the ground floor and where one arrives upstairs.
    pub landing: Vec2,
    /// Unit direction the flight rises along (towards the back wall).
    pub dir: Vec2,
    /// Number of storeys (ground floor included), capped at [`MAX_FLOOR`].
    pub floors: u8,
    /// Storey height of this building.
    pub storey: f32,
}

impl Stair {
    /// The flight up to the next floor on floors 2 and above (beside the landing).
    pub fn up(&self) -> Vec2 {
        self.landing + Vec2::new(self.dir.y, -self.dir.x) * 1.25
    }

    /// Height of floor `f` (1 = the street).
    pub fn floor_y(&self, f: u8) -> f32 {
        f32::from(f.max(1) - 1) * self.storey
    }
}

/// `plan.stair`: a landing at the back of the ground floor of a multi-storey
/// building, clear of its furniture and reachable from the door.
pub fn plan_stair(world: &WorldData, b: &Bld, ground: &[Fixture]) -> Option<Stair> {
    if b.lv < 2 {
        return None;
    }
    let door = b.door?;
    let anchor = door.p - door.n * 1.4;
    let u = door.tangent();
    let inward = -door.n;
    let (mut left, mut right, mut depth) = (0.0_f32, 0.0_f32, 0.0_f32);
    for p in &b.pts {
        let d = *p - door.p;
        left = left.min(d.dot(u));
        right = right.max(d.dot(u));
        depth = depth.max(d.dot(inward));
    }
    if depth < 4.0 || right - left < 3.0 {
        return None;
    }
    let mut v = (depth - 1.1).min(16.0);
    while v >= 3.0 {
        for frac in [0.75_f32, -0.75, 0.4, -0.4, 0.0] {
            let x = if frac >= 0.0 {
                right * frac
            } else {
                -left * frac
            };
            let p = door.p + u * x + inward * v;
            if !usable_floor(world, b, p, 0.85)
                || !usable_floor(world, b, p + inward * 0.9, 0.5)
                || ground.iter().any(|f| f.contains(p, 0.9))
            {
                continue;
            }
            if !route_clear(world, b, anchor, p, ground) {
                continue;
            }
            let storey = (b.h / b.lv.max(1) as f32).clamp(2.8, 4.2);
            return Some(Stair {
                building: b.id,
                landing: p,
                dir: inward,
                floors: (b.lv.min(u32::from(MAX_FLOOR))) as u8,
                storey,
            });
        }
        v -= 0.5;
    }
    None
}

fn route_clear(world: &WorldData, b: &Bld, from: Vec2, to: Vec2, fixtures: &[Fixture]) -> bool {
    let radius = PLAYER_RADIUS + 0.1;
    if fixtures.iter().any(|f| route_hits(f, from, to, radius)) {
        return false;
    }
    let steps = (from.distance(to) / 0.1).ceil().max(1.0) as usize;
    (0..=steps).all(|i| usable_floor(world, b, from.lerp(to, i as f32 / steps as f32), radius))
}

/// The kinds of furniture on floor `f` of a building with `floors` storeys.
pub fn floor_kinds(f: u8, floors: u8) -> &'static [FixtureKind] {
    if f == 2 {
        &[
            FixtureKind::Sofa,
            FixtureKind::Kitchen,
            FixtureKind::Fridge,
            FixtureKind::Table,
            FixtureKind::Bed,
            FixtureKind::Wardrobe,
        ]
    } else if f == floors && floors >= 3 {
        &[
            FixtureKind::Altar,
            FixtureKind::Wardrobe,
            FixtureKind::Table,
            FixtureKind::Shelf,
        ]
    } else {
        &[
            FixtureKind::Bed,
            FixtureKind::Wardrobe,
            FixtureKind::Table,
            FixtureKind::Bed,
            FixtureKind::Wardrobe,
        ]
    }
}

/// The furnished layout of floor `f` (2 and up): candidates along both side
/// walls, each reachable from the landing (and the flight up) without covering
/// either. Deterministic in the building, the floor and the stair.
pub fn plan_floor(world: &WorldData, b: &Bld, stair: &Stair, f: u8) -> Vec<Fixture> {
    let Some(door) = b.door else {
        return Vec::new();
    };
    let u = door.tangent();
    let inward = -door.n;
    let (mut left, mut right, mut depth) = (0.0_f32, 0.0_f32, 0.0_f32);
    for p in &b.pts {
        let d = *p - door.p;
        left = left.min(d.dot(u));
        right = right.max(d.dot(u));
        depth = depth.max(d.dot(inward));
    }
    let up = (f < stair.floors).then(|| stair.up());
    let mut out: Vec<Fixture> = Vec::new();
    // a different arrangement per floor: start on alternating sides and depths
    let flip = if (u32::from(f) + b.id).is_multiple_of(2) {
        1.0
    } else {
        -1.0
    };
    for (slot, &kind) in floor_kinds(f, stair.floors).iter().enumerate() {
        let size = kind.size();
        let mut candidates = Vec::new();
        for side in [-1.0_f32, 1.0] {
            let bound = if side < 0.0 { -left } else { right };
            let min_x = 0.4 + size.z * 0.5;
            let max_x = (bound - size.z * 0.5 - 0.22).min(9.0);
            if max_x < min_x {
                continue;
            }
            for xi in 0..=4 {
                let x = side * (max_x - (max_x - min_x) * xi as f32 / 4.0);
                let first_v = (size.x * 0.5 + 0.5).max(1.0);
                let last_v = (depth - size.x * 0.5 - 0.25).min(16.0);
                let mut v = first_v;
                while v <= last_v {
                    let pos = door.p + u * x + inward * v;
                    let front = u * -side;
                    let (label_zh, label_en) = kind.labels();
                    let fixture = Fixture {
                        id: format!("u{}_{}_{}", b.id, f, slot),
                        building: b.id,
                        pos,
                        approach: pos + front * (size.z * 0.5 + 0.64),
                        front,
                        kind,
                        loot: kind.loot(),
                        label_zh,
                        label_en,
                    };
                    let preferred_side = if slot % 2 == 0 { -flip } else { flip };
                    let preferred_depth = 1.6 + (slot / 2) as f32 * 2.6 + f32::from(f % 3) * 0.6;
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
            .find(|(_, cand)| fits_up(world, b, stair, up, cand, &out))
        {
            out.push(fixture);
        }
    }
    out
}

fn fits_up(
    world: &WorldData,
    b: &Bld,
    stair: &Stair,
    up: Option<Vec2>,
    f: &Fixture,
    existing: &[Fixture],
) -> bool {
    let cs = f.corners();
    let contained = (0..4).all(|edge| {
        let a = cs[edge];
        let z = cs[(edge + 1) % 4];
        let n = (a.distance(z) / 0.2).ceil().max(1.0) as usize;
        (0..=n).all(|i| usable_floor(world, b, a.lerp(z, i as f32 / n as f32), 0.16))
    });
    if !contained
        || existing.iter().any(|e| overlaps(f, e, 0.24))
        || f.contains(stair.landing, 1.0)
        || up.is_some_and(|p| f.contains(p, 1.0))
    {
        return false;
    }
    let mut all: Vec<Fixture> = existing.to_vec();
    all.push(f.clone());
    let reach = |to: Vec2| route_clear(world, b, stair.landing, to, &all);
    reach(f.approach)
        && existing.iter().all(|e| reach(e.approach))
        && up.is_none_or(|p| route_clear(world, b, stair.landing, p, &all))
}

/// Plans every stairwell (after the ground-floor furniture is installed).
pub fn install(w: &mut WorldData) {
    let mut stairs = Vec::new();
    for b in &w.blds {
        let ground: Vec<Fixture> = w
            .interiors
            .iter()
            .filter(|f| f.building == b.id)
            .cloned()
            .collect();
        if let Some(s) = plan_stair(w, b, &ground) {
            stairs.push(s);
        }
    }
    w.stairs = stairs;
}

/// The stairwell of a building, if it has one.
pub fn stair_of(w: &WorldData, building: u32) -> Option<&Stair> {
    w.stairs
        .binary_search_by_key(&building, |s| s.building)
        .ok()
        .map(|i| &w.stairs[i])
}

/// Vertices of a simple flight of `n` steps rising along `dir` from `at`.
pub fn flight_boxes(at: Vec2, dir: Vec2, y0: f32, rise: f32, n: usize) -> Vec<(Vec3, Vec3, f32)> {
    let yaw = dir.x.atan2(dir.y);
    (0..n)
        .map(|k| {
            let t = (k as f32 + 0.5) / n as f32;
            let p = at + dir * (t * 1.8 - 0.4);
            let h = rise * (k as f32 + 1.0) / n as f32;
            (
                Vec3::new(p.x, y0 + h / 2.0, p.y),
                Vec3::new(1.0, h, 1.8 / n as f32),
                yaw,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hundreds_of_buildings_have_a_stairwell() {
        let w = crate::map::world();
        assert!(w.stairs.len() > 150, "stairs {}", w.stairs.len());
        assert!(w.stairs.windows(2).all(|p| p[0].building < p[1].building));
        for s in &w.stairs {
            let b = &w.blds[s.building as usize];
            assert!(b.lv >= 2 && s.floors >= 2 && s.floors <= MAX_FLOOR);
            assert_eq!(w.bld_at(s.landing).map(|x| x.id), Some(b.id));
            assert!(
                w.interiors
                    .iter()
                    .filter(|f| f.building == b.id)
                    .all(|f| !f.contains(s.landing, 0.8))
            );
        }
    }

    #[test]
    fn every_floor_is_furnished_with_its_own_containers() {
        let w = crate::map::world();
        let s = w.stairs.iter().find(|s| s.floors >= 3).expect("a walk-up");
        let b = &w.blds[s.building as usize];
        let f2 = plan_floor(w, b, s, 2);
        let f3 = plan_floor(w, b, s, 3);
        assert!(!f2.is_empty() && !f3.is_empty());
        assert!(
            f2.iter()
                .all(|f| f.id.starts_with(&format!("u{}_2_", b.id)))
        );
        assert!(
            f3.iter()
                .all(|f| f.id.starts_with(&format!("u{}_3_", b.id)))
        );
        assert!(f2.iter().any(|f| f.loot.is_some()));
        // deterministic
        assert_eq!(
            plan_floor(w, b, s, 2)
                .iter()
                .map(|f| f.pos)
                .collect::<Vec<_>>(),
            f2.iter().map(|f| f.pos).collect::<Vec<_>>()
        );
    }
}
