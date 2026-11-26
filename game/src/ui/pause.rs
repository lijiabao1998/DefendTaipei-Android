//! Pause screen on top of [`GameFlow::Playing`]. It is not a flow state.
//!
//! Visibility follows [`super::TogglePause`]. This module does not read the keyboard.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::audio::SfxEvent;
use crate::combat::GameState;
use crate::game_flow::{Campaign, GameFlow};
use crate::i18n::{Language, UiFont, tr};
use crate::input::ScreenRect;
use crate::save::{SaveAndMenu, SaveStatus};
use crate::survival::Shelter;

use super::layout::pause_layout;
use super::{ConsumedPresses, PauseOpen, StateScoped, TogglePause};

#[derive(Component)]
pub(super) struct PauseRoot;

#[derive(Component)]
pub(super) struct ResumeButton;

#[derive(Component)]
pub(super) struct MenuButton;

#[derive(Component)]
pub(super) struct SettingsButton;

#[derive(Component)]
pub(super) struct PauseHeading;

#[derive(Component)]
pub(super) struct PauseStatus;
#[derive(Component)]
pub(super) struct PauseSummary;
#[derive(Component)]
pub(super) struct PauseFrame;

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
    mut settings: ResMut<super::settings::SettingsUi>,
    mut flush: MessageWriter<crate::settings::FlushSettings>,
) {
    if !matches!(flow.get(), GameFlow::Playing) {
        toggles.clear();
        close(&mut commands, &mut pause, &existing);
        return;
    }
    if settings.open || settings.captured {
        if !toggles.is_empty() && settings.open {
            settings.open = false;
            settings.captured = true;
            flush.write(crate::settings::FlushSettings);
            println!("DTUI action=SettingsClose");
        }
        toggles.clear();
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
    let on_settings = consumed
        .points
        .iter()
        .any(|point| layout.settings.contains(*point));
    let mut open = pause.0;
    for _ in toggles.read() {
        open = !open;
    }
    if open && on_resume {
        open = false;
    } else if open && on_menu {
        println!("DTUI action=SaveMenu");
        save_menu.write(SaveAndMenu);
    }
    if open && on_settings {
        settings.open = true;
        settings.captured = true;
        sfx.write(SfxEvent::Ui { confirm: false });
        println!("DTUI action=SettingsOpen");
    }
    let changed = open != pause.0;
    pause.0 = open;
    if changed {
        println!("DTUI pause={open}");
        sfx.write(SfxEvent::Ui { confirm: false });
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
    campaign: Option<Res<Campaign>>,
    shelter: Option<Res<Shelter>>,
    game: Option<Res<GameState>>,
    mut decorations: Query<
        (&mut Node, Option<&PauseFrame>),
        (
            Or<(With<PauseFrame>, With<PauseSummary>)>,
            Without<PauseHeading>,
            Without<ResumeButton>,
            Without<MenuButton>,
            Without<PauseStatus>,
        ),
    >,
    mut summary: Query<
        &mut Text,
        (
            With<PauseSummary>,
            Without<PauseHeading>,
            Without<ResumeButton>,
            Without<MenuButton>,
            Without<PauseStatus>,
        ),
    >,
    mut heading: Query<
        (&mut Node, &mut Text),
        (
            With<PauseHeading>,
            Without<PauseSummary>,
            Without<PauseFrame>,
        ),
    >,
    mut resume: Query<
        (&mut Node, &mut Text),
        (
            With<ResumeButton>,
            Without<PauseHeading>,
            Without<PauseSummary>,
            Without<PauseFrame>,
        ),
    >,
    mut menu: Query<
        (&mut Node, &mut Text),
        (
            With<MenuButton>,
            Without<PauseHeading>,
            Without<ResumeButton>,
            Without<PauseSummary>,
            Without<PauseFrame>,
        ),
    >,
    mut setting_buttons: Query<
        (&mut Node, &mut Text),
        (
            With<SettingsButton>,
            Without<PauseHeading>,
            Without<ResumeButton>,
            Without<MenuButton>,
            Without<PauseSummary>,
            Without<PauseFrame>,
            Without<PauseStatus>,
        ),
    >,
    mut status: Query<
        (&mut Node, &mut Text),
        (
            With<PauseStatus>,
            Without<PauseHeading>,
            Without<ResumeButton>,
            Without<MenuButton>,
            Without<PauseSummary>,
            Without<PauseFrame>,
        ),
    >,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };
    let layout = pause_layout(Vec2::new(window.width(), window.height()));
    for (mut node, frame) in &mut decorations {
        place(
            &mut node,
            if frame.is_some() {
                layout.frame
            } else {
                layout.summary
            },
        );
    }
    let minute = campaign.as_ref().map_or(0, |c| c.game_min.max(0.0) as u32);
    let day = shelter.as_ref().map_or(1, |s| s.day);
    let hp = game.as_ref().map_or(100.0, |g| g.hp);
    let copy = format!(
        "{} · {} {} · {:02}:{:02} · HP {:.0}",
        tr(*lang, "萬華", "Wanhua"),
        tr(*lang, "第", "Day"),
        day,
        minute / 60 % 24,
        minute % 60,
        hp
    );
    for mut text in &mut summary {
        if text.0 != copy {
            text.0 = copy.clone();
        }
    }
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
    for (mut node, mut text) in &mut setting_buttons {
        place(&mut node, layout.settings);
        text.0 = tr(*lang, "設定", "Settings").to_owned();
    }
    for (mut node, mut text) in &mut status {
        place(&mut node, layout.status);
        let next = save
            .error
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if save.notice.is_empty() {
                    tr(
                        *lang,
                        "時間已暫停。離開前儲存，才能從這裡繼續。",
                        "Time is paused. Save before leaving to continue from here.",
                    )
                } else {
                    &save.notice
                }
            });
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
            BackgroundColor(Color::srgba(0.02, 0.025, 0.03, 0.80)),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            GlobalZIndex(50),
        ))
        .with_children(|parent| {
            parent.spawn((
                PauseFrame,
                placed_node(layout.frame, true),
                BackgroundColor(Color::srgb(0.065, 0.075, 0.078)),
                BorderColor::all(Color::srgb(0.51, 0.44, 0.28)),
            ));
            parent.spawn(label(PauseSummary, layout.summary, 13.0, font, ""));
            parent.spawn(label(
                PauseHeading,
                layout.heading,
                28.0,
                font,
                tr(lang, "暫停", "Paused"),
            ));
            parent
                .spawn(button(
                    ResumeButton,
                    layout.resume,
                    font,
                    tr(lang, "繼續", "Resume"),
                ))
                .insert((
                    BackgroundColor(Color::srgb(0.76, 0.63, 0.36)),
                    TextColor(Color::srgb(0.10, 0.11, 0.09)),
                ));
            parent.spawn(button(
                MenuButton,
                layout.menu,
                font,
                tr(lang, "儲存並返回主選單", "Save and return to menu"),
            ));
            parent.spawn(button(
                SettingsButton,
                layout.settings,
                font,
                tr(lang, "設定", "Settings"),
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
    // A Bevy Text leaf does not vertically centre its own glyphs through flex
    // alignment. Inset the line box while preserving the full finger target.
    let mut node = placed_node(rect, true);
    node.padding = UiRect::vertical(px(((rect.height() - 26.0) * 0.5).max(0.0)));
    (
        marker,
        node,
        BackgroundColor(Color::srgb(0.14, 0.16, 0.15)),
        BorderColor::all(Color::srgba(0.95, 0.85, 0.55, 0.8)),
        Text::new(text),
        TextFont::from_font_size(FontSize::Px(20.0)).with_font(font.0.clone()),
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
