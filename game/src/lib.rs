//! Defend Taipei — native vertical slice.
//!
//! All gameplay lives in this crate so the desktop binary (`src/main.rs`) and the
//! Android `cdylib` (`mobile/`) run exactly the same code.

use bevy::{prelude::*, window::WindowMode};

mod audio;
mod camera;
mod city;
pub mod combat;
pub mod game_flow;
mod geom;
mod hud;
mod human;
pub mod i18n;
pub mod input;
pub mod map;
mod player;
pub mod roles;
mod story;
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
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(window),
                    ..default()
                })
                // The crate lives in `game/`; audio and the UI font live in the repo-root `assets/`.
                // `cargo run` sets CARGO_MANIFEST_DIR to `game/`. Android ignores this and
                // reads the APK asset root. Override with BEVY_ASSET_ROOT when launching
                // the binary directly.
                .set(bevy::asset::AssetPlugin {
                    file_path: "../assets".to_string(),
                    ..default()
                }),
        )
        .insert_resource(ClearColor(Color::srgb(0.08, 0.075, 0.07)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.81, 0.89, 1.0),
            brightness: 600.0,
            ..default()
        })
        .add_plugins((
            map::MapPlugin,
            human::HumanPlugin,
            player::PlayerPlugin,
            camera::CameraPlugin,
            i18n::I18nPlugin,
            input::InputPlugin,
            touch::TouchPlugin,
            combat::CombatPlugin,
            game_flow::GameFlowPlugin,
            audio::AudioPlugin,
            hud::HudPlugin,
            ui::FlowUiPlugin,
        ))
        .run();
}
