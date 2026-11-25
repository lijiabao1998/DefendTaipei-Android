//! Small CPU mesh builder shared by the map and the 白面人 rig.
//!
//! The primitives follow three.js geometry conventions (sizes, segment layout, UVs, +Y up,
//! counter-clockwise front faces) so the original TypeScript generators port number-for-number:
//! `BoxGeometry`, `PlaneGeometry`, `CylinderGeometry`, `SphereGeometry`, `LatheGeometry`,
//! `TorusGeometry`, `CapsuleGeometry`, `ShapeGeometry`. Geometry is accumulated into one
//! [`MeshBuf`] per material and turned into a single Bevy [`Mesh`] (merged draw).

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use std::f32::consts::{PI, TAU};

/// three.js `Color.setHex` → linear RGBA (three converts sRGB hex to linear working space).
pub fn hex(c: u32) -> [f32; 4] {
    let l = |v: u32| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    [l((c >> 16) & 255), l((c >> 8) & 255), l(c & 255), 1.0]
}

/// Multiply a linear colour (three `Color.multiplyScalar`).
pub fn scale_col(c: [f32; 4], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

#[derive(Default, Clone)]
pub struct MeshBuf {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub col: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
}

/// three.js order: scale, rotateX, rotateY, rotateZ, translate.
pub fn trs(p: [f32; 3], r: [f32; 3], s: [f32; 3]) -> Mat4 {
    Mat4::from_translation(Vec3::from(p))
        * Mat4::from_rotation_z(r[2])
        * Mat4::from_rotation_y(r[1])
        * Mat4::from_rotation_x(r[0])
        * Mat4::from_scale(Vec3::from(s))
}

pub fn at(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
}

/// Rotate about Y by `ry`, then translate (the usual "place on the ground" transform).
pub fn place(x: f32, y: f32, z: f32, ry: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z)) * Mat4::from_rotation_y(ry)
}

impl MeshBuf {
    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }
    pub fn vcount(&self) -> usize {
        self.pos.len()
    }
    fn v(&mut self, p: [f32; 3], n: [f32; 3], uv: [f32; 2]) -> u32 {
        self.pos.push(p);
        self.nrm.push(n);
        self.uv.push(uv);
        self.col.push([1.0; 4]);
        (self.pos.len() - 1) as u32
    }
    /// Append `o` transformed by `m`, tinted by `c` (multiplies existing vertex colour).
    pub fn add(&mut self, o: &MeshBuf, m: Mat4, c: [f32; 4]) {
        let base = self.pos.len() as u32;
        let nm = Mat3::from_mat4(m).inverse().transpose();
        let flip = Mat3::from_mat4(m).determinant() < 0.0;
        for i in 0..o.pos.len() {
            self.pos
                .push(m.transform_point3(Vec3::from(o.pos[i])).into());
            self.nrm
                .push((nm * Vec3::from(o.nrm[i])).normalize_or_zero().into());
            self.uv.push(o.uv[i]);
            let oc = o.col[i];
            self.col
                .push([oc[0] * c[0], oc[1] * c[1], oc[2] * c[2], oc[3] * c[3]]);
        }
        if flip {
            for t in o.idx.chunks(3) {
                self.idx
                    .extend_from_slice(&[base + t[0], base + t[2], base + t[1]]);
            }
        } else {
            self.idx.extend(o.idx.iter().map(|i| base + i));
        }
    }
    pub fn add_white(&mut self, o: &MeshBuf, m: Mat4) {
        self.add(o, m, [1.0; 4]);
    }
    pub fn append(&mut self, o: MeshBuf) {
        self.add(&o, Mat4::IDENTITY, [1.0; 4]);
    }
    /// Per-vertex shade jitter like character-rig `Parts.add` (0.93 + 0.07·sin(i·12.9898)).
    pub fn jitter(mut self) -> Self {
        for (i, c) in self.col.iter_mut().enumerate() {
            let v = 0.93 + 0.07 * (i as f32 * 12.9898).sin();
            c[0] *= v;
            c[1] *= v;
            c[2] *= v;
        }
        self
    }
    pub fn transformed(self, m: Mat4) -> Self {
        let mut o = MeshBuf::default();
        o.add(&self, m, [1.0; 4]);
        o
    }
    pub fn tinted(mut self, c: [f32; 4]) -> Self {
        for k in self.col.iter_mut() {
            *k = [k[0] * c[0], k[1] * c[1], k[2] * c[2], k[3] * c[3]];
        }
        self
    }
    pub fn map_uv(mut self, f: impl Fn([f32; 2], [f32; 3]) -> [f32; 2]) -> Self {
        for (uv, p) in self.uv.iter_mut().zip(&self.pos) {
            *uv = f(*uv, *p);
        }
        self
    }
    /// Smooth normals from the triangles (curved primitives).
    pub fn smooth_normals(&mut self) {
        let mut acc = vec![Vec3::ZERO; self.pos.len()];
        for t in self.idx.chunks(3) {
            let a = Vec3::from(self.pos[t[0] as usize]);
            let b = Vec3::from(self.pos[t[1] as usize]);
            let c = Vec3::from(self.pos[t[2] as usize]);
            let n = (b - a).cross(c - a);
            for &i in t {
                acc[i as usize] += n;
            }
        }
        // weld normals of coincident vertices (lathe / sphere seams)
        let mut map: std::collections::HashMap<[i32; 3], Vec3> = Default::default();
        let key = |p: [f32; 3]| {
            [
                (p[0] * 1e4).round() as i32,
                (p[1] * 1e4).round() as i32,
                (p[2] * 1e4).round() as i32,
            ]
        };
        for (i, p) in self.pos.iter().enumerate() {
            *map.entry(key(*p)).or_default() += acc[i];
        }
        for (i, p) in self.pos.iter().enumerate() {
            let n = map[&key(*p)];
            self.nrm[i] = if n.length_squared() > 1e-20 {
                n.normalize().into()
            } else {
                [0.0, 1.0, 0.0]
            };
        }
    }
    pub fn into_mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
        .with_inserted_indices(Indices::U32(self.idx))
    }
    pub fn aabb(&self) -> (Vec3, Vec3) {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for p in &self.pos {
            lo = lo.min(Vec3::from(*p));
            hi = hi.max(Vec3::from(*p));
        }
        (lo, hi)
    }
}

// ---------------------------------------------------------------- primitives

/// three `BoxGeometry(w, h, d)` (1 segment per side), per-face 0..1 UVs.
pub fn cuboid(w: f32, h: f32, d: f32) -> MeshBuf {
    let mut m = MeshBuf::default();
    let (x, y, z) = (w / 2.0, h / 2.0, d / 2.0);
    // (normal, u axis, v axis, half sizes) — face order and orientation as three.js
    let faces: [([f32; 3], Vec3, Vec3, f32, f32, f32); 6] = [
        ([1.0, 0.0, 0.0], Vec3::NEG_Z, Vec3::Y, z, y, x),
        ([-1.0, 0.0, 0.0], Vec3::Z, Vec3::Y, z, y, x),
        ([0.0, 1.0, 0.0], Vec3::X, Vec3::NEG_Z, x, z, y),
        ([0.0, -1.0, 0.0], Vec3::X, Vec3::Z, x, z, y),
        ([0.0, 0.0, 1.0], Vec3::X, Vec3::Y, x, y, z),
        ([0.0, 0.0, -1.0], Vec3::NEG_X, Vec3::Y, x, y, z),
    ];
    for (n, u, v, hu, hv, hn) in faces {
        let c = Vec3::from(n) * hn;
        let b = m.pos.len() as u32;
        for (su, sv) in [(-1.0, 1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
            let p = c + u * hu * su + v * hv * sv;
            m.v(p.into(), n, [(su + 1.0) / 2.0, (sv + 1.0) / 2.0]);
        }
        m.idx
            .extend_from_slice(&[b, b + 2, b + 1, b + 2, b + 3, b + 1]);
    }
    m
}

/// three `PlaneGeometry(w, h)` in the XY plane facing +Z.
pub fn plane(w: f32, h: f32) -> MeshBuf {
    let mut m = MeshBuf::default();
    let n = [0.0, 0.0, 1.0];
    m.v([-w / 2.0, h / 2.0, 0.0], n, [0.0, 1.0]);
    m.v([w / 2.0, h / 2.0, 0.0], n, [1.0, 1.0]);
    m.v([-w / 2.0, -h / 2.0, 0.0], n, [0.0, 0.0]);
    m.v([w / 2.0, -h / 2.0, 0.0], n, [1.0, 0.0]);
    m.idx.extend_from_slice(&[0, 2, 1, 2, 3, 1]);
    m
}

/// Two-sided version of [`plane`] (three `side: DoubleSide`).
pub fn plane2(w: f32, h: f32) -> MeshBuf {
    let mut m = plane(w, h);
    let back = plane(w, h).transformed(Mat4::from_rotation_y(PI));
    m.append(back);
    m
}

/// three `CylinderGeometry(rt, rb, h, seg, 1, open, thetaStart, thetaLength)`, centred.
pub fn cylinder_ex(rt: f32, rb: f32, h: f32, seg: usize, open: bool, t0: f32, tl: f32) -> MeshBuf {
    let mut m = MeshBuf::default();
    let slope = (rb - rt) / h;
    for row in 0..2 {
        let v = row as f32;
        let r = if row == 0 { rt } else { rb };
        let y = h / 2.0 - v * h;
        for i in 0..=seg {
            let u = i as f32 / seg as f32;
            let th = t0 + u * tl;
            let (s, c) = th.sin_cos();
            let n = Vec3::new(s, slope, c).normalize();
            m.v([r * s, y, r * c], n.into(), [u, 1.0 - v]);
        }
    }
    let w = seg as u32 + 1;
    for i in 0..seg as u32 {
        let (a, b, c, d) = (i, i + w, i + w + 1, i + 1);
        m.idx.extend_from_slice(&[a, b, d, b, c, d]);
    }
    if !open {
        for top in [true, false] {
            let (r, y, ny) = if top {
                (rt, h / 2.0, 1.0)
            } else {
                (rb, -h / 2.0, -1.0)
            };
            if r <= 0.0 {
                continue;
            }
            let centre = m.v([0.0, y, 0.0], [0.0, ny, 0.0], [0.5, 0.5]);
            let start = m.pos.len() as u32;
            for i in 0..=seg {
                let th = t0 + i as f32 / seg as f32 * tl;
                let (s, c) = th.sin_cos();
                m.v(
                    [r * s, y, r * c],
                    [0.0, ny, 0.0],
                    [c * 0.5 + 0.5, s * 0.5 * ny + 0.5],
                );
            }
            for i in 0..seg as u32 {
                if top {
                    m.idx.extend_from_slice(&[start + i, start + i + 1, centre]);
                } else {
                    m.idx.extend_from_slice(&[start + i + 1, start + i, centre]);
                }
            }
        }
    }
    m
}

pub fn cylinder(rt: f32, rb: f32, h: f32, seg: usize) -> MeshBuf {
    cylinder_ex(rt, rb, h, seg, false, 0.0, TAU)
}

/// character-rig `cyl(r0, r1, len)`: three `CylinderGeometry(r1, r0, len)` hung below the joint.
pub fn cyl(r0: f32, r1: f32, len: f32, seg: usize) -> MeshBuf {
    cylinder(r1, r0, len, seg).transformed(at(0.0, -len / 2.0, 0.0))
}

/// three `SphereGeometry(r, ws, hs, phiStart, phiLength, thetaStart, thetaLength)`.
pub fn sphere_ex(r: f32, ws: usize, hs: usize, p0: f32, pl: f32, t0: f32, tl: f32) -> MeshBuf {
    let mut m = MeshBuf::default();
    for iy in 0..=hs {
        let v = iy as f32 / hs as f32;
        for ix in 0..=ws {
            let u = ix as f32 / ws as f32;
            let phi = p0 + u * pl;
            let th = t0 + v * tl;
            let p = Vec3::new(
                -r * phi.cos() * th.sin(),
                r * th.cos(),
                r * phi.sin() * th.sin(),
            );
            m.v(p.into(), p.normalize_or_zero().into(), [u, 1.0 - v]);
        }
    }
    let w = ws as u32 + 1;
    for iy in 0..hs as u32 {
        for ix in 0..ws as u32 {
            let a = iy * w + ix + 1;
            let b = iy * w + ix;
            let c = (iy + 1) * w + ix;
            let d = (iy + 1) * w + ix + 1;
            if iy != 0 || t0 > 0.0 {
                m.idx.extend_from_slice(&[a, b, d]);
            }
            if iy != hs as u32 - 1 || t0 + tl < PI {
                m.idx.extend_from_slice(&[b, c, d]);
            }
        }
    }
    m
}

pub fn sphere(r: f32, ws: usize, hs: usize) -> MeshBuf {
    sphere_ex(r, ws, hs, 0.0, TAU, 0.0, PI)
}

/// three `LatheGeometry(points (r, y), seg, phiStart, phiLength)`.
pub fn lathe(pts: &[[f32; 2]], seg: usize, p0: f32, pl: f32) -> MeshBuf {
    let mut m = MeshBuf::default();
    for i in 0..=seg {
        let phi = p0 + i as f32 / seg as f32 * pl;
        let (s, c) = phi.sin_cos();
        for (j, p) in pts.iter().enumerate() {
            m.v(
                [p[0] * s, p[1], p[0] * c],
                [0.0, 1.0, 0.0],
                [i as f32 / seg as f32, j as f32 / (pts.len() - 1) as f32],
            );
        }
    }
    let n = pts.len() as u32;
    for i in 0..seg as u32 {
        for j in 0..n - 1 {
            let base = j + i * n;
            let (a, b, c, d) = (base, base + n, base + n + 1, base + 1);
            m.idx.extend_from_slice(&[a, b, d, c, d, b]);
        }
    }
    m.smooth_normals();
    m
}

/// three `TorusGeometry(R, tube, radialSeg, tubularSeg, arc)` in the XY plane.
pub fn torus(rr: f32, tube: f32, rs: usize, ts: usize, arc: f32) -> MeshBuf {
    let mut m = MeshBuf::default();
    for j in 0..=rs {
        for i in 0..=ts {
            let u = i as f32 / ts as f32 * arc;
            let v = j as f32 / rs as f32 * TAU;
            let p = Vec3::new(
                (rr + tube * v.cos()) * u.cos(),
                (rr + tube * v.cos()) * u.sin(),
                tube * v.sin(),
            );
            let c = Vec3::new(rr * u.cos(), rr * u.sin(), 0.0);
            m.v(
                p.into(),
                (p - c).normalize_or_zero().into(),
                [i as f32 / ts as f32, j as f32 / rs as f32],
            );
        }
    }
    let w = ts as u32 + 1;
    for j in 1..=rs as u32 {
        for i in 1..=ts as u32 {
            let a = w * j + i - 1;
            let b = w * (j - 1) + i - 1;
            let c = w * (j - 1) + i;
            let d = w * j + i;
            m.idx.extend_from_slice(&[a, b, d, b, c, d]);
        }
    }
    m
}

/// three `CapsuleGeometry(r, len, capSeg, radialSeg)` along Y (total height len + 2r).
pub fn capsule(r: f32, len: f32, cap_seg: usize, radial: usize) -> MeshBuf {
    // profile: bottom hemisphere, straight, top hemisphere — built as a lathe
    let mut prof = Vec::new();
    for i in 0..=cap_seg {
        let a = -PI / 2.0 + i as f32 / cap_seg as f32 * PI / 2.0;
        prof.push([r * a.cos(), -len / 2.0 + r * a.sin()]);
    }
    for i in 0..=cap_seg {
        let a = i as f32 / cap_seg as f32 * PI / 2.0;
        prof.push([r * a.cos(), len / 2.0 + r * a.sin()]);
    }
    lathe(&prof, radial, 0.0, TAU)
}

/// Polygon cap (three `ShapeGeometry`) in the XY plane facing +Z; `pts` any winding.
pub fn shape(pts: &[Vec2]) -> MeshBuf {
    let mut m = MeshBuf::default();
    let tris = triangulate(pts);
    for p in pts {
        m.v([p.x, p.y, 0.0], [0.0, 0.0, 1.0], [p.x, p.y]);
    }
    m.idx = tris;
    m
}

/// Signed area (CCW positive in x-right / y-up).
pub fn signed_area(p: &[Vec2]) -> f32 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u.x * v.y - v.x * u.y;
    }
    a / 2.0
}

/// Ear-clipping triangulation; returns CCW triangles as indices into `pts`.
pub fn triangulate(pts: &[Vec2]) -> Vec<u32> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let ccw = signed_area(pts) > 0.0;
    let mut v: Vec<usize> = if ccw {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };
    let mut out = Vec::new();
    let cross = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
    let mut guard = 0;
    while v.len() > 3 && guard < n * n {
        guard += 1;
        let m = v.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (v[(i + m - 1) % m], v[i], v[(i + 1) % m]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            if cross(a, b, c) <= 1e-9 {
                continue;
            }
            let inside = v.iter().any(|&k| {
                if k == ia || k == ib || k == ic {
                    return false;
                }
                let p = pts[k];
                cross(a, b, p) > 0.0 && cross(b, c, p) > 0.0 && cross(c, a, p) > 0.0
            });
            if inside {
                continue;
            }
            out.extend_from_slice(&[ia as u32, ib as u32, ic as u32]);
            v.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            // degenerate remainder: fan it
            for i in 1..v.len() - 1 {
                out.extend_from_slice(&[v[0] as u32, v[i] as u32, v[i + 1] as u32]);
            }
            return out;
        }
    }
    if v.len() == 3 {
        out.extend_from_slice(&[v[0] as u32, v[1] as u32, v[2] as u32]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangulates_concave_l() {
        let l = [
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(2.0, 1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(1.0, 2.0),
            Vec2::new(0.0, 2.0),
        ];
        let t = triangulate(&l);
        assert_eq!(t.len(), 12);
        let area: f32 = t
            .chunks(3)
            .map(|c| {
                let (a, b, d) = (l[c[0] as usize], l[c[1] as usize], l[c[2] as usize]);
                (b - a).perp_dot(d - a) / 2.0
            })
            .sum();
        assert!((area - 3.0).abs() < 1e-4);
    }

    #[test]
    fn box_has_24_verts() {
        let b = cuboid(1.0, 2.0, 3.0);
        assert_eq!(b.vcount(), 24);
        assert_eq!(b.idx.len(), 36);
        let (lo, hi) = b.aabb();
        assert_eq!(hi - lo, Vec3::new(1.0, 2.0, 3.0));
    }
}
