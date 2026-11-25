//! Native inventory, shelter, barter and conscience panels.
//!
//! These panels consume player input but keep the world clock running, just as
//! the web panels do. Pages are laid out from the actual viewport; a small
//! landscape phone gets fewer rows instead of losing buttons below the screen.

use bevy::{ecs::system::SystemParam, prelude::*, window::PrimaryWindow};

use crate::game_flow::{Campaign, GameFlow};
use crate::i18n::{Language, UiFont, tr};
use crate::input::{ScreenRect, safe_insets};
use crate::inventory::{Inventory, ItemKind, item};
use crate::player::Player;
use crate::save::{SaveGame, SaveStatus};
use crate::survival::{MoralChoice, Shelter, TRADE_ITEMS, TRADE_SPOTS, Upgrade, trade_at};
use crate::worldplay::{MoralPrompt, PlayerNeeds, WorldAction, WorldNotice};

use super::{ConsumedPresses, PauseOpen, StateScoped, TogglePause};

/// Input capture is separate from pause: looking in a bag does not stop dawn.
#[derive(Resource, Default, Debug)]
pub struct UiModal {
    pub open: bool,
    /// Keeps a toolbar press captured until every pointer has been released.
    pub pointer_captured: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    Inventory,
    Shelter,
    Trade,
    Close,
}

#[derive(Message, Clone, Copy, Debug)]
pub struct OpenPanel(pub PanelKind);

impl UiModal {
    pub fn blocks_input(&self) -> bool {
        self.open || self.pointer_captured
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Inventory,
    Shelter,
    Trade,
    Journal,
    Moral,
}

#[derive(Resource, Default)]
pub(super) struct PanelState {
    view: Option<View>,
    page: usize,
}

#[derive(Component)]
pub(super) struct SurvivalUiRoot;

#[derive(Clone, Debug, PartialEq)]
enum Action {
    Open(View),
    Close,
    Page(usize),
    Deposit,
    Pray,
    Build(Upgrade),
    Buy(String),
    Sell(String),
    Use(String),
    Sleep,
    FillWater,
    Speed(u8),
    Interact,
    Save,
    Pause,
    Moral(MoralChoice),
}

#[derive(Clone, Debug, PartialEq)]
struct Widget {
    rect: ScreenRect,
    text: String,
    action: Option<Action>,
    enabled: bool,
    font_size: f32,
}

#[derive(Clone, Default, Debug, PartialEq)]
struct Frame {
    panel: Option<ScreenRect>,
    widgets: Vec<Widget>,
}

impl Frame {
    fn text(&mut self, rect: ScreenRect, text: String, font_size: f32) {
        self.widgets.push(Widget {
            rect,
            text,
            action: None,
            enabled: true,
            font_size,
        });
    }

    fn button(&mut self, rect: ScreenRect, text: String, action: Action, enabled: bool) {
        self.widgets.push(Widget {
            rect,
            text,
            action: Some(action),
            enabled,
            font_size: 15.0,
        });
    }

    fn action_at(&self, point: Vec2) -> Option<Action> {
        self.widgets
            .iter()
            .rev()
            .find(|widget| widget.enabled && widget.rect.contains(point) && widget.action.is_some())
            .and_then(|widget| widget.action.clone())
    }
}

#[derive(Clone)]
struct Row {
    text: String,
    buttons: Vec<(String, Action, bool)>,
}

impl Row {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            buttons: Vec::new(),
        }
    }

    fn action(text: impl Into<String>, label: &str, action: Action, enabled: bool) -> Self {
        Self {
            text: text.into(),
            buttons: vec![(label.to_owned(), action, enabled)],
        }
    }
}

#[derive(Resource, Default)]
pub(super) struct RenderCache(Frame);

pub(super) fn toolbar_rects(viewport: Vec2) -> [ScreenRect; 5] {
    let insets = safe_insets(viewport);
    let gap = 5.0;
    let width = (viewport.x - insets.left - insets.right).max(1.0);
    let button_w = (width - gap * 4.0) / 5.0;
    let y = viewport.y - insets.bottom - 46.0;
    std::array::from_fn(|i| {
        ScreenRect::new(insets.left + i as f32 * (button_w + gap), y, button_w, 42.0)
    })
}

pub(super) fn toolbar_hit(viewport: Vec2, point: Vec2) -> bool {
    toolbar_rects(viewport)
        .iter()
        .any(|rect| rect.contains(point))
}

fn panel_rect(viewport: Vec2) -> ScreenRect {
    let insets = safe_insets(viewport);
    let width = (viewport.x - insets.left - insets.right).clamp(1.0, 900.0);
    ScreenRect::new(
        (viewport.x - width) * 0.5,
        insets.top + 4.0,
        width,
        (viewport.y - insets.top - insets.bottom - 8.0).max(1.0),
    )
}

fn position(player: &Query<&Transform, With<Player>>) -> [f32; 2] {
    player
        .iter()
        .next()
        .map(|transform| [transform.translation.x, transform.translation.z])
        .unwrap_or([f32::MAX, f32::MAX])
}

fn item_name(id: &str, lang: Language) -> &str {
    item(id).map_or(id, |definition| definition.name(lang))
}

fn inventory_rows(inventory: &Inventory, campaign: &Campaign, lang: Language) -> Vec<Row> {
    let mut rows = vec![Row::action(
        tr(
            lang,
            "找個安全的室內休息，恢復疲勞。",
            "Rest safely indoors to recover fatigue.",
        ),
        if campaign.sleeping {
            tr(lang, "起床", "Wake")
        } else {
            tr(lang, "睡覺", "Sleep")
        },
        Action::Sleep,
        true,
    )];
    rows.push(Row {
        text: tr(lang, "時間速度", "Time speed").to_owned(),
        buttons: [1, 3, 20]
            .into_iter()
            .map(|speed| {
                (
                    format!("×{speed}"),
                    Action::Speed(speed),
                    campaign.speed != speed,
                )
            })
            .collect(),
    });
    for (id, count) in inventory.stacks().iter().filter(|(_, n)| *n > 0) {
        let Some(definition) = item(id) else { continue };
        let equipped = inventory.equipped == *id;
        let label = if equipped {
            tr(lang, "卸下", "Unequip")
        } else if matches!(definition.kind, ItemKind::Gun | ItemKind::Melee) {
            tr(lang, "裝備", "Equip")
        } else if definition.raw || definition.id == "cstove" {
            tr(lang, "烹煮", "Cook")
        } else if definition.mag > 0 {
            tr(lang, "取出子彈", "Unload")
        } else {
            tr(lang, "使用", "Use")
        };
        let usable = definition.usable();
        let text = format!(
            "{} ×{}\n{}",
            definition.name(lang),
            count,
            definition.description(lang)
        );
        rows.push(if usable {
            Row::action(text, label, Action::Use(id.clone()), true)
        } else {
            Row::text(text)
        });
    }
    if inventory.stacks().iter().all(|(_, count)| *count == 0) {
        rows.push(Row::text(tr(lang, "背包是空的。", "Your bag is empty.")));
    }
    rows
}

fn shelter_rows(
    shelter: &Shelter,
    inventory: &Inventory,
    pos: [f32; 2],
    lang: Language,
) -> Vec<Row> {
    let here = shelter.at_home(pos);
    let mut rows = Vec::new();
    if !here {
        let home = shelter.home.map_or_else(
            || tr(lang, "尚未設定住處", "Home not set").to_string(),
            |home| match lang {
                Language::ZhTw => format!("住處座標：{:.0}, {:.0}", home[0], home[1]),
                Language::En => format!("Home: {:.0}, {:.0}", home[0], home[1]),
            },
        );
        rows.push(Row::text(format!(
            "{}\n{home}",
            tr(
                lang,
                "回到家才能建造、存放或拜拜。",
                "Return home to build, deposit or pray."
            )
        )));
    }
    rows.push(Row::action(
        tr(
            lang,
            "把背包裡所有食物、水和藥放進共同存糧。",
            "Deposit all food, drinks and medicine from your bag.",
        ),
        tr(lang, "存放", "Deposit"),
        Action::Deposit,
        here,
    ));
    rows.push(Row::action(
        tr(
            lang,
            "神明廳拜拜：每天一次，安定大家的心。",
            "Pray at the household shrine once each day.",
        ),
        tr(lang, "拜拜", "Pray"),
        Action::Pray,
        here && shelter.pray_day != shelter.day,
    ));
    for upgrade in [
        Upgrade::Boards,
        Upgrade::Beds,
        Upgrade::Stove,
        Upgrade::Rain,
        Upgrade::Planter,
    ] {
        let built = shelter.has(upgrade);
        let cost = upgrade
            .cost()
            .iter()
            .map(|(id, amount)| format!("{} ×{}", item_name(id, lang), amount))
            .collect::<Vec<_>>()
            .join(" / ");
        let enough = upgrade
            .cost()
            .iter()
            .all(|(id, count)| inventory.count(id) >= *count);
        rows.push(Row::action(
            format!(
                "{}\n{}\n{}",
                upgrade.name(lang),
                cost,
                upgrade.description(lang)
            ),
            if built {
                tr(lang, "已完成", "Built")
            } else {
                tr(lang, "建造", "Build")
            },
            Action::Build(upgrade),
            here && enough && !built,
        ));
    }
    for survivor in &shelter.group {
        let status = if survivor.dead {
            tr(lang, "已過世", "Deceased").to_owned()
        } else if !survivor.here {
            tr(lang, "還沒來", "Not here yet").to_owned()
        } else {
            match lang {
                Language::ZhTw => format!(
                    "飽 {:.0} / 病 {:.0} / 傷 {:.0} / 心情 {:.0}",
                    survivor.hunger, survivor.sick, survivor.hurt, survivor.mood
                ),
                Language::En => format!(
                    "Full {:.0} / Ill {:.0} / Hurt {:.0} / Mood {:.0}",
                    survivor.hunger, survivor.sick, survivor.hurt, survivor.mood
                ),
            }
        };
        rows.push(Row::text(format!(
            "{} · {}\n{}",
            survivor.name(lang),
            survivor.role(lang),
            status
        )));
    }
    let mut stock = shelter
        .stash
        .stacks()
        .iter()
        .filter(|(_, count)| *count > 0)
        .peekable();
    if stock.peek().is_none() {
        rows.push(Row::text(tr(
            lang,
            "共同存糧：空的",
            "Shared stockpile: empty",
        )));
    }
    for (id, count) in stock {
        rows.push(Row::text(format!(
            "{}: {} ×{}",
            tr(lang, "存糧", "Stockpile"),
            item_name(id, lang),
            count
        )));
    }
    rows.push(Row::action(
        tr(
            lang,
            "到住處的水源附近接一瓶水。停水時無法取水。",
            "Fill a bottle near the water fixture at home. Unavailable during water cuts.",
        ),
        tr(lang, "裝飲用水", "Fill water"),
        Action::FillWater,
        here,
    ));
    rows
}

fn trade_rows(shelter: &Shelter, inventory: &Inventory, pos: [f32; 2], lang: Language) -> Vec<Row> {
    let nearby = trade_at(pos).is_some();
    let mut rows = Vec::new();
    if !nearby {
        rows.push(Row::text(tr(
            lang,
            "走到交易點附近才能買賣。",
            "Walk close to a trading spot to barter.",
        )));
        for spot in TRADE_SPOTS {
            rows.push(Row::text(format!(
                "{}: {:.0}, {:.0}",
                spot.name(lang),
                spot.pos[0],
                spot.pos[1]
            )));
        }
    }
    for id in TRADE_ITEMS.iter().copied().filter(|id| *id != "cig") {
        let buy = shelter.price(id);
        let sell = (buy - 1).max(1);
        rows.push(Row {
            text: format!("{} ×{}", item_name(id, lang), inventory.count(id)),
            buttons: vec![
                (
                    format!("{} {}", tr(lang, "買", "Buy"), buy),
                    Action::Buy(id.to_owned()),
                    nearby && inventory.count("cig") >= buy as u32,
                ),
                (
                    format!("{} {}", tr(lang, "賣", "Sell"), sell),
                    Action::Sell(id.to_owned()),
                    nearby && inventory.count(id) > 0,
                ),
            ],
        });
    }
    rows
}

fn moral_rows(inventory: &Inventory, lang: Language) -> Vec<Row> {
    vec![
        Row::text(tr(
            lang,
            "這戶還有人住。門縫裡，一位獨居的阿公縮在床邊，櫃子裡是他的藥和最後一點米。",
            "Someone still lives here. An elderly man is alone by his bed. The cabinet holds his medicine and the last of his rice.",
        )),
        Row::action(
            tr(
                lang,
                "還是拿走：大家心情下降，良心下降。",
                "Take it anyway: group morale and conscience fall.",
            ),
            tr(lang, "拿走", "Take"),
            Action::Moral(MoralChoice::Take),
            true,
        ),
        Row::action(
            tr(
                lang,
                "留一包米給他：花費米 ×1，良心提升。",
                "Leave him rice: costs rice ×1, raises conscience.",
            ),
            tr(lang, "送米", "Give"),
            Action::Moral(MoralChoice::Give),
            inventory.count("rice") > 0,
        ),
        Row::action(
            tr(lang, "輕輕把門帶上。", "Quietly close the door."),
            tr(lang, "離開", "Leave"),
            Action::Moral(MoralChoice::Leave),
            true,
        ),
    ]
}

fn row_height(row: &Row, width: f32) -> f32 {
    let available = (width - row.buttons.len() as f32 * 77.0 - 16.0).max(60.0);
    let lines: f32 = row
        .text
        .split('\n')
        .map(|line| {
            let units: f32 = line
                .chars()
                .map(|ch| if ch.is_ascii() { 0.57 } else { 1.0 })
                .sum();
            (units * 15.0 / available).ceil().max(1.0)
        })
        .sum();
    (lines * 19.0 + 16.0).max(52.0)
}

fn pages(rows: Vec<Row>, width: f32, height: f32) -> Vec<Vec<(Row, f32)>> {
    let mut result = vec![Vec::new()];
    let mut used = 0.0;
    for row in rows {
        let h = row_height(&row, width).min(height.max(1.0));
        if used + h > height && !result.last().unwrap().is_empty() {
            result.push(Vec::new());
            used = 0.0;
        }
        result.last_mut().unwrap().push((row, h));
        used += h;
    }
    result
}

fn view_name(view: View, lang: Language) -> &'static str {
    match view {
        View::Inventory => tr(lang, "背包", "Bag"),
        View::Shelter => tr(lang, "避難所", "Shelter"),
        View::Trade => tr(lang, "交易", "Barter"),
        View::Journal => tr(lang, "日誌", "Journal"),
        View::Moral => tr(lang, "門後的人", "Behind the door"),
    }
}

fn make_frame(
    viewport: Vec2,
    state: &PanelState,
    shelter: &Shelter,
    inventory: &Inventory,
    campaign: &Campaign,
    needs: &PlayerNeeds,
    pos: [f32; 2],
    lang: Language,
    notice: &WorldNotice,
    save: &SaveStatus,
) -> Frame {
    let mut frame = Frame::default();
    let toolbar = toolbar_rects(viewport);
    let Some(view) = state.view else {
        for (rect, label, action) in [
            (
                toolbar[0],
                tr(lang, "背包 I", "Bag I"),
                Action::Open(View::Inventory),
            ),
            (
                toolbar[1],
                tr(lang, "避難所 C", "Shelter C"),
                Action::Open(View::Shelter),
            ),
            (
                toolbar[2],
                tr(lang, "交易 T", "Trade T"),
                Action::Open(View::Trade),
            ),
            (toolbar[3], tr(lang, "互動 E", "Use E"), Action::Interact),
            (toolbar[4], tr(lang, "暫停", "Pause"), Action::Pause),
        ] {
            frame.button(rect, label.to_owned(), action, true);
        }
        if !notice.text.is_empty() {
            let width = (toolbar[4].max.x - toolbar[0].min.x).min(600.0);
            frame.text(
                ScreenRect::new(
                    (viewport.x - width) * 0.5,
                    viewport.y * 0.5 - 24.0,
                    width,
                    48.0,
                ),
                notice.text.clone(),
                14.0,
            );
        }
        return frame;
    };

    let panel = panel_rect(viewport);
    frame.panel = Some(panel);
    let x = panel.min.x + 10.0;
    let width = (panel.width() - 20.0).max(1.0);
    let header_y = panel.min.y + 8.0;
    let title = match view {
        View::Shelter => format!(
            "{} · {} {}",
            view_name(view, lang),
            tr(lang, "第", "Day"),
            shelter.day
        ),
        View::Trade => trade_at(pos).map_or_else(
            || view_name(view, lang).to_string(),
            |i| TRADE_SPOTS[i].name(lang).to_string(),
        ),
        _ => view_name(view, lang).to_string(),
    };
    frame.text(
        ScreenRect::new(x, header_y, width - 64.0, 42.0),
        title,
        23.0,
    );
    frame.button(
        ScreenRect::new(x + width - 64.0, header_y, 64.0, 42.0),
        tr(lang, "關閉", "Close").to_owned(),
        Action::Close,
        true,
    );

    let mut y = header_y + 50.0;
    if view != View::Moral {
        let gap = 5.0;
        let tab_w = (width - gap * 3.0) / 4.0;
        for (i, tab) in [View::Inventory, View::Shelter, View::Trade, View::Journal]
            .into_iter()
            .enumerate()
        {
            frame.button(
                ScreenRect::new(x + i as f32 * (tab_w + gap), y, tab_w, 38.0),
                view_name(tab, lang).to_owned(),
                Action::Open(tab),
                view != tab,
            );
        }
        y += 46.0;
    }
    let summary = match view {
        View::Inventory => match lang {
            Language::ZhTw => format!(
                "負重 {:.1} kg · 飽食 {:.0} · 水分 {:.0} · 疲勞 {:.0}",
                inventory.weight(),
                needs.hunger,
                needs.thirst,
                needs.fatigue
            ),
            Language::En => format!(
                "{:.1} kg · Fullness {:.0} · Hydration {:.0} · Fatigue {:.0}",
                inventory.weight(),
                needs.hunger,
                needs.thirst,
                needs.fatigue
            ),
        },
        View::Shelter => match lang {
            Language::ZhTw => format!(
                "心情 {} · 緊縮 ×{:.2} · 良心 {}\n{}",
                shelter.avg_mood(),
                shelter.scarcity,
                shelter.karma,
                shelter.event
            ),
            Language::En => format!(
                "Morale {} · Scarcity ×{:.2} · Conscience {}\n{}",
                shelter.avg_mood(),
                shelter.scarcity,
                shelter.karma,
                shelter.event
            ),
        },
        View::Trade => match lang {
            Language::ZhTw => format!(
                "長樂菸 ×{} · 緊縮 ×{:.2} · 標價單位：包菸",
                inventory.count("cig"),
                shelter.scarcity
            ),
            Language::En => format!(
                "Cigarettes ×{} · Scarcity ×{:.2} · Prices in packs",
                inventory.count("cig"),
                shelter.scarcity
            ),
        },
        View::Journal => tr(
            lang,
            "每天清晨六點更新。開啟面板時，世界仍在運作。",
            "Updated at 06:00. The world keeps running while this panel is open.",
        )
        .to_string(),
        View::Moral => String::new(),
    };
    if !summary.is_empty() {
        frame.text(ScreenRect::new(x, y, width, 44.0), summary, 14.0);
        y += 48.0;
    }
    let footer_y = panel.max.y - 52.0;
    let status_y = footer_y - 42.0;
    let available_h = (status_y - y - 4.0).max(42.0);
    let rows = match view {
        View::Inventory => inventory_rows(inventory, campaign, lang),
        View::Shelter => shelter_rows(shelter, inventory, pos, lang),
        View::Trade => trade_rows(shelter, inventory, pos, lang),
        View::Moral => moral_rows(inventory, lang),
        View::Journal => {
            if shelter.log.is_empty() {
                vec![Row::text(tr(lang, "還沒有日誌。", "No entries yet."))]
            } else {
                shelter
                    .log
                    .iter()
                    .map(|line| Row::text(line.clone()))
                    .collect()
            }
        }
    };
    let pages = pages(rows, width, available_h);
    let page = state.page.min(pages.len() - 1);
    for (row, height) in &pages[page] {
        let action_width = row.buttons.len() as f32 * 77.0;
        frame.text(
            ScreenRect::new(x, y, (width - action_width - 6.0).max(1.0), *height - 4.0),
            row.text.clone(),
            15.0,
        );
        for (i, (label, action, enabled)) in row.buttons.iter().enumerate() {
            frame.button(
                ScreenRect::new(
                    x + width - action_width + i as f32 * 77.0,
                    y + 4.0,
                    72.0,
                    (*height - 12.0).max(36.0),
                ),
                label.clone(),
                action.clone(),
                *enabled,
            );
        }
        y += height;
    }
    let status = if let Some(error) = &save.error {
        error.clone()
    } else if !notice.text.is_empty() {
        notice.text.clone()
    } else {
        save.notice.clone()
    };
    frame.text(ScreenRect::new(x, status_y, width, 38.0), status, 13.0);
    let nav_w = (width * 0.24).min(110.0);
    frame.button(
        ScreenRect::new(x, footer_y, nav_w, 42.0),
        tr(lang, "上一頁", "Previous").to_owned(),
        Action::Page(page.saturating_sub(1)),
        page > 0,
    );
    frame.text(
        ScreenRect::new(x + nav_w, footer_y, width - nav_w * 3.0 - 10.0, 42.0),
        format!("{} / {}", page + 1, pages.len()),
        15.0,
    );
    frame.button(
        ScreenRect::new(x + width - nav_w * 2.0 - 5.0, footer_y, nav_w, 42.0),
        tr(lang, "下一頁", "Next").to_owned(),
        Action::Page(page + 1),
        page + 1 < pages.len(),
    );
    frame.button(
        ScreenRect::new(x + width - nav_w, footer_y, nav_w, 42.0),
        tr(lang, "儲存", "Save").to_owned(),
        Action::Save,
        view != View::Moral,
    );
    frame
}

#[derive(SystemParam)]
pub(super) struct PanelWorld<'w, 's> {
    player: Query<'w, 's, &'static Transform, With<Player>>,
    shelter: ResMut<'w, Shelter>,
    inventory: ResMut<'w, Inventory>,
    campaign: Res<'w, Campaign>,
    needs: Res<'w, PlayerNeeds>,
    prompt: Res<'w, MoralPrompt>,
    lang: Res<'w, Language>,
    notice: ResMut<'w, WorldNotice>,
    save_status: Res<'w, SaveStatus>,
}

pub(super) fn drive_panels(
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    consumed: Res<ConsumedPresses>,
    mut state: ResMut<PanelState>,
    mut modal: ResMut<UiModal>,
    world: PanelWorld,
    mut opens: MessageReader<OpenPanel>,
    mut world_actions: MessageWriter<WorldAction>,
    mut saves: MessageWriter<SaveGame>,
    mut pauses: MessageWriter<TogglePause>,
) {
    let PanelWorld {
        player,
        mut shelter,
        mut inventory,
        campaign,
        needs,
        prompt,
        lang,
        mut notice,
        save_status,
    } = world;
    let previous_view = state.view;
    if flow.get() != &GameFlow::Playing || pause.0 {
        state.view = None;
        modal.open = false;
        opens.clear();
        return;
    }
    for open in opens.read() {
        state.view = match open.0 {
            PanelKind::Inventory => Some(View::Inventory),
            PanelKind::Shelter => Some(View::Shelter),
            PanelKind::Trade => Some(View::Trade),
            PanelKind::Close => None,
        };
        state.page = 0;
    }
    if prompt.0.is_some() && state.view != Some(View::Moral) {
        state.view = Some(View::Moral);
        state.page = 0;
    } else if prompt.0.is_none() && state.view == Some(View::Moral) {
        state.view = None;
    }
    let viewport = windows
        .iter()
        .next()
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    let pos = position(&player);
    let frame = make_frame(
        viewport,
        &state,
        &shelter,
        &inventory,
        &campaign,
        &needs,
        pos,
        *lang,
        &notice,
        &save_status,
    );
    let mut action = consumed
        .points
        .iter()
        .find_map(|point| frame.action_at(*point));
    if state.view.is_some() && keys.just_pressed(KeyCode::Escape) {
        keys.clear_just_pressed(KeyCode::Escape);
        action = Some(Action::Close);
    } else if state.view != Some(View::Moral) {
        for (key, view) in [
            (KeyCode::KeyI, View::Inventory),
            (KeyCode::KeyC, View::Shelter),
            (KeyCode::KeyT, View::Trade),
        ] {
            if keys.just_pressed(key) {
                action = Some(if state.view == Some(view) {
                    Action::Close
                } else {
                    Action::Open(view)
                });
            }
        }
    }
    if let Some(action) = &action {
        println!("DTUI action={action:?}");
    }
    let buying = matches!(action, Some(Action::Buy(_)));
    match action {
        Some(Action::Open(view)) => {
            state.view = Some(view);
            state.page = 0;
        }
        Some(Action::Close) => {
            if state.view == Some(View::Moral) {
                world_actions.write(WorldAction::Moral(MoralChoice::Leave));
            }
            state.view = None;
        }
        Some(Action::Page(page)) => state.page = page,
        Some(Action::Deposit) => {
            notice.text = match shelter.deposit(&mut inventory, pos) {
                Ok(_) => tr(
                    *lang,
                    "物資放進了存糧。",
                    "Supplies added to the stockpile.",
                )
                .to_owned(),
                Err(error) => error.message(*lang).to_owned(),
            }
        }
        Some(Action::Pray) => {
            notice.text = match shelter.pray_at(pos, *lang) {
                Ok(message) => message,
                Err(error) => error.message(*lang).to_owned(),
            }
        }
        Some(Action::Build(upgrade)) => {
            notice.text = match shelter.build(upgrade, &mut inventory, pos) {
                Ok(()) => format!("{}: {}", upgrade.name(*lang), tr(*lang, "完成", "complete")),
                Err(error) => error.message(*lang).to_owned(),
            }
        }
        Some(Action::Buy(id)) | Some(Action::Sell(id)) => {
            notice.text = match shelter.barter(&mut inventory, &id, buying, pos) {
                Ok(()) => format!(
                    "{} {}",
                    tr(*lang, "交易完成：", "Traded:"),
                    item_name(&id, *lang)
                ),
                Err(error) => error.message(*lang).to_owned(),
            };
        }
        Some(Action::Use(id)) => {
            world_actions.write(WorldAction::Use(id));
        }
        Some(Action::Sleep) => {
            world_actions.write(WorldAction::Sleep);
            state.view = None;
        }
        Some(Action::FillWater) => {
            world_actions.write(WorldAction::FillWater);
        }
        Some(Action::Speed(speed)) => {
            world_actions.write(WorldAction::Speed(speed));
        }
        Some(Action::Interact) => {
            world_actions.write(WorldAction::Interact);
        }
        Some(Action::Save) => {
            notice.text.clear();
            saves.write(SaveGame);
        }
        Some(Action::Pause) => {
            state.view = None;
            pauses.write(TogglePause);
        }
        Some(Action::Moral(choice)) => {
            world_actions.write(WorldAction::Moral(choice));
            state.view = None;
        }
        None => {}
    }
    modal.open = state.view.is_some();
    if previous_view != state.view {
        println!("DTUI panel={:?}", state.view);
    }
}

pub(super) fn render_panels(
    mut commands: Commands,
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    windows: Query<&Window, With<PrimaryWindow>>,
    player: Query<&Transform, With<Player>>,
    state: Res<PanelState>,
    shelter: Res<Shelter>,
    inventory: Res<Inventory>,
    campaign: Res<Campaign>,
    needs: Res<PlayerNeeds>,
    lang: Res<Language>,
    notice: Res<WorldNotice>,
    save: Res<SaveStatus>,
    font: Res<UiFont>,
    roots: Query<Entity, With<SurvivalUiRoot>>,
    mut cache: ResMut<RenderCache>,
) {
    let frame = if flow.get() == &GameFlow::Playing && !pause.0 {
        let viewport = windows
            .iter()
            .next()
            .map(|w| Vec2::new(w.width(), w.height()))
            .unwrap_or(Vec2::new(1280.0, 720.0));
        make_frame(
            viewport,
            &state,
            &shelter,
            &inventory,
            &campaign,
            &needs,
            position(&player),
            *lang,
            &notice,
            &save,
        )
    } else {
        Frame::default()
    };
    if cache.0 == frame && (!roots.is_empty() || frame.widgets.is_empty()) {
        return;
    }
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    cache.0 = frame.clone();
    if frame.widgets.is_empty() {
        return;
    }
    commands
        .spawn((
            SurvivalUiRoot,
            StateScoped(GameFlow::Playing),
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                top: px(0.0),
                width: percent(100.0),
                height: percent(100.0),
                ..default()
            },
            GlobalZIndex(if frame.panel.is_some() { 60 } else { 25 }),
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            if let Some(panel) = frame.panel {
                parent.spawn((
                    node_at(panel),
                    BackgroundColor(Color::srgb(0.055, 0.065, 0.07)),
                    BorderColor::all(Color::srgb(0.5, 0.43, 0.25)),
                ));
            }
            for widget in frame.widgets {
                let button = widget.action.is_some();
                let bg = if button {
                    if widget.enabled {
                        Color::srgb(0.24, 0.25, 0.19)
                    } else {
                        Color::srgb(0.10, 0.12, 0.12)
                    }
                } else if frame.panel.is_none() {
                    Color::srgba(0.025, 0.03, 0.035, 0.88)
                } else {
                    Color::NONE
                };
                parent.spawn((
                    node_at(widget.rect),
                    BackgroundColor(bg),
                    BorderColor::all(if button {
                        Color::srgba(0.7, 0.62, 0.36, 0.65)
                    } else {
                        Color::NONE
                    }),
                    Text::new(widget.text),
                    TextFont::from_font_size(FontSize::Px(widget.font_size))
                        .with_font(font.0.clone()),
                    TextColor(if widget.enabled {
                        Color::srgb(0.96, 0.93, 0.84)
                    } else {
                        Color::srgb(0.57, 0.59, 0.55)
                    }),
                    TextLayout::justify(if button {
                        Justify::Center
                    } else {
                        Justify::Left
                    }),
                ));
            }
        });
}

fn node_at(rect: ScreenRect) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(rect.min.x),
        top: px(rect.min.y),
        width: px(rect.width()),
        height: px(rect.height()),
        padding: UiRect::all(px(4.0)),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        border: UiRect::all(px(1.0)),
        border_radius: BorderRadius::all(px(5.0)),
        overflow: Overflow::clip(),
        ..default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_actions_cannot_be_selected() {
        let rect = ScreenRect::new(10.0, 20.0, 80.0, 48.0);
        let mut frame = Frame::default();
        frame.button(
            rect,
            "Buy".to_owned(),
            Action::Buy("medkit".to_owned()),
            false,
        );
        assert_eq!(frame.action_at(rect.center()), None);
        frame.widgets[0].enabled = true;
        assert_eq!(
            frame.action_at(rect.center()),
            Some(Action::Buy("medkit".to_owned()))
        );
    }

    #[test]
    fn toolbar_and_pages_fit_phone_orientations() {
        for viewport in [
            Vec2::new(390.0, 844.0),
            Vec2::new(844.0, 390.0),
            Vec2::new(1280.0, 720.0),
        ] {
            let rects = toolbar_rects(viewport);
            for (i, rect) in rects.iter().enumerate() {
                assert!(rect.min.x >= 0.0 && rect.max.x <= viewport.x);
                assert!(rect.max.y <= viewport.y);
                assert!(rect.height() >= 40.0);
                for other in &rects[i + 1..] {
                    assert!(!rect.intersects(*other));
                }
            }
            let available = panel_rect(viewport).height() - 250.0;
            let rows = (0..31)
                .map(|_| {
                    Row::text("A longer journal entry that wraps safely on a narrow phone screen.")
                })
                .collect();
            let pages = pages(rows, panel_rect(viewport).width() - 20.0, available);
            assert_eq!(pages.iter().map(Vec::len).sum::<usize>(), 31);
            for page in pages {
                assert!(page.iter().map(|(_, h)| h).sum::<f32>() <= available + 0.01);
            }
        }
    }

    #[test]
    fn modal_capture_is_not_a_pause_flag() {
        let mut modal = UiModal::default();
        assert!(!modal.blocks_input());
        modal.pointer_captured = true;
        assert!(modal.blocks_input());
        modal.pointer_captured = false;
        modal.open = true;
        assert!(modal.blocks_input());
    }

    #[test]
    fn every_panel_action_stays_inside_the_viewport() {
        let shelter = Shelter {
            home: Some([0.0, 0.0]),
            ..default()
        };
        let inventory = Inventory::from_role(&crate::roles::ROLES[0]);
        let campaign = Campaign::default();
        let needs = PlayerNeeds {
            hunger: 100.0,
            thirst: 100.0,
            fatigue: 0.0,
        };
        let notice = WorldNotice {
            text: String::new(),
        };
        let save = SaveStatus::default();
        for viewport in [Vec2::new(390.0, 844.0), Vec2::new(844.0, 390.0)] {
            for view in [
                View::Inventory,
                View::Shelter,
                View::Trade,
                View::Journal,
                View::Moral,
            ] {
                for lang in [Language::ZhTw, Language::En] {
                    for page in 0..40 {
                        let state = PanelState {
                            view: Some(view),
                            page,
                        };
                        let frame = make_frame(
                            viewport,
                            &state,
                            &shelter,
                            &inventory,
                            &campaign,
                            &needs,
                            [0.0, 0.0],
                            lang,
                            &notice,
                            &save,
                        );
                        let actions: Vec<_> = frame
                            .widgets
                            .iter()
                            .filter(|w| w.action.is_some())
                            .collect();
                        for (i, widget) in actions.iter().enumerate() {
                            assert!(widget.rect.min.x >= 0.0 && widget.rect.min.y >= 0.0);
                            assert!(
                                widget.rect.max.x <= viewport.x && widget.rect.max.y <= viewport.y,
                                "{view:?}: {widget:?}"
                            );
                            for other in &actions[i + 1..] {
                                assert!(
                                    !widget.rect.intersects(other.rect),
                                    "{view:?}: {} / {}",
                                    widget.text,
                                    other.text
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn shelter_deposit_click_moves_supplies_and_keeps_world_unpaused() {
        use bevy::state::app::StatesPlugin;

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(GameFlow::Playing)
            .init_resource::<PauseOpen>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ConsumedPresses>()
            .init_resource::<UiModal>()
            .insert_resource(PanelState {
                view: Some(View::Shelter),
                page: 0,
            })
            .init_resource::<Shelter>()
            .init_resource::<Inventory>()
            .init_resource::<Campaign>()
            .insert_resource(PlayerNeeds {
                hunger: 100.0,
                thirst: 100.0,
                fatigue: 0.0,
            })
            .insert_resource(MoralPrompt(None))
            .init_resource::<Language>()
            .insert_resource(WorldNotice {
                text: String::new(),
            })
            .init_resource::<SaveStatus>()
            .add_message::<OpenPanel>()
            .add_message::<WorldAction>()
            .add_message::<SaveGame>()
            .add_message::<TogglePause>()
            .add_systems(Update, drive_panels);
        app.world_mut().spawn((Player, Transform::default()));
        app.world_mut().resource_mut::<Shelter>().home = Some([0.0, 0.0]);
        app.world_mut().resource_mut::<Inventory>().add("rice", 3);
        app.world_mut().resource_mut::<Inventory>().add("plank", 2);
        let world = app.world();
        let frame = make_frame(
            Vec2::new(1280.0, 720.0),
            world.resource::<PanelState>(),
            world.resource::<Shelter>(),
            world.resource::<Inventory>(),
            world.resource::<Campaign>(),
            world.resource::<PlayerNeeds>(),
            [0.0, 0.0],
            Language::ZhTw,
            world.resource::<WorldNotice>(),
            world.resource::<SaveStatus>(),
        );
        let point = frame
            .widgets
            .iter()
            .find(|w| w.action == Some(Action::Deposit))
            .unwrap()
            .rect
            .center();
        app.world_mut()
            .resource_mut::<ConsumedPresses>()
            .points
            .push(point);
        app.update();
        assert_eq!(app.world().resource::<Inventory>().count("rice"), 0);
        assert_eq!(app.world().resource::<Inventory>().count("plank"), 2);
        assert_eq!(app.world().resource::<Shelter>().stash.count("rice"), 5);
        assert!(app.world().resource::<UiModal>().open);
        assert!(!app.world().resource::<PauseOpen>().0);
    }
}
