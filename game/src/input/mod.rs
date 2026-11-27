//! Desktop keys and the original touch pad. Both write [`PlayerInput`].
//!
//! `player` still fills `MoveInput.keys` and is not edited here. The touch pad
//! is copied into `MoveInput.stick` so the existing movement keeps working.
//! [`PlayerInput::move`] is that same sum.

mod gesture;
mod layout;
mod preferences;
mod touchctl;

use bevy::{
    input::touch::{TouchInput, TouchPhase},
    prelude::*,
    window::PrimaryWindow,
};

use crate::game_flow::GameFlow;
use crate::player::MoveInput;

pub use gesture::{Hands as TouchControl, JoySample};
pub use layout::{HudLayout, Insets, Orientation, ScreenRect, orientation, safe_insets};
pub use preferences::{
    CONTROL_ACTIONS, ControlAction, ControlPreferences, bindable_key, key_label,
};
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
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerInput {
    pub r#move: Vec2,
    pub aim: Vec2,
    pub fire: bool,
    /// Held brake intent; only read while occupying a vehicle.
    pub vehicle_brake: bool,
    /// Portrait 蹲 toggle.
    pub crouch: bool,
    /// Seconds of retreat remaining (original `MB.retreat`).
    pub retreat: f32,
    /// Stick length in 0..=1 after the 0.12 dead zone. Run when this is above 0.93.
    pub stick: f32,
}

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
            .init_resource::<ControlPreferences>()
            .init_resource::<TouchControl>()
            .add_message::<TouchInput>()
            .configure_sets(Update, InputSet.before(crate::player::move_system_set()))
            .add_systems(Startup, spawn_controls)
            .add_systems(Update, gather_player_input.in_set(InputSet))
            // Pause is driven after input intent and before the flow stage.
            // Widget visibility must see that same-frame result too.
            .add_systems(Update, sync_widgets.after(crate::game_flow::FlowSet));
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
    for touch in touches
        .iter_just_pressed()
        .filter(|touch| touches.get_pressed(touch.id()).is_some())
    {
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
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    controls: Res<ControlPreferences>,
    fleet: Option<Res<crate::vehicles::VehicleFleet>>,
    mut touch_events: MessageReader<TouchInput>,
    mut previous_layout: Local<Option<(Vec2, u8, Option<u32>)>>,
    modal: Option<Res<crate::ui::UiModal>>,
    pause: Option<Res<crate::ui::PauseOpen>>,
    mut hands: ResMut<TouchControl>,
    mut move_input: ResMut<MoveInput>,
    mut player: ResMut<PlayerInput>,
) {
    let events: Vec<_> = touch_events.read().copied().collect();
    let Some((window_id, window)) = windows.iter().next() else {
        clear_input(&mut player, &mut hands, &mut move_input);
        return;
    };
    if !window.focused
        || flow.get() != &GameFlow::Playing
        || modal.is_some_and(|m| m.blocks_input())
        || pause.is_some_and(|p| p.0)
    {
        clear_input(&mut player, &mut hands, &mut move_input);
        return;
    }

    let viewport = Vec2::new(window.width(), window.height());
    let occupant = fleet.as_ref().and_then(|fleet| fleet.current);
    let driving = occupant.is_some();
    let armored = fleet
        .as_ref()
        .and_then(|fleet| fleet.current())
        .is_some_and(|v| v.spec().armored);
    let layout_key = (viewport, controls.button_percent, occupant);
    if previous_layout.is_some_and(|old| old != layout_key) {
        let crouch = hands.crouch;
        *hands = TouchControl {
            crouch,
            ..default()
        };
    }
    let layout = HudLayout::for_controls(viewport, controls.button_percent, driving);
    if *previous_layout != Some(layout_key) {
        let rect = |r: ScreenRect| [r.min.x, r.min.y, r.max.x, r.max.y];
        println!(
            "DTPAD layout={}",
            serde_json::json!({"viewport":[viewport.x,viewport.y],"driving":driving,"armored":armored,"joy":rect(layout.joy),"act":layout.act.map(rect),"sec":layout.sec.map(rect),"button_percent":controls.button_percent})
        );
    }
    *previous_layout = Some(layout_key);
    let pointers = sample_pointers(&touches, &mouse, window.cursor_position());
    // Touches contains only the final state. Ordered events preserve a complete
    // tap between slow frames and distinguish cancellation from a released tap.
    let mut started_fire = None;
    let mut tap_fire = false;
    for event in events.iter().filter(|event| event.window == window_id) {
        match event.phase {
            TouchPhase::Started => {
                if hands.fire_id.is_none()
                    && layout.act.is_some_and(|rect| rect.contains(event.position))
                {
                    hands.fire_id = Some(event.id);
                    started_fire = Some(event.id);
                    controls.haptic(15);
                } else if hands.crouch_id.is_none()
                    && layout.sec.is_some_and(|rect| rect.contains(event.position))
                {
                    hands.crouch_id = Some(event.id);
                    hands.crouch_secs = 0.0;
                    hands.crouch_long = false;
                    controls.haptic(15);
                }
            }
            TouchPhase::Ended | TouchPhase::Canceled => {
                if hands.fire_id == Some(event.id) {
                    tap_fire |= event.phase == TouchPhase::Ended && started_fire == Some(event.id);
                    hands.fire_id = None;
                }
                if hands.crouch_id == Some(event.id) {
                    if event.phase == TouchPhase::Ended && !driving && !hands.crouch_long {
                        hands.crouch = !hands.crouch;
                    }
                    hands.crouch_id = None;
                    hands.crouch_secs = 0.0;
                    hands.crouch_long = false;
                }
                if event.phase == TouchPhase::Canceled && hands.joy_id == Some(event.id) {
                    hands.joy_id = None;
                    hands.joy = JoySample::default();
                }
            }
            TouchPhase::Moved => {}
        }
    }
    if !driving && controls.just_pressed(&keys, ControlAction::Sneak) {
        hands.crouch = !hands.crouch;
    }
    if driving {
        hands.crouch = false;
        // A held driving brake never becomes crouch or retreat on release.
        hands.crouch_long = true;
    }
    let frame: HandFrame = step_hands(&mut hands, &pointers, &layout, time.delta_secs());
    if !driving && frame.retreat {
        controls.haptic(30);
    }
    if mouse.just_pressed(MouseButton::Left)
        && window.cursor_position().is_some_and(|point| {
            layout.act.is_some_and(|rect| rect.contains(point))
                || layout.sec.is_some_and(|rect| rect.contains(point))
        })
    {
        controls.haptic(15);
    }
    let stick_vec = frame.joy.move_vector();
    let key = keyboard_vector(
        controls.pressed(&keys, ControlAction::Up),
        controls.pressed(&keys, ControlAction::Down),
        controls.pressed(&keys, ControlAction::Left),
        controls.pressed(&keys, ControlAction::Right),
    );
    let mouse_on_pad = hands.claims(MOUSE);
    let retreat = if driving {
        0.0
    } else if frame.retreat {
        RETREAT_SECS
    } else {
        (player.retreat - time.delta_secs()).max(0.0)
    };
    *player = PlayerInput {
        r#move: combine_move(key, stick_vec),
        aim: Vec2::ZERO,
        fire: (!driving || armored)
            && (tap_fire || frame.fire || (mouse.pressed(MouseButton::Left) && !mouse_on_pad)),
        vehicle_brake: driving
            && (controls.pressed(&keys, ControlAction::VehicleBrake)
                || hands.crouch_id.is_some()
                || (!armored && (frame.fire || tap_fire))),
        crouch: hands.crouch,
        retreat,
        stick: if stick_vec == Vec2::ZERO {
            0.0
        } else {
            frame.joy.deflection()
        },
    };
    move_input.stick = stick_vec;
}

fn sync_widgets(
    flow: Res<State<GameFlow>>,
    modal: Option<Res<crate::ui::UiModal>>,
    pause: Option<Res<crate::ui::PauseOpen>>,
    hands: Res<TouchControl>,
    controls: Res<ControlPreferences>,
    fleet: Option<Res<crate::vehicles::VehicleFleet>>,
    language: Option<Res<crate::i18n::Language>>,
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
    mut act_label: Query<(&mut Text, &mut TextFont), (With<ActLabel>, Without<SecLabel>)>,
    mut sec_label: Query<(&mut Text, &mut TextFont), (With<SecLabel>, Without<ActLabel>)>,
) {
    let viewport = windows
        .iter()
        .next()
        .map(|window| Vec2::new(window.width(), window.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    let driving = fleet.as_ref().is_some_and(|fleet| fleet.current.is_some());
    let armored = fleet
        .as_ref()
        .and_then(|fleet| fleet.current())
        .is_some_and(|v| v.spec().armored);
    let lang = language.as_deref().copied().unwrap_or_default();
    let layout = HudLayout::for_controls(viewport, controls.button_percent, driving);
    let playing = flow.get() == &GameFlow::Playing
        && !modal.is_some_and(|m| m.blocks_input())
        && !pause.is_some_and(|p| p.0);
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
    let fire_label = if driving && !armored {
        crate::i18n::tr(lang, "煞車", "Brake")
    } else if armored {
        crate::i18n::tr(lang, "射", "Fire")
    } else {
        crate::i18n::tr(lang, "打", "Hit")
    };
    for (mut fire, mut font) in &mut act_label {
        if fire.0 != fire_label {
            fire.0 = fire_label.to_string();
        }
        font.font_size = FontSize::Px(if lang == crate::i18n::Language::En {
            22.0
        } else {
            32.0
        });
    }
    let crouch_label = if driving {
        crate::i18n::tr(lang, "煞", "Brake")
    } else if hands.crouch {
        crate::i18n::tr(lang, "起", "Up")
    } else {
        crate::i18n::tr(lang, "蹲", "Crouch")
    };
    for (mut crouch, mut font) in &mut sec_label {
        font.font_size = FontSize::Px(if lang == crate::i18n::Language::En {
            13.0
        } else {
            22.0
        });
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
mod integration_tests {
    use super::*;
    use bevy::{input::touch::touch_screen_input_system, time::TimeUpdateStrategy};
    use std::time::Duration;

    fn app(viewport: Vec2, vehicle: Option<crate::vehicles::VehicleKind>) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                16,
            )))
            .insert_resource(State::new(GameFlow::Playing))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Touches>()
            .init_resource::<ControlPreferences>()
            .init_resource::<TouchControl>()
            .init_resource::<MoveInput>()
            .init_resource::<PlayerInput>()
            .init_resource::<crate::ui::PauseOpen>()
            .init_resource::<crate::ui::UiModal>()
            .add_message::<TouchInput>()
            .add_systems(PreUpdate, touch_screen_input_system)
            .add_systems(Update, gather_player_input);
        if let Some(kind) = vehicle {
            app.insert_resource(crate::vehicles::VehicleFleet {
                vehicles: vec![crate::vehicles::VehicleState::new(
                    0,
                    kind,
                    Vec2::ZERO,
                    0.0,
                    &mut || 0.5,
                )],
                current: Some(0),
            });
        }
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: (viewport.x as u32, viewport.y as u32).into(),
                    focused: true,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        (app, window)
    }

    fn touch(app: &mut App, window: Entity, phase: TouchPhase, position: Vec2) {
        app.world_mut().write_message(TouchInput {
            window,
            phase,
            id: 0,
            position,
            force: None,
        });
    }

    #[test]
    fn ordered_short_taps_fire_once_and_cancellation_does_not_toggle_crouch() {
        let viewport = Vec2::new(390.0, 844.0);
        let (mut app, window) = app(viewport, None);
        let layout = HudLayout::for_controls(viewport, 100, false);
        let act = layout.act.unwrap().center();
        touch(&mut app, window, TouchPhase::Started, act);
        touch(&mut app, window, TouchPhase::Ended, act);
        app.update();
        assert!(app.world().resource::<PlayerInput>().fire);
        app.update();
        assert!(!app.world().resource::<PlayerInput>().fire);
        let sec = layout.sec.unwrap().center();
        touch(&mut app, window, TouchPhase::Started, sec);
        touch(&mut app, window, TouchPhase::Canceled, sec);
        app.update();
        assert!(!app.world().resource::<PlayerInput>().crouch);
        touch(&mut app, window, TouchPhase::Started, sec);
        touch(&mut app, window, TouchPhase::Ended, sec);
        app.update();
        assert!(app.world().resource::<PlayerInput>().crouch);
        app.update();
        assert!(app.world().resource::<PlayerInput>().crouch);
    }

    #[test]
    fn driving_touch_brake_never_crouches_or_fires_personal_weapon_and_pause_clears_it() {
        for viewport in [Vec2::new(390.0, 844.0), Vec2::new(844.0, 390.0)] {
            let (mut app, window) = app(viewport, Some(crate::vehicles::VehicleKind::Kei));
            let layout = HudLayout::for_controls(viewport, 100, true);
            let act = layout.act.unwrap().center();
            touch(&mut app, window, TouchPhase::Started, act);
            app.update();
            let input = app.world().resource::<PlayerInput>();
            assert!(input.vehicle_brake);
            assert!(!input.fire && !input.crouch && input.retreat == 0.0);
            app.world_mut().resource_mut::<crate::ui::PauseOpen>().0 = true;
            app.update();
            assert_eq!(
                *app.world().resource::<PlayerInput>(),
                PlayerInput::default()
            );
            touch(&mut app, window, TouchPhase::Ended, act);
            app.update();
            app.world_mut().resource_mut::<crate::ui::PauseOpen>().0 = false;
            app.update();
            assert!(!app.world().resource::<PlayerInput>().vehicle_brake);
            let sec = layout.sec.unwrap().center();
            touch(&mut app, window, TouchPhase::Started, sec);
            app.update();
            assert!(app.world().resource::<PlayerInput>().vehicle_brake);
            touch(&mut app, window, TouchPhase::Ended, sec);
            app.update();
            assert!(!app.world().resource::<PlayerInput>().crouch);
            assert_eq!(app.world().resource::<PlayerInput>().retreat, 0.0);
        }
    }

    #[test]
    fn mounted_fire_and_rebound_movement_use_the_live_control_preferences() {
        let viewport = Vec2::new(844.0, 390.0);
        let (mut app, window) = app(viewport, Some(crate::vehicles::VehicleKind::Zbl08));
        let act = HudLayout::for_controls(viewport, 100, true)
            .act
            .unwrap()
            .center();
        touch(&mut app, window, TouchPhase::Started, act);
        touch(&mut app, window, TouchPhase::Ended, act);
        app.update();
        assert!(app.world().resource::<PlayerInput>().fire);
        assert!(!app.world().resource::<PlayerInput>().vehicle_brake);
        app.world_mut()
            .resource_mut::<ControlPreferences>()
            .rebind(ControlAction::Up, KeyCode::KeyE)
            .unwrap();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.update();
        assert_eq!(app.world().resource::<PlayerInput>().r#move, Vec2::ZERO);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyE);
        app.update();
        assert_eq!(app.world().resource::<PlayerInput>().r#move, Vec2::Y);
    }
}
