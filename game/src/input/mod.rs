//! Desktop keys and the original touch pad. Both write [`PlayerInput`].
//!
//! `player` still fills `MoveInput.keys` and is not edited here. The touch pad
//! is copied into `MoveInput.stick` so the existing movement keeps working.
//! [`PlayerInput::move`] is that same sum.

mod gesture;
mod layout;
mod touchctl;

use bevy::{prelude::*, window::PrimaryWindow};

use crate::game_flow::GameFlow;
use crate::player::MoveInput;

pub use gesture::{Hands as TouchControl, JoySample};
pub use layout::{HudLayout, Insets, Orientation, ScreenRect, orientation, safe_insets};
pub use touchctl::{ctx_hint, ctx_label};

use gesture::{HandFrame, Pointer, combine_move, keyboard_vector, step_hands};
use layout::{ACT_SIZE, JOY_KNOB, JOY_SIZE, RETREAT_SECS};

const MOUSE: u64 = u64::MAX;

/// Screen-space command for one frame.
///
/// `move` and `aim` use +x right and +y up. `move` is spelled `r#move` because
/// `move` is a Rust keyword.
///
/// `aim` stays zero in this slice: the original aims by dragging a scoped
/// weapon, which the skeleton does not have. `stick` is the pad deflection
/// after the dead zone; the original starts running above 0.93.
///
/// `fire` is the rising edge of a click or the portrait 打 button, so one
/// press is one shot. `reload`, `interact`, `slot`, and `pause` are the same
/// kind of edge for R, E, the number keys, and Esc.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerInput {
    pub r#move: Vec2,
    pub aim: Vec2,
    pub fire: bool,
    /// Portrait 蹲 toggle.
    pub crouch: bool,
    /// Seconds of retreat remaining (original `MB.retreat`).
    pub retreat: f32,
    /// Stick length in 0..=1 after the 0.12 dead zone. Run when this is above 0.93.
    pub stick: f32,
    /// R, this frame only.
    pub reload: bool,
    /// E, this frame only.
    pub interact: bool,
    /// Digit or numpad key pressed this frame. `Some(0)` is the 0 key.
    pub slot: Option<u8>,
    /// Esc, this frame only. The pause screen reads this; it is not a flow state.
    pub pause: bool,
}

/// Esc opens this. While it is set, gameplay fields on [`PlayerInput`] stay clear.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GamePause(pub bool);

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct InputSet;

#[derive(Component)]
struct JoyBase;

#[derive(Component)]
struct JoyKnob;

#[derive(Component)]
struct ActButton;

#[derive(Component)]
struct SecButton;

#[derive(Component)]
struct ActLabel;

#[derive(Component)]
struct SecLabel;

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerInput>()
            .init_resource::<GamePause>()
            .init_resource::<TouchControl>()
            .configure_sets(Update, InputSet.before(crate::player::move_system_set()))
            .add_systems(Startup, spawn_controls)
            .add_systems(
                Update,
                (gather_player_input, sync_widgets).chain().in_set(InputSet),
            );
    }
}

fn spawn_controls(mut commands: Commands) {
    commands
        .spawn((
            JoyBase,
            Node {
                position_type: PositionType::Absolute,
                width: px(JOY_SIZE),
                height: px(JOY_SIZE),
                border: UiRect::all(px(2.0)),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(20.0 / 255.0, 22.0 / 255.0, 24.0 / 255.0, 0.35)),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.35)),
            Visibility::Hidden,
            GlobalZIndex(12),
        ))
        .with_child((
            JoyKnob,
            Node {
                width: px(JOY_KNOB),
                height: px(JOY_KNOB),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(Color::srgba(
                242.0 / 255.0,
                210.0 / 255.0,
                122.0 / 255.0,
                0.75,
            )),
        ));

    commands
        .spawn((
            ActButton,
            round_pad(ACT_SIZE),
            BackgroundColor(Color::srgba(
                150.0 / 255.0,
                30.0 / 255.0,
                24.0 / 255.0,
                0.82,
            )),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.55)),
            Visibility::Hidden,
            GlobalZIndex(16),
        ))
        .with_child((
            ActLabel,
            Text::new("打"),
            TextFont::from_font_size(FontSize::Px(32.0)),
            TextColor(Color::WHITE),
        ));

    commands
        .spawn((
            SecButton,
            round_pad(60.0),
            BackgroundColor(Color::srgba(20.0 / 255.0, 26.0 / 255.0, 32.0 / 255.0, 0.72)),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.55)),
            Visibility::Hidden,
            GlobalZIndex(16),
        ))
        .with_child((
            SecLabel,
            Text::new("蹲"),
            TextFont::from_font_size(FontSize::Px(22.0)),
            TextColor(Color::WHITE),
        ));
}

fn round_pad(size: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: px(size),
        height: px(size),
        border: UiRect::all(px(2.0)),
        border_radius: BorderRadius::MAX,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

fn clear_input(player: &mut PlayerInput, hands: &mut TouchControl, move_input: &mut MoveInput) {
    *player = PlayerInput::default();
    *hands = TouchControl::default();
    move_input.stick = Vec2::ZERO;
}

fn sample_pointers(
    touches: &Touches,
    mouse: &ButtonInput<MouseButton>,
    cursor: Option<Vec2>,
) -> Vec<Pointer> {
    let mut pointers: Vec<Pointer> = touches
        .iter()
        .map(|touch| Pointer {
            id: touch.id(),
            position: touch.position(),
            just_pressed: false,
        })
        .collect();
    for touch in touches.iter_just_pressed() {
        if let Some(existing) = pointers.iter_mut().find(|pointer| pointer.id == touch.id()) {
            existing.just_pressed = true;
        } else {
            pointers.push(Pointer {
                id: touch.id(),
                position: touch.position(),
                just_pressed: true,
            });
        }
    }
    if mouse.pressed(MouseButton::Left)
        && let Some(cursor) = cursor
    {
        pointers.push(Pointer {
            id: MOUSE,
            position: cursor,
            just_pressed: mouse.just_pressed(MouseButton::Left),
        });
    }
    pointers
}

fn gather_player_input(
    time: Res<Time>,
    flow: Res<State<GameFlow>>,
    pause: Res<GamePause>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut hands: ResMut<TouchControl>,
    mut move_input: ResMut<MoveInput>,
    mut player: ResMut<PlayerInput>,
) {
    let edges = read_key_edges(&keys);
    let Some(window) = windows.iter().next() else {
        clear_input(&mut player, &mut hands, &mut move_input);
        return;
    };
    if flow.get() != &GameFlow::Playing || pause.0 {
        clear_input(&mut player, &mut hands, &mut move_input);
        if flow.get() == &GameFlow::Playing {
            player.pause = edges.pause;
        }
        return;
    }

    let viewport = Vec2::new(window.width(), window.height());
    let layout = HudLayout::from_viewport(viewport);
    let pointers = sample_pointers(&touches, &mouse, window.cursor_position());
    let frame: HandFrame = step_hands(&mut hands, &pointers, &layout, time.delta_secs());
    let stick_vec = frame.joy.move_vector();
    let key = keyboard_vector(
        keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]),
        keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]),
        keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]),
        keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]),
    );
    let touch_fire = pointers
        .iter()
        .any(|pointer| pointer.just_pressed && hands.fire_id == Some(pointer.id));
    let retreat = if frame.retreat {
        RETREAT_SECS
    } else {
        (player.retreat - time.delta_secs()).max(0.0)
    };
    *player = PlayerInput {
        r#move: combine_move(key, stick_vec),
        aim: Vec2::ZERO,
        fire: touch_fire || mouse_fire_edge(&mouse, hands.claims(MOUSE)),
        crouch: hands.crouch,
        retreat,
        stick: if stick_vec == Vec2::ZERO {
            0.0
        } else {
            frame.joy.deflection()
        },
        reload: edges.reload,
        interact: edges.interact,
        slot: edges.slot,
        pause: edges.pause,
    };
    move_input.stick = stick_vec;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyEdges {
    pub reload: bool,
    pub interact: bool,
    pub pause: bool,
    pub slot: Option<u8>,
}

pub(crate) fn read_key_edges(keys: &ButtonInput<KeyCode>) -> KeyEdges {
    const SLOTS: [(KeyCode, u8); 20] = [
        (KeyCode::Digit1, 1),
        (KeyCode::Digit2, 2),
        (KeyCode::Digit3, 3),
        (KeyCode::Digit4, 4),
        (KeyCode::Digit5, 5),
        (KeyCode::Digit6, 6),
        (KeyCode::Digit7, 7),
        (KeyCode::Digit8, 8),
        (KeyCode::Digit9, 9),
        (KeyCode::Digit0, 0),
        (KeyCode::Numpad1, 1),
        (KeyCode::Numpad2, 2),
        (KeyCode::Numpad3, 3),
        (KeyCode::Numpad4, 4),
        (KeyCode::Numpad5, 5),
        (KeyCode::Numpad6, 6),
        (KeyCode::Numpad7, 7),
        (KeyCode::Numpad8, 8),
        (KeyCode::Numpad9, 9),
        (KeyCode::Numpad0, 0),
    ];
    KeyEdges {
        reload: keys.just_pressed(KeyCode::KeyR),
        interact: keys.just_pressed(KeyCode::KeyE),
        pause: keys.just_pressed(KeyCode::Escape),
        slot: SLOTS
            .into_iter()
            .find(|(key, _)| keys.just_pressed(*key))
            .map(|(_, slot)| slot),
    }
}

/// Left click that is not already claimed by the pad or a portrait button.
pub(crate) fn mouse_fire_edge(mouse: &ButtonInput<MouseButton>, claimed: bool) -> bool {
    mouse.just_pressed(MouseButton::Left) && !claimed
}

fn sync_widgets(
    flow: Res<State<GameFlow>>,
    hands: Res<TouchControl>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut joy: Query<(&mut Node, &mut Visibility), With<JoyBase>>,
    mut knob: Query<&mut UiTransform, (With<JoyKnob>, Without<JoyBase>)>,
    mut act: Query<
        (&mut Node, &mut Visibility, &mut BackgroundColor),
        (With<ActButton>, Without<JoyBase>, Without<SecButton>),
    >,
    mut sec: Query<
        (&mut Node, &mut Visibility, &mut BackgroundColor),
        (With<SecButton>, Without<JoyBase>, Without<ActButton>),
    >,
    mut act_label: Query<&mut Text, (With<ActLabel>, Without<SecLabel>)>,
    mut sec_label: Query<&mut Text, (With<SecLabel>, Without<ActLabel>)>,
) {
    let viewport = windows
        .iter()
        .next()
        .map(|window| Vec2::new(window.width(), window.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    let layout = HudLayout::from_viewport(viewport);
    let playing = flow.get() == &GameFlow::Playing;
    let shown = if playing {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for (mut node, mut visibility) in &mut joy {
        place(&mut node, layout.joy);
        *visibility = shown;
    }
    for mut transform in &mut knob {
        transform.translation = if hands.joy_id.is_some() {
            Val2::px(hands.joy.knob_dx, hands.joy.knob_dy)
        } else {
            Val2::px(0.0, 0.0)
        };
    }
    let pressed = Color::srgba(1.0, 200.0 / 255.0, 80.0 / 255.0, 0.85);
    for (mut node, mut visibility, mut color) in &mut act {
        if let Some(rect) = layout.act {
            place(&mut node, rect);
            *visibility = shown;
        } else {
            *visibility = Visibility::Hidden;
        }
        color.0 = if hands.fire_id.is_some() {
            pressed
        } else {
            Color::srgba(150.0 / 255.0, 30.0 / 255.0, 24.0 / 255.0, 0.82)
        };
    }
    for (mut node, mut visibility, mut color) in &mut sec {
        if let Some(rect) = layout.sec {
            place(&mut node, rect);
            *visibility = shown;
        } else {
            *visibility = Visibility::Hidden;
        }
        color.0 = if hands.crouch_id.is_some() {
            pressed
        } else if hands.crouch {
            Color::srgba(31.0 / 255.0, 90.0 / 255.0, 58.0 / 255.0, 0.9)
        } else {
            Color::srgba(20.0 / 255.0, 26.0 / 255.0, 32.0 / 255.0, 0.72)
        };
    }
    for mut fire in &mut act_label {
        if fire.0 != "打" {
            fire.0 = "打".to_string();
        }
    }
    let crouch_label = if hands.crouch { "起" } else { "蹲" };
    for mut crouch in &mut sec_label {
        if crouch.0 != crouch_label {
            crouch.0 = crouch_label.to_string();
        }
    }
}

fn place(node: &mut Node, rect: ScreenRect) {
    node.left = px(rect.min.x);
    node.top = px(rect.min.y);
    node.width = px(rect.width());
    node.height = px(rect.height());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_edges_cover_wasd_arrows_reload_interact_numbers_and_esc() {
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyR);
        keys.press(KeyCode::KeyE);
        keys.press(KeyCode::Escape);
        keys.press(KeyCode::Digit4);
        keys.press(KeyCode::ArrowUp);
        keys.press(KeyCode::KeyA);
        let edges = read_key_edges(&keys);
        assert!(edges.reload && edges.interact && edges.pause);
        assert_eq!(edges.slot, Some(4));
        let movement = keyboard_vector(true, false, true, false);
        assert!(movement.y > 0.0 && movement.x < 0.0);

        keys.clear();
        keys.press(KeyCode::Numpad0);
        assert_eq!(read_key_edges(&keys).slot, Some(0));
        assert!(!read_key_edges(&keys).reload);
    }

    #[test]
    fn mouse_fire_is_the_press_edge() {
        let mut mouse = ButtonInput::default();
        mouse.press(MouseButton::Left);
        assert!(mouse_fire_edge(&mouse, false));
        assert!(!mouse_fire_edge(&mouse, true));
        mouse.clear();
        assert!(mouse.pressed(MouseButton::Left));
        assert!(!mouse_fire_edge(&mouse, false));
    }
}
