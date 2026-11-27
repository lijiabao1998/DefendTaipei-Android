//! Persistent carried inventory and original item definitions.
//! Counts are separate from the shelter stockpile; building spends carried materials.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::i18n::{Language, tr};
use crate::roles::Role;
use crate::survival::Stash;

mod catalog;
pub use catalog::{ITEMS, LOOT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Food,
    Drink,
    Med,
    Melee,
    Gun,
    Ammo,
    Tool,
}

impl ItemKind {
    pub fn is_supply(self) -> bool {
        matches!(self, Self::Food | Self::Drink | Self::Med)
    }

    pub fn is_weapon(self) -> bool {
        matches!(self, Self::Melee | Self::Gun)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ItemDef {
    pub id: &'static str,
    pub name_zh: &'static str,
    pub name_en: &'static str,
    pub kind: ItemKind,
    pub weight: f32,
    pub hunger: f32,
    pub thirst: f32,
    pub fatigue: f32,
    pub hp: f32,
    pub stop_bleed: bool,
    pub raw: bool,
    pub perish: bool,
    pub cook: Option<&'static str>,
    pub risk: f32,
    pub damage: f32,
    pub range: f32,
    pub ammo: Option<&'static str>,
    pub noise: f32,
    pub cooldown: f32,
    pub cap: u32,
    pub mag: u32,
    pub anti_armor: f32,
    pub cal: Option<&'static str>,
    pub cal_zh: Option<&'static str>,
    pub roc: bool,
    pub desc_zh: &'static str,
    pub desc_en: &'static str,
}

impl ItemDef {
    const DEFAULT: Self = Self {
        id: "",
        name_zh: "",
        name_en: "",
        kind: ItemKind::Tool,
        weight: 0.0,
        hunger: 0.0,
        thirst: 0.0,
        fatigue: 0.0,
        hp: 0.0,
        stop_bleed: false,
        raw: false,
        perish: false,
        cook: None,
        risk: 0.0,
        damage: 0.0,
        range: 0.0,
        ammo: None,
        noise: 0.0,
        cooldown: 0.0,
        cap: 0,
        mag: 0,
        anti_armor: 0.0,
        cal: None,
        cal_zh: None,
        roc: false,
        desc_zh: "",
        desc_en: "",
    };

    pub fn name(self, lang: Language) -> &'static str {
        tr(lang, self.name_zh, self.name_en)
    }

    pub fn description(self, lang: Language) -> &'static str {
        tr(lang, self.desc_zh, self.desc_en)
    }

    pub fn usable(self) -> bool {
        self.kind.is_supply()
            || (self.kind.is_weapon()
                && (self.anti_armor == 0.0 || matches!(self.id, "at4" | "kestrel" | "jav")))
            || self.mag > 0
            || self.id == "cstove"
            || matches!(self.id, "gas" | "patch" | "carbat" | "hose")
    }
}

pub fn item(id: &str) -> Option<&'static ItemDef> {
    ITEMS.iter().find(|item| item.id == id)
}

#[derive(Clone, Copy, Debug)]
pub struct LootEntry {
    pub item: &'static str,
    pub weight: f32,
    pub max: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct LootTable {
    pub kind: &'static str,
    pub entries: &'static [LootEntry],
}

/// Original `src/systems/pla-loot.ts` weighted drops, preserving RNG call order.
pub fn soldier_drop(id: u32, rng: &mut impl FnMut() -> f64) -> Stash {
    fn choose(rng: &mut impl FnMut() -> f64, choices: &[(&'static str, f64)]) -> &'static str {
        let mut value = rng() * choices.iter().map(|(_, weight)| weight).sum::<f64>();
        for &(key, weight) in choices {
            value -= weight;
            if value <= 0.0 {
                return key;
            }
        }
        choices[0].0
    }
    let random_type = (((f64::from(id) * 91.7 + 3.1).sin() * 43758.5453) % 1.0 + 1.0) % 1.0;
    let soldier_type = if random_type < 0.66 {
        0
    } else if random_type < 0.78 {
        1
    } else if random_type < 0.86 {
        2
    } else if random_type < 0.94 {
        3
    } else {
        4
    };
    let mut loot = Stash::default();
    let mut add = |key: &str, count: u32| {
        if count > 0 {
            loot.add(key, count);
        }
    };
    match soldier_type {
        0 => {
            if rng() < 0.35 {
                add(choose(rng, &[("qbz191", 3.0), ("qbz95", 2.0)]), 1);
            }
            add("mag191", u32::from(rng() < 0.5));
            add("a556", (rng() * 25.0).floor() as u32);
        }
        1 => {
            if rng() < 0.4 {
                add(choose(rng, &[("qbu191", 2.0), ("qbu88", 1.0)]), 1);
            }
            add(if rng() < 0.5 { "mag88" } else { "mag191" }, 1);
            add("a556", (rng() * 12.0).floor() as u32);
        }
        2 => {
            if rng() < 0.3 {
                add("qjy88", 1);
            }
            add("belt88", u32::from(rng() < 0.6));
            add("a556", 10 + (rng() * 40.0).floor() as u32);
        }
        3 => {
            if rng() < 0.55 {
                add("qsz92", 1);
            }
            add("mag92", 1 + u32::from(rng() < 0.3));
        }
        _ => {
            if rng() < 0.4 {
                add(choose(rng, &[("qcw05", 2.0), ("qbs09", 1.0)]), 1);
            }
            add(
                if rng() < 0.5 { "mag05" } else { "a12" },
                if rng() < 0.5 { 1 } else { 4 },
            );
        }
    }
    if rng() < if soldier_type == 4 { 0.15 } else { 0.03 } {
        add("fpv", 1);
    }
    if rng() < 0.1 {
        add("dmotor", 1 + (rng() * 2.0).floor() as u32);
    }
    if rng() < 0.18 {
        add("grenade", 1);
    }
    if rng() < 0.08 {
        add("helmet", 1);
    }
    if rng() < 0.04 {
        add("vest", 1);
    }
    if rng() < 0.45 {
        add("ration", 1);
    }
    if rng() < 0.25 {
        add("bandage", 1);
    }
    loot
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Inventory {
    pub items: Stash,
    pub equipped: String,
}

impl Inventory {
    pub fn from_role(role: &Role) -> Self {
        let mut inventory = Self::default();
        for &(key, count) in role.inv {
            inventory.add(key, u32::from(count));
        }
        inventory
    }

    pub fn count(&self, key: &str) -> u32 {
        self.items.count(key)
    }

    pub fn add(&mut self, key: &str, count: u32) {
        if count > 0 {
            self.items.add(key, count);
        }
    }

    pub fn take(&mut self, key: &str, count: u32) -> bool {
        if !self.items.take(key, count) {
            return false;
        }
        self.items.remove_empty();
        if self.equipped == key && self.count(key) == 0 {
            self.equipped.clear();
        }
        true
    }

    pub fn stacks(&self) -> &[(String, u32)] {
        self.items.stacks()
    }

    pub fn weight(&self) -> f32 {
        self.stacks()
            .iter()
            .map(|(key, count)| item(key).map_or(0.0, |d| d.weight * *count as f32))
            // f32 Sum starts at -0.0. An empty bag must display positive zero.
            .fold(0.0, |total, weight| total + weight)
    }

    pub fn is_valid(&self) -> bool {
        self.items.is_valid()
            && self.stacks().iter().all(|(key, _)| item(key).is_some())
            && (self.equipped.is_empty()
                || (self.count(&self.equipped) > 0
                    && item(&self.equipped).is_some_and(|d| d.kind.is_weapon())))
    }

    /// Uses the original cook order. Explicit items still require a stove and butane.
    pub fn cook(
        &mut self,
        key: Option<&str>,
        thirst: f32,
        hunger: f32,
    ) -> Result<&'static str, InventoryError> {
        if self.count("cstove") == 0 {
            return Err(InventoryError::NoStove);
        }
        if self.count("butane") == 0 {
            return Err(InventoryError::NoFuel);
        }
        let order = if thirst < hunger {
            [
                "rainwater",
                "rawrice",
                "mianxian",
                "rawfish",
                "rawpork",
                "rawegg",
                "cabbage",
                "sweetpotato",
                "noodle",
            ]
        } else {
            [
                "rawpork",
                "rawfish",
                "rawrice",
                "mianxian",
                "rawegg",
                "cabbage",
                "sweetpotato",
                "noodle",
                "rainwater",
            ]
        };
        let key = key
            .or_else(|| order.into_iter().find(|key| self.count(key) > 0))
            .ok_or(InventoryError::NothingToCook)?;
        let output = item(key)
            .and_then(|d| d.cook)
            .ok_or(InventoryError::NothingToCook)?;
        if self.count(key) == 0 {
            return Err(InventoryError::MissingItem);
        }
        self.take(key, 1);
        self.take("butane", 1);
        self.add(output, 1);
        Ok(output)
    }

    /// `risk_roll` is supplied by gameplay so this operation remains deterministic in tests.
    pub fn use_item(
        &mut self,
        key: &str,
        vitals: &mut Vitals,
        medic: f32,
        risk_roll: f32,
    ) -> Result<UseResult, InventoryError> {
        let d = item(key).ok_or(InventoryError::UnknownItem)?;
        if self.count(key) == 0 {
            return Err(InventoryError::MissingItem);
        }
        if d.kind.is_weapon() {
            if d.anti_armor != 0.0 && !matches!(d.id, "at4" | "kestrel" | "jav") {
                return Err(InventoryError::Unsupported);
            }
            if self.equipped == key {
                self.equipped.clear();
            } else {
                self.equipped = key.to_owned();
            }
            return Ok(UseResult::Equipped(!self.equipped.is_empty()));
        }
        if d.mag > 0
            && let Some(ammo) = d.ammo
        {
            self.take(key, 1);
            self.add(ammo, d.mag);
            return Ok(UseResult::Unloaded { ammo, count: d.mag });
        }
        if d.raw || key == "cstove" {
            let output = self.cook(
                if key == "cstove" { None } else { Some(key) },
                vitals.thirst,
                vitals.hunger,
            )?;
            return Ok(UseResult::Cooked(output));
        }
        if !d.kind.is_supply() {
            return Err(InventoryError::Unsupported);
        }
        self.take(key, 1);
        vitals.hunger = (vitals.hunger + d.hunger).clamp(0.0, 100.0);
        vitals.thirst = (vitals.thirst + d.thirst).clamp(0.0, 100.0);
        vitals.fatigue = (vitals.fatigue + d.fatigue).clamp(0.0, 100.0);
        let multiplier = if d.kind == ItemKind::Med {
            medic.max(1.0)
        } else {
            1.0
        };
        vitals.hp = (vitals.hp + d.hp * multiplier).clamp(0.0, 100.0);
        if d.stop_bleed {
            vitals.bleeding = false;
        }
        let upset = d.risk > 0.0 && risk_roll < d.risk;
        if upset {
            vitals.hp = (vitals.hp - 8.0).max(0.0);
            vitals.thirst = (vitals.thirst - 12.0).max(0.0);
        }
        Ok(UseResult::Consumed { upset })
    }

    /// 06:00 after the scheduled blackout has lasted a day: ceil(n/2) perishable units spoil.
    pub fn spoil(&mut self) -> u32 {
        self.items.spoil()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Vitals {
    pub hp: f32,
    pub hunger: f32,
    pub thirst: f32,
    pub fatigue: f32,
    pub bleeding: bool,
}

impl Default for Vitals {
    fn default() -> Self {
        Self {
            hp: 100.0,
            hunger: 100.0,
            thirst: 100.0,
            fatigue: 0.0,
            bleeding: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseResult {
    Equipped(bool),
    Cooked(&'static str),
    Unloaded { ammo: &'static str, count: u32 },
    Consumed { upset: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryError {
    UnknownItem,
    MissingItem,
    NoStove,
    NoFuel,
    NothingToCook,
    Unsupported,
}

impl InventoryError {
    pub fn message(self, lang: Language) -> &'static str {
        match self {
            Self::UnknownItem => tr(lang, "找不到這項物品。", "Unknown item."),
            Self::MissingItem => tr(
                lang,
                "背包裡沒有這項物品。",
                "That item is not in your pack.",
            ),
            Self::NoStove => tr(
                lang,
                "需要卡式爐才能煮。",
                "You need a camping stove to cook.",
            ),
            Self::NoFuel => tr(lang, "卡式瓦斯罐用完了。", "You are out of butane cans."),
            Self::NothingToCook => tr(
                lang,
                "背包裡沒有要煮的東西。",
                "There is nothing to cook in your pack.",
            ),
            Self::Unsupported => tr(
                lang,
                "靠近相應設施使用這項物品。",
                "Use this item at the appropriate location.",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::ROLES;

    #[test]
    fn empty_bag_and_removing_last_item_never_render_negative_zero() {
        let mut bag = Inventory::default();
        assert_eq!(bag.weight().to_bits(), 0.0_f32.to_bits());
        assert_eq!(format!("{:.2} kg", bag.weight()), "0.00 kg");
        bag.add("rice", 1);
        assert!(bag.weight() > 0.0);
        assert!(bag.take("rice", 1));
        assert_eq!(bag.weight().to_bits(), 0.0_f32.to_bits());
        assert_eq!(format!("{:.1} kg", bag.weight()), "0.0 kg");
    }

    #[test]
    fn implemented_anti_armor_weapons_equip_without_consuming_a_round() {
        let mut bag = Inventory::default();
        let mut vitals = Vitals {
            hp: 100.0,
            hunger: 100.0,
            thirst: 100.0,
            fatigue: 0.0,
            bleeding: false,
        };
        for id in ["at4", "kestrel", "jav"] {
            bag.add(id, 1);
            assert!(item(id).unwrap().usable());
            assert_eq!(
                bag.use_item(id, &mut vitals, 1.0, 1.0),
                Ok(UseResult::Equipped(true))
            );
            assert_eq!(bag.equipped, id);
            assert_eq!(bag.count(id), 1);
        }
        for unsupported in ITEMS
            .iter()
            .filter(|item| item.anti_armor > 0.0 && !matches!(item.id, "at4" | "kestrel" | "jav"))
        {
            assert!(!unsupported.usable());
        }
    }

    #[test]
    fn soldier_drops_match_original_typescript_and_rng_consumption() {
        #[derive(Deserialize)]
        struct DropCase {
            id: u32,
            items: Stash,
            final_seed: u32,
        }
        let cases: Vec<DropCase> = serde_json::from_str(include_str!(
            "../tests/fixtures/soldier-drop-reference.json"
        ))
        .unwrap();
        for case in cases {
            let mut seed = case.id + 1;
            let actual = soldier_drop(case.id, &mut || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f64::from(seed) / 4_294_967_296.0
            });
            assert_eq!(actual, case.items, "soldier {}", case.id);
            assert_eq!(seed, case.final_seed, "soldier {} RNG consumption", case.id);
        }
    }

    #[test]
    fn original_catalog_and_role_packs_are_complete() {
        assert_eq!(ITEMS.len(), 126);
        assert_eq!(LOOT.len(), 18);
        for d in ITEMS {
            assert_eq!(ITEMS.iter().filter(|other| other.id == d.id).count(), 1);
            assert!(d.weight.is_finite() && d.weight >= 0.0);
            if let Some(output) = d.cook {
                assert!(item(output).is_some());
            }
            if let Some(ammo) = d.ammo {
                assert!(item(ammo).is_some());
            }
        }
        for table in LOOT {
            for entry in table.entries {
                assert!(item(entry.item).is_some());
                assert!(entry.weight > 0.0 && entry.max > 0);
            }
        }
        for role in &ROLES {
            let bag = Inventory::from_role(role);
            assert!(bag.is_valid());
            for &(key, count) in role.inv {
                assert_eq!(bag.count(key), u32::from(count));
            }
        }
    }

    #[test]
    fn cooking_consumes_one_ingredient_and_fuel_only_after_validation() {
        let mut bag = Inventory::default();
        bag.add("rawrice", 2);
        let original = bag.clone();
        assert_eq!(
            bag.cook(Some("rawrice"), 50.0, 50.0),
            Err(InventoryError::NoStove)
        );
        assert_eq!(bag, original);
        bag.add("cstove", 1);
        bag.add("butane", 1);
        assert_eq!(bag.cook(Some("rawrice"), 50.0, 50.0), Ok("cookedrice"));
        assert_eq!(bag.count("rawrice"), 1);
        assert_eq!(bag.count("butane"), 0);
        assert_eq!(bag.count("cookedrice"), 1);
        assert_eq!(bag.count("cstove"), 1);
    }

    #[test]
    fn medical_skill_and_unboiled_water_apply_web_effects() {
        let mut bag = Inventory::default();
        bag.add("bandage", 1);
        bag.add("rainwater", 1);
        let mut vitals = Vitals {
            hp: 20.0,
            hunger: 50.0,
            thirst: 20.0,
            fatigue: 20.0,
            bleeding: true,
        };
        bag.use_item("bandage", &mut vitals, 2.0, 1.0).unwrap();
        assert_eq!(vitals.hp, 20.0 + item("bandage").unwrap().hp * 2.0);
        assert!(!vitals.bleeding);
        let hp = vitals.hp;
        let result = bag.use_item("rainwater", &mut vitals, 1.0, 0.0).unwrap();
        assert_eq!(result, UseResult::Consumed { upset: true });
        assert_eq!(vitals.hp, hp - 8.0);
        assert_eq!(
            vitals.thirst,
            20.0 + item("rainwater").unwrap().thirst - 12.0
        );
    }

    #[test]
    fn spoilage_uses_ceil_half_and_leaves_dry_goods() {
        let mut bag = Inventory::default();
        bag.add("bento", 3);
        bag.add("rice", 1);
        bag.add("rawrice", 4);
        bag.add("can", 2);
        assert_eq!(bag.spoil(), 3);
        assert_eq!(bag.count("bento"), 1);
        assert_eq!(bag.count("rice"), 0);
        assert_eq!(bag.count("rawrice"), 4);
        assert_eq!(bag.count("can"), 2);
        assert_eq!(bag.count("rotten"), 3);
    }
}
