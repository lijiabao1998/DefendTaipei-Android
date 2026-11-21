//! Performance overlay + `DTPERF` log lines for automated smoke tests.
//!
//! # Overlay toggle
//! - **Default:** visible in debug builds, hidden in release builds.
//! - **Desktop:** press `F3`, or start with `DEFEND_TAIPEI_PERF=1` to force the
//!   overlay on (any build). `DEFEND_TAIPEI_PERF=0` forces it off.
//! - **Android:** three-finger tap anywhere toggles the overlay.
//!
//! # Logging
//! Every ~5 s a one-line summary is printed to stdout / logcat, tagged with the
//! literal prefix `DTPERF` so CI can grep it regardless of the Android log tag:
//! ```text
//! DTPERF fps=58.2 avg_ms=17.2 p99_ms=24.1 max_ms=31.0 entities=412 overlay=0
//! ```
//! Logging always runs (even when the overlay is hidden).

use bevy::{
    diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin},
    prelude::*,
};

const LOG_INTERVAL_SECS: f32 = 5.0;
/// Keep ~3 s of frame times at 60 fps for p99 / max.
const FRAME_HISTORY: usize = 180;

#[derive(Resource)]
struct PerfOverlayEnabled(bool);

#[derive(Resource)]
struct PerfLogTimer(f32);

#[derive(Resource, Default)]
struct FrameHistory {
    /// Recent frame times in milliseconds (newest at the back).
    ms: Vec<f32>,
}

#[derive(Component)]
struct PerfRoot;

#[derive(Component)]
struct PerfText;

pub struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        // Emit one line immediately so short-lived CI runs still see DTPERF even
        // if the process dies before the first periodic dump.
        println!("DTPERF boot overlay_default={}", overlay_default_on() as u8);

        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin::default(),
        ))
        .insert_resource(PerfOverlayEnabled(overlay_default_on()))
        .insert_resource(PerfLogTimer(0.0))
        .init_resource::<FrameHistory>()
        .add_systems(Startup, spawn_overlay)
        .add_systems(
            Update,
            (
                record_frame_time,
                toggle_overlay,
                update_overlay,
                log_perf_summary,
            )
                .chain(),
        );
    }
}

fn overlay_default_on() -> bool {
    if let Ok(v) = std::env::var("DEFEND_TAIPEI_PERF") {
        let t = v.trim();
        if t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("on") {
            return true;
        }
        if t == "0" || t.eq_ignore_ascii_case("false") || t.eq_ignore_ascii_case("off") {
            return false;
        }
    }
    cfg!(debug_assertions)
}

fn spawn_overlay(mut commands: Commands, enabled: Res<PerfOverlayEnabled>) {
    let font = TextFont {
        font_size: FontSize::Px(14.0),
        ..default()
    };
    // Top-right corner: clear of the left-half joystick and the top-left HUD.
    commands
        .spawn((
            PerfRoot,
            Node {
                position_type: PositionType::Absolute,
                right: px(10),
                top: px(10),
                padding: UiRect::axes(px(8), px(6)),
                border_radius: BorderRadius::all(px(6)),
                max_width: percent(46),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            Visibility::from_enabled(enabled.0),
            GlobalZIndex(30),
        ))
        .with_child((
            PerfText,
            Text::new("PERF …"),
            font,
            TextColor(Color::srgb(0.85, 1.0, 0.75)),
        ));
}

trait VisibilityExt {
    fn from_enabled(on: bool) -> Self;
}
impl VisibilityExt for Visibility {
    fn from_enabled(on: bool) -> Self {
        if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    }
}

fn record_frame_time(time: Res<Time>, mut hist: ResMut<FrameHistory>) {
    let ms = time.delta_secs() * 1000.0;
    if !ms.is_finite() || ms <= 0.0 {
        return;
    }
    hist.ms.push(ms);
    if hist.ms.len() > FRAME_HISTORY {
        let drop = hist.ms.len() - FRAME_HISTORY;
        hist.ms.drain(0..drop);
    }
}

fn toggle_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    touches: Res<Touches>,
    mut enabled: ResMut<PerfOverlayEnabled>,
    mut root: Query<&mut Visibility, With<PerfRoot>>,
) {
    let key = keys.just_pressed(KeyCode::F3);
    // Three-finger tap: any frame where a touch just started and ≥3 are down.
    let three_finger = touches.any_just_pressed() && touches.iter().count() >= 3;
    if !(key || three_finger) {
        return;
    }
    enabled.0 = !enabled.0;
    for mut vis in &mut root {
        *vis = Visibility::from_enabled(enabled.0);
    }
    println!(
        "DTPERF overlay_toggled={}",
        if enabled.0 { "on" } else { "off" }
    );
}

fn update_overlay(
    enabled: Res<PerfOverlayEnabled>,
    diagnostics: Res<DiagnosticsStore>,
    hist: Res<FrameHistory>,
    mut text: Query<&mut Text, With<PerfText>>,
    mut root: Query<&mut Visibility, With<PerfRoot>>,
) {
    for mut vis in &mut root {
        let want = Visibility::from_enabled(enabled.0);
        if *vis != want {
            *vis = want;
        }
    }
    if !enabled.0 {
        return;
    }
    let snap = snapshot(&diagnostics, &hist, enabled.0);
    if let Ok(mut t) = text.single_mut() {
        t.0 = format!(
            "FPS {:>5.1}\navg {:>5.1} ms  p99 {:>5.1}  max {:>5.1}\nentities {}",
            snap.fps, snap.avg_ms, snap.p99_ms, snap.max_ms, snap.entities
        );
    }
}

fn log_perf_summary(
    time: Res<Time>,
    mut timer: ResMut<PerfLogTimer>,
    diagnostics: Res<DiagnosticsStore>,
    hist: Res<FrameHistory>,
    enabled: Res<PerfOverlayEnabled>,
) {
    timer.0 += time.delta_secs();
    if timer.0 < LOG_INTERVAL_SECS {
        return;
    }
    timer.0 = 0.0;
    let snap = snapshot(&diagnostics, &hist, enabled.0);
    // One line, grep-friendly. Goes to stdout (desktop) / logcat (Android).
    println!(
        "DTPERF fps={:.1} avg_ms={:.2} p99_ms={:.2} max_ms={:.2} entities={} overlay={}",
        snap.fps, snap.avg_ms, snap.p99_ms, snap.max_ms, snap.entities, enabled.0 as u8
    );
}

struct PerfSnapshot {
    fps: f64,
    avg_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    entities: u64,
}

fn snapshot(diagnostics: &DiagnosticsStore, hist: &FrameHistory, _overlay: bool) -> PerfSnapshot {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed().or(d.average()))
        .unwrap_or(0.0);
    let diag_avg = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.average())
        .unwrap_or(0.0);
    let entities = diagnostics
        .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        .and_then(|d| d.value())
        .unwrap_or(0.0) as u64;

    let (avg_ms, p99_ms, max_ms) = if hist.ms.is_empty() {
        (diag_avg, diag_avg, diag_avg)
    } else {
        let mut sorted = hist.ms.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let max = *sorted.last().unwrap() as f64;
        // 1% worst ≈ 99th percentile of frame time.
        let idx = ((sorted.len() as f64) * 0.99).floor() as usize;
        let p99 = sorted[idx.min(sorted.len() - 1)] as f64;
        let avg = sorted.iter().sum::<f32>() as f64 / sorted.len() as f64;
        (avg, p99, max)
    };

    PerfSnapshot {
        fps,
        avg_ms,
        p99_ms,
        max_ms,
        entities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p99_from_history() {
        let mut hist = FrameHistory::default();
        for i in 0..100 {
            hist.ms.push(10.0 + i as f32 * 0.1);
        }
        let store = DiagnosticsStore::default();
        let snap = snapshot(&store, &hist, false);
        assert!(snap.avg_ms > 10.0);
        assert!(snap.p99_ms >= snap.avg_ms);
        assert!(snap.max_ms >= snap.p99_ms);
    }
}
