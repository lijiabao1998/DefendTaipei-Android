//! v24.14 奪回 on the street (Web `src/systems/reclaim.ts`).
//!
//! While a core area is occupied its walls carry the occupier's vertical red
//! order banners and its wide roads are crossed by curfew notices (all invented,
//! baked into `assets/textures/orders.png`). When it is reclaimed they come down,
//! ROC flags hang from the windows and fly on the taller roofs, and neighbours
//! stand along the streets among red firecracker paper, some waving small flags.
//! The moment an area falls a crowd also comes out around the player.
//!
//! Each state is one or two merged meshes per area. Geometry uses its own
//! random streams (the Web's seeds), so nothing else in the world moves because
//! of it. Banners are built at start; an area's flags and neighbours are built
//! the first time it is seen reclaimed.

use bevy::prelude::*;

use crate::game_flow::{Campaign, GameFlow};
use crate::geom::{MeshBuf, capsule, cuboid, cylinder, hex, sphere};
use crate::save::LoadCompleted;
use crate::sectors::{ReclaimParty, SECTORS, sector_of};

const N: usize = SECTORS.len();
const SHIRT: [u32; 9] = [
    0x3a4f7a, 0xc9c4b4, 0x7a2f2a, 0x4a6a4a, 0xd8d2c0, 0x2f2f33, 0x8a6a3a, 0x9a4a6a, 0x5f7f92,
];
const LEGS: [u32; 4] = [0x23262e, 0x3a3f4a, 0x4a4238, 0x2c3a4a];
const SKIN: [u32; 3] = [0xd9b08c, 0xc99a74, 0xe2bd9c];
const PAPER: [u32; 4] = [0xc8141a, 0xe0241f, 0xa80f14, 0xe8b818];

/// The Web's `(Math.imul(s, 1664525) + 1013904223) >>> 0) / 2^32` stream.
#[derive(Clone)]
struct Lcg(u32);

impl Lcg {
    fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (f64::from(self.0) / 4_294_967_296.0) as f32
    }
    fn pick(&mut self, v: &[u32]) -> u32 {
        v[(self.f() * v.len() as f32) as usize % v.len()]
    }
}

/// A textured quad: width × height in its local XY plane, centred on its local
/// origin shifted by `off`, sampling the Bevy UV rectangle `[u0, v_top, u1, v_bottom]`.
fn quad(w: f32, h: f32, off: Vec3, uv: [f32; 4]) -> MeshBuf {
    let mut m = MeshBuf::default();
    let [u0, vt, u1, vb] = uv;
    let c = [
        (Vec3::new(-w / 2.0, h / 2.0, 0.0), [u0, vt]),
        (Vec3::new(w / 2.0, h / 2.0, 0.0), [u1, vt]),
        (Vec3::new(-w / 2.0, -h / 2.0, 0.0), [u0, vb]),
        (Vec3::new(w / 2.0, -h / 2.0, 0.0), [u1, vb]),
    ];
    for (p, uv) in c {
        m.pos.push((p + off).into());
        m.nrm.push([0.0, 0.0, 1.0]);
        m.uv.push(uv);
        m.col.push([1.0; 4]);
    }
    m.idx.extend_from_slice(&[0, 2, 1, 2, 3, 1]);
    m
}

/// three.js `place(g, x, y, z, ry, rx, rz)`: rotate Z, then X, then Y, then translate.
fn place(x: f32, y: f32, z: f32, ry: f32, rx: f32, rz: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
        * Mat4::from_rotation_y(ry)
        * Mat4::from_rotation_x(rx)
        * Mat4::from_rotation_z(rz)
}

const FULL: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

fn cell_v(i: usize) -> [f32; 4] {
    [i as f32 * 0.125, 0.0, (i + 1) as f32 * 0.125, 1.0]
}

fn cell_h(i: usize) -> [f32; 4] {
    [0.5, i as f32 * 0.25, 1.0, (i + 1) as f32 * 0.25]
}

/// Everything planned for one area.
#[derive(Default)]
struct Plan {
    /// Order banners on walls and across roads (atlas).
    banners: MeshBuf,
    /// Steel poles of the cross-street notices (vertex colours).
    poles: MeshBuf,
    /// Window and roof flags: (position, yaw, pitch, size).
    flags: Vec<(Vec3, f32, f32, f32)>,
    /// Roof flag poles.
    flag_poles: Vec<Vec3>,
    /// Street spots for neighbours: (x, z, count).
    crowd: Vec<(Vec2, u32)>,
}

/// Per-area meshes and state.
#[derive(Resource, Default)]
struct StreetState {
    plans: Vec<Plan>,
    occ: [Option<Entity>; N],
    free: [Option<Entity>; N],
    gathered: [bool; N],
}

#[derive(Resource)]
struct StreetArt {
    orders: Handle<StandardMaterial>,
    flag: Handle<StandardMaterial>,
    crowd: Handle<StandardMaterial>,
}

/// Counters for tests and QA (Web `stats`).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct StreetStats {
    pub banners: u32,
    pub crossings: u32,
    pub flags: u32,
    pub people: u32,
}

pub struct ReclaimStreetPlugin;

impl Plugin for ReclaimStreetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StreetStats>()
            .add_systems(Startup, build)
            .add_systems(OnEnter(GameFlow::Prologue), reset_run)
            .add_systems(PreUpdate, regather_on_load)
            .add_systems(Update, (sync, gather).chain());
    }
}

fn plan_all(stats: &mut StreetStats) -> Vec<Plan> {
    let w = crate::map::world();
    let mut plans: Vec<Plan> = (0..N).map(|_| Plan::default()).collect();
    let mut n_flags = [0u32; N];
    let mut r = Lcg(0x7e11a);
    // ---- walls: vertical order banners while occupied, window flags once reclaimed
    for b in &w.blds {
        if b.h < 6.5 {
            continue;
        }
        let sec = sector_of(b.c);
        let pts = &b.pts;
        for i in 0..pts.len() {
            if b.fill && i != 0 {
                continue;
            }
            let (a, e) = (pts[i], pts[(i + 1) % pts.len()]);
            let l = a.distance(e);
            if l < 3.0 {
                continue;
            }
            let u = (e - a) / l;
            let mut n = Vec2::new(-u.y, u.x);
            let m = (a + e) * 0.5;
            if crate::map::data::pip(m + n * 0.5, pts) {
                n = -n;
            }
            if w.bld_at(m + n * 4.0).is_some() || w.bld_at(m + n * 8.0).is_some() {
                continue;
            }
            let fy = n.x.atan2(n.y);
            if r.f() < 0.22 && plans[sec].banners.vcount() < 150 * 4 {
                let h = (b.h - 5.2).min(4.6);
                if h >= 2.4 {
                    let d = 0.8 + r.f() * (l - 1.6).max(0.1);
                    let cell = (r.f() * 4.0) as usize % 4;
                    let p = a + u * d + n * 0.3;
                    plans[sec].banners.add(
                        &quad(h / 4.0, h, Vec3::ZERO, cell_v(cell)),
                        place(p.x, 4.9 + h / 2.0, p.y, fy, 0.0, 0.0),
                        [1.0; 4],
                    );
                    stats.banners += 1;
                }
            }
            // window flags, sticking out from the wall and hanging down a little
            let yaw_out = (-n.y).atan2(n.x);
            let mut d = 1.0 + r.f() * 3.0;
            while d < l - 0.8 && n_flags[sec] < 320 {
                if r.f() <= 0.5 {
                    let floors = ((b.h - 6.0) / 3.2).max(1.0);
                    let y = 4.6 + (r.f() * floors).floor() * 3.2 + 1.0;
                    let p = a + u * d + n * 0.12;
                    let _seed = r.f();
                    plans[sec]
                        .flags
                        .push((Vec3::new(p.x, y, p.y), yaw_out, -0.32, 1.05));
                    n_flags[sec] += 1;
                    stats.flags += 1;
                }
                d += 3.5 + r.f() * 6.0;
            }
            // a big flag on the roof of the taller buildings
            if !b.fill && b.h >= 10.0 && r.f() < 0.05 && n_flags[sec] < 320 {
                let _seed = r.f();
                let q = m - n * 1.2;
                plans[sec]
                    .flags
                    .push((Vec3::new(q.x, b.h + 3.1, q.y), yaw_out + 0.6, -0.04, 2.1));
                plans[sec].flag_poles.push(Vec3::new(q.x, b.h + 1.95, q.y));
                n_flags[sec] += 1;
                stats.flags += 1;
            }
        }
    }
    // ---- streets: order banners across the wide roads; neighbours along the rest
    let mut sites: Vec<Vec<(Vec2, f32)>> = vec![Vec::new(); N];
    for q in &w.roads {
        let width = q.w;
        if width < 4.0 {
            continue;
        }
        let mut next = 8.0 + r.f() * 30.0;
        let mut next_b = 40.0 + r.f() * 60.0;
        let mut run = 0.0;
        for pair in q.p.windows(2) {
            let (a, e) = (pair[0], pair[1]);
            let l = a.distance(e);
            if l < 0.5 {
                continue;
            }
            let u = (e - a) / l;
            while next < run + l {
                let s = next - run;
                let side = if r.f() < 0.5 { 1.0 } else { -1.0 };
                let p = a + u * s + Vec2::new(-u.y, u.x) * side * (width / 2.0 + 1.1);
                if w.bld_at(p).is_none() {
                    sites[sector_of(p)].push((p, width));
                }
                next += 7.0 + r.f() * 15.0;
            }
            if width >= 9.0 {
                while next_b < run + l {
                    let s = next_b - run;
                    let p = a + u * s;
                    let sec = sector_of(p);
                    let span = width * 0.92;
                    let fy = u.x.atan2(u.y);
                    let cell = (r.f() * 4.0) as usize % 4;
                    plans[sec].banners.add(
                        &quad(span, span / 8.0, Vec3::ZERO, cell_h(cell)),
                        place(p.x, 6.4, p.y, fy, 0.0, 0.0),
                        [1.0; 4],
                    );
                    for sd in [-1.0, 1.0] {
                        let c = p + Vec2::new(u.y, -u.x) * sd * (span / 2.0);
                        plans[sec].poles.add(
                            &cuboid(0.1, 6.9, 0.1),
                            Mat4::from_translation(Vec3::new(c.x, 3.45, c.y)),
                            hex(0x4a4a4a),
                        );
                    }
                    stats.crossings += 1;
                    next_b += 70.0 + r.f() * 70.0;
                }
            }
            run += l;
        }
    }
    // ---- neighbours: evenly along all of an area's streets
    for (sec, list) in sites.iter().enumerate() {
        let stride = (list.len() as f32 / 110.0).max(1.0);
        let mut f = 0.0;
        while (f as usize) < list.len() {
            let (p, _) = list[f as usize];
            let count = 2 + (r.f() * 4.0) as u32;
            stats.people += count;
            plans[sec].crowd.push((p, count));
            f += stride;
        }
    }
    plans
}

/// A knot of people and the firecracker paper at their feet (`group`).
fn group(out: &mut MeshBuf, at: Vec2, n: u32, spread: f32, paper: f32, n_paper: u32, r: &mut Lcg) {
    let w = crate::map::world();
    for _ in 0..n {
        let p = at + Vec2::new(r.f() - 0.5, r.f() - 0.5) * spread;
        person(out, p, r);
    }
    for _ in 0..n_paper {
        let q = at + Vec2::new(r.f() - 0.5, r.f() - 0.5) * paper;
        if w.bld_at(q).is_some() {
            continue;
        }
        let sz = 0.05 + r.f() * 0.09;
        let yaw = r.f() * std::f32::consts::TAU;
        let col = r.pick(&PAPER);
        out.add(
            &quad(sz, sz * 1.5, Vec3::ZERO, FULL),
            place(q.x, 0.03, q.y, yaw, -std::f32::consts::FRAC_PI_2, 0.0),
            hex(col),
        );
    }
}

/// One low-poly neighbour, a third of them waving a small flag (`person`).
fn person(out: &mut MeshBuf, p: Vec2, r: &mut Lcg) {
    let yaw = r.f() * std::f32::consts::TAU;
    let _seed = r.f();
    let shirt = r.pick(&SHIRT);
    let legs = r.pick(&LEGS);
    let skin = r.pick(&SKIN);
    let base = Mat4::from_translation(Vec3::new(p.x, 0.0, p.y)) * Mat4::from_rotation_y(yaw);
    let at = |x: f32, y: f32, z: f32| base * Mat4::from_translation(Vec3::new(x, y, z));
    out.add(&capsule(0.17, 0.36, 2, 6), at(0.0, 1.08, 0.0), hex(shirt));
    out.add(&sphere(0.115, 6, 4), at(0.0, 1.55, 0.0), hex(skin));
    out.add(&cuboid(0.11, 0.78, 0.13), at(-0.09, 0.39, 0.0), hex(legs));
    out.add(&cuboid(0.11, 0.78, 0.13), at(0.09, 0.39, 0.0), hex(legs));
    out.add(&cuboid(0.07, 0.56, 0.08), at(-0.23, 1.04, 0.0), hex(shirt));
    out.add(&cuboid(0.07, 0.56, 0.08), at(0.23, 1.04, 0.0), hex(shirt));
    if r.f() < 0.3 {
        out.add(
            &quad(0.42, 0.28, Vec3::new(0.47, 1.9, 0.0), FULL),
            base,
            hex(0xc8141a),
        );
        out.add(
            &quad(0.19, 0.13, Vec3::new(0.36, 1.97, 0.002), FULL),
            base,
            hex(0x1a2f7a),
        );
        out.add(&cuboid(0.02, 0.9, 0.02), at(0.25, 1.6, 0.0), hex(0xd8d2c0));
    }
}

fn build(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut stats: ResMut<StreetStats>,
) {
    let atlas = images.add(crate::map::textures::embedded(include_bytes!(
        "../../assets/textures/orders.png"
    )));
    let art = StreetArt {
        orders: materials.add(StandardMaterial {
            base_color_texture: Some(atlas),
            perceptual_roughness: 0.85,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        flag: materials.add(StandardMaterial {
            base_color_texture: Some(images.add(crate::remnant::flag_image())),
            perceptual_roughness: 0.8,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        crowd: materials.add(StandardMaterial {
            perceptual_roughness: 0.9,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
    };
    let mut plans = plan_all(&mut stats);
    let mut state = StreetState {
        plans: Vec::new(),
        ..default()
    };
    let poles_mat = materials.add(StandardMaterial {
        perceptual_roughness: 0.7,
        ..default()
    });
    for (i, plan) in plans.iter_mut().enumerate() {
        if plan.banners.is_empty() {
            continue;
        }
        let banners = std::mem::take(&mut plan.banners);
        let poles = std::mem::take(&mut plan.poles);
        let root = commands
            .spawn((Transform::default(), Visibility::default()))
            .with_children(|c| {
                c.spawn((
                    Mesh3d(meshes.add(banners.into_mesh())),
                    MeshMaterial3d(art.orders.clone()),
                    Transform::default(),
                    bevy::light::NotShadowCaster,
                ));
                if !poles.is_empty() {
                    c.spawn((
                        Mesh3d(meshes.add(poles.into_mesh())),
                        MeshMaterial3d(poles_mat.clone()),
                        Transform::default(),
                    ));
                }
            })
            .id();
        state.occ[i] = Some(root);
    }
    state.plans = plans;
    commands.insert_resource(state);
    commands.insert_resource(art);
}

fn reset_run(mut commands: Commands, mut state: ResMut<StreetState>) {
    for slot in state.free.iter_mut().take(N) {
        if let Some(e) = slot.take() {
            commands.entity(e).despawn();
        }
    }
    state.gathered = [false; N];
}

/// The free (reclaimed) meshes of area `i`, built the first time.
fn ensure_free(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    art: &StreetArt,
    state: &mut StreetState,
    i: usize,
) -> Entity {
    if let Some(e) = state.free[i] {
        return e;
    }
    let plan = &state.plans[i];
    let mut flags = MeshBuf::default();
    for (p, yaw, pitch, size) in &plan.flags {
        let h = size * 2.0 / 3.0;
        flags.add(
            &quad(*size, h, Vec3::new(size / 2.0, 0.0, 0.0), FULL),
            place(p.x, p.y, p.z, *yaw, 0.0, *pitch),
            [1.0; 4],
        );
    }
    let mut crowd = MeshBuf::default();
    for p in &plan.flag_poles {
        crowd.add(
            &cylinder(0.04, 0.05, 3.9, 5),
            Mat4::from_translation(*p),
            hex(0x4a4a4a),
        );
    }
    let mut rl = Lcg(0x2c0de ^ (i as u32).wrapping_mul(0x9e37_79b9));
    for (at, n) in &plan.crowd {
        group(&mut crowd, *at, *n, 1.8, 5.0, 14, &mut rl);
    }
    let root = commands
        .spawn((Transform::default(), Visibility::Hidden))
        .id();
    if !flags.is_empty() {
        let f = commands
            .spawn((
                Mesh3d(meshes.add(flags.into_mesh())),
                MeshMaterial3d(art.flag.clone()),
                Transform::default(),
                bevy::light::NotShadowCaster,
            ))
            .id();
        commands.entity(root).add_child(f);
    }
    if !crowd.is_empty() {
        let c = commands
            .spawn((
                Mesh3d(meshes.add(crowd.into_mesh())),
                MeshMaterial3d(art.crowd.clone()),
                Transform::default(),
                bevy::light::NotShadowCaster,
            ))
            .id();
        commands.entity(root).add_child(c);
    }
    state.free[i] = Some(root);
    root
}

/// Match the street to the reclaimed list (`sync`).
fn sync(
    mut commands: Commands,
    campaign: Res<Campaign>,
    art: Res<StreetArt>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut state: ResMut<StreetState>,
    mut vis: Query<&mut Visibility>,
) {
    for (i, sector) in SECTORS.iter().enumerate() {
        let on = campaign.reclaimed.iter().any(|n| n == sector.zh);
        if let Some(e) = state.occ[i]
            && let Ok(mut v) = vis.get_mut(e)
        {
            v.set_if_neq(if on {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            });
        }
        if on {
            let e = ensure_free(&mut commands, &mut meshes, &art, &mut state, i);
            if let Ok(mut v) = vis.get_mut(e) {
                v.set_if_neq(Visibility::Inherited);
            }
        } else if let Some(e) = state.free[i]
            && let Ok(mut v) = vis.get_mut(e)
        {
            v.set_if_neq(Visibility::Hidden);
        }
    }
}

/// The crowd that comes out around the player the moment area `i` falls
/// (`gather`), and again where it stood after a Continue.
fn gather(
    mut commands: Commands,
    mut parties: MessageReader<ReclaimParty>,
    art: Res<StreetArt>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut state: ResMut<StreetState>,
) {
    for party in parties.read() {
        let i = party.sector;
        if i >= N || state.gathered[i] {
            continue;
        }
        state.gathered[i] = true;
        let w = crate::map::world();
        let mut r = Lcg(0x51ab_u32.wrapping_add((i as u32).wrapping_mul(7919)));
        let mut out = MeshBuf::default();
        let mut placed = 0;
        for _ in 0..90 {
            if placed >= 16 {
                break;
            }
            let a = r.f() * std::f32::consts::TAU;
            let d = 5.0 + r.f() * 20.0;
            let q = party.at + Vec2::from_angle(a) * d;
            if w.bld_at(q).is_some() {
                continue;
            }
            let n = 2 + (r.f() * 3.0) as u32;
            group(&mut out, q, n, 1.6, 6.0, 22, &mut r);
            placed += 1;
        }
        if out.is_empty() {
            continue;
        }
        let free = ensure_free(&mut commands, &mut meshes, &art, &mut state, i);
        let c = commands
            .spawn((
                Mesh3d(meshes.add(out.into_mesh())),
                MeshMaterial3d(art.crowd.clone()),
                Transform::default(),
                bevy::light::NotShadowCaster,
            ))
            .id();
        commands.entity(free).add_child(c);
    }
}

/// After Continue the crowds stand where they gathered (no second party).
fn regather_on_load(
    mut commands: Commands,
    mut loaded: MessageReader<LoadCompleted>,
    campaign: Res<Campaign>,
    mut state: ResMut<StreetState>,
    mut parties: MessageWriter<ReclaimParty>,
) {
    if loaded.read().count() == 0 {
        return;
    }
    for slot in state.free.iter_mut().take(N) {
        if let Some(e) = slot.take() {
            commands.entity(e).despawn();
        }
    }
    state.gathered = [false; N];
    for (name, at) in &campaign.reclaim_at {
        if let Some(i) = SECTORS.iter().position(|s| s.zh == name) {
            parties.write(ReclaimParty {
                sector: i,
                at: Vec2::from_array(*at),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_core_areas_are_dressed_for_occupation_and_liberation() {
        let mut stats = StreetStats::default();
        let plans = plan_all(&mut stats);
        assert!(stats.banners > 50, "banners {}", stats.banners);
        assert!(stats.crossings > 5, "crossings {}", stats.crossings);
        assert!(stats.flags > 300, "flags {}", stats.flags);
        assert!(stats.people > 300, "people {}", stats.people);
        for (i, plan) in plans.iter().enumerate().take(6) {
            assert!(!plan.banners.is_empty(), "area {i} has banners");
            assert!(!plan.flags.is_empty(), "area {i} has flags");
            assert!(!plan.crowd.is_empty(), "area {i} has neighbours");
        }
        // an outer area keeps only the core's edge streets nearest its anchor
        for plan in plans.iter().skip(6) {
            assert!(plan.crowd.len() < plans[0].crowd.len());
        }
    }

    #[test]
    fn a_neighbour_has_a_body_and_paper_lies_flat() {
        let mut out = MeshBuf::default();
        let mut r = Lcg(1);
        group(&mut out, Vec2::new(200.0, 200.0), 3, 1.8, 5.0, 14, &mut r);
        assert!(out.vcount() > 3 * 100);
        let (lo, hi) = out.aabb();
        assert!(lo.y >= -0.01 && hi.y > 1.6 && hi.y < 2.2);
    }

    #[test]
    fn atlas_cells_follow_the_web_layout() {
        assert_eq!(cell_v(3), [0.375, 0.0, 0.5, 1.0]);
        assert_eq!(cell_h(1), [0.5, 0.25, 1.0, 0.5]);
    }
}
