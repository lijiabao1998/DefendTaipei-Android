//! Fortified positions that are part of the street (Web `main.ts` `remnantSites`,
//! `sandbags`): the four corners the v24.3 ROC remnants hold, their sandbag walls
//! and flag. Planned with the world so the sandbags are ordinary low cover for
//! movement, sight and shots, exactly as `addSeg(…, true)` makes them in the Web.

use bevy::math::Vec2;

use super::data::{Seg, WorldData};

/// One remnant position: the point on the carriageway and the direction the
/// squad faces (the road's normal).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RemnantSite {
    pub p: Vec2,
    pub ang: f32,
}

/// A sandbag wall: centre, direction it runs along, and bag count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SandbagWall {
    pub c: Vec2,
    pub ang: f32,
    pub n: u32,
}

/// Height of the sandbag cover (`low` segments are 1 m).
pub const SANDBAG_H: f32 = 1.0;
/// One bag (`sandGeo`): 0.6 × 0.3 × 0.45 m.
pub const BAG: [f32; 3] = [0.6, 0.3, 0.45];

const ROAD_TYPES: [&str; 4] = ["tertiary", "secondary", "residential", "unclassified"];

/// `remnantSites`: four street corners 110–420 m from `c`, more than 160 m
/// apart, one per quarter of the compass, on the open carriageway of a mid-size road.
pub fn remnant_sites(w: &WorldData, c: Vec2) -> Vec<RemnantSite> {
    let mut cand: Vec<(RemnantSite, f32)> = Vec::new();
    for r in &w.roads {
        if !ROAD_TYPES.contains(&r.t.as_str()) || r.p.len() < 2 {
            continue;
        }
        for seg in r.p.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            if a.distance(b) < 14.0 {
                continue;
            }
            let p = (a + b) * 0.5;
            let d = p.distance(c);
            if !(110.0..=420.0).contains(&d) || w.bld_at(p).is_some() {
                continue;
            }
            let ang = (b.y - a.y).atan2(b.x - a.x) + std::f32::consts::FRAC_PI_2;
            cand.push((RemnantSite { p, ang }, d));
        }
    }
    // deterministic: spread by bearing, nearest first within each quarter
    cand.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut out: Vec<RemnantSite> = Vec::new();
    for qd in 0..4 {
        let pick = cand.iter().find(|(q, _)| {
            let bearing = (q.p.y - c.y).atan2(q.p.x - c.x);
            let quarter = (((bearing + std::f32::consts::PI) / std::f32::consts::TAU) * 4.0).floor()
                as i32
                % 4;
            quarter == qd && out.iter().all(|o| o.p.distance(q.p) > 160.0)
        });
        if let Some((site, _)) = pick {
            out.push(*site);
        }
    }
    out
}

impl RemnantSite {
    fn u(&self) -> Vec2 {
        Vec2::from_angle(self.ang)
    }

    /// The front wall facing the street and the short flank wall.
    pub fn walls(&self) -> [SandbagWall; 2] {
        let u = self.u();
        [
            SandbagWall {
                c: self.p + u * 1.6,
                ang: self.ang + std::f32::consts::FRAC_PI_2,
                n: 5,
            },
            SandbagWall {
                c: Vec2::new(
                    self.p.x - u.y * 2.2 + u.x * 0.4,
                    self.p.y + u.x * 2.2 + u.y * 0.4,
                ),
                ang: self.ang,
                n: 3,
            },
        ]
    }

    /// The flag behind them.
    pub fn flag(&self) -> Vec2 {
        let u = self.u();
        Vec2::new(
            self.p.x - u.x * 1.4 + u.y * 1.2,
            self.p.y - u.y * 1.4 - u.x * 1.2,
        )
    }

    /// Where man `k` (0..4) stands, in a line behind the front wall.
    pub fn man(&self, k: usize) -> Vec2 {
        let u = self.u();
        let off = (k as f32 - 1.5) * 0.95;
        Vec2::new(
            self.p.x - u.y * off + u.x * 0.4,
            self.p.y + u.x * off + u.y * 0.4,
        )
    }

    /// The crate of gear a fallen position leaves (one metre behind it).
    pub fn gear(&self) -> Vec2 {
        self.p - self.u()
    }
}

impl SandbagWall {
    /// The wall as one low cover segment.
    pub fn seg(&self) -> Seg {
        let u = Vec2::from_angle(self.ang);
        let hl = self.n as f32 * 0.62 / 2.0;
        Seg {
            a: self.c - u * hl,
            b: self.c + u * hl,
            h: Some(SANDBAG_H),
            bld: None,
        }
    }

    /// Bag centres (x, y, z) and the yaw for each bag: three staggered rows.
    pub fn bags(&self) -> Vec<([f32; 3], f32)> {
        let u = Vec2::from_angle(self.ang);
        let n = self.n as i32;
        let mut out = Vec::new();
        for row in 0..3 {
            let count = n - if row == 2 { 1 } else { 0 };
            for k in 0..count {
                let o = (k as f32 - (n - 1) as f32 / 2.0 + (row % 2) as f32 * 0.5) * 0.62;
                let p = self.c + u * o;
                out.push(([p.x, 0.17 + row as f32 * 0.3, p.y], -self.ang));
            }
        }
        out
    }
}

/// A loose row of sandbags (`decay.ts` `sandbagRow`): centre, direction, length, rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SandbagRow {
    pub c: Vec2,
    pub ang: f32,
    pub len: f32,
    pub rows: u32,
}

impl SandbagRow {
    pub fn seg(&self) -> Seg {
        let u = Vec2::from_angle(self.ang);
        Seg {
            a: self.c - u * self.len / 2.0,
            b: self.c + u * self.len / 2.0,
            h: Some(SANDBAG_H),
            bld: None,
        }
    }

    /// Bag centres (x, y, z) and yaw: every 0.5 m, alternate rows offset 0.25 m.
    pub fn bags(&self) -> Vec<([f32; 3], f32)> {
        let u = Vec2::from_angle(self.ang);
        let mut out = Vec::new();
        for r in 0..self.rows {
            let mut d = -self.len / 2.0 + (r % 2) as f32 * 0.25;
            while d < self.len / 2.0 {
                let p = self.c + u * d;
                out.push(([p.x, 0.1 + r as f32 * 0.2, p.y], -self.ang));
                d += 0.5;
            }
        }
        out
    }
}

/// A 解放軍 checkpoint across a wide street near the start (`decay.ts`): two
/// sandbag emplacements, one on each side, held by two sentries each.
#[derive(Clone, Debug, PartialEq)]
pub struct Checkpoint {
    pub p: Vec2,
    pub rows: Vec<SandbagRow>,
}

impl Checkpoint {
    /// Where sentry `g` (0 or 1) stands (`main.ts`: cp ± (2.2, 1)).
    pub fn post(&self, g: usize) -> Vec2 {
        if g == 0 {
            self.p + Vec2::new(-2.2, -1.0)
        } else {
            self.p + Vec2::new(2.2, 1.0)
        }
    }
}

/// Two checkpoints on roads at least 6 m wide, 35–112 m from `c` (`R * 0.8`
/// with the Web's R = 140) and 50 m apart. The Web shuffles the segments with its
/// session random; a fixed stream keeps the native world deterministic.
pub fn checkpoints(w: &WorldData, c: Vec2) -> Vec<Checkpoint> {
    let mut segs: Vec<(Vec2, Vec2, f32)> = Vec::new();
    for r in &w.roads {
        if r.w < 6.0 {
            continue;
        }
        for pair in r.p.windows(2) {
            if pair[0].distance(pair[1]) > 0.5 {
                segs.push((pair[0], pair[1], r.w));
            }
        }
    }
    let mut seed = 0x0c4e_c9b1_u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed
    };
    let mut keyed: Vec<(u32, (Vec2, Vec2, f32))> = segs.into_iter().map(|s| (next(), s)).collect();
    keyed.sort_by_key(|(k, _)| *k);
    let mut out: Vec<Checkpoint> = Vec::new();
    for (_, (a, b, width)) in keyed {
        if out.len() >= 2 {
            break;
        }
        let m = (a + b) * 0.5;
        let dc = m.distance(c);
        if !(35.0..=112.0).contains(&dc) || out.iter().any(|q| q.p.distance(m) < 50.0) {
            continue;
        }
        let u = (b - a).normalize();
        let n = Vec2::new(-u.y, u.x);
        let ang = u.y.atan2(u.x);
        let mut rows = Vec::new();
        for sd in [-1.0_f32, 1.0] {
            let e = m + n * (width / 2.0 - 1.2) * sd;
            rows.push(SandbagRow {
                c: e,
                ang,
                len: 3.2,
                rows: 3,
            });
            rows.push(SandbagRow {
                c: e - u * 1.6 + n * 0.8 * -sd,
                ang: ang + std::f32::consts::FRAC_PI_2,
                len: 1.6,
                rows: 2,
            });
        }
        out.push(Checkpoint { p: m, rows });
    }
    out
}

/// Adds the remnants' and the checkpoints' sandbags to the world's cover
/// (called while planning the world, after the spawn is known).
pub fn install(w: &mut WorldData) {
    let sites = remnant_sites(w, w.spawn);
    for site in &sites {
        for wall in site.walls() {
            w.add_seg(wall.seg());
        }
    }
    w.remnants = sites;
    let cps = checkpoints(w, w.spawn);
    for cp in &cps {
        for row in &cp.rows {
            w.add_seg(row.seg());
        }
    }
    w.checkpoints = cps;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_positions_spread_around_the_spawn() {
        let w = crate::map::world();
        let sites = &w.remnants;
        assert_eq!(sites.len(), 4, "one per quarter");
        for (i, a) in sites.iter().enumerate() {
            let d = a.p.distance(w.spawn);
            assert!((110.0..=420.0).contains(&d), "{d}");
            assert!(w.bld_at(a.p).is_none());
            for b in &sites[i + 1..] {
                assert!(a.p.distance(b.p) > 160.0);
            }
        }
    }

    #[test]
    fn sandbags_are_low_cover_in_the_world() {
        let w = crate::map::world();
        let site = w.remnants[0];
        let front = site.walls()[0].seg();
        assert!(
            w.segs
                .iter()
                .any(|s| s.a == front.a && s.b == front.b && s.h == Some(SANDBAG_H))
        );
        assert_eq!(site.walls()[0].bags().len(), 5 + 5 + 4);
    }

    #[test]
    fn two_checkpoints_near_the_start_with_four_posts() {
        let w = crate::map::world();
        assert_eq!(w.checkpoints.len(), 2);
        for cp in &w.checkpoints {
            let d = cp.p.distance(w.spawn);
            assert!((35.0..=112.0).contains(&d), "{d}");
            assert_eq!(cp.rows.len(), 4);
        }
        assert!(w.checkpoints[0].p.distance(w.checkpoints[1].p) >= 50.0);
    }
}
