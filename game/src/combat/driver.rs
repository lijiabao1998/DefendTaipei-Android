//! TEMPORARY population / restart driver — TODO(lijiabao5): replace with the game-flow
//! state machine, story and encounter placement, then delete this file.
//!
//! Only here so the build stays playable: it keeps a growing number of 解放軍 patrols
//! around the player (as the skeleton's waves did, shown as "Wave" on the HUD) and, after
//! the player dies, writes [`ResetCombat`] on tap / click / R or after a few seconds.
//! It uses nothing but the public API (`SpawnEnemy`, `ResetCombat`, `GameState`).

use bevy::prelude::*;

use super::{Enemy, EnemyKind, GameState, ResetCombat, SpawnEnemy};
use crate::city::{Lcg, MAP_HALF, Obstacles};
use crate::player::Player;

/// Set false to turn the temporary driver off (e.g. once game flow spawns encounters).
pub const TEMP_DRIVER_ENABLED: bool = true;
const STEP_SECS: f32 = 25.0;
const SPAWN_EVERY: f32 = 4.0;
const RING_MIN: f32 = 18.0;
const RING_MAX: f32 = 30.0;
const AUTO_RESTART_SECS: f32 = 5.0;
const RESTART_GRACE: f32 = 0.6;

#[derive(Resource)]
struct TempDriver {
    spawn_t: f32,
    over_t: f32,
    rng: Lcg,
}

impl Default for TempDriver {
    fn default() -> Self {
        Self {
            spawn_t: 0.5,
            over_t: 0.0,
            rng: Lcg(0x5eed_1949),
        }
    }
}

/// Alive soldiers wanted at escalation step `wave` (1-based).
pub fn target_population(wave: u32) -> usize {
    (2 + 2 * wave as usize).min(16)
}

pub struct TempDriverPlugin;

impl Plugin for TempDriverPlugin {
    fn build(&self, app: &mut App) {
        if !TEMP_DRIVER_ENABLED {
            return;
        }
        app.init_resource::<TempDriver>().add_systems(
            Update,
            (temp_spawner, temp_restart)
                .chain()
                .before(super::CombatSet)
                .after(super::CombatInputSet),
        );
    }
}

fn temp_spawner(
    time: Res<Time>,
    obstacles: Option<Res<Obstacles>>,
    mut state: ResMut<GameState>,
    mut d: ResMut<TempDriver>,
    player: Query<&Transform, With<Player>>,
    enemies: Query<&Enemy>,
    mut spawn: MessageWriter<SpawnEnemy>,
) {
    if state.over {
        return;
    }
    let wave = 1 + (state.survived / STEP_SECS) as u32;
    if state.wave != wave {
        state.wave = wave;
    }
    d.spawn_t -= time.delta_secs();
    if d.spawn_t > 0.0 {
        return;
    }
    d.spawn_t = SPAWN_EVERY;
    let alive = enemies.iter().filter(|e| e.0.alive()).count();
    if alive >= target_population(wave) {
        return;
    }
    let Ok(p) = player.single() else { return };
    let p = p.translation.xz();
    // a patrol of 2–3 on a street just outside the view
    for _ in 0..16 {
        let a = d.rng.range(0.0, std::f32::consts::TAU);
        let r = d.rng.range(RING_MIN, RING_MAX);
        let c = p + Vec2::from_angle(a) * r;
        let blocked = |q: Vec2| {
            q.abs().max_element() > MAP_HALF - 2.0
                || obstacles
                    .as_deref()
                    .is_some_and(|o| o.0.iter().any(|b| b.contains(q, 0.8)))
        };
        if blocked(c) {
            continue;
        }
        let n = 2 + (d.rng.next_f32() * 2.0) as usize;
        for i in 0..n {
            let q = c + Vec2::new(i as f32 * 1.2, (i % 2) as f32 * 1.2);
            let kind = if d.rng.next_f32() < 0.15 {
                EnemyKind::PlaSentry
            } else {
                EnemyKind::PlaSoldier
            };
            spawn.write(SpawnEnemy {
                pos: Vec3::new(q.x, 0.0, q.y),
                kind,
            });
        }
        break;
    }
}

fn temp_restart(
    time: Res<Time>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    touches: Option<Res<Touches>>,
    state: Res<GameState>,
    mut d: ResMut<TempDriver>,
    mut reset: MessageWriter<ResetCombat>,
) {
    if !state.over {
        d.over_t = 0.0;
        return;
    }
    d.over_t += time.delta_secs();
    let pressed = keys
        .is_some_and(|k| k.any_just_pressed([KeyCode::KeyR, KeyCode::Space, KeyCode::Enter]))
        || mouse.is_some_and(|m| m.just_pressed(MouseButton::Left))
        || touches.is_some_and(|t| t.any_just_pressed());
    if d.over_t >= AUTO_RESTART_SECS || (pressed && d.over_t > RESTART_GRACE) {
        d.over_t = 0.0;
        d.spawn_t = 0.5;
        reset.write(ResetCombat);
    }
}
