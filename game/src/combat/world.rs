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
}

/// Default world: open ground.
pub struct OpenGround;
impl CombatWorld for OpenGround {}

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
