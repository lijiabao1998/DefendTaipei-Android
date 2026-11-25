//! Shelter simulation port of web v24.42.0, src/systems/survival.ts.
//! Keep rules independent of UI, rendering, and platform storage.

use bevy::prelude::*;

use crate::game_flow::{Campaign, GameFlow};
use crate::i18n::{Language, tr};

const FOOD: &[&str] = &[
    "can",
    "peanutcan",
    "cookedrice",
    "noodle",
    "rice",
    "cupnoodle",
    "roufong",
    "mantou",
    "cracker",
    "canfruit",
    "driedtofu",
    "snack",
    "bento",
];
const WATER: &[&str] = &["water", "boiled", "rainwater", "tea"];

/// Preserve JavaScript object insertion order: a raid steals the first nonempty stack.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stash(Vec<(String, u32)>);

impl Stash {
    pub fn count(&self, key: &str) -> u32 {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map_or(0, |(_, count)| *count)
    }

    pub fn add(&mut self, key: &str, count: u32) {
        if let Some((_, n)) = self.0.iter_mut().find(|(k, _)| k == key) {
            *n += count;
        } else {
            self.0.push((key.to_owned(), count));
        }
    }

    pub fn take(&mut self, key: &str, count: u32) -> bool {
        if let Some((_, n)) = self.0.iter_mut().find(|(k, _)| k == key)
            && *n >= count
        {
            *n -= count;
            return true;
        }
        false
    }

    pub fn stacks(&self) -> &[(String, u32)] {
        &self.0
    }

    fn consume_first(&mut self, keys: &[&str]) -> bool {
        keys.iter().any(|key| self.take(key, 1))
    }

    fn raid(&mut self) -> Option<bool> {
        let (key, count) = self.0.iter_mut().find(|(_, n)| *n > 0)?;
        *count = count.saturating_sub(2);
        Some(key == "rice")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skill {
    Cook,
    Pray,
    Build,
    Scavenge,
}

#[derive(Debug, Clone)]
pub struct Survivor {
    pub id: &'static str,
    pub skill: Skill,
    pub hunger: f64,
    pub sick: f64,
    pub hurt: f64,
    pub mood: f64,
    pub here: bool,
    pub dead: bool,
}

impl Survivor {
    fn new(id: &'static str, skill: Skill, here: bool) -> Self {
        Self {
            id,
            skill,
            hunger: 70.0,
            sick: 0.0,
            hurt: 0.0,
            mood: 70.0,
            here,
            dead: false,
        }
    }

    pub fn name(&self, lang: Language) -> &'static str {
        match self.id {
            "ama" => tr(lang, "阿嬤", "Grandma"),
            "miao" => tr(lang, "廟公", "Temple keeper"),
            "adi" => tr(lang, "阿迪", "Adi"),
            "mei" => tr(lang, "小美", "Xiaomei"),
            _ => self.id,
        }
    }

    pub fn role(&self, lang: Language) -> &'static str {
        match self.id {
            "ama" => tr(lang, "隔壁的阿嬤（會煮飯）", "Grandma next door (cooking)"),
            "miao" => tr(
                lang,
                "青山宮廟公（會安定人心）",
                "Qingshan Temple keeper (morale)",
            ),
            "adi" => tr(
                lang,
                "印尼移工（會修東西）",
                "Indonesian migrant worker (repairs)",
            ),
            "mei" => tr(
                lang,
                "樓下的鄰居（會找東西）",
                "Downstairs neighbor (scavenging)",
            ),
            _ => self.id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upgrade {
    Boards,
    Beds,
    Stove,
    Rain,
    Planter,
}

impl Upgrade {
    pub const fn cost(self) -> &'static [(&'static str, u32)] {
        match self {
            Self::Boards => &[("plank", 3)],
            Self::Beds => &[("plank", 2), ("cloth", 2)],
            Self::Stove => &[("scrap", 2), ("butane", 2)],
            Self::Rain => &[("scrap", 2), ("bucket", 1)],
            Self::Planter => &[("plank", 1), ("seeds", 1)],
        }
    }

    pub fn name(self, lang: Language) -> &'static str {
        match self {
            Self::Boards => tr(lang, "釘木板封窗", "Board up the windows"),
            Self::Beds => tr(lang, "撿來的床墊", "Salvaged mattresses"),
            Self::Stove => tr(lang, "瓦斯爐", "Gas stove"),
            Self::Rain => tr(lang, "頂樓集雨桶", "Rooftop rain barrel"),
            Self::Planter => tr(lang, "陽台菜圃", "Balcony vegetable garden"),
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct Shelter {
    pub day: u32,
    pub karma: i32,
    pub scarcity: f64,
    pub upgrades: Vec<Upgrade>,
    pub group: Vec<Survivor>,
    pub stash: Stash,
    pub event: String,
    pub log: Vec<String>,
    pub plant_day: u32,
    pub pray_day: u32,
    pub raided: u32,
    pub stole: u32,
    pub gave: u32,
}

impl Default for Shelter {
    fn default() -> Self {
        let mut stash = Stash::default();
        stash.add("rice", 2);
        stash.add("water", 2);
        Self {
            day: 1,
            karma: 0,
            scarcity: 1.0,
            upgrades: Vec::new(),
            group: vec![
                Survivor::new("ama", Skill::Cook, true),
                Survivor::new("miao", Skill::Pray, false),
                Survivor::new("adi", Skill::Build, false),
                Survivor::new("mei", Skill::Scavenge, true),
            ],
            stash,
            event: String::new(),
            log: Vec::new(),
            plant_day: 0,
            pray_day: 0,
            raided: 0,
            stole: 0,
            gave: 0,
        }
    }
}

/// Adapter boundary: future weather and utility systems can read/write these flags.
/// Do not pretend they already affect street lighting or refrigeration.
#[derive(Resource, Debug, Clone, Copy)]
pub struct ShelterEnvironment {
    pub raining: bool,
    pub power: bool,
    pub water: bool,
}

impl Default for ShelterEnvironment {
    fn default() -> Self {
        Self {
            raining: false,
            power: true,
            water: true,
        }
    }
}

// Original Park-Miller generator; integer arithmetic is exact for the original day range.
struct DayRng(u64);

impl DayRng {
    fn new(day: u32) -> Self {
        let seed = (u64::from(day) * 2_654_435_761) % 2_147_483_647;
        Self(if seed == 0 { 7 } else { seed })
    }

    fn next(&mut self) -> f64 {
        self.0 = (self.0 * 16_807) % 2_147_483_647;
        self.0 as f64 / 2_147_483_647.0
    }
}

pub fn is_night(game_min: f64) -> bool {
    let hour = (game_min / 60.0) % 24.0;
    !(6.0..18.0).contains(&hour)
}

fn js_round(n: f64) -> i32 {
    (n + 0.5).floor() as i32
}

impl Shelter {
    pub fn has(&self, upgrade: Upgrade) -> bool {
        self.upgrades.contains(&upgrade)
    }

    pub fn avg_mood(&self) -> i32 {
        let (sum, count) = self
            .group
            .iter()
            .filter(|g| g.here && !g.dead)
            .fold((0.0, 0_u32), |(sum, n), g| (sum + g.mood, n + 1));
        if count == 0 {
            0
        } else {
            js_round(sum / f64::from(count))
        }
    }

    /// Original barter valuation; unknown items are one cigarette equivalent.
    pub fn price(&self, key: &str) -> i32 {
        let base = match key {
            "powerbank" | "pill" | "rawrice" => 4.0,
            "medkit" => 8.0,
            "cstove" => 6.0,
            "can" | "rice" | "bandage" | "gas" | "butane" | "peanutcan" => 3.0,
            "mask" | "battery" | "noodle" | "mianxian" | "roufong" => 2.0,
            _ => 1.0,
        };
        let scarce = ["rice", "can", "noodle", "pill", "bandage", "medkit", "water"]
            .iter()
            .any(|part| key.contains(part));
        js_round(if scarce { base * self.scarcity } else { base }).max(1)
    }

    pub fn pray(&mut self, lang: Language) -> String {
        if self.pray_day == self.day {
            return tr(lang, "今天已經拜過了。", "You have already prayed today.").to_owned();
        }
        self.pray_day = self.day;
        let boost = if self
            .group
            .iter()
            .any(|g| g.id == "miao" && g.here && !g.dead)
        {
            15
        } else {
            8
        };
        for g in self.group.iter_mut().filter(|g| !g.dead) {
            g.mood = (g.mood + f64::from(boost)).min(100.0);
        }
        match lang {
            Language::ZhTw => format!("點了香，在神明廳前拜拜。大家的心情好一點（+{boost}）。"),
            Language::En => format!(
                "You lit incense and prayed at the household altar. Everyone feels a little better (+{boost})."
            ),
        }
    }

    /// A morning at 06:00, in the same RNG and mutation order as the web game.
    pub fn day_tick(&mut self, env: &mut ShelterEnvironment, lang: Language) -> Vec<String> {
        self.day += 1;
        self.scarcity = (1.0 + f64::from(self.day) * 0.07).min(3.0);
        let mut rng = DayRng::new(self.day);
        let mut out = Vec::new();
        let boards = self.has(Upgrade::Boards);
        let beds = self.has(Upgrade::Beds);
        let mut meals = 0;
        if self.has(Upgrade::Stove) && self.stash.consume_first(&["rawrice", "mianxian", "rice"]) {
            meals = self.group.iter().filter(|g| g.here && !g.dead).count();
            out.push(
                tr(
                    lang,
                    "阿嬤用瓦斯爐煮了一鍋稀飯。",
                    "Grandma cooked a pot of rice porridge on the gas stove.",
                )
                .to_owned(),
            );
        }
        for i in 0..self.group.len() {
            let g = &mut self.group[i];
            if !g.here || g.dead {
                continue;
            }
            if meals > 0 {
                meals -= 1;
                g.hunger = (g.hunger + 45.0).min(100.0);
            } else if self.stash.consume_first(FOOD) {
                g.hunger = (g.hunger + 35.0).min(100.0);
            } else {
                g.hunger -= 25.0;
            }
            g.hunger -= 15.0;
            if !self.stash.consume_first(WATER) {
                g.sick += 12.0;
            }
            if g.hunger < 20.0 {
                g.sick += 10.0;
                g.mood -= 10.0;
            }
            if !boards && rng.next() < 0.08 {
                g.sick += 15.0;
            }
            g.hurt = (g.hurt - if beds { 18.0 } else { 8.0 }).max(0.0);
            let name = g.name(lang);
            if g.sick > 30.0 && self.stash.take("pill", 1) {
                g.sick -= 35.0;
                out.push(match lang {
                    Language::ZhTw => format!("{name}吃了止痛錠，燒退了一點。"),
                    Language::En => format!("{name} took a painkiller; the fever eased a little."),
                });
            } else {
                g.sick = (g.sick - if beds { 10.0 } else { 4.0 }).max(0.0);
            }
            g.mood -= 2.0 + self.scarcity * 1.5;
            if self.karma < -2 {
                g.mood -= 4.0;
            }
            g.hunger = g.hunger.clamp(0.0, 100.0);
            g.mood = g.mood.clamp(0.0, 100.0);
            if g.sick >= 100.0 || (g.hunger <= 0.0 && g.sick > 60.0) {
                g.dead = true;
                out.push(match lang {
                    Language::ZhTw => format!("💀 {name}沒撐過這一夜。"),
                    Language::En => format!("💀 {name} did not survive the night."),
                });
                for o in &mut self.group {
                    o.mood -= 20.0;
                }
            } else if g.mood < 15.0 {
                out.push(match lang {
                    Language::ZhTw => format!("{name}整天不說話，眼神空洞（憂鬱）。"),
                    Language::En => format!(
                        "{name} has been silent all day, staring blankly (depressed)."
                    ),
                });
            }
        }
        if self.has(Upgrade::Planter) && self.day - self.plant_day >= 2 {
            self.plant_day = self.day;
            self.stash.add("veg", 2);
            self.stash.add("snack", 1);
            out.push(
                tr(
                    lang,
                    "陽台的地瓜葉可以收了。",
                    "The sweet potato leaves on the balcony are ready to harvest.",
                )
                .to_owned(),
            );
        }
        if self.has(Upgrade::Rain) && env.raining {
            self.stash.add("rainwater", 3);
        }
        if rng.next() < (if boards { 0.04 } else { 0.2 }) * self.scarcity
            && let Some(rice) = self.stash.raid()
        {
            self.raided += 1;
            let what = if rice {
                tr(lang, "米", "rice")
            } else {
                tr(lang, "存貨", "supplies")
            };
            out.push(match lang {
                Language::ZhTw => format!("🔦 半夜有人撬窗，偷走了一些{what}。"),
                Language::En => format!(
                    "🔦 Someone forced a window during the night and stole some {what}."
                ),
            });
        }
        if let Some(g) = self.group.iter_mut().find(|g| !g.here && !g.dead)
            && rng.next() < 0.35 + f64::from(self.karma) * 0.05
        {
            g.here = true;
            let role = g.role(lang);
            out.push(match lang {
                Language::ZhTw => format!("🚪 {role}敲門求收留，你讓他進來了。"),
                Language::En => format!(
                    "🚪 {role} knocked, asking for shelter. You let them in."
                ),
            });
        }
        let index = (rng.next() * 7.0).floor() as usize;
        let (title, description) = self.apply_event(index, env, lang);
        self.event = title.to_owned();
        out.insert(
            0,
            match lang {
                Language::ZhTw => format!("第 {} 天・{title}：{description}", self.day),
                Language::En => format!("Day {} · {title}: {description}", self.day),
            },
        );
        self.log.splice(0..0, out.iter().cloned());
        self.log.truncate(30);
        out
    }

    fn apply_event(
        &mut self,
        index: usize,
        env: &mut ShelterEnvironment,
        lang: Language,
    ) -> (&'static str, &'static str) {
        match index {
            0 => {
                env.power = false;
                (
                    tr(lang, "⚡ 停電", "⚡ Blackout"),
                    tr(
                        lang,
                        "整區停電，冰箱裡的東西撐不了多久。",
                        "The district has lost power. Food in the fridge will not last long.",
                    ),
                )
            }
            1 => {
                env.water = false;
                (
                    tr(lang, "🚱 停水", "🚱 Water outage"),
                    tr(
                        lang,
                        "自來水停了，只能靠存水和雨水。",
                        "The taps have run dry. Only stored water and rainwater remain.",
                    ),
                )
            }
            2 => {
                for g in &mut self.group {
                    g.mood -= 6.0;
                }
                (
                    tr(lang, "📢 防空警報", "📢 Air-raid warning"),
                    tr(
                        lang,
                        "防空警報響了一整晚，大家都沒睡好。",
                        "Air-raid sirens sounded all night. Nobody slept well.",
                    ),
                )
            }
            3 => {
                if self.has(Upgrade::Rain) {
                    self.stash.add("rainwater", 4);
                }
                let message = if self.has(Upgrade::Boards) {
                    tr(
                        lang,
                        "颱風過境，封好的窗撐住了。",
                        "The typhoon passed. The boarded windows held.",
                    )
                } else {
                    tr(
                        lang,
                        "颱風把沒封的窗吹破，大家又濕又冷。",
                        "The typhoon shattered the unboarded windows, leaving everyone cold and wet.",
                    )
                };
                (tr(lang, "🌀 颱風", "🌀 Typhoon"), message)
            }
            4 => {
                self.stash.add("rice", 2);
                self.stash.add("can", 1);
                (
                    tr(lang, "🍚 鄰長發物資", "🍚 Neighborhood relief"),
                    tr(
                        lang,
                        "鄰長挨家挨戶送來兩包米和罐頭。",
                        "The neighborhood chief went door to door, delivering two bags of rice and canned food.",
                    ),
                )
            }
            5 => {
                for g in &mut self.group {
                    g.mood -= 3.0;
                }
                (
                    tr(lang, "📻 疏散廣播", "📻 Evacuation broadcast"),
                    tr(
                        lang,
                        "廣播要老弱婦孺往南疏散，但橋都被封了。",
                        "The broadcast urges vulnerable civilians to evacuate south, but every bridge is blocked.",
                    ),
                )
            }
            _ => (
                tr(lang, "☁ 平靜的一天", "☁ A quiet day"),
                tr(
                    lang,
                    "難得沒有槍聲，大家在屋裡補衣服、聊天。",
                    "For once, there is no gunfire. Everyone stays inside, mending clothes and talking.",
                ),
            ),
        }
    }
}

/// Same dawn bucket as main.ts. First sample is a baseline, not a free daily tick.
#[derive(Resource, Default)]
struct DawnTracker(Option<i64>);

impl DawnTracker {
    fn crossed(&mut self, game_min: f64) -> bool {
        let day = ((game_min - 360.0) / 1440.0).floor() as i64;
        match self.0 {
            None => {
                self.0 = Some(day);
                false
            }
            Some(previous) if day > previous => {
                self.0 = Some(day);
                true
            }
            _ => false,
        }
    }
}

pub struct ShelterPlugin;

impl Plugin for ShelterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shelter>()
            .init_resource::<ShelterEnvironment>()
            .init_resource::<DawnTracker>()
            .add_systems(OnEnter(GameFlow::Prologue), reset)
            .add_systems(Update, tick.run_if(in_state(GameFlow::Playing)));
    }
}

fn reset(
    mut shelter: ResMut<Shelter>,
    mut env: ResMut<ShelterEnvironment>,
    mut dawn: ResMut<DawnTracker>,
) {
    *shelter = Shelter::default();
    *env = ShelterEnvironment::default();
    *dawn = DawnTracker::default();
}

fn tick(
    campaign: Res<Campaign>,
    lang: Res<Language>,
    mut shelter: ResMut<Shelter>,
    mut env: ResMut<ShelterEnvironment>,
    mut dawn: ResMut<DawnTracker>,
) {
    if dawn.crossed(f64::from(campaign.game_min)) {
        shelter.day_tick(&mut env, *lang);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_shelter_matches_web() {
        let s = Shelter::default();
        assert_eq!(s.day, 1);
        assert_eq!(s.group.len(), 4);
        assert_eq!(s.group.iter().filter(|g| g.here).count(), 2);
        assert_eq!(s.stash.count("rice"), 2);
        assert_eq!(s.stash.count("water"), 2);
        assert_eq!(s.avg_mood(), 70);
    }

    #[test]
    fn first_dawn_matches_original_day_two() {
        let mut s = Shelter::default();
        let mut env = ShelterEnvironment::default();
        let out = s.day_tick(&mut env, Language::ZhTw);
        assert_eq!(s.day, 2);
        assert!((s.scarcity - 1.14).abs() < 1e-9);
        assert_eq!(s.stash.count("rice"), 0);
        assert_eq!(s.stash.count("water"), 0);
        assert_eq!(s.group[0].hunger, 85.0);
        assert!((s.group[0].mood - 66.29).abs() < 1e-9);
        assert_eq!(s.group[3].hunger, 85.0);
        assert!(!s.group[1].here);
        assert_eq!(s.event, "🌀 颱風");
        assert!(out[0].starts_with("第 2 天"));
    }

    #[test]
    fn second_dawn_matches_original_day_three() {
        let mut s = Shelter::default();
        let mut env = ShelterEnvironment::default();
        s.day_tick(&mut env, Language::En);
        s.day_tick(&mut env, Language::En);
        assert_eq!(s.day, 3);
        assert_eq!(s.group[0].hunger, 45.0);
        assert_eq!(s.group[0].sick, 8.0);
        assert_eq!(s.event, "📻 Evacuation broadcast");
    }

    #[test]
    fn prayer_once_per_day_and_keeper_bonus() {
        let mut s = Shelter::default();
        s.pray(Language::En);
        assert_eq!(s.avg_mood(), 78);
        assert_eq!(s.pray(Language::En), "You have already prayed today.");
        s.day += 1;
        s.group[1].here = true;
        s.pray(Language::En);
        assert_eq!(s.group[0].mood, 93.0);
    }

    #[test]
    fn stash_keeps_insertion_order_and_zero_entries() {
        let mut stash = Stash::default();
        stash.add("water", 3);
        stash.add("rice", 1);
        assert_eq!(stash.raid(), Some(false));
        assert_eq!(stash.count("water"), 1);
        assert!(stash.take("water", 1));
        stash.add("water", 2);
        assert_eq!(stash.raid(), Some(false));
        assert_eq!(stash.count("rice"), 1);
    }

    #[test]
    fn stove_and_upgrades_use_original_costs() {
        assert_eq!(Upgrade::Stove.cost(), &[("scrap", 2), ("butane", 2)]);
        let mut s = Shelter::default();
        s.upgrades.push(Upgrade::Stove);
        s.day_tick(&mut ShelterEnvironment::default(), Language::En);
        assert_eq!(s.stash.count("rice"), 1);
        assert_eq!(s.group[0].hunger, 85.0);
        assert_eq!(s.group[3].hunger, 85.0);
    }

    #[test]
    fn day_night_dawn_and_barter() {
        assert!(is_night(359.0));
        assert!(!is_night(360.0));
        assert!(is_night(1080.0));
        let mut d = DawnTracker::default();
        assert!(!d.crossed(440.0));
        assert!(!d.crossed(1799.0));
        assert!(d.crossed(1800.0));
        assert!(!d.crossed(1801.0));
        let s = Shelter {
            scarcity: 2.0,
            ..Shelter::default()
        };
        assert_eq!(s.price("rice"), 6);
        assert_eq!(s.price("medkit"), 16);
        assert_eq!(s.price("battery"), 2);
        assert_eq!(s.price("rainwater"), 2);
    }

    #[test]
    fn utility_events_and_log_cap() {
        let mut s = Shelter::default();
        let mut env = ShelterEnvironment::default();
        s.day_tick(&mut env, Language::En);
        s.day_tick(&mut env, Language::En);
        s.day_tick(&mut env, Language::En);
        assert!(!env.power);
        for _ in 0..60 {
            s.day_tick(&mut env, Language::En);
        }
        assert!(s.log.len() <= 30);
        assert_eq!(s.scarcity, 3.0);
    }
}
