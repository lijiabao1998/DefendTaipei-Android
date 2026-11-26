//! Native title and departure flow, following the Web game's separate menu,
//! survivor cards and final departure review. Only Depart creates a new run.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::audio::SfxEvent;
use crate::game_flow::{GameFlow, StartGame};
use crate::i18n::{Language, UiFont, tr};
use crate::input::ScreenRect;
use crate::inventory::item;
use crate::roles::{Gender, ROLES, Role, SelectedRole};
use crate::save::{ContinueGame, SaveStatus};

use super::layout::{brief_layout, home_layout, title_layout};
use super::{ConsumedPresses, StateScoped, portrait};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TitlePage {
    #[default]
    Home,
    Survivor,
    Briefing,
    Controls,
    About,
}

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TitleMenu {
    pub page: TitlePage,
    pub help_page: usize,
}

/// Semantic actions are shared by pointer input, keyboard input and native QA.
#[derive(Message, Clone, Copy, Debug)]
pub(crate) enum TitleAction {
    NewGame,
    Settings,
    Role(usize),
    Gender(Gender),
    Language(Language),
    Next,
    Back,
    Continue,
    Depart,
    Help,
    About,
    HelpNext,
    HelpPrevious,
}

#[derive(Component)]
pub(super) struct TitleRoot;

#[derive(Resource, Default)]
pub(super) struct TitleAssets {
    portraits: Vec<(usize, Gender, Handle<Image>)>,
    plan: Option<Handle<Image>>,
}

#[derive(Clone, Debug, PartialEq)]
struct RenderKey {
    viewport: Vec2,
    menu: TitleMenu,
    selected: SelectedRole,
    language: Language,
    available: bool,
    notice: String,
    error: Option<String>,
    day_hours: u8,
}

#[derive(Resource, Default)]
pub(super) struct TitleCache(Option<RenderKey>);

const INK: Color = Color::srgb(0.075, 0.08, 0.075);
const PANEL: Color = Color::srgb(0.105, 0.108, 0.097);
const PAPER: Color = Color::srgb(0.94, 0.91, 0.82);
const MUTED: Color = Color::srgb(0.66, 0.66, 0.6);
const GOLD: Color = Color::srgb(0.82, 0.68, 0.40);
const LINE: Color = Color::srgb(0.34, 0.31, 0.24);

pub(super) fn reset(mut menu: ResMut<TitleMenu>, mut cache: ResMut<TitleCache>) {
    *menu = TitleMenu::default();
    cache.0 = None;
}

pub(super) fn pointer_actions(
    flow: Res<State<GameFlow>>,
    menu: Res<TitleMenu>,
    windows: Query<&Window, With<PrimaryWindow>>,
    consumed: Res<ConsumedPresses>,
    keys: Res<ButtonInput<KeyCode>>,
    mut actions: MessageWriter<TitleAction>,
    settings: Option<Res<super::settings::SettingsUi>>,
) {
    if flow.get() != &GameFlow::Title || settings.as_ref().is_some_and(|s| s.open || s.captured) {
        return;
    }
    let viewport = viewport(&windows);
    for point in &consumed.points {
        if let Some(action) = hit_action(*menu, viewport, *point) {
            actions.write(action);
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        actions.write(TitleAction::Back);
    } else if keys.any_just_pressed([KeyCode::Enter, KeyCode::Space]) {
        actions.write(match menu.page {
            TitlePage::Home => TitleAction::NewGame,
            TitlePage::Survivor => TitleAction::Next,
            TitlePage::Briefing => TitleAction::Depart,
            TitlePage::Controls => TitleAction::HelpNext,
            TitlePage::About => TitleAction::Back,
        });
    }
}

fn hit_action(menu: TitleMenu, viewport: Vec2, point: Vec2) -> Option<TitleAction> {
    if menu.page == TitlePage::Home {
        let layout = home_layout(viewport);
        return [
            (layout.new_game, TitleAction::NewGame),
            (layout.continue_game, TitleAction::Continue),
            (layout.help, TitleAction::Help),
            (layout.settings, TitleAction::Settings),
            (layout.about, TitleAction::About),
            (layout.lang_zh, TitleAction::Language(Language::ZhTw)),
            (layout.lang_en, TitleAction::Language(Language::En)),
        ]
        .into_iter()
        .find_map(|(rect, action)| rect.contains(point).then_some(action));
    }
    let layout = title_layout(viewport);
    if layout.lang_zh.contains(point) {
        return Some(TitleAction::Language(Language::ZhTw));
    }
    if layout.lang_en.contains(point) {
        return Some(TitleAction::Language(Language::En));
    }
    if layout.back.contains(point) {
        return Some(if menu.page == TitlePage::Controls && menu.help_page > 0 {
            TitleAction::HelpPrevious
        } else {
            TitleAction::Back
        });
    }
    if layout.start.contains(point) {
        return Some(match menu.page {
            TitlePage::Survivor => TitleAction::Next,
            TitlePage::Briefing => TitleAction::Depart,
            TitlePage::Controls => TitleAction::HelpNext,
            _ => TitleAction::Back,
        });
    }
    if menu.page == TitlePage::Survivor {
        if layout.male.contains(point) {
            return Some(TitleAction::Gender(Gender::Male));
        }
        if layout.female.contains(point) {
            return Some(TitleAction::Gender(Gender::Female));
        }
        return layout
            .roles
            .iter()
            .position(|r| r.contains(point))
            .map(TitleAction::Role);
    }
    None
}

pub(super) fn apply_actions(
    flow: Res<State<GameFlow>>,
    mut events: MessageReader<TitleAction>,
    mut menu: ResMut<TitleMenu>,
    mut selected: ResMut<SelectedRole>,
    mut language: ResMut<Language>,
    save: Res<SaveStatus>,
    mut start: MessageWriter<StartGame>,
    mut load: MessageWriter<ContinueGame>,
    mut sfx: MessageWriter<SfxEvent>,
    mut settings: Option<ResMut<super::settings::SettingsUi>>,
) {
    let mut departing = false;
    for action in events.read() {
        if flow.get() != &GameFlow::Title
            || departing
            || settings.as_ref().is_some_and(|s| s.open || s.captured)
        {
            continue;
        }
        let applied = match *action {
            TitleAction::Settings if menu.page == TitlePage::Home => {
                if let Some(settings) = settings.as_deref_mut() {
                    settings.open = true;
                    settings.captured = true;
                    println!("DTUI action=SettingsOpen");
                }
                true
            }
            TitleAction::Language(value) => {
                *language = value;
                true
            }
            TitleAction::NewGame if menu.page == TitlePage::Home => {
                menu.page = TitlePage::Survivor;
                println!("DTUI action=ChooseSurvivor");
                true
            }
            TitleAction::Role(index) if menu.page == TitlePage::Survivor && index < ROLES.len() => {
                selected.id = ROLES[index].id;
                println!("DTUI action=SelectRole role={}", selected.id);
                true
            }
            TitleAction::Gender(value) if menu.page == TitlePage::Survivor => {
                selected.gender = value;
                true
            }
            TitleAction::Next if menu.page == TitlePage::Survivor => {
                menu.page = TitlePage::Briefing;
                println!("DTUI action=ReviewDeparture");
                true
            }
            TitleAction::Depart if menu.page == TitlePage::Briefing => {
                println!("DTUI action=NewGame");
                start.write(StartGame);
                departing = true;
                true
            }
            TitleAction::Continue if menu.page == TitlePage::Home && save.available => {
                println!("DTUI action=Continue");
                load.write(ContinueGame);
                departing = true;
                true
            }
            TitleAction::Help if menu.page == TitlePage::Home => {
                menu.page = TitlePage::Controls;
                menu.help_page = 0;
                true
            }
            TitleAction::About if menu.page == TitlePage::Home => {
                menu.page = TitlePage::About;
                true
            }
            TitleAction::HelpNext if menu.page == TitlePage::Controls => {
                if menu.help_page < 2 {
                    menu.help_page += 1;
                } else {
                    menu.page = TitlePage::Home;
                }
                true
            }
            TitleAction::HelpPrevious if menu.page == TitlePage::Controls => {
                menu.help_page = menu.help_page.saturating_sub(1);
                true
            }
            TitleAction::Back => {
                menu.page = if menu.page == TitlePage::Briefing {
                    TitlePage::Survivor
                } else {
                    TitlePage::Home
                };
                true
            }
            _ => false,
        };
        if applied {
            sfx.write(SfxEvent::Ui { confirm: false });
        }
    }
}

pub(super) fn render(
    mut commands: Commands,
    flow: Res<State<GameFlow>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    menu: Res<TitleMenu>,
    selected: Res<SelectedRole>,
    language: Res<Language>,
    save: Res<SaveStatus>,
    font: Res<UiFont>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut assets: ResMut<TitleAssets>,
    mut cache: ResMut<TitleCache>,
    preferences: Option<Res<crate::settings::GamePreferences>>,
    roots: Query<Entity, With<TitleRoot>>,
) {
    if flow.get() != &GameFlow::Title {
        return;
    }
    let key = RenderKey {
        viewport: viewport(&windows),
        menu: *menu,
        selected: *selected,
        language: *language,
        available: save.available,
        notice: save.notice.clone(),
        error: save.error.clone(),
        day_hours: preferences.as_ref().map_or(1, |p| p.day_hours),
    };
    if cache.0.as_ref() == Some(&key) && !roots.is_empty() {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    if let Some(images) = images.as_deref_mut() {
        if assets.plan.is_none() {
            assets.plan = Some(images.add(city_plan()));
        }
        if matches!(menu.page, TitlePage::Survivor | TitlePage::Briefing) {
            for (index, role) in ROLES.iter().enumerate() {
                if !assets
                    .portraits
                    .iter()
                    .any(|(i, g, _)| *i == index && *g == selected.gender)
                {
                    assets.portraits.push((
                        index,
                        selected.gender,
                        images.add(portrait::render(role, selected.gender)),
                    ));
                }
            }
        }
    }
    commands
        .spawn((
            TitleRoot,
            StateScoped(GameFlow::Title),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                ..default()
            },
            BackgroundColor(INK),
            GlobalZIndex(40),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
        ))
        .with_children(|root| {
            if let Some(plan) = &assets.plan {
                root.spawn((
                    node(ScreenRect::new(0.0, 0.0, key.viewport.x, key.viewport.y)),
                    ImageNode::new(plan.clone()).with_mode(NodeImageMode::Stretch),
                ));
            }
            if menu.page == TitlePage::Home {
                render_home(root, &key, &font);
            } else {
                render_header(root, &key, &font);
                match menu.page {
                    TitlePage::Survivor => render_survivors(root, &key, &assets, &font),
                    TitlePage::Briefing => render_briefing(root, &key, &assets, &font),
                    TitlePage::Controls | TitlePage::About => render_information(root, &key, &font),
                    TitlePage::Home => {}
                }
            }
        });
    cache.0 = Some(key);
}

fn render_home(root: &mut ChildSpawnerCommands, key: &RenderKey, font: &UiFont) {
    let l = home_layout(key.viewport);
    let lang = key.language;
    text(root, l.eyebrow, "TAIPEI / WANHUA", 10.0, GOLD, font);
    languages(root, l.lang_zh, l.lang_en, lang, font);
    text(
        root,
        l.heading,
        tr(lang, "保衛台北", "DEFEND TAIPEI"),
        if lang == Language::En {
            l.title_font * 0.64
        } else {
            l.title_font
        },
        PAPER,
        font,
    );
    text(
        root,
        l.subtitle,
        tr(lang, "萬華淪陷區 · 城市生存", "WANHUA UNDER SIEGE"),
        13.0,
        GOLD,
        font,
    );
    line(
        root,
        ScreenRect::new(l.subtitle.min.x, l.subtitle.max.y + 8.0, 56.0, 2.0),
        GOLD,
    );
    text(
        root,
        l.intro,
        tr(
            lang,
            "登陸三天後，台北淪陷。\n燈還亮著，水還有。\n你要帶著什麼，撐過下一個早晨？",
            "Three days after the landings, Taipei fell.\nThe lights are still on. Water still runs.\nWhat will get you through tomorrow?",
        ),
        l.body_font,
        PAPER,
        font,
    );
    text(
        root,
        l.facts,
        tr(
            lang,
            "搜刮物資 / 守住避難所 / 活著離開",
            "SCAVENGE / SHELTER / SURVIVE",
        ),
        if key.viewport.y < 700.0 { 10.0 } else { 12.0 },
        MUTED,
        font,
    );
    button(
        root,
        l.continue_game,
        tr(lang, "繼續遊戲", "Continue"),
        if key.available {
            ButtonStyle::Continue
        } else {
            ButtonStyle::Disabled
        },
        font,
    );
    button(
        root,
        l.new_game,
        tr(lang, "新遊戲  →", "New game  →"),
        ButtonStyle::Primary,
        font,
    );
    button(
        root,
        l.help,
        tr(lang, "操作說明", "Controls"),
        ButtonStyle::Quiet,
        font,
    );
    button(
        root,
        l.about,
        tr(lang, "製作名單", "About"),
        ButtonStyle::Quiet,
        font,
    );
    button(
        root,
        l.settings,
        tr(lang, "設定", "Settings"),
        ButtonStyle::Quiet,
        font,
    );
    let notice = key.error.as_deref().unwrap_or_else(|| {
        if !key.notice.is_empty() {
            &key.notice
        } else if key.available {
            tr(
                lang,
                "已有存檔，可接續上一段旅程。",
                "A saved journey is ready to continue.",
            )
        } else {
            tr(
                lang,
                "尚無存檔。從一位倖存者開始。",
                "No saved journey. Choose a survivor to begin.",
            )
        }
    });
    text(root, l.notice, notice, 11.0, MUTED, font);
    text(
        root,
        l.footer,
        "DEFEND TAIPEI   /   WANHUA",
        9.0,
        MUTED,
        font,
    );
}

fn render_header(root: &mut ChildSpawnerCommands, key: &RenderKey, font: &UiFont) {
    let l = title_layout(key.viewport);
    let lang = key.language;
    text(
        root,
        ScreenRect::new(l.frame.min.x, l.frame.min.y + 5.0, 150.0, 22.0),
        tr(lang, "保衛台北 / 萬華", "DEFEND TAIPEI"),
        11.0,
        GOLD,
        font,
    );
    languages(root, l.lang_zh, l.lang_en, lang, font);
    let heading = match key.menu.page {
        TitlePage::Survivor => tr(lang, "選擇你的倖存者", "Choose your survivor"),
        TitlePage::Briefing => tr(lang, "出發前確認", "Before you depart"),
        TitlePage::Controls => tr(lang, "在萬華生存", "Surviving Wanhua"),
        _ => tr(lang, "關於保衛台北", "About Defend Taipei"),
    };
    text(root, l.heading, heading, l.heading_font, PAPER, font);
    let stages = match key.menu.page {
        TitlePage::Survivor => tr(
            lang,
            "01  選擇角色     /     02  出發資訊",
            "01  SURVIVOR     /     02  DEPARTURE",
        ),
        TitlePage::Briefing => tr(
            lang,
            "01  選擇角色     /     02  出發資訊",
            "01  SURVIVOR     /     02  DEPARTURE",
        ),
        TitlePage::Controls => match key.menu.help_page {
            0 => tr(
                lang,
                "01 / 03    移動與戰鬥",
                "01 / 03    MOVEMENT & COMBAT",
            ),
            1 => tr(
                lang,
                "02 / 03    搜刮與生存",
                "02 / 03    SCAVENGING & SURVIVAL",
            ),
            _ => tr(
                lang,
                "03 / 03    儲存與續玩",
                "03 / 03    SAVING YOUR JOURNEY",
            ),
        },
        _ => "DEFEND TAIPEI / NATIVE EDITION",
    };
    text(root, l.steps, stages, 11.0, GOLD, font);
    line(
        root,
        ScreenRect::new(l.steps.min.x, l.steps.max.y + 5.0, l.steps.width(), 1.0),
        LINE,
    );
}

fn render_survivors(
    root: &mut ChildSpawnerCommands,
    key: &RenderKey,
    assets: &TitleAssets,
    font: &UiFont,
) {
    let l = title_layout(key.viewport);
    let lang = key.language;
    button(
        root,
        l.male,
        Gender::Male.label(lang),
        if key.selected.gender == Gender::Male {
            ButtonStyle::Selected
        } else {
            ButtonStyle::Quiet
        },
        font,
    );
    button(
        root,
        l.female,
        Gender::Female.label(lang),
        if key.selected.gender == Gender::Female {
            ButtonStyle::Selected
        } else {
            ButtonStyle::Quiet
        },
        font,
    );
    for (index, rect) in l.roles.iter().copied().enumerate() {
        let role = &ROLES[index];
        let selected = key.selected.id == role.id;
        panel(
            root,
            rect,
            if selected {
                Color::srgb(0.24, 0.21, 0.14)
            } else {
                PANEL
            },
            if selected { GOLD } else { LINE },
        );
        let tall = rect.height() >= 96.0;
        let thumb_h = if tall {
            (rect.height() - 45.0).min(140.0)
        } else {
            (rect.height() - 10.0).min(50.0)
        };
        let thumb_w = thumb_h * 160.0 / 208.0;
        let thumb = ScreenRect::new(
            if tall {
                rect.center().x - thumb_w * 0.5
            } else {
                rect.min.x + 3.0
            },
            rect.min.y + 3.0,
            thumb_w,
            thumb_h,
        );
        portrait_node(root, assets, index, key.selected.gender, thumb);
        if tall {
            text_center(
                root,
                ScreenRect::new(
                    rect.min.x + 5.0,
                    rect.max.y - 41.0,
                    rect.width() - 10.0,
                    20.0,
                ),
                role.given(key.selected.gender, lang),
                14.0,
                PAPER,
                font,
            );
            text_center(
                root,
                ScreenRect::new(
                    rect.min.x + 4.0,
                    rect.max.y - 20.0,
                    rect.width() - 8.0,
                    17.0,
                ),
                role_short(role, lang),
                11.0,
                if selected { GOLD } else { MUTED },
                font,
            );
        } else {
            let tx = thumb.max.x + 4.0;
            let tw = rect.max.x - tx - 4.0;
            text(
                root,
                ScreenRect::new(tx, rect.min.y + 6.0, tw, rect.height() - 11.0),
                &format!(
                    "{}\n{}",
                    role.given(key.selected.gender, lang),
                    role_short(role, lang)
                ),
                if tw < 65.0 { 10.0 } else { 11.0 },
                PAPER,
                font,
            );
        }
        if selected {
            line(
                root,
                ScreenRect::new(rect.min.x + 1.0, rect.min.y + 1.0, 3.0, rect.height() - 2.0),
                GOLD,
            );
        }
    }
    profile(root, l.detail, key, assets, font);
    button(
        root,
        l.back,
        tr(lang, "← 返回", "← Back"),
        ButtonStyle::Quiet,
        font,
    );
    button(
        root,
        l.start,
        tr(lang, "下一步  →", "Next  →"),
        ButtonStyle::Primary,
        font,
    );
    text(
        root,
        l.save_status,
        tr(
            lang,
            "出發前可以返回，現有存檔會保留。",
            "Your current save is kept until you depart.",
        ),
        10.0,
        MUTED,
        font,
    );
}

fn profile(
    root: &mut ChildSpawnerCommands,
    rect: ScreenRect,
    key: &RenderKey,
    assets: &TitleAssets,
    font: &UiFont,
) {
    let role = key.selected.role();
    let lang = key.language;
    panel(root, rect, INK, LINE);
    let x = rect.min.x + 12.0;
    let width = rect.width() - 24.0;
    let mut y = rect.min.y + 8.0;
    if rect.height() > 280.0 {
        text(
            root,
            ScreenRect::new(x, y, width, 16.0),
            tr(lang, "角色檔案", "SURVIVOR PROFILE"),
            10.0,
            GOLD,
            font,
        );
        y += 22.0;
        let index = ROLES.iter().position(|r| r.id == role.id).unwrap_or(0);
        portrait_node(
            root,
            assets,
            index,
            key.selected.gender,
            ScreenRect::new(rect.center().x - 55.0, y, 110.0, 143.0),
        );
        y += 148.0;
        text(
            root,
            ScreenRect::new(x, y, width, 56.0),
            native_description(role, lang),
            12.0,
            MUTED,
            font,
        );
        y += 62.0;
    }
    text(
        root,
        ScreenRect::new(x, y, width, 24.0),
        &identity(role, key.selected.gender, lang),
        if width < 220.0 { 12.0 } else { 14.0 },
        PAPER,
        font,
    );
    y += 27.0;
    let trait_h = if width < 220.0 { 31.0 } else { 20.0 };
    text(
        root,
        ScreenRect::new(x, y, width, trait_h),
        survivor_trait(role, lang),
        11.0,
        GOLD,
        font,
    );
    y += trait_h + 4.0;
    if (130.0..=280.0).contains(&rect.height()) && width >= 250.0 {
        text(
            root,
            ScreenRect::new(x, y, width, 32.0),
            native_description(role, lang),
            11.0,
            MUTED,
            font,
        );
        y += 35.0;
    }
    text(
        root,
        ScreenRect::new(x, y, width, (rect.max.y - y - 6.0).max(16.0)),
        &format!(
            "{} {}",
            tr(lang, "物資：", "KIT:"),
            supplies(role, lang, " · ")
        ),
        10.0,
        MUTED,
        font,
    );
}

/// Native introductions describe the kit and mechanics available in this edition.
/// The original biographies remain intact in `roles` for the future story port.
fn native_description(role: &Role, lang: Language) -> &'static str {
    match role.id {
        "student" => tr(
            lang,
            "帶著零食與運動飲料出發，休息時恢復較快。",
            "Snacks and a sports drink for the road. Rest restores your energy faster.",
        ),
        "courier" => tr(
            lang,
            "輕裝出發，包裡有飲水、泡麵和一把刀。",
            "Travel light with water, instant noodles and a knife in your pack.",
        ),
        "nurse" => tr(
            lang,
            "帶著繃帶、止痛藥和急救包，治療時更有效。",
            "Bandages, painkillers and a first aid kit. Your treatments are more effective.",
        ),
        "tech" => tr(
            lang,
            "帶著鐵鎚、鐵管和木板，蒐集避難所需要的材料。",
            "A hammer, pipe and timber to start gathering what your shelter needs.",
        ),
        "jp" => tr(
            lang,
            "多備一顆電池，帶著乾糧和飲水踏上旅程。",
            "A spare battery, dry food and drinking water for the journey ahead.",
        ),
        _ => tr(
            lang,
            "帶著咖啡、飲水與食物，開始在萬華求生。",
            "Coffee, drinking water and food for your first steps through Wanhua.",
        ),
    }
}

/// Only traits already implemented by the native game are advertised here.
fn survivor_trait(role: &Role, lang: Language) -> &'static str {
    match role.id {
        "student" => tr(
            lang,
            "休息恢復較快 · 背包稍大",
            "Faster rest recovery · Larger pack",
        ),
        "courier" => tr(
            lang,
            "輕裝背包 · 開局配刀",
            "Light pack · Starts with a knife",
        ),
        "nurse" => tr(
            lang,
            "醫療效果提升 · 急救物資",
            "Better treatment · Medical supplies",
        ),
        "tech" => tr(
            lang,
            "工具齊備 · 自帶修繕材料",
            "Tools and building materials",
        ),
        "jp" => tr(
            lang,
            "備用電池 · 乾糧與飲水",
            "Spare battery · Food and water",
        ),
        _ => tr(
            lang,
            "標準背包 · 均衡補給",
            "Standard pack · Balanced supplies",
        ),
    }
}

fn render_briefing(
    root: &mut ChildSpawnerCommands,
    key: &RenderKey,
    assets: &TitleAssets,
    font: &UiFont,
) {
    let title = title_layout(key.viewport);
    let l = brief_layout(key.viewport);
    let lang = key.language;
    let role = key.selected.role();
    let index = ROLES.iter().position(|r| r.id == role.id).unwrap_or(0);
    panel(root, l.identity, PANEL, LINE);
    let portrait_h = (l.identity.height() - 18.0).min(190.0);
    let portrait_w = portrait_h * 160.0 / 208.0;
    portrait_node(
        root,
        assets,
        index,
        key.selected.gender,
        ScreenRect::new(
            l.identity.min.x + 5.0,
            l.identity.min.y + 10.0,
            portrait_w,
            portrait_h,
        ),
    );
    let x = l.identity.min.x + portrait_w + 14.0;
    let width = l.identity.max.x - x - 12.0;
    text(
        root,
        ScreenRect::new(x, l.identity.min.y + 14.0, width, 16.0),
        tr(lang, "你的倖存者", "YOUR SURVIVOR"),
        10.0,
        GOLD,
        font,
    );
    text(
        root,
        ScreenRect::new(x, l.identity.min.y + 39.0, width, 58.0),
        &format!(
            "{}\n{}",
            role.given(key.selected.gender, lang),
            role_short(role, lang)
        ),
        if title.compact { 17.0 } else { 22.0 },
        PAPER,
        font,
    );
    if l.identity.height() >= 150.0 {
        text(
            root,
            ScreenRect::new(
                x,
                l.identity.min.y + 100.0,
                width,
                l.identity.height() - 112.0,
            ),
            &format!(
                "{}\n{}",
                identity(role, key.selected.gender, lang),
                tr(lang, "萬華 · 城市生存", "Wanhua · Survival")
            ),
            12.0,
            MUTED,
            font,
        );
    }
    panel(root, l.supplies, INK, LINE);
    let r = l.supplies;
    let x = r.min.x + 14.0;
    let width = r.width() - 28.0;
    let short = r.height() < 205.0;
    text(
        root,
        ScreenRect::new(x, r.min.y + 12.0, width, 18.0),
        tr(lang, "出發資訊", "DEPARTURE BRIEF"),
        11.0,
        GOLD,
        font,
    );
    text(
        root,
        ScreenRect::new(x, r.min.y + 38.0, width, if short { 40.0 } else { 56.0 }),
        &match lang {
            Language::ZhTw => format!(
                "一天 = 現實 {} 小時（1×）\n開場後，帶著物資回到避難所。",
                key.day_hours
            ),
            Language::En => format!(
                "One day = {} real hour(s) at 1×.\nBring supplies back to your shelter.",
                key.day_hours
            ),
        },
        if short { 11.0 } else { 14.0 },
        PAPER,
        font,
    );
    let kit_y = r.min.y + if short { 82.0 } else { 105.0 };
    text(
        root,
        ScreenRect::new(x, kit_y, width, r.max.y - kit_y - 12.0),
        &format!(
            "{}\n{}",
            tr(lang, "初始背包", "STARTING KIT"),
            supplies(role, lang, if short { " · " } else { "\n" })
        ),
        if short { 11.0 } else { 13.0 },
        GOLD,
        font,
    );
    text(
        root,
        l.notice,
        if key.available {
            tr(
                lang,
                "出發將取代目前存檔；死亡會結束本局。",
                "Departing replaces your save. Death ends this run.",
            )
        } else {
            tr(
                lang,
                "離開遊戲可儲存；死亡會結束本局。",
                "You can save and leave. Death ends this run.",
            )
        },
        10.0,
        MUTED,
        font,
    );
    button(
        root,
        title.back,
        tr(lang, "← 上一步", "← Back"),
        ButtonStyle::Quiet,
        font,
    );
    button(
        root,
        title.start,
        tr(lang, "走進萬華  ▶", "Depart  ▶"),
        ButtonStyle::Primary,
        font,
    );
}

fn render_information(root: &mut ChildSpawnerCommands, key: &RenderKey, font: &UiFont) {
    let l = title_layout(key.viewport);
    let lang = key.language;
    let rect = ScreenRect::new(
        l.frame.min.x,
        l.steps.max.y + 16.0,
        l.frame.width(),
        l.save_status.min.y - l.steps.max.y - 28.0,
    );
    panel(root, rect, INK, LINE);
    let content = if key.menu.page == TitlePage::About {
        tr(
            lang,
            "保衛台北 / Defend Taipei\n\n一位普通人，在萬華活下去的故事。\n原作：DefendTaipei-Lab\n原生版本：Rust + Bevy\n\n地圖資料 © OpenStreetMap contributors（ODbL）\n字型：Noto Sans CJK TC（SIL OFL）\n錄音：Free Firearm Sound Library、rubberduck（CC0）\n\n本版可探索萬華核心街區，包含避難所、\n物資生存與存檔續玩。",
            "DEFEND TAIPEI\n\nAn ordinary person's survival in Wanhua.\nOriginal: DefendTaipei-Lab\nNative edition: Rust + Bevy\n\nMap © OpenStreetMap contributors (ODbL)\nFont: Noto Sans CJK TC (SIL OFL)\nRecordings: Free Firearm Sound Library / rubberduck (CC0)\n\nThis edition covers the Wanhua core, shelter,\nsupplies, survival and saved journeys.",
        )
    } else {
        match key.menu.help_page {
            0 => tr(
                lang,
                "移動\n拖曳左側搖桿；推到底可奔跑。\n\n戰鬥\n按住「打」攻擊；槍械需要對應彈藥。\n點「蹲」降低姿態，善用牆角與掩蔽物。\n\n視角\n雙指縮放；可隨時旋轉手機。\n鍵盤：WASD 移動，滑鼠左鍵攻擊。",
                "MOVE\nDrag the left pad. Full deflection runs.\n\nFIGHT\nHold Attack. Guns need matching ammunition.\nTap Crouch and use corners and cover.\n\nCAMERA\nPinch to zoom; rotate your phone freely.\nWASD to move; left mouse to attack.\nM / Map: explore, pan, zoom and mark supplies.",
            ),
            1 => tr(
                lang,
                "搜刮\n接近物資箱、倒下的敵人或門，點「互動」。\n\n照顧自己\n從「背包」裝備、飲食、包紮或睡眠。\n加速時間時，附近的危險會讓你停下。\n\n避難所\n把食物、水與藥品存入共同存糧。\n清晨六點結算；記得留夠大家的份。",
                "SCAVENGE\nApproach a box, fallen enemy or door; tap Interact.\n\nLOOK AFTER YOURSELF\nOpen Bag to equip, eat, drink, treat wounds or sleep.\nNearby danger cancels accelerated time.\n\nSHELTER\nDeposit food, water and medicine in shared stock.\nDaily needs settle at 06:00. Leave enough for everyone.",
            ),
            _ => tr(
                lang,
                "儲存\n遊戲會定期自動儲存。\n面板裡也有「儲存」按鈕。\n\n離開與繼續\n暫停後選「儲存並返回主選單」。\n下次從「繼續遊戲」回到原來的位置。\n\n一段人生\n開始新局會取代舊存檔。\n死亡即結束本局，沒有讀檔復活。",
                "SAVE\nYour run is saved automatically at intervals.\nPanels also have a manual Save button.\n\nLEAVE AND RETURN\nPause, then choose Save and return to menu.\nContinue resumes your saved position and supplies.\n\nONE LIFE\nA new run replaces the previous save.\nDeath ends the run; it cannot be reloaded.",
            ),
        }
    };
    let font_size = if key.viewport.y < 500.0 {
        11.0
    } else if key.viewport.y < 700.0 {
        12.0
    } else {
        15.0
    };
    if key.viewport.y < 500.0 {
        let sections: Vec<_> = content.split("\n\n").collect();
        let columns: Vec<String> = if key.menu.page == TitlePage::About {
            vec![sections[..2].join("\n"), sections[2..].join("\n")]
        } else {
            sections.iter().map(|s| (*s).to_owned()).collect()
        };
        let gap = 20.0;
        let column_width =
            (rect.width() - 32.0 - gap * (columns.len() - 1) as f32) / columns.len() as f32;
        for (index, content) in columns.iter().enumerate() {
            text(
                root,
                ScreenRect::new(
                    rect.min.x + 16.0 + index as f32 * (column_width + gap),
                    rect.min.y + 12.0,
                    column_width,
                    rect.height() - 24.0,
                ),
                content,
                font_size,
                PAPER,
                font,
            );
        }
    } else {
        text(
            root,
            ScreenRect::new(
                rect.min.x + 16.0,
                rect.min.y + 14.0,
                rect.width() - 32.0,
                rect.height() - 28.0,
            ),
            content,
            font_size,
            PAPER,
            font,
        );
    }
    button(
        root,
        l.back,
        if key.menu.page == TitlePage::Controls && key.menu.help_page > 0 {
            tr(lang, "← 上一頁", "← Previous")
        } else {
            tr(lang, "← 返回", "← Back")
        },
        ButtonStyle::Quiet,
        font,
    );
    button(
        root,
        l.start,
        if key.menu.page == TitlePage::Controls && key.menu.help_page < 2 {
            tr(lang, "下一頁  →", "Next  →")
        } else {
            tr(lang, "返回主選單", "Back to menu")
        },
        ButtonStyle::Primary,
        font,
    );
}

fn identity(role: &Role, gender: Gender, lang: Language) -> String {
    format!(
        "{} · {} · {}{}",
        role.given(gender, lang),
        gender.label(lang),
        role.age_of(gender),
        tr(lang, " 歲", " years")
    )
}

fn supplies(role: &Role, lang: Language, separator: &str) -> String {
    role.inv
        .iter()
        .map(|(id, count)| {
            format!(
                "{} ×{}",
                item(id).map_or(*id, |item| item.name(lang)),
                count
            )
        })
        .collect::<Vec<_>>()
        .join(separator)
}

fn role_short(role: &Role, lang: Language) -> &'static str {
    match role.id {
        "tech" => tr(lang, "水電師傅", "Technician"),
        "jp" => tr(lang, "日籍駐在員", "Expat engineer"),
        "courier" => tr(lang, "外送員", "Courier"),
        _ => role.name(lang),
    }
}

fn portrait_node(
    root: &mut ChildSpawnerCommands,
    assets: &TitleAssets,
    role: usize,
    gender: Gender,
    rect: ScreenRect,
) {
    if let Some((_, _, image)) = assets
        .portraits
        .iter()
        .find(|(i, g, _)| *i == role && *g == gender)
    {
        root.spawn((node(rect), ImageNode::new(image.clone())));
    }
}

fn languages(
    root: &mut ChildSpawnerCommands,
    zh: ScreenRect,
    en: ScreenRect,
    lang: Language,
    font: &UiFont,
) {
    button(
        root,
        zh,
        "繁中",
        if lang == Language::ZhTw {
            ButtonStyle::Selected
        } else {
            ButtonStyle::Quiet
        },
        font,
    );
    button(
        root,
        en,
        "EN",
        if lang == Language::En {
            ButtonStyle::Selected
        } else {
            ButtonStyle::Quiet
        },
        font,
    );
}

#[derive(Clone, Copy)]
enum ButtonStyle {
    Primary,
    Continue,
    Quiet,
    Selected,
    Disabled,
}

fn button(
    root: &mut ChildSpawnerCommands,
    rect: ScreenRect,
    label: &str,
    style: ButtonStyle,
    font: &UiFont,
) {
    let (bg, fg, border) = match style {
        ButtonStyle::Primary => (GOLD, INK, GOLD),
        ButtonStyle::Continue => (
            Color::srgb(0.19, 0.28, 0.22),
            PAPER,
            Color::srgb(0.37, 0.46, 0.35),
        ),
        ButtonStyle::Selected => (Color::srgb(0.29, 0.25, 0.16), GOLD, GOLD),
        ButtonStyle::Quiet => (PANEL, PAPER, LINE),
        ButtonStyle::Disabled => (
            Color::srgb(0.10, 0.11, 0.10),
            Color::srgb(0.37, 0.39, 0.35),
            Color::srgb(0.2, 0.22, 0.19),
        ),
    };
    panel(root, rect, bg, border);
    text_center(
        root,
        ScreenRect::new(
            rect.min.x + 5.0,
            rect.min.y + 3.0,
            rect.width() - 10.0,
            rect.height() - 6.0,
        ),
        label,
        if rect.height() <= 36.0 {
            12.0
        } else if rect.height() <= 48.0 {
            14.0
        } else {
            18.0
        },
        fg,
        font,
    );
}

fn panel(root: &mut ChildSpawnerCommands, rect: ScreenRect, background: Color, border: Color) {
    root.spawn((
        Node {
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(5.0)),
            ..node(rect)
        },
        BackgroundColor(background),
        BorderColor::all(border),
    ));
}

fn line(root: &mut ChildSpawnerCommands, rect: ScreenRect, color: Color) {
    root.spawn((node(rect), BackgroundColor(color)));
}

fn text(
    root: &mut ChildSpawnerCommands,
    rect: ScreenRect,
    value: &str,
    size: f32,
    color: Color,
    font: &UiFont,
) {
    root.spawn((
        node(rect),
        Text::new(value),
        TextFont::from_font_size(FontSize::Px(size)).with_font(font.0.clone()),
        TextColor(color),
        TextLayout::justify(Justify::Left),
    ));
}

fn text_center(
    root: &mut ChildSpawnerCommands,
    rect: ScreenRect,
    value: &str,
    size: f32,
    color: Color,
    font: &UiFont,
) {
    root.spawn(Node {
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..node(rect)
    })
    .with_children(|parent| {
        parent.spawn((
            Node {
                max_width: percent(100.0),
                ..default()
            },
            Text::new(value),
            TextFont::from_font_size(FontSize::Px(size)).with_font(font.0.clone()),
            TextColor(color),
            TextLayout::justify(Justify::Center),
        ));
    });
}

fn node(rect: ScreenRect) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(rect.min.x),
        top: px(rect.min.y),
        width: px(rect.width()),
        height: px(rect.height()),
        ..default()
    }
}

fn viewport(windows: &Query<&Window, With<PrimaryWindow>>) -> Vec2 {
    windows
        .iter()
        .next()
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0))
}

/// The real Wanhua street plan, subdued behind the menu. Deterministic and built once.
fn city_plan() -> Image {
    use crate::map::canvas::{Canvas, rgba};
    let mut c = Canvas::new(640, 640);
    for y in 0..640 {
        for x in 0..640 {
            let noise = (((x * 73 + y * 151) ^ (x * y + 19)) % 17) as f32 / 2550.0;
            let glow = (1.0
                - Vec2::new((x as f32 - 520.0) / 650.0, (y as f32 - 210.0) / 600.0).length())
            .max(0.0);
            c.px[y * 640 + x] = [
                0.045 + glow * 0.025 + noise,
                0.052 + glow * 0.024 + noise,
                0.049 + glow * 0.017 + noise,
                1.0,
            ];
        }
    }
    let world = crate::map::world();
    let project = |p: Vec2| {
        (
            (p.x - world.min.x) / (world.max.x - world.min.x) * 640.0,
            (p.y - world.min.y) / (world.max.y - world.min.y) * 640.0,
        )
    };
    for road in &world.roads {
        let points: Vec<_> = road.p.iter().map(|p| project(*p)).collect();
        c.polyline(
            &points,
            (road.w * 0.24).clamp(0.5, 3.0),
            rgba(
                163.0,
                151.0,
                111.0,
                if road.w > 12.0 { 0.12 } else { 0.045 },
            ),
        );
    }
    c.to_image(true, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::state::app::StatesPlugin;

    fn app(available: bool) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(GameFlow::Title)
            .init_resource::<TitleMenu>()
            .init_resource::<SelectedRole>()
            .init_resource::<Language>()
            .insert_resource(SaveStatus {
                available,
                ..default()
            })
            .add_message::<TitleAction>()
            .add_message::<StartGame>()
            .add_message::<ContinueGame>()
            .add_message::<SfxEvent>()
            .add_systems(Update, apply_actions);
        app
    }

    fn act(app: &mut App, action: TitleAction) {
        app.world_mut().write_message(action);
        app.update();
    }

    #[test]
    fn selection_and_back_never_start_or_overwrite_a_saved_run() {
        let mut app = app(true);
        act(&mut app, TitleAction::Depart);
        assert!(app.world().resource::<Messages<StartGame>>().is_empty());
        act(&mut app, TitleAction::NewGame);
        act(&mut app, TitleAction::Role(3));
        act(&mut app, TitleAction::Gender(Gender::Female));
        act(&mut app, TitleAction::Next);
        assert_eq!(
            app.world().resource::<TitleMenu>().page,
            TitlePage::Briefing
        );
        assert!(app.world().resource::<Messages<StartGame>>().is_empty());
        assert!(app.world().resource::<SaveStatus>().available);
        act(&mut app, TitleAction::Back);
        assert_eq!(
            app.world().resource::<TitleMenu>().page,
            TitlePage::Survivor
        );
        act(&mut app, TitleAction::Next);
        act(&mut app, TitleAction::Depart);
        assert_eq!(app.world().resource::<Messages<StartGame>>().len(), 1);
        assert_eq!(app.world().resource::<SelectedRole>().id, "nurse");
        assert_eq!(
            app.world().resource::<SelectedRole>().gender,
            Gender::Female
        );
    }

    #[test]
    fn continue_only_loads_an_available_save_from_home() {
        for available in [false, true] {
            let mut app = app(available);
            act(&mut app, TitleAction::Continue);
            assert_eq!(
                app.world().resource::<Messages<ContinueGame>>().is_empty(),
                !available
            );
            assert!(app.world().resource::<Messages<StartGame>>().is_empty());
            app.world_mut()
                .resource_mut::<Messages<ContinueGame>>()
                .clear();
            act(&mut app, TitleAction::NewGame);
            act(&mut app, TitleAction::Continue);
            assert!(app.world().resource::<Messages<ContinueGame>>().is_empty());
        }
    }

    #[test]
    fn pointer_uses_the_same_guarded_navigation_as_qa() {
        let viewport = Vec2::new(390.0, 844.0);
        let mut app = app(false);
        let action = hit_action(
            *app.world().resource::<TitleMenu>(),
            viewport,
            home_layout(viewport).new_game.center(),
        )
        .unwrap();
        act(&mut app, action);
        let action = hit_action(
            *app.world().resource::<TitleMenu>(),
            viewport,
            title_layout(viewport).start.center(),
        )
        .unwrap();
        act(&mut app, action);
        assert_eq!(
            app.world().resource::<TitleMenu>().page,
            TitlePage::Briefing
        );
        assert!(app.world().resource::<Messages<StartGame>>().is_empty());
    }
}
