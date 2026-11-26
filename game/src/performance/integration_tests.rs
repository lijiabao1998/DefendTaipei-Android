//! Exercise the real observer with explicit frame-boundary intervals. These are
//! behavioral tests, not performance measurements or sleep-based benchmarks.
use super::*;

fn observer_app() -> App {
    let device = DeviceSnapshot::default();
    let effective = EffectivePerformance::default();
    let preferences = PerformancePreferences::default();
    let dimensions = RenderDimensions {
        ui: [390, 844],
        scene: [390, 844],
        scale: 100,
    };
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(State::new(GameFlow::Playing))
        .insert_resource(FrameClock {
            interval_ms: Some(100.0),
            unpaced_ms: Some(20.0),
            harness: true,
            ..default()
        })
        .init_resource::<Foreground>()
        .insert_resource(Telemetry {
            capture: Some(report::Capture::new(
                &device,
                dimensions,
                &effective,
                preferences,
                None,
                true,
            )),
            ..default()
        })
        .insert_resource(device)
        .insert_resource(effective)
        .insert_resource(preferences)
        .insert_resource(dimensions)
        .init_resource::<PauseOpen>()
        .init_resource::<UiModal>()
        .init_resource::<crate::ui::settings::SettingsUi>()
        .init_resource::<Language>()
        .init_resource::<PerformanceMetrics>()
        .insert_resource(PerformanceStatus {
            capture_active: true,
            ..default()
        })
        .add_systems(Update, observe);
    app
}

fn interval(app: &mut App, ms: f64) {
    app.world_mut().resource_mut::<FrameClock>().interval_ms = Some(ms);
    app.update();
}

fn recording_app() -> App {
    let mut app = observer_app();
    // The first eligible boundary is deliberately discarded. It may still
    // contain time spent in a menu or background before this observation.
    interval(&mut app, 40_000.0);
    for _ in 0..5 {
        interval(&mut app, 1000.0);
    }
    let telemetry = app.world().resource::<Telemetry>();
    let capture = telemetry.capture.as_ref().unwrap();
    assert_eq!(capture.warmup_seconds, 5.0);
    assert_eq!(capture.duration_seconds, 0.0);
    assert!(capture.frames.is_empty());
    app
}

#[derive(Clone, Copy, Debug)]
enum Gate {
    Pause,
    Settings,
    Modal,
    ModalPointer,
    Focus,
    LifecycleBoundary,
    AndroidBackground,
}

impl Gate {
    fn set(self, app: &mut App, blocked: bool) {
        match self {
            Self::Pause => app.world_mut().resource_mut::<PauseOpen>().0 = blocked,
            Self::Settings => {
                app.world_mut()
                    .resource_mut::<crate::ui::settings::SettingsUi>()
                    .open = blocked;
            }
            Self::Modal => app.world_mut().resource_mut::<UiModal>().open = blocked,
            Self::ModalPointer => {
                app.world_mut().resource_mut::<UiModal>().pointer_captured = blocked;
            }
            Self::Focus => app.world_mut().resource_mut::<Foreground>().active = !blocked,
            Self::LifecycleBoundary => {
                app.world_mut().resource_mut::<Foreground>().interrupted = blocked;
            }
            Self::AndroidBackground => {
                app.world_mut().resource_mut::<DeviceSnapshot>().foreground = Some(!blocked);
            }
        }
    }
}

#[test]
fn every_menu_focus_and_background_gate_excludes_time_and_first_resume_boundary() {
    for gate in [
        Gate::Pause,
        Gate::Settings,
        Gate::Modal,
        Gate::ModalPointer,
        Gate::Focus,
        Gate::LifecycleBoundary,
        Gate::AndroidBackground,
    ] {
        let mut app = recording_app();
        interval(&mut app, 100.0);
        gate.set(&mut app, true);
        for _ in 0..4 {
            interval(&mut app, 30_000.0);
        }
        assert!(
            app.world().resource::<PerformanceMetrics>().paused,
            "{gate:?}"
        );
        assert!(
            app.world().resource::<PerformanceStatus>().capture_active,
            "{gate:?}"
        );
        {
            let telemetry = app.world().resource::<Telemetry>();
            let capture = telemetry.capture.as_ref().unwrap();
            assert_eq!(capture.duration_seconds, 0.1, "{gate:?}");
            assert_eq!(capture.frames.len(), 1, "{gate:?}");
            assert!(telemetry.window.is_empty(), "{gate:?}");
        }
        gate.set(&mut app, false);
        interval(&mut app, 45_000.0);
        {
            let telemetry = app.world().resource::<Telemetry>();
            let capture = telemetry.capture.as_ref().unwrap();
            assert_eq!(capture.duration_seconds, 0.1, "{gate:?}");
            assert_eq!(capture.frames.len(), 1, "{gate:?}");
        }
        interval(&mut app, 200.0);
        let telemetry = app.world().resource::<Telemetry>();
        let capture = telemetry.capture.as_ref().unwrap();
        assert!((capture.duration_seconds - 0.3).abs() < 1e-9, "{gate:?}");
        assert_eq!(capture.frames.len(), 2, "{gate:?}");
        assert_eq!(capture.frames[1].ms, 200.0, "{gate:?}");
        assert!(
            !app.world().resource::<PerformanceMetrics>().paused,
            "{gate:?}"
        );
        assert!(telemetry.report_task.is_none(), "{gate:?}");
    }
}

#[test]
fn warmup_accumulates_only_active_intervals_and_real_stalls_are_preserved() {
    let mut app = observer_app();
    interval(&mut app, 60_000.0);
    for _ in 0..3 {
        interval(&mut app, 1000.0);
    }
    Gate::Settings.set(&mut app, true);
    interval(&mut app, 25_000.0);
    interval(&mut app, 25_000.0);
    Gate::Settings.set(&mut app, false);
    interval(&mut app, 30_000.0);
    for _ in 0..2 {
        interval(&mut app, 1000.0);
    }
    {
        let telemetry = app.world().resource::<Telemetry>();
        let capture = telemetry.capture.as_ref().unwrap();
        assert_eq!(capture.warmup_seconds, 5.0);
        assert_eq!(capture.duration_seconds, 0.0);
        assert!(capture.frames.is_empty());
    }
    // A real foreground stall is evidence. It must not be clamped to Bevy's
    // virtual-time max delta or discarded as though it were a background gap.
    interval(&mut app, 2500.0);
    interval(&mut app, 20.0);
    let telemetry = app.world().resource::<Telemetry>();
    let capture = telemetry.capture.as_ref().unwrap();
    assert_eq!(capture.frames.len(), 2);
    assert!((capture.duration_seconds - 2.52).abs() < 1e-9);
    assert_eq!(capture.frames[0].ms, 2500.0);
    let summary = FrameSummary::from_samples(capture.frames.iter().copied());
    assert_eq!(summary.max_ms, 2500.0);
    assert_eq!(summary.samples, 2);
    assert_eq!(summary.over_budget_frames, 1);
}

#[test]
fn invalid_intervals_do_not_poison_capture_or_create_report_io() {
    let mut app = recording_app();
    for value in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        interval(&mut app, value);
    }
    app.world_mut().resource_mut::<FrameClock>().interval_ms = None;
    app.update();
    let telemetry = app.world().resource::<Telemetry>();
    let capture = telemetry.capture.as_ref().unwrap();
    assert_eq!(capture.duration_seconds, 0.0);
    assert!(capture.frames.is_empty());
    assert!(telemetry.report_task.is_none());
    assert!(app.world().resource::<PerformanceStatus>().capture_active);
}

#[test]
fn leaving_game_cancels_partial_capture_without_replacing_previous_report() {
    let mut app = recording_app();
    interval(&mut app, 100.0);
    app.world_mut()
        .resource_mut::<PerformanceStatus>()
        .report_available = true;
    app.insert_resource(State::new(GameFlow::Title));
    interval(&mut app, 60_000.0);
    let telemetry = app.world().resource::<Telemetry>();
    assert!(telemetry.capture.is_none());
    assert!(telemetry.report_task.is_none());
    let status = app.world().resource::<PerformanceStatus>();
    assert!(!status.capture_active);
    assert!(status.report_available);
    assert!(!status.report_status.is_empty());
}

#[test]
fn fresh_actual_refresh_limits_pressure_budget_without_changing_requested_cap() {
    let mut device = DeviceSnapshot {
        reported: true,
        supported_refresh_hz: vec![60.0, 120.0],
        current_refresh_hz: Some(60.0),
        ..default()
    };
    assert_eq!(policy_budget_fps(120, &device), 60.0);
    assert_eq!(policy_budget_fps(30, &device), 30.0);
    device.current_refresh_hz = Some(120.0);
    assert_eq!(policy_budget_fps(90, &device), 90.0);
    device.current_refresh_hz = Some(60.0);
    device.sample_age_secs = 11.0;
    assert_eq!(policy_budget_fps(120, &device), 120.0);
    device.sample_age_secs = 0.0;
    device.reported = false;
    assert_eq!(policy_budget_fps(120, &device), 120.0);
    device.reported = true;
    device.current_refresh_hz = Some(f32::NAN);
    assert_eq!(policy_budget_fps(120, &device), 120.0);
}
