//! Building geometry from the planned [`Bld`]s — the mesh half of `buildMap` (main.ts) and
//! `buildArcade` (arcade.ts): textured wall slabs with floor-banded UVs, the roof slab, the
//! interior floor, the 騎樓 (columns, soffit + beam, bay lamps, per-owner tiled floor, roll-up
//! shutters), the shop sign on the fascia and the stainless rooftop water tank.

use bevy::prelude::*;
use std::collections::HashMap;

use super::data::*;
use super::textures::{SIGN_COLS, SIGN_ROWS};
use crate::geom::{self, MeshBuf, hex, place};

/// Material slots of the city (see [`super::textures::MapMaterials`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mat {
    Wall(u8),
    Roof(u8),
    Tin,
    Floor,
    ArcadeFloor(u8),
    Column,
    Shutter,
    Ceiling,
    Lamp,
    VCol,
    Metal,
    Glass,
    Sign,
    VSign,
    Brick,
    Road,
    Ground,
    Foliage,
    Marking,
    Walk,
    Curb,
}

/// Geometry accumulated per material.
#[derive(Default)]
pub struct Parts(pub HashMap<Mat, MeshBuf>);

impl Parts {
    pub fn get(&mut self, m: Mat) -> &mut MeshBuf {
        self.0.entry(m).or_default()
    }
    pub fn add(&mut self, m: Mat, g: &MeshBuf, t: Mat4, c: [f32; 4]) {
        self.get(m).add(g, t, c);
    }
    pub fn merge(&mut self, o: Parts) {
        for (k, v) in o.0 {
            self.get(k).append(v);
        }
    }
    pub fn vcount(&self) -> usize {
        self.0.values().map(|m| m.vcount()).sum()
    }
}

/// Meshes of one building, split by how the camera treats them.
#[derive(Default)]
pub struct BuildingMeshes {
    /// Fades to see-through when it hides the player (wallMatsT).
    pub walls: Parts,
    /// Roof slab, rooftop and arcade soffit: hidden while the player is inside (roof cutaway).
    pub roof: Parts,
    /// Floor, arcade floor / columns / shutters, signs: always drawn.
    pub base: Parts,
}

pub const WHITE: [f32; 4] = [1.0; 4];

/// wallBox: BoxGeometry(l + 0.25, y1 − y0, 0.25) along the edge, UVs in floor bands
/// (u: one window bay per 3.2 m, v: one storey per FH, aligned to world height).
pub fn wall_box(out: &mut MeshBuf, w: &WallBox) {
    let l = w.a.distance(w.c);
    let g = geom::cuboid(l + 0.25, w.y1 - w.y0, 0.25).map_uv(|uv, _| {
        [
            uv[0] * l / 3.2,
            // three flips canvas textures (v up); Bevy samples v down
            -((w.y0 - (GF - FH) + uv[1] * (w.y1 - w.y0)) / FH),
        ]
    });
    let d = w.c - w.a;
    let m = (w.a + w.c) / 2.0;
    out.add(
        &g,
        place(m.x, (w.y0 + w.y1) / 2.0, m.y, -d.y.atan2(d.x)),
        WHITE,
    );
}

/// Horizontal polygon at height y, facing up (or down), world-planar UVs of `uv_m` metres.
pub fn flat_poly(pts: &[Vec2], y: f32, up: bool, uv_m: f32) -> MeshBuf {
    let mut m = MeshBuf::default();
    let tris = geom::triangulate(pts);
    for p in pts {
        m.pos.push([p.x, y, p.y]);
        m.nrm.push([0.0, if up { 1.0 } else { -1.0 }, 0.0]);
        m.uv.push([p.x / uv_m, p.y / uv_m]);
        m.col.push(WHITE);
    }
    // triangulate() is CCW in (x, z); seen from +Y that is clockwise → swap for an upward face
    for t in tris.chunks(3) {
        if up {
            m.idx.extend_from_slice(&[t[0], t[2], t[1]]);
        } else {
            m.idx.extend_from_slice(t);
        }
    }
    m
}

/// Box placed in the arcade frame (arcade.ts `place`).
fn arc_box(w: f32, h: f32, d: f32, p: Vec2, y: f32, ang: f32) -> (MeshBuf, Mat4) {
    (geom::cuboid(w, h, d), place(p.x, y, p.y, -ang))
}

pub fn arcade(b: &Bld, out: &mut BuildingMeshes) {
    let Some(a) = b.arcade else { return };
    let ang = a.u.y.atan2(a.u.x);
    let at = |s: f32, d: f32| a.a + a.u * s - a.n * d;
    let nb = ((a.dl - COL) / BAY).round().max(1.0) as usize;
    let step = (a.dl - 0.7) / nb as f32;
    for k in 0..=nb {
        let p = at(0.35 + k as f32 * step, 0.3);
        if !pip(p, &b.pts) {
            continue;
        }
        let (g, t) = arc_box(COL, GF, COL, p, GF / 2.0, ang);
        let g = g.map_uv(|uv, _| [uv[0], uv[1] * 6.0]);
        out.base.add(Mat::Column, &g, t, WHITE);
    }
    // continuous ceiling (soffit of the first floor) and the beam over the columns
    let (g, t) = arc_box(a.dl, 0.22, ARC, at(a.dl / 2.0, ARC / 2.0), GF - 0.11, ang);
    out.roof.add(Mat::Ceiling, &g, t, WHITE);
    let (g, t) = arc_box(a.dl, 0.45, 0.25, at(a.dl / 2.0, 0.3), GF - 0.2, ang);
    out.roof.add(Mat::Ceiling, &g, t, WHITE);
    // each bay tiled by its own owner
    let bl = a.dl / nb as f32;
    for k in 0..nb {
        let f = at((k as f32 + 0.5) * bl, ARC / 2.0);
        let hsh = ((f.x * 12.9898 + f.y * 78.233).sin() * 43_758.547)
            .abs()
            .fract();
        let si = (hsh * super::textures::ARCADE_TILE_N as f32) as u8
            % super::textures::ARCADE_TILE_N as u8;
        let (g, t) = arc_box(bl, 0.06, ARC, f, 0.03, ang);
        let g = g.transformed(t).map_uv(|_, p| [p[0], p[2]]);
        out.base
            .add(Mat::ArcadeFloor(si), &g, Mat4::IDENTITY, WHITE);
    }
    for k in 0..nb {
        let s = 0.35 + (k as f32 + 0.5) * step;
        let (g, t) = arc_box(1.0, 0.04, 0.18, at(s, ARC / 2.0), GF - 0.24, ang);
        out.roof.add(Mat::Lamp, &g, t, WHITE);
        // roll-up shutter on each bay of the recessed shopfront, except at the shop door
        if (s - a.dl / 2.0).abs() > step / 2.0 {
            let (g, t) = arc_box(step - COL - 0.1, 2.9, 0.06, at(s, ARC - 0.15), 1.45, ang);
            let g = g.map_uv(|uv, _| [uv[0], uv[1] * 6.0]);
            out.base.add(Mat::Shutter, &g, t, WHITE);
        }
    }
}

pub fn sign(b: &Bld, out: &mut BuildingMeshes) {
    let Some(s) = b.sign else { return };
    let (cu, cv) = (
        (s.idx % SIGN_COLS as usize) as f32 / SIGN_COLS,
        (s.idx / SIGN_COLS as usize) as f32 / SIGN_ROWS,
    );
    let g = geom::plane(s.w, 1.1)
        .map_uv(|uv, _| [cu + uv[0] / SIGN_COLS, cv + (1.0 - uv[1]) / SIGN_ROWS]);
    out.base
        .add(Mat::Sign, &g, place(s.p.x, s.p.y, s.p.z, s.ry), WHITE);
    // back of the sign board (plain)
    let back = geom::cuboid(s.w, 1.1, 0.06);
    out.base.add(
        Mat::VCol,
        &back,
        place(s.p.x, s.p.y, s.p.z, s.ry) * geom::at(0.0, 0.0, -0.04),
        hex(0x3a3c3e),
    );
}

pub fn tank(b: &Bld, out: &mut BuildingMeshes) {
    let Some((tp, st)) = b.tank else { return };
    let t = geom::cylinder(0.6, 0.6, 1.4, 10);
    out.roof.add(
        Mat::Metal,
        &t,
        geom::at(tp.x, b.h + 0.7 + st, tp.y),
        hex(0xd8d8d0),
    );
    if st > 0.0 {
        for (ox, oz) in [(-0.45, -0.45), (0.45, -0.45), (0.45, 0.45), (-0.45, 0.45)] {
            let lg = geom::cuboid(0.07, st, 0.07);
            out.roof.add(
                Mat::Metal,
                &lg,
                geom::at(tp.x + ox, b.h + st / 2.0, tp.y + oz),
                hex(0x7f9fb0),
            );
        }
    }
}

/// LOD of a building / chunk: near = everything (window units, rooftop shacks via the detail
/// layer), mid = masses + arcades + signs, far = masses only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lod {
    Near,
    Mid,
    Far,
}

pub fn building(b: &Bld, lod: Lod) -> BuildingMeshes {
    let mut out = BuildingMeshes::default();
    let wm = out.walls.get(Mat::Wall(b.wall_mat as u8));
    for w in &b.walls {
        if lod == Lod::Far && w.y0 > 0.0 && w.y1 - w.y0 < FH {
            continue; // door lintels vanish at distance
        }
        wall_box(wm, w);
    }
    let roof = flat_poly(&b.pts, b.h, true, 9.0);
    out.roof.get(Mat::Roof(b.roof_mat as u8)).append(roof);
    if lod == Lod::Far {
        return out;
    }
    out.base
        .get(Mat::Floor)
        .append(flat_poly(&b.pts, 0.03, true, 2.0));
    arcade(b, &mut out);
    sign(b, &mut out);
    tank(b, &mut out);
    out
}
