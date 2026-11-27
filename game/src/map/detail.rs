//! Close-range building detail, built lazily around the player (the original does the same with
//! its distance-sorted `roofJobs` and the facade radius):
//! - facade.ts window units at every painted window slot: dark reveal, lit / dark / curtained
//!   pane, two sliding glass sashes, 鋁窗 aluminium frame, 鐵窗 iron cages (4 patterns), tin
//!   awnings (雨遮), split AC units; some boarded up or broken;
//! - rooftop fine fittings: gable / arched / mono-pitch shacks are split into a persistent
//!   shell and near-only doors, barred windows, AC, laundry and plants. Shared roof layouts
//!   in roofs.rs reserve the whole footprint and preserve the silhouette at Mid LOD;
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
    surround: MeshBuf,
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
            // A raised masonry opening makes the sash sit in a real 22 cm-deep reveal.
            // The inner edge stays outside the lit pane; this is not an opaque front plate.
            surround: merge(
                frame(WW + 0.30, WH + 0.30, 0.12, 0.22, 0.11)
                    .into_iter()
                    .chain([bx(WW + 0.34, 0.07, 0.32, 0.0, -WH / 2.0 - 0.13, 0.13)])
                    .collect(),
            ),
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
    let exterior = super::exterior::plan(b, super::world());
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
        // Rear access landings occupy a few original facade slots. Consume their RNG
        // draws as before, so hiding one slot cannot change the remaining room lights.
        let mut suppressed = Parts::default();
        let out = if super::exterior::window_obscured(&exterior, w) {
            &mut suppressed
        } else {
            &mut *out
        };
        let base = place(w.p.x, w.p.y, w.p.z, w.ry);
        let (cr, sr) = (w.ry.cos(), w.ry.sin());
        let side = |dx: f32| Vec3::new(w.p.x + cr * dx, w.p.y, w.p.z - sr * dx);
        out.add(Mat::VCol, &kit.reveal, base, hex(0x15181b));
        const REVEALS: [u32; 7] = [
            0xc3bbaa, 0xb8b5aa, 0xcac1ac, 0xb4aaa0, 0xbca88e, 0xb4bdbc, 0xc4bbae,
        ];
        out.add(
            Mat::VCol,
            &kit.surround,
            base,
            hex(REVEALS[b.wall_mat % REVEALS.len()]),
        );
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
            out.add(Mat::WindowGlow, &kit.pane, base, hex(c));
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
        // Mat::Glass is an opaque reflective pane. It can cover an unlit room,
        // but would completely hide the emissive occupied-room pane behind it.
        // Lit rooms retain their curtains, sash frames and exterior iron cage.
        if !broken && !lit {
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

/// rooftin.ts makeShack, split into independent shell/fine layers from one stable seed.
/// Local x spans width, +z is its front, and y rises from the building's existing roof.
pub(crate) fn shack_layer(w: f32, d: f32, seed: u64, open: bool, fine: bool) -> Parts {
    let mut r = Park(seed);
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
        let p = uv_m(geom::plane(len, h)).transformed(place(ox, h / 2.0, oz, ry));
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
        let mut face = if open {
            let mut two = g.clone();
            let mut back = g;
            for triangle in back.idx.chunks_mut(3) {
                triangle.swap(1, 2);
            }
            for normal in &mut back.nrm {
                *normal = [-normal[0], -normal[1], -normal[2]];
            }
            two.append(back);
            two
        } else {
            g
        };
        if !open && ex < 0.0 {
            for triangle in face.idx.chunks_mut(3) {
                triangle.swap(1, 2);
            }
            for normal in &mut face.nrm {
                *normal = [-normal[0], -normal[1], -normal[2]];
            }
        }
        let t = rot * geom::at(ex, h, 0.0) * Mat4::from_rotation_y(FRAC_PI_2);
        o.add(Mat::Tin, &face, t, wall_c);
    };
    let closed_eave = |side: f32, height: f32, o: &mut Parts| {
        if !open {
            o.add(
                Mat::Tin,
                &uv_m(geom::plane(l - ov * 2.0, height)),
                rot * geom::at(0.0, h + height / 2.0, side * (s - ov * 2.0) / 2.0)
                    * Mat4::from_rotation_y(if side > 0.0 { 0.0 } else { PI }),
                wall_c,
            );
        }
    };
    if kind < 0.45 {
        // gable (雙斜)
        let pitch = 0.18 + r.f() * 0.12;
        ridge = s / 2.0 * pitch;
        for sd in [-1.0f32, 1.0] {
            let half = (s / 2.0).hypot(ridge);
            let p = uv_m(if open {
                geom::plane2(l, half)
            } else {
                geom::plane(l, half)
            });
            let t = rot
                * geom::at(0.0, h + ridge / 2.0, sd * s / 4.0)
                * Mat4::from_rotation_x(-FRAC_PI_2 + sd * ridge.atan2(s / 2.0));
            o.add(Mat::Tin, &p, t, roof_c);
        }
        let s2 = (s - ov * 2.0) / 2.0;
        let edge = pitch * ov;
        for e in [-1.0f32, 1.0] {
            let profile = [
                Vec2::new(-s2, 0.0),
                Vec2::new(s2, 0.0),
                Vec2::new(s2, edge),
                Vec2::new(0.0, ridge),
                Vec2::new(-s2, edge),
            ];
            gable_end(&profile, e * (l - ov * 2.0) / 2.0, &mut o);
            closed_eave(e, edge, &mut o);
        }
    } else if kind < 0.75 {
        // shallow arch (圓弧浪板)
        let rise = 0.35 + r.f() * 0.3;
        let rr = (s * s / 4.0 + rise * rise) / (2.0 * rise);
        let th = 2.0 * (s / 2.0 / rr).asin();
        ridge = rise;
        let c = geom::cylinder_ex(rr, rr, l, 8, true, -th / 2.0, th);
        let mut c2 = c.clone();
        // Only an open canopy exposes its underside. Closed sheds have no visible
        // interior; keeping a second inward skin for every resident shed wastes vertices.
        if open {
            let mut inner = c.clone();
            for t in inner.idx.chunks_mut(3) {
                t.swap(1, 2);
            }
            for n in inner.nrm.iter_mut() {
                *n = [-n[0], -n[1], -n[2]];
            }
            c2.append(inner);
        }
        let c2 = c2.map_uv(|uv, p| [p[1], uv[0] * th * rr]);
        // cylinder_ex's arc is centred on local +Z, not +X. Rotate that radial
        // direction upward while laying its Y axis along the shed. Without the
        // second rotation the large-radius arc sits beside/below the building.
        let t = rot
            * geom::at(0.0, h + rise - rr, 0.0)
            * Mat4::from_rotation_z(FRAC_PI_2)
            * Mat4::from_rotation_y(FRAC_PI_2);
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
            closed_eave(e, (rr * rr - s2 * s2).max(0.0).sqrt() - (rr - rise), &mut o);
        }
    } else {
        // mono-pitch (單斜)
        let drop = 0.35 + r.f() * 0.25;
        ridge = drop;
        let p = uv_m(if open {
            geom::plane2(l, s.hypot(drop))
        } else {
            geom::plane(l, s.hypot(drop))
        });
        let t = rot
            * geom::at(0.0, h + drop / 2.0, 0.0)
            * Mat4::from_rotation_x(-FRAC_PI_2 - drop.atan2(s));
        o.add(Mat::Tin, &p, t, roof_c);
        let low = drop * ov / s;
        let high = drop - low;
        for (side, edge_height) in [(1.0, high), (-1.0, low)] {
            let edge = uv_m(if open {
                geom::plane2(l - ov * 2.0, edge_height)
            } else {
                geom::plane(l - ov * 2.0, edge_height)
            });
            o.add(
                Mat::Tin,
                &edge,
                rot * geom::at(0.0, h + edge_height / 2.0, side * (s - ov * 2.0) / 2.0)
                    * Mat4::from_rotation_y(if side > 0.0 { 0.0 } else { PI }),
                wall_c,
            );
        }
        // Close the two sloping side wedges as well as the high front edge. Their
        // exterior faces remain visible after the redundant inward skins are removed.
        let half = (s - ov * 2.0) * 0.5;
        let profile = [
            Vec2::new(-half, 0.0),
            Vec2::new(half, 0.0),
            Vec2::new(half, low),
            Vec2::new(-half, high),
        ];
        for side in [-1.0, 1.0] {
            gable_end(&profile, side * (l - ov * 2.0) * 0.5, &mut o);
        }
    }
    let roof_height = |x: f32, z: f32| {
        let cross = if long { z } else { x };
        if kind < 0.45 {
            h + ridge * (1.0 - cross.abs() / (s * 0.5))
        } else if kind < 0.75 {
            let radius = (s * s * 0.25 + ridge * ridge) / (2.0 * ridge);
            h + ridge - radius + (radius * radius - cross * cross).max(0.0).sqrt()
        } else {
            h + ridge * 0.5 + ridge * cross / s
        }
    };
    // Open canopies retain their four structural corner posts at every LOD; they
    // must never look like floating roofs. The two intermediate posts are fine detail.
    if open {
        for (x, z) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            let px = x * (w / 2.0 - 0.1);
            let pz = z * (d / 2.0 - 0.1);
            let top = roof_height(px, pz);
            det(
                geom::cuboid(0.09, top, 0.09),
                geom::at(px, top / 2.0, pz),
                0x6a6e6a,
                &mut o,
            );
        }
    }
    // Both layers consume the same shape RNG before branching. Gutters, downpipes,
    // ridge trim and small edge uprights are near fittings, not resident silhouette.
    if !fine {
        return o;
    }
    o = Parts::default();
    if kind < 0.45 {
        det(
            geom::cuboid(l, 0.12, 0.14),
            rot * geom::at(0.0, h + ridge + 0.03, 0.0),
            0x8a8e8c,
            &mut o,
        );
    }
    for side in [-1.0, 1.0] {
        det(
            geom::cuboid(l, 0.1, 0.12),
            rot * geom::at(0.0, h - 0.02, side * (s / 2.0 - 0.06)),
            0x9a9e9a,
            &mut o,
        );
    }
    det(
        geom::cuboid(0.08, h, 0.08),
        geom::at(w / 2.0 + 0.06, h / 2.0, d / 2.0 + 0.06),
        0x8a8e8a,
        &mut o,
    );
    if open {
        for z in [-1.0, 1.0] {
            let pz = z * (d / 2.0 - 0.1);
            let top = roof_height(0.0, pz);
            det(
                geom::cuboid(0.09, top, 0.09),
                geom::at(0.0, top / 2.0, pz),
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
    }
    if !open {
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

/// Outward normal of an actual wall slab. Recessed arcade walls are inside the footprint,
/// so polygon containment alone cannot tell which side is their public-facing side.
fn wall_normal(b: &Bld, w: &WallBox) -> Vec2 {
    let u = (w.c - w.a).normalize_or_zero();
    let mut n = Vec2::new(-u.y, u.x);
    let mid = (w.a + w.c) / 2.0;
    if let Some(d) = b.door
        && u.dot(d.n).abs() < 0.01
        && (mid - d.p).dot(d.n).abs() < 0.05
    {
        if n.dot(d.n) < 0.0 {
            n = -n;
        }
    } else if pip(mid + n * 0.4, &b.pts) {
        n = -n;
    }
    n
}

/// A small ragged render / paper patch, in the wall's XY plane with +Z facing out.
/// The edge has eight unequal corners rather than repeated rectangular stickers.
fn ragged_patch(w: f32, h: f32, r: &mut Park) -> MeshBuf {
    let mut g = MeshBuf::default();
    g.pos.push([0.0, 0.0, 0.0]);
    g.nrm.push([0.0, 0.0, 1.0]);
    g.uv.push([w / 2.0, h / 2.0]);
    g.col.push(WHITE);
    for k in 0..8 {
        let ang = k as f32 * PI / 4.0;
        let s = 0.7 + r.f() * 0.3;
        let (x, y) = (ang.cos() * w * 0.5 * s, ang.sin() * h * 0.5 * s);
        g.pos.push([x, y, 0.0]);
        g.nrm.push([0.0, 0.0, 1.0]);
        g.uv.push([x + w / 2.0, h / 2.0 - y]);
        g.col.push(WHITE);
    }
    for k in 0..8u32 {
        g.idx.extend([0, k + 1, (k + 1) % 8 + 1]);
    }
    g
}

/// Ground-level details from walkup.ts / decay.ts: mixed tiled dado, exposed render,
/// water pipes, electrical meters and weathered notices. They follow individual wall
/// slabs, whose door holes are already cut, so none can seal an entrance accidentally.
fn frontage_wear(b: &Bld, out: &mut Parts) {
    let mut r = Park(b.seed7 ^ 0x6d70_8f31);
    let commercial = super::commercial::plan(b, super::world());
    let Some(door) = b.door else { return };
    let mut walls: Vec<&WallBox> = b
        .walls
        .iter()
        .filter(|w| w.y0 < 0.01 && w.y1 >= DOOR_H && w.a.distance(w.c) > 1.0)
        .collect();
    // Prefer the front wall portions on both sides of the entrance. A highly irregular
    // OSM footprint must not turn into an unlimited number of utility/detail batches.
    walls.sort_by(|a, c| {
        let rank = |w: &WallBox| {
            let facing = wall_normal(b, w).dot(door.n);
            let dist = ((w.a + w.c) / 2.0).distance(door.p);
            dist + if facing > 0.95 { 0.0 } else { 1000.0 }
        };
        rank(a).total_cmp(&rank(c))
    });
    for (index, w) in walls.into_iter().take(6).enumerate() {
        // Preserve every old RNG draw even when a new closed shopfront owns this
        // wall. Meters and brick patches must not poke through its glazing/shutter.
        let mut covered = Parts::default();
        let out = if commercial
            .as_ref()
            .is_some_and(|plan| super::commercial::covers_wall(plan, w))
        {
            &mut covered
        } else {
            &mut *out
        };
        let l = w.a.distance(w.c);
        let mid = (w.a + w.c) / 2.0;
        let n = wall_normal(b, w);
        let t = place(mid.x, 0.0, mid.y, n.x.atan2(n.y));
        let front = n.dot(door.n) > 0.95;
        // Old owners re-tile only the bottom of their own frontage. Different bays retain
        // their own age/palette, while wall textures remain visible above the splash zone.
        if front || r.f() < 0.65 {
            let h = 0.74 + r.f() * 0.24;
            let g = geom::plane(l, h).map_uv(|uv, _| [uv[0] * l, uv[1] * h]);
            out.add(
                Mat::ArcadeFloor(((b.id as usize + index) % super::textures::ARCADE_TILE_N) as u8),
                &g,
                t * geom::at(0.0, h / 2.0 + 0.04, 0.145),
                hex(0xb8b7a8),
            );
            // Irregular broken tile edge / mortar exposure, attached to the same plane.
            let patch = ragged_patch((l * 0.25).clamp(0.35, 1.4), 0.34, &mut r);
            out.add(
                Mat::VCol,
                &patch,
                t * geom::at(0.0, 0.64, 0.151),
                hex(0x8f8878),
            );
        }
        let max_w = (l - 0.35).min(1.65);
        let pw = max_w * (0.55 + r.f() * 0.4);
        let ph = 0.5 + r.f() * 0.65;
        let x = (r.f() - 0.5) * (l - pw - 0.2).max(0.0);
        let y = 1.0 + r.f() * 0.6;
        let patch = ragged_patch(pw, ph, &mut r);
        out.add(
            Mat::VCol,
            &patch,
            t * trs([x, y, 0.148], [0.0; 3], [1.13, 1.13, 1.0]),
            hex(0xbdb4a0),
        );
        out.add(Mat::Brick, &patch, t * geom::at(x, y, 0.153), WHITE);
        // Moisture gathers at the wall base in interrupted patches, not a continuous
        // perfectly straight dark border around every building.
        for k in 0..2 {
            let pw = (l * 0.3).clamp(0.3, 1.7);
            let g = ragged_patch(pw, 0.22 + r.f() * 0.16, &mut r);
            let x = (k as f32 - 0.5) * (l - pw) * 0.55;
            out.add(Mat::VCol, &g, t * geom::at(x, 0.18, 0.159), hex(0x626859));
        }
        if index >= 2 || l < 1.5 {
            continue;
        }
        // One utility group on each side of the entry, below the first-floor windows.
        let px = if index == 0 { -1.0 } else { 1.0 } * (l / 2.0 - 0.25);
        let pipe_h = 3.2f32.min(w.y1 - 0.2);
        out.add(
            Mat::VCol,
            &geom::cylinder(0.045, 0.045, pipe_h, 6),
            t * geom::at(px, pipe_h / 2.0 + 0.08, 0.22),
            hex(if index == 0 { 0x737b69 } else { 0x786657 }),
        );
        for y in [0.45, 1.7, 2.85] {
            out.add(
                Mat::VCol,
                &bx(0.14, 0.05, 0.16, px, y, 0.19),
                t,
                hex(0x56584f),
            );
        }
        let mx = (px + if px < 0.0 { 0.45 } else { -0.45 }).clamp(-l / 2.0 + 0.3, l / 2.0 - 0.3);
        out.add(
            Mat::VCol,
            &bx(0.42, 0.6, 0.13, mx, 1.62, 0.22),
            t,
            hex(0x9c9e8e),
        );
        out.add(
            Mat::VCol,
            &bx(0.36, 0.45, 0.015, mx, 1.66, 0.293),
            t,
            hex(0x6b7269),
        );
        out.add(
            Mat::VCol,
            &geom::cylinder(0.105, 0.105, 0.02, 10),
            t * geom::at(mx, 1.73, 0.312) * Mat4::from_rotation_x(FRAC_PI_2),
            hex(0xc0c5b7),
        );
        out.add(
            Mat::VCol,
            &bx(0.17, 0.045, 0.018, mx, 1.71, 0.328),
            t,
            hex(0x303b35),
        );
        out.add(
            Mat::VCol,
            &bx(0.025, 0.7, 0.025, mx, 2.27, 0.17),
            t,
            hex(0x474f47),
        );
        out.add(
            Mat::VCol,
            &bx(l.min(3.5), 0.025, 0.025, 0.0, 2.62, 0.17),
            t,
            hex(0x474f47),
        );
        if front && l > 2.2 {
            let nx = if mx < 0.0 { 0.5 } else { -0.5 };
            let notice = t * geom::at(nx, 1.62, 0.18) * Mat4::from_rotation_z(-0.04 + r.f() * 0.08);
            let paper = ragged_patch(0.5, 0.68, &mut r);
            out.add(Mat::VCol, &paper, notice, hex(0xc9c1a9));
            out.add(
                Mat::VCol,
                &geom::plane(0.27, 0.075),
                notice * geom::at(0.0, 0.16, 0.004),
                hex(0x8e4236),
            );
            for k in 0..4 {
                let line = 0.2 - (k % 2) as f32 * 0.055;
                out.add(
                    Mat::VCol,
                    &geom::plane(line, 0.018),
                    notice * geom::at(-0.02, 0.04 - k as f32 * 0.06, 0.005),
                    hex(0x676250),
                );
            }
        }
    }
}

/// walkup.ts street dressing on the actual street-facing wall, rather than an unrelated
/// longest wall (which could put the original native AC/sign/brick details in mid-air).
pub fn walkup(b: &Bld, out: &mut Parts) {
    let Some(door) = b.door else { return };
    if b.lv > 5 {
        return;
    }
    let mut r = Park(b.seed8 ^ 0x5bd1);
    let front_plane = if b.arcade.is_some() { ARC } else { 0.0 };
    let Some(w) = b
        .walls
        .iter()
        .filter(|w| {
            w.y1 > GF + 1.0
                && wall_normal(b, w).dot(door.n) > 0.95
                && ((((w.a + w.c) / 2.0 - door.p).dot(door.n)) - front_plane).abs() < 0.05
        })
        .max_by(|a, c| a.a.distance(a.c).total_cmp(&c.a.distance(c.c)))
    else {
        return;
    };
    let l = w.a.distance(w.c);
    if l < 3.0 {
        return;
    }
    let exterior = super::exterior::plan(b, super::world());
    let commercial = super::commercial::plan(b, super::world()).is_some();
    let temple = super::commercial::is_named_temple(b);
    let u = (w.c - w.a) / l;
    let n = wall_normal(b, w);
    let ry = n.x.atan2(n.y);
    let lt = |s: f32, y: f32, z: f32| {
        let p = w.a + u * s + n * (0.125 + z);
        place(p.x, y, p.y, ry)
    };
    if b.h > GF + FH && r.f() < 0.55 {
        let idx = (r.f() * VSIGN_N as f32) as usize % VSIGN_N;
        let s = if r.f() < 0.5 { 0.6 } else { l - 0.6 };
        let y = (GF + 1.6).min(b.h - 1.5);
        let transform = lt(s, y, 0.5);
        if !commercial
            && !super::exterior::attachment_conflicts(
                b,
                &exterior,
                transform.transform_point3(Vec3::ZERO),
                Vec3::new(0.05, 1.4, 0.375),
                n,
            )
        {
            // The old random blade identity is replaced by the shop's own atlas
            // label. Its draws above remain, preserving the following AC layout.
            let g = geom::cuboid(0.1, 2.8, 0.75).map_uv(|uv, _| {
                if temple {
                    // The blank background outside the vertical atlas border;
                    // no random restaurant/clinic text on a named temple.
                    [(idx as f32 + 1.5 / 64.0) / VSIGN_N as f32, 1.5 / 256.0]
                } else {
                    [(idx as f32 + uv[0]) / VSIGN_N as f32, 1.0 - uv[1]]
                }
            });
            out.add(
                Mat::VSign,
                &g,
                transform,
                if temple { hex(0x30241b) } else { WHITE },
            );
        }
    }
    let floors = ((b.h - GF) / FH).floor() as i32;
    for f in 0..floors.max(0) {
        if r.f() < 0.45 {
            let s = 0.8 + r.f() * (l - 1.6);
            let y = GF + f as f32 * FH + 1.0;
            let t = lt(s, y, 0.0);
            // Keep the same random draw sequence, but omit a whole compressor group
            // if its body/base would be embedded in a newly admitted balcony.
            if super::exterior::attachment_conflicts(
                b,
                &exterior,
                t.transform_point3(Vec3::new(0.0, 0.0, 0.2)),
                Vec3::new(0.42, 0.32, 0.2),
                n,
            ) {
                continue;
            }
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
    if r.f() < 0.5 && w.y1 - w.y0 > 2.0 {
        let (pw, ph) = (0.8 + r.f() * 0.8, 0.7 + r.f() * 0.7);
        let s = pw / 2.0 + 0.15 + r.f() * (l - pw - 0.3).max(0.0);
        let y = w.y0 + ph / 2.0 + 0.2 + r.f() * (w.y1 - w.y0 - ph - 0.4).max(0.0);
        let g = ragged_patch(pw, ph, &mut r);
        out.add(Mat::Brick, &g, lt(s, y, 0.025), WHITE);
    }
}

/// A narrow bar between two local points, used for sagging cables and awning brackets.
fn frontage_bar(out: &mut Parts, t: Mat4, a: Vec3, b: Vec3, width: f32, col: u32) {
    let delta = b - a;
    let length = delta.length();
    if length <= 0.001 {
        return;
    }
    let transform = Mat4::from_translation((a + b) / 2.0)
        * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, delta / length));
    out.add(
        Mat::VCol,
        &geom::cuboid(width, length, width),
        t * transform,
        hex(col),
    );
}

/// Keep all floor dressing away from every nearby doorway, including doors in an
/// adjacent lot. The larger clear zone also contains the dynamic door-leaf swing.
fn frontage_floor_clear(world: &WorldData, b: &Bld, p: Vec2, normal: Vec2) -> bool {
    if world.road_dist(p) < 0.55 || !world.free(p, 0.19) {
        return false;
    }
    if let Some(other) = world.bld_at(p)
        && (other.id != b.id || b.arcade.is_none())
    {
        return false;
    }
    // Leave a real walking strip outside this wall-hugging detail. Decoration is
    // visual only, so it must not suggest an impassable obstacle in a narrow alley.
    if !world.free(p + normal * 0.8, 0.48) {
        return false;
    }
    for id in world.blds_in(p - Vec2::splat(8.0), p + Vec2::splat(8.0)) {
        let Some(door) = world.blds[id as usize].door else {
            continue;
        };
        let relative = p - door.p;
        if relative.dot(door.tangent()).abs() < 1.8 && (-1.1..1.9).contains(&relative.dot(door.n)) {
            return false;
        }
    }
    true
}

fn frontage_planter(out: &mut Parts, t: Mat4, seed: u64) {
    let mut r = Park(seed.max(1));
    let pot = if r.f() < 0.6 { 0x8c5340 } else { 0x616b59 };
    out.add(
        Mat::VCol,
        &geom::cylinder(0.2, 0.145, 0.32, 8),
        t * geom::at(0.0, 0.16, 0.0),
        hex(pot),
    );
    out.add(
        Mat::VCol,
        &geom::cylinder(0.204, 0.204, 0.045, 8),
        t * geom::at(0.0, 0.31, 0.0),
        hex(0x9e7860),
    );
    out.add(
        Mat::VCol,
        &geom::cylinder(0.181, 0.181, 0.012, 8),
        t * geom::at(0.0, 0.336, 0.0),
        hex(0x3e4331),
    );
    for k in 0..7 {
        let angle = k as f32 * std::f32::consts::TAU / 7.0 + r.f() * 0.3;
        let height = 0.3 + r.f() * 0.3;
        let leaf = geom::plane2(0.11 + r.f() * 0.06, height);
        out.add(
            Mat::VCol,
            &leaf,
            t * geom::at(0.0, 0.35, 0.0)
                * Mat4::from_rotation_y(angle)
                * Mat4::from_rotation_z(0.35 + r.f() * 0.6)
                * geom::at(0.0, height * 0.4, 0.0),
            hex(if k % 3 == 0 { 0x647648 } else { 0x405d3e }),
        );
    }
}

/// A small, legible layer of everyday Wanhua life from alley.ts: shop awnings,
/// bundled service cables and doorway plants. It follows actual solid wall slabs,
/// not a second guessed facade, and uses the existing near-detail material batches.
fn frontage_life(b: &Bld, world: &WorldData, out: &mut Parts) {
    let Some(door) = b.door else { return };
    let commercial = super::commercial::plan(b, world).is_some();
    let mut walls: Vec<_> = b
        .walls
        .iter()
        .filter(|w| {
            w.y0 < 0.01
                && w.y1 >= DOOR_H
                && w.a.distance(w.c) >= 1.8
                && wall_normal(b, w).dot(door.n) > 0.95
        })
        .collect();
    walls.sort_by(|a, c| {
        ((a.a + a.c) / 2.0)
            .distance_squared(door.p)
            .total_cmp(&((c.a + c.c) / 2.0).distance_squared(door.p))
    });
    for (index, wall) in walls.into_iter().take(2).enumerate() {
        let mut r = Park(
            (b.seed8 ^ 0x7019_5b65)
                .wrapping_add(index as u64 * 7919)
                .max(1),
        );
        let width = (wall.a.distance(wall.c) - 0.28).min(5.2);
        let mid = (wall.a + wall.c) / 2.0;
        let normal = wall_normal(b, wall);
        let transform = place(mid.x, 0.0, mid.y, normal.x.atan2(normal.y));
        // Shallow segmented wires follow the upper wall; they never span a street or
        // attach to a different building whose chunk could unload independently.
        let wire_y = (b.h - 0.3).min(3.65);
        for line in 0..2 {
            for segment in 0..5 {
                let point = |s: usize| {
                    let t = s as f32 / 5.0;
                    Vec3::new(
                        (t - 0.5) * width,
                        wire_y + line as f32 * 0.07 - (t * PI).sin() * 0.14,
                        0.185,
                    )
                };
                frontage_bar(
                    out,
                    transform,
                    point(segment),
                    point(segment + 1),
                    0.021,
                    0x393b35,
                );
            }
        }
        for x in [-width * 0.48, width * 0.48] {
            out.add(
                Mat::VCol,
                &bx(0.085, 0.16, 0.045, x, wire_y, 0.175),
                transform,
                hex(0x757b69),
            );
        }

        // Arcades already have a continuous ceiling. Extra awnings belong on exposed
        // shop-house faces, and the drip edge stays outside the carriageway.
        let depth = 0.72 + r.f() * 0.27;
        let edge = mid + normal * (depth + 0.15);
        let other_lot = world.bld_at(edge).is_some_and(|other| other.id != b.id);
        if b.arcade.is_none()
            && width >= 2.2
            && b.h >= 3.5
            && world.road_dist(edge) > 0.15
            && !other_lot
            && (b.kind != "home" || r.f() < 0.42)
        {
            let color = [0x648276, 0x8c5243, 0x777c6e, 0x526f6d][(r.f() * 4.0) as usize % 4];
            // Retain the original conditional and colour draw before suppressing
            // its geometry; the later plants keep exactly their previous RNG.
            if !commercial {
                let awning =
                    geom::cuboid(width, 0.035, depth).map_uv(|_, p| [p[0] * 1.5, p[2] * 1.5]);
                let at = transform
                    * geom::at(0.0, 3.05, 0.16)
                    * Mat4::from_rotation_x(0.16)
                    * geom::at(0.0, 0.0, depth / 2.0);
                out.add(Mat::Tin, &awning, at, hex(color));
                out.add(
                    Mat::VCol,
                    &bx(width, 0.1, 0.04, 0.0, 2.88, depth + 0.15),
                    transform,
                    hex(0x777064),
                );
                for x in [-width * 0.42, width * 0.42] {
                    frontage_bar(
                        out,
                        transform,
                        Vec3::new(x, 2.62, 0.2),
                        Vec3::new(x, 2.93, depth * 0.82),
                        0.035,
                        0x5a5b51,
                    );
                }
            }
        }

        // Wall-hugging pots are deliberately not crates, cabinets or water fixtures:
        // the existing searchable / refillable objects remain visually distinct.
        for side in [-1.0, 1.0] {
            let local_x = side * (width * 0.5 - 0.35);
            let local = transform.transform_point3(Vec3::new(local_x, 0.0, 0.43));
            if r.f() > 0.68 || !frontage_floor_clear(world, b, local.xz(), normal) {
                continue;
            }
            frontage_planter(out, transform * geom::at(local_x, 0.09, 0.43), r.0);
        }
    }
}

/// The source already names these places of worship. Ornament only their
/// existing solid entry flanks; this does not replace the original building
/// with a fabricated landmark or create another shrine/loot container.
pub fn temple_frontage(b: &Bld) -> Parts {
    let mut out = Parts::default();
    if !super::commercial::is_named_temple(b) {
        return out;
    }
    let Some(door) = b.door else { return out };
    let mut walls: Vec<_> = b
        .walls
        .iter()
        .filter(|w| {
            w.y0 < 0.01
                && w.y1 >= 3.0
                && w.a.distance(w.c) >= 0.9
                && wall_normal(b, w).dot(door.n) > 0.95
        })
        .collect();
    walls.sort_by(|a, c| {
        ((a.a + a.c) * 0.5)
            .distance_squared(door.p)
            .total_cmp(&((c.a + c.c) * 0.5).distance_squared(door.p))
    });
    let canopy = super::commercial::plan(b, super::world()).and_then(|plan| plan.canopy);
    for (index, wall) in walls.into_iter().take(2).enumerate() {
        let normal = wall_normal(b, wall);
        let mid = (wall.a + wall.c) * 0.5;
        let t = place(mid.x, 0.0, mid.y, normal.x.atan2(normal.y));
        // Red lacquer and restrained gold trim sit on the original wall plane.
        out.add(
            Mat::VCol,
            &geom::plane(0.23, 1.35),
            t * geom::at(0.0, 1.73, 0.16),
            hex(0x7c2722),
        );
        for x in [-0.095, 0.095] {
            out.add(
                Mat::Metal,
                &geom::plane(0.012, 1.25),
                t * geom::at(x, 1.73, 0.166),
                hex(0xb79649),
            );
        }
        // A wall lantern beneath a deep canopy is hidden by its own soffit in
        // the normal high camera. Hang it at an existing canopy flank, outside
        // the plaque width, while retaining exactly the same bounded assembly.
        let lantern_t = if let Some(canopy) = canopy {
            let side = if (mid - canopy.centre).dot(canopy.u) < 0.0
                || ((mid - canopy.centre).dot(canopy.u).abs() < 0.01 && index == 0)
            {
                -1.0
            } else {
                1.0
            };
            let centre = canopy.centre
                + canopy.u * (side * (canopy.width * 0.5 - 0.28))
                + canopy.n * (canopy.depth + 0.12);
            let anchor = centre - canopy.n * 0.46;
            place(anchor.x, 0.0, anchor.y, canopy.n.x.atan2(canopy.n.y))
        } else {
            t
        };
        let edge = lantern_t.transform_point3(Vec3::new(0.0, 0.0, 0.72)).xz();
        let world = super::world();
        if world.road_dist(edge) <= 0.1 || world.bld_at(edge).is_some_and(|other| other.id != b.id)
        {
            continue;
        }
        out.add(
            Mat::Metal,
            &bx(0.035, 0.035, 0.34, 0.0, 3.02, 0.31),
            lantern_t,
            hex(0x4b463a),
        );
        out.add(
            Mat::Metal,
            &bx(
                0.018,
                if canopy.is_some() { 0.46 } else { 0.18 },
                0.018,
                0.0,
                if canopy.is_some() { 3.08 } else { 2.94 },
                0.46,
            ),
            lantern_t,
            hex(0x716047),
        );
        let globe =
            geom::sphere(0.185, 8, 5).transformed(Mat4::from_scale(Vec3::new(1.0, 1.22, 1.0)));
        out.add(
            Mat::VCol,
            &globe,
            lantern_t * geom::at(0.0, 2.65, 0.46),
            hex(0xb9442b),
        );
        for y in [2.45, 2.85] {
            out.add(
                Mat::Metal,
                &geom::cylinder(0.085, 0.085, 0.035, 8),
                lantern_t * geom::at(0.0, y, 0.46),
                hex(0xb1914c),
            );
        }
        out.add(
            Mat::VCol,
            &bx(0.032, 0.13, 0.032, 0.0, 2.36, 0.46),
            lantern_t,
            hex(0x843321),
        );
    }
    out
}

/// Everything the near layer adds to a building.
pub fn near_detail(b: &Bld, kit: &WindowKit) -> Parts {
    let mut p = Parts::default();
    windows(b, kit, &mut p);
    walkup(b, &mut p);
    frontage_wear(b, &mut p);
    frontage_life(b, super::world(), &mut p);
    p.merge(temple_frontage(b));
    p
}

/// Fine roof fittings, separate so the roof cutaway hides them with the persistent shell.
pub fn near_roof(b: &Bld) -> Parts {
    super::roofs::near_detail(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_temple_accents_keep_existing_door_apertures_clear() {
        let world = super::super::world();
        let mut dressed = 0;
        for b in &world.blds {
            let geometry = temple_frontage(b);
            if geometry.vcount() == 0 {
                continue;
            }
            dressed += 1;
            assert!(geometry.vcount() < 1_000);
            assert!(matches!(b.name.chars().last(), Some('寺' | '宮' | '廟')));
            let door = b.door.unwrap();
            for mesh in geometry.0.values() {
                assert!(mesh.pos.iter().all(|p| p.iter().all(|v| v.is_finite())));
                for p in &mesh.pos {
                    let delta = Vec2::new(p[0], p[2]) - door.p;
                    if p[1] < 2.25 {
                        assert!(delta.dot(door.tangent()).abs() > DOOR_W * 0.5);
                    }
                }
            }
        }
        assert!(dressed >= 3);
    }

    #[test]
    fn every_tin_roof_shape_stays_over_its_walls_in_both_lot_orientations() {
        for (width, depth) in [(3.2, 6.8), (8.0, 4.0), (5.4, 5.4)] {
            for seed in 1..=128 {
                let shell = shack_layer(width, depth, seed * 7919, false, false);
                assert!(
                    shell.vcount() <= 64 && shell.0.len() == 1,
                    "closed shed resident mesh contains fine fittings: {} vertices",
                    shell.vcount()
                );
                let canopy = shack_layer(width, depth, seed * 7919, true, false);
                assert!(canopy.vcount() <= 176);
                let posts = &canopy.0[&Mat::Metal];
                assert_eq!(
                    posts.vcount(),
                    96,
                    "open roof lost its four corner supports"
                );
                for mesh in shell.0.values() {
                    for point in &mesh.pos {
                        let p = Vec3::from(*point);
                        assert!(
                            p.x.abs() <= width * 0.5 + 0.36
                                && p.z.abs() <= depth * 0.5 + 0.36
                                && p.y >= -0.01
                                && p.y <= 5.6,
                            "shed {width}×{depth} seed {seed} roof leaves its walls: {p:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn open_mono_pitch_caps_do_not_mirror_through_the_roof_plane() {
        let mut checked = 0;
        for seed in 1..=128 {
            let canopy = shack_layer(8.0, 4.0, seed * 7919, true, false);
            let tin = &canopy.0[&Mat::Tin];
            let upward: Vec<_> = tin
                .nrm
                .iter()
                .enumerate()
                .filter(|(_, normal)| normal[1] > 0.1)
                .collect();
            let Some(&(first, normal)) = upward.first() else {
                continue;
            };
            let normal = Vec3::from(*normal);
            if !upward
                .iter()
                .all(|(_, n)| normal.dot(Vec3::from(**n)) > 0.9999)
            {
                continue;
            }
            let origin = Vec3::from(tin.pos[first]);
            for point in &tin.pos {
                assert!(normal.dot(Vec3::from(*point) - origin) < 0.002);
            }
            // The four permanent posts must reach the actual sloping roof plane.
            for post in canopy.0[&Mat::Metal].pos.as_chunks::<24>().0 {
                let centre = post.iter().map(|p| Vec3::from(*p)).sum::<Vec3>() / 24.0;
                let top = post.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
                let tip = Vec3::new(centre.x, top, centre.z);
                assert!(normal.dot(tip - origin).abs() < 0.002);
            }
            checked += 1;
        }
        assert!(checked > 10);
    }

    #[test]
    fn deep_window_surround_has_open_pane_and_shared_bounded_geometry() {
        let kit = WindowKit::new();
        assert_eq!(kit.surround.vcount(), 120);
        let (lo, hi) = kit.surround.aabb();
        assert!(hi.z - lo.z >= 0.3, "window surround has no physical depth");
        for triangle in kit.surround.idx.as_chunks::<3>().0 {
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for &i in triangle {
                let point = Vec3::from(kit.surround.pos[i as usize]);
                assert!(point.is_finite());
                lo = lo.min(point);
                hi = hi.max(point);
            }
            assert!(
                !(lo.x < (WW - 0.1) * 0.5
                    && hi.x > -(WW - 0.1) * 0.5
                    && lo.y < (WH - 0.1) * 0.5
                    && hi.y > -(WH - 0.1) * 0.5),
                "masonry occludes the emissive/window opening: {lo:?}..{hi:?}"
            );
        }
        // Check the actual map slots and their rotated placement, not an assumed floor
        // number. New masonry must stay above every ground door / folded stair ladder.
        let world = super::super::world();
        let mut slots = 0;
        for b in &world.blds {
            for w in &b.wins {
                let transform = place(w.p.x, w.p.y, w.p.z, w.ry);
                for point in &kit.surround.pos {
                    let p = transform.transform_point3(Vec3::from(*point));
                    assert!(p.is_finite());
                    assert!(
                        p.y >= super::super::exterior::LOWEST_STRUCTURE_Y,
                        "window masonry crosses the ground corridor at building {}",
                        b.id
                    );
                }
                slots += 1;
            }
        }
        assert!(slots > 1000);
    }

    #[test]
    fn emissive_windows_are_not_hidden_behind_opaque_glazing() {
        let world = super::super::world();
        let mut building = world
            .blds
            .iter()
            .find(|b| !b.wins.is_empty())
            .unwrap()
            .clone();
        building.wins.truncate(1);
        let kit = WindowKit::new();
        let mut occupied = 0;
        for id in 0..64 {
            building.id = id;
            let mut parts = Parts::default();
            windows(&building, &kit, &mut parts);
            if parts.0.contains_key(&Mat::WindowGlow) {
                occupied += 1;
                assert!(
                    !parts.0.contains_key(&Mat::Glass),
                    "opaque glass hides an occupied window"
                );
                assert!(
                    parts.0.contains_key(&Mat::Metal),
                    "occupied window loses sash/cage frame"
                );
            }
        }
        assert!(occupied >= 8);
    }

    #[test]
    fn frontage_life_keeps_doors_and_roadways_clear_with_a_bounded_mesh() {
        let world = super::super::world();
        let mut buildings = 0;
        let mut awnings = 0;
        let mut planted = 0;
        for b in world
            .blds
            .iter()
            .filter(|b| b.c.distance(world.spawn) < 180.0)
        {
            let mut parts = Parts::default();
            frontage_life(b, world, &mut parts);
            if parts.vcount() == 0 {
                continue;
            }
            buildings += 1;
            awnings += usize::from(parts.0.contains_key(&Mat::Tin));
            planted += usize::from(parts.0.values().any(|m| m.pos.iter().any(|p| p[1] < 1.0)));
            assert!(
                parts.vcount() <= 2200,
                "frontage-life budget at {}: {}",
                b.id,
                parts.vcount()
            );
            assert!(parts.0.keys().all(|k| matches!(k, Mat::VCol | Mat::Tin)));
            for mesh in parts.0.values() {
                for p in &mesh.pos {
                    let p = Vec3::from(*p);
                    assert!(p.is_finite());
                    if p.y < 1.0 {
                        assert!(
                            world.road_dist(p.xz()) > -0.05,
                            "planter on the carriageway at {}",
                            b.id
                        );
                    }
                }
            }
            let nearby = world.blds_in(b.min - 3.0, b.max + 3.0);
            for id in nearby {
                let Some(door) = world.blds[id as usize].door else {
                    continue;
                };
                let inv = place(door.p.x, 0.0, door.p.y, door.n.x.atan2(door.n.y)).inverse();
                for mesh in parts.0.values() {
                    let points: Vec<_> = mesh
                        .pos
                        .iter()
                        .map(|p| inv.transform_point3(Vec3::from(*p)))
                        .collect();
                    for tri in mesh.idx.as_chunks::<3>().0 {
                        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                        for &i in tri {
                            lo = lo.min(points[i as usize]);
                            hi = hi.max(points[i as usize]);
                        }
                        let blocks_swing = lo.x < 1.15
                            && hi.x > -1.15
                            && lo.y < 2.4
                            && hi.y > 0.1
                            && lo.z < 1.2
                            && hi.z > -0.45;
                        assert!(
                            !blocks_swing,
                            "frontage {} overlaps doorway {}: {lo:?}..{hi:?}",
                            b.id, id
                        );
                    }
                }
            }
        }
        assert!(
            buildings > 20 && awnings > 5 && planted > 10,
            "insufficient visible frontage variety: {buildings}/{awnings}/{planted}"
        );
    }

    #[test]
    fn frontage_utilities_are_bounded_and_do_not_cover_open_doorways() {
        let world = super::super::world();
        let mut checked = 0;
        for b in world
            .blds
            .iter()
            .filter(|b| b.c.distance(world.spawn) < 95.0)
        {
            let Some(d) = b.door else { continue };
            let mut detail = Parts::default();
            frontage_wear(b, &mut detail);
            assert!(
                detail.vcount() <= 1500,
                "facade budget exceeded at {}",
                b.id
            );
            // Only assert passage at doors which the existing map marks as reachable.
            // This does not change infill/OSM geometry or pretend every original lot opens
            // directly onto the street; it proves decoration adds no new visual barrier.
            if ![-0.6, 0.0, 0.6]
                .into_iter()
                .all(|z| world.free(d.p + d.n * z, 0.45))
            {
                continue;
            }
            let inv = place(d.p.x, 0.0, d.p.y, d.n.x.atan2(d.n.y)).inverse();
            for g in detail.0.values() {
                let points: Vec<Vec3> = g
                    .pos
                    .iter()
                    .map(|p| inv.transform_point3(Vec3::from(*p)))
                    .collect();
                for tri in g.idx.as_chunks::<3>().0 {
                    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                    for &i in tri {
                        assert!(points[i as usize].is_finite());
                        lo = lo.min(points[i as usize]);
                        hi = hi.max(points[i as usize]);
                    }
                    let overlaps = lo.x < 0.55
                        && hi.x > -0.55
                        && lo.y < 2.1
                        && hi.y > 0.2
                        && lo.z < 0.35
                        && hi.z > -0.25;
                    assert!(
                        !overlaps,
                        "facade detail crosses door {}: {lo:?}..{hi:?}",
                        b.id
                    );
                }
            }
            checked += 1;
        }
        assert!(checked > 10);
    }
}
