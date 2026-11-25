//! Cached survivor portraits made from the same faceless rig as the game world.
//! A small CPU rasterizer avoids six extra cameras, render passes, or animated rigs
//! while the menu is open. Twelve 160 × 208 textures cover every role and gender.

use bevy::prelude::*;

use crate::geom::MeshBuf;
use crate::human::rig::{J, JOINTS, RigOpts, make_rig};
use crate::map::canvas::Canvas;
use crate::roles::{Gender, Role};

const WIDTH: usize = 160;
const HEIGHT: usize = 208;

pub(super) fn render(role: &Role, gender: Gender) -> Image {
    pixels(role, gender).to_image(true, false)
}

fn pixels(role: &Role, gender: Gender) -> Canvas {
    let female = gender == Gender::Female;
    let look = if female { role.look_f } else { role.look_m };
    let rig = make_rig(&RigOpts {
        female,
        top: look.top,
        bottom: look.bottom,
        shoes: look.shoes,
        soldier: false,
        jacket: look.jacket,
        pack: None,
        outfit: role.id,
        gun: "",
    });
    let mut canvas = Canvas::new(WIDTH, HEIGHT);
    canvas.px.fill([0.0; 4]);
    // Contact shadow remains part of the cached portrait, not a world light.
    for y in 185..202 {
        for x in 38..126 {
            let d = ((x as f32 - 82.0) / 44.0).powi(2) + ((y as f32 - 194.0) / 8.0).powi(2);
            if d < 1.0 {
                canvas.px[y * WIDTH + x] = [0.015, 0.014, 0.012, (1.0 - d) * 0.55];
            }
        }
    }
    let mut depth = vec![f32::INFINITY; WIDTH * HEIGHT];
    let turn = Mat4::from_rotation_y(-0.35);
    let mut joints = [Mat4::IDENTITY; JOINTS];
    for i in 0..JOINTS {
        let pose = match i {
            n if n == J::UaL as usize => Quat::from_rotation_z(0.08),
            n if n == J::UaR as usize => Quat::from_rotation_z(-0.08),
            _ => rig.rest_rot[i],
        };
        let local = Mat4::from_rotation_translation(pose, rig.offset[i]);
        joints[i] = rig.parent[i].map_or(turn, |parent| joints[parent as usize]) * local;
    }
    let view = Transform::from_translation(Vec3::new(0.0, 1.12, 4.1))
        .looking_at(Vec3::new(0.0, 0.96, 0.0), Vec3::Y)
        .to_matrix()
        .inverse();
    for (joint, mesh) in rig.skin.iter().chain(&rig.cloth) {
        raster(&mut canvas, &mut depth, mesh, joints[*joint as usize], view);
    }
    canvas
}

fn raster(canvas: &mut Canvas, depth: &mut [f32], mesh: &MeshBuf, model: Mat4, view: Mat4) {
    let light = Vec3::new(-0.5, 0.8, 0.65).normalize();
    let rim = Vec3::new(0.7, 0.3, -0.8).normalize();
    let world_normal = Mat3::from_mat4(model).inverse().transpose();
    let projected: Vec<Vec3> = mesh
        .pos
        .iter()
        .map(|p| {
            let p = view.transform_point3(model.transform_point3(Vec3::from(*p)));
            Vec3::new(80.0 + p.x * 94.0, 104.0 - p.y * 94.0, -p.z)
        })
        .collect();
    let lit: Vec<Vec3> = mesh
        .nrm
        .iter()
        .zip(&mesh.col)
        .map(|(normal, color)| {
            let n = (world_normal * Vec3::from(*normal)).normalize_or_zero();
            let diffuse = 0.5 + 0.65 * n.dot(light).max(0.0);
            let edge = 0.14 * n.dot(rim).max(0.0);
            Vec3::new(color[0], color[1], color[2]) * diffuse
                + Vec3::new(edge * 0.68, edge * 0.78, edge)
        })
        .collect();
    for triangle in mesh.idx.as_chunks::<3>().0 {
        let ids = [
            triangle[0] as usize,
            triangle[1] as usize,
            triangle[2] as usize,
        ];
        let [a, b, c] = ids.map(|index| projected[index]);
        let area = edge(a.truncate(), b.truncate(), c.truncate());
        if area.abs() < 0.0001 {
            continue;
        }
        let min = a.min(b).min(c).truncate().floor().max(Vec2::ZERO);
        let max = a
            .max(b)
            .max(c)
            .truncate()
            .ceil()
            .min(Vec2::new(WIDTH as f32 - 1.0, HEIGHT as f32 - 1.0));
        for y in min.y as usize..=max.y as usize {
            for x in min.x as usize..=max.x as usize {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let u = edge(b.truncate(), c.truncate(), p) / area;
                let v = edge(c.truncate(), a.truncate(), p) / area;
                let w = 1.0 - u - v;
                if u < -0.001 || v < -0.001 || w < -0.001 {
                    continue;
                }
                let z = a.z * u + b.z * v + c.z * w;
                let index = y * WIDTH + x;
                if z >= depth[index] {
                    continue;
                }
                depth[index] = z;
                let color = lit[ids[0]] * u + lit[ids[1]] * v + lit[ids[2]] * w;
                let srgb = |value: f32| {
                    let value = value.clamp(0.0, 1.0);
                    if value <= 0.0031308 {
                        value * 12.92
                    } else {
                        1.055 * value.powf(1.0 / 2.4) - 0.055
                    }
                };
                canvas.px[index] = [srgb(color.x), srgb(color.y), srgb(color.z), 1.0];
            }
        }
    }
}

fn edge(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    (p.x - a.x) * (b.y - a.y) - (p.y - a.y) * (b.x - a.x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::ROLES;

    #[test]
    fn portraits_have_transparent_margins_and_distinct_outfits() {
        let office = pixels(&ROLES[0], Gender::Male);
        let nurse = pixels(&ROLES[3], Gender::Female);
        assert_eq!(office.px[0][3], 0.0);
        let body: Vec<_> = office.px.iter().filter(|p| p[3] == 1.0).collect();
        assert!(body.len() > 2500 && body.len() < WIDTH * HEIGHT / 2);
        assert!(body.iter().all(|p| p.iter().all(|v| v.is_finite())));
        assert_ne!(office.bytes(), nurse.bytes());
        assert_eq!(office.bytes(), pixels(&ROLES[0], Gender::Male).bytes());
    }
}
