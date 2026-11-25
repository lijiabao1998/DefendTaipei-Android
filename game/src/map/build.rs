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
        if (s - a.dl / 2.0).abs() - (step - COL) / 2.0 > DOOR_W / 2.0 + 0.1 {
            let (g, t) = arc_box(step - COL - 0.1, 2.9, 0.06, at(s, ARC - 0.15), 1.45, ang);
            let g = g.map_uv(|uv, _| [uv[0], uv[1] * 6.0]);
            out.base.add(Mat::Shutter, &g, t, WHITE);
            // The shutter is seated in actual guide rails, with a weathered roll box above.
            // All three sit against the existing recessed wall, never across the door gap.
            for side in [-1.0, 1.0] {
                let (g, t) = arc_box(
                    0.09,
                    2.94,
                    0.12,
                    at(s + side * (step - COL) / 2.0, ARC - 0.1),
                    1.47,
                    ang,
                );
                out.base.add(Mat::VCol, &g, t, hex(0x65675e));
            }
            let (g, t) = arc_box(step - COL + 0.12, 0.22, 0.28, at(s, ARC - 0.16), 3.02, ang);
            out.base.add(Mat::VCol, &g, t, hex(0x878a80));
        }
    }
}

/// How much solid ground-floor wall is available beside an existing door. A folded-open
/// leaf may only cover that wall; it must not project into another entrance or off a corner.
fn door_flanks(b: &Bld, d: Door) -> [f32; 2] {
    let u = d.tangent();
    let mut space = [0.0f32; 2];
    for wall in &b.walls {
        if wall.y0 > 0.01 || wall.y1 < DOOR_H {
            continue;
        }
        let a = wall.a - d.p;
        let c = wall.c - d.p;
        if a.dot(d.n).abs() > 0.05 || c.dot(d.n).abs() > 0.05 {
            continue;
        }
        let (a, c) = (a.dot(u), c.dot(u));
        let (lo, hi) = (a.min(c), a.max(c));
        if (hi + DOOR_W / 2.0).abs() < 0.05 {
            space[0] = space[0].max(-lo - DOOR_W / 2.0);
        }
        if (lo - DOOR_W / 2.0).abs() < 0.05 {
            space[1] = space[1].max(hi - DOOR_W / 2.0);
        }
    }
    space
}

/// Street-readable doors from doors.ts: the red home gate / ochre shop frame, a stone sill
/// and recognisable iron leaves. Leaves are already folded flat against the *solid* wall.
/// The native game currently has no closed-door state, so rendering a closed leaf here
/// would contradict its collision world. The 1.6 m opening remains unobstructed.
pub fn entrance(b: &Bld, lod: Lod) -> Parts {
    let mut out = Parts::default();
    let Some(d) = b.door else { return out };
    if lod == Lod::Far {
        return out;
    }
    let t = place(d.p.x, 0.0, d.p.y, d.n.x.atan2(d.n.y));
    let home = b.kind == "home";
    let paint = if home { 0xad3c30 } else { 0xc97838 };
    let gate = if home { 0x86372d } else { 0x365b49 };
    let rail = if home { 0x4c2924 } else { 0x293c32 };
    let box_at = |w, h, depth, x, y, z| (geom::cuboid(w, h, depth), t * geom::at(x, y, z));
    for x in [-DOOR_W / 2.0, DOOR_W / 2.0] {
        let (g, tr) = box_at(0.16, DOOR_H + 0.08, 0.38, x, DOOR_H / 2.0, 0.03);
        out.add(Mat::VCol, &g, tr, hex(paint));
    }
    let (g, tr) = box_at(DOOR_W + 0.32, 0.18, 0.38, 0.0, DOOR_H, 0.03);
    out.add(Mat::VCol, &g, tr, hex(paint));
    let (g, tr) = box_at(DOOR_W + 0.22, 0.06, 0.7, 0.0, 0.055, 0.08);
    out.add(Mat::VCol, &g, tr, hex(0xb5afa2));

    let flanks = door_flanks(b, d);
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        let width = (flanks[i] - 0.14).clamp(0.0, DOOR_W / 2.0 - 0.08);
        if width < 0.28 {
            continue;
        }
        let x = side * (DOOR_W / 2.0 + 0.04 + width / 2.0);
        let (g, tr) = box_at(width, 2.22, 0.055, x, 1.15, 0.175);
        out.add(Mat::VCol, &g, tr, hex(gate));
        // Two horizontal straps remain legible at Mid LOD.
        for y in [0.4, 1.95] {
            let (g, tr) = box_at(width, 0.055, 0.04, x, y, 0.22);
            out.add(Mat::VCol, &g, tr, hex(rail));
        }
        if lod == Lod::Near {
            for k in 1..=3 {
                let bar_x = x - width / 2.0 + width * k as f32 / 4.0;
                let (g, tr) = box_at(0.025, 1.94, 0.04, bar_x, 1.15, 0.22);
                out.add(Mat::VCol, &g, tr, hex(rail));
            }
            let (g, tr) = box_at(0.055, 0.18, 0.07, x + side * width * 0.28, 1.05, 0.23);
            out.add(Mat::Metal, &g, tr, hex(0xb6a381));
            // Chipped paint at the kickplate; small solid patches, not a uniform noise layer.
            for (offset, w, h) in [(-0.2, 0.16f32, 0.045), (0.13, 0.12, 0.08)] {
                let (g, tr) = box_at(
                    w.min(width * 0.3),
                    h,
                    0.008,
                    x + width * offset,
                    0.18,
                    0.208,
                );
                out.add(Mat::VCol, &g, tr, hex(0x655546));
            }
        }
    }
    if lod == Lod::Near {
        let (g, tr) = box_at(1.12, 0.018, 0.42, 0.0, 0.093, 0.51);
        out.add(
            Mat::VCol,
            &g,
            tr,
            hex(if home { 0x555a4d } else { 0x66564a }),
        );
        // The address is fictional and stable per building; use geometry for the numerals
        // so city detail does not allocate UI text entities or a per-building font texture.
        let side = if flanks[1] >= 0.62 { 1.0 } else { -1.0 };
        let available = if side > 0.0 { flanks[1] } else { flanks[0] };
        if available >= 0.62 {
            let plaque = t * geom::at(side * 1.11, 2.03, 0.24);
            out.add(
                Mat::VCol,
                &geom::cuboid(0.48, 0.25, 0.025),
                plaque,
                hex(0xd4d5c7),
            );
            out.add(
                Mat::VCol,
                &geom::plane(0.43, 0.2),
                plaque * geom::at(0.0, 0.0, 0.014),
                hex(0x345b66),
            );
            address_digits(&mut out, plaque * geom::at(0.0, 0.0, 0.018), b.id % 180 + 1);
        }
    }
    out
}

fn address_digits(out: &mut Parts, t: Mat4, number: u32) {
    let digits = number.to_string();
    const SEGMENTS: [u8; 10] = [0x3f, 0x06, 0x5b, 0x4f, 0x66, 0x6d, 0x7d, 0x07, 0x7f, 0x6f];
    // top, upper-right, lower-right, bottom, lower-left, upper-left, middle
    const BARS: [(f32, f32, f32, f32); 7] = [
        (0.0, 0.065, 0.055, 0.013),
        (0.028, 0.033, 0.013, 0.06),
        (0.028, -0.033, 0.013, 0.06),
        (0.0, -0.065, 0.055, 0.013),
        (-0.028, -0.033, 0.013, 0.06),
        (-0.028, 0.033, 0.013, 0.06),
        (0.0, 0.0, 0.055, 0.013),
    ];
    for (i, digit) in digits.bytes().enumerate() {
        let x = (i as f32 - (digits.len() as f32 - 1.0) / 2.0) * 0.1;
        for (bit, &(dx, dy, w, h)) in BARS.iter().enumerate() {
            if SEGMENTS[(digit - b'0') as usize] & (1 << bit) != 0 {
                out.add(
                    Mat::VCol,
                    &geom::plane(w, h),
                    t * geom::at(x + dx, dy, 0.0),
                    hex(0xe4e1cd),
                );
            }
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
    out.base.merge(entrance(b, lod));
    sign(b, &mut out);
    tank(b, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Obstacles;
    use crate::player::PLAYER_RADIUS;

    #[test]
    fn visible_entrances_leave_a_clear_player_passage_at_every_orientation() {
        let world = super::super::world();
        let mut checked = 0;
        let mut arcades = 0;
        for b in &world.blds {
            let Some(d) = b.door else { continue };
            let near = entrance(b, Lod::Near);
            assert!(near.vcount() > 90, "building {} has no door frame", b.id);
            assert!(near.vcount() <= 700, "unbounded door detail at {}", b.id);
            assert!(entrance(b, Lod::Mid).vcount() <= 250);
            assert_eq!(entrance(b, Lod::Far).vcount(), 0);
            let inv = place(d.p.x, 0.0, d.p.y, d.n.x.atan2(d.n.y)).inverse();
            for g in near.0.values() {
                let p: Vec<Vec3> = g
                    .pos
                    .iter()
                    .map(|p| inv.transform_point3(Vec3::from(*p)))
                    .collect();
                for tri in g.idx.as_chunks::<3>().0 {
                    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                    for &i in tri {
                        assert!(p[i as usize].is_finite());
                        lo = lo.min(p[i as usize]);
                        hi = hi.max(p[i as usize]);
                    }
                    // A 1.3 m wide, 2.05 m high passage, wider than the player's 0.9 m
                    // capsule, must remain free above the low traversable stone sill.
                    let overlaps = lo.x < 0.65
                        && hi.x > -0.65
                        && lo.y < 2.2
                        && hi.y > 0.15
                        && lo.z < 0.7
                        && hi.z > -0.3;
                    assert!(
                        !overlaps,
                        "entrance mesh blocks building {}: {lo:?}..{hi:?}",
                        b.id
                    );
                }
            }
            checked += 1;
            arcades += usize::from(b.arcade.is_some());
        }
        assert!(checked > 400 && arcades > 20);
    }

    #[test]
    fn home_door_passage_and_solid_flanks_match_player_collision() {
        let world = super::super::world();
        let home = world
            .blds
            .iter()
            .filter(|b| b.kind == "home" && !b.fill && b.door.is_some())
            .min_by(|a, b| {
                a.door
                    .unwrap()
                    .p
                    .distance_squared(world.spawn)
                    .total_cmp(&b.door.unwrap().p.distance_squared(world.spawn))
            })
            .expect("spawn has a home door");
        let d = home.door.unwrap();
        let obstacles = Obstacles(world.clone());
        for side in [-0.12, 0.0, 0.12] {
            for step in -6..=6 {
                let p = d.p + d.tangent() * side + d.n * step as f32 * 0.1;
                assert!(
                    obstacles.collide(p, PLAYER_RADIUS).distance(p) < 0.002,
                    "home {} door blocks the player at {p:?}",
                    home.id
                );
            }
        }
        let beside = d.p + d.tangent() * (DOOR_W / 2.0 + 0.3) + d.n * 0.12;
        assert!(
            obstacles.collide(beside, PLAYER_RADIUS).distance(beside) > 0.2,
            "the open door must not remove its adjacent wall collision"
        );
    }
}
