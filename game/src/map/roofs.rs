//! Stable rooftop layouts shared by the chunk silhouette and the close detail layer.
//!
//! Roofs use the existing entrance frame. Every object reserves its complete overhang
//! and service area; the existing tank is a reservation too. This is rendering data
//! only: building IDs, collision, entrances, interiors and planning RNG stay untouched.

use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

use super::build::{Lod, Mat, Parts, WHITE};
use super::data::{Bld, Park, RoofClass, pip, seg_dist};
use crate::geom::{self, MeshBuf, hex, place, trs};

/// Oriented horizontal footprint. Local +Z faces the existing street entrance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoofRect {
    pub c: Vec2,
    pub u: Vec2,
    pub n: Vec2,
    pub width: f32,
    pub depth: f32,
}

impl RoofRect {
    fn point(self, x: f32, z: f32) -> Vec2 {
        self.c + self.u * x + self.n * z
    }

    fn corners(self) -> [Vec2; 4] {
        [
            self.point(-self.width / 2.0, -self.depth / 2.0),
            self.point(self.width / 2.0, -self.depth / 2.0),
            self.point(self.width / 2.0, self.depth / 2.0),
            self.point(-self.width / 2.0, self.depth / 2.0),
        ]
    }

    fn transform(self, y: f32) -> Mat4 {
        place(self.c.x, y, self.c.y, self.n.x.atan2(self.n.y))
    }

    /// Separating-axis overlap, including a 12 cm maintenance gap.
    fn overlaps(self, other: Self) -> bool {
        [self.u, self.n, other.u, other.n].into_iter().all(|axis| {
            let radius = |rect: Self| {
                rect.width * 0.5 * rect.u.dot(axis).abs()
                    + rect.depth * 0.5 * rect.n.dot(axis).abs()
            };
            (self.c - other.c).dot(axis).abs() < radius(self) + radius(other) + 0.12
        })
    }

    /// Corners alone miss concave OSM notches: check the edges and notch vertices too.
    fn fits(self, b: &Bld) -> bool {
        let corners = self.corners();
        if !corners.iter().all(|p| pip(*p, &b.pts)) {
            return false;
        }
        for i in 0..b.pts.len() {
            let a = b.pts[i];
            let z = b.pts[(i + 1) % b.pts.len()];
            for j in 0..4 {
                let p = corners[j];
                let q = corners[(j + 1) % 4];
                if seg_dist(p, a, z) < 0.18 || seg_dist(a, p, q) < 0.18 || crosses(a, z, p, q) {
                    return false;
                }
            }
            let d = a - self.c;
            if d.dot(self.u).abs() < self.width * 0.5 && d.dot(self.n).abs() < self.depth * 0.5 {
                return false;
            }
        }
        true
    }
}

fn crosses(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let cross = |x: Vec2, y: Vec2| x.x * y.y - x.y * y.x;
    let ab = b - a;
    let cd = d - c;
    let den = cross(ab, cd);
    if den.abs() < 1e-6 {
        return false;
    }
    let t = cross(c - a, cd) / den;
    let u = cross(c - a, ab) / den;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShackPlan {
    /// Wall footprint; gutters, AC and front laundry live inside the larger envelope.
    pub area: RoofRect,
    pub envelope: RoofRect,
    pub seed: u64,
    pub open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UtilityKind {
    Solar,
    Compressor,
    Laundry,
    Antenna,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UtilityPlan {
    pub area: RoofRect,
    pub kind: UtilityKind,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoofLayout {
    pub shacks: Vec<ShackPlan>,
    /// Small roof-access stair headhouse where its entire footprint fits.
    pub access: Option<RoofRect>,
    pub utilities: Vec<UtilityPlan>,
}

fn roof_frame(b: &Bld) -> RoofRect {
    let n = b.door.map_or_else(
        || {
            let mut edge = Vec2::X;
            let mut longest = 0.0;
            for i in 0..b.pts.len() {
                let d = b.pts[(i + 1) % b.pts.len()] - b.pts[i];
                if d.length_squared() > longest {
                    longest = d.length_squared();
                    edge = d.normalize_or_zero();
                }
            }
            Vec2::new(-edge.y, edge.x)
        },
        |d| d.n,
    );
    let u = Vec2::new(n.y, -n.x);
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for p in &b.pts {
        let q = Vec2::new(p.dot(u), p.dot(n));
        lo = lo.min(q);
        hi = hi.max(q);
    }
    let centre = (lo + hi) / 2.0;
    RoofRect {
        c: u * centre.x + n * centre.y,
        u,
        n,
        width: hi.x - lo.x,
        depth: hi.y - lo.y,
    }
}

/// Bounded, deterministic search from rear to front. Every LOD uses the same result.
fn reserve(
    b: &Bld,
    frame: RoofRect,
    size: Vec2,
    occupied: &mut Vec<RoofRect>,
    phase: usize,
) -> Option<RoofRect> {
    if size.x + 0.36 > frame.width || size.y + 0.36 > frame.depth {
        return None;
    }
    let xs = [0.0, -0.36, 0.36, -0.18, 0.18, -0.49, 0.49];
    let zs = [-0.48, -0.28, 0.0, 0.28, 0.48];
    for z in zs {
        for i in 0..xs.len() {
            let x = xs[(i + phase) % xs.len()];
            let area = RoofRect {
                c: frame.point(
                    x * (frame.width - size.x - 0.36),
                    z * (frame.depth - size.y - 0.36),
                ),
                width: size.x,
                depth: size.y,
                ..frame
            };
            if area.fits(b) && occupied.iter().all(|other| !area.overlaps(*other)) {
                occupied.push(area);
                return Some(area);
            }
        }
    }
    None
}

/// Immutable rendering metadata. No mutable world-planner calls or RNG consumption.
pub fn layout(b: &Bld) -> RoofLayout {
    let mut result = RoofLayout::default();
    if b.rcls == RoofClass::None || b.pts.len() < 3 {
        return result;
    }
    let frame = roof_frame(b);
    let mut occupied = Vec::with_capacity(8);
    if let Some((c, _)) = b.tank {
        // Existing cylinder radius is 0.6 m; stand legs extend to ±0.485 m.
        // Reserve 0.9 m in both directions so new geometry cannot clip through it.
        occupied.push(RoofRect {
            c,
            width: 1.8,
            depth: 1.8,
            ..frame
        });
    }
    let mut random = Park(b.seed7 ^ 0x1b73_a915);
    let wide_walkup = b.lv >= 4 && frame.width > 15.0 && b.area < 600.0;
    if b.shack {
        let count = if wide_walkup { 2 } else { 1 };
        let width = if wide_walkup {
            (frame.width * 0.35).clamp(3.2, 8.0)
        } else {
            (frame.width * (0.65 + random.f() * 0.1)).clamp(2.6, 10.5)
        };
        let depth = (frame.depth * (0.44 + random.f() * 0.1)).clamp(2.4, 7.5);
        for i in 0..count {
            for scale in [1.0, 0.82, 0.66] {
                let width = width * scale;
                let depth = depth * scale;
                if width < 2.5 || depth < 2.25 {
                    continue;
                }
                // Front laundry reaches +d/2 +0.9 m, plus pots, gutters and wall AC.
                let size = Vec2::new(width + 1.1, depth + 1.8);
                if let Some(envelope) = reserve(b, frame, size, &mut occupied, 1 + i * 2) {
                    result.shacks.push(ShackPlan {
                        area: RoofRect {
                            c: envelope.c - frame.n * 0.35,
                            width,
                            depth,
                            ..frame
                        },
                        envelope,
                        seed: b.seed7.wrapping_add(104729 * (i as u64 + 1)),
                        open: b.lv <= 3 && (b.id + i as u32).is_multiple_of(9),
                    });
                    break;
                }
            }
        }
    }
    // A stepped masonry volume breaks the repeated tin-box skyline. This visual upper
    // floor component does not introduce traversable floors or interactive doorways.
    if b.area >= 55.0 {
        result.access = reserve(
            b,
            frame,
            Vec2::new(2.65, 3.0),
            &mut occupied,
            (b.id as usize % 2) + 1,
        );
    }
    let mut random = Park(b.seed8 ^ 0x3cf0_91e7);
    for (kind, size, probability) in [
        (UtilityKind::Solar, Vec2::new(2.35, 1.65), 0.55),
        (UtilityKind::Laundry, Vec2::new(2.8, 1.1), 0.6),
        (UtilityKind::Compressor, Vec2::new(1.15, 0.75), 0.75),
        (UtilityKind::Antenna, Vec2::new(1.5, 0.7), 0.4),
    ] {
        if random.f() <= probability
            && let Some(area) = reserve(b, frame, size, &mut occupied, b.id as usize % 7)
        {
            result.utilities.push(UtilityPlan { area, kind });
        }
    }
    result
}

fn parapet_height(b: &Bld) -> f32 {
    if b.lv <= 3 {
        0.72
    } else {
        0.43 + (b.id % 3) as f32 * 0.07
    }
}

/// Keep only visible faces of a box, using geom::cuboid's documented face order.
/// Buried bottoms and covered tops do not reach the screen. Joint ends remain capped:
/// neighbouring roof edges do not fully cover them at every corner angle.
fn box_faces(width: f32, height: f32, depth: f32, faces: &[usize]) -> MeshBuf {
    let source = geom::cuboid(width, height, depth);
    let mut out = MeshBuf::default();
    for &face in faces {
        let first = face * 4;
        let base = out.pos.len() as u32;
        out.pos.extend_from_slice(&source.pos[first..first + 4]);
        out.nrm.extend_from_slice(&source.nrm[first..first + 4]);
        out.uv.extend_from_slice(&source.uv[first..first + 4]);
        out.col.extend_from_slice(&source.col[first..first + 4]);
        out.idx
            .extend([base, base + 2, base + 1, base + 2, base + 3, base + 1]);
    }
    out
}

fn parapets(b: &Bld, fine: bool, out: &mut Parts) {
    if b.rcls == RoofClass::None {
        return;
    }
    let ph = parapet_height(b);
    let tint = [0xbdb5a4, 0xaaa99b, 0xc5bdad, 0xaaa99f][b.id as usize % 4];
    for i in 0..b.pts.len() {
        let a = b.pts[i];
        let c = b.pts[(i + 1) % b.pts.len()];
        let d = c - a;
        let length = d.length();
        if length < 0.3 {
            continue;
        }
        let middle = (a + c) * 0.5;
        let transform = place(middle.x, b.h, middle.y, -d.y.atan2(d.x));
        if !fine {
            out.add(
                Mat::VCol,
                // The coping covers the top; retain the ends to close roof corners.
                &box_faces(length + 0.06, ph, 0.18, &[0, 1, 4, 5]),
                transform * geom::at(0.0, ph / 2.0, 0.0),
                hex(tint),
            );
            out.add(
                Mat::VCol,
                &box_faces(length + 0.12, 0.07, 0.27, &[0, 1, 2, 4, 5]),
                transform * geom::at(0.0, ph + 0.035, 0.0),
                hex(0xd0c8b7),
            );
        } else if b.lv >= 4 {
            // Thin rails are near detail along with their uprights; the solid parapet
            // and its coping remain identical in both chunk LODs.
            for y in [0.81, 1.02] {
                out.add(
                    Mat::Metal,
                    &geom::cuboid(length, 0.035, 0.035),
                    transform * geom::at(0.0, y, 0.0),
                    hex(0x7b817d),
                );
            }
            let posts = (length / 2.4).ceil().clamp(1.0, 12.0) as usize;
            for k in 0..=posts {
                out.add(
                    Mat::Metal,
                    &geom::cuboid(0.04, 1.02 - ph, 0.04),
                    transform
                        * geom::at(
                            -length * 0.5 + length * k as f32 / posts as f32,
                            (ph + 1.02) * 0.5,
                            0.0,
                        ),
                    hex(0x777d78),
                );
            }
        }
    }
}

/// Near and Mid share the same primary rooftop mass; the 70/95 m fine-detail boundary
/// removes fittings, never the whole shed or the roof-access volume.
pub fn structure(b: &Bld, lod: Lod) -> Parts {
    let mut out = Parts::default();
    if lod == Lod::Far || b.rcls == RoofClass::None {
        return out;
    }
    parapets(b, false, &mut out);
    let plan = layout(b);
    for shack in plan.shacks {
        let shell = super::detail::shack_layer(
            shack.area.width,
            shack.area.depth,
            shack.seed,
            shack.open,
            false,
        );
        let transform = shack.area.transform(b.h);
        for (material, mesh) in shell.0 {
            out.add(material, &mesh, transform, WHITE);
        }
    }
    if let Some(access) = plan.access {
        let transform = access.transform(b.h);
        let mut walls = box_faces(2.35, 2.35, 2.7, &[0, 1, 2, 4, 5]);
        for (point, colour) in walls.pos.iter().zip(&mut walls.col) {
            // A low damp gradient is vertex colour in the existing mesh/material.
            // Keep these older concrete volumes quieter than the coloured tin sheds.
            let shade = 0.72 + 0.28 * ((point[1] + 1.175) / 2.35).clamp(0.0, 1.0);
            *colour = [shade, shade, shade, 1.0];
        }
        let palette = [0x93948a, 0xa29b89, 0x8e9892, 0xaa9e87];
        out.add(
            Mat::VCol,
            &walls,
            transform * geom::at(0.0, 1.175, 0.0),
            hex(palette[((b.seed8 ^ b.seed7) % palette.len() as u64) as usize]),
        );
        out.add(
            Mat::VCol,
            &box_faces(2.6, 0.16, 2.95, &[0, 1, 2, 4, 5]),
            transform * geom::at(0.0, 2.38, 0.0),
            hex(0xaaa591),
        );
    }
    out
}

/// Fine fittings use exactly the same reservations as the persistent shell.
pub fn near_detail(b: &Bld) -> Parts {
    let mut out = Parts::default();
    if b.rcls == RoofClass::None {
        return out;
    }
    parapets(b, true, &mut out);
    let plan = layout(b);
    for shack in plan.shacks {
        let fine = super::detail::shack_layer(
            shack.area.width,
            shack.area.depth,
            shack.seed,
            shack.open,
            true,
        );
        let transform = shack.area.transform(b.h);
        for (material, mesh) in fine.0 {
            out.add(material, &mesh, transform, WHITE);
        }
    }
    if let Some(access) = plan.access {
        let transform = access.transform(b.h);
        out.add(
            Mat::Shutter,
            &geom::cuboid(0.86, 1.95, 0.04),
            transform * geom::at(0.24, 0.98, 1.37),
            hex(0x8eaaa4),
        );
        for x in [-0.25, 0.73] {
            out.add(
                Mat::Metal,
                &geom::cuboid(0.06, 2.08, 0.08),
                transform * geom::at(x, 1.04, 1.38),
                hex(0x737b76),
            );
        }
        out.add(
            Mat::Metal,
            &geom::cuboid(1.04, 0.06, 0.08),
            transform * geom::at(0.24, 2.05, 1.38),
            hex(0x737b76),
        );
        out.add(
            Mat::Glass,
            &geom::cuboid(0.72, 0.38, 0.03),
            transform * geom::at(-0.43, 1.62, -1.37),
            WHITE,
        );
    }
    for object in plan.utilities {
        let transform = object.area.transform(b.h);
        match object.kind {
            UtilityKind::Solar => {
                out.add(
                    Mat::Glass,
                    &geom::cuboid(1.9, 0.08, 1.0),
                    transform * trs([0.0, 0.7, 0.0], [-0.6, 0.0, 0.0], [1.0; 3]),
                    WHITE,
                );
                out.add(
                    Mat::Metal,
                    &geom::cylinder(0.22, 0.22, 1.9, 10),
                    transform * trs([0.0, 1.15, -0.45], [0.0, 0.0, FRAC_PI_2], [1.0; 3]),
                    hex(0xd8dcdc),
                );
                for x in [-0.8, 0.8] {
                    out.add(
                        Mat::Metal,
                        &geom::cuboid(0.06, 1.0, 0.06),
                        transform * geom::at(x, 0.5, -0.4),
                        hex(0x777d77),
                    );
                }
            }
            UtilityKind::Compressor => {
                out.add(
                    Mat::VCol,
                    &geom::cuboid(0.82, 0.56, 0.32),
                    transform * geom::at(0.0, 0.28, 0.0),
                    hex(0xd7d4c8),
                );
                out.add(
                    Mat::VCol,
                    &geom::cuboid(0.42, 0.42, 0.02),
                    transform * geom::at(-0.12, 0.28, 0.17),
                    hex(0x363d3a),
                );
            }
            UtilityKind::Laundry => {
                for x in [-1.2, 1.2] {
                    out.add(
                        Mat::Metal,
                        &geom::cuboid(0.055, 1.85, 0.055),
                        transform * geom::at(x, 0.925, 0.0),
                        hex(0xa1aaa4),
                    );
                    out.add(
                        Mat::VCol,
                        &geom::cuboid(0.24, 0.12, 0.34),
                        transform * geom::at(x, 0.06, 0.0),
                        hex(0x9e9b8a),
                    );
                }
                out.add(
                    Mat::Metal,
                    &geom::cuboid(2.5, 0.035, 0.035),
                    transform * geom::at(0.0, 1.79, 0.0),
                    hex(0x8b9691),
                );
                for (i, colour) in [0x9d5e4e, 0xd5c7ac, 0x657b92, 0xa9ae97]
                    .into_iter()
                    .enumerate()
                {
                    out.add(
                        Mat::VCol,
                        &geom::cuboid(0.4, 0.62, 0.025),
                        transform * geom::at(-0.85 + i as f32 * 0.56, 1.44, 0.0),
                        hex(colour),
                    );
                }
            }
            UtilityKind::Antenna => {
                out.add(
                    Mat::Metal,
                    &geom::cuboid(0.045, 2.2, 0.045),
                    transform * geom::at(0.0, 1.1, 0.0),
                    hex(0x7c817d),
                );
                for (y, width) in [(1.95, 1.2), (1.65, 0.9), (1.35, 0.6)] {
                    out.add(
                        Mat::Metal,
                        &geom::cuboid(width, 0.035, 0.035),
                        transform * geom::at(0.0, y, 0.0),
                        hex(0x7c817d),
                    );
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reservations(b: &Bld, plan: &RoofLayout) -> Vec<RoofRect> {
        let mut result: Vec<_> = plan.shacks.iter().map(|s| s.envelope).collect();
        result.extend(plan.access);
        result.extend(plan.utilities.iter().map(|u| u.area));
        if let Some((c, _)) = b.tank {
            result.push(RoofRect {
                c,
                width: 1.8,
                depth: 1.8,
                ..roof_frame(b)
            });
        }
        result
    }

    #[test]
    fn roof_layouts_fit_entire_footprints_and_reserve_existing_tanks() {
        let world = super::super::world();
        let (mut sheds, mut access, mut twins) = (0, 0, 0);
        for b in &world.blds {
            let plan = layout(b);
            assert_eq!(plan, layout(b), "non-deterministic roof {}", b.id);
            for s in &plan.shacks {
                assert!(s.envelope.fits(b), "shack overhang leaves lot {}", b.id);
                let inv = s.envelope.transform(0.0).inverse();
                for fine in [false, true] {
                    let parts = super::super::detail::shack_layer(
                        s.area.width,
                        s.area.depth,
                        s.seed,
                        s.open,
                        fine,
                    );
                    for mesh in parts.0.values() {
                        for point in &mesh.pos {
                            let p = inv.transform_point3(
                                s.area.transform(0.0).transform_point3(Vec3::from(*point)),
                            );
                            assert!(
                                p.x.abs() <= s.envelope.width * 0.5 + 0.01
                                    && p.z.abs() <= s.envelope.depth * 0.5 + 0.01,
                                "roof {} object escapes its reservation: {p:?}",
                                b.id
                            );
                        }
                    }
                }
            }
            for area in plan
                .access
                .iter()
                .chain(plan.utilities.iter().map(|u| &u.area))
            {
                assert!(area.fits(b), "utility leaves roof {}", b.id);
            }
            let areas = reservations(b, &plan);
            for i in 0..areas.len() {
                for j in i + 1..areas.len() {
                    assert!(
                        !areas[i].overlaps(areas[j]),
                        "roof {} objects {i} and {j} overlap",
                        b.id
                    );
                }
            }
            sheds += plan.shacks.len();
            access += usize::from(plan.access.is_some());
            twins += usize::from(plan.shacks.len() == 2);
        }
        assert!(
            sheds > 100 && access > 50 && twins > 5,
            "{sheds}/{access}/{twins}"
        );
    }

    #[test]
    fn roof_major_silhouette_is_identical_at_near_and_mid_with_bounded_meshes() {
        let world = super::super::world();
        let mut checked = 0;
        for b in &world.blds {
            let near = structure(b, Lod::Near);
            let mid = structure(b, Lod::Mid);
            let fine = near_detail(b);
            assert_eq!(structure(b, Lod::Far).vcount(), 0);
            assert_eq!(near.vcount(), mid.vcount());
            assert!(
                near.vcount() <= 1280,
                "roof {} shell {}",
                b.id,
                near.vcount()
            );
            assert!(
                fine.vcount() <= 16000,
                "roof {} detail {}",
                b.id,
                fine.vcount()
            );
            assert!(near.0.len() <= 3 && fine.0.len() <= 6);
            for (material, mesh) in &near.0 {
                let other = &mid.0[material];
                assert_eq!(mesh.pos, other.pos);
                assert_eq!(mesh.idx, other.idx);
            }
            for mesh in near.0.values().chain(fine.0.values()) {
                assert_eq!(mesh.pos.len(), mesh.nrm.len());
                assert!(mesh.pos.iter().all(|p| Vec3::from(*p).is_finite()));
                assert!(mesh.nrm.iter().all(|p| Vec3::from(*p).is_finite()));
                assert!(mesh.idx.iter().all(|i| (*i as usize) < mesh.pos.len()));
            }
            checked += usize::from(near.vcount() > 0);
        }
        assert!(checked > 200);
    }

    #[test]
    fn roof_corner_caps_close_the_exposed_wall_and_coping_end_strips() {
        let mut b = super::super::world().blds[0].clone();
        b.pts = vec![
            Vec2::ZERO,
            Vec2::new(10.0, 0.0),
            Vec2::splat(10.0),
            Vec2::new(0.0, 10.0),
        ];
        b.rcls = RoofClass::Walkup;
        b.lv = 4;
        let mut parts = Parts::default();
        parapets(&b, false, &mut parts);
        let mesh = &parts.0[&Mat::VCol];
        // At this 90-degree corner, the neighbouring edge begins at z=-0.03/-0.06.
        // These outward-facing end strips would therefore remain open without caps.
        for point in [
            Vec3::new(10.03, b.h + 0.2, -0.06),
            Vec3::new(10.06, b.h + parapet_height(&b) + 0.035, -0.1),
        ] {
            let covered = mesh.idx.as_chunks::<3>().0.iter().any(|triangle| {
                let p = triangle.map(|i| Vec3::from(mesh.pos[i as usize]));
                if p.iter().any(|p| (p.x - point.x).abs() > 0.002)
                    || mesh.nrm[triangle[0] as usize][0] < 0.99
                {
                    return false;
                }
                let q = point.yz();
                let sides: Vec<_> = (0..3)
                    .map(|i| (p[(i + 1) % 3].yz() - p[i].yz()).perp_dot(q - p[i].yz()))
                    .collect();
                sides.iter().all(|v| *v >= -0.001) || sides.iter().all(|v| *v <= 0.001)
            });
            assert!(covered, "uncapped roof corner at {point:?}");
        }
    }

    #[test]
    fn concave_roof_notch_cannot_cut_through_a_reserved_rectangle() {
        let mut b = super::super::world().blds[0].clone();
        b.pts = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(6.0, 10.0),
            Vec2::new(6.0, 3.0),
            Vec2::new(4.0, 3.0),
            Vec2::new(4.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        let area = RoofRect {
            c: Vec2::new(5.0, 5.0),
            u: Vec2::X,
            n: Vec2::Y,
            width: 8.0,
            depth: 4.0,
        };
        assert!(area.corners().iter().all(|p| pip(*p, &b.pts)));
        assert!(!area.fits(&b));
        assert!(
            RoofRect {
                c: Vec2::new(2.0, 5.0),
                width: 2.0,
                ..area
            }
            .fits(&b)
        );
        assert!(
            !RoofRect {
                c: Vec2::new(2.0, 5.0),
                width: 3.9,
                ..area
            }
            .fits(&b)
        );
    }
}
