//! v24.5 停下來的車流 — the occupied city's streets (Web `src/map/wreckage.ts`).
//!
//! Three days after the landing the traffic simply stopped: cars left in their
//! lanes (some slewed, some crashed, a few burnt out), buses abandoned on the
//! wide roads, scooters packed nose-in along the kerbs and dropped in the lanes,
//! water-filled and jersey barriers across lanes, and litter everywhere.
//! Vehicles and barriers are low cover (cars 1.45 m, buses 2.8 m, barriers 1 m),
//! planned with the world so movement, sight and shots share them. Rendering
//! merges each 80 m cell and material into one mesh, so off-screen cells are
//! culled whole.

use bevy::prelude::*;
use std::collections::HashMap;

use super::data::{WorldData, seg_dist};
use crate::geom::{MeshBuf, cuboid, hex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WreckKind {
    Car,
    Bus,
    Scooter,
    Barrier,
    Debris,
    Scorch,
}

/// One placed piece: kind, ground position, heading, roll (a scooter on its
/// side) and the instance colour that tints its white parts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wreck {
    pub kind: WreckKind,
    pub p: Vec2,
    pub ang: f32,
    pub tilt: f32,
    pub color: u32,
}

impl Wreck {
    /// The Web's `Euler(0, ang, tilt, 'YXZ')` at ground level.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_translation(Vec3::new(self.p.x, 0.0, self.p.y))
            * Mat4::from_rotation_y(self.ang)
            * Mat4::from_rotation_z(self.tilt)
    }
}

/// Instances are grouped by 80 m cells.
pub const CELL_W: f32 = 80.0;

const CAR: [u32; 8] = [
    0xd8d8d8, 0x30343a, 0x8a1a1a, 0x2a4a7a, 0xbfbfbf, 0xe8e8e8, 0x6a6e72, 0x1f3a2a,
];
const SCOOT: [u32; 11] = [
    0xd8d8d4, 0xd8d8d4, 0x26282a, 0x26282a, 0x8e9498, 0x8e9498, 0xa63a30, 0x2f4f86, 0x7fa8a2,
    0xb8963a, 0xb07f90,
];
const BURNT: u32 = 0x1c1a18;
const TAXI: u32 = 0xf2c21a;

struct Rng(u32);

impl Rng {
    fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
    fn pick(&mut self, v: &[u32]) -> u32 {
        v[(self.f() * v.len() as f32) as usize % v.len()]
    }
}

/// `buildWreckage`: plans every piece (`Obstacles` group 4 builds cover colliders from it).
/// `blocked` keeps doorways and the start clear; `keep_off` leaves
/// fortified corners, checkpoints, road junctions and armor routes alone
/// (checked last, so the stream is stable).
pub fn plan(
    w: &WorldData,
    blocked: impl Fn(Vec2) -> bool,
    keep_off: impl Fn(Vec2) -> bool,
) -> Vec<Wreck> {
    let mut rng = Rng(0x0005_e1ec);
    let mut out: Vec<Wreck> = Vec::new();
    let mut taken: Vec<Vec2> = Vec::new();
    let free = |p: Vec2, r: f32, taken: &[Vec2]| {
        w.bld_at(p).is_none() && !blocked(p) && taken.iter().all(|q| q.distance(p) > r)
    };
    let put = |out: &mut Vec<Wreck>, kind, p: Vec2, ang, color, tilt| {
        if keep_off(p) {
            return;
        }
        out.push(Wreck {
            kind,
            p,
            ang,
            tilt,
            color,
        });
    };
    for r in &w.roads {
        let width = r.w;
        let main = width >= 9.0; // tertiary and up: lanes of traffic
        for pair in r.p.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let l = a.distance(b);
            if l < 6.0 {
                continue;
            }
            let t = (b - a) / l;
            let n = Vec2::new(-t.y, t.x);
            let head = t.x.atan2(t.y);
            let at = |s: f32, off: f32| a + t * s + n * off;
            // kerbside scooter rows: 4–9 scooters nose-in, 0.75 m apart
            if width >= 6.0 {
                let mut s = 4.0 + rng.f() * 16.0;
                while s < l - 4.0 {
                    let side = if rng.f() < 0.5 { -1.0 } else { 1.0 };
                    let off = width / 2.0 - 0.7;
                    let count = 4 + (rng.f() * 6.0) as u32;
                    for k in 0..count {
                        let ts = s + k as f32 * 0.75;
                        if ts > l - 2.0 {
                            break;
                        }
                        let p = at(ts, off * side);
                        if w.bld_at(p).is_some() || blocked(p) {
                            continue;
                        }
                        let down = rng.f() < 0.12; // knocked over in the panic
                        let ang = head + std::f32::consts::FRAC_PI_2 * side + (rng.f() - 0.5) * 0.3;
                        let color = rng.pick(&SCOOT);
                        put(
                            &mut out,
                            WreckKind::Scooter,
                            p,
                            ang,
                            color,
                            if down { 1.35 } else { 0.0 },
                        );
                    }
                    s += 34.0 + rng.f() * 40.0;
                }
            }
            if !main && width >= 6.0 {
                // residential lanes: cars nose to tail along one kerb
                let side = if rng.f() < 0.5 { -1.0 } else { 1.0 };
                let mut s = 5.0 + rng.f() * 12.0;
                while s < l - 4.0 {
                    if rng.f() >= 0.4 {
                        let p = at(s, (width / 2.0 - 1.05) * side);
                        if free(p, 4.6, &taken) {
                            taken.push(p);
                            let burnt = rng.f() < 0.04;
                            let flip = if rng.f() < 0.5 {
                                std::f32::consts::PI
                            } else {
                                0.0
                            };
                            let ang = head + flip + (rng.f() - 0.5) * 0.12;
                            let color = if burnt {
                                BURNT
                            } else if rng.f() < 0.12 {
                                TAXI
                            } else {
                                rng.pick(&CAR)
                            };
                            put(&mut out, WreckKind::Car, p, ang, color, 0.0);
                            if burnt {
                                let a2 = rng.f() * std::f32::consts::TAU;
                                put(&mut out, WreckKind::Scorch, p, a2, 0x0c0a08, 0.0);
                            }
                        }
                    }
                    s += 6.0 + rng.f() * 14.0;
                }
            }
            if !main {
                // a dropped scooter or two on the narrow streets
                let mut s = rng.f() * 30.0;
                while s < l {
                    let p = at(s, (rng.f() - 0.5) * width * 0.5);
                    if free(p, 1.2, &taken) {
                        let ang = rng.f() * std::f32::consts::TAU;
                        let color = rng.pick(&SCOOT);
                        put(&mut out, WreckKind::Scooter, p, ang, color, 1.4);
                    }
                    s += 28.0 + rng.f() * 40.0;
                }
            } else {
                // abandoned traffic: one vehicle every 9–24 m per direction
                for dir in [-1.0_f32, 1.0] {
                    let mut s = 3.0 + rng.f() * 10.0;
                    while s < l - 3.0 {
                        let lane = dir * (width / 4.0 + (rng.f() - 0.5) * 0.8);
                        let p = at(s, lane);
                        let bus = width >= 12.0 && rng.f() < 0.1;
                        if free(p, if bus { 7.0 } else { 3.4 }, &taken) {
                            let crash = rng.f() < 0.18;
                            let slew = if crash {
                                (rng.f() - 0.5) * 1.6
                            } else {
                                (rng.f() - 0.5) * 0.22
                            };
                            let ang =
                                head + if dir < 0.0 { std::f32::consts::PI } else { 0.0 } + slew;
                            let burnt = rng.f() < 0.09;
                            taken.push(p);
                            if bus {
                                let color = if burnt {
                                    BURNT
                                } else {
                                    rng.pick(&[0x2a8a5a, 0x3a6ab8, 0xe8e8e8])
                                };
                                put(&mut out, WreckKind::Bus, p, ang, color, 0.0);
                            } else {
                                let taxi = !burnt && rng.f() < 0.2;
                                let color = if burnt {
                                    BURNT
                                } else if taxi {
                                    TAXI
                                } else {
                                    rng.pick(&CAR)
                                };
                                put(&mut out, WreckKind::Car, p, ang, color, 0.0);
                            }
                            if burnt {
                                let a2 = rng.f() * std::f32::consts::TAU;
                                put(&mut out, WreckKind::Scorch, p, a2, 0x0c0a08, 0.0);
                            }
                            // glass and paper around it
                            for _ in 0..3 {
                                let q = p + Vec2::new(rng.f() - 0.5, rng.f() - 0.5) * 5.0;
                                let a2 = rng.f() * std::f32::consts::TAU;
                                let color =
                                    rng.pick(&[0xe8e4d6, 0xbfe0e8, 0xd84a3a, 0x3a6ab8, 0xf2f2f2]);
                                put(&mut out, WreckKind::Debris, q, a2, color, 0.0);
                            }
                        }
                        s += 9.0 + rng.f() * 15.0;
                    }
                }
                // a barrier line across one lane here and there
                if l > 30.0 && rng.f() < 0.35 {
                    let s = l * (0.3 + rng.f() * 0.4);
                    let side = if rng.f() < 0.5 { -1.0 } else { 1.0 };
                    for k in 0..(width / 4.2).ceil() as u32 {
                        let p = at(s, side * (0.6 + k as f32 * 2.1));
                        if !free(p, 1.5, &taken) {
                            continue;
                        }
                        let water = rng.f() < 0.6;
                        let ang = head + std::f32::consts::FRAC_PI_2 + (rng.f() - 0.5) * 0.25;
                        let color = if water {
                            if k % 2 == 1 { 0xd8d8d8 } else { 0xc8302a }
                        } else {
                            0x9a9a94
                        };
                        put(&mut out, WreckKind::Barrier, p, ang, color, 0.0);
                    }
                }
            }
            // litter along every road
            let mut s = rng.f() * 8.0;
            while s < l {
                let p = at(s, (rng.f() - 0.5) * width);
                let a2 = rng.f() * std::f32::consts::TAU;
                let color = rng.pick(&[0xe8e4d6, 0xf2f2f2, 0x2a2a2a, 0xd84a3a, 0x8fc8c0]);
                put(&mut out, WreckKind::Debris, p, a2, color, 0.0);
                s += 7.0 + rng.f() * 10.0;
            }
        }
    }
    out
}

/// Low cover colliders for [`crate::map::Obstacles`] group 4 (vehicles and barriers).
pub fn colliders(wrecks: &[Wreck]) -> Vec<super::collide::DynamicCollider> {
    let mut out = Vec::new();
    let mut id = 0x3_0000_0000_u64;
    for x in wrecks {
        let (hw, hl, height) = match x.kind {
            WreckKind::Car => (0.9, 2.2, 1.45),
            WreckKind::Bus => (1.25, 5.5, 2.8),
            WreckKind::Barrier => (1.0, 0.25, 1.0),
            WreckKind::Scooter | WreckKind::Debris | WreckKind::Scorch => continue,
        };
        let u = Vec2::new(x.ang.sin(), x.ang.cos());
        let v = Vec2::new(u.y, -u.x);
        out.push(super::collide::DynamicCollider {
            id,
            corners: [
                x.p + u * hl + v * hw,
                x.p + u * hl - v * hw,
                x.p - u * hl - v * hw,
                x.p - u * hl + v * hw,
            ],
            height,
        });
        id += 1;
    }
    out
}

/// Adds the wreckage to the world without mutating `w.segs` (cover lives in
/// `Obstacles` group 4), keeping fortified corners, checkpoints, road
/// endpoints and armoured patrol routes clear.
pub fn install(w: &mut WorldData) {
    let doors: Vec<Vec2> = w.blds.iter().filter_map(|b| b.door.map(|d| d.p)).collect();
    let spawn = w.spawn;
    let corners: Vec<Vec2> = w
        .remnants
        .iter()
        .map(|s| s.p)
        .chain(w.checkpoints.iter().map(|c| c.p))
        .collect();
    let endpoints: Vec<Vec2> = w
        .roads
        .iter()
        .filter_map(|r| r.p.first().copied())
        .chain(w.roads.iter().filter_map(|r| r.p.last().copied()))
        .collect();
    let armor_routes: Vec<Vec<[f32; 2]>> = crate::combat::armor::ArmorFleet::planned(w)
        .units
        .into_iter()
        .map(|u| u.route)
        .collect();
    w.wrecks = plan(
        w,
        |p| p.distance(spawn) < 25.0 || doors.iter().any(|d| d.distance(p) < 3.5),
        |p| {
            corners.iter().any(|c| c.distance(p) < 10.0)
                || endpoints.iter().any(|e| e.distance(p) < 8.0)
                || armor_routes.iter().any(|route| {
                    route.windows(2).any(|ab| {
                        seg_dist(p, Vec2::from_array(ab[0]), Vec2::from_array(ab[1])) < 6.5
                    })
                })
        },
    );
}

fn part(m: &mut MeshBuf, w: f32, h: f32, d: f32, x: f32, y: f32, z: f32, c: u32) {
    m.add(
        &cuboid(w, h, d),
        Mat4::from_translation(Vec3::new(x, y, z)),
        hex(c),
    );
}

/// Sedan / taxi: body tinted by the instance colour, dark glass, black wheels.
pub fn car_geo() -> MeshBuf {
    let mut m = MeshBuf::default();
    part(&mut m, 1.8, 0.62, 4.4, 0.0, 0.62, 0.0, 0xffffff);
    part(&mut m, 1.62, 0.56, 2.2, 0.0, 1.2, -0.25, 0x3a4048);
    part(&mut m, 1.7, 0.08, 0.9, 0.0, 0.95, 1.55, 0xd8d8d8);
    for z in [-1.4, 1.35] {
        part(&mut m, 0.22, 0.6, 0.62, -0.86, 0.3, z, 0x141414);
        part(&mut m, 0.22, 0.6, 0.62, 0.86, 0.3, z, 0x141414);
    }
    m
}

/// City bus: long body, window band, roof unit.
pub fn bus_geo() -> MeshBuf {
    let mut m = MeshBuf::default();
    part(&mut m, 2.5, 2.4, 11.0, 0.0, 1.55, 0.0, 0xffffff);
    part(&mut m, 2.52, 0.9, 10.2, 0.0, 2.05, -0.2, 0x2a3138);
    part(&mut m, 1.6, 0.35, 2.6, 0.0, 2.9, -1.5, 0xc8c8c8);
    for z in [-3.6, 3.4] {
        part(&mut m, 0.25, 0.9, 1.0, -1.2, 0.45, z, 0x141414);
        part(&mut m, 0.25, 0.9, 1.0, 1.2, 0.45, z, 0x141414);
    }
    m
}

/// Scooter (機車): body, front shield, seat, the green delivery box.
pub fn scooter_geo() -> MeshBuf {
    let mut m = MeshBuf::default();
    part(&mut m, 0.34, 0.5, 1.5, 0.0, 0.3, 0.0, 0xffffff);
    part(&mut m, 0.32, 0.5, 0.24, 0.0, 0.75, 0.5, 0xffffff);
    part(&mut m, 0.36, 0.12, 0.55, 0.0, 0.6, -0.2, 0x1e1e1e);
    part(&mut m, 0.32, 0.26, 0.36, 0.0, 0.6, -0.62, 0x2a6a3a);
    m
}

/// Water-filled barrier (red / white) or concrete jersey barrier, 2 m long.
pub fn barrier_geo() -> MeshBuf {
    let mut m = MeshBuf::default();
    part(&mut m, 2.0, 0.8, 0.5, 0.0, 0.4, 0.0, 0xffffff);
    part(&mut m, 1.9, 0.12, 0.36, 0.0, 0.86, 0.0, 0xffffff);
    m
}

/// A scrap of paper, plastic or glass flat on the road.
pub fn debris_geo() -> MeshBuf {
    cuboid(0.4, 0.005, 0.3).transformed(Mat4::from_translation(Vec3::new(0.0, 0.02, 0.0)))
}

/// A burnt car's scorch mark (`CircleGeometry(2.6, 14)`).
pub fn scorch_geo() -> MeshBuf {
    let pts: Vec<Vec2> = (0..14)
        .map(|i| Vec2::from_angle(i as f32 / 14.0 * std::f32::consts::TAU) * 2.6)
        .collect();
    crate::geom::shape(&pts).transformed(
        Mat4::from_translation(Vec3::new(0.0, 0.015, 0.0))
            * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2),
    )
}

/// Spawns the merged wreckage meshes: per 80 m cell, one vertex-coloured mesh
/// for the vehicles and barriers, one for the litter and one for scorch marks.
pub struct WreckagePlugin;

impl Plugin for WreckagePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_wreckage);
    }
}

fn spawn_wreckage(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let w = super::world();
    let geos = [
        (WreckKind::Car, car_geo()),
        (WreckKind::Bus, bus_geo()),
        (WreckKind::Scooter, scooter_geo()),
        (WreckKind::Barrier, barrier_geo()),
        (WreckKind::Debris, debris_geo()),
        (WreckKind::Scorch, scorch_geo()),
    ];
    let geo = |k: WreckKind| &geos.iter().find(|(g, _)| *g == k).unwrap().1;
    // 0: vehicles and barriers, 1: litter, 2: scorch
    let mut cells: HashMap<(i32, i32, u8), MeshBuf> = HashMap::new();
    for x in &w.wrecks {
        let layer = match x.kind {
            WreckKind::Debris => 1,
            WreckKind::Scorch => 2,
            _ => 0,
        };
        let key = (
            (x.p.x / CELL_W).floor() as i32,
            (x.p.y / CELL_W).floor() as i32,
            layer,
        );
        cells
            .entry(key)
            .or_default()
            .add(geo(x.kind), x.matrix(), hex(x.color));
    }
    let paint = materials.add(StandardMaterial {
        perceptual_roughness: 0.45,
        metallic: 0.3,
        ..default()
    });
    let litter = materials.add(StandardMaterial {
        perceptual_roughness: 0.9,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let scorch = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.55),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let mut keys: Vec<_> = cells.keys().copied().collect();
    keys.sort_unstable();
    for key in keys {
        let buf = cells.remove(&key).unwrap();
        let material = match key.2 {
            0 => paint.clone(),
            1 => litter.clone(),
            _ => scorch.clone(),
        };
        let mut e = commands.spawn((
            WreckCell,
            Mesh3d(meshes.add(buf.into_mesh())),
            MeshMaterial3d(material),
            Transform::default(),
            Visibility::default(),
        ));
        if key.2 != 0 {
            e.insert(bevy::light::NotShadowCaster);
        }
    }
}

/// One merged cell of wreckage.
#[derive(Component)]
pub struct WreckCell;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_streets_are_full_and_the_start_is_clear() {
        let w = crate::map::world();
        let count = |k| w.wrecks.iter().filter(|x| x.kind == k).count();
        assert!(
            count(WreckKind::Car) > 100,
            "cars {}",
            count(WreckKind::Car)
        );
        assert!(count(WreckKind::Scooter) > 200);
        assert!(count(WreckKind::Debris) > 500);
        assert!(count(WreckKind::Barrier) > 0);
        for x in &w.wrecks {
            if matches!(x.kind, WreckKind::Car | WreckKind::Bus | WreckKind::Barrier) {
                assert!(x.p.distance(w.spawn) >= 25.0);
                assert!(w.bld_at(x.p).is_none());
            }
        }
        for site in &w.remnants {
            assert!(w.wrecks.iter().all(|x| x.p.distance(site.p) >= 10.0));
        }
    }

    #[test]
    fn plan_is_deterministic() {
        let w = crate::map::world();
        let a = plan(w, |_| false, |_| false);
        let b = plan(w, |_| false, |_| false);
        assert_eq!(a, b);
    }

    #[test]
    fn vehicles_are_low_cover() {
        let w = crate::map::world();
        let car = w
            .wrecks
            .iter()
            .find(|x| x.kind == WreckKind::Car)
            .expect("a car");
        let cols = colliders(&w.wrecks);
        assert!(cols.iter().any(|c| {
            (c.height - 1.45).abs() < 0.01 && c.corners.iter().any(|p| p.distance(car.p) < 3.0)
        }));
        let obs = crate::map::Obstacles::new(w.clone());
        assert_eq!(obs.find_cover(car.p, 2.4), Some(true));
    }
}
