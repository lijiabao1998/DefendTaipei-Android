//! Shelter simulation port of web v24.42.0, src/systems/survival.ts.
//! Keep rules independent of UI, rendering, and platform storage.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::game_flow::{Campaign, GameFlow};
use crate::i18n::{Language, tr};
use crate::inventory::{Inventory, item};

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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
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
            *n = n.saturating_add(count);
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

    pub fn remove_empty(&mut self) {
        self.0.retain(|(_, count)| *count > 0);
    }

    pub fn is_valid(&self) -> bool {
        self.0.len() <= 1024
            && self.0.iter().enumerate().all(|(i, (key, count))| {
                !key.is_empty()
                    && key.len() <= 128
                    && *count <= 1_000_000
                    && self.0[..i].iter().all(|(other, _)| key != other)
            })
    }

    pub fn spoil(&mut self) -> u32 {
        let mut spoiled = 0;
        let mut emptied = Vec::new();
        for (key, count) in &mut self.0 {
            if *count > 0 && item(key).is_some_and(|d| d.perish) {
                let rot = count.div_ceil(2);
                *count -= rot;
                spoiled += rot;
                if *count == 0 {
                    emptied.push(key.clone());
                }
            }
        }
        self.0.retain(|(key, _)| !emptied.contains(key));
        if spoiled > 0 {
            self.add("rotten", spoiled);
        }
        spoiled
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Skill {
    Cook,
    Pray,
    Build,
    Scavenge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Survivor {
    pub id: String,
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
            id: id.to_owned(),
            skill,
            hunger: 70.0,
            sick: 0.0,
            hurt: 0.0,
            mood: 70.0,
            here,
            dead: false,
        }
    }

    pub fn name(&self, lang: Language) -> &str {
        match self.id.as_str() {
            "ama" => tr(lang, "阿嬤", "Grandma"),
            "miao" => tr(lang, "廟公", "Temple keeper"),
            "adi" => tr(lang, "阿迪", "Adi"),
            "mei" => tr(lang, "小美", "Xiaomei"),
            _ => &self.id,
        }
    }

    pub fn role(&self, lang: Language) -> &str {
        match self.id.as_str() {
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
            _ => &self.id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Upgrade {
    Boards,
    Beds,
    Stove,
    Rain,
    Planter,
}

impl Upgrade {
    pub const ALL: [Self; 5] = [
        Self::Boards,
        Self::Beds,
        Self::Stove,
        Self::Rain,
        Self::Planter,
    ];

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

    pub fn description(self, lang: Language) -> &'static str {
        match self {
            Self::Boards => tr(
                lang,
                "夜裡被搶的機會大減",
                "Greatly reduces the risk of overnight theft",
            ),
            Self::Beds => tr(
                lang,
                "大家睡得好，生病和受傷好得快",
                "Better sleep helps everyone recover from illness and injury",
            ),
            Self::Stove => tr(
                lang,
                "一包白米（或麵線）煮成全組的一餐",
                "One bag of rice (or vermicelli) feeds the whole group",
            ),
            Self::Rain => tr(
                lang,
                "下雨天自動接雨水",
                "Automatically collects water when it rains",
            ),
            Self::Planter => tr(
                lang,
                "每兩天收一把地瓜葉",
                "Harvest sweet potato leaves every two days",
            ),
        }
    }
}

/// The original cigarette-equivalent market catalog, in its original order.
pub const TRADE_ITEMS: &[&str] = &[
    "cig",
    "umbrella",
    "poncho",
    "mask",
    "powerbank",
    "bread",
    "battery",
    "can",
    "rice",
    "noodle",
    "pill",
    "bandage",
    "medkit",
    "water",
    "plank",
    "scrap",
    "cloth",
    "seeds",
    "gas",
    "rawrice",
    "butane",
    "cstove",
    "mianxian",
    "peanutcan",
    "roufong",
    "cracker",
    "canfruit",
    "driedtofu",
    "boiled",
];

#[derive(Clone, Copy, Debug)]
pub struct TradeSpot {
    pub name_zh: &'static str,
    pub name_en: &'static str,
    pub pos: [f32; 2],
}

impl TradeSpot {
    pub fn name(self, lang: Language) -> &'static str {
        tr(lang, self.name_zh, self.name_en)
    }
}

pub const TRADE_SPOTS: &[TradeSpot] = &[
    TradeSpot {
        name_zh: "龍山寺廟埕・以物易物",
        name_en: "Longshan Temple courtyard · Barter",
        pos: [-260.0, 392.0],
    },
    TradeSpot {
        name_zh: "東園街市場・以物易物",
        name_en: "Dongyuan Street Market · Barter",
        pos: [-40.0, 610.0],
    },
];

fn near(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < 4.0
}

pub fn trade_at(position: [f32; 2]) -> Option<usize> {
    TRADE_SPOTS.iter().position(|spot| near(position, spot.pos))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoralChoice {
    Take,
    Give,
    Leave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionError {
    AwayFromHome,
    AwayFromMarket,
    AlreadyBuilt,
    InsufficientMaterials,
    InsufficientCigarettes,
    MissingItem,
    NotTraded,
}

impl ActionError {
    pub fn message(self, lang: Language) -> &'static str {
        match self {
            Self::AwayFromHome => tr(
                lang,
                "回到避難所才能建造、存放或拜拜。",
                "Return to your shelter to build, deposit supplies, or pray.",
            ),
            Self::AwayFromMarket => tr(
                lang,
                "靠近龍山寺或東園街市場攤位才能交易。",
                "Move close to a Longshan Temple or Dongyuan Street barter stall.",
            ),
            Self::AlreadyBuilt => tr(lang, "這項設施已經完成。", "This upgrade is already built."),
            Self::InsufficientMaterials => tr(
                lang,
                "背包裡的建材不夠。",
                "Your pack does not contain enough materials.",
            ),
            Self::InsufficientCigarettes => {
                tr(lang, "長樂菸不夠。", "You do not have enough cigarettes.")
            }
            Self::MissingItem => tr(
                lang,
                "背包裡沒有這項物品。",
                "That item is not in your pack.",
            ),
            Self::NotTraded => tr(
                lang,
                "攤位沒有交換這項物品。",
                "The stall does not trade this item.",
            ),
        }
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    pub home: Option<[f32; 2]>,
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
            home: None,
        }
    }
}

/// Weather and utility flags shared by the shelter, world, and persistent save.
#[derive(Resource, Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct ShelterEnvironment {
    pub raining: bool,
    pub power: bool,
    pub water: bool,
}

impl ShelterEnvironment {
    pub fn is_valid(&self) -> bool {
        true
    }
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
    pub fn at_home(&self, position: [f32; 2]) -> bool {
        self.home.is_some_and(|home| near(position, home))
    }

    pub fn is_valid(&self) -> bool {
        (1..=1_000_000).contains(&self.day)
            && self.scarcity.is_finite()
            && (1.0..=3.0).contains(&self.scarcity)
            && (-1_000_000..=1_000_000).contains(&self.karma)
            && [self.raided, self.stole, self.gave]
                .into_iter()
                .all(|n| n <= 1_000_000)
            && self.plant_day <= self.day
            && self.pray_day <= self.day
            && self.home.is_none_or(|p| {
                p.into_iter()
                    .all(|v| v.is_finite() && v.abs() < 1_000_000.0)
            })
            && self.stash.is_valid()
            && self.upgrades.len() <= Upgrade::ALL.len()
            && self
                .upgrades
                .iter()
                .enumerate()
                .all(|(i, upgrade)| !self.upgrades[..i].contains(upgrade))
            && self.group.len() <= 64
            && self.group.iter().enumerate().all(|(i, g)| {
                !g.id.is_empty()
                    && g.id.len() <= 64
                    && self.group[..i].iter().all(|other| other.id != g.id)
                    && [g.hunger, g.sick, g.hurt, g.mood]
                        .into_iter()
                        .all(|v| v.is_finite() && (-10_000.0..=10_000.0).contains(&v))
            })
            && self.log.len() <= 30
            && self.log.iter().all(|entry| entry.len() <= 8192)
            && self.event.len() <= 1024
    }

    pub fn build(
        &mut self,
        upgrade: Upgrade,
        bag: &mut Inventory,
        position: [f32; 2],
    ) -> Result<(), ActionError> {
        if !self.at_home(position) {
            return Err(ActionError::AwayFromHome);
        }
        if self.has(upgrade) {
            return Err(ActionError::AlreadyBuilt);
        }
        if !upgrade
            .cost()
            .iter()
            .all(|(key, count)| bag.count(key) >= *count)
        {
            return Err(ActionError::InsufficientMaterials);
        }
        for &(key, count) in upgrade.cost() {
            bag.take(key, count);
        }
        self.upgrades.push(upgrade);
        Ok(())
    }

    pub fn deposit(&mut self, bag: &mut Inventory, position: [f32; 2]) -> Result<u32, ActionError> {
        if !self.at_home(position) {
            return Err(ActionError::AwayFromHome);
        }
        let supplies: Vec<_> = bag
            .stacks()
            .iter()
            .filter(|(key, count)| *count > 0 && item(key).is_some_and(|d| d.kind.is_supply()))
            .cloned()
            .collect();
        let mut total = 0;
        for (key, count) in supplies {
            bag.take(&key, count);
            self.stash.add(&key, count);
            total += count;
        }
        Ok(total)
    }

    pub fn pray_at(&mut self, position: [f32; 2], lang: Language) -> Result<String, ActionError> {
        if !self.at_home(position) {
            return Err(ActionError::AwayFromHome);
        }
        Ok(self.pray(lang))
    }

    pub fn barter(
        &self,
        bag: &mut Inventory,
        key: &str,
        buy: bool,
        position: [f32; 2],
    ) -> Result<(), ActionError> {
        if trade_at(position).is_none() {
            return Err(ActionError::AwayFromMarket);
        }
        if key == "cig" || !TRADE_ITEMS.contains(&key) {
            return Err(ActionError::NotTraded);
        }
        let price = self.price(key) as u32;
        if buy {
            if !bag.take("cig", price) {
                return Err(ActionError::InsufficientCigarettes);
            }
            bag.add(key, 1);
        } else {
            if !bag.take(key, 1) {
                return Err(ActionError::MissingItem);
            }
            bag.add("cig", price.saturating_sub(1).max(1));
        }
        Ok(())
    }

    /// The caller owns the container's persistent `asked` flag and allows this choice once.
    pub fn moral(
        &mut self,
        choice: MoralChoice,
        bag: &mut Inventory,
        lang: Language,
    ) -> Result<String, ActionError> {
        let text = match choice {
            MoralChoice::Take => {
                self.karma -= 1;
                self.stole += 1;
                for g in &mut self.group {
                    g.mood = (g.mood - 8.0).max(0.0);
                }
                tr(
                    lang,
                    "你拿走了阿公的東西。回家的路上，誰都沒說話。",
                    "You take the old man's belongings. Nobody speaks on the way home.",
                )
            }
            MoralChoice::Give => {
                if !bag.take("rice", 1) {
                    return Err(ActionError::MissingItem);
                }
                self.karma += 2;
                self.gave += 1;
                for g in &mut self.group {
                    g.mood = (g.mood + 5.0).min(100.0);
                }
                tr(
                    lang,
                    "你留下一包米。阿公合掌說了聲多謝。",
                    "You leave a bag of rice. The old man presses his palms together and thanks you.",
                )
            }
            MoralChoice::Leave => {
                self.karma += 1;
                tr(lang, "你輕輕把門帶上。", "You quietly close the door.")
            }
        };
        Ok(text.to_owned())
    }

    pub fn ending_text(&self, lang: Language) -> String {
        let separator = tr(lang, "、", ", ");
        let alive: Vec<_> = self.group.iter().filter(|g| g.here && !g.dead).collect();
        let dead: Vec<_> = self.group.iter().filter(|g| g.dead).collect();
        let names = if alive.is_empty() {
            tr(lang, "沒有人", "no one").to_owned()
        } else {
            alive
                .iter()
                .map(|g| g.name(lang))
                .collect::<Vec<_>>()
                .join(separator)
        };
        let mut text = match lang {
            Language::ZhTw => format!(
                "撐了 {} 天，避難所裡還有 {} 位街坊：{}。",
                self.day,
                alive.len(),
                names
            ),
            Language::En => format!(
                "After {} days, {} neighbors remain at the shelter: {}.",
                self.day,
                alive.len(),
                names
            ),
        };
        if !dead.is_empty() {
            let names = dead
                .iter()
                .map(|g| g.name(lang))
                .collect::<Vec<_>>()
                .join(separator);
            text.push_str(&match lang {
                Language::ZhTw => format!("{names}沒能看到這一天。"),
                Language::En => format!("{names} did not live to see this day."),
            });
        }
        text.push_str(if self.karma <= -3 {
            tr(lang, "萬華奪回了，但你拿走過別人的救命藥，夜裡還是會想起那扇門。", "Wanhua is free, but you took someone else’s life-saving medicine. At night, you still remember that door.")
        } else if self.karma >= 3 {
            tr(lang, "大家都記得，最難的時候是你把米分給了巷口的人。", "Everyone remembers how you shared your rice with the people at the alley entrance when times were hardest.")
        } else {
            tr(lang, "萬華奪回了，大家互相扶著走出巷子。", "Wanhua is free. Supporting one another, everyone walks out of the alley.")
        });
        if self.avg_mood() < 30 {
            text.push_str(tr(
                lang,
                "活下來的人笑不太出來。",
                "The survivors find little reason to smile.",
            ));
        }
        text
    }

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
            "mask" | "battery" | "noodle" | "mianxian" | "roufong" | "seeds" | "canfruit" => 2.0,
            _ => 1.0,
        };
        let scarce = [
            "rice", "can", "noodle", "pill", "bandage", "medkit", "water",
        ]
        .into_iter()
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
            let name = g.name(lang).to_owned();
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
                    Language::ZhTw => format!("{name}沒撐過這一夜。"),
                    Language::En => format!("{name} did not survive the night."),
                });
                for o in &mut self.group {
                    o.mood -= 20.0;
                }
            } else if g.mood < 15.0 {
                out.push(match lang {
                    Language::ZhTw => format!("{name}整天不說話，眼神空洞（憂鬱）。"),
                    Language::En => {
                        format!("{name} has been silent all day, staring blankly (depressed).")
                    }
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
                Language::ZhTw => format!("半夜有人撬窗，偷走了一些{what}。"),
                Language::En => {
                    format!("Someone forced a window during the night and stole some {what}.")
                }
            });
        }
        if let Some(g) = self.group.iter_mut().find(|g| !g.here && !g.dead)
            && rng.next() < 0.35 + f64::from(self.karma) * 0.05
        {
            g.here = true;
            let role = g.role(lang);
            out.push(match lang {
                Language::ZhTw => format!("{role}敲門求收留，你讓他進來了。"),
                Language::En => format!("{role} knocked, asking for shelter. You let them in."),
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
                    tr(lang, "停電", "Blackout"),
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
                    tr(lang, "停水", "Water outage"),
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
                    tr(lang, "防空警報", "Air-raid warning"),
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
                (tr(lang, "颱風", "Typhoon"), message)
            }
            4 => {
                self.stash.add("rice", 2);
                self.stash.add("can", 1);
                (
                    tr(lang, "鄰長發物資", "Neighborhood relief"),
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
                    tr(lang, "疏散廣播", "Evacuation broadcast"),
                    tr(
                        lang,
                        "廣播要老弱婦孺往南疏散，但橋都被封了。",
                        "The broadcast urges vulnerable civilians to evacuate south, but every bridge is blocked.",
                    ),
                )
            }
            _ => (
                tr(lang, "平靜的一天", "A quiet day"),
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
#[derive(Resource, Default, Debug, Clone)]
pub struct DawnTracker(pub Option<i64>);

impl DawnTracker {
    pub fn from_minutes(game_min: f32) -> Self {
        let mut tracker = Self::default();
        tracker.reset_at(f64::from(game_min));
        tracker
    }

    pub fn reset_at(&mut self, game_min: f64) {
        self.0 = Some(((game_min - 360.0) / 1440.0).floor() as i64);
    }

    pub fn crossed(&mut self, game_min: f64) -> bool {
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

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShelterSet;

pub struct ShelterPlugin;

impl Plugin for ShelterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shelter>()
            .init_resource::<ShelterEnvironment>()
            .init_resource::<DawnTracker>()
            .add_systems(OnEnter(GameFlow::Prologue), reset)
            .configure_sets(Update, ShelterSet.after(crate::game_flow::FlowSet))
            .add_systems(
                Update,
                tick.in_set(ShelterSet).run_if(in_state(GameFlow::Playing)),
            );
    }
}

pub(crate) fn reset(
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

    #[derive(Deserialize)]
    struct ReferenceSnapshot {
        shelter: Shelter,
        env: ShelterEnvironment,
    }

    #[derive(Deserialize)]
    struct ReferenceCase {
        name: String,
        pray: bool,
        initial: ReferenceSnapshot,
        dawns: Vec<ReferenceSnapshot>,
        ending: String,
    }

    #[derive(Deserialize)]
    struct ReferencePrices {
        scarcity: f64,
        values: std::collections::BTreeMap<String, i32>,
    }

    #[derive(Deserialize)]
    struct ReferenceFixtures {
        cases: Vec<ReferenceCase>,
        prices: Vec<ReferencePrices>,
    }

    fn assert_reference(actual: &serde_json::Value, expected: &serde_json::Value, path: &str) {
        use serde_json::Value;
        match (actual, expected) {
            (Value::Number(a), Value::Number(b)) => {
                assert!(
                    (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-9,
                    "{path}: {a} != {b}"
                );
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len(), "{path}");
                for (i, (a, b)) in a.iter().zip(b).enumerate() {
                    assert_reference(a, b, &format!("{path}[{i}]"));
                }
            }
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(a.len(), b.len(), "{path}");
                for (key, a) in a {
                    assert_reference(a, b.get(key).unwrap(), &format!("{path}.{key}"));
                }
            }
            _ => assert_eq!(actual, expected, "{path}"),
        }
    }

    /// Fixtures execute the original TypeScript, not a second transcription of these Rust rules.
    #[test]
    fn all_dawn_mutations_and_prices_match_executed_web_reference() {
        let fixtures: ReferenceFixtures =
            serde_json::from_str(include_str!("../tests/fixtures/shelter-reference.json")).unwrap();
        for case in fixtures.cases {
            let mut shelter = case.initial.shelter;
            let mut env = case.initial.env;
            for expected in case.dawns {
                if case.pray {
                    shelter.pray(Language::En);
                }
                shelter.day_tick(&mut env, Language::En);
                assert_reference(
                    &serde_json::to_value(&shelter).unwrap(),
                    &serde_json::to_value(&expected.shelter).unwrap(),
                    &format!("{} day {}", case.name, shelter.day),
                );
                assert_eq!(
                    (env.raining, env.power, env.water),
                    (expected.env.raining, expected.env.power, expected.env.water)
                );
                assert!(shelter.is_valid(), "{} day {}", case.name, shelter.day);
            }
            assert_eq!(
                shelter.ending_text(Language::En),
                case.ending,
                "{}",
                case.name
            );
        }
        for band in fixtures.prices {
            let shelter = Shelter {
                scarcity: band.scarcity,
                ..Shelter::default()
            };
            for (key, expected) in band.values {
                assert_eq!(
                    shelter.price(&key),
                    expected,
                    "{} at scarcity {}",
                    key,
                    band.scarcity
                );
            }
        }
    }

    #[test]
    fn shelter_actions_require_home_and_spend_carried_materials_atomically() {
        let mut shelter = Shelter {
            home: Some([10.0, 20.0]),
            ..Shelter::default()
        };
        let mut bag = Inventory::default();
        bag.add("plank", 3);
        bag.add("cloth", 1);
        shelter.stash.add("cloth", 20);
        let original_bag = bag.clone();
        assert_eq!(
            shelter.build(Upgrade::Boards, &mut bag, [14.0, 20.0]),
            Err(ActionError::AwayFromHome)
        );
        assert_eq!(
            shelter.build(Upgrade::Beds, &mut bag, [10.0, 20.0]),
            Err(ActionError::InsufficientMaterials)
        );
        assert_eq!(bag, original_bag);
        assert_eq!(shelter.stash.count("cloth"), 20);
        shelter
            .build(Upgrade::Boards, &mut bag, [10.0, 20.0])
            .unwrap();
        assert_eq!(bag.count("plank"), 0);
        assert_eq!(
            shelter.build(Upgrade::Boards, &mut bag, [10.0, 20.0]),
            Err(ActionError::AlreadyBuilt)
        );
        bag.add("rice", 2);
        bag.add("water", 3);
        bag.add("pill", 1);
        bag.add("a9", 8);
        assert_eq!(shelter.deposit(&mut bag, [10.0, 20.0]), Ok(6));
        assert_eq!(shelter.stash.count("rice"), 4);
        assert_eq!(bag.count("cloth"), 1);
        assert_eq!(bag.count("a9"), 8);
        assert_eq!(bag.count("water"), 0);
    }

    #[test]
    fn market_rejects_remote_invalid_and_unaffordable_actions() {
        let shelter = Shelter {
            scarcity: 2.0,
            ..Shelter::default()
        };
        let mut bag = Inventory::default();
        bag.add("cig", 3);
        assert_eq!(
            shelter.barter(&mut bag, "rice", true, [0.0, 0.0]),
            Err(ActionError::AwayFromMarket)
        );
        assert_eq!(
            shelter.barter(&mut bag, "rice", true, TRADE_SPOTS[0].pos),
            Err(ActionError::InsufficientCigarettes)
        );
        assert_eq!(
            shelter.barter(&mut bag, "cig", true, TRADE_SPOTS[0].pos),
            Err(ActionError::NotTraded)
        );
        assert_eq!(bag.count("cig"), 3);
        shelter
            .barter(&mut bag, "seeds", true, TRADE_SPOTS[0].pos)
            .unwrap();
        assert_eq!(bag.count("seeds"), 1);
        assert_eq!(bag.count("cig"), 1);
        shelter
            .barter(&mut bag, "seeds", false, TRADE_SPOTS[0].pos)
            .unwrap();
        assert_eq!(bag.count("cig"), 2);
        assert_eq!(bag.count("seeds"), 0);
    }

    #[test]
    fn moral_outcomes_and_ending_follow_web_thresholds() {
        let mut shelter = Shelter::default();
        let mut bag = Inventory::default();
        assert_eq!(
            shelter.moral(MoralChoice::Give, &mut bag, Language::En),
            Err(ActionError::MissingItem)
        );
        assert_eq!(shelter.karma, 0);
        shelter
            .moral(MoralChoice::Take, &mut bag, Language::En)
            .unwrap();
        assert_eq!(shelter.stole, 1);
        assert!(shelter.group.iter().all(|g| g.mood == 62.0));
        bag.add("rice", 1);
        shelter
            .moral(MoralChoice::Give, &mut bag, Language::En)
            .unwrap();
        assert_eq!((shelter.karma, shelter.gave, bag.count("rice")), (1, 1, 0));
        shelter
            .moral(MoralChoice::Leave, &mut bag, Language::En)
            .unwrap();
        assert_eq!(shelter.karma, 2);
        shelter.karma = 3;
        assert!(
            shelter
                .ending_text(Language::En)
                .contains("shared your rice")
        );
        shelter.karma = -3;
        shelter.group[0].dead = true;
        shelter.group[3].mood = 0.0;
        let ending = shelter.ending_text(Language::En);
        assert!(ending.contains("Grandma did not live"));
        assert!(ending.contains("life-saving medicine"));
        assert!(ending.ends_with("The survivors find little reason to smile."));
    }

    #[test]
    fn restored_dawn_baseline_does_not_charge_the_same_morning_twice() {
        let mut dawn = DawnTracker::from_minutes(1800.0);
        assert!(!dawn.crossed(1800.0));
        assert!(!dawn.crossed(3239.0));
        assert!(dawn.crossed(3240.0));
        assert!(!dawn.crossed(3240.0));
    }

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
        assert_eq!(s.event, "颱風");
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
        assert_eq!(s.event, "Evacuation broadcast");
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
