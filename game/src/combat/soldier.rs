//! PLA soldier brain: port of the per-soldier block of `update()` in the original
//! `src/core/main.ts` (vision `plaLook`, patrol / investigate / combat, cover, v22.11 squad
//! roles, peek-shooting, rifle-butt melee, wounded retreat, coordinated search) and of
//! `damageSol`. Pure data + functions so it is unit-testable; the ECS glue is in `mod.rs`.

use std::collections::VecDeque;

use bevy::math::Vec2;

use super::EnemyKind;
use super::squad::{self, Role};
use super::tuning::*;
use super::world::CombatWorld;
use crate::city::Lcg;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SolState {
    #[default]
    Patrol,
    Investigate,
    Combat,
    Dead,
}

/// One soldier's AI state (original `Sol`).
#[derive(Clone, Debug, Default)]
pub struct Soldier {
    pub id: u32,
    pub kind: EnemyKind,
    /// Gun id (tuning::GUNS) the model holds: QBZ-191 (`rifle`) or QBZ-95-1 (`qbz95`).
    pub weapon: &'static str,
    pub hp: f32,
    pub pos: Vec2,
    /// Facing angle: direction (cos dir, sin dir) on the (x, z) ground plane.
    pub dir: f32,
    pub state: SolState,
    pub route: Vec<Vec2>,
    pub wi: usize,
    pub target: Option<Vec2>,
    pub alert: f32,
    pub timer: f32,
    pub cd: f32,
    /// Sentry: holds this point instead of patrolling.
    pub post: Option<Vec2>,
    pub last_seen: Option<Vec2>,
    pub vs: f32,
    pub stun: f32,
    pub aiming: bool,
    pub cov: Option<Vec2>,
    pub cov_t: f32,
    pub crouch: bool,
    pub peek_t: f32,
    pub m_cd: f32,
    pub m_hit: Option<f32>,
    pub role: Role,
    pub slot: usize,
    pub sq_n: usize,
    pub flank_t: Option<f32>,
    pub retreat_t: f32,
    pub retreated: bool,
    pub search: VecDeque<Vec2>,
    /// Speed actually moved last tick (m/s), for the animation.
    pub moved: f32,
    /// Seconds of the hurt flinch left (rig.hurtT).
    pub hurt_t: f32,
}

impl Soldier {
    pub fn new(id: u32, kind: EnemyKind, pos: Vec2, route: Vec<Vec2>) -> Self {
        let route = if route.is_empty() { vec![pos] } else { route };
        Self {
            id,
            kind,
            // v23.5 a mix of QBZ-191 and bullpup QBZ-95-1 silhouettes
            weapon: if id % 4 == 1 { "qbz95" } else { "rifle" },
            hp: SOLDIER_HP,
            pos,
            state: SolState::Patrol,
            wi: 1,
            post: (kind == EnemyKind::Garrison).then_some(pos),
            route,
            ..Default::default()
        }
    }

    pub fn alive(&self) -> bool {
        self.state != SolState::Dead
    }

    /// `solActive`: night patrols only at night.
    pub fn active(&self, night: bool) -> bool {
        self.kind != EnemyKind::NightPatrol || night
    }
}

/// What the soldiers know about the player this frame (original `PlaMark` for the player).
#[derive(Clone, Copy, Debug)]
pub struct PlayerMark {
    pub pos: Vec2,
    pub alive: bool,
    pub sneak: bool,
    pub cover: bool,
    pub light: bool,
    pub inside: bool,
    /// Height soldiers aim at (playerAimH: 0.75 crouched in low cover, 0.9 sneaking, 1.25).
    pub aim_h: f32,
}

impl PlayerMark {
    pub fn at(pos: Vec2) -> Self {
        Self {
            pos,
            alive: true,
            sneak: false,
            cover: false,
            light: false,
            inside: false,
            aim_h: 1.25,
        }
    }
}

pub struct Ctx<'a> {
    pub dt: f32,
    pub night: bool,
    /// Rain amount 0..1 (WX.amt).
    pub rain: f32,
    pub player: PlayerMark,
    pub world: &'a dyn CombatWorld,
}

/// A shot a soldier fired this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolShot {
    pub from: Vec2,
    /// Where the tracer ends.
    pub to: Vec2,
    pub hit: bool,
    pub dmg: f32,
    pub bleed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Out {
    pub shot: Option<SolShot>,
    /// Rifle butt landed on the player for this much damage.
    pub melee_dmg: Option<f32>,
    /// Just switched to combat: radio for support from here (and make noise).
    pub spotted: Option<Vec2>,
    /// Asked for a "split up and search" callout.
    pub search_callout: bool,
    /// "I'm hit! Cover me!"
    pub retreat_callout: bool,
}

fn rnd(rng: &mut Lcg) -> f32 {
    rng.next_f32()
}

fn ang_diff(a: f32, b: f32) -> f32 {
    (a - b).sin().atan2((a - b).cos())
}

/// `plaLook`: does soldier `s` see the player, and the range it sees at.
pub fn look(s: &Soldier, c: &Ctx, dist: f32) -> (bool, f32) {
    let m = &c.player;
    let mut range = if c.night { VISION_NIGHT } else { VISION_DAY }
        * if m.sneak { 0.55 } else { 1.0 }
        * if m.light && c.night { 2.0 } else { 1.0 };
    if m.inside {
        range *= 0.6;
    } else {
        range *= 1.0 - c.rain * 0.3;
    }
    let d = m.pos - s.pos;
    let facing = ang_diff(d.y.atan2(d.x), s.dir).abs() < VISION_HALF_ANGLE;
    let sees = m.alive
        && dist < range
        && (facing || dist < VISION_NEAR)
        && c.world.shot_ray(s.pos, m.pos, 1.6, m.aim_h + 0.1, 0.6) >= 1.0;
    (sees, range)
}

/// One AI tick for a live, active soldier. Mirrors the original order of operations.
pub fn tick(s: &mut Soldier, c: &Ctx, rng: &mut Lcg) -> Out {
    let mut out = Out::default();
    let dt = c.dt;
    s.hurt_t = (s.hurt_t - dt).max(0.0);
    let pp = c.player.pos;
    let dist = pp.distance(s.pos);
    if dist > DORMANT_DIST {
        s.moved = 0.0;
        return out;
    }
    let (sees, range) = look(s, c, dist);
    let d = pp - s.pos;
    if sees {
        s.alert += dt * (1.6 - dist / range) * 1.6;
        if s.state != SolState::Combat {
            s.state = SolState::Investigate;
            s.target = Some(pp);
            s.timer = 15.0;
        }
    } else {
        s.alert = (s.alert - dt * 0.15).max(0.0);
    }
    if s.alert >= 1.0 && s.state != SolState::Combat {
        s.state = SolState::Combat;
        out.spotted = Some(pp);
    }
    if sees && s.state == SolState::Combat {
        s.last_seen = Some(pp);
    }
    let mut goal: Option<Vec2> = None;
    let mut sp = if c.night {
        SPEED_PATROL_NIGHT
    } else {
        SPEED_PATROL_DAY
    };
    match s.state {
        SolState::Patrol => {
            let mut g = *s.route.get(s.wi).unwrap_or(&s.route[0]);
            if g.distance(s.pos) < 1.0 {
                s.wi += 1;
                if s.wi >= s.route.len() {
                    s.route.reverse();
                    s.wi = 1;
                }
                g = *s.route.get(s.wi).unwrap_or(&s.route[0]);
            }
            goal = Some(g);
        }
        SolState::Investigate => {
            let t = s.target.unwrap_or(s.pos);
            goal = Some(t);
            sp = SPEED_INVESTIGATE;
            s.timer -= dt;
            if s.timer <= 0.0 || t.distance(s.pos) < 1.0 {
                if s.timer <= 0.0 {
                    s.state = SolState::Patrol;
                    s.alert = 0.0;
                } else if let Some(n) = s.search.pop_front() {
                    s.target = Some(n);
                    s.timer = 8.0;
                } else {
                    s.timer = s.timer.min(4.0);
                    goal = None;
                    s.dir += dt;
                }
            }
        }
        SolState::Combat => {
            s.cd -= dt;
            if sees && dist < ENGAGE_DIST {
                goal = (dist > CLOSE_IN_DIST).then_some(pp);
                s.dir = d.y.atan2(d.x);
                // v21.4 cover: run to the nearest cover between them and you, then crouch / peek and fire
                s.cov_t -= dt;
                if s.cov_t <= 0.0 {
                    s.cov_t = 5.0 + rnd(rng) * 3.0;
                    s.cov = c.world.cover_spot(s.pos, pp);
                }
                let mut at_cov = false;
                if let Some(f) = s.flank_t.as_mut() {
                    *f -= dt;
                }
                let flanking = s.role.is_flank() && s.flank_t.unwrap_or(0.0) > 0.0 && dist > 4.0;
                if let (Some(cv), false) = (s.cov, flanking) {
                    if cv.distance(s.pos) > 0.5 {
                        goal = Some(cv);
                        sp = SPEED_TO_COVER;
                    } else {
                        goal = None;
                        at_cov = true;
                    }
                }
                if flanking {
                    let side = if s.role == Role::FlankL { 1.0 } else { -1.0 };
                    let fp = squad::flank_point(s.pos, pp, side);
                    if c.world.building_at(fp).is_none() && fp.distance(s.pos) > 1.2 {
                        goal = Some(fp);
                        sp = SPEED_FLANK;
                    } else {
                        s.flank_t = Some(0.0);
                    }
                }
                s.peek_t += dt;
                s.crouch = at_cov && !squad::peek_up(s.peek_t, s.slot);
                // v22.10 point-blank: rifle-butt strike with a readable wind-up instead of firing
                s.m_cd -= dt;
                if let Some(mh) = s.m_hit {
                    let mh = mh - dt;
                    if mh <= 0.0 {
                        s.m_hit = None;
                        let f = s.dir.cos() * d.x + s.dir.sin() * d.y;
                        if dist < MELEE_REACH && f > 0.2 && c.player.alive {
                            out.melee_dmg = Some(MELEE_DMG_MIN + rnd(rng) * MELEE_DMG_RAND);
                        }
                    } else {
                        s.m_hit = Some(mh);
                    }
                } else if dist < MELEE_START_DIST && s.m_cd <= 0.0 && s.stun <= 0.0 {
                    s.m_hit = Some(MELEE_WINDUP);
                    s.m_cd = 1.5 + rnd(rng) * 0.4;
                    s.cd = s.cd.max(0.8);
                    s.crouch = false;
                }
                if s.m_hit.is_none()
                    && s.retreat_t <= 0.0
                    && s.cd <= 0.0
                    && s.stun <= 0.0
                    && !s.crouch
                    && (s.cov.is_none() || at_cov || dist < CLOSE_IN_DIST)
                {
                    s.cd = (SOL_FIRE_CD_MIN + rnd(rng) * SOL_FIRE_CD_RAND)
                        * match (s.role, flanking) {
                            (Role::Suppress, _) => 0.6,
                            (_, true) => 1.6,
                            _ => 1.0,
                        };
                    out.shot = Some(fire(s, c, dist, rng));
                }
            } else if let Some(ls) = s.last_seen {
                goal = Some(ls);
                sp = SPEED_TO_COVER;
                let dl = ls.distance(s.pos);
                if dl < 1.2 || (s.role != Role::Suppress && s.slot > 0 && dl < 6.0) {
                    // v22.11 coordinated search: split the last-known area, one sweep each
                    let b = c.world.building_at(ls);
                    let pts = squad::search_points(ls, s.slot, s.sq_n.max(1), b, |p| {
                        c.world.building_at(p) == b
                    });
                    s.search = pts.into();
                    out.search_callout = s.slot == 0;
                    s.last_seen = None;
                    s.state = SolState::Investigate;
                    s.target = s.search.pop_front();
                    s.timer = 10.0;
                    s.alert = 0.5;
                    s.flank_t = None;
                }
            } else {
                s.state = SolState::Investigate;
                s.target = Some(s.pos);
                s.timer = 8.0;
            }
        }
        SolState::Dead => return out,
    }
    // v22.11 wounded → fall back once, away from the player, holding fire
    if s.state == SolState::Combat && s.hp < RETREAT_HP && !s.retreated {
        s.retreated = true;
        s.retreat_t = RETREAT_T;
        out.retreat_callout = true;
    }
    if s.retreat_t > 0.0 {
        s.retreat_t -= dt;
        goal = Some(squad::retreat_point(s.pos, pp, 6.0));
        sp = SPEED_RETREAT;
        s.cov = None;
        s.crouch = false;
    }
    if s.state != SolState::Combat {
        s.cov = None;
        s.crouch = false;
    }
    s.aiming =
        s.state == SolState::Combat && sees && dist < ENGAGE_DIST && s.stun <= 0.0 && !s.crouch;
    if s.stun > 0.0 {
        s.stun -= dt;
        goal = None;
    }
    if let (Some(post), SolState::Patrol) = (s.post, s.state) {
        goal = (post.distance(s.pos) > 0.8).then_some(post);
        if goal.is_none() {
            s.dir += (s.id as f32).sin() * dt * 0.3;
        }
    }
    s.moved = 0.0;
    if let Some(g) = goal {
        let gd = g - s.pos;
        let gl = gd.length();
        if gl > 0.3 {
            let td = gd.y.atan2(gd.x);
            if !s.aiming {
                s.dir += ang_diff(td, s.dir).clamp(-dt * 6.0, dt * 6.0);
            }
            s.vs += (sp - s.vs) * (dt * 4.0).min(1.0);
            let step = gd / gl * s.vs * dt;
            s.pos += step;
            s.moved = step.length() / dt.max(1e-6);
        }
    }
    if s.moved == 0.0 {
        s.vs *= (1.0 - dt * 6.0).max(0.0);
    }
    out
}

/// The soldier's rifle shot at the player: wall ray first, then the hit roll.
fn fire(s: &Soldier, c: &Ctx, dist: f32, rng: &mut Lcg) -> SolShot {
    let m = &c.player;
    let clear = c.world.shot_ray(s.pos, m.pos, 1.4, m.aim_h, 1.0);
    let hit_p = (SOL_HIT_BASE - dist / SOL_HIT_FALLOFF).max(SOL_HIT_FLOOR)
        * if m.sneak { 0.7 } else { 1.0 }
        * if c.night { 0.7 } else { 1.0 }
        * if m.cover { 0.5 } else { 1.0 }
        * if s.role == Role::Suppress { 0.6 } else { 1.0 };
    let hit = clear >= 1.0 && rnd(rng) < hit_p;
    let to = if hit {
        m.pos
    } else if clear < 1.0 {
        s.pos + (m.pos - s.pos) * clear
    } else {
        m.pos + Vec2::new(rnd(rng) - 0.5, rnd(rng) - 0.5) * 3.0
    };
    let (dmg, bleed) = if hit {
        (
            SOL_DMG_MIN + rnd(rng) * SOL_DMG_RAND,
            rnd(rng) < SOL_BLEED_CHANCE,
        )
    } else {
        (0.0, false)
    };
    SolShot {
        from: s.pos,
        to,
        hit,
        dmg,
        bleed,
    }
}

/// `damageSol`: returns true if this hit killed the soldier (exactly once).
/// `from` is where the shot came from (the soldier goes looking there).
pub fn damage(s: &mut Soldier, dmg: f32, from: Vec2) -> bool {
    if !s.alive() {
        return false;
    }
    s.hp -= dmg;
    if s.hp <= 0.0 {
        s.state = SolState::Dead;
        s.aiming = false;
        s.crouch = false;
        return true;
    }
    s.hurt_t = 0.35;
    s.m_hit = None; // a hit interrupts a rifle-butt wind-up
    s.stun = s.stun.max(0.4);
    s.state = SolState::Combat;
    s.alert = 1.0;
    s.last_seen = Some(from);
    false
}

/// Radio (`radio`): idle soldiers near `from` go to investigate `p`. Returns how many came.
pub fn radio<'a>(
    soldiers: impl Iterator<Item = &'a mut Soldier>,
    p: Vec2,
    from: Vec2,
    from_id: u32,
    night: bool,
) -> usize {
    let mut n = 0;
    for o in soldiers {
        if o.id == from_id
            || !o.alive()
            || o.state == SolState::Combat
            || !o.active(night)
            || o.post.is_some()
            || o.pos.distance(from) > RADIO_R
        {
            continue;
        }
        o.state = SolState::Investigate;
        o.target = Some(p);
        o.timer = 25.0;
        o.alert = o.alert.max(0.6);
        n += 1;
    }
    n
}

/// `noise`: soldiers (not already fighting) within `r` of (x, z) come to look.
pub fn noise<'a>(
    soldiers: impl Iterator<Item = &'a mut Soldier>,
    at: Vec2,
    r: f32,
    rain: f32,
    night: bool,
    rng: &mut Lcg,
) {
    let r = r * (1.0 - rain * 0.25);
    for s in soldiers {
        if s.alive() && s.state != SolState::Combat && s.active(night) && s.pos.distance(at) < r {
            s.state = SolState::Investigate;
            s.target = Some(at + Vec2::new(rnd(rng) - 0.5, rnd(rng) - 0.5) * 3.0);
            s.timer = 20.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::world::OpenGround;

    struct Wall;
    impl CombatWorld for Wall {
        fn shot_ray(&self, _: Vec2, _: Vec2, _: f32, _: f32, _: f32) -> f32 {
            0.5
        }
    }

    fn ctx<'a>(player: Vec2, world: &'a dyn CombatWorld) -> Ctx<'a> {
        Ctx {
            dt: 0.1,
            night: false,
            rain: 0.0,
            player: PlayerMark::at(player),
            world,
        }
    }

    fn facing_soldier() -> Soldier {
        let mut s = Soldier::new(0, EnemyKind::Patrol, Vec2::ZERO, vec![]);
        s.dir = 0.0; // facing +x
        s
    }

    #[test]
    fn sees_in_cone_not_behind_or_through_walls() {
        let s = facing_soldier();
        let w = OpenGround;
        assert!(look(&s, &ctx(Vec2::new(10.0, 0.0), &w), 10.0).0);
        assert!(!look(&s, &ctx(Vec2::new(-10.0, 0.0), &w), 10.0).0, "behind");
        assert!(
            !look(&s, &ctx(Vec2::new(25.0, 0.0), &w), 25.0).0,
            "beyond 20 m by day"
        );
        assert!(
            look(&s, &ctx(Vec2::new(-1.0, 0.0), &w), 1.0).0,
            "close enough behind"
        );
        assert!(!look(&s, &ctx(Vec2::new(10.0, 0.0), &Wall), 10.0).0, "wall");
    }

    #[test]
    fn alert_builds_to_combat_and_radios_once() {
        let mut s = facing_soldier();
        let w = OpenGround;
        let c = ctx(Vec2::new(10.0, 0.0), &w);
        let mut rng = Lcg(1);
        let mut spotted = 0;
        for _ in 0..40 {
            spotted += tick(&mut s, &c, &mut rng).spotted.is_some() as u32;
        }
        assert_eq!(s.state, SolState::Combat);
        assert_eq!(spotted, 1);
        assert_eq!(s.last_seen, Some(Vec2::new(10.0, 0.0)));
    }

    #[test]
    fn combat_soldier_fires_on_cooldown_and_hits_sometimes() {
        let mut s = facing_soldier();
        s.state = SolState::Combat;
        s.alert = 1.0;
        let w = OpenGround;
        let c = ctx(Vec2::new(6.0, 0.0), &w);
        let mut rng = Lcg(7);
        let (mut shots, mut hits) = (0, 0);
        for _ in 0..600 {
            if let Some(sh) = tick(&mut s, &c, &mut rng).shot {
                shots += 1;
                hits += sh.hit as u32;
                if sh.hit {
                    assert!((SOL_DMG_MIN..SOL_DMG_MIN + SOL_DMG_RAND).contains(&sh.dmg));
                }
            }
        }
        // 60 s at 1.1–1.7 s between shots
        assert!((30..=60).contains(&shots), "shots {shots}");
        // hit chance at 6 m ≈ 0.45
        assert!(hits > shots / 5 && hits < shots, "hits {hits}/{shots}");
    }

    #[test]
    fn wall_blocks_soldier_hits() {
        let mut s = facing_soldier();
        s.state = SolState::Combat;
        let c = ctx(Vec2::new(1.0, 0.0), &Wall); // within VISION_NEAR? no: Wall blocks sight
        let mut rng = Lcg(3);
        for _ in 0..100 {
            assert!(tick(&mut s, &c, &mut rng).shot.is_none_or(|sh| !sh.hit));
        }
    }

    #[test]
    fn point_blank_rifle_butt_lands() {
        let mut s = facing_soldier();
        s.state = SolState::Combat;
        s.alert = 1.0;
        let w = OpenGround;
        let c = ctx(Vec2::new(1.0, 0.0), &w);
        let mut rng = Lcg(5);
        let mut melee = None;
        for _ in 0..20 {
            melee = melee.or(tick(&mut s, &c, &mut rng).melee_dmg);
        }
        let m = melee.expect("rifle butt");
        assert!((MELEE_DMG_MIN..MELEE_DMG_MIN + MELEE_DMG_RAND).contains(&m));
    }

    #[test]
    fn damage_kills_once_and_provokes() {
        let mut s = facing_soldier();
        assert!(!damage(&mut s, 62.0, Vec2::new(3.0, 3.0)));
        assert_eq!(s.state, SolState::Combat);
        assert_eq!(s.last_seen, Some(Vec2::new(3.0, 3.0)));
        assert!(s.stun > 0.0);
        assert!(damage(&mut s, 62.0, Vec2::ZERO));
        assert_eq!(s.state, SolState::Dead);
        assert!(!damage(&mut s, 62.0, Vec2::ZERO), "already dead");
    }

    #[test]
    fn wounded_retreats_once() {
        let mut s = facing_soldier();
        s.state = SolState::Combat;
        s.hp = 30.0;
        let w = OpenGround;
        let c = ctx(Vec2::new(10.0, 0.0), &w);
        let mut rng = Lcg(9);
        let o = tick(&mut s, &c, &mut rng);
        assert!(o.retreat_callout);
        assert!(s.retreat_t > 0.0);
        assert!(s.pos.x < 0.0, "moves away from the player");
        assert!(!tick(&mut s, &c, &mut rng).retreat_callout);
    }

    #[test]
    fn lost_contact_turns_into_search() {
        let mut s = facing_soldier();
        s.state = SolState::Combat;
        s.last_seen = Some(Vec2::new(0.5, 0.0));
        let w = OpenGround;
        let c = ctx(Vec2::new(-50.0, 0.0), &w); // out of sight
        let mut rng = Lcg(2);
        let o = tick(&mut s, &c, &mut rng);
        assert!(o.search_callout);
        assert_eq!(s.state, SolState::Investigate);
        assert_eq!(s.search.len(), 2);
    }

    #[test]
    fn radio_and_noise_pull_idle_soldiers() {
        let mut list = [
            Soldier::new(0, EnemyKind::Patrol, Vec2::ZERO, vec![]),
            Soldier::new(1, EnemyKind::Patrol, Vec2::new(30.0, 0.0), vec![]),
            Soldier::new(2, EnemyKind::Patrol, Vec2::new(90.0, 0.0), vec![]),
            Soldier::new(3, EnemyKind::Garrison, Vec2::new(5.0, 0.0), vec![]),
        ];
        let n = radio(list.iter_mut(), Vec2::new(1.0, 1.0), Vec2::ZERO, 0, false);
        assert_eq!(
            n, 1,
            "only #1: #0 is the caller, #2 too far, #3 holds its post"
        );
        assert_eq!(list[1].state, SolState::Investigate);
        let mut rng = Lcg(4);
        noise(
            list.iter_mut(),
            Vec2::new(90.0, 0.0),
            10.0,
            0.0,
            false,
            &mut rng,
        );
        assert_eq!(list[2].state, SolState::Investigate);
    }

    #[test]
    fn night_patrol_only_at_night() {
        let s = Soldier::new(0, EnemyKind::NightPatrol, Vec2::ZERO, vec![]);
        assert!(!s.active(false));
        assert!(s.active(true));
    }

    #[test]
    fn weapon_mix_matches_original() {
        let w: Vec<_> = (0..4)
            .map(|i| Soldier::new(i, EnemyKind::Patrol, Vec2::ZERO, vec![]).weapon)
            .collect();
        assert_eq!(w, ["rifle", "qbz95", "rifle", "rifle"]);
    }
}
