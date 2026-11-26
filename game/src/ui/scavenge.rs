//! Selective scavenging uses the same saved stock as the visible furniture.
//! All buttons fit inside the safe viewport; short landscapes paginate one
//! readable card at a time instead of clipping the take / close controls.

use bevy::prelude::*;

use crate::i18n::{Language, tr};
use crate::input::ScreenRect;
use crate::inventory::{Inventory, item};
use crate::save::SaveStatus;
use crate::worldplay::{
    LOOT_CAPACITY_KG, LootTake, ScavengeSession, ScavengeState, WorldNotice, pickable_count,
};

use super::shelter::{Action, Frame, Tone, View, item_category, panel_rect};

pub(super) fn make_frame(
    viewport: Vec2,
    requested_page: usize,
    source: Option<(&ScavengeSession, &ScavengeState)>,
    inventory: &Inventory,
    lang: Language,
    notice: &WorldNotice,
    save: &SaveStatus,
) -> Frame {
    let panel = panel_rect(viewport);
    let mut frame = Frame {
        panel: Some(panel),
        ..default()
    };
    let x = panel.min.x + 10.0;
    let width = (panel.width() - 20.0).max(1.0);
    let top = panel.min.y + 8.0;
    let target = source.and_then(|(session, _)| session.target.as_ref());
    let stock = source.and_then(|(session, progress)| {
        session.target.as_ref().and_then(|target| {
            progress
                .loot
                .iter()
                .find(|(id, _)| id == &target.id)
                .map(|(_, stash)| stash)
        })
    });
    let reachable = source.is_some_and(|(session, _)| session.reachable);
    let entries: Vec<_> = stock
        .into_iter()
        .flat_map(|stock| stock.stacks())
        .filter(|(_, n)| *n > 0)
        .collect();
    let title = target.map_or_else(
        || tr(lang, "搜刮", "Scavenge").into(),
        |target| target.name(lang).to_owned(),
    );
    frame.text(ScreenRect::new(x, top, width - 64.0, 48.0), title, 19.0);
    frame.tone(Tone::Heading);
    frame.button(
        ScreenRect::new(x + width - 60.0, top, 60.0, 42.0),
        tr(lang, "關閉", "Close").into(),
        Action::Close,
        true,
    );

    let mut y = top + 54.0;
    frame.text(
        ScreenRect::new(x, y, width, 28.0),
        format!(
            "{} {:.2} / {:.0} kg · {} {}",
            tr(lang, "背包", "Bag"),
            inventory.weight(),
            LOOT_CAPACITY_KG,
            entries.len(),
            tr(lang, "種物資", "stacks")
        ),
        14.0,
    );
    frame.tone(Tone::Muted);
    y += 30.0;
    if panel.height() > 480.0 {
        frame.text(
            ScreenRect::new(x, y, width, 38.0),
            tr(
                lang,
                "逐件挑選，未拿取的物資會留在原處。世界仍在運作。",
                "Choose what to carry; leftovers stay here. The world keeps running.",
            )
            .into(),
            13.0,
        );
        frame.tone(Tone::Muted);
        y += 42.0;
    }
    let nav_y = panel.max.y - 50.0;
    let action_y = nav_y - 48.0;
    let status_y = action_y - 38.0;
    let area_h = (status_y - y - 4.0).max(1.0);
    let row_h = 74.0_f32.min(area_h);
    let rows_per_page = ((area_h / 74.0).floor() as usize).max(1);
    let page_count = entries.len().div_ceil(rows_per_page).max(1);
    let page = requested_page.min(page_count - 1);
    for (id, count) in entries
        .iter()
        .skip(page * rows_per_page)
        .take(rows_per_page)
        .copied()
    {
        let Some(definition) = item(id) else { continue };
        let can_take = reachable && pickable_count(inventory, id, *count) > 0;
        frame
            .cards
            .push((ScreenRect::new(x, y, width, row_h - 4.0), false));
        let button_w = 56.0;
        let buttons_x = x + width - button_w * 2.0 - 9.0;
        let text_w = (buttons_x - x - 12.0).max(1.0);
        frame.text(
            ScreenRect::new(x + 6.0, y + 4.0, text_w, 34.0),
            format!("{} ×{}", definition.name(lang), count),
            14.0,
        );
        frame.text(
            ScreenRect::new(x + 6.0, y + 39.0, text_w, (row_h - 47.0).max(16.0)),
            format!(
                "{} · {:.2} kg\n{} {}",
                item_category(definition.kind, lang),
                definition.weight * *count as f32,
                tr(lang, "已攜帶", "Carried"),
                inventory.count(id)
            ),
            11.0,
        );
        frame.tone(Tone::Muted);
        frame.button(
            ScreenRect::new(buttons_x, y + 6.0, button_w, (row_h - 16.0).max(36.0)),
            tr(lang, "拿 1", "Take 1").into(),
            Action::Loot(LootTake::One(id.clone())),
            can_take,
        );
        frame.button(
            ScreenRect::new(
                buttons_x + button_w + 5.0,
                y + 6.0,
                button_w,
                (row_h - 16.0).max(36.0),
            ),
            tr(lang, "整疊", "Stack").into(),
            Action::Loot(LootTake::Stack(id.clone())),
            can_take,
        );
        y += row_h;
    }
    if entries.is_empty() {
        frame.text(
            ScreenRect::new(x + 8.0, y + 8.0, width - 16.0, (area_h - 16.0).min(84.0)),
            tr(
                lang,
                "已經搜刮一空。這裡不會因為重開遊戲而補貨。",
                "Already looted. Reloading the game will not refill this container.",
            )
            .into(),
            16.0,
        );
        frame.tone(Tone::Muted);
    }
    let any_fit = reachable
        && entries
            .iter()
            .any(|(id, n)| pickable_count(inventory, id, *n) > 0);
    let status = if !reachable {
        tr(
            lang,
            "已離開物資附近，請關閉後靠近再搜尋。",
            "Out of reach. Close this panel and approach the supplies.",
        )
        .into()
    } else if let Some(error) = &save.error {
        error.clone()
    } else if !notice.text.is_empty() {
        notice.text.clone()
    } else if !entries.is_empty() && !any_fit {
        tr(
            lang,
            "已達負重上限，剩餘物資會留在原處。",
            "Bag full. Remaining supplies stay here.",
        )
        .into()
    } else {
        save.notice.clone()
    };
    frame.text(ScreenRect::new(x, status_y, width, 34.0), status, 12.0);
    frame.tone(Tone::Muted);
    let take_w = width * 0.5;
    let small_w = (width - take_w - 10.0) / 2.0;
    frame.button(
        ScreenRect::new(x, action_y, take_w, 42.0),
        tr(lang, "拿取可攜帶物資", "Take what fits").into(),
        Action::Loot(LootTake::All),
        any_fit,
    );
    frame.button(
        ScreenRect::new(x + take_w + 5.0, action_y, small_w, 42.0),
        tr(lang, "背包", "Bag").into(),
        Action::Open(View::Inventory),
        true,
    );
    frame.button(
        ScreenRect::new(x + take_w + small_w + 10.0, action_y, small_w, 42.0),
        tr(lang, "儲存", "Save").into(),
        Action::Save,
        true,
    );
    let nav_w = (width * 0.3).min(140.0);
    frame.button(
        ScreenRect::new(x, nav_y, nav_w, 42.0),
        tr(lang, "上一頁", "Previous").into(),
        Action::Page(page.saturating_sub(1)),
        page > 0,
    );
    frame.text(
        ScreenRect::new(x + nav_w, nav_y, width - nav_w * 2.0, 42.0),
        format!("{} / {}", page + 1, page_count),
        15.0,
    );
    frame.button(
        ScreenRect::new(x + width - nav_w, nav_y, nav_w, 42.0),
        tr(lang, "下一頁", "Next").into(),
        Action::Page(page + 1),
        page + 1 < page_count,
    );
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survival::Stash;
    use crate::worldplay::LootTarget;

    #[test]
    fn compact_loot_controls_fit_without_overlaps_in_both_languages() {
        let mut stock = Stash::default();
        for id in [
            "water", "rice", "noodle", "bandage", "mag191", "a556", "plank", "cstove",
        ] {
            stock.add(id, 4);
        }
        let session = ScavengeSession {
            target: Some(LootTarget {
                id: "c97".into(),
                label_zh: "泡麵零食貨架".into(),
                label_en: "Noodles and snacks".into(),
            }),
            reachable: true,
        };
        let mut progress = ScavengeState::default();
        progress.loot.push(("c97".into(), stock));
        for viewport in [
            Vec2::new(320.0, 640.0),
            Vec2::new(390.0, 844.0),
            Vec2::new(640.0, 360.0),
            Vec2::new(844.0, 390.0),
        ] {
            for lang in [Language::ZhTw, Language::En] {
                for page in 0..8 {
                    let frame = make_frame(
                        viewport,
                        page,
                        Some((&session, &progress)),
                        &Inventory::default(),
                        lang,
                        &WorldNotice::default(),
                        &SaveStatus::default(),
                    );
                    let panel = frame.panel.unwrap();
                    let controls: Vec<_> = frame
                        .widgets
                        .iter()
                        .filter(|widget| widget.action.is_some())
                        .collect();
                    for (index, widget) in controls.iter().enumerate() {
                        assert!(
                            widget.rect.min.cmpge(panel.min).all()
                                && widget.rect.max.cmple(panel.max).all(),
                            "{viewport:?}: {:?}",
                            widget.action
                        );
                        assert!(widget.rect.width() >= 44.0 && widget.rect.height() >= 36.0);
                        for other in &controls[index + 1..] {
                            let overlap = widget.rect.min.cmplt(other.rect.max).all()
                                && other.rect.min.cmplt(widget.rect.max).all();
                            assert!(
                                !overlap,
                                "{viewport:?}: {:?} overlaps {:?}",
                                widget.action, other.action
                            );
                        }
                    }
                    assert!(
                        controls
                            .iter()
                            .any(|widget| widget.action == Some(Action::Close))
                    );
                    assert!(
                        controls.iter().any(|widget| matches!(
                            widget.action,
                            Some(Action::Loot(LootTake::All))
                        ))
                    );
                }
            }
        }
    }

    #[test]
    fn empty_full_or_unreachable_stock_disables_all_pickup_controls() {
        let target = LootTarget {
            id: "c97".into(),
            label_zh: "貨架".into(),
            label_en: "Shelf".into(),
        };
        for mode in 0..3 {
            let session = ScavengeSession {
                target: Some(target.clone()),
                reachable: mode != 2,
            };
            let mut stock = Stash::default();
            if mode != 0 {
                stock.add("water", 5);
            }
            let mut progress = ScavengeState::default();
            progress.loot.push((target.id.clone(), stock));
            let mut inventory = Inventory::default();
            if mode == 1 {
                inventory.add("plank", 15);
            }
            let frame = make_frame(
                Vec2::new(390.0, 844.0),
                0,
                Some((&session, &progress)),
                &inventory,
                Language::ZhTw,
                &WorldNotice::default(),
                &SaveStatus::default(),
            );
            assert!(
                frame
                    .widgets
                    .iter()
                    .filter(|widget| matches!(widget.action, Some(Action::Loot(_))))
                    .all(|widget| !widget.enabled)
            );
            assert!(
                frame
                    .widgets
                    .iter()
                    .any(|widget| widget.action == Some(Action::Close) && widget.enabled)
            );
        }
    }

    #[test]
    fn android_scavenge_smoke_points_hit_actual_pickup_and_close_controls() {
        let session = ScavengeSession {
            target: Some(LootTarget {
                id: "x814_3".into(),
                label_zh: "結帳櫃台".into(),
                label_en: "Checkout counter".into(),
            }),
            reachable: true,
        };
        let mut stock = Stash::default();
        stock.add("water", 3);
        stock.add("can", 2);
        let mut progress = ScavengeState::default();
        progress.loot.push(("x814_3".into(), stock));
        let frame = make_frame(
            Vec2::new(390.0, 844.0),
            0,
            Some((&session, &progress)),
            &Inventory::default(),
            Language::ZhTw,
            &WorldNotice::default(),
            &SaveStatus::default(),
        );
        for (action, point) in [
            (
                Action::Loot(LootTake::One("water".into())),
                Vec2::new(271.0, 217.0),
            ),
            (
                Action::Loot(LootTake::Stack("water".into())),
                Vec2::new(332.0, 217.0),
            ),
            (Action::Loot(LootTake::All), Vec2::new(110.0, 731.0)),
            (Action::Save, Vec2::new(324.0, 731.0)),
            (Action::Close, Vec2::new(334.0, 77.0)),
        ] {
            let button = frame
                .widgets
                .iter()
                .find(|widget| widget.action.as_ref() == Some(&action))
                .unwrap();
            assert!(
                button.enabled && button.rect.contains(point),
                "{action:?}: {:?}",
                button.rect
            );
        }
    }
}
