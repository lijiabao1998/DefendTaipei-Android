//! Street level: paved lots, asphalt from the OSM ways (main.ts buildMap roads), lane markings
//! and zebra crossings (roads.ts), street lamps (buildLamps), parked scooters / cars / taxis /
//! trucks (VehicleSystem spots) and street trees (trees.ts: 台灣欒樹, 樟樹, 茄苳 crowns of many
//! small lumpy clumps, darker at the heart, lighter on top).

pub mod furnishings;
mod markings;
pub use markings::{
    JunctionApproach, ParkingPlan, RoadMarkingPlan, approach_geometry, parking_geometry,
    parking_plans, road_marking_geometry, road_marking_plans,
};

use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use super::build::{Mat, Parts, WHITE, flat_poly};
use super::data::*;
use crate::geom::{self, MeshBuf, hex, place, scale_col};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropKind {
    Lamp,
    Scooter,
    Car,
    Taxi,
    Truck,
    Tree(u8),
}

#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub kind: PropKind,
    pub p: Vec2,
    pub ang: f32,
    pub seed: u32,
}

fn hash3(a: f32, b: f32, c: f32) -> f32 {
    let v = (a * 12.9898 + b * 78.233 + c * 37.719).sin() * 43_758.547;
    v - v.floor()
}

impl WorldData {
    /// Lamps every 26 m on alternating sides, scooter rows and parked cars (main.ts buildMap),
    /// street trees on the wider roads. Cars and trunks become cover segments.
    pub fn plan_props(&mut self) -> Vec<Prop> {
        let mut rng = WorldRng(0x0b5e_55ed);
        let mut out = Vec::new();
        let mut new_segs = Vec::new();
        for r in &self.roads {
            let w = r.w;
            if w < 6.0 {
                continue;
            }
            for i in 0..r.p.len() - 1 {
                let (a, b) = (r.p[i], r.p[i + 1]);
                let l = a.distance(b);
                if l < 4.0 {
                    continue;
                }
                let u = (b - a) / l;
                let n = Vec2::new(-u.y, u.x);
                let mut s = 2.0;
                while s < l - 2.0 {
                    let side = if rng.f() < 0.5 { 1.0 } else { -1.0 };
                    let p = a + u * s + n * (w / 2.0 + 0.4) * side;
                    if self.bld_at(p).is_none() {
                        let d = -n * side;
                        out.push(Prop {
                            kind: PropKind::Lamp,
                            p,
                            ang: d.x.atan2(d.y),
                            seed: 0,
                        });
                    }
                    s += 26.0;
                }
                if rng.f() < 0.12 {
                    let t = rng.f() * 0.6 + 0.2;
                    let side = if rng.f() < 0.5 { 1.0 } else { -1.0 };
                    let cnt = 2 + (rng.f() * 3.0) as i32;
                    for k in 0..cnt {
                        let p = a + (b - a) * t + u * k as f32 * 0.75 + n * (w / 2.0 - 0.7) * side;
                        if self.bld_at(p).is_none() {
                            let d = -n * side;
                            out.push(Prop {
                                kind: PropKind::Scooter,
                                p,
                                ang: d.x.atan2(d.y),
                                seed: (rng.f() * 1e6) as u32,
                            });
                        }
                    }
                }
                if w >= 9.0 && rng.f() < 0.08 {
                    let t = rng.f() * 0.6 + 0.2;
                    let side = if rng.f() < 0.5 { 1.0 } else { -1.0 };
                    let p = a + (b - a) * t + n * (w / 2.0 - 1.6) * side;
                    let q = rng.f();
                    let kind = if q < 0.15 {
                        PropKind::Truck
                    } else if q < 0.35 {
                        PropKind::Taxi
                    } else {
                        PropKind::Car
                    };
                    if self.bld_at(p).is_none() {
                        let (hl, hw, h) = if kind == PropKind::Truck {
                            (3.25, 1.15, 2.8)
                        } else {
                            (2.2, 0.9, 1.45)
                        };
                        let cs = [(-hw, -hl), (hw, -hl), (hw, hl), (-hw, hl)]
                            .map(|(x, y)| p + n * x + u * y);
                        for q in 0..4 {
                            new_segs.push(Seg {
                                a: cs[q],
                                b: cs[(q + 1) % 4],
                                h: Some(h),
                                bld: None,
                            });
                        }
                        out.push(Prop {
                            kind,
                            p,
                            ang: u.x.atan2(u.y),
                            seed: (rng.f() * 1e6) as u32,
                        });
                    }
                }
                // street trees on the sidewalk strip of wider roads
                if w >= 9.0 {
                    let mut s = 6.0 + rng.f() * 6.0;
                    while s < l - 3.0 {
                        let side = if rng.f() < 0.5 { 1.0 } else { -1.0 };
                        let p = a + u * s + n * (w / 2.0 + 1.1) * side;
                        if self.bld_at(p).is_none()
                            && self.free(p, 0.9)
                            && !out.iter().rev().take(40).any(|q| q.p.distance(p) < 3.0)
                        {
                            let k = (rng.f() * 3.0) as u8 % 3;
                            out.push(Prop {
                                kind: PropKind::Tree(k),
                                p,
                                ang: rng.f() * std::f32::consts::TAU,
                                seed: (rng.f() * 1e6) as u32,
                            });
                            let tr = 0.3;
                            new_segs.push(Seg {
                                a: p - u * tr,
                                b: p + u * tr,
                                h: Some(2.5),
                                bld: None,
                            });
                        }
                        s += 14.0 + rng.f() * 8.0;
                    }
                }
            }
        }
        for s in new_segs {
            self.add_seg(s);
        }
        out
    }
}

/// Road junction points (where two or more ways share a node).
pub fn junctions(roads: &[Road]) -> HashMap<(i32, i32), u32> {
    let mut m: HashMap<(i32, i32), u32> = HashMap::new();
    for r in roads {
        for p in &r.p {
            *m.entry(((p.x * 10.0).round() as i32, (p.y * 10.0).round() as i32))
                .or_default() += 1;
        }
    }
    m
}

/// Asphalt ribbons, markings and crossings for the road segments whose midpoint is in [min, max).
pub fn roads(
    w: &WorldData,
    junc: &HashMap<(i32, i32), u32>,
    min: Vec2,
    max: Vec2,
    lod_far: bool,
    out: &mut Parts,
) {
    let doors: Vec<Door> = w.blds.iter().filter_map(|b| b.door).collect();
    for r in &w.roads {
        let rw = r.w;
        for i in 0..r.p.len() - 1 {
            let (a, b) = (r.p[i], r.p[i + 1]);
            let m = (a + b) / 2.0;
            if m.x < min.x || m.y < min.y || m.x >= max.x || m.y >= max.y {
                continue;
            }
            let l = a.distance(b);
            if l < 0.1 {
                continue;
            }
            let ang = (b - a).y.atan2((b - a).x);
            // PlaneGeometry(len + w/2, w), wider roads drawn on top (0.02 + w·0.0005)
            let g = geom::plane(l + rw * 0.5, rw)
                .transformed(
                    place(m.x, 0.02 + rw * 0.0005, m.y, -ang) * Mat4::from_rotation_x(-FRAC_PI_2),
                )
                .map_uv(|_, p| [p[0] / 8.0, p[2] / 8.0]);
            out.get(Mat::Road).append(g);
            if lod_far {
                continue;
            }
            let segment_doors = relevant_doors(&doors, a, b, rw);
            road_wear(w, &segment_doors, junc, a, b, rw, out);
            if rw < 9.0 {
                continue;
            }
            sidewalks(w, &segment_doors, a, b, rw, out);
        }
    }
    if !lod_far {
        for plan in markings::road_marking_plans_in(w, min, max) {
            out.merge(road_marking_geometry(&plan));
        }
        for plan in markings::parking_plans_in(w, min, max) {
            out.merge(parking_geometry(&plan));
        }
        furnishings::build_chunk(w, min, max, out);
    }
}

/// Flat markings use the existing batched vertex-colour material. Heights stay below
/// lane markings on roads and just above paving on walks, avoiding z-fighting.
fn flat_mark(out: &mut Parts, p: Vec2, width: f32, depth: f32, angle: f32, y: f32, col: u32) {
    out.add(
        Mat::VCol,
        &geom::plane(width, depth),
        place(p.x, y, p.y, angle) * Mat4::from_rotation_x(-FRAC_PI_2),
        hex(col),
    );
}

/// A chunk owns a road by its midpoint, but some source segments extend more than
/// 700 m. Filter by the entire segment, not the chunk rectangle, so distant ends keep
/// their entrance clearance too. The margin covers the widest walk and arcade route.
fn relevant_doors(doors: &[Door], a: Vec2, b: Vec2, rw: f32) -> Vec<Door> {
    let margin = Vec2::splat(rw * 0.5 + 13.0);
    let lo = a.min(b) - margin;
    let hi = a.max(b) + margin;
    doors
        .iter()
        .copied()
        .filter(|door| door.p.x >= lo.x && door.p.y >= lo.y && door.p.x <= hi.x && door.p.y <= hi.y)
        .collect()
}

/// Keep the full route from the original door plane through its arcade to the street
/// free of decorative clutter. Door and obstacle records themselves never change.
fn entrance_clear(p: Vec2, doors: &[Door]) -> bool {
    doors.iter().all(|door| {
        let d = p - door.p;
        let forward = d.dot(door.n);
        let across = d.perp_dot(door.n).abs();
        !((-0.8..=ARC + 3.5).contains(&forward) && across < 1.25)
    })
}

/// Shallow rubbish, gum, broken masonry and paper at the gutter — alley.ts / decay.ts
/// dressing without invisible collisions or alterations to saved loot identifiers.
fn edge_litter(out: &mut Parts, p: Vec2, y: f32, angle: f32, seed: u64) {
    let mut rng = super::canvas::Rng(seed);
    let t = place(p.x, y, p.y, angle);
    for i in 0..4 {
        let x = (rng.f() - 0.5) * 0.85;
        let z = (rng.f() - 0.5) * 0.25;
        let col = [0xa99a78, 0xc2bca7, 0x706a58, 0x8b6c50][i];
        let w = 0.06 + rng.f() * 0.16;
        let d = 0.04 + rng.f() * 0.12;
        let h = 0.008 + rng.f() * 0.035;
        out.add(
            Mat::VCol,
            &geom::cuboid(w, h, d),
            t * place(x, h * 0.5, z, rng.f() * 6.0),
            hex(col),
        );
    }
    // Folded paper: two low facets, with one corner turned up.
    let mut paper = geom::plane(0.2, 0.13)
        .transformed(geom::at(0.18, 0.008, 0.12) * Mat4::from_rotation_x(-FRAC_PI_2));
    if let Some(v) = paper.pos.first_mut() {
        v[1] += 0.025;
    }
    out.add(Mat::VCol, &paper, t, hex(0xc8bea4));
    // A few green shoots emerge from a mortar seam rather than a large grass patch.
    for i in 0..3 {
        out.add(
            Mat::VCol,
            &geom::plane(0.015, 0.06 + i as f32 * 0.015),
            t * place(-0.22 + i as f32 * 0.03, 0.035, -0.08, i as f32 * 0.9),
            hex(0x4b5334),
        );
    }
}

fn drain_grate(out: &mut Parts, p: Vec2, y: f32, angle: f32) {
    flat_mark(out, p, 0.88, 0.34, angle, y, 0x242724);
    let t = place(p.x, y + 0.003, p.y, angle);
    for i in 0..9 {
        out.add(
            Mat::Metal,
            &geom::plane(0.038, 0.3),
            t * geom::at(-0.39 + i as f32 * 0.0975, 0.0, 0.0) * Mat4::from_rotation_x(-FRAC_PI_2),
            hex(0x57564b),
        );
    }
    for z in [-0.16, 0.16] {
        out.add(
            Mat::Metal,
            &geom::plane(0.9, 0.024),
            t * geom::at(0.0, 0.0, z) * Mat4::from_rotation_x(-FRAC_PI_2),
            hex(0x57564b),
        );
    }
}

/// More visible street wear is tied to road edges and utility access instead of
/// identical coloured patches repeated every 8 m. All geometry is merged per material.
fn road_wear(
    w: &WorldData,
    doors: &[Door],
    junctions: &HashMap<(i32, i32), u32>,
    a: Vec2,
    b: Vec2,
    rw: f32,
    out: &mut Parts,
) {
    let len = a.distance(b);
    if len < 5.0 {
        return;
    }
    let u = (b - a) / len;
    let n = Vec2::new(-u.y, u.x);
    let angle = -u.y.atan2(u.x);
    let y = 0.023 + rw * 0.0005;
    let margin = |p: Vec2| {
        let k = ((p.x * 10.0).round() as i32, (p.y * 10.0).round() as i32);
        if junctions.get(&k).copied().unwrap_or(0) >= 2 {
            rw * 0.5 + 5.0
        } else {
            2.0
        }
    };
    let (start, end) = (margin(a), len - margin(b));
    if end <= start {
        return;
    }
    for side in [-1.0, 1.0] {
        let mut d = start;
        while d < end {
            let h = hash3(a.x + d, b.y, side * 31.0);
            let p = a + u * d + n * side * (rw * 0.5 - 0.21);
            if w.bld_at(p).is_none() {
                let shape = [
                    p - u * 1.3 - n * 0.07,
                    p + u * 0.7 - n * (0.09 + h * 0.1),
                    p + u * 1.5 + n * 0.08,
                    p - u * 0.8 + n * 0.13,
                ];
                out.add(
                    Mat::VCol,
                    &flat_poly(&shape, y, true, 1.0),
                    Mat4::IDENTITY,
                    hex(0x383a32),
                );
                if rw >= 6.0 && h > 0.4 {
                    drain_grate(out, p, y + 0.003, angle);
                } else if rw < 9.0 && h > 0.65 && entrance_clear(p, doors) {
                    edge_litter(out, p, y + 0.002, angle, (h * 1e6) as u64);
                }
            }
            d += 9.0 + h * 4.0;
        }
    }
    if end - start > 12.0 && rw >= 5.0 {
        let h = hash3(a.x, b.y, 97.0);
        let p = a + u * (start + (end - start) * (0.36 + h * 0.25)) + n * rw * 0.22;
        if w.bld_at(p).is_none() {
            // Cast-iron access cover set in a worn concrete collar.
            let t = geom::at(p.x, y + 0.006, p.y);
            out.add(
                Mat::VCol,
                &geom::cylinder(0.5, 0.5, 0.014, 20),
                t,
                hex(0x727064),
            );
            out.add(
                Mat::Metal,
                &geom::cylinder(0.41, 0.41, 0.018, 20),
                t,
                hex(0x3a3e39),
            );
            for k in -2i32..=2 {
                let q = p + n * (k as f32 * 0.13);
                let width = if k.abs() == 2 { 0.48 } else { 0.66 };
                flat_mark(out, q, width, 0.018, angle, y + 0.017, 0x77766b);
            }
            let q = p + u * 0.34;
            flat_mark(out, q, 0.12, 0.055, angle, y + 0.019, 0x161b18);
        }
    }
}

/// Paved ground for a chunk rect (lotTex, 1 texture = 8 m).
pub fn ground(min: Vec2, max: Vec2, out: &mut Parts) {
    let c = (min + max) / 2.0;
    let s = max - min;
    let g = geom::plane(s.x, s.y)
        .transformed(geom::at(c.x, 0.0, c.y) * Mat4::from_rotation_x(-FRAC_PI_2))
        .map_uv(|_, p| [p[0] / 8.0, p[2] / 8.0]);
    out.get(Mat::Ground).append(g);
}

fn bx(w: f32, h: f32, d: f32, x: f32, y: f32, z: f32) -> MeshBuf {
    geom::cuboid(w, h, d).transformed(geom::at(x, y, z))
}

/// A planar vehicle panel with an explicit outward winding. Four vertices keep
/// small plates, glazing and trim cheap while retaining real surface geometry.
fn panel(points: [[f32; 3]; 4]) -> MeshBuf {
    let normal = (Vec3::from(points[1]) - Vec3::from(points[0]))
        .cross(Vec3::from(points[2]) - Vec3::from(points[0]))
        .normalize();
    MeshBuf {
        pos: points.to_vec(),
        nrm: vec![normal.into(); 4],
        uv: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        col: vec![WHITE; 4],
        idx: vec![0, 1, 2, 0, 2, 3],
    }
}

/// Small six-sided wheel hub, facing +X in the vehicle's local coordinates.
fn hub(radius: f32) -> MeshBuf {
    let mut mesh = MeshBuf::default();
    mesh.pos.push([0.0, 0.0, 0.0]);
    for i in 0..6 {
        let a = i as f32 * std::f32::consts::TAU / 6.0;
        mesh.pos.push([0.0, radius * a.cos(), radius * a.sin()]);
    }
    mesh.nrm = vec![[1.0, 0.0, 0.0]; 7];
    mesh.uv = vec![[0.5, 0.5]; 7];
    mesh.col = vec![WHITE; 7];
    for i in 0..6u32 {
        mesh.idx.extend_from_slice(&[0, i + 1, (i + 1) % 6 + 1]);
    }
    mesh
}

/// Street lamp (buildLamps): pole, arm, head, anchored foot and access hatch.
fn lamp(out: &mut Parts, t: Mat4) {
    out.add(
        Mat::Metal,
        &bx(0.3, 0.12, 0.3, 0.0, 0.06, 0.0),
        t,
        hex(0x555b58),
    );
    out.add(
        Mat::VCol,
        &geom::plane(0.085, 0.32),
        t * geom::at(0.0, 0.48, 0.091),
        hex(0x353b39),
    );
    out.add(
        Mat::Metal,
        &geom::cylinder(0.07, 0.09, 5.0, 6),
        t * geom::at(0.0, 2.5, 0.0),
        hex(0x5a5e60),
    );
    out.add(
        Mat::Metal,
        &bx(0.08, 0.08, 1.2, 0.0, 4.95, 0.55),
        t,
        hex(0x5a5e60),
    );
    out.add(
        Mat::Lamp,
        &bx(0.32, 0.1, 0.5, 0.0, 4.88, 1.1),
        t,
        hex(0xfff0d0),
    );
}

const SCOOTER_C: [u32; 8] = [
    0xd8d8d0, 0x2a2a2c, 0xb82a24, 0x3a6aa8, 0xe8c840, 0x7a8a6a, 0xf0f0f0, 0x6a3a8a,
];

/// Parked 機車: body shell, floorboard, seat, front apron, handlebar, wheels, mirror stalks.
fn scooter(out: &mut Parts, t: Mat4, seed: u32) {
    let c = hex(SCOOTER_C[seed as usize % SCOOTER_C.len()]);
    let dark = hex(0x1c1c1e);
    for z in [-0.62f32, 0.6] {
        out.add(
            Mat::VCol,
            &geom::cylinder(0.24, 0.24, 0.1, 12),
            t * geom::at(0.0, 0.24, z) * Mat4::from_rotation_z(FRAC_PI_2),
            dark,
        );
    }
    out.add(Mat::Metal, &bx(0.36, 0.34, 0.75, 0.0, 0.5, -0.38), t, c); // rear body
    out.add(
        Mat::VCol,
        &bx(0.32, 0.1, 0.62, 0.0, 0.72, -0.34),
        t,
        hex(0x262628),
    ); // seat
    out.add(
        Mat::Metal,
        &bx(0.3, 0.08, 0.45, 0.0, 0.28, 0.12),
        t,
        hex(0x3a3a3c),
    ); // floorboard
    out.add(
        Mat::Metal,
        &geom::cuboid(0.34, 0.75, 0.14),
        t * geom::at(0.0, 0.62, 0.5) * Mat4::from_rotation_x(-0.25),
        c,
    ); // front apron
    out.add(
        Mat::Metal,
        &bx(0.06, 0.5, 0.06, 0.0, 0.95, 0.56),
        t,
        hex(0x5a5a5c),
    );
    out.add(
        Mat::Metal,
        &bx(0.62, 0.04, 0.05, 0.0, 1.17, 0.56),
        t,
        hex(0x2a2a2c),
    ); // handlebar
    out.add(
        Mat::VCol,
        &bx(0.16, 0.08, 0.04, 0.0, 1.0, 0.63),
        t,
        hex(0xfff6e0),
    );
    for x in [-0.22f32, 0.22] {
        out.add(
            Mat::Metal,
            &bx(0.02, 0.22, 0.02, x, 1.28, 0.54),
            t,
            hex(0x2a2a2c),
        );
        out.add(
            Mat::Metal,
            &bx(0.12, 0.075, 0.045, x, 1.39, 0.54),
            t,
            hex(0x303638),
        );
        out.add(
            Mat::Glass,
            &geom::plane(0.09, 0.05),
            t * place(x, 1.39, 0.514, std::f32::consts::PI),
            hex(0xb5c9d0),
        );
    }
    // A shallow rear lens and plate distinguish the seat/body end from the front
    // apron; they are non-emissive, as all these original vehicles remain parked.
    for (width, height, y, color) in [(0.23, 0.09, 0.61, 0xb43c31), (0.19, 0.12, 0.43, 0xd6d5bf)] {
        out.add(
            Mat::VCol,
            &geom::plane(width, height),
            t * place(0.0, y, -0.758, std::f32::consts::PI),
            hex(color),
        );
    }
    for z in [-0.62, 0.6] {
        for side in [-1.0, 1.0] {
            out.add(
                Mat::Metal,
                &hub(0.105),
                t * place(
                    side * 0.052,
                    0.24,
                    z,
                    if side < 0.0 {
                        std::f32::consts::PI
                    } else {
                        0.0
                    },
                ),
                hex(0x969b96),
            );
        }
    }
}

/// Parked vehicles retain the original cover footprint. The old solid cabin hid
/// its inset glass; exposed sloping windows now meet a narrow roof and side pillars.
fn car(out: &mut Parts, t: Mat4, kind: PropKind, seed: u32) {
    let dark = hex(0x1a1a1c);
    let (c, l, wd) = match kind {
        PropKind::Taxi => (0xf0c419, 4.3, 1.75),
        PropKind::Truck => (0x56603a, 6.4, 2.25),
        _ => (
            [
                0xd8d8d8u32,
                0x2a2c30,
                0x8a2a24,
                0x3a4a6a,
                0xb0b4b8,
                0x5a6a5a,
            ][seed as usize % 6],
            4.3,
            1.75,
        ),
    };
    let body = hex(c);
    for z in [-l * 0.32, l * 0.32] {
        for side in [-1.0, 1.0] {
            let x = side * (wd * 0.5 - 0.1);
            out.add(
                Mat::VCol,
                &geom::cylinder(0.33, 0.33, 0.24, 12),
                t * geom::at(x, 0.33, z) * Mat4::from_rotation_z(FRAC_PI_2),
                dark,
            );
            out.add(
                Mat::Metal,
                &hub(0.18),
                t * place(
                    x + side * 0.121,
                    0.33,
                    z,
                    if side < 0.0 {
                        std::f32::consts::PI
                    } else {
                        0.0
                    },
                ),
                hex(0x8d9699),
            );
        }
    }
    if kind == PropKind::Truck {
        // Keep the original 2.3 × 6.5 m collision envelope and 2.8 m cover height.
        out.add(
            Mat::Metal,
            &bx(wd, 1.3, 1.8, 0.0, 1.25, l * 0.5 - 0.9),
            t,
            body,
        );
        out.add(
            Mat::Glass,
            &geom::plane(wd - 0.25, 0.52),
            t * geom::at(0.0, 1.56, 3.204),
            hex(0x9baeb4),
        );
        for side in [-1.0, 1.0] {
            let x = side * (wd * 0.5 + 0.003);
            out.add(
                Mat::Glass,
                &geom::plane(1.16, 0.52),
                t * place(x, 1.56, 2.35, side * FRAC_PI_2),
                hex(0x8eaaad),
            );
            out.add(
                Mat::VCol,
                &geom::plane(0.2, 0.045),
                t * place(x + side * 0.002, 1.12, 2.0, side * FRAC_PI_2),
                hex(0x333b37),
            );
        }
        out.add(
            Mat::Tin,
            &bx(wd + 0.05, 1.9, l - 2.0, 0.0, 1.75, -0.9),
            t,
            scale_col(body, 0.85),
        );
        // Rear doors are painted on the existing cargo shell; narrow lock bars and
        // lower reflectors give the rear an actual readable construction.
        out.add(
            Mat::VCol,
            &geom::plane(0.025, 1.66),
            t * place(0.0, 1.75, -3.103, std::f32::consts::PI),
            hex(0x293128),
        );
        for x in [-0.34, 0.34] {
            out.add(
                Mat::Metal,
                &bx(0.035, 1.65, 0.035, x, 1.75, -3.121),
                t,
                hex(0x9a9e8b),
            );
        }
        for x in [-0.91, 0.91] {
            out.add(
                Mat::VCol,
                &geom::plane(0.22, 0.11),
                t * place(x, 0.94, -3.125, std::f32::consts::PI),
                hex(0xc04734),
            );
        }
        out.add(
            Mat::VCol,
            &bx(wd, 0.17, 0.12, 0.0, 0.54, 3.16),
            t,
            hex(0x42483f),
        );
        out.add(
            Mat::VCol,
            &geom::plane(0.8, 0.22),
            t * geom::at(0.0, 0.91, 3.207),
            hex(0x303b35),
        );
        for x in [-0.76, 0.76] {
            out.add(
                Mat::VCol,
                &geom::plane(0.4, 0.16),
                t * geom::at(x, 0.87, 3.209),
                hex(0xe4e5c9),
            );
        }
        out.add(
            Mat::VCol,
            &geom::plane(0.38, 0.13),
            t * geom::at(0.0, 0.56, 3.224),
            hex(0xc8ccba),
        );
        return;
    }
    out.add(Mat::Metal, &bx(wd, 0.62, l, 0.0, 0.62, 0.0), t, body);
    out.add(Mat::Metal, &bx(1.43, 0.08, 1.53, 0.0, 1.4, -0.2), t, body);
    let glass = hex(0x8eaab2);
    // Sloping windshield and rear screen: lower corners meet the body, upper
    // corners meet the roof, with no opaque cabin inside either glass surface.
    out.add(
        Mat::Glass,
        &panel([
            [-0.80, 0.94, 1.0],
            [0.80, 0.94, 1.0],
            [0.70, 1.36, 0.56],
            [-0.70, 1.36, 0.56],
        ]),
        t,
        glass,
    );
    out.add(
        Mat::Glass,
        &panel([
            [0.80, 0.94, -1.32],
            [-0.80, 0.94, -1.32],
            [-0.70, 1.36, -0.96],
            [0.70, 1.36, -0.96],
        ]),
        t,
        glass,
    );
    for side in [-1.0, 1.0] {
        let mut points = [
            [side * 0.81, 0.94, -1.32],
            [side * 0.81, 0.94, 1.0],
            [side * 0.71, 1.36, 0.56],
            [side * 0.71, 1.36, -0.96],
        ];
        if side > 0.0 {
            points.reverse();
        }
        out.add(Mat::Glass, &panel(points), t, glass);
        let mut pillar = [
            [side * 0.817, 0.94, -0.22],
            [side * 0.817, 0.94, -0.12],
            [side * 0.717, 1.36, -0.12],
            [side * 0.717, 1.36, -0.22],
        ];
        if side > 0.0 {
            pillar.reverse();
        }
        out.add(Mat::VCol, &panel(pillar), t, hex(0x343c3e));
        // Outer door seam and flush handles stay inside the existing cover width.
        let x = side * (wd * 0.5 + 0.003);
        out.add(
            Mat::VCol,
            &geom::plane(0.014, 0.48),
            t * place(x, 0.68, -0.17, side * FRAC_PI_2),
            hex(0x424a48),
        );
        for z in [-0.85, 0.47] {
            out.add(
                Mat::Metal,
                &geom::plane(0.18, 0.04),
                t * place(x + side * 0.002, 0.83, z, side * FRAC_PI_2),
                hex(0xb8bdb6),
            );
        }
    }
    for end in [-1.0, 1.0] {
        let angle = if end < 0.0 { std::f32::consts::PI } else { 0.0 };
        out.add(
            Mat::VCol,
            &bx(wd - 0.05, 0.12, 0.08, 0.0, 0.47, end * 2.13),
            t,
            hex(0x3b4140),
        );
        out.add(
            Mat::VCol,
            &geom::plane(0.34, 0.115),
            t * place(0.0, 0.58, end * 2.177, angle),
            hex(0xd3d6c5),
        );
        for x in [-0.56, 0.56] {
            out.add(
                Mat::VCol,
                &geom::plane(0.4, 0.14),
                t * place(x, 0.77, end * 2.153, angle),
                hex(if end > 0.0 { 0xefebd0 } else { 0xb73b32 }),
            );
        }
    }
    out.add(
        Mat::VCol,
        &geom::plane(0.56, 0.16),
        t * geom::at(0.0, 0.77, 2.155),
        hex(0x2a3334),
    );
    if kind == PropKind::Taxi {
        out.add(
            Mat::VCol,
            &bx(0.5, 0.13, 0.22, 0.0, 1.505, -0.15),
            t,
            hex(0xffedb0),
        );
        // A dark inset on both sides reads as a taxi roof board without adding a
        // new texture/material or claiming a particular real operator.
        for end in [-1.0, 1.0] {
            out.add(
                Mat::VCol,
                &geom::plane(0.34, 0.055),
                t * place(
                    0.0,
                    1.505,
                    -0.15 + end * 0.113,
                    if end < 0.0 { std::f32::consts::PI } else { 0.0 },
                ),
                hex(0x556044),
            );
        }
    }
}

/// trees.ts `leaves`: a lumpy low-poly clump, shaded darker at the bottom.
fn leaves(out: &mut MeshBuf, r: f32, p: Vec3, sy: f32, col: u32, lump: f32) {
    let mut g = geom::sphere(r, 7, 5);
    for v in g.pos.iter_mut() {
        let k = hash3(
            (v[0] * 40.0 + p.x * 3.0).round(),
            (v[1] * 40.0 + p.y * 3.0).round(),
            (v[2] * 40.0 + p.z * 3.0).round(),
        );
        let m = 1.0 + (k - 0.5) * 2.0 * lump;
        *v = [v[0] * m, v[1] * m * sy, v[2] * m];
    }
    let cc = hex(col);
    for (i, c) in g.col.iter_mut().enumerate() {
        let cy = g.pos[i][1] / (r * sy);
        let f = (0.66 + 0.42 * ((cy + 1.0) / 2.0).clamp(0.0, 1.0))
            * (0.93 + 0.14 * hash3(i as f32, p.x * 7.0 + 1.0, p.y * 7.0 + 2.0));
        *c = [cc[0] * f, cc[1] * f, cc[2] * f, 1.0];
    }
    g.smooth_normals();
    out.add(&g, geom::at(p.x, p.y, p.z), WHITE);
}

/// trees.ts `crown`: a dark heart and many small clumps over it, lighter towards the top.
fn crown(out: &mut MeshBuf, c: Vec3, rr: f32, sy: f32, n: usize, col: u32) {
    let dk = scale_col(hex(col), 0.62);
    let mut heart = MeshBuf::default();
    leaves(
        &mut heart,
        rr * 0.74,
        c - Vec3::Y * rr * sy * 0.08,
        sy,
        0xffffff,
        0.18,
    );
    out.add(&heart, Mat4::IDENTITY, dk);
    for k in 0..n {
        let t = (k as f32 + 0.5) / n as f32;
        let h = 1.0 - 1.55 * t;
        let ring = (1.0 - h * h).max(0.0).sqrt();
        let a = k as f32 * 2.39996 + c.x;
        let r = rr * (0.34 + 0.15 * hash3(k as f32, c.x + 1.0, c.y + 2.0));
        let shade = 0.9 + 0.22 * hash3(k as f32, c.z + 3.0, 5.0) + 0.06 * h;
        let mut clump = MeshBuf::default();
        let p = Vec3::new(
            c.x + a.cos() * ring * rr * 0.74,
            c.y + h * rr * sy * 0.74,
            c.z + a.sin() * ring * rr * 0.74,
        );
        leaves(&mut clump, r, p, 0.8, 0xffffff, 0.3);
        out.add(&clump, Mat4::IDENTITY, scale_col(hex(col), shade));
    }
}

fn tree(out: &mut Parts, t: Mat4, kind: u8) {
    let limb = |r0: f32, r1: f32, h: f32| {
        geom::cylinder_ex(r1, r0, h, 5, true, 0.0, std::f32::consts::TAU)
    };
    let mut fol = MeshBuf::default();
    match kind {
        0 => {
            // 台灣欒樹 luan
            out.add(
                Mat::VCol,
                &limb(0.21, 0.14, 3.0),
                t * geom::at(0.0, 1.5, 0.0),
                hex(0x5a4a3c),
            );
            out.add(
                Mat::VCol,
                &limb(0.11, 0.06, 1.8),
                t * geom::at(0.45, 3.4, 0.0) * Mat4::from_rotation_z(0.5),
                hex(0x5a4a3c),
            );
            out.add(
                Mat::VCol,
                &limb(0.1, 0.06, 1.7),
                t * geom::at(-0.45, 3.5, 0.1) * Mat4::from_rotation_z(-0.55),
                hex(0x5a4a3c),
            );
            crown(&mut fol, Vec3::new(0.0, 4.75, 0.0), 2.3, 0.8, 12, 0x4f8034);
        }
        1 => {
            // 樟樹 camphor
            out.add(
                Mat::VCol,
                &limb(0.32, 0.22, 2.6),
                t * geom::at(0.0, 1.3, 0.0),
                hex(0x4e4236),
            );
            out.add(
                Mat::VCol,
                &limb(0.18, 0.1, 2.2),
                t * geom::at(0.45, 3.2, 0.0) * Mat4::from_rotation_z(0.42),
                hex(0x4e4236),
            );
            out.add(
                Mat::VCol,
                &limb(0.18, 0.1, 2.2),
                t * geom::at(-0.45, 3.2, 0.0) * Mat4::from_rotation_z(-0.42),
                hex(0x4e4236),
            );
            crown(&mut fol, Vec3::new(0.0, 5.0, 0.0), 3.0, 0.74, 16, 0x668c38);
        }
        _ => {
            // 茄苳 bishopwood
            out.add(
                Mat::VCol,
                &limb(0.34, 0.24, 2.7),
                t * geom::at(0.0, 1.35, 0.0),
                hex(0x7a4a34),
            );
            crown(&mut fol, Vec3::new(0.0, 4.5, 0.0), 2.5, 0.84, 12, 0x2f5f2c);
        }
    }
    out.add(Mat::Foliage, &fol, t, WHITE);
}

/// Full original prop at its real world transform. Native QA regenerates these
/// exact batched parts instead of spawning an independent display-only model.
pub fn prop_geometry(prop: &Prop) -> Parts {
    let mut out = Parts::default();
    let t = place(prop.p.x, 0.0, prop.p.y, prop.ang);
    match prop.kind {
        PropKind::Tree(kind) => tree(&mut out, t, kind),
        PropKind::Lamp => lamp(&mut out, t),
        PropKind::Scooter => scooter(&mut out, t, prop.seed),
        kind => car(&mut out, t, kind, prop.seed),
    }
    out
}

pub fn props(list: &[Prop], min: Vec2, max: Vec2, lod_far: bool, out: &mut Parts) {
    for p in list {
        if p.p.x < min.x || p.p.y < min.y || p.p.x >= max.x || p.p.y >= max.y {
            continue;
        }
        let t = place(p.p.x, 0.0, p.p.y, p.ang);
        match p.kind {
            PropKind::Tree(k) => tree(out, t, k),
            _ if lod_far => {}
            PropKind::Lamp => lamp(out, t),
            PropKind::Scooter => scooter(out, t, p.seed),
            k => car(out, t, k, p.seed),
        }
    }
}

/// Exactly the original per-segment width/paint stream, also consulted by parking
/// plans so a painted parking bay is never added beside a red or yellow curb.
fn curb_style(a: Vec2, b: Vec2, rw: f32, side: f32) -> (f32, f32) {
    let mut seed = ((a.x * 73.1 + a.y * 19.7 + b.x * 7.3 + b.y * 3.1 + side * 11.0).floor() as i64)
        .unsigned_abs()
        % 2147483646;
    if seed == 0 {
        seed = 1;
    }
    let mut next = || {
        seed = (seed * 16807) % 2147483647;
        seed as f32 / 2147483647.0
    };
    let width = if rw >= 12.0 {
        2.6 + next() * 1.2
    } else {
        1.6 + next() * 0.8
    };
    (width, next())
}

/// roads.ts `curb`: an 18 cm granite curb (some painted red §169 / yellow §168 no-parking) and a
/// 15 cm paved walk behind it, 2.6–3.8 m on main roads and 1.6–2.4 m on streets, stopped at the
/// building line, in 4 m slabs; pieces that would land on a crossing road are left out.
fn sidewalks(w: &WorldData, doors: &[Door], a: Vec2, b: Vec2, rw: f32, out: &mut Parts) {
    let l = a.distance(b);
    if l < 3.0 {
        return;
    }
    let u = (b - a) / l;
    let n = Vec2::new(-u.y, u.x);
    let ang = u.y.atan2(u.x);
    for sgn in [-1.0f32, 1.0] {
        let (sw, paint) = curb_style(a, b, rw, sgn);
        let o0 = rw / 2.0 + 0.18;
        let at = |t: f32, o: f32| a + u * t + n * (sgn * o);
        let off_road = |p: Vec2| w.road_dist(p) > -0.05;
        let k = (l / 4.0).ceil().max(1.0) as usize;
        let pl = l / k as f32;
        for i in 0..k {
            let tm = (i as f32 + 0.5) * pl;
            let cp = at(tm, rw / 2.0 + 0.09);
            if w.bld_at(cp).is_some() || !off_road(cp) {
                continue;
            }
            let g = geom::cuboid(pl, 0.18, 0.18).transformed(place(cp.x, 0.09, cp.y, -ang));
            if paint < 0.45 {
                out.add(Mat::VCol, &g, Mat4::IDENTITY, hex(0xb8302a));
            } else if paint < 0.72 {
                out.add(Mat::VCol, &g, Mat4::IDENTITY, hex(0xd8a828));
            } else {
                out.get(Mat::Curb)
                    .append(g.map_uv(|uv, _| [uv[0] * pl, uv[1]]));
            }
            let mut ws = 0.0;
            for f in [0.25, 0.5, 0.75, 1.0] {
                let q = at(tm, o0 + sw * f - 0.1);
                if w.bld_at(q).is_some() || !off_road(q) {
                    break;
                }
                ws = sw * f;
            }
            if ws < 0.7 {
                continue;
            }
            let c = at(tm, o0 + ws / 2.0);
            let g = geom::cuboid(pl + 0.02, 0.15, ws)
                .transformed(place(c.x, 0.075, c.y, -ang))
                .map_uv(|_, p| {
                    let v = Vec2::new(p[0], p[2]);
                    [v.dot(u) / 2.0, v.dot(n) / 2.0]
                });
            out.get(Mat::Walk).append(g);
            // The 30 cm facility course belongs beside the curb, not repeated across
            // the whole walk as a stripe baked into a repeating texture.
            let strip = at(tm, o0 + 0.16);
            let g = geom::plane(pl, 0.28)
                .transformed(
                    place(strip.x, 0.153, strip.y, -ang) * Mat4::from_rotation_x(-FRAC_PI_2),
                )
                .map_uv(|_, p| [Vec2::new(p[0], p[2]).dot(u) / 0.6, 0.5]);
            out.get(Mat::Curb).append(g);
            let mut seam = tm - pl * 0.5;
            while seam < tm + pl * 0.5 {
                let q = at(seam, o0 + 0.16);
                flat_mark(out, q, 0.012, 0.28, -ang, 0.156, 0x514f47);
                seam += 0.3;
            }
            // Paint is scuffed off the curb top; all little chips join the existing
            // vertex-coloured street mesh, so they add no per-chip draw calls.
            for k in 0..3 {
                let h = hash3(cp.x, cp.y, k as f32 + sgn * 7.0);
                if h < 0.38 {
                    continue;
                }
                let q = cp + u * (h - 0.5) * pl;
                flat_mark(out, q, 0.08 + h * 0.22, 0.08, -ang, 0.182, 0x858275);
            }
            // Low litter / grit stays at the curb. The central walk and every
            // entrance corridor remain clear, including a recessed arcade door.
            let h = hash3(cp.x, cp.y, 53.0);
            if h > 0.57 && ws >= 1.2 {
                let q = at(tm + pl * 0.16, o0 + 0.48);
                if entrance_clear(q, doors) && w.free(q, 0.28) {
                    edge_litter(out, q, 0.156, -ang, (h * 1e6) as u64);
                }
            }
        }
    }
}

#[cfg(test)]
mod dressing_tests {
    use super::*;

    #[test]
    fn parked_vehicle_details_keep_existing_car_and_truck_cover_bounds() {
        for (kind, half_width, half_length, height, budget) in [
            (PropKind::Car, 0.9, 2.2, 1.45, 650),
            (PropKind::Taxi, 0.9, 2.2, 1.575, 700),
            (PropKind::Truck, 1.15, 3.25, 2.8, 700),
            (PropKind::Scooter, 0.35, 0.87, 1.43, 600),
        ] {
            let prop = Prop {
                kind,
                p: Vec2::ZERO,
                ang: 0.0,
                seed: 19,
            };
            let parts = prop_geometry(&prop);
            assert!(
                parts.vcount() <= budget,
                "{kind:?} exceeded its phone mesh budget"
            );
            for mesh in parts.0.values() {
                for point in &mesh.pos {
                    assert!(point.iter().all(|v| v.is_finite()));
                    assert!(
                        point[0].abs() <= half_width + 0.001,
                        "{kind:?} width {point:?}"
                    );
                    assert!(
                        point[2].abs() <= half_length + 0.001,
                        "{kind:?} length {point:?}"
                    );
                    assert!(
                        point[1] >= -0.001 && point[1] <= height + 0.001,
                        "{kind:?} height {point:?}"
                    );
                }
            }
            let glass = &parts.0[&Mat::Glass];
            assert!(
                !glass.is_empty(),
                "exposed vehicle glazing is part of the actual prop batch"
            );
            for (point, normal) in glass.pos.iter().zip(&glass.nrm) {
                if kind != PropKind::Scooter && normal[0].abs() > 0.3 {
                    assert!(point[0] * normal[0] > 0.0, "side glass must face outwards");
                }
            }
        }
    }

    #[test]
    fn street_props_remain_material_batched_and_absent_at_far_lod() {
        let kinds = [
            PropKind::Lamp,
            PropKind::Scooter,
            PropKind::Car,
            PropKind::Taxi,
            PropKind::Truck,
        ];
        let list: Vec<_> = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| Prop {
                kind,
                p: Vec2::new(index as f32 * 10.0, 0.0),
                ang: 0.45,
                seed: 27,
            })
            .collect();
        let mut near = Parts::default();
        props(
            &list,
            Vec2::splat(-20.0),
            Vec2::splat(100.0),
            false,
            &mut near,
        );
        assert!(
            near.0.len() <= 5,
            "the same materials merge across all vehicles"
        );
        let mut far = Parts::default();
        props(
            &list,
            Vec2::splat(-20.0),
            Vec2::splat(100.0),
            true,
            &mut far,
        );
        assert_eq!(far.vcount(), 0);
        let lamp = prop_geometry(&list[0]);
        assert!(lamp.vcount() < 160);
    }

    #[test]
    fn decorative_litter_keeps_the_whole_arcade_entrance_clear() {
        for normal in [Vec2::X, Vec2::Y, Vec2::new(0.6, 0.8)] {
            let door = Door {
                p: Vec2::new(14.0, -7.0),
                n: normal,
            };
            let tangent = Vec2::new(-normal.y, normal.x);
            for ahead in [0.0, 1.0, ARC, ARC + 3.0] {
                assert!(!entrance_clear(door.p + normal * ahead, &[door]));
                assert!(!entrance_clear(
                    door.p + normal * ahead + tangent * 1.0,
                    &[door]
                ));
            }
            assert!(entrance_clear(door.p + tangent * 1.5, &[door]));
        }
    }

    #[test]
    fn door_prefilter_keeps_entrances_at_both_ends_of_long_segments() {
        let doors = [
            Door {
                p: Vec2::new(-12.0, 18.0),
                n: -Vec2::Y,
            },
            Door {
                p: Vec2::new(753.0, -18.0),
                n: Vec2::Y,
            },
            Door {
                p: Vec2::new(400.0, 60.0),
                n: -Vec2::Y,
            },
        ];
        let near = relevant_doors(&doors, Vec2::ZERO, Vec2::new(746.7, 0.0), 12.0);
        assert_eq!(near.len(), 2);
        assert_eq!(near[0].p, doors[0].p);
        assert_eq!(near[1].p, doors[1].p);
    }

    #[test]
    fn gutter_dressing_has_a_small_bounded_mesh_and_no_tall_obstacles() {
        let mut parts = Parts::default();
        edge_litter(&mut parts, Vec2::ZERO, 0.0, 0.0, 124);
        let vertices: usize = parts.0.values().map(MeshBuf::vcount).sum();
        assert!(vertices < 160, "a gutter patch must remain cheap on phones");
        for mesh in parts.0.values() {
            assert!(mesh.pos.iter().all(|p| p.iter().all(|v| v.is_finite())));
            assert!(mesh.pos.iter().all(|p| p[1] < 0.1 && p[1] > -0.02));
        }
    }
}
