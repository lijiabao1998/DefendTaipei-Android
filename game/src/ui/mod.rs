//! Survivor select, prologue, pause, and the four original endings.
//!
//! Buttons only write [`crate::game_flow::StartGame`], [`crate::game_flow::SkipPrologue`],
//! and [`crate::game_flow::RestartGame`]. The UI reads [`crate::game_flow::GameFlow`]
//! and does not set it. Each screen root is [`StateScoped`] so it despawns with the
//! state. Pointer presses on those screens are consumed here; the flow module's
//! own overlay is left in place.

mod layout;
mod pause;
mod portrait;
mod scavenge;
mod screens;
pub(crate) mod settings;
mod shelter;
mod title;

pub(crate) use title::{TitleAction, TitleMenu, TitlePage};

pub use shelter::{OpenPanel, PanelKind, UiModal};

use bevy::{prelude::*, window::PrimaryWindow};

use crate::game_flow::GameFlow;
use crate::roles::SelectedRole;

/// Show or hide the pause screen. Input (Esc) sends this; the UI does not read keys.
#[derive(Message, Clone, Copy, Debug)]
pub struct TogglePause;

/// Whether the pause screen is up. Flipped by [`TogglePause`].
#[derive(Resource, Default)]
pub(super) struct PauseOpen(pub bool);

/// Bevy 0.20 renamed `StateScoped` to [`DespawnOnExit`]. Same marker: the entity
/// and its children despawn when `GameFlow` leaves this value.
pub(super) use bevy::prelude::DespawnOnExit as StateScoped;

use layout::control_hit;

/// Pointer presses swallowed by our screens, in window coordinates.
#[derive(Resource, Default)]
pub(crate) struct ConsumedPresses {
    pub points: Vec<Vec2>,
}

pub struct FlowUiPlugin;

impl Plugin for FlowUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedRole>()
            .init_resource::<TitleMenu>()
            .init_resource::<title::TitleAssets>()
            .init_resource::<title::TitleCache>()
            .init_resource::<ConsumedPresses>()
            .init_resource::<crate::dialogue::Dialogue>()
            .add_message::<crate::dialogue::DialogueChosen>()
            .init_resource::<PauseOpen>()
            .init_resource::<settings::SettingsUi>()
            .init_resource::<settings::SettingsCache>()
            .init_resource::<crate::audio::AudioSettings>()
            .init_resource::<crate::settings::GamePreferences>()
            .init_resource::<crate::performance::PerformancePreferences>()
            .init_resource::<crate::settings::SettingsStatus>()
            .add_message::<crate::performance::PerformanceAction>()
            .add_message::<settings::SettingsAction>()
            .add_message::<crate::settings::FlushSettings>()
            .add_message::<bevy::input::touch::TouchInput>()
            .add_message::<bevy::input::mouse::MouseButtonInput>()
            .add_message::<bevy::window::WindowFocused>()
            .add_message::<bevy::window::AppLifecycle>()
            .init_resource::<UiModal>()
            .init_resource::<crate::worldplay::ScavengeSession>()
            .init_resource::<shelter::PanelState>()
            .init_resource::<shelter::RenderCache>()
            .add_message::<TogglePause>()
            .add_message::<TitleAction>()
            .add_message::<OpenPanel>()
            .add_systems(OnEnter(GameFlow::Title), title::reset)
            .add_systems(
                Update,
                (settings::pointer_actions, settings::apply_actions)
                    .chain()
                    .in_set(settings::SettingsSet)
                    .before(title::pointer_actions)
                    .before(pause::drive_pause),
            )
            .add_systems(
                Update,
                (title::pointer_actions, title::apply_actions)
                    .chain()
                    .before(crate::game_flow::FlowSet),
            )
            .add_systems(
                PreUpdate,
                consume_ui_clicks.after(bevy::input::InputSystems),
            )
            .add_systems(
                Update,
                shelter::drive_panels
                    .before(crate::worldmap::MapInputSet)
                    .before(crate::input::InputSet),
            )
            .add_systems(
                Update,
                (
                    screens::ensure_screens,
                    screens::restyle,
                    screens::refresh_copy,
                    screens::prologue_clicks,
                    screens::death_clicks,
                    pause::drive_pause
                        .after(crate::worldplay::WorldInputSet)
                        .before(crate::game_flow::FlowSet),
                    pause::place_pause.after(pause::drive_pause),
                )
                    .after(crate::i18n::apply_font),
            );
        app.add_systems(
            PostUpdate,
            (shelter::render_panels, title::render, settings::render)
                .before(bevy::ui::UiSystems::Prepare)
                .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
    }
}

fn consume_ui_clicks(
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut touches: ResMut<Touches>,
    mut consumed: ResMut<ConsumedPresses>,
    map: Option<Res<crate::worldmap::MapOpen>>,
    geometry: Option<Res<crate::hud::HudGeometry>>,
    mut modal: ResMut<UiModal>,
) {
    consumed.points.clear();
    let Some(window) = windows.iter().next() else {
        return;
    };
    let viewport = Vec2::new(window.width(), window.height());
    let hit = |point: Vec2| {
        control_hit(flow.get(), viewport, point, pause.0)
            || (flow.get() == &GameFlow::Playing
                && (modal.open
                    || shelter::toolbar_hit(viewport, point)
                    || geometry
                        .as_ref()
                        .and_then(|g| g.banner)
                        .is_some_and(|rect| rect.contains(point))
                    || crate::worldmap::captures_pointer(
                        viewport,
                        point,
                        map.as_ref().is_some_and(|m| m.0),
                    )))
    };
    if mouse.just_pressed(MouseButton::Left)
        && let Some(cursor) = window.cursor_position()
        && hit(cursor)
    {
        consumed.points.push(cursor);
        mouse.clear_just_pressed(MouseButton::Left);
    }
    let touches_hit: Vec<_> = touches
        .iter_just_pressed()
        .filter(|touch| hit(touch.position()))
        .map(|touch| (touch.id(), touch.position()))
        .collect();
    for (id, position) in touches_hit {
        consumed.points.push(position);
        touches.clear_just_pressed(id);
    }
    if !consumed.points.is_empty() {
        modal.pointer_captured = true;
    } else if !mouse.pressed(MouseButton::Left) && touches.iter().next().is_none() {
        modal.pointer_captured = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{state::app::StatesPlugin, time::TimeUpdateStrategy};
    use std::time::Duration;

    #[test]
    fn pause_toolbar_captures_input_and_stops_the_clock_in_the_same_frame() {
        use crate::audio::SfxEvent;
        use crate::combat::{GameState, PlayerDied, ResetCombat};
        use crate::game_flow::{Campaign, FlowSet, GameFlowPlugin};
        use crate::i18n::{Language, UiFont};
        use crate::input::{InputPlugin, InputSet, PlayerInput};
        use crate::inventory::Inventory;
        use crate::player::{MoveInput, Player};
        use crate::save::{ContinueGame, SaveAndMenu, SaveGame, SaveStatus};
        use crate::survival::Shelter;
        use crate::worldplay::{MoralPrompt, PlayerNeeds, WorldAction, WorldInputSet, WorldNotice};

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .init_resource::<Language>()
            .init_resource::<UiFont>()
            .init_resource::<GameState>()
            .init_resource::<Inventory>()
            .init_resource::<Shelter>()
            .init_resource::<PlayerNeeds>()
            .init_resource::<MoralPrompt>()
            .init_resource::<WorldNotice>()
            .init_resource::<SaveStatus>()
            .init_resource::<MoveInput>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Touches>()
            .add_message::<ResetCombat>()
            .add_message::<PlayerDied>()
            .add_message::<SfxEvent>()
            .add_message::<WorldAction>()
            .add_message::<SaveGame>()
            .add_message::<ContinueGame>()
            .add_message::<SaveAndMenu>()
            .add_plugins((GameFlowPlugin, InputPlugin, FlowUiPlugin))
            .insert_state(GameFlow::Playing)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)))
            .configure_sets(Update, WorldInputSet.after(InputSet).before(FlowSet));
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
        app.world_mut().spawn((Player, Transform::default()));
        app.world_mut().resource_mut::<Campaign>().game_min = 440.0;
        app.update();
        let before = app.world().resource::<Campaign>().game_min;
        let point = shelter::toolbar_rects(Vec2::new(390.0, 844.0))[4].center();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(point));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.update();
        assert!(app.world().resource::<Time>().delta_secs() > 0.0);
        assert!(app.world().resource::<PauseOpen>().0);
        assert!(app.world().resource::<UiModal>().pointer_captured);
        assert_eq!(app.world().resource::<Campaign>().game_min, before);
        assert!(!app.world().resource::<PlayerInput>().fire);
        assert_eq!(app.world().resource::<PlayerInput>().r#move, Vec2::ZERO);

        // Android screenshots exposed the pads above an otherwise working
        // modal. Check the actual control entities, including after release.
        let controls_hidden = |app: &mut App| {
            let world = app.world_mut();
            let mut query = world.query::<(&GlobalZIndex, &Visibility)>();
            let controls: Vec<_> = query
                .iter(world)
                .filter(|(layer, _)| matches!(layer.0, 12 | 16))
                .map(|(_, visibility)| *visibility)
                .collect();
            assert_eq!(controls.len(), 3);
            controls.iter().all(|v| *v == Visibility::Hidden)
        };
        assert!(controls_hidden(&mut app));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        assert!(!app.world().resource::<UiModal>().pointer_captured);
        assert!(controls_hidden(&mut app));
        app.world_mut().resource_mut::<PauseOpen>().0 = false;
        app.world_mut().write_message(OpenPanel(PanelKind::Shelter));
        app.update();
        assert!(app.world().resource::<UiModal>().open);
        assert!(controls_hidden(&mut app));
    }
}
