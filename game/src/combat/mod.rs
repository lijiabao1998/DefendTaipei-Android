//! Combat & enemies — port of the original defendtaipei combat
//! (`src/core/main.ts` soldier loop / playerAttack / damageSol / grenades,
//! `src/npc/squadai.ts`, gun stats from `src/systems/items.ts` + `src/npc/firearms.ts`).
//!
//! Public API (see the PR description):
//! - Messages read: [`SpawnEnemy`], [`ResetCombat`].
//! - Messages written: [`EnemyDied`], [`PlayerDied`], [`PlayerHit`], [`Gunshot`], [`Callout`].
//! - [`spawn_enemy`] / [`spawn_enemy_with_route`] for direct spawning.
//! - [`CombatInput`] (aim / fire), [`CombatEnv`] (night, rain), [`PlayerStance`].
//! - [`CombatWorldRes`] / [`world::CombatWorld`]: line of sight, cover, buildings (map owner plugs in).
//! - [`GameState`]: HP, bleeding, kills, … for the HUD.
//! - Character visuals: [`standin`] (swap for the real rig).

use bevy::prelude::*;

use crate::city::{Lcg, Obstacles};
use crate::player::{Player, resolve_collisions, spawn_point};

pub mod driver;
pub mod input;
pub mod sfx;
pub mod soldier;
pub mod squad;
pub mod standin;
pub mod tuning;
pub mod world;

pub use input::{CombatInput, CombatInputSet};
pub use soldier::{SolState, Soldier};
pub use world::{CombatWorld, CombatWorldRes};

use soldier::{Ctx, PlayerMark};
use tuning::*;

// ── Public types ────────────────────────────────────────────────────────

/// Occupation units the original places (same variants as lijiabao5's game-flow PR #5):
/// street patrols (`spawnSoldiers`), the night half of that set, district outpost garrisons
/// (`dSoldier` with a post), and the four armor models on main roads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EnemyKind {
    /// 解放軍 street patrol.
    #[default]
    Patrol,
    /// 解放軍 night patrol: only out at night (`Sol.night`).
    NightPatrol,
    /// 解放軍 outpost garrison: holds its post (`Sol.post`), does not answer the radio.
    Garrison,
    /// Armor (ZBL-08 / ZBD-05 / 猛士 / ZTD-05): not ported yet — `SpawnEnemy` ignores these.
    Zbl08,
    Zbd05,
    Mengshi,
    Ztd05,
}

impl EnemyKind {
    pub fn is_soldier(self) -> bool {
        matches!(
            self,
            EnemyKind::Patrol | EnemyKind::NightPatrol | EnemyKind::Garrison
        )
    }
}

/// Player combat state shown by the HUD.
#[derive(Resource, Debug, Clone)]
pub struct GameState {
    pub hp: f32,
    pub bleed: bool,
    /// Seconds of hit stagger left (original `player.stun`; movement slow-down not wired yet).
    pub stun: f32,
    pub kills: u32,
    /// TEMPORARY (driver): escalation step shown by the skeleton HUD.
    pub wave: u32,
    /// Player is dead.
    pub over: bool,
    pub survived: f32,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            hp: PLAYER_MAX_HP,
            bleed: false,
            stun: 0.0,
            kills: 0,
            wave: 1,
            over: false,
            survived: 0.0,
        }
    }
}

/// World conditions the soldiers react to (night: shorter sight, night patrols; rain: shorter sight / hearing).
#[derive(Resource, Debug, Clone, Default)]
pub struct CombatEnv {
    pub night: bool,
    /// 0..1
    pub rain: f32,
}

/// What the player is doing that changes how soldiers see / hit them (written by the
/// player / input owners; all false until those features are ported).
#[derive(Resource, Debug, Clone, Default)]
pub struct PlayerStance {
    pub sneak: bool,
    /// In cover (soldiers hit ×0.5).
    pub cover: bool,
    /// Low cover, crouched (aim height 0.75 m).
    pub low_cover: bool,
    /// Torch on (seen ×2 at night).
    pub light: bool,
    /// Inside a building (seen at ×0.6 range).
    pub inside: bool,
}

/// A 解放軍 soldier. The AI state is the ported `Sol`.
#[derive(Component, Debug)]
pub struct Enemy(pub Soldier);

/// A dead soldier's body (cleared after `BODY_SECS`).
#[derive(Component)]
pub struct Body {
    pub kind: EnemyKind,
    t: f32,
}

// ── Messages ────────────────────────────────────────────────────────────

/// Request: spawn one enemy standing at `pos` (y ignored). Patrols a short route through `pos`.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct SpawnEnemy {
    pub pos: Vec3,
    pub kind: EnemyKind,
}

/// A hostile reached 0 HP (exactly once per enemy). `kind` is the unit that died.
/// Same shape as lijiabao5's game-flow PR (#5): `EnemyDied { kind, pos }`.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct EnemyDied {
    pub kind: EnemyKind,
    pub pos: Vec3,
}

/// The player's HP reached 0 (once per life).
#[derive(Message, Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerDied;

/// The player was hurt (for the red flash / HUD log / audio).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct PlayerHit {
    pub dmg: f32,
    pub cause: HitCause,
    /// This hit started bleeding.
    pub bleed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitCause {
    Rifle,
    RifleButt,
    Grenade,
}

/// A gun went off (audio owner plays it).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Gunshot {
    pub pos: Vec3,
    pub by_player: bool,
    pub fam: GunFam,
}

/// A soldier shouted / radioed (original `callout` / `radio` log lines, Traditional Chinese).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Callout {
    pub pos: Vec3,
    pub text: &'static str,
}

/// Request: despawn all enemies, bodies, tracers and grenades, restore the player's HP
/// (and position). Restart buttons / game flow write this.
#[derive(Message, Clone, Copy, Debug, Default, PartialEq)]
pub struct ResetCombat;

// ── Internal resources ──────────────────────────────────────────────────

#[derive(Resource)]
pub struct PlayerGun {
    pub gun: &'static Gun,
    cd: f32,
}

impl Default for PlayerGun {
    fn default() -> Self {
        Self {
            gun: gun(PLAYER_START_GUN).expect("start gun"),
            cd: 0.0,
        }
    }
}

/// Global AI timers (radio / grenade cooldowns, camping detector).
#[derive(Resource, Default)]
struct AiTimers {
    radio_cd: f32,
    gren_cd: f32,
    camp_t: f32,
    camp_at: Vec2,
    next_id: u32,
}

#[derive(Resource)]
pub struct CombatRng(pub Lcg);

impl Default for CombatRng {
    fn default() -> Self {
        Self(Lcg(0xdefe_17a1))
    }
}

#[derive(Component)]
struct Grenade {
    from: Vec2,
    to: Vec2,
    t: f32,
    fuse: f32,
    landed: bool,
}

/// Short-lived effect entity (tracer, blast, grenade warning ring) despawned after `t`.
#[derive(Component)]
pub struct Fx {
    pub t: f32,
    pub grow: f32,
}

#[derive(Component)]
struct GrenadeWarn(Entity);

/// All combat gameplay systems (they run after player movement and combat input).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatSet;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameState>()
            .init_resource::<CombatInput>()
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
            .add_message::<ResetCombat>()
            .add_message::<crate::audio::SfxEvent>()
            .configure_sets(
                Update,
                (
                    CombatInputSet.after(crate::player::MoveSet),
                    CombatSet.after(CombatInputSet),
                ),
            )
            .add_systems(
                Update,
                (
                    apply_reset,
                    handle_spawn_requests,
                    assign_squads,
                    soldiers_think,
                    grenades,
                    player_fire,
                    player_vitals,
                    tick_bodies,
                    sfx::combat_sfx,
                )
                    .chain()
                    .in_set(CombatSet),
            )
            .add_plugins((standin::StandinPlugin, driver::TempDriverPlugin));
        #[cfg(not(target_os = "android"))]
        app.add_systems(Update, input::desktop_mouse_input.in_set(CombatInputSet));
    }
}

// ── Spawning ────────────────────────────────────────────────────────────

/// Spawns one enemy at ground point `pos` (x, z) patrolling a short line through it.
pub fn spawn_enemy(commands: &mut Commands, id: u32, pos: Vec2, kind: EnemyKind) -> Entity {
    let a = (id as f32 * 2.399).rem_euclid(std::f32::consts::TAU);
    let off = Vec2::from_angle(a) * 8.0;
    spawn_enemy_with_route(commands, id, pos, kind, vec![pos, pos + off, pos - off])
}

/// Spawns one enemy at `pos` patrolling `route` back and forth (original: a piece of road).
/// Visuals are attached by the stand-in renderer (or the real rig) on `Added<Enemy>`.
pub fn spawn_enemy_with_route(
    commands: &mut Commands,
    id: u32,
    pos: Vec2,
    kind: EnemyKind,
    route: Vec<Vec2>,
) -> Entity {
    let s = Soldier::new(id, kind, pos, route);
    commands
        .spawn((
            Enemy(s),
            Transform::from_xyz(pos.x, 0.0, pos.y),
            Visibility::default(),
        ))
        .id()
}

fn handle_spawn_requests(
    mut commands: Commands,
    mut requests: MessageReader<SpawnEnemy>,
    mut timers: ResMut<AiTimers>,
    enemies: Query<(), With<Enemy>>,
) {
    let mut alive = enemies.iter().count();
    for req in requests.read() {
        if alive >= MAX_SOLDIERS {
            continue;
        }
        if !req.kind.is_soldier() {
            // TODO(lijiabao2): armor (main.ts `armor`) is a later combat PR.
            warn_once!("SpawnEnemy: armor {:?} not ported yet; ignored", req.kind);
            continue;
        }
        let id = timers.next_id;
        timers.next_id += 1;
        spawn_enemy(&mut commands, id, req.pos.xz(), req.kind);
        alive += 1;
    }
}

fn apply_reset(
    mut commands: Commands,
    mut requests: MessageReader<ResetCombat>,
    mut state: ResMut<GameState>,
    mut gun: ResMut<PlayerGun>,
    mut timers: ResMut<AiTimers>,
    mut player: Query<(&mut Transform, &mut Visibility), With<Player>>,
    field: Query<Entity, Or<(With<Enemy>, With<Body>, With<Fx>, With<Grenade>)>>,
) {
    if requests.read().count() == 0 {
        return;
    }
    for e in &field {
        commands.entity(e).despawn();
    }
    *state = GameState::default();
    gun.cd = 0.0;
    let next_id = timers.next_id;
    *timers = AiTimers {
        next_id,
        ..default()
    };
    for (mut t, mut vis) in &mut player {
        t.translation = spawn_point();
        *vis = Visibility::Inherited;
    }
}

// ── Soldier AI ──────────────────────────────────────────────────────────

/// v22.11 squads re-formed every frame from soldiers in combat (sorted by id).
fn assign_squads(
    env: Res<CombatEnv>,
    mut enemies: Query<&mut Enemy>,
    mut callouts: MessageWriter<Callout>,
) {
    let cs: Vec<(u32, Vec2)> = enemies
        .iter()
        .filter(|e| e.0.state == SolState::Combat && e.0.active(env.night))
        .map(|e| (e.0.id, e.0.pos))
        .collect();
    if cs.is_empty() {
        return;
    }
    for g in squad::squads(&cs) {
        for mut e in &mut enemies {
            let s = &mut e.0;
            let Some(i) = g.iter().position(|&id| id == s.id) else {
                continue;
            };
            let role = squad::role_of(i, g.len());
            if role.is_flank() && s.role != role && s.flank_t.is_none() {
                s.flank_t = Some(FLANK_T);
                callouts.write(Callout {
                    pos: Vec3::new(s.pos.x, 1.8, s.pos.y),
                    text: if role == squad::Role::FlankL {
                        "左翼包抄！"
                    } else {
                        "右翼包抄！"
                    },
                });
            }
            s.role = role;
            s.slot = i;
            s.sq_n = g.len();
        }
    }
}

fn player_mark(pos: Vec2, alive: bool, st: &PlayerStance, firing: bool) -> PlayerMark {
    PlayerMark {
        pos,
        alive,
        sneak: st.sneak,
        cover: st.cover,
        light: st.light,
        inside: st.inside,
        aim_h: if st.low_cover && !firing {
            0.75
        } else if st.sneak {
            0.9
        } else {
            1.25
        },
    }
}

fn soldiers_think(
    mut commands: Commands,
    time: Res<Time>,
    env: Res<CombatEnv>,
    stance: Res<PlayerStance>,
    world: Res<CombatWorldRes>,
    obstacles: Option<Res<Obstacles>>,
    gun: Res<PlayerGun>,
    mut state: ResMut<GameState>,
    mut timers: ResMut<AiTimers>,
    mut rng: ResMut<CombatRng>,
    mut player: Query<&mut Transform, (With<Player>, Without<Enemy>)>,
    mut enemies: Query<(&mut Enemy, &mut Transform)>,
    mut hits: MessageWriter<PlayerHit>,
    mut shots: MessageWriter<Gunshot>,
    mut callouts: MessageWriter<Callout>,
) {
    let dt = time.delta_secs();
    timers.radio_cd -= dt;
    let Ok(mut ptf) = player.single_mut() else {
        return;
    };
    let pp = ptf.translation.xz();
    let c = Ctx {
        dt,
        night: env.night,
        rain: env.rain,
        player: player_mark(pp, !state.over, &stance, gun.cd > 0.05),
        world: world.0.as_ref(),
    };
    let mut spotted: Vec<(u32, Vec2, Vec2)> = Vec::new();
    let mut noises: Vec<(Vec2, f32)> = Vec::new();
    for (mut e, mut tf) in &mut enemies {
        let s = &mut e.0;
        if !s.alive() || !s.active(env.night) {
            continue;
        }
        let out = soldier::tick(s, &c, &mut rng.0);
        if let Some(obs) = obstacles.as_deref() {
            s.pos = resolve_collisions(s.pos, SOLDIER_RADIUS, obs);
        }
        tf.translation.x = s.pos.x;
        tf.translation.z = s.pos.y;
        let at3 = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
        if let Some(sh) = out.shot {
            shots.write(Gunshot {
                pos: at3(sh.from, 1.4),
                by_player: false,
                fam: gun_fam(s.weapon),
            });
            noises.push((s.pos, NOISE_SOL_SHOT));
            tracer(&mut commands, at3(sh.from, TRACER_H), at3(sh.to, TRACER_H));
            if sh.hit && !state.over {
                state.hp -= sh.dmg;
                state.stun = 0.35;
                let new_bleed = sh.bleed && !state.bleed;
                state.bleed |= sh.bleed;
                hits.write(PlayerHit {
                    dmg: sh.dmg,
                    cause: HitCause::Rifle,
                    bleed: new_bleed,
                });
            }
        }
        if let Some(dmg) = out.melee_dmg
            && !state.over
        {
            state.hp -= dmg;
            state.stun = 0.45;
            let away = (pp - s.pos).normalize_or_zero() * MELEE_KNOCKBACK;
            let kb = pp + away;
            let kb = obstacles.as_deref().map_or(kb, |o| {
                resolve_collisions(kb, crate::player::PLAYER_RADIUS, o)
            });
            ptf.translation.x = kb.x;
            ptf.translation.z = kb.y;
            hits.write(PlayerHit {
                dmg,
                cause: HitCause::RifleButt,
                bleed: false,
            });
            callouts.write(Callout {
                pos: at3(pp, 1.8),
                text: "你被槍托擊中！",
            });
        }
        if let Some(p) = out.spotted {
            spotted.push((s.id, s.pos, p));
            noises.push((s.pos, NOISE_SPOTTED));
        }
        if out.search_callout {
            callouts.write(Callout {
                pos: at3(s.pos, 1.8),
                text: "分散搜索！",
            });
        }
        if out.retreat_callout {
            callouts.write(Callout {
                pos: at3(s.pos, 1.8),
                text: "我中彈了，掩護我！",
            });
        }
    }
    for (id, from, p) in spotted {
        if timers.radio_cd <= 0.0 {
            timers.radio_cd = RADIO_CD;
            let n = soldier::radio(
                enemies.iter_mut().map(|(e, _)| &mut e.into_inner().0),
                p,
                from,
                id,
                env.night,
            );
            callouts.write(Callout {
                pos: Vec3::new(from.x, 1.8, from.y),
                text: if n > 0 {
                    "（無線電）「發現目標，請求支援！」——附近士兵正趕來"
                } else {
                    "（無線電）「發現目標，請求支援！」"
                },
            });
        }
    }
    for (at, r) in noises {
        soldier::noise(
            enemies.iter_mut().map(|(e, _)| &mut e.into_inner().0),
            at,
            r,
            env.rain,
            env.night,
            &mut rng.0,
        );
    }
}

fn gun_fam(id: &str) -> GunFam {
    gun(id).map_or(GunFam::Rifle, |g| g.fam)
}

/// Grenades (squadai.ts / main.ts `updateGrenades`): a squad that knows roughly where a
/// player is camping out of sight lobs one; it lands, warns, and goes off after the fuse.
fn grenades(
    mut commands: Commands,
    time: Res<Time>,
    env: Res<CombatEnv>,
    world: Res<CombatWorldRes>,
    fx: Option<Res<standin::CombatFxAssets>>,
    mut state: ResMut<GameState>,
    mut timers: ResMut<AiTimers>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform), Without<Grenade>>,
    mut grens: Query<(Entity, &mut Grenade, &mut Transform, Option<&GrenadeWarn>)>,
    player: Query<&Transform, (With<Player>, Without<Enemy>, Without<Grenade>)>,
    mut hits: MessageWriter<PlayerHit>,
    mut died: MessageWriter<EnemyDied>,
    mut callouts: MessageWriter<Callout>,
    mut rng: ResMut<CombatRng>,
    mut sfx: MessageWriter<crate::audio::SfxEvent>,
) {
    let dt = time.delta_secs();
    let Ok(ptf) = player.single() else { return };
    let pp = ptf.translation.xz();
    timers.gren_cd -= dt;
    if pp.distance(timers.camp_at) > 2.5 {
        timers.camp_at = pp;
        timers.camp_t = 0.0;
    }
    let near = |q: Option<Vec2>| q.is_some_and(|q| q.distance(pp) < 5.0);
    let mut any_aiming = false;
    let mut thrower: Option<Entity> = None;
    let mut hunters = 0;
    for (ent, e, _) in &enemies {
        let s = &e.0;
        let hunting = s.alive()
            && s.active(env.night)
            && ((s.state == SolState::Combat && near(s.last_seen))
                || (s.state == SolState::Investigate && s.alert >= 0.5 && near(s.target)));
        if !hunting {
            continue;
        }
        hunters += 1;
        any_aiming |= s.aiming;
        let d = s.pos.distance(pp);
        if thrower.is_none() && d > GREN_MIN && d < GREN_MAX && s.stun <= 0.0 && s.retreat_t <= 0.0
        {
            thrower = Some(ent);
        }
    }
    if hunters > 0 && !any_aiming && !state.over {
        timers.camp_t += dt;
    } else {
        timers.camp_t = (timers.camp_t - dt * 2.0).max(0.0);
    }
    if timers.camp_t > GREN_CAMP
        && timers.gren_cd <= 0.0
        && let Some(th) = thrower
        && let Ok((_, mut e, _)) = enemies.get_mut(th)
    {
        let s = &mut e.0;
        let lk = s.last_seen.or(s.target).unwrap_or(pp);
        s.last_seen = Some(lk);
        timers.gren_cd = GREN_CD;
        timers.camp_t = 0.0;
        // lands near the last-known spot with a small deterministic scatter
        let j = (s.id as i32 * 37 + 11) % 7;
        let to = lk + Vec2::new(((j % 3) - 1) as f32 * 0.9, ((j / 3) - 1) as f32 * 0.9);
        let mut g = commands.spawn((
            Grenade {
                from: s.pos,
                to,
                t: 0.0,
                fuse: GREN_FUSE,
                landed: false,
            },
            Transform::from_xyz(s.pos.x, 1.6, s.pos.y),
            Visibility::default(),
        ));
        if let Some(fx) = fx.as_deref() {
            g.insert((
                Mesh3d(fx.grenade_mesh.clone()),
                MeshMaterial3d(fx.grenade_mat.clone()),
            ));
        }
        callouts.write(Callout {
            pos: Vec3::new(s.pos.x, 1.8, s.pos.y),
            text: "手榴彈！",
        });
    }
    for (ge, mut g, mut tf, warn) in &mut grens {
        if !g.landed {
            g.t += dt;
            let u = (g.t / GREN_FLIGHT).min(1.0);
            let p = g.from + (g.to - g.from) * u;
            tf.translation = Vec3::new(p.x, 1.6 + 4.0 * u * (1.0 - u) - 1.5 * u, p.y);
            if u >= 1.0 {
                g.landed = true;
                sfx.write(crate::audio::SfxEvent::Clink {
                    dist: pp.distance(g.to),
                });
                if let Some(fx) = fx.as_deref() {
                    let w = standin::spawn_grenade_warn(&mut commands, fx, g.to);
                    commands.entity(ge).insert(GrenadeWarn(w));
                }
            }
            continue;
        }
        g.fuse -= dt;
        if g.fuse > 0.0 {
            continue;
        }
        commands.entity(ge).despawn();
        if let Some(w) = warn {
            commands.entity(w.0).despawn();
        }
        if let Some(fx) = fx.as_deref() {
            standin::spawn_blast(&mut commands, fx, g.to);
        }
        let p = g.to;
        sfx.write(crate::audio::SfxEvent::ShellBoom {
            dist: pp.distance(p),
            indoor: false,
        });
        for (ent, mut e, tf) in &mut enemies {
            let s = &mut e.0;
            let d = s.pos.distance(p);
            if s.alive() && d < GREN_WOUND && soldier::damage(s, squad::grenade_dmg(d), p) {
                commands.entity(ent).insert(Body {
                    kind: s.kind,
                    t: BODY_SECS,
                });
                died.write(EnemyDied {
                    kind: s.kind,
                    pos: tf.translation,
                });
            }
        }
        soldier::noise(
            enemies.iter_mut().map(|(_, e, _)| &mut e.into_inner().0),
            p,
            NOISE_GRENADE,
            env.rain,
            env.night,
            &mut rng.0,
        );
        let dp = pp.distance(p);
        let mut dmg = squad::grenade_dmg(dp);
        if dmg <= 0.0 || state.over {
            continue;
        }
        if world.0.cover_near(pp, 1.2) && dp > 1.5 {
            dmg *= 0.6;
        }
        state.hp -= dmg;
        let new_bleed = dmg > 15.0 && !state.bleed;
        state.bleed |= new_bleed;
        hits.write(PlayerHit {
            dmg,
            cause: HitCause::Grenade,
            bleed: new_bleed,
        });
    }
}

// ── Player shooting (playerAttack) ──────────────────────────────────────

/// The nearest live soldier in sight within `r` (main.ts `nearestFoe`).
fn nearest_foe<'a>(
    p: Vec2,
    r: f32,
    world: &dyn CombatWorld,
    night: bool,
    soldiers: impl Iterator<Item = &'a Soldier>,
) -> Option<Vec2> {
    let mut best = None;
    let mut bd = r;
    for s in soldiers {
        if !s.alive() || !s.active(night) {
            continue;
        }
        let d = s.pos.distance(p);
        if d < bd && world.shot_ray(p, s.pos, 1.6, 1.6, 0.0) >= 1.0 {
            best = Some(s.pos);
            bd = d;
        }
    }
    best
}

fn player_fire(
    mut commands: Commands,
    time: Res<Time>,
    input: Res<CombatInput>,
    env: Res<CombatEnv>,
    world: Res<CombatWorldRes>,
    mut gun: ResMut<PlayerGun>,
    mut state: ResMut<GameState>,
    mut rng: ResMut<CombatRng>,
    player: Query<&Transform, (With<Player>, Without<Enemy>)>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform)>,
    mut died: MessageWriter<EnemyDied>,
    mut shots: MessageWriter<Gunshot>,
) {
    gun.cd = (gun.cd - time.delta_secs()).max(0.0);
    let Ok(ptf) = player.single() else { return };
    if state.over || gun.cd > 0.0 {
        return;
    }
    let p0 = ptf.translation.xz();
    let w = world.0.as_ref();
    let foe = |only_fighting: bool| {
        nearest_foe(
            p0,
            AUTO_AIM_RANGE,
            w,
            env.night,
            enemies
                .iter()
                .map(|(_, e, _)| &e.0)
                .filter(|s| !only_fighting || s.state == SolState::Combat),
        )
    };
    let aim_pt = if input.fire {
        match input.aim {
            Some(a) => Some(p0 + a * 5.0),
            None => Some(foe(false).unwrap_or_else(|| {
                // no one in sight: straight ahead (local −Z is forward on the player)
                let f = ptf.forward();
                p0 + Vec2::new(f.x, f.z) * 5.0
            })),
        }
    } else if input.auto_fire {
        // TEMPORARY stand-in for holding 打: only answers soldiers already fighting the
        // player, so the auto-fire does not snipe unaware patrols (stealth still matters).
        foe(true)
    } else {
        None
    };
    let Some(aim_pt) = aim_pt else { return };
    let dir = (aim_pt - p0).normalize_or_zero();
    if dir == Vec2::ZERO {
        return;
    }
    let g = gun.gun;
    gun.cd = g.cd;
    shots.write(Gunshot {
        pos: ptf.translation + Vec3::Y * SHOT_FROM_H,
        by_player: true,
        fam: g.fam,
    });
    // v21.4 the round stops at the first wall / cover it meets (aimed at chest height)
    let mut hd = g.range * w.shot_ray(p0, p0 + dir * g.range, SHOT_FROM_H, SHOT_TO_H, 1.0);
    let mut hit: Option<Entity> = None;
    for (ent, e, _) in &enemies {
        let s = &e.0;
        if !s.alive() {
            continue;
        }
        let to = s.pos - p0;
        let t = to.dot(dir);
        if t < 0.0 || t > hd {
            continue;
        }
        let perp = (to - dir * t).length();
        let th = if s.crouch {
            SHOT_TO_H_CROUCH
        } else {
            SHOT_TO_H
        };
        if perp < SHOT_HIT_RADIUS && w.shot_ray(p0, s.pos, SHOT_FROM_H, th, 1.0) >= 1.0 {
            hit = Some(ent);
            hd = t;
        }
    }
    let end = p0 + dir * hd;
    tracer(
        &mut commands,
        Vec3::new(p0.x, TRACER_H, p0.y),
        Vec3::new(end.x, TRACER_H, end.y),
    );
    if let Some(ent) = hit
        && let Ok((_, mut e, tf)) = enemies.get_mut(ent)
    {
        let dmg = g.dmg * (0.8 + rng.0.next_f32() * 0.4);
        if soldier::damage(&mut e.0, dmg, p0) {
            state.kills += 1;
            commands.entity(ent).insert(Body {
                kind: e.0.kind,
                t: BODY_SECS,
            });
            died.write(EnemyDied {
                kind: e.0.kind,
                pos: tf.translation,
            });
        }
    }
    // the shot is heard (gun noise radius)
    soldier::noise(
        enemies.iter_mut().map(|(_, e, _)| &mut e.into_inner().0),
        p0,
        g.noise,
        env.rain,
        env.night,
        &mut rng.0,
    );
}

/// Spawns a tracer line from `a` to `b` (main.ts `tracer`, 0.08 s).
pub fn tracer(commands: &mut Commands, a: Vec3, b: Vec3) {
    let len = a.distance(b);
    if len < 0.05 {
        return;
    }
    commands.spawn((
        standin::TracerFx,
        Fx {
            t: TRACER_SECS,
            grow: 0.0,
        },
        Transform::from_translation((a + b) / 2.0)
            .looking_at(b, Vec3::Y)
            .with_scale(Vec3::new(0.035, 0.035, len)),
        Visibility::default(),
    ));
}

fn player_vitals(
    time: Res<Time>,
    mut state: ResMut<GameState>,
    mut died: MessageWriter<PlayerDied>,
) {
    if state.over {
        return;
    }
    let dt = time.delta_secs();
    state.survived += dt;
    state.stun = (state.stun - dt).max(0.0);
    if state.bleed {
        state.hp -= dt * BLEED_PER_SEC;
    }
    if state.hp <= 0.0 {
        state.hp = 0.0;
        state.over = true;
        died.write(PlayerDied);
    }
}

fn tick_bodies(
    mut commands: Commands,
    time: Res<Time>,
    mut bodies: Query<(Entity, &mut Body)>,
    mut fx: Query<(Entity, &mut Fx, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut b) in &mut bodies {
        b.t -= dt;
        if b.t <= 0.0 {
            commands.entity(e).despawn();
        }
    }
    for (e, mut f, mut t) in &mut fx {
        f.t -= dt;
        if f.t <= 0.0 {
            commands.entity(e).despawn();
        } else if f.grow > 0.0 {
            t.scale *= 1.0 + f.grow * dt;
        }
    }
}

#[cfg(test)]
mod tests;
