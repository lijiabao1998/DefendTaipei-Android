//! On-screen control placement, taken from `src/ui/style.css`.
//!
//! The joystick is a fixed pad (`#joy`, 130px, radius 55), not a floating stick.
//! Portrait adds the original `#pbact` (88px) and `#pbsec` (60px) buttons and
//! drops the pad from `bottom: 28px` to `bottom: 52px`.

use bevy::prelude::*;

pub const JOY_SIZE: f32 = 130.0;
pub const JOY_RADIUS: f32 = 55.0;
pub const JOY_KNOB: f32 = 56.0;
/// Below this stick length the original ignores the pad (`jm > 0.12`).
pub const DEAD_ZONE: f32 = 0.12;
/// At or above this deflection the original starts running.
pub const RUN_AT: f32 = 0.93;
pub const ACT_SIZE: f32 = 88.0;
pub const SEC_SIZE: f32 = 60.0;
/// Long-press on 蹲 before it becomes a retreat, in seconds.
pub const CROUCH_HOLD: f32 = 0.45;
pub const RETREAT_SECS: f32 = 1.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Landscape,
    Portrait,
}

pub fn orientation(viewport: Vec2) -> Orientation {
    if viewport.y > viewport.x {
        Orientation::Portrait
    } else {
        Orientation::Landscape
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Insets {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

/// Stand-in for CSS `env(safe-area-inset-*)`. Landscape keeps the side edges
/// clear; portrait keeps the top and the home indicator clear.
pub fn safe_insets(viewport: Vec2) -> Insets {
    let (left, right, top, bottom): (f32, f32, f32, f32) = match orientation(viewport) {
        Orientation::Landscape => (36.0, 36.0, 14.0, 16.0),
        Orientation::Portrait => (16.0, 16.0, 44.0, 32.0),
    };
    let max_x = viewport.x * 0.12;
    let max_y = viewport.y * 0.10;
    Insets {
        left: left.min(max_x).max(0.0),
        right: right.min(max_x).max(0.0),
        top: top.min(max_y).max(0.0),
        bottom: bottom.min(max_y).max(0.0),
    }
}

/// Window-space rectangle, origin at the top-left, +y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenRect {
    pub min: Vec2,
    pub max: Vec2,
}

impl ScreenRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            min: Vec2::new(x, y),
            max: Vec2::new(x + w, y + h),
        }
    }

    pub fn width(self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn center(self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    pub fn contains(self, point: Vec2) -> bool {
        point.x >= self.min.x
            && point.x < self.max.x
            && point.y >= self.min.y
            && point.y < self.max.y
    }

    pub fn intersects(self, other: Self) -> bool {
        self.min.x < other.max.x
            && other.min.x < self.max.x
            && self.min.y < other.max.y
            && other.min.y < self.max.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudLayout {
    pub orientation: Orientation,
    pub insets: Insets,
    pub joy: ScreenRect,
    /// Portrait attack / interact button. Hidden in landscape, matching `#pbtns`.
    pub act: Option<ScreenRect>,
    /// Portrait 蹲 button.
    pub sec: Option<ScreenRect>,
}

impl HudLayout {
    /// Existing button centers remain usable touch points.
    /// Original Web sizes scale only the two thumb buttons, not the joystick.
    /// A driver needs both buttons in either orientation.
    pub fn for_controls(viewport: Vec2, button_percent: u8, driving: bool) -> Self {
        let mut layout = Self::from_viewport(viewport);
        if driving && layout.orientation == Orientation::Landscape {
            let bottom = viewport.y - layout.insets.bottom - 54.0;
            let right = viewport.x - layout.insets.right - 16.0;
            layout.act = Some(ScreenRect::new(
                right - ACT_SIZE,
                bottom - ACT_SIZE,
                ACT_SIZE,
                ACT_SIZE,
            ));
            layout.sec = Some(ScreenRect::new(
                right - SEC_SIZE - 14.0,
                bottom - ACT_SIZE - SEC_SIZE - 8.0,
                SEC_SIZE,
                SEC_SIZE,
            ));
        }
        let scale = match button_percent {
            85 => 0.85,
            110 => 1.1,
            _ => 1.0,
        };
        for rect in [&mut layout.act, &mut layout.sec].into_iter().flatten() {
            let center = rect.center();
            let size = (rect.max - rect.min) * scale;
            rect.min = center - size * 0.5;
            rect.max = center + size * 0.5;
        }
        // The scaled target stays clear of the bottom toolbar. The original
        // portrait CSS otherwise overlaps its lower edge by six pixels.
        let bottom = viewport.y - layout.insets.bottom - 50.0;
        let shift = layout
            .act
            .map_or(0.0, |rect| (rect.max.y - bottom).max(0.0));
        for rect in [&mut layout.act, &mut layout.sec].into_iter().flatten() {
            rect.min.y -= shift;
            rect.max.y -= shift;
        }
        layout
    }

    pub fn from_viewport(viewport: Vec2) -> Self {
        let orientation = orientation(viewport);
        let insets = safe_insets(viewport);
        let left = 20.0 + insets.left;
        let bottom_css = match orientation {
            Orientation::Portrait => 52.0,
            Orientation::Landscape => 28.0,
        };
        let bottom = bottom_css + insets.bottom;
        let joy_top = viewport.y - bottom - JOY_SIZE;
        let joy = ScreenRect::new(left, joy_top, JOY_SIZE, JOY_SIZE);
        let (act, sec) = match orientation {
            Orientation::Landscape => (None, None),
            Orientation::Portrait => {
                let act_right = 16.0 + insets.right;
                let act_bottom = 40.0 + insets.bottom;
                let act = ScreenRect::new(
                    viewport.x - act_right - ACT_SIZE,
                    viewport.y - act_bottom - ACT_SIZE,
                    ACT_SIZE,
                    ACT_SIZE,
                );
                let sec_right = 30.0 + insets.right;
                let sec_bottom = 56.0 + ACT_SIZE + insets.bottom;
                let sec = ScreenRect::new(
                    viewport.x - sec_right - SEC_SIZE,
                    viewport.y - sec_bottom - SEC_SIZE,
                    SEC_SIZE,
                    SEC_SIZE,
                );
                (Some(act), Some(sec))
            }
        };
        Self {
            orientation,
            insets,
            joy,
            act,
            sec,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_is_landscape_and_tall_is_portrait() {
        assert_eq!(orientation(Vec2::new(800.0, 800.0)), Orientation::Landscape);
        assert_eq!(orientation(Vec2::new(390.0, 844.0)), Orientation::Portrait);
    }

    #[test]
    fn joystick_follows_the_stylesheet() {
        let tall = HudLayout::from_viewport(Vec2::new(390.0, 844.0));
        let wide = HudLayout::from_viewport(Vec2::new(844.0, 390.0));
        assert_eq!(tall.joy.width(), JOY_SIZE);
        let tall_bottom = 844.0 - tall.joy.max.y;
        let wide_bottom = 390.0 - wide.joy.max.y;
        assert!(tall_bottom > wide_bottom);
        assert!(tall.act.is_some() && tall.sec.is_some());
        assert!(wide.act.is_none() && wide.sec.is_none());
        let act = tall.act.unwrap();
        let sec = tall.sec.unwrap();
        assert_eq!(act.width(), ACT_SIZE);
        assert_eq!(sec.width(), SEC_SIZE);
        assert!(!tall.joy.intersects(act));
        assert!(!tall.joy.intersects(sec));
        assert!(!act.intersects(sec));
        assert!(act.min.x > tall.joy.max.x);
    }

    #[test]
    fn insets_shrink_on_a_tiny_window() {
        let insets = safe_insets(Vec2::new(200.0, 180.0));
        assert!(insets.left + insets.right < 200.0);
        assert!(insets.top + insets.bottom < 180.0);
    }

    #[test]
    fn driver_buttons_scale_without_touching_each_other_or_the_toolbar() {
        for size in [
            Vec2::new(390.0, 844.0),
            Vec2::new(844.0, 390.0),
            Vec2::new(360.0, 640.0),
        ] {
            for percent in [85, 100, 110] {
                let layout = HudLayout::for_controls(size, percent, true);
                let act = layout.act.unwrap();
                let sec = layout.sec.unwrap();
                assert!(!act.intersects(sec));
                assert!(!act.intersects(layout.joy));
                assert!(!sec.intersects(layout.joy));
                assert!(act.max.y < size.y - layout.insets.bottom - 46.0);
                assert!(
                    act.min.x >= layout.insets.left && act.max.x <= size.x - layout.insets.right
                );
            }
        }
        let default = HudLayout::for_controls(Vec2::new(390.0, 844.0), 100, false);
        let original = HudLayout::from_viewport(Vec2::new(390.0, 844.0));
        assert_eq!(default.joy, original.joy);
        assert!(
            default
                .act
                .unwrap()
                .contains(original.act.unwrap().center())
        );
        assert!(
            default
                .sec
                .unwrap()
                .contains(original.sec.unwrap().center())
        );
    }
}
