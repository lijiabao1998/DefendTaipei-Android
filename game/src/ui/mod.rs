//! Survivor select, prologue, and the four original endings.
//!
//! Buttons only write [`crate::game_flow::StartGame`], [`crate::game_flow::SkipPrologue`],
//! and [`crate::game_flow::RestartGame`]. The UI reads [`crate::game_flow::GameFlow`]
//! and does not set it. `game/src/game_flow.rs` is the provisional stand-in for
//! lijiabao5's module; delete that file when PR #5 lands.

mod layout;
mod screens;

use bevy::prelude::*;

use crate::game_flow::GameFlow;
use crate::roles::SelectedRole;

pub struct FlowUiPlugin;

impl Plugin for FlowUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedRole>()
            .add_systems(OnEnter(GameFlow::Title), screens::enter_title)
            .add_systems(OnExit(GameFlow::Title), screens::despawn_title)
            .add_systems(
                Update,
                (
                    screens::sync_overlay_screens,
                    screens::restyle,
                    screens::refresh_copy,
                    screens::title_clicks,
                    screens::prologue_clicks,
                    screens::death_clicks,
                )
                    .after(crate::i18n::apply_font),
            );
    }
}
