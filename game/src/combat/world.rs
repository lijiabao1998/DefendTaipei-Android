//! What combat asks the map about. The map / collision owner (Grok Bot's map module)
//! implements [`CombatWorld`] and installs it with `CombatWorldRes::set`; until then
//! [`OpenGround`] is used: no walls, no cover, nothing blocks a shot or a look.

use bevy::prelude::*;

use super::squad::Footprint;

/// Map queries used by soldiers, the player's shots and grenades
/// (original: `shotRay`, `solCover`, `bldAt`, `findCover` in main.ts).
pub trait CombatWorld: Send + Sync + 'static {
    /// Fraction (0..=1) of the way from `p` to `q` a shot / look gets before a wall or cover
    /// stops it. `ys` / `yt` are shooter / target heights (m), `own` ignores walls this close
    /// to the shooter. 1.0 = clear. (main.ts `shotRay`)
    fn shot_ray(&self, p: Vec2, q: Vec2, ys: f32, yt: f32, own: f32) -> f32 {
        let _ = (p, q, ys, yt, own);
        1.0
    }
    /// A cover spot for a soldier at `from` fighting someone at `target`: just behind the
    /// nearest cover segment within 8 m. (main.ts `solCover`)
    fn cover_spot(&self, from: Vec2, target: Vec2) -> Option<Vec2> {
        let _ = (from, target);
        None
    }
    /// Footprint of the building at `p`, if any. (main.ts `bldAt`)
    fn building_at(&self, p: Vec2) -> Option<Footprint> {
        let _ = p;
        None
    }
    /// Is there cover within `r` of `p` (grenade damage ×0.6 behind it). (main.ts `findCover`)
    fn cover_near(&self, p: Vec2, r: f32) -> bool {
        let _ = (p, r);
        false
    }
    /// True for the stand-in worlds ([`OpenGround`], [`ObstacleWorld`]): combat may replace them
    /// with the city's building boxes. A real map world keeps the default `false`.
    fn stand_in(&self) -> bool {
        false
    }
}

/// Default world: open ground.
pub struct OpenGround;
impl CombatWorld for OpenGround {
    fn stand_in(&self) -> bool {
        true
    }
}

/// Walls from the placeholder city's building boxes (`crate::city::Obstacles`), until the map
/// owner installs the real map. Shots and looks stop at the first box taller than the ray at
/// that point; `building_at` is the box under a point. No low cover yet.
pub struct ObstacleWorld(pub Vec<crate::city::Aabb2>);

impl ObstacleWorld {
    /// Fraction of the segment `p → q` before it enters box `b` (1.0 = misses it), slab test.
    fn enter(p: Vec2, q: Vec2, b: &crate::city::Aabb2) -> f32 {
        let d = q - p;
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for (o, dd, lo, hi) in [(p.x, d.x, b.min.x, b.max.x), (p.y, d.y, b.min.y, b.max.y)] {
            if dd.abs() < 1e-6 {
                if o <= lo || o >= hi {
                    return 1.0;
                }
            } else {
                let (mut a, mut c) = ((lo - o) / dd, (hi - o) / dd);
                if a > c {
                    std::mem::swap(&mut a, &mut c);
                }
                t0 = t0.max(a);
                t1 = t1.min(c);
                if t0 >= t1 {
                    return 1.0;
                }
            }
        }
        t0
    }
}

impl CombatWorld for ObstacleWorld {
    fn shot_ray(&self, p: Vec2, q: Vec2, ys: f32, yt: f32, own: f32) -> f32 {
        let len = p.distance(q);
        let mut best = 1.0f32;
        for b in &self.0 {
            // (standing inside a box, e.g. a doorway: that box does not block, as `own` walls)
            if b.contains(p, 0.0) || b.contains(q, 0.0) {
                continue;
            }
            let t = Self::enter(p, q, b);
            if t >= best || t * len < own {
                continue;
            }
            // the ray's height where it enters the box
            if ys + (yt - ys) * t < b.height {
                best = t;
            }
        }
        best
    }
    fn building_at(&self, p: Vec2) -> Option<Footprint> {
        self.0
            .iter()
            .find(|b| b.contains(p, 0.0))
            .map(|b| Footprint {
                min: b.min,
                max: b.max,
            })
    }
    fn stand_in(&self) -> bool {
        true
    }
}

#[derive(Resource)]
pub struct CombatWorldRes(pub Box<dyn CombatWorld>);

impl Default for CombatWorldRes {
    fn default() -> Self {
        Self(Box::new(OpenGround))
    }
}

impl CombatWorldRes {
    pub fn set(&mut self, w: impl CombatWorld) {
        self.0 = Box::new(w);
    }
}

/// Until the map owner installs a real world, walls come from the city's building boxes
/// (otherwise every look and shot passes through buildings).
pub(super) fn walls_from_city(
    obstacles: Option<Res<crate::city::Obstacles>>,
    mut world: ResMut<CombatWorldRes>,
) {
    let Some(obs) = obstacles else { return };
    if (obs.is_changed() || world.is_added()) && world.0.stand_in() {
        world.set(ObstacleWorld(obs.0.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::city::Aabb2;

    fn wall() -> ObstacleWorld {
        ObstacleWorld(vec![Aabb2 {
            min: Vec2::new(4.0, -2.0),
            max: Vec2::new(6.0, 2.0),
            height: 10.0,
        }])
    }

    #[test]
    fn a_building_stops_looks_and_shots() {
        let w = wall();
        let t = w.shot_ray(Vec2::ZERO, Vec2::new(10.0, 0.0), 1.6, 1.6, 0.0);
        assert!((t - 0.4).abs() < 1e-5, "{t}");
        assert_eq!(
            w.shot_ray(Vec2::ZERO, Vec2::new(10.0, 5.0), 1.6, 1.6, 0.0),
            1.0,
            "past the corner"
        );
        assert_eq!(
            w.shot_ray(Vec2::ZERO, Vec2::new(3.0, 0.0), 1.6, 1.6, 0.0),
            1.0,
            "short of it"
        );
        let low = ObstacleWorld(vec![Aabb2 {
            height: 1.0,
            ..wall().0[0]
        }]);
        assert_eq!(
            low.shot_ray(Vec2::ZERO, Vec2::new(10.0, 0.0), 1.6, 1.6, 0.0),
            1.0,
            "over a low box"
        );
        assert!(w.building_at(Vec2::new(5.0, 0.0)).is_some());
        assert!(w.building_at(Vec2::new(0.0, 0.0)).is_none());
    }
}
