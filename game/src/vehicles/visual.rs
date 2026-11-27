//! Shared, brand-free procedural ground vehicles. Shapes follow drive.ts and
//! armor.ts; native vertex-colour batches keep one body/wheel/turret mesh per kind.
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::f32::consts::FRAC_PI_2;

use super::{VehicleFleet, VehicleKind};
use crate::geom::{self, MeshBuf, hex};

pub struct VehicleModel {
    pub body: MeshBuf,
    pub wheels: MeshBuf,
    pub turret: MeshBuf,
}

fn bx(out: &mut MeshBuf, size: Vec3, pos: Vec3, color: u32) {
    out.add(
        &geom::cuboid(size.x, size.y, size.z),
        Mat4::from_translation(pos),
        hex(color),
    );
}
fn wheel(out: &mut MeshBuf, radius: f32, width: f32, pos: Vec3) {
    out.add(
        &geom::cylinder(radius, radius, width, 12),
        Mat4::from_translation(pos) * Mat4::from_rotation_z(FRAC_PI_2),
        hex(0x17191a),
    );
    for sign in [-1.0, 1.0] {
        out.add(
            &geom::cylinder(radius * 0.52, radius * 0.52, 0.012, 8),
            Mat4::from_translation(pos + Vec3::X * sign * (width * 0.5 + 0.008))
                * Mat4::from_rotation_z(FRAC_PI_2),
            hex(0x7b8182),
        );
    }
}
fn barrel(out: &mut MeshBuf, radius: f32, length: f32, pos: Vec3) {
    out.add(
        &geom::cylinder(radius, radius, length, 8),
        Mat4::from_translation(pos) * Mat4::from_rotation_x(FRAC_PI_2),
        hex(0x25291f),
    );
}

pub fn model(kind: VehicleKind) -> VehicleModel {
    let mut m = VehicleModel {
        body: MeshBuf::default(),
        wheels: MeshBuf::default(),
        turret: MeshBuf::default(),
    };
    let s = kind.spec();
    let b = &mut m.body;
    if s.armored {
        let col = 0x627046;
        if matches!(kind, VehicleKind::Zbd05 | VehicleKind::Ztd05) {
            bx(b, Vec3::new(3.3, 1.3, 7.5), Vec3::new(0.0, 1.25, -0.3), col);
            bx(b, Vec3::new(3.3, 0.9, 2.0), Vec3::new(0.0, 1.45, 3.7), col);
            bx(
                b,
                Vec3::new(3.6, 0.18, 8.3),
                Vec3::new(0.0, 1.92, 0.0),
                0x596744,
            );
            for side in [-1.0, 1.0] {
                bx(
                    &mut m.wheels,
                    Vec3::new(0.55, 0.9, 8.0),
                    Vec3::new(side * 1.55, 0.55, -0.2),
                    0x1d201b,
                );
                for i in 0..6 {
                    wheel(
                        &mut m.wheels,
                        0.38,
                        0.6,
                        Vec3::new(side * 1.55, 0.45, -3.6 + i as f32 * 1.35),
                    );
                }
            }
            if kind == VehicleKind::Ztd05 {
                bx(
                    &mut m.turret,
                    Vec3::new(2.8, 0.9, 3.2),
                    Vec3::new(0.0, 2.45, 0.2),
                    col,
                );
                bx(
                    &mut m.turret,
                    Vec3::new(2.4, 0.6, 1.2),
                    Vec3::new(0.0, 2.35, -1.8),
                    col,
                );
                barrel(&mut m.turret, 0.11, 5.4, Vec3::new(0.0, 2.55, 4.3));
            } else {
                bx(
                    &mut m.turret,
                    Vec3::new(2.2, 0.8, 2.2),
                    Vec3::new(0.0, 2.4, 0.6),
                    col,
                );
                barrel(&mut m.turret, 0.08, 2.8, Vec3::new(0.0, 2.5, 2.1));
            }
        } else if kind == VehicleKind::Zbl08 {
            bx(b, Vec3::new(3.0, 1.5, 6.6), Vec3::new(0.0, 1.6, -0.4), col);
            bx(b, Vec3::new(2.9, 1.2, 1.5), Vec3::new(0.0, 1.7, 3.0), col);
            for side in [-1.0, 1.0] {
                for z in [-2.9, -1.5, 1.2, 2.6] {
                    wheel(&mut m.wheels, 0.55, 0.45, Vec3::new(side * 1.45, 0.55, z));
                }
            }
            bx(
                &mut m.turret,
                Vec3::new(2.0, 0.7, 2.0),
                Vec3::new(0.0, 2.7, 0.4),
                col,
            );
            barrel(&mut m.turret, 0.07, 2.4, Vec3::new(0.0, 2.75, 1.9));
        } else {
            bx(b, Vec3::new(2.2, 1.0, 4.9), Vec3::new(0.0, 1.0, 0.0), col);
            bx(b, Vec3::new(2.1, 0.9, 2.6), Vec3::new(0.0, 1.95, -0.6), col);
            bx(
                b,
                Vec3::new(1.9, 0.6, 0.04),
                Vec3::new(0.0, 1.95, 0.72),
                0x1a2228,
            );
            for side in [-1.0, 1.0] {
                for z in [-1.6, 1.6] {
                    wheel(&mut m.wheels, 0.48, 0.4, Vec3::new(side * 1.05, 0.48, z));
                }
            }
            bx(
                &mut m.turret,
                Vec3::new(0.7, 0.4, 0.7),
                Vec3::new(0.0, 2.6, -0.6),
                0x3a3d36,
            );
            barrel(&mut m.turret, 0.05, 1.3, Vec3::new(0.0, 2.75, 0.1));
        }
        // Shared stencil-like panels and irregular block camouflage, no insignia.
        for side in [-1.0, 1.0] {
            for i in 0..5 {
                bx(
                    b,
                    Vec3::new(0.012, 0.22, 0.55),
                    Vec3::new(
                        side * (s.width * 0.5 + 0.01),
                        1.2 + (i % 2) as f32 * 0.3,
                        -s.length * 0.33 + i as f32 * s.length * 0.15,
                    ),
                    if i % 2 == 0 { 0x39452e } else { 0x797655 },
                );
            }
        }
        return m;
    }
    if s.two {
        if s.pedal {
            let frame = if kind == VehicleKind::Ubike {
                0x2a9a8a
            } else {
                0xd4abb9
            };
            bx(
                b,
                Vec3::new(0.055, 0.055, 0.95),
                Vec3::new(0.0, 0.5, 0.0),
                frame,
            );
            bx(
                b,
                Vec3::new(0.055, 0.55, 0.055),
                Vec3::new(0.0, 0.6, -0.2),
                frame,
            );
            bx(
                b,
                Vec3::new(0.055, 0.6, 0.055),
                Vec3::new(0.0, 0.68, 0.48),
                frame,
            );
            bx(
                b,
                Vec3::new(0.52, 0.04, 0.04),
                Vec3::new(0.0, 0.99, 0.45),
                0x242728,
            );
            bx(
                b,
                Vec3::new(0.22, 0.07, 0.29),
                Vec3::new(0.0, 0.85, -0.2),
                0x292425,
            );
            bx(
                b,
                Vec3::new(0.36, 0.24, 0.3),
                Vec3::new(0.0, 0.82, 0.67),
                if kind == VehicleKind::Ubike {
                    0xc1ddd4
                } else {
                    0x9b8a66
                },
            );
            for z in [-0.55, 0.6] {
                wheel(&mut m.wheels, 0.33, 0.05, Vec3::new(0.0, 0.33, z));
            }
        } else {
            let color = match kind {
                VehicleKind::Delivery => 0x536a8c,
                VehicleKind::Escooter => 0xa5d6d5,
                VehicleKind::Moto => 0x713a30,
                _ => 0xb4493e,
            };
            if kind == VehicleKind::Moto {
                bx(
                    b,
                    Vec3::new(0.27, 0.24, 0.5),
                    Vec3::new(0.0, 0.88, 0.2),
                    color,
                );
                bx(
                    b,
                    Vec3::new(0.23, 0.3, 0.5),
                    Vec3::new(0.0, 0.5, 0.05),
                    0x494b4a,
                );
                bx(
                    b,
                    Vec3::new(0.26, 0.1, 0.62),
                    Vec3::new(0.0, 0.82, -0.25),
                    0x252528,
                );
                for z in [-0.72, 0.78] {
                    wheel(&mut m.wheels, 0.31, 0.14, Vec3::new(0.0, 0.31, z));
                }
            } else {
                bx(
                    b,
                    Vec3::new(0.36, 0.28, 0.95),
                    Vec3::new(0.0, 0.42, -0.05),
                    color,
                );
                bx(
                    b,
                    Vec3::new(0.32, 0.6, 0.18),
                    Vec3::new(0.0, 0.66, 0.48),
                    color,
                );
                bx(
                    b,
                    Vec3::new(0.34, 0.1, 0.56),
                    Vec3::new(0.0, 0.62, -0.2),
                    0x252528,
                );
                bx(
                    b,
                    Vec3::new(0.3, 0.08, 0.42),
                    Vec3::new(0.0, 0.32, 0.25),
                    0x303233,
                );
                for z in [-0.45, 0.55] {
                    wheel(&mut m.wheels, 0.2, 0.1, Vec3::new(0.0, 0.2, z));
                }
            }
            bx(
                b,
                Vec3::new(0.62, 0.04, 0.04),
                Vec3::new(0.0, 1.0, 0.56),
                0xa7aaaa,
            );
            bx(
                b,
                Vec3::new(0.16, 0.1, 0.06),
                Vec3::new(0.0, 0.88, 0.62),
                0xe9e4b8,
            );
            bx(
                b,
                Vec3::new(0.2, 0.1, 0.012),
                Vec3::new(0.0, 0.36, -0.56),
                0xe2e0d5,
            );
            if kind == VehicleKind::Delivery {
                bx(
                    b,
                    Vec3::new(0.46, 0.42, 0.42),
                    Vec3::new(0.0, 0.92, -0.38),
                    0x3a3f8a,
                );
                bx(
                    b,
                    Vec3::new(0.3, 0.12, 0.012),
                    Vec3::new(0.0, 0.93, -0.6),
                    0xf3dc8d,
                );
            }
        }
        return m;
    }
    let (l, w) = (s.length, s.width);
    let paint = match kind {
        VehicleKind::Taxi => 0xeabc27,
        VehicleKind::Kei => 0x416d9a,
        VehicleKind::Van => 0xcaccc3,
        VehicleKind::Bus => 0xdbe4dc,
        VehicleKind::Garbage => 0xe8c528,
        _ => 0x627387,
    };
    if matches!(kind, VehicleKind::Bus | VehicleKind::Garbage) {
        bx(
            b,
            Vec3::new(w, if kind == VehicleKind::Bus { 2.6 } else { 2.2 }, l),
            Vec3::new(0.0, if kind == VehicleKind::Bus { 1.65 } else { 1.5 }, 0.0),
            paint,
        );
        for sign in [-1.0, 1.0] {
            bx(
                b,
                Vec3::new(0.012, 0.9, l * 0.84),
                Vec3::new(sign * (w * 0.5 + 0.012), 2.1, 0.0),
                0x26343c,
            );
            bx(
                b,
                Vec3::new(0.014, 0.35, l * 0.9),
                Vec3::new(sign * (w * 0.5 + 0.014), 0.9, 0.0),
                if kind == VehicleKind::Bus {
                    0x398565
                } else {
                    0x617857
                },
            );
        }
    } else if kind == VehicleKind::Kei {
        bx(
            b,
            Vec3::new(w, 1.35, 1.25),
            Vec3::new(0.0, 1.0, l * 0.5 - 0.62),
            paint,
        );
        bx(
            b,
            Vec3::new(w * 0.94, 0.55, 0.035),
            Vec3::new(0.0, 1.32, l * 0.5 + 0.02),
            0x26343c,
        );
        bx(
            b,
            Vec3::new(w, 0.12, l - 1.3),
            Vec3::new(0.0, 0.62, -0.62),
            0x626364,
        );
        for sign in [-1.0, 1.0] {
            bx(
                b,
                Vec3::new(0.04, 0.35, l - 1.3),
                Vec3::new(sign * w * 0.5, 0.85, -0.62),
                0x8b8f91,
            );
        }
        bx(
            b,
            Vec3::new(w, 0.35, 0.04),
            Vec3::new(0.0, 0.85, -l * 0.5 + 0.02),
            0x8b8f91,
        );
    } else if kind == VehicleKind::Van {
        bx(
            b,
            Vec3::new(w, 1.55, l * 0.88),
            Vec3::new(0.0, 1.12, -l * 0.06),
            paint,
        );
        bx(
            b,
            Vec3::new(w, 0.8, l * 0.14),
            Vec3::new(0.0, 0.75, l * 0.43),
            paint,
        );
        bx(
            b,
            Vec3::new(w * 0.92, 0.6, 0.035),
            Vec3::new(0.0, 1.4, l * 0.381),
            0x26343c,
        );
        for sign in [-1.0, 1.0] {
            bx(
                b,
                Vec3::new(0.012, 0.45, l * 0.6),
                Vec3::new(sign * (w * 0.5 + 0.01), 1.45, -0.1),
                0x26343c,
            );
        }
    } else {
        bx(b, Vec3::new(w, 0.62, l), Vec3::new(0.0, 0.66, 0.0), paint);
        bx(
            b,
            Vec3::new(w * 0.9, 0.55, l * 0.48),
            Vec3::new(0.0, 1.24, -0.15),
            paint,
        );
        for sign in [-1.0, 1.0] {
            bx(
                b,
                Vec3::new(w * 0.86, 0.44, 0.025),
                Vec3::new(0.0, 1.24, -0.15 + sign * (l * 0.24 + 0.014)),
                0x26343c,
            );
            bx(
                b,
                Vec3::new(0.025, 0.4, l * 0.43),
                Vec3::new(sign * (w * 0.45 + 0.014), 1.26, -0.15),
                0x26343c,
            );
        }
        if kind == VehicleKind::Taxi {
            bx(
                b,
                Vec3::new(0.62, 0.2, 0.26),
                Vec3::new(0.0, 1.62, -0.15),
                0xf4ecd8,
            );
        }
    }
    let wr = if matches!(kind, VehicleKind::Bus | VehicleKind::Garbage) {
        0.5
    } else if kind == VehicleKind::Kei {
        0.28
    } else {
        0.32
    };
    for sign in [-1.0, 1.0] {
        for z in [
            -l * 0.5 + (l * 0.17).max(0.6),
            l * 0.5 - (l * 0.17).max(0.55),
        ] {
            wheel(
                &mut m.wheels,
                wr,
                0.22,
                Vec3::new(sign * (w * 0.5 - 0.05), wr, z),
            );
        }
        bx(
            b,
            Vec3::new(0.26, 0.12, 0.025),
            Vec3::new(sign * (w * 0.5 - 0.3), 0.72, l * 0.5 + 0.014),
            0xe9e4bd,
        );
        bx(
            b,
            Vec3::new(0.23, 0.12, 0.025),
            Vec3::new(sign * (w * 0.5 - 0.3), 0.72, -l * 0.5 - 0.014),
            0xad2924,
        );
    }
    m
}

/// Shared by the renderer and the reproducible geometry workload inventory.
pub fn docking_post() -> MeshBuf {
    geom::cuboid(0.12, 0.9, 0.12).tinted(hex(0x2a9a8a))
}

#[derive(Component)]
pub struct VehicleVisual(pub u32, pub VehicleKind);
#[derive(Component)]
pub(crate) struct VehicleTurret(pub u32);
#[derive(Component)]
pub(crate) struct BikeDock;

/// Retained upload metadata: Bevy may discard CPU arrays after extraction.
#[derive(Component, Clone, Copy, Debug)]
pub struct VehicleGeometry {
    pub vertices: usize,
    pub indices: usize,
    pub min: Vec3,
    pub max: Vec3,
}
impl VehicleGeometry {
    fn from_mesh(mesh: &MeshBuf) -> Self {
        let (min, max) = mesh.aabb();
        Self {
            vertices: mesh.pos.len(),
            indices: mesh.idx.len(),
            min,
            max,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct VehicleArt {
    models: HashMap<
        VehicleKind,
        (
            (Handle<Mesh>, VehicleGeometry),
            (Handle<Mesh>, VehicleGeometry),
            Option<(Handle<Mesh>, VehicleGeometry)>,
        ),
    >,
    material: Handle<StandardMaterial>,
}

pub(crate) fn setup_art(
    mut commands: Commands,
    fleet: Res<VehicleFleet>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut art: ResMut<VehicleArt>,
) {
    art.material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.72,
        metallic: 0.16,
        ..default()
    });
    for kind in VehicleKind::ALL {
        let m = model(kind);
        let body_geometry = VehicleGeometry::from_mesh(&m.body);
        let wheel_geometry = VehicleGeometry::from_mesh(&m.wheels);
        let turret = (!m.turret.is_empty()).then(|| {
            let geometry = VehicleGeometry::from_mesh(&m.turret);
            (meshes.add(m.turret.into_mesh()), geometry)
        });
        art.models.insert(
            kind,
            (
                (meshes.add(m.body.into_mesh()), body_geometry),
                (meshes.add(m.wheels.into_mesh()), wheel_geometry),
                turret,
            ),
        );
    }
    // The original docking posts stay on the pavement when a bike is freed.
    // They are created from the deterministic initial fleet, not parented to a
    // moving bicycle or recreated from its saved current position.
    let dock = meshes.add(docking_post().into_mesh());
    for v in fleet.vehicles.iter().filter(|v| v.docked) {
        let p = v.position() + Vec2::new(v.heading.sin(), v.heading.cos()) * 0.7;
        commands.spawn((
            BikeDock,
            Mesh3d(dock.clone()),
            MeshMaterial3d(art.material.clone()),
            Transform::from_xyz(p.x, 0.45, p.y),
            Visibility::Hidden,
        ));
    }
}

pub(crate) fn sync_visuals(
    mut commands: Commands,
    fleet: Res<VehicleFleet>,
    art: Res<VehicleArt>,
    mut roots: Query<
        (Entity, &VehicleVisual, &mut Transform, &mut Visibility),
        Without<VehicleTurret>,
    >,
    mut turrets: Query<(&VehicleTurret, &mut Transform), Without<VehicleVisual>>,
    mut docks: Query<
        (&Transform, &mut Visibility),
        (
            With<BikeDock>,
            Without<VehicleVisual>,
            Without<VehicleTurret>,
        ),
    >,
    player: Query<
        &Transform,
        (
            With<crate::player::Player>,
            Without<VehicleVisual>,
            Without<VehicleTurret>,
        ),
    >,
    flow: Res<State<crate::game_flow::GameFlow>>,
) {
    let position = player.single().map_or(Vec2::ZERO, |p| p.translation.xz());
    let live = matches!(
        flow.get(),
        crate::game_flow::GameFlow::Playing | crate::game_flow::GameFlow::Prologue
    );
    let mut found = HashSet::new();
    for (entity, visual, mut transform, mut visibility) in &mut roots {
        let Some(v) = fleet.get(visual.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        if visual.1 != v.kind {
            commands.entity(entity).despawn();
            continue;
        }
        found.insert(v.id);
        transform.translation = Vec3::new(v.pos[0], 0.0, v.pos[1]);
        transform.rotation = Quat::from_rotation_y(v.heading)
            * Quat::from_rotation_z(
                if v.spec().two && !v.spec().pedal && fleet.current != Some(v.id) {
                    0.12
                } else {
                    0.0
                },
            );
        *visibility = if live && v.position().distance(position) < 220.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for v in &fleet.vehicles {
        if found.contains(&v.id) {
            continue;
        }
        let Some((body, wheels, turret)) = art.models.get(&v.kind) else {
            continue;
        };
        commands
            .spawn((
                VehicleVisual(v.id, v.kind),
                Mesh3d(body.0.clone()),
                body.1,
                MeshMaterial3d(art.material.clone()),
                Transform::from_xyz(v.pos[0], 0.0, v.pos[1])
                    .with_rotation(Quat::from_rotation_y(v.heading)),
                Visibility::Hidden,
            ))
            .with_children(|p| {
                p.spawn((
                    Mesh3d(wheels.0.clone()),
                    wheels.1,
                    MeshMaterial3d(art.material.clone()),
                    Transform::default(),
                ));
                if let Some(turret) = turret {
                    p.spawn((
                        VehicleTurret(v.id),
                        Mesh3d(turret.0.clone()),
                        turret.1,
                        MeshMaterial3d(art.material.clone()),
                        Transform::default(),
                    ));
                }
            });
    }
    for (turret, mut transform) in &mut turrets {
        if let Some(v) = fleet.get(turret.0) {
            transform.rotation = Quat::from_rotation_y(v.turret_yaw);
        }
    }
    for (transform, mut visibility) in &mut docks {
        *visibility = if live && transform.translation.xz().distance(position) < 220.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}
