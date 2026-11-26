use super::render::RenderDimensions;
use super::{EffectivePerformance, PerformancePreferences, WARMUP_SECONDS};
use crate::device::{DeviceSnapshot, PERF_REPORT_FILENAME};
use bevy::render::renderer::RenderAdapterInfo;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FrameSample {
    pub ms: f64,
    pub target_fps: u16,
    pub render_scale: u8,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct FrameSummary {
    pub samples: u64,
    pub mean_ms: f64,
    pub fps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub over_budget_frames: u64,
    pub over_budget_ratio: f64,
}
impl FrameSummary {
    pub fn from_samples(samples: impl Iterator<Item = FrameSample>) -> Self {
        let samples: Vec<_> = samples
            .filter(|s| s.ms.is_finite() && s.ms > 0.0 && s.target_fps > 0)
            .collect();
        if samples.is_empty() {
            return Self::default();
        }
        let mut times: Vec<_> = samples.iter().map(|s| s.ms).collect();
        times.sort_by(f64::total_cmp);
        let count = times.len();
        let mean = times.iter().sum::<f64>() / count as f64;
        let percentile = |p: f64| {
            times[((count as f64 * p).ceil() as usize)
                .saturating_sub(1)
                .min(count - 1)]
        };
        let over = samples
            .iter()
            .filter(|s| s.ms > 1000.0 / s.target_fps as f64 * 1.25)
            .count() as u64;
        Self {
            samples: count as u64,
            mean_ms: mean,
            fps: 1000.0 / mean,
            p50_ms: percentile(0.5),
            p95_ms: percentile(0.95),
            p99_ms: percentile(0.99),
            max_ms: times[count - 1],
            over_budget_frames: over,
            over_budget_ratio: over as f64 / count as f64,
        }
    }
}

#[derive(Serialize)]
pub struct GpuInfo {
    name: String,
    backend: String,
    driver: String,
    driver_info: String,
    vendor_id: u32,
    device_id: u32,
}
#[derive(Serialize)]
pub struct PerformanceReport {
    pub schema_version: u32,
    pub measurement: &'static str,
    pub source: &'static str,
    pub build_revision: &'static str,
    pub app_version: &'static str,
    pub created_unix_seconds: u64,
    pub duration_seconds: f64,
    pub warmup_seconds: f64,
    pub summary: FrameSummary,
    pub frames: Vec<FrameSample>,
    pub gpu: Option<GpuInfo>,
    pub device_start: DeviceSnapshot,
    pub device_end: DeviceSnapshot,
    pub render_dimensions_start: RenderDimensions,
    pub render_dimensions_end: RenderDimensions,
    pub effective_start: EffectivePerformance,
    pub effective_end: EffectivePerformance,
    pub preferences_start: PerformancePreferences,
    pub interruptions: u32,
    pub events: Vec<String>,
    pub notes: Vec<&'static str>,
}

pub(super) struct Capture {
    pub warmup_seconds: f64,
    pub duration_seconds: f64,
    pub frames: Vec<FrameSample>,
    device: DeviceSnapshot,
    dimensions: RenderDimensions,
    effective: EffectivePerformance,
    preferences: PerformancePreferences,
    gpu: Option<GpuInfo>,
    harness: bool,
    interrupted: bool,
    interruptions: u32,
    events: Vec<String>,
    last_effective: EffectivePerformance,
}
impl Capture {
    pub fn new(
        device: &DeviceSnapshot,
        dimensions: RenderDimensions,
        effective: &EffectivePerformance,
        preferences: PerformancePreferences,
        adapter: Option<&RenderAdapterInfo>,
        harness: bool,
    ) -> Self {
        Self {
            warmup_seconds: 0.0,
            duration_seconds: 0.0,
            frames: Vec::with_capacity(7200),
            device: device.clone(),
            dimensions,
            effective: effective.clone(),
            preferences,
            gpu: adapter.map(|gpu| GpuInfo {
                name: gpu.name.clone(),
                backend: format!("{:?}", gpu.backend),
                driver: gpu.driver.clone(),
                driver_info: gpu.driver_info.clone(),
                vendor_id: gpu.vendor,
                device_id: gpu.device,
            }),
            harness,
            interrupted: true,
            interruptions: 0,
            events: Vec::new(),
            last_effective: effective.clone(),
        }
    }
    pub fn interruption(&mut self) {
        if !self.interrupted {
            self.interruptions = self.interruptions.saturating_add(1);
        }
        self.interrupted = true;
    }
    pub fn observe(&mut self, sample: FrameSample, effective: &EffectivePerformance) {
        self.interrupted = false;
        if self.warmup_seconds < WARMUP_SECONDS {
            self.warmup_seconds += sample.ms / 1000.0;
            return;
        }
        if effective != &self.last_effective {
            if self.events.len() < 64 {
                self.events.push(format!(
                    "{:.3}s: {} fps, {}%, {}",
                    self.duration_seconds,
                    effective.target_fps,
                    effective.render_scale,
                    effective.reason
                ));
            }
            self.last_effective = effective.clone();
        }
        self.duration_seconds += sample.ms / 1000.0;
        self.frames.push(sample);
    }
    pub fn finish(
        self,
        device: &DeviceSnapshot,
        dimensions: RenderDimensions,
        effective: &EffectivePerformance,
    ) -> PerformanceReport {
        PerformanceReport {
            schema_version: 1,
            measurement: "app_frame_interval_not_gpu_or_present_time",
            source: if self.harness {
                "fixed_timestep_qa"
            } else {
                "live_session"
            },
            build_revision: option_env!("DTP_BUILD_REVISION").unwrap_or("unknown"),
            app_version: env!("CARGO_PKG_VERSION"),
            created_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |t| t.as_secs()),
            duration_seconds: self.duration_seconds,
            warmup_seconds: self.warmup_seconds,
            summary: FrameSummary::from_samples(self.frames.iter().copied()),
            frames: self.frames,
            gpu: self.gpu,
            device_start: self.device,
            device_end: device.clone(),
            render_dimensions_start: self.dimensions,
            render_dimensions_end: dimensions,
            effective_start: self.effective,
            effective_end: effective.clone(),
            preferences_start: self.preferences,
            interruptions: self.interruptions,
            events: self.events,
            notes: vec![
                "Monotonic app-loop intervals include frame-cap sleep and rendering waits; these are not GPU timestamps or display presentation measurements.",
                "Only active gameplay is sampled. Startup, five-second warmup, menus, loss of focus and background time are excluded.",
                "P95/P99 use nearest-rank percentiles. Over-budget means >125% of the cap budget recorded for that frame.",
                "System RAM is not this app's memory budget. PSS is KiB. Thermal headroom is dimensionless, not Celsius.",
                "A live session is observational; its route, combat and weather are not standardized. Emulator/software-renderer results do not predict phone performance.",
                "No telemetry is uploaded. Android export opens a user-controlled share sheet.",
            ],
        }
    }
}

pub(super) fn report_path() -> Option<PathBuf> {
    crate::settings::settings_path().map(|path| path.with_file_name(PERF_REPORT_FILENAME))
}
pub(super) fn valid_existing_report(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if metadata.len() > 4 * 1024 * 1024 {
        return false;
    }
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    serde_json::from_slice::<serde_json::Value>(&bytes).is_ok_and(|value| {
        value["schema_version"] == 1
            && value["measurement"] == "app_frame_interval_not_gpu_or_present_time"
            && value["frames"].is_array()
    })
}
pub(super) fn write_report(report: &PerformanceReport) -> Result<PathBuf, String> {
    let path = report_path().ok_or("Report directory unavailable")?;
    write_report_at(&path, report)?;
    Ok(path)
}
fn write_report_at(path: &Path, report: &PerformanceReport) -> Result<(), String> {
    let parent = path.parent().ok_or("Report parent unavailable")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temp = parent.join(format!(".perf-report-{}-{stamp}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        let bytes = serde_json::to_vec_pretty(report).map_err(std::io::Error::other)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(std::io::Error::other("report exceeds bounded file size"));
        }
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if !valid_existing_report(&temp) {
            return Err(std::io::Error::other("report read-back failed"));
        }
        fs::rename(&temp, path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quantiles_and_missed_budgets_use_real_intervals_and_per_frame_cap() {
        let samples = (1..=100).map(|i| FrameSample {
            ms: i as f64,
            target_fps: 60,
            render_scale: 85,
        });
        let s = FrameSummary::from_samples(samples);
        assert_eq!(
            (s.p50_ms, s.p95_ms, s.p99_ms, s.max_ms),
            (50.0, 95.0, 99.0, 100.0)
        );
        assert_eq!(s.mean_ms, 50.5);
        assert_eq!(s.over_budget_frames, 80);
        let slow_cap = FrameSummary::from_samples(
            [FrameSample {
                ms: 34.0,
                target_fps: 30,
                render_scale: 70,
            }]
            .into_iter(),
        );
        assert_eq!(slow_cap.over_budget_frames, 0);
    }
    #[test]
    fn warmup_is_excluded_and_interruptions_do_not_invent_elapsed_time() {
        let effective = EffectivePerformance::default();
        let mut c = Capture::new(
            &DeviceSnapshot::default(),
            RenderDimensions::default(),
            &effective,
            PerformancePreferences::default(),
            None,
            true,
        );
        for _ in 0..5 {
            c.observe(
                FrameSample {
                    ms: 1000.0,
                    target_fps: 60,
                    render_scale: 100,
                },
                &effective,
            );
        }
        assert_eq!(c.duration_seconds, 0.0);
        assert!(c.frames.is_empty());
        c.observe(
            FrameSample {
                ms: 100.0,
                target_fps: 60,
                render_scale: 100,
            },
            &effective,
        );
        c.interruption();
        c.interruption();
        assert_eq!(c.duration_seconds, 0.1);
        assert_eq!(c.interruptions, 1);
        let report = c.finish(
            &DeviceSnapshot::default(),
            RenderDimensions::default(),
            &effective,
        );
        assert_eq!(report.source, "fixed_timestep_qa");
        assert_eq!(report.summary.samples, 1);
    }
    #[test]
    fn report_is_atomic_valid_json_and_does_not_touch_game_or_preferences() {
        let dir = std::env::temp_dir().join(format!("dtperf-report-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("save.json"), b"unchanged game").unwrap();
        fs::write(dir.join("settings.json"), b"unchanged settings").unwrap();
        let effective = EffectivePerformance::default();
        let c = Capture::new(
            &DeviceSnapshot::default(),
            RenderDimensions::default(),
            &effective,
            PerformancePreferences::default(),
            None,
            true,
        );
        let report = c.finish(
            &DeviceSnapshot::default(),
            RenderDimensions::default(),
            &effective,
        );
        let path = dir.join(PERF_REPORT_FILENAME);
        write_report_at(&path, &report).unwrap();
        assert!(valid_existing_report(&path));
        assert_eq!(fs::read(dir.join("save.json")).unwrap(), b"unchanged game");
        assert_eq!(
            fs::read(dir.join("settings.json")).unwrap(),
            b"unchanged settings"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
