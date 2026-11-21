//! Defend Taipei — native vertical slice.
//!
//! All gameplay lives in this crate so the desktop binary (`src/main.rs`) and the
//! Android `cdylib` (`mobile/`) run exactly the same code.

use bevy::{prelude::*, window::WindowMode};

mod camera;
mod city;
mod combat;
mod game_flow;
mod hud;
mod player;
mod story;
mod touch;

/// Builds and runs the game. Called by the desktop `main` and by the Android entry point.
pub fn run() {
    let window = Window {
        title: "Defend Taipei — Wanhua slice".into(),
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
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(window),
                    ..default()
                })
                // Crate dir is `game/`; shared files live in the repo-root `assets/`.
                .set(bevy::asset::AssetPlugin {
                    file_path: "../assets".to_string(),
                    ..default()
                }),
        )
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
            touch::TouchPlugin,
            combat::CombatPlugin,
            game_flow::GameFlowPlugin,
            hud::HudPlugin,
        ))
        .run();
}
