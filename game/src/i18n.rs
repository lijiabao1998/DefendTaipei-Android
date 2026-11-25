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
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    use super::*;

    /// Symbols the subset always keeps, even before a screen uses them.
    const EXTRA_SYMBOLS: &str = "°←↑→↓▶";

    /// Variation selectors are not glyphs. U+23ED (⏭) is not in Noto Sans CJK;
    /// the skip button draws ▶. Story lines are scanned with every other string,
    /// so a missing story character fails this test. Emoji are rewritten to words
    /// before they are drawn.
    const SKIP: &[char] = &['\u{fe0e}', '\u{fe0f}', '⏭'];

    #[test]
    fn traditional_chinese_is_the_default() {
        assert_eq!(tr(Language::ZhTw, "保衛台北", "Defend Taipei"), "保衛台北");
        assert_eq!(
            tr(Language::En, "保衛台北", "Defend Taipei"),
            "Defend Taipei"
        );
        assert_eq!(Language::default(), Language::ZhTw);
    }

    #[test]
    fn subset_covers_every_displayed_character() {
        let needed = displayed_chars();
        let ui = cmap_codepoints(include_bytes!("../../assets/fonts/DefendTaipeiUI.otf"));
        let asset = cmap_codepoints(include_bytes!("../../assets/fonts/NotoSansTC-Subset.otf"));
        let missing: Vec<char> = needed
            .into_iter()
            .filter(|ch| !ch.is_control() && !SKIP.contains(ch))
            .filter(|ch| !ui.contains(&(*ch as u32)) || !asset.contains(&(*ch as u32)))
            .collect();
        assert!(
            missing.is_empty(),
            "run tools/subset_ui_font.py — missing {:?}",
            missing
                .iter()
                .map(|ch| format!("U+{:04X} {ch}", *ch as u32))
                .collect::<Vec<_>>()
        );
    }

    fn displayed_chars() -> BTreeSet<char> {
        let mut chars: BTreeSet<char> = EXTRA_SYMBOLS.chars().collect();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        walk(&root, &mut chars);
        chars
    }

    fn walk(dir: &Path, chars: &mut BTreeSet<char>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "target" || name == ".git" || name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                walk(&path, chars);
            } else if name.ends_with(".rs")
                && let Ok(text) = fs::read_to_string(&path)
            {
                scan_strings(&text, chars);
            }
        }
    }

    fn scan_strings(src: &str, out: &mut BTreeSet<char>) {
        let chars: Vec<char> = src.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '/' && chars.get(i + 1) == Some(&'/') {
                i += 2;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i = (i + 2).min(chars.len());
                continue;
            }
            if chars[i] == 'r'
                && let Some(body) = raw_string(&chars, i)
            {
                out.extend(body.0);
                i = body.1;
                continue;
            }
            if chars[i] == '"'
                && let Some(body) = cooked_string(&chars, i)
            {
                out.extend(body.0);
                i = body.1;
                continue;
            }
            i += 1;
        }
    }

    fn raw_string(chars: &[char], start: usize) -> Option<(Vec<char>, usize)> {
        let mut i = start + 1;
        let mut hashes = 0;
        while chars.get(i) == Some(&'#') {
            hashes += 1;
            i += 1;
        }
        if chars.get(i) != Some(&'"') {
            return None;
        }
        i += 1;
        let body_start = i;
        while i < chars.len() {
            if chars[i] == '"' && (0..hashes).all(|n| chars.get(i + 1 + n) == Some(&'#')) {
                return Some((chars[body_start..i].to_vec(), i + 1 + hashes));
            }
            i += 1;
        }
        None
    }

    fn cooked_string(chars: &[char], start: usize) -> Option<(Vec<char>, usize)> {
        let mut i = start + 1;
        let mut body = Vec::new();
        while i < chars.len() {
            match chars[i] {
                '"' => return Some((body, i + 1)),
                '\\' => {
                    i += 1;
                    let escaped = chars.get(i).copied()?;
                    match escaped {
                        'n' => body.push('\n'),
                        'r' => body.push('\r'),
                        't' => body.push('\t'),
                        '0' => body.push('\0'),
                        '\\' | '\'' | '"' => body.push(escaped),
                        'u' => {
                            if chars.get(i + 1) != Some(&'{') {
                                return None;
                            }
                            i += 2;
                            let hex_start = i;
                            while chars.get(i).is_some_and(|ch| ch.is_ascii_hexdigit()) {
                                i += 1;
                            }
                            if chars.get(i) != Some(&'}') {
                                return None;
                            }
                            let hex: String = chars[hex_start..i].iter().collect();
                            let code = u32::from_str_radix(&hex, 16).ok()?;
                            body.push(char::from_u32(code)?);
                        }
                        _ => body.push(escaped),
                    }
                    i += 1;
                }
                ch => {
                    body.push(ch);
                    i += 1;
                }
            }
        }
        None
    }

    fn cmap_codepoints(font: &[u8]) -> BTreeSet<u32> {
        let cmap = table(font, *b"cmap").expect("cmap table");
        let num_tables = u16_at(cmap, 2) as usize;
        let mut points = BTreeSet::new();
        for index in 0..num_tables {
            let record = 4 + index * 8;
            let offset = u32_at(cmap, record + 4) as usize;
            if offset >= cmap.len() {
                continue;
            }
            match u16_at(cmap, offset) {
                4 => parse_format4(cmap, offset, &mut points),
                12 => parse_format12(cmap, offset, &mut points),
                _ => {}
            }
        }
        points
    }

    fn parse_format4(data: &[u8], start: usize, out: &mut BTreeSet<u32>) {
        let seg_count = u16_at(data, start + 6) as usize / 2;
        let end_at = start + 14;
        let start_at = end_at + seg_count * 2 + 2;
        let delta_at = start_at + seg_count * 2;
        let offset_at = delta_at + seg_count * 2;
        for seg in 0..seg_count {
            let end = u16_at(data, end_at + seg * 2);
            let start_code = u16_at(data, start_at + seg * 2);
            let delta = i32::from(u16_at(data, delta_at + seg * 2) as i16);
            let range_offset = u16_at(data, offset_at + seg * 2) as usize;
            let mut code = start_code;
            while code <= end {
                let glyph = if range_offset == 0 {
                    (i32::from(code) + delta) as u16
                } else {
                    let index =
                        offset_at + seg * 2 + range_offset + usize::from(code - start_code) * 2;
                    if index + 1 >= data.len() {
                        0
                    } else {
                        let id = u16_at(data, index);
                        if id == 0 {
                            0
                        } else {
                            (i32::from(id) + delta) as u16
                        }
                    }
                };
                if glyph != 0 && code != 0xFFFF {
                    out.insert(u32::from(code));
                }
                if code == u16::MAX {
                    break;
                }
                code += 1;
            }
        }
    }

    fn parse_format12(data: &[u8], start: usize, out: &mut BTreeSet<u32>) {
        let groups = u32_at(data, start + 12) as usize;
        for group in 0..groups {
            let at = start + 16 + group * 12;
            if at + 12 > data.len() {
                break;
            }
            let start_code = u32_at(data, at);
            let end_code = u32_at(data, at + 4);
            let glyph = u32_at(data, at + 8);
            let mut code = start_code;
            while code <= end_code {
                if glyph + (code - start_code) != 0 {
                    out.insert(code);
                }
                if code == u32::MAX {
                    break;
                }
                code += 1;
            }
        }
    }

    fn table(font: &[u8], tag: [u8; 4]) -> Option<&[u8]> {
        let num_tables = u16_at(font, 4) as usize;
        for index in 0..num_tables {
            let record = 12 + index * 16;
            if font.get(record..record + 4) == Some(&tag) {
                let offset = u32_at(font, record + 8) as usize;
                let length = u32_at(font, record + 12) as usize;
                return font.get(offset..offset + length);
            }
        }
        None
    }

    fn u16_at(data: &[u8], offset: usize) -> u16 {
        u16::from_be_bytes([data[offset], data[offset + 1]])
    }

    fn u32_at(data: &[u8], offset: usize) -> u32 {
        u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])
    }
}
