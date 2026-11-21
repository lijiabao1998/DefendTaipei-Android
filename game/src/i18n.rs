//! Session language. The web build reloads to switch; the native port swaps strings in place.
//! Default is Traditional Chinese, matching `i18n.ts`.

use bevy::prelude::*;

/// `zh-TW` or `en`. Anything else stays Traditional Chinese, as in `normalizeLanguage`.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Language {
    #[default]
    ZhTw,
    En,
}

pub fn tr(lang: Language, zh: &'static str, en: &'static str) -> &'static str {
    match lang {
        Language::ZhTw => zh,
        Language::En => en,
    }
}

/// Subset of Noto Sans CJK TC (SIL OFL). See `assets/LICENSES.md`.
#[derive(Resource, Default)]
pub struct UiFont(pub Handle<Font>);

pub struct I18nPlugin;

impl Plugin for I18nPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Language>()
            .init_resource::<UiFont>()
            .add_systems(Startup, load_font)
            .add_systems(Update, apply_font);
    }
}

fn load_font(mut fonts: ResMut<Assets<Font>>, mut ui_font: ResMut<UiFont>) {
    let face = Font::from_bytes(include_bytes!("../../assets/fonts/DefendTaipeiUI.otf").to_vec());
    ui_font.0 = fonts.add(face);
}

pub(crate) fn apply_font(ui_font: Res<UiFont>, mut text: Query<&mut TextFont>) {
    if ui_font.0 == Handle::default() {
        return;
    }
    let source = FontSource::Handle(ui_font.0.clone());
    for mut font in &mut text {
        if font.font != source {
            font.font = source.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traditional_chinese_is_the_default() {
        assert_eq!(tr(Language::ZhTw, "保衛台北", "Defend Taipei"), "保衛台北");
        assert_eq!(
            tr(Language::En, "保衛台北", "Defend Taipei"),
            "Defend Taipei"
        );
        assert_eq!(Language::default(), Language::ZhTw);
    }
}
