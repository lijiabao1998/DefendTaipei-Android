//! Mid and far mesh builders. Port of `src/map/tiles/mid.ts`.
//!
//! These turn data the map module already has into a mesh. They do not spawn
//! entities and they do not invent buildings. Mid is one merged mesh of
//! extruded footprint boxes. Far is a coarse terrain grid; heights come from
//! the sampler the caller passes (flat when the sampler returns 0). Skyline
//! blocks stay with baked tile data, which this port does not include.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;

/// One building footprint. `pts` is the outline on x/z; the box is the
/// axis-aligned bounds, extruded to `height`.
#[derive(Clone, Debug)]
pub struct Footprint {
    pub pts: Vec<[f32; 2]>,
    pub height: f32,
}

/// `midBuildings`. Empty outlines are skipped. Winding matches the web quads
/// (`a,b,c` + `a,c,d`).
pub fn mid_buildings(buildings: &[Footprint]) -> Mesh {
    let mut pos = Vec::new();
    for building in buildings {
        if building.pts.is_empty() {
            continue;
        }
        let mut x0 = f32::INFINITY;
        let mut z0 = f32::INFINITY;
        let mut x1 = f32::NEG_INFINITY;
        let mut z1 = f32::NEG_INFINITY;
        for [x, z] in &building.pts {
            x0 = x0.min(*x);
            x1 = x1.max(*x);
            z0 = z0.min(*z);
            z1 = z1.max(*z);
        }
        if !x0.is_finite() {
            continue;
        }
        let h = building.height;
        quad(&mut pos, [x0, h, z0], [x0, h, z1], [x1, h, z1], [x1, h, z0]);
        quad(
            &mut pos,
            [x0, 0.0, z1],
            [x1, 0.0, z1],
            [x1, h, z1],
            [x0, h, z1],
        );
        quad(
            &mut pos,
            [x1, 0.0, z0],
            [x0, 0.0, z0],
            [x0, h, z0],
            [x1, h, z0],
        );
        quad(
            &mut pos,
            [x1, 0.0, z1],
            [x1, 0.0, z0],
            [x1, h, z0],
            [x1, h, z1],
        );
        quad(
            &mut pos,
            [x0, 0.0, z0],
            [x0, 0.0, z1],
            [x0, h, z1],
            [x0, h, z0],
        );
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    if mesh.count_vertices() > 0 {
        mesh.compute_normals();
    }
    mesh
}

/// `farTerrain` with the web default: a flat 4×4 segment grid.
pub fn far_terrain(x0: f32, z0: f32, size: f32) -> Mesh {
    far_terrain_sampled(x0, z0, size, 4, |_, _| 0.0)
}

/// `farTerrain`. `(n + 1)²` vertices over `[x0, x0 + size]` × `[z0, z0 + size]`.
pub fn far_terrain_sampled(
    x0: f32,
    z0: f32,
    size: f32,
    n: u32,
    h_at: impl Fn(f32, f32) -> f32,
) -> Mesh {
    let n = n.max(1);
    let mut pos = Vec::with_capacity((n as usize + 1) * (n as usize + 1));
    for iz in 0..=n {
        for ix in 0..=n {
            let x = x0 + size * ix as f32 / n as f32;
            let z = z0 + size * iz as f32 / n as f32;
            pos.push([x, h_at(x, z), z]);
        }
    }
    let stride = n + 1;
    let mut indices = Vec::with_capacity((n * n * 6) as usize);
    for iz in 0..n {
        for ix in 0..n {
            let a = iz * stride + ix;
            let b = a + 1;
            let c = a + stride;
            let d = c + 1;
            // a -- b
            // |    |
            // c -- d
            indices.extend([a, b, d, a, d, c]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_normals();
    mesh
}

fn quad(pos: &mut Vec<[f32; 3]>, a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) {
    pos.extend([a, b, c, a, c, d]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mid_extrudes_one_box_per_footprint() {
        let mesh = mid_buildings(&[Footprint {
            pts: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 4.0]],
            height: 8.0,
        }]);
        assert_eq!(mesh.count_vertices(), 30);
        let pos = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert!(pos.iter().any(|p| (p[1] - 8.0).abs() < 1e-4));
        assert!(pos.iter().all(|p| (0.0..=10.0).contains(&p[0])));
        let normals = mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .unwrap()
            .as_float3()
            .unwrap();
        let up = normals.iter().filter(|n| n[1] > 0.5).count();
        assert!(up >= 6, "roof should face up, up verts = {up}");
    }

    #[test]
    fn far_grid_is_a_tile_sized_heightfield() {
        let mesh = far_terrain_sampled(256.0, -256.0, 256.0, 4, |x, _| x * 0.01);
        assert_eq!(mesh.count_vertices(), 25);
        let pos = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert!(
            pos.iter()
                .any(|p| (p[0] - 256.0).abs() < 1e-3 && (p[2] + 256.0).abs() < 1e-3)
        );
        let lifted = pos.iter().find(|p| (p[0] - 512.0).abs() < 1e-3).unwrap();
        assert!((lifted[1] - 5.12).abs() < 1e-3);
    }
}
