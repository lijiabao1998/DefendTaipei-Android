//! Survivor select, prologue, pause, and the four original endings.
//!
//! Buttons only write [`crate::game_flow::StartGame`], [`crate::game_flow::SkipPrologue`],
//! and [`crate::game_flow::RestartGame`]. The UI reads [`crate::game_flow::GameFlow`]
//! and does not set it. Each screen root is [`StateScoped`] so it despawns with the
//! state. Pointer presses on those screens are consumed here; the flow module's
//! own overlay is left in place.

mod layout;
mod pause;
mod screens;

use bevy::{prelude::*, window::PrimaryWindow};

use crate::game_flow::GameFlow;
use crate::input::GamePause;
use crate::roles::SelectedRole;

/// Bevy 0.20 renamed `StateScoped` to [`DespawnOnExit`]. Same marker: the entity
/// and its children despawn when `GameFlow` leaves this value.
pub(super) use bevy::prelude::DespawnOnExit as StateScoped;

use layout::control_hit;

/// Pointer presses swallowed by our screens, in window coordinates.
#[derive(Resource, Default)]
pub(super) struct ConsumedPresses {
    pub points: Vec<Vec2>,
}

pub struct FlowUiPlugin;

impl Plugin for FlowUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedRole>()
            .init_resource::<ConsumedPresses>()
            .add_systems(OnEnter(GameFlow::Title), screens::enter_title)
            .add_systems(
                PreUpdate,
                consume_ui_clicks.after(bevy::input::InputSystems),
            )
            .add_systems(
                Update,
                (
                    screens::ensure_screens,
                    screens::restyle,
                    screens::refresh_copy,
                    screens::title_clicks,
                    screens::prologue_clicks,
                    screens::death_clicks,
                    pause::drive_pause,
                    pause::place_pause,
                )
                    .after(crate::i18n::apply_font)
                    .after(crate::input::InputSet),
            );
    }
}

fn consume_ui_clicks(
    flow: Res<State<GameFlow>>,
    pause: Res<GamePause>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut touches: ResMut<Touches>,
    mut consumed: ResMut<ConsumedPresses>,
) {
    consumed.points.clear();
    let Some(window) = windows.iter().next() else {
        return;
    };
    let viewport = Vec2::new(window.width(), window.height());
    let hit = |point: Vec2| control_hit(flow.get(), viewport, point, pause.0);
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
}
