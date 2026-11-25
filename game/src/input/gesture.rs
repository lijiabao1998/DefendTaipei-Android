//! Fixed-pad joystick and the portrait 打 / 蹲 buttons.
//!
//! `joy.x` / `joy.y` match `joystick.ts`: both are in [-1, 1], and +y is down the
//! screen. Gameplay movement flips that Y so +y is up.

use bevy::prelude::*;

use super::layout::{CROUCH_HOLD, DEAD_ZONE, HudLayout, JOY_RADIUS, RUN_AT};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    pub id: u64,
    pub position: Vec2,
    pub just_pressed: bool,
}

/// One sample of `#joy`, in the original's coordinate system.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JoySample {
    pub x: f32,
    pub y: f32,
    pub knob_dx: f32,
    pub knob_dy: f32,
}

impl JoySample {
    pub fn deflection(self) -> f32 {
        self.x.hypot(self.y).min(1.0)
    }

    /// Screen-up movement. Zero inside the 0.12 dead zone; otherwise the raw stick.
    pub fn move_vector(self) -> Vec2 {
        if self.deflection() > DEAD_ZONE {
            Vec2::new(self.x, -self.y)
        } else {
            Vec2::ZERO
        }
    }

    pub fn running(self) -> bool {
        self.deflection() > RUN_AT
    }
}

pub fn sample_joy(center: Vec2, pointer: Vec2) -> JoySample {
    let mut dx = pointer.x - center.x;
    let mut dy = pointer.y - center.y;
    let length = dx.hypot(dy);
    if length > JOY_RADIUS {
        let scale = JOY_RADIUS / length;
        dx *= scale;
        dy *= scale;
    }
    JoySample {
        x: dx / JOY_RADIUS,
        y: dy / JOY_RADIUS,
        knob_dx: dx,
        knob_dy: dy,
    }
}

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Hands {
    pub joy_id: Option<u64>,
    pub fire_id: Option<u64>,
    pub crouch_id: Option<u64>,
    pub crouch_secs: f32,
    pub crouch_long: bool,
    pub crouch: bool,
    pub joy: JoySample,
}

impl Hands {
    pub fn claims(&self, id: u64) -> bool {
        self.joy_id == Some(id) || self.fire_id == Some(id) || self.crouch_id == Some(id)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandFrame {
    pub joy: JoySample,
    pub joy_held: bool,
    pub fire: bool,
    /// Tap on 蹲 released before the long-press.
    pub crouch_toggled: bool,
    /// Long-press just crossed 0.45s.
    pub retreat: bool,
}

pub fn step_hands(
    hands: &mut Hands,
    pointers: &[Pointer],
    layout: &HudLayout,
    dt: f32,
) -> HandFrame {
    let alive =
        |id: Option<u64>| id.is_some_and(|id| pointers.iter().any(|pointer| pointer.id == id));
    let mut released_toggle = false;
    if !alive(hands.joy_id) {
        hands.joy_id = None;
        hands.joy = JoySample::default();
    }
    if !alive(hands.fire_id) {
        hands.fire_id = None;
    }
    if !alive(hands.crouch_id) {
        if hands.crouch_id.is_some() && !hands.crouch_long {
            hands.crouch = !hands.crouch;
            released_toggle = true;
        }
        hands.crouch_id = None;
        hands.crouch_secs = 0.0;
        hands.crouch_long = false;
    }
    let mut frame = claim_and_sample(hands, pointers, layout, dt);
    frame.crouch_toggled = released_toggle;
    frame
}

fn claim_and_sample(
    hands: &mut Hands,
    pointers: &[Pointer],
    layout: &HudLayout,
    dt: f32,
) -> HandFrame {
    for pointer in pointers.iter().filter(|pointer| pointer.just_pressed) {
        if hands.claims(pointer.id) {
            continue;
        }
        if hands.fire_id.is_none()
            && layout
                .act
                .is_some_and(|rect| rect.contains(pointer.position))
        {
            hands.fire_id = Some(pointer.id);
            continue;
        }
        if hands.crouch_id.is_none()
            && layout
                .sec
                .is_some_and(|rect| rect.contains(pointer.position))
        {
            hands.crouch_id = Some(pointer.id);
            hands.crouch_secs = 0.0;
            hands.crouch_long = false;
            continue;
        }
        if hands.joy_id.is_none() && layout.joy.contains(pointer.position) {
            hands.joy_id = Some(pointer.id);
        }
    }

    if let Some(id) = hands.joy_id {
        if let Some(pointer) = pointers.iter().find(|pointer| pointer.id == id) {
            hands.joy = sample_joy(layout.joy.center(), pointer.position);
        }
    } else {
        hands.joy = JoySample::default();
    }

    let mut retreat = false;
    if hands.crouch_id.is_some() && !hands.crouch_long {
        hands.crouch_secs += dt;
        if hands.crouch_secs >= CROUCH_HOLD {
            hands.crouch_long = true;
            retreat = true;
        }
    }

    HandFrame {
        joy: hands.joy,
        joy_held: hands.joy_id.is_some(),
        fire: hands.fire_id.is_some(),
        crouch_toggled: false,
        retreat,
    }
}

pub fn keyboard_vector(up: bool, down: bool, left: bool, right: bool) -> Vec2 {
    let mut vector = Vec2::ZERO;
    if up {
        vector.y += 1.0;
    }
    if down {
        vector.y -= 1.0;
    }
    if right {
        vector.x += 1.0;
    }
    if left {
        vector.x -= 1.0;
    }
    vector.normalize_or_zero()
}

/// Keyboard is normalized first, then the stick is added, matching `main.ts`.
pub fn combine_move(keyboard: Vec2, stick: Vec2) -> Vec2 {
    let vector = keyboard + stick;
    if vector.length() > 1.0 {
        vector.normalize()
    } else {
        vector
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::layout::{DEAD_ZONE, HudLayout, JOY_RADIUS};

    fn layout() -> HudLayout {
        HudLayout::from_viewport(Vec2::new(390.0, 844.0))
    }

    #[test]
    fn radius_clamp_dead_zone_and_y_flip() {
        let center = Vec2::ZERO;
        let inside = sample_joy(center, Vec2::new(JOY_RADIUS * 0.12, 0.0));
        assert!(inside.deflection() <= DEAD_ZONE + 1.0e-4);
        assert_eq!(inside.move_vector(), Vec2::ZERO);

        let right = sample_joy(center, Vec2::new(JOY_RADIUS, 0.0));
        assert!((right.move_vector() - Vec2::X).length() < 1.0e-4);
        assert!(right.running());
        let half = sample_joy(center, Vec2::new(JOY_RADIUS * 0.5, 0.0));
        assert!(!half.running());

        let down = sample_joy(center, Vec2::new(0.0, JOY_RADIUS));
        assert!((down.y - 1.0).abs() < 1.0e-4);
        assert!((down.move_vector() - Vec2::new(0.0, -1.0)).length() < 1.0e-4);

        let past = sample_joy(center, Vec2::new(JOY_RADIUS * 4.0, 0.0));
        assert!((past.deflection() - 1.0).abs() < 1.0e-4);
        assert!(past.running());
    }

    #[test]
    fn stick_only_starts_inside_the_pad_and_keeps_that_finger() {
        let layout = layout();
        let mut hands = Hands::default();
        let outside = layout.joy.center() + Vec2::new(layout.joy.width(), 0.0);
        let frame = step_hands(
            &mut hands,
            &[Pointer {
                id: 1,
                position: outside,
                just_pressed: true,
            }],
            &layout,
            0.016,
        );
        assert!(!frame.joy_held);

        step_hands(
            &mut hands,
            &[Pointer {
                id: 2,
                position: layout.joy.center(),
                just_pressed: true,
            }],
            &layout,
            0.016,
        );
        assert_eq!(hands.joy_id, Some(2));

        let frame = step_hands(
            &mut hands,
            &[
                Pointer {
                    id: 2,
                    position: layout.joy.center() + Vec2::new(400.0, 0.0),
                    just_pressed: false,
                },
                Pointer {
                    id: 3,
                    position: layout.joy.center(),
                    just_pressed: true,
                },
            ],
            &layout,
            0.016,
        );
        assert_eq!(hands.joy_id, Some(2));
        assert!(frame.joy.move_vector().x > 0.9);
        assert!(!hands.claims(3));
    }

    #[test]
    fn portrait_buttons_are_independent_of_the_stick() {
        let layout = layout();
        let act = layout.act.unwrap().center();
        let sec = layout.sec.unwrap().center();
        let mut hands = Hands::default();
        let frame = step_hands(
            &mut hands,
            &[
                Pointer {
                    id: 1,
                    position: layout.joy.center() + Vec2::new(JOY_RADIUS, 0.0),
                    just_pressed: true,
                },
                Pointer {
                    id: 4,
                    position: act,
                    just_pressed: true,
                },
            ],
            &layout,
            0.016,
        );
        assert!(frame.joy_held);
        assert!(frame.fire);
        assert_ne!(hands.joy_id, hands.fire_id);

        step_hands(&mut hands, &[], &layout, 0.016);
        step_hands(
            &mut hands,
            &[Pointer {
                id: 5,
                position: sec,
                just_pressed: true,
            }],
            &layout,
            0.016,
        );
        let frame = step_hands(
            &mut hands,
            &[Pointer {
                id: 5,
                position: sec,
                just_pressed: false,
            }],
            &layout,
            0.2,
        );
        assert!(!frame.retreat);
        let frame = step_hands(&mut hands, &[], &layout, 0.016);
        assert!(frame.crouch_toggled);
        assert!(hands.crouch);

        step_hands(
            &mut hands,
            &[Pointer {
                id: 6,
                position: sec,
                just_pressed: true,
            }],
            &layout,
            0.016,
        );
        let frame = step_hands(
            &mut hands,
            &[Pointer {
                id: 6,
                position: sec,
                just_pressed: false,
            }],
            &layout,
            0.5,
        );
        assert!(frame.retreat);
        let before = hands.crouch;
        let frame = step_hands(&mut hands, &[], &layout, 0.016);
        assert!(!frame.crouch_toggled);
        assert_eq!(hands.crouch, before);
    }

    #[test]
    fn landscape_has_no_thumb_buttons() {
        let layout = HudLayout::from_viewport(Vec2::new(1280.0, 720.0));
        let mut hands = Hands::default();
        let frame = step_hands(
            &mut hands,
            &[Pointer {
                id: 1,
                position: Vec2::new(1200.0, 640.0),
                just_pressed: true,
            }],
            &layout,
            0.016,
        );
        assert!(!frame.fire);
        assert!(!frame.joy_held);
    }
}
