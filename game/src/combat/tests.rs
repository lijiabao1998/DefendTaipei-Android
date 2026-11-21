//! ECS-level tests: combat systems in a headless `App` (no window / renderer).

use std::time::Duration;

use bevy::ecs::message::{MessageCursor, Messages};

use super::*;

fn test_app() -> App {
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<GameState>()
        .init_resource::<WeaponInput>()
        .init_resource::<AutoFire>()
        .init_resource::<CombatEnv>()
        .init_resource::<PlayerStance>()
        .init_resource::<CombatWorldRes>()
        .init_resource::<PlayerGun>()
        .init_resource::<AiTimers>()
        .init_resource::<CombatRng>()
        .add_message::<SpawnEnemy>()
        .add_message::<EnemyDied>()
        .add_message::<PlayerDied>()
        .add_message::<PlayerHit>()
        .add_message::<Gunshot>()
        .add_message::<Callout>()
        .add_message::<ResetCombat>();
    app.world_mut()
        .spawn((Player, Transform::default(), Visibility::default()));
    app
}

fn step(app: &mut App, secs: f32) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(secs));
    app.update();
}

fn drain<M: Message + Clone>(app: &App, cursor: &mut MessageCursor<M>) -> Vec<M> {
    let msgs = app.world().resource::<Messages<M>>();
    cursor.read(msgs).cloned().collect()
}

fn soldier_at(app: &mut App, id: u32, pos: Vec2) -> Entity {
    let mut s = Soldier::new(id, EnemyKind::Patrol, pos, vec![]);
    // face the player at the origin
    s.dir = (-pos.y).atan2(-pos.x);
    app.world_mut()
        .spawn((Enemy(s), Transform::from_xyz(pos.x, 0.0, pos.y)))
        .id()
}

#[test]
fn player_shot_kills_soldier_once() {
    let mut app = test_app();
    app.add_systems(Update, player_fire);
    let e = soldier_at(&mut app, 0, Vec2::new(10.0, 0.0));
    {
        let mut en = app.world_mut().get_mut::<Enemy>(e).unwrap();
        en.0.hp = 10.0;
        en.0.state = SolState::Combat;
    }
    let mut died = MessageCursor::<EnemyDied>::default();
    let mut shots = MessageCursor::<Gunshot>::default();
    let (mut n, mut n_shots) = (0, 0);
    for _ in 0..10 {
        step(&mut app, 0.2);
        n += drain(&app, &mut died).len();
        n_shots += drain(&app, &mut shots).len();
    }
    assert_eq!(n, 1, "EnemyDied exactly once");
    assert_eq!(n_shots, 1, "auto-fire stops once the target is down");
    assert_eq!(app.world().resource::<GameState>().kills, 1);
    let s = &app.world().get::<Enemy>(e).unwrap().0;
    assert_eq!(s.state, SolState::Dead);
    assert!(app.world().get::<Body>(e).is_some());
}

#[test]
fn non_lethal_shot_provokes_soldier() {
    let mut app = test_app();
    app.add_systems(Update, player_fire);
    let e = soldier_at(&mut app, 0, Vec2::new(10.0, 0.0));
    // holding 打 (fire without aim): nearest soldier in sight, even an unaware one
    app.world_mut().resource_mut::<WeaponInput>().fire = true;
    step(&mut app, 0.01);
    let s = &app.world().get::<Enemy>(e).unwrap().0;
    let rifle = gun("rifle").unwrap();
    assert!(
        s.hp <= SOLDIER_HP - rifle.dmg * 0.8 + 1e-3 && s.hp >= SOLDIER_HP - rifle.dmg * 1.2 - 1e-3
    );
    assert_eq!(s.state, SolState::Combat);
    assert_eq!(s.last_seen, Some(Vec2::ZERO));
}

#[test]
fn auto_fire_ignores_far_or_unaware_soldiers() {
    let mut app = test_app();
    app.add_systems(Update, player_fire);
    let far = soldier_at(&mut app, 0, Vec2::new(30.0, 0.0));
    app.world_mut().get_mut::<Enemy>(far).unwrap().0.state = SolState::Combat;
    let unaware = soldier_at(&mut app, 1, Vec2::new(0.0, 10.0));
    let mut shots = MessageCursor::<Gunshot>::default();
    step(&mut app, 0.2);
    assert!(drain(&app, &mut shots).is_empty());
    assert_eq!(app.world().get::<Enemy>(far).unwrap().0.hp, SOLDIER_HP);
    assert_eq!(app.world().get::<Enemy>(unaware).unwrap().0.hp, SOLDIER_HP);
}

#[test]
fn soldiers_kill_player_and_player_died_once() {
    let mut app = test_app();
    app.add_systems(Update, (soldiers_think, player_vitals).chain());
    app.world_mut().resource_mut::<GameState>().hp = 25.0;
    for i in 0..3 {
        let e = soldier_at(&mut app, i, Vec2::new(6.0, i as f32 * 2.0 - 2.0));
        let mut en = app.world_mut().get_mut::<Enemy>(e).unwrap();
        en.0.state = SolState::Combat;
        en.0.alert = 1.0;
    }
    let mut died = MessageCursor::<PlayerDied>::default();
    let mut hits = MessageCursor::<PlayerHit>::default();
    let (mut n_died, mut n_hits) = (0, 0);
    for _ in 0..600 {
        step(&mut app, 0.1);
        n_died += drain(&app, &mut died).len();
        n_hits += drain(&app, &mut hits).len();
    }
    assert_eq!(n_died, 1, "PlayerDied exactly once");
    assert!(n_hits >= 1);
    let st = app.world().resource::<GameState>();
    assert!(st.over);
    assert_eq!(st.hp, 0.0);
}

#[test]
fn bleeding_drains_hp() {
    let mut app = test_app();
    app.add_systems(Update, player_vitals);
    app.world_mut().resource_mut::<GameState>().bleed = true;
    step(&mut app, 10.0);
    let hp = app.world().resource::<GameState>().hp;
    assert!((hp - (PLAYER_MAX_HP - 10.0 * BLEED_PER_SEC)).abs() < 1e-3);
}

#[test]
fn spawn_message_spawns_soldier() {
    let mut app = test_app();
    app.add_systems(Update, handle_spawn_requests);
    app.world_mut().write_message(SpawnEnemy {
        pos: Vec3::new(4.0, 0.0, -3.0),
        kind: EnemyKind::Garrison,
    });
    step(&mut app, 0.016);
    let w = app.world_mut();
    let list: Vec<_> = w
        .query::<&Enemy>()
        .iter(w)
        .map(|e| (e.0.kind, e.0.pos, e.0.post))
        .collect();
    assert_eq!(
        list,
        vec![(
            EnemyKind::Garrison,
            Vec2::new(4.0, -3.0),
            Some(Vec2::new(4.0, -3.0))
        )]
    );
}

#[test]
fn armor_spawn_is_ignored_until_ported() {
    let mut app = test_app();
    app.add_systems(Update, handle_spawn_requests);
    for kind in [
        EnemyKind::Zbl08,
        EnemyKind::Zbd05,
        EnemyKind::Mengshi,
        EnemyKind::Ztd05,
    ] {
        app.world_mut().write_message(SpawnEnemy {
            pos: Vec3::ZERO,
            kind,
        });
    }
    step(&mut app, 0.016);
    let w = app.world_mut();
    assert_eq!(w.query::<&Enemy>().iter(w).count(), 0);
}

#[test]
fn shots_become_gunshot_sfx_with_distance() {
    use crate::audio::{GunKind, SfxEvent};
    let mut app = test_app();
    app.add_message::<SfxEvent>();
    app.add_systems(Update, super::sfx::combat_sfx);
    app.world_mut().write_message(Gunshot {
        pos: Vec3::new(30.0, 1.4, 40.0),
        by_player: false,
        fam: super::tuning::GunFam::Lmg,
    });
    step(&mut app, 0.016);
    let msgs = app.world().resource::<Messages<SfxEvent>>();
    let got: Vec<_> = msgs.iter_current_update_messages().copied().collect();
    assert_eq!(got.len(), 1);
    match got[0] {
        SfxEvent::Gunshot { kind, dist, indoor } => {
            assert_eq!(kind, GunKind::Mg);
            assert!(!indoor);
            assert!((dist - 50.0).abs() < 0.01, "dist {dist}");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn reset_clears_field_and_restores_player() {
    let mut app = test_app();
    app.add_systems(Update, apply_reset);
    soldier_at(&mut app, 0, Vec2::ONE);
    soldier_at(&mut app, 1, Vec2::NEG_ONE);
    {
        let mut s = app.world_mut().resource_mut::<GameState>();
        s.hp = 0.0;
        s.over = true;
        s.bleed = true;
        s.kills = 4;
    }
    app.world_mut().write_message(ResetCombat);
    step(&mut app, 0.016);
    let w = app.world_mut();
    assert_eq!(w.query_filtered::<(), With<Enemy>>().iter(w).count(), 0);
    let s = w.resource::<GameState>();
    assert_eq!(s.hp, PLAYER_MAX_HP);
    assert!(!s.over && !s.bleed);
    assert_eq!(s.kills, 0);
    let p = w
        .query_filtered::<&Transform, With<Player>>()
        .single(w)
        .unwrap();
    assert_eq!(p.translation, spawn_point());
}

#[test]
fn squads_assign_roles_and_flank_callouts() {
    let mut app = test_app();
    app.add_systems(Update, assign_squads);
    for i in 0..3 {
        let e = soldier_at(&mut app, i, Vec2::new(10.0 + i as f32, 0.0));
        app.world_mut().get_mut::<Enemy>(e).unwrap().0.state = SolState::Combat;
    }
    let mut calls = MessageCursor::<Callout>::default();
    step(&mut app, 0.016);
    let w = app.world_mut();
    let mut roles: Vec<_> = w
        .query::<&Enemy>()
        .iter(w)
        .map(|e| (e.0.id, e.0.role))
        .collect();
    roles.sort_by_key(|r| r.0);
    use squad::Role::*;
    assert_eq!(roles, vec![(0, Suppress), (1, FlankL), (2, FlankR)]);
    let texts: Vec<_> = drain(&app, &mut calls)
        .into_iter()
        .map(|c| c.text)
        .collect();
    assert_eq!(texts, vec!["左翼包抄！", "右翼包抄！"]);
}
