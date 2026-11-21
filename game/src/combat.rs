//! Minimal survival loop: hostiles flood in from the edges of the view in waves,
//! the player auto-fires at the nearest one in range, contact drains HP.

use bevy::prelude::*;

use crate::audio::SfxEvent;
use crate::city::{Lcg, MAP_HALF, Obstacles};
use crate::player::{PLAYER_RADIUS, Player, resolve_collisions, spawn_point};

const ENEMY_RADIUS: f32 = 0.45;
const FIRE_RANGE: f32 = 15.0;
const FIRE_INTERVAL: f32 = 0.22;
const BULLET_SPEED: f32 = 45.0;
const WAVE_SECONDS: f32 = 25.0;

#[derive(Resource)]
pub struct GameState {
    pub hp: f32,
    pub kills: u32,
    pub wave: u32,
    pub over: bool,
    pub survived: f32,
    wave_timer: f32,
    spawn_timer: f32,
    fire_timer: f32,
    rng: Lcg,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            hp: 100.0,
            kills: 0,
            wave: 1,
            over: false,
            survived: 0.0,
            wave_timer: 0.0,
            spawn_timer: 1.5,
            fire_timer: 0.0,
            rng: Lcg(0xdefe_17a1),
        }
    }
}

#[derive(Component)]
pub struct Enemy {
    hp: i32,
    speed: f32,
}

#[derive(Component)]
struct Bullet {
    vel: Vec3,
    life: f32,
}

#[derive(Resource)]
struct CombatAssets {
    enemy_mesh: Handle<Mesh>,
    enemy_mat: Handle<StandardMaterial>,
    bullet_mesh: Handle<Mesh>,
    bullet_mat: Handle<StandardMaterial>,
}

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameState>()
            .add_systems(Startup, load_assets)
            .add_systems(
                Update,
                (
                    tick_waves,
                    spawn_enemies,
                    move_enemies,
                    auto_fire,
                    move_bullets,
                    restart,
                )
                    .chain()
                    .after(crate::player::MoveSet),
            );
    }
}

fn load_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(CombatAssets {
        enemy_mesh: meshes.add(Capsule3d::new(ENEMY_RADIUS, 0.9)),
        enemy_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(0.62, 0.14, 0.12),
            perceptual_roughness: 0.7,
            ..default()
        }),
        bullet_mesh: meshes.add(Sphere::new(0.14)),
        bullet_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.9, 0.4),
            emissive: LinearRgba::rgb(4.0, 3.0, 0.8),
            unlit: true,
            ..default()
        }),
    });
}

fn tick_waves(time: Res<Time>, mut state: ResMut<GameState>) {
    if state.over {
        return;
    }
    let dt = time.delta_secs();
    state.survived += dt;
    state.wave_timer += dt;
    if state.wave_timer >= WAVE_SECONDS {
        state.wave_timer = 0.0;
        state.wave += 1;
    }
}

fn spawn_enemies(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<CombatAssets>,
    obstacles: Res<Obstacles>,
    mut state: ResMut<GameState>,
    player: Single<&Transform, With<Player>>,
    enemies: Query<(), With<Enemy>>,
) {
    if state.over {
        return;
    }
    state.spawn_timer -= time.delta_secs();
    if state.spawn_timer > 0.0 {
        return;
    }
    let wave = state.wave as f32;
    state.spawn_timer = (2.2 - wave * 0.25).max(0.45);
    let cap = 6 + state.wave as usize * 4;
    if enemies.iter().count() >= cap {
        return;
    }
    let burst = 1 + state.wave / 2;
    let p = player.translation.xz();
    for _ in 0..burst {
        // Try a few ring positions just outside the visible area that are on a street.
        for _ in 0..12 {
            let a = state.rng.range(0.0, std::f32::consts::TAU);
            let r = state.rng.range(26.0, 34.0);
            let c = p + Vec2::from_angle(a) * r;
            if c.abs().max_element() > MAP_HALF - 1.0
                || obstacles.0.iter().any(|o| o.contains(c, 0.8))
            {
                continue;
            }
            let speed = 3.2 + wave * 0.35 + state.rng.range(-0.4, 0.6);
            commands.spawn((
                Enemy {
                    hp: 2 + (state.wave as i32) / 3,
                    speed,
                },
                Mesh3d(assets.enemy_mesh.clone()),
                MeshMaterial3d(assets.enemy_mat.clone()),
                Transform::from_xyz(c.x, 0.9, c.y),
            ));
            break;
        }
    }
}

fn move_enemies(
    time: Res<Time>,
    obstacles: Res<Obstacles>,
    mut state: ResMut<GameState>,
    mut sfx: MessageWriter<SfxEvent>,
    player: Single<&Transform, (With<Player>, Without<Enemy>)>,
    mut enemies: Query<(&Enemy, &mut Transform)>,
) {
    if state.over {
        return;
    }
    let dt = time.delta_secs();
    let p = player.translation.xz();
    // Snapshot positions for a cheap separation pass so the horde does not stack.
    let snapshot: Vec<Vec2> = enemies.iter().map(|(_, t)| t.translation.xz()).collect();
    let mut damage = 0.0;
    for (enemy, mut t) in &mut enemies {
        let e = t.translation.xz();
        let to = p - e;
        let dist = to.length();
        if dist < PLAYER_RADIUS + ENEMY_RADIUS + 0.15 {
            damage += 10.0 * dt;
            continue;
        }
        let mut dir = to / dist.max(1e-4);
        for &o in &snapshot {
            let d = e - o;
            let l = d.length();
            if l > 1e-3 && l < 1.1 {
                dir += d / l * (1.1 - l) * 1.5;
            }
        }
        let next = e + dir.normalize_or_zero() * enemy.speed * dt;
        let next = resolve_collisions(next, ENEMY_RADIUS, &obstacles);
        t.translation.x = next.x;
        t.translation.z = next.y;
    }
    if damage > 0.0 {
        sfx.write(if state.hp - damage <= 0.0 {
            SfxEvent::PlayerDeath
        } else {
            SfxEvent::PlayerHurt
        });
    }
    state.hp = (state.hp - damage).max(0.0);
    if state.hp <= 0.0 {
        state.over = true;
    }
}

fn auto_fire(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<CombatAssets>,
    mut state: ResMut<GameState>,
    mut sfx: MessageWriter<SfxEvent>,
    player: Single<&Transform, With<Player>>,
    enemies: Query<&Transform, With<Enemy>>,
) {
    if state.over {
        return;
    }
    state.fire_timer -= time.delta_secs();
    if state.fire_timer > 0.0 {
        return;
    }
    let muzzle = player.translation + Vec3::Y * 1.05;
    let Some(target) = enemies
        .iter()
        .map(|t| t.translation)
        .filter(|t| t.xz().distance(muzzle.xz()) < FIRE_RANGE)
        .min_by(|a, b| {
            a.xz()
                .distance_squared(muzzle.xz())
                .total_cmp(&b.xz().distance_squared(muzzle.xz()))
        })
    else {
        return;
    };
    state.fire_timer = FIRE_INTERVAL;
    sfx.write(SfxEvent::Gunshot);
    let dir = (Vec3::new(target.x, muzzle.y, target.z) - muzzle).normalize_or_zero();
    commands.spawn((
        Bullet {
            vel: dir * BULLET_SPEED,
            life: FIRE_RANGE / BULLET_SPEED + 0.1,
        },
        Mesh3d(assets.bullet_mesh.clone()),
        MeshMaterial3d(assets.bullet_mat.clone()),
        Transform::from_translation(muzzle + dir * 0.6),
    ));
}

fn move_bullets(
    mut commands: Commands,
    time: Res<Time>,
    obstacles: Res<Obstacles>,
    mut state: ResMut<GameState>,
    mut sfx: MessageWriter<SfxEvent>,
    mut bullets: Query<(Entity, &mut Bullet, &mut Transform), Without<Enemy>>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform)>,
) {
    let dt = time.delta_secs();
    for (be, mut b, mut t) in &mut bullets {
        let prev = t.translation.xz();
        t.translation += b.vel * dt;
        b.life -= dt;
        let p = t.translation.xz();
        let mut dead = b.life <= 0.0 || obstacles.0.iter().any(|o| o.contains(p, 0.0));
        if !dead {
            for (ee, mut enemy, et) in &mut enemies {
                // Swept test so fast bullets cannot skip over a target on slow frames.
                if enemy.hp > 0
                    && segment_distance(prev, p, et.translation.xz()) < ENEMY_RADIUS + 0.2
                {
                    enemy.hp -= 1;
                    if enemy.hp <= 0 {
                        commands.entity(ee).despawn();
                        state.kills += 1;
                        sfx.write(SfxEvent::EnemyDeath);
                    }
                    dead = true;
                    break;
                }
            }
        }
        if dead {
            commands.entity(be).despawn();
        }
    }
}

fn restart(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    touches: Res<Touches>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<GameState>,
    mut player: Single<&mut Transform, With<Player>>,
    enemies: Query<Entity, With<Enemy>>,
    bullets: Query<Entity, With<Bullet>>,
) {
    if !state.over {
        return;
    }
    let pressed = keys.any_just_pressed([KeyCode::KeyR, KeyCode::Space, KeyCode::Enter])
        || touches.any_just_pressed()
        || mouse.just_pressed(MouseButton::Left);
    if !pressed {
        return;
    }
    for e in enemies.iter().chain(bullets.iter()) {
        commands.entity(e).despawn();
    }
    *state = GameState::default();
    player.translation = spawn_point();
}

/// Distance from `c` to the segment `a`–`b`.
fn segment_distance(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 1e-8 {
        ((c - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t).distance(c)
}
