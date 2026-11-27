//! Actual UI → map → gameplay-input schedules, without a renderer or a window server.

use super::*;
use bevy::{
    input::touch::{TouchInput, TouchPhase},
    state::app::StatesPlugin,
    time::TimeUpdateStrategy,
};
use std::time::Duration;

use crate::audio::{Ambience, SfxEvent};
use crate::combat::{CombatEnv, EnemyDied, GameState, PlayerDied, ResetCombat};
use crate::game_flow::GameFlowPlugin;
use crate::i18n::{Language, UiFont};
use crate::input::{InputPlugin, PlayerInput};
use crate::map::Obstacles;
use crate::player::{MoveInput, PlayerPlugin};
use crate::save::{ContinueGame, LoadCompleted, SaveAndMenu, SaveStatus};
use crate::survival::ShelterPlugin;
use crate::ui::FlowUiPlugin;
use crate::worldplay::WorldPlayPlugin;

fn input_app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )))
        .init_resource::<Language>()
        .init_resource::<UiFont>()
        .init_resource::<GameState>()
        .init_resource::<CombatEnv>()
        .init_resource::<Ambience>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<AccumulatedMouseScroll>()
        .init_resource::<SaveStatus>()
        .insert_resource(Obstacles::new(crate::map::world().clone()))
        .insert_resource(crate::map::world().plan_evac())
        .add_message::<ResetCombat>()
        .add_message::<PlayerDied>()
        .add_message::<EnemyDied>()
        .add_message::<SfxEvent>()
        .add_message::<SaveGame>()
        .add_message::<ContinueGame>()
        .add_message::<SaveAndMenu>()
        .add_message::<LoadCompleted>()
        .add_message::<bevy::window::WindowFocused>()
        .add_message::<bevy::window::AppLifecycle>()
        .add_message::<TouchInput>()
        .add_systems(
            PreUpdate,
            bevy::input::touch::touch_screen_input_system.in_set(bevy::input::InputSystems),
        )
        .add_plugins((
            GameFlowPlugin,
            PlayerPlugin,
            InputPlugin,
            FlowUiPlugin,
            ShelterPlugin,
            WorldPlayPlugin,
            WorldMapPlugin,
        ))
        .insert_state(GameFlow::Playing);
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: (390, 844).into(),
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    app.world_mut()
        .resource_mut::<crate::game_flow::Campaign>()
        .game_min = 440.0;
    app.update();
    (app, window)
}

fn player_position(app: &mut App) -> Vec3 {
    let world = app.world_mut();
    world
        .query_filtered::<&Transform, With<Player>>()
        .single(world)
        .unwrap()
        .translation
}

fn assert_gameplay_blocked(app: &mut App, original_position: Vec3) {
    assert!(app.world().resource::<MapOpen>().0);
    assert!(app.world().resource::<UiModal>().open);
    assert!(!app.world().resource::<PauseOpen>().0);
    assert!(!app.world().resource::<PlayerInput>().fire);
    assert_eq!(app.world().resource::<PlayerInput>().r#move, Vec2::ZERO);
    assert_eq!(app.world().resource::<MoveInput>().combined(), Vec2::ZERO);
    assert_eq!(player_position(app), original_position);
}

#[test]
fn pressing_m_blocks_a_held_move_and_fire_in_the_opening_frame() {
    let (mut app, window) = input_app();
    let before = player_position(&mut app);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(160.0, 380.0)));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.press(KeyCode::KeyW);
    keys.press(KeyCode::KeyM);
    app.update();
    assert_gameplay_blocked(&mut app, before);
}

#[test]
fn keyboard_open_then_close_keeps_preexisting_mouse_press_captured_until_release() {
    let (mut app, window) = input_app();
    let before = player_position(&mut app);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(160.0, 380.0)));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyM);
    app.update();
    assert_gameplay_blocked(&mut app, before);
    // Match the real input plugin: the button remains held, but its initial
    // just-pressed edge is not replayed on the next frame of this minimal app.
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear_just_pressed(MouseButton::Left);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<MapOpen>().0);
    assert!(app.world().resource::<UiModal>().pointer_captured);
    assert!(!app.world().resource::<PlayerInput>().fire);
    assert_eq!(player_position(&mut app), before);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert!(!app.world().resource::<UiModal>().pointer_captured);
}

#[test]
fn map_button_is_captured_and_escape_closes_only_the_map() {
    let (mut app, window) = input_app();
    let before = player_position(&mut app);
    let button = toggle_rect(Vec2::new(390.0, 844.0)).center();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(button));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.update();
    assert_gameplay_blocked(&mut app, before);
    assert!(app.world().resource::<UiModal>().pointer_captured);

    // Keep the opening pointer down. Closing the map must not release that
    // captured press into the attack control or also trigger world pause.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<MapOpen>().0);
    assert!(!app.world().resource::<PauseOpen>().0);
    assert!(!app.world().resource::<UiModal>().open);
    assert!(app.world().resource::<UiModal>().pointer_captured);
    assert!(!app.world().resource::<PlayerInput>().fire);
    assert_eq!(player_position(&mut app), before);
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Escape)
    );

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::KeyW);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert!(!app.world().resource::<UiModal>().pointer_captured);
    assert!(!app.world().resource::<PauseOpen>().0);
}

fn touch(app: &mut App, window: Entity, id: u64, phase: TouchPhase, position: Vec2) {
    app.world_mut().write_message(TouchInput {
        phase,
        position,
        window,
        force: None,
        id,
    });
}

fn tap_events(app: &mut App, window: Entity, p: Vec2) {
    touch(app, window, 0, TouchPhase::Started, p);
    touch(app, window, 0, TouchPhase::Ended, p);
}

#[test]
fn real_driving_brake_and_map_touches_are_distinct_at_large_button_size() {
    for viewport in [
        Vec2::new(390.0, 844.0),
        Vec2::new(844.0, 390.0),
        Vec2::new(1280.0, 720.0),
    ] {
        let (mut app, window) = input_app();
        let position = player_position(&mut app).xz();
        let car = crate::vehicles::VehicleState::new(
            9,
            crate::vehicles::VehicleKind::Kei,
            position,
            0.0,
            &mut || 0.5,
        );
        app.insert_resource(crate::vehicles::VehicleFleet {
            vehicles: vec![car],
            current: Some(9),
        });
        app.world_mut()
            .resource_mut::<crate::input::ControlPreferences>()
            .button_percent = 110;
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_physical_resolution(viewport.x as u32, viewport.y as u32);
        app.update();
        let pads = HudLayout::for_controls(viewport, 110, true);
        touch(
            &mut app,
            window,
            0,
            TouchPhase::Started,
            pads.act.unwrap().center(),
        );
        app.update();
        assert!(
            app.world().resource::<PlayerInput>().vehicle_brake,
            "{viewport:?}: actual Brake touch was intercepted"
        );
        assert!(!app.world().resource::<PlayerInput>().fire);
        assert!(!app.world().resource::<MapOpen>().0);
        touch(
            &mut app,
            window,
            0,
            TouchPhase::Ended,
            pads.act.unwrap().center(),
        );
        app.update();
        assert!(!app.world().resource::<PlayerInput>().vehicle_brake);
        tap_events(
            &mut app,
            window,
            toggle_rect_for_controls(viewport, 110, true).center(),
        );
        app.update();
        assert!(
            app.world().resource::<MapOpen>().0,
            "{viewport:?}: actual Map touch did not open it"
        );
        assert!(!app.world().resource::<PlayerInput>().vehicle_brake);
        assert!(!app.world().resource::<PlayerInput>().fire);
    }
}

fn open_with_supplies(app: &mut App, window: Entity) {
    let viewport = Vec2::new(390.0, 844.0);
    tap_events(app, window, toggle_rect(viewport).center());
    let supplies = map_layout(viewport)
        .buttons
        .iter()
        .find(|(_, button)| matches!(button, Button::Tool(MapTool::Mark(NoteKind::Supplies))))
        .unwrap()
        .0
        .center();
    tap_events(app, window, supplies);
}

#[test]
fn short_android_note_tap_between_frames_is_not_lost() {
    let (mut app, window) = input_app();
    open_with_supplies(&mut app, window);
    app.update();
    assert!(app.world().resource::<MapOpen>().0);
    // Android CI's failing sequence: the tool is already selected, and both
    // DOWN and UP of the note arrive before the next application update.
    tap_events(&mut app, window, Vec2::new(220.0, 350.0));
    app.update();
    assert_eq!(app.world().resource::<MapProgress>().notes.len(), 1);
}

#[test]
fn coalesced_android_taps_reusing_pointer_zero_select_tool_and_mark_once() {
    let (mut app, window) = input_app();
    let before = player_position(&mut app);
    // The failed Android run produced all DOWN/UP pairs between slow frames.
    // Include opening, toolbar selection and annotation in one update, reusing
    // the Android pointer ID. A final Touches snapshot cannot represent this.
    open_with_supplies(&mut app, window);
    tap_events(&mut app, window, Vec2::new(220.0, 350.0));
    app.update();
    assert_gameplay_blocked(&mut app, before);
    let notes = &app.world().resource::<MapProgress>().notes;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].kind, NoteKind::Supplies);
    assert_eq!(app.world().resource::<Messages<SaveGame>>().len(), 1);
    app.update();
    assert_eq!(app.world().resource::<MapProgress>().notes.len(), 1);
}

#[test]
fn canceled_touch_and_drag_out_then_back_never_become_annotations() {
    let (mut app, window) = input_app();
    open_with_supplies(&mut app, window);
    app.update();
    let a = Vec2::new(150.0, 300.0);
    touch(&mut app, window, 0, TouchPhase::Started, a);
    app.update();
    touch(&mut app, window, 0, TouchPhase::Canceled, a);
    app.update();
    assert!(app.world().resource::<MapProgress>().notes.is_empty());
    touch(&mut app, window, 0, TouchPhase::Started, a);
    touch(&mut app, window, 0, TouchPhase::Moved, a + Vec2::X * 70.0);
    touch(&mut app, window, 0, TouchPhase::Moved, a);
    touch(&mut app, window, 0, TouchPhase::Ended, a);
    app.update();
    assert!(app.world().resource::<MapProgress>().notes.is_empty());
    tap_events(&mut app, window, a);
    app.update();
    assert_eq!(app.world().resource::<MapProgress>().notes.len(), 1);
}

#[test]
fn coalesced_pinch_zooms_without_creating_notes_and_next_tap_works() {
    let (mut app, window) = input_app();
    open_with_supplies(&mut app, window);
    app.update();
    let scale = app.world().resource::<MapView>().scale;
    let a = Vec2::new(150.0, 350.0);
    let b = Vec2::new(210.0, 350.0);
    touch(&mut app, window, 0, TouchPhase::Started, a);
    touch(&mut app, window, 1, TouchPhase::Started, b);
    touch(&mut app, window, 1, TouchPhase::Moved, b + Vec2::X * 60.0);
    touch(&mut app, window, 0, TouchPhase::Ended, a);
    touch(&mut app, window, 1, TouchPhase::Ended, b + Vec2::X * 60.0);
    app.update();
    assert!(app.world().resource::<MapView>().scale > scale);
    assert!(app.world().resource::<MapProgress>().notes.is_empty());
    tap_events(&mut app, window, a);
    app.update();
    assert_eq!(app.world().resource::<MapProgress>().notes.len(), 1);
}

#[test]
fn final_up_position_and_rotation_cancel_unfinished_marks() {
    let (mut app, window) = input_app();
    open_with_supplies(&mut app, window);
    app.update();
    let a = Vec2::new(150.0, 350.0);
    touch(&mut app, window, 0, TouchPhase::Started, a);
    // Some platforms coalesce MOVE but preserve the release position.
    touch(&mut app, window, 0, TouchPhase::Ended, a + Vec2::Y * 60.0);
    app.update();
    assert!(app.world().resource::<MapProgress>().notes.is_empty());
    touch(&mut app, window, 0, TouchPhase::Started, a);
    app.update();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set(844.0, 390.0);
    app.update();
    touch(&mut app, window, 0, TouchPhase::Ended, a);
    app.update();
    assert!(app.world().resource::<MapProgress>().notes.is_empty());
}

#[test]
fn same_frame_rotation_does_not_reinterpret_completed_taps_in_old_coordinates() {
    let (mut app, window) = input_app();
    open_with_supplies(&mut app, window);
    app.update();
    // Both taps complete before the rotation update and reuse Android ID 0.
    // Their positions are inside both papers, so geometry alone cannot reject
    // them. A later intentional tap must work normally in the new layout.
    tap_events(&mut app, window, Vec2::new(200.0, 150.0));
    tap_events(&mut app, window, Vec2::new(300.0, 160.0));
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set(844.0, 390.0);
    app.update();
    assert!(app.world().resource::<MapProgress>().notes.is_empty());
    tap_events(&mut app, window, Vec2::new(300.0, 160.0));
    app.update();
    assert_eq!(app.world().resource::<MapProgress>().notes.len(), 1);
}

#[test]
fn held_opening_touch_cannot_mark_or_drag_and_close_does_not_fire() {
    let (mut app, window) = input_app();
    let before = player_position(&mut app);
    app.world_mut().resource_mut::<MapView>().tool = MapTool::Mark(NoteKind::Supplies);
    let toggle = toggle_rect(Vec2::new(390.0, 844.0)).center();
    touch(&mut app, window, 0, TouchPhase::Started, toggle);
    app.update();
    let center = app.world().resource::<MapView>().center;
    let paper = Vec2::new(200.0, 350.0);
    touch(&mut app, window, 0, TouchPhase::Moved, paper);
    app.update();
    touch(&mut app, window, 0, TouchPhase::Ended, paper);
    app.update();
    assert_eq!(app.world().resource::<MapView>().center, center);
    assert!(app.world().resource::<MapProgress>().notes.is_empty());
    assert_gameplay_blocked(&mut app, before);
    tap_events(&mut app, window, Vec2::new(342.0, 65.0));
    app.update();
    assert!(!app.world().resource::<MapOpen>().0);
    assert!(!app.world().resource::<PlayerInput>().fire);
    assert_eq!(player_position(&mut app), before);
}
