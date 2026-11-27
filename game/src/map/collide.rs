//! Collision and line-of-sight queries over the wall / cover segments of the map — ports of
//! `collide`, `los`, `shotRay`, `solCover`, `findCover` and `bldAt` (main.ts). Walls are
//! segments on an 8 m grid; a circle is pushed out of every segment near it.

use bevy::prelude::*;
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, RwLock};

use super::data::{CELL, Seg, WorldData, seg_dist};
use crate::combat::squad::Footprint;
use crate::combat::world::CombatWorld;

/// The map's collision world. Cheap to clone (shared).
#[derive(Resource, Clone)]
pub struct Obstacles(
    pub Arc<WorldData>,
    crate::doors::DoorRuntime,
    Arc<RwLock<BTreeMap<u8, Vec<DynamicCollider>>>>,
);

/// A live vehicle hull. Groups let civilian and hostile-armor systems replace
/// their own colliders without dropping the other system's cover. Clones of
/// `Obstacles` (including the combat world) observe the same published shapes.
#[derive(Clone, Debug)]
pub struct DynamicCollider {
    pub id: u64,
    pub corners: [Vec2; 4],
    pub height: f32,
}

impl DynamicCollider {
    fn segments(&self) -> impl Iterator<Item = Seg> + '_ {
        (0..4).map(|i| Seg {
            a: self.corners[i],
            b: self.corners[(i + 1) % 4],
            h: Some(self.height),
            bld: None,
        })
    }
    fn contains(&self, p: Vec2) -> bool {
        convex_contains(self.corners, p)
    }
}

fn convex_contains(corners: [Vec2; 4], p: Vec2) -> bool {
    let mut sign = 0.0_f32;
    for i in 0..4 {
        let a = corners[i];
        let d = corners[(i + 1) % 4] - a;
        let cross = d.perp_dot(p - a);
        if cross.abs() < 1e-5 {
            continue;
        }
        if sign != 0.0 && sign.signum() != cross.signum() {
            return false;
        }
        sign = cross;
    }
    true
}

/// A wall / cover hit along a segment query.
#[derive(Clone, Copy, Debug)]
pub struct SegmentHit {
    /// Fraction of the way from start to end (0..=1).
    pub t: f32,
    pub point: Vec2,
    /// Height of the cover hit (`None` = full wall).
    pub h: Option<f32>,
    /// Building the wall belongs to.
    pub bld: Option<u32>,
}

fn seg_inter(p: Vec2, q: Vec2, a: Vec2, b: Vec2) -> Option<(f32, f32)> {
    let d = q - p;
    let e = b - a;
    let den = d.x * e.y - d.y * e.x;
    if den.abs() < 1e-9 {
        return None;
    }
    let t = ((a.x - p.x) * e.y - (a.y - p.y) * e.x) / den;
    let u = ((a.x - p.x) * d.y - (a.y - p.y) * d.x) / den;
    (0.0..=1.0).contains(&t).then_some(())?;
    (0.0..=1.0).contains(&u).then_some((t, u))
}

impl Obstacles {
    pub fn new(world: Arc<WorldData>) -> Self {
        let doors = crate::doors::DoorRuntime::new(&world);
        Self(world, doors, Arc::default())
    }

    pub fn replace_dynamic(&self, group: u8, colliders: Vec<DynamicCollider>) {
        if let Ok(mut shared) = self.2.write() {
            shared.insert(group, colliders);
        }
    }

    fn dynamic(&self, ignore: Option<u64>, mut f: impl FnMut(&DynamicCollider)) {
        if let Ok(shared) = self.2.read() {
            for collider in shared.values().flatten() {
                if ignore != Some(collider.id) {
                    f(collider);
                }
            }
        }
    }

    /// Test an oriented rectangular hull, including edges and contained cover.
    /// A point-only test would let a long vehicle straddle a thin wall.
    pub fn footprint_blocked(&self, corners: [Vec2; 4], ignore: Option<u64>) -> bool {
        self.footprint_blocked_ignoring(corners, ignore.as_slice())
    }

    /// A contact response may move one hull out of another. Those two hulls
    /// must not block that separating motion, but walls and every other hull do.
    pub fn footprint_blocked_ignoring(&self, corners: [Vec2; 4], ignore: &[u64]) -> bool {
        let mut blocked = corners.iter().any(|&p| self.static_blocked(p, 0.04));
        let centre = corners.into_iter().sum::<Vec2>() * 0.25;
        blocked |= self.static_blocked(centre, 0.04);
        for i in 0..4 {
            let a = corners[i];
            let b = corners[(i + 1) % 4];
            self.static_segs_along(a, b, |seg, _| {
                blocked |=
                    seg_inter(a, b, seg.a, seg.b).is_some() || convex_contains(corners, seg.a);
            });
        }
        self.dynamic(None, |hull| {
            if ignore.contains(&hull.id) {
                return;
            }
            blocked |= hull.corners.iter().any(|&p| convex_contains(corners, p))
                || corners.iter().any(|&p| hull.contains(p));
            for i in 0..4 {
                for seg in hull.segments() {
                    blocked |= seg_inter(corners[i], corners[(i + 1) % 4], seg.a, seg.b).is_some();
                }
            }
        });
        blocked
    }

    pub fn doors(&self) -> &crate::doors::DoorRuntime {
        &self.1
    }

    pub fn world(&self) -> &WorldData {
        &self.0
    }

    /// Segments whose grid cells the line p→q passes (los / shotRay walk).
    fn static_segs_along(&self, p: Vec2, q: Vec2, mut f: impl FnMut(&Seg, bool)) {
        let d = q - p;
        let n = (d.length() / (CELL * 0.5)).ceil() as i32 + 1;
        let mut seen: HashSet<u32> = HashSet::new();
        for i in 0..=n {
            let pt = p + d * (i as f32 / n as f32);
            let k = ((pt.x / CELL).floor() as i32, (pt.y / CELL).floor() as i32);
            for &s in self.0.segs_in_cell(k) {
                if seen.insert(s) {
                    f(
                        &self.0.segs[s as usize],
                        s as usize >= self.0.interior_seg_start,
                    );
                }
            }
        }
        self.1.along(p, q, |seg| f(seg, true));
    }

    fn segs_along(&self, p: Vec2, q: Vec2, mut f: impl FnMut(&Seg, bool)) {
        self.static_segs_along(p, q, &mut f);
        self.dynamic(None, |hull| {
            // A weapon's muzzle may start inside the vehicle carrying it.
            // The target hull still blocks incoming fire from outside.
            if !hull.contains(p) {
                for seg in hull.segments() {
                    f(&seg, true);
                }
            }
        });
    }

    /// collide(): push a circle of radius r out of every wall / cover segment and keep it
    /// inside the playable bounds.
    pub fn collide(&self, mut p: Vec2, r: f32) -> Vec2 {
        for _ in 0..2 {
            for s in self.0.near_segs(p) {
                let d = s.b - s.a;
                let l2 = d.length_squared().max(1e-12);
                let t = ((p - s.a).dot(d) / l2).clamp(0.0, 1.0);
                let c = s.a + d * t;
                let o = p - c;
                let l = o.length();
                if l < r && l > 1e-6 {
                    p = c + o / l * r;
                }
            }
            self.1.near(p, r, |s| {
                let d = s.b - s.a;
                let t = ((p - s.a).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0);
                let c = s.a + d * t;
                let offset = p - c;
                if offset.length_squared() < r * r {
                    let normal = if offset.length_squared() > 1e-12 {
                        offset.normalize()
                    } else {
                        Vec2::new(-d.y, d.x).normalize_or_zero()
                    };
                    p = c + normal * r;
                }
            });
            self.dynamic(None, |hull| {
                let inside = hull.contains(p);
                let mut nearest = (f32::MAX, p, Vec2::X);
                for seg in hull.segments() {
                    let d = seg.b - seg.a;
                    let t = ((p - seg.a).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0);
                    let c = seg.a + d * t;
                    let distance = c.distance(p);
                    if distance < nearest.0 {
                        let centre = hull.corners.into_iter().sum::<Vec2>() * 0.25;
                        let mut normal = Vec2::new(-d.y, d.x).normalize_or_zero();
                        if normal.dot(c - centre) < 0.0 {
                            normal = -normal;
                        }
                        nearest = (distance, c, normal);
                    }
                }
                if inside {
                    p = nearest.1 + nearest.2 * (r + 0.002);
                } else if nearest.0 < r {
                    p = nearest.1 + (p - nearest.1).normalize_or_zero() * (r + 0.002);
                }
            });
        }
        p.clamp(self.0.min, self.0.max)
    }

    /// Substeps keep a long Android frame / NPC tick from crossing a closed
    /// aperture in one jump. Ordinary 60 Hz walking still uses a single step.
    pub fn move_circle(&self, from: Vec2, to: Vec2, radius: f32) -> Vec2 {
        let delta = to - from;
        let steps = (delta.length() / (radius.max(0.1) * 0.5)).ceil().max(1.0) as u32;
        let step = delta / steps as f32;
        let mut pos = from;
        for _ in 0..steps {
            pos = self.collide(pos + step, radius);
        }
        pos
    }

    /// Would a circle at p overlap a wall, a building or the map edge?
    pub fn blocked(&self, p: Vec2, r: f32) -> bool {
        self.blocked_except(p, r, None)
    }

    pub fn blocked_except(&self, p: Vec2, r: f32, ignore: Option<u64>) -> bool {
        let mut blocked = self.static_blocked(p, r);
        self.dynamic(ignore, |hull| {
            blocked |= hull.contains(p) || hull.segments().any(|s| seg_dist(p, s.a, s.b) < r);
        });
        blocked
    }

    fn static_blocked(&self, p: Vec2, r: f32) -> bool {
        let mut door = false;
        self.1.near(p, r, |s| door |= seg_dist(p, s.a, s.b) < r);
        p.x < self.0.min.x + r
            || p.y < self.0.min.y + r
            || p.x > self.0.max.x - r
            || p.y > self.0.max.y - r
            || !self.0.free(p, r)
            || self.0.bld_at(p).is_some()
            || door
    }

    /// los(): no wall or cover crosses p→q.
    pub fn los(&self, p: Vec2, q: Vec2) -> bool {
        let mut clear = true;
        self.segs_along(p, q, |s, _| {
            if clear && seg_inter(p, q, s.a, s.b).is_some() {
                clear = false;
            }
        });
        clear
    }

    /// First wall / cover crossing p→q, for a ray at height y0 at p rising/falling to y1
    /// at q. Cover lower than the ray is skipped.
    pub fn segment_hit(&self, p: Vec2, q: Vec2, y0: f32, y1: f32) -> Option<SegmentHit> {
        self.segment_hit_except(p, q, y0, y1, None)
    }

    pub fn segment_hit_except(
        &self,
        p: Vec2,
        q: Vec2,
        y0: f32,
        y1: f32,
        ignore: Option<u64>,
    ) -> Option<SegmentHit> {
        let mut best: Option<SegmentHit> = None;
        let mut test = |s: &Seg, _: bool| {
            if let Some((t, _)) = seg_inter(p, q, s.a, s.b) {
                if best.is_some_and(|b| b.t <= t) {
                    return;
                }
                if s.h.is_some_and(|h| y0 + (y1 - y0) * t > h) {
                    return;
                }
                best = Some(SegmentHit {
                    t,
                    point: p + (q - p) * t,
                    h: s.h,
                    bld: s.bld,
                });
            }
        };
        self.static_segs_along(p, q, &mut test);
        self.dynamic(ignore, |hull| {
            if !hull.contains(p) {
                for seg in hull.segments() {
                    test(&seg, true);
                }
            }
        });
        best
    }

    /// Raycast from `o` along `dir` (normalised) up to `max` m; distance to the first wall.
    pub fn raycast(&self, o: Vec2, dir: Vec2, max: f32, y: f32) -> Option<f32> {
        self.segment_hit(o, o + dir * max, y, y).map(|h| h.t * max)
    }

    /// shotRay(): fraction of p→q clear of walls (and cover the ray passes under).
    pub fn shot_ray(&self, p: Vec2, q: Vec2, ys: f32, yt: f32, own: f32) -> f32 {
        let d = q - p;
        let len = d.length().max(1e-6);
        let mut best = 1.0f32;
        let mut test = |s: &Seg, solid_near_origin: bool| {
            let e = s.b - s.a;
            let den = d.x * e.y - d.y * e.x;
            if den.abs() < 1e-9 {
                return;
            }
            let t = ((s.a.x - p.x) * e.y - (s.a.y - p.y) * e.x) / den;
            let u = ((s.a.x - p.x) * d.y - (s.a.y - p.y) * d.x) / den;
            if t < 0.0
                || t > best
                || !(0.0..=1.0).contains(&u)
                || (!solid_near_origin && t * len < own)
            {
                return;
            }
            if s.h.is_some_and(|h| ys + (yt - ys) * t > h) {
                return;
            }
            best = t;
        };
        self.static_segs_along(p, q, &mut test);
        self.dynamic(None, |hull| {
            // Combat applies an occupied vehicle's damage protection. Its own
            // hull must not make the carried actor invisible and invulnerable.
            if !hull.contains(p) && !hull.contains(q) {
                for seg in hull.segments() {
                    test(&seg, true);
                }
            }
        });
        best
    }

    /// solCover(): behind the nearest cover segment within 8 m, as seen from the target.
    pub fn cover_spot(&self, from: Vec2, tg: Vec2) -> Option<Vec2> {
        let mut best = None;
        let mut bd = 8.0;
        let dt = from.distance(tg);
        let mut candidates: Vec<Seg> = self.0.near_segs(from).copied().collect();
        self.dynamic(None, |hull| candidates.extend(hull.segments()));
        for g in &candidates {
            if g.h.is_none() {
                continue;
            }
            let m = (g.a + g.b) / 2.0;
            let tv = m - tg;
            let tl = tv.length().max(1e-6);
            if tl > dt || tl < 4.0 {
                continue;
            }
            let c = m + tv / tl * 0.7;
            let d = c.distance(from);
            if d < bd && self.0.bld_at(c).is_none() && self.los(from, c) {
                best = Some(c);
                bd = d;
            }
        }
        best
    }

    /// findCover(): nearest segment within r — `Some(low)` where low = crouch-height cover.
    pub fn find_cover(&self, p: Vec2, r: f32) -> Option<bool> {
        let mut best: Option<(f32, Option<f32>)> = None;
        for s in self.0.near_segs(p) {
            let d = seg_dist(p, s.a, s.b);
            if d < r && best.is_none_or(|b| d < b.0) {
                best = Some((d, s.h));
            }
        }
        self.1.near(p, r, |s| {
            let d = seg_dist(p, s.a, s.b);
            if d < r && best.is_none_or(|b| d < b.0) {
                best = Some((d, s.h));
            }
        });
        self.dynamic(None, |hull| {
            for s in hull.segments() {
                let d = seg_dist(p, s.a, s.b);
                if d < r && best.is_none_or(|b| d < b.0) {
                    best = Some((d, s.h));
                }
            }
        });
        best.map(|b| b.1.unwrap_or(99.0) <= 1.6)
    }

    pub fn building_at(&self, p: Vec2) -> Option<Footprint> {
        self.0.bld_at(p).map(|b| Footprint {
            min: b.min,
            max: b.max,
        })
    }
}

impl CombatWorld for Obstacles {
    fn shot_ray(&self, p: Vec2, q: Vec2, ys: f32, yt: f32, own: f32) -> f32 {
        Obstacles::shot_ray(self, p, q, ys, yt, own)
    }
    fn cover_spot(&self, from: Vec2, target: Vec2) -> Option<Vec2> {
        Obstacles::cover_spot(self, from, target)
    }
    fn building_at(&self, p: Vec2) -> Option<Footprint> {
        Obstacles::building_at(self, p)
    }
    fn cover_near(&self, p: Vec2, r: f32) -> bool {
        self.find_cover(p, r).is_some()
    }
}
