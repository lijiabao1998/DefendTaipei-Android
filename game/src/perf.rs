//! Opt-in overlay over real app-loop measurements. No per-frame sorting.
use crate::{
    input::safe_insets,
    performance::{
        EffectivePerformance, PerformanceMetrics, PerformancePreferences, render::RenderDimensions,
    },
};
use bevy::{prelude::*, window::PrimaryWindow};
#[derive(Component)]
struct PerfRoot;
#[derive(Component)]
struct PerfText;
#[derive(Resource, Default)]
struct PerfTimers {
    display: f32,
    log: f32,
}
pub struct PerfPlugin;
impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        println!("DTPERF boot measurement=app_frame_interval");
        app.init_resource::<PerfTimers>()
            .add_systems(Startup, spawn_overlay)
            .add_systems(
                Update,
                (toggle, display)
                    .chain()
                    .after(crate::performance::PerformanceSet),
            );
    }
}
fn environment_override() -> Option<bool> {
    std::env::var("DEFEND_TAIPEI_PERF").ok().and_then(|v| {
        match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "on" => Some(true),
            "0" | "false" | "off" => Some(false),
            _ => None,
        }
    })
}
fn spawn_overlay(mut commands: Commands) {
    commands
        .spawn((
            PerfRoot,
            Node {
                position_type: PositionType::Absolute,
                right: px(10),
                top: px(10),
                padding: UiRect::axes(px(8), px(6)),
                border_radius: BorderRadius::all(px(6)),
                max_width: percent(42),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
            Visibility::Hidden,
            GlobalZIndex(30),
            Pickable::IGNORE,
        ))
        .with_child((
            PerfText,
            Text::new("PERF"),
            TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(Color::srgb(0.85, 1.0, 0.75)),
        ));
}
fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    touches: Res<Touches>,
    mut preferences: ResMut<PerformancePreferences>,
) {
    if keys.just_pressed(KeyCode::F3) || (touches.any_just_pressed() && touches.iter().count() >= 3)
    {
        preferences.overlay = !preferences.overlay;
        println!(
            "DTPERF overlay_toggled={}",
            if preferences.overlay { "on" } else { "off" }
        );
    }
}
fn display(
    time: Res<Time<Real>>,
    mut timers: ResMut<PerfTimers>,
    preferences: Res<PerformancePreferences>,
    metrics: Res<PerformanceMetrics>,
    effective: Res<EffectivePerformance>,
    dimensions: Res<RenderDimensions>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    mut roots: Query<(&mut Node, &mut Visibility), With<PerfRoot>>,
    mut texts: Query<&mut Text, With<PerfText>>,
) {
    timers.display += time.delta_secs();
    timers.log += time.delta_secs();
    let enabled = environment_override().unwrap_or(preferences.overlay);
    for (mut node, mut visibility) in &mut roots {
        visibility.set_if_neq(if enabled {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if let Some(window) = window.as_ref() {
            let size = window.size();
            let safe = safe_insets(size);
            node.top = px(safe.top + 10.0);
            node.right = px(safe.right + 10.0);
            node.max_width = px((size.x * 0.42)
                .min(size.x - safe.left - safe.right - 24.0)
                .max(96.0));
        }
    }
    let summary = &metrics.summary;
    if timers.display >= 0.5 {
        timers.display = 0.0;
        if enabled && let Ok(mut text) = texts.single_mut() {
            text.0 = format!(
                "FPS {:.1} / {}{}\nP95 {:.1}  P99 {:.1} ms\n3D {}×{} · {}%\nentities {}\n{}",
                summary.fps,
                effective.target_fps,
                if metrics.paused { " (paused)" } else { "" },
                summary.p95_ms,
                summary.p99_ms,
                dimensions.scene[0],
                dimensions.scene[1],
                effective.render_scale,
                metrics.entities,
                effective.reason
            );
        }
    }
    if timers.log >= 5.0 {
        timers.log = 0.0;
        println!(
            "DTPERF fps={:.1} avg_ms={:.2} p99_ms={:.2} max_ms={:.2} entities={} overlay={} target={} scale={} paused={} measurement=app_frame_interval",
            summary.fps,
            summary.mean_ms,
            summary.p99_ms,
            summary.max_ms,
            metrics.entities,
            enabled as u8,
            effective.target_fps,
            effective.render_scale,
            metrics.paused as u8
        );
    }
}
