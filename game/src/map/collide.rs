//! Collision and line-of-sight queries over the wall / cover segments of the map — ports of
//! `collide`, `los`, `shotRay`, `solCover`, `findCover` and `bldAt` (main.ts). Walls are
//! segments on an 8 m grid; a circle is pushed out of every segment near it.

use bevy::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;

use super::data::{CELL, Seg, WorldData, seg_dist};
use crate::combat::squad::Footprint;
use crate::combat::world::CombatWorld;

/// The map's collision world. Cheap to clone (shared).
#[derive(Resource, Clone)]
pub struct Obstacles(pub Arc<WorldData>);

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
    pub fn world(&self) -> &WorldData {
        &self.0
    }

    /// Segments whose grid cells the line p→q passes (los / shotRay walk).
    fn segs_along(&self, p: Vec2, q: Vec2, mut f: impl FnMut(&Seg)) {
        let d = q - p;
        let n = (d.length() / (CELL * 0.5)).ceil() as i32 + 1;
        let mut seen: HashSet<u32> = HashSet::new();
        for i in 0..=n {
            let pt = p + d * (i as f32 / n as f32);
            let k = ((pt.x / CELL).floor() as i32, (pt.y / CELL).floor() as i32);
            for &s in self.0.segs_in_cell(k) {
                if seen.insert(s) {
                    f(&self.0.segs[s as usize]);
                }
            }
        }
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
        }
        p.clamp(self.0.min, self.0.max)
    }

    /// Would a circle at p overlap a wall, a building or the map edge?
    pub fn blocked(&self, p: Vec2, r: f32) -> bool {
        p.x < self.0.min.x + r
            || p.y < self.0.min.y + r
            || p.x > self.0.max.x - r
            || p.y > self.0.max.y - r
            || !self.0.free(p, r)
            || self.0.bld_at(p).is_some()
    }

    /// los(): no wall or cover crosses p→q.
    pub fn los(&self, p: Vec2, q: Vec2) -> bool {
        let mut clear = true;
        self.segs_along(p, q, |s| {
            if clear && seg_inter(p, q, s.a, s.b).is_some() {
                clear = false;
            }
        });
        clear
    }

    /// First wall / cover crossing p→q, for a ray at height y0 at p rising/falling to y1
    /// at q. Cover lower than the ray is skipped.
    pub fn segment_hit(&self, p: Vec2, q: Vec2, y0: f32, y1: f32) -> Option<SegmentHit> {
        let mut best: Option<SegmentHit> = None;
        self.segs_along(p, q, |s| {
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
        self.segs_along(p, q, |s| {
            let e = s.b - s.a;
            let den = d.x * e.y - d.y * e.x;
            if den.abs() < 1e-9 {
                return;
            }
            let t = ((s.a.x - p.x) * e.y - (s.a.y - p.y) * e.x) / den;
            let u = ((s.a.x - p.x) * d.y - (s.a.y - p.y) * d.x) / den;
            if t < 0.0 || t > best || !(0.0..=1.0).contains(&u) || t * len < own {
                return;
            }
            if s.h.is_some_and(|h| ys + (yt - ys) * t > h) {
                return;
            }
            best = t;
        });
        best
    }

    /// solCover(): behind the nearest cover segment within 8 m, as seen from the target.
    pub fn cover_spot(&self, from: Vec2, tg: Vec2) -> Option<Vec2> {
        let mut best = None;
        let mut bd = 8.0;
        let dt = from.distance(tg);
        for g in self.0.near_segs(from) {
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
