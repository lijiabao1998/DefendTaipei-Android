//! City textures and shared materials — port of `src/map/textures.ts` (walls: painted render /
//! 二丁掛 tile / red brick / 小口磚 with 鐵窗 windows and grime, asphalt, paved lots), the
//! 騎樓 tiles and roll-up shutters of `arcade.ts`, the corrugated tin of `rooftin.ts`, the roof
//! slabs (`roofTex`) and the brick of `walkup.ts`. Painted once at startup on the CPU canvas.

use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
};

use super::canvas::{Canvas, Rng, css, rgba};

pub const WALL_N: usize = 7;
pub const ROOF_N: usize = 6;
/// Tin palette (rooftin.ts PAL): colour, weight.
pub const TIN_PAL: [(u32, u32); 9] = [
    (0x5f8c84, 30),
    (0x6f9488, 18),
    (0x4f7a78, 10),
    (0x9a4436, 18),
    (0xa85a46, 8),
    (0x8a6a58, 6),
    (0xd8d8d0, 4),
    (0x5a7ea0, 4),
    (0x7a5a40, 2),
];
pub const ARCADE_TILE_N: usize = 3;
/// Shop sign atlas (tools/generate_signs.py): 4 × 5 cells of 256 × 64.
pub const SIGN_COLS: f32 = 4.0;
pub const SIGN_ROWS: f32 = 5.0;
pub const SIGN_PARODY: usize = 10;
pub const VSIGN_N: usize = 8;

/// Atlas index of a typed shop sign (store / pharmacy / police / b2sign kinds).
pub fn sign_index(kind: &str) -> Option<usize> {
    Some(match kind {
        "store" => 10,
        "pharmacy" => 11,
        "police" => 12,
        "grocery" => 13,
        "breakfast" => 14,
        "betel" => 15,
        "market" => 16,
        "hardware" => 17,
        _ => return None,
    })
}

#[derive(Resource, Clone)]
pub struct MapMaterials {
    pub walls: Vec<Handle<StandardMaterial>>,
    /// See-through copies used when a building hides the player (wallMatsT, opacity 0.22).
    pub walls_faded: Vec<Handle<StandardMaterial>>,
    pub roofs: Vec<Handle<StandardMaterial>>,
    pub roofs_faded: Handle<StandardMaterial>,
    pub tin: Handle<StandardMaterial>,
    pub road: Handle<StandardMaterial>,
    pub ground: Handle<StandardMaterial>,
    pub walk: Handle<StandardMaterial>,
    pub curb: Handle<StandardMaterial>,
    pub floor: Handle<StandardMaterial>,
    pub arcade_floor: Vec<Handle<StandardMaterial>>,
    pub column: Handle<StandardMaterial>,
    pub shutter: Handle<StandardMaterial>,
    pub ceiling: Handle<StandardMaterial>,
    pub lamp: Handle<StandardMaterial>,
    /// Vertex-coloured cloth / concrete / painted parts (matC / conc).
    pub vcol: Handle<StandardMaterial>,
    /// Vertex-coloured metal parts (detMat: metalness 0.5, roughness 0.45).
    pub metal: Handle<StandardMaterial>,
    pub glass: Handle<StandardMaterial>,
    pub signs: Handle<StandardMaterial>,
    pub vsigns: Handle<StandardMaterial>,
    pub brick: Handle<StandardMaterial>,
    pub foliage: Handle<StandardMaterial>,
    pub marking: Handle<StandardMaterial>,
}

fn wall_tex(wi: usize) -> Canvas {
    const WALLC: [&str; WALL_N] = [
        "#c9c1b0", "#b8b4a8", "#d8cfbd", "#a9a397", "#c4b49c", "#9fa7a8", "#d1c6b8",
    ];
    let mut r = Rng(0xa11_0000 + wi as u64 * 7919);
    let mut c = Canvas::new(256, 256);
    let tile = wi.is_multiple_of(2);
    c.fill_rect(0.0, 0.0, 256.0, 256.0, css(WALLC[wi]));
    if wi == 3 {
        // old shop-houses in red brick
        c.fill_rect(0.0, 0.0, 256.0, 256.0, css("#9c8f7e"));
        for row in 0..43 {
            for k in -1..14 {
                let v = 128.0 + r.f() * 44.0;
                let x = k as f32 * 19.0 + if row % 2 == 1 { 9.0 } else { 0.0 };
                c.fill_rect(
                    x,
                    row as f32 * 6.0,
                    18.0,
                    5.0,
                    rgba(v, v * 0.44, v * 0.31, 1.0),
                );
            }
        }
    } else if wi == 5 {
        // small white tile (小口磚) with dark grout
        c.fill_rect(0.0, 0.0, 256.0, 256.0, css("#7f7f78"));
        for row in 0..32 {
            for k in 0..32 {
                let v = 212.0 + r.f() * 30.0;
                c.fill_rect(
                    k as f32 * 8.0,
                    row as f32 * 8.0,
                    7.0,
                    7.0,
                    rgba(v, v, v - 6.0, 1.0),
                );
            }
        }
    }
    if tile {
        let ln = rgba(70.0, 60.0, 50.0, 0.35);
        let mut y = 0.0;
        while y < 256.0 {
            c.fill_rect(0.0, y - 0.5, 256.0, 1.0, ln);
            let mut x = if (y / 8.0) as i32 % 2 == 1 { 0.0 } else { 12.0 };
            while x < 256.0 {
                c.fill_rect(x - 0.5, y, 1.0, 8.0, ln);
                x += 24.0;
            }
            y += 8.0;
        }
        for _ in 0..40 {
            let x = (r.f() * 10.0).floor() * 24.0;
            let y = (r.f() * 32.0).floor() * 8.0;
            c.fill_rect(x, y, 24.0, 8.0, rgba(120.0, 120.0, 115.0, 0.9));
        } // missing tiles
    }
    for _ in 0..1400 {
        let w = r.f() < 0.7;
        let a = r.f() * 0.08;
        let col = if w {
            rgba(0.0, 0.0, 0.0, a)
        } else {
            rgba(255.0, 255.0, 255.0, a)
        };
        let (x, y, sw, sh) = (
            r.f() * 256.0,
            r.f() * 256.0,
            1.0 + r.f() * 4.0,
            1.0 + r.f() * 4.0,
        );
        c.fill_rect(x, y, sw, sh, col);
    }
    for _ in 0..18 {
        let (x, y0, w, h) = (
            r.f() * 256.0,
            r.f() * 120.0,
            2.0 + r.f() * 10.0,
            40.0 + r.f() * 180.0,
        );
        c.grad_rect_v(
            x,
            y0,
            w,
            h,
            y0,
            y0 + h,
            rgba(40.0, 36.0, 28.0, 0.38),
            rgba(40.0, 36.0, 28.0, 0.0),
        );
    } // rain streaks
    for _ in 0..6 {
        let (x, y, rr) = (r.f() * 220.0, r.f() * 220.0, 8.0 + r.f() * 26.0);
        let pts: Vec<(f32, f32)> = (0..10)
            .map(|k| {
                let a = k as f32 / 10.0 * std::f32::consts::TAU;
                let q = rr * (0.5 + r.f() * 0.6);
                (x + a.cos() * q, y + a.sin() * q * 0.7)
            })
            .collect();
        c.fill_poly(&pts, css("#8f8a82"));
        let mut ring = pts.clone();
        ring.push(pts[0]);
        c.polyline(&ring, 1.0, rgba(255.0, 255.0, 255.0, 0.25));
    } // peeling plaster
    c.grad_rect_v(
        0.0,
        190.0,
        256.0,
        66.0,
        190.0,
        256.0,
        rgba(30.0, 40.0, 25.0, 0.0),
        rgba(30.0, 40.0, 25.0, 0.55),
    ); // damp at base
    // window with rusty iron bars (鐵窗); some broken
    let broken = wi % 3 == 1;
    c.fill_rect(60.0, 46.0, 136.0, 108.0, css("#1e252c"));
    c.fill_rect(
        66.0,
        52.0,
        124.0,
        96.0,
        css(if broken { "#20262c" } else { "#4e6478" }),
    );
    if broken {
        c.fill_poly(
            &[(80.0, 60.0), (150.0, 70.0), (120.0, 130.0), (90.0, 110.0)],
            css("#0c0e10"),
        );
        for _ in 0..10 {
            let (dx, dy) = ((r.f() - 0.5) * 120.0, (r.f() - 0.5) * 90.0);
            c.line(
                120.0,
                95.0,
                120.0 + dx,
                95.0 + dy,
                1.0,
                rgba(200.0, 220.0, 235.0, 0.7),
            );
        }
        c.fill_rect(150.0, 52.0, 40.0, 96.0, css("#5a6a7a"));
    } else {
        c.fill_rect(70.0, 56.0, 30.0, 88.0, rgba(255.0, 255.0, 255.0, 0.12));
    }
    let rust = css("#5a3018");
    c.stroke_rect(54.0, 40.0, 148.0, 120.0, 5.0, rust);
    let mut x = 66.0;
    while x < 200.0 {
        c.line(x, 40.0, x, 160.0, 3.0, rust);
        x += 14.0;
    }
    c.line(54.0, 100.0, 202.0, 100.0, 3.0, rust);
    // long-term grime: cement-dust streaks under the sill, black stains from the slab edge, rust
    for _ in 0..26 {
        let x = 50.0 + r.f() * 160.0;
        let (w, h) = (1.0 + r.f() * 5.0, 40.0 + r.f() * 98.0);
        c.grad_rect_v(
            x,
            158.0,
            w,
            h,
            158.0,
            256.0,
            rgba(105.0, 100.0, 92.0, 0.55),
            rgba(105.0, 100.0, 92.0, 0.0),
        );
    }
    for _ in 0..14 {
        let x = r.f() * 256.0;
        let (w, x2, y2) = (6.0 + r.f() * 14.0, 3.0 + r.f() * 4.0, 30.0 + r.f() * 60.0);
        // gradient triangle approximated by stacked rows
        for row in 0..(y2 as i32) {
            let t = row as f32 / y2;
            let a = 0.6 * (1.0 - row as f32 / 90.0).max(0.0);
            let l = x + x2 * t;
            let rr = x + w + (x2 - w) * t;
            c.fill_rect(
                l,
                row as f32,
                (rr - l).max(0.0),
                1.0,
                rgba(20.0, 20.0, 18.0, a),
            );
        }
    }
    c.grad_rect_v(
        0.0,
        0.0,
        256.0,
        18.0,
        0.0,
        18.0,
        rgba(25.0, 25.0, 22.0, 0.7),
        rgba(25.0, 25.0, 22.0, 0.0),
    );
    for _ in 0..4 {
        let (x, y) = (r.f() * 230.0, 165.0 + r.f() * 60.0);
        let (rx, ry) = (10.0 + r.f() * 18.0, 6.0 + r.f() * 10.0);
        c.fill_ellipse(x, y, rx, ry, rgba(60.0, 55.0, 50.0, 0.25));
    }
    for _ in 0..9 {
        let x = 56.0 + r.f() * 146.0;
        let (w, h) = (2.0 + r.f() * 3.0, 30.0 + r.f() * 60.0);
        c.grad_rect_v(
            x,
            160.0,
            w,
            h,
            160.0,
            230.0,
            rgba(120.0, 60.0, 20.0, 0.6),
            rgba(120.0, 60.0, 20.0, 0.0),
        );
    } // rust drips
    c
}

fn noise_tex(
    base: &str,
    seed: u64,
    n: usize,
    light: f32,
    patches: impl Fn(&mut Canvas, &mut Rng),
) -> Canvas {
    let mut r = Rng(seed);
    let mut c = Canvas::new(256, 256);
    c.fill_rect(0.0, 0.0, 256.0, 256.0, css(base));
    for _ in 0..n {
        let w = r.f() < 0.5;
        let a = r.f() * light;
        let col = if w {
            rgba(255.0, 255.0, 255.0, a)
        } else {
            rgba(0.0, 0.0, 0.0, a)
        };
        let (x, y) = (r.f() * 256.0, r.f() * 256.0);
        c.fill_rect(x, y, 2.0, 2.0, col);
    }
    patches(&mut c, &mut r);
    c
}

fn cracks(c: &mut Canvas, r: &mut Rng, n: usize, steps: usize, jump: f32, col: [f32; 4]) {
    for _ in 0..n {
        let (mut x, mut y) = (r.f() * 256.0, r.f() * 256.0);
        let mut pts = vec![(x, y)];
        for _ in 0..steps {
            x += (r.f() - 0.5) * jump;
            y += (r.f() - 0.5) * jump;
            pts.push((x, y));
        }
        c.polyline(&pts, 0.8, col);
    }
}

fn road_tex() -> Canvas {
    noise_tex("#3c3e41", 77, 2500, 0.07, |c, r| {
        for _ in 0..5 {
            let col = rgba(
                20.0 + r.f() * 30.0,
                20.0 + r.f() * 30.0,
                25.0 + r.f() * 30.0,
                0.6,
            );
            let (x, y, w, h) = (
                r.f() * 220.0,
                r.f() * 220.0,
                20.0 + r.f() * 60.0,
                15.0 + r.f() * 40.0,
            );
            c.fill_rect(x, y, w, h, col);
        } // asphalt patches
        cracks(c, r, 5, 4, 14.0, rgba(25.0, 25.0, 25.0, 0.22));
        for _ in 0..6 {
            let (x, y) = (r.f() * 256.0, r.f() * 256.0);
            c.radial(
                x,
                y,
                20.0,
                0.6,
                rgba(10.0, 10.0, 12.0, 0.16),
                rgba(10.0, 10.0, 12.0, 0.0),
            );
        } // oil stains
    })
}

/// paving.ts Lehmer RNG (seeded per texture).
struct Lehmer(u64);
impl Lehmer {
    fn f(&mut self) -> f32 {
        self.0 = (self.0 * 16807) % 2147483647;
        self.0 as f32 / 2147483647.0
    }
}

const SIDE_COL: [[f32; 3]; 5] = [
    [150.0, 146.0, 140.0],
    [168.0, 150.0, 128.0],
    [132.0, 128.0, 124.0],
    [176.0, 162.0, 140.0],
    [120.0, 104.0, 92.0],
];
const MORTAR: [f32; 4] = [60.0 / 255.0, 56.0 / 255.0, 50.0 / 255.0, 0.85];

/// paving.ts `rgb(c, j, r)`: the colour with a random ±j/2 brightness.
fn jit(c: [f32; 3], j: f32, r: &mut Lehmer) -> [f32; 4] {
    let k = 1.0 + (r.f() - 0.5) * j;
    rgba(
        (c[0] * k).round().clamp(0.0, 255.0),
        (c[1] * k).round().clamp(0.0, 255.0),
        (c[2] * k).round().clamp(0.0, 255.0),
        1.0,
    )
}

/// grime: dark blotches, gum spots (decay theme).
fn grime(c: &mut Canvas, r: &mut Lehmer, w: f32, h: f32, n: usize) {
    for _ in 0..n {
        let rad = 20.0 + r.f() * 60.0;
        let a = 0.12 + r.f() * 0.2;
        let (x, y, sy) = (r.f() * w, r.f() * h, 0.5 + r.f());
        c.radial(
            x,
            y,
            rad,
            sy,
            rgba(30.0, 28.0, 24.0, a),
            rgba(30.0, 28.0, 24.0, 0.0),
        );
    }
    for _ in 0..n * 3 {
        let light = r.f() < 0.5;
        let a = 0.25 + r.f() * 0.3;
        let col = if light {
            rgba(40.0, 38.0, 34.0, a)
        } else {
            rgba(210.0, 206.0, 196.0, a)
        };
        let (x, y, rr) = (r.f() * w, r.f() * h, 1.0 + r.f() * 2.5);
        c.fill_ellipse(x, y, rr, rr, col);
    }
}

/// Sidewalk bricks (paving.ts `sidewalkTex`): herringbone field of 10×20 cm in a grey / earth mix
/// with a 30×30 border course; 1 texture = 2 m.
fn sidewalk_tex() -> Canvas {
    let mut r = Lehmer(7);
    let mut c = Canvas::new(512, 512);
    c.fill_rect(0.0, 0.0, 512.0, 512.0, MORTAR);
    let u = 25.6;
    let pal = [
        SIDE_COL[0],
        SIDE_COL[1],
        SIDE_COL[2],
        SIDE_COL[0],
        SIDE_COL[3],
    ];
    for y in -2i32..22 {
        for x in -2i32..22 {
            let px = (x * 2 + y.rem_euclid(2)) as f32 * u;
            let py = y as f32 * u;
            let vert = (x + y).rem_euclid(2) == 0;
            let col = jit(
                pal[(r.f() * pal.len() as f32) as usize % pal.len()],
                0.12,
                &mut r,
            );
            if vert {
                c.fill_rect(px + 1.0, py + 1.0, u - 2.0, 2.0 * u - 2.0, col);
            } else {
                c.fill_rect(px + 1.0, py + 1.0, 2.0 * u - 2.0, u - 2.0, col);
            }
        }
    }
    let mut x = 0.0;
    while x < 512.0 {
        c.fill_rect(x, 0.0, u * 3.0, u * 3.0, MORTAR);
        let col = jit([110.0, 108.0, 104.0], 0.1, &mut r);
        c.fill_rect(x + 1.5, 1.5, u * 3.0 - 3.0, u * 3.0 - 3.0, col);
        x += u * 3.0;
    }
    grime(&mut c, &mut r, 512.0, 512.0, 14);
    c
}

/// Paved lots (paving.ts `lotTex`): worn concrete slabs, old 30×30 tiles and brick repairs,
/// 1 texture = 8 m.
fn lot_tex() -> Canvas {
    let mut r = Lehmer(31);
    let mut c = Canvas::new(512, 512);
    c.fill_rect(0.0, 0.0, 512.0, 512.0, css("#8a8680"));
    for _ in 0..1800 {
        let v = 110.0 + r.f() * 70.0;
        let (x, y, w, h) = (
            r.f() * 512.0,
            r.f() * 512.0,
            1.0 + r.f() * 3.0,
            1.0 + r.f() * 3.0,
        );
        c.fill_rect(x, y, w, h, rgba(v, v - 3.0, v - 9.0, 0.25));
    }
    let u = 6.4;
    for _ in 0..2 {
        let w = (2.0 + (r.f() * 6.0).floor()) * u * 3.0;
        let h = (2.0 + (r.f() * 5.0).floor()) * u * 3.0;
        let x0 = (r.f() * 512.0 / (u * 3.0)).floor() * u * 3.0;
        let y0 = (r.f() * 512.0 / (u * 3.0)).floor() * u * 3.0;
        let col = SIDE_COL[(r.f() * SIDE_COL.len() as f32) as usize % SIDE_COL.len()];
        let brick = r.f() < 0.4;
        let (sx, sy) = if brick {
            (u * 2.0, u)
        } else {
            (u * 3.0, u * 3.0)
        };
        let mut y = y0;
        while y < y0 + h {
            let mut x = x0
                - if brick && ((y / u).round() as i32) % 2 == 1 {
                    u
                } else {
                    0.0
                };
            while x < x0 + w {
                let (xx, yy) = (x.rem_euclid(512.0), y.rem_euclid(512.0));
                c.fill_rect(xx, yy, sx, sy, MORTAR);
                let cc = jit(col, 0.14, &mut r);
                c.fill_rect(xx + 1.2, yy + 1.2, sx - 2.4, sy - 2.4, cc);
                x += sx;
            }
            y += sy;
        }
    }
    // slab joints + cracks
    let joint = rgba(40.0, 38.0, 34.0, 0.45);
    let mut x = 0.0;
    while x <= 512.0 {
        c.line(x, 0.0, x, 512.0, 1.2, joint);
        c.line(0.0, x, 512.0, x, 1.2, joint);
        x += 128.0;
    }
    for _ in 0..12 {
        let (mut x, mut y) = (r.f() * 512.0, r.f() * 512.0);
        let mut pts = vec![(x, y)];
        for _ in 0..8 {
            x += (r.f() - 0.5) * 40.0;
            y += (r.f() - 0.5) * 40.0;
            pts.push((x, y));
        }
        c.polyline(&pts, 0.8, joint);
    }
    // moss in joints
    for _ in 0..60 {
        let a = 0.2 + r.f() * 0.3;
        let x = (r.f() * 4.0).floor() * 128.0 - 2.0 + r.f() * 4.0;
        let (y, h) = (r.f() * 512.0, 4.0 + r.f() * 20.0);
        c.fill_rect(x, y, 3.0, h, rgba(70.0, 95.0, 50.0, a));
    }
    grime(&mut c, &mut r, 512.0, 512.0, 22);
    c
}

/// Granite-grey curb stone (paving.ts `curbTex`), speckled, with 1 m joints.
fn curb_tex() -> Canvas {
    let mut r = Lehmer(11);
    let mut c = Canvas::new(128, 32);
    c.fill_rect(0.0, 0.0, 128.0, 32.0, css("#9a988f"));
    for _ in 0..500 {
        let v = 90.0 + r.f() * 90.0;
        let (x, y) = (r.f() * 128.0, r.f() * 32.0);
        c.fill_rect(x, y, 1.0, 1.0, rgba(v, v, v - 4.0, 0.5));
    }
    c.fill_rect(63.0, 0.0, 1.5, 32.0, rgba(50.0, 48.0, 44.0, 0.6));
    c.fill_rect(127.0, 0.0, 1.0, 32.0, rgba(50.0, 48.0, 44.0, 0.6));
    c
}

fn floor_tex() -> Canvas {
    let mut c = Canvas::new(64, 64);
    c.fill_rect(0.0, 0.0, 64.0, 64.0, css("#b9ad98"));
    let s = css("#8f8572");
    for (x, y) in [(0.0, 0.0), (32.0, 32.0), (0.0, 32.0), (32.0, 0.0)] {
        c.stroke_rect(x, y, 32.0, 32.0, 1.0, s);
    }
    c
}

/// 騎樓 mosaic / ceramic tiles (arcade.ts `tileTex`).
fn tile_tex(base: &str, line: &str, n: usize, seed: u64) -> Canvas {
    let mut r = Rng(seed);
    let mut c = Canvas::new(128, 128);
    c.fill_rect(0.0, 0.0, 128.0, 128.0, css(base));
    let cell = 128.0 / n as f32;
    for _ in 0..200 {
        let (x, y) = (r.f() * 128.0, r.f() * 128.0);
        let a = r.f() * 0.08;
        c.fill_rect(x, y, cell, cell, rgba(0.0, 0.0, 0.0, a));
    }
    for i in 0..=n {
        let p = i as f32 * cell;
        c.fill_rect(p - 1.0, 0.0, 2.0, 128.0, css(line));
        c.fill_rect(0.0, p - 1.0, 128.0, 2.0, css(line));
    }
    c
}

fn shutter_tex() -> Canvas {
    let mut r = Rng(5150);
    let mut c = Canvas::new(64, 64);
    c.fill_rect(0.0, 0.0, 64.0, 64.0, css("#9a9c9e"));
    let mut y = 0.0;
    while y < 64.0 {
        c.fill_rect(0.0, y, 64.0, 1.0, css("#6e7072"));
        c.fill_rect(0.0, y + 1.0, 64.0, 1.0, css("#b4b6b8"));
        y += 4.0;
    }
    for _ in 0..30 {
        let a = r.f() * 0.25;
        let (x, yy, w, h) = (
            r.f() * 64.0,
            r.f() * 64.0,
            2.0 + r.f() * 8.0,
            1.0 + r.f() * 6.0,
        );
        c.fill_rect(x, yy, w, h, rgba(90.0, 60.0, 40.0, a));
    }
    c
}

/// Corrugated tin (rooftin.ts `mkTex`), painted white so the per-shack palette colour comes
/// from the vertex colour: rib shading, laps, rust streaks and spots, sun fade.
fn tin_tex() -> Canvas {
    let mut r = Rng(1234);
    let mut c = Canvas::new(128, 128);
    c.fill_rect(0.0, 0.0, 128.0, 128.0, css("#e8e8e8"));
    let mut x = 0.0;
    while x < 128.0 {
        c.fill_rect(x, 0.0, 3.0, 128.0, rgba(255.0, 255.0, 255.0, 0.2));
        c.fill_rect(x + 6.0, 0.0, 3.0, 128.0, rgba(0.0, 0.0, 0.0, 0.18));
        x += 128.0 / 10.0;
    } // rib shading
    for y in [0.0, 64.0] {
        c.fill_rect(0.0, y, 128.0, 1.0, rgba(0.0, 0.0, 0.0, 0.18));
    } // panel laps
    for _ in 0..9 {
        let (x, w, l) = (r.f() * 128.0, 2.0 + r.f() * 6.0, 30.0 + r.f() * 98.0);
        c.grad_rect_v(
            x,
            0.0,
            w,
            l,
            0.0,
            l,
            rgba(110.0, 52.0, 22.0, 0.75),
            rgba(110.0, 52.0, 22.0, 0.0),
        );
    } // rust streaks
    for _ in 0..14 {
        let col = rgba(
            90.0 + r.f() * 40.0,
            40.0 + r.f() * 20.0,
            20.0,
            0.25 + r.f() * 0.4,
        );
        let (x, y, rr) = (r.f() * 128.0, r.f() * 128.0, 1.0 + r.f() * 4.0);
        c.fill_ellipse(x, y, rr, rr, col);
    }
    c.grad_rect_v(
        0.0,
        0.0,
        128.0,
        128.0,
        0.0,
        128.0,
        rgba(240.0, 240.0, 230.0, 0.18),
        rgba(0.0, 0.0, 0.0, 0.1),
    );
    c
}

/// Roof slabs (rooftin.ts `roofTex`): faded paint over concrete, bitumen, stains, moss, cracks.
fn roof_tex(kind: usize, seed: u64) -> Canvas {
    let mut r = Rng(seed);
    let base = [
        ("#6f8f6a", "#5e8060"),
        ("#8a8e8a", "#7c807e"),
        ("#9a968c", "#86827a"),
        ("#5a5a58", "#4a4a48"),
        ("#a2584a", "#8e4c40"),
        ("#7a9a7a", "#91a88e"),
    ][kind];
    let mut c = Canvas::new(256, 256);
    c.fill_rect(0.0, 0.0, 256.0, 256.0, css(base.0));
    for _ in 0..40 {
        let mut col = if r.f() < 0.5 {
            css(base.1)
        } else {
            rgba(150.0, 150.0, 140.0, 0.35)
        };
        col[3] *= 0.25 + r.f() * 0.4;
        let (x, y, rx, ry) = (
            r.f() * 256.0,
            r.f() * 256.0,
            10.0 + r.f() * 50.0,
            6.0 + r.f() * 30.0,
        );
        c.fill_ellipse(x, y, rx, ry, col);
    }
    if kind == 4 {
        let l = rgba(50.0, 20.0, 10.0, 0.5);
        for y in (0..256).step_by(16) {
            c.fill_rect(0.0, y as f32, 256.0, 1.0, l);
            let mut x = ((y / 16) % 2 * 12) as f32;
            while x < 256.0 {
                c.fill_rect(x, y as f32, 1.0, 16.0, l);
                x += 24.0;
            }
        }
    }
    for _ in 0..2 + kind % 3 {
        let col = rgba(28.0, 28.0, 26.0, 0.35 + r.f() * 0.25);
        let (x, y, w, h) = (
            r.f() * 256.0,
            r.f() * 256.0,
            20.0 + r.f() * 36.0,
            12.0 + r.f() * 20.0,
        );
        c.fill_rect(x - w / 2.0, y - h / 2.0, w, h, col);
    } // bitumen patches
    for _ in 0..6 {
        let (x, y, rr, sy) = (
            r.f() * 256.0,
            r.f() * 256.0,
            30.0 + r.f() * 30.0,
            0.6 + r.f(),
        );
        c.radial(
            x,
            y,
            rr,
            sy,
            rgba(40.0, 40.0, 30.0, 0.35),
            rgba(40.0, 40.0, 30.0, 0.0),
        );
    } // puddle stains
    for _ in 0..10 {
        let col = rgba(
            50.0 + r.f() * 30.0,
            80.0 + r.f() * 40.0,
            40.0,
            0.3 + r.f() * 0.3,
        );
        let x = r.f() * 256.0;
        let y = if r.f() < 0.6 {
            if r.f() < 0.5 {
                r.f() * 14.0
            } else {
                256.0 - r.f() * 14.0
            }
        } else {
            r.f() * 256.0
        };
        let rr = 2.0 + r.f() * 8.0;
        c.fill_ellipse(x, y, rr, rr, col);
    } // moss
    for _ in 0..4 {
        let (x, w) = (r.f() * 256.0, 4.0 + r.f() * 6.0);
        c.grad_rect_v(
            x,
            0.0,
            w,
            90.0,
            0.0,
            90.0,
            rgba(15.0, 15.0, 15.0, 0.6),
            rgba(15.0, 15.0, 15.0, 0.0),
        );
    }
    cracks(&mut c, &mut r, 7, 8, 22.0, rgba(30.0, 30.0, 30.0, 0.55));
    c
}

fn brick_tex() -> Canvas {
    let mut r = Rng(4242);
    let mut c = Canvas::new(128, 128);
    c.fill_rect(0.0, 0.0, 128.0, 128.0, css("#7a5a4a"));
    for y in (0..128).step_by(16) {
        let mut x = if (y / 16) % 2 == 1 { -16 } else { 0 };
        while x < 128 {
            let v = 120.0 + r.f() * 50.0;
            c.fill_rect(
                x as f32 + 1.0,
                y as f32 + 1.0,
                30.0,
                14.0,
                rgba(v + 30.0, v * 0.45, v * 0.32, 1.0),
            );
            x += 32;
        }
    }
    for _ in 0..60 {
        let (x, y, w) = (
            r.f() * 128.0,
            if r.f() < 0.5 { 0.0 } else { 120.0 },
            6.0 + r.f() * 14.0,
        );
        c.fill_rect(x, y, w, 8.0, rgba(210.0, 205.0, 192.0, 0.85));
    }
    c
}

fn embedded(bytes: &[u8]) -> Image {
    let mut img = Image::from_buffer(
        bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .expect("embedded sign atlas");
    // rebuild with mips (signs are seen small from the iso camera)
    let w = img.width() as usize;
    let h = img.height() as usize;
    if let Some(data) = img.data.take() {
        let rgba8: Vec<u8> = if data.len() == w * h * 4 {
            data
        } else {
            data.chunks(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect()
        };
        img = super::canvas::image_with_mips(w, h, &rgba8, true, false);
    }
    img
}

pub fn build_materials(
    images: &mut Assets<Image>,
    mats: &mut Assets<StandardMaterial>,
) -> MapMaterials {
    let mut tex = |c: Canvas| images.add(c.to_image(true, true));
    let walls_t: Vec<Handle<Image>> = (0..WALL_N).map(|i| tex(wall_tex(i))).collect();
    let roof_kinds = [0, 5, 1, 2, 3, 4];
    let roofs_t: Vec<Handle<Image>> = roof_kinds
        .iter()
        .enumerate()
        .map(|(i, &k)| tex(roof_tex(k, 999 + i as u64 * 131)))
        .collect();
    let road_t = tex(road_tex());
    let lot_t = tex(lot_tex());
    let walk_t = tex(sidewalk_tex());
    let curb_t = tex(curb_tex());
    let floor_t = tex(floor_tex());
    let arc_t: Vec<Handle<Image>> = [
        ("#b8a890", "#7a6a58", 8, 31),
        ("#c8bca8", "#8a7e6a", 6, 32),
        ("#9a8a7a", "#6a5a4a", 10, 33),
    ]
    .iter()
    .map(|&(b, l, n, s)| tex(tile_tex(b, l, n, s)))
    .collect();
    let col_t = tex(tile_tex("#c9b9a0", "#8a7a64", 8, 34));
    let shut_t = tex(shutter_tex());
    let tin_t = tex(tin_tex());
    let brick_t = tex(brick_tex());
    let signs_t = images.add(embedded(include_bytes!(
        "../../../assets/textures/signs.png"
    )));
    let vsigns_t = images.add(embedded(include_bytes!(
        "../../../assets/textures/vsigns.png"
    )));

    let std = |t: Option<Handle<Image>>, rough: f32| StandardMaterial {
        base_color_texture: t,
        perceptual_roughness: rough,
        ..default()
    };
    MapMaterials {
        walls: walls_t
            .iter()
            .map(|t| mats.add(std(Some(t.clone()), 0.95)))
            .collect(),
        walls_faded: walls_t
            .iter()
            .map(|t| {
                mats.add(StandardMaterial {
                    base_color: Color::srgba(1.0, 1.0, 1.0, 0.22),
                    alpha_mode: AlphaMode::Blend,
                    ..std(Some(t.clone()), 0.9)
                })
            })
            .collect(),
        roofs: roofs_t
            .iter()
            .map(|t| {
                mats.add(StandardMaterial {
                    double_sided: true,
                    cull_mode: None,
                    ..std(Some(t.clone()), 0.95)
                })
            })
            .collect(),
        roofs_faded: mats.add(StandardMaterial {
            base_color: Color::srgba(0.6, 0.6, 0.58, 0.18),
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        tin: mats.add(StandardMaterial {
            metallic: 0.55,
            perceptual_roughness: 0.42,
            double_sided: true,
            cull_mode: None,
            ..std(Some(tin_t), 0.42)
        }),
        road: mats.add(std(Some(road_t), 0.95)),
        ground: mats.add(std(Some(lot_t), 1.0)),
        walk: mats.add(std(Some(walk_t), 0.9)),
        curb: mats.add(std(Some(curb_t), 0.85)),
        floor: mats.add(StandardMaterial {
            double_sided: true,
            cull_mode: None,
            ..std(Some(floor_t), 0.9)
        }),
        arcade_floor: arc_t
            .into_iter()
            .map(|t| mats.add(std(Some(t), 0.55)))
            .collect(),
        column: mats.add(std(Some(col_t), 0.45)),
        shutter: mats.add(StandardMaterial {
            metallic: 0.4,
            ..std(Some(shut_t), 0.6)
        }),
        ceiling: mats.add(StandardMaterial {
            base_color: Color::srgb_u8(0xcf, 0xca, 0xc0),
            perceptual_roughness: 0.9,
            ..default()
        }),
        lamp: mats.add(StandardMaterial {
            base_color: Color::WHITE,
            emissive: LinearRgba::rgb(1.2, 1.1, 0.95),
            ..default()
        }),
        vcol: mats.add(std(None, 0.88)),
        metal: mats.add(StandardMaterial {
            metallic: 0.5,
            ..std(None, 0.45)
        }),
        glass: mats.add(StandardMaterial {
            base_color: Color::srgb_u8(0x9f, 0xb4, 0xc0),
            metallic: 0.6,
            perceptual_roughness: 0.12,
            reflectance: 0.8,
            ..default()
        }),
        signs: mats.add(StandardMaterial {
            base_color_texture: Some(signs_t.clone()),
            emissive_texture: Some(signs_t),
            emissive: LinearRgba::rgb(0.25, 0.25, 0.25),
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        vsigns: mats.add(StandardMaterial {
            base_color_texture: Some(vsigns_t.clone()),
            emissive_texture: Some(vsigns_t),
            emissive: LinearRgba::rgb(0.13, 0.13, 0.13),
            perceptual_roughness: 0.6,
            ..default()
        }),
        brick: mats.add(StandardMaterial {
            depth_bias: 2.0,
            ..std(Some(brick_t), 0.95)
        }),
        foliage: mats.add(StandardMaterial {
            perceptual_roughness: 0.9,
            ..default()
        }),
        marking: mats.add(StandardMaterial {
            perceptual_roughness: 0.8,
            depth_bias: 4.0,
            ..default()
        }),
    }
}
