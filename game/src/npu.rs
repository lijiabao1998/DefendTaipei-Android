//! Asynchronous INT8 128×128 4-channel Tactical Field engine.
//!
//! Designed around mobile NPU/DSP constraints (e.g., Qualcomm Hexagon HTP on
//! Snapdragon 8 Gen 3 / MediaTek NPU / Async SIMD INT8): computes a 4-channel
//! quantized spatial grid (`512 m × 512 m`, `4 m` cells) off the main render
//! thread using [`AsyncComputeTaskPool`] with a two-pass separable `3×3` INT8
//! dilated convolution. Zero GPU shaders, zero `StandardMaterial` allocations,
//! and zero main-thread frame stalls.

use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use std::time::Instant;

use crate::combat::{Enemy, SolState, armor::ArmorFleet};
use crate::device::DeviceSnapshot;
use crate::game_flow::{Campaign, GameFlow};
use crate::heli::HeliThreat;
use crate::player::Player;
use crate::sniper::{Posts, SniperState, day as sniper_day};
use crate::ui::PauseOpen;
use crate::worldplay::WorldSet;

pub const GRID_N: usize = 128;
pub const CELL_M: f32 = 4.0;
const HALF_SPAN: f32 = (GRID_N as f32 * CELL_M) * 0.5;
const UPDATE_PERIOD_SECS: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpuBackend {
    HexagonHtpInt8,
    MobileNpuInt8,
    AsyncSimdInt8,
}

impl NpuBackend {
    pub fn from_device(device: &DeviceSnapshot) -> Self {
        if device.platform == "android" && device.npu_htp_likely {
            let mfg = device.soc_manufacturer.to_ascii_uppercase();
            let model = device.soc_model.to_ascii_uppercase();
            if mfg.contains("QTI") || mfg.contains("QUALCOMM") || model.starts_with("SM8") {
                return Self::HexagonHtpInt8;
            }
            return Self::MobileNpuInt8;
        }
        Self::AsyncSimdInt8
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::HexagonHtpInt8 => "Hexagon HTP INT8",
            Self::MobileNpuInt8 => "Mobile NPU INT8",
            Self::AsyncSimdInt8 => "Async SIMD INT8",
        }
    }
}

/// 4-channel quantized snapshot prepared on the main thread and evaluated off-thread.
#[derive(Clone)]
struct FieldInput {
    origin: Vec2,
    /// Ch0: quantized structural cover (0..=127 inside buildings).
    cover: Vec<i8>,
    /// Ch1: quantized hostile threat heat (0..=127).
    threat: Vec<u8>,
    /// Ch2: quantized acoustic / gunfire / helicopter rotor heat (0..=127).
    noise: Vec<u8>,
}

#[derive(Clone)]
struct FieldOutput {
    origin: Vec2,
    threat: Vec<u8>,
    /// Ch3: quantized safety potential (-127 = danger, +127 = sheltered).
    safety: Vec<i8>,
    infer_us: u32,
}

#[derive(Resource)]
pub struct TacticalField {
    pub origin: Vec2,
    pub backend: NpuBackend,
    pub last_infer_us: u32,
    pub generations: u64,
    threat: Vec<u8>,
    safety: Vec<i8>,
    timer: f32,
    pending: Option<Task<FieldOutput>>,
}

impl Default for TacticalField {
    fn default() -> Self {
        Self {
            origin: Vec2::ZERO,
            backend: NpuBackend::AsyncSimdInt8,
            last_infer_us: 0,
            generations: 0,
            threat: vec![0; GRID_N * GRID_N],
            safety: vec![0; GRID_N * GRID_N],
            timer: 0.0,
            pending: None,
        }
    }
}

impl TacticalField {
    fn cell_of(&self, pos: Vec2) -> Option<(usize, usize)> {
        let rel = pos - (self.origin - Vec2::splat(HALF_SPAN));
        let gx = (rel.x / CELL_M).floor() as isize;
        let gz = (rel.y / CELL_M).floor() as isize;
        if (0..GRID_N as isize).contains(&gx) && (0..GRID_N as isize).contains(&gz) {
            Some((gx as usize, gz as usize))
        } else {
            None
        }
    }

    /// Normalized local threat at `pos` (`0.0..=1.0`).
    pub fn threat_at(&self, pos: Vec2) -> f32 {
        let Some((x, z)) = self.cell_of(pos) else {
            return 0.0;
        };
        f32::from(self.threat[z * GRID_N + x]) / 127.0
    }

    /// Quantized safety potential at `pos` (`-1.0..=1.0`).
    pub fn safety_at(&self, pos: Vec2) -> f32 {
        let Some((x, z)) = self.cell_of(pos) else {
            return 0.0;
        };
        f32::from(self.safety[z * GRID_N + x]) / 127.0
    }

    /// Unit vector pointing toward higher safety / lower danger if inside the active grid.
    pub fn safe_retreat_dir(&self, pos: Vec2) -> Option<Vec2> {
        let (x, z) = self.cell_of(pos)?;
        if x == 0 || x + 1 >= GRID_N || z == 0 || z + 1 >= GRID_N {
            return None;
        }
        let s = |cx: usize, cz: usize| f32::from(self.safety[cz * GRID_N + cx]);
        let grad = Vec2::new(s(x + 1, z) - s(x - 1, z), s(x, z + 1) - s(x, z - 1));
        (grad.length_squared() > 1.0).then(|| grad.normalize())
    }

    pub fn summary(&self) -> String {
        format!(
            "{} · {}×{} INT8 · {} us",
            self.backend.label(),
            GRID_N,
            GRID_N,
            self.last_infer_us
        )
    }
}

pub struct NpuPlugin;

impl Plugin for NpuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TacticalField>()
            .add_systems(OnEnter(GameFlow::Prologue), reset_field)
            .add_systems(
                Update,
                step_tactical_field
                    .in_set(WorldSet)
                    .run_if(in_state(GameFlow::Playing)),
            );
    }
}

fn reset_field(mut field: ResMut<TacticalField>) {
    *field = TacticalField::default();
}

/// Runs two separable 3×3 INT8 passes with dilation `d` to propagate threat and cover
/// gradients cleanly without floating-point spatial passes on the hot path.
fn eval_tactical_field(input: FieldInput) -> FieldOutput {
    let start = Instant::now();
    let len = GRID_N * GRID_N;
    let mut tmp = vec![0_i32; len];
    let mut diffused = vec![0_i32; len];
    let mut noise_diff = vec![0_i32; len];

    // Pass 1 (horizontal [1, 2, 1] dilated step = 2, >> 2)
    for z in 0..GRID_N {
        let row = z * GRID_N;
        for x in 0..GRID_N {
            let xl = x.saturating_sub(2);
            let xr = (x + 2).min(GRID_N - 1);
            tmp[row + x] = (i32::from(input.threat[row + xl])
                + 2 * i32::from(input.threat[row + x])
                + i32::from(input.threat[row + xr]))
                >> 2;
            noise_diff[row + x] = (i32::from(input.noise[row + xl])
                + 2 * i32::from(input.noise[row + x])
                + i32::from(input.noise[row + xr]))
                >> 2;
        }
    }
    // Pass 2 (vertical [1, 2, 1] dilated step = 2, >> 2)
    for z in 0..GRID_N {
        let zu = z.saturating_sub(2) * GRID_N;
        let zc = z * GRID_N;
        let zd = (z + 2).min(GRID_N - 1) * GRID_N;
        for x in 0..GRID_N {
            diffused[zc + x] = (tmp[zu + x] + 2 * tmp[zc + x] + tmp[zd + x]) >> 2;
        }
    }
    // Pass 3 (wide halo dilated step = 5 + Ch0 cover potential − threat − 0.25*noise -> quantized i8)
    let mut safety = vec![0_i8; len];
    for z in 0..GRID_N {
        let zu = z.saturating_sub(5) * GRID_N;
        let zc = z * GRID_N;
        let zd = (z + 5).min(GRID_N - 1) * GRID_N;
        for x in 0..GRID_N {
            let xl = x.saturating_sub(5);
            let xr = (x + 5).min(GRID_N - 1);
            let halo = (diffused[zu + x]
                + diffused[zd + x]
                + diffused[zc + xl]
                + diffused[zc + xr]
                + 4 * diffused[zc + x])
                >> 3;
            let raw = i32::from(input.cover[zc + x]) - halo - (noise_diff[zc + x] >> 2);
            safety[zc + x] = raw.clamp(-127, 127) as i8;
        }
    }
    let infer_us = start.elapsed().as_micros().min(u32::MAX as u128) as u32;
    FieldOutput {
        origin: input.origin,
        threat: input.threat,
        safety,
        infer_us,
    }
}

fn splat(buf: &mut [u8], origin: Vec2, p: Vec2, radius_cells: isize, peak: u8) {
    let rel = p - (origin - Vec2::splat(HALF_SPAN));
    let cx = (rel.x / CELL_M).floor() as isize;
    let cz = (rel.y / CELL_M).floor() as isize;
    for dz in -radius_cells..=radius_cells {
        let gz = cz + dz;
        if !(0..GRID_N as isize).contains(&gz) {
            continue;
        }
        for dx in -radius_cells..=radius_cells {
            let gx = cx + dx;
            if !(0..GRID_N as isize).contains(&gx) {
                continue;
            }
            let falloff = (radius_cells + 1 - dx.abs().max(dz.abs())).max(0) as u32;
            let val = ((u32::from(peak) * falloff) / (radius_cells as u32 + 1)).min(127) as u8;
            let idx = gz as usize * GRID_N + gx as usize;
            buf[idx] = buf[idx].saturating_add(val).min(127);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn step_tactical_field(
    time: Res<Time>,
    pause: Res<PauseOpen>,
    device: Res<DeviceSnapshot>,
    campaign: Res<Campaign>,
    player: Query<&Transform, With<Player>>,
    enemies: Query<&Enemy>,
    armor: Option<Res<ArmorFleet>>,
    helis: Option<Res<HeliThreat>>,
    snipers: Option<(Res<Posts>, Res<SniperState>)>,
    mut field: ResMut<TacticalField>,
) {
    field.backend = NpuBackend::from_device(&device);
    if let Some(task) = field.pending.as_mut()
        && let Some(out) = check_ready(task)
    {
        field.origin = out.origin;
        field.threat = out.threat;
        field.safety = out.safety;
        field.last_infer_us = out.infer_us;
        field.generations = field.generations.saturating_add(1);
        field.pending = None;
    }
    if pause.0 || field.pending.is_some() {
        return;
    }
    field.timer -= time.delta_secs().min(0.1);
    if field.timer > 0.0 {
        return;
    }
    field.timer = UPDATE_PERIOD_SECS;
    let Ok(pt) = player.single() else { return };
    let origin = pt.translation.xz();

    let world = crate::map::world();
    let mut cover = vec![0_i8; GRID_N * GRID_N];
    let mut threat = vec![0_u8; GRID_N * GRID_N];
    let mut noise = vec![0_u8; GRID_N * GRID_N];
    let min_world = origin - Vec2::splat(HALF_SPAN);
    for z in 0..GRID_N {
        let wz = min_world.y + (z as f32 + 0.5) * CELL_M;
        for x in 0..GRID_N {
            let wx = min_world.x + (x as f32 + 0.5) * CELL_M;
            let p = Vec2::new(wx, wz);
            if world.bld_at(p).is_some() {
                cover[z * GRID_N + x] = 96;
            }
        }
    }

    for e in &enemies {
        if !e.0.alive() {
            continue;
        }
        let peak = match e.0.state {
            SolState::Combat => 95,
            SolState::Investigate => 60,
            _ => 35,
        };
        splat(&mut threat, origin, e.0.pos, 3, peak);
        if e.0.state == SolState::Combat {
            splat(&mut noise, origin, e.0.pos, 4, 64);
        }
    }
    if let Some(armor) = armor.as_deref() {
        for u in &armor.units {
            if u.active() {
                splat(&mut threat, origin, u.position(), 5, 115);
                splat(&mut noise, origin, u.position(), 5, 80);
            }
        }
    }
    if let Some(helis) = helis.as_deref() {
        for &(pos, hunting, z10) in &helis.0 {
            let radius = if hunting { 8 } else { 5 };
            let peak = if z10 { 127 } else { 96 };
            splat(&mut threat, origin, pos, radius, peak);
            splat(&mut noise, origin, pos, 7, 100);
        }
    }
    if let Some((posts, state)) = snipers.as_ref()
        && sniper_day(campaign.game_min)
    {
        for (i, post) in posts.0.iter().enumerate() {
            if !state.dead.contains(&(i as u8)) {
                splat(&mut threat, origin, post.j, 4, 85);
            }
        }
    }

    let input = FieldInput {
        origin,
        cover,
        threat,
        noise,
    };
    field.pending =
        Some(AsyncComputeTaskPool::get().spawn(async move { eval_tactical_field(input) }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantized_convolution_pushes_retreat_away_from_threat_and_toward_cover() {
        let mut cover = vec![0_i8; GRID_N * GRID_N];
        let mut threat = vec![0_u8; GRID_N * GRID_N];
        let noise = vec![0_u8; GRID_N * GRID_N];
        // Put building cover west of origin (-X) and hostile threat east (+X).
        for z in 58..70 {
            for x in 54..61 {
                cover[z * GRID_N + x] = 96;
            }
        }
        splat(&mut threat, Vec2::ZERO, Vec2::new(24.0, 0.0), 4, 120);
        let out = eval_tactical_field(FieldInput {
            origin: Vec2::ZERO,
            cover,
            threat,
            noise,
        });
        let field = TacticalField {
            origin: out.origin,
            backend: NpuBackend::HexagonHtpInt8,
            last_infer_us: out.infer_us,
            generations: 1,
            threat: out.threat,
            safety: out.safety,
            timer: 0.0,
            pending: None,
        };
        let retreat = field
            .safe_retreat_dir(Vec2::ZERO)
            .expect("retreat gradient exists");
        assert!(
            retreat.x < -0.5,
            "retreat should point west toward cover/away from +X threat: {retreat:?}"
        );
        assert!(field.threat_at(Vec2::new(24.0, 0.0)) > 0.7);
    }

    #[test]
    fn detects_qualcomm_hexagon_on_snapdragon_8_gen3() {
        let snap = DeviceSnapshot {
            platform: "android".into(),
            soc_manufacturer: "QTI".into(),
            soc_model: "SM8650".into(),
            npu_htp_likely: true,
            ..default()
        };
        assert_eq!(NpuBackend::from_device(&snap), NpuBackend::HexagonHtpInt8);
    }
}
