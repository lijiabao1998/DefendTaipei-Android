//! Data from the web build's `src/story`: act titles, the act-1 events, the
//! act-4 all-clear, 北廣 AM765 scripts, and the prologue phone pushes.
//!
//! Chinese copy is the original text. The catalog is not a scheduler. The web
//! build's own notes say acts 2 and 3 have no events, and nothing in the catalog
//! decides when an event fires. [`RunStoryEvent`] is the manual hook
//! (`window.__game.storyEvent` in the original).

use bevy::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoryEmit {
    Broadcast(&'static str),
    AlertTest,
    Alert,
    AlertClear,
}

#[allow(dead_code)]
pub struct StoryEvent {
    pub id: &'static str,
    pub act: u8,
    pub text: &'static str,
    pub ending: bool,
    pub emits: &'static [StoryEmit],
}

pub struct RadioScript {
    pub id: &'static str,
    pub text: &'static str,
}

pub const AM765_STATION: &str = "📻 北廣 AM765（北都廣播電台）";

pub const AM765: &[RadioScript] = &[
    RadioScript {
        id: "am765_drill_01",
        text: "這裡是北廣 AM765。國防單位表示，解放軍今日在台灣周邊海空域進行演習，請市民保持冷靜，留意後續消息。",
    },
    RadioScript {
        id: "am765_all_clear",
        text: "這裡是北廣 AM765。台北市已解除警報，請市民依照指示返家，注意路上安全。",
    },
];

/// Prologue phone pushes: seconds into the commute, sender, text. From `prologue.ts`.
pub const PRO_NEWS: &[(f32, &str, &str)] = &[
    (3.0, "📱 早安推播", "今日台北：多雲 26°C，午後局部陣雨。"),
    (
        12.0,
        "📱 新聞快訊",
        "國防部：共軍於台海周邊實施「聯合演習」，政府呼籲民眾照常上班上課。",
    ),
    (
        24.0,
        "📱 交通",
        "北都地下鐵正常營運；西門町周邊道路車多請改道。",
    ),
    (
        38.0,
        "📱 新聞快訊",
        "多處網站遭受大規模網路攻擊，部分銀行 App 暫停服務。",
    ),
];

const ACT1: &[StoryEvent] = &[
    StoryEvent {
        id: "act1_drill_news",
        act: 1,
        text: "收音機傳來消息：解放軍今天又在台灣周邊展開演習。",
        ending: false,
        emits: &[StoryEmit::Broadcast("am765_drill_01")],
    },
    StoryEvent {
        id: "act1_panic_buying",
        act: 1,
        text: "萬華的超市門口排起長龍，泡麵和瓶裝水的貨架空了一半。",
        ending: false,
        emits: &[],
    },
    StoryEvent {
        id: "act1_siren_test",
        act: 1,
        text: "整點的防空警報測試響起，路人只是抬頭看了一眼。",
        ending: false,
        emits: &[StoryEmit::AlertTest],
    },
];

pub const ALL_CLEAR_ID: &str = "ending_all_clear";

const ACT4: &[StoryEvent] = &[StoryEvent {
    id: ALL_CLEAR_ID,
    act: 4,
    text: "長長一聲警報響過，這次是解除警報。街上的人慢慢走出地下室。",
    ending: true,
    emits: &[
        StoryEmit::AlertClear,
        StoryEmit::Broadcast("am765_all_clear"),
    ],
}];

#[allow(dead_code)]
pub const STORY_ACTS: &[(u8, &str)] = &[(1, "前兆"), (2, "警報"), (3, "圍城"), (4, "結局")];

pub fn story_events() -> impl Iterator<Item = &'static StoryEvent> {
    ACT1.iter().chain(ACT4.iter())
}

pub fn event_by_id(id: &str) -> Option<&'static StoryEvent> {
    story_events().find(|event| event.id == id)
}

pub fn script_by_id(id: &str) -> Option<&'static RadioScript> {
    AM765.iter().find(|script| script.id == id)
}

/// The fifteen 萬華 areas that have to be taken back. Neighbouring districts are not part of this count.
pub const WANHUA_DISTRICTS: &[&str] = &[
    "西門町",
    "中華路",
    "康定路",
    "龍山寺",
    "貴陽街",
    "華西街",
    "西園路",
    "華江",
    "東園",
    "萬大路",
    "南機場",
    "青年公園",
    "加蚋仔",
    "馬場町",
    "雙園",
];

/// UI or debug asks the runner to fire one catalog id. A repeat does nothing.
#[derive(Message, Clone, Debug)]
pub struct RunStoryEvent {
    pub id: String,
}

/// `alert` bus: test chime, 萬安 siren, or 解除警報.
#[allow(dead_code)]
#[derive(Message, Clone, Copy, Debug)]
pub struct StoryAlert {
    pub kind: AlertKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Test,
    Alert,
    Clear,
}

/// A 北廣 line started. Audio can play the bed; UI can show `text`.
#[allow(dead_code)]
#[derive(Message, Clone, Copy, Debug)]
pub struct StoryBroadcast {
    pub id: &'static str,
    pub station: &'static str,
    pub text: &'static str,
}

/// Map reports one of [`WANHUA_DISTRICTS`] taken back.
#[derive(Message, Clone, Debug)]
pub struct DistrictReclaimed {
    pub name: String,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvacRoute {
    Helicopter,
    Boat,
}

/// Extraction cinematic finished. The original offers helicopter or boat to the carrier.
#[derive(Message, Clone, Copy, Debug)]
pub struct EvacComplete {
    pub route: EvacRoute,
}
