//! Opt-in overlay over real app-loop measurements. No per-frame sorting.
use crate::{
    input::{Orientation, ScreenRect, orientation, safe_insets},
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

/// Reserve the full survival HUD and radio card, even when the current run has
/// fewer condition rows. A very small viewport uses the full diagnostics page
/// instead of painting telemetry across survival information or controls.
pub(crate) fn overlay_layout(viewport: Vec2) -> Option<ScreenRect> {
    overlay_layout_for_controls(viewport, false, 100)
}

fn overlay_layout_for_controls(
    viewport: Vec2,
    driving: bool,
    button_percent: u8,
) -> Option<ScreenRect> {
    let safe = safe_insets(viewport);
    let mut hud = crate::hud::survival_hud_layout(viewport, true);
    let pad = crate::input::HudLayout::for_controls(viewport, button_percent, driving);
    let controls_top = pad
        .sec
        .map_or(pad.joy.min.y, |sec| sec.min.y.min(pad.joy.min.y));
    hud.hint.min.y = controls_top - 48.0;
    hud.hint.max.y = controls_top - 8.0;
    let portrait = orientation(viewport) == Orientation::Portrait;
    let x = if portrait {
        safe.left
    } else {
        hud.card.max.x + 8.0
    };
    let y = hud
        .banner
        .max
        .y
        .max(if portrait { 0.0 } else { hud.hint.max.y })
        + 6.0
        + if portrait && driving { 84.0 } else { 0.0 };
    let right = if !portrait {
        let controls = crate::input::HudLayout::for_controls(viewport, button_percent, driving);
        controls
            .act
            .into_iter()
            .chain(controls.sec)
            .chain(Some(crate::worldmap::toggle_rect_for_controls(
                viewport,
                button_percent,
                driving,
            )))
            .map(|rect| rect.min.x - 8.0)
            .fold(viewport.x - safe.right, f32::min)
    } else {
        viewport.x - safe.right
    };
    let width = right - x;
    let bottom = if portrait {
        hud.hint.min.y - 6.0
    } else {
        viewport.y - safe.bottom - 54.0
    };
    let height = (bottom - y).min(84.0);
    (width >= 180.0 && height >= 42.0).then(|| ScreenRect::new(x, y, width, height))
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
                left: px(10),
                top: px(10),
                padding: UiRect::axes(px(8), px(6)),
                border_radius: BorderRadius::all(px(6)),
                overflow: Overflow::clip(),
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
    fleet: Option<Res<crate::vehicles::VehicleFleet>>,
    controls: Option<Res<crate::input::ControlPreferences>>,
    mut roots: Query<(&mut Node, &mut Visibility), With<PerfRoot>>,
    mut texts: Query<&mut Text, With<PerfText>>,
) {
    timers.display += time.delta_secs();
    timers.log += time.delta_secs();
    let enabled = environment_override().unwrap_or(preferences.overlay);
    for (mut node, mut visibility) in &mut roots {
        let driving = fleet.as_ref().is_some_and(|fleet| fleet.current.is_some());
        let layout = window.as_ref().and_then(|window| {
            if driving {
                overlay_layout_for_controls(
                    window.size(),
                    true,
                    controls.as_ref().map_or(100, |prefs| prefs.button_percent),
                )
            } else {
                overlay_layout(window.size())
            }
        });
        visibility.set_if_neq(if enabled && layout.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if let Some(rect) = layout {
            node.top = px(rect.min.y);
            node.left = px(rect.min.x);
            node.width = px(rect.width());
            node.height = px(rect.height());
        }
    }
    let summary = &metrics.summary;
    if timers.display >= 0.5 {
        timers.display = 0.0;
        if enabled && let Ok(mut text) = texts.single_mut() {
            text.0 = format!(
                "FPS {:.1} / {}{} · P95 {:.1} · P99 {:.1} ms\n3D {}×{} · {}% · entities {}\n{}",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_leave_the_map_and_actual_vehicle_panel_accessible() {
        for size in [
            Vec2::new(320.0, 568.0),
            Vec2::new(360.0, 640.0),
            Vec2::new(390.0, 844.0),
            Vec2::new(800.0, 360.0),
            Vec2::new(844.0, 390.0),
            Vec2::new(1280.0, 720.0),
        ] {
            for percent in [85, 100, 110] {
                for driving in [false, true] {
                    let Some(overlay) = overlay_layout_for_controls(size, driving, percent) else {
                        continue;
                    };
                    let map = crate::worldmap::toggle_rect_for_controls(size, percent, driving);
                    assert!(
                        !overlay.intersects(map),
                        "{size:?}/{percent}/{driving}: map overlaps diagnostics"
                    );
                    if driving {
                        for conditions in [false, true] {
                            for radio in [false, true] {
                                let hud = crate::hud::survival_hud_layout_for_controls(
                                    size, conditions, percent, true,
                                );
                                let geometry = crate::hud::HudGeometry {
                                    viewport: size,
                                    button_percent: percent,
                                    card: Some(hud.card),
                                    banner: radio.then_some(hud.banner),
                                };
                                for armored in [false, true] {
                                    let panel = crate::vehicles::ui::panel_layout(
                                        size,
                                        Some(&geometry),
                                        armored,
                                    );
                                    assert!(
                                        !overlay.intersects(panel),
                                        "{size:?}/{percent}/{conditions}/{radio}/{armored}: status overlaps diagnostics"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn portrait_and_landscape_diagnostics_never_cover_survival_instruments() {
        for size in [
            Vec2::new(390.0, 844.0),
            Vec2::new(844.0, 390.0),
            Vec2::new(1280.0, 720.0),
            Vec2::new(720.0, 1280.0),
        ] {
            let overlay = overlay_layout(size).expect("normal viewport can show diagnostics");
            let hud = crate::hud::survival_hud_layout(size, true);
            for reserved in [hud.card, hud.banner, hud.hint] {
                assert!(
                    !overlay.intersects(reserved),
                    "{size:?}: {overlay:?} intersects {reserved:?}"
                );
            }
            let controls = crate::input::HudLayout::from_viewport(size);
            assert!(!overlay.intersects(controls.joy));
            assert!(controls.act.is_none_or(|rect| !overlay.intersects(rect)));
            assert!(controls.sec.is_none_or(|rect| !overlay.intersects(rect)));
            assert!(overlay.max.x <= size.x && overlay.max.y <= size.y);
        }
    }
}
