//! Close-range building detail, built lazily around the player (the original does the same with
//! its distance-sorted `roofJobs` and the facade radius):
//! - facade.ts window units at every painted window slot: dark reveal, lit / dark / curtained
//!   pane, two sliding glass sashes, 鋁窗 aluminium frame, 鐵窗 iron cages (4 patterns), tin
//!   awnings (雨遮), split AC units; some boarded up or broken;
//! - rooftin.ts 頂樓加蓋 tin shacks (gable / arched / mono-pitch roof, gutters, downpipe, door,
//!   barred windows, AC, tank on a stand, laundry pole, potted plants, antenna) placed on the
//!   roof along the lot, plus the 女兒牆 parapet and solar heater / antenna clutter (dressRoof);
//! - walkup.ts vertical shop-sign blades, AC units with drip stains and peeled-render brick.

use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, PI};

use super::build::{Mat, Parts, WHITE};
use super::data::*;
use super::textures::{TIN_PAL, VSIGN_N};
use crate::geom::{self, MeshBuf, hex, place, trs};

/// facade.ts prng (mulberry32).
pub struct Mulberry(pub u32);
impl Mulberry {
    pub fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6d2b79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64 / 4294967296.0) as f32
    }
    pub fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[(self.f() * v.len() as f32) as usize % v.len()]
    }
}

fn bx(w: f32, h: f32, d: f32, x: f32, y: f32, z: f32) -> MeshBuf {
    geom::cuboid(w, h, d).transformed(geom::at(x, y, z))
}

const WW: f32 = 1.7;
const WH: f32 = 1.35;

/// Shared window-unit parts (facade.ts `def`), local +Z out of the wall.
pub struct WindowKit {
    reveal: MeshBuf,
    pane: MeshBuf,
    glass: MeshBuf,
    alu: MeshBuf,
    cage: [MeshBuf; 4],
    ac: MeshBuf,
    ac_grill: MeshBuf,
    awn: MeshBuf,
    board: MeshBuf,
    curtain: MeshBuf,
    blind: MeshBuf,
}

fn frame(w: f32, h: f32, t: f32, d: f32, z: f32) -> [MeshBuf; 4] {
    [
        bx(w, t, d, 0.0, h / 2.0 - t / 2.0, z),
        bx(w, t, d, 0.0, -h / 2.0 + t / 2.0, z),
        bx(t, h, d, w / 2.0 - t / 2.0, 0.0, z),
        bx(t, h, d, -w / 2.0 + t / 2.0, 0.0, z),
    ]
}

impl WindowKit {
    pub fn new() -> Self {
        let merge = |v: Vec<MeshBuf>| {
            let mut m = MeshBuf::default();
            for g in v {
                m.append(g);
            }
            m
        };
        let mut alu: Vec<MeshBuf> = frame(WW, WH, 0.06, 0.07, 0.04).into();
        alu.push(bx(0.05, WH - 0.1, 0.05, -0.02, 0.0, 0.05));
        alu.push(bx(0.05, WH - 0.1, 0.05, 0.03, 0.0, 0.065));
        alu.push(bx(WW + 0.1, 0.05, 0.16, 0.0, -WH / 2.0 - 0.04, 0.08));
        let cage = |pat: usize| {
            let (w, h, d, t) = (WW + 0.3, WH + 0.3, 0.32, 0.035);
            let mut gs: Vec<MeshBuf> = frame(w, h, t, t, d).into();
            for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                gs.push(bx(t, t, d, sx * w / 2.0, sy * h / 2.0, d / 2.0));
            }
            gs.push(bx(w, 0.02, d, 0.0, h / 2.0, d / 2.0));
            gs.push(bx(w, 0.02, d, 0.0, -h / 2.0, d / 2.0));
            match pat {
                0 => {
                    let mut x = -w / 2.0 + 0.12;
                    while x < w / 2.0 {
                        gs.push(bx(0.016, h, 0.016, x, 0.0, d));
                        x += 0.12;
                    }
                }
                1 => {
                    let mut x = -w / 2.0 + 0.15;
                    while x < w / 2.0 {
                        gs.push(bx(0.016, h, 0.016, x, 0.0, d));
                        x += 0.15;
                    }
                    let mut y = -h / 2.0 + 0.3;
                    while y < h / 2.0 {
                        gs.push(bx(w, 0.016, 0.016, 0.0, y, d));
                        y += 0.3;
                    }
                }
                2 => {
                    // 花窗 diamond lattice, clipped to the frame
                    for i in -3..=3 {
                        for s in [0.785f32, -0.785] {
                            let l = h * 1.1;
                            let g = geom::cuboid(0.016, l, 0.016).transformed(
                                geom::at(i as f32 * 0.32, 0.0, d) * Mat4::from_rotation_z(s),
                            );
                            gs.push(g);
                        }
                    }
                }
                _ => {
                    let mut x = -w / 2.0 + 0.1;
                    while x < w / 2.0 {
                        gs.push(bx(0.016, h, 0.016, x, 0.0, d));
                        x += 0.1;
                    }
                    for k in 0..4 {
                        let g = geom::torus(0.16, 0.012, 4, 14, std::f32::consts::TAU).transformed(
                            geom::at(
                                -w / 2.0 + 0.25 + k as f32 * (w - 0.5) / 3.0,
                                h / 2.0 - 0.25,
                                d,
                            ),
                        );
                        gs.push(g);
                    }
                }
            }
            merge(gs)
        };
        Self {
            reveal: geom::plane(WW + 0.24, WH + 0.24).transformed(geom::at(0.0, 0.0, 0.012)),
            pane: geom::plane(WW - 0.1, WH - 0.1).transformed(geom::at(0.0, 0.0, 0.022)),
            glass: geom::plane(WW / 2.0, WH - 0.1),
            alu: merge(alu),
            cage: [cage(0), cage(1), cage(2), cage(3)],
            ac: merge(vec![
                bx(0.75, 0.48, 0.5, 0.0, 0.0, 0.25),
                bx(0.8, 0.03, 0.55, 0.0, -0.27, 0.25),
                bx(0.03, 0.03, 0.55, -0.3, -0.3, 0.25),
                bx(0.03, 0.03, 0.55, 0.3, -0.3, 0.25),
            ]),
            ac_grill: geom::cylinder(0.17, 0.17, 0.01, 12)
                .transformed(geom::at(0.12, 0.0, 0.502) * Mat4::from_rotation_x(FRAC_PI_2)),
            awn: geom::cuboid(WW + 0.5, 0.025, 0.55)
                .transformed(Mat4::from_rotation_x(0.35) * geom::at(0.0, 0.0, 0.27)),
            board: merge(vec![
                bx(WW + 0.1, 0.22, 0.03, 0.0, 0.35, 0.06),
                bx(WW + 0.1, 0.22, 0.03, 0.0, -0.05, 0.06),
                bx(WW + 0.1, 0.22, 0.03, 0.0, -0.42, 0.06),
                geom::cuboid(0.2, WH + 0.2, 0.03)
                    .transformed(geom::at(0.0, 0.0, 0.08) * Mat4::from_rotation_z(0.6)),
            ]),
            curtain: geom::plane(0.75, WH - 0.12).transformed(geom::at(0.0, 0.0, 0.028)),
            blind: geom::plane(WW - 0.14, 0.7).transformed(geom::at(0.0, 0.0, 0.029)),
        }
    }
}

impl Default for WindowKit {
    fn default() -> Self {
        Self::new()
    }
}

const RUST: [u32; 7] = [
    0x5a3220, 0x6e3c22, 0x7a4a2a, 0x3a3a36, 0xd0d0c8, 0x8a8a84, 0x3d5a46,
];
const LIT: [u32; 4] = [0xffd9a0, 0xfff0d8, 0xe8f4ff, 0xffc890];
const CUR: [u32; 6] = [0xe8dcc0, 0xd890a0, 0x7aa0c8, 0xc8c8a0, 0x9a6a5a, 0xf0f0f0];
const AWC: [u32; 10] = [
    0x6f8f86, 0x5f8a86, 0x7e948a, 0x6a8a80, 0x9a4a3a, 0x8a5040, 0xa85a46, 0x6f8f86, 0x9a4a3a,
    0x8a9aa0,
];

/// facade.ts window units for one building.
pub fn windows(b: &Bld, kit: &WindowKit, out: &mut Parts) {
    let mut r = Mulberry(20261025 ^ b.id.wrapping_mul(2654435761));
    let style = r.f();
    let alu_c = if style < 0.6 {
        0xb8bcc0
    } else if style < 0.85 {
        0x6a5a48
    } else {
        0x3a3a3a
    };
    let cage_pat = (r.f() * 4.0) as usize % 4;
    let cage_c = r.pick(&RUST);
    for w in &b.wins {
        let base = place(w.p.x, w.p.y, w.p.z, w.ry);
        let (cr, sr) = (w.ry.cos(), w.ry.sin());
        let side = |dx: f32| Vec3::new(w.p.x + cr * dx, w.p.y, w.p.z - sr * dx);
        out.add(Mat::VCol, &kit.reveal, base, hex(0x15181b));
        let q = r.f();
        if q < 0.07 {
            out.add(Mat::VCol, &kit.board, base, hex(0x9a7a4a));
            continue;
        }
        let broken = q < 0.13;
        let lit = !broken && r.f() < 0.38;
        if lit {
            // lit rooms read warm through the glass (unlit-looking bright vertex colour)
            let c = if r.f() < 0.16 { 0xffb060 } else { r.pick(&LIT) };
            out.add(Mat::Lamp, &kit.pane, base, hex(c));
        } else {
            let c = r.pick(&[0x3a4048u32, 0x48443c, 0x40404a]);
            out.add(Mat::VCol, &kit.pane, base, hex(c));
        }
        let cq = r.f();
        if cq < 0.45 {
            let cc = r.pick(&CUR);
            let dx = (WW / 2.0 - 0.37) * if r.f() < 0.5 { 1.0 } else { -1.0 };
            let p = side(dx);
            out.add(Mat::VCol, &kit.curtain, place(p.x, p.y, p.z, w.ry), hex(cc));
            if r.f() < 0.5 {
                let p = side(-dx);
                out.add(Mat::VCol, &kit.curtain, place(p.x, p.y, p.z, w.ry), hex(cc));
            }
        } else if cq < 0.65 {
            out.add(
                Mat::VCol,
                &kit.blind,
                base * geom::at(0.0, 0.3, 0.0),
                hex(0xe8e4d8),
            );
        }
        if !broken {
            let sl = (r.f() - 0.5) * 0.5;
            let a1 = -WW / 4.0 + sl.max(0.0);
            let a2 = WW / 4.0 + sl.min(0.0);
            out.add(
                Mat::Glass,
                &kit.glass,
                base * geom::at(a1, 0.0, 0.03),
                WHITE,
            );
            out.add(
                Mat::Glass,
                &kit.glass,
                base * geom::at(a2, 0.0, 0.045),
                WHITE,
            );
        }
        out.add(Mat::Metal, &kit.alu, base, hex(alu_c));
        if r.f() < 0.62 {
            let p = if r.f() < 0.8 {
                cage_pat
            } else {
                (r.f() * 4.0) as usize % 4
            };
            out.add(Mat::Metal, &kit.cage[p], base, hex(cage_c));
        }
        if r.f() < 0.42 {
            let c = r.pick(&AWC);
            out.add(
                Mat::Metal,
                &kit.awn,
                base * geom::at(0.0, WH / 2.0 + 0.25, 0.0),
                hex(c),
            );
        }
        if r.f() < 0.3 {
            let s = if r.f() < 0.5 { 1.0 } else { -1.0 };
            let ay = -0.35 + if r.f() < 0.5 { 0.0 } else { 0.6 };
            let p = side(s * (WW / 2.0 + 0.55));
            let t = place(p.x, p.y + ay, p.z, w.ry);
            let c = r.pick(&[0xd8d2c0u32, 0xe8e8e0, 0xb8b0a0]);
            out.add(Mat::VCol, &kit.ac, t, hex(c));
            out.add(Mat::VCol, &kit.ac_grill, t, hex(0x2a2a2a));
        }
    }
}

// ------------------------------------------------------------------ rooftops

fn pick_tin(q: f32) -> u32 {
    let tot: u32 = TIN_PAL.iter().map(|p| p.1).sum();
    let mut acc = 0.0;
    for (c, w) in TIN_PAL {
        acc += w as f32 / tot as f32;
        if q < acc {
            return c;
        }
    }
    TIN_PAL[0].0
}

/// uvMeters on the xy plane.
fn uv_m(g: MeshBuf) -> MeshBuf {
    g.map_uv(|_, p| [p[0], -p[1]])
}

/// rooftin.ts makeShack: a 頂樓加蓋 tin house, local frame (x along W, z along D, y up from the roof).
pub fn shack(w: f32, d: f32, r: &mut Park, open: bool) -> Parts {
    let mut o = Parts::default();
    let h = 2.5 + r.f() * 0.4;
    let wall_c = hex(pick_tin(r.f()));
    let roof_c = if r.f() < 0.6 {
        wall_c
    } else {
        hex(pick_tin(r.f()))
    };
    let det = |g: MeshBuf, t: Mat4, c: u32, o: &mut Parts| o.add(Mat::Metal, &g, t, hex(c));
    let wall = |len: f32, ry: f32, ox: f32, oz: f32, o: &mut Parts| {
        let p = uv_m(geom::plane2(len, h)).transformed(place(ox, h / 2.0, oz, ry));
        o.add(Mat::Tin, &p, Mat4::IDENTITY, wall_c);
    };
    if !open {
        wall(w, 0.0, 0.0, d / 2.0, &mut o);
        wall(w, PI, 0.0, -d / 2.0, &mut o);
        wall(d, FRAC_PI_2, w / 2.0, 0.0, &mut o);
        wall(d, -FRAC_PI_2, -w / 2.0, 0.0, &mut o);
    }
    let kind = r.f();
    let ov = 0.35;
    let long = w >= d;
    let l = if long { w } else { d } + ov * 2.0;
    let s = if long { d } else { w } + ov * 2.0;
    let rot = if long {
        Mat4::IDENTITY
    } else {
        Mat4::from_rotation_y(FRAC_PI_2)
    };
    let ridge;
    let gable_end = |pts: &[Vec2], ex: f32, o: &mut Parts| {
        let g = geom::shape(pts);
        let g = uv_m(g);
        let mut two = g.clone();
        two.append(g.transformed(Mat4::from_rotation_y(PI)));
        let t = rot * geom::at(ex, h, 0.0) * Mat4::from_rotation_y(FRAC_PI_2);
        o.add(Mat::Tin, &two, t, wall_c);
    };
    if kind < 0.45 {
        // gable (雙斜)
        let pitch = 0.18 + r.f() * 0.12;
        ridge = s / 2.0 * pitch;
        for sd in [-1.0f32, 1.0] {
            let half = (s / 2.0).hypot(ridge);
            let p = uv_m(geom::plane2(l, half));
            let t = rot
                * geom::at(0.0, h + ridge / 2.0, sd * s / 4.0)
                * Mat4::from_rotation_x(-FRAC_PI_2 + sd * ridge.atan2(s / 2.0));
            o.add(Mat::Tin, &p, t, roof_c);
        }
        let s2 = (s - ov * 2.0) / 2.0;
        for e in [-1.0f32, 1.0] {
            let tri = [
                Vec2::new(-s2, 0.0),
                Vec2::new(s2, 0.0),
                Vec2::new(0.0, ridge * (s - ov * 2.0) / s),
            ];
            gable_end(&tri, e * (l - ov * 2.0) / 2.0, &mut o);
        }
        det(
            geom::cuboid(l, 0.12, 0.14),
            rot * geom::at(0.0, h + ridge + 0.03, 0.0),
            0x8a8e8c,
            &mut o,
        );
    } else if kind < 0.75 {
        // shallow arch (圓弧浪板)
        let rise = 0.35 + r.f() * 0.3;
        let rr = (s * s / 4.0 + rise * rise) / (2.0 * rise);
        let th = 2.0 * (s / 2.0 / rr).asin();
        ridge = rise;
        let c = geom::cylinder_ex(rr, rr, l, 18, true, -th / 2.0, th);
        let mut c2 = c.clone();
        // inside face too (DoubleSide)
        let mut inner = c.clone();
        for t in inner.idx.chunks_mut(3) {
            t.swap(1, 2);
        }
        for n in inner.nrm.iter_mut() {
            *n = [-n[0], -n[1], -n[2]];
        }
        c2.append(inner);
        let c2 = c2.map_uv(|uv, p| [p[1], uv[0] * th * rr]);
        let t = rot * geom::at(0.0, h + rise - rr, 0.0) * Mat4::from_rotation_z(FRAC_PI_2);
        o.add(Mat::Tin, &c2, t, roof_c);
        let s2 = (s - ov * 2.0) / 2.0;
        let mut prof = vec![Vec2::new(-s2, 0.0), Vec2::new(s2, 0.0)];
        for k in 0..=8 {
            let z = s2 - k as f32 / 8.0 * 2.0 * s2;
            prof.push(Vec2::new(
                z,
                ((rr * rr - z * z).max(0.0).sqrt() - (rr - rise)).max(0.0),
            ));
        }
        for e in [-1.0f32, 1.0] {
            gable_end(&prof, e * (l - ov * 2.0) / 2.0, &mut o);
        }
    } else {
        // mono-pitch (單斜)
        let drop = 0.35 + r.f() * 0.25;
        ridge = drop;
        let p = uv_m(geom::plane2(l, s.hypot(drop)));
        let t = rot
            * geom::at(0.0, h + drop / 2.0, 0.0)
            * Mat4::from_rotation_x(-FRAC_PI_2 - drop.atan2(s));
        o.add(Mat::Tin, &p, t, roof_c);
        let hi = uv_m(geom::plane2(l - ov * 2.0, drop));
        o.add(
            Mat::Tin,
            &hi,
            rot * geom::at(0.0, h + drop / 2.0, (s - ov * 2.0) / 2.0),
            wall_c,
        );
    }
    // eave gutters + downpipe
    det(
        geom::cuboid(l, 0.1, 0.12),
        rot * geom::at(0.0, h - 0.02, s / 2.0 - 0.06),
        0x9a9e9a,
        &mut o,
    );
    det(
        geom::cuboid(l, 0.1, 0.12),
        rot * geom::at(0.0, h - 0.02, -s / 2.0 + 0.06),
        0x9a9e9a,
        &mut o,
    );
    det(
        geom::cuboid(0.08, h, 0.08),
        geom::at(w / 2.0 + 0.06, h / 2.0, d / 2.0 + 0.06),
        0x8a8e8a,
        &mut o,
    );
    if open {
        for (x, z) in [
            (1.0, 1.0),
            (1.0, -1.0),
            (-1.0, 1.0),
            (-1.0, -1.0),
            (0.0, 1.0),
            (0.0, -1.0),
        ] {
            det(
                geom::cuboid(0.09, h, 0.09),
                geom::at(x * (w / 2.0 - 0.1), h / 2.0, z * (d / 2.0 - 0.1)),
                0x6a6e6a,
                &mut o,
            );
        }
    } else {
        for (x, z) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            det(
                geom::cuboid(0.07, h, 0.07),
                geom::at(x * w / 2.0, h / 2.0, z * d / 2.0),
                0xb8bab4,
                &mut o,
            );
        }
        // door on the front (+z) + aluminium windows with 鐵窗 bars
        let dx = (r.f() - 0.5) * (w - 1.4);
        let dc = [0x6a5040u32, 0x8a8a84, 0x3a5a6a][(r.f() * 3.0) as usize % 3];
        det(
            geom::cuboid(0.85, 2.0, 0.05),
            geom::at(dx, 1.0, d / 2.0 + 0.03),
            dc,
            &mut o,
        );
        det(
            geom::cuboid(0.06, 0.06, 0.06),
            geom::at(dx + 0.32, 1.0, d / 2.0 + 0.07),
            0xd0c080,
            &mut o,
        );
        let win = |cx: f32, face: f32, ww: f32, o: &mut Parts| {
            let z = face * (d / 2.0 + 0.04);
            o.add(
                Mat::Glass,
                &geom::cuboid(ww, 0.9, 0.02),
                geom::at(cx, 1.55, z),
                WHITE,
            );
            for (bw, bh, bd, x, y) in [
                (ww + 0.08, 0.05, 0.05, cx, 2.02),
                (ww + 0.08, 0.05, 0.05, cx, 1.08),
                (0.05, 0.95, 0.05, cx - ww / 2.0, 1.55),
                (0.05, 0.95, 0.05, cx + ww / 2.0, 1.55),
                (0.03, 0.9, 0.03, cx, 1.55),
            ] {
                o.add(
                    Mat::Metal,
                    &geom::cuboid(bw, bh, bd),
                    geom::at(x, y, z),
                    hex(0xd8dcdc),
                );
            }
            let bz = z + face * 0.14;
            let n = (ww / 0.14).round() as i32;
            for k in 0..=n {
                o.add(
                    Mat::Metal,
                    &geom::cuboid(0.02, 1.1, 0.02),
                    geom::at(cx - ww / 2.0 + k as f32 * 0.14, 1.55, bz),
                    hex(0x3a3e3e),
                );
            }
            for y in [2.1, 1.0] {
                o.add(
                    Mat::Metal,
                    &geom::cuboid(ww + 0.1, 0.03, 0.2),
                    geom::at(cx, y, z + face * 0.08),
                    hex(0x3a3e3e),
                );
            }
        };
        let nw = ((w - 1.6) / 1.8).floor().max(1.0) as i32;
        for i in 0..nw {
            let x = -w / 2.0 + 0.9 + (i as f32 + 0.5) * (w - 1.8) / nw as f32;
            if (x - dx).abs() > 1.0 {
                win(x, 1.0, 1.0, &mut o);
            }
        }
        if r.f() < 0.7 {
            let x = (r.f() - 0.5) * (w - 1.6);
            win(x, -1.0, 1.1, &mut o);
        }
        for _ in 0..2 {
            if r.f() < 0.6 {
                let (pw, ph) = (0.6 + r.f() * 0.8, 0.6 + r.f() * 0.9);
                let (px, py) = ((r.f() - 0.5) * (w - 1.0), 0.5 + r.f() * 1.4);
                let c = [0x8a5038u32, 0xb0aaa0, 0x5a7a74][(r.f() * 3.0) as usize % 3];
                o.add(
                    Mat::Tin,
                    &geom::cuboid(pw, ph, 0.03),
                    geom::at(px, py, -d / 2.0 - 0.03),
                    hex(c),
                );
            }
        }
    }
    // AC unit on the wall, stainless tank on a stand, laundry pole, pots, antenna
    if r.f() < 0.7 {
        let z = (r.f() - 0.5) * (d - 1.0);
        det(
            geom::cuboid(0.8, 0.55, 0.3),
            geom::at(w / 2.0 + 0.2, 1.9, z) * Mat4::from_rotation_y(FRAC_PI_2),
            0xe0ddd2,
            &mut o,
        );
    }
    if r.f() < 0.6 {
        let (tx, tz, ty) = (
            (r.f() - 0.5) * w * 0.5,
            (r.f() - 0.5) * d * 0.4,
            h + ridge + 0.1,
        );
        det(
            geom::cylinder(0.5, 0.5, 1.1, 12),
            geom::at(tx, ty + 0.9, tz) * Mat4::from_rotation_z(FRAC_PI_2),
            0xd4d8da,
            &mut o,
        );
        for (a, b) in [(-0.4, -0.3), (0.4, -0.3), (-0.4, 0.3), (0.4, 0.3)] {
            det(
                geom::cuboid(0.05, 0.4, 0.05),
                geom::at(tx + a, ty + 0.2, tz + b),
                0x707470,
                &mut o,
            );
        }
    }
    if r.f() < 0.5 {
        let lz = d / 2.0 + 0.9;
        det(
            geom::cuboid(0.05, 1.8, 0.05),
            geom::at(-w / 2.0 + 0.4, 0.9, lz),
            0x9aa0a0,
            &mut o,
        );
        det(
            geom::cuboid(0.05, 1.8, 0.05),
            geom::at(w / 2.0 - 0.4, 0.9, lz),
            0x9aa0a0,
            &mut o,
        );
        det(
            geom::cuboid(w - 0.8, 0.03, 0.03),
            geom::at(0.0, 1.75, lz),
            0x9aa0a0,
            &mut o,
        );
        for (k, c) in [0xd84a4au32, 0xe8e0c8, 0x3a6aa8, 0xe8c040]
            .iter()
            .enumerate()
        {
            o.add(
                Mat::VCol,
                &geom::cuboid(0.4, 0.6, 0.02),
                geom::at(-w / 2.0 + 1.0 + k as f32 * (w - 2.0) / 4.0, 1.42, lz),
                hex(*c),
            );
        }
    }
    for _ in 0..3 {
        if r.f() < 0.5 {
            let px = -w / 2.0 + 0.3 + r.f() * (w - 0.6);
            o.add(
                Mat::VCol,
                &geom::cuboid(0.35, 0.35, 0.35),
                geom::at(px, 0.18, d / 2.0 + 0.35),
                hex(0x8a5a3a),
            );
            o.add(
                Mat::Foliage,
                &geom::sphere(0.3, 6, 4),
                geom::at(px, 0.55, d / 2.0 + 0.35),
                hex(0x3a7a3a),
            );
        }
    }
    if r.f() < 0.5 {
        let ax = (r.f() - 0.5) * w * 0.6;
        det(
            geom::cuboid(0.04, 1.6, 0.04),
            geom::at(ax, h + ridge + 0.8, 0.0),
            0x777777,
            &mut o,
        );
        det(
            geom::cuboid(1.0, 0.03, 0.03),
            geom::at(ax, h + ridge + 1.4, 0.0),
            0x777777,
            &mut o,
        );
        det(
            geom::cuboid(0.7, 0.03, 0.03),
            geom::at(ax, h + ridge + 1.15, 0.0),
            0x777777,
            &mut o,
        );
    }
    o
}

/// Oriented frame of a footprint along its longest edge: origin, u, n (inward), length, depth.
fn lot_frame(pts: &[Vec2]) -> (Vec2, Vec2, Vec2, f32, f32) {
    let (mut best, mut u) = (0.0f32, Vec2::X);
    for i in 0..pts.len() {
        let d = pts[(i + 1) % pts.len()] - pts[i];
        if d.length() > best {
            best = d.length();
            u = d / best;
        }
    }
    let n = Vec2::new(-u.y, u.x);
    let (mut a0, mut a1, mut b0, mut b1) = (1e9f32, -1e9f32, 1e9f32, -1e9f32);
    for p in pts {
        a0 = a0.min(p.dot(u));
        a1 = a1.max(p.dot(u));
        b0 = b0.min(p.dot(n));
        b1 = b1.max(p.dot(n));
    }
    let o = u * (a0 + a1) / 2.0 + n * (b0 + b1) / 2.0;
    (o, u, n, a1 - a0, b1 - b0)
}

/// placeRooftop (simplified): one shack along the lot, kept inside the footprint and away from
/// the street front, then dressRoof's parapet and clutter.
pub fn rooftop(b: &Bld, out: &mut Parts) {
    let h = b.h;
    // 女兒牆 parapet along every roof edge
    for i in 0..b.pts.len() {
        let (a, c) = (b.pts[i], b.pts[(i + 1) % b.pts.len()]);
        let l = a.distance(c);
        if l < 0.3 {
            continue;
        }
        let d = c - a;
        let m = (a + c) / 2.0;
        out.add(
            Mat::VCol,
            &geom::cuboid(l + 0.2, 0.9, 0.18),
            place(m.x, h + 0.45, m.y, -d.y.atan2(d.x)),
            hex(0xbdb8ac),
        );
    }
    let mut r = Park(b.seed7);
    if b.shack {
        let (o, u, n, len, dep) = lot_frame(&b.pts);
        let mut w = (len * (0.55 + r.f() * 0.3)).clamp(2.6, 11.0);
        let mut d = (dep * (0.45 + r.f() * 0.3)).clamp(2.4, 8.0);
        // push toward the back of the lot (away from the door)
        let back = b.door.map_or(Vec2::ZERO, |dr| -dr.n);
        let shift = if back.dot(n) > 0.0 { n } else { -n } * (dep - d) * 0.3;
        for _ in 0..4 {
            let c = o + shift;
            let corners = [
                c + u * (w / 2.0 + 0.35) + n * (d / 2.0 + 0.35),
                c - u * (w / 2.0 + 0.35) + n * (d / 2.0 + 0.35),
                c + u * (w / 2.0 + 0.35) - n * (d / 2.0 + 0.35),
                c - u * (w / 2.0 + 0.35) - n * (d / 2.0 + 0.35),
                c,
            ];
            if corners.iter().all(|p| pip(*p, &b.pts)) {
                let open = r.f() < 0.12;
                let s = shack(w, d, &mut r, open);
                let ang = -u.y.atan2(u.x);
                let t = place(c.x, h, c.y, ang);
                for (k, g) in s.0 {
                    out.add(k, &g, t, WHITE);
                }
                break;
            }
            w *= 0.8;
            d *= 0.8;
            if w < 2.4 || d < 2.2 {
                break;
            }
        }
    }
    // dressRoof clutter: 太陽能熱水器, antenna, small AC compressors
    let mut r = Park(b.seed8);
    let (o, u, n, len, dep) = lot_frame(&b.pts);
    let spot = |r: &mut Park| o + u * (r.f() - 0.5) * len * 0.8 + n * (r.f() - 0.5) * dep * 0.8;
    if b.area > 25.0 && r.f() < 0.45 {
        let p = spot(&mut r);
        if pip(p, &b.pts) && b.tank.is_none_or(|t| t.0.distance(p) > 2.0) {
            let ang = -u.y.atan2(u.x);
            let t = place(p.x, h, p.y, ang);
            out.add(
                Mat::Glass,
                &geom::cuboid(1.9, 0.08, 1.0),
                t * trs([0.0, 0.7, 0.0], [-0.6, 0.0, 0.0], [1.0; 3]),
                WHITE,
            );
            out.add(
                Mat::Metal,
                &geom::cylinder(0.22, 0.22, 1.9, 10),
                t * trs([0.0, 1.15, -0.45], [0.0, 0.0, FRAC_PI_2], [1.0; 3]),
                hex(0xd8dcdc),
            );
            for x in [-0.8, 0.8] {
                out.add(
                    Mat::Metal,
                    &geom::cuboid(0.05, 1.0, 0.05),
                    t * geom::at(x, 0.5, -0.4),
                    hex(0x7a7e7c),
                );
            }
        }
    }
    for _ in 0..(b.area / 80.0).min(3.0) as usize {
        let p = spot(&mut r);
        if pip(p, &b.pts) && r.f() < 0.6 {
            let ang = -u.y.atan2(u.x);
            out.add(
                Mat::VCol,
                &geom::cuboid(0.82, 0.56, 0.32),
                place(p.x, h + 0.28, p.y, ang),
                hex(0xe6e2d8),
            );
            out.add(
                Mat::VCol,
                &geom::cuboid(0.42, 0.42, 0.02),
                place(p.x, h + 0.28, p.y, ang) * geom::at(-0.12, 0.0, 0.17),
                hex(0x3a3c3e),
            );
        }
    }
    if r.f() < 0.35 {
        let p = spot(&mut r);
        if pip(p, &b.pts) {
            out.add(
                Mat::Metal,
                &geom::cuboid(0.04, 2.2, 0.04),
                geom::at(p.x, h + 1.1, p.y),
                hex(0x777777),
            );
            for (y, l) in [(1.9, 1.2), (1.6, 0.9), (1.3, 0.6)] {
                out.add(
                    Mat::Metal,
                    &geom::cuboid(l, 0.03, 0.03),
                    geom::at(p.x, h + y, p.y),
                    hex(0x777777),
                );
            }
        }
    }
}

/// walkup.ts street dressing: vertical sign blades, split AC units with drip stains, and
/// peeled render showing red brick, on the door edge of low walk-ups.
pub fn walkup(b: &Bld, out: &mut Parts) {
    let Some(door) = b.door else { return };
    if b.lv > 5 {
        return;
    }
    let mut r = Park(b.seed8 ^ 0x5bd1);
    // the door edge is the wall whose midpoint is nearest the door, facing the same way
    let Some(w) = b
        .walls
        .iter()
        .filter(|w| w.y0 == 0.0 || w.y0 == GF)
        .max_by(|a, c| a.a.distance(a.c).total_cmp(&c.a.distance(c.c)))
    else {
        return;
    };
    let l = w.a.distance(w.c);
    if l < 3.0 {
        return;
    }
    let u = (w.c - w.a) / l;
    let mut n = Vec2::new(-u.y, u.x);
    if n.dot(door.n) < 0.0 {
        n = -n;
    }
    let ry = n.x.atan2(n.y);
    let lt = |s: f32, y: f32, z: f32| {
        let p = w.a + u * s + n * (0.125 + z);
        place(p.x, y, p.y, ry)
    };
    if b.h > GF + FH && r.f() < 0.55 {
        let idx = (r.f() * VSIGN_N as f32) as usize % VSIGN_N;
        let s = if r.f() < 0.5 { 0.6 } else { l - 0.6 };
        let g = geom::cuboid(0.1, 2.8, 0.75)
            .map_uv(|uv, _| [(idx as f32 + uv[0]) / VSIGN_N as f32, 1.0 - uv[1]]);
        let y = (GF + 1.6).min(b.h - 1.5);
        out.add(Mat::VSign, &g, lt(s, y, 0.5), WHITE);
    }
    let floors = ((b.h - GF) / FH).floor() as i32;
    for f in 0..floors.max(0) {
        if r.f() < 0.45 {
            let s = 0.8 + r.f() * (l - 1.6);
            let y = GF + f as f32 * FH + 1.0;
            let t = lt(s, y, 0.0);
            out.add(
                Mat::VCol,
                &geom::cuboid(0.82, 0.56, 0.3),
                t * geom::at(0.0, 0.0, 0.2),
                hex(0xe6e2d8),
            );
            out.add(
                Mat::VCol,
                &geom::cuboid(0.42, 0.42, 0.02),
                t * geom::at(-0.12, 0.0, 0.36),
                hex(0x3a3c3e),
            );
            out.add(
                Mat::VCol,
                &geom::cuboid(0.84, 0.04, 0.4),
                t * geom::at(0.0, -0.3, 0.2),
                hex(0x5a5048),
            );
            out.add(
                Mat::VCol,
                &geom::cuboid(0.08, 0.9, 0.01),
                t * geom::at(0.3, -0.8, 0.012),
                hex(0x7d7870),
            );
        }
    }
    if r.f() < 0.5 {
        let (s, y) = (0.6 + r.f() * (l - 1.2), 1.0 + r.f() * (b.h - 2.0).max(0.5));
        for k in 0..2 + (r.f() * 2.0) as i32 {
            let (pw, ph) = (0.5 + r.f() * 1.2, 0.4 + r.f() * 0.9);
            let g = geom::plane(pw, ph).map_uv(|uv, _| [uv[0] * pw, uv[1] * ph]);
            let t = lt(
                s + (r.f() - 0.5) * 0.8,
                y + (r.f() - 0.5) * 0.6,
                0.015 + k as f32 * 0.002,
            );
            out.add(Mat::Brick, &g, t, WHITE);
        }
    }
}

/// Everything the near layer adds to a building.
pub fn near_detail(b: &Bld, kit: &WindowKit) -> Parts {
    let mut p = Parts::default();
    windows(b, kit, &mut p);
    walkup(b, &mut p);
    p
}

/// Rooftop shack + clutter (separate so the roof cutaway can hide it with the roof).
pub fn near_roof(b: &Bld) -> Parts {
    let mut p = Parts::default();
    if b.rcls != RoofClass::None || b.area < 600.0 {
        rooftop(b, &mut p);
    }
    p
}
