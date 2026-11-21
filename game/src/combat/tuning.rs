//! Every combat tuning number lives here so balance passes touch one file.

use super::EnemyKind;

// ── Player ──────────────────────────────────────────────────────────────
/// Max HP. The HUD bar divides by 100, so keep this at 100 until the HUD reads it.
pub const PLAYER_MAX_HP: f32 = 100.0;
/// Invulnerability window after taking a hit (seconds).
pub const PLAYER_INVULN_SECS: f32 = 0.8;
/// Blink rate of the player model while invulnerable.
pub const PLAYER_BLINK_HZ: f32 = 12.0;
/// Extra reach beyond the two radii at which an enemy counts as touching.
pub const CONTACT_SLOP: f32 = 0.15;

// ── Weapon (rifle) ──────────────────────────────────────────────────────
pub const MUZZLE_HEIGHT: f32 = 1.05;
pub const FIRE_INTERVAL: f32 = 0.2;
pub const FIRE_RANGE: f32 = 15.0;
pub const BULLET_DAMAGE: i32 = 1;
pub const BULLET_SPEED: f32 = 45.0;
pub const BULLET_RADIUS: f32 = 0.14;
/// Hard cap on live projectiles (mobile budget).
pub const MAX_BULLETS: usize = 48;

// ── Enemies ─────────────────────────────────────────────────────────────
/// Hard cap on live enemies (mobile budget); the wave director waits when full.
pub const MAX_ENEMIES: usize = 40;
pub const HIT_FLASH_SECS: f32 = 0.08;
pub const DEATH_FX_SECS: f32 = 0.25;
/// Push strength of the anti-stacking separation steer.
pub const SEPARATION_STRENGTH: f32 = 1.6;
/// Enemies spawn on a ring this far from the player (just outside the view).
pub const SPAWN_RING_MIN: f32 = 26.0;
pub const SPAWN_RING_MAX: f32 = 34.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnemyStats {
    pub hp: i32,
    pub speed: f32,
    pub radius: f32,
    pub height: f32,
    pub contact_damage: f32,
    pub color: [f32; 3],
}

pub const RUSHER: EnemyStats = EnemyStats {
    hp: 2,
    speed: 4.4,
    radius: 0.45,
    height: 0.9,
    contact_damage: 10.0,
    color: [0.62, 0.14, 0.12],
};

pub const BRUTE: EnemyStats = EnemyStats {
    hp: 7,
    speed: 2.5,
    radius: 0.65,
    height: 1.1,
    contact_damage: 22.0,
    color: [0.30, 0.22, 0.18],
};

pub const fn stats(kind: EnemyKind) -> EnemyStats {
    match kind {
        EnemyKind::Rusher => RUSHER,
        EnemyKind::Brute => BRUTE,
    }
}

// ── Waves ───────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaveSpec {
    pub rushers: u32,
    pub brutes: u32,
    /// Seconds between individual spawns in this wave.
    pub spawn_interval: f32,
}

pub const WAVES: &[WaveSpec] = &[
    WaveSpec {
        rushers: 6,
        brutes: 0,
        spawn_interval: 1.0,
    },
    WaveSpec {
        rushers: 10,
        brutes: 1,
        spawn_interval: 0.8,
    },
    WaveSpec {
        rushers: 12,
        brutes: 3,
        spawn_interval: 0.65,
    },
    WaveSpec {
        rushers: 16,
        brutes: 5,
        spawn_interval: 0.5,
    },
    WaveSpec {
        rushers: 20,
        brutes: 8,
        spawn_interval: 0.4,
    },
];
/// Delay before wave 1 starts.
pub const FIRST_WAVE_DELAY: f32 = 2.0;
/// Break between a cleared wave and the next one.
pub const WAVE_BREAK: f32 = 5.0;
