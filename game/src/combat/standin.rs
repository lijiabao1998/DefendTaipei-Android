//! STAND-IN character renderer + combat effects.
//!
//! The 解放軍 soldiers here are deliberately simple placeholder bodies, kept behind two
//! systems so the real rig (Grok Bot's port of `src/npc/human.ts`, branch
//! `feat/wanhua-models`) can replace them without touching the AI:
//! - [`attach_standin`] runs on `Added<Enemy>` and builds the visual children;
//! - [`sync_standin`] reads the AI state on [`Enemy`] (facing `dir`, `crouch`, `aiming`,
//!   `moved`, `hurt_t`, `state`) every frame.
//!
//! To swap: disable `StandinPlugin`'s soldier systems (or set `StandinSettings::soldiers = false`)
//! and drive the rig from the same `Enemy` fields. The vision cone (original `cone` mesh)
//! and the effects (tracers, grenade, blast) are combat visuals and stay here.

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use super::tuning::*;
use super::{CombatEnv, Enemy, Fx, SolState};

#[derive(Resource)]
pub struct StandinSettings {
    /// Draw the placeholder soldier bodies. Set false once the real rig drives `Enemy`.
    pub soldiers: bool,
}

impl Default for StandinSettings {
    fn default() -> Self {
        Self { soldiers: true }
    }
}

#[derive(Resource)]
pub struct StandinAssets {
    body: Handle<Mesh>,
    head: Handle<Mesh>,
    gun: Handle<Mesh>,
    uniform: Handle<StandardMaterial>,
    helmet: Handle<StandardMaterial>,
    gun_mat: Handle<StandardMaterial>,
    hurt: Handle<StandardMaterial>,
    cone: Handle<Mesh>,
    /// [patrol day, patrol night, investigate, combat] × [day, night] opacity
    cone_mats: [[Handle<StandardMaterial>; 2]; 3],
    cone_patrol_night: [Handle<StandardMaterial>; 2],
}

/// Shared meshes / materials for tracers, grenades and blasts.
#[derive(Resource)]
pub struct CombatFxAssets {
    pub tracer_mesh: Handle<Mesh>,
    pub tracer_mat: Handle<StandardMaterial>,
    pub grenade_mesh: Handle<Mesh>,
    pub grenade_mat: Handle<StandardMaterial>,
    pub disc_mesh: Handle<Mesh>,
    pub warn_core_mat: Handle<StandardMaterial>,
    pub warn_wound_mat: Handle<StandardMaterial>,
    pub blast_mesh: Handle<Mesh>,
    pub blast_mat: Handle<StandardMaterial>,
}

/// Marker: a tracer effect (mesh attached by [`attach_tracer_mesh`]).
#[derive(Component)]
pub struct TracerFx;

#[derive(Component)]
pub struct StandinParts {
    model: Entity,
    body: Entity,
    cone: Entity,
}

pub struct StandinPlugin;

impl Plugin for StandinPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StandinSettings>()
            .add_systems(Startup, load_assets)
            .add_systems(
                Update,
                (attach_standin, sync_standin, attach_tracer_mesh)
                    .chain()
                    .after(super::CombatSet),
            );
    }
}

fn srgb(hex: u32) -> Color {
    Color::srgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

fn cone_mesh() -> Mesh {
    let n = 20;
    let mut pos = vec![[0.0, 0.0, 0.0]];
    for i in 0..=n {
        let a = -VISION_HALF_ANGLE + 2.0 * VISION_HALF_ANGLE * i as f32 / n as f32;
        pos.push([a.cos(), 0.0, a.sin()]);
    }
    let mut idx = Vec::new();
    for i in 0..n as u32 {
        idx.extend_from_slice(&[0, i + 2, i + 1]);
    }
    let normals = vec![[0.0, 1.0, 0.0]; pos.len()];
    let uvs = vec![[0.0, 0.0]; pos.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(idx))
}

fn lit(mats: &mut Assets<StandardMaterial>, c: Color) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: c,
        perceptual_roughness: 0.85,
        ..default()
    })
}

fn flat(
    mats: &mut Assets<StandardMaterial>,
    c: Color,
    alpha: Option<f32>,
) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: alpha.map_or(c, |a| c.with_alpha(a)),
        unlit: true,
        alpha_mode: if alpha.is_some() {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        cull_mode: None,
        ..default()
    })
}

fn cone_set(mats: &mut Assets<StandardMaterial>, hex: u32) -> [Handle<StandardMaterial>; 2] {
    [
        flat(mats, srgb(hex), Some(CONE_OPACITY_DAY)),
        flat(mats, srgb(hex), Some(CONE_OPACITY_NIGHT)),
    ]
}

fn load_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let m = mats.as_mut();
    // SOLDIER_OPTS: top 0x55603c, bottom 0x4a5434
    let uniform = lit(m, srgb(0x55603c));
    let helmet = lit(m, srgb(0x4a5434));
    let gun_mat = lit(m, srgb(0x24282b));
    let hurt = flat(m, Color::WHITE, None);
    let cone_mats = [
        cone_set(m, 0xffee88),
        cone_set(m, 0xffa030),
        cone_set(m, 0xff3030),
    ];
    let cone_patrol_night = cone_set(m, 0xfff6c0);
    let tracer_mat = flat(m, srgb(0xffdd88), None);
    let grenade_mat = lit(m, srgb(0x2c3022));
    let warn_core_mat = flat(m, srgb(0xff2020), Some(0.35));
    let warn_wound_mat = flat(m, srgb(0xff4020), Some(0.15));
    let blast_mat = flat(m, srgb(0xffa040), Some(0.7));
    commands.insert_resource(StandinAssets {
        body: meshes.add(Capsule3d::new(0.25, 1.0)),
        head: meshes.add(Sphere::new(0.15).mesh().ico(1).unwrap()),
        gun: meshes.add(Cuboid::new(0.07, 0.09, 0.75)),
        uniform,
        helmet,
        gun_mat,
        hurt,
        cone: meshes.add(cone_mesh()),
        cone_mats,
        cone_patrol_night,
    });
    commands.insert_resource(CombatFxAssets {
        tracer_mesh: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        tracer_mat,
        grenade_mesh: meshes.add(Sphere::new(0.09).mesh().ico(1).unwrap()),
        grenade_mat,
        disc_mesh: meshes.add(Circle::new(1.0)),
        warn_core_mat,
        warn_wound_mat,
        blast_mesh: meshes.add(Sphere::new(0.6).mesh().ico(2).unwrap()),
        blast_mat,
    });
}

/// Builds the stand-in body + vision cone for each new soldier.
pub fn attach_standin(
    mut commands: Commands,
    settings: Res<StandinSettings>,
    assets: Option<Res<StandinAssets>>,
    added: Query<Entity, Added<Enemy>>,
) {
    let Some(a) = assets else { return };
    for e in &added {
        let cone = commands
            .spawn((
                Mesh3d(a.cone.clone()),
                MeshMaterial3d(a.cone_mats[0][0].clone()),
                Transform::from_xyz(0.0, 0.06, 0.0),
                NotShadowCaster,
            ))
            .id();
        let body = commands
            .spawn((
                Mesh3d(a.body.clone()),
                MeshMaterial3d(a.uniform.clone()),
                Transform::from_xyz(0.0, 0.75, 0.0),
            ))
            .id();
        let head = commands
            .spawn((
                Mesh3d(a.head.clone()),
                MeshMaterial3d(a.helmet.clone()),
                Transform::from_xyz(0.0, 1.62, 0.0),
            ))
            .id();
        let gun = commands
            .spawn((
                Mesh3d(a.gun.clone()),
                MeshMaterial3d(a.gun_mat.clone()),
                Transform::from_xyz(0.2, 1.2, 0.3),
            ))
            .id();
        let model = commands
            .spawn((
                Transform::default(),
                if settings.soldiers {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ))
            .add_children(&[body, head, gun])
            .id();
        commands
            .entity(e)
            .add_children(&[model, cone])
            .insert(StandinParts { model, body, cone });
    }
}

/// Poses the stand-in from the AI state (rig.ts would animate idle / carry / run / aim / sneak / death).
pub fn sync_standin(
    env: Res<CombatEnv>,
    assets: Option<Res<StandinAssets>>,
    mut soldiers: Query<(&Enemy, &StandinParts, &mut Visibility)>,
    mut parts: Query<
        (
            &mut Transform,
            Option<&mut MeshMaterial3d<StandardMaterial>>,
            Option<&mut Visibility>,
        ),
        Without<Enemy>,
    >,
) {
    let Some(a) = assets else { return };
    let night = env.night as usize;
    for (e, p, mut vis) in &mut soldiers {
        let s = &e.0;
        vis.set_if_neq(if s.active(env.night) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        let dead = s.state == SolState::Dead;
        // model: forward is +Z; original `mesh.rotation.y = π/2 − dir`
        if let Ok((mut t, _, _)) = parts.get_mut(p.model) {
            let yaw = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2 - s.dir);
            let (rot, y, sy) = if dead {
                (
                    yaw * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                    0.25,
                    1.0,
                )
            } else if s.crouch && s.moved < 0.2 {
                (yaw, 0.0, 0.68)
            } else {
                (yaw, 0.0, 1.0)
            };
            t.rotation = rot;
            t.translation.y = y;
            t.scale = Vec3::new(1.0, sy, 1.0);
        }
        if let Ok((_, Some(mut m), _)) = parts.get_mut(p.body) {
            let want = if s.hurt_t > 0.25 { &a.hurt } else { &a.uniform };
            if m.0 != *want {
                m.0 = want.clone();
            }
        }
        if let Ok((mut t, m, v)) = parts.get_mut(p.cone) {
            t.rotation = Quat::from_rotation_y(-s.dir);
            t.scale = Vec3::splat(if env.night { VISION_NIGHT } else { VISION_DAY });
            if let Some(mut v) = v {
                v.set_if_neq(if dead {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                });
            }
            if let Some(mut m) = m {
                let want = match s.state {
                    SolState::Combat => &a.cone_mats[2][night],
                    SolState::Investigate => &a.cone_mats[1][night],
                    _ if env.night => &a.cone_patrol_night[1],
                    _ => &a.cone_mats[0][0],
                };
                if m.0 != *want {
                    m.0 = want.clone();
                }
            }
        }
    }
}

fn attach_tracer_mesh(
    mut commands: Commands,
    fx: Option<Res<CombatFxAssets>>,
    added: Query<Entity, Added<TracerFx>>,
) {
    let Some(fx) = fx else { return };
    for e in &added {
        commands.entity(e).insert((
            Mesh3d(fx.tracer_mesh.clone()),
            MeshMaterial3d(fx.tracer_mat.clone()),
            NotShadowCaster,
        ));
    }
}

/// Red warning circles where a grenade landed (blastFx.warn: core and wound radii).
pub fn spawn_grenade_warn(commands: &mut Commands, fx: &CombatFxAssets, at: Vec2) -> Entity {
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let core = commands
        .spawn((
            Mesh3d(fx.disc_mesh.clone()),
            MeshMaterial3d(fx.warn_core_mat.clone()),
            // child of the (rotated, scaled) wound disc: local +Z is world up
            Transform::from_xyz(0.0, 0.0, 0.002).with_scale(Vec3::splat(GREN_CORE / GREN_WOUND)),
            NotShadowCaster,
        ))
        .id();
    commands
        .spawn((
            Mesh3d(fx.disc_mesh.clone()),
            MeshMaterial3d(fx.warn_wound_mat.clone()),
            Transform::from_xyz(at.x, 0.05, at.y)
                .with_rotation(flat)
                .with_scale(Vec3::splat(GREN_WOUND)),
            Visibility::default(),
            NotShadowCaster,
        ))
        .add_child(core)
        .id()
}

/// Explosion flash (blastFx.explode).
pub fn spawn_blast(commands: &mut Commands, fx: &CombatFxAssets, at: Vec2) {
    commands.spawn((
        Fx { t: 0.3, grow: 6.0 },
        Mesh3d(fx.blast_mesh.clone()),
        MeshMaterial3d(fx.blast_mat.clone()),
        Transform::from_xyz(at.x, 0.4, at.y),
        NotShadowCaster,
    ));
}
