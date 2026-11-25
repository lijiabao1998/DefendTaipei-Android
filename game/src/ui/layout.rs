//! Title and death-screen geometry. Buttons stay large enough to tap.
//! Portrait stacks the six survivors in two columns; a wide landscape window
//! uses one row.

use bevy::prelude::*;

use crate::game_flow::GameFlow;
use crate::input::{Orientation, ScreenRect, orientation, safe_insets};

#[derive(Clone, Debug, PartialEq)]
pub struct TitleLayout {
    pub orientation: Orientation,
    pub heading: ScreenRect,
    pub intro: ScreenRect,
    pub male: ScreenRect,
    pub female: ScreenRect,
    pub lang_zh: ScreenRect,
    pub lang_en: ScreenRect,
    pub roles: [ScreenRect; 6],
    pub detail: ScreenRect,
    pub start: ScreenRect,
    pub heading_font: f32,
    pub body_font: f32,
    pub button_font: f32,
}

pub fn title_layout(viewport: Vec2) -> TitleLayout {
    let orient = orientation(viewport);
    let insets = safe_insets(viewport);
    let portrait = orient == Orientation::Portrait;
    let gap = if portrait { 8.0 } else { 10.0 };
    let x = insets.left;
    let inner_w = (viewport.x - insets.left - insets.right).max(1.0);
    let bottom_limit = viewport.y - insets.bottom - 8.0;
    let start_h = 72.0;
    let start_w = if portrait {
        inner_w
    } else {
        inner_w.min(420.0)
    };
    let start = ScreenRect::new(
        x + (inner_w - start_w) * 0.5,
        bottom_limit - start_h,
        start_w,
        start_h,
    );

    let mut y = insets.top + 6.0;
    let heading = ScreenRect::new(x, y, inner_w, if portrait { 36.0 } else { 44.0 });
    y = heading.max.y + gap;

    let lang_w = 96.0_f32.min(inner_w * 0.22).max(64.0);
    let row_h = 48.0;
    let gender_w = ((inner_w - gap * 3.0 - lang_w * 2.0) * 0.5).max(64.0);
    let male = ScreenRect::new(x, y, gender_w, row_h);
    let female = ScreenRect::new(x + gender_w + gap, y, gender_w, row_h);
    let lang_en = ScreenRect::new(x + inner_w - lang_w, y, lang_w, row_h);
    let lang_zh = ScreenRect::new((lang_en.min.x - gap - lang_w).max(x), y, lang_w, row_h);
    y = male.max.y + gap;

    let intro = ScreenRect::new(x, y, inner_w, if portrait { 64.0 } else { 44.0 });
    y = intro.max.y + gap;

    let detail_h = if portrait { 64.0 } else { 48.0 };
    let detail_top = (start.min.y - gap - detail_h).max(y);
    let detail = ScreenRect::new(x, detail_top, inner_w, detail_h);
    let roles_top = y;
    let roles_h = (detail.min.y - gap - roles_top).max(40.0);
    let columns: usize = if !portrait && inner_w >= 960.0 {
        6
    } else if portrait || inner_w < 700.0 {
        2
    } else {
        3
    };
    let rows = 6 / columns;
    let card_w = (inner_w - gap * (columns as f32 - 1.0)) / columns as f32;
    let card_h = ((roles_h - gap * (rows as f32 - 1.0)) / rows as f32).max(36.0);
    let mut roles = [ScreenRect::new(0.0, 0.0, 1.0, 1.0); 6];
    for (index, slot) in roles.iter_mut().enumerate() {
        let col = index % columns;
        let row = index / columns;
        *slot = ScreenRect::new(
            x + col as f32 * (card_w + gap),
            roles_top + row as f32 * (card_h + gap),
            card_w,
            card_h,
        );
    }

    TitleLayout {
        orientation: orient,
        heading,
        intro,
        male,
        female,
        lang_zh,
        lang_en,
        roles,
        detail,
        start,
        heading_font: if portrait { 28.0 } else { 36.0 },
        body_font: if portrait { 14.0 } else { 16.0 },
        button_font: if portrait { 18.0 } else { 20.0 },
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
    overlay_layout(viewport, 24.0)
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

    #[test]
    fn title_buttons_fit_both_orientations() {
        for viewport in [
            Vec2::new(1280.0, 720.0),
            Vec2::new(390.0, 844.0),
            Vec2::new(844.0, 390.0),
        ] {
            let layout = title_layout(viewport);
            assert!(layout.start.height() >= 64.0, "{viewport:?}");
            inside(layout.start, viewport);
            assert!(layout.start.max.y <= viewport.y - safe_insets(viewport).bottom + 1.0);
            inside(layout.male, viewport);
            for card in layout.roles {
                inside(card, viewport);
            }
        }
        let tall = title_layout(Vec2::new(390.0, 844.0));
        assert_eq!(tall.orientation, Orientation::Portrait);
        assert!(tall.roles[1].min.x + 1.0 >= tall.roles[0].max.x);
        assert!(tall.roles[2].min.y + 1.0 >= tall.roles[0].max.y);
        let wide = title_layout(Vec2::new(1280.0, 720.0));
        assert_eq!(wide.orientation, Orientation::Landscape);
        assert!((wide.roles[1].min.y - wide.roles[0].min.y).abs() < 1.0);
        assert!(wide.roles[1].min.x + 1.0 >= wide.roles[0].max.x);
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
