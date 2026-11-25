//! Survivor select and the death screen.
//!
//! These systems only read [`GameFlow`]. The start button writes [`StartGame`];
//! the death button writes [`RestartGame`]. Neither one sets the next state.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::audio::SfxEvent;
use crate::combat::GameState;
use crate::game_flow::{Campaign, Ending, GameFlow, RestartGame, SkipPrologue, StartGame};
use crate::i18n::{Language, UiFont, tr};
use crate::input::ScreenRect;
use crate::roles::{Gender, ROLES, Role, SelectedRole};

use super::layout::{death_layout, title_layout};
use super::pause::{MenuButton, PauseHeading, ResumeButton};
use super::{ConsumedPresses, StateScoped};

#[derive(Component)]
pub(super) struct TitleRoot;

#[derive(Component)]
pub(super) struct HeadingText;

#[derive(Component)]
pub(super) struct IntroText;

#[derive(Component)]
pub(super) struct DetailText;

#[derive(Component)]
pub(super) struct StartButton;

#[derive(Component)]
pub(super) struct RestartButton;

#[derive(Component)]
pub(super) struct SkipButton;

#[derive(Component)]
pub(super) struct PrologueRoot;

#[derive(Component)]
pub(super) struct EndingRoot;

#[derive(Component, Clone, Copy)]
pub(super) struct GenderButton(pub(super) Gender);

#[derive(Component, Clone, Copy)]
pub(super) struct LangButton(pub(super) Language);

#[derive(Component, Clone, Copy)]
pub(super) struct RoleButton(pub(super) usize);

pub(super) fn enter_title(
    mut commands: Commands,
    windows: Query<&Window, With<PrimaryWindow>>,
    font: Res<UiFont>,
) {
    let viewport = viewport_of(&windows);
    let layout = title_layout(viewport);
    commands
        .spawn((
            TitleRoot,
            StateScoped(GameFlow::Title),
            dimmer(),
            GlobalZIndex(40),
        ))
        .with_children(|parent| {
            parent.spawn(labeled(
                HeadingText,
                layout.heading,
                layout.heading_font,
                &font,
                Color::srgb(0.96, 0.82, 0.38),
            ));
            parent.spawn(labeled(
                IntroText,
                layout.intro,
                layout.body_font,
                &font,
                Color::srgb(0.93, 0.9, 0.82),
            ));
            parent.spawn(choice(
                GenderButton(Gender::Male),
                layout.male,
                layout.button_font,
                &font,
            ));
            parent.spawn(choice(
                GenderButton(Gender::Female),
                layout.female,
                layout.button_font,
                &font,
            ));
            parent.spawn(choice(
                LangButton(Language::ZhTw),
                layout.lang_zh,
                layout.body_font,
                &font,
            ));
            parent.spawn(choice(
                LangButton(Language::En),
                layout.lang_en,
                layout.body_font,
                &font,
            ));
            for (index, rect) in layout.roles.iter().enumerate() {
                parent.spawn(choice(RoleButton(index), *rect, layout.body_font, &font));
            }
            parent.spawn(labeled(
                DetailText,
                layout.detail,
                layout.body_font,
                &font,
                Color::srgb(0.95, 0.93, 0.86),
            ));
            parent.spawn(choice(
                StartButton,
                layout.start,
                layout.button_font + 2.0,
                &font,
            ));
        });
}

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
            Option<&GenderButton>,
            Option<&LangButton>,
            Option<&RoleButton>,
            Option<&StartButton>,
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
    let title = matches!(flow.get(), GameFlow::Title).then(|| title_layout(viewport));
    let overlay = matches!(flow.get(), GameFlow::Prologue | GameFlow::GameOver(_))
        .then(|| death_layout(viewport));
    for (mut node, mut font, heading, intro, detail, gender, lang, role, start, restart, skip) in
        &mut nodes
    {
        let placed = if heading.is_some() {
            title
                .as_ref()
                .map(|layout| (layout.heading, layout.heading_font))
                .or_else(|| {
                    overlay
                        .as_ref()
                        .map(|layout| (layout.heading, layout.heading_font))
                })
        } else if intro.is_some() {
            title
                .as_ref()
                .map(|layout| (layout.intro, layout.body_font))
                .or_else(|| {
                    overlay
                        .as_ref()
                        .map(|layout| (layout.sub, layout.body_font))
                })
        } else if detail.is_some() {
            title
                .as_ref()
                .map(|layout| (layout.detail, layout.body_font))
                .or_else(|| {
                    overlay
                        .as_ref()
                        .map(|layout| (layout.body, layout.body_font))
                })
        } else if let Some(gender) = gender {
            title.as_ref().map(|layout| {
                (
                    match gender.0 {
                        Gender::Male => layout.male,
                        Gender::Female => layout.female,
                    },
                    layout.button_font,
                )
            })
        } else if let Some(lang) = lang {
            title.as_ref().map(|layout| {
                (
                    match lang.0 {
                        Language::ZhTw => layout.lang_zh,
                        Language::En => layout.lang_en,
                    },
                    layout.body_font,
                )
            })
        } else if let Some(role) = role {
            title
                .as_ref()
                .map(|layout| (layout.roles[role.0], layout.body_font))
        } else if start.is_some() {
            title
                .as_ref()
                .map(|layout| (layout.start, layout.button_font + 2.0))
        } else if restart.is_some() || skip.is_some() {
            overlay
                .as_ref()
                .map(|layout| (layout.restart, layout.body_font + 2.0))
        } else {
            None
        };
        if let Some((rect, size)) = placed {
            place(&mut node, rect);
            font.font_size = FontSize::Px(size);
        }
    }
}

pub(super) fn refresh_copy(
    flow: Res<State<GameFlow>>,
    lang: Res<Language>,
    selected: Res<SelectedRole>,
    game: Res<GameState>,
    campaign: Res<Campaign>,
    mut texts: Query<
        (
            &mut Text,
            Option<&HeadingText>,
            Option<&IntroText>,
            Option<&DetailText>,
            Option<&GenderButton>,
            Option<&LangButton>,
            Option<&RoleButton>,
            Option<&StartButton>,
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
    let state = flow.get().clone();
    let day = (game.survived / 1440.0).floor() as i32 + 1;
    for (mut text, heading, intro, detail, gender, language, role, start, restart, skip) in
        &mut texts
    {
        let next = match &state {
            GameFlow::Title if heading.is_some() => Some(tr(*lang, "保衛台北", "Defend Taipei").to_string()),
            GameFlow::Title if intro.is_some() => Some(
                tr(
                    *lang,
                    "解放軍登陸三天後，台北淪陷。萬華的燈還亮著、水還有。\n你是躲在巷弄裡的倖存者。死亡即永久結束。",
                    "Three days after the landings, Taipei has fallen. Wanhua still has lights and water.\nYou are a survivor in its alleys. Death is permanent.",
                )
                .to_string(),
            ),
            GameFlow::Title if detail.is_some() => {
                Some(detail_line(selected.role(), selected.gender, *lang))
            }
            GameFlow::Title if let Some(gender) = gender => Some(gender.0.label(*lang).to_string()),
            GameFlow::Title if let Some(language) = language => Some(
                match language.0 {
                    Language::ZhTw => "繁體中文",
                    Language::En => "English",
                }
                .to_string(),
            ),
            GameFlow::Title if let Some(role) = role => {
                Some(ROLES[role.0].card_line(selected.gender, *lang))
            }
            GameFlow::Title if start.is_some() => {
                Some(tr(*lang, "新遊戲", "New game").to_string())
            }
            GameFlow::Prologue if heading.is_some() => {
                Some(tr(*lang, "07:20　鬧鐘響了", "07:20  The alarm goes off").to_string())
            }
            GameFlow::Prologue if intro.is_some() => {
                Some(tr(*lang, "今天也要去西門町", "Another morning heading to Ximending").to_string())
            }
            GameFlow::Prologue if detail.is_some() => Some(
                tr(
                    *lang,
                    "跟著路上的人流。",
                    "Follow the people on the street.",
                )
                .to_string(),
            ),
            GameFlow::Prologue if skip.is_some() => {
                Some(tr(*lang, "跳過開場 ▶", "Skip intro ▶").to_string())
            }
            GameFlow::GameOver(ending) => {
                ending_line(*ending, *lang, day, game.kills, campaign.reclaimed.len(), heading.is_some(), intro.is_some(), detail.is_some(), restart.is_some())
            }
            _ => None,
        };
        if let Some(next) = next
            && text.0 != next
        {
            text.0 = next;
        }
    }
}

pub(super) fn title_clicks(
    flow: Res<State<GameFlow>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    consumed: Res<ConsumedPresses>,
    mut lang: ResMut<Language>,
    mut selected: ResMut<SelectedRole>,
    mut start_event: MessageWriter<StartGame>,
    mut sfx: MessageWriter<SfxEvent>,
    mut roles: Query<(&RoleButton, &mut BackgroundColor)>,
    mut genders: Query<(&GenderButton, &mut BackgroundColor), Without<RoleButton>>,
) {
    if !matches!(flow.get(), GameFlow::Title) {
        return;
    }
    let Some(window) = windows.iter().next() else {
        return;
    };
    let layout = title_layout(Vec2::new(window.width(), window.height()));
    let mut pressed = false;
    for point in &consumed.points {
        if layout.male.contains(*point) {
            selected.gender = Gender::Male;
            pressed = true;
        } else if layout.female.contains(*point) {
            selected.gender = Gender::Female;
            pressed = true;
        } else if layout.lang_zh.contains(*point) {
            *lang = Language::ZhTw;
            pressed = true;
        } else if layout.lang_en.contains(*point) {
            *lang = Language::En;
            pressed = true;
        } else if let Some(index) = layout.roles.iter().position(|rect| rect.contains(*point)) {
            selected.id = ROLES[index].id;
            pressed = true;
        } else if layout.start.contains(*point) {
            start_event.write(StartGame);
            pressed = true;
        }
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::Space]) {
        start_event.write(StartGame);
        pressed = true;
    }
    if pressed {
        sfx.write(SfxEvent::Clink { dist: 0.0 });
    }
    for (button, mut color) in &mut roles {
        color.0 = if ROLES[button.0].id == selected.id {
            Color::srgb(0.45, 0.32, 0.12)
        } else {
            Color::srgba(0.16, 0.13, 0.1, 0.92)
        };
    }
    for (button, mut color) in &mut genders {
        color.0 = if button.0 == selected.gender {
            Color::srgb(0.55, 0.4, 0.16)
        } else {
            Color::srgba(0.2, 0.17, 0.13, 0.92)
        };
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
    let skip_rect = death_layout(viewport_of(&windows)).restart;
    let on_button = consumed
        .points
        .iter()
        .any(|point| skip_rect.contains(*point));
    if on_button || keys.any_just_pressed([KeyCode::Enter, KeyCode::Space, KeyCode::Escape]) {
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
    let layout = death_layout(viewport);
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

fn detail_line(role: &Role, gender: Gender, lang: Language) -> String {
    let age = match lang {
        Language::ZhTw => format!("{} 歲", role.age_of(gender)),
        Language::En => format!("{} years old", role.age_of(gender)),
    };
    format!(
        "{}・{}　{}　{}\n{}\n{}",
        role.given(gender, lang),
        role.name(lang),
        gender.label(lang),
        age,
        role.blurb(lang),
        role.perks(lang).join("・")
    )
}
