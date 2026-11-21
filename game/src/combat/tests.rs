//! ECS-level tests: run combat systems in a headless `App` (no window / renderer)
//! and check the messages they write.

use std::time::Duration;

use bevy::ecs::message::{MessageCursor, Messages};
use bevy::state::app::StatesPlugin;

use super::*;
use crate::flow::{FlowPlugin, GameFlow, RestartGame};

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(StatesPlugin)
        .insert_state(GameFlow::Playing)
        .init_resource::<Time>()
        .init_resource::<Obstacles>()
        .init_resource::<GameState>()
        .init_resource::<Waves>()
        .init_resource::<Weapon>()
        .init_resource::<CombatRng>()
        .add_message::<SpawnEnemy>()
        .add_message::<EnemyDied>()
        .add_message::<PlayerDied>()
        .add_message::<WaveStarted>()
        .add_message::<WaveCleared>()
        .add_message::<ResetCombat>();
    app.world_mut().spawn((
        Player,
        Transform::from_xyz(0.0, 0.0, 0.0),
        Visibility::default(),
    ));
    app
}

fn step(app: &mut App, secs: f32) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(secs));
    app.update();
}

/// Counts new messages of type `M` since the cursor was last read.
fn drain<M: Message + Clone>(app: &App, cursor: &mut MessageCursor<M>) -> Vec<M> {
    let msgs = app.world().resource::<Messages<M>>();
    cursor.read(msgs).cloned().collect()
}

fn enemy(kind: EnemyKind, hp: i32, at: Vec2) -> impl Bundle {
    (
        Enemy {
            kind,
            hp,
            flash: 0.0,
        },
        Transform::from_xyz(at.x, 1.0, at.y),
    )
}

fn bullet_at(at: Vec2) -> impl Bundle {
    (
        Bullet {
            vel: Vec3::X,
            life: 1.0,
        },
        Transform::from_xyz(at.x, MUZZLE_HEIGHT, at.y),
    )
}

#[test]
fn killing_bullet_writes_enemy_died_once_and_counts_kill() {
    let mut app = test_app();
    app.add_systems(Update, move_bullets);
    let pos = Vec2::new(5.0, 5.0);
    let e = app.world_mut().spawn(enemy(EnemyKind::Rusher, 1, pos)).id();
    // Two bullets on the same enemy in one frame: only one kill.
    app.world_mut().spawn(bullet_at(pos));
    app.world_mut().spawn(bullet_at(pos));
    let mut c = MessageCursor::<EnemyDied>::default();

    step(&mut app, 0.0);
    let died = drain(&app, &mut c);
    assert_eq!(died.len(), 1);
    assert_eq!(died[0].entity, e);
    assert_eq!(died[0].kind, EnemyKind::Rusher);
    assert_eq!(app.world().resource::<GameState>().kills, 1);
    // Enemy is now dying (no longer an `Enemy`), bullets are gone.
    assert!(app.world().get::<Enemy>(e).is_none());
    assert!(app.world().get::<Dying>(e).is_some());

    step(&mut app, 0.0);
    assert!(drain(&app, &mut c).is_empty());
}

#[test]
fn non_lethal_hit_flashes_without_message() {
    let mut app = test_app();
    app.add_systems(Update, move_bullets);
    let pos = Vec2::new(-3.0, 2.0);
    let e = app
        .world_mut()
        .spawn(enemy(EnemyKind::Brute, BRUTE.hp, pos))
        .id();
    app.world_mut().spawn(bullet_at(pos));
    let mut c = MessageCursor::<EnemyDied>::default();
    step(&mut app, 0.0);
    assert!(drain(&app, &mut c).is_empty());
    let en = app.world().get::<Enemy>(e).unwrap();
    assert_eq!(en.hp, BRUTE.hp - BULLET_DAMAGE);
    assert!(en.flash > 0.0);
}

#[test]
fn contact_kills_player_writes_player_died_once_and_enters_lost() {
    let mut app = test_app();
    app.add_systems(Update, contact_damage);
    app.world_mut().resource_mut::<GameState>().hp = 5.0;
    app.world_mut()
        .spawn(enemy(EnemyKind::Brute, BRUTE.hp, Vec2::new(0.3, 0.0)));
    let mut died = MessageCursor::<PlayerDied>::default();

    let mut n_died = 0;
    for _ in 0..5 {
        step(&mut app, 0.1);
        n_died += drain(&app, &mut died).len();
    }
    assert_eq!(n_died, 1);
    assert_eq!(
        *app.world().resource::<State<GameFlow>>().get(),
        GameFlow::Lost
    );
    let s = app.world().resource::<GameState>();
    assert!(s.over);
    assert_eq!(s.hp, 0.0);
}

#[test]
fn contact_respects_invulnerability() {
    let mut app = test_app();
    app.add_systems(Update, contact_damage);
    app.world_mut()
        .spawn(enemy(EnemyKind::Rusher, RUSHER.hp, Vec2::new(0.3, 0.0)));
    step(&mut app, 0.01);
    let hp1 = app.world().resource::<GameState>().hp;
    assert_eq!(hp1, PLAYER_MAX_HP - RUSHER.contact_damage);
    // Within the i-frame window: no more damage.
    step(&mut app, PLAYER_INVULN_SECS * 0.5);
    assert_eq!(app.world().resource::<GameState>().hp, hp1);
    // After it: hit again.
    step(&mut app, PLAYER_INVULN_SECS);
    assert_eq!(
        app.world().resource::<GameState>().hp,
        PLAYER_MAX_HP - 2.0 * RUSHER.contact_damage
    );
}

#[test]
fn waves_run_to_won_state() {
    let mut app = test_app();
    // Nobody actually spawns the requested enemies, so every wave clears at once.
    app.add_systems(Update, run_waves);
    let mut started = MessageCursor::<WaveStarted>::default();
    let mut spawns = MessageCursor::<SpawnEnemy>::default();
    let (mut n_started, mut n_spawn) = (vec![], 0);
    for _ in 0..3000 {
        step(&mut app, 0.1);
        n_started.extend(drain(&app, &mut started).into_iter().map(|w| w.wave));
        n_spawn += drain(&app, &mut spawns).len();
    }
    let expected: u32 = WAVES.iter().map(|w| w.rushers + w.brutes).sum();
    assert_eq!(n_started, (1..=WAVES.len() as u32).collect::<Vec<_>>());
    assert_eq!(n_spawn as u32, expected);
    assert_eq!(
        *app.world().resource::<State<GameFlow>>().get(),
        GameFlow::Won
    );
    let s = app.world().resource::<GameState>();
    assert!(s.won && !s.over);
    assert_eq!(s.wave, WAVES.len() as u32);
}

#[test]
fn reset_clears_field_and_restores_player() {
    let mut app = test_app();
    app.add_systems(Update, apply_reset);
    app.world_mut()
        .spawn(enemy(EnemyKind::Rusher, 2, Vec2::ONE));
    app.world_mut()
        .spawn(enemy(EnemyKind::Brute, 7, Vec2::NEG_ONE));
    app.world_mut().spawn(bullet_at(Vec2::ZERO));
    {
        let mut s = app.world_mut().resource_mut::<GameState>();
        s.hp = 0.0;
        s.over = true;
        s.kills = 12;
        s.wave = 3;
    }
    app.world_mut().write_message(ResetCombat);
    step(&mut app, 0.016);

    let w = app.world_mut();
    assert_eq!(w.query_filtered::<(), With<Enemy>>().iter(w).count(), 0);
    assert_eq!(w.query_filtered::<(), With<Bullet>>().iter(w).count(), 0);
    let s = w.resource::<GameState>();
    assert_eq!(s.hp, PLAYER_MAX_HP);
    assert!(!s.over && !s.won);
    assert_eq!((s.kills, s.wave), (0, 1));
    let p = w
        .query_filtered::<&Transform, With<Player>>()
        .single(w)
        .unwrap();
    assert_eq!(p.translation, spawn_point());
}

#[test]
fn flow_title_auto_starts_then_restarts_after_loss() {
    let mut app = App::new();
    app.add_plugins((StatesPlugin, FlowPlugin))
        .init_resource::<Time>()
        .add_message::<ResetCombat>();
    let mut resets = MessageCursor::<ResetCombat>::default();
    let flow = |app: &App| *app.world().resource::<State<GameFlow>>().get();
    assert_eq!(flow(&app), GameFlow::Title);
    step(&mut app, 0.1);
    step(&mut app, 0.1);
    assert_eq!(
        flow(&app),
        GameFlow::Playing,
        "AUTO_FLOW_TEMP skips the title"
    );

    app.world_mut()
        .resource_mut::<NextState<GameFlow>>()
        .set(GameFlow::Lost);
    step(&mut app, 0.0);
    assert_eq!(flow(&app), GameFlow::Lost);
    let mut t = 0.0;
    while flow(&app) == GameFlow::Lost {
        step(&mut app, 0.1);
        t += 0.1;
        assert!(t < 30.0, "should auto-restart");
    }
    assert_eq!(flow(&app), GameFlow::Playing);
    assert!(t >= crate::flow::AUTO_RESTART_SECS_TEMP - 0.11);
    assert_eq!(drain(&app, &mut resets).len(), 1);
}

#[test]
fn restart_button_resets_and_plays() {
    let mut app = App::new();
    app.add_plugins((StatesPlugin, FlowPlugin))
        .init_resource::<Time>()
        .add_message::<ResetCombat>();
    app.world_mut()
        .resource_mut::<NextState<GameFlow>>()
        .set(GameFlow::Won);
    step(&mut app, 0.0);
    let mut resets = MessageCursor::<ResetCombat>::default();
    app.world_mut().write_message(RestartGame);
    step(&mut app, 0.0);
    step(&mut app, 0.0);
    assert_eq!(drain(&app, &mut resets).len(), 1);
    assert_eq!(
        *app.world().resource::<State<GameFlow>>().get(),
        GameFlow::Playing
    );
}
