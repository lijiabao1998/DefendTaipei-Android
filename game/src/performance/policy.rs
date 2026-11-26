//! Conservative capability policy. RAM is a memory constraint, never a GPU score.
use super::{PerformancePreferences, PerformanceProfile};
use crate::device::DeviceSnapshot;
use bevy::prelude::*;
use serde::Serialize;

#[derive(Resource, Clone, Debug, PartialEq, Serialize)]
pub struct EffectivePerformance {
    pub target_fps: u16,
    pub render_scale: u8,
    pub reason: String,
    pub adaptive_step: u8,
}
impl Default for EffectivePerformance {
    fn default() -> Self {
        Self {
            target_fps: 60,
            render_scale: 100,
            reason: "initializing".into(),
            adaptive_step: 0,
        }
    }
}

/// A requested cap is not a promise that the OS will select that display mode.
/// Prefer rates which divide a reported same-resolution display mode evenly.
pub fn supported_cap(request: u16, device: &DeviceSnapshot) -> u16 {
    let rates = &device.supported_refresh_hz;
    if rates.is_empty() {
        return request.min(60);
    }
    for candidate in [120, 90, 60, 30] {
        if candidate > request {
            continue;
        }
        if rates.iter().any(|hz| {
            let divisor = (*hz / candidate as f32).round();
            divisor >= 1.0 && (*hz - candidate as f32 * divisor).abs() < 1.1
        }) {
            return candidate;
        }
    }
    // Unusual 50/144 Hz-only displays: a rate at or below the requested cap.
    // No unreported high-refresh mode is inferred from the phone's name.
    let hz = rates.iter().copied().fold(0.0, f32::max);
    if hz >= 20.0 {
        (hz / (hz / request as f32).ceil()).round() as u16
    } else {
        request.min(60)
    }
}

pub fn resolve(
    preferences: PerformancePreferences,
    device: &DeviceSnapshot,
    adaptive_step: u8,
    android: bool,
) -> EffectivePerformance {
    let p = preferences.sanitize();
    let mut cap = supported_cap(p.frame_cap, device);
    let mut scale = match p.profile {
        PerformanceProfile::Auto if android => 85,
        PerformanceProfile::Auto | PerformanceProfile::Quality => 100,
        PerformanceProfile::Balanced => 85,
        PerformanceProfile::Battery => 70,
    };
    let mut reason = match p.profile {
        PerformanceProfile::Auto => "automatic",
        PerformanceProfile::Battery => "battery profile",
        PerformanceProfile::Balanced => "balanced profile",
        PerformanceProfile::Quality => "quality profile",
    };
    if p.profile == PerformanceProfile::Battery {
        cap = cap.min(30);
    }
    if p.profile == PerformanceProfile::Auto
        && (device.low_ram == Some(true)
            || device
                .total_memory_bytes
                .is_some_and(|bytes| bytes < 4 * 1024 * 1024 * 1024))
    {
        scale = 70;
        cap = cap.min(30);
        reason = "limited memory";
    }
    if p.render_scale != 0 {
        scale = p.render_scale;
    }
    let step = adaptive_step.min(3);
    if p.profile == PerformanceProfile::Auto {
        if p.render_scale == 0 {
            scale = match step {
                0 => scale,
                1 => {
                    if scale <= 85 {
                        scale.min(70)
                    } else {
                        85
                    }
                }
                _ => 70,
            };
        }
        if step >= 3 {
            cap = cap.min(30);
        }
        if step > 0 {
            reason = "sustained slow frames";
        }
    }
    if device.is_fresh() {
        if device.power_save == Some(true) {
            cap = cap.min(30);
            scale = scale.min(70);
            reason = "system battery saver";
        }
        if device.low_memory == Some(true) {
            cap = cap.min(30);
            scale = scale.min(70);
            reason = "system memory pressure";
        }
        let headroom = device.thermal_headroom.filter(|_| {
            device
                .thermal_headroom_age_ms
                .is_some_and(|age| age <= 20_000)
        });
        if device.thermal_status.is_some_and(|level| level >= 3)
            || headroom.is_some_and(|value| value >= 1.0)
        {
            cap = cap.min(30);
            scale = scale.min(70);
            reason = "thermal protection";
        } else if device.thermal_status == Some(2) || headroom.is_some_and(|value| value >= 0.85) {
            cap = cap.min(60);
            scale = scale.min(85);
            reason = "thermal caution";
        }
    }
    EffectivePerformance {
        target_fps: cap.max(15),
        render_scale: scale,
        reason: reason.into(),
        adaptive_step: step,
    }
}

/// Immediate thermal protection with deliberately slower recovery. Fresh
/// readings must remain below the recovery thresholds for 15 seconds before
/// dropping one level (severe -> caution -> normal). A stale bridge or unknown
/// status cannot establish cooling: retain the last thermal limit, label that
/// uncertainty, and restart the recovery window when observations return.
///
/// This latch survives preference changes. Background time and long update
/// gaps do not count as verified cooling, so resuming an activity cannot skip
/// the recovery window. Unsupported devices that never trigger a thermal
/// limit remain unaffected.
#[derive(Default, Debug)]
pub struct ThermalRecovery {
    level: u8,
    cool_secs: f32,
}

impl ThermalRecovery {
    pub fn apply(
        &mut self,
        next: &mut EffectivePerformance,
        device: &DeviceSnapshot,
        elapsed: f32,
    ) {
        let requested_level = match next.reason.as_str() {
            "thermal protection" => 2,
            "thermal caution" => 1,
            _ => 0,
        };
        if requested_level >= self.level {
            self.level = requested_level;
            self.cool_secs = 0.0;
        } else if self.level > 0 {
            let headroom = device.thermal_headroom.filter(|value| {
                value.is_finite()
                    && *value >= 0.0
                    && device
                        .thermal_headroom_age_ms
                        .is_some_and(|age| age <= 20_000)
            });
            let (status_limit, headroom_limit) = if self.level == 2 {
                (3, 0.90)
            } else {
                (2, 0.75)
            };
            let measured_cool = device.is_fresh()
                && device.foreground != Some(false)
                && device
                    .thermal_status
                    .is_some_and(|status| status < status_limit)
                && headroom.is_none_or(|value| value < headroom_limit);
            // This is called each app frame. A long stalled/background interval
            // says nothing about how long the device was actually cool.
            // Software renderers and genuinely slow phones can take >1 s per
            // update. Credit at most one observed second, without requiring
            // an unrealistically fast frame to recover. Very long gaps and
            // explicit lifecycle discontinuities (elapsed=0) restart the hold.
            let continuous = elapsed.is_finite() && elapsed > 0.0 && elapsed <= 5.0;
            if measured_cool && continuous {
                self.cool_secs += elapsed.min(1.0);
                if self.cool_secs >= 15.0 {
                    self.level -= 1;
                    self.cool_secs = 0.0;
                }
            } else {
                self.cool_secs = 0.0;
            }
        }
        if self.level == 0 {
            return;
        }
        let (cap, scale) = if self.level == 2 { (30, 70) } else { (60, 85) };
        let adds_constraint = next.target_fps > cap || next.render_scale > scale;
        next.target_fps = next.target_fps.min(cap);
        next.render_scale = next.render_scale.min(scale);
        // An already equally/more restrictive independent limit retains its
        // real reason (e.g. battery saver or low-memory protection).
        if adds_constraint || requested_level > 0 {
            let unavailable = !device.is_fresh() || device.thermal_status.is_none();
            next.reason = match (self.level, requested_level, unavailable) {
                (2, 2, _) => "thermal protection",
                (1, 1, _) => "thermal caution",
                (2, _, true) => "thermal protection readings unavailable",
                (1, _, true) => "thermal caution readings unavailable",
                (2, _, false) => "thermal protection recovery",
                _ => "thermal caution recovery",
            }
            .into();
        }
    }
}

/// Three consecutive poor two-second windows lower one step. Recovery requires
/// thirty seconds of both good current intervals and measured headroom for the
/// next cap. A stable 30 FPS interval alone cannot prove the app can do 60 FPS.
#[derive(Default, Debug)]
pub struct AdaptiveController {
    pub step: u8,
    poor: u8,
    healthy_secs: f32,
}
impl AdaptiveController {
    pub fn observe(
        &mut self,
        p95_ms: f64,
        budget_ms: f64,
        unpaced_p95_ms: f64,
        recovery_budget_ms: f64,
        elapsed: f32,
    ) {
        if !p95_ms.is_finite()
            || p95_ms <= 0.0
            || !budget_ms.is_finite()
            || budget_ms <= 0.0
            || !elapsed.is_finite()
            || elapsed <= 0.0
        {
            self.poor = 0;
            self.healthy_secs = 0.0;
            return;
        }
        if p95_ms > budget_ms * 1.25 {
            self.healthy_secs = 0.0;
            self.poor = self.poor.saturating_add(1);
            if self.poor >= 3 {
                self.step = (self.step + 1).min(3);
                self.poor = 0;
            }
        } else {
            self.poor = 0;
            let recovery_headroom = unpaced_p95_ms.is_finite()
                && unpaced_p95_ms >= 0.0
                && recovery_budget_ms.is_finite()
                && recovery_budget_ms > 0.0
                && unpaced_p95_ms <= recovery_budget_ms * 0.80;
            if p95_ms <= budget_ms * 1.10 && recovery_headroom {
                self.healthy_secs += elapsed;
                if self.healthy_secs >= 30.0 {
                    self.step = self.step.saturating_sub(1);
                    self.healthy_secs = 0.0;
                }
            } else {
                self.healthy_secs = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_devices_never_get_an_invented_high_refresh_mode() {
        let p = PerformancePreferences {
            frame_cap: 120,
            ..default()
        };
        let actual = resolve(p, &DeviceSnapshot::default(), 0, true);
        assert_eq!((actual.target_fps, actual.render_scale), (60, 85));
        let d = DeviceSnapshot {
            supported_refresh_hz: vec![60.0, 120.0],
            ..default()
        };
        assert_eq!(supported_cap(90, &d), 60);
        assert_eq!(supported_cap(120, &d), 120);
    }
    #[test]
    fn thermal_and_power_saver_override_manual_quality_but_stale_readings_do_not() {
        let p = PerformancePreferences {
            profile: PerformanceProfile::Quality,
            frame_cap: 120,
            render_scale: 100,
            ..default()
        };
        let mut d = DeviceSnapshot {
            reported: true,
            supported_refresh_hz: vec![120.0],
            thermal_status: Some(3),
            ..default()
        };
        assert_eq!(
            (
                resolve(p, &d, 0, true).target_fps,
                resolve(p, &d, 0, true).render_scale
            ),
            (30, 70)
        );
        d.sample_age_secs = 11.0;
        assert_eq!(resolve(p, &d, 0, true).target_fps, 120);
        d.sample_age_secs = 0.0;
        d.thermal_status = None;
        d.power_save = Some(true);
        assert_eq!(resolve(p, &d, 0, true).render_scale, 70);
    }
    #[test]
    fn hysteresis_requires_sustained_pressure_and_slow_recovery() {
        let mut a = AdaptiveController::default();
        a.observe(40.0, 16.67, 40.0, 16.67, 2.0);
        a.observe(40.0, 16.67, 40.0, 16.67, 2.0);
        assert_eq!(a.step, 0);
        a.observe(40.0, 16.67, 40.0, 16.67, 2.0);
        assert_eq!(a.step, 1);
        for _ in 0..14 {
            a.observe(16.8, 16.67, 10.0, 16.67, 2.0);
        }
        assert_eq!(a.step, 1);
        a.observe(16.8, 16.67, 10.0, 16.67, 2.0);
        assert_eq!(a.step, 0);
    }

    #[test]
    fn a_paced_thirty_fps_interval_does_not_prove_sixty_fps_headroom() {
        let mut a = AdaptiveController {
            step: 3,
            ..default()
        };
        for _ in 0..90 {
            a.observe(33.4, 33.33, 25.0, 16.67, 2.0);
        }
        assert_eq!(a.step, 3, "25 ms of work cannot sustain a 16.67 ms budget");
        for _ in 0..14 {
            a.observe(33.4, 33.33, 10.0, 16.67, 2.0);
        }
        assert_eq!(a.step, 3);
        a.observe(33.4, 33.33, 10.0, 16.67, 2.0);
        assert_eq!(a.step, 2);
    }

    fn quality_preferences() -> PerformancePreferences {
        PerformancePreferences {
            profile: PerformanceProfile::Quality,
            frame_cap: 120,
            render_scale: 100,
            ..default()
        }
    }

    fn thermal_device(status: u8, headroom: f32) -> DeviceSnapshot {
        DeviceSnapshot {
            reported: true,
            supported_refresh_hz: vec![120.0],
            thermal_status: Some(status),
            thermal_headroom: Some(headroom),
            thermal_headroom_age_ms: Some(0),
            foreground: Some(true),
            ..default()
        }
    }

    fn thermal_step(
        latch: &mut ThermalRecovery,
        device: &DeviceSnapshot,
        elapsed: f32,
    ) -> EffectivePerformance {
        let mut next = resolve(quality_preferences(), device, 0, true);
        latch.apply(&mut next, device, elapsed);
        next
    }

    #[test]
    fn thermal_recovery_demotes_in_two_observed_fifteen_second_windows() {
        let mut latch = ThermalRecovery::default();
        let hot = thermal_step(&mut latch, &thermal_device(3, 1.1), 0.0);
        assert_eq!((hot.target_fps, hot.render_scale), (30, 70));
        let caution = thermal_device(2, 0.89);
        for _ in 0..14 {
            assert_eq!(thermal_step(&mut latch, &caution, 1.0).target_fps, 30);
        }
        let next = thermal_step(&mut latch, &caution, 1.0);
        assert_eq!((next.target_fps, next.render_scale), (60, 85));
        let cool = thermal_device(1, 0.70);
        for _ in 0..14 {
            assert_eq!(thermal_step(&mut latch, &cool, 1.0).target_fps, 60);
        }
        let next = thermal_step(&mut latch, &cool, 1.0);
        assert_eq!((next.target_fps, next.render_scale), (120, 100));
        assert_eq!(next.reason, "quality profile");
    }

    #[test]
    fn boundary_oscillation_restarts_thermal_recovery_and_escalation_is_immediate() {
        let mut latch = ThermalRecovery::default();
        thermal_step(&mut latch, &thermal_device(3, 1.0), 0.0);
        for _ in 0..14 {
            thermal_step(&mut latch, &thermal_device(2, 0.89), 1.0);
        }
        assert_eq!(
            thermal_step(&mut latch, &thermal_device(2, 0.91), 1.0).target_fps,
            30,
        );
        for _ in 0..14 {
            assert_eq!(
                thermal_step(&mut latch, &thermal_device(2, 0.89), 1.0).target_fps,
                30,
            );
        }
        assert_eq!(
            thermal_step(&mut latch, &thermal_device(2, 0.89), 1.0).target_fps,
            60,
        );
        assert_eq!(
            thermal_step(&mut latch, &thermal_device(3, 1.0), 0.0).target_fps,
            30,
        );
    }

    #[test]
    fn stale_unknown_and_background_time_never_count_as_thermal_cooling() {
        let mut latch = ThermalRecovery::default();
        thermal_step(&mut latch, &thermal_device(3, 1.1), 0.0);
        let mut cool = thermal_device(0, 0.5);
        for _ in 0..14 {
            thermal_step(&mut latch, &cool, 1.0);
        }
        cool.sample_age_secs = 11.0;
        let held = thermal_step(&mut latch, &cool, 30.0);
        assert_eq!(held.target_fps, 30);
        assert_eq!(held.reason, "thermal protection readings unavailable");
        cool.sample_age_secs = 0.0;
        cool.thermal_status = None;
        for _ in 0..30 {
            assert_eq!(thermal_step(&mut latch, &cool, 1.0).target_fps, 30);
        }
        cool.thermal_status = Some(0);
        cool.foreground = Some(false);
        assert_eq!(thermal_step(&mut latch, &cool, 60.0).target_fps, 30);
        cool.foreground = Some(true);
        assert_eq!(thermal_step(&mut latch, &cool, 60.0).target_fps, 30);
        for _ in 0..14 {
            assert_eq!(thermal_step(&mut latch, &cool, 1.0).target_fps, 30);
        }
        assert_eq!(thermal_step(&mut latch, &cool, 1.0).target_fps, 60);
    }

    #[test]
    fn thermal_latch_preserves_an_independently_stricter_constraint_reason() {
        let mut latch = ThermalRecovery::default();
        thermal_step(&mut latch, &thermal_device(3, 1.1), 0.0);
        let mut device = thermal_device(0, 0.5);
        device.power_save = Some(true);
        let next = thermal_step(&mut latch, &device, 1.0);
        assert_eq!((next.target_fps, next.render_scale), (30, 70));
        assert_eq!(next.reason, "system battery saver");
    }

    #[test]
    fn first_android_auto_pressure_step_reduces_actual_resolution() {
        let device = DeviceSnapshot::default();
        let initial = resolve(PerformancePreferences::default(), &device, 0, true);
        let pressured = resolve(PerformancePreferences::default(), &device, 1, true);
        assert_eq!(initial.render_scale, 85);
        assert_eq!(pressured.render_scale, 70);
    }
}
