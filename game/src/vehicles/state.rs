use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::{VehicleKind, VehicleSpec};
use crate::combat::armor::MountedGunState;
use crate::i18n::{Language, tr};
use crate::inventory::Inventory;
use crate::map::collide::DynamicCollider;
use crate::map::{Obstacles, WorldData};
use crate::roles::Skills;

/// Every value that changes a vehicle is persisted. Mesh handles, input,
/// in-progress wiring and elapsed noise timers are deliberately transient.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleState {
    pub id: u32,
    pub kind: VehicleKind,
    pub pos: [f32; 2],
    pub heading: f32,
    pub speed: f32,
    pub fuel: f32,
    pub battery: f32,
    pub tyre: f32,
    pub engine: f32,
    pub key: bool,
    pub docked: bool,
    #[serde(default)]
    pub gun: Option<MountedGunState>,
    #[serde(default)]
    pub turret_yaw: f32,
}

impl VehicleState {
    pub fn new(
        id: u32,
        kind: VehicleKind,
        pos: Vec2,
        heading: f32,
        r: &mut impl FnMut() -> f32,
    ) -> Self {
        let s = kind.spec();
        let fuel = s.fuel_capacity * (0.1 + r() * 0.7);
        let battery = 0.5 + r() * 0.5;
        let tyre = if r() < 0.25 { 0.3 } else { 1.0 };
        let engine = 0.6 + r() * 0.4;
        let key = r() < 0.4;
        let mut gun = s.armored.then(|| MountedGunState::new(kind));
        if let Some(gun) = &mut gun {
            let supply = 0.6 + r() * 0.4;
            for g in &mut gun.guns {
                g.reserve = (g.reserve as f32 * supply).round() as u32;
            }
        }
        Self {
            id,
            kind,
            pos: pos.into(),
            heading,
            speed: 0.0,
            fuel,
            battery,
            tyre: if s.armored { 1.0 } else { tyre },
            engine,
            key: s.armored || key,
            docked: false,
            gun,
            turret_yaw: 0.0,
        }
    }
    pub fn spec(&self) -> &'static VehicleSpec {
        self.kind.spec()
    }
    pub fn position(&self) -> Vec2 {
        Vec2::from_array(self.pos)
    }
    pub fn corners_at(&self, pos: Vec2, heading: f32) -> [Vec2; 4] {
        footprint(pos, heading, self.spec().length, self.spec().width)
    }
    pub fn collider(&self) -> DynamicCollider {
        DynamicCollider {
            id: self.id as u64,
            corners: self.corners_at(self.position(), self.heading),
            height: if self.spec().armored {
                2.7
            } else if self.spec().two {
                1.0
            } else if matches!(
                self.kind,
                VehicleKind::Bus | VehicleKind::Garbage | VehicleKind::Van
            ) {
                2.7
            } else {
                1.55
            },
        }
    }
    pub fn status(&self, lang: Language) -> String {
        let s = self.spec();
        let fuel = if s.pedal {
            tr(lang, "人力", "Pedal").to_string()
        } else if s.electric {
            format!(
                "{} {:.0}%",
                tr(lang, "電量", "Charge"),
                self.fuel / s.fuel_capacity * 100.0
            )
        } else {
            format!(
                "{} {:.1}/{:.0} L",
                tr(lang, "燃油", "Fuel"),
                self.fuel,
                s.fuel_capacity
            )
        };
        format!(
            "{} · {} · {} {:.0}% · {} {:.0}% · {} {:.0}%",
            s.name(lang),
            fuel,
            tr(lang, "輪胎", "Tires"),
            self.tyre * 100.0,
            tr(lang, "電瓶", "Battery"),
            self.battery * 100.0,
            tr(lang, "引擎", "Engine"),
            self.engine * 100.0
        )
    }
    pub fn valid(&self, world: &WorldData) -> bool {
        let s = self.spec();
        let unit = |n: f32| n.is_finite() && (0.0..=1.0).contains(&n);
        self.id < 100_000
            && self.pos.iter().all(|x| x.is_finite())
            && self.position().cmpge(world.min).all()
            && self.position().cmple(world.max).all()
            && self.heading.is_finite()
            && self.heading.abs() < 100_000.0
            && self.turret_yaw.is_finite()
            && self.turret_yaw.abs() < 100_000.0
            && self.speed.is_finite()
            && self.speed.abs() <= 100.0
            && self.fuel.is_finite()
            && (0.0..=s.fuel_capacity).contains(&self.fuel)
            && unit(self.battery)
            && unit(self.tyre)
            && unit(self.engine)
            && (!self.docked || self.kind == VehicleKind::Ubike)
            && match &self.gun {
                Some(g) => s.armored && g.validate(self.kind),
                None => !s.armored,
            }
    }
}

pub fn footprint(pos: Vec2, heading: f32, length: f32, width: f32) -> [Vec2; 4] {
    let forward = Vec2::new(heading.sin(), heading.cos());
    let right = Vec2::new(heading.cos(), -heading.sin());
    [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]
        .map(|(x, y)| pos + right * (x * width) + forward * (y * length))
}

#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleFleet {
    pub vehicles: Vec<VehicleState>,
    pub current: Option<u32>,
}

impl Default for VehicleFleet {
    fn default() -> Self {
        Self::planned(crate::map::world())
    }
}

impl VehicleFleet {
    pub fn current(&self) -> Option<&VehicleState> {
        self.current.and_then(|id| self.get(id))
    }
    pub fn current_mut(&mut self) -> Option<&mut VehicleState> {
        self.current.and_then(|id| self.get_mut(id))
    }
    pub fn get(&self, id: u32) -> Option<&VehicleState> {
        self.vehicles.iter().find(|v| v.id == id)
    }
    pub fn get_mut(&mut self, id: u32) -> Option<&mut VehicleState> {
        self.vehicles.iter_mut().find(|v| v.id == id)
    }
    pub fn nearest(&self, pos: Vec2, radius: f32) -> Option<&VehicleState> {
        self.vehicles
            .iter()
            .filter_map(|v| {
                let d = v.position().distance(pos) - v.spec().length * 0.3;
                (d < radius).then_some((v, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(v, _)| v)
    }
    pub fn reachable(&self, id: u32, pos: Vec2, obstacles: &Obstacles) -> bool {
        self.get(id).is_some_and(|v| {
            v.position().distance(pos) - v.spec().length * 0.3 < 2.6
                && obstacles
                    .segment_hit_except(pos, v.position(), 1.0, 1.0, Some(v.id as u64))
                    .is_none()
        })
    }
    pub fn hint(&self, pos: Vec2, lang: Language) -> Option<(String, String)> {
        if let Some(v) = self.current() {
            return Some((
                tr(lang, "下車", "Exit vehicle").into(),
                v.spec().name(lang).into(),
            ));
        }
        self.nearest(pos, 2.6).map(|v| {
            (
                tr(lang, "檢查／上車", "Inspect / board").into(),
                v.status(lang),
            )
        })
    }
    pub fn validate(&self) -> bool {
        let mut ids = HashSet::new();
        self.vehicles.len() <= 256
            && self
                .vehicles
                .iter()
                .all(|v| ids.insert(v.id) && v.valid(crate::map::world()))
            && self
                .current
                .is_none_or(|id| self.get(id).is_some_and(|v| v.spec().drive && !v.docked))
    }
    pub fn add_captured(&mut self, kind: VehicleKind, pos: Vec2, heading: f32) -> u32 {
        let id = self
            .vehicles
            .iter()
            .map(|v| v.id)
            .max()
            .map_or(0, |id| id + 1);
        let mut rng = crate::map::data::Park((pos.x.abs() * 13.0 + pos.y.abs() * 7.0) as u64 | 1);
        let mut v = VehicleState::new(id, kind, pos, heading, &mut || rng.f());
        v.engine = 0.4;
        self.vehicles.push(v);
        id
    }
    pub fn publish_colliders(&self, obstacles: &Obstacles) {
        obstacles.replace_dynamic(
            1,
            self.vehicles.iter().map(VehicleState::collider).collect(),
        );
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Hotwire {
    pub id: u32,
    pub remaining: f32,
}

#[derive(Resource, Default, Debug)]
pub struct VehicleRuntime {
    pub hotwire: Option<Hotwire>,
    pub noise_timer: f32,
    pub entered: u32,
    pub exited: u32,
    pub collisions: u32,
    pub distance_driven: f32,
    pub fuel_used: f32,
}

#[derive(Debug, Default)]
pub struct VehicleOutcome {
    pub text: String,
    pub noise: f32,
    pub changed: bool,
}

impl VehicleOutcome {
    fn message(lang: Language, zh: &'static str, en: &'static str) -> Self {
        Self {
            text: tr(lang, zh, en).into(),
            ..default()
        }
    }
}

pub fn enter(
    fleet: &mut VehicleFleet,
    runtime: &mut VehicleRuntime,
    id: u32,
    inventory: &Inventory,
    skills: Skills,
    lang: Language,
) -> VehicleOutcome {
    if fleet.current.is_some() {
        return VehicleOutcome::default();
    }
    let Some(v) = fleet.get_mut(id) else {
        return VehicleOutcome::default();
    };
    let s = v.spec();
    if !s.drive {
        return VehicleOutcome::message(
            lang,
            "這台大型車輛無法駕駛，可作為掩護。",
            "This large vehicle cannot be driven; use it as cover.",
        );
    }
    let mut noise = 0.0;
    if v.docked {
        if inventory.count("hammer") == 0 && inventory.count("pipe") == 0 {
            return VehicleOutcome::message(
                lang,
                "單車仍鎖在停車柱上，需要鐵鎚或鐵管撬開。",
                "The bicycle is docked. A hammer or pipe is needed to pry it free.",
            );
        }
        v.docked = false;
        noise = 26.0;
    }
    if s.key && !v.key {
        if !skills.lockpick {
            return VehicleOutcome::message(
                lang,
                "沒有鑰匙，水電師傅能接線發動。",
                "No key. A handyman can hot-wire the ignition.",
            );
        }
        match runtime.hotwire {
            Some(wire) if wire.id == id && wire.remaining <= 0.0 => {
                v.key = true;
                runtime.hotwire = None;
            }
            Some(wire) if wire.id == id => {
                return VehicleOutcome {
                    text: format!(
                        "{} {:.1} s",
                        tr(lang, "接線中，還需", "Hot-wiring; remaining"),
                        wire.remaining
                    ),
                    ..default()
                };
            }
            _ => {
                runtime.hotwire = Some(Hotwire {
                    id,
                    remaining: if skills.repair > 0 { 2.5 } else { 4.0 },
                });
                return VehicleOutcome::message(
                    lang,
                    "開始接線，留在車旁，完成後再互動發動。",
                    "Hot-wiring started. Stay beside the vehicle and interact again when ready.",
                );
            }
        }
    }
    if !s.pedal && !s.electric && v.fuel <= 0.05 {
        return VehicleOutcome::message(
            lang,
            "油箱空了，需要汽油或塑膠管抽油。",
            "The fuel tank is empty. Find gasoline or a siphon hose.",
        );
    }
    if s.electric && v.fuel <= 0.02 {
        return VehicleOutcome::message(
            lang,
            "電量耗盡，供電恢復後停放充電。",
            "The battery is empty. Park it to charge while grid power is available.",
        );
    }
    if !s.pedal && v.battery < 0.15 {
        return VehicleOutcome::message(
            lang,
            "啟動電瓶沒電，需要汽車電瓶。",
            "The starter battery is flat. A car battery is needed.",
        );
    }
    if !s.pedal && v.engine < 0.1 {
        return VehicleOutcome::message(
            lang,
            "引擎損壞，需要工具修理。",
            "The engine is broken. Repair it with tools.",
        );
    }
    let text = format!("{} {}", tr(lang, "已上車：", "Boarded:"), s.name(lang));
    fleet.current = Some(id);
    runtime.entered += 1;
    VehicleOutcome {
        text,
        noise,
        changed: true,
    }
}

pub fn refuel(v: &mut VehicleState, inventory: &mut Inventory, lang: Language) -> VehicleOutcome {
    let s = v.spec();
    if s.pedal || s.electric || s.fuel_capacity <= 0.0 {
        return VehicleOutcome::message(
            lang,
            "這台車不能添加汽油。",
            "This vehicle does not accept gasoline.",
        );
    }
    // Inventory gasoline is one whole litre per item. Do not create an
    // over-capacity tank through Web's ceil(capacity-fuel) rounding edge case.
    let amount = inventory
        .count("gas")
        .min((s.fuel_capacity - v.fuel).floor().max(0.0) as u32);
    if amount == 0 {
        return VehicleOutcome::message(
            lang,
            "沒有可添加的汽油，或油箱已滿。",
            "No gasoline available, or the tank is full.",
        );
    }
    inventory.take("gas", amount);
    v.fuel += amount as f32;
    VehicleOutcome {
        text: format!("{} {amount} L", tr(lang, "已添加汽油", "Added gasoline")),
        changed: true,
        noise: 0.0,
    }
}

pub fn siphon(v: &mut VehicleState, inventory: &mut Inventory, lang: Language) -> VehicleOutcome {
    let s = v.spec();
    if s.pedal || s.electric || s.fuel_capacity <= 0.0 {
        return VehicleOutcome::message(
            lang,
            "這台車沒有汽油油箱。",
            "This vehicle has no gasoline tank.",
        );
    }
    if inventory.count("hose") == 0 {
        return VehicleOutcome::message(
            lang,
            "需要塑膠管才能抽油。",
            "A plastic hose is needed to siphon fuel.",
        );
    }
    let available = (v.fuel.floor().max(0.0) as u32).min(5);
    let amount = crate::worldplay::pickable_count(inventory, "gas", available);
    if amount == 0 {
        return VehicleOutcome::message(
            lang,
            "油量不足一公升，或背包已滿。",
            "Less than one litre remains, or the backpack is full.",
        );
    }
    v.fuel -= amount as f32;
    inventory.add("gas", amount);
    VehicleOutcome {
        text: format!("{} {amount} L", tr(lang, "已抽出汽油", "Siphoned gasoline")),
        noise: 4.0,
        changed: true,
    }
}

pub fn repair(
    v: &mut VehicleState,
    inventory: &mut Inventory,
    skills: Skills,
    lang: Language,
) -> VehicleOutcome {
    let mut parts = Vec::new();
    let mut noise = 0.0;
    if v.tyre < 0.9 && inventory.take("patch", 1) {
        v.tyre = 1.0;
        parts.push(tr(lang, "輪胎已修補", "Tire patched"));
    }
    if v.battery < 0.5 && !v.spec().pedal && inventory.take("carbat", 1) {
        v.battery = 1.0;
        parts.push(tr(lang, "電瓶已更換", "Battery replaced"));
    }
    if v.engine < 0.7
        && !v.spec().pedal
        && (inventory.count("hammer") > 0 || inventory.count("pipe") > 0)
    {
        v.engine = (v.engine + if skills.repair > 0 { 0.6 } else { 0.25 }).min(1.0);
        parts.push(tr(lang, "引擎已修理", "Engine repaired"));
        noise = 12.0;
    }
    VehicleOutcome {
        changed: !parts.is_empty(),
        text: if parts.is_empty() {
            tr(
                lang,
                "不需要修理，或缺少修理用品。",
                "No repair needed, or required supplies are missing.",
            )
            .into()
        } else {
            parts.join(" · ")
        },
        noise,
    }
}

/// Pure port of DriveSystem.update's ground acceleration/steering. Collision
/// is supplied by the shared live world and swept in short travel increments.
pub fn drive_step(
    v: &mut VehicleState,
    want: Vec2,
    dt: f32,
    ride: f32,
    blocked: impl Fn([Vec2; 4]) -> bool,
) -> f32 {
    let s = v.spec();
    let m = want.length().min(1.0);
    let vmax = s.vmax
        * if s.two && ride > 0.0 { ride } else { 1.0 }
        * (0.45 + 0.55 * v.tyre)
        * if s.pedal { 1.0 } else { 0.5 + 0.5 * v.engine };
    let mut target = 0.0;
    let previous_heading = v.heading;
    if m > 0.1 {
        // Match JavaScript's f64 trigonometry at exactly pi. Evaluating sin(pi)
        // in f32 flips its tiny rounding sign and reverses steering when a
        // backing vehicle crosses the source's 2 m/s direction threshold.
        let wanted = (want.x as f64).atan2(want.y as f64);
        let wrap = |a: f64| a.sin().atan2(a.cos()) as f32;
        let mut delta = wrap(wanted - v.heading as f64);
        if delta.abs() > 2.4 && v.speed.abs() < 2.0 {
            target = -vmax * 0.3 * m;
            delta = wrap(wanted + std::f64::consts::PI - v.heading as f64);
        } else {
            target = vmax * m * (delta * 0.6).cos().max(0.25);
        }
        let steer =
            s.turn * (v.speed.abs() / 3.0 + 0.15).min(1.0) * if v.speed < 0.0 { -1.0 } else { 1.0 };
        v.heading += delta.clamp(-1.0, 1.0) * steer * dt * if v.tyre < 0.5 { 0.7 } else { 1.0 };
        v.heading = angle(v.heading);
    }
    if (!s.pedal && v.fuel <= 0.0) || (!s.pedal && v.engine < 0.1) {
        target = 0.0;
    }
    v.speed += (target - v.speed).clamp(-s.acceleration * 2.0 * dt, s.acceleration * dt);
    let from = v.position();
    let next = from + Vec2::new(v.heading.sin(), v.heading.cos()) * v.speed * dt;
    let mut impact = 0.0;
    if blocked(v.corners_at(next, v.heading)) {
        impact = v.speed.abs();
        v.heading = previous_heading;
        if impact > 6.0 {
            v.engine = (v.engine - impact * 0.02).max(0.0);
            v.tyre = (v.tyre - impact * 0.01).max(0.0);
        }
        v.speed *= -0.15;
    } else {
        v.pos = next.into();
    }
    if !s.pedal {
        v.fuel = (v.fuel - s.burn * dt * (0.2 + v.speed.abs() / s.vmax.max(1.0))).max(0.0);
    }
    impact
}

pub fn angle(v: f32) -> f32 {
    v.sin().atan2(v.cos())
}
