//! Port of `src/npc/squadai.ts` (v22.11 PLA squad tactics): pure, deterministic helpers
//! (no randomness, no time) so every client derives the same roles / points.

use bevy::math::Vec2;

use super::tuning::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Role {
    Suppress,
    FlankL,
    FlankR,
    #[default]
    Rifle,
}

impl Role {
    pub fn is_flank(self) -> bool {
        matches!(self, Role::FlankL | Role::FlankR)
    }
}

/// Cluster combat soldiers (single-link, radius `SQUAD_R`); members sorted by id → stable roles.
/// Input: (id, position). Output: squads of ids, each sorted by id.
pub fn squads(list: &[(u32, Vec2)]) -> Vec<Vec<u32>> {
    let mut left: Vec<(u32, Vec2)> = list.to_vec();
    left.sort_by_key(|m| m.0);
    let mut out = Vec::new();
    while !left.is_empty() {
        let mut g = vec![left.remove(0)];
        let mut i = 0;
        while i < g.len() {
            let mut j = left.len();
            while j > 0 {
                j -= 1;
                if left[j].1.distance(g[i].1) < SQUAD_R {
                    g.push(left.remove(j));
                }
            }
            i += 1;
        }
        let mut ids: Vec<u32> = g.into_iter().map(|m| m.0).collect();
        ids.sort();
        out.push(ids);
    }
    out
}

/// Slot 0 suppresses from cover, 1/2 flank left/right, the rest are riflemen; a lone soldier just fights.
pub fn role_of(slot: usize, n: usize) -> Role {
    if n < 2 {
        return Role::Rifle;
    }
    match slot {
        0 => Role::Suppress,
        1 => Role::FlankL,
        2 => Role::FlankR,
        _ => Role::Rifle,
    }
}

/// Point to the side of the player (relative to the squad's line of approach), pulled a little toward the soldier.
pub fn flank_point(s: Vec2, p: Vec2, side: f32) -> Vec2 {
    let d = p - s;
    let l = d.length();
    let l = if l == 0.0 { 1.0 } else { l };
    let n = Vec2::new(-d.y / l, d.x / l);
    p + n * side * FLANK_DIST - d / l * 2.0
}

pub fn retreat_point(s: Vec2, p: Vec2, d: f32) -> Vec2 {
    let v = s - p;
    let l = v.length();
    let l = if l == 0.0 { 1.0 } else { l };
    s + v / l * d
}

/// Peek-shoot in turns: squad members alternate crouch windows (3 s cycle, offset by slot).
pub fn peek_up(peek_t: f32, slot: usize) -> bool {
    (peek_t + slot as f32 * 1.5) % 3.0 >= 1.5
}

/// Building footprint for [`search_points`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    pub min: Vec2,
    pub max: Vec2,
}

/// Coordinated search of the last-known position: each member gets a different sweep of 3 points.
/// Inside a building the points stay inside its footprint (quadrants ≈ rooms).
pub fn search_points(
    lkp: Vec2,
    slot: usize,
    n: usize,
    bld: Option<Footprint>,
    inside: impl Fn(Vec2) -> bool,
) -> Vec<Vec2> {
    let mut pts = Vec::with_capacity(3);
    if let Some(b) = bld {
        let c = (b.min + b.max) / 2.0;
        let h = (b.max - b.min) / 4.0;
        let q = [
            Vec2::new(c.x - h.x, c.y - h.y),
            Vec2::new(c.x + h.x, c.y - h.y),
            Vec2::new(c.x + h.x, c.y + h.y),
            Vec2::new(c.x - h.x, c.y + h.y),
        ];
        for k in 0..3 {
            let p = q[(slot + k) % 4];
            pts.push(if inside(p) { p } else { lkp });
        }
        return pts;
    }
    let a0 = (slot as f32 / n.max(1) as f32) * std::f32::consts::TAU + 0.7;
    for k in 0..3 {
        let a = a0 + k as f32 * 0.9;
        let r = 4.0 + k as f32 * 2.5;
        pts.push(lkp + Vec2::new(a.cos(), a.sin()) * r);
    }
    pts
}

pub fn grenade_dmg(d: f32) -> f32 {
    if d < GREN_CORE {
        return GREN_CORE_DMG * (1.0 - 0.3 * (d / GREN_CORE));
    }
    if d < GREN_WOUND {
        return GREN_WOUND_DMG * ((GREN_WOUND - d) / (GREN_WOUND - GREN_CORE)).powi(2);
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squads_cluster_single_link_and_sort() {
        // 3 → 1 → 2 chain within 25 m of each other; 9 far away
        let list = [
            (3, Vec2::new(0.0, 0.0)),
            (9, Vec2::new(200.0, 0.0)),
            (1, Vec2::new(20.0, 0.0)),
            (2, Vec2::new(40.0, 0.0)),
        ];
        assert_eq!(squads(&list), vec![vec![1, 2, 3], vec![9]]);
    }

    #[test]
    fn roles_match_original() {
        assert_eq!(role_of(0, 1), Role::Rifle);
        assert_eq!(role_of(0, 3), Role::Suppress);
        assert_eq!(role_of(1, 3), Role::FlankL);
        assert_eq!(role_of(2, 3), Role::FlankR);
        assert_eq!(role_of(3, 5), Role::Rifle);
    }

    #[test]
    fn flank_point_is_beside_player() {
        // soldier south of player, approaching +z: left flank is −x side (normal (−dz, dx))
        let fp = flank_point(Vec2::new(0.0, -10.0), Vec2::ZERO, 1.0);
        assert!((fp - Vec2::new(-7.0, -2.0)).length() < 1e-5, "{fp:?}");
        let fr = flank_point(Vec2::new(0.0, -10.0), Vec2::ZERO, -1.0);
        assert!((fr - Vec2::new(7.0, -2.0)).length() < 1e-5, "{fr:?}");
    }

    #[test]
    fn retreat_goes_away() {
        let r = retreat_point(Vec2::new(0.0, 5.0), Vec2::ZERO, 6.0);
        assert!((r - Vec2::new(0.0, 11.0)).length() < 1e-5);
    }

    #[test]
    fn peek_alternates_by_slot() {
        assert!(!peek_up(0.0, 0));
        assert!(peek_up(0.0, 1));
        assert!(peek_up(1.6, 0));
        assert!(!peek_up(1.6, 1));
    }

    #[test]
    fn search_points_open_and_inside() {
        let p = search_points(Vec2::ZERO, 0, 2, None, |_| true);
        assert_eq!(p.len(), 3);
        assert!((p[0].length() - 4.0).abs() < 1e-4);
        assert!((p[2].length() - 9.0).abs() < 1e-4);
        let b = Footprint {
            min: Vec2::new(0.0, 0.0),
            max: Vec2::new(8.0, 8.0),
        };
        let q = search_points(Vec2::new(1.0, 1.0), 1, 2, Some(b), |_| true);
        assert_eq!(
            q,
            vec![
                Vec2::new(6.0, 2.0),
                Vec2::new(6.0, 6.0),
                Vec2::new(2.0, 6.0)
            ]
        );
    }

    #[test]
    fn grenade_falloff() {
        assert_eq!(grenade_dmg(0.0), GREN_CORE_DMG);
        assert!((grenade_dmg(GREN_CORE - 1e-4) - GREN_CORE_DMG * 0.7).abs() < 0.01);
        assert!(grenade_dmg(4.0) > 0.0 && grenade_dmg(4.0) < GREN_WOUND_DMG);
        assert_eq!(grenade_dmg(GREN_WOUND), 0.0);
    }
}
