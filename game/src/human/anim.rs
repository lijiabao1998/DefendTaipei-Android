//! Procedural animation of the 白面人 rig — a port of character-rig `motion.ts` (stride-locked
//! base gait, critically damped pose springs, yaw-rate / acceleration tracking) and the
//! `animate` / `deathPose` layers of human.ts: sneak crouch, rifle carry, aim, gun recoil with
//! heat, hurt flinch, the three death falls, and the hip IK that keeps the stance foot planted.

use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, PI};

use crate::combat::tuning::{GunFam, recoil};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AnimState {
    #[default]
    Idle,
    Walk,
    Run,
    Sneak,
    Aim,
    Shoot,
    Death,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum K {
    HipY,
    HipYaw,
    BodyX,
    BodyZ,
    BodyZoff,
    SpX,
    SpY,
    SpZ,
    ChX,
    ChY,
    HdX,
    HdY,
    ThL,
    ThR,
    ThLz,
    ThRz,
    ShL,
    ShR,
    FtL,
    FtR,
    UaL,
    UaR,
    UaLz,
    UaRz,
    UaLy,
    UaRy,
    LaL,
    LaR,
    LaLy,
    LaRy,
}
pub const NK: usize = 30;

#[derive(Clone, Copy, Default)]
pub struct Pose(pub [f32; NK]);
impl std::ops::Index<K> for Pose {
    type Output = f32;
    fn index(&self, k: K) -> &f32 {
        &self.0[k as usize]
    }
}
impl std::ops::IndexMut<K> for Pose {
    fn index_mut(&mut self, k: K) -> &mut f32 {
        &mut self.0[k as usize]
    }
}

pub fn sm(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Joint rotations / offsets written by [`animate`] (read by the plugin into transforms).
#[derive(Clone, Copy, Default, Debug)]
pub struct Joints {
    pub hips_y: f32,
    pub hips_yaw: f32,
    pub body_rot: Vec3,
    pub body_pos: Vec3,
    pub spine: Vec3,
    pub chest: Vec3,
    pub head: Vec3,
    pub th_l: Vec3,
    pub th_r: Vec3,
    pub sh_l: f32,
    pub sh_r: f32,
    pub ft_l: f32,
    pub ft_r: f32,
    pub ua_l: Vec3,
    pub ua_r: Vec3,
    pub la_l: Vec3,
    pub la_r: Vec3,
    pub weapon: Vec3,
    pub gun_back: f32,
    /// Extra yaw spin of the whole rig (death variant 2).
    pub spin: f32,
}

/// Per-character animation state (the animation half of human.ts `Rig`).
#[derive(Clone, Debug)]
pub struct Anim {
    pub phase: f32,
    pub t: f32,
    pub sp: [[f32; 2]; NK],
    pub init: bool,
    pub last_yaw: f32,
    pub yaw_rate: f32,
    pub last_speed: f32,
    pub accel: f32,
    pub w_aim: f32,
    pub w_sneak: f32,
    pub w_carry: f32,
    pub shoot_t: f32,
    pub heat: f32,
    pub jit: [f32; 2],
    pub hurt_t: f32,
    pub hurt_side: f32,
    pub dead_t: f32,
    pub death_v: u8,
    pub lt: f32,
    pub ls: f32,
    pub gun: Option<GunFam>,
    pub out: Joints,
    seed: u32,
}

impl Anim {
    pub fn new(lt: f32, ls: f32, gun: Option<GunFam>, seed: u32) -> Self {
        let r =
            |k: u32| ((seed.wrapping_mul(2654435761).wrapping_add(k * 97)) % 1000) as f32 / 1000.0;
        Self {
            phase: r(1) * 6.0,
            t: r(2) * 10.0,
            sp: [[0.0; 2]; NK],
            init: false,
            last_yaw: 0.0,
            yaw_rate: 0.0,
            last_speed: 0.0,
            accel: 0.0,
            w_aim: 0.0,
            w_sneak: 0.0,
            w_carry: 0.0,
            shoot_t: 0.0,
            heat: 0.0,
            jit: [0.0; 2],
            hurt_t: 0.0,
            hurt_side: 1.0,
            dead_t: 0.0,
            death_v: 0,
            lt,
            ls,
            gun,
            out: Joints::default(),
            seed,
        }
    }

    fn rnd(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    /// fireShot: restart the kick, add heat, roll a sideways shudder.
    pub fn fire(&mut self) {
        let fam = self.gun.unwrap_or(GunFam::Rifle);
        let r = recoil(fam);
        self.shoot_t = 0.14;
        self.heat = (self.heat + r.heat).min(1.0);
        self.jit = [
            (self.rnd() - 0.5) * 2.0 * r.jitter,
            (self.rnd() - 0.5) * 2.0 * r.jitter,
        ];
    }

    pub fn hurt(&mut self) {
        self.hurt_t = 0.35;
        self.hurt_side = if self.rnd() < 0.5 { -1.0 } else { 1.0 };
    }

    fn track(&mut self, yaw: f32, speed: f32, dt: f32) {
        let mut dy = yaw - self.last_yaw;
        dy = dy.sin().atan2(dy.cos());
        self.last_yaw = yaw;
        self.yaw_rate += (dy / dt - self.yaw_rate) * (dt * 8.0).min(1.0);
        self.accel += ((speed - self.last_speed) / dt - self.accel) * (dt * 6.0).min(1.0);
        self.last_speed = speed;
    }

    /// Critically damped springs toward the target pose (applySprings), then the joint angles.
    fn springs(&mut self, t: &Pose, dt: f32, om: f32) {
        use K::*;
        if !self.init {
            self.init = true;
            self.sp[HipY as usize][0] = t[HipY];
        }
        let n = (dt / 0.012).ceil().max(1.0) as usize;
        let h = dt / n as f32;
        for k in 0..NK {
            let o = if (SpX as usize..=ChY as usize).contains(&k)
                || (BodyX as usize..=BodyZoff as usize).contains(&k)
                || k == HipYaw as usize
            {
                om * 0.7
            } else {
                om
            };
            let e = &mut self.sp[k];
            for _ in 0..n {
                let a = o * o * (t.0[k] - e[0]) - 2.0 * o * e[1];
                e[1] += a * h;
                e[0] += e[1] * h;
            }
        }
        let v = |k: K| self.sp[k as usize][0];
        self.out = Joints {
            hips_y: v(HipY),
            hips_yaw: v(HipYaw),
            body_rot: Vec3::new(v(BodyX), 0.0, v(BodyZ)),
            body_pos: Vec3::new(v(BodyZoff), 0.0, 0.0),
            spine: Vec3::new(v(SpX), v(SpY), v(SpZ)),
            chest: Vec3::new(v(ChX), v(ChY), 0.0),
            head: Vec3::new(v(HdX), v(HdY), 0.0),
            th_l: Vec3::new(v(ThL), 0.0, v(ThLz)),
            th_r: Vec3::new(v(ThR), 0.0, v(ThRz)),
            sh_l: v(ShL),
            sh_r: v(ShR),
            ft_l: v(FtL),
            ft_r: v(FtR),
            ua_l: Vec3::new(v(UaL), v(UaLy), v(UaLz)),
            ua_r: Vec3::new(v(UaR), v(UaRy), v(UaRz)),
            la_l: Vec3::new(v(LaL), v(LaLy), 0.0),
            la_r: Vec3::new(v(LaR), v(LaRy), 0.0),
            weapon: Vec3::new(FRAC_PI_2, 0.0, 0.0),
            gun_back: 0.0,
            spin: self.out.spin,
        };
    }

    /// One animation step. `yaw` is the facing (rad) of the root, `speed` in m/s.
    pub fn animate(&mut self, state: AnimState, yaw: f32, speed: f32, dt: f32) {
        use K::*;
        if dt <= 0.0 {
            return;
        }
        self.t += dt;
        self.shoot_t = (self.shoot_t - dt).max(0.0);
        self.heat = (self.heat - dt * if self.shoot_t > 0.0 { 0.9 } else { 1.8 }).max(0.0);
        self.hurt_t = (self.hurt_t - dt).max(0.0);
        self.track(yaw, speed, dt);
        let ease = |w: &mut f32, v: f32, rate: f32| *w += (v - *w) * (dt * rate).min(1.0);
        let aiming = matches!(state, AnimState::Aim | AnimState::Shoot) || self.shoot_t > 0.0;
        ease(&mut self.w_aim, if aiming { 1.0 } else { 0.0 }, 10.0);
        ease(
            &mut self.w_sneak,
            if state == AnimState::Sneak { 1.0 } else { 0.0 },
            5.0,
        );
        let long_gun = self.gun.is_some_and(|g| g != GunFam::Pistol);
        ease(
            &mut self.w_carry,
            if long_gun && state != AnimState::Aim {
                1.0
            } else {
                0.0
            },
            6.0,
        );
        if state == AnimState::Death {
            self.death(dt);
            return;
        }
        self.dead_t = 0.0;
        self.out.spin = 0.0;
        let (lt, ls) = (self.lt, self.ls);
        let mut t = Pose::default();
        // ---- baseGait ----
        let s = speed;
        let run_w = sm((s - 3.6) / 2.2);
        let walk_w = sm(s / 1.2);
        let sn = self.w_sneak;
        let stride = (0.95 + 0.42 * s) * (1.0 - 0.35 * sn);
        let duty = 0.55 - 0.2 * run_w;
        let mut amp = walk_w
            * ((stride * duty) / (2.0 * (lt + ls)))
                .clamp(0.2, 0.78)
                .asin()
            * (1.0 - 0.35 * sn);
        let turn_step = if s < 0.4 && self.yaw_rate.abs() > 1.2 {
            ((self.yaw_rate.abs() - 1.2) / 3.0).min(1.0)
        } else {
            0.0
        };
        self.phase += (2.0 * PI * s) / stride * dt + turn_step * dt * 9.0;
        if turn_step > 0.0 {
            amp = amp.max(0.16 * turn_step);
        }
        let p = self.phase;
        let (sl, cl) = (p.sin(), p.cos());
        let leg = |sinv: f32, cosv: f32| {
            let th = -sinv * amp;
            let swing = cosv.max(0.0);
            (
                th,
                swing * amp * (1.3 + run_w * 1.6) + 0.05 + sinv.max(0.0) * amp * 0.25 * run_w,
            )
        };
        let (tl, kl) = leg(sl, cl);
        let (tr, kr) = leg(-sl, -cl);
        t[ThL] = tl;
        t[ThR] = tr;
        t[ShL] = kl;
        t[ShR] = kr;
        let breath = (self.t * 1.7).sin();
        let idle_w = 1.0 - walk_w;
        t[HipYaw] = -sl * amp * 0.25 + idle_w * (self.t * 0.37).sin() * 0.05;
        t[SpY] = sl * amp * 0.32;
        t[ChY] = -sl * amp * 0.1;
        t[BodyZoff] = idle_w * (self.t * 0.37).sin() * 0.02;
        t[ThLz] = idle_w * 0.05 + 0.05 * walk_w + 0.04 * run_w;
        t[ThRz] = -idle_w * 0.03 - 0.05 * walk_w - 0.04 * run_w;
        t[ChX] = breath * 0.018 * idle_w - run_w * 0.05;
        t[HdX] = -breath * 0.012;
        t[HdY] = idle_w * (self.t * 0.29).sin() * 0.35 * (1.0 - self.w_aim);
        t[SpX] = 0.04 + run_w * 0.24 + walk_w * 0.04 + (self.accel * 0.035).clamp(-0.12, 0.18);
        t[BodyZ] = (-self.yaw_rate * s * 0.035).clamp(-0.32, 0.32);
        t[UaL] = tr * 0.85 * (1.0 - self.w_carry);
        t[UaR] = tl * 0.85;
        t[UaLz] = 0.1 + run_w * 0.08 + breath * 0.01;
        t[UaRz] = -0.1 - run_w * 0.08 - breath * 0.01;
        t[LaL] = -0.18 - amp * 0.35 - run_w * 1.05 + (-tr).max(0.0) * 0.25;
        t[LaR] = -0.18 - amp * 0.35 - run_w * 1.05 + (-tl).max(0.0) * 0.25;
        // ---- sneak: deep crouch ----
        if sn > 0.01 {
            t[ThL] += -0.95 * sn;
            t[ThR] += -0.95 * sn;
            t[ShL] += 1.45 * sn;
            t[ShR] += 1.45 * sn;
            t[SpX] += 0.5 * sn;
            t[HdX] -= 0.35 * sn;
            t[UaL] += -0.45 * sn;
            t[UaR] += -0.45 * sn;
            t[LaL] -= 0.7 * sn;
            t[LaR] -= 0.7 * sn;
            t[UaLz] += 0.1 * sn;
        }
        t[FtL] = -(t[ThL] + t[ShL]) * 0.85 + cl.max(0.0) * amp * 0.3;
        t[FtR] = -(t[ThR] + t[ShR]) * 0.85 + (-cl).max(0.0) * amp * 0.3;
        // ---- carry ----
        let c = self.w_carry;
        if c > 0.01 {
            t[UaR] = t[UaR] * (1.0 - c) - 0.55 * c;
            t[LaR] = t[LaR] * (1.0 - c) - 1.25 * c;
            t[UaL] = t[UaL] * (1.0 - c) - 0.85 * c;
            t[LaL] = t[LaL] * (1.0 - c) - 1.35 * c;
            t[UaLz] = t[UaLz] * (1.0 - c) - 0.6 * c;
            t[UaRz] = t[UaRz] * (1.0 - c) - 0.1 * c;
        }
        // ---- aim ----
        let a = self.w_aim;
        if a > 0.01 {
            let kick = if self.shoot_t > 0.05 { 0.22 } else { 0.0 };
            let two = if long_gun { 1.0 } else { 0.7 };
            t[UaR] = t[UaR] * (1.0 - a) + (-1.5 - kick) * a;
            t[LaR] = t[LaR] * (1.0 - a) + 0.02 * a;
            t[UaRz] *= 1.0 - a;
            t[UaRy] = 0.1 * a;
            t[UaL] = t[UaL] * (1.0 - a) + (-1.35 - kick * 0.5) * a;
            t[LaL] = t[LaL] * (1.0 - a) - 0.4 * a;
            t[UaLz] = t[UaLz] * (1.0 - a) - 0.55 * two * a;
            t[UaLy] = -0.25 * a;
            t[SpY] *= 1.0 - a;
            t[ChY] = t[ChY] * (1.0 - a) + 0.12 * a;
            t[SpX] = t[SpX] * (1.0 - a * 0.5) - kick * 0.4;
            t[HdX] += kick * -0.3;
            t[HdY] *= 1.0 - a;
        }
        // ---- hurt flinch ----
        if self.hurt_t > 0.0 {
            let h = sm(self.hurt_t / 0.35);
            t[SpX] -= 0.45 * h;
            t[HdX] -= 0.5 * h;
            t[SpZ] += 0.25 * h * self.hurt_side;
            t[UaL] -= 0.7 * h;
            t[UaLz] += 0.4 * h;
            t[UaR] -= 0.3 * h;
            t[ThR] += -0.3 * h;
            t[ShR] += 0.4 * h;
            t[BodyX] -= 0.08 * h;
        }
        // ---- hip IK ----
        let eff = |th: f32, sh: f32| lt * th.cos() + ls * (th + sh).cos();
        t[HipY] = eff(t[ThL], t[ShL]).max(eff(t[ThR], t[ShR])) + 0.105 + breath * 0.003;
        self.springs(&t, dt, 16.0);
        // foot plant from the rendered leg
        let g = |k: K| self.sp[k as usize][0];
        let reach = (eff(g(ThL), g(ShL)) * g(ThLz).cos()).max(eff(g(ThR), g(ShR)) * g(ThRz).cos());
        self.out.hips_y = reach + 0.105 + 0.03 * walk_w + breath * 0.003;
        // ---- gun recoil ----
        let mut rec_z = 0.0;
        if let Some(fam) = self.gun {
            let r = recoil(fam);
            let t0 = 1.0 - self.shoot_t / 0.14;
            let c = if self.shoot_t > 0.0 {
                if t0 < 0.18 {
                    t0 / 0.18
                } else {
                    (1.0 - (t0 - 0.18) / 0.82).powi(2)
                }
            } else {
                0.0
            };
            let hh = self.heat * a;
            let bb = r.body;
            let o = &mut self.out;
            o.weapon = Vec3::new(
                FRAC_PI_2 - c * r.kick - hh * r.climb,
                self.jit[0] * c,
                self.jit[1] * c * 1.5,
            );
            o.gun_back = -c * r.back;
            o.ua_r.x -= (c * 0.16 + hh * 0.08) * bb;
            o.la_r.x -= c * 0.12 * bb;
            o.ua_l.x -= (c * 0.1 + hh * 0.06) * bb;
            o.chest.x -= (c * 0.06 + hh * 0.07) * bb;
            o.chest.y += self.jit[0] * c * 0.8 * bb;
            o.spine.z += self.jit[1] * c * 0.5 * bb;
            o.head.x -= c * 0.05 + hh * 0.03;
            rec_z = -(c * 0.025 + hh * 0.035) * bb;
        }
        let hz = if self.hurt_t > 0.0 {
            0.12 * sm(self.hurt_t / 0.35)
        } else {
            0.0
        };
        self.out.body_pos = Vec3::new(self.sp[BodyZoff as usize][0], 0.0, -hz + rec_z);
    }

    fn death(&mut self, dt: f32) {
        use K::*;
        if self.dead_t == 0.0 {
            self.death_v = (self.rnd() * 3.0) as u8 % 3;
            self.out.spin = 0.0;
        }
        self.dead_t = (self.dead_t + dt * 1.25).min(1.0);
        let d = self.dead_t;
        let mut t = Pose::default();
        let buck = sm(d / 0.35);
        let fall = sm((d - 0.2) / 0.7);
        let (mut bx, mut bz) = (0.0, 0.0);
        match self.death_v {
            0 => {
                t[ThL] = -0.9 * buck + 0.6 * fall;
                t[ThR] = -0.6 * buck + 0.3 * fall;
                t[ShL] = 1.6 * buck - 1.2 * fall;
                t[ShR] = 1.2 * buck - 0.9 * fall;
                t[UaL] = -2.6 * fall;
                t[UaR] = -2.2 * fall;
                t[UaLz] = 0.7;
                t[UaRz] = -0.9;
                t[HdX] = -0.4 * fall;
                t[SpX] = 0.3 * buck - 0.3 * fall;
                bx = -FRAC_PI_2 * fall;
            }
            1 => {
                t[ThL] = -0.7 * buck + 0.5 * fall;
                t[ThR] = -0.3 * buck + 0.4 * fall;
                t[ShL] = 1.4 * buck - 1.0 * fall;
                t[ShR] = 0.9 * buck - 0.5 * fall;
                t[UaL] = -0.6 * buck - 2.2 * fall;
                t[UaR] = -0.4 - 2.6 * fall;
                t[LaL] = -0.8;
                t[LaR] = -1.4 * fall;
                t[UaLz] = 0.5;
                t[UaRz] = -0.6;
                t[SpX] = 0.6 * buck - 0.4 * fall;
                t[HdY] = 0.9 * fall;
                bx = FRAC_PI_2 * fall;
            }
            _ => {
                t[ThL] = -1.2 * buck;
                t[ThR] = -0.4 * buck;
                t[ShL] = 1.8 * buck;
                t[ShR] = 1.0 * buck;
                t[SpZ] = 0.4 * buck;
                t[UaL] = -0.4 - 1.6 * fall;
                t[UaR] = -0.3;
                t[UaLz] = 1.0 * fall;
                t[UaRz] = -0.2;
                t[LaR] = -1.2;
                t[HdX] = 0.3;
                t[HdY] = -0.6 * fall;
                bz = FRAC_PI_2 * fall;
                self.out.spin += dt * 2.2 * (1.0 - d);
            }
        }
        t[HipY] = (self.lt + self.ls) * (1.0 - 0.45 * buck) + 0.075;
        self.springs(&t, dt, 18.0);
        self.out.body_rot = Vec3::new(bx, 0.0, bz);
        self.out.body_pos = Vec3::new(0.0, 0.13 * fall, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walking_keeps_a_foot_near_the_ground_and_advances_phase() {
        let (lt, ls, _) = crate::human::rig::body_dims(false);
        let mut a = Anim::new(lt, ls, Some(GunFam::Rifle), 7);
        let p0 = a.phase;
        for _ in 0..240 {
            a.animate(AnimState::Walk, 0.0, 1.6, 1.0 / 60.0);
        }
        assert!(a.phase > p0 + 1.0);
        // hips sit at about leg length: neither sunk nor floating
        assert!(
            a.out.hips_y > 0.75 && a.out.hips_y < 1.05,
            "hips {}",
            a.out.hips_y
        );
        for _ in 0..120 {
            a.animate(AnimState::Death, 0.0, 0.0, 1.0 / 60.0);
        }
        assert!(a.out.body_rot.x.abs() > 1.2 || a.out.body_rot.z.abs() > 1.2);
    }
}
