//! Prologue and the four ending screens. Title navigation lives in `title`.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::audio::SfxEvent;
use crate::combat::GameState;
use crate::game_flow::{Campaign, Ending, GameFlow, RestartGame, SkipPrologue};
use crate::i18n::{Language, UiFont, tr};
use crate::input::ScreenRect;
use crate::survival::Shelter;

use super::layout::{death_layout, prologue_layout};
use super::pause::{MenuButton, PauseHeading, ResumeButton};
use super::{ConsumedPresses, StateScoped};

#[derive(Component)]
pub(super) struct HeadingText;
#[derive(Component)]
pub(super) struct IntroText;
#[derive(Component)]
pub(super) struct DetailText;
#[derive(Component)]
pub(super) struct RestartButton;
#[derive(Component)]
pub(super) struct SkipButton;
#[derive(Component)]
pub(super) struct PrologueRoot;
#[derive(Component)]
pub(super) struct EndingRoot;

pub(super) fn restyle(
    flow: Res<State<GameFlow>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut nodes: Query<
        (
            &mut Node,
            &mut TextFont,
            Option<&HeadingText>,
            Option<&IntroText>,
            Option<&DetailText>,
            Option<&RestartButton>,
            Option<&SkipButton>,
        ),
        (
            Without<ResumeButton>,
            Without<MenuButton>,
            Without<PauseHeading>,
        ),
    >,
) {
    let viewport = viewport_of(&windows);
    let layout = match flow.get() {
        GameFlow::Prologue => prologue_layout(viewport),
        GameFlow::GameOver(_) => death_layout(viewport),
        _ => return,
    };
    for (mut node, mut font, heading, intro, detail, restart, skip) in &mut nodes {
        let placed = if heading.is_some() {
            Some((layout.heading, layout.heading_font))
        } else if intro.is_some() {
            Some((layout.sub, layout.body_font))
        } else if detail.is_some() {
            Some((layout.body, layout.body_font))
        } else if restart.is_some() || skip.is_some() {
            Some((layout.restart, layout.body_font + 2.0))
        } else {
            None
        };
        if let Some((rect, size)) = placed {
            if node.left != px(rect.min.x)
                || node.top != px(rect.min.y)
                || node.width != px(rect.width())
                || node.height != px(rect.height())
            {
                place(&mut node, rect);
            }
            if font.font_size != FontSize::Px(size) {
                font.font_size = FontSize::Px(size);
            }
        }
    }
}

pub(super) fn refresh_copy(
    flow: Res<State<GameFlow>>,
    lang: Res<Language>,
    game: Res<GameState>,
    campaign: Res<Campaign>,
    shelter: Res<Shelter>,
    mut texts: Query<
        (
            &mut Text,
            Option<&HeadingText>,
            Option<&IntroText>,
            Option<&DetailText>,
            Option<&RestartButton>,
            Option<&SkipButton>,
        ),
        (
            Without<ResumeButton>,
            Without<MenuButton>,
            Without<PauseHeading>,
        ),
    >,
) {
    for (mut text, heading, intro, detail, restart, skip) in &mut texts {
        let next = match flow.get() {
            GameFlow::Prologue if heading.is_some() => {
                Some(tr(*lang, "07:20　鬧鐘響了", "07:20  The alarm goes off").to_owned())
            }
            GameFlow::Prologue if intro.is_some() => Some(
                tr(
                    *lang,
                    "今天也要去西門町",
                    "Another morning heading to Ximending",
                )
                .to_owned(),
            ),
            GameFlow::Prologue if detail.is_some() => Some(
                tr(
                    *lang,
                    "跟著路上的人流。",
                    "Follow the people on the street.",
                )
                .to_owned(),
            ),
            GameFlow::Prologue if skip.is_some() => {
                Some(tr(*lang, "跳過開場 ▶", "Skip intro ▶").to_owned())
            }
            GameFlow::GameOver(ending) => ending_line(
                *ending,
                *lang,
                shelter.day as i32,
                game.kills,
                campaign.reclaimed.len(),
                heading.is_some(),
                intro.is_some(),
                detail.is_some(),
                restart.is_some(),
            )
            .map(|line| {
                if detail.is_some() {
                    format!("{line}\n\n{}", shelter.ending_text(*lang))
                } else {
                    line
                }
            }),
            _ => None,
        };
        if let Some(next) = next
            && text.0 != next
        {
            text.0 = next;
        }
    }
}

pub(super) fn death_clicks(
    flow: Res<State<GameFlow>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    consumed: Res<ConsumedPresses>,
    mut restart: MessageWriter<RestartGame>,
    mut sfx: MessageWriter<SfxEvent>,
) {
    if !matches!(flow.get(), GameFlow::GameOver(_)) {
        return;
    }
    let restart_rect = death_layout(viewport_of(&windows)).restart;
    let on_button = consumed
        .points
        .iter()
        .any(|point| restart_rect.contains(*point));
    if on_button || keys.any_just_pressed([KeyCode::Enter, KeyCode::Space, KeyCode::KeyR]) {
        restart.write(RestartGame);
        sfx.write(SfxEvent::Clink { dist: 0.0 });
    }
}

pub(super) fn prologue_clicks(
    flow: Res<State<GameFlow>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    consumed: Res<ConsumedPresses>,
    mut skip: MessageWriter<SkipPrologue>,
    mut sfx: MessageWriter<SfxEvent>,
) {
    if !matches!(flow.get(), GameFlow::Prologue) {
        return;
    }
    let skip_rect = prologue_layout(viewport_of(&windows)).restart;
    let on_button = consumed
        .points
        .iter()
        .any(|point| skip_rect.contains(*point));
    if on_button || keys.any_just_pressed([KeyCode::Enter, KeyCode::Space, KeyCode::Escape]) {
        println!("DTUI action=Skip");
        skip.write(SkipPrologue);
        sfx.write(SfxEvent::Clink { dist: 0.0 });
    }
}

pub(super) fn ensure_screens(
    mut commands: Commands,
    flow: Res<State<GameFlow>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    font: Res<UiFont>,
    prologue: Query<Entity, With<PrologueRoot>>,
    endings: Query<Entity, With<EndingRoot>>,
) {
    if flow.is_changed() {
        let viewport = viewport_of(&windows);
        println!(
            "DTUI flow={:?} viewport={}x{}",
            flow.get(),
            viewport.x,
            viewport.y
        );
    }
    match flow.get().clone() {
        GameFlow::Prologue if prologue.is_empty() => {
            spawn_overlay(
                &mut commands,
                viewport_of(&windows),
                &font,
                false,
                GameFlow::Prologue,
            );
        }
        GameFlow::GameOver(ending) if endings.is_empty() => {
            spawn_overlay(
                &mut commands,
                viewport_of(&windows),
                &font,
                true,
                GameFlow::GameOver(ending),
            );
        }
        _ => {}
    }
}

fn spawn_overlay(
    commands: &mut Commands,
    viewport: Vec2,
    font: &UiFont,
    ending: bool,
    state: GameFlow,
) {
    let layout = if ending {
        death_layout(viewport)
    } else {
        prologue_layout(viewport)
    };
    let color = if ending {
        Color::srgb(0.86, 0.22, 0.18)
    } else {
        Color::srgb(0.96, 0.82, 0.38)
    };
    let spawn = |parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands| {
        parent.spawn(labeled(
            HeadingText,
            layout.heading,
            layout.heading_font,
            font,
            color,
        ));
        parent.spawn(labeled(
            IntroText,
            layout.sub,
            layout.body_font,
            font,
            Color::WHITE,
        ));
        parent.spawn(labeled(
            DetailText,
            layout.body,
            layout.body_font,
            font,
            Color::srgb(0.95, 0.93, 0.86),
        ));
        if ending {
            parent.spawn(choice_bg(
                RestartButton,
                layout.restart,
                layout.body_font + 2.0,
                font,
                Color::srgb(0.55, 0.12, 0.1),
            ));
        } else {
            parent.spawn(choice(
                SkipButton,
                layout.restart,
                layout.body_font + 2.0,
                font,
            ));
        }
    };
    let mut root = if ending {
        commands.spawn((EndingRoot, StateScoped(state), dimmer(), GlobalZIndex(40)))
    } else {
        commands.spawn((PrologueRoot, StateScoped(state), dimmer(), GlobalZIndex(40)))
    };
    root.with_children(|parent| spawn(parent));
}

fn ending_line(
    ending: Ending,
    lang: Language,
    day: i32,
    kills: u32,
    reclaimed: usize,
    heading: bool,
    intro: bool,
    detail: bool,
    restart: bool,
) -> Option<String> {
    let (head, sub, body, button) = match ending {
        Ending::Death => (
            tr(lang, "你陣亡了", "You died"),
            tr(lang, "這就是你的結局。", "This is how your story ends."),
            match lang {
                Language::ZhTw => format!(
                    "你在淪陷的台北撐過了 {day} 天，擊倒 {kills} 名佔領軍，奪回了 {reclaimed} 個街區。"
                ),
                Language::En => format!(
                    "You survived occupied Taipei for {day} days, defeated {kills} occupation troops and reclaimed {reclaimed} districts."
                ),
            },
            tr(lang, "重新開始（新的倖存者）", "Start again (new survivor)"),
        ),
        Ending::Reclaimed => (
            tr(lang, "奪回萬華", "Wanhua reclaimed"),
            tr(
                lang,
                "結局：國軍與萬華的街坊並肩，把佔領軍趕出了艋舺。",
                "Ending: the army and Wanhua's neighbors drove the occupation out of Bangka.",
            ),
            match lang {
                Language::ZhTw => {
                    format!("存活 {day} 天　奪回街區 {reclaimed}　擊倒佔領軍 {kills}")
                }
                Language::En => {
                    format!("Survived {day} days. Districts {reclaimed}. Troops defeated {kills}.")
                }
            },
            tr(lang, "再玩一次", "Play again"),
        ),
        Ending::EvacHelicopter => (
            tr(
                lang,
                "您已成功撤離至 福特號航空母艦（USS Gerald R. Ford, CVN-78）",
                "You evacuated to USS Gerald R. Ford (CVN-78)",
            ),
            tr(
                lang,
                "另一個結局：你活了下來，把萬華的故事帶出去。",
                "Another ending: you lived, and carried Wanhua's story out.",
            ),
            tr(
                lang,
                "撤離路線：直升機・河濱撤離區",
                "Route: helicopter, riverside LZ",
            )
            .to_string(),
            tr(lang, "新遊戲", "New game"),
        ),
        Ending::EvacBoat => (
            tr(
                lang,
                "您已成功撤離至 福特號航空母艦（USS Gerald R. Ford, CVN-78）",
                "You evacuated to USS Gerald R. Ford (CVN-78)",
            ),
            tr(
                lang,
                "另一個結局：你活了下來，把萬華的故事帶出去。",
                "Another ending: you lived, and carried Wanhua's story out.",
            ),
            tr(
                lang,
                "撤離路線：小艇・淡水河出海",
                "Route: boat, down the Tamsui",
            )
            .to_string(),
            tr(lang, "新遊戲", "New game"),
        ),
    };
    if heading {
        Some(head.to_string())
    } else if intro {
        Some(sub.to_string())
    } else if detail {
        Some(body)
    } else if restart {
        Some(button.to_string())
    } else {
        None
    }
}

fn labeled(
    marker: impl Component,
    rect: ScreenRect,
    size: f32,
    font: &UiFont,
    color: Color,
) -> impl Bundle {
    (
        marker,
        Node {
            position_type: PositionType::Absolute,
            left: px(rect.min.x),
            top: px(rect.min.y),
            width: px(rect.width()),
            height: px(rect.height()),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Text::new(""),
        TextFont::from_font_size(FontSize::Px(size)).with_font(font.0.clone()),
        TextColor(color),
        TextLayout::justify(Justify::Center),
    )
}

fn choice(marker: impl Component, rect: ScreenRect, size: f32, font: &UiFont) -> impl Bundle {
    choice_bg(
        marker,
        rect,
        size,
        font,
        Color::srgba(0.16, 0.13, 0.1, 0.92),
    )
}

fn choice_bg(
    marker: impl Component,
    rect: ScreenRect,
    size: f32,
    font: &UiFont,
    background: Color,
) -> impl Bundle {
    (
        marker,
        placed(rect),
        BackgroundColor(background),
        BorderColor::all(Color::srgba(0.85, 0.7, 0.4, 0.45)),
        Text::new(""),
        TextFont::from_font_size(FontSize::Px(size)).with_font(font.0.clone()),
        TextColor(Color::srgb(0.98, 0.94, 0.9)),
        TextLayout::justify(Justify::Center),
    )
}

fn placed(rect: ScreenRect) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(rect.min.x),
        top: px(rect.min.y),
        width: px(rect.width()),
        height: px(rect.height()),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border: UiRect::all(px(1.0)),
        border_radius: BorderRadius::all(px(8.0)),
        ..default()
    }
}

fn dimmer() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: px(0.0),
            top: px(0.0),
            width: percent(100.0),
            height: percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.04, 0.03, 0.025)),
        Pickable {
            should_block_lower: true,
            is_hoverable: true,
        },
    )
}

fn viewport_of(windows: &Query<&Window, With<PrimaryWindow>>) -> Vec2 {
    windows
        .iter()
        .next()
        .map(|window| Vec2::new(window.width(), window.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0))
}

fn place(node: &mut Node, rect: ScreenRect) {
    node.left = px(rect.min.x);
    node.top = px(rect.min.y);
    node.width = px(rect.width());
    node.height = px(rect.height());
}
