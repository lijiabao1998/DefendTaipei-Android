//! blocks.ts `blockInfill` (v24.6): the block interiors of the core built solid — rows of
//! 4–7 m 透天 / 公寓 behind the street fronts, back to back across 1.6 m 防火巷, cut every
//! 26–40 m by a 3 m 巷 running in from the street. Rasterised at 1 m; deterministic (own hash).

use bevy::prelude::*;

use super::data::{Road, pip, road_w, seg_dist};

fn hash(a: f32, b: f32) -> f32 {
    let v = (a * 12.9898 + b * 78.233).sin() * 43_758.547;
    v - v.floor()
}

struct Raster {
    x0: f32,
    z0: f32,
    w: usize,
    h: usize,
}

impl Raster {
    fn cell(&self, x: f32, z: f32) -> Option<usize> {
        let i = (x - self.x0).floor();
        let k = (z - self.z0).floor();
        (i >= 0.0 && k >= 0.0 && (i as usize) < self.w && (k as usize) < self.h)
            .then(|| k as usize * self.w + i as usize)
    }
    fn range(&self, lo: f32, hi: f32, o: f32, n: usize) -> (usize, usize) {
        (
            ((lo - o).floor().max(0.0)) as usize,
            ((hi - o).ceil().min(n as f32 - 1.0)).max(0.0) as usize,
        )
    }
    fn capsule(&self, m: &mut [u8], a: Vec2, b: Vec2, r: f32) {
        let (i0, i1) = self.range(a.x.min(b.x) - r, a.x.max(b.x) + r, self.x0, self.w);
        let (k0, k1) = self.range(a.y.min(b.y) - r, a.y.max(b.y) + r, self.z0, self.h);
        for k in k0..=k1 {
            for i in i0..=i1 {
                let p = Vec2::new(self.x0 + i as f32 + 0.5, self.z0 + k as f32 + 0.5);
                if seg_dist(p, a, b) <= r {
                    m[k * self.w + i] = 1;
                }
            }
        }
    }
    fn poly(&self, m: &mut [u8], p: &[Vec2]) {
        let (lo, hi) = bbox(p);
        let (i0, i1) = self.range(lo.x, hi.x, self.x0, self.w);
        let (k0, k1) = self.range(lo.y, hi.y, self.z0, self.h);
        for k in k0..=k1 {
            for i in i0..=i1 {
                if pip(
                    Vec2::new(self.x0 + i as f32 + 0.5, self.z0 + k as f32 + 0.5),
                    p,
                ) {
                    m[k * self.w + i] = 1;
                }
            }
        }
    }
}

fn bbox(p: &[Vec2]) -> (Vec2, Vec2) {
    p.iter()
        .fold((Vec2::splat(1e9), Vec2::splat(-1e9)), |(a, b), q| {
            (a.min(*q), b.max(*q))
        })
}

/// Rectangle (centre, along-road axis u, half sizes), grown by g on every side.
fn rect_q(c: Vec2, u: Vec2, hw: f32, hd: f32, g: f32) -> Vec<Vec2> {
    let n = Vec2::new(-u.y, u.x);
    let (a, d) = (hw + g, hd + g);
    vec![
        c - u * a - n * d,
        c + u * a - n * d,
        c + u * a + n * d,
        c - u * a + n * d,
    ]
}

fn half_w(r: &Road) -> f32 {
    road_w(r.t.trim_end_matches("_link")).unwrap_or(2.5) / 2.0
}

/// Lots for the empty block interiors of `bbox` = [x0, z0, x1, z1]: (footprint, floors).
pub fn block_infill(
    buildings: &[(Vec<Vec2>, bool)],
    roads: &[Road],
    bb: [f32; 4],
    max: usize,
) -> Vec<(Vec<Vec2>, u32)> {
    let ras = Raster {
        x0: bb[0],
        z0: bb[1],
        w: (bb[2] - bb[0]).ceil() as usize,
        h: (bb[3] - bb[1]).ceil() as usize,
    };
    let n = ras.w * ras.h;
    let (mut road, mut band, mut obs) = (vec![0u8; n], vec![0u8; n], vec![0u8; n]);
    for r in roads {
        let hw = half_w(r);
        for s in r.p.windows(2) {
            ras.capsule(&mut road, s[0], s[1], hw + 1.2);
            ras.capsule(&mut band, s[0], s[1], hw + 3.0);
        }
    }
    // existing footprints grown by the 1.6 m 防火巷, and the way out of every door (4 m × 12 m)
    for (p, fill) in buildings {
        if p.len() < 3 {
            continue;
        }
        let c = p.iter().copied().sum::<Vec2>() / p.len() as f32;
        let grown: Vec<Vec2> = p
            .iter()
            .map(|q| *q + (*q - c).normalize_or(Vec2::X) * 1.6)
            .collect();
        ras.poly(&mut obs, &grown);
        let (mut di, mut dl) = (0, 0.0);
        if *fill {
            dl = p[0].distance(p[1]);
        } else {
            for i in 0..p.len() {
                let l = p[i].distance(p[(i + 1) % p.len()]);
                if l > dl {
                    dl = l;
                    di = i;
                }
            }
        }
        let (a, cc) = (p[di], p[(di + 1) % p.len()]);
        let u = (cc - a) / if dl > 0.0 { dl } else { 1.0 };
        let m = (a + cc) / 2.0;
        let mut nn = Vec2::new(-u.y, u.x);
        if pip(m + nn * 0.5, p) {
            nn = -nn;
        }
        ras.poly(
            &mut obs,
            &[
                m - u * 2.0 - nn * 0.5,
                m + u * 2.0 - nn * 0.5,
                m + u * 2.0 + nn * 12.0,
                m - u * 2.0 + nn * 12.0,
            ],
        );
    }
    // lanes: a 3 m 巷 every 26–40 m, 55 m in from each side of every street with a kerb
    for r in roads {
        let w = half_w(r) * 2.0;
        if w < 6.0 {
            continue;
        }
        for s in r.p.windows(2) {
            let (a, b) = (s[0], s[1]);
            let l = a.distance(b);
            if l < 20.0 {
                continue;
            }
            let t = (b - a) / l;
            let mut d = 10.0 + hash(a.x, a.y) * 16.0;
            while d < l - 8.0 {
                for side in [-1.0f32, 1.0] {
                    let nn = Vec2::new(-t.y, t.x) * side;
                    let p0 = a + t * d + nn * (w / 2.0);
                    ras.poly(
                        &mut obs,
                        &[
                            p0 - t * 1.5,
                            p0 + t * 1.5,
                            p0 + t * 1.5 + nn * 55.0,
                            p0 - t * 1.5 + nn * 55.0,
                        ],
                    );
                }
                d += 26.0 + hash(a.x + d, a.y - d) * 14.0;
            }
        }
    }
    // candidates: march in from every road side, row by row
    struct Cand {
        p: Vec2,
        d: f32,
        u: Vec2,
        n: Vec2,
    }
    let depth = 75.0;
    let mut cand: Vec<Cand> = Vec::new();
    for r in roads {
        let hw = half_w(r);
        for s in r.p.windows(2) {
            let (a, b) = (s[0], s[1]);
            let l = a.distance(b);
            if l < 4.0 {
                continue;
            }
            let t = (b - a) / l;
            let mut d = 2.0 + hash(a.x * 3.0, b.y) * 3.0;
            while d < l - 2.0 {
                for side in [-1.0f32, 1.0] {
                    let mut dd = 9.0;
                    while dd < depth {
                        let nn = Vec2::new(-t.y * side, t.x * side);
                        let p = a + t * d + nn * (hw + dd);
                        if let Some(c) = ras.cell(p.x, p.y)
                            && band[c] == 0
                            && obs[c] == 0
                        {
                            cand.push(Cand {
                                p,
                                d: dd,
                                u: t,
                                n: nn,
                            });
                        }
                        dd += 4.6;
                    }
                }
                d += 4.6;
            }
        }
    }
    cand.sort_by(|a, b| a.d.total_cmp(&b.d));
    let clear = |obs: &[u8], q: &[Vec2]| -> bool {
        let (lo, hi) = bbox(q);
        if lo.x < bb[0] || lo.y < bb[1] || hi.x > bb[2] || hi.y > bb[3] {
            return false;
        }
        let i0 = (lo.x - bb[0]).floor() as usize;
        let i1 = ((hi.x - bb[0]).ceil() as usize).saturating_sub(1);
        let k0 = (lo.y - bb[1]).floor() as usize;
        let k1 = ((hi.y - bb[1]).ceil() as usize).saturating_sub(1);
        for k in k0..=k1.min(ras.h - 1) {
            for i in i0..=i1.min(ras.w - 1) {
                let j = k * ras.w + i;
                if (obs[j] != 0 || road[j] != 0)
                    && pip(Vec2::new(bb[0] + i as f32 + 0.5, bb[1] + k as f32 + 0.5), q)
                {
                    return false;
                }
            }
        }
        true
    };
    let mut out = Vec::new();
    for c in cand {
        if out.len() >= max {
            break;
        }
        if ras.cell(c.p.x, c.p.y).is_some_and(|j| obs[j] != 0) {
            continue;
        }
        if hash(c.p.x * 0.7, c.p.y * 1.3) < 0.08 {
            continue; // the odd courtyard / car park left open
        }
        // floors first, then a footprint that can carry them
        let tall = hash(c.p.x * 3.0, c.p.y * 5.0);
        let want = if tall < 0.09 {
            7 + (hash(c.p.y * 7.0, c.p.x) * 6.0).floor() as u32
        } else if tall < 0.26 {
            5 + (hash(c.p.x * 5.0, c.p.y * 9.0) * 2.0).floor() as u32
        } else {
            3 + (hash(c.p.x * 11.0, c.p.y * 13.0) * 2.0).floor() as u32
        };
        let (hw0, hd0) = (hash(c.p.x, c.p.y * 2.0), hash(c.p.y, c.p.x * 2.0));
        let w0 = if want >= 7 {
            15.0 + hw0 * 9.0
        } else if want >= 5 {
            10.0 + hw0 * 5.0
        } else {
            7.0 + hw0 * 3.0
        };
        let d0 = if want >= 7 {
            15.0 + hd0 * 7.0
        } else if want >= 5 {
            12.0 + hd0 * 6.0
        } else {
            11.0 + hd0 * 5.0
        };
        for k in [1.0f32, 0.85, 0.7] {
            let w = (w0 * k).max(5.0);
            let d = (d0 * k).max(9.0);
            let push = (d / 2.0 - 7.0).max(0.0);
            let p = c.p + c.n * push;
            let q = rect_q(p, c.u, w / 2.0, d / 2.0, 0.0);
            if !clear(&obs, &q) {
                continue;
            }
            ras.poly(&mut obs, &rect_q(p, c.u, w / 2.0, d / 2.0, 1.6));
            let l = (((w.min(d) / 1.7).floor() as u32) + 1).clamp(2, want.max(2));
            out.push((q, l));
            break;
        }
    }
    out
}
