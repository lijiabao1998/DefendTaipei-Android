//! v24.20 hand grenades and v24.21 戰況加劇 (Web `playerThrow`, `grenadeTick`,
//! `blastReach`, `WAR`, `warTick`, `warBarrage`, `warStrike`, `warImpact` and
//! `updateShells` in `src/core/main.ts`).
//!
//! **Grenades.** The service grenade and grandpa's old grenade can be thrown the
//! way the player faces, up to 13 m (stopping at the first wall, sailing over
//! low cover). About 3.5 s later it goes off: almost certainly lethal within
//! 5 m, wounding to 15 m, walls in between stop it, and the player's own blast
//! hurts them too. After decades in a cupboard, three in ten old grenades are duds.
//!
//! **The war gets worse.** The war level rises from 0 at the landing to 1 about
//! a week in, faster with every area taken back. Bursts of distant small-arms
//! fire come more and more often. From the first evening mortar barrages fall
//! 30–130 m away under red warning rings with a whistle; once the level passes
//! 0.3 an air-raid warning precedes a long-range rocket, a cruise missile or a
//! short-range ballistic missile, whose big ring lands about ten seconds later
//! and leaves a burning crater. Radii and damage are the Web's rough game values.
//! Indoors stops most fragments (a close missile still hurts through walls),
//! cover halves them, and nothing falls while the player sleeps.

use bevy::prelude::*;

use crate::audio::{AlertKind as SoundAlert, GunKind, SfxEvent};
use crate::combat::standin::CombatFxAssets;
use crate::combat::{
    self, Body, CombatEnv, CombatRng, CombatWorldRes, Enemy, EnemyDied, GameState, HitCause,
    PlayerHit,
};
use crate::game_flow::{Campaign, GameFlow};
use crate::i18n::{Language, tr};
use crate::inventory::Inventory;
use crate::player::Player;
use crate::ui::PauseOpen;
use crate::worldplay::{WorldNotice, WorldSet};

/// `GREN`: lethal core, wounding ring, fuse, throw range, damages, dud chance.
pub const GREN_CORE: f32 = 5.0;
pub const GREN_WOUND: f32 = 15.0;
pub const GREN_FUSE: f32 = 3.5;
pub const GREN_RANGE: f32 = 13.0;
const GREN_CORE_DMG: f32 = 95.0;
const GREN_WOUND_DMG: f32 = 42.0;
const OLD_DUD: f32 = 0.3;

/// One kind of incoming fire (`WarWeapon`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WarWeapon {
    pub zh: &'static str,
    pub en: &'static str,
    pub core: f32,
    pub wound: f32,
    pub core_dmg: f32,
    pub wound_dmg: f32,
    pub big: f32,
    pub lead: f32,
    pub mortar: bool,
}

pub const MORTAR: WarWeapon = WarWeapon {
    zh: "迫擊砲彈",
    en: "Mortar round",
    core: 6.0,
    wound: 18.0,
    core_dmg: 85.0,
    wound_dmg: 50.0,
    big: 1.4,
    lead: 5.0,
    mortar: true,
};
pub const ROCKET: WarWeapon = WarWeapon {
    zh: "遠程火箭彈",
    en: "Long-range rocket",
    core: 16.0,
    wound: 45.0,
    core_dmg: 190.0,
    wound_dmg: 70.0,
    big: 2.6,
    lead: 8.0,
    mortar: false,
};
pub const SRBM: WarWeapon = WarWeapon {
    zh: "短程彈道飛彈",
    en: "Short-range ballistic missile",
    core: 24.0,
    wound: 70.0,
    core_dmg: 240.0,
    wound_dmg: 80.0,
    big: 4.0,
    lead: 10.0,
    mortar: false,
};
pub const CRUISE: WarWeapon = WarWeapon {
    zh: "巡弋飛彈",
    en: "Cruise missile",
    core: 22.0,
    wound: 62.0,
    core_dmg: 230.0,
    wound_dmg: 75.0,
    big: 3.6,
    lead: 9.0,
    mortar: false,
};

/// `weaponDmg`: damage at distance `d`.
pub fn weapon_dmg(w: &WarWeapon, d: f32) -> f32 {
    if d < w.core {
        w.core_dmg * (1.0 - 0.4 * (d / w.core))
    } else if d < w.wound {
        w.wound_dmg * ((w.wound - d) / (w.wound - w.core))
    } else {
        0.0
    }
}

/// The player's grenade at distance `d` (core / wound falloff).
pub fn grenade_dmg_player(d: f32) -> f32 {
    if d < GREN_CORE {
        GREN_CORE_DMG * (1.0 - 0.4 * (d / GREN_CORE))
    } else if d < GREN_WOUND {
        GREN_WOUND_DMG * ((GREN_WOUND - d) / (GREN_WOUND - GREN_CORE))
    } else {
        0.0
    }
}

/// `warLevel`: 0 at the landing, 1 about a week in; reclaiming makes it hotter.
pub fn war_level(game_min: f32, reclaimed: usize) -> f32 {
    ((game_min / 1440.0 - 0.25) / 6.0 + reclaimed as f32 * 0.015).clamp(0.0, 1.0)
}

/// The level at which shells start to fall: about the evening of the first day.
pub const WAR_START: f32 = 0.1;

/// The player throws a grenade from the bag (`playerThrow`).
#[derive(Message, Clone, Debug)]
pub struct ThrowGrenade(pub String);

#[derive(Component)]
struct ThrownGrenade {
    t: f32,
    fly: f32,
    fly_t: f32,
    a: Vec2,
    b: Vec2,
    dud: bool,
}

/// An incoming round under its warning ring.
#[derive(Component)]
struct Shell {
    t: f32,
    p: Vec2,
    w: WarWeapon,
    whistled: bool,
    ring: Entity,
}

/// A burning crater (`warFires`).
#[derive(Component)]
struct Crater {
    t: f32,
    r: f32,
    puff: f32,
    seed: f32,
}

#[derive(Component)]
struct Flame(f32);

/// Timers (not saved, like the Web's).
#[derive(Resource)]
struct WarClock {
    gun: f32,
    shell: f32,
    big: f32,
    /// Distant shots still to play: (delay, distance, kind).
    pending: Vec<(f32, f32, GunKind)>,
}

impl Default for WarClock {
    fn default() -> Self {
        Self {
            gun: 18.0,
            shell: 160.0,
            big: 420.0,
            pending: Vec::new(),
        }
    }
}

/// Counters for tests and QA (`warStat`).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct WarStats {
    pub level: f32,
    pub gunfire: u32,
    pub shells: u32,
    pub rockets: u32,
    pub missiles: u32,
    pub craters: u32,
    pub thrown: u32,
    pub duds: u32,
}

#[derive(Resource)]
struct WarArt {
    grenade: Handle<Mesh>,
    grenade_mat: Handle<StandardMaterial>,
    crater: Handle<Mesh>,
    crater_mat: Handle<StandardMaterial>,
    flame: Handle<Mesh>,
    flame_mat: Handle<StandardMaterial>,
}

/// Ask for a barrage or a strike at once (QA and tools).
#[derive(Message, Clone, Copy, Debug)]
pub enum WarCall {
    Barrage { n: u32, at: Vec2 },
    Strike { w: WarWeapon, at: Vec2 },
}

pub struct WarPlugin;

impl Plugin for WarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WarClock>()
            .init_resource::<WarStats>()
            .add_message::<ThrowGrenade>()
            .add_message::<WarCall>()
            .add_systems(Startup, setup_art)
            .add_systems(OnEnter(GameFlow::Prologue), reset)
            .add_systems(OnExit(GameFlow::Playing), clear)
            .add_systems(
                Update,
                (throw, fly_grenades, war_tick, war_calls, shells, craters)
                    .chain()
                    .in_set(WorldSet)
                    .run_if(in_state(GameFlow::Playing)),
            );
    }
}

/// Remove every round in the air, grenade and crater at once (QA fixtures).
pub fn clear_sky(world: &mut World) {
    let rings: Vec<Entity> = world
        .query::<&Shell>()
        .iter(world)
        .map(|s| s.ring)
        .collect();
    let all: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<ThrownGrenade>, With<Shell>, With<Crater>, With<Flame>)>>()
        .iter(world)
        .collect();
    for e in rings.into_iter().chain(all) {
        if let Ok(ent) = world.get_entity_mut(e) {
            ent.despawn();
        }
    }
}

/// Rounds still in the air (QA).
pub fn shells_pending(world: &mut World) -> usize {
    world.query::<&Shell>().iter(world).count()
}

fn reset(mut clock: ResMut<WarClock>, mut stats: ResMut<WarStats>) {
    *clock = WarClock::default();
    *stats = WarStats::default();
}

fn clear(
    mut commands: Commands,
    q: Query<Entity, Or<(With<ThrownGrenade>, With<Shell>, With<Crater>, With<Flame>)>>,
    shells: Query<&Shell>,
) {
    for s in &shells {
        commands.entity(s.ring).try_despawn();
    }
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

fn setup_art(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(WarArt {
        grenade: meshes.add(Sphere::new(0.09).mesh().uv(8, 6)),
        grenade_mat: materials.add(StandardMaterial {
            base_color: Color::srgb_u8(0x3a, 0x4a, 0x2e),
            perceptual_roughness: 0.6,
            ..default()
        }),
        crater: meshes.add(Circle::new(1.0)),
        crater_mat: materials.add(StandardMaterial {
            base_color: Color::srgba(0.05, 0.04, 0.03, 0.85),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        flame: meshes.add(Sphere::new(0.5).mesh().ico(1).unwrap()),
        flame_mat: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.55, 0.15, 0.75),
            emissive: LinearRgba::rgb(4.0, 1.4, 0.3),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
    });
}

/// `blastReach`: a wall between stops the blast; low cover is blown over.
fn blast_reach(world: &CombatWorldRes, p: Vec2, q: Vec2) -> bool {
    let d = q - p;
    let l = d.length().max(1e-3);
    let n = Vec2::new(-d.y, d.x) / l * 0.8;
    world.0.shot_ray(p, q, 1.5, 1.5, 0.0) >= 1.0
        || world.0.shot_ray(p, q + n, 1.5, 1.5, 0.0) >= 1.0
        || world.0.shot_ray(p, q - n, 1.5, 1.5, 0.0) >= 1.0
}

#[allow(clippy::too_many_arguments)]
fn throw(
    mut commands: Commands,
    mut throws: MessageReader<ThrowGrenade>,
    lang: Res<Language>,
    art: Res<WarArt>,
    env: Res<CombatEnv>,
    world: Res<CombatWorldRes>,
    upstairs: Option<Res<crate::upstairs::Upstairs>>,
    (mut inventory, mut stats, mut rng): (ResMut<Inventory>, ResMut<WarStats>, ResMut<CombatRng>),
    mut notice: ResMut<WorldNotice>,
    player: Query<&Transform, With<Player>>,
    mut enemies: Query<&mut Enemy>,
) {
    for ThrowGrenade(key) in throws.read() {
        if upstairs.as_deref().is_some_and(|u| u.active()) {
            notice.text = tr(
                *lang,
                "在樓上丟不出去。",
                "There is nowhere to throw it up here.",
            )
            .into();
            continue;
        }
        if !matches!(key.as_str(), "grenade" | "oldgrenade") || !inventory.take(key, 1) {
            continue;
        }
        let Ok(tf) = player.single() else { continue };
        let p = tf.translation.xz();
        let f = tf.forward();
        let u = Vec2::new(f.x, f.z).normalize_or(Vec2::Y);
        // it sails over low cover and stops at the first wall
        let want = GREN_RANGE;
        let clear = world.0.shot_ray(p, p + u * want, 1.7, 1.7, 0.4);
        let d = if clear < 1.0 {
            (want * clear - 0.4).max(1.0)
        } else {
            want
        };
        let dud = key == "oldgrenade" && rng.0.next_f32() < OLD_DUD;
        commands.spawn((
            ThrownGrenade {
                t: GREN_FUSE,
                fly: 0.25 + d * 0.055,
                fly_t: 0.0,
                a: p,
                b: p + u * d,
                dud,
            },
            Mesh3d(art.grenade.clone()),
            MeshMaterial3d(art.grenade_mat.clone()),
            Transform::from_xyz(p.x, 1.3, p.y).with_scale(Vec3::new(1.0, 1.25, 1.0)),
        ));
        stats.thrown += 1;
        notice.text = tr(*lang, "你把手榴彈丟了出去！", "You throw the grenade!").into();
        for mut e in &mut enemies {
            combat::soldier::noise(
                std::iter::once(&mut e.0),
                p,
                6.0,
                env.rain,
                env.night,
                &mut rng.0,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fly_grenades(
    mut commands: Commands,
    time: Res<Time>,
    pause: Res<PauseOpen>,
    lang: Res<Language>,
    env: Res<CombatEnv>,
    world: Res<CombatWorldRes>,
    fx: Option<Res<CombatFxAssets>>,
    (mut game, mut stats, mut rng): (ResMut<GameState>, ResMut<WarStats>, ResMut<CombatRng>),
    mut notice: ResMut<WorldNotice>,
    player: Query<&Transform, (With<Player>, Without<ThrownGrenade>)>,
    mut q: Query<(Entity, &mut ThrownGrenade, &mut Transform), Without<Player>>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform), (Without<Player>, Without<ThrownGrenade>)>,
    mut out: (
        MessageWriter<SfxEvent>,
        MessageWriter<EnemyDied>,
        MessageWriter<PlayerHit>,
    ),
) {
    if pause.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let pp = player.single().map_or(Vec2::ZERO, |t| t.translation.xz());
    for (e, mut g, mut tf) in &mut q {
        g.t -= dt;
        g.fly_t = (g.fly_t + dt).min(g.fly);
        let f = g.fly_t / g.fly;
        let at = g.a.lerp(g.b, f);
        tf.translation = Vec3::new(
            at.x,
            0.12 + 1.5 * 4.0 * f * (1.0 - f) + (1.0 - f) * 1.2,
            at.y,
        );
        if g.t > 0.0 {
            continue;
        }
        commands.entity(e).despawn();
        if g.dud {
            stats.duds += 1;
            notice.text = tr(
                *lang,
                "……沒有炸。放了太多年，它啞了。",
                "...Nothing. After so many years, it is a dud.",
            )
            .into();
            continue;
        }
        let p = g.b;
        if let Some(fx) = fx.as_deref() {
            combat::standin::spawn_blast(&mut commands, fx, p);
        }
        out.0.write(SfxEvent::ShellBoom {
            dist: pp.distance(p),
            indoor: world.0.building_at(pp).is_some(),
        });
        for (ent, mut o, tf) in &mut enemies {
            let d = o.0.pos.distance(p);
            if o.0.alive() && d < GREN_WOUND && blast_reach(&world, p, o.0.pos) {
                let dmg = if d < GREN_CORE {
                    240.0
                } else {
                    GREN_WOUND_DMG * 1.6 * ((GREN_WOUND - d) / (GREN_WOUND - GREN_CORE))
                };
                if combat::soldier::damage(&mut o.0, dmg, p) {
                    // the player's own grenade: theirs
                    game.kills += 1;
                    commands.entity(ent).insert(Body::new(o.0.kind));
                    out.1.write(EnemyDied {
                        id: o.0.id,
                        kind: o.0.kind,
                        pos: tf.translation,
                    });
                }
            }
            combat::soldier::noise(
                std::iter::once(&mut o.0),
                p,
                45.0,
                env.rain,
                env.night,
                &mut rng.0,
            );
        }
        let dp = pp.distance(p);
        if dp < GREN_WOUND && !game.over && blast_reach(&world, p, pp) {
            let dmg = grenade_dmg_player(dp);
            game.hp -= dmg;
            let bleed = dmg > 15.0 && !game.bleed;
            game.bleed |= bleed;
            out.2.write(PlayerHit {
                dmg,
                cause: HitCause::Grenade,
                bleed,
            });
            if bleed {
                notice.text = tr(
                    *lang,
                    "被破片擊中，流血了！",
                    "Fragments hit you. You are bleeding!",
                )
                .into();
            }
        }
    }
}

/// `warSpot`: a ring around the player.
fn war_spot(p: Vec2, min_d: f32, max_d: f32, rng: &mut crate::city::Lcg) -> Vec2 {
    let a = rng.next_f32() * std::f32::consts::TAU;
    let d = min_d + rng.next_f32() * (max_d - min_d);
    p + Vec2::from_angle(a) * d
}

#[allow(clippy::too_many_arguments)]
fn war_tick(
    time: Res<Time>,
    pause: Res<PauseOpen>,
    campaign: Res<Campaign>,
    world: Res<CombatWorldRes>,
    upstairs: Option<Res<crate::upstairs::Upstairs>>,
    mut clock: ResMut<WarClock>,
    mut stats: ResMut<WarStats>,
    mut rng: ResMut<CombatRng>,
    player: Query<&Transform, With<Player>>,
    mut calls: MessageWriter<WarCall>,
    mut sfx: MessageWriter<SfxEvent>,
) {
    if pause.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let level = war_level(campaign.game_min, campaign.reclaimed.len());
    stats.level = level;
    let Ok(pt) = player.single() else { return };
    let p = upstairs
        .as_deref()
        .and_then(|u| u.landing())
        .unwrap_or(pt.translation.xz());
    let indoor =
        upstairs.as_deref().is_some_and(|u| u.active()) || world.0.building_at(p).is_some();
    // the war in the distance: bursts of small arms, more often as it goes on
    for shot in &mut clock.pending {
        shot.0 -= dt;
    }
    for (_, dist, kind) in clock.pending.iter().filter(|s| s.0 <= 0.0) {
        sfx.write(SfxEvent::Gunshot {
            kind: *kind,
            dist: *dist,
            indoor,
        });
    }
    clock.pending.retain(|s| s.0 > 0.0);
    clock.gun -= dt;
    if clock.gun <= 0.0 {
        clock.gun = (40.0 - 32.0 * level) * (0.6 + rng.0.next_f32() * 0.8);
        stats.gunfire += 1;
        let dist = 140.0 + rng.0.next_f32() * 380.0;
        let mg = rng.0.next_f32() < 0.3;
        let n = 3 + (rng.0.next_f32() * (5.0 + 10.0 * level)) as u32;
        let mut t = 0.0;
        for _ in 0..n {
            clock
                .pending
                .push((t, dist, if mg { GunKind::Mg } else { GunKind::Rifle }));
            t += if mg {
                0.09
            } else {
                0.17 + rng.0.next_f32() * 0.24
            };
        }
    }
    // nothing falls on the first day, or while the player is asleep
    if level < WAR_START || campaign.sleeping {
        return;
    }
    clock.shell -= dt;
    if clock.shell <= 0.0 {
        clock.shell = (260.0 - 190.0 * level) * (0.7 + rng.0.next_f32() * 0.6);
        let n = 2 + (rng.0.next_f32() * (2.0 + 3.0 * level)) as u32;
        let at = war_spot(p, 30.0, 130.0, &mut rng.0);
        calls.write(WarCall::Barrage { n, at });
    }
    if level > 0.3 {
        clock.big -= dt;
        if clock.big <= 0.0 {
            clock.big = (900.0 - 600.0 * level) * (0.7 + rng.0.next_f32() * 0.6);
            let r = rng.0.next_f32();
            let w = if r < 0.55 {
                ROCKET
            } else if r < 0.8 {
                CRUISE
            } else {
                SRBM
            };
            // inside the wound ring only once the war is at its worst
            let at = war_spot(p, w.core + 8.0 + (1.0 - level) * 60.0, 240.0, &mut rng.0);
            calls.write(WarCall::Strike { w, at });
        }
    }
}

/// A red warning ring: the faint wound disc with the brighter core inside it.
fn spawn_ring(
    commands: &mut Commands,
    fx: &CombatFxAssets,
    at: Vec2,
    core: f32,
    wound: f32,
) -> Entity {
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let inner = commands
        .spawn((
            Mesh3d(fx.disc_mesh.clone()),
            MeshMaterial3d(fx.warn_core_mat.clone()),
            Transform::from_xyz(0.0, 0.0, 0.002).with_scale(Vec3::splat(core / wound)),
            bevy::light::NotShadowCaster,
        ))
        .id();
    commands
        .spawn((
            Mesh3d(fx.disc_mesh.clone()),
            MeshMaterial3d(fx.warn_wound_mat.clone()),
            Transform::from_xyz(at.x, 0.06, at.y)
                .with_rotation(flat)
                .with_scale(Vec3::splat(wound)),
            Visibility::default(),
            bevy::light::NotShadowCaster,
        ))
        .add_child(inner)
        .id()
}

#[allow(clippy::too_many_arguments)]
fn war_calls(
    mut commands: Commands,
    mut calls: MessageReader<WarCall>,
    lang: Res<Language>,
    fx: Option<Res<CombatFxAssets>>,
    (mut stats, mut rng): (ResMut<WarStats>, ResMut<CombatRng>),
    mut campaign: ResMut<Campaign>,
    mut notice: ResMut<WorldNotice>,
    mut sfx: MessageWriter<SfxEvent>,
) {
    let Some(fx) = fx.as_deref() else {
        calls.clear();
        return;
    };
    for call in calls.read() {
        match *call {
            WarCall::Barrage { n, at } => {
                stats.shells += n;
                let mut t = MORTAR.lead;
                for _ in 0..n {
                    let a = rng.0.next_f32() * std::f32::consts::TAU;
                    let r = 3.0 + rng.0.next_f32() * 16.0;
                    let q = at + Vec2::from_angle(a) * r;
                    let ring = spawn_ring(&mut commands, fx, q, MORTAR.core, MORTAR.wound);
                    commands.spawn(Shell {
                        t,
                        p: q,
                        w: MORTAR,
                        whistled: false,
                        ring,
                    });
                    t += 0.9 + rng.0.next_f32() * 1.2;
                }
            }
            WarCall::Strike { w, at } => {
                if w == ROCKET {
                    stats.rockets += 1;
                } else {
                    stats.missiles += 1;
                }
                let ring = spawn_ring(&mut commands, fx, at, w.core, w.wound);
                commands.spawn(Shell {
                    t: w.lead,
                    p: at,
                    w,
                    whistled: false,
                    ring,
                });
                sfx.write(SfxEvent::Alert {
                    kind: SoundAlert::Alert,
                });
                notice.text = match *lang {
                    Language::ZhTw => format!("防空警報！{}來襲，快躲進室內或離開紅圈！", w.zh),
                    Language::En => format!(
                        "Air raid warning! {} incoming. Get indoors or out of the ring!",
                        w.en
                    ),
                };
                // slowDown
                campaign.speed = 1;
                campaign.sleeping = false;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn shells(
    mut commands: Commands,
    time: Res<Time>,
    pause: Res<PauseOpen>,
    lang: Res<Language>,
    env: Res<CombatEnv>,
    world: Res<CombatWorldRes>,
    fx: Option<Res<CombatFxAssets>>,
    art: Res<WarArt>,
    upstairs: Option<Res<crate::upstairs::Upstairs>>,
    (mut game, mut stats, mut rng): (ResMut<GameState>, ResMut<WarStats>, ResMut<CombatRng>),
    mut notice: ResMut<WorldNotice>,
    player: Query<&Transform, With<Player>>,
    mut q: Query<(Entity, &mut Shell)>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform), Without<Player>>,
    mut out: (
        MessageWriter<SfxEvent>,
        MessageWriter<EnemyDied>,
        MessageWriter<PlayerHit>,
    ),
) {
    if pause.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let Ok(pt) = player.single() else { return };
    let up = upstairs.as_deref().is_some_and(|u| u.active());
    let pp = upstairs
        .as_deref()
        .and_then(|u| u.landing())
        .unwrap_or(pt.translation.xz());
    for (e, mut s) in &mut q {
        s.t -= dt;
        if !s.whistled && s.t < 1.6 {
            s.whistled = true;
            out.0.write(SfxEvent::Whistle {
                dist: pp.distance(s.p),
            });
        }
        if s.t > 0.0 {
            continue;
        }
        commands.entity(e).despawn();
        commands.entity(s.ring).try_despawn();
        let w = s.w;
        let p = s.p;
        if let Some(fx) = fx.as_deref() {
            combat::standin::spawn_blast(&mut commands, fx, p);
        }
        let indoor = up || world.0.building_at(pp).is_some();
        out.0.write(SfxEvent::ShellBoom {
            dist: pp.distance(p),
            indoor,
        });
        for (ent, mut o, tf) in &mut enemies {
            let d = o.0.pos.distance(p);
            if o.0.alive() && d < w.wound {
                let dmg = if d < w.core {
                    300.0
                } else {
                    weapon_dmg(&w, d) * 1.5
                };
                if combat::soldier::damage(&mut o.0, dmg, p) {
                    commands.entity(ent).insert(Body::new(o.0.kind));
                    out.1.write(EnemyDied {
                        id: o.0.id,
                        kind: o.0.kind,
                        pos: tf.translation,
                    });
                }
            }
            combat::soldier::noise(
                std::iter::once(&mut o.0),
                p,
                if w.mortar { 60.0 } else { w.wound * 2.0 },
                env.rain,
                env.night,
                &mut rng.0,
            );
        }
        let dp = pp.distance(p);
        if !w.mortar {
            stats.craters += 1;
            commands.spawn((
                Crater {
                    t: 45.0,
                    r: w.core * 0.35,
                    puff: 0.0,
                    seed: p.x * 0.13 + p.y * 0.07,
                },
                Mesh3d(art.crater.clone()),
                MeshMaterial3d(art.crater_mat.clone()),
                Transform::from_xyz(p.x, 0.03, p.y)
                    .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::splat(w.core * 0.45)),
                bevy::light::NotShadowCaster,
            ));
            notice.text = match *lang {
                Language::ZhTw => format!(
                    "{}落在約 {} 公尺外，地面在晃，玻璃全碎了。",
                    w.zh,
                    dp.round()
                ),
                Language::En => format!(
                    "{} hit about {} m from you. The ground shakes and windows blow in.",
                    w.en,
                    dp.round()
                ),
            };
        }
        let mut dmg = weapon_dmg(&w, dp);
        if dmg <= 0.0 || game.over {
            continue;
        }
        // indoors the walls stop almost all fragments; a close missile comes through
        if indoor {
            dmg *= if w.mortar {
                0.15
            } else if dp < w.core {
                0.6
            } else {
                0.25
            };
        } else if world.0.cover_near(pp, 1.4) {
            dmg *= 0.5;
        }
        game.hp -= dmg;
        let bleed = dmg > 15.0 && !game.bleed;
        game.bleed |= bleed;
        out.2.write(PlayerHit {
            dmg,
            cause: HitCause::Grenade,
            bleed,
        });
        if bleed {
            notice.text = tr(
                *lang,
                "被砲彈破片擊中，流血了！",
                "Shell fragments hit you. You are bleeding!",
            )
            .into();
        }
    }
}

fn craters(
    mut commands: Commands,
    time: Res<Time>,
    pause: Res<PauseOpen>,
    art: Res<WarArt>,
    mut q: Query<(Entity, &mut Crater, &Transform), Without<Flame>>,
    mut flames: Query<(Entity, &mut Flame, &mut Transform), Without<Crater>>,
) {
    if pause.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    for (e, mut c, tf) in &mut q {
        c.t -= dt;
        if c.t <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        c.puff -= dt;
        if c.puff <= 0.0 {
            c.puff = 0.28;
            c.seed += 1.618;
            let a = (c.seed * 12.9898).sin() * std::f32::consts::TAU;
            let r = (c.seed * 78.233).sin().abs() * c.r;
            let p = tf.translation.xz() + Vec2::from_angle(a) * r;
            commands.spawn((
                Flame(0.8),
                Mesh3d(art.flame.clone()),
                MeshMaterial3d(art.flame_mat.clone()),
                Transform::from_xyz(p.x, 0.5, p.y),
                bevy::light::NotShadowCaster,
            ));
        }
    }
    for (e, mut f, mut tf) in &mut flames {
        f.0 -= dt;
        if f.0 <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        tf.translation.y += dt * 1.6;
        let k = 1.2 + (0.8 - f.0) * 1.25;
        tf.scale = Vec3::splat(k * (f.0 / 0.8).sqrt().max(0.2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn war_level_rises_over_a_week_and_with_reclaimed_areas() {
        assert_eq!(war_level(0.0, 0), 0.0);
        assert!(war_level(1440.0 * 0.25, 0) <= 1e-6);
        let day2 = war_level(1440.0 * 1.25, 0);
        assert!((day2 - 1.0 / 6.0).abs() < 1e-5);
        assert!(war_level(1440.0 * 1.25, 4) > day2);
        assert_eq!(war_level(1440.0 * 30.0, 0), 1.0);
        // shells start about the first evening
        assert!(war_level(1440.0 * 0.8, 0) < WAR_START);
        assert!(war_level(1440.0 * 0.9, 0) >= WAR_START);
    }

    #[test]
    fn damage_falls_off_through_core_and_wound_rings() {
        for w in [MORTAR, ROCKET, SRBM, CRUISE] {
            assert_eq!(weapon_dmg(&w, 0.0), w.core_dmg);
            assert!((weapon_dmg(&w, w.core - 1e-3) - w.core_dmg * 0.6).abs() < 0.1);
            assert!(weapon_dmg(&w, w.core + 1.0) < w.wound_dmg);
            assert_eq!(weapon_dmg(&w, w.wound), 0.0);
        }
        assert_eq!(grenade_dmg_player(0.0), 95.0);
        assert!(grenade_dmg_player(10.0) > 0.0);
        assert_eq!(grenade_dmg_player(GREN_WOUND), 0.0);
    }
}
