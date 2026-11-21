//! Defend Taipei — native vertical slice.
//!
//! All gameplay lives in this crate so the desktop binary (`src/main.rs`) and the
//! Android `cdylib` (`mobile/`) run exactly the same code.

use bevy::{prelude::*, window::WindowMode};

mod camera;
mod city;
mod combat;
pub mod game_flow;
mod hud;
pub mod i18n;
pub mod input;
mod player;
pub mod roles;
mod touch;
pub mod ui;

/// Builds and runs the game. Called by the desktop `main` and by the Android entry point.
pub fn run() {
    let window = Window {
        title: "保衛台北 Defend Taipei".into(),
        // Phones run fullscreen; desktop opens a resizable landscape window you can
        // drag into a portrait shape to test the phone layout.
        mode: if cfg!(target_os = "android") {
            WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
        } else {
            WindowMode::Windowed
        },
        resolution: (1280, 720).into(),
        resizable: true,
        ..default()
    };

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(window),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.60, 0.64, 0.67)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.81, 0.89, 1.0),
            brightness: 600.0,
            ..default()
        })
        .add_plugins((
            city::CityPlugin,
            player::PlayerPlugin,
            camera::CameraPlugin,
            i18n::I18nPlugin,
            game_flow::GameFlowPlugin,
            input::InputPlugin,
            touch::TouchPlugin,
            combat::CombatPlugin,
            hud::HudPlugin,
            ui::FlowUiPlugin,
        ))
        .run();
}
