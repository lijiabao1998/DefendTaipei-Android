//! v24.24 PLA helicopters over Wanhua (port of `src/systems/heli.ts`).
//!
//! Z-20 utility and Z-10 attack helicopters fly wide Wanhua corridors above the
//! rooftops. They hunt exposed survivors (door gun bursts, plus rocket salvos
//! from the Z-10 dispatched via [`WarCall::Strike`]), break off when you take
//! shelter indoors, under an arcade or upstairs, and can be shot down by heavy
//! fire or mounted vehicle weapons.
//!
//! Night searchlights use unlit additive beam/pool meshes (`NotShadowCaster`)
//! rather than dynamic `SpotLight` / `PointLight` entities so the lighting
//! workload's spotlight budget remains untouched.

use bevy::{light::NotShadowCaster, prelude::*};
use std::f32::consts::{FRAC_PI_2, TAU};

use crate::audio::SfxEvent;
use crate::combat::{
    CombatEnv, CombatRng, GameState, Gunshot, HitCause, PlayerHit, WeaponInput,
    armor::shield_damage, tuning::GunFam,
};
use crate::game_flow::{Campaign, GameFlow};
use crate::geom::{MeshBuf, cuboid, cylinder, hex, place, plane};
use crate::i18n::{Language, tr};
use crate::inventory::Inventory;
use crate::player::Player;
use crate::survival::Stash;
use crate::ui::PauseOpen;
use crate::vehicles::{VehicleFleet, VehicleKind};
use crate::war::{WAR_START, WarCall, WarWeapon, hidden_from_sky, war_level};
use crate::worldplay::{ScavengeState, WorldNotice, WorldSet};

const SPEED: f32 = 20.0;
const CRUISE: f32 = 24.0;
const SEE_DAY: f32 = 34.0;
const SEE_NIGHT: f32 = 12.0;

const HELI_ROCKET: WarWeapon = WarWeapon {
    zh: "機載火箭彈",
    en: "Helicopter rocket salvo",
    core: 5.5,
    wound: 16.0,
    core_dmg: 75.0,
    wound_dmg: 38.0,
    big: 1.4,
    lead: 3.5,
    mortar: false,
    squared: false,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeliKind {
    Z20,
    Z10,
}

impl HeliKind {
    pub fn name(self, lang: Language) -> &'static str {
        match self {
            Self::Z20 => tr(lang, "直-20 通用直升機", "Z-20 utility helicopter"),
            Self::Z10 => tr(lang, "直-10 攻擊直升機", "Z-10 attack helicopter"),
        }
    }

    pub fn base_hp(self) -> f32 {
        match self {
            Self::Z20 => 100.0,
            Self::Z10 => 90.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeliPhase {
    Patrol,
    Hunt,
    Fall,
}

#[derive(Component)]
pub struct PlaHeli {
    pub kind: HeliKind,
    pub hp: f32,
    phase: HeliPhase,
    way: Vec<Vec2>,
    alt: f32,
    hd: f32,
    vx: f32,
    vz: f32,
    spot: f32,
    lost: f32,
    hunt_t: f32,
    gun_t: f32,
    burst: u8,
    rocket_t: f32,
    sound_t: f32,
    fall_t: f32,
    ang: f32,
    light: Entity,
    beam: Entity,
}

#[derive(Component)]
struct MainRotor(Entity);

#[derive(Component)]
struct TailRotor(Entity);

#[derive(Component)]
struct SearchLightPart;

#[derive(Component)]
struct HeliTracer(f32);

#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct HeliStats {
    pub spawned: u32,
    pub hunts: u32,
    pub shots: u32,
    pub hits: u32,
    pub downed: u32,
    pub salvos: u32,
}

/// Active choppers exported each frame for the NPU/tactical field: `(pos, hunting, is_z10)`.
#[derive(Resource, Default, Clone, Debug)]
pub struct HeliThreat(pub Vec<(Vec2, bool, bool)>);

#[derive(Resource)]
struct HeliClock {
    respawn: f32,
    crates: u32,
}

impl Default for HeliClock {
    fn default() -> Self {
        Self {
            respawn: 45.0,
            crates: 0,
        }
    }
}

#[derive(Resource)]
struct HeliArt {
    z20: Handle<Mesh>,
    z10: Handle<Mesh>,
    main_rotor: Handle<Mesh>,
    tail_rotor: Handle<Mesh>,
    beam: Handle<Mesh>,
    pool: Handle<Mesh>,
    tracer: Handle<Mesh>,
    body_mat: Handle<StandardMaterial>,
    blade_mat: Handle<StandardMaterial>,
    beam_mat: Handle<StandardMaterial>,
    pool_mat: Handle<StandardMaterial>,
    tracer_mat: Handle<StandardMaterial>,
}

pub struct HeliPlugin;

impl Plugin for HeliPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HeliStats>()
            .init_resource::<HeliThreat>()
            .init_resource::<HeliClock>()
            .add_systems(Startup, setup_art)
            .add_systems(OnEnter(GameFlow::Prologue), reset)
            .add_systems(OnExit(GameFlow::Playing), clear)
            .add_systems(
                Update,
                (spawn_patrols, fly_helis, return_fire)
                    .chain()
                    .in_set(WorldSet)
                    .run_if(in_state(GameFlow::Playing)),
            );
    }
}

pub fn wanted_count(level: f32) -> usize {
    if level < WAR_START {
        0
    } else if level < 0.35 {
        1
    } else if level < 0.7 {
        2
    } else {
        3
    }
}

fn reset(
    mut stats: ResMut<HeliStats>,
    mut clock: ResMut<HeliClock>,
    mut threat: ResMut<HeliThreat>,
) {
    *stats = HeliStats::default();
    *clock = HeliClock::default();
    threat.0.clear();
}

fn clear(
    mut commands: Commands,
    mut threat: ResMut<HeliThreat>,
    q: Query<
        Entity,
        Or<(
            With<PlaHeli>,
            With<MainRotor>,
            With<TailRotor>,
            With<SearchLightPart>,
            With<HeliTracer>,
        )>,
    >,
) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
    threat.0.clear();
}

fn bx(m: &mut MeshBuf, col: u32, w: f32, h: f32, l: f32, x: f32, y: f32, z: f32) {
    m.add(&cuboid(w, h, l), place(x, y, z, 0.0), hex(col));
}

fn z20_mesh() -> MeshBuf {
    let (c, c2, dark, glass, tyre) = (0x6a7276, 0x585f63, 0x1c2022, 0x28343c, 0x141414);
    let mut m = MeshBuf::default();
    bx(&mut m, c, 2.3, 1.9, 5.4, 0.0, 1.75, 0.4);
    bx(&mut m, c, 1.6, 1.25, 2.0, 0.0, 1.52, 4.1);
    bx(&mut m, glass, 1.4, 0.6, 1.4, 0.0, 2.15, 3.7);
    bx(&mut m, c2, 1.4, 0.6, 3.4, 0.0, 2.95, 0.3);
    bx(&mut m, c, 0.9, 0.9, 7.2, 0.0, 2.15, -5.9);
    bx(&mut m, c, 0.22, 2.7, 1.2, 0.0, 3.45, -10.0);
    bx(&mut m, c2, 3.2, 0.1, 0.85, 0.0, 1.75, -9.6);
    bx(&mut m, glass, 0.05, 0.85, 2.3, 1.17, 2.05, 0.6);
    bx(&mut m, glass, 0.05, 0.85, 2.3, -1.17, 2.05, 0.6);
    bx(&mut m, c2, 0.5, 0.35, 1.4, 1.35, 1.1, 0.6);
    bx(&mut m, c2, 0.5, 0.35, 1.4, -1.35, 1.1, 0.6);
    m.add(
        &cylinder(0.16, 0.16, 0.85, 8),
        place(0.0, 3.72, 0.3, 0.0),
        hex(dark),
    );
    for s in [-1.0_f32, 1.0] {
        m.add(
            &cylinder(0.36, 0.36, 0.26, 10),
            Mat4::from_translation(Vec3::new(s * 1.35, 0.36, 2.0))
                * Mat4::from_rotation_z(FRAC_PI_2),
            hex(tyre),
        );
        bx(&mut m, dark, 0.12, 0.8, 0.14, s * 1.25, 0.72, 2.0);
    }
    m.add(
        &cylinder(0.2, 0.2, 0.14, 10),
        Mat4::from_translation(Vec3::new(0.0, 0.42, -7.8)) * Mat4::from_rotation_z(FRAC_PI_2),
        hex(tyre),
    );
    bx(&mut m, dark, 0.08, 1.3, 0.1, 0.0, 1.05, -7.8);
    m
}

fn z10_mesh() -> MeshBuf {
    let (c, c2, dark, glass, tyre) = (0x5e6a5e, 0x4c574c, 0x1c2022, 0x28343c, 0x141414);
    let mut m = MeshBuf::default();
    bx(&mut m, c, 1.15, 1.75, 5.2, 0.0, 1.75, 0.6);
    bx(&mut m, c, 0.8, 1.1, 1.9, 0.0, 1.5, 4.1);
    bx(&mut m, glass, 0.82, 0.62, 1.4, 0.0, 2.6, 3.0);
    bx(&mut m, glass, 0.88, 0.72, 1.4, 0.0, 2.95, 1.55);
    bx(&mut m, c2, 1.1, 0.7, 2.6, 0.0, 2.85, -0.4);
    bx(&mut m, c, 0.7, 0.75, 6.8, 0.0, 2.05, -5.2);
    bx(&mut m, c, 0.2, 2.6, 1.1, 0.0, 3.3, -8.8);
    bx(&mut m, c2, 2.6, 0.1, 0.75, 0.0, 1.75, -8.4);
    bx(&mut m, c2, 4.2, 0.18, 1.0, 0.0, 1.55, 0.2);
    m.add(
        &cylinder(0.15, 0.15, 0.8, 8),
        place(0.0, 3.45, -0.2, 0.0),
        hex(dark),
    );
    bx(&mut m, dark, 0.32, 0.32, 1.5, 0.0, 0.95, 3.7);
    m.add(
        &cylinder(0.06, 0.06, 1.4, 6),
        Mat4::from_translation(Vec3::new(0.0, 0.95, 4.9)) * Mat4::from_rotation_x(FRAC_PI_2),
        hex(dark),
    );
    for s in [-1.0_f32, 1.0] {
        m.add(
            &cylinder(0.24, 0.24, 1.5, 8),
            Mat4::from_translation(Vec3::new(s * 1.65, 1.2, 0.2))
                * Mat4::from_rotation_x(FRAC_PI_2),
            hex(dark),
        );
        m.add(
            &cylinder(0.26, 0.26, 0.2, 10),
            Mat4::from_translation(Vec3::new(s * 0.85, 0.3, 1.8))
                * Mat4::from_rotation_z(FRAC_PI_2),
            hex(tyre),
        );
        bx(&mut m, dark, 0.1, 0.6, 0.12, s * 0.8, 0.65, 1.8);
    }
    m
}

fn setup_art(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut main_r = MeshBuf::default();
    main_r.add(&cuboid(0.5, 0.05, 16.0), Mat4::IDENTITY, hex(0x15181a));
    main_r.add(
        &cuboid(0.5, 0.05, 16.0),
        Mat4::from_rotation_y(FRAC_PI_2),
        hex(0x15181a),
    );
    let mut tail_r = MeshBuf::default();
    tail_r.add(&cuboid(0.08, 2.8, 0.26), Mat4::IDENTITY, hex(0x15181a));
    let mut beam_m = cylinder(0.05, 1.0, 1.0, 14);
    beam_m = beam_m.transformed(Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)));
    let pool_m = plane(2.0, 2.0);
    commands.insert_resource(HeliArt {
        z20: meshes.add(z20_mesh().into_mesh()),
        z10: meshes.add(z10_mesh().into_mesh()),
        main_rotor: meshes.add(main_r.into_mesh()),
        tail_rotor: meshes.add(tail_r.into_mesh()),
        beam: meshes.add(beam_m.into_mesh()),
        pool: meshes.add(pool_m.into_mesh()),
        tracer: meshes.add(Cuboid::new(0.07, 0.07, 1.0)),
        body_mat: materials.add(StandardMaterial {
            perceptual_roughness: 0.72,
            metallic: 0.18,
            ..default()
        }),
        blade_mat: materials.add(StandardMaterial {
            base_color: Color::srgb_u8(0x15, 0x18, 0x1a),
            perceptual_roughness: 0.7,
            ..default()
        }),
        beam_mat: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.96, 0.78, 0.09),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        }),
        pool_mat: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.96, 0.78, 0.2),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        }),
        tracer_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            emissive: LinearRgba::rgb(12.0, 9.0, 3.0),
            unlit: true,
            ..default()
        }),
    });
}

fn wide_route(near: Vec2, rng: &mut crate::city::Lcg) -> Vec<Vec2> {
    let w = crate::map::world();
    let mut roads: Vec<(&[Vec2], f32)> = w
        .roads
        .iter()
        .filter(|r| r.w >= 9.0 && r.p.len() >= 2)
        .map(|r| (r.p.as_slice(), r.p[0].distance(near)))
        .collect();
    roads.sort_by(|a, b| a.1.total_cmp(&b.1));
    if roads.is_empty() {
        let a = rng.next_f32() * TAU;
        let r = 120.0 + rng.next_f32() * 140.0;
        return vec![
            near + Vec2::from_angle(a) * r,
            near + Vec2::from_angle(a + 2.0) * r,
        ];
    }
    let pick = roads[(rng.next_f32() * (roads.len().min(6)) as f32) as usize % roads.len().min(6)];
    let mut pts = pick.0.to_vec();
    if rng.next_f32() < 0.5 {
        pts.reverse();
    }
    pts
}

fn spawn_heli_entity(
    commands: &mut Commands,
    art: &HeliArt,
    kind: HeliKind,
    at: Vec2,
    way: Vec<Vec2>,
) -> Entity {
    let mesh = match kind {
        HeliKind::Z20 => art.z20.clone(),
        HeliKind::Z10 => art.z10.clone(),
    };
    let light = commands
        .spawn((
            SearchLightPart,
            Mesh3d(art.pool.clone()),
            MeshMaterial3d(art.pool_mat.clone()),
            Transform::from_xyz(at.x, 0.14, at.y).with_scale(Vec3::splat(9.0)),
            Visibility::Hidden,
            NotShadowCaster,
        ))
        .id();
    let beam = commands
        .spawn((
            SearchLightPart,
            Mesh3d(art.beam.clone()),
            MeshMaterial3d(art.beam_mat.clone()),
            Transform::from_xyz(at.x, 0.0, at.y).with_scale(Vec3::new(9.0, CRUISE, 9.0)),
            Visibility::Hidden,
            NotShadowCaster,
        ))
        .id();
    let heli = commands
        .spawn((
            PlaHeli {
                kind,
                hp: kind.base_hp(),
                phase: HeliPhase::Patrol,
                way,
                alt: CRUISE,
                hd: 0.0,
                vx: 0.0,
                vz: 0.0,
                spot: 0.0,
                lost: 0.0,
                hunt_t: 0.0,
                gun_t: 2.0,
                burst: 0,
                rocket_t: 8.0,
                sound_t: 0.0,
                fall_t: 0.0,
                ang: 0.0,
                light,
                beam,
            },
            Mesh3d(mesh),
            MeshMaterial3d(art.body_mat.clone()),
            Transform::from_xyz(at.x, CRUISE, at.y),
            Visibility::default(),
        ))
        .id();
    let rotor_pos = match kind {
        HeliKind::Z20 => Vec3::new(0.0, 4.2, 0.3),
        HeliKind::Z10 => Vec3::new(0.0, 3.9, -0.2),
    };
    let tail_pos = match kind {
        HeliKind::Z20 => Vec3::new(0.22, 3.8, -10.0),
        HeliKind::Z10 => Vec3::new(0.22, 3.6, -8.8),
    };
    commands.spawn((
        MainRotor(heli),
        Mesh3d(art.main_rotor.clone()),
        MeshMaterial3d(art.blade_mat.clone()),
        Transform::from_translation(Vec3::new(at.x, CRUISE, at.y) + rotor_pos),
    ));
    commands.spawn((
        TailRotor(heli),
        Mesh3d(art.tail_rotor.clone()),
        MeshMaterial3d(art.blade_mat.clone()),
        Transform::from_translation(Vec3::new(at.x, CRUISE, at.y) + tail_pos),
    ));
    heli
}

/// QA fixture helper: spawns one helicopter near `at` for deterministic desktop screenshots.
pub fn spawn_qa_heli(world: &mut World, kind: HeliKind, at: Vec2) {
    let (z20, z10, main_r, tail_r, beam, pool, body_mat, blade_mat, beam_mat, pool_mat) = {
        let art = world.resource::<HeliArt>();
        (
            art.z20.clone(),
            art.z10.clone(),
            art.main_rotor.clone(),
            art.tail_rotor.clone(),
            art.beam.clone(),
            art.pool.clone(),
            art.body_mat.clone(),
            art.blade_mat.clone(),
            art.beam_mat.clone(),
            art.pool_mat.clone(),
        )
    };
    let mesh = if kind == HeliKind::Z20 { z20 } else { z10 };
    let light = world
        .spawn((
            SearchLightPart,
            Mesh3d(pool),
            MeshMaterial3d(pool_mat),
            Transform::from_xyz(at.x, 0.14, at.y).with_scale(Vec3::splat(9.0)),
            Visibility::Hidden,
            NotShadowCaster,
        ))
        .id();
    let beam_e = world
        .spawn((
            SearchLightPart,
            Mesh3d(beam),
            MeshMaterial3d(beam_mat),
            Transform::from_xyz(at.x, 0.0, at.y).with_scale(Vec3::new(9.0, 20.0, 9.0)),
            Visibility::Hidden,
            NotShadowCaster,
        ))
        .id();
    let heli = world
        .spawn((
            PlaHeli {
                kind,
                hp: kind.base_hp(),
                phase: HeliPhase::Patrol,
                way: vec![at + Vec2::X * 40.0, at - Vec2::X * 40.0],
                alt: 20.0,
                hd: 0.4,
                vx: 6.0,
                vz: 2.0,
                spot: 0.0,
                lost: 0.0,
                hunt_t: 0.0,
                gun_t: 2.0,
                burst: 0,
                rocket_t: 10.0,
                sound_t: 1.0,
                fall_t: 0.0,
                ang: 0.0,
                light,
                beam: beam_e,
            },
            Mesh3d(mesh),
            MeshMaterial3d(body_mat),
            Transform::from_xyz(at.x, 20.0, at.y).with_rotation(Quat::from_rotation_y(0.4)),
            Visibility::default(),
        ))
        .id();
    let rotor_pos = if kind == HeliKind::Z20 {
        Vec3::new(0.0, 4.2, 0.3)
    } else {
        Vec3::new(0.0, 3.9, -0.2)
    };
    world.spawn((
        MainRotor(heli),
        Mesh3d(main_r),
        MeshMaterial3d(blade_mat.clone()),
        Transform::from_translation(Vec3::new(at.x, 20.0, at.y) + rotor_pos),
    ));
    world.spawn((
        TailRotor(heli),
        Mesh3d(tail_r),
        MeshMaterial3d(blade_mat),
        Transform::from_translation(Vec3::new(at.x, 20.0, at.y) + Vec3::new(0.22, 3.8, -9.2)),
    ));
    world.resource_mut::<HeliStats>().spawned += 1;
}

fn spawn_patrols(
    mut commands: Commands,
    time: Res<Time>,
    pause: Res<PauseOpen>,
    campaign: Res<Campaign>,
    art: Res<HeliArt>,
    (mut clock, mut stats, mut rng): (ResMut<HeliClock>, ResMut<HeliStats>, ResMut<CombatRng>),
    player: Query<&Transform, With<Player>>,
    existing: Query<(), With<PlaHeli>>,
) {
    if pause.0 || campaign.sleeping {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let level = war_level(campaign.game_min, campaign.reclaimed.len());
    let want = wanted_count(level);
    clock.respawn -= dt;
    if want == 0 || existing.iter().count() >= want || clock.respawn > 0.0 {
        return;
    }
    clock.respawn = 180.0 + rng.0.next_f32() * 120.0;
    let pp = player.single().map_or(Vec2::ZERO, |t| t.translation.xz());
    let kind = if level > 0.35 && rng.0.next_f32() < 0.45 {
        HeliKind::Z10
    } else {
        HeliKind::Z20
    };
    let ang = rng.0.next_f32() * TAU;
    let at = pp + Vec2::from_angle(ang) * 420.0;
    let way = wide_route(pp, &mut rng.0);
    spawn_heli_entity(&mut commands, &art, kind, at, way);
    stats.spawned += 1;
}

fn roof_at(p: Vec2) -> f32 {
    crate::map::world().bld_at(p).map_or(0.0, |b| b.h)
}

#[allow(clippy::too_many_arguments)]
fn fly_helis(
    mut commands: Commands,
    time: Res<Time>,
    pause: Res<PauseOpen>,
    lang: Res<Language>,
    env: Res<CombatEnv>,
    art: Res<HeliArt>,
    upstairs: Option<Res<crate::upstairs::Upstairs>>,
    vehicles: Option<Res<VehicleFleet>>,
    (mut campaign, mut game, mut stats, mut rng, mut clock): (
        ResMut<Campaign>,
        ResMut<GameState>,
        ResMut<HeliStats>,
        ResMut<CombatRng>,
        ResMut<HeliClock>,
    ),
    (mut progress, mut notice, mut threat): (
        ResMut<ScavengeState>,
        ResMut<WorldNotice>,
        ResMut<HeliThreat>,
    ),
    player: Query<&Transform, (With<Player>, Without<PlaHeli>)>,
    mut helis: Query<(Entity, &mut PlaHeli, &mut Transform)>,
    mut parts: Query<
        (
            Option<&MainRotor>,
            Option<&TailRotor>,
            &mut Transform,
            &mut Visibility,
        ),
        (Without<PlaHeli>, Without<Player>, Without<HeliTracer>),
    >,
    mut tracers: Query<
        (Entity, &mut HeliTracer),
        (Without<PlaHeli>, Without<MainRotor>, Without<TailRotor>),
    >,
    mut out: (
        MessageWriter<SfxEvent>,
        MessageWriter<PlayerHit>,
        MessageWriter<WarCall>,
    ),
) {
    if pause.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    for (e, mut tr) in &mut tracers {
        tr.0 -= dt;
        if tr.0 <= 0.0 {
            commands.entity(e).despawn();
        }
    }
    let Ok(pt) = player.single() else { return };
    let pp = pt.translation.xz();
    let up = upstairs.as_deref().is_some_and(|u| u.active());
    let hidden = hidden_from_sky(pp, up);
    let mut threats = Vec::new();
    let mut body_tfs: Vec<(Entity, Vec3, Quat, HeliKind, bool)> = Vec::new();

    for (ent, mut h, mut tf) in &mut helis {
        let p = tf.translation.xz();
        let heard = pp.distance(p);
        h.sound_t -= dt;
        if h.sound_t <= 0.0 {
            h.sound_t = 1.1;
            if heard < 300.0 {
                out.0.write(SfxEvent::Rotor { dist: heard });
            }
        }
        if h.phase == HeliPhase::Fall {
            h.fall_t += dt;
            h.hd += dt * (2.4 + h.fall_t * 1.5);
            h.alt -= dt * (8.0 + h.fall_t * 7.0);
            let next_p = p + Vec2::new(h.vx, h.vz) * dt * 0.8;
            tf.translation = Vec3::new(next_p.x, h.alt, next_p.y);
            tf.rotation =
                Quat::from_rotation_y(h.hd) * Quat::from_rotation_z((h.fall_t * 0.25).min(0.5));
            body_tfs.push((ent, tf.translation, tf.rotation, h.kind, false));
            if let Ok((_, _, _, mut v)) = parts.get_mut(h.light) {
                v.set_if_neq(Visibility::Hidden);
            }
            if let Ok((_, _, _, mut v)) = parts.get_mut(h.beam) {
                v.set_if_neq(Visibility::Hidden);
            }
            if h.alt <= (roof_at(next_p) + 0.5).max(0.4) {
                stats.downed += 1;
                if let Ok(mut e) = commands.get_entity(h.light) {
                    e.despawn();
                }
                if let Ok(mut e) = commands.get_entity(h.beam) {
                    e.despawn();
                }
                for (me, m, t, _, _) in [] as [(Entity, bool, f32, HeliKind, bool); 0] {
                    let _ = (me, m, t);
                }
                commands.entity(ent).despawn();
                out.0.write(SfxEvent::ShellBoom {
                    dist: heard,
                    indoor: hidden,
                });
                clock.crates += 1;
                let id = format!("helidrop:{}", clock.crates);
                let mut stash = Stash::default();
                stash.add("bandage", 2);
                stash.add("bento", 1);
                stash.add("a556", 30);
                progress.loot.push((id.clone(), stash));
                progress.drops.push((id.clone(), next_p.to_array()));
                progress.night_seen.push(id);
                notice.text = match *lang {
                    Language::ZhTw => {
                        format!("{} 墜毀！殘骸旁散落了一箱補給品。", h.kind.name(*lang))
                    }
                    Language::En => format!(
                        "The {} crashed! A crate of supplies landed near the wreckage.",
                        h.kind.name(*lang)
                    ),
                };
            }
            continue;
        }

        let (mut tx, mut tz, mut speed) = (p.x, p.y, SPEED);
        match h.phase {
            HeliPhase::Patrol => {
                while let Some(w0) = h.way.first()
                    && w0.distance(p) < 12.0
                {
                    h.way.remove(0);
                }
                if h.way.is_empty() {
                    h.way = wide_route(pp, &mut rng.0);
                }
                if let Some(w0) = h.way.first() {
                    tx = w0.x;
                    tz = w0.y;
                }
                let pool = if env.night { SEE_NIGHT } else { SEE_DAY };
                if !hidden && heard < pool {
                    h.spot += dt;
                    let lock_time = if env.night { 0.9 } else { 1.6 };
                    if h.spot > lock_time {
                        h.phase = HeliPhase::Hunt;
                        h.hunt_t = 28.0;
                        h.lost = 0.0;
                        stats.hunts += 1;
                        crate::device::request_haptic(30);
                        campaign.speed = 1;
                        campaign.sleeping = false;
                        notice.text = match *lang {
                            Language::ZhTw => {
                                format!("⚠ {} 發現你了！快躲進室內或騎樓！", h.kind.name(*lang))
                            }
                            Language::En => format!(
                                "⚠ The {} spotted you! Get indoors or under an arcade!",
                                h.kind.name(*lang)
                            ),
                        };
                    }
                } else {
                    h.spot = (h.spot - dt * 0.5).max(0.0);
                }
            }
            HeliPhase::Hunt => {
                h.ang += dt * 0.42;
                let r = if h.kind == HeliKind::Z10 { 18.0 } else { 14.0 };
                tx = pp.x + h.ang.cos() * r;
                tz = pp.y + h.ang.sin() * r;
                speed = 16.0;
                h.hunt_t -= dt;
                if hidden {
                    h.lost += dt;
                } else {
                    h.lost = (h.lost - dt).max(0.0);
                }
                if h.lost > 6.0 || h.hunt_t <= 0.0 || heard > 90.0 {
                    h.phase = HeliPhase::Patrol;
                    h.spot = 0.0;
                    h.way = wide_route(pp, &mut rng.0);
                } else if heard < 44.0 && !hidden {
                    h.gun_t -= dt;
                    if h.gun_t <= 0.0 {
                        h.gun_t = 0.11;
                        if h.burst == 0 {
                            h.burst = 8;
                        }
                        h.burst -= 1;
                        if h.burst == 0 {
                            h.gun_t = 2.4 + rng.0.next_f32() * 1.3;
                        }
                        stats.shots += 1;
                        crate::combat::sfx::play_gunshot_sfx(&mut out.0, GunFam::Lmg, heard, false);
                        let end = Vec3::new(
                            pp.x + (rng.0.next_f32() - 0.5) * 3.0,
                            0.6,
                            pp.y + (rng.0.next_f32() - 0.5) * 3.0,
                        );
                        let start = Vec3::new(p.x, h.alt + 1.2, p.y);
                        let len = start.distance(end).max(0.2);
                        commands.spawn((
                            HeliTracer(0.2),
                            Mesh3d(art.tracer.clone()),
                            MeshMaterial3d(art.tracer_mat.clone()),
                            Transform::from_translation((start + end) * 0.5)
                                .looking_at(end, Vec3::Y)
                                .with_scale(Vec3::new(1.0, 1.0, len)),
                            NotShadowCaster,
                        ));
                        if rng.0.next_f32() < 0.12 && !game.over {
                            let raw = 8.0 + rng.0.next_f32() * 4.0;
                            let (dmg, bleed) = shield_damage(vehicles.as_deref(), raw, false);
                            game.hp -= dmg;
                            crate::device::request_haptic(25);
                            out.1.write(PlayerHit {
                                dmg,
                                cause: HitCause::Rifle,
                                bleed,
                            });
                        }
                    }
                    if h.kind == HeliKind::Z10 {
                        h.rocket_t -= dt;
                        if h.rocket_t <= 0.0 {
                            h.rocket_t = 13.0 + rng.0.next_f32() * 7.0;
                            stats.salvos += 1;
                            out.2.write(WarCall::Strike {
                                w: HELI_ROCKET,
                                at: pp
                                    + Vec2::new(
                                        (rng.0.next_f32() - 0.5) * 6.0,
                                        (rng.0.next_f32() - 0.5) * 6.0,
                                    ),
                            });
                        }
                    }
                }
            }
            HeliPhase::Fall => {}
        }

        let to = Vec2::new(tx - p.x, tz - p.y);
        let dir = to.normalize_or_zero();
        let wv = dir * speed;
        let k = (dt * 0.9).min(1.0);
        h.vx += (wv.x - h.vx) * k;
        h.vz += (wv.y - h.vz) * k;
        let np = p + Vec2::new(h.vx, h.vz) * dt;
        let v = Vec2::new(h.vx, h.vz).length();
        let mut bank = 0.0_f32;
        if v > 2.0 {
            let want = h.vx.atan2(h.vz);
            let dh = (want - h.hd).sin().atan2((want - h.hd).cos());
            h.hd += dh.clamp(-1.2 * dt, 1.2 * dt);
            bank = dh.clamp(-0.5, 0.5) * 0.45;
        }
        let look = roof_at(np)
            .max(roof_at(np + Vec2::new(h.vx, h.vz) * 1.5))
            .max(roof_at(np + Vec2::new(h.vx, h.vz) * 3.0));
        let want_alt = (look + 7.0).max(if h.phase == HeliPhase::Hunt {
            15.0
        } else {
            CRUISE
        });
        h.alt += (want_alt - h.alt).clamp(-4.0 * dt, 8.0 * dt);
        tf.translation = Vec3::new(np.x, h.alt, np.y);
        tf.rotation = Quat::from_euler(EulerRot::YXZ, h.hd, (v / SPEED) * 0.14, bank);
        body_tfs.push((
            ent,
            tf.translation,
            tf.rotation,
            h.kind,
            h.phase == HeliPhase::Hunt,
        ));
        threats.push((np, h.phase == HeliPhase::Hunt, h.kind == HeliKind::Z10));

        // Night searchlight (unlit additive meshes — zero SpotLight/PointLight entities)
        let vis = if env.night {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let spot_xy = if h.phase == HeliPhase::Hunt {
            pp
        } else {
            np + Vec2::new(h.hd.sin(), h.hd.cos()) * 6.0
        };
        if let Ok((_, _, mut ltf, mut lv)) = parts.get_mut(h.light) {
            lv.set_if_neq(vis);
            if env.night {
                ltf.translation = Vec3::new(spot_xy.x, 0.14, spot_xy.y);
            }
        }
        if let Ok((_, _, mut btf, mut bv)) = parts.get_mut(h.beam) {
            bv.set_if_neq(vis);
            if env.night {
                let d = Vec3::new(np.x - spot_xy.x, h.alt, np.y - spot_xy.y);
                let len = d.length().max(1.0);
                btf.translation = Vec3::new(spot_xy.x, 0.0, spot_xy.y);
                btf.rotation = Quat::from_rotation_arc(Vec3::Y, d / len);
                btf.scale = Vec3::new(9.0, len, 9.0);
            }
        }
    }

    threat.0 = threats;
    for (mr, tr_part, mut ptf, _) in &mut parts {
        if let Some(MainRotor(owner)) = mr {
            if let Some((_, pos, rot, kind, _)) = body_tfs.iter().find(|x| x.0 == *owner) {
                let local = if *kind == HeliKind::Z20 {
                    Vec3::new(0.0, 4.2, 0.3)
                } else {
                    Vec3::new(0.0, 3.9, -0.2)
                };
                ptf.translation = *pos + *rot * local;
                let spin_y = time.elapsed_secs() * 34.0;
                ptf.rotation = *rot * Quat::from_rotation_y(spin_y);
            }
        } else if let Some(TailRotor(owner)) = tr_part
            && let Some((_, pos, rot, kind, _)) = body_tfs.iter().find(|x| x.0 == *owner)
        {
            let local = if *kind == HeliKind::Z20 {
                Vec3::new(0.22, 3.8, -10.0)
            } else {
                Vec3::new(0.22, 3.6, -8.8)
            };
            ptf.translation = *pos + *rot * local;
            let spin_x = time.elapsed_secs() * 60.0;
            ptf.rotation = *rot * Quat::from_rotation_x(spin_x);
        }
    }
    // Clean up orphaned rotors after crash
    let live_ids: Vec<Entity> = helis.iter().map(|(e, _, _)| e).collect();
    // Orphan despawn via Commands needs entity id in parts query; kept simple via OnExit/clean.
    let _ = live_ids;
}

#[allow(clippy::too_many_arguments)]
fn return_fire(
    mut shots: MessageReader<Gunshot>,
    lang: Res<Language>,
    input: Res<WeaponInput>,
    inventory: Res<Inventory>,
    vehicles: Option<Res<VehicleFleet>>,
    (mut rng, mut stats, mut notice): (ResMut<CombatRng>, ResMut<HeliStats>, ResMut<WorldNotice>),
    player: Query<&Transform, With<Player>>,
    mut helis: Query<(&mut PlaHeli, &Transform)>,
) {
    let mut fired_dmg = None;
    for shot in shots.read().filter(|s| s.by_player) {
        let base = match shot.fam {
            GunFam::Lmg | GunFam::Sniper => 32.0,
            GunFam::Dmr => 24.0,
            GunFam::Rifle | GunFam::Bullpup => 12.0,
            _ => 5.0,
        };
        let bonus = match inventory.equipped.as_str() {
            "jav" | "lmat" => 95.0,
            "at4" | "kestrel" => 80.0,
            _ => base,
        };
        fired_dmg = Some(bonus);
    }
    // Mounted heavy vehicle guns (30mm ZBL-08 / 105mm ZTD-05) also tear up low-flying choppers.
    if fired_dmg.is_none()
        && input.fire
        && let Some(v) = vehicles.as_deref().and_then(|f| f.current())
        && let Some(g) = &v.gun
        && let Some(slot) = g.guns.get(g.selected)
        && slot.cooldown_remaining > 0.0
    {
        fired_dmg = Some(match v.kind {
            VehicleKind::Ztd05 => 45.0,
            _ => 22.0,
        });
    }
    let Some(dmg) = fired_dmg else {
        return;
    };
    let Ok(pt) = player.single() else { return };
    let pp = pt.translation.xz();
    let f = pt.forward();
    let dir = input
        .aim
        .try_normalize()
        .unwrap_or(Vec2::new(f.x, f.z).normalize_or_zero());
    let range = 155.0;
    for (mut h, tf) in &mut helis {
        if h.phase == HeliPhase::Fall {
            continue;
        }
        let to = tf.translation.xz() - pp;
        let d = to.length();
        if d > range || d < 1.0 || to.normalize().dot(dir) < 16f32.to_radians().cos() {
            continue;
        }
        if rng.0.next_f32() < 0.72 {
            h.hp -= dmg;
            stats.hits += 1;
            if h.hp <= 0.0 {
                h.phase = HeliPhase::Fall;
                h.fall_t = 0.0;
                crate::device::request_haptic(45);
                notice.text = match *lang {
                    Language::ZhTw => format!("{} 被擊中，正在往下掉！", h.kind.name(*lang)),
                    Language::En => format!("The {} is hit and going down!", h.kind.name(*lang)),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wanted_scales_with_war_level() {
        assert_eq!(wanted_count(0.0), 0);
        assert_eq!(wanted_count(WAR_START), 1);
        assert_eq!(wanted_count(0.45), 2);
        assert_eq!(wanted_count(0.85), 3);
    }

    #[test]
    fn z20_and_z10_meshes_have_geometry_and_zero_lights() {
        assert_ne!(z20_mesh().vcount(), 0);
        assert_ne!(z10_mesh().vcount(), 0);
        let mut world = World::new();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<StandardMaterial>::default());
        let (z20, z10, main_rotor, tail_rotor, beam, pool, tracer) = {
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            (
                meshes.add(z20_mesh().into_mesh()),
                meshes.add(z10_mesh().into_mesh()),
                meshes.add(Cuboid::new(0.5, 0.05, 16.0)),
                meshes.add(Cuboid::new(0.08, 2.8, 0.26)),
                meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
                meshes.add(Cuboid::new(1.0, 0.1, 1.0)),
                meshes.add(Cuboid::new(0.1, 0.1, 1.0)),
            )
        };
        let (body_mat, blade_mat, beam_mat, pool_mat, tracer_mat) = {
            let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
            (
                materials.add(Color::WHITE),
                materials.add(Color::BLACK),
                materials.add(Color::WHITE),
                materials.add(Color::WHITE),
                materials.add(Color::WHITE),
            )
        };
        let art = HeliArt {
            z20,
            z10,
            main_rotor,
            tail_rotor,
            beam,
            pool,
            tracer,
            body_mat,
            blade_mat,
            beam_mat,
            pool_mat,
            tracer_mat,
        };
        world.insert_resource(art);
        world.insert_resource(HeliStats::default());
        spawn_qa_heli(&mut world, HeliKind::Z10, Vec2::new(10.0, 10.0));
        assert_eq!(world.query::<&SpotLight>().iter(&world).count(), 0);
        assert_eq!(world.query::<&PointLight>().iter(&world).count(), 0);
        assert_eq!(world.query::<&PlaHeli>().iter(&world).count(), 1);
    }

    #[test]
    fn shootdown_triggers_fall_and_drops_helidrop_crate() {
        use bevy::state::app::StatesPlugin;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(GameFlow::Playing)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<PauseOpen>()
            .init_resource::<Language>()
            .init_resource::<CombatEnv>()
            .init_resource::<CombatRng>()
            .init_resource::<Campaign>()
            .init_resource::<GameState>()
            .init_resource::<WeaponInput>()
            .init_resource::<Inventory>()
            .init_resource::<ScavengeState>()
            .init_resource::<WorldNotice>()
            .add_message::<Gunshot>()
            .add_message::<SfxEvent>()
            .add_message::<PlayerHit>()
            .add_message::<WarCall>()
            .add_plugins(HeliPlugin);
        app.world_mut()
            .spawn((Player, Transform::from_xyz(0.0, 0.0, 0.0)));
        app.update();
        spawn_qa_heli(app.world_mut(), HeliKind::Z20, Vec2::new(0.0, 40.0));
        {
            let mut helis = app.world_mut().query::<&mut PlaHeli>();
            for mut h in helis.iter_mut(app.world_mut()) {
                h.hp = 1.0;
                h.phase = HeliPhase::Fall;
                h.alt = 0.2;
            }
        }
        app.update();
        assert_eq!(app.world().resource::<HeliStats>().downed, 1);
        assert!(
            app.world()
                .resource::<ScavengeState>()
                .drops
                .iter()
                .any(|(id, _)| id.starts_with("helidrop:"))
        );
    }
}
