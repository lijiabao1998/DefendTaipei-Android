//! Defend Taipei — native vertical slice.
//!
//! All gameplay lives in this crate so the desktop binary (`src/main.rs`) and the
//! Android `cdylib` (`mobile/`) run exactly the same code.

use bevy::{prelude::*, window::WindowMode};

mod audio;
#[cfg(not(target_os = "android"))]
mod audio_qa;
mod camera;
pub mod chunk;
mod city;
pub mod combat;
pub mod device;
pub mod doors;
pub mod game_flow;
mod geom;
mod hud;
mod human;
pub mod i18n;
pub mod input;
pub mod inventory;
pub mod map;
mod perf;
pub mod performance;
mod player;
#[cfg(not(target_os = "android"))]
mod qa;
pub mod roles;
pub mod save;
pub mod settings;
mod story;
pub mod survival;
mod touch;
pub mod ui;
pub mod worldmap;
pub mod worldplay;

/// Builds and runs the game. Called by the desktop `main` and by the Android entry point.
pub fn run() {
    #[cfg(not(target_os = "android"))]
    if std::env::args().any(|arg| arg == "--audio-qa") {
        audio_qa::run();
        return;
    }
    // Early marker for CI / logcat grepping (before window / GPU init).
    println!("DTPERF boot starting");

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

    #[cfg(not(target_os = "android"))]
    let qa_mode = std::env::args().any(|arg| arg == "--qa");
    #[cfg(target_os = "android")]
    let qa_mode = false;
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(window),
            exit_condition: if qa_mode {
                bevy::window::ExitCondition::DontExit
            } else {
                bevy::window::ExitCondition::OnAllClosed
            },
            ..default()
        })
        // The crate lives in `game/`; audio and the UI font live in the repo-root `assets/`.
        // `cargo run` sets CARGO_MANIFEST_DIR to `game/`. Android ignores this and
        // reads the APK asset root. Override with BEVY_ASSET_ROOT when launching
        // the binary directly.
        .set(bevy::asset::AssetPlugin {
            file_path: "../assets".to_string(),
            ..default()
        });
    #[cfg(not(target_os = "android"))]
    let plugins = if qa_mode {
        plugins.disable::<bevy::winit::WinitPlugin>()
    } else {
        plugins
    };
    let mut app = App::new();
    app.add_plugins(plugins)
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
        ))
        .add_plugins((
            settings::SettingsPlugin,
            device::DevicePlugin,
            performance::PerformancePlugin,
            game_flow::GameFlowPlugin,
            audio::AudioPlugin,
            hud::HudPlugin,
            ui::FlowUiPlugin,
            chunk::ChunkPlugin,
            perf::PerfPlugin,
            survival::ShelterPlugin,
            worldplay::WorldPlayPlugin,
            save::SavePlugin,
            doors::DoorPlugin,
            worldmap::WorldMapPlugin,
            map::atmosphere::AtmospherePlugin,
        ));
    #[cfg(not(target_os = "android"))]
    if qa_mode {
        app.add_plugins((
            bevy::app::ScheduleRunnerPlugin::run_loop(std::time::Duration::from_millis(16)),
            qa::QaPlugin,
        ));
    }
    let exit = app.run();
    #[cfg(not(target_os = "android"))]
    if qa_mode && exit.is_error() {
        std::process::exit(1);
    }
    #[cfg(target_os = "android")]
    let _ = exit;
}
