//! Spatial chunking of big static meshes. Port of `src/core/chunk.ts`.
//!
//! Boot-time builders merge a large area into one mesh. That mesh is drawn,
//! including the shadow pass, whenever any corner is on screen. This splits it
//! by triangle centroid into [`ChunkOptions::cell`]-metre pieces (default 60 m,
//! on x/z) so frustum culling can drop the off-screen pieces.
//!
//! Pieces keep the source mesh's local vertex positions. The world matrix is
//! used only to choose the cell. The caller parents the pieces under the
//! original entity so they inherit its transform, and stops drawing the
//! original (drop its `Mesh3d`). This function does not touch the scene.

use std::collections::BTreeMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, MeshVertexAttribute, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

/// `chunkify` options. Defaults are the web defaults.
#[derive(Clone, Copy, Debug)]
pub struct ChunkOptions {
    /// `minTris`. Meshes with fewer triangles stay whole.
    pub min_tris: u32,
    /// `minSpan`. World-axis span (max of x and z) below this stays whole.
    pub min_span: f32,
    /// `cell`, metres.
    pub cell: f32,
    /// `instanced`. A scene walk can skip instance clouds when this is false.
    /// [`chunkify_instances`] still runs when called.
    pub instanced: bool,
    /// `icell`. Instance grid, metres. Defaults to the same 60 m as `cell`.
    pub icell: f32,
    /// `iminTris`. `count * tris_per_instance` must reach this, and `count > 8`.
    pub imin_tris: u32,
}

impl Default for ChunkOptions {
    fn default() -> Self {
        Self {
            min_tris: 12_000,
            min_span: 100.0,
            cell: 60.0,
            instanced: false,
            icell: 60.0,
            imin_tris: 12_000,
        }
    }
}

/// `ChunkStats`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkStats {
    pub meshes: u32,
    pub pieces: u32,
    pub tris: u32,
}

/// Result of [`chunkify_mesh`]. `Keep` means the mesh was too small, too narrow,
/// not a triangle list, or already sat in one cell.
#[derive(Debug)]
pub enum ChunkedMesh {
    Keep,
    Split {
        pieces: Vec<Mesh>,
        stats: ChunkStats,
    },
}

/// Split `mesh` into cell pieces. `world` is the mesh's world matrix
/// (`matrixWorld`).
pub fn chunkify_mesh(mesh: &Mesh, world: Mat4, opts: &ChunkOptions) -> ChunkedMesh {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList || opts.cell <= 0.0 {
        return ChunkedMesh::Keep;
    }
    let Some(expanded) = expand_triangles(mesh) else {
        return ChunkedMesh::Keep;
    };
    let tris = expanded.tri_count;
    if tris < opts.min_tris {
        return ChunkedMesh::Keep;
    }
    let Some(positions) = expanded.positions() else {
        return ChunkedMesh::Keep;
    };
    if world_span_xz(positions, world) < opts.min_span {
        return ChunkedMesh::Keep;
    }

    let mut cells: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
    for t in 0..(positions.len() / 3) {
        let mut centroid = Vec3::ZERO;
        for k in 0..3 {
            let p = positions[t * 3 + k];
            centroid += world.transform_point3(Vec3::from(p));
        }
        centroid /= 3.0;
        let key = (
            (centroid.x / opts.cell).floor() as i32,
            (centroid.z / opts.cell).floor() as i32,
        );
        cells.entry(key).or_default().push(t as u32);
    }
    if cells.len() < 2 {
        return ChunkedMesh::Keep;
    }

    let mut pieces = Vec::with_capacity(cells.len());
    for list in cells.values() {
        let mut indices = Vec::with_capacity(list.len() * 3);
        for &t in list {
            let base = t * 3;
            indices.extend_from_slice(&[base, base + 1, base + 2]);
        }
        pieces.push(mesh_from_vertices(&expanded.attributes, &indices));
    }
    ChunkedMesh::Split {
        pieces,
        stats: ChunkStats {
            meshes: 1,
            pieces: cells.len() as u32,
            tris,
        },
    }
}

/// One static instanced cloud. `matrices` are the local instance matrices.
/// Dynamic clouds (debris, casings) must not be passed in.
#[derive(Clone, Debug)]
pub struct InstanceCloud {
    pub matrices: Vec<Mat4>,
    pub colors: Option<Vec<[f32; 3]>>,
    pub tris_per_instance: u32,
}

/// One cell of an instanced cloud. Matrices stay in local space.
#[derive(Clone, Debug)]
pub struct InstancePiece {
    pub cell_x: i32,
    pub cell_z: i32,
    pub matrices: Vec<Mat4>,
    pub colors: Option<Vec<[f32; 3]>>,
}

/// Instanced path of `chunkify`. `None` means "leave the cloud alone".
pub fn chunkify_instances(
    cloud: &InstanceCloud,
    world: Mat4,
    opts: &ChunkOptions,
) -> Option<(Vec<InstancePiece>, ChunkStats)> {
    if opts.icell <= 0.0 {
        return None;
    }
    let count = cloud.matrices.len();
    if count <= 8 {
        return None;
    }
    let total = count as u64 * u64::from(cloud.tris_per_instance);
    if total < u64::from(opts.imin_tris) {
        return None;
    }
    let mut cells: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
    for (i, mat) in cloud.matrices.iter().enumerate() {
        let p = world.transform_point3(mat.transform_point3(Vec3::ZERO));
        let key = (
            (p.x / opts.icell).floor() as i32,
            (p.z / opts.icell).floor() as i32,
        );
        cells.entry(key).or_default().push(i);
    }
    if cells.len() < 2 {
        return None;
    }
    let pieces = cells
        .into_iter()
        .map(|((cell_x, cell_z), list)| {
            let colors = cloud.colors.as_ref().map(|colors| {
                list.iter()
                    .map(|&i| colors.get(i).copied().unwrap_or([1.0, 1.0, 1.0]))
                    .collect()
            });
            InstancePiece {
                cell_x,
                cell_z,
                matrices: list.iter().map(|&i| cloud.matrices[i]).collect(),
                colors,
            }
        })
        .collect::<Vec<_>>();
    let piece_count = pieces.len() as u32;
    Some((
        pieces,
        ChunkStats {
            meshes: 1,
            pieces: piece_count,
            tris: total as u32,
        },
    ))
}

struct Expanded {
    attributes: Vec<(MeshVertexAttribute, VertexAttributeValues)>,
    tri_count: u32,
}

impl Expanded {
    fn positions(&self) -> Option<&[[f32; 3]]> {
        self.attributes.iter().find_map(|(attr, values)| {
            if attr.id == Mesh::ATTRIBUTE_POSITION.id {
                values.as_float3()
            } else {
                None
            }
        })
    }
}

fn expand_triangles(mesh: &Mesh) -> Option<Expanded> {
    let index_list: Vec<u32> = match mesh.indices() {
        Some(Indices::U32(v)) => v.clone(),
        Some(Indices::U16(v)) => v.iter().map(|&i| u32::from(i)).collect(),
        None => (0..mesh.count_vertices() as u32).collect(),
    };
    if index_list.len() < 3 || !index_list.len().is_multiple_of(3) {
        return None;
    }
    let mut attributes = Vec::new();
    for (attr, values) in mesh.attributes() {
        attributes.push((*attr, take_vertices(values, &index_list)?));
    }
    if attributes.is_empty() {
        return None;
    }
    Some(Expanded {
        tri_count: (index_list.len() / 3) as u32,
        attributes,
    })
}

fn mesh_from_vertices(
    attributes: &[(MeshVertexAttribute, VertexAttributeValues)],
    indices: &[u32],
) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    for (attr, values) in attributes {
        if let Some(copied) = take_vertices(values, indices) {
            mesh.insert_attribute(*attr, copied);
        }
    }
    mesh
}

fn world_span_xz(positions: &[[f32; 3]], world: Mat4) -> f32 {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for p in positions {
        let w = world.transform_point3(Vec3::from(*p));
        min = min.min(w);
        max = max.max(w);
    }
    (max.x - min.x).max(max.z - min.z)
}

fn take_vertices(src: &VertexAttributeValues, indices: &[u32]) -> Option<VertexAttributeValues> {
    if indices.iter().any(|&i| i as usize >= src.len()) {
        return None;
    }
    macro_rules! arm {
        ($($v:ident),+ $(,)?) => {
            match src {
                $(
                    VertexAttributeValues::$v(data) => {
                        let mut out = Vec::with_capacity(indices.len());
                        for &i in indices {
                            out.push(data[i as usize].clone());
                        }
                        Some(VertexAttributeValues::$v(out))
                    }
                )+
            }
        };
    }
    arm!(
        Uint8,
        Uint8x2,
        Uint8x4,
        Sint8,
        Sint8x2,
        Sint8x4,
        Unorm8,
        Unorm8x2,
        Unorm8x4,
        Snorm8,
        Snorm8x2,
        Snorm8x4,
        Uint16,
        Uint16x2,
        Uint16x4,
        Sint16,
        Sint16x2,
        Sint16x4,
        Unorm16,
        Unorm16x2,
        Unorm16x4,
        Snorm16,
        Snorm16x2,
        Snorm16x4,
        Float16,
        Float16x2,
        Float16x4,
        Float32,
        Float32x2,
        Float32x3,
        Float32x4,
        Uint32,
        Uint32x2,
        Uint32x3,
        Uint32x4,
        Sint32,
        Sint32x2,
        Sint32x3,
        Sint32x4,
        Float64,
        Float64x2,
        Float64x3,
        Float64x4,
        Unorm10_10_10_2,
        Unorm8x4Bgra,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_mesh(quads_x: u32, quads_z: u32, size: f32) -> Mesh {
        let mut pos = Vec::new();
        let mut indices = Vec::new();
        for z in 0..quads_z {
            for x in 0..quads_x {
                let x0 = size * x as f32 / quads_x as f32;
                let x1 = size * (x + 1) as f32 / quads_x as f32;
                let z0 = size * z as f32 / quads_z as f32;
                let z1 = size * (z + 1) as f32 / quads_z as f32;
                let base = pos.len() as u32;
                pos.extend([[x0, 0.0, z0], [x1, 0.0, z0], [x1, 0.0, z1], [x0, 0.0, z1]]);
                indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
            }
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_indices(Indices::U32(indices));
        mesh
    }

    fn loose() -> ChunkOptions {
        ChunkOptions {
            min_tris: 2,
            min_span: 1.0,
            ..ChunkOptions::default()
        }
    }

    #[test]
    fn defaults_match_the_web_chunker() {
        let opts = ChunkOptions::default();
        assert_eq!(
            (
                opts.min_tris,
                opts.min_span,
                opts.cell,
                opts.icell,
                opts.imin_tris
            ),
            (12_000, 100.0, 60.0, 60.0, 12_000)
        );
    }

    #[test]
    fn small_or_single_cell_mesh_is_kept() {
        let mesh = grid_mesh(1, 1, 10.0);
        assert!(matches!(
            chunkify_mesh(&mesh, Mat4::IDENTITY, &ChunkOptions::default()),
            ChunkedMesh::Keep
        ));
        let wide_but_one_cell = grid_mesh(2, 2, 50.0);
        assert!(matches!(
            chunkify_mesh(&wide_but_one_cell, Mat4::IDENTITY, &loose()),
            ChunkedMesh::Keep
        ));
    }

    #[test]
    fn triangles_go_to_the_cell_of_their_centroid() {
        let mesh = grid_mesh(4, 1, 240.0);
        let ChunkedMesh::Split { pieces, stats } = chunkify_mesh(&mesh, Mat4::IDENTITY, &loose())
        else {
            panic!("expected a split");
        };
        assert!(stats.pieces >= 2);
        assert_eq!(pieces.len(), stats.pieces as usize);
        assert_eq!(stats.tris, 8);
        let mut seen_x = Vec::new();
        for piece in &pieces {
            let pos = piece
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap();
            let centroid =
                pos.iter().fold(Vec3::ZERO, |acc, p| acc + Vec3::from(*p)) / pos.len() as f32;
            seen_x.push((centroid.x / 60.0).floor() as i32);
        }
        seen_x.sort_unstable();
        seen_x.dedup();
        assert!(seen_x.len() >= 2, "cells: {seen_x:?}");
        assert!(seen_x.contains(&0));
    }

    #[test]
    fn instances_split_by_world_position() {
        let matrices = (0..10)
            .map(|i| Mat4::from_translation(Vec3::new(i as f32 * 30.0, 0.0, 0.0)))
            .collect();
        let cloud = InstanceCloud {
            matrices,
            colors: Some(vec![[1.0, 0.0, 0.0]; 10]),
            tris_per_instance: 100,
        };
        let opts = ChunkOptions {
            imin_tris: 10,
            icell: 60.0,
            ..ChunkOptions::default()
        };
        let (pieces, stats) = chunkify_instances(&cloud, Mat4::IDENTITY, &opts).unwrap();
        assert!(pieces.len() >= 2);
        assert_eq!(stats.meshes, 1);
        let total: usize = pieces.iter().map(|p| p.matrices.len()).sum();
        assert_eq!(total, 10);
        assert_eq!(
            pieces[0].colors.as_ref().unwrap().len(),
            pieces[0].matrices.len()
        );
    }

    #[test]
    fn clouds_at_the_count_cutoff_stay_whole() {
        let cloud = InstanceCloud {
            matrices: vec![Mat4::IDENTITY; 8],
            colors: None,
            tris_per_instance: 10_000,
        };
        assert!(chunkify_instances(&cloud, Mat4::IDENTITY, &ChunkOptions::default()).is_none());
    }
}
