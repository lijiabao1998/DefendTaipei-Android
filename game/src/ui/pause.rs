//! Pause screen on top of [`GameFlow::Playing`]. It is not a flow state.
//!
//! Visibility follows [`super::TogglePause`]. This module does not read the keyboard.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::audio::SfxEvent;
use crate::game_flow::GameFlow;
use crate::i18n::{Language, UiFont, tr};
use crate::input::ScreenRect;
use crate::save::{SaveAndMenu, SaveStatus};

use super::layout::pause_layout;
use super::{ConsumedPresses, PauseOpen, StateScoped, TogglePause};

#[derive(Component)]
pub(super) struct PauseRoot;

#[derive(Component)]
pub(super) struct ResumeButton;

#[derive(Component)]
pub(super) struct MenuButton;

#[derive(Component)]
pub(super) struct PauseHeading;

#[derive(Component)]
pub(super) struct PauseStatus;

pub(super) fn drive_pause(
    mut commands: Commands,
    flow: Res<State<GameFlow>>,
    mut pause: ResMut<PauseOpen>,
    mut toggles: MessageReader<TogglePause>,
    consumed: Res<ConsumedPresses>,
    windows: Query<&Window, With<PrimaryWindow>>,
    font: Res<UiFont>,
    lang: Res<Language>,
    mut sfx: MessageWriter<SfxEvent>,
    mut save_menu: MessageWriter<SaveAndMenu>,
    existing: Query<Entity, With<PauseRoot>>,
) {
    if !matches!(flow.get(), GameFlow::Playing) {
        toggles.clear();
        close(&mut commands, &mut pause, &existing);
        return;
    }
    let viewport = windows
        .iter()
        .next()
        .map(|window| Vec2::new(window.width(), window.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    let layout = pause_layout(viewport);
    let on_resume = consumed
        .points
        .iter()
        .any(|point| layout.resume.contains(*point));
    let on_menu = consumed
        .points
        .iter()
        .any(|point| layout.menu.contains(*point));
    let mut open = pause.0;
    for _ in toggles.read() {
        open = !open;
    }
    if open && on_resume {
        open = false;
    } else if open && on_menu {
        save_menu.write(SaveAndMenu);
    }
    let changed = open != pause.0;
    pause.0 = open;
    if changed {
        sfx.write(SfxEvent::Clink { dist: 0.0 });
    }
    if open && existing.is_empty() {
        spawn_pause(&mut commands, viewport, &font, *lang);
    } else if !open {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
    }
}

pub(super) fn place_pause(
    windows: Query<&Window, With<PrimaryWindow>>,
    lang: Res<Language>,
    save: Res<SaveStatus>,
    mut heading: Query<(&mut Node, &mut Text), With<PauseHeading>>,
    mut resume: Query<(&mut Node, &mut Text), (With<ResumeButton>, Without<PauseHeading>)>,
    mut menu: Query<
        (&mut Node, &mut Text),
        (
            With<MenuButton>,
            Without<PauseHeading>,
            Without<ResumeButton>,
        ),
    >,
    mut status: Query<
        (&mut Node, &mut Text),
        (
            With<PauseStatus>,
            Without<PauseHeading>,
            Without<ResumeButton>,
            Without<MenuButton>,
        ),
    >,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };
    let layout = pause_layout(Vec2::new(window.width(), window.height()));
    for (mut node, mut text) in &mut heading {
        place(&mut node, layout.heading);
        let next = tr(*lang, "暫停", "Paused");
        if text.0 != next {
            text.0 = next.to_string();
        }
    }
    for (mut node, mut text) in &mut resume {
        place(&mut node, layout.resume);
        let next = tr(*lang, "繼續", "Resume");
        if text.0 != next {
            text.0 = next.to_string();
        }
    }
    for (mut node, mut text) in &mut menu {
        place(&mut node, layout.menu);
        let next = tr(*lang, "儲存並返回主選單", "Save and return to menu");
        if text.0 != next {
            text.0 = next.to_string();
        }
    }
    for (mut node, mut text) in &mut status {
        place(&mut node, layout.status);
        let next = save.error.as_deref().unwrap_or(&save.notice);
        if text.0 != next {
            text.0 = next.to_owned();
        }
    }
}

fn close(
    commands: &mut Commands,
    pause: &mut PauseOpen,
    existing: &Query<Entity, With<PauseRoot>>,
) {
    pause.0 = false;
    for entity in existing {
        commands.entity(entity).despawn();
    }
}

fn spawn_pause(commands: &mut Commands, viewport: Vec2, font: &UiFont, lang: Language) {
    let layout = pause_layout(viewport);
    commands
        .spawn((
            PauseRoot,
            StateScoped(GameFlow::Playing),
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                top: px(0.0),
                width: percent(100.0),
                height: percent(100.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.025, 0.72)),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            GlobalZIndex(50),
        ))
        .with_children(|parent| {
            parent.spawn(label(
                PauseHeading,
                layout.heading,
                28.0,
                font,
                tr(lang, "暫停", "Paused"),
            ));
            parent.spawn(button(
                ResumeButton,
                layout.resume,
                font,
                tr(lang, "繼續", "Resume"),
            ));
            parent.spawn(button(
                MenuButton,
                layout.menu,
                font,
                tr(lang, "儲存並返回主選單", "Save and return to menu"),
            ));
            parent.spawn(label(PauseStatus, layout.status, 14.0, font, ""));
        });
}

fn label(
    marker: impl Component,
    rect: ScreenRect,
    size: f32,
    font: &UiFont,
    text: &str,
) -> impl Bundle {
    (
        marker,
        placed_node(rect, false),
        Text::new(text),
        TextFont::from_font_size(FontSize::Px(size)).with_font(font.0.clone()),
        TextColor(Color::srgb(0.96, 0.93, 0.84)),
        TextLayout::justify(Justify::Center),
    )
}

fn button(marker: impl Component, rect: ScreenRect, font: &UiFont, text: &str) -> impl Bundle {
    (
        marker,
        placed_node(rect, true),
        BackgroundColor(Color::srgb(0.45, 0.32, 0.12)),
        BorderColor::all(Color::srgba(0.95, 0.85, 0.55, 0.8)),
        Text::new(text),
        TextFont::from_font_size(FontSize::Px(22.0)).with_font(font.0.clone()),
        TextColor(Color::srgb(0.98, 0.96, 0.9)),
        TextLayout::justify(Justify::Center),
    )
}

fn placed_node(rect: ScreenRect, border: bool) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(rect.min.x),
        top: px(rect.min.y),
        width: px(rect.width()),
        height: px(rect.height()),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border: UiRect::all(px(if border { 1.0 } else { 0.0 })),
        border_radius: BorderRadius::all(px(8.0)),
        ..default()
    }
}

fn place(node: &mut Node, rect: ScreenRect) {
    node.left = px(rect.min.x);
    node.top = px(rect.min.y);
    node.width = px(rect.width());
    node.height = px(rect.height());
}
