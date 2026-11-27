use super::{VehicleAction, VehicleFleet};
use crate::i18n::{Language, tr};
use crate::input::{Orientation, ScreenRect, orientation, safe_insets};
use bevy::{prelude::*, window::PrimaryWindow};

#[derive(Component)]
pub(crate) struct Root;
#[derive(Component)]
pub(crate) struct Status;
#[derive(Component)]
pub(crate) struct GunButton(usize);
#[derive(Component)]
pub(crate) struct GunLabel(usize);

pub(crate) fn panel_layout(
    viewport: Vec2,
    presented: Option<&crate::hud::HudGeometry>,
    armored: bool,
) -> ScreenRect {
    let safe = safe_insets(viewport);
    let presented = presented.filter(|geometry| geometry.viewport == viewport);
    let card = presented
        .and_then(|geometry| geometry.card)
        .unwrap_or_else(|| crate::hud::survival_hud_layout(viewport, true).card);
    let banner = presented.and_then(|geometry| geometry.banner);
    if orientation(viewport) == Orientation::Portrait {
        let gap = if banner.is_some_and(|banner| banner.height() < 94.0) {
            4.0
        } else {
            6.0
        };
        let mut panel = ScreenRect::new(
            safe.left,
            banner.map_or(card.max.y, |banner| banner.max.y) + gap,
            viewport.x - safe.left - safe.right,
            if armored { 76.0 } else { 44.0 },
        );
        let percent = presented.map_or(100, |geometry| geometry.button_percent);
        let pads = crate::input::HudLayout::for_controls(viewport, percent, true);
        let map = crate::worldmap::toggle_rect_for_controls(viewport, percent, true);
        if panel.intersects(map)
            || pads.sec.is_some_and(|rect| panel.intersects(rect))
            || pads.act.is_some_and(|rect| panel.intersects(rect))
        {
            // A short phone can show conditions + a radio card together. Use
            // the vacant left column above its joystick instead of covering
            // the map or either thumb control with status/reload targets.
            let right = pads
                .sec
                .into_iter()
                .chain(pads.act)
                .map(|rect| rect.min.x)
                .fold(map.min.x, f32::min)
                - 8.0;
            panel.max.x = right;
            let desired_height: f32 = if armored { 76.0 } else { 60.0 };
            panel.max.y =
                panel.min.y + desired_height.min((pads.joy.min.y - 8.0 - panel.min.y).max(48.0));
        }
        panel
    } else {
        ScreenRect::new(
            card.max.x + 8.0,
            safe.top,
            (viewport.x - card.max.x - safe.right - 8.0).max(1.0),
            if armored { 48.0 } else { 32.0 },
        )
    }
}
fn button_layout(viewport: Vec2, presented: Option<&crate::hud::HudGeometry>) -> [ScreenRect; 2] {
    let panel = panel_layout(viewport, presented, true);
    let portrait = orientation(viewport) == Orientation::Portrait;
    let height = if portrait && panel.height() >= 64.0 {
        32.0
    } else {
        28.0
    };
    let width = (panel.width() - 12.0) * 0.5;
    [
        ScreenRect::new(panel.min.x + 4.0, panel.max.y - height - 3.0, width, height),
        ScreenRect::new(
            panel.min.x + 8.0 + width,
            panel.max.y - height - 3.0,
            width,
            height,
        ),
    ]
}
pub(crate) fn captures_pointer(
    viewport: Vec2,
    point: Vec2,
    armored: bool,
    presented: Option<&crate::hud::HudGeometry>,
) -> bool {
    armored
        && button_layout(viewport, presented)
            .iter()
            .any(|rect| rect.contains(point))
}
pub(crate) fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Root,
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::all(px(4)),
                overflow: Overflow::clip(),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.06, 0.07, 0.84)),
            Visibility::Hidden,
            GlobalZIndex(28),
            Pickable::IGNORE,
        ))
        .with_child((
            Status,
            Text::new(""),
            TextFont::from_font_size(FontSize::Px(11.0)),
            TextColor(Color::srgb(0.92, 0.96, 0.82)),
            TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
        ));
    for i in 0..2 {
        commands
            .spawn((
                GunButton(i),
                Node {
                    position_type: PositionType::Absolute,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.15, 0.24, 0.22, 0.96)),
                Visibility::Hidden,
                GlobalZIndex(29),
                Pickable::IGNORE,
            ))
            .with_child((
                GunLabel(i),
                Text::new(""),
                TextFont::from_font_size(FontSize::Px(13.0)),
                TextColor(Color::WHITE),
            ));
    }
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    windows: Query<&Window, With<PrimaryWindow>>,
    fleet: Res<VehicleFleet>,
    lang: Res<Language>,
    flow: Res<State<crate::game_flow::GameFlow>>,
    pause: Res<crate::ui::PauseOpen>,
    modal: Res<crate::ui::UiModal>,
    geometry: Option<Res<crate::hud::HudGeometry>>,
    mut roots: Query<(&mut Node, &mut Visibility), With<Root>>,
    mut text: Query<&mut Text, With<Status>>,
    mut buttons: Query<(&GunButton, &mut Node, &mut Visibility), Without<Root>>,
    mut labels: Query<(&GunLabel, &mut Text), Without<Status>>,
    mut logged: Local<Option<(u32, Vec2, ScreenRect)>>,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };
    let viewport = Vec2::new(window.width(), window.height());
    let active = flow.get() == &crate::game_flow::GameFlow::Playing && !pause.0 && !modal.open;
    let current = active.then(|| fleet.current()).flatten();
    let panel = panel_layout(
        viewport,
        geometry.as_deref(),
        current.is_some_and(|v| v.spec().armored),
    );
    let log_key = current.map(|v| (v.id, viewport, panel));
    if *logged != log_key {
        *logged = log_key;
        if let Some(v) = current {
            let rect = |r: ScreenRect| [r.min.x, r.min.y, r.max.x, r.max.y];
            println!(
                "DTVEHICLEUI layout={}",
                serde_json::json!({
                    "viewport": [viewport.x, viewport.y], "id": v.id, "kind": v.kind,
                    "panel": rect(panel),
                    "buttons": if v.spec().armored { button_layout(viewport, geometry.as_deref()).map(rect).to_vec() } else { Vec::new() }
                })
            );
        }
    }
    for (mut node, mut visibility) in &mut roots {
        node.left = px(panel.min.x);
        node.top = px(panel.min.y);
        node.width = px(panel.width());
        node.height = px(panel.height());
        *visibility = if current.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for mut text in &mut text {
        text.0 = if let Some(v) = current {
            if let Some(gun) = v.gun.as_ref().and_then(|g| g.guns.get(g.selected)) {
                let spec = gun.weapon.spec();
                let name = tr(*lang, spec.name_zh, spec.name_en);
                if panel.height() < 64.0 && orientation(viewport) == Orientation::Portrait {
                    format!(
                        "{name} {}/{}{}",
                        gun.ready,
                        gun.reserve,
                        if gun.reload_remaining > 0.0 {
                            format!(" · {:.0}s", gun.reload_remaining)
                        } else if gun.jammed {
                            format!(" · {}", tr(*lang, "過熱", "Hot"))
                        } else {
                            String::new()
                        }
                    )
                } else {
                    format!(
                        "{:.0} km/h · {} {:.0}% · {name} {}/{} · {} {:.0}%{}",
                        v.speed.abs() * 3.6,
                        tr(*lang, "燃油", "Fuel"),
                        v.fuel / v.spec().fuel_capacity * 100.0,
                        gun.ready,
                        gun.reserve,
                        tr(*lang, "熱度", "Heat"),
                        gun.heat,
                        if gun.reload_remaining > 0.0 {
                            format!(" · {:.1}s", gun.reload_remaining)
                        } else if gun.jammed {
                            format!(" · {}", tr(*lang, "過熱", "Hot"))
                        } else {
                            String::new()
                        }
                    )
                }
            } else {
                format!("{:.0} km/h · {}", v.speed.abs() * 3.6, v.status(*lang))
            }
        } else {
            String::new()
        };
    }
    let rects = button_layout(viewport, geometry.as_deref());
    for (button, mut node, mut visibility) in &mut buttons {
        let r = rects[button.0];
        node.left = px(r.min.x);
        node.top = px(r.min.y);
        node.width = px(r.width());
        node.height = px(r.height());
        *visibility = if current.is_some_and(|v| v.gun.is_some()) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for (label, mut text) in &mut labels {
        text.0 = if label.0 == 0 {
            tr(*lang, "裝填", "Reload")
        } else {
            tr(*lang, "切換武器", "Switch gun")
        }
        .into();
    }
}
pub(crate) fn button_actions(
    windows: Query<&Window, With<PrimaryWindow>>,
    fleet: Res<VehicleFleet>,
    presses: Res<crate::ui::ConsumedPresses>,
    flow: Res<State<crate::game_flow::GameFlow>>,
    pause: Res<crate::ui::PauseOpen>,
    modal: Res<crate::ui::UiModal>,
    geometry: Option<Res<crate::hud::HudGeometry>>,
    mut actions: MessageWriter<VehicleAction>,
) {
    if flow.get() != &crate::game_flow::GameFlow::Playing
        || pause.0
        || modal.open
        || !fleet.current().is_some_and(|v| v.spec().armored)
    {
        return;
    }
    let Some(window) = windows.iter().next() else {
        return;
    };
    let rects = button_layout(
        Vec2::new(window.width(), window.height()),
        geometry.as_deref(),
    );
    for p in &presses.points {
        if rects[0].contains(*p) {
            actions.write(VehicleAction::ReloadGun);
        } else if rects[1].contains(*p) {
            actions.write(VehicleAction::SwitchGun);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_and_mounted_targets_follow_actual_hud_without_covering_controls() {
        for viewport in [
            Vec2::new(320.0, 568.0),
            Vec2::new(320.0, 640.0),
            Vec2::new(360.0, 640.0),
            Vec2::new(390.0, 844.0),
            Vec2::new(800.0, 360.0),
            Vec2::new(844.0, 390.0),
            Vec2::new(1280.0, 720.0),
        ] {
            for percent in [85, 100, 110] {
                let pads = crate::input::HudLayout::for_controls(viewport, percent, true);
                let map = crate::worldmap::toggle_rect_for_controls(viewport, percent, true);
                for conditions in [false, true] {
                    let hud = crate::hud::survival_hud_layout_for_controls(
                        viewport, conditions, percent, true,
                    );
                    for radio in [false, true] {
                        let geometry = crate::hud::HudGeometry {
                            viewport,
                            button_percent: percent,
                            card: Some(hud.card),
                            banner: radio.then_some(hud.banner),
                        };
                        for armored in [false, true] {
                            let panel = panel_layout(viewport, Some(&geometry), armored);
                            assert!(
                                panel.width() >= 160.0 && panel.height() >= 32.0,
                                "{viewport:?}/{conditions}/{radio}: {panel:?}"
                            );
                            assert!(
                                panel.min.cmpge(Vec2::ZERO).all()
                                    && panel.max.cmple(viewport).all()
                            );
                            for reserved in [
                                Some(hud.card),
                                geometry.banner,
                                Some(pads.joy),
                                pads.act,
                                pads.sec,
                                Some(map),
                            ]
                            .into_iter()
                            .flatten()
                            {
                                assert!(
                                    !panel.intersects(reserved),
                                    "{viewport:?}/{percent}/{conditions}/{radio}/{armored}: {panel:?} overlaps {reserved:?}"
                                );
                            }
                            if armored {
                                let buttons = button_layout(viewport, Some(&geometry));
                                assert!(!buttons[0].intersects(buttons[1]));
                                for button in buttons {
                                    assert!(button.width() >= 68.0 && button.height() >= 28.0);
                                    assert!(
                                        button.min.cmpge(panel.min).all()
                                            && button.max.cmple(panel.max).all()
                                    );
                                    assert!(captures_pointer(
                                        viewport,
                                        button.center(),
                                        true,
                                        Some(&geometry)
                                    ));
                                }
                                assert!(!captures_pointer(
                                    viewport,
                                    map.center(),
                                    true,
                                    Some(&geometry)
                                ));
                                assert!(!captures_pointer(
                                    viewport,
                                    pads.act.unwrap().center(),
                                    true,
                                    Some(&geometry)
                                ));
                            }
                        }
                    }
                }
            }
        }
        let viewport = Vec2::new(390.0, 844.0);
        let hud = crate::hud::survival_hud_layout_for_controls(viewport, false, 110, true);
        let geometry = crate::hud::HudGeometry {
            viewport,
            button_percent: 110,
            card: Some(hud.card),
            banner: None,
        };
        let panel = panel_layout(viewport, Some(&geometry), false);
        assert_eq!(panel.min.y, 200.0);
        assert_eq!(panel.height(), 44.0);
    }
}
