//! Portrait context-button labels, ported from `src/player/touchctl.ts`.
//!
//! The skeleton has no interaction prompts yet. When one shows up, the 打 button
//! uses these labels; an empty prompt stays on 打.

use crate::i18n::{Language, tr};

pub fn ctx_label(prompt: &str, key: &str, lang: Language) -> Option<String> {
    let prefix = format!("[{key}]");
    let rest = prompt.strip_prefix(&prefix)?;
    let text = rest.trim().split(['｜', '|']).next().unwrap_or("").trim();
    let lower = text.to_lowercase();
    let label = if text.starts_with('救') || lower.starts_with("save ") {
        tr(lang, "救人", "Save")
    } else if contains_any(text, &["搜刮", "翻找", "搜"])
        || contains_any(&lower, &["search", "loot", "rummage"])
    {
        tr(lang, "搜", "Search")
    } else if text.contains("關門") || lower.contains("close door") {
        tr(lang, "關門", "Close")
    } else if text.contains("開門")
        || text.contains('撬')
        || lower.contains("open door")
        || lower.contains("pry")
    {
        tr(lang, "開門", "Open")
    } else if text.contains("爬上") || lower.contains("climb to") || lower.contains("climb up") {
        tr(lang, "爬上", "Climb")
    } else if contains_any(text, &["下樓", "爬下", "下到"])
        || contains_any(
            &lower,
            &[
                "go downstairs",
                "climb down",
                "descend",
                "take the stairs",
                "down to the street",
            ],
        )
    {
        tr(lang, "下樓", "Down")
    } else if text.contains("上樓") || lower.contains("go upstairs") {
        tr(lang, "上樓", "Up")
    } else if text.contains("下車") || contains_any(&lower, &["get off", "get out", "exit vehicle"])
    {
        tr(lang, "下車", "Exit")
    } else if ride_label(text, &lower) {
        tr(lang, "上車", "Ride")
    } else if contains_any(text, &["捷運", "進站", "搭乘", "月台", "回地面"])
        || contains_any(
            &lower,
            &[
                "metro",
                "station",
                "platform",
                "return to street",
                "orange line",
                "return to the surface",
            ],
        )
    {
        tr(lang, "捷運", "Metro")
    } else if text.contains("避難所") || lower.contains("shelter") {
        tr(lang, "避難", "Shelter")
    } else if contains_any(text, &["購", "買", "商店", "交易", "換"])
        || contains_any(&lower, &["shop", "buy", "trade", "barter", "insert coins"])
    {
        tr(lang, "交易", "Trade")
    } else if contains_any(text, &["交談", "對話", "聯絡人", "說話"])
        || contains_any(&lower, &["talk", "speak", "contact"])
    {
        tr(lang, "交談", "Talk")
    } else if contains_any(text, &["撿", "拿"]) || contains_any(&lower, &["pick up", "take"]) {
        tr(lang, "撿", "Take")
    } else if lower.starts_with("inspect") {
        tr(lang, "查看", "Inspect")
    } else {
        return Some(fallback_label(text, lang));
    };
    Some(label.to_string())
}

pub fn ctx_hint(prompt: &str, key: &str) -> String {
    let prefix = format!("[{key}]");
    let Some(rest) = prompt.strip_prefix(&prefix) else {
        return String::new();
    };
    rest.split(['｜', '|'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn ride_label(text: &str, lower: &str) -> bool {
    if contains_any(text, &["騎", "開車", "上車"]) || text.starts_with('開') {
        return true;
    }
    let vehicle = !lower.contains("line") && !lower.contains("metro") && !lower.contains("train");
    vehicle
        && (lower.starts_with("ride")
            || lower.starts_with("drive")
            || (lower.starts_with("board") && vehicle))
}

fn fallback_label(text: &str, lang: Language) -> String {
    let trimmed = text.split(['（', '(']).next().unwrap_or(text).trim();
    let short: String = trimmed.chars().take(2).collect();
    if short.is_empty() || lang == Language::En {
        tr(lang, "互動", "Interact").to_string()
    } else {
        short
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_original_table() {
        let lang = Language::ZhTw;
        assert_eq!(
            ctx_label("[E] 搜刮 便利商店｜[B] 封門", "E", lang).as_deref(),
            Some("搜")
        );
        assert_eq!(
            ctx_label("[E] 救阿財（剩 45 秒）", "E", lang).as_deref(),
            Some("救人")
        );
        assert_eq!(ctx_label("[E] 開門", "E", lang).as_deref(), Some("開門"));
        assert_eq!(
            ctx_label("[E] 捷運進站", "E", lang).as_deref(),
            Some("捷運")
        );
        assert_eq!(
            ctx_label("[E] inspect the crate", "E", Language::En).as_deref(),
            Some("Inspect")
        );
        assert_eq!(ctx_label("搜刮", "E", lang), None);
        assert_eq!(
            ctx_hint("[E] 搜刮 便利商店｜[B] 封門", "E"),
            "搜刮 便利商店"
        );
    }

    #[test]
    fn ride_does_not_swallow_the_metro() {
        assert_eq!(
            ctx_label("[E] 搭乘捷運", "E", Language::ZhTw).as_deref(),
            Some("捷運")
        );
        assert_eq!(
            ctx_label("[E] 騎機車", "E", Language::ZhTw).as_deref(),
            Some("上車")
        );
    }
}
