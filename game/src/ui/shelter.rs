//! Native inventory, shelter, barter and conscience panels.
//!
//! These panels consume player input but keep the world clock running, just as
//! the web panels do. Pages are laid out from the actual viewport; a small
//! landscape phone gets fewer rows instead of losing buttons below the screen.

use bevy::{ecs::system::SystemParam, prelude::*, window::PrimaryWindow};

use crate::audio::SfxEvent;
use crate::game_flow::{Campaign, GameFlow};
use crate::i18n::{Language, UiFont, tr};
use crate::input::{ScreenRect, safe_insets};
use crate::inventory::{Inventory, ItemKind, item};
use crate::player::Player;
use crate::save::{SaveGame, SaveStatus};
use crate::story::TIMELINE;
use crate::survival::{MoralChoice, Shelter, TRADE_ITEMS, TRADE_SPOTS, Upgrade, trade_at};
use crate::worldplay::{
    InteractionHint, LootTake, MoralPrompt, PlayerNeeds, ScavengeSession, ScavengeState,
    WorldAction, WorldNotice,
};

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
    Journal,
    Scavenge,
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
pub(super) enum View {
    Inventory,
    Shelter,
    Trade,
    Journal,
    Moral,
    Scavenge,
    /// A conversation card from [`crate::dialogue::Dialogue`].
    Talk,
}

#[derive(Resource, Default)]
pub(super) struct PanelState {
    view: Option<View>,
    page: usize,
}

#[derive(Component)]
pub(super) struct SurvivalUiRoot;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Action {
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
    Loot(LootTake),
    Barricade,
    /// An option of the open conversation card, by index.
    Talk(usize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tone {
    #[default]
    Normal,
    Heading,
    Muted,
    Selected,
    Badge(ItemKind),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Widget {
    pub(super) rect: ScreenRect,
    pub(super) text: String,
    pub(super) action: Option<Action>,
    pub(super) enabled: bool,
    font_size: f32,
    tone: Tone,
}

#[derive(Clone, Default, Debug, PartialEq)]
pub(super) struct Frame {
    pub(super) panel: Option<ScreenRect>,
    pub(super) widgets: Vec<Widget>,
    pub(super) cards: Vec<(ScreenRect, bool)>,
}

impl Frame {
    pub(super) fn text(&mut self, rect: ScreenRect, text: String, font_size: f32) {
        self.widgets.push(Widget {
            rect,
            text,
            action: None,
            enabled: true,
            font_size,
            tone: Tone::Normal,
        });
    }

    pub(super) fn button(&mut self, rect: ScreenRect, text: String, action: Action, enabled: bool) {
        self.widgets.push(Widget {
            rect,
            text,
            action: Some(action),
            enabled,
            font_size: 15.0,
            tone: Tone::Normal,
        });
    }

    pub(super) fn tone(&mut self, tone: Tone) {
        if let Some(widget) = self.widgets.last_mut() {
            widget.tone = tone;
        }
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
    item_kind: Option<ItemKind>,
    equipped: bool,
}

impl Row {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            buttons: Vec::new(),
            item_kind: None,
            equipped: false,
        }
    }

    fn action(text: impl Into<String>, label: &str, action: Action, enabled: bool) -> Self {
        Self {
            text: text.into(),
            buttons: vec![(label.to_owned(), action, enabled)],
            item_kind: None,
            equipped: false,
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

pub(super) fn quick_action_rects(viewport: Vec2) -> [ScreenRect; 2] {
    let y = toolbar_rects(viewport)[0].min.y - 38.0;
    let cx = viewport.x * 0.5;
    [
        ScreenRect::new(cx - 69.0, y, 66.0, 34.0),
        ScreenRect::new(cx + 3.0, y, 66.0, 34.0),
    ]
}

pub(super) fn quick_action_hit(viewport: Vec2, point: Vec2, inventory: Option<&Inventory>) -> bool {
    let Some(inventory) = inventory else {
        return false;
    };
    let rects = quick_action_rects(viewport);
    ((inventory.count("grenade") > 0 || inventory.count("oldgrenade") > 0)
        && rects[0].contains(point))
        || (inventory.count("jpradio") > 0 && rects[1].contains(point))
}

pub(super) fn panel_rect(viewport: Vec2) -> ScreenRect {
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

pub(super) fn item_category(kind: ItemKind, lang: Language) -> &'static str {
    match kind {
        ItemKind::Food => tr(lang, "食物", "Food"),
        ItemKind::Drink => tr(lang, "飲水", "Drink"),
        ItemKind::Med => tr(lang, "醫療", "Medical"),
        ItemKind::Melee => tr(lang, "近戰", "Melee"),
        ItemKind::Gun => tr(lang, "槍械", "Firearm"),
        ItemKind::Ammo => tr(lang, "彈藥", "Ammunition"),
        ItemKind::Tool => tr(lang, "工具", "Tools"),
    }
}

fn item_badge(kind: ItemKind, lang: Language) -> &'static str {
    match kind {
        ItemKind::Food => tr(lang, "食", "FD"),
        ItemKind::Drink => tr(lang, "飲", "H2O"),
        ItemKind::Med => tr(lang, "醫", "+"),
        ItemKind::Melee => tr(lang, "刃", "ML"),
        ItemKind::Gun => tr(lang, "槍", "GN"),
        ItemKind::Ammo => tr(lang, "彈", "AM"),
        ItemKind::Tool => tr(lang, "具", "TL"),
    }
}

fn item_color(kind: ItemKind) -> Color {
    match kind {
        ItemKind::Food => Color::srgb(0.79, 0.58, 0.29),
        ItemKind::Drink => Color::srgb(0.43, 0.66, 0.83),
        ItemKind::Med => Color::srgb(0.62, 0.75, 0.56),
        ItemKind::Melee | ItemKind::Gun => Color::srgb(0.83, 0.62, 0.49),
        ItemKind::Ammo => Color::srgb(0.79, 0.71, 0.46),
        ItemKind::Tool => Color::srgb(0.65, 0.69, 0.70),
    }
}

/// Fired timeline IDs already belong to the atomic save. Derive the original
/// report text from those IDs, never reveal later broadcasts or fabricate a
/// reception timestamp; the displayed time is the report's scheduled time.
fn journal_rows(campaign: &Campaign, shelter: &Shelter, lang: Language) -> Vec<Row> {
    let mut rows = vec![Row::text(tr(
        lang,
        "收音機與通報",
        "Radio and public notices",
    ))];
    let mut received = false;
    for line in TIMELINE
        .iter()
        .rev()
        .filter(|line| campaign.fired.iter().any(|id| id == line.id))
    {
        received = true;
        rows.push(Row::text(format!(
            "{} {} · {:02}:{:02} · {}\n{}",
            tr(lang, "第", "Day"),
            line.minute / 1440 + 1,
            line.minute / 60 % 24,
            line.minute % 60,
            line.who,
            line.text
        )));
    }
    if !received {
        rows.push(Row::text(tr(
            lang,
            "還沒有收到通報。",
            "No broadcasts received yet.",
        )));
    }
    rows.push(Row::text(tr(
        lang,
        "避難所每日紀錄",
        "Daily shelter record",
    )));
    if shelter.log.is_empty() {
        rows.push(Row::text(tr(
            lang,
            "每天清晨六點結算，紀錄會留在這裡。",
            "Daily records appear here after the 06:00 settlement.",
        )));
    } else {
        rows.extend(shelter.log.iter().map(|line| Row::text(line.clone())));
    }
    rows
}

fn apply_binding_labels(
    frame: &mut Frame,
    controls: Option<&crate::input::ControlPreferences>,
    language: Language,
) {
    if frame.panel.is_some() {
        return;
    }
    let controls = controls.unwrap_or_else(|| crate::input::ControlPreferences::default_ref());
    for widget in &mut frame.widgets {
        let (label, action) = match widget.action.as_ref() {
            Some(Action::Open(View::Inventory)) => (
                tr(language, "背包", "Bag"),
                crate::input::ControlAction::Inventory,
            ),
            Some(Action::Open(View::Shelter)) => (
                tr(language, "避難所", "Shelter"),
                crate::input::ControlAction::Shelter,
            ),
            Some(Action::Open(View::Trade)) => (
                tr(language, "交易", "Trade"),
                crate::input::ControlAction::Trade,
            ),
            Some(Action::Interact) => (
                widget
                    .text
                    .strip_suffix(" E")
                    .unwrap_or(tr(language, "互動", "Use")),
                crate::input::ControlAction::Interact,
            ),
            _ => continue,
        };
        widget.text = format!("{label} {}", controls.key_label(action));
    }
}

fn apply_interaction_label(frame: &mut Frame, hint: Option<&InteractionHint>) {
    if let Some(hint) = hint.filter(|hint| !hint.label.is_empty()) {
        for widget in &mut frame.widgets {
            if widget.action == Some(Action::Interact) {
                widget.text = format!("{} E", hint.label);
                widget.font_size = 13.0;
            }
        }
    }
}

fn widget_colors(widget: &Widget, panel: bool) -> (Color, Color, Color) {
    let ink = Color::srgb(0.91, 0.89, 0.82);
    let gold = Color::srgb(0.91, 0.77, 0.47);
    if let Tone::Badge(kind) = widget.tone {
        return (
            Color::srgb(0.065, 0.078, 0.083),
            Color::srgba(0.37, 0.40, 0.38, 0.7),
            item_color(kind),
        );
    }
    if widget.action.is_some() {
        if widget.tone == Tone::Selected {
            return (
                Color::srgb(0.24, 0.22, 0.15),
                Color::srgb(0.68, 0.56, 0.31),
                gold,
            );
        }
        if !widget.enabled {
            return (
                Color::srgb(0.075, 0.087, 0.09),
                Color::srgba(0.29, 0.31, 0.29, 0.45),
                Color::srgb(0.44, 0.47, 0.44),
            );
        }
        return (
            Color::srgb(0.19, 0.20, 0.17),
            Color::srgba(0.56, 0.49, 0.31, 0.75),
            ink,
        );
    }
    let bg = if panel {
        Color::NONE
    } else {
        Color::srgba(0.025, 0.03, 0.035, 0.90)
    };
    let text = match widget.tone {
        Tone::Heading => gold,
        Tone::Muted => Color::srgb(0.67, 0.72, 0.67),
        _ => ink,
    };
    (bg, Color::NONE, text)
}

fn inventory_rows(
    inventory: &Inventory,
    campaign: &Campaign,
    lang: Language,
    door: Option<bool>,
) -> Vec<Row> {
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
    if let Some(barricaded) = door {
        rows.push(Row::action(
            tr(
                lang,
                if barricaded {
                    "附近入口已封住。拆除可取回一塊木板。"
                } else {
                    "封住附近入口：需要槌子與木板 ×1，敲打會引起注意。"
                },
                if barricaded {
                    "Nearby entrance is barricaded. Remove it to recover one plank."
                } else {
                    "Barricade nearby entrance: needs a hammer and 1 plank. Hammering makes noise."
                },
            ),
            tr(
                lang,
                if barricaded {
                    "拆木板 B"
                } else {
                    "封門 B"
                },
                if barricaded { "Remove B" } else { "Board B" },
            ),
            Action::Barricade,
            barricaded || (inventory.count("hammer") > 0 && inventory.count("plank") > 0),
        ));
    }
    rows.push(Row {
        text: tr(lang, "時間速度", "Time speed").to_owned(),
        item_kind: None,
        equipped: false,
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
            "{} ×{}{}\n{} · {:.2} kg\n{}",
            definition.name(lang),
            count,
            if equipped {
                tr(lang, " · 已裝備", " · Equipped")
            } else {
                ""
            },
            item_category(definition.kind, lang),
            definition.weight * *count as f32,
            definition.description(lang)
        );
        let mut row = if usable {
            Row::action(text, label, Action::Use(id.clone()), true)
        } else {
            Row::text(text)
        };
        row.item_kind = Some(definition.kind);
        row.equipped = equipped;
        rows.push(row);
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
            item_kind: item(id).map(|d| d.kind),
            equipped: false,
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
    let badge_w = if row.item_kind.is_some() { 42.0 } else { 0.0 };
    let available = (width - row.buttons.len() as f32 * 77.0 - badge_w - 16.0).max(60.0);
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
        View::Scavenge => tr(lang, "搜刮", "Scavenge"),
        View::Talk => tr(lang, "交談", "Talk"),
    }
}

#[derive(Default)]
struct PanelExtras<'a> {
    loot: Option<(&'a ScavengeSession, &'a ScavengeState)>,
    door: Option<bool>,
    talk: Option<&'a crate::dialogue::DialogueCard>,
}

fn talk_rows(card: Option<&crate::dialogue::DialogueCard>) -> Vec<Row> {
    let Some(card) = card else {
        return Vec::new();
    };
    let mut rows = vec![Row::text(card.body.clone())];
    rows.extend(card.options.iter().enumerate().map(|(i, option)| {
        Row::action(
            option.detail.clone(),
            &option.label,
            Action::Talk(i),
            option.enabled,
        )
    }));
    rows
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
    extras: PanelExtras,
) -> Frame {
    if state.view == Some(View::Scavenge) {
        return super::scavenge::make_frame(
            viewport,
            state.page,
            extras.loot,
            inventory,
            lang,
            notice,
            save,
        );
    }
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
                tr(lang, "避難所 H", "Shelter H"),
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
        let quick = quick_action_rects(viewport);
        if inventory.count("grenade") > 0 || inventory.count("oldgrenade") > 0 {
            let gren_id = if inventory.count("grenade") > 0 {
                "grenade"
            } else {
                "oldgrenade"
            };
            frame.button(
                quick[0],
                tr(lang, "投彈", "Frag").to_owned(),
                Action::Use(gren_id.to_owned()),
                true,
            );
        }
        if inventory.count("jpradio") > 0 {
            frame.button(
                quick[1],
                tr(lang, "無線電", "Radio").to_owned(),
                Action::Use("jpradio".to_owned()),
                true,
            );
        }
        // Gameplay notices share the HUD context slot and expire there. A fixed
        // screen-centre toast obscured the radio card on short portrait phones.
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
        View::Talk => extras.talk.map_or_else(
            || view_name(view, lang).to_string(),
            |card| card.title.clone(),
        ),
        _ => view_name(view, lang).to_string(),
    };
    frame.text(
        ScreenRect::new(x, header_y, width - 64.0, 42.0),
        title,
        23.0,
    );
    frame.tone(Tone::Heading);
    frame.button(
        ScreenRect::new(x + width - 64.0, header_y, 64.0, 42.0),
        tr(lang, "關閉", "Close").to_owned(),
        Action::Close,
        true,
    );

    let mut y = header_y + 50.0;
    if view != View::Moral && view != View::Talk {
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
            if view == tab {
                frame.tone(Tone::Selected);
            }
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
            "已收到的通報與每日紀錄。開啟面板時，世界仍在運作。",
            "Received broadcasts and daily records. The world keeps running while this panel is open.",
        )
        .to_string(),
        View::Moral | View::Scavenge | View::Talk => String::new(),
    };
    if !summary.is_empty() {
        frame.text(ScreenRect::new(x, y, width, 44.0), summary, 14.0);
        frame.tone(Tone::Muted);
        y += 48.0;
    }
    let footer_y = panel.max.y - 52.0;
    let status_y = footer_y - 42.0;
    let available_h = (status_y - y - 4.0).max(42.0);
    let rows = match view {
        View::Inventory => inventory_rows(inventory, campaign, lang, extras.door),
        View::Shelter => shelter_rows(shelter, inventory, pos, lang),
        View::Trade => trade_rows(shelter, inventory, pos, lang),
        View::Moral => moral_rows(inventory, lang),
        View::Talk => talk_rows(extras.talk),
        View::Journal => journal_rows(campaign, shelter, lang),
        View::Scavenge => Vec::new(),
    };
    let pages = pages(rows, width, available_h);
    let page = state.page.min(pages.len() - 1);
    for (row, height) in &pages[page] {
        let action_width = row.buttons.len() as f32 * 77.0;
        frame.cards.push((
            ScreenRect::new(x, y + 1.0, width, *height - 5.0),
            row.equipped,
        ));
        let badge_w = if row.item_kind.is_some() { 42.0 } else { 0.0 };
        if let Some(kind) = row.item_kind {
            frame.text(
                ScreenRect::new(x + 5.0, y + 8.0, 32.0, 32.0),
                item_badge(kind, lang).to_owned(),
                13.0,
            );
            frame.tone(Tone::Badge(kind));
        }
        let text_x = x + badge_w + 3.0;
        let text_w = (width - action_width - badge_w - 10.0).max(1.0);
        if let Some((title, detail)) = row.text.split_once('\n') {
            // The source inventory distinguishes names from metadata. Keep the
            // whole text in the pagination estimate, then style the two levels.
            let title_h = if title.chars().count() as f32 * 15.0 > text_w {
                40.0
            } else {
                23.0
            };
            frame.text(
                ScreenRect::new(text_x, y + 3.0, text_w, title_h),
                title.to_owned(),
                14.0,
            );
            frame.text(
                ScreenRect::new(
                    text_x,
                    y + 3.0 + title_h,
                    text_w,
                    (*height - title_h - 10.0).max(16.0),
                ),
                detail.to_owned(),
                12.0,
            );
            frame.tone(Tone::Muted);
        } else {
            frame.text(
                ScreenRect::new(text_x, y + 1.0, text_w, *height - 5.0),
                row.text.clone(),
                15.0,
            );
        }
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
    frame.tone(Tone::Muted);
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
        view != View::Moral && view != View::Talk,
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
    dialogue: ResMut<'w, crate::dialogue::Dialogue>,
    lang: Res<'w, Language>,
    notice: ResMut<'w, WorldNotice>,
    save_status: Res<'w, SaveStatus>,
    hint: Option<Res<'w, InteractionHint>>,
    geometry: Option<Res<'w, crate::hud::HudGeometry>>,
    scavenge: ResMut<'w, ScavengeSession>,
    progress: Option<Res<'w, ScavengeState>>,
    obstacles: Option<Res<'w, crate::map::Obstacles>>,
}

pub(super) fn drive_panels(
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    controls: Option<Res<crate::input::ControlPreferences>>,
    consumed: Res<ConsumedPresses>,
    mut state: ResMut<PanelState>,
    mut modal: ResMut<UiModal>,
    world: PanelWorld,
    mut opens: MessageReader<OpenPanel>,
    mut world_actions: MessageWriter<WorldAction>,
    mut saves: MessageWriter<SaveGame>,
    mut pauses: MessageWriter<TogglePause>,
    map: Option<Res<crate::worldmap::MapOpen>>,
    mut sound: MessageWriter<SfxEvent>,
    mut talk_choices: MessageWriter<crate::dialogue::DialogueChosen>,
) {
    let PanelWorld {
        player,
        mut shelter,
        mut inventory,
        campaign,
        needs,
        prompt,
        mut dialogue,
        lang,
        mut notice,
        save_status,
        hint,
        geometry,
        mut scavenge,
        progress,
        obstacles,
    } = world;
    let previous_view = state.view;
    if flow.get() != &GameFlow::Playing || pause.0 {
        state.view = None;
        scavenge.target = None;
        modal.open = false;
        opens.clear();
        return;
    }
    if map.as_ref().is_some_and(|m| m.0) && opens.is_empty() {
        state.view = None;
        scavenge.target = None;
        modal.open = true;
        return;
    }
    for open in opens.read() {
        state.view = match open.0 {
            PanelKind::Inventory => Some(View::Inventory),
            PanelKind::Shelter => Some(View::Shelter),
            PanelKind::Trade => Some(View::Trade),
            PanelKind::Journal => Some(View::Journal),
            PanelKind::Scavenge => scavenge.target.as_ref().map(|_| View::Scavenge),
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
    if prompt.0.is_none() {
        if dialogue.0.is_some() && state.view != Some(View::Talk) {
            state.view = Some(View::Talk);
            state.page = 0;
            scavenge.target = None;
        } else if dialogue.0.is_none() && state.view == Some(View::Talk) {
            state.view = None;
        }
    }
    let viewport = windows
        .iter()
        .next()
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    let pos = position(&player);
    let mut frame = make_frame(
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
        PanelExtras {
            loot: progress.as_deref().map(|progress| (&*scavenge, progress)),
            door: obstacles.as_deref().and_then(|obstacles| {
                crate::doors::nearest_door(Vec2::from_array(pos), obstacles)
                    .map(|door| obstacles.doors().is_barricaded(door.id))
            }),
            talk: dialogue.0.as_ref(),
        },
    );
    apply_interaction_label(&mut frame, hint.as_deref());
    apply_binding_labels(&mut frame, controls.as_deref(), *lang);
    let mut action = consumed.points.iter().find_map(|point| {
        frame.action_at(*point).or_else(|| {
            (state.view.is_none()
                && campaign.notice_t > 0.0
                && geometry
                    .as_ref()
                    .and_then(|g| g.banner)
                    .is_some_and(|rect| rect.contains(*point)))
            .then_some(Action::Open(View::Journal))
        })
    });
    if state.view.is_some() && keys.just_pressed(KeyCode::Escape) {
        keys.clear_just_pressed(KeyCode::Escape);
        action = Some(Action::Close);
    } else if state.view != Some(View::Moral) && state.view != Some(View::Talk) {
        let controls = controls
            .as_deref()
            .unwrap_or_else(|| crate::input::ControlPreferences::default_ref());
        for (action_key, view) in [
            (crate::input::ControlAction::Inventory, View::Inventory),
            (crate::input::ControlAction::Shelter, View::Shelter),
            (crate::input::ControlAction::Trade, View::Trade),
        ] {
            if controls.just_pressed(&keys, action_key) {
                controls.clear_just_pressed(&mut keys, action_key);
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
            sound.write(SfxEvent::Ui { confirm: false });
        }
        Some(Action::Close) => {
            if state.view == Some(View::Moral) {
                world_actions.write(WorldAction::Moral(MoralChoice::Leave));
            }
            if state.view == Some(View::Talk) {
                dialogue.0 = None;
            }
            state.view = None;
            sound.write(SfxEvent::Ui { confirm: false });
        }
        Some(Action::Page(page)) => {
            if state.page != page {
                sound.write(SfxEvent::Ui { confirm: false });
            }
            state.page = page;
        }
        Some(Action::Deposit) => {
            notice.text = match shelter.deposit(&mut inventory, pos) {
                Ok(count) => {
                    if count > 0 {
                        sound.write(SfxEvent::Pickup);
                    }
                    tr(
                        *lang,
                        "物資放進了存糧。",
                        "Supplies added to the stockpile.",
                    )
                    .to_owned()
                }
                Err(error) => error.message(*lang).to_owned(),
            }
        }
        Some(Action::Pray) => {
            let before = shelter.pray_day;
            notice.text = match shelter.pray_at(pos, *lang) {
                Ok(message) => {
                    if shelter.pray_day != before {
                        sound.write(SfxEvent::Ui { confirm: true });
                    }
                    message
                }
                Err(error) => error.message(*lang).to_owned(),
            }
        }
        Some(Action::Build(upgrade)) => {
            notice.text = match shelter.build(upgrade, &mut inventory, pos) {
                Ok(()) => {
                    sound.write(SfxEvent::Build);
                    format!("{}: {}", upgrade.name(*lang), tr(*lang, "完成", "complete"))
                }
                Err(error) => error.message(*lang).to_owned(),
            }
        }
        Some(Action::Buy(id)) | Some(Action::Sell(id)) => {
            notice.text = match shelter.barter(&mut inventory, &id, buying, pos) {
                Ok(()) => {
                    sound.write(SfxEvent::Ui { confirm: true });
                    format!(
                        "{} {}",
                        tr(*lang, "交易完成：", "Traded:"),
                        item_name(&id, *lang)
                    )
                }
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
        Some(Action::Talk(index)) => {
            if let Some(option) = dialogue
                .0
                .take()
                .and_then(|card| card.options.get(index).cloned())
                .filter(|option| option.enabled)
            {
                talk_choices.write(crate::dialogue::DialogueChosen(option.key));
                sound.write(SfxEvent::Ui { confirm: true });
            }
            state.view = None;
        }
        Some(Action::Loot(take)) => {
            if let Some(target) = &scavenge.target {
                world_actions.write(WorldAction::TakeLoot {
                    container: target.id.clone(),
                    take,
                });
            }
        }
        Some(Action::Barricade) => {
            world_actions.write(WorldAction::Barricade);
        }
        None => {}
    }
    if state.view != Some(View::Scavenge) {
        scavenge.target = None;
    }
    modal.open = state.view.is_some();
    if previous_view != state.view {
        println!("DTUI panel={:?}", state.view);
    }
}

#[derive(SystemParam)]
pub(super) struct PanelRenderWorld<'w> {
    shelter: Res<'w, Shelter>,
    inventory: Res<'w, Inventory>,
    campaign: Res<'w, Campaign>,
    needs: Res<'w, PlayerNeeds>,
    lang: Res<'w, Language>,
    notice: Res<'w, WorldNotice>,
    save: Res<'w, SaveStatus>,
    font: Res<'w, UiFont>,
    map: Option<Res<'w, crate::worldmap::MapOpen>>,
    hint: Option<Res<'w, InteractionHint>>,
    scavenge: Res<'w, ScavengeSession>,
    progress: Option<Res<'w, ScavengeState>>,
    obstacles: Option<Res<'w, crate::map::Obstacles>>,
    controls: Option<Res<'w, crate::input::ControlPreferences>>,
    dialogue: Option<Res<'w, crate::dialogue::Dialogue>>,
}

pub(super) fn render_panels(
    mut commands: Commands,
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    windows: Query<&Window, With<PrimaryWindow>>,
    player: Query<&Transform, With<Player>>,
    state: Res<PanelState>,
    world: PanelRenderWorld,
    roots: Query<Entity, With<SurvivalUiRoot>>,
    mut cache: ResMut<RenderCache>,
) {
    let PanelRenderWorld {
        shelter,
        inventory,
        campaign,
        needs,
        lang,
        notice,
        save,
        font,
        map,
        hint,
        scavenge,
        progress,
        obstacles,
        controls,
        dialogue,
    } = world;
    let mut frame =
        if flow.get() == &GameFlow::Playing && !pause.0 && !map.as_ref().is_some_and(|m| m.0) {
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
                PanelExtras {
                    loot: progress.as_deref().map(|progress| (&*scavenge, progress)),
                    door: obstacles.as_deref().and_then(|obstacles| {
                        crate::doors::nearest_door(Vec2::from_array(position(&player)), obstacles)
                            .map(|door| obstacles.doors().is_barricaded(door.id))
                    }),
                    talk: dialogue.as_deref().and_then(|d| d.0.as_ref()),
                },
            )
        } else {
            Frame::default()
        };
    apply_interaction_label(&mut frame, hint.as_deref());
    apply_binding_labels(&mut frame, controls.as_deref(), *lang);
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
    let viewport = windows
        .iter()
        .next()
        .map_or(Vec2::new(1280.0, 720.0), |window| window.size());
    let buttons: Vec<_> = frame.widgets.iter().filter_map(|widget| widget.action.as_ref().map(|action| serde_json::json!({"action":format!("{action:?}"),"rect":[widget.rect.min.x,widget.rect.min.y,widget.rect.max.x,widget.rect.max.y],"enabled":widget.enabled}))).collect();
    println!(
        "DTPANEL layout={}",
        serde_json::json!({"viewport":[viewport.x,viewport.y],"view":format!("{:?}",state.view),"buttons":buttons})
    );
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
            for (rect, selected) in frame.cards {
                parent.spawn((
                    node_at(rect),
                    BackgroundColor(if selected {
                        Color::srgb(0.20, 0.20, 0.15)
                    } else {
                        Color::srgb(0.105, 0.122, 0.13)
                    }),
                    BorderColor::all(if selected {
                        Color::srgb(0.62, 0.51, 0.29)
                    } else {
                        Color::srgba(0.24, 0.27, 0.26, 0.55)
                    }),
                    Pickable::IGNORE,
                ));
            }
            for widget in frame.widgets {
                let button = widget.action.is_some();
                let (bg, border, text_color) = widget_colors(&widget, frame.panel.is_some());
                parent.spawn((
                    node_at(widget.rect),
                    BackgroundColor(bg),
                    BorderColor::all(border),
                    Text::new(widget.text),
                    TextFont::from_font_size(FontSize::Px(widget.font_size))
                        .with_font(font.0.clone()),
                    TextColor(text_color),
                    TextLayout::justify(if button || matches!(widget.tone, Tone::Badge(_)) {
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
                            PanelExtras::default(),
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
        use bevy::ecs::message::{MessageCursor, Messages};
        use bevy::state::app::StatesPlugin;

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(GameFlow::Playing)
            .init_resource::<PauseOpen>()
            .init_resource::<ScavengeSession>()
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
            .add_message::<SfxEvent>()
            .init_resource::<crate::dialogue::Dialogue>()
            .add_message::<crate::dialogue::DialogueChosen>()
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
            PanelExtras::default(),
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
        let mut sounds = MessageCursor::<SfxEvent>::default();
        app.update();
        let emitted: Vec<_> = sounds
            .read(app.world().resource::<Messages<SfxEvent>>())
            .copied()
            .collect();
        assert!(matches!(emitted.as_slice(), [SfxEvent::Pickup]));
        assert_eq!(app.world().resource::<Inventory>().count("rice"), 0);
        assert_eq!(app.world().resource::<Inventory>().count("plank"), 2);
        assert_eq!(app.world().resource::<Shelter>().stash.count("rice"), 5);
        assert!(app.world().resource::<UiModal>().open);
        assert!(!app.world().resource::<PauseOpen>().0);
        app.update();
        assert_eq!(
            sounds
                .read(app.world().resource::<Messages<SfxEvent>>())
                .count(),
            0,
            "empty deposit cannot replay the success sound"
        );
    }
    #[test]
    fn journal_only_shows_received_reports_and_survives_campaign_roundtrip() {
        let mut campaign = Campaign {
            game_min: 20_000.0,
            ..default()
        };
        campaign.fired.push("a".to_owned());
        let received = TIMELINE.iter().find(|line| line.id == "a").unwrap();
        let future = TIMELINE.iter().find(|line| line.id == "e1").unwrap();
        let shelter = Shelter {
            log: vec!["Daily shelter entry".to_owned()],
            ..default()
        };
        let copy = journal_rows(&campaign, &shelter, Language::En)
            .into_iter()
            .map(|row| row.text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(copy.contains(received.text));
        assert!(copy.contains("08:05"));
        assert!(copy.contains("Daily shelter entry"));
        assert!(!copy.contains(future.text));
        let restored: Campaign =
            serde_json::from_slice(&serde_json::to_vec(&campaign).unwrap()).unwrap();
        let restored_copy = journal_rows(&restored, &shelter, Language::En)
            .into_iter()
            .map(|row| row.text)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(copy, restored_copy);
    }

    #[test]
    fn equipped_item_card_and_context_action_use_live_inventory_and_hint() {
        let mut inventory = Inventory::from_role(&crate::roles::ROLES[0]);
        inventory.add("knife", 1);
        inventory.equipped = "knife".to_owned();
        let rows = inventory_rows(&inventory, &Campaign::default(), Language::En, None);
        let weapon = rows.iter().find(|row| row.equipped).unwrap();
        assert_eq!(weapon.item_kind, Some(ItemKind::Melee));
        assert!(weapon.text.contains("Equipped"));
        assert_eq!(weapon.buttons[0].1, Action::Use("knife".to_owned()));
        let mut frame = Frame::default();
        let rect = toolbar_rects(Vec2::new(390.0, 844.0))[3];
        frame.button(rect, "Use E".to_owned(), Action::Interact, true);
        apply_interaction_label(
            &mut frame,
            Some(&InteractionHint {
                label: "Open door".to_owned(),
                detail: String::new(),
            }),
        );
        assert_eq!(frame.widgets[0].text, "Open door E");
        assert_eq!(frame.action_at(rect.center()), Some(Action::Interact));
    }

    #[test]
    fn android_barricade_smoke_point_hits_the_live_inventory_action() {
        let mut inventory = Inventory::default();
        inventory.add("hammer", 1);
        inventory.add("plank", 2);
        for barricaded in [false, true] {
            let frame = make_frame(
                Vec2::new(390.0, 844.0),
                &PanelState {
                    view: Some(View::Inventory),
                    page: 0,
                },
                &Shelter::default(),
                &inventory,
                &Campaign::default(),
                &PlayerNeeds::default(),
                [0.0, 0.0],
                Language::ZhTw,
                &WorldNotice::default(),
                &SaveStatus::default(),
                PanelExtras {
                    door: Some(barricaded),
                    ..default()
                },
            );
            let button = frame
                .widgets
                .iter()
                .find(|widget| widget.action == Some(Action::Barricade))
                .unwrap();
            println!("barricade {barricaded}: {:?}", button.rect.center());
            assert!(button.rect.contains(Vec2::new(323.0, 279.0)));
            assert_eq!(
                frame.action_at(Vec2::new(323.0, 279.0)),
                Some(Action::Barricade)
            );
        }
    }
}
