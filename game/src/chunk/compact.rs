//! Vertex packing. Port of `src/core/compact.ts`.
//!
//! The web game's memory is mostly vertex data: a uv channel on a mesh whose
//! material has no texture, 32-bit normals on rough surfaces, and 32-bit colours
//! on flat paint. `compact` runs once a mesh has been built, before it is drawn.
//!
//! - drop uv unless a texture needs it
//! - normals as four signed bytes (xyz + padding). Three components is not a
//!   vertex format the hardware has; a Windows driver would convert them
//! - colours as four 16-bit channels
//! - meshes under [`MIN_VERTS`] vertices are left alone
//!
//! # Bevy materials
//!
//! [`bevy::pbr::StandardMaterial`] reads `Float32x3` normals and `Float32x4`
//! colours. Packed `Snorm8x4` / `Unorm16x4` attributes keep the original byte
//! layout and the same attribute id, so a material whose shader declares those
//! formats can bind them. A stock material will not. Uv removal is safe for an
//! untextured stock material. Leave [`CompactOptions::enabled`] off for meshes
//! that must stay on `StandardMaterial`.
//!
//! Bevy's colour attribute is four floats, not three. The alpha channel is
//! packed along with rgb. The web code only saw three and wrote an opaque alpha.
//!
//! # CPU release
//!
//! The web `RELEASE` path drops the page's copy of a district tile once the GPU
//! has it. Bevy's equivalent is [`RenderAssetUsages::RENDER_WORLD`]: after
//! extract, `Assets<Mesh>::get` no longer returns the mesh and the GPU copy
//! remains. There is no upload callback. [`discard_mesh`] frees big CPU arrays
//! immediately and must run while the mesh is still in the main world.

use std::collections::HashSet;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, VertexAttributeValues, VertexFormat};
use bevy::prelude::*;
use bevy::shape::Aabb3d;

/// Original `MIN_VERTS`. The memory is in the big meshes.
pub const MIN_VERTS: usize = 200;

/// Original `SMALL`. Arrays under this size are left for the allocator.
pub const SMALL_BYTES: usize = 16_384;

/// `COMPACT` counters. `done` is `geometry.userData.compact`.
#[derive(Default, Debug, Resource)]
pub struct CompactStats {
    pub meshes: u64,
    pub verts: u64,
    /// Bytes no longer held.
    pub saved: u64,
    pub ms: f32,
    done: HashSet<u64>,
}

/// `RELEASE` counters.
#[derive(Debug, Resource)]
pub struct ReleaseStats {
    pub on: bool,
    pub held: u64,
    pub held_bytes: u64,
    pub attrs: u64,
    pub bytes: u64,
    pub freed: u64,
    pub freed_bytes: u64,
    pub ms: f32,
}

impl Default for ReleaseStats {
    fn default() -> Self {
        Self {
            on: true,
            held: 0,
            held_bytes: 0,
            attrs: 0,
            bytes: 0,
            freed: 0,
            freed_bytes: 0,
            ms: 0.0,
        }
    }
}

/// Material facts the web code reads off the Three.js material.
#[derive(Clone, Copy, Debug, Default)]
pub struct CompactMaterial {
    /// Any of map / alphaMap / aoMap / bumpMap / normalMap / roughnessMap /
    /// metalnessMap / emissiveMap / lightMap / specularMap / displacementMap.
    pub uses_uv: bool,
    pub vertex_colors: bool,
    /// `isShaderMaterial`. Left unpacked.
    pub shader_material: bool,
}

/// Per-mesh flags that make `compact` skip, plus the release switch.
#[derive(Clone, Copy, Debug)]
pub struct CompactOptions {
    /// `COMPACT.on`. `?nocompact=1` turns it off.
    pub enabled: bool,
    /// `userData.noCompact`.
    pub no_compact: bool,
    /// Geometry used by more than one mesh, or `geometry.shared`.
    pub shared: bool,
    /// Instanced or skinned meshes are not packed.
    pub instanced_or_skinned: bool,
    /// Morph targets. Bevy has no Three.js morph map; the caller sets this.
    pub morph: bool,
    /// Drop the CPU copy after GPU upload. Only for content that can be built
    /// again (district tiles). Original `release` argument.
    pub release: bool,
}

impl Default for CompactOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            no_compact: false,
            shared: false,
            instanced_or_skinned: false,
            morph: false,
            release: false,
        }
    }
}

/// Why a mesh was left unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactSkip {
    Disabled,
    Already,
    Flagged,
    Shared,
    InstancedOrSkinned,
    Morph,
    ShaderMaterial,
    TooSmall,
    NotStaticFloat,
}

/// Result of one [`compact_mesh`] call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactOutcome {
    Packed { saved: u64 },
    Skipped(CompactSkip),
}

/// `compact` for a single mesh. `cookie` stands in for `geometry.userData.compact`
/// (an asset id, or any stable non-zero id). `0` skips the idempotency set.
pub fn compact_mesh(
    mesh: &mut Mesh,
    material: &CompactMaterial,
    opts: &CompactOptions,
    stats: &mut CompactStats,
    release: &mut ReleaseStats,
    cookie: u64,
) -> CompactOutcome {
    if !opts.enabled {
        return CompactOutcome::Skipped(CompactSkip::Disabled);
    }
    if cookie != 0 && stats.done.contains(&cookie) {
        return CompactOutcome::Skipped(CompactSkip::Already);
    }
    if opts.no_compact {
        return CompactOutcome::Skipped(CompactSkip::Flagged);
    }
    if opts.shared {
        return CompactOutcome::Skipped(CompactSkip::Shared);
    }
    if opts.instanced_or_skinned {
        return CompactOutcome::Skipped(CompactSkip::InstancedOrSkinned);
    }
    if opts.morph {
        return CompactOutcome::Skipped(CompactSkip::Morph);
    }
    if material.shader_material {
        return CompactOutcome::Skipped(CompactSkip::ShaderMaterial);
    }
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return CompactOutcome::Skipped(CompactSkip::NotStaticFloat);
    };
    if pos.len() < MIN_VERTS {
        return CompactOutcome::Skipped(CompactSkip::TooSmall);
    }
    let verts = pos.len() as u64;
    let started = std::time::Instant::now();
    let mut saved = 0u64;

    if !material.uses_uv
        && let Some(uv) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
    {
        saved += uv.get_bytes().len() as u64;
        mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0);
    }

    if let Some(VertexAttributeValues::Float32x3(nor)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        let packed = pack_normals(nor);
        saved += (nor.len() * 12 - packed.len() * 4) as u64;
        let mut attr = Mesh::ATTRIBUTE_NORMAL;
        attr.format = VertexFormat::Snorm8x4;
        mesh.insert_attribute(attr, VertexAttributeValues::Snorm8x4(packed));
    }

    match mesh.attribute(Mesh::ATTRIBUTE_COLOR).cloned() {
        Some(VertexAttributeValues::Float32x4(col)) if !material.vertex_colors => {
            saved += (col.len() * 16) as u64;
            mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR);
        }
        Some(VertexAttributeValues::Float32x4(col)) => {
            let packed = pack_colors(&col);
            saved += (col.len() * 16 - packed.len() * 8) as u64;
            let mut attr = Mesh::ATTRIBUTE_COLOR;
            attr.format = VertexFormat::Unorm16x4;
            mesh.insert_attribute(attr, VertexAttributeValues::Unorm16x4(packed));
        }
        _ => {}
    }

    if opts.release && release.on {
        let t0 = std::time::Instant::now();
        if mesh.final_aabb.is_none() {
            mesh.final_aabb = bounds_of(mesh);
        }
        release.held += 1;
        release.held_bytes += held_bytes(mesh);
        mesh.asset_usage = RenderAssetUsages::RENDER_WORLD;
        release.ms += t0.elapsed().as_secs_f32() * 1000.0;
    }

    if cookie != 0 {
        stats.done.insert(cookie);
    }
    stats.meshes += 1;
    stats.verts += verts;
    stats.saved += saved;
    stats.ms += started.elapsed().as_secs_f32() * 1000.0;
    CompactOutcome::Packed { saved }
}

/// `discard` / `freeNow`. Big arrays of a mesh that will not be drawn go back
/// now. Arrays under [`SMALL_BYTES`] are left alone.
pub fn discard_mesh(mesh: &mut Mesh, release: &mut ReleaseStats) {
    if !release.on {
        return;
    }
    let ids: Vec<_> = mesh
        .attributes()
        .filter_map(|(attr, values)| {
            let n = values.get_bytes().len();
            (n >= SMALL_BYTES).then_some((attr.id, n))
        })
        .collect();
    for (id, n) in ids {
        mesh.remove_attribute(id);
        release.freed += 1;
        release.freed_bytes += n as u64;
    }
    let index_bytes = match mesh.indices() {
        Some(Indices::U16(v)) => v.len() * 2,
        Some(Indices::U32(v)) => v.len() * 4,
        None => 0,
    };
    if index_bytes >= SMALL_BYTES {
        mesh.remove_indices();
        release.freed += 1;
        release.freed_bytes += index_bytes as u64;
    }
}

/// `freeNow` for one mesh the caller already decided was compacted and unshared.
pub fn free_now(mesh: &mut Mesh, release: &mut ReleaseStats) {
    discard_mesh(mesh, release);
}

fn pack_normals(src: &[[f32; 3]]) -> Vec<[i8; 4]> {
    src.iter()
        .map(|n| [pack_i8(n[0]), pack_i8(n[1]), pack_i8(n[2]), 0])
        .collect()
}

fn pack_i8(v: f32) -> i8 {
    // `Int8Array` stores `Math.round(v * 127)`. Normals sit in -1..1; clamp so a
    // slightly long vector cannot wrap the way a raw cast would.
    (v * 127.0).round().clamp(-127.0, 127.0) as i8
}

fn pack_colors(src: &[[f32; 4]]) -> Vec<[u16; 4]> {
    src.iter()
        .map(|c| {
            [
                pack_u16(c[0]),
                pack_u16(c[1]),
                pack_u16(c[2]),
                pack_u16(c[3]),
            ]
        })
        .collect()
}

fn pack_u16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn bounds_of(mesh: &Mesh) -> Option<Aabb3d> {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    if pos.is_empty() {
        return None;
    }
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for p in pos {
        let v = Vec3::from(*p);
        min = min.min(v);
        max = max.max(v);
    }
    Some(Aabb3d::from_min_max(min, max))
}

fn held_bytes(mesh: &Mesh) -> u64 {
    let mut n = 0u64;
    for (_, values) in mesh.attributes() {
        n += values.get_bytes().len() as u64;
    }
    if let Some(indices) = mesh.indices() {
        n += match indices {
            Indices::U16(v) => v.len() * 2,
            Indices::U32(v) => v.len() * 4,
        } as u64;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::PrimitiveTopology;

    fn big_mesh(with_uv: bool, with_color: bool) -> Mesh {
        let n = MIN_VERTS;
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0]; n]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n]);
        if with_uv {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
        }
        if with_color {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0, 0.5, 0.0, 1.0]; n]);
        }
        mesh
    }

    fn pack(
        mesh: &mut Mesh,
        material: &CompactMaterial,
        opts: &CompactOptions,
        cookie: u64,
    ) -> (CompactOutcome, CompactStats) {
        let mut stats = CompactStats::default();
        let mut release = ReleaseStats::default();
        let outcome = compact_mesh(mesh, material, opts, &mut stats, &mut release, cookie);
        (outcome, stats)
    }

    #[test]
    fn small_mesh_is_left_alone() {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0]; 10]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 10]);
        let (outcome, stats) = pack(
            &mut mesh,
            &CompactMaterial::default(),
            &CompactOptions::default(),
            1,
        );
        assert_eq!(outcome, CompactOutcome::Skipped(CompactSkip::TooSmall));
        assert_eq!(stats.meshes, 0);
        assert!(matches!(
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
            Some(VertexAttributeValues::Float32x3(_))
        ));
    }

    #[test]
    fn drops_unused_uv_and_packs_normals() {
        let mut mesh = big_mesh(true, false);
        let (outcome, stats) = pack(
            &mut mesh,
            &CompactMaterial::default(),
            &CompactOptions::default(),
            7,
        );
        let CompactOutcome::Packed { saved } = outcome else {
            panic!("expected pack, got {outcome:?}");
        };
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_none());
        match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Snorm8x4(v)) => {
                assert_eq!(v.len(), MIN_VERTS);
                assert_eq!(v[0], [0, 127, 0, 0]);
            }
            other => panic!("normals not packed: {other:?}"),
        }
        // uv: 200 * 8, normals: 200 * 12 - 200 * 4.
        assert_eq!(saved, (MIN_VERTS * 16) as u64);
        assert_eq!(stats.saved, saved);
        assert_eq!(stats.verts, MIN_VERTS as u64);
    }

    #[test]
    fn keeps_uv_when_a_texture_uses_it() {
        let mut mesh = big_mesh(true, false);
        let material = CompactMaterial {
            uses_uv: true,
            ..CompactMaterial::default()
        };
        let _ = pack(&mut mesh, &material, &CompactOptions::default(), 2);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
    }

    #[test]
    fn drops_colors_nobody_reads_and_packs_the_rest() {
        let mut unused = big_mesh(false, true);
        let _ = pack(
            &mut unused,
            &CompactMaterial::default(),
            &CompactOptions::default(),
            3,
        );
        assert!(unused.attribute(Mesh::ATTRIBUTE_COLOR).is_none());

        let mut used = big_mesh(false, true);
        let material = CompactMaterial {
            vertex_colors: true,
            ..CompactMaterial::default()
        };
        let _ = pack(&mut used, &material, &CompactOptions::default(), 4);
        match used.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Unorm16x4(v)) => {
                assert_eq!(v[0][0], u16::MAX);
                assert_eq!(v[0][3], u16::MAX);
                assert!((v[0][1] as i32 - 32768).abs() <= 1);
            }
            other => panic!("colors not packed: {other:?}"),
        }
    }

    #[test]
    fn second_call_is_a_no_op() {
        let mut mesh = big_mesh(true, false);
        let mut stats = CompactStats::default();
        let mut release = ReleaseStats::default();
        let opts = CompactOptions::default();
        let material = CompactMaterial::default();
        assert!(matches!(
            compact_mesh(&mut mesh, &material, &opts, &mut stats, &mut release, 9),
            CompactOutcome::Packed { .. }
        ));
        assert_eq!(
            compact_mesh(&mut mesh, &material, &opts, &mut stats, &mut release, 9),
            CompactOutcome::Skipped(CompactSkip::Already)
        );
        assert_eq!(stats.meshes, 1);
    }

    #[test]
    fn shader_and_shared_meshes_are_skipped() {
        let mut mesh = big_mesh(false, false);
        let shader = CompactMaterial {
            shader_material: true,
            ..CompactMaterial::default()
        };
        let (outcome, _) = pack(&mut mesh, &shader, &CompactOptions::default(), 1);
        assert_eq!(
            outcome,
            CompactOutcome::Skipped(CompactSkip::ShaderMaterial)
        );
        let opts = CompactOptions {
            shared: true,
            ..CompactOptions::default()
        };
        let (outcome, _) = pack(&mut mesh, &CompactMaterial::default(), &opts, 1);
        assert_eq!(outcome, CompactOutcome::Skipped(CompactSkip::Shared));
    }

    #[test]
    fn release_marks_render_world_and_discard_frees_big_arrays() {
        let n = 2000;
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 1.0, 0.0]; n]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n]);
        let opts = CompactOptions {
            release: true,
            ..CompactOptions::default()
        };
        let mut stats = CompactStats::default();
        let mut release = ReleaseStats::default();
        compact_mesh(
            &mut mesh,
            &CompactMaterial::default(),
            &opts,
            &mut stats,
            &mut release,
            11,
        );
        assert_eq!(mesh.asset_usage, RenderAssetUsages::RENDER_WORLD);
        assert!(mesh.final_aabb.is_some());
        assert!(release.held_bytes > 0);

        let mut cpu = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        cpu.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0]; n]);
        let mut release = ReleaseStats::default();
        discard_mesh(&mut cpu, &mut release);
        assert!(cpu.attribute(Mesh::ATTRIBUTE_POSITION).is_none());
        assert!(release.freed >= 1);
        assert!(release.freed_bytes >= SMALL_BYTES as u64);
    }

    #[test]
    fn disabled_switch_packs_nothing() {
        let mut mesh = big_mesh(true, true);
        let opts = CompactOptions {
            enabled: false,
            ..CompactOptions::default()
        };
        let (outcome, stats) = pack(&mut mesh, &CompactMaterial::default(), &opts, 1);
        assert_eq!(outcome, CompactOutcome::Skipped(CompactSkip::Disabled));
        assert_eq!(stats.saved, 0);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
    }
}
