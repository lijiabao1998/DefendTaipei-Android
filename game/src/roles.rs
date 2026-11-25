//! Playable survivors, ported from `src/player/roles.ts`.
//! Looks are the original outfit colors. This crate does not spawn character models.

use bevy::prelude::*;

use crate::i18n::{Language, tr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Gender {
    #[default]
    Male,
    Female,
}

impl Gender {
    pub fn label(self, lang: Language) -> &'static str {
        match self {
            Self::Male => tr(lang, "男", "Male"),
            Self::Female => tr(lang, "女", "Female"),
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Male => 0,
            Self::Female => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spawn {
    Home,
    Hospital,
    Scooter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    School,
    Office,
    Hospital,
    Delivery,
    Site,
}

#[derive(Clone, Copy, Debug)]
pub struct Skills {
    pub medic: u8,
    pub repair: u8,
    pub lockpick: bool,
    pub ride: f32,
    pub streets: bool,
    pub carry: f32,
    pub rest: f32,
}

impl Skills {
    const fn new() -> Self {
        Self {
            medic: 0,
            repair: 0,
            lockpick: false,
            ride: 0.0,
            streets: false,
            carry: 1.0,
            rest: 1.0,
        }
    }
}

/// Faceless outfit colors from the original `L()` helper (`0xRRGGBB`).
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub hair: u32,
    pub top: u32,
    pub bottom: u32,
    pub shoes: u32,
    pub jacket: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub struct Role {
    pub id: &'static str,
    pub name_zh: &'static str,
    pub name_en: &'static str,
    /// Male age, then female age.
    pub age: [u8; 2],
    pub name_m_zh: &'static str,
    pub name_m_en: &'static str,
    pub name_f_zh: &'static str,
    pub name_f_en: &'static str,
    pub blurb_zh: &'static str,
    pub blurb_en: &'static str,
    pub spawn: Spawn,
    pub dest: Dest,
    pub dest_zh: &'static str,
    pub dest_en: &'static str,
    pub skills: Skills,
    pub inv: &'static [(&'static str, u8)],
    pub look_m: Look,
    pub look_f: Look,
    pub perks_zh: &'static [&'static str],
    pub perks_en: &'static [&'static str],
}

impl Role {
    pub fn name(self, lang: Language) -> &'static str {
        tr(lang, self.name_zh, self.name_en)
    }

    pub fn given(self, gender: Gender, lang: Language) -> &'static str {
        match (gender, lang) {
            (Gender::Male, Language::ZhTw) => self.name_m_zh,
            (Gender::Male, Language::En) => self.name_m_en,
            (Gender::Female, Language::ZhTw) => self.name_f_zh,
            (Gender::Female, Language::En) => self.name_f_en,
        }
    }

    pub fn age_of(self, gender: Gender) -> u8 {
        self.age[gender.index()]
    }

    pub fn blurb(self, lang: Language) -> &'static str {
        tr(lang, self.blurb_zh, self.blurb_en)
    }

    pub fn perks(self, lang: Language) -> &'static [&'static str] {
        match lang {
            Language::ZhTw => self.perks_zh,
            Language::En => self.perks_en,
        }
    }

    pub fn card_line(self, gender: Gender, lang: Language) -> String {
        format!("{}・{}", self.given(gender, lang), self.name(lang))
    }
}

const fn look(top: u32, bottom: u32, shoes: u32, jacket: Option<u32>) -> Look {
    Look {
        hair: 0x1a1410,
        top,
        bottom,
        shoes,
        jacket,
    }
}

pub static ROLES: [Role; 6] = [
    Role {
        id: "office",
        name_zh: "上班族",
        name_en: "Office worker",
        age: [31, 29],
        name_m_zh: "志偉",
        name_m_en: "Zhiwei",
        name_f_zh: "雅婷",
        name_f_en: "Yating",
        blurb_zh: "每天搭捷運到西門町的辦公大樓上班",
        blurb_en: "Takes the MRT to an office building in Ximending every day.",
        spawn: Spawn::Home,
        dest: Dest::Office,
        dest_zh: "公司（辦公大樓）",
        dest_en: "Workplace (office building)",
        skills: Skills {
            carry: 1.0,
            ..Skills::new()
        },
        inv: &[("coffee", 1), ("water", 1), ("noodle", 1)],
        look_m: look(0xeef0f4, 0x262a34, 0x111111, Some(0x262a34)),
        look_f: look(0xf2eee6, 0x3a3f4a, 0x111111, Some(0x3a3f4a)),
        perks_zh: &["背包正常", "起點：自家"],
        perks_en: &["Standard backpack", "Starts at home"],
    },
    Role {
        id: "student",
        name_zh: "學生",
        name_en: "Student",
        age: [17, 16],
        name_m_zh: "冠宇",
        name_m_en: "Guanyu",
        name_f_zh: "欣妤",
        name_f_en: "Xinyu",
        blurb_zh: "高中生，早上騎腳踏車去學校",
        blurb_en: "A high school student who cycles to school each morning.",
        spawn: Spawn::Home,
        dest: Dest::School,
        dest_zh: "學校",
        dest_en: "School",
        skills: Skills {
            carry: 1.1,
            rest: 1.5,
            ..Skills::new()
        },
        inv: &[("snack", 2), ("sport", 1), ("water", 1)],
        look_m: look(0xf6f6f6, 0x1e2a50, 0x222222, None),
        look_f: look(0xf6f6f6, 0x1e2a50, 0x222222, None),
        perks_zh: &["體力恢復快", "背包稍大（書包）"],
        perks_en: &["Faster recovery", "Slightly larger backpack (schoolbag)"],
    },
    Role {
        id: "courier",
        name_zh: "外送員",
        name_en: "Delivery rider",
        age: [32, 26],
        name_m_zh: "阿明",
        name_m_en: "A-Ming",
        name_f_zh: "佳琪",
        name_f_en: "Jiaqi",
        blurb_zh: "機車就停在巷口，閉著眼都認得萬華的巷子",
        blurb_en: "Your scooter is parked at the alley entrance. You know Wanhua's lanes by heart.",
        spawn: Spawn::Scooter,
        dest: Dest::Delivery,
        dest_zh: "外送取餐點",
        dest_en: "Delivery pickup point",
        skills: Skills {
            ride: 1.25,
            streets: true,
            carry: 0.75,
            ..Skills::new()
        },
        inv: &[("water", 1), ("cupnoodle", 1), ("knife", 1)],
        look_m: look(0x3a7a5a, 0x2a2a30, 0xe8e8e8, Some(0x2f6a4c)),
        look_f: look(0x3a7a5a, 0x2a2a30, 0xe8e8e8, Some(0x2f6a4c)),
        perks_zh: &["熟悉街道（地圖全開）", "騎車更快", "背包小"],
        perks_en: &[
            "Knows the streets (full map revealed)",
            "Faster riding",
            "Small backpack",
        ],
    },
    Role {
        id: "nurse",
        name_zh: "護理師",
        name_en: "Nurse",
        age: [29, 27],
        name_m_zh: "柏翰",
        name_m_en: "Bohan",
        name_f_zh: "小雯",
        name_f_en: "Xiaowen",
        blurb_zh: "值完夜班在醫院宿舍醒來",
        blurb_en: "Wakes up in the hospital dorm after a night shift.",
        spawn: Spawn::Hospital,
        dest: Dest::Hospital,
        dest_zh: "醫院急診",
        dest_en: "Hospital emergency room",
        skills: Skills {
            medic: 2,
            ..Skills::new()
        },
        inv: &[("bandage", 4), ("pill", 2), ("medkit", 1), ("water", 1)],
        look_m: look(0x5a9ab0, 0x5a9ab0, 0xf4f4f4, None),
        look_f: look(0xd890a8, 0xd890a8, 0xf4f4f4, None),
        perks_zh: &["醫療技能：包紮效果加倍", "起點：醫院宿舍"],
        perks_en: &[
            "Medical skill: double bandage effectiveness",
            "Starts at the hospital dorm",
        ],
    },
    Role {
        id: "tech",
        name_zh: "水電師傅",
        name_en: "Maintenance technician",
        age: [45, 41],
        name_m_zh: "阿財",
        name_m_en: "A-Cai",
        name_f_zh: "淑芬",
        name_f_en: "Shufen",
        blurb_zh: "修電修水、開鎖接線都會，今天要去工地",
        blurb_en: "Skilled in plumbing, wiring, lockpicking and hot-wiring. Heading to a worksite today.",
        spawn: Spawn::Home,
        dest: Dest::Site,
        dest_zh: "工地",
        dest_en: "Construction site",
        skills: Skills {
            repair: 2,
            lockpick: true,
            ..Skills::new()
        },
        inv: &[("hammer", 1), ("pipe", 1), ("plank", 2), ("water", 1)],
        look_m: look(0x5a6a7a, 0x3a4048, 0xe8e8e8, None),
        look_f: look(0x6a7a8a, 0x3a4048, 0xe8e8e8, None),
        perks_zh: &["修理：無線電只需 1 顆電池", "開鎖／接線發動", "工具箱"],
        perks_en: &[
            "Repair: radios need only 1 battery",
            "Lockpicking / hot-wiring",
            "Toolbox",
        ],
    },
    Role {
        id: "jp",
        name_zh: "日籍駐在員",
        name_en: "Japanese expat engineer",
        age: [36, 33],
        name_m_zh: "佐藤健太",
        name_m_en: "Kenta Sato",
        name_f_zh: "高木美咲",
        name_f_en: "Misaki Takagi",
        blurb_zh: "日商公司派駐台北的工程師，在萬華住了五年，台語聽得懂一點",
        blurb_en: "An engineer posted to Taipei by a Japanese company; five years in Wanhua, understands a little Taiwanese.",
        spawn: Spawn::Home,
        dest: Dest::Office,
        dest_zh: "公司（辦公大樓）",
        dest_en: "Workplace (office building)",
        skills: Skills {
            carry: 1.0,
            ..Skills::new()
        },
        inv: &[("cracker", 1), ("water", 1), ("battery", 1)],
        look_m: look(0xdfe4e8, 0x2b2f3a, 0x1a1a1a, Some(0x4a5260)),
        look_f: look(0xf0ece4, 0x4a4038, 0x2a2220, Some(0x7a6a58)),
        perks_zh: &["會說日語：一開始就知道自衛隊連絡官在哪", "多帶一顆電池"],
        perks_en: &[
            "Speaks Japanese: knows where the JSDF liaison is from the start",
            "Carries a spare battery",
        ],
    },
];

/// Unknown ids fall through to the courier, matching `roleById`.
pub fn role_by_id(id: Option<&str>) -> &'static Role {
    id.and_then(|id| ROLES.iter().find(|role| role.id == id))
        .unwrap_or(&ROLES[2])
}

/// Who the title screen has picked. The menu starts on the first card (office worker).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedRole {
    pub id: &'static str,
    pub gender: Gender,
}

impl Default for SelectedRole {
    fn default() -> Self {
        Self {
            id: ROLES[0].id,
            gender: Gender::Male,
        }
    }
}

impl SelectedRole {
    pub fn role(self) -> &'static Role {
        role_by_id(Some(self.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_role_is_the_courier() {
        assert_eq!(role_by_id(None).id, "courier");
        assert_eq!(role_by_id(Some("nope")).id, "courier");
        assert_eq!(role_by_id(Some("nurse")).spawn, Spawn::Hospital);
        assert!(role_by_id(Some("tech")).skills.lockpick);
        assert_eq!(role_by_id(Some("nurse")).skills.medic, 2);
        assert!(role_by_id(Some("courier")).skills.streets);
        assert_eq!(role_by_id(Some("office")).look_m.top, 0xeef0f4);
        assert_eq!(ROLES.len(), 6);
    }
}
