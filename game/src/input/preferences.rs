//! Native controls from the pinned Web `src/ui/settings.ts` contract.
//! Bindings apply to implemented actions; Esc, mouse fire and F/F3 diagnostics
//! remain fixed. Duplicate bindings swap, so a key cannot trigger two actions.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::i18n::{Language, tr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAction {
    Up,
    Down,
    Left,
    Right,
    Sprint,
    Sneak,
    Interact,
    Inventory,
    Map,
    Shelter,
    Trade,
    Barricade,
    Sleep,
    Speed1,
    Speed3,
    Speed20,
    VehicleBrake,
    VehicleReload,
    VehicleSwitch,
}

pub const CONTROL_ACTIONS: [ControlAction; 19] = [
    ControlAction::Up,
    ControlAction::Down,
    ControlAction::Left,
    ControlAction::Right,
    ControlAction::Sprint,
    ControlAction::Sneak,
    ControlAction::Interact,
    ControlAction::Inventory,
    ControlAction::Map,
    ControlAction::Shelter,
    ControlAction::Trade,
    ControlAction::Barricade,
    ControlAction::Sleep,
    ControlAction::Speed1,
    ControlAction::Speed3,
    ControlAction::Speed20,
    ControlAction::VehicleBrake,
    ControlAction::VehicleReload,
    ControlAction::VehicleSwitch,
];

impl ControlAction {
    pub fn default_key(self) -> KeyCode {
        match self {
            Self::Up => KeyCode::KeyW,
            Self::Down => KeyCode::KeyS,
            Self::Left => KeyCode::KeyA,
            Self::Right => KeyCode::KeyD,
            Self::Sprint => KeyCode::ShiftLeft,
            Self::Sneak => KeyCode::KeyC,
            Self::Interact => KeyCode::KeyE,
            Self::Inventory => KeyCode::Tab,
            Self::Map => KeyCode::KeyM,
            Self::Shelter => KeyCode::KeyH,
            Self::Trade => KeyCode::KeyT,
            Self::Barricade => KeyCode::KeyB,
            Self::Sleep => KeyCode::KeyZ,
            Self::Speed1 => KeyCode::Digit1,
            Self::Speed3 => KeyCode::Digit2,
            Self::Speed20 => KeyCode::Digit3,
            Self::VehicleBrake => KeyCode::Space,
            Self::VehicleReload => KeyCode::KeyR,
            Self::VehicleSwitch => KeyCode::KeyV,
        }
    }

    pub fn label(self, language: Language) -> &'static str {
        match self {
            Self::Up => tr(language, "前進", "Move forward"),
            Self::Down => tr(language, "後退", "Move backward"),
            Self::Left => tr(language, "向左", "Move left"),
            Self::Right => tr(language, "向右", "Move right"),
            Self::Sprint => tr(language, "奔跑（按住）", "Sprint (hold)"),
            Self::Sneak => tr(language, "蹲低／起身", "Crouch / stand"),
            Self::Interact => tr(language, "互動／上下車", "Interact / enter / exit"),
            Self::Inventory => tr(language, "背包", "Inventory"),
            Self::Map => tr(language, "地圖", "Map"),
            Self::Shelter => tr(language, "避難所", "Shelter"),
            Self::Trade => tr(language, "交易", "Trade"),
            Self::Barricade => tr(language, "封門", "Barricade"),
            Self::Sleep => tr(language, "睡覺", "Sleep"),
            Self::Speed1 => tr(language, "時間 ×1", "Time ×1"),
            Self::Speed3 => tr(language, "時間 ×3", "Time ×3"),
            Self::Speed20 => tr(language, "時間 ×20", "Time ×20"),
            Self::VehicleBrake => tr(language, "載具煞車（按住）", "Vehicle brake (hold)"),
            Self::VehicleReload => tr(language, "車載武器裝填", "Reload mounted weapon"),
            Self::VehicleSwitch => tr(language, "切換車載武器", "Switch mounted weapon"),
        }
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlPreferences {
    pub vibration: bool,
    /// Original Web button presets, represented as integer percentages.
    pub button_percent: u8,
    pub bindings: BTreeMap<ControlAction, String>,
}

impl Default for ControlPreferences {
    fn default() -> Self {
        Self {
            vibration: true,
            button_percent: 100,
            bindings: CONTROL_ACTIONS
                .into_iter()
                .map(|action| (action, key_code(action.default_key())))
                .collect(),
        }
    }
}

impl ControlPreferences {
    pub fn default_ref() -> &'static Self {
        static DEFAULT: OnceLock<ControlPreferences> = OnceLock::new();
        DEFAULT.get_or_init(Self::default)
    }

    pub fn sanitize(mut self) -> Self {
        if !matches!(self.button_percent, 85 | 100 | 110) {
            self.button_percent = 100;
        }
        let mut used = BTreeSet::new();
        self.bindings.retain(|_, code| {
            parse_key(code).is_some_and(bindable_key) && used.insert(code.clone())
        });
        for action in CONTROL_ACTIONS {
            if self.bindings.contains_key(&action) {
                continue;
            }
            let preferred = key_code(action.default_key());
            let code = if !used.contains(&preferred) {
                preferred
            } else {
                CONTROL_ACTIONS
                    .iter()
                    .map(|a| key_code(a.default_key()))
                    .find(|key| !used.contains(key))
                    .expect("fewer bindings than actions leave a default key free")
            };
            used.insert(code.clone());
            self.bindings.insert(action, code);
        }
        self
    }

    pub fn key(&self, action: ControlAction) -> KeyCode {
        self.bindings
            .get(&action)
            .and_then(|code| parse_key(code))
            .filter(|key| *key != KeyCode::KeyF)
            .unwrap_or_else(|| action.default_key())
    }

    pub fn key_label(&self, action: ControlAction) -> String {
        key_label(self.key(action))
    }

    /// Returns the action swapped with this one. Reserved/unsupported keys
    /// leave both bindings untouched and return `Err`.
    pub fn rebind(
        &mut self,
        action: ControlAction,
        key: KeyCode,
    ) -> Result<Option<ControlAction>, &'static str> {
        if !bindable_key(key) {
            return Err("reserved or unsupported key");
        }
        let code = key_code(key);
        let other = self
            .bindings
            .iter()
            .find(|(candidate, current)| **candidate != action && **current == code)
            .map(|(candidate, _)| *candidate);
        if let Some(other) = other {
            self.bindings.insert(other, key_code(self.key(action)));
        }
        self.bindings.insert(action, code);
        Ok(other)
    }

    fn keys(&self, action: ControlAction) -> impl Iterator<Item = KeyCode> {
        let bound = self.key(action);
        let mut keys = [Some(bound), None];
        let alias = match action {
            ControlAction::Up => Some(KeyCode::ArrowUp),
            ControlAction::Down => Some(KeyCode::ArrowDown),
            ControlAction::Left => Some(KeyCode::ArrowLeft),
            ControlAction::Right => Some(KeyCode::ArrowRight),
            ControlAction::Inventory => Some(KeyCode::KeyI),
            ControlAction::Sprint if bound == KeyCode::ShiftLeft => Some(KeyCode::ShiftRight),
            ControlAction::Sprint if bound == KeyCode::ShiftRight => Some(KeyCode::ShiftLeft),
            _ => None,
        };
        if let Some(alias) = alias
            && alias != bound
            && !CONTROL_ACTIONS
                .iter()
                .any(|other| *other != action && self.key(*other) == alias)
        {
            keys[1] = Some(alias);
        }
        keys.into_iter().flatten()
    }

    pub fn pressed(&self, keys: &ButtonInput<KeyCode>, action: ControlAction) -> bool {
        keys.any_pressed(self.keys(action))
    }

    pub fn just_pressed(&self, keys: &ButtonInput<KeyCode>, action: ControlAction) -> bool {
        keys.any_just_pressed(self.keys(action))
    }

    pub fn clear_just_pressed(&self, keys: &mut ButtonInput<KeyCode>, action: ControlAction) {
        for key in self.keys(action) {
            keys.clear_just_pressed(key);
        }
    }

    pub fn button_scale(&self) -> f32 {
        f32::from(self.button_percent) * 0.01
    }

    pub fn haptic(&self, duration_ms: u32) -> bool {
        self.vibration && crate::device::request_haptic(duration_ms)
    }
}

pub fn key_code(key: KeyCode) -> String {
    format!("{key:?}")
}

pub fn key_label(key: KeyCode) -> String {
    let code = key_code(key);
    if let Some(letter) = code
        .strip_prefix("Key")
        .or_else(|| code.strip_prefix("Digit"))
    {
        return letter.to_owned();
    }
    match key {
        KeyCode::ShiftLeft => "Shift",
        KeyCode::ShiftRight => "R Shift",
        KeyCode::ControlLeft => "Ctrl",
        KeyCode::ControlRight => "R Ctrl",
        KeyCode::AltLeft => "Alt",
        KeyCode::AltRight => "R Alt",
        KeyCode::ArrowUp => "↑",
        KeyCode::ArrowDown => "↓",
        KeyCode::ArrowLeft => "←",
        KeyCode::ArrowRight => "→",
        _ => return code,
    }
    .into()
}

fn parse_key(code: &str) -> Option<KeyCode> {
    macro_rules! known_keys {
        ($($key:ident),* $(,)?) => { match code { $(stringify!($key) => Some(KeyCode::$key),)* _ => None } };
    }
    known_keys!(
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        ShiftLeft,
        ShiftRight,
        ControlLeft,
        ControlRight,
        AltLeft,
        AltRight,
        Space,
        Tab,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        Enter,
        Backquote,
        Minus,
        Equal,
        BracketLeft,
        BracketRight,
        Backslash,
        Semicolon,
        Quote,
        Comma,
        Period,
        Slash,
    )
}

pub fn bindable_key(key: KeyCode) -> bool {
    key != KeyCode::KeyF && parse_key(&key_code(key)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebinding_swaps_without_double_actions_and_keeps_escape_fixed() {
        let mut prefs = ControlPreferences::default();
        assert_eq!(
            prefs.rebind(ControlAction::Interact, KeyCode::KeyW),
            Ok(Some(ControlAction::Up))
        );
        assert_eq!(prefs.key(ControlAction::Up), KeyCode::KeyE);
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyW);
        assert!(prefs.just_pressed(&keys, ControlAction::Interact));
        assert!(!prefs.pressed(&keys, ControlAction::Up));
        let before = prefs.clone();
        for key in [KeyCode::Escape, KeyCode::F3, KeyCode::KeyF] {
            assert!(prefs.rebind(ControlAction::Interact, key).is_err());
            assert_eq!(prefs, before);
        }
    }

    #[test]
    fn fallback_keys_yield_to_explicit_bindings() {
        let mut prefs = ControlPreferences::default();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyI);
        assert!(prefs.just_pressed(&keys, ControlAction::Inventory));
        prefs
            .rebind(ControlAction::Interact, KeyCode::KeyI)
            .unwrap();
        assert!(!prefs.just_pressed(&keys, ControlAction::Inventory));
        assert!(prefs.just_pressed(&keys, ControlAction::Interact));
        keys.reset_all();
        keys.press(KeyCode::ShiftRight);
        assert!(prefs.pressed(&keys, ControlAction::Sprint));
        prefs
            .rebind(ControlAction::Interact, KeyCode::ShiftRight)
            .unwrap();
        assert!(!prefs.pressed(&keys, ControlAction::Sprint));
        prefs.clear_just_pressed(&mut keys, ControlAction::Interact);
        assert!(!prefs.just_pressed(&keys, ControlAction::Interact));
    }

    #[test]
    fn malformed_or_old_preferences_remain_complete_and_unique() {
        let value: ControlPreferences = serde_json::from_str(
            r#"{"button_percent":255,"bindings":{"up":"KeyE","interact":"KeyE","sprint":"Escape"}}"#,
        )
        .unwrap();
        let value = value.sanitize();
        assert_eq!(value.button_percent, 100);
        assert_eq!(value.bindings.len(), CONTROL_ACTIONS.len());
        assert_eq!(
            value.bindings.values().collect::<BTreeSet<_>>().len(),
            CONTROL_ACTIONS.len()
        );
        assert_eq!(value.key(ControlAction::Up), KeyCode::KeyE);
        assert_eq!(
            serde_json::from_str::<ControlPreferences>("{}").unwrap(),
            ControlPreferences::default()
        );
        assert_eq!(
            serde_json::from_str::<ControlPreferences>(&serde_json::to_string(&value).unwrap())
                .unwrap(),
            value
        );
    }
}
