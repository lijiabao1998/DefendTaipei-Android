//! Pure combat rules (no ECS), so they can be unit-tested directly.

use bevy::math::Vec2;

use super::EnemyKind;
use super::tuning::{self, WaveSpec};

/// Outcome of damaging something with hit points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitResult {
    /// Took damage and is still alive.
    Hurt,
    /// This hit took it from alive to dead (fire death logic exactly once).
    Killed,
    /// Was already dead; nothing happened.
    Ignored,
}

/// Subtracts `dmg` from `hp`; reports `Killed` only on the alive → dead transition.
pub fn damage_hp(hp: &mut i32, dmg: i32) -> HitResult {
    if *hp <= 0 {
        return HitResult::Ignored;
    }
    *hp -= dmg.max(0);
    if *hp <= 0 {
        HitResult::Killed
    } else {
        HitResult::Hurt
    }
}

/// Player vitals: HP plus the post-hit invulnerability timer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vitals {
    pub hp: f32,
    pub invuln: f32,
}

impl Vitals {
    pub fn full() -> Self {
        Self {
            hp: tuning::PLAYER_MAX_HP,
            invuln: 0.0,
        }
    }

    pub fn alive(&self) -> bool {
        self.hp > 0.0
    }

    /// Applies a contact hit unless invulnerable or dead; starts the i-frames.
    pub fn take_hit(&mut self, dmg: f32) -> HitResult {
        if !self.alive() || self.invuln > 0.0 || dmg <= 0.0 {
            return HitResult::Ignored;
        }
        self.hp = (self.hp - dmg).max(0.0);
        if self.hp <= 0.0 {
            HitResult::Killed
        } else {
            self.invuln = tuning::PLAYER_INVULN_SECS;
            HitResult::Hurt
        }
    }

    pub fn tick(&mut self, dt: f32) {
        self.invuln = (self.invuln - dt).max(0.0);
    }
}

/// Steering push away from neighbours closer than the sum of radii (+ a little air).
pub fn separation(me: Vec2, my_r: f32, others: impl IntoIterator<Item = (Vec2, f32)>) -> Vec2 {
    let mut push = Vec2::ZERO;
    for (o, r) in others {
        let d = me - o;
        let l = d.length();
        let want = my_r + r + 0.2;
        if l > 1e-3 && l < want {
            push += d / l * (want - l) / want;
        }
    }
    push * tuning::SEPARATION_STRENGTH
}

/// Distance from `c` to the segment `a`–`b`.
pub fn segment_distance(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 1e-8 {
        ((c - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t).distance(c)
}

/// Nearest point within `range` of `origin`.
pub fn nearest_in_range(
    origin: Vec2,
    range: f32,
    points: impl IntoIterator<Item = Vec2>,
) -> Option<Vec2> {
    points
        .into_iter()
        .map(|p| (p.distance_squared(origin), p))
        .filter(|(d2, _)| *d2 < range * range)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p)
}

/// Spawn order for one wave: brutes spread evenly through the rushers.
pub fn wave_queue(spec: &WaveSpec) -> Vec<EnemyKind> {
    let total = spec.rushers + spec.brutes;
    let mut out = Vec::with_capacity(total as usize);
    for i in 0..total {
        // Bresenham-style distribution; queue is popped from the back.
        let brute = (i + 1) * spec.brutes / total.max(1) > i * spec.brutes / total.max(1);
        out.push(if brute {
            EnemyKind::Brute
        } else {
            EnemyKind::Rusher
        });
    }
    out.reverse();
    out
}

#[derive(Clone, Debug, PartialEq)]
pub enum WavePhase {
    /// Counting down to the start of wave `index`.
    Waiting { t: f32 },
    /// Spawning the queue for the current wave.
    Spawning { queue: Vec<EnemyKind>, t: f32 },
    /// Everything spawned; waiting for the last enemy to die.
    Clearing,
    /// All waves cleared.
    Won,
}

/// What happened during one director tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaveTick {
    pub spawn: Option<EnemyKind>,
    /// 1-based wave number that just started.
    pub started: Option<u32>,
    /// 1-based wave number that was just cleared.
    pub cleared: Option<u32>,
    pub won: bool,
}

/// Wave pacing: a short delay, spawn a wave's queue, wait for it to be cleared,
/// take a break, repeat; after the last wave report `won`.
#[derive(Clone, Debug, PartialEq)]
pub struct WaveDirector {
    pub index: usize,
    pub phase: WavePhase,
    waves: &'static [WaveSpec],
}

impl Default for WaveDirector {
    fn default() -> Self {
        Self::new(tuning::WAVES)
    }
}

impl WaveDirector {
    pub fn new(waves: &'static [WaveSpec]) -> Self {
        Self {
            index: 0,
            phase: WavePhase::Waiting {
                t: tuning::FIRST_WAVE_DELAY,
            },
            waves,
        }
    }

    /// 1-based number of the current (or upcoming) wave.
    pub fn wave_number(&self) -> u32 {
        (self.index.min(self.waves.len().saturating_sub(1)) + 1) as u32
    }

    pub fn total_waves(&self) -> u32 {
        self.waves.len() as u32
    }

    /// Advances pacing by `dt`. `alive` is the current live enemy count.
    pub fn tick(&mut self, dt: f32, alive: usize) -> WaveTick {
        let mut out = WaveTick::default();
        match &mut self.phase {
            WavePhase::Waiting { t } => {
                *t -= dt;
                if *t <= 0.0 {
                    if let Some(spec) = self.waves.get(self.index) {
                        out.started = Some(self.index as u32 + 1);
                        self.phase = WavePhase::Spawning {
                            queue: wave_queue(spec),
                            t: 0.0,
                        };
                    } else {
                        self.phase = WavePhase::Won;
                        out.won = true;
                    }
                }
            }
            WavePhase::Spawning { queue, t } => {
                if queue.is_empty() {
                    // Checked one tick after the last pop so `alive` includes it.
                    self.phase = WavePhase::Clearing;
                } else {
                    *t -= dt;
                    if *t <= 0.0 && alive < tuning::MAX_ENEMIES {
                        out.spawn = queue.pop();
                        *t = self.waves[self.index].spawn_interval;
                    }
                }
            }
            WavePhase::Clearing => {
                if alive == 0 {
                    out.cleared = Some(self.index as u32 + 1);
                    if self.index + 1 >= self.waves.len() {
                        self.phase = WavePhase::Won;
                        out.won = true;
                    } else {
                        self.index += 1;
                        self.phase = WavePhase::Waiting {
                            t: tuning::WAVE_BREAK,
                        };
                    }
                }
            }
            WavePhase::Won => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_reports_kill_exactly_once() {
        let mut hp = 2;
        assert_eq!(damage_hp(&mut hp, 1), HitResult::Hurt);
        assert_eq!(damage_hp(&mut hp, 1), HitResult::Killed);
        assert_eq!(damage_hp(&mut hp, 1), HitResult::Ignored);
        assert_eq!(hp, 0);
    }

    #[test]
    fn overkill_is_still_one_kill() {
        let mut hp = tuning::RUSHER.hp;
        assert_eq!(damage_hp(&mut hp, 99), HitResult::Killed);
        assert!(hp <= 0);
    }

    #[test]
    fn player_iframes_block_followup_hits() {
        let mut v = Vitals::full();
        assert_eq!(v.take_hit(10.0), HitResult::Hurt);
        assert_eq!(v.hp, tuning::PLAYER_MAX_HP - 10.0);
        // Still invulnerable
        assert_eq!(v.take_hit(10.0), HitResult::Ignored);
        v.tick(tuning::PLAYER_INVULN_SECS + 0.01);
        assert_eq!(v.take_hit(10.0), HitResult::Hurt);
        assert_eq!(v.hp, tuning::PLAYER_MAX_HP - 20.0);
    }

    #[test]
    fn player_dies_once() {
        let mut v = Vitals {
            hp: 5.0,
            invuln: 0.0,
        };
        assert_eq!(v.take_hit(22.0), HitResult::Killed);
        assert_eq!(v.hp, 0.0);
        assert!(!v.alive());
        assert_eq!(v.take_hit(22.0), HitResult::Ignored);
    }

    #[test]
    fn separation_pushes_apart_and_ignores_far() {
        let p = separation(Vec2::ZERO, 0.45, [(Vec2::new(0.5, 0.0), 0.45)]);
        assert!(p.x < 0.0, "should push away (-x), got {p:?}");
        let far = separation(Vec2::ZERO, 0.45, [(Vec2::new(5.0, 0.0), 0.45)]);
        assert_eq!(far, Vec2::ZERO);
    }

    #[test]
    fn segment_distance_hits_midpoint() {
        let d = segment_distance(
            Vec2::new(-1.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 0.3),
        );
        assert!((d - 0.3).abs() < 1e-5);
    }

    #[test]
    fn nearest_in_range_respects_range() {
        let pts = [
            Vec2::new(3.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(30.0, 0.0),
        ];
        assert_eq!(
            nearest_in_range(Vec2::ZERO, 15.0, pts),
            Some(Vec2::new(1.0, 1.0))
        );
        assert_eq!(nearest_in_range(Vec2::ZERO, 0.5, pts), None);
    }

    #[test]
    fn wave_queue_counts_and_mix() {
        let spec = WaveSpec {
            rushers: 10,
            brutes: 3,
            spawn_interval: 0.5,
        };
        let q = wave_queue(&spec);
        assert_eq!(q.len(), 13);
        assert_eq!(q.iter().filter(|k| **k == EnemyKind::Brute).count(), 3);
        // Popped from the back: the wave opens with rushers and brutes are spread
        // out (never two in a row) rather than bunched together.
        assert_eq!(*q.last().unwrap(), EnemyKind::Rusher);
        assert!(
            !q.windows(2)
                .any(|w| w == [EnemyKind::Brute, EnemyKind::Brute])
        );
    }

    #[test]
    fn waves_escalate() {
        for w in tuning::WAVES.windows(2) {
            assert!(w[1].rushers + w[1].brutes > w[0].rushers + w[0].brutes);
        }
    }

    /// Runs the director to completion, killing everything as soon as it spawns.
    #[test]
    fn director_runs_all_waves_then_wins() {
        static WAVES: &[WaveSpec] = &[
            WaveSpec {
                rushers: 2,
                brutes: 0,
                spawn_interval: 0.1,
            },
            WaveSpec {
                rushers: 1,
                brutes: 1,
                spawn_interval: 0.1,
            },
        ];
        let mut d = WaveDirector::new(WAVES);
        let (mut spawned, mut started, mut cleared, mut won) = (vec![], vec![], vec![], 0);
        let mut alive = 0usize;
        for _ in 0..10_000 {
            let t = d.tick(0.05, alive);
            if let Some(k) = t.spawn {
                spawned.push(k);
                alive += 1;
            } else {
                alive = 0; // player kills instantly
            }
            started.extend(t.started);
            cleared.extend(t.cleared);
            won += t.won as u32;
        }
        assert_eq!(spawned.len(), 4);
        assert_eq!(started, vec![1, 2]);
        assert_eq!(cleared, vec![1, 2]);
        assert_eq!(won, 1, "won must be reported exactly once");
        assert_eq!(d.phase, WavePhase::Won);
    }

    #[test]
    fn director_waits_for_clear_and_respects_cap() {
        static WAVES: &[WaveSpec] = &[WaveSpec {
            rushers: 1,
            brutes: 0,
            spawn_interval: 0.1,
        }];
        let mut d = WaveDirector::new(WAVES);
        // At the cap nothing spawns.
        let mut got = None;
        for _ in 0..200 {
            got = got.or(d.tick(0.05, tuning::MAX_ENEMIES).spawn);
        }
        assert!(got.is_none());
        // Below the cap it spawns, then never clears while alive > 0.
        let mut spawned = false;
        for _ in 0..200 {
            spawned |= d.tick(0.05, 1).spawn.is_some();
            assert!(!d.tick(0.0, 1).won);
        }
        assert!(spawned);
        assert_eq!(d.phase, WavePhase::Clearing);
        assert!(d.tick(0.05, 0).won);
    }
}
