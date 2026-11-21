//! Combat: player health, rifle + projectiles, enemy spawning / AI / death,
//! wave pacing and win / lose.
//!
//! Public API for the rest of the game (see the PR description for details):
//! - Messages written by combat: [`EnemyDied`], [`PlayerDied`], [`WaveStarted`],
//!   [`WaveCleared`]. Combat also sets `GameFlow::Won` (all waves cleared) and
//!   `GameFlow::Lost` (player died); see `crate::flow`.
//! - Messages read by combat: [`SpawnEnemy`] (spawn one enemy), [`ResetCombat`]
//!   (clear the field, full HP, back to wave 1).
//! - [`spawn_enemy`] for direct spawning with `Commands`.
//! - [`CombatInput`] (aim / fire), written in [`CombatInputSet`].
//! - [`GameState`] (HP, kills, wave, over / won) for the HUD.
//! - All tuning numbers live in [`tuning`].

use bevy::prelude::*;

use crate::city::{Lcg, MAP_HALF, Obstacles};
use crate::flow::GameFlow;
use crate::player::{PLAYER_RADIUS, Player, resolve_collisions, spawn_point};

pub mod input;
pub mod logic;
pub mod tuning;

pub use input::{CombatInput, CombatInputSet};
use logic::{
    HitResult, Vitals, WaveDirector, damage_hp, nearest_in_range, segment_distance, separation,
};
use tuning::*;

// ── Public types ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnemyKind {
    /// Fast, fragile melee rusher.
    Rusher,
    /// Slow, tough, hits hard.
    Brute,
}

/// Combat / run state shown by the HUD.
#[derive(Resource, Debug, Clone)]
pub struct GameState {
    pub hp: f32,
    /// Seconds of post-hit invulnerability left.
    pub invuln: f32,
    pub kills: u32,
    /// 1-based current wave.
    pub wave: u32,
    pub total_waves: u32,
    /// Player is dead (lost).
    pub over: bool,
    /// All waves cleared (won).
    pub won: bool,
    pub survived: f32,
}

impl Default for GameState {
    fn default() -> Self {
        let v = Vitals::full();
        Self {
            hp: v.hp,
            invuln: v.invuln,
            kills: 0,
            wave: 1,
            total_waves: WAVES.len() as u32,
            over: false,
            won: false,
            survived: 0.0,
        }
    }
}

impl GameState {
    fn running(&self) -> bool {
        !self.over && !self.won
    }
}

#[derive(Component, Debug)]
pub struct Enemy {
    pub kind: EnemyKind,
    pub hp: i32,
    flash: f32,
}

#[derive(Component)]
pub struct Bullet {
    vel: Vec3,
    life: f32,
}

/// Enemy that was killed and is playing its short death shrink.
#[derive(Component)]
pub struct Dying {
    t: f32,
}

/// Shared meshes / materials: every enemy of a kind uses the same handles
/// so they batch cheaply on mobile.
#[derive(Resource, Clone)]
pub struct CombatAssets {
    pub rusher_mesh: Handle<Mesh>,
    pub rusher_mat: Handle<StandardMaterial>,
    pub brute_mesh: Handle<Mesh>,
    pub brute_mat: Handle<StandardMaterial>,
    pub flash_mat: Handle<StandardMaterial>,
    pub bullet_mesh: Handle<Mesh>,
    pub bullet_mat: Handle<StandardMaterial>,
}

impl CombatAssets {
    fn mesh(&self, kind: EnemyKind) -> Handle<Mesh> {
        match kind {
            EnemyKind::Rusher => self.rusher_mesh.clone(),
            EnemyKind::Brute => self.brute_mesh.clone(),
        }
    }
    fn mat(&self, kind: EnemyKind) -> Handle<StandardMaterial> {
        match kind {
            EnemyKind::Rusher => self.rusher_mat.clone(),
            EnemyKind::Brute => self.brute_mat.clone(),
        }
    }
}

// ── Messages ────────────────────────────────────────────────────────────

/// Request: spawn one enemy at a ground position (x, z). Capped at `MAX_ENEMIES`.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct SpawnEnemy {
    pub pos: Vec2,
    pub kind: EnemyKind,
}

/// An enemy was killed (written exactly once per enemy).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct EnemyDied {
    pub entity: Entity,
    pub kind: EnemyKind,
    pub pos: Vec2,
}

/// The player's HP reached 0 (written once per death).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct PlayerDied {
    pub pos: Vec2,
}

#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct WaveStarted {
    pub wave: u32,
    pub total: u32,
}

#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct WaveCleared {
    pub wave: u32,
    pub total: u32,
}

/// Request: despawn all enemies / bullets, restore player HP and position,
/// back to wave 1. `RestartGame` (flow) writes this; it is handled in every state.
#[derive(Message, Clone, Copy, Debug, Default, PartialEq)]
pub struct ResetCombat;

// ── Internal resources ──────────────────────────────────────────────────

#[derive(Resource, Default)]
struct Waves(WaveDirector);

#[derive(Resource, Default)]
struct Weapon {
    cooldown: f32,
}

#[derive(Resource)]
struct CombatRng(Lcg);

impl Default for CombatRng {
    fn default() -> Self {
        Self(Lcg(0xdefe_17a1))
    }
}

/// All combat gameplay systems (they run after player movement).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatSet;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameState>()
            .init_resource::<CombatInput>()
            .init_resource::<Waves>()
            .init_resource::<Weapon>()
            .init_resource::<CombatRng>()
            .add_message::<SpawnEnemy>()
            .add_message::<EnemyDied>()
            .add_message::<PlayerDied>()
            .add_message::<WaveStarted>()
            .add_message::<WaveCleared>()
            .add_message::<ResetCombat>()
            .add_systems(Startup, load_assets)
            .configure_sets(
                Update,
                (
                    CombatInputSet.after(crate::player::MoveSet),
                    CombatSet.after(CombatInputSet),
                ),
            )
            .add_systems(OnEnter(GameFlow::Won), mark_won)
            .add_systems(
                Update,
                (
                    apply_reset,
                    (
                        run_waves,
                        handle_spawn_requests,
                        move_enemies,
                        contact_damage,
                        fire_weapon,
                        move_bullets,
                    )
                        .chain()
                        .run_if(in_state(GameFlow::Playing)),
                    (tick_enemy_flash, tick_dying, blink_player),
                )
                    .chain()
                    .in_set(CombatSet),
            );
        #[cfg(not(target_os = "android"))]
        app.add_systems(Update, input::desktop_mouse_input.in_set(CombatInputSet));
    }
}

fn load_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut mat = |c: [f32; 3]| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(c[0], c[1], c[2]),
            perceptual_roughness: 0.7,
            ..default()
        })
    };
    let rusher_mat = mat(RUSHER.color);
    let brute_mat = mat(BRUTE.color);
    let flash_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        ..default()
    });
    let bullet_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.9, 0.4),
        emissive: LinearRgba::rgb(4.0, 3.0, 0.8),
        unlit: true,
        ..default()
    });
    commands.insert_resource(CombatAssets {
        rusher_mesh: meshes.add(Capsule3d::new(RUSHER.radius, RUSHER.height)),
        rusher_mat,
        brute_mesh: meshes.add(Capsule3d::new(BRUTE.radius, BRUTE.height)),
        brute_mat,
        flash_mat,
        bullet_mesh: meshes.add(Sphere::new(BULLET_RADIUS).mesh().ico(1).unwrap()),
        bullet_mat,
    });
}

/// Spawns one enemy of `kind` standing at ground position `pos` (x, z).
pub fn spawn_enemy(
    commands: &mut Commands,
    assets: &CombatAssets,
    pos: Vec2,
    kind: EnemyKind,
) -> Entity {
    let s = stats(kind);
    commands
        .spawn((
            Enemy {
                kind,
                hp: s.hp,
                flash: 0.0,
            },
            Mesh3d(assets.mesh(kind)),
            MeshMaterial3d(assets.mat(kind)),
            Transform::from_xyz(pos.x, s.height * 0.5 + s.radius, pos.y),
        ))
        .id()
}

// ── Systems ─────────────────────────────────────────────────────────────

fn apply_reset(
    mut commands: Commands,
    mut requests: MessageReader<ResetCombat>,
    mut state: ResMut<GameState>,
    mut waves: ResMut<Waves>,
    mut weapon: ResMut<Weapon>,
    mut player: Query<(&mut Transform, &mut Visibility), With<Player>>,
    field: Query<Entity, Or<(With<Enemy>, With<Bullet>, With<Dying>)>>,
) {
    if requests.read().count() == 0 {
        return;
    }
    for e in &field {
        commands.entity(e).despawn();
    }
    *state = GameState::default();
    *waves = Waves::default();
    *weapon = Weapon::default();
    for (mut t, mut vis) in &mut player {
        t.translation = spawn_point();
        *vis = Visibility::Inherited;
    }
}

/// Picks a street position on a ring just outside the view around the player.
fn find_spawn_pos(rng: &mut Lcg, around: Vec2, obstacles: &Obstacles) -> Vec2 {
    let mut fallback = around;
    for _ in 0..16 {
        let a = rng.range(0.0, std::f32::consts::TAU);
        let r = rng.range(SPAWN_RING_MIN, SPAWN_RING_MAX);
        let c = around + Vec2::from_angle(a) * r;
        fallback = c;
        if c.abs().max_element() > MAP_HALF - 1.0 || obstacles.0.iter().any(|o| o.contains(c, 0.8))
        {
            continue;
        }
        return c;
    }
    resolve_collisions(fallback, BRUTE.radius, obstacles)
}

fn run_waves(
    time: Res<Time>,
    obstacles: Res<Obstacles>,
    mut state: ResMut<GameState>,
    mut waves: ResMut<Waves>,
    mut rng: ResMut<CombatRng>,
    player: Query<&Transform, With<Player>>,
    enemies: Query<(), With<Enemy>>,
    mut spawn: MessageWriter<SpawnEnemy>,
    mut started: MessageWriter<WaveStarted>,
    mut cleared: MessageWriter<WaveCleared>,
    mut next: ResMut<NextState<GameFlow>>,
) {
    if !state.running() {
        return;
    }
    let dt = time.delta_secs();
    state.survived += dt;
    let total = waves.0.total_waves();
    let tick = waves.0.tick(dt, enemies.iter().count());
    if let Some(kind) = tick.spawn {
        let around = player
            .iter()
            .next()
            .map_or(Vec2::ZERO, |t| t.translation.xz());
        let pos = find_spawn_pos(&mut rng.0, around, &obstacles);
        spawn.write(SpawnEnemy { pos, kind });
    }
    if let Some(wave) = tick.started {
        state.wave = wave;
        started.write(WaveStarted { wave, total });
    }
    if let Some(wave) = tick.cleared {
        cleared.write(WaveCleared { wave, total });
    }
    if tick.won {
        state.won = true;
        next.set(GameFlow::Won);
    }
}

fn handle_spawn_requests(
    mut commands: Commands,
    mut requests: MessageReader<SpawnEnemy>,
    assets: Option<Res<CombatAssets>>,
    enemies: Query<(), With<Enemy>>,
) {
    let Some(assets) = assets else {
        requests.clear();
        return;
    };
    let mut alive = enemies.iter().count();
    for req in requests.read() {
        if alive >= MAX_ENEMIES {
            continue;
        }
        spawn_enemy(&mut commands, &assets, req.pos, req.kind);
        alive += 1;
    }
}

fn move_enemies(
    time: Res<Time>,
    obstacles: Res<Obstacles>,
    state: Res<GameState>,
    player: Single<&Transform, (With<Player>, Without<Enemy>)>,
    mut enemies: Query<(&Enemy, &mut Transform)>,
) {
    if !state.running() {
        return;
    }
    let dt = time.delta_secs();
    let p = player.translation.xz();
    // Snapshot for the separation pass (n ≤ MAX_ENEMIES, so O(n²) is fine).
    let snapshot: Vec<(Vec2, f32)> = enemies
        .iter()
        .map(|(e, t)| (t.translation.xz(), stats(e.kind).radius))
        .collect();
    for (i, (enemy, mut t)) in enemies.iter_mut().enumerate() {
        let s = stats(enemy.kind);
        let e = t.translation.xz();
        let to = p - e;
        let dist = to.length();
        let touching = dist < PLAYER_RADIUS + s.radius + CONTACT_SLOP;
        let chase = if touching {
            Vec2::ZERO
        } else {
            to / dist.max(1e-4)
        };
        let others = snapshot
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, o)| *o);
        let steer = (chase + separation(e, s.radius, others)).clamp_length_max(1.0);
        if steer.length_squared() < 1e-6 {
            continue;
        }
        let next = resolve_collisions(e + steer * s.speed * dt, s.radius, &obstacles);
        t.translation.x = next.x;
        t.translation.z = next.y;
    }
}

fn contact_damage(
    time: Res<Time>,
    mut state: ResMut<GameState>,
    player: Single<&Transform, With<Player>>,
    enemies: Query<(&Enemy, &Transform), Without<Player>>,
    mut died: MessageWriter<PlayerDied>,
    mut next: ResMut<NextState<GameFlow>>,
) {
    if !state.running() {
        return;
    }
    let mut v = Vitals {
        hp: state.hp,
        invuln: state.invuln,
    };
    v.tick(time.delta_secs());
    let p = player.translation.xz();
    let dmg = enemies
        .iter()
        .filter(|(e, t)| {
            e.hp > 0
                && t.translation.xz().distance(p)
                    < PLAYER_RADIUS + stats(e.kind).radius + CONTACT_SLOP
        })
        .map(|(e, _)| stats(e.kind).contact_damage)
        .fold(0.0, f32::max);
    let result = v.take_hit(dmg);
    if state.hp != v.hp || state.invuln != v.invuln {
        state.hp = v.hp;
        state.invuln = v.invuln;
    }
    if result == HitResult::Killed {
        state.over = true;
        died.write(PlayerDied { pos: p });
        next.set(GameFlow::Lost);
    }
}

fn fire_weapon(
    mut commands: Commands,
    time: Res<Time>,
    assets: Option<Res<CombatAssets>>,
    input: Res<CombatInput>,
    state: Res<GameState>,
    mut weapon: ResMut<Weapon>,
    player: Single<&Transform, With<Player>>,
    enemies: Query<&Transform, With<Enemy>>,
    bullets: Query<(), With<Bullet>>,
) {
    weapon.cooldown = (weapon.cooldown - time.delta_secs()).max(0.0);
    let Some(assets) = assets else { return };
    if !state.running() || weapon.cooldown > 0.0 || bullets.iter().count() >= MAX_BULLETS {
        return;
    }
    let origin = player.translation.xz();
    let nearest = || {
        nearest_in_range(
            origin,
            FIRE_RANGE,
            enemies.iter().map(|t| t.translation.xz()),
        )
    };
    let dir = if input.fire {
        input
            .aim
            .or_else(|| nearest().map(|t| (t - origin).normalize_or_zero()))
    } else if input.auto_fire {
        nearest().map(|t| (t - origin).normalize_or_zero())
    } else {
        None
    };
    let Some(dir) = dir.filter(|d| d.length_squared() > 0.5) else {
        return;
    };
    weapon.cooldown = FIRE_INTERVAL;
    let dir = Vec3::new(dir.x, 0.0, dir.y);
    let muzzle = Vec3::new(origin.x, MUZZLE_HEIGHT, origin.y) + dir * 0.6;
    commands.spawn((
        Bullet {
            vel: dir * BULLET_SPEED,
            life: FIRE_RANGE / BULLET_SPEED + 0.1,
        },
        Mesh3d(assets.bullet_mesh.clone()),
        MeshMaterial3d(assets.bullet_mat.clone()),
        Transform::from_translation(muzzle),
        bevy::light::NotShadowCaster,
    ));
}

fn move_bullets(
    mut commands: Commands,
    time: Res<Time>,
    obstacles: Res<Obstacles>,
    mut state: ResMut<GameState>,
    mut bullets: Query<(Entity, &mut Bullet, &mut Transform), Without<Enemy>>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform)>,
    mut died: MessageWriter<EnemyDied>,
) {
    let dt = time.delta_secs();
    for (be, mut b, mut t) in &mut bullets {
        let prev = t.translation.xz();
        t.translation += b.vel * dt;
        b.life -= dt;
        let p = t.translation.xz();
        let mut spent = b.life <= 0.0 || obstacles.0.iter().any(|o| o.contains(p, 0.0));
        if !spent {
            for (ee, mut enemy, et) in &mut enemies {
                let r = stats(enemy.kind).radius + BULLET_RADIUS;
                // Swept test so fast bullets cannot skip a target on slow frames.
                if enemy.hp <= 0 || segment_distance(prev, p, et.translation.xz()) >= r {
                    continue;
                }
                spent = true;
                match damage_hp(&mut enemy.hp, BULLET_DAMAGE) {
                    HitResult::Hurt => enemy.flash = HIT_FLASH_SECS,
                    HitResult::Killed => {
                        state.kills += 1;
                        died.write(EnemyDied {
                            entity: ee,
                            kind: enemy.kind,
                            pos: et.translation.xz(),
                        });
                        commands
                            .entity(ee)
                            .remove::<Enemy>()
                            .insert(Dying { t: DEATH_FX_SECS });
                    }
                    HitResult::Ignored => {}
                }
                break;
            }
        }
        if spent {
            commands.entity(be).despawn();
        }
    }
}

fn tick_enemy_flash(
    time: Res<Time>,
    assets: Option<Res<CombatAssets>>,
    mut enemies: Query<(&mut Enemy, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(assets) = assets else { return };
    let dt = time.delta_secs();
    for (mut e, mut mat) in &mut enemies {
        if e.flash <= 0.0 {
            continue;
        }
        e.flash -= dt;
        let want = if e.flash > 0.0 {
            assets.flash_mat.clone()
        } else {
            assets.mat(e.kind)
        };
        if mat.0 != want {
            mat.0 = want;
        }
    }
}

fn tick_dying(
    mut commands: Commands,
    time: Res<Time>,
    assets: Option<Res<CombatAssets>>,
    mut dying: Query<(
        Entity,
        &mut Dying,
        &mut Transform,
        Option<&mut MeshMaterial3d<StandardMaterial>>,
    )>,
) {
    for (e, mut d, mut t, mat) in &mut dying {
        d.t -= time.delta_secs();
        if d.t <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        // Flash white and shrink into the ground.
        if let (Some(assets), Some(mut mat)) = (assets.as_ref(), mat)
            && mat.0 != assets.flash_mat
        {
            mat.0 = assets.flash_mat.clone();
        }
        let k = (d.t / DEATH_FX_SECS).clamp(0.05, 1.0);
        t.scale = Vec3::new(1.0 + (1.0 - k) * 0.4, k, 1.0 + (1.0 - k) * 0.4);
    }
}

/// Keeps `GameState::won` in sync if something else moved the flow to Won.
fn mark_won(mut state: ResMut<GameState>) {
    state.won = true;
}

/// Blinks the player while invulnerable after a hit.
fn blink_player(state: Res<GameState>, mut player: Query<&mut Visibility, With<Player>>) {
    let want = if state.invuln > 0.0
        && state.running()
        && ((state.invuln * PLAYER_BLINK_HZ * 2.0) as i32) % 2 == 1
    {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut vis in &mut player {
        vis.set_if_neq(want);
    }
}

#[cfg(test)]
mod tests;
