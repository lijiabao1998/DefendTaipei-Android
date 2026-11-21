//! Minimal survival loop: hostiles flood in from the edges of the view in waves,
//! the player auto-fires at the nearest one in range, contact drains HP.

use bevy::prelude::*;

use crate::city::Obstacles;
use crate::player::{PLAYER_RADIUS, Player, resolve_collisions, spawn_point};

const ENEMY_RADIUS: f32 = 0.45;
const FIRE_RANGE: f32 = 15.0;
const FIRE_INTERVAL: f32 = 0.22;
const BULLET_SPEED: f32 = 45.0;

#[derive(Resource)]
pub struct GameState {
    pub hp: f32,
    pub kills: u32,
    pub over: bool,
    pub survived: f32,
    fire_timer: f32,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            hp: 100.0,
            kills: 0,
            over: false,
            survived: 0.0,
            fire_timer: 0.0,
        }
    }
}

/// Occupation units the original actually places: street patrols (`spawnSoldiers`),
/// the night half of that set, district garrisons, and the four armor models on main roads.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnemyKind {
    Patrol,
    NightPatrol,
    Garrison,
    Zbl08,
    Zbd05,
    Mengshi,
    Ztd05,
}

/// Ask combat to place one hostile. Game flow (and later the map) sends this.
#[derive(Message, Clone, Copy, Debug)]
pub struct SpawnEnemy {
    pub pos: Vec3,
    pub kind: EnemyKind,
}

/// A hostile reached 0 HP. `kind` is the unit that died.
#[allow(dead_code)]
#[derive(Message, Clone, Copy, Debug)]
pub struct EnemyDied {
    pub kind: EnemyKind,
    pub pos: Vec3,
}

/// The player's HP reached 0.
#[derive(Message, Clone, Copy, Debug)]
pub struct PlayerDied;

/// Clear enemies and bullets and restore the player. Fired on a new run.
#[derive(Message, Clone, Copy, Debug)]
pub struct ResetCombat;

#[derive(Component)]
pub struct Enemy {
    hp: i32,
    speed: f32,
    pub kind: EnemyKind,
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
            .add_message::<SpawnEnemy>()
            .add_message::<EnemyDied>()
            .add_message::<PlayerDied>()
            .add_message::<ResetCombat>()
            .add_systems(Startup, load_assets)
            .add_systems(
                Update,
                (
                    apply_spawns,
                    move_enemies,
                    auto_fire,
                    move_bullets,
                    apply_reset,
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

fn stats_for(kind: EnemyKind) -> (i32, f32) {
    match kind {
        EnemyKind::Patrol => (3, 3.4),
        EnemyKind::NightPatrol => (3, 3.8),
        EnemyKind::Garrison => (6, 2.2),
        EnemyKind::Zbl08 | EnemyKind::Zbd05 | EnemyKind::Mengshi | EnemyKind::Ztd05 => (12, 2.0),
    }
}

/// Placeholder body for [`SpawnEnemy`]. Real soldiers and armor belong to combat.
fn apply_spawns(
    mut commands: Commands,
    assets: Res<CombatAssets>,
    mut spawns: MessageReader<SpawnEnemy>,
) {
    for spawn in spawns.read() {
        let (hp, speed) = stats_for(spawn.kind);
        commands.spawn((
            Enemy {
                hp,
                speed,
                kind: spawn.kind,
            },
            Mesh3d(assets.enemy_mesh.clone()),
            MeshMaterial3d(assets.enemy_mat.clone()),
            Transform::from_translation(spawn.pos),
        ));
    }
}

fn move_enemies(
    time: Res<Time>,
    obstacles: Res<Obstacles>,
    mut state: ResMut<GameState>,
    mut deaths: MessageWriter<PlayerDied>,
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
    state.hp = (state.hp - damage).max(0.0);
    if state.hp <= 0.0 && !state.over {
        state.over = true;
        deaths.write(PlayerDied);
    }
}

fn auto_fire(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<CombatAssets>,
    mut state: ResMut<GameState>,
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
    mut died: MessageWriter<EnemyDied>,
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
                        died.write(EnemyDied {
                            kind: enemy.kind,
                            pos: et.translation,
                        });
                        commands.entity(ee).despawn();
                        state.kills += 1;
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

fn apply_reset(
    mut commands: Commands,
    mut resets: MessageReader<ResetCombat>,
    mut state: ResMut<GameState>,
    mut player: Single<&mut Transform, With<Player>>,
    enemies: Query<Entity, With<Enemy>>,
    bullets: Query<Entity, With<Bullet>>,
) {
    if resets.is_empty() {
        return;
    }
    resets.clear();
    for entity in enemies.iter().chain(bullets.iter()) {
        commands.entity(entity).despawn();
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
