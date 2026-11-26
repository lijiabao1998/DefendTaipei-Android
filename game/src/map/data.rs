//! The 萬華 core map: OpenStreetMap footprints and roads (`public/data/wanhua.json`, © OpenStreetMap
//! contributors, ODbL) planned exactly like `buildMap` in the original `src/core/main.ts`:
//! storeys from OSM levels (else 透天 2–3F / 公寓 4–5F / a few taller), a door on the longest
//! edge, 騎樓 arcades on road-facing edges (`arcade.ts`), shop types and signs, wall segments for
//! collision / cover, window slots matching the painted wall texture, water tanks, rooftop
//! classes, plus the v23.9 street-frontage row-house infill (`infill.ts`).
//!
//! Planning is pure data (no meshes): the chunk builder turns it into geometry on demand.

use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;

/// Ground-floor height (shop + arcade), upper-floor height, arcade depth, target bay width.
pub const GF: f32 = 3.8;
pub const FH: f32 = 3.2;
pub const ARC: f32 = 3.6;
pub const BAY: f32 = 4.5;
pub const COL: f32 = 0.5;
/// Street-door opening, shared by the collision planner and the visible entrance frame.
pub const DOOR_W: f32 = 1.6;
pub const DOOR_H: f32 = 2.4;
/// Segment grid cell (main.ts CELL).
pub const CELL: f32 = 8.0;
/// Hand-built core (the original 1.1 × 1.0 km bbox, 西門町–龍山寺).
pub const CORE: [f32; 4] = [-555.0, -497.0, 555.0, 497.0];

pub const ATTRIBUTION: &str = "地圖資料 © OpenStreetMap contributors（ODbL）";

/// Road width in metres by OSM highway type (`_link` roads use their parent type). world.ts ROADW.
pub fn road_w(t: &str) -> Option<f32> {
    Some(match t.trim_end_matches("_link") {
        "trunk" => 18.0,
        "primary" => 16.0,
        "secondary" => 12.0,
        "tertiary" => 9.0,
        "residential" | "unclassified" => 6.0,
        "living_street" => 5.0,
        "service" => 4.0,
        "pedestrian" => 6.0,
        "footway" => 2.5,
        _ => return None,
    })
}

#[derive(Deserialize)]
struct RawBld {
    p: Vec<[f32; 2]>,
    #[serde(default)]
    l: u32,
    #[serde(default)]
    n: String,
    #[serde(default)]
    s: String,
}
#[derive(Deserialize)]
struct RawRoad {
    p: Vec<[f32; 2]>,
    t: String,
    #[serde(default)]
    n: String,
}
#[derive(Deserialize)]
struct Raw {
    buildings: Vec<RawBld>,
    roads: Vec<RawRoad>,
}

/// The original global `rnd()` (LCG, seed 20261024) so the city comes out the same way.
pub struct WorldRng(pub u32);
impl WorldRng {
    pub fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 as f64 / 4294967296.0) as f32
    }
}

/// Lehmer generator used for per-building seeds (`sd = sd * 16807 % 2147483647`).
pub struct Park(pub u64);
impl Park {
    pub fn f(&mut self) -> f32 {
        self.0 = (self.0 * 16807) % 2147483647;
        (self.0 as f64 / 2147483647.0) as f32
    }
}

/// A wall or cover segment. `h = None` is a full wall; Some(h) is cover of that height.
#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub a: Vec2,
    pub b: Vec2,
    pub h: Option<f32>,
    /// Building that owns it (walls) — for the see-through fade and roof cutaway.
    pub bld: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub struct WallBox {
    pub a: Vec2,
    pub c: Vec2,
    pub y0: f32,
    pub y1: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Win {
    pub p: Vec3,
    pub ry: f32,
    pub floor: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Arcade {
    pub a: Vec2,
    pub u: Vec2,
    pub n: Vec2,
    pub dl: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Door {
    pub p: Vec2,
    /// Outward normal.
    pub n: Vec2,
}

impl Door {
    /// Local +X runs across the opening; local +Z points out towards the street.
    pub fn tangent(self) -> Vec2 {
        Vec2::new(self.n.y, -self.n.x)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SignSpec {
    pub p: Vec3,
    pub ry: f32,
    pub w: f32,
    /// Atlas cell (textures.rs SIGN_*).
    pub idx: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoofClass {
    Row,
    Walkup,
    Big,
    None,
}

#[derive(Clone, Debug)]
pub struct Bld {
    pub id: u32,
    pub pts: Vec<Vec2>,
    pub area: f32,
    pub h: f32,
    pub lv: u32,
    pub kind: &'static str,
    pub name: String,
    pub c: Vec2,
    pub min: Vec2,
    pub max: Vec2,
    pub fill: bool,
    pub door: Option<Door>,
    pub arcade: Option<Arcade>,
    pub sign: Option<SignSpec>,
    pub walls: Vec<WallBox>,
    pub wins: Vec<Win>,
    pub wall_mat: usize,
    pub roof_mat: usize,
    /// Water tank on the roof: position and stand height.
    pub tank: Option<(Vec2, f32)>,
    pub rcls: RoofClass,
    pub shack: bool,
    pub seed7: u64,
    pub seed8: u64,
}

#[derive(Clone, Debug)]
pub struct Road {
    pub p: Vec<Vec2>,
    pub t: String,
    pub w: f32,
    pub name: String,
}

pub struct WorldData {
    pub blds: Vec<Bld>,
    pub roads: Vec<Road>,
    pub segs: Vec<Seg>,
    seg_grid: HashMap<(i32, i32), Vec<u32>>,
    bld_grid: HashMap<(i32, i32), Vec<u32>>,
    road_grid: HashMap<(i32, i32), Vec<(u32, u32)>>,
    /// Playable bounds (road bbox inset 10 m, clipped to the core).
    pub min: Vec2,
    pub max: Vec2,
    pub spawn: Vec2,
    pub props: Vec<super::street::Prop>,
    /// Deterministic ground-floor furniture, shared by rendering and scavenging.
    pub interiors: Vec<super::interiors::Fixture>,
    /// Furniture is appended last and never ignored by the own-cover shot rule.
    pub(crate) interior_seg_start: usize,
    /// v24.3 the four street corners held by ROC remnants (sandbags already in `segs`).
    pub remnants: Vec<super::fortify::RemnantSite>,
    /// The two 解放軍 checkpoints near the start (`decay.ts` checkpoints).
    pub checkpoints: Vec<super::fortify::Checkpoint>,
    /// v24.5 abandoned traffic, scooters, barriers and litter (cover already in `segs`).
    pub wrecks: Vec<super::wreckage::Wreck>,
}

const BG: f32 = 16.0;
const RG: f32 = 24.0;

pub fn pip(p: Vec2, pts: &[Vec2]) -> bool {
    let mut c = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let (pi, pj) = (pts[i], pts[j]);
        if (pi.y > p.y) != (pj.y > p.y) && p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x
        {
            c = !c;
        }
        j = i;
    }
    c
}

pub fn area(pts: &[Vec2]) -> f32 {
    crate::geom::signed_area(pts).abs()
}

pub fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    let l2 = d.length_squared().max(1e-12);
    let t = ((p - a).dot(d) / l2).clamp(0.0, 1.0);
    (a + d * t - p).length()
}

fn hash2(a: f32, b: f32) -> f32 {
    let v = (a * 12.9898 + b * 78.233).sin() * 43_758.547;
    v - v.floor()
}

fn cells(min: Vec2, max: Vec2, size: f32) -> impl Iterator<Item = (i32, i32)> {
    let (x0, x1) = ((min.x / size).floor() as i32, (max.x / size).floor() as i32);
    let (z0, z1) = ((min.y / size).floor() as i32, (max.y / size).floor() as i32);
    (x0..=x1).flat_map(move |x| (z0..=z1).map(move |z| (x, z)))
}

impl WorldData {
    pub fn load() -> Self {
        let raw: Raw = serde_json::from_slice(include_bytes!("../../../assets/data/wanhua.json"))
            .expect("wanhua.json");
        let roads: Vec<Road> = raw
            .roads
            .into_iter()
            .filter(|r| r.p.len() >= 2)
            .map(|r| Road {
                p: r.p.iter().map(|q| Vec2::new(q[0], q[1])).collect(),
                w: road_w(&r.t).unwrap_or(3.0),
                t: r.t,
                name: r.n,
            })
            .collect();
        let mut w = WorldData {
            blds: Vec::new(),
            roads,
            segs: Vec::new(),
            seg_grid: HashMap::new(),
            bld_grid: HashMap::new(),
            road_grid: HashMap::new(),
            min: Vec2::ZERO,
            max: Vec2::ZERO,
            spawn: Vec2::ZERO,
            props: Vec::new(),
            interiors: Vec::new(),
            interior_seg_start: usize::MAX,
            remnants: Vec::new(),
            checkpoints: Vec::new(),
            wrecks: Vec::new(),
        };
        for (ri, r) in w.roads.iter().enumerate() {
            let hw = r.w / 2.0;
            for i in 0..r.p.len() - 1 {
                let (a, b) = (r.p[i], r.p[i + 1]);
                for k in cells(a.min(b) - hw, a.max(b) + hw, RG) {
                    w.road_grid
                        .entry(k)
                        .or_default()
                        .push((ri as u32, i as u32));
                }
            }
        }
        let (mut lo, mut hi) = (Vec2::splat(1e9), Vec2::splat(-1e9));
        for r in &w.roads {
            for p in &r.p {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        w.min = (lo + 10.0).max(Vec2::new(CORE[0], CORE[1]));
        w.max = (hi - 10.0).min(Vec2::new(CORE[2], CORE[3]));

        let mut src: Vec<(Vec<Vec2>, u32, String, String, bool)> = raw
            .buildings
            .into_iter()
            .map(|b| {
                (
                    b.p.iter().map(|q| Vec2::new(q[0], q[1])).collect(),
                    b.l,
                    b.n,
                    b.s,
                    false,
                )
            })
            .collect();
        let fill = frontage_infill(&src, &w.roads, Vec2::new(70.0, 40.0), 220.0, 200);
        src.extend(
            fill.into_iter()
                .map(|(p, l)| (p, l, String::new(), String::new(), true)),
        );
        // v24.6 block interiors of the whole core (blocks.ts; the original streams these per district tile)
        let fp: Vec<(Vec<Vec2>, bool)> = src.iter().map(|b| (b.0.clone(), b.4)).collect();
        let bb = [CORE[0] + 4.0, CORE[1] + 4.0, CORE[2] - 4.0, CORE[3] - 4.0];
        let lots = super::blocks::block_infill(&fp, &w.roads, bb, 2400);
        src.extend(
            lots.into_iter()
                .map(|(p, l)| (p, l, String::new(), String::new(), true)),
        );
        w.plan_buildings(src);
        w.pick_spawn();
        super::fortify::install(&mut w);
        w.props = w.plan_props();
        super::wreckage::install(&mut w);
        super::interiors::install(&mut w);
        w
    }

    /// Distance from (x, z) to the nearest road edge (negative = on the carriageway).
    pub fn road_dist(&self, p: Vec2) -> f32 {
        let mut best = 1e9f32;
        let k = ((p.x / RG).floor() as i32, (p.y / RG).floor() as i32);
        let mut any = false;
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(v) = self.road_grid.get(&(k.0 + dx, k.1 + dz)) {
                    for &(ri, i) in v {
                        any = true;
                        let r = &self.roads[ri as usize];
                        let d = seg_dist(p, r.p[i as usize], r.p[i as usize + 1]) - r.w / 2.0;
                        best = best.min(d);
                    }
                }
            }
        }
        if !any {
            best = best.min(RG);
        }
        best
    }

    pub fn add_seg(&mut self, s: Seg) {
        let i = self.segs.len() as u32;
        for k in cells(s.a.min(s.b), s.a.max(s.b), CELL) {
            self.seg_grid.entry(k).or_default().push(i);
        }
        self.segs.push(s);
    }

    /// Segments in the 3 × 3 cells around p (nearSegs).
    pub fn near_segs(&self, p: Vec2) -> impl Iterator<Item = &Seg> {
        let (cx, cz) = ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32);
        let mut ids: Vec<u32> = Vec::new();
        for i in -1..=1 {
            for j in -1..=1 {
                if let Some(v) = self.seg_grid.get(&(cx + i, cz + j)) {
                    ids.extend_from_slice(v);
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter().map(move |i| &self.segs[i as usize])
    }

    pub fn segs_in_cell(&self, k: (i32, i32)) -> &[u32] {
        self.seg_grid.get(&k).map_or(&[], |v| v.as_slice())
    }

    /// Building at p (bldAt).
    pub fn bld_at(&self, p: Vec2) -> Option<&Bld> {
        let k = ((p.x / BG).floor() as i32, (p.y / BG).floor() as i32);
        self.bld_grid
            .get(&k)?
            .iter()
            .map(|&i| &self.blds[i as usize])
            .find(|b| {
                p.x > b.min.x && p.x < b.max.x && p.y > b.min.y && p.y < b.max.y && pip(p, &b.pts)
            })
    }

    /// Buildings whose bbox overlaps the rect.
    pub fn blds_in(&self, min: Vec2, max: Vec2) -> Vec<u32> {
        let mut v: Vec<u32> = cells(min, max, BG)
            .filter_map(|k| self.bld_grid.get(&k))
            .flatten()
            .copied()
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    fn plan_buildings(&mut self, src: Vec<(Vec<Vec2>, u32, String, String, bool)>) {
        let mut rng = WorldRng(20261024);
        let mut seed0: Option<u32> = None;
        let mut pending_segs: Vec<Seg> = Vec::new();
        for (id, (pts, l, name, s, fill)) in src.into_iter().enumerate() {
            // v23.9 infill lots come last and use their own RNG stream
            if fill && seed0.is_none() {
                seed0 = Some(rng.0);
                rng.0 = 0x5f11;
            }
            let mut pts = pts;
            if pts.len() > 3 && pts.first() == pts.last() {
                pts.pop();
            }
            if pts.len() < 3 {
                continue;
            }
            let a_ = area(&pts);
            if !(12.0..=6000.0).contains(&a_) {
                continue;
            }
            let idf = id as f32;
            let hh = |k: f32| hash2(idf, k);
            if l == 0 {
                rng.f();
            }
            let rr = hh(1.0);
            let lv = if l > 0 {
                l.min(if rr < 0.45 { 3 } else { 9 })
            } else if rr < 0.5 {
                2 + (hh(2.0) * 2.0) as u32
            } else if rr < 0.88 {
                4 + (hh(2.0) * 2.0) as u32
            } else {
                6 + (hh(2.0) * 2.0) as u32
            };
            let h = GF + (lv.min(7) as f32 - 1.0) * FH;
            let kind = shop_type(&s, &mut rng);
            let n = pts.len() as f32;
            let c = pts.iter().copied().sum::<Vec2>() / n;
            let (mut min, mut max) = (Vec2::splat(1e9), Vec2::splat(-1e9));
            for p in &pts {
                min = min.min(*p);
                max = max.max(*p);
            }
            let bid = self.blds.len() as u32;
            // door: longest edge (infill: edge 0 is the street front)
            let (mut di, mut dl) = (0usize, 0.0f32);
            for i in 0..pts.len() {
                let l = pts[i].distance(pts[(i + 1) % pts.len()]);
                if l > dl {
                    dl = l;
                    di = i;
                }
            }
            if fill {
                di = 0;
                dl = pts[0].distance(pts[1]);
            }
            let mut b = Bld {
                id: bid,
                pts: pts.clone(),
                area: a_,
                h,
                lv,
                kind,
                name,
                c,
                min,
                max,
                fill,
                door: None,
                arcade: None,
                sign: None,
                walls: Vec::new(),
                wins: Vec::new(),
                wall_mat: id % super::textures::WALL_N,
                roof_mat: 0,
                tank: None,
                rcls: RoofClass::None,
                shack: false,
                seed7: 0,
                seed8: 0,
            };
            let mut segs: Vec<Seg> = Vec::new();
            let wall = |a: Vec2, cc: Vec2| Seg {
                a,
                b: cc,
                h: None,
                bld: Some(bid),
            };
            let np = pts.len();
            for i in 0..np {
                let (a, cc) = (pts[i], pts[(i + 1) % np]);
                if i == di && dl > 2.6 && a_ > 20.0 {
                    let u = (cc - a) / dl;
                    let m = dl / 2.0;
                    let d1 = a + u * (m - DOOR_W / 2.0);
                    let d2 = a + u * (m + DOOR_W / 2.0);
                    let mut mid = (d1 + d2) / 2.0;
                    let mut nrm = Vec2::new(-u.y, u.x);
                    if pip(mid + nrm * 0.6, &pts) {
                        nrm = -nrm;
                    }
                    let arc_ok = if h > 6.0 && rng.f() < 0.7 {
                        self.arcade_fits(&pts, a, cc, u, nrm, dl, h)
                    } else {
                        None
                    };
                    if let Some(arc) = arc_ok {
                        // upper floors flush with the footprint edge; ground floor recessed behind columns
                        b.push_wall(a, cc, GF, h, true, &pts, fill, i == di);
                        let ia = a - nrm * ARC;
                        let ic = cc - nrm * ARC;
                        let da = d1 - nrm * ARC;
                        let dc = d2 - nrm * ARC;
                        b.push_wall(ia, da, 0.0, GF, false, &pts, fill, false);
                        b.push_wall(dc, ic, 0.0, GF, false, &pts, fill, false);
                        b.push_wall(da, dc, DOOR_H, GF, false, &pts, fill, false);
                        segs.push(wall(ia, da));
                        segs.push(wall(dc, ic));
                        mid = (da + dc) / 2.0;
                        // columns on the street edge (buildArcade)
                        let nb = ((dl - COL) / BAY).round().max(1.0) as usize;
                        let step = (dl - 0.7) / nb as f32;
                        for k in 0..=nb {
                            let s = 0.35 + k as f32 * step;
                            let p = a + u * s - nrm * 0.3;
                            if pip(p, &pts) {
                                segs.push(Seg {
                                    a: p - u * COL / 2.0,
                                    b: p + u * COL / 2.0,
                                    h: None,
                                    bld: Some(bid),
                                });
                            }
                        }
                        b.arcade = Some(arc);
                    } else {
                        b.push_wall(a, d1, 0.0, h, true, &pts, fill, true);
                        b.push_wall(d2, cc, 0.0, h, true, &pts, fill, true);
                        b.push_wall(d1, d2, DOOR_H, h, true, &pts, fill, true);
                        segs.push(wall(a, d1));
                        segs.push(wall(d2, cc));
                    }
                    b.door = Some(Door { p: mid, n: nrm });
                    // shop sign (fascia above the arcade / over the door); infill gets none
                    if (h > 6.0 && rng.f() < 0.6 && !fill) || kind != "home" {
                        let idx = match super::textures::sign_index(kind) {
                            Some(i) => i,
                            None => {
                                (rng.f() * super::textures::SIGN_PARODY as f32) as usize
                                    % super::textures::SIGN_PARODY
                            }
                        };
                        let (sy, so) = if b.arcade.is_some() {
                            (GF + 0.75, ARC + 0.16)
                        } else {
                            (3.0, 0.16)
                        };
                        let sp = mid + nrm * so;
                        b.sign = Some(SignSpec {
                            p: Vec3::new(sp.x, sy, sp.y),
                            ry: nrm.x.atan2(nrm.y),
                            w: (dl * 0.8).min(5.0),
                            idx,
                        });
                    }
                } else {
                    b.push_wall(a, cc, 0.0, h, true, &pts, fill, i == di);
                    segs.push(wall(a, cc));
                }
            }
            b.roof_mat =
                (hh(9.0) * super::textures::ROOF_N as f32) as usize % super::textures::ROOF_N;
            // rooftop additions 頂樓加蓋 + water tanks
            b.rcls = roof_class(&pts, lv, b.kind, &b.name);
            if a_ > 40.0 {
                rng.f();
            }
            b.shack = a_ > 30.0
                && (b.rcls != RoofClass::Row || hh(3.0) < 0.9)
                && b.rcls != RoofClass::None;
            b.seed7 = (hh(7.0) * 1e6) as u64 + 1;
            b.seed8 = (hh(8.0) * 1e6) as u64 + 7;
            let tq = rng.f();
            if !fill && (tq < 0.5 || hh(4.0) < if lv <= 3 { 0.5 } else { 0.0 }) {
                let st = if lv <= 3 { 1.6 } else { 0.0 };
                let tp = if tq < 0.5 {
                    c + Vec2::new(rng.f() - 0.5, rng.f() - 0.5) * 2.0
                } else {
                    c + Vec2::new(hh(5.0) - 0.5, hh(6.0) - 0.5) * 2.0
                };
                if pip(tp, &pts) {
                    b.tank = Some((tp, st));
                }
            }
            pending_segs.extend(segs);
            for k in cells(b.min, b.max, BG) {
                self.bld_grid.entry(k).or_default().push(bid);
            }
            self.blds.push(b);
        }
        for s in pending_segs {
            self.add_seg(s);
        }
    }

    /// arcadeFits: the edge can take a 騎樓 frame (deep enough, faces a road, not in the roadway).
    #[allow(clippy::too_many_arguments)]
    fn arcade_fits(
        &self,
        pts: &[Vec2],
        a: Vec2,
        c: Vec2,
        u: Vec2,
        n: Vec2,
        dl: f32,
        h: f32,
    ) -> Option<Arcade> {
        if dl < 3.5 || h < GF + FH - 0.1 {
            return None;
        }
        let mid = (a + c) / 2.0;
        let at = |s: f32, d: f32| a + u * s - n * d;
        for p in [
            at(0.6, ARC + 0.3),
            at(dl - 0.6, ARC + 0.3),
            at(dl / 2.0, ARC + 2.5),
            at(0.4, 0.3),
            at(dl - 0.4, 0.3),
        ] {
            if !pip(p, pts) {
                return None;
            }
        }
        if self.road_dist(mid + n * 1.5) > 2.5 {
            return None;
        }
        for p in [a, c, mid] {
            if self.road_dist(p) < 0.2 {
                return None;
            }
        }
        Some(Arcade { a, u, n, dl })
    }

    /// The original spawns in a furnished home near the middle of the detailed core; we start
    /// on the street in front of that home's door.
    fn pick_spawn(&mut self) {
        let homes: Vec<&Bld> = self
            .blds
            .iter()
            .filter(|b| {
                !b.fill
                    && b.door.is_some()
                    && b.kind == "home"
                    && b.c.x.abs() < 167.0
                    && b.c.y.abs() < 167.0
            })
            .collect();
        if homes.is_empty() {
            self.spawn = Vec2::new(70.0, 40.0);
            return;
        }
        let mid = homes[homes.len() / 2].c;
        let mut cands: Vec<&Bld> = homes
            .iter()
            .copied()
            .filter(|b| b.area > 70.0 && b.area < 400.0)
            .collect();
        cands.sort_by(|a, b| a.c.distance(mid).total_cmp(&b.c.distance(mid)));
        for b in cands.iter().chain(homes.iter()) {
            let d = b.door.unwrap();
            let out = if b.arcade.is_some() { ARC + 2.0 } else { 2.0 };
            let p = d.p + d.n * out;
            if self.bld_at(p).is_none() && self.free(p, 0.6) {
                self.spawn = p;
                return;
            }
        }
        self.spawn = mid;
    }

    /// No wall within r of p.
    pub fn free(&self, p: Vec2, r: f32) -> bool {
        self.near_segs(p).all(|s| seg_dist(p, s.a, s.b) >= r)
    }
}

impl Bld {
    /// wallBox: one wall slab (0.25 m thick, centred on the footprint edge) + the window slots
    /// that line up with the painted wall-texture windows.
    #[allow(clippy::too_many_arguments)]
    fn push_wall(
        &mut self,
        a: Vec2,
        c: Vec2,
        y0: f32,
        y1: f32,
        win: bool,
        pts: &[Vec2],
        fill: bool,
        door_edge: bool,
    ) {
        let l = a.distance(c);
        if l < 0.05 {
            return;
        }
        self.walls.push(WallBox { a, c, y0, y1 });
        if !(win && self.h > GF + 1.0 && (!fill || door_edge)) {
            return;
        }
        let u = (c - a) / l;
        let mut n = Vec2::new(-u.y, u.x);
        let mut back = false;
        if pip((a + c) / 2.0 + n * 0.5, pts) {
            n = -n;
            back = true;
        }
        let t = 3.2 * (l + 0.25) / l;
        for k in 0..40 {
            let sd = if back {
                l + 0.125 - (k as f32 + 0.5) * t
            } else {
                -0.125 + (k as f32 + 0.5) * t
            };
            if if back { sd < 0.9 } else { sd > l - 0.9 } {
                break;
            }
            if sd < 0.9 || sd > l - 0.9 {
                continue;
            }
            let mut f = 1;
            while GF + (f as f32 - 1.0) * FH + 2.7 < self.h {
                let p = a + u * sd + n * 0.126;
                let y = GF + (f as f32 - 1.0) * FH + 1.95;
                if y > y0 && y < y1 {
                    self.wins.push(Win {
                        p: Vec3::new(p.x, y, p.y),
                        ry: n.x.atan2(n.y),
                        floor: f,
                    });
                }
                f += 1;
            }
        }
    }
}

fn shop_type(s: &str, rng: &mut WorldRng) -> &'static str {
    if s.contains("convenience") || s.contains("supermarket") {
        return "store";
    }
    if s.contains("pharmacy") || s.contains("chemist") {
        return "pharmacy";
    }
    if s.contains("police") {
        return "police";
    }
    let r = rng.f();
    if r < 0.06 {
        "store"
    } else if r < 0.1 {
        "pharmacy"
    } else if r < 0.115 {
        "police"
    } else {
        "home"
    }
}

/// rooftin.ts roofClass: floors, footprint area and lot shape (oriented bbox).
pub fn roof_class(pts: &[Vec2], lv: u32, kind: &str, name: &str) -> RoofClass {
    const NONRES: [&str; 12] = [
        "school", "temple", "market", "hospital", "police", "station", "office", "hotel",
        "parking", "寺", "宮", "市場",
    ];
    let k = format!("{kind} {name}");
    if NONRES.iter().any(|w| k.contains(w)) {
        return RoofClass::None;
    }
    let a = area(pts);
    if lv >= 6 || a > 600.0 {
        return RoofClass::Big;
    }
    let (mut best, mut th) = (0.0f32, 0.0f32);
    for i in 0..pts.len() {
        let d = pts[(i + 1) % pts.len()] - pts[i];
        if d.length() > best {
            best = d.length();
            th = d.y.atan2(d.x);
        }
    }
    let (su, cu) = th.sin_cos();
    let (mut v0, mut v1) = (1e9f32, -1e9f32);
    for p in pts {
        let v = -p.x * su + p.y * cu;
        v0 = v0.min(v);
        v1 = v1.max(v);
    }
    let d = v1 - v0;
    if lv <= 5 && (6.0..=22.0).contains(&d) && a <= 450.0 {
        RoofClass::Row
    } else if lv <= 5 {
        RoofClass::Walkup
    } else {
        RoofClass::Big
    }
}

/// infill.ts frontageInfill: fill empty street frontage near the spawn with 透天 / 公寓 lots
/// (4.2–7 m wide, 6.5–16 m deep), never over a footprint, a road, or an OSM door's corridor.
pub fn frontage_infill(
    blds: &[(Vec<Vec2>, u32, String, String, bool)],
    roads: &[Road],
    centre: Vec2,
    radius: f32,
    max: usize,
) -> Vec<(Vec<Vec2>, u32)> {
    const C: f32 = 16.0;
    type Grid = HashMap<(i32, i32), Vec<usize>>;
    let add = |g: &mut Grid, lo: Vec2, hi: Vec2, v: usize| {
        for k in cells(lo, hi, C) {
            g.entry(k).or_default().push(v);
        }
    };
    let near = |g: &Grid, lo: Vec2, hi: Vec2| -> Vec<usize> {
        let mut v: Vec<usize> = cells(lo, hi, C)
            .filter_map(|k| g.get(&k))
            .flatten()
            .copied()
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let bbox = |p: &[Vec2]| {
        let (mut lo, mut hi) = (Vec2::splat(1e9), Vec2::splat(-1e9));
        for q in p {
            lo = lo.min(*q);
            hi = hi.max(*q);
        }
        (lo, hi)
    };
    let mut polys: Vec<Vec<Vec2>> = Vec::new(); // keep-out polygons (bg)
    let mut bg: Grid = HashMap::new();
    let mut og: Grid = HashMap::new(); // OSM footprints only (index into polys)
    for (p, ..) in blds {
        if p.len() < 3 {
            continue;
        }
        let (lo, hi) = bbox(p);
        let i = polys.len();
        polys.push(p.clone());
        add(&mut bg, lo, hi, i);
        add(&mut og, lo, hi, i);
        // keep a 4 m × 12 m corridor out of the OSM door open
        let (mut dl, mut di) = (0.0, 0);
        for i in 0..p.len() {
            let l = p[i].distance(p[(i + 1) % p.len()]);
            if l > dl {
                dl = l;
                di = i;
            }
        }
        let (a, c) = (p[di], p[(di + 1) % p.len()]);
        let m = (a + c) / 2.0;
        let u = (c - a) / dl.max(1e-6);
        let mut n = Vec2::new(-u.y, u.x);
        if pip(m + n * 0.5, p) {
            n = -n;
        }
        let cor = vec![
            m - u * 2.0 - n * 0.5,
            m + u * 2.0 - n * 0.5,
            m + u * 2.0 + n * 12.0,
            m - u * 2.0 + n * 12.0,
        ];
        let (lo, hi) = bbox(&cor);
        let i = polys.len();
        polys.push(cor);
        add(&mut bg, lo, hi, i);
    }
    let mut rsegs: Vec<(Vec2, Vec2, f32)> = Vec::new();
    let mut rg: Grid = HashMap::new();
    for r in roads {
        let hw = road_w(&r.t).unwrap_or(2.5) / 2.0;
        for i in 0..r.p.len() - 1 {
            let (a, b) = (r.p[i], r.p[i + 1]);
            add(&mut rg, a.min(b) - hw, a.max(b) + hw, rsegs.len());
            rsegs.push((a, b, hw));
        }
    }
    let mut out: Vec<(Vec<Vec2>, u32)> = Vec::new();
    let edge_pts = |q: &[Vec2]| -> Vec<Vec2> {
        let mut v = Vec::new();
        for i in 0..4 {
            let (a, c) = (q[i], q[(i + 1) % 4]);
            for k in 0..=4 {
                v.push(a + (c - a) * (k as f32 / 4.0));
            }
        }
        v
    };
    let ok = |q: &[Vec2], polys: &Vec<Vec<Vec2>>, bg: &Grid| -> bool {
        let (lo, hi) = bbox(q);
        let ep = edge_pts(q);
        let cen = (lo + hi) / 2.0;
        for si in near(&rg, lo - 10.0, hi + 10.0) {
            let (a, b, hw) = rsegs[si];
            if ep.iter().any(|p| seg_dist(*p, a, b) < hw + 1.1) || seg_dist(cen, a, b) < hw + 1.1 {
                return false;
            }
        }
        let g = 1.6;
        for pi in near(&og, lo - g, hi + g) {
            let p = &polys[pi];
            for e in &ep {
                for j in 0..p.len() {
                    if seg_dist(*e, p[j], p[(j + 1) % p.len()]) < g {
                        return false;
                    }
                }
            }
        }
        for pi in near(bg, lo, hi) {
            let p = &polys[pi];
            if p.iter().any(|v| pip(*v, q)) {
                return false;
            }
            let (a, ux, vx) = (q[0], (q[1] - q[0]) / 4.0, (q[3] - q[0]) / 4.0);
            for k in 0..=4 {
                for j in 0..=4 {
                    if pip(a + ux * k as f32 + vx * j as f32, p) {
                        return false;
                    }
                }
            }
        }
        true
    };
    const FRONT: [&str; 7] = [
        "primary",
        "secondary",
        "tertiary",
        "residential",
        "unclassified",
        "living_street",
        "service",
    ];
    let md = |r: &Road| {
        r.p.iter()
            .map(|q| q.distance(centre))
            .fold(f32::MAX, f32::min)
    };
    let mut order: Vec<(usize, f32)> = roads.iter().enumerate().map(|(i, r)| (i, md(r))).collect();
    order.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (ri0, _) in order {
        let r = &roads[ri0];
        let ri = (ri0 + 1) as f32;
        if !FRONT.contains(&r.t.trim_end_matches("_link")) {
            continue;
        }
        let hw = road_w(&r.t).unwrap_or(3.0) / 2.0;
        for i in 0..r.p.len() - 1 {
            let (a, b) = (r.p[i], r.p[i + 1]);
            let l = a.distance(b);
            if l < 8.0 {
                continue;
            }
            let u = (b - a) / l;
            for side in [1.0f32, -1.0] {
                let n = Vec2::new(-u.y, u.x) * side;
                let off = hw + 1.6;
                let (mut s, mut cnt) = (2.5f32, 0u32);
                while s < l - 2.5 {
                    let fi = i as f32;
                    let h1 = hash2(ri * 31.0 + fi, s * 7.0 + side);
                    let h2 = hash2(ri * 17.0 + fi * 3.0, s * 5.0 - side);
                    let w = 4.2 + h1 * 2.8;
                    if s + w > l - 2.5 {
                        break;
                    }
                    let mut placed = false;
                    let mut out_of_range = false;
                    for d in [8.0 + h2 * 8.0, 9.0, 6.5] {
                        let p0 = a + u * s + n * off;
                        let q = [p0, p0 + u * w, p0 + u * w + n * d, p0 + n * d];
                        let m = p0 + (u * w + n * d) / 2.0;
                        if m.distance(centre) > radius {
                            out_of_range = true;
                            break;
                        }
                        if ok(&q, &polys, &bg) {
                            let e = 0.06;
                            let qq: Vec<Vec2> =
                                [q[0] + u * e, q[1] - u * e, q[2] - u * e, q[3] + u * e]
                                    .iter()
                                    .map(|p| (*p * 100.0).round() / 100.0)
                                    .collect();
                            let lv = if hash2(ri + cnt as f32, fi + s) < 0.55 {
                                3 + (hash2(s, ri) * 2.0) as u32
                            } else {
                                4 + (hash2(ri, s) * 3.0) as u32
                            };
                            let (lo, hi) = bbox(&qq);
                            let pi = polys.len();
                            polys.push(qq.clone());
                            add(&mut bg, lo, hi, pi);
                            out.push((qq, lv));
                            if out.len() >= max {
                                return out;
                            }
                            placed = true;
                            cnt += 1;
                            s += w;
                            break;
                        }
                    }
                    let _ = out_of_range;
                    if !placed {
                        s += 1.5;
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wanhua_plans() {
        let w = WorldData::load();
        assert!(w.blds.len() > 400, "{} buildings", w.blds.len());
        assert!(w.blds.iter().filter(|b| b.fill).count() > 50);
        assert!(w.blds.iter().filter(|b| b.arcade.is_some()).count() > 20);
        assert!(w.segs.len() > 2000);
        // spawn is on open ground
        assert!(w.bld_at(w.spawn).is_none());
        assert!(w.free(w.spawn, 0.6));
    }
}

#[cfg(test)]
mod dbg {
    use super::*;
    #[test]
    #[ignore]
    fn stats() {
        let w = WorldData::load();
        eprintln!(
            "blds {} roads {} spawn {:?} min {:?} max {:?}",
            w.blds.len(),
            w.roads.len(),
            w.spawn,
            w.min,
            w.max
        );
        let near = w
            .blds
            .iter()
            .filter(|b| b.c.distance(w.spawn) < 60.0)
            .count();
        eprintln!("near60 {near}");
        let (mut lo, mut hi) = (Vec2::splat(1e9), Vec2::splat(-1e9));
        for b in &w.blds {
            lo = lo.min(b.c);
            hi = hi.max(b.c);
        }
        eprintln!("bld bbox {lo:?} {hi:?}");
        eprintln!("props {}", w.props.len());
    }
}
