//! 白面人 model: a port of `makeRig` (src/npc/human.ts) and the character-rig kit — the
//! faceless glossy mannequin, built from lathes / cylinders / spheres / boxes per joint, with the
//! soldier gear (plate carrier, pouches, radio, helmet, goggles) and the profession outfits
//! (office, student, courier, nurse, tech), plus the firearms.ts gun models.
//!
//! Geometry is merged to one mesh per joint and material (skin / cloth), like `Parts.build`.

use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

use crate::geom::{self, MeshBuf, hex, scale_col, trs};

/// Joints, in hierarchy order (a parent always comes first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum J {
    Body,
    Hips,
    Spine,
    Chest,
    Neck,
    Head,
    UaL,
    LaL,
    HandL,
    UaR,
    LaR,
    HandR,
    ThL,
    ShL,
    FtL,
    ThR,
    ShR,
    FtR,
    Weapon,
    GunInner,
}

pub const JOINTS: usize = 20;

pub const SKIN: u32 = 0xf1eee8;

#[derive(Clone, Debug, PartialEq)]
pub struct RigOpts {
    pub female: bool,
    pub top: u32,
    pub bottom: u32,
    pub shoes: u32,
    pub soldier: bool,
    pub jacket: Option<u32>,
    pub pack: Option<u32>,
    pub outfit: &'static str,
    /// Gun id held (firearms.ts spec), "" for none.
    pub gun: &'static str,
}

impl RigOpts {
    /// SOLDIER_OPTS of main.ts.
    pub fn soldier(gun: &'static str) -> Self {
        Self {
            female: false,
            top: 0x55603c,
            bottom: 0x4a5434,
            shoes: 0x1a1a1a,
            soldier: true,
            jacket: None,
            pack: None,
            outfit: "",
            gun,
        }
    }
    /// A cache key for the built meshes.
    pub fn key(&self) -> String {
        format!(
            "{}{:06x}{:06x}{:06x}{}{:?}{:?}{}{}",
            self.female as u8,
            self.top,
            self.bottom,
            self.shoes,
            self.soldier as u8,
            self.jacket,
            self.pack,
            self.outfit,
            self.gun
        )
    }
}

/// Built model: joint rest offsets (parent-relative), geometry per joint, leg lengths.
pub struct RigModel {
    pub parent: [Option<J>; JOINTS],
    pub offset: [Vec3; JOINTS],
    /// Rest rotation (weapon pitch).
    pub rest_rot: [Quat; JOINTS],
    pub skin: Vec<(J, MeshBuf)>,
    pub cloth: Vec<(J, MeshBuf)>,
    pub gun: Option<MeshBuf>,
    pub muzzle: Vec3,
    pub lt: f32,
    pub ls: f32,
}

/// shared mannequin proportions (character-rig `bodyDims`).
pub fn body_dims(female: bool) -> (f32, f32, f32) {
    let k = if female { 0.95 } else { 1.0 };
    let (lt, ls) = (0.45 * k, 0.43 * k);
    (lt, ls, lt + ls + 0.075)
}

struct Kit {
    skin: Vec<MeshBuf>,
    cloth: Vec<MeshBuf>,
    /// LOD detail is merged in too (phones get the whole mannequin; the detail is tiny).
    skin_c: [f32; 4],
}

impl Kit {
    fn put(v: &mut [MeshBuf], j: J, g: MeshBuf, c: u32, p: [f32; 3], r: [f32; 3], s: [f32; 3]) {
        let g = g.tinted(hex(c)).jitter();
        v[j as usize].add(&g, trs(p, r, s), [1.0; 4]);
    }
    fn cl(&mut self, j: J, g: MeshBuf, c: u32, p: [f32; 3], r: [f32; 3], s: [f32; 3]) {
        Self::put(&mut self.cloth, j, g, c, p, r, s);
    }
    fn c0(&mut self, j: J, g: MeshBuf, c: u32) {
        self.cl(j, g, c, [0.0; 3], [0.0; 3], [1.0; 3]);
    }
    fn cp(&mut self, j: J, g: MeshBuf, c: u32, p: [f32; 3]) {
        self.cl(j, g, c, p, [0.0; 3], [1.0; 3]);
    }
    fn sk(&mut self, j: J, g: MeshBuf, p: [f32; 3], r: [f32; 3], s: [f32; 3]) {
        let g = g.tinted(self.skin_c).jitter();
        self.skin[j as usize].add(&g, trs(p, r, s), [1.0; 4]);
    }
    fn ring(&mut self, j: J, r: f32, y: f32, c: u32, sz: [f32; 3]) {
        self.cl(
            j,
            geom::torus(r, 0.006, 4, 20, TAU),
            c,
            [0.0, y, 0.0],
            [PI / 2.0, 0.0, 0.0],
            sz,
        );
    }
}

fn lathe(pts: &[[f32; 2]], seg: usize) -> MeshBuf {
    geom::lathe(pts, seg, 0.0, TAU)
}
fn cyl(r0: f32, r1: f32, len: f32, seg: usize) -> MeshBuf {
    geom::cyl(r0, r1, len, seg)
}
fn sph(r: f32) -> MeshBuf {
    geom::sphere(r, 12, 9)
}
fn sphn(r: f32, w: usize, h: usize) -> MeshBuf {
    geom::sphere(r, w, h)
}
fn bx(x: f32, y: f32, z: f32) -> MeshBuf {
    geom::cuboid(x, y, z)
}
fn cap(r: f32, l: f32) -> MeshBuf {
    geom::capsule(r, l, 3, 8)
}
fn dk(c: u32, k: f32) -> u32 {
    // three Color(c).multiplyScalar(k).getHex(): linear scale, back to sRGB hex
    let l = scale_col(hex(c), k);
    let s = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let e = if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (e * 255.0).round() as u32
    };
    (s(l[0]) << 16) | (s(l[1]) << 8) | s(l[2])
}
fn light(c: u32) -> f32 {
    let [r, g, b, _] = [
        ((c >> 16) & 255) as f32 / 255.0,
        ((c >> 8) & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
        0.0,
    ];
    (r.max(g).max(b) + r.min(g).min(b)) / 2.0
}

const NO: [f32; 3] = [0.0; 3];
const ONE: [f32; 3] = [1.0; 3];

pub fn make_rig(o: &RigOpts) -> RigModel {
    let f = o.female;
    let s = o.soldier;
    let (lt, ls, hip_h) = body_dims(f);
    let mut parent = [None; JOINTS];
    let mut offset = [Vec3::ZERO; JOINTS];
    let mut jt = |j: J, p: Option<J>, x: f32, y: f32, z: f32| {
        parent[j as usize] = p;
        offset[j as usize] = Vec3::new(x, y, z);
    };
    let mut k = Kit {
        skin: vec![MeshBuf::default(); JOINTS],
        cloth: vec![MeshBuf::default(); JOINTS],
        skin_c: hex(SKIN),
    };
    use J::*;
    jt(Body, None, 0.0, 0.0, 0.0);
    jt(Hips, Some(Body), 0.0, hip_h, 0.0);
    // pelvis / pants seat
    let pelvis: &[[f32; 2]] = if f {
        &[
            [0.0, -0.1],
            [0.12, -0.09],
            [0.165, -0.02],
            [0.155, 0.06],
            [0.13, 0.1],
        ]
    } else {
        &[
            [0.0, -0.1],
            [0.12, -0.09],
            [0.155, -0.02],
            [0.15, 0.06],
            [0.14, 0.1],
        ]
    };
    k.cl(Hips, lathe(pelvis, 14), o.bottom, NO, NO, [1.0, 1.0, 0.68]);
    k.cl(
        Hips,
        cyl(0.145, 0.145, 0.035, 14),
        if s { 0x2a2a1e } else { 0x2a2420 },
        [0.0, 0.1, 0.0],
        NO,
        [1.0, 1.0, 0.7],
    ); // belt
    if !s {
        k.cp(Hips, bx(0.045, 0.032, 0.012), 0xb8a070, [0.0, 0.1, 0.103]); // buckle
        for x in [-0.065, 0.065] {
            k.cl(
                Hips,
                bx(0.075, 0.085, 0.008),
                o.bottom,
                [x, 0.03, -0.1],
                [0.15, 0.0, 0.0],
                ONE,
            );
        }
        k.cp(Hips, bx(0.006, 0.09, 0.006), 0x1c1a18, [0.012, 0.03, 0.1]);
        for x in [-0.07f32, 0.07] {
            k.cl(
                Hips,
                bx(0.06, 0.006, 0.01),
                0x1c1a18,
                [x, 0.075, 0.095],
                [0.0, 0.0, x * 6.0],
                ONE,
            );
        }
    } else {
        for x in [-0.11f32, 0.0, 0.11] {
            k.cp(
                Hips,
                bx(0.06, 0.07, 0.04),
                0x3c4428,
                [x, 0.07, if x == 0.0 { -0.11 } else { 0.1 }],
            );
        }
    }
    jt(Spine, Some(Hips), 0.0, 0.09, 0.0);
    let torso: Vec<[f32; 2]> = if f {
        vec![
            [0.125, 0.0],
            [0.11, 0.1],
            [0.135, 0.22],
            [0.155, 0.3],
            [0.16, 0.36],
            [0.14, 0.42],
            [0.07, 0.45],
        ]
    } else {
        vec![
            [0.14, 0.0],
            [0.132, 0.1],
            [0.155, 0.22],
            [0.18, 0.32],
            [0.19, 0.38],
            [0.165, 0.43],
            [0.07, 0.46],
        ]
    };
    k.cl(Spine, lathe(&torso, 16), o.top, NO, NO, [1.0, 1.0, 0.62]);
    jt(Chest, Some(Spine), 0.0, 0.36, 0.0);
    if f {
        for x in [-0.06, 0.06] {
            k.cl(
                Chest,
                sph(0.075),
                o.top,
                [x, -0.06, 0.07],
                NO,
                [1.0, 0.85, 0.75],
            );
        }
    }
    k.cl(
        Chest,
        sphn(1.0, 14, 8),
        o.top,
        [0.0, 0.03, 0.0],
        NO,
        [if f { 0.165 } else { 0.2 }, 0.075, 0.115],
    ); // shoulders
    if let Some(jc) = o.jacket {
        let jp: Vec<[f32; 2]> = torso.iter().take(6).map(|p| [p[0] + 0.018, p[1]]).collect();
        k.cl(
            Spine,
            geom::lathe(&jp, 16, 0.45, TAU - 0.9),
            jc,
            NO,
            NO,
            [1.0, 1.0, 0.66],
        );
        k.cl(
            Chest,
            cyl(0.1, 0.12, 0.05, 14),
            jc,
            [0.0, 0.1, -0.01],
            NO,
            [1.0, 1.0, 0.75],
        ); // collar
        for sd in [-1.0f32, 1.0] {
            k.cl(
                Chest,
                bx(0.035, 0.17, 0.012),
                jc,
                [sd * 0.062, -0.02, 0.115],
                [-0.12, 0.0, -sd * 0.3],
                ONE,
            );
            k.cl(
                Spine,
                bx(0.07, 0.05, 0.012),
                jc,
                [sd * 0.09, 0.14, 0.105],
                [0.0, sd * 0.2, 0.0],
                ONE,
            );
        }
    } else {
        k.cl(
            Chest,
            cyl(0.075, 0.085, 0.03, 12),
            o.top,
            [0.0, 0.09, 0.0],
            NO,
            [1.0, 1.0, 0.8],
        );
    }
    if s {
        // plate carrier + pouches + radio
        k.cp(Chest, bx(0.34, 0.3, 0.06), 0x39412a, [0.0, -0.13, 0.11]);
        k.cp(Chest, bx(0.34, 0.32, 0.06), 0x39412a, [0.0, -0.13, -0.12]);
        for x in [-0.11, 0.0, 0.11] {
            k.cp(Chest, bx(0.085, 0.11, 0.05), 0x46502f, [x, -0.2, 0.155]);
        }
        k.cp(Chest, bx(0.06, 0.06, 0.03), 0x8a7a40, [0.12, -0.02, 0.15]);
        k.cp(Chest, bx(0.07, 0.13, 0.05), 0x222222, [-0.15, -0.05, -0.15]);
        k.cp(
            Chest,
            cyl(0.006, 0.004, 0.35, 4),
            0x111111,
            [-0.16, 0.3, -0.16],
        );
        k.cp(Chest, bx(0.25, 0.22, 0.1), 0x3a422b, [0.0, -0.12, -0.2]); // hydration pack
    }
    if let Some(pc) = o.pack {
        k.cp(Chest, bx(0.3, 0.38, 0.16), pc, [0.0, -0.14, -0.2]);
        k.cp(Chest, bx(0.26, 0.12, 0.06), pc, [0.0, -0.24, -0.3]);
        k.cp(Chest, bx(0.24, 0.05, 0.14), 0x222222, [0.0, 0.06, -0.2]);
        for x in [-0.1, 0.1] {
            k.cl(
                Chest,
                bx(0.04, 0.36, 0.015),
                0x222222,
                [x, -0.12, 0.105],
                [-0.1, 0.0, 0.0],
                ONE,
            );
            k.cp(Chest, bx(0.04, 0.015, 0.2), 0x222222, [x, 0.07, -0.02]);
        }
    }
    jt(Neck, Some(Chest), 0.0, 0.08, 0.0);
    k.sk(Neck, cyl(0.05, 0.042, 0.11, 12), [0.0, 0.095, 0.0], NO, ONE);
    jt(Head, Some(Neck), 0.0, 0.09, 0.0);
    // egg head: one smooth lathe, wider crown tapering to a soft chin, no features
    let head: &[[f32; 2]] = &[
        [0.0, -0.03],
        [0.04, -0.022],
        [0.068, 0.01],
        [0.084, 0.06],
        [0.091, 0.11],
        [0.086, 0.16],
        [0.066, 0.2],
        [0.034, 0.222],
        [0.0, 0.228],
    ];
    k.sk(
        Head,
        lathe(head, 22),
        NO,
        [-0.08, 0.0, 0.0],
        [0.92, 0.9, 1.04],
    );
    if s {
        k.cl(
            Head,
            geom::sphere_ex(0.118, 16, 8, 0.0, TAU, 0.0, PI * 0.52),
            0x4c5733,
            [0.0, 0.1, -0.01],
            NO,
            [0.98, 0.95, 1.08],
        );
        k.cl(
            Head,
            geom::torus(0.115, 0.01, 4, 18, TAU),
            0x3e4828,
            [0.0, 0.105, -0.01],
            [PI / 2.0, 0.0, 0.0],
            [0.98, 1.08, 1.0],
        );
        k.cp(Head, bx(0.15, 0.035, 0.05), 0x1c1c1c, [0.0, 0.135, 0.095]);
        k.cp(Head, bx(0.2, 0.012, 0.02), 0x1c1c1c, [0.0, 0.13, 0.03]); // goggles on helmet
        k.cp(Head, bx(0.012, 0.09, 0.012), 0x222222, [-0.085, 0.04, 0.02]);
        k.cp(Head, bx(0.012, 0.09, 0.012), 0x222222, [0.085, 0.04, 0.02]); // chin strap
    }
    // arms (+x is the character's left)
    let arm_c = if s { o.top } else { o.jacket.unwrap_or(o.top) };
    for (sd, ua, la, hand) in [(1.0f32, UaL, LaL, HandL), (-1.0, UaR, LaR, HandR)] {
        jt(ua, Some(Chest), sd * if f { 0.165 } else { 0.2 }, 0.0, 0.0);
        k.cp(
            ua,
            sph(if f { 0.05 } else { 0.06 }),
            arm_c,
            [0.0, -0.01, 0.0],
        );
        k.cl(
            ua,
            cyl(if f { 0.05 } else { 0.058 }, 0.046, 0.28, 10),
            arm_c,
            NO,
            NO,
            [1.0, 1.0, 0.92],
        );
        if s {
            k.cp(ua, bx(0.05, 0.05, 0.01), 0x8a7a40, [sd * 0.058, -0.06, 0.0]);
        }
        jt(la, Some(ua), 0.0, -0.28, 0.0);
        let long = s || o.jacket.is_some();
        let d = dk(arm_c, 0.72);
        if long {
            k.c0(la, sphn(0.046, 10, 8), arm_c);
            k.c0(la, cyl(0.046, 0.04, 0.22, 10), arm_c);
            k.cp(la, cyl(0.043, 0.043, 0.025, 10), arm_c, [0.0, -0.2, 0.0]);
            k.sk(la, cyl(0.032, 0.03, 0.05, 8), [0.0, -0.21, 0.0], NO, ONE);
            for y in [-0.01, 0.02] {
                k.ring(la, 0.047, y, d, ONE);
            }
        } else {
            k.cp(la, cyl(0.05, 0.048, 0.04, 10), arm_c, [0.0, 0.02, 0.0]);
            k.sk(la, sphn(0.041, 10, 8), NO, NO, ONE);
            k.sk(la, cyl(0.04, 0.03, 0.26, 10), NO, NO, ONE);
        }
        jt(hand, Some(la), 0.0, -0.26, 0.0);
        if s {
            k.cl(
                hand,
                sph(0.04),
                0x252520,
                [0.0, -0.04, 0.005],
                NO,
                [0.85, 1.2, 1.0],
            );
        } else {
            k.sk(
                hand,
                sphn(0.036, 10, 8),
                [0.0, -0.035, 0.003],
                NO,
                [0.62, 1.15, 1.0],
            );
            for i in 0..4 {
                k.sk(
                    hand,
                    cap(0.0085, 0.045),
                    [0.0, -0.085, 0.024 - i as f32 * 0.015],
                    [0.0, 0.0, sd * 0.06],
                    ONE,
                );
            }
            k.sk(
                hand,
                cap(0.009, 0.035),
                [sd * 0.005, -0.045, 0.04],
                [0.6, 0.0, 0.0],
                ONE,
            );
        }
    }
    // legs
    for (sd, th, sh, ft) in [(1.0f32, ThL, ShL, FtL), (-1.0, ThR, ShR, FtR)] {
        jt(
            th,
            Some(Hips),
            sd * if f { 0.088 } else { 0.085 },
            -0.03,
            0.0,
        );
        k.c0(
            th,
            cyl(if f { 0.08 } else { 0.078 }, 0.056, lt, 12),
            o.bottom,
        );
        jt(sh, Some(th), 0.0, -lt, 0.0);
        k.c0(sh, sph(0.056), o.bottom);
        k.c0(sh, cyl(0.055, 0.044, ls, 10), o.bottom);
        if !s {
            k.cp(
                sh,
                cyl(0.05, 0.056, 0.05, 12),
                o.bottom,
                [0.0, -ls + 0.06, 0.0],
            );
            let d = dk(o.bottom, 0.7);
            for y in [0.03, -0.02] {
                k.ring(sh, 0.057, y, d, ONE);
            }
            k.ring(sh, 0.052, -ls + 0.1, d, ONE);
            k.cp(
                th,
                bx(0.005, lt * 0.9, 0.006),
                d,
                [sd * 0.072, -lt / 2.0, 0.0],
            );
        } else {
            k.cp(sh, bx(0.09, 0.1, 0.04), 0x2a2e20, [0.0, -0.02, 0.05]);
            k.cp(
                sh,
                cyl(0.05, 0.048, 0.14, 10),
                0x1a1a1a,
                [0.0, -ls + 0.13, 0.0],
            );
        }
        jt(ft, Some(sh), 0.0, -ls, 0.0);
        k.cp(
            ft,
            bx(0.1, 0.03, 0.26),
            if s { 0x111111 } else { 0x3a3a3a },
            [0.0, -0.06, 0.045],
        );
        k.cl(
            ft,
            cap(0.045, 0.14),
            o.shoes,
            [0.0, -0.03, 0.05],
            [PI / 2.0, 0.0, 0.0],
            [1.0, 1.0, 0.75],
        );
        if !s && light(o.shoes) > 0.7 {
            k.cp(ft, bx(0.098, 0.012, 0.265), 0xe8e4da, [0.0, -0.044, 0.045]);
            for i in 0..3 {
                let i = i as f32;
                k.cp(
                    ft,
                    bx(0.05, 0.005, 0.008),
                    0xf0f0f0,
                    [0.0, -0.005 - i * 0.004, 0.06 + i * 0.025],
                );
            }
        }
    }
    outfit(&mut k, o, &torso, ls);
    // weapon pivot in the right hand
    jt(Weapon, Some(HandR), 0.0, 0.0, 0.0);
    jt(GunInner, Some(Weapon), 0.0, 0.0, 0.0);
    let mut rest_rot = [Quat::IDENTITY; JOINTS];
    rest_rot[Weapon as usize] = Quat::from_rotation_x(PI / 2.0);
    let (gun, muzzle) = gun_model(o.gun).map_or((None, Vec3::ZERO), |(g, m)| (Some(g), m));
    let take = |v: Vec<MeshBuf>| -> Vec<(J, MeshBuf)> {
        v.into_iter()
            .enumerate()
            .filter(|(_, m)| !m.is_empty())
            .map(|(i, m)| (JOINT_LIST[i], m))
            .collect()
    };
    RigModel {
        parent,
        offset,
        rest_rot,
        skin: take(k.skin),
        cloth: take(k.cloth),
        gun,
        muzzle,
        lt,
        ls,
    }
}

pub const JOINT_LIST: [J; JOINTS] = [
    J::Body,
    J::Hips,
    J::Spine,
    J::Chest,
    J::Neck,
    J::Head,
    J::UaL,
    J::LaL,
    J::HandL,
    J::UaR,
    J::LaR,
    J::HandR,
    J::ThL,
    J::ShL,
    J::FtL,
    J::ThR,
    J::ShR,
    J::FtR,
    J::Weapon,
    J::GunInner,
];

/// Profession outfits: detail only through clothing.
fn outfit(k: &mut Kit, o: &RigOpts, torso: &[[f32; 2]], ls: f32) {
    use J::*;
    let f = o.female;
    let seam = 0x1c1a18;
    let skirt = |k: &mut Kit, c: u32, len: f32, pleats: bool| {
        k.cl(
            Hips,
            lathe(&[[0.15, 0.08], [0.17, 0.0], [0.2, -0.12], [0.23, -len]], 18),
            c,
            NO,
            NO,
            [1.0, 1.0, 0.75],
        );
        if pleats {
            for i in 0..14 {
                let a = i as f32 / 14.0 * TAU;
                k.cl(
                    Hips,
                    bx(0.012, len * 0.85, 0.02),
                    dk(c, 0.72),
                    [a.sin() * 0.2, -len * 0.5, a.cos() * 0.2 * 0.75],
                    [a.cos() * -0.18, a, 0.0],
                    ONE,
                );
            }
        }
    };
    let collar = |k: &mut Kit, c: u32| {
        for sd in [-1.0f32, 1.0] {
            k.cl(
                Chest,
                bx(0.07, 0.012, 0.06),
                c,
                [sd * 0.045, 0.085, 0.07],
                [0.5, sd * 0.5, sd * 0.3],
                ONE,
            );
        }
    };
    let badge = |k: &mut Kit, y: f32, x: f32| {
        k.cp(Chest, bx(0.05, 0.07, 0.006), 0xf4f4f0, [x, y, 0.135]);
        k.cp(Chest, bx(0.04, 0.02, 0.007), 0x3a6aa8, [x, y + 0.02, 0.136]);
    };
    match o.outfit {
        "office" => {
            collar(k, 0xf4f4f6);
            k.cp(
                Chest,
                bx(0.035, 0.03, 0.02),
                if f { 0xf4f4f6 } else { 0x7a1e2a },
                [0.0, 0.06, 0.1],
            );
            if !f {
                k.cl(
                    Chest,
                    bx(0.04, 0.24, 0.012),
                    0x7a1e2a,
                    [0.0, -0.07, 0.122],
                    [0.08, 0.0, 0.0],
                    ONE,
                );
            }
            k.cl(
                Chest,
                geom::torus(0.09, 0.004, 3, 16, PI),
                0x2a5aa8,
                [0.0, 0.02, 0.06],
                [-0.4, 0.0, PI],
                ONE,
            );
            badge(k, -0.12, 0.0);
            for y in [-0.12, -0.2] {
                k.cp(Chest, sph(0.008), 0x111111, [0.02, y, 0.128]);
            }
            k.ring(Hips, 0.15, 0.1, seam, [1.0, 1.0, 0.66]);
            if f {
                skirt(k, o.bottom, 0.3, false);
            }
            k.cp(HandL, bx(0.08, 0.28, 0.36), 0x3a2a1e, [0.0, -0.2, 0.0]);
            k.cp(HandL, bx(0.02, 0.04, 0.12), 0x1a1a1a, [0.0, -0.05, 0.0]);
            for ft in [FtL, FtR] {
                k.cp(ft, bx(0.098, 0.025, 0.24), 0x111111, [0.0, -0.05, 0.05]);
            }
        }
        "student" => {
            collar(k, 0xffffff);
            k.cp(Chest, bx(0.05, 0.012, 0.005), 0x2a4a9a, [0.08, -0.03, 0.13]);
            if f {
                skirt(k, o.bottom, 0.34, true);
                k.cp(Chest, bx(0.03, 0.06, 0.01), 0x2a3a6a, [0.0, 0.05, 0.11]);
            } else {
                k.ring(Hips, 0.15, 0.1, 0x111111, [1.0, 1.0, 0.66]);
            }
            k.cl(
                Chest,
                bx(0.06, 0.6, 0.012),
                0x2a2a2a,
                [0.0, -0.1, 0.1],
                [0.0, 0.0, 0.75],
                ONE,
            );
            k.cl(
                Hips,
                bx(0.3, 0.24, 0.09),
                o.pack.unwrap_or(0x2a3a6a),
                [-0.2, 0.02, 0.02],
                [0.0, PI / 2.0, 0.0],
                ONE,
            );
            for sh in [ShL, ShR] {
                k.cp(
                    sh,
                    cyl(0.045, 0.043, 0.1, 10),
                    0xffffff,
                    [0.0, -ls + 0.1, 0.0],
                );
            }
        }
        "courier" => {
            k.cl(
                Head,
                geom::sphere_ex(0.128, 16, 10, 0.0, TAU, 0.0, PI * 0.55),
                0x2f6a4c,
                [0.0, 0.1, -0.01],
                [-0.15, 0.0, 0.0],
                ONE,
            );
            k.cl(
                Head,
                bx(0.2, 0.012, 0.08),
                0x2a2e30,
                [0.0, 0.175, 0.1],
                [-0.25, 0.0, 0.0],
                ONE,
            );
            k.cp(Head, bx(0.26, 0.03, 0.01), 0xffe680, [0.0, 0.17, 0.0]);
            k.cp(Chest, bx(0.42, 0.42, 0.28), 0x3a3f8a, [0.0, -0.1, -0.26]);
            k.cp(Chest, bx(0.425, 0.07, 0.285), 0xffe680, [0.0, 0.03, -0.26]);
            k.cp(Chest, bx(0.2, 0.08, 0.005), 0xffe680, [0.0, -0.14, -0.402]);
            for x in [-0.1, 0.1] {
                k.cl(
                    Chest,
                    bx(0.04, 0.34, 0.015),
                    0x222222,
                    [x, -0.12, 0.112],
                    [-0.1, 0.0, 0.0],
                    ONE,
                );
            }
            k.ring(Spine, 0.15, 0.0, seam, [1.0, 1.0, 0.66]);
            for x in [-0.1, 0.1] {
                k.cp(Chest, bx(0.012, 0.3, 0.005), 0xd8d8d8, [x, -0.15, 0.124]);
            }
            k.cp(Chest, bx(0.008, 0.3, 0.008), 0x222222, [0.01, -0.12, 0.125]);
        }
        "nurse" => {
            let d = dk(o.top, 0.7);
            k.cl(
                Chest,
                bx(0.1, 0.012, 0.03),
                d,
                [-0.03, 0.06, 0.1],
                [0.0, 0.0, 0.7],
                ONE,
            );
            k.cl(
                Chest,
                bx(0.1, 0.012, 0.03),
                d,
                [0.03, 0.06, 0.1],
                [0.0, 0.0, -0.7],
                ONE,
            );
            k.cp(
                Chest,
                bx(0.08, 0.06, 0.01),
                dk(o.top, 0.85),
                [-0.08, -0.12, 0.125],
            );
            k.cp(
                Chest,
                bx(0.006, 0.05, 0.006),
                0x2a2a2a,
                [-0.06, -0.09, 0.128],
            );
            badge(k, -0.05, 0.08);
            k.cp(Neck, bx(0.13, 0.05, 0.02), 0x8ac8e0, [0.0, 0.0, 0.06]);
            for sd in [-1.0f32, 1.0] {
                k.cp(
                    Neck,
                    bx(0.004, 0.004, 0.08),
                    0xf4f4f4,
                    [sd * 0.06, 0.02, 0.02],
                );
            }
            for sh in [ShL, ShR] {
                k.ring(sh, 0.044, -ls + 0.16, dk(o.bottom, 0.7), ONE);
            }
            for ft in [FtL, FtR] {
                k.cp(ft, bx(0.1, 0.035, 0.25), 0xf4f4f4, [0.0, -0.055, 0.05]);
            }
        }
        "tech" => {
            let vp: Vec<[f32; 2]> = torso.iter().take(6).map(|p| [p[0] + 0.03, p[1]]).collect();
            k.cl(
                Spine,
                geom::lathe(&vp, 16, 0.5, TAU - 1.0),
                0x8a7a4a,
                NO,
                NO,
                [1.0, 1.0, 0.7],
            );
            for x in [-0.08, 0.08] {
                for y in [0.15, 0.25] {
                    k.cp(Spine, bx(0.07, 0.06, 0.02), 0x6a5a34, [x, y, 0.12]);
                }
            }
            k.cl(
                Hips,
                cyl(0.17, 0.17, 0.05, 16),
                0x4a3420,
                [0.0, 0.11, 0.0],
                NO,
                [1.0, 1.0, 0.72],
            );
            for a in [-1.1f32, 1.1, 2.4] {
                k.cp(
                    Hips,
                    bx(0.07, 0.1, 0.05),
                    0x5a4028,
                    [a.sin() * 0.17, 0.05, a.cos() * 0.13],
                );
            }
            k.cp(
                Hips,
                cyl(0.012, 0.012, 0.18, 6),
                0x7a5a3a,
                [-0.17, 0.08, 0.06],
            );
            k.cp(Hips, bx(0.09, 0.03, 0.03), 0x555555, [-0.17, 0.09, 0.06]);
            for sh in [ShL, ShR] {
                k.cp(
                    sh,
                    cyl(0.06, 0.058, 0.3, 12),
                    0x2a4a8a,
                    [0.0, -ls + 0.31, 0.0],
                );
            }
            for ft in [FtL, FtR] {
                k.cp(ft, bx(0.11, 0.05, 0.27), 0x2a4a8a, [0.0, -0.045, 0.045]);
            }
            k.cp(
                Head,
                geom::sphere_ex(0.11, 16, 8, 0.0, TAU, 0.0, PI * 0.42),
                0x3a5a7a,
                [0.0, 0.105, -0.01],
            );
            k.cl(
                Head,
                bx(0.15, 0.012, 0.09),
                0x3a5a7a,
                [0.0, 0.135, 0.1],
                [-0.15, 0.0, 0.0],
                ONE,
            );
        }
        _ => {}
    }
}

// ---- firearms.ts (the models the soldiers and the player carry) ----
const STEEL: u32 = 0x2c3034;
const BLK: u32 = 0x1b1e21;
const DARK: u32 = 0x24282b;
const GUNM: u32 = 0x3a3f43;
const PLA_OD: u32 = 0x4c5340;
const ROC_GRN: u32 = 0x3c4434;

enum P {
    B(f32, f32, f32, f32, f32, f32, u32, f32),
    C(f32, f32, f32, f32, f32, u32),
}
fn b(w: f32, h: f32, l: f32, x: f32, y: f32, z: f32, c: u32) -> P {
    P::B(w, h, l, x, y, z, c, 0.0)
}
fn br(w: f32, h: f32, l: f32, x: f32, y: f32, z: f32, c: u32, rx: f32) -> P {
    P::B(w, h, l, x, y, z, c, rx)
}
fn c(r: f32, l: f32, x: f32, y: f32, z: f32, col: u32) -> P {
    P::C(r, l, x, y, z, col)
}

fn spec(id: &str) -> Option<(Vec<P>, [f32; 3])> {
    Some(match id {
        "rifle" | "qbz191" => (
            vec![
                b(0.042, 0.075, 0.27, 0.0, 0.042, 0.03, PLA_OD),
                b(0.024, 0.012, 0.44, 0.0, 0.087, 0.1, BLK),
                b(0.006, 0.02, 0.05, -0.023, 0.06, 0.06, BLK),
                b(0.05, 0.055, 0.25, 0.0, 0.055, 0.3, PLA_OD),
                c(0.011, 0.16, 0.0, 0.062, 0.5, STEEL),
                c(0.016, 0.05, 0.0, 0.062, 0.6, BLK),
                br(0.03, 0.086, 0.042, 0.0, -0.036, -0.02, BLK, -0.3),
                b(0.006, 0.008, 0.05, 0.0, -0.002, 0.035, BLK),
                br(0.028, 0.095, 0.06, 0.0, -0.04, 0.1, DARK, 0.15),
                br(0.028, 0.06, 0.056, 0.0, -0.11, 0.12, DARK, 0.35),
                b(0.022, 0.022, 0.12, 0.0, 0.05, -0.14, BLK),
                b(0.036, 0.08, 0.16, 0.0, 0.035, -0.27, PLA_OD),
                b(0.04, 0.03, 0.12, 0.0, 0.082, -0.26, PLA_OD),
                b(0.042, 0.09, 0.016, 0.0, 0.032, -0.355, BLK),
                b(0.018, 0.022, 0.018, 0.0, 0.104, 0.36, BLK),
                b(0.02, 0.024, 0.02, 0.0, 0.104, -0.04, BLK),
            ],
            [0.0, 0.062, 0.63],
        ),
        "qbz95" => (
            vec![
                b(0.05, 0.085, 0.52, 0.0, 0.04, -0.09, PLA_OD),
                b(0.054, 0.1, 0.12, 0.0, 0.03, -0.37, PLA_OD),
                b(0.056, 0.104, 0.016, 0.0, 0.03, -0.435, BLK),
                b(0.04, 0.045, 0.18, 0.0, 0.03, 0.24, PLA_OD),
                c(0.011, 0.14, 0.0, 0.045, 0.39, STEEL),
                c(0.015, 0.05, 0.0, 0.045, 0.47, BLK),
                br(0.032, 0.09, 0.044, 0.0, -0.04, 0.0, BLK, -0.2),
                b(0.03, 0.05, 0.11, 0.0, -0.03, 0.06, PLA_OD),
                br(0.03, 0.1, 0.064, 0.0, -0.055, -0.13, DARK, 0.18),
                b(0.006, 0.026, 0.06, -0.028, 0.06, -0.2, BLK),
                b(0.026, 0.03, 0.06, 0.0, 0.1, 0.12, PLA_OD),
                b(0.026, 0.03, 0.06, 0.0, 0.1, -0.18, PLA_OD),
                b(0.03, 0.024, 0.36, 0.0, 0.128, -0.03, PLA_OD),
            ],
            [0.0, 0.045, 0.5],
        ),
        "t91" => (
            vec![
                b(0.038, 0.05, 0.2, 0.0, 0.022, 0.02, GUNM),
                b(0.042, 0.046, 0.25, 0.0, 0.066, 0.035, GUNM),
                b(0.006, 0.02, 0.05, -0.022, 0.068, 0.06, BLK),
                b(0.03, 0.01, 0.025, 0.0, 0.094, -0.085, BLK),
                b(0.024, 0.012, 0.24, 0.0, 0.095, 0.04, BLK),
                b(0.02, 0.022, 0.018, 0.0, 0.11, -0.06, BLK),
                b(0.056, 0.06, 0.26, 0.0, 0.062, 0.29, ROC_GRN),
                b(0.058, 0.004, 0.24, 0.0, 0.093, 0.29, BLK),
                c(0.011, 0.2, 0.0, 0.066, 0.5, STEEL),
                b(0.012, 0.06, 0.016, 0.0, 0.1, 0.43, BLK),
                b(0.03, 0.008, 0.02, 0.0, 0.068, 0.43, BLK),
                c(0.015, 0.055, 0.0, 0.066, 0.625, BLK),
                br(0.03, 0.088, 0.04, 0.0, -0.034, -0.025, BLK, -0.32),
                b(0.006, 0.008, 0.055, 0.0, -0.004, 0.035, BLK),
                br(0.028, 0.09, 0.062, 0.0, -0.035, 0.095, DARK, 0.12),
                br(0.028, 0.07, 0.058, 0.0, -0.105, 0.112, DARK, 0.32),
                c(0.016, 0.16, 0.0, 0.05, -0.17, GUNM),
                b(0.04, 0.085, 0.16, 0.0, 0.032, -0.28, ROC_GRN),
                b(0.044, 0.1, 0.016, 0.0, 0.026, -0.365, BLK),
            ],
            [0.0, 0.066, 0.655],
        ),
        _ => return None,
    })
}

/// firearms.ts `gunModel`: one merged vertex-coloured model, +Z forward, plus the muzzle point.
pub fn gun_model(id: &str) -> Option<(MeshBuf, Vec3)> {
    let (parts, muzzle) = spec(id)?;
    let mut m = MeshBuf::default();
    for p in parts {
        match p {
            P::B(w, h, l, x, y, z, col, rx) => {
                m.add(
                    &geom::cuboid(w, h, l),
                    geom::at(x, y, z) * Mat4::from_rotation_x(rx),
                    hex(col),
                );
            }
            P::C(r, l, x, y, z, col) => {
                m.add(
                    &geom::cylinder(r, r, l, 8),
                    geom::at(x, y, z) * Mat4::from_rotation_x(PI / 2.0),
                    hex(col),
                );
            }
        }
    }
    Some((m, Vec3::from(muzzle)))
}
