//! Local performance controls and real-time evidence. Frame intervals are wall
//! clock observations of the app loop, not GPU timing or presented-frame latency.
mod policy;
pub(crate) mod render;
mod report;
pub(crate) mod upscale;
pub use policy::EffectivePerformance;
pub use report::{FrameSample, FrameSummary};

use crate::{
    device::{self, DeviceSnapshot},
    game_flow::GameFlow,
    i18n::{Language, tr},
    ui::{PauseOpen, UiModal},
};
use bevy::{
    prelude::*,
    render::renderer::RenderAdapterInfo,
    tasks::{IoTaskPool, Task, futures::check_ready},
    window::{AppLifecycle, WindowFocused},
};
use policy::AdaptiveController;
use render::RenderDimensions;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

pub const CAPTURE_SECONDS: f64 = 60.0;
pub const WARMUP_SECONDS: f64 = 5.0;
const MAX_CAPTURE_FRAMES: usize = 16_384;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceProfile {
    #[default]
    Auto,
    Battery,
    Balanced,
    Quality,
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformancePreferences {
    pub profile: PerformanceProfile,
    pub frame_cap: u16,
    /// Zero lets the profile/controller choose; other values are percentages.
    pub render_scale: u8,
    pub overlay: bool,
}
impl Default for PerformancePreferences {
    fn default() -> Self {
        Self {
            profile: PerformanceProfile::Auto,
            frame_cap: 60,
            render_scale: 0,
            overlay: false,
        }
    }
}
impl PerformancePreferences {
    pub fn sanitize(mut self) -> Self {
        if !matches!(self.frame_cap, 30 | 60 | 90 | 120) {
            self.frame_cap = 60;
        }
        if !matches!(self.render_scale, 0 | 60 | 70 | 85 | 100) {
            self.render_scale = 0;
        }
        self
    }
}

#[derive(Resource, Clone, Default, Debug)]
pub struct PerformanceStatus {
    pub summary: String,
    pub revision: u64,
    pub capture_active: bool,
    pub capture_seconds: f32,
    pub report_available: bool,
    pub report_status: String,
}
#[derive(Message, Clone, Copy, Debug)]
pub enum PerformanceAction {
    StartCapture,
    CancelCapture,
    ExportReport,
}
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PerformanceSet;

#[derive(Resource, Clone, Default)]
pub struct PerformanceMetrics {
    pub summary: FrameSummary,
    pub entities: usize,
    pub paused: bool,
}

#[derive(Resource)]
struct FrameClock {
    last: Option<Instant>,
    interval_ms: Option<f64>,
    /// Work and waits since the previous frame boundary, before our own sleep.
    /// This is an app-loop observation, not a CPU/GPU execution-time profiler.
    unpaced_ms: Option<f64>,
    harness: bool,
}
impl Default for FrameClock {
    fn default() -> Self {
        Self {
            last: None,
            interval_ms: None,
            unpaced_ms: None,
            harness: std::env::args().any(|arg| arg == "--qa"),
        }
    }
}
#[derive(Resource)]
struct Foreground {
    active: bool,
    interrupted: bool,
}
impl Default for Foreground {
    fn default() -> Self {
        Self {
            active: true,
            interrupted: false,
        }
    }
}

#[derive(Resource, Default)]
struct Telemetry {
    recent: VecDeque<FrameSample>,
    window: Vec<FrameSample>,
    unpaced_window: Vec<FrameSample>,
    elapsed: f64,
    eligible_last: bool,
    policy: AdaptiveController,
    thermal: policy::ThermalRecovery,
    preferences: Option<PerformancePreferences>,
    capture: Option<report::Capture>,
    report_task: Option<Task<Result<PathBuf, String>>>,
    share_after_write: bool,
    tick: f64,
    #[cfg(target_os = "android")]
    last_share_status: Option<device::ReportShareStatus>,
}

pub struct PerformancePlugin;
impl Plugin for PerformancePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PerformancePreferences>()
            .init_resource::<PerformanceStatus>()
            .init_resource::<PerformanceMetrics>()
            .init_resource::<EffectivePerformance>()
            .init_resource::<FrameClock>()
            .init_resource::<Foreground>()
            .init_resource::<Telemetry>()
            .add_message::<PerformanceAction>()
            .add_message::<WindowFocused>()
            .add_message::<AppLifecycle>()
            .add_systems(Startup, locate_report)
            .add_systems(First, pace_frame.before(bevy::time::TimeSystems))
            .add_systems(PreUpdate, lifecycle)
            .add_systems(
                Update,
                (actions, observe, resolve_policy, publish_status)
                    .chain()
                    .in_set(PerformanceSet)
                    .after(crate::ui::settings::SettingsSet),
            );
        upscale::install(app);
        render::install(app);
    }
}

/// Sleep at the next frame boundary, not after a frame has already spent its
/// budget. This is an application-side cap; VSync/Android still control present.
/// It does not claim AGDK Swappy or hardware frame-pacing integration.
fn pace_frame(
    mut clock: ResMut<FrameClock>,
    effective: Res<EffectivePerformance>,
    foreground: Res<Foreground>,
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
) {
    clock.unpaced_ms = clock.last.map(|last| last.elapsed().as_secs_f64() * 1000.0);
    if !clock.harness
        && foreground.active
        && let Some(last) = clock.last
    {
        let cap = if flow.get() == &GameFlow::Playing && !pause.0 {
            effective.target_fps
        } else {
            effective.target_fps.min(30)
        };
        let period = Duration::from_secs_f64(1.0 / cap.max(15) as f64);
        if let Some(wait) = period.checked_sub(last.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    let now = Instant::now();
    clock.interval_ms = clock
        .last
        .map(|last| now.duration_since(last).as_secs_f64() * 1000.0);
    clock.last = Some(now);
}

fn lifecycle(
    mut focus: MessageReader<WindowFocused>,
    mut lifecycle: MessageReader<AppLifecycle>,
    mut foreground: ResMut<Foreground>,
) {
    foreground.interrupted = false;
    for event in focus.read() {
        if foreground.active != event.focused {
            foreground.interrupted = true;
        }
        foreground.active = event.focused;
    }
    for event in lifecycle.read() {
        if matches!(event, AppLifecycle::WillSuspend | AppLifecycle::Suspended) {
            foreground.active = false;
            foreground.interrupted = true;
        }
        if matches!(event, AppLifecycle::Running | AppLifecycle::WillResume) {
            foreground.active = true;
            foreground.interrupted = true;
        }
    }
}

fn locate_report(mut status: ResMut<PerformanceStatus>) {
    status.report_available =
        report::report_path().is_some_and(|path| report::valid_existing_report(&path));
}

fn actions(
    mut messages: MessageReader<PerformanceAction>,
    flow: Res<State<GameFlow>>,
    language: Res<Language>,
    device: Res<DeviceSnapshot>,
    dimensions: Res<RenderDimensions>,
    effective: Res<EffectivePerformance>,
    preferences: Res<PerformancePreferences>,
    clock: Res<FrameClock>,
    adapter: Option<Res<RenderAdapterInfo>>,
    mut telemetry: ResMut<Telemetry>,
    mut status: ResMut<PerformanceStatus>,
) {
    if let Some(task) = telemetry.report_task.as_mut()
        && let Some(result) = check_ready(task)
    {
        telemetry.report_task = None;
        match result {
            Ok(path) => {
                status.report_available = true;
                status.report_status = tr(
                    *language,
                    "效能報告已儲存，可分享 JSON",
                    "Performance report saved; JSON export is ready",
                )
                .into();
                println!("DTPERFORMANCE report_saved={}", path.display());
                if telemetry.share_after_write {
                    share_report(&mut status, *language);
                }
            }
            Err(error) => {
                status.report_status = format!("Report save failed: {error}");
                println!("DTPERFORMANCE report_error={error}");
            }
        }
        telemetry.share_after_write = false;
    }
    for action in messages.read() {
        match action {
            PerformanceAction::StartCapture => {
                if flow.get() != &GameFlow::Playing {
                    status.report_status = tr(
                        *language,
                        "請先進入遊戲，再開始效能記錄",
                        "Enter a game before starting a capture",
                    )
                    .into();
                    continue;
                }
                if telemetry.report_task.is_some() {
                    status.report_status = tr(
                        *language,
                        "正在儲存上一份報告",
                        "The previous report is still being saved",
                    )
                    .into();
                    continue;
                }
                telemetry.capture = Some(report::Capture::new(
                    &device,
                    *dimensions,
                    &effective,
                    *preferences,
                    adapter.as_deref(),
                    clock.harness,
                ));
                status.capture_active = true;
                status.capture_seconds = 0.0;
                status.report_status = tr(
                    *language,
                    "關閉設定並繼續遊戲；暖機 5 秒後記錄 60 秒",
                    "Close settings and resume; 5 s warmup, then 60 s recording",
                )
                .into();
                println!("DTPERFORMANCE capture=waiting");
            }
            PerformanceAction::CancelCapture => {
                telemetry.capture = None;
                status.capture_active = false;
                status.capture_seconds = 0.0;
                status.report_status = tr(
                    *language,
                    "已取消記錄，保留上一份完整報告",
                    "Capture cancelled; the previous complete report is retained",
                )
                .into();
                println!("DTPERFORMANCE capture=cancelled");
            }
            PerformanceAction::ExportReport => {
                if telemetry.report_task.is_some() {
                    telemetry.share_after_write = true;
                } else if status.report_available {
                    share_report(&mut status, *language);
                } else {
                    status.report_status = tr(
                        *language,
                        "先完成一次 60 秒效能記錄",
                        "Complete a 60-second capture first",
                    )
                    .into();
                }
            }
        }
    }
}

fn share_report(status: &mut PerformanceStatus, language: Language) {
    #[cfg(target_os = "android")]
    {
        if device::request_report_share() {
            status.report_status = tr(
                language,
                "正在開啟 Android 分享選單",
                "Opening the Android share sheet",
            )
            .into();
        } else {
            status.report_status = tr(
                language,
                "分享功能暫不可用，報告仍已儲存",
                "Sharing is unavailable; the report remains saved",
            )
            .into();
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        status.report_status = match report::report_path() {
            Some(path) => format!("{} {}", tr(language, "報告：", "Report:"), path.display()),
            None => "Report directory unavailable".into(),
        };
    }
}

fn observe(
    clock: Res<FrameClock>,
    foreground: Res<Foreground>,
    device: Res<DeviceSnapshot>,
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    modal: Res<UiModal>,
    settings: Res<crate::ui::settings::SettingsUi>,
    effective: Res<EffectivePerformance>,
    preferences: Res<PerformancePreferences>,
    dimensions: Res<RenderDimensions>,
    language: Res<Language>,
    entities: Query<Entity>,
    mut telemetry: ResMut<Telemetry>,
    mut metrics: ResMut<PerformanceMetrics>,
    mut status: ResMut<PerformanceStatus>,
) {
    let eligible = flow.get() == &GameFlow::Playing
        && !pause.0
        && !modal.blocks_input()
        && !settings.open
        && foreground.active
        && !foreground.interrupted
        && device.foreground != Some(false);
    metrics.paused = !eligible;
    if !eligible || !telemetry.eligible_last {
        telemetry.eligible_last = eligible;
        telemetry.window.clear();
        telemetry.unpaced_window.clear();
        telemetry.elapsed = 0.0;
        if let Some(capture) = telemetry.capture.as_mut() {
            capture.interruption();
        }
        if flow.get() != &GameFlow::Playing && telemetry.capture.take().is_some() {
            status.capture_active = false;
            status.report_status = tr(
                *language,
                "已離開遊戲，記錄已取消",
                "Capture cancelled after leaving the game",
            )
            .into();
        }
        return;
    }
    let Some(ms) = clock.interval_ms.filter(|ms| ms.is_finite() && *ms > 0.0) else {
        return;
    };
    let sample = FrameSample {
        ms,
        target_fps: effective.target_fps,
        render_scale: effective.render_scale,
    };
    telemetry.recent.push_back(sample);
    while telemetry.recent.len() > 600 {
        telemetry.recent.pop_front();
    }
    telemetry.window.push(sample);
    if let Some(ms) = clock.unpaced_ms.filter(|ms| ms.is_finite() && *ms > 0.0) {
        telemetry.unpaced_window.push(FrameSample { ms, ..sample });
    }
    telemetry.elapsed += ms / 1000.0;
    if telemetry.elapsed >= 2.0 {
        let summary = FrameSummary::from_samples(telemetry.recent.iter().copied());
        let window = FrameSummary::from_samples(telemetry.window.iter().copied());
        let unpaced = FrameSummary::from_samples(telemetry.unpaced_window.iter().copied());
        metrics.summary = summary;
        metrics.entities = entities.iter().count();
        let elapsed = telemetry.elapsed as f32;
        if !clock.harness {
            let recovery = policy::resolve(
                *preferences,
                &device,
                telemetry.policy.step.saturating_sub(1),
                cfg!(target_os = "android"),
            );
            telemetry.policy.observe(
                window.p95_ms,
                1000.0 / policy_budget_fps(effective.target_fps, &device),
                if unpaced.samples > 0 {
                    unpaced.p95_ms
                } else {
                    f64::INFINITY
                },
                1000.0 / policy_budget_fps(recovery.target_fps, &device),
                elapsed,
            );
        }
        telemetry.window.clear();
        telemetry.unpaced_window.clear();
        telemetry.elapsed = 0.0;
    }
    if let Some(capture) = telemetry.capture.as_mut() {
        let started = capture.warmup_seconds >= WARMUP_SECONDS;
        if started && capture.frames.is_empty() {
            capture.begin_sampling(&device, *dimensions, &effective, *preferences);
        }
        capture.observe(sample, &effective);
        if !started && capture.warmup_seconds >= WARMUP_SECONDS {
            println!("DTPERFORMANCE capture=recording");
        }
        status.capture_seconds = capture.duration_seconds as f32;
        if capture.frames.len() >= MAX_CAPTURE_FRAMES {
            telemetry.capture = None;
            status.capture_active = false;
            status.report_status =
                "Capture exceeded its bounded sample limit; previous report retained".into();
        } else if capture.duration_seconds >= CAPTURE_SECONDS {
            let capture = telemetry.capture.take().expect("capture just observed");
            let report = capture.finish(&device, *dimensions, &effective);
            println!(
                "DTPERFORMANCE capture=complete samples={} seconds={:.3}",
                report.summary.samples, report.duration_seconds
            );
            telemetry.report_task =
                Some(IoTaskPool::get().spawn(async move { report::write_report(&report) }));
            status.capture_active = false;
            status.report_status = tr(
                *language,
                "記錄完成，正在儲存報告",
                "Capture complete; saving the report",
            )
            .into();
        }
    }
}

fn resolve_policy(
    preferences: Res<PerformancePreferences>,
    device: Res<DeviceSnapshot>,
    clock: Res<FrameClock>,
    foreground: Res<Foreground>,
    mut telemetry: ResMut<Telemetry>,
    mut effective: ResMut<EffectivePerformance>,
) {
    if telemetry.preferences.is_none_or(|previous| {
        previous.profile != preferences.profile
            || previous.frame_cap != preferences.frame_cap
            || previous.render_scale != preferences.render_scale
    }) {
        telemetry.policy = AdaptiveController::default();
        telemetry.window.clear();
        telemetry.unpaced_window.clear();
        telemetry.elapsed = 0.0;
    }
    telemetry.preferences = Some(*preferences);
    let mut next = policy::resolve(
        *preferences,
        &device,
        if clock.harness {
            0
        } else {
            telemetry.policy.step
        },
        cfg!(target_os = "android"),
    );
    if !clock.harness {
        telemetry.thermal.apply(
            &mut next,
            &device,
            if foreground.active && !foreground.interrupted {
                (clock.interval_ms.unwrap_or(0.0) / 1000.0) as f32
            } else {
                0.0
            },
        );
    }
    if *effective != next {
        println!(
            "DTPERFORMANCE effective={}",
            serde_json::to_string(&next).unwrap_or_default()
        );
        *effective = next;
    }
    device::request_frame_rate_hint(if device.foreground == Some(false) {
        0
    } else {
        effective.target_fps as u32
    });
}

/// A refused refresh hint is not evidence of a rendering bottleneck. The
/// controller evaluates pressure against the currently observed display rate.
pub(crate) fn policy_budget_fps(cap: u16, device: &DeviceSnapshot) -> f64 {
    let refresh = if device.is_fresh() {
        device.current_refresh_hz
    } else {
        None
    };
    refresh
        .filter(|hz| hz.is_finite() && *hz >= 15.0)
        .map_or(cap as f64, |hz| (cap as f64).min(hz as f64))
}

fn publish_status(
    clock: Res<FrameClock>,
    metrics: Res<PerformanceMetrics>,
    device: Res<DeviceSnapshot>,
    effective: Res<EffectivePerformance>,
    dimensions: Res<RenderDimensions>,
    language: Res<Language>,
    mut telemetry: ResMut<Telemetry>,
    mut status: ResMut<PerformanceStatus>,
) {
    telemetry.tick += clock.interval_ms.unwrap_or(0.0).min(1000.0) / 1000.0;
    if telemetry.tick < 0.5 && !effective.is_changed() && !dimensions.is_changed() {
        return;
    }
    telemetry.tick = 0.0;
    let summary = &metrics.summary;
    let model = if device.model.is_empty() {
        "Unknown / desktop"
    } else {
        &device.model
    };
    let pss = device
        .process_pss_kb
        .map(|kb| format!("{:.0} MiB", kb as f64 / 1024.0))
        .unwrap_or_else(|| "unknown".into());
    let fps = if summary.samples > 0 {
        format!("{:.1}", summary.fps)
    } else {
        "--".into()
    };
    let state = if metrics.paused {
        tr(*language, "暫停量測", "sampling paused")
    } else {
        tr(*language, "實際遊戲記錄", "live gameplay")
    };
    status.summary = format!(
        "{model}\nFPS {fps} / {} · P95 {:.1} / P99 {:.1} ms\n3D {}×{} · UI {}×{}\nPSS {pss} · Thermal {}{}\n{} · {state}",
        effective.target_fps,
        summary.p95_ms,
        summary.p99_ms,
        dimensions.scene[0],
        dimensions.scene[1],
        dimensions.ui[0],
        dimensions.ui[1],
        device.thermal_label(),
        if device.reported && !device.is_fresh() {
            " (stale)"
        } else {
            ""
        },
        effective.reason
    );
    status.revision = status.revision.wrapping_add(1);
    if status.capture_active {
        let warmup = telemetry.capture.as_ref().map_or(0.0, |c| c.warmup_seconds);
        status.report_status = if warmup < WARMUP_SECONDS {
            format!(
                "{} {:.1}/5 s · {}",
                tr(*language, "暖機", "Warmup"),
                warmup,
                tr(*language, "關閉設定並繼續遊戲", "close settings and resume")
            )
        } else {
            format!(
                "{} {:.1}/60 s · {}",
                tr(*language, "記錄", "Capture"),
                status.capture_seconds,
                tr(*language, "選單及背景不計時", "menus/background excluded")
            )
        };
    }
    #[cfg(target_os = "android")]
    {
        let share = device::report_share_status();
        if telemetry.last_share_status != Some(share) {
            telemetry.last_share_status = Some(share);
            match share {
                device::ReportShareStatus::ChooserOpened => {
                    status.report_status = tr(
                        *language,
                        "分享選單已開啟，請自行選擇儲存或傳送",
                        "Share sheet opened; choose where to save or send",
                    )
                    .into()
                }
                device::ReportShareStatus::FileMissing => {
                    status.report_status = tr(
                        *language,
                        "找不到報告，請重新完成記錄",
                        "Report file missing; complete another capture",
                    )
                    .into()
                }
                device::ReportShareStatus::Unavailable | device::ReportShareStatus::Failed => {
                    status.report_status = tr(
                        *language,
                        "無法開啟分享選單，報告仍已保留",
                        "Share sheet unavailable; report retained",
                    )
                    .into()
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod integration_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_numeric_preferences_fall_back_to_safe_defaults() {
        let p = PerformancePreferences {
            frame_cap: 999,
            render_scale: 1,
            ..default()
        }
        .sanitize();
        assert_eq!(p.frame_cap, 60);
        assert_eq!(p.render_scale, 0);
        assert_eq!(
            serde_json::from_str::<PerformancePreferences>("{}").unwrap(),
            PerformancePreferences::default()
        );
    }
}
