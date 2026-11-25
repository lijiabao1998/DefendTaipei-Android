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

/// One line from `main.ts` `STORY`, in in-game minutes from 00:00 on day 1.
pub struct TimelineLine {
    pub minute: u32,
    pub id: &'static str,
    pub who: &'static str,
    pub text: &'static str,
    /// `who` starts with 📻 in the original, so it also plays the radio bed.
    pub radio: bool,
}

/// `startPrologue` sets the clock to 07:20.
pub const PROLOGUE_START_MIN: f32 = 7.0 * 60.0 + 20.0;

/// Stand-in day length until the survival-mode hour picker exists.
/// One in-game hour passes each real second, so the original minute table
/// (07:20 through the day-7 evacuation) finishes in one sitting.
pub const GAME_MINUTES_PER_REAL_SECOND: f32 = 60.0;

/// Shown while the evacuation window is open. The riverside pad and the
/// informant's boat are not on this map yet, so H and B stand in for walking
/// into the zone and meeting the contact.
pub const EVAC_STANDIN: &str = "暫代撤離區（河濱與橡皮艇尚未接入地圖）\nH　直升機・河濱撤離區\nB　小艇・聯絡人：「人都到了？上船，快。」";

/// 第3天 19:00 斷電・第5天 07:00 斷水. Day 7 06:00 through day 8 18:00 is the evac window.
pub const POWER_OFF: u32 = 2 * 1440 + 19 * 60;
pub const WATER_OFF: u32 = 4 * 1440 + 7 * 60;
pub const EVAC_OPEN: u32 = 6 * 1440 + 6 * 60;
pub const EVAC_CLOSE: u32 = 7 * 1440 + 18 * 60;

/// In-game minute when each act-1 catalog event is due. The web build does not schedule these.
pub const CATALOG_AT: &[(u32, &str)] = &[
    (8 * 60, "act1_siren_test"),
    (10 * 60, "act1_drill_news"),
    (18 * 60, "act1_panic_buying"),
];

pub const TIMELINE: &[TimelineLine] = &[
    TimelineLine {
        minute: 8 * 60 + 5,
        id: "a",
        who: "📱 國家級警報",
        text: "萬華、中正一帶實施戒嚴，民眾非必要請勿外出。夜間 20:00 後宵禁。",
        radio: false,
    },
    TimelineLine {
        minute: 12 * 60,
        id: "b",
        who: "📻 北廣 AM765（北都廣播電台）",
        text: "…佔領當局宣稱「水電維持正常供應」，但多處變電所已遭接管…",
        radio: true,
    },
    TimelineLine {
        minute: 1440 + 7 * 60,
        id: "r1",
        who: "📻 地下電台「自由之聲」",
        text: "…這裡是自由之聲。國軍與志願者仍在新店溪南岸堅守，請萬華的鄉親保持冷靜、儲水、避開大馬路…",
        radio: true,
    },
    TimelineLine {
        minute: 1440 + 9 * 60,
        id: "c",
        who: "💬 里長通訊群組",
        text: "各位鄰居：聽說電力公司員工被帶走了，大家趕快把水桶裝滿、手機充飽電！",
        radio: false,
    },
    TimelineLine {
        minute: 2 * 1440 + 14 * 60,
        id: "d",
        who: "📱 電力公司簡訊",
        text: "【停電通知】因「軍事管制」，萬華區即日起停止供電，復電時間未定。",
        radio: false,
    },
    TimelineLine {
        minute: POWER_OFF,
        id: "P",
        who: "⚡ 斷電",
        text: "整條街一瞬間暗了下來。冰箱停了，裡面的食物撐不了多久。",
        radio: false,
    },
    TimelineLine {
        minute: 2 * 1440 + 21 * 60,
        id: "r2",
        who: "📻 地下電台「自由之聲」",
        text: "…停電了？別慌。收音機用電池還能撐。宵禁時佔領軍會帶手電筒巡邏，看到光束就躲進騎樓…",
        radio: true,
    },
    TimelineLine {
        minute: 3 * 1440 + 10 * 60,
        id: "e",
        who: "📻 收音機（電池）",
        text: "…自來水事業處人員已撤離，加壓站停擺，高樓層將最先停水…",
        radio: true,
    },
    TimelineLine {
        minute: 3 * 1440 + 13 * 60,
        id: "r3",
        who: "📻 地下電台「自由之聲」",
        text: "…注意！今天 15:00 前後會有一批物資空投到萬華，紅色煙霧就是信號。佔領軍也會去搶，動作要快！",
        radio: true,
    },
    TimelineLine {
        minute: 3 * 1440 + 15 * 60,
        id: "AD",
        who: "🪂 空投",
        text: "遠處升起一道紅色煙霧——物資箱落地了！佔領軍的巡邏隊正往那邊移動。",
        radio: false,
    },
    TimelineLine {
        minute: WATER_OFF,
        id: "W",
        who: "🚱 停水",
        text: "水龍頭只流出幾滴黃水就停了。從現在起，每一瓶水都很珍貴。",
        radio: false,
    },
    TimelineLine {
        minute: 4 * 1440 + 20 * 60,
        id: "r4",
        who: "📻 地下電台「自由之聲」",
        text: "…停水第一天。把水桶、臉盆放到頂樓或戶外，下雨就能接水。雨水記得煮過再喝…",
        radio: true,
    },
    TimelineLine {
        minute: 5 * 1440 + 20 * 60,
        id: "e0",
        who: "📻 地下電台「自由之聲」",
        text: "…有消息說，盟軍正在河濱準備一條撤離通道。明早請守在收音機旁…",
        radio: true,
    },
    TimelineLine {
        minute: 6 * 1440 + 12 * 60,
        id: "r5",
        who: "📻 地下電台「自由之聲」",
        text: "…西門町一帶已有三個街區被奪回！每一區的勝利，都讓他們更害怕…",
        radio: true,
    },
    TimelineLine {
        minute: EVAC_OPEN,
        id: "e1",
        who: "📻 地下電台「自由之聲」",
        text: "撤離窗口開啟！西側河濱撤離區（綠色 H、紅色信號彈）將有直升機接應，時限至第 8 天 18:00。想留下來奪回家園的人，我們也與你同在。",
        radio: true,
    },
    TimelineLine {
        minute: EVAC_CLOSE,
        id: "e2",
        who: "📻 地下電台「自由之聲」",
        text: "撤離窗口已關閉。留下來的人——台北靠你們奪回。",
        radio: true,
    },
];

pub fn evac_open(game_min: f32, already: bool) -> bool {
    !already && game_min >= EVAC_OPEN as f32 && game_min < EVAC_CLOSE as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_matches_the_original_clock() {
        assert_eq!(PROLOGUE_START_MIN, 440.0);
        assert_eq!(POWER_OFF, 2 * 1440 + 19 * 60);
        assert_eq!(WATER_OFF, 4 * 1440 + 7 * 60);
        assert_eq!(EVAC_OPEN, 6 * 1440 + 6 * 60);
        assert_eq!(EVAC_CLOSE, 7 * 1440 + 18 * 60);
        let open = TIMELINE.iter().find(|line| line.id == "e1").unwrap();
        let shut = TIMELINE.iter().find(|line| line.id == "e2").unwrap();
        assert_eq!(open.minute, EVAC_OPEN);
        assert_eq!(shut.minute, EVAC_CLOSE);
        assert!(open.radio && shut.radio);
        assert!(TIMELINE.iter().any(|line| line.id == "b" && line.radio));
        assert!(TIMELINE.iter().any(|line| line.id == "P" && !line.radio));
        assert!(evac_open(EVAC_OPEN as f32, false));
        assert!(!evac_open(EVAC_CLOSE as f32, false));
        assert!(!evac_open(EVAC_OPEN as f32 + 10.0, true));
    }
}
