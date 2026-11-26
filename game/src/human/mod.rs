//! 白面人 characters: the rig model ([`rig`]) and its procedural animation ([`anim`]), driven
//! for the player, the 解放軍 soldiers (from the combat AI's `Enemy` fields, replacing the
//! stand-in capsules) and the civilians wandering the streets of 萬華.

pub mod anim;
pub mod rig;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

use anim::{Anim, AnimState};
use rig::{J, JOINT_LIST, JOINTS, RigOpts};

use crate::combat::standin::StandinSettings;
use crate::combat::tuning::GunFam;
use crate::combat::{Enemy, GameState, Gunshot, PlayerStance, soldier::SolState};
use crate::geom::MeshBuf;
use crate::player::Player;
use crate::roles::{Gender, SelectedRole};

/// Who wears the rig (decides what drives it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Driver {
    Player,
    Soldier,
    Civilian,
    /// Driven by its owner writing [`RigAnim`] (ROC remnants, the JSDF liaison, families).
    Ally,
}

/// Put this on an entity (with a Transform) to give it a 白面人 body.
#[derive(Component, Clone, Debug)]
pub struct Humanoid {
    pub opts: RigOpts,
    pub driver: Driver,
}

impl Humanoid {
    /// The player before a role is picked: the courier (roleById fallback) in its look.
    pub fn player() -> Self {
        Self {
            opts: player_opts(SelectedRole::default()),
            driver: Driver::Player,
        }
    }
}

fn player_opts(sel: SelectedRole) -> RigOpts {
    let role = sel.role();
    let female = sel.gender == Gender::Female;
    let look = if female { role.look_f } else { role.look_m };
    RigOpts {
        female,
        top: look.top,
        bottom: look.bottom,
        shoes: look.shoes,
        soldier: false,
        jacket: look.jacket,
        pack: None,
        outfit: role.id,
        gun: "",
    }
}

/// The spawned rig: its root (under the character) and one entity per joint.
#[derive(Component)]
pub struct RigParts {
    pub root: Entity,
    pub joints: [Entity; JOINTS],
    pub flash: Option<Entity>,
    pub built: RigOpts,
}

/// Animation state of a rig, plus what the driver wants it to do this frame.
#[derive(Component)]
pub struct RigAnim {
    pub anim: Anim,
    pub state: AnimState,
    pub speed: f32,
    /// Facing (rad about +Y) of the model's +Z.
    pub yaw: f32,
    last_pos: Vec2,
    hurt_was: f32,
    offset: [Vec3; JOINTS],
}

/// A wandering civilian.
#[derive(Component)]
pub struct Civilian {
    target: Vec2,
    speed: f32,
    wait: f32,
    seed: u32,
}

struct BuiltRig {
    parent: [Option<J>; JOINTS],
    offset: [Vec3; JOINTS],
    meshes: Vec<(J, Handle<Mesh>, u8)>,
    lt: f32,
    ls: f32,
    rest_rot: [Quat; JOINTS],
    muzzle: Vec3,
}

#[derive(Resource)]
struct RigAssets {
    skin: Handle<StandardMaterial>,
    cloth: Handle<StandardMaterial>,
    gun: Handle<StandardMaterial>,
    flash_mesh: Handle<Mesh>,
    flash_mat: Handle<StandardMaterial>,
    cache: HashMap<String, Arc<BuiltRig>>,
}

pub struct HumanPlugin;

impl Plugin for HumanPlugin {
    fn build(&self, app: &mut App) {
        // the real rig replaces the combat stand-in bodies (the vision cones stay)
        app.insert_resource(StandinSettings { soldiers: false })
            .add_systems(Startup, (setup, spawn_civilians))
            .add_systems(
                Update,
                (
                    tag_soldiers,
                    player_look,
                    build_rigs,
                    (drive_player, drive_soldiers, drive_civilians),
                    animate_rigs,
                )
                    .chain()
                    .after(crate::combat::CombatSet),
            );
    }
}

/// firearms.ts muzzle flash: two crossed cards stretched along +Z and one facing forward,
/// additive orange-white radial glow.
fn flash_mesh() -> MeshBuf {
    let mut m = MeshBuf::default();
    for r in [0.0, FRAC_PI_2] {
        m.add_white(
            &crate::geom::plane2(0.16, 0.36),
            Mat4::from_translation(Vec3::new(0.0, 0.0, 0.17))
                * Mat4::from_rotation_z(r)
                * Mat4::from_rotation_x(FRAC_PI_2),
        );
    }
    m.add_white(
        &crate::geom::plane2(0.22, 0.22),
        Mat4::from_translation(Vec3::new(0.0, 0.0, 0.02)),
    );
    m
}

fn flash_image() -> Image {
    use crate::map::canvas::{Canvas, rgba};
    let mut c = Canvas::new(64, 64);
    c.radial(
        32.0,
        32.0,
        8.0,
        1.0,
        rgba(255.0, 255.0, 235.0, 1.0),
        rgba(255.0, 214.0, 120.0, 0.95),
    );
    c.radial(
        32.0,
        32.0,
        32.0,
        1.0,
        rgba(255.0, 130.0, 40.0, 0.5),
        rgba(255.0, 90.0, 20.0, 0.0),
    );
    c.radial(
        32.0,
        32.0,
        14.0,
        1.0,
        rgba(255.0, 240.0, 200.0, 0.9),
        rgba(255.0, 180.0, 90.0, 0.0),
    );
    c.to_image(true, false)
}

fn setup(
    mut commands: Commands,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
) {
    let tex = images.add(flash_image());
    commands.insert_resource(RigAssets {
        flash_mesh: meshes.add(flash_mesh().into_mesh()),
        flash_mat: mats.add(StandardMaterial {
            base_color_texture: Some(tex),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        // glossy satin mannequin (matS: roughness 0.32, clearcoat 0.7)
        skin: mats.add(StandardMaterial {
            perceptual_roughness: 0.32,
            clearcoat: 0.7,
            clearcoat_perceptual_roughness: 0.28,
            reflectance: 0.55,
            ..default()
        }),
        cloth: mats.add(StandardMaterial {
            perceptual_roughness: 0.88,
            ..default()
        }),
        gun: mats.add(StandardMaterial {
            perceptual_roughness: 0.5,
            metallic: 0.3,
            ..default()
        }),
        cache: HashMap::new(),
    });
}

fn soldier_gun(id: &str) -> &'static str {
    if id == "qbz95" { "qbz95" } else { "rifle" }
}

fn gun_fam(id: &str) -> Option<GunFam> {
    crate::inventory::item(id)
        .filter(|d| d.kind == crate::inventory::ItemKind::Gun)
        .map(|_| crate::combat::gun_fam(id))
}

/// Every new soldier gets a soldier rig (SOLDIER_OPTS) with its gun.
fn tag_soldiers(mut commands: Commands, added: Query<(Entity, &Enemy), Added<Enemy>>) {
    for (e, en) in &added {
        commands.entity(e).insert(Humanoid {
            opts: RigOpts::soldier(soldier_gun(en.0.weapon)),
            driver: Driver::Soldier,
        });
    }
}

/// The player wears the picked role's look.
fn player_look(
    sel: Res<SelectedRole>,
    inventory: Option<Res<crate::inventory::Inventory>>,
    mut q: Query<&mut Humanoid, With<Player>>,
) {
    for mut h in &mut q {
        let mut o = player_opts(*sel);
        o.gun = inventory
            .as_deref()
            .and_then(|bag| {
                crate::inventory::item(&bag.equipped)
                    .filter(|d| d.kind.is_weapon() && bag.count(d.id) > 0)
            })
            .map_or("", |d| d.id);
        if h.opts != o {
            h.opts = o;
        }
    }
}

fn upload(meshes: &mut Assets<Mesh>, o: &RigOpts) -> BuiltRig {
    let mut m = rig::make_rig(o);
    if m.gun.is_none()
        && let Some(d) = crate::inventory::item(o.gun)
    {
        let mut mesh = MeshBuf::default();
        if d.kind == crate::inventory::ItemKind::Melee {
            let (width, length, color) = match d.id {
                "knife" => (0.08, 0.27, 0xb4bac0),
                "bat" => (0.05, 0.80, 0x916f46),
                _ => (0.035, 0.84, 0x727c83),
            };
            mesh.add(
                &crate::geom::cuboid(width, 0.035, length),
                crate::geom::at(0.0, 0.0, length * 0.35),
                crate::geom::hex(color),
            );
            m.gun = Some(mesh);
        } else if d.kind == crate::inventory::ItemKind::Gun && d.anti_armor == 0.0 {
            let pistol = d.id == "t75p" || d.id == "qsz92" || d.id == "pistol";
            let length = if pistol { 0.22 } else { 0.68 };
            mesh.add(
                &crate::geom::cuboid(0.045, 0.07, length),
                crate::geom::at(0.0, 0.04, length * 0.25),
                crate::geom::hex(0x343c39),
            );
            mesh.add(
                &crate::geom::cuboid(0.035, 0.12, 0.045),
                crate::geom::at(0.0, -0.04, 0.0),
                crate::geom::hex(0x252a2b),
            );
            m.gun = Some(mesh);
            m.muzzle = Vec3::new(0.0, 0.04, length * 0.75);
        }
    }
    let mut out = Vec::new();
    let mut put = |j: J, b: MeshBuf, mat: u8| out.push((j, meshes.add(b.into_mesh()), mat));
    for (j, b) in m.skin {
        put(j, b, 0);
    }
    for (j, b) in m.cloth {
        put(j, b, 1);
    }
    if let Some(g) = m.gun {
        put(J::GunInner, g, 2);
    }
    BuiltRig {
        parent: m.parent,
        offset: m.offset,
        meshes: out,
        lt: m.lt,
        ls: m.ls,
        rest_rot: m.rest_rot,
        muzzle: m.muzzle,
    }
}

/// (Re)build the rig entities of every new / changed [`Humanoid`].
fn build_rigs(
    mut commands: Commands,
    mut assets: ResMut<RigAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    q: Query<(Entity, &Humanoid, Option<&RigParts>), Changed<Humanoid>>,
) {
    for (e, h, parts) in &q {
        if parts.is_some_and(|p| p.built == h.opts) {
            continue;
        }
        if let Some(p) = parts {
            commands.entity(p.root).despawn();
        }
        let key = h.opts.key();
        let built = match assets.cache.get(&key) {
            Some(b) => b.clone(),
            None => {
                let b = Arc::new(upload(&mut meshes, &h.opts));
                assets.cache.insert(key, b.clone());
                b
            }
        };
        let root = commands
            .spawn((Transform::default(), Visibility::default()))
            .id();
        let mut joints = [Entity::PLACEHOLDER; JOINTS];
        for i in 0..JOINT_LIST.len() {
            let t = Transform::from_translation(built.offset[i]).with_rotation(built.rest_rot[i]);
            let je = commands.spawn((t, Visibility::default())).id();
            let parent = built.parent[i].map_or(root, |p| joints[p as usize]);
            commands.entity(parent).add_child(je);
            joints[i] = je;
        }
        for (j, mesh, mat) in &built.meshes {
            let m = match mat {
                0 => assets.skin.clone(),
                1 => assets.cloth.clone(),
                _ => assets.gun.clone(),
            };
            let me = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(m),
                    Transform::default(),
                ))
                .id();
            if h.driver == Driver::Civilian {
                commands.entity(me).insert(NotShadowCaster);
            }
            commands.entity(joints[*j as usize]).add_child(me);
        }
        let flash = built.meshes.iter().any(|m| m.0 == J::GunInner).then(|| {
            let f = commands
                .spawn((
                    Mesh3d(assets.flash_mesh.clone()),
                    MeshMaterial3d(assets.flash_mat.clone()),
                    Transform::from_translation(built.muzzle),
                    Visibility::Hidden,
                    NotShadowCaster,
                ))
                .id();
            commands.entity(joints[J::GunInner as usize]).add_child(f);
            f
        });
        commands.entity(e).add_child(root);
        let gun = gun_fam(h.opts.gun);
        let seed = e.index_u32().wrapping_mul(7919);
        commands.entity(e).insert((
            RigParts {
                root,
                joints,
                flash,
                built: h.opts.clone(),
            },
            RigAnim {
                anim: Anim::new(built.lt, built.ls, gun, seed),
                state: AnimState::Idle,
                speed: 0.0,
                yaw: 0.0,
                last_pos: Vec2::NAN,
                hurt_was: 0.0,
                offset: built.offset,
            },
        ));
    }
}

/// Measured ground speed of an entity this frame (m/s), smoothed.
fn measure(r: &mut RigAnim, p: Vec2, dt: f32) -> f32 {
    let v = if r.last_pos.is_nan() || dt <= 0.0 {
        0.0
    } else {
        r.last_pos.distance(p) / dt
    };
    r.last_pos = p;
    // teleports (respawn) read as standing still
    let v = if v > 12.0 { 0.0 } else { v };
    r.speed += (v - r.speed) * (dt * 10.0).min(1.0);
    r.speed
}

fn drive_player(
    time: Res<Time>,
    state: Res<GameState>,
    stance: Res<PlayerStance>,
    mut shots: MessageReader<Gunshot>,
    mut q: Query<(&Transform, &mut RigAnim), With<Player>>,
) {
    let dt = time.delta_secs();
    let fired = shots.read().any(|s| s.by_player);
    for (t, mut r) in &mut q {
        let sp = measure(&mut r, t.translation.xz(), dt);
        // the player's root looks down local −Z; the model faces +Z
        let f = t.forward();
        r.yaw = f.x.atan2(f.z);
        if fired {
            r.anim.fire();
        }
        r.state = if state.hp <= 0.0 {
            AnimState::Death
        } else if r.anim.shoot_t > 0.0 {
            AnimState::Shoot
        } else if stance.sneak || stance.low_cover {
            AnimState::Sneak
        } else if sp > 4.5 {
            AnimState::Run
        } else if sp > 0.2 {
            AnimState::Walk
        } else {
            AnimState::Idle
        };
    }
}

fn drive_soldiers(
    time: Res<Time>,
    mut shots: MessageReader<Gunshot>,
    mut q: Query<(&Enemy, &Transform, &mut RigAnim)>,
) {
    let dt = time.delta_secs();
    let fire: Vec<Vec2> = shots
        .read()
        .filter(|s| !s.by_player)
        .map(|s| s.pos.xz())
        .collect();
    for (e, t, mut r) in &mut q {
        let s = &e.0;
        let _ = measure(&mut r, t.translation.xz(), dt);
        r.speed = s.moved;
        // original: mesh.rotation.y = π/2 − dir (model faces +Z)
        r.yaw = FRAC_PI_2 - s.dir;
        if fire.iter().any(|p| p.distance(s.pos) < 0.6) {
            r.anim.fire();
        }
        if s.hurt_t > r.hurt_was + 0.01 {
            r.anim.hurt();
        }
        r.hurt_was = s.hurt_t;
        r.state = if s.state == SolState::Dead {
            AnimState::Death
        } else if s.aiming {
            AnimState::Aim
        } else if s.crouch && s.moved < 0.2 {
            AnimState::Sneak
        } else if s.moved > 4.5 {
            AnimState::Run
        } else if s.moved > 0.2 {
            AnimState::Walk
        } else {
            AnimState::Idle
        };
    }
}

/// Write the rig's joint transforms from its animation.
fn animate_rigs(
    time: Res<Time>,
    player_gun: Option<Res<crate::combat::PlayerGun>>,
    mut q: Query<(&mut RigAnim, &RigParts, Option<&Player>)>,
    mut tf: Query<&mut Transform, (Without<RigAnim>, Without<Player>)>,
    mut vis: Query<&mut Visibility, Without<RigAnim>>,
) {
    let dt = time.delta_secs().min(0.1);
    let e = |v: Vec3| Quat::from_euler(EulerRot::XYZ, v.x, v.y, v.z);
    for (mut r, p, player) in &mut q {
        let (state, yaw, speed) = (r.state, r.yaw, r.speed);
        r.anim.animate(state, yaw, speed, dt);
        let mut o = r.anim.out;
        if player.is_some()
            && let Some(progress) = player_gun.as_deref().and_then(|gun| gun.melee_progress())
        {
            let swing = (progress * PI).sin();
            o.ua_r.x -= swing * 1.5;
            o.ua_r.z -= swing * 0.4;
            o.la_r.x -= swing * 0.8;
            o.chest.y += (progress - 0.44) * swing * 0.8;
        }
        if let Some(f) = p.flash
            && let Ok(mut v) = vis.get_mut(f)
        {
            // flashT: 50 ms of flame at the start of each 140 ms kick
            v.set_if_neq(if r.anim.shoot_t > 0.09 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        // the root carries the facing: the player's own transform already turns, the rest don't
        let root_rot = if player.is_some() {
            Quat::from_rotation_y(PI)
        } else {
            Quat::from_rotation_y(yaw)
        } * Quat::from_rotation_y(o.spin);
        if let Ok(mut t) = tf.get_mut(p.root) {
            t.rotation = root_rot;
        }
        let set = |tf: &mut Query<&mut Transform, (Without<RigAnim>, Without<Player>)>,
                   j: J,
                   pos: Option<Vec3>,
                   rot: Quat| {
            if let Ok(mut t) = tf.get_mut(p.joints[j as usize]) {
                if let Some(pos) = pos {
                    t.translation = pos;
                }
                t.rotation = rot;
            }
        };
        let off = r.offset;
        set(&mut tf, J::Body, Some(o.body_pos), e(o.body_rot));
        set(
            &mut tf,
            J::Hips,
            Some(Vec3::new(
                off[J::Hips as usize].x,
                o.hips_y,
                off[J::Hips as usize].z,
            )),
            Quat::from_rotation_y(o.hips_yaw),
        );
        set(&mut tf, J::Spine, None, e(o.spine));
        set(&mut tf, J::Chest, None, e(o.chest));
        set(&mut tf, J::Head, None, e(o.head));
        set(&mut tf, J::ThL, None, e(o.th_l));
        set(&mut tf, J::ThR, None, e(o.th_r));
        set(&mut tf, J::ShL, None, Quat::from_rotation_x(o.sh_l));
        set(&mut tf, J::ShR, None, Quat::from_rotation_x(o.sh_r));
        set(&mut tf, J::FtL, None, Quat::from_rotation_x(o.ft_l));
        set(&mut tf, J::FtR, None, Quat::from_rotation_x(o.ft_r));
        set(&mut tf, J::UaL, None, e(o.ua_l));
        set(&mut tf, J::UaR, None, e(o.ua_r));
        set(&mut tf, J::LaL, None, e(o.la_l));
        set(&mut tf, J::LaR, None, e(o.la_r));
        set(&mut tf, J::Weapon, None, e(o.weapon));
        set(
            &mut tf,
            J::GunInner,
            Some(Vec3::new(0.0, 0.0, o.gun_back)),
            Quat::IDENTITY,
        );
    }
}

// ---- civilians ----

const CIV_TOPS: [u32; 10] = [
    0x9a6a8a, 0xd8d4c8, 0x3d5a78, 0xb84a52, 0x6a7a5a, 0xe0c89a, 0x2a2a30, 0x8a9ab0, 0xc87a3a,
    0xf0f0f0,
];
const CIV_BOTTOMS: [u32; 6] = [0x34363e, 0x2a3a5a, 0x3a3f4a, 0x5a4a3a, 0x1e2a50, 0x6a6a6a];
const CIV_SHOES: [u32; 4] = [0xe8e8e8, 0x222222, 0x3a2a1e, 0xf4f4f4];

fn lcg(s: &mut u32) -> f32 {
    *s = s.wrapping_mul(1664525).wrapping_add(1013904223);
    (*s >> 8) as f32 / (1u32 << 24) as f32
}

/// A few 萬華 residents out on the street around the start.
fn spawn_civilians(mut commands: Commands) {
    let w = crate::map::world();
    let obs = crate::map::Obstacles::new(w.clone());
    let mut seed = 0x51_7e_u32;
    let mut n = 0;
    for _ in 0..400 {
        if n >= 14 {
            break;
        }
        let a = lcg(&mut seed) * std::f32::consts::TAU;
        let r = 6.0 + lcg(&mut seed) * 40.0;
        let p = w.spawn + Vec2::from_angle(a) * r;
        if obs.blocked(p, 0.5) {
            continue;
        }
        n += 1;
        let pick = |s: &mut u32, v: &[u32]| v[(lcg(s) * v.len() as f32) as usize % v.len()];
        let opts = RigOpts {
            female: lcg(&mut seed) < 0.5,
            top: pick(&mut seed, &CIV_TOPS),
            bottom: pick(&mut seed, &CIV_BOTTOMS),
            shoes: pick(&mut seed, &CIV_SHOES),
            soldier: false,
            jacket: (lcg(&mut seed) < 0.25).then(|| pick(&mut seed, &CIV_BOTTOMS)),
            pack: (lcg(&mut seed) < 0.2).then_some(0x2f3a2a),
            outfit: "",
            gun: "",
        };
        commands.spawn((
            Humanoid {
                opts,
                driver: Driver::Civilian,
            },
            Civilian {
                target: p,
                speed: 0.9 + lcg(&mut seed) * 0.6,
                wait: lcg(&mut seed) * 3.0,
                seed,
            },
            Transform::from_xyz(p.x, 0.0, p.y),
            Visibility::default(),
        ));
    }
}

fn drive_civilians(
    time: Res<Time>,
    obstacles: Res<crate::map::Obstacles>,
    mut q: Query<(&mut Civilian, &mut Transform, Option<&mut RigAnim>)>,
) {
    let dt = time.delta_secs().min(0.1);
    for (mut c, mut t, r) in &mut q {
        let p = t.translation.xz();
        if c.wait > 0.0 {
            c.wait -= dt;
            if let Some(mut r) = r {
                r.speed = 0.0;
                r.state = AnimState::Idle;
            }
            continue;
        }
        let d = c.target - p;
        if d.length() < 0.5 {
            // next errand: a free spot a few metres to 15 m away, then a pause
            let mut s = c.seed;
            for _ in 0..12 {
                let q = p + Vec2::from_angle(lcg(&mut s) * std::f32::consts::TAU)
                    * (3.0 + lcg(&mut s) * 12.0);
                if !obstacles.blocked(q, 0.5) && obstacles.los(p, q) {
                    c.target = q;
                    break;
                }
            }
            c.wait = lcg(&mut s) * 4.0;
            c.seed = s;
            continue;
        }
        let dir = d.normalize();
        let np = obstacles.move_circle(p, p + dir * c.speed * dt, 0.35);
        if np.distance(p) < c.speed * dt * 0.3 {
            c.target = p; // stuck: pick another errand
        }
        t.translation.x = np.x;
        t.translation.z = np.y;
        if let Some(mut r) = r {
            r.speed = c.speed;
            r.yaw = dir.x.atan2(dir.y);
            r.state = AnimState::Walk;
        }
    }
}
