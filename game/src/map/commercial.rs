//! Render-only shopfronts on the existing ground-floor walls and entrances.
//!
//! Shop identity comes from the original sign cell, not from a new gameplay shop
//! classification. Named temples retain that stored cell but render an unbranded
//! plaque instead of a random shop name. Main and projecting signs agree. The planner leaves
//! world RNG, footprints, collisions, loot and all saved identifiers untouched.
//! Shallow closed display panels remain against solid walls; this layer creates
//! neither extra entrances nor decorative objects that look like pickup fixtures.

use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

use super::build::{BuildingMeshes, Lod, Mat, Parts, WHITE};
use super::data::{ARC, BAY, Bld, COL, DOOR_H, SignSpec, WallBox, WorldData, pip, seg_dist};
use super::exterior::{self, Frontage};
use super::textures::{SIGN_COLS, SIGN_ROWS};
use crate::geom::{self, MeshBuf, hex, place};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShopFamily {
    Food,
    Traditional,
    Convenience,
    Pharmacy,
    Police,
}

impl ShopFamily {
    pub fn name(self) -> &'static str {
        match self {
            Self::Food => "food",
            Self::Traditional => "traditional",
            Self::Convenience => "convenience",
            Self::Pharmacy => "pharmacy",
            Self::Police => "police",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelFinish {
    Glazed,
    Shutter,
    Timber,
}

/// A closed, shallow treatment of an existing solid wall, never a new opening.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShopPanel {
    pub centre: Vec2,
    pub u: Vec2,
    pub n: Vec2,
    pub width: f32,
    pub height: f32,
    pub finish: PanelFinish,
    pub owner: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Canopy {
    /// Centre on the existing non-arcaded ground-floor wall.
    pub centre: Vec2,
    pub u: Vec2,
    pub n: Vec2,
    pub width: f32,
    pub depth: f32,
    pub mount_y: f32,
    pub edge_y: f32,
    pub striped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BladeSign {
    /// Centre of a double-sided, outward-projecting horizontal lightbox.
    pub centre: Vec3,
    /// Normal of the original facade; the lightbox's long axis projects along it.
    pub normal: Vec2,
    pub tangent: Vec2,
    pub width: f32,
    pub height: f32,
    pub sign_index: usize,
}

#[derive(Clone, Debug)]
pub struct CommercialPlan {
    pub family: ShopFamily,
    pub sign_index: usize,
    pub frontage: Frontage,
    pub storefront: Frontage,
    pub fascia: SignSpec,
    pub fascia_height: f32,
    pub fascia_depth: f32,
    pub fascia_rim: bool,
    pub panels: Vec<ShopPanel>,
    pub canopy: Option<Canopy>,
    pub blade: Option<BladeSign>,
    pub secondary_fascia: bool,
    pub arcaded: bool,
    pub palette: usize,
}

/// Runtime batches: only low kickplates stay with the base; full-height closed
/// panels, signage, canopy and under-soffit fascia follow the roof cutaway.
#[derive(Default)]
pub struct CommercialMeshes {
    pub base: Parts,
    pub shell: Parts,
}

pub const MAX_NEAR_VERTICES: usize = 1_400;
pub const MAX_MID_VERTICES: usize = 600;
const PANEL_Z: f32 = 0.205;
const PANEL_FRONT: f32 = 0.285;
const FASCIA_H: f32 = 1.02;

/// Original map names identify these religious entrances. This affects only
/// rendering: no building kind, fixture, sign index or saved ID is rewritten.
pub fn is_named_temple(building: &Bld) -> bool {
    matches!(building.name.chars().last(), Some('寺' | '宮' | '廟'))
}

fn family(index: usize) -> ShopFamily {
    match index {
        0 | 2 | 6 | 14 => ShopFamily::Food,
        10 | 13 | 16 => ShopFamily::Convenience,
        11 => ShopFamily::Pharmacy,
        12 => ShopFamily::Police,
        _ => ShopFamily::Traditional,
    }
}

fn variation(b: &Bld) -> u32 {
    let mut value = b.id.wrapping_mul(0x85eb_ca6b) ^ b.seed8 as u32 ^ 0x7d31_52a9;
    value = (value ^ (value >> 16)).wrapping_mul(0x7feb_352d);
    value ^ (value >> 15)
}

fn rect(centre: Vec2, u: Vec2, n: Vec2, width: f32, near: f32, far: f32) -> [Vec2; 4] {
    [
        centre - u * (width * 0.5) + n * near,
        centre + u * (width * 0.5) + n * near,
        centre + u * (width * 0.5) + n * far,
        centre - u * (width * 0.5) + n * far,
    ]
}

fn intersects(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let ab = b - a;
    let cd = d - c;
    let denominator = ab.perp_dot(cd);
    if denominator.abs() < 1e-7 {
        return seg_dist(a, c, d) < 0.0001
            || seg_dist(b, c, d) < 0.0001
            || seg_dist(c, a, b) < 0.0001
            || seg_dist(d, a, b) < 0.0001;
    }
    let t = (c - a).perp_dot(cd) / denominator;
    let s = (c - a).perp_dot(ab) / denominator;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&s)
}

fn edge_distance(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> f32 {
    if intersects(a, b, c, d) {
        0.0
    } else {
        seg_dist(a, c, d)
            .min(seg_dist(b, c, d))
            .min(seg_dist(c, a, b))
            .min(seg_dist(d, a, b))
    }
}

fn overlap(a: &[Vec2], b: &[Vec2], margin: f32) -> bool {
    a.iter().any(|p| pip(*p, b))
        || b.iter().any(|p| pip(*p, a))
        || (0..a.len()).any(|i| {
            (0..b.len()).any(|j| {
                edge_distance(a[i], a[(i + 1) % a.len()], b[j], b[(j + 1) % b.len()]) <= margin
            })
        })
}

/// Exact segment/rectangle distance against every relevant road segment, including
/// a short segment wholly inside the rectangle. Corner-only sampling misses these.
fn road_clear(world: &WorldData, corners: &[Vec2; 4], margin: f32) -> bool {
    let (lo, hi) = bounds(corners);
    for road in &world.roads {
        let radius = road.w * 0.5 + margin;
        for pair in road.p.windows(2) {
            let a = pair[0];
            let b = pair[1];
            if a.min(b).x - radius > hi.x
                || a.max(b).x + radius < lo.x
                || a.min(b).y - radius > hi.y
                || a.max(b).y + radius < lo.y
            {
                continue;
            }
            if pip(a, corners)
                || pip(b, corners)
                || (0..4).any(|i| edge_distance(a, b, corners[i], corners[(i + 1) % 4]) <= radius)
            {
                return false;
            }
        }
    }
    true
}

fn bounds(points: &[Vec2]) -> (Vec2, Vec2) {
    points.iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    )
}

/// The complete projection must stay clear of adjacent lots and the carriageway.
/// Arcaded wall panels are inside their own footprint, in the already-open arcade.
/// A hanging fascia may straddle only that known open front edge; every other
/// footprint edge still rejects the complete rectangle, including concave notches.
fn envelope_clear(b: &Bld, world: &WorldData, corners: &[Vec2; 4], in_arcade: bool) -> bool {
    let (lo, hi) = bounds(corners);
    for id in world.blds_in(lo - 0.04, hi + 0.04) {
        let other = &world.blds[id as usize];
        if other.id != b.id && overlap(corners, &other.pts, 0.025) {
            return false;
        }
    }
    if !road_clear(world, corners, 0.035) {
        return false;
    }
    if in_arcade {
        let Some(arcade) = b.arcade else {
            return false;
        };
        if corners.iter().any(|point| {
            let relative = *point - arcade.a;
            let along = relative.dot(arcade.u);
            let outward = relative.dot(arcade.n);
            along < 0.025
                || along > arcade.dl - 0.025
                || outward < -ARC + 0.11
                || outward > 0.10
                || (outward < -0.0001 && !pip(*point, &b.pts))
                || (outward > 0.0001 && pip(*point, &b.pts))
        }) {
            return false;
        }
        !(0..4).any(|i| {
            (0..b.pts.len()).any(|j| {
                let a = b.pts[j];
                let c = b.pts[(j + 1) % b.pts.len()];
                let open_front = (a - arcade.a).dot(arcade.n).abs() < 0.01
                    && (c - arcade.a).dot(arcade.n).abs() < 0.01;
                !open_front && intersects(corners[i], corners[(i + 1) % 4], a, c)
            })
        })
    } else {
        !overlap(corners, &b.pts, 0.0)
    }
}

fn door_clear(world: &WorldData, corners: &[Vec2; 4]) -> bool {
    let (lo, hi) = bounds(corners);
    for id in world.blds_in(lo - 2.0, hi + 2.0) {
        let Some(door) = world.blds[id as usize].door else {
            continue;
        };
        // The actual two leaves pivot out from +/-0.8m. This larger reservation
        // contains both leaf sweeps and the existing entrance/player route.
        let passage = rect(door.p, door.tangent(), door.n, 2.4, -0.45, 1.3);
        if overlap(corners, &passage, 0.035) {
            return false;
        }
    }
    true
}

fn walking_strip_clear(world: &WorldData, centre: Vec2, u: Vec2, n: Vec2, width: f32) -> bool {
    let centre = centre + n * 0.96;
    let a = centre - u * (width * 0.5);
    let c = centre + u * (width * 0.5);
    // The longest panel is 5.8m, wholly inside this existing 3x3-cell query.
    // Testing a swept segment keeps the entire 0.96m strip clear, including
    // obstacles between the old left/centre/right sample positions.
    world
        .near_segs(centre)
        .all(|wall| edge_distance(a, c, wall.a, wall.b) >= 0.48)
}

fn wall_span(b: &Bld, wall: &WallBox) -> Option<(f32, f32)> {
    let door = b.door?;
    if wall.y0 > 0.01 || wall.y1 < DOOR_H {
        return None;
    }
    let a = wall.a - door.p;
    let c = wall.c - door.p;
    if a.dot(door.n).abs() > 0.04 || c.dot(door.n).abs() > 0.04 {
        return None;
    }
    let a = a.dot(door.tangent());
    let c = c.dot(door.tangent());
    Some((a.min(c), a.max(c)))
}

fn finish(family: ShopFamily, index: usize, seed: u32) -> PanelFinish {
    match family {
        ShopFamily::Convenience | ShopFamily::Pharmacy | ShopFamily::Police => PanelFinish::Glazed,
        ShopFamily::Food if index % 2 == seed as usize % 2 => PanelFinish::Glazed,
        ShopFamily::Traditional if seed.is_multiple_of(3) => PanelFinish::Timber,
        _ => PanelFinish::Shutter,
    }
}

/// Rendering metadata only; this never modifies the Bld or WorldData it inspects.
pub fn plan(b: &Bld, world: &WorldData) -> Option<CommercialPlan> {
    let original = b.sign?;
    let door = b.door?;
    let frontage = exterior::specification(b).front?;
    if original.idx >= 18 || frontage.width < 2.65 || b.h < 3.65 {
        return None;
    }
    let storefront = Frontage {
        a: frontage.a - door.n * if b.arcade.is_some() { ARC } else { 0.0 },
        ..frontage
    };
    let family = family(original.idx);
    let seed = variation(b);
    let architecture = exterior::plan(b, world);
    let mut fascia = original;
    let deep_rect = rect(
        frontage.centre(),
        frontage.u,
        frontage.n,
        original.w + 0.12,
        0.05,
        0.30,
    );
    let deep = envelope_clear(b, world, &deep_rect, false)
        && !exterior::attachment_conflicts(
            b,
            &architecture,
            Vec3::new(original.p.x, original.p.y, original.p.z),
            Vec3::new(original.w * 0.5 + 0.07, 0.57, 0.15),
            door.n,
        );
    if deep {
        fascia.p += Vec3::new(door.n.x, 0.0, door.n.y) * 0.09;
    }
    let mut panels = Vec::new();
    let mut candidates = Vec::new();
    if let Some(arcade) = b.arcade {
        let bays = ((arcade.dl - COL) / BAY).round().max(1.0) as usize;
        let step = (arcade.dl - 0.7) / bays as f32;
        for index in 0..bays {
            let s = 0.35 + (index as f32 + 0.5) * step;
            let centre = arcade.a + arcade.u * s - arcade.n * ARC;
            let offset = (centre - door.p).dot(door.tangent());
            let half_width = (step - COL - 0.1) * 0.5;
            for wall in &b.walls {
                let Some((lo, hi)) = wall_span(b, wall) else {
                    continue;
                };
                let mut left = (offset - half_width).max(lo + 0.05);
                let mut right = (offset + half_width).min(hi - 0.05);
                if hi <= 0.0 {
                    right = right.min(-1.27);
                } else if lo >= 0.0 {
                    left = left.max(1.27);
                } else {
                    continue;
                }
                // A compact arcade bay can straddle the door. Its two actual
                // solid flanks are useful closed panels; the doorway is never
                // covered just to keep a whole theoretical ownership bay.
                if right - left >= 1.0 {
                    candidates.push(((left + right) * 0.5, (right - left).min(5.8)));
                }
            }
        }
    } else {
        for wall in &b.walls {
            let Some((lo, hi)) = wall_span(b, wall) else {
                continue;
            };
            // Leave the door leaf, street number and utility strip unobstructed.
            let (lo, hi) = if hi <= 0.0 {
                (lo + 0.18, hi.min(-1.27))
            } else if lo >= 0.0 {
                (lo.max(1.27), hi - 0.18)
            } else {
                continue;
            };
            if hi - lo < 1.0 {
                continue;
            }
            let count = (((hi - lo) / 3.8).ceil() as usize).clamp(1, 2);
            let width = ((hi - lo) / count as f32 - 0.1).min(5.8);
            for index in 0..count {
                let offset = if hi <= 0.0 {
                    hi - width * 0.5 - index as f32 * (width + 0.1)
                } else {
                    lo + width * 0.5 + index as f32 * (width + 0.1)
                };
                candidates.push((offset, width));
            }
        }
    }
    candidates.sort_by(|a, c| a.0.abs().total_cmp(&c.0.abs()).then(a.0.total_cmp(&c.0)));
    for (offset, width) in candidates {
        if panels.len() >= 4 || width < 1.0 {
            continue;
        }
        let left = offset - width * 0.5;
        let right = offset + width * 0.5;
        let fits_wall = b
            .walls
            .iter()
            .filter_map(|wall| wall_span(b, wall))
            .any(|(lo, hi)| left >= lo + 0.025 && right <= hi - 0.025);
        if !fits_wall {
            continue;
        }
        let centre = door.p + door.tangent() * offset;
        let corners = rect(
            centre,
            door.tangent(),
            door.n,
            width + 0.02,
            0.14,
            PANEL_FRONT + 0.02,
        );
        if !envelope_clear(b, world, &corners, b.arcade.is_some()) || !door_clear(world, &corners) {
            continue;
        }
        // The complete wall-hugging panel lies behind the ordinary player capsule;
        // still require a genuine 0.96m walking strip outside the treatment.
        if !walking_strip_clear(world, centre, door.tangent(), door.n, width) {
            continue;
        }
        panels.push(ShopPanel {
            centre,
            u: door.tangent(),
            n: door.n,
            width,
            height: 2.66,
            finish: finish(family, panels.len(), seed),
            owner: panels.len(),
        });
    }
    let mut canopy = None;
    if b.arcade.is_none() && family != ShopFamily::Police {
        let width = (frontage.width - 0.34).min(original.w + 1.6).min(7.2);
        for depth in [1.12, 0.82, 0.60] {
            let corners = rect(
                frontage.centre(),
                frontage.u,
                frontage.n,
                width + 0.04,
                0.14,
                depth + 0.25,
            );
            let centre = frontage.centre() + frontage.n * (0.195 + depth * 0.5);
            if width >= 2.3
                && envelope_clear(b, world, &corners, false)
                && !exterior::attachment_conflicts(
                    b,
                    &architecture,
                    Vec3::new(centre.x, 3.10, centre.y),
                    Vec3::new(width * 0.5 + 0.02, 0.51, depth * 0.5 + 0.055),
                    frontage.n,
                )
            {
                canopy = Some(Canopy {
                    centre: frontage.centre(),
                    u: frontage.u,
                    n: frontage.n,
                    width,
                    depth,
                    mount_y: 3.57,
                    edge_y: 3.31,
                    striped: matches!(family, ShopFamily::Food | ShopFamily::Traditional),
                });
                break;
            }
        }
    }
    let mut blade = None;
    if family != ShopFamily::Police && frontage.width >= original.w + 1.15 {
        let offset = frontage.width * 0.5 - 0.36;
        let height = 0.44;
        let width = 1.28;
        let y = if b.arcade.is_some() { 2.95 } else { 2.88 };
        for side in [
            if seed.is_multiple_of(2) { -1.0 } else { 1.0 },
            if seed.is_multiple_of(2) { 1.0 } else { -1.0 },
        ] {
            // An arcade blade mounts to the actual recessed storefront, not to
            // an imaginary wall across the open corridor between its columns.
            let anchor = storefront.centre() + frontage.u * (side * offset);
            let corners = rect(anchor, frontage.u, frontage.n, 0.18, 0.13, 0.16 + width);
            let centre = anchor + frontage.n * (0.16 + width * 0.5);
            if envelope_clear(b, world, &corners, b.arcade.is_some())
                && !exterior::attachment_conflicts(
                    b,
                    &architecture,
                    Vec3::new(centre.x, y, centre.y),
                    Vec3::new(0.09, height * 0.5 + 0.04, width * 0.5),
                    frontage.n,
                )
            {
                blade = Some(BladeSign {
                    centre: Vec3::new(centre.x, y, centre.y),
                    normal: frontage.n,
                    tangent: frontage.u,
                    width,
                    height,
                    sign_index: original.idx,
                });
                break;
            }
        }
    }
    let (fascia_height, fascia_depth, fascia_rim) = if let Some(canopy) = canopy {
        // A wall-mounted label would sit underneath the canopy in the normal
        // high isometric view. Its same identity lives on the front lightbox.
        // The lower rim stays above 2.6m, clear of every original door and leaf.
        let point = canopy.centre + canopy.n * (canopy.depth + 0.22);
        fascia.p = Vec3::new(point.x, 3.02, point.y);
        (0.70, 0.18, true)
    } else if deep {
        (FASCIA_H, 0.20, true)
    } else {
        // Retain the old footprint where even a slightly deeper rim cannot fit.
        (1.10, 0.06, false)
    };
    let secondary_fascia = b.arcade.is_some()
        && frontage.width > 4.0
        && envelope_clear(
            b,
            world,
            &rect(
                frontage.centre(),
                frontage.u,
                frontage.n,
                original.w.min(3.2) + 0.10,
                -0.07,
                0.08,
            ),
            true,
        );
    Some(CommercialPlan {
        family,
        sign_index: original.idx,
        frontage,
        storefront,
        fascia,
        fascia_height,
        fascia_depth,
        fascia_rim,
        panels,
        canopy,
        blade,
        secondary_fascia,
        arcaded: b.arcade.is_some(),
        palette: seed as usize % 3,
    })
}

/// Keep the original arcade shutter wherever no new complete panel replaces it.
pub fn replaces_shutter(plan: &CommercialPlan, centre: Vec2, width: f32) -> bool {
    plan.panels.iter().any(|panel| {
        let relative = centre - panel.centre;
        relative.dot(panel.n).abs() < 0.2
            && relative.dot(panel.u).abs() + width * 0.5 <= panel.width * 0.5 + 0.015
    })
}

/// Existing utility/stain detail must not poke through an opaque shop panel.
pub fn covers_wall(plan: &CommercialPlan, wall: &WallBox) -> bool {
    plan.panels.iter().any(|panel| {
        let centre = (wall.a + wall.c) * 0.5;
        (centre - panel.centre).dot(panel.n).abs() < 0.04
            && (wall.c - wall.a).normalize_or_zero().dot(panel.u).abs() > 0.99
            && (centre - panel.centre).dot(panel.u).abs()
                < wall.a.distance(wall.c) * 0.5 + panel.width * 0.5
            && wall.y0 < 0.01
    })
}

fn colors(plan: &CommercialPlan, temple: bool) -> (u32, u32, u32) {
    if temple {
        return (0x8d3028, 0xba9650, 0x573528);
    }
    let (paint, trim, shutter) = match plan.family {
        ShopFamily::Food => (0x884739, 0xd7c8a5, [0x78877c, 0xa58a6d, 0x8b7162]),
        ShopFamily::Traditional => (0x526b66, 0xc3b695, [0x808a82, 0x817e70, 0x8c7770]),
        ShopFamily::Convenience => (0x467460, 0xe2dcc9, [0x8f9d91, 0x9caa9a, 0x819080]),
        ShopFamily::Pharmacy => (0x4d806e, 0xe3dfcb, [0x92a499, 0x96aaa0, 0x83978d]),
        ShopFamily::Police => (0x3f536f, 0xd9dbd1, [0x8b959c, 0x8b959c, 0x8b959c]),
    };
    (paint, trim, shutter[plan.palette])
}

/// Face selection follows geom::cuboid's +/-X, +/-Y, +/-Z order. Buried backs
/// are omitted; the outward faces retain flat normals and deterministic winding.
fn box_faces(size: Vec3, mask: u8) -> MeshBuf {
    let source = geom::cuboid(size.x, size.y, size.z);
    let mut mesh = MeshBuf::default();
    for face in 0..6 {
        if mask & (1 << face) == 0 {
            continue;
        }
        let start = mesh.vcount() as u32;
        let range = face * 4..face * 4 + 4;
        mesh.pos.extend_from_slice(&source.pos[range.clone()]);
        mesh.nrm.extend_from_slice(&source.nrm[range.clone()]);
        mesh.uv.extend_from_slice(&source.uv[range.clone()]);
        mesh.col.extend_from_slice(&source.col[range]);
        for &index in &source.idx[face * 6..face * 6 + 6] {
            mesh.idx.push(start + index - face as u32 * 4);
        }
    }
    mesh
}

fn box_at(parts: &mut Parts, t: Mat4, size: Vec3, at: Vec3, color: u32, detailed: bool) {
    parts.add(
        Mat::VCol,
        &box_faces(size, if detailed { 0b01_1111 } else { 0b01_0111 }),
        t * Mat4::from_translation(at),
        hex(color),
    );
}

fn plane_at(parts: &mut Parts, t: Mat4, width: f32, height: f32, at: Vec3, color: u32) {
    parts.add(
        Mat::VCol,
        &geom::plane(width, height),
        t * Mat4::from_translation(at),
        hex(color),
    );
}

fn sign_face(parts: &mut Parts, t: Mat4, width: f32, height: f32, index: usize, temple: bool) {
    let col = (index % SIGN_COLS as usize) as f32;
    let row = (index / SIGN_COLS as usize) as f32;
    // Inset by half a texel to keep adjacent atlas identities out of the border.
    let face = geom::plane(width, height).map_uv(|uv, _| {
        if temple {
            super::textures::unbranded_sign_uv(index)
        } else {
            [
                (col + (0.5 + uv[0] * 255.0) / 256.0) / SIGN_COLS,
                (row + (0.5 + (1.0 - uv[1]) * 63.0) / 64.0) / SIGN_ROWS,
            ]
        }
    });
    parts.add(
        Mat::Sign,
        &face,
        t,
        if temple { hex(0x30241b) } else { WHITE },
    );
}

fn panel_geometry(
    panel: ShopPanel,
    plan: &CommercialPlan,
    lod: Lod,
    temple: bool,
    out: &mut Parts,
) {
    let (paint, trim, shutter) = colors(plan, temple);
    let t = place(
        panel.centre.x,
        0.0,
        panel.centre.y,
        panel.n.x.atan2(panel.n.y),
    );
    let bottom = 0.14;
    let top = bottom + panel.height;
    let detailed = lod == Lod::Near;
    let width = panel.width;
    let panel_color = if temple {
        0x573528
    } else {
        match panel.finish {
            PanelFinish::Glazed => 0x293f42,
            PanelFinish::Shutter => shutter,
            PanelFinish::Timber => 0x70543f,
        }
    };
    // Front and returns form a closed display/shutter on an existing wall. Dark
    // glazing is opaque on purpose: it is not a fake traversable interior opening.
    box_at(
        out,
        t,
        Vec3::new(width, panel.height, 0.10),
        Vec3::new(0.0, bottom + panel.height * 0.5, PANEL_Z - 0.01),
        panel_color,
        detailed,
    );
    let base_h = if panel.finish == PanelFinish::Glazed {
        0.61
    } else {
        0.20
    };
    plane_at(
        out,
        t,
        width - 0.09,
        base_h,
        Vec3::new(0.0, bottom + base_h * 0.5, PANEL_Z + 0.043),
        paint,
    );
    for x in [-width * 0.5 + 0.055, width * 0.5 - 0.055] {
        box_at(
            out,
            t,
            Vec3::new(0.11, panel.height + 0.04, 0.075),
            Vec3::new(x, bottom + panel.height * 0.5, PANEL_Z + 0.04),
            trim,
            detailed,
        );
    }
    box_at(
        out,
        t,
        Vec3::new(width, 0.16, 0.12),
        Vec3::new(0.0, top + 0.035, PANEL_Z + 0.02),
        paint,
        detailed,
    );
    if detailed {
        match panel.finish {
            PanelFinish::Glazed => {
                for x in [-width * 0.25, width * 0.25] {
                    plane_at(
                        out,
                        t,
                        width * 0.44,
                        0.045,
                        Vec3::new(x, 2.12, PANEL_FRONT - 0.003),
                        if temple { 0xa78548 } else { 0x7a9292 },
                    );
                    plane_at(
                        out,
                        t,
                        width * 0.39,
                        0.30,
                        Vec3::new(x, 1.9, PANEL_FRONT - 0.006),
                        if temple { 0x663a29 } else { 0x40595a },
                    );
                }
                box_at(
                    out,
                    t,
                    Vec3::new(0.055, panel.height - base_h, 0.045),
                    Vec3::new(
                        0.0,
                        bottom + base_h + (panel.height - base_h) * 0.5,
                        PANEL_Z + 0.05,
                    ),
                    trim,
                    true,
                );
                plane_at(
                    out,
                    t,
                    width - 0.2,
                    0.06,
                    Vec3::new(0.0, 1.34, PANEL_FRONT - 0.001),
                    trim,
                );
            }
            PanelFinish::Shutter => {
                for rib in 0..13 {
                    plane_at(
                        out,
                        t,
                        width - 0.18,
                        0.022,
                        Vec3::new(0.0, 0.38 + rib as f32 * 0.17, PANEL_Z + 0.045),
                        if temple { 0x39271c } else { 0x626b63 },
                    );
                }
                plane_at(
                    out,
                    t,
                    (width * 0.18).min(0.55),
                    0.06,
                    Vec3::new(0.0, 0.34, PANEL_Z + 0.052),
                    if temple { 0xa78548 } else { 0x4e5853 },
                );
            }
            PanelFinish::Timber => {
                for slat in 1..6 {
                    plane_at(
                        out,
                        t,
                        0.032,
                        panel.height - 0.15,
                        Vec3::new(
                            (slat as f32 / 6.0 - 0.5) * (width - 0.16),
                            bottom + panel.height * 0.5,
                            PANEL_Z + 0.045,
                        ),
                        0x3f3e32,
                    );
                }
                plane_at(
                    out,
                    t,
                    width - 0.16,
                    0.075,
                    Vec3::new(0.0, 1.22, PANEL_Z + 0.048),
                    0xb09c78,
                );
            }
        }
        // Owner-specific repaired kick strip and one narrow wear mark, not random
        // freestanding clutter or another batch/material.
        plane_at(
            out,
            t,
            width * 0.23,
            0.06,
            Vec3::new(
                width
                    * if panel.owner.is_multiple_of(2) {
                        -0.25
                    } else {
                        0.25
                    },
                0.22,
                PANEL_FRONT - 0.001,
            ),
            0x8d8a75,
        );
    }
}

fn canopy_geometry(canopy: Canopy, plan: &CommercialPlan, lod: Lod, temple: bool, out: &mut Parts) {
    let (paint, trim, _) = colors(plan, temple);
    let t = place(
        canopy.centre.x,
        0.0,
        canopy.centre.y,
        canopy.n.x.atan2(canopy.n.y),
    );
    let slope = ((canopy.mount_y - canopy.edge_y) / canopy.depth).atan();
    let length = canopy.depth / slope.cos();
    let sheet = t
        * geom::at(0.0, canopy.mount_y, 0.16)
        * Mat4::from_rotation_x(slope)
        * geom::at(0.0, 0.0, length * 0.5);
    let count = if canopy.striped { 8 } else { 1 };
    for strip in 0..count {
        let width = canopy.width / count as f32;
        let x = (strip as f32 + 0.5) * width - canopy.width * 0.5;
        let color = if !temple && canopy.striped && strip % 2 == 1 {
            trim
        } else {
            paint
        };
        // One top/bottom pair per cloth strip. These are truly two-sided sheet
        // surfaces, with no buried cuboid faces between adjoining stripes.
        let sheet_panel =
            geom::plane2(width, length).transformed(Mat4::from_rotation_x(-FRAC_PI_2));
        out.add(
            Mat::VCol,
            &sheet_panel,
            sheet * geom::at(x, 0.0, 0.0),
            hex(color),
        );
    }
    // The front valance and both closed side returns make the canopy legible from
    // the fixed isometric camera. Its lowest point remains above 3.1 metres.
    box_at(
        out,
        t,
        Vec3::new(canopy.width, 0.14, 0.055),
        Vec3::new(0.0, canopy.edge_y - 0.055, canopy.depth + 0.16),
        paint,
        lod == Lod::Near,
    );
    if lod == Lod::Near {
        for side in [-1.0, 1.0] {
            let triangle = geom::shape(&[
                Vec2::new(0.16, canopy.mount_y),
                Vec2::new(canopy.depth + 0.16, canopy.edge_y),
                Vec2::new(0.16, canopy.edge_y),
            ]);
            out.add(
                Mat::VCol,
                &triangle,
                t * geom::at(side * canopy.width * 0.5, 0.0, 0.0)
                    * Mat4::from_rotation_y(-FRAC_PI_2),
                hex(trim),
            );
            out.add(
                Mat::VCol,
                &triangle,
                t * geom::at(side * canopy.width * 0.5, 0.0, 0.0)
                    * Mat4::from_rotation_y(-FRAC_PI_2)
                    * Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0)),
                hex(trim),
            );
        }
        plane_at(
            out,
            t,
            canopy.width - 0.1,
            0.028,
            Vec3::new(0.0, canopy.edge_y - 0.09, canopy.depth + 0.189),
            trim,
        );
    }
}

/// Exact mesh payload of the commercial assembly, also used by native visual QA.
pub fn geometry(b: &Bld, plan: &CommercialPlan, lod: Lod) -> CommercialMeshes {
    let mut output = CommercialMeshes::default();
    if lod == Lod::Far {
        return output;
    }
    let temple = is_named_temple(b);
    let (paint, trim, _) = colors(plan, temple);
    let sign = plan.fascia;
    let t = place(sign.p.x, sign.p.y, sign.p.z, sign.ry);
    let detailed = lod == Lod::Near;
    let rim = if plan.fascia_rim { 0.12 } else { 0.0 };
    // One deep rim body with an inset illuminated face, not four complete frame
    // cuboids. Front/back layers are deliberately separated to avoid z fighting.
    let body = Vec3::new(sign.w + rim, plan.fascia_height + rim, plan.fascia_depth);
    if plan.canopy.is_some() {
        // The canopy-front lightbox is suspended away from the wall. Its exposed
        // rear needs an opaque face so the emissive label never reads backwards.
        output.shell.add(
            Mat::VCol,
            &geom::cuboid(body.x, body.y, body.z),
            t * geom::at(0.0, 0.0, -plan.fascia_depth * 0.5),
            hex(paint),
        );
    } else {
        box_at(
            &mut output.shell,
            t,
            body,
            Vec3::new(0.0, 0.0, -plan.fascia_depth * 0.5),
            paint,
            true,
        );
    }
    sign_face(
        &mut output.shell,
        t * geom::at(0.0, 0.0, 0.008),
        sign.w,
        plan.fascia_height,
        plan.sign_index,
        temple,
    );
    if detailed && plan.fascia_rim {
        plane_at(
            &mut output.shell,
            t,
            sign.w + 0.05,
            0.025,
            Vec3::new(0.0, plan.fascia_height * 0.5 + 0.04, 0.012),
            trim,
        );
        for x in [-sign.w * 0.5 - 0.03, sign.w * 0.5 + 0.03] {
            plane_at(
                &mut output.shell,
                t,
                0.025,
                plan.fascia_height + 0.06,
                Vec3::new(x, 0.0, 0.012),
                trim,
            );
        }
    }
    for &panel in &plan.panels {
        panel_geometry(panel, plan, lod, temple, &mut output.shell);
        let t = place(
            panel.centre.x,
            0.0,
            panel.centre.y,
            panel.n.x.atan2(panel.n.y),
        );
        plane_at(
            &mut output.base,
            t,
            panel.width - 0.10,
            0.18,
            Vec3::new(0.0, 0.23, PANEL_Z + 0.057),
            paint,
        );
    }
    if let Some(canopy) = plan.canopy {
        canopy_geometry(canopy, plan, lod, temple, &mut output.shell);
    }
    if let Some(blade) = plan.blade {
        let bt = place(
            blade.centre.x,
            blade.centre.y,
            blade.centre.z,
            blade.normal.x.atan2(blade.normal.y),
        );
        // Thin axis is facade tangent, long axis projects outward. Each side has
        // its own outward-facing atlas plane, so its name is never mirrored.
        output.shell.add(
            Mat::VCol,
            &geom::cuboid(0.14, blade.height + 0.06, blade.width),
            bt,
            hex(paint),
        );
        for side in [-1.0, 1.0] {
            let face =
                bt * geom::at(side * 0.073, 0.0, 0.0) * Mat4::from_rotation_y(side * FRAC_PI_2);
            sign_face(
                &mut output.shell,
                face,
                blade.width - 0.09,
                blade.height - 0.025,
                blade.sign_index,
                temple,
            );
        }
        if detailed {
            for y in [-blade.height * 0.35, blade.height * 0.35] {
                box_at(
                    &mut output.shell,
                    bt,
                    Vec3::new(0.17, 0.045, 0.12),
                    Vec3::new(0.0, y, -blade.width * 0.5 + 0.035),
                    0x696e64,
                    true,
                );
            }
        }
    }
    if plan.secondary_fascia {
        // A suspended board under the street-side soffit remains readable in the
        // ordinary high camera; a board on the 3.6m-recessed wall is roof-occluded.
        // Both short hangers meet the actual existing 3.58m soffit underside.
        let centre = plan.frontage.centre() + plan.frontage.n * 0.025;
        let at = place(
            centre.x,
            3.16,
            centre.y,
            plan.frontage.n.x.atan2(plan.frontage.n.y),
        );
        let width = sign.w.min(3.2);
        // Its rear is now exposed to the arcade rather than buried in a wall.
        output.shell.add(
            Mat::VCol,
            &geom::cuboid(width + 0.10, 0.46, 0.085),
            at,
            hex(trim),
        );
        sign_face(
            &mut output.shell,
            at * geom::at(0.0, 0.0, 0.046),
            width,
            0.38,
            plan.sign_index,
            temple,
        );
        for x in [-width * 0.42, width * 0.42] {
            output.shell.add(
                Mat::VCol,
                &geom::cuboid(0.045, 0.21, 0.055),
                at * geom::at(x, 0.325, -0.06),
                hex(0x6a7269),
            );
        }
    }
    output
}

/// Called in the chunk building path; the roof merge survives map::build_chunk's
/// later assignment of Wave 1's separate facade structure.
pub fn building(b: &Bld, world: &WorldData, lod: Lod, out: &mut BuildingMeshes) -> bool {
    let Some(plan) = plan(b, world) else {
        return false;
    };
    let mesh = geometry(b, &plan, lod);
    out.base.merge(mesh.base);
    out.roof.merge(mesh.shell);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_temple_plaques_remove_shop_text_without_changing_commercial_payload() {
        let world = super::super::world();
        let mut checked = 0;
        for building in world.blds.iter().filter(|b| is_named_temple(b)) {
            let Some(plan) = plan(building, world) else {
                continue;
            };
            let mut generic = building.clone();
            generic.name.clear();
            let blank = super::super::textures::unbranded_sign_uv(plan.sign_index);
            for lod in [Lod::Near, Lod::Mid] {
                let temple = geometry(building, &plan, lod);
                let original = geometry(&generic, &plan, lod);
                for (temple, original) in [
                    (&temple.base, &original.base),
                    (&temple.shell, &original.shell),
                ] {
                    assert_eq!(temple.0.len(), original.0.len());
                    for (material, mesh) in &temple.0 {
                        let unchanged = &original.0[material];
                        assert_eq!(
                            mesh.pos, unchanged.pos,
                            "unchanged temple frontage envelope"
                        );
                        assert_eq!(mesh.idx, unchanged.idx, "unchanged temple geometry cost");
                    }
                }
                let plaque = &temple.shell.0[&Mat::Sign];
                assert!(plaque.uv.iter().all(|uv| *uv == blank));
                assert!(
                    original.shell.0[&Mat::Sign]
                        .uv
                        .windows(2)
                        .any(|pair| pair[0] != pair[1])
                );
                assert_eq!(plan.sign_index, building.sign.unwrap().idx);
            }
            checked += 1;
        }
        assert!(
            checked >= 3,
            "named production temples must use the override"
        );
    }

    fn within_box(point: Vec3, origin: Vec3, normal: Vec2, lo: Vec3, hi: Vec3) -> bool {
        let relative = point - origin;
        let tangent = Vec2::new(normal.y, -normal.x);
        let local = Vec3::new(
            relative.xz().dot(tangent),
            relative.y,
            relative.xz().dot(normal),
        );
        local.cmpge(lo - Vec3::splat(0.0002)).all() && local.cmple(hi + Vec3::splat(0.0002)).all()
    }

    /// Independent bounds of every emitted group, including trims and brackets.
    /// These use the public reservation metadata, not the placement acceptance
    /// predicate, so a longer bracket or added rim cannot silently escape it.
    fn in_reservation(point: Vec3, plan: &CommercialPlan) -> bool {
        let sign = plan.fascia;
        let rim = if plan.fascia_rim { 0.12 } else { 0.0 };
        let normal = Vec2::new(sign.ry.sin(), sign.ry.cos());
        if within_box(
            point,
            sign.p,
            normal,
            Vec3::new(
                -(sign.w + rim) * 0.5,
                -(plan.fascia_height + rim) * 0.5,
                -plan.fascia_depth,
            ),
            Vec3::new(
                (sign.w + rim) * 0.5,
                (plan.fascia_height + rim) * 0.5,
                0.013,
            ),
        ) {
            return true;
        }
        for panel in &plan.panels {
            if within_box(
                point,
                Vec3::new(panel.centre.x, 0.0, panel.centre.y),
                panel.n,
                Vec3::new(-panel.width * 0.5 - 0.01, 0.12, 0.14),
                Vec3::new(
                    panel.width * 0.5 + 0.01,
                    panel.height + 0.26,
                    PANEL_FRONT + 0.02,
                ),
            ) {
                return true;
            }
        }
        if let Some(canopy) = plan.canopy
            && within_box(
                point,
                Vec3::new(canopy.centre.x, 0.0, canopy.centre.y),
                canopy.n,
                Vec3::new(-canopy.width * 0.5 - 0.02, canopy.edge_y - 0.13, 0.14),
                Vec3::new(
                    canopy.width * 0.5 + 0.02,
                    canopy.mount_y + 0.02,
                    canopy.depth + 0.25,
                ),
            )
        {
            return true;
        }
        if let Some(blade) = plan.blade
            && within_box(
                point,
                blade.centre,
                blade.normal,
                Vec3::new(-0.09, -blade.height * 0.5 - 0.03, -blade.width * 0.5 - 0.03),
                Vec3::new(0.09, blade.height * 0.5 + 0.03, blade.width * 0.5),
            )
        {
            return true;
        }
        plan.secondary_fascia
            && within_box(
                point,
                Vec3::new(plan.frontage.centre().x, 3.16, plan.frontage.centre().y),
                plan.frontage.n,
                Vec3::new(-(sign.w.min(3.2) + 0.10) * 0.5, -0.23, -0.07),
                Vec3::new((sign.w.min(3.2) + 0.10) * 0.5, 0.43, 0.08),
            )
    }

    #[test]
    fn full_world_commercial_plans_preserve_shop_identity_and_clear_real_routes() {
        let world = super::super::world();
        let mut families = [0usize; 5];
        let mut panels = 0;
        let mut canopies = 0;
        let mut blades = 0;
        for b in &world.blds {
            let Some(plan) = plan(b, world) else { continue };
            families[plan.family as usize] += 1;
            assert_eq!(plan.sign_index, b.sign.unwrap().idx);
            assert_eq!(plan.family, family(b.sign.unwrap().idx));
            assert!(plan.panels.len() <= 4);
            for panel in &plan.panels {
                panels += 1;
                let corners = rect(
                    panel.centre,
                    panel.u,
                    panel.n,
                    panel.width + 0.02,
                    0.14,
                    PANEL_FRONT + 0.02,
                );
                assert!(
                    envelope_clear(b, world, &corners, plan.arcaded),
                    "panel envelope at {}",
                    b.id
                );
                assert!(
                    door_clear(world, &corners),
                    "panel over entrance at {}",
                    b.id
                );
                let d = b.door.unwrap();
                let offset = (panel.centre - d.p).dot(d.tangent());
                assert!(
                    b.walls
                        .iter()
                        .filter_map(|wall| wall_span(b, wall))
                        .any(|(lo, hi)| offset - panel.width * 0.5 >= lo + 0.02
                            && offset + panel.width * 0.5 <= hi - 0.02)
                );
            }
            if let Some(canopy) = plan.canopy {
                canopies += 1;
                assert!(!plan.arcaded);
                assert!(envelope_clear(
                    b,
                    world,
                    &rect(
                        canopy.centre,
                        canopy.u,
                        canopy.n,
                        canopy.width + 0.04,
                        0.14,
                        canopy.depth + 0.25
                    ),
                    false
                ));
            }
            if let Some(blade) = plan.blade {
                blades += 1;
                assert_eq!(blade.sign_index, plan.sign_index);
                assert!(blade.centre.y - blade.height * 0.5 - 0.03 > 2.6);
                let anchor = blade.centre.xz() - blade.normal * (0.16 + blade.width * 0.5);
                assert!(envelope_clear(
                    b,
                    world,
                    &rect(
                        anchor,
                        blade.tangent,
                        blade.normal,
                        0.18,
                        0.13,
                        0.16 + blade.width
                    ),
                    plan.arcaded
                ));
            }
            if plan.family == ShopFamily::Police {
                assert!(plan.canopy.is_none() && plan.blade.is_none());
            }
        }
        assert!(
            families.iter().all(|count| *count > 5),
            "family coverage: {families:?}"
        );
        assert!(
            panels > 100 && canopies > 40 && blades > 20,
            "panel/canopy/blade coverage: {panels}/{canopies}/{blades}"
        );
    }

    #[test]
    fn actual_commercial_meshes_have_bounded_valid_geometry_and_keep_mid_silhouettes() {
        let world = super::super::world();
        for b in &world.blds {
            let Some(plan) = plan(b, world) else { continue };
            let near = geometry(b, &plan, Lod::Near);
            let mid = geometry(b, &plan, Lod::Mid);
            assert!(
                near.base.vcount() + near.shell.vcount() <= MAX_NEAR_VERTICES,
                "near {}: {}",
                b.id,
                near.base.vcount() + near.shell.vcount()
            );
            assert!(
                mid.base.vcount() + mid.shell.vcount() <= MAX_MID_VERTICES,
                "mid {}: {}",
                b.id,
                mid.base.vcount() + mid.shell.vcount()
            );
            assert_eq!(geometry(b, &plan, Lod::Far).shell.vcount(), 0);
            assert!(near.base.0.keys().all(|mat| *mat == Mat::VCol));
            assert!(
                near.shell
                    .0
                    .keys()
                    .all(|mat| matches!(mat, Mat::VCol | Mat::Sign))
            );
            assert_eq!(
                near.shell.0[&Mat::Sign].vcount(),
                mid.shell.0[&Mat::Sign].vcount()
            );
            for mesh in [&near.shell.0[&Mat::Sign], &mid.shell.0[&Mat::Sign]] {
                for uv in &mesh.uv {
                    let column = (uv[0] * SIGN_COLS).floor() as usize;
                    let row = (uv[1] * SIGN_ROWS).floor() as usize;
                    assert_eq!(
                        row * SIGN_COLS as usize + column,
                        b.sign.unwrap().idx,
                        "emitted label changed identity at {}",
                        b.id
                    );
                }
            }
            let nearby_doors: Vec<_> = world
                .blds_in(b.min - 3.0, b.max + 3.0)
                .into_iter()
                .filter_map(|id| world.blds[id as usize].door.map(|door| (id, door)))
                .collect();
            for parts in [&near.base, &near.shell, &mid.base, &mid.shell] {
                for mesh in parts.0.values() {
                    assert_eq!(mesh.pos.len(), mesh.nrm.len());
                    assert_eq!(mesh.pos.len(), mesh.uv.len());
                    assert_eq!(mesh.pos.len(), mesh.col.len());
                    assert!(mesh.pos.iter().all(|p| Vec3::from(*p).is_finite()));
                    assert!(
                        mesh.nrm
                            .iter()
                            .all(|n| (Vec3::from(*n).length() - 1.0).abs() < 0.001)
                    );
                    assert!(mesh.idx.iter().all(|i| (*i as usize) < mesh.pos.len()));
                    for point in &mesh.pos {
                        assert!(
                            in_reservation(Vec3::from(*point), &plan),
                            "emitted geometry escaped admitted envelopes at {}: {point:?}",
                            b.id
                        );
                    }
                    for tri in mesh.idx.as_chunks::<3>().0 {
                        let [a, c, d] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
                        let cross = (Vec3::from(mesh.pos[c]) - Vec3::from(mesh.pos[a]))
                            .cross(Vec3::from(mesh.pos[d]) - Vec3::from(mesh.pos[a]));
                        assert!(cross.length_squared() > 1e-12, "degenerate at {}", b.id);
                        assert!(
                            cross.dot(Vec3::from(mesh.nrm[a])) > 0.0,
                            "inverted face at {}",
                            b.id
                        );
                        let y_min = mesh.pos[a][1].min(mesh.pos[c][1]).min(mesh.pos[d][1]);
                        let y_max = mesh.pos[a][1].max(mesh.pos[c][1]).max(mesh.pos[d][1]);
                        if y_min < 2.4 && y_max > 0.1 {
                            let polygon = [
                                Vec3::from(mesh.pos[a]).xz(),
                                Vec3::from(mesh.pos[c]).xz(),
                                Vec3::from(mesh.pos[d]).xz(),
                            ];
                            for (id, door) in &nearby_doors {
                                let passage =
                                    rect(door.p, door.tangent(), door.n, 2.30, -0.45, 1.20);
                                assert!(
                                    !overlap(&polygon, &passage, 0.0),
                                    "actual triangle of shop {} enters door {} leaf/player route",
                                    b.id,
                                    id
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn complete_envelopes_detect_crossing_roads_and_concave_notches() {
        let outer = [
            Vec2::new(-3.0, -1.0),
            Vec2::new(3.0, -1.0),
            Vec2::new(3.0, 1.0),
            Vec2::new(-3.0, 1.0),
        ];
        assert!(
            edge_distance(
                Vec2::new(0.0, -2.0),
                Vec2::new(0.0, 2.0),
                outer[0],
                outer[1]
            ) == 0.0
        );
        let notch = [
            Vec2::new(-4.0, -2.0),
            Vec2::new(4.0, -2.0),
            Vec2::new(4.0, 2.0),
            Vec2::new(0.4, 2.0),
            Vec2::new(0.4, 0.0),
            Vec2::new(-0.4, 0.0),
            Vec2::new(-0.4, 2.0),
            Vec2::new(-4.0, 2.0),
        ];
        assert!(outer.iter().all(|p| pip(*p, &notch)));
        assert!((0..4).any(|i| (0..notch.len()).any(|j| intersects(
            outer[i],
            outer[(i + 1) % 4],
            notch[j],
            notch[(j + 1) % notch.len()]
        ))));
    }
}
