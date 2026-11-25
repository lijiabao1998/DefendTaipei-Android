//! Actual UI → map → gameplay-input schedules, without a renderer or a window server.

use super::*;
use bevy::{state::app::StatesPlugin, time::TimeUpdateStrategy};
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
