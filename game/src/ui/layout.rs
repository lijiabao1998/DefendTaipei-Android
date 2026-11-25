//! Title and death-screen geometry. Buttons stay large enough to tap.
//! Portrait stacks the six survivors in two columns; a wide landscape window
//! uses one row.

use bevy::prelude::*;

use crate::game_flow::GameFlow;
use crate::input::{Orientation, ScreenRect, orientation, safe_insets};

/// Home and survivor pages share the safe-area bounds but use different
/// compositions. The short landscape layout puts the profile beside the cards.
#[derive(Clone, Debug, PartialEq)]
pub struct TitleLayout {
    pub orientation: Orientation,
    pub frame: ScreenRect,
    pub heading: ScreenRect,
    pub steps: ScreenRect,
    pub male: ScreenRect,
    pub female: ScreenRect,
    pub lang_zh: ScreenRect,
    pub lang_en: ScreenRect,
    pub roles: [ScreenRect; 6],
    pub detail: ScreenRect,
    pub back: ScreenRect,
    pub start: ScreenRect,
    pub save_status: ScreenRect,
    pub heading_font: f32,
    pub body_font: f32,
    pub compact: bool,
}

pub fn title_layout(viewport: Vec2) -> TitleLayout {
    let orientation = orientation(viewport);
    let safe = safe_insets(viewport);
    let portrait = orientation == Orientation::Portrait;
    let compact = viewport.y < 700.0;
    let width = (viewport.x - safe.left - safe.right).clamp(1.0, 1120.0);
    let x = (viewport.x - width) * 0.5;
    let top = safe.top;
    let bottom = viewport.y - safe.bottom;
    let gap = if compact { 8.0 } else { 12.0 };
    let action_h = if portrait || !compact { 56.0 } else { 48.0 };
    let back_w = if portrait { width * 0.34 } else { 150.0 };
    let start_w = if portrait {
        width - back_w - gap
    } else {
        260.0
    };
    let back = ScreenRect::new(x, bottom - action_h, back_w, action_h);
    let start = ScreenRect::new(x + width - start_w, back.min.y, start_w, action_h);
    let save_status = ScreenRect::new(x, back.min.y - 28.0, width, 20.0);
    let lang_w = if portrait { 58.0 } else { 74.0 };
    let lang_en = ScreenRect::new(x + width - lang_w, top, lang_w, 32.0);
    let lang_zh = ScreenRect::new(lang_en.min.x - lang_w - 6.0, top, lang_w, 32.0);
    let heading = ScreenRect::new(x, top + 40.0, width, if compact { 28.0 } else { 36.0 });
    let steps = ScreenRect::new(x, heading.max.y + 6.0, width, 22.0);
    let male = ScreenRect::new(x, steps.max.y + 10.0, 64.0, 34.0);
    let female = ScreenRect::new(x + 70.0, male.min.y, 64.0, 34.0);
    let roles_top = male.max.y + 10.0;
    let detail_h = if compact { 100.0 } else { 140.0 };
    let (grid_w, grid_bottom, detail) = if portrait {
        let detail = ScreenRect::new(x, save_status.min.y - gap - detail_h, width, detail_h);
        (width, detail.min.y - gap, detail)
    } else {
        let detail_w = (width * 0.28).clamp(208.0, 300.0);
        let detail = ScreenRect::new(
            x + width - detail_w,
            male.min.y,
            detail_w,
            save_status.min.y - male.min.y - gap,
        );
        (width - detail_w - gap, save_status.min.y - gap, detail)
    };
    let columns = if portrait && !compact { 2 } else { 3 };
    let rows = 6 / columns;
    let card_w = (grid_w - gap * (columns - 1) as f32) / columns as f32;
    let card_h = (grid_bottom - roles_top - gap * (rows - 1) as f32) / rows as f32;
    let roles = std::array::from_fn(|index| {
        ScreenRect::new(
            x + (index % columns) as f32 * (card_w + gap),
            roles_top + (index / columns) as f32 * (card_h + gap),
            card_w,
            card_h,
        )
    });
    TitleLayout {
        orientation,
        frame: ScreenRect::new(x, top, width, bottom - top),
        heading,
        steps,
        male,
        female,
        lang_zh,
        lang_en,
        roles,
        detail,
        back,
        start,
        save_status,
        heading_font: if compact { 23.0 } else { 28.0 },
        body_font: if compact { 12.0 } else { 14.0 },
        compact,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HomeLayout {
    pub frame: ScreenRect,
    pub eyebrow: ScreenRect,
    pub heading: ScreenRect,
    pub subtitle: ScreenRect,
    pub intro: ScreenRect,
    pub facts: ScreenRect,
    pub continue_game: ScreenRect,
    pub new_game: ScreenRect,
    pub help: ScreenRect,
    pub about: ScreenRect,
    pub notice: ScreenRect,
    pub footer: ScreenRect,
    pub lang_zh: ScreenRect,
    pub lang_en: ScreenRect,
    pub title_font: f32,
    pub body_font: f32,
}

pub fn home_layout(viewport: Vec2) -> HomeLayout {
    let safe = safe_insets(viewport);
    let portrait = orientation(viewport) == Orientation::Portrait;
    let compact = viewport.y < 700.0;
    let width = (viewport.x - safe.left - safe.right).clamp(1.0, 1120.0);
    let x = (viewport.x - width) * 0.5;
    let top = safe.top;
    let bottom = viewport.y - safe.bottom;
    let menu_w = if portrait {
        width
    } else {
        (width * 0.38).min(370.0)
    };
    let menu_x = x + width - menu_w;
    let menu_top = if portrait {
        bottom - 242.0
    } else {
        (top + (bottom - top - 242.0) * 0.5).max(top + 42.0)
    };
    let continue_game = ScreenRect::new(menu_x, menu_top, menu_w, 58.0);
    let new_game = ScreenRect::new(menu_x, menu_top + 68.0, menu_w, 58.0);
    let help = ScreenRect::new(menu_x, menu_top + 138.0, (menu_w - 10.0) * 0.5, 44.0);
    let about = ScreenRect::new(help.max.x + 10.0, help.min.y, help.width(), help.height());
    let notice = ScreenRect::new(menu_x, menu_top + 190.0, menu_w, 28.0);
    let footer = ScreenRect::new(x, bottom - 16.0, width, 16.0);
    let lang_w = if portrait { 58.0 } else { 74.0 };
    let lang_en = ScreenRect::new(x + width - lang_w, top, lang_w, 32.0);
    let lang_zh = ScreenRect::new(lang_en.min.x - lang_w - 6.0, top, lang_w, 32.0);
    let hero_w = if portrait {
        width
    } else {
        width - menu_w - 40.0
    };
    let hero_top = if portrait {
        top + if compact { 46.0 } else { 78.0 }
    } else {
        top + 56.0
    };
    let title_font = if portrait {
        if compact { 46.0 } else { 62.0 }
    } else if compact {
        51.0
    } else {
        72.0
    };
    let heading = ScreenRect::new(x, hero_top, hero_w, title_font * 1.35);
    let subtitle = ScreenRect::new(x, heading.max.y + 4.0, hero_w, 22.0);
    let intro = ScreenRect::new(
        x,
        subtitle.max.y + if compact { 14.0 } else { 32.0 },
        hero_w,
        if compact { 52.0 } else { 82.0 },
    );
    let facts = ScreenRect::new(
        x,
        intro.max.y + 16.0,
        hero_w,
        if compact { 22.0 } else { 48.0 },
    );
    HomeLayout {
        frame: ScreenRect::new(x, top, width, bottom - top),
        eyebrow: ScreenRect::new(x, top + 6.0, (width - lang_w * 2.0 - 18.0).max(1.0), 20.0),
        heading,
        subtitle,
        intro,
        facts,
        continue_game,
        new_game,
        help,
        about,
        notice,
        footer,
        lang_zh,
        lang_en,
        title_font,
        body_font: if compact { 12.0 } else { 15.0 },
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BriefLayout {
    pub identity: ScreenRect,
    pub supplies: ScreenRect,
    pub notice: ScreenRect,
}

pub fn brief_layout(viewport: Vec2) -> BriefLayout {
    let title = title_layout(viewport);
    let x = title.frame.min.x;
    let width = title.frame.width();
    let top = title.steps.max.y + 16.0;
    let bottom = title.save_status.min.y - 12.0;
    let height = bottom - top;
    let gap = 12.0;
    let (identity, supplies) = if title.orientation == Orientation::Portrait {
        let identity_h = (height * 0.43).min(250.0);
        (
            ScreenRect::new(x, top, width, identity_h),
            ScreenRect::new(x, top + identity_h + gap, width, height - identity_h - gap),
        )
    } else {
        let left_w = width * 0.43;
        (
            ScreenRect::new(x, top, left_w, height),
            ScreenRect::new(x + left_w + gap, top, width - left_w - gap, height),
        )
    };
    BriefLayout {
        identity,
        supplies,
        notice: title.save_status,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeathLayout {
    pub heading: ScreenRect,
    pub sub: ScreenRect,
    pub body: ScreenRect,
    pub restart: ScreenRect,
    pub heading_font: f32,
    pub body_font: f32,
}

pub fn death_layout(viewport: Vec2) -> DeathLayout {
    let insets = safe_insets(viewport);
    let portrait = orientation(viewport) == Orientation::Portrait;
    let width = (viewport.x - insets.left - insets.right).max(1.0);
    let top = insets.top + 18.0;
    let heading = ScreenRect::new(insets.left, top, width, if portrait { 100.0 } else { 62.0 });
    let sub = ScreenRect::new(
        insets.left,
        heading.max.y + 8.0,
        width,
        if portrait { 64.0 } else { 48.0 },
    );
    let button_w = width.min(460.0);
    let restart = ScreenRect::new(
        insets.left + (width - button_w) * 0.5,
        viewport.y - insets.bottom - 80.0,
        button_w,
        72.0,
    );
    let body = ScreenRect::new(
        insets.left,
        sub.max.y + 8.0,
        width,
        (restart.min.y - sub.max.y - 24.0).max(1.0),
    );
    DeathLayout {
        heading,
        sub,
        body,
        restart,
        heading_font: if portrait { 26.0 } else { 30.0 },
        body_font: if viewport.y < 500.0 { 14.0 } else { 16.0 },
    }
}

/// Same column as the ending screen, shifted down so the prologue phone card
/// (drawn above this UI) does not cover the heading.
pub fn prologue_layout(viewport: Vec2) -> DeathLayout {
    overlay_layout(viewport, 88.0)
}

fn overlay_layout(viewport: Vec2, top_gap: f32) -> DeathLayout {
    let insets = safe_insets(viewport);
    let portrait = orientation(viewport) == Orientation::Portrait;
    let inner_w = (viewport.x - insets.left - insets.right).max(1.0);
    let x = insets.left;
    let top = insets.top + top_gap;
    let heading = ScreenRect::new(x, top, inner_w, 48.0);
    let sub = ScreenRect::new(x, heading.max.y + 8.0, inner_w, 32.0);
    let body = ScreenRect::new(x, sub.max.y + 8.0, inner_w, 48.0);
    let button_w = inner_w.min(if portrait { inner_w } else { 460.0 });
    let restart = ScreenRect::new(
        x + (inner_w - button_w) * 0.5,
        body.max.y + 16.0,
        button_w,
        72.0,
    );
    DeathLayout {
        heading,
        sub,
        body,
        restart,
        heading_font: if portrait { 32.0 } else { 40.0 },
        body_font: if portrait { 16.0 } else { 18.0 },
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PauseLayout {
    pub heading: ScreenRect,
    pub resume: ScreenRect,
    pub menu: ScreenRect,
    pub status: ScreenRect,
}

pub fn pause_layout(viewport: Vec2) -> PauseLayout {
    let insets = safe_insets(viewport);
    let inner_w = (viewport.x - insets.left - insets.right).max(1.0);
    let button_w = inner_w.min(420.0);
    let x = insets.left + (inner_w - button_w) * 0.5;
    let height = 72.0;
    let gap = 12.0;
    let top = ((viewport.y - height * 2.0 - gap) * 0.5).max(insets.top + 48.0);
    PauseLayout {
        heading: ScreenRect::new(x, (top - 48.0).max(insets.top), button_w, 40.0),
        resume: ScreenRect::new(x, top, button_w, height),
        menu: ScreenRect::new(x, top + height + gap, button_w, height),
        status: ScreenRect::new(x, top + height * 2.0 + gap + 8.0, button_w, 56.0),
    }
}

/// True when a pointer press lands on a screen this UI owns.
/// Those presses are consumed so they do not fall through to the flow overlay.
pub fn control_hit(flow: &GameFlow, _viewport: Vec2, _point: Vec2, paused: bool) -> bool {
    match flow {
        GameFlow::Title | GameFlow::Prologue | GameFlow::GameOver(_) => true,
        GameFlow::Playing => paused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(rect: ScreenRect, viewport: Vec2) {
        let insets = safe_insets(viewport);
        assert!(rect.min.x >= insets.left - 1.0, "{rect:?} {viewport:?}");
        assert!(rect.max.x <= viewport.x - insets.right + 1.0, "{rect:?}");
        assert!(rect.min.y >= -1.0, "{rect:?}");
        assert!(rect.max.y <= viewport.y + 1.0, "{rect:?}");
        assert!(rect.width() > 8.0 && rect.height() > 8.0, "{rect:?}");
    }

    const VIEWPORTS: [Vec2; 7] = [
        Vec2::new(320.0, 568.0),
        Vec2::new(320.0, 640.0),
        Vec2::new(390.0, 844.0),
        Vec2::new(720.0, 1280.0),
        Vec2::new(800.0, 360.0),
        Vec2::new(844.0, 390.0),
        Vec2::new(1280.0, 720.0),
    ];

    #[test]
    fn title_cards_and_navigation_fit_small_and_rotated_screens() {
        for viewport in VIEWPORTS {
            let layout = title_layout(viewport);
            assert!(layout.start.height() >= 48.0, "{viewport:?}");
            for rect in [
                layout.heading,
                layout.steps,
                layout.start,
                layout.back,
                layout.male,
                layout.female,
                layout.lang_zh,
                layout.lang_en,
                layout.detail,
                layout.save_status,
            ] {
                inside(rect, viewport);
            }
            assert!(!layout.start.intersects(layout.back));
            assert!(layout.start.max.y <= viewport.y - safe_insets(viewport).bottom + 1.0);
            for (index, card) in layout.roles.iter().enumerate() {
                inside(*card, viewport);
                assert!(card.height() >= 40.0, "{viewport:?}: {card:?}");
                for other in layout.roles.iter().skip(index + 1) {
                    assert!(!card.intersects(*other));
                }
                for other in [
                    layout.detail,
                    layout.save_status,
                    layout.start,
                    layout.back,
                    layout.male,
                    layout.female,
                ] {
                    assert!(
                        !card.intersects(other),
                        "{viewport:?}: {card:?} overlaps {other:?}"
                    );
                }
            }
            let brief = brief_layout(viewport);
            for rect in [brief.identity, brief.supplies, brief.notice] {
                inside(rect, viewport);
            }
            assert!(!brief.identity.intersects(brief.supplies));
            assert!(!brief.supplies.intersects(layout.start));
        }
        let tall = title_layout(Vec2::new(390.0, 844.0));
        assert_eq!(tall.orientation, Orientation::Portrait);
        assert!(tall.roles[1].min.x > tall.roles[0].max.x);
        assert!(tall.roles[2].min.y > tall.roles[0].max.y);
        let wide = title_layout(Vec2::new(1280.0, 720.0));
        assert_eq!(wide.orientation, Orientation::Landscape);
        assert_eq!(wide.roles[1].min.y, wide.roles[0].min.y);
    }

    #[test]
    fn home_has_separate_finger_targets_without_covering_the_story() {
        for viewport in VIEWPORTS {
            let layout = home_layout(viewport);
            let buttons = [
                layout.continue_game,
                layout.new_game,
                layout.help,
                layout.about,
            ];
            for (index, rect) in buttons.iter().enumerate() {
                inside(*rect, viewport);
                assert!(rect.height() >= 44.0);
                for other in buttons.iter().skip(index + 1) {
                    assert!(!rect.intersects(*other));
                }
                for other in [layout.heading, layout.subtitle, layout.intro, layout.facts] {
                    assert!(
                        !rect.intersects(other),
                        "{viewport:?}: {rect:?} overlaps {other:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn death_restart_is_a_finger_button() {
        for viewport in [Vec2::new(1280.0, 720.0), Vec2::new(390.0, 844.0)] {
            let layout = death_layout(viewport);
            assert!(layout.restart.height() >= 64.0);
            inside(layout.restart, viewport);
        }
    }

    #[test]
    fn menu_screens_consume_the_pointer() {
        let viewport = Vec2::new(1280.0, 720.0);
        let female = title_layout(viewport).female.center();
        assert!(control_hit(&GameFlow::Title, viewport, female, false));
        assert!(control_hit(&GameFlow::Prologue, viewport, female, false));
        assert!(control_hit(
            &GameFlow::GameOver(crate::game_flow::Ending::Death),
            viewport,
            female,
            false
        ));
        assert!(!control_hit(&GameFlow::Playing, viewport, female, false));
        assert!(control_hit(&GameFlow::Playing, viewport, female, true));
        assert!(pause_layout(viewport).resume.height() >= 64.0);
    }
}
