//! Headless, real-time audio-output fixtures. The companion Python harness
//! records the actual PulseAudio monitor; these counters are not sound proof.
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::audio::Decodable;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::window::{AppLifecycle, WindowFocused};

use crate::audio::{
    self, AudioCategory, AudioDiagnostics, AudioSettings, GunKind, SfxEvent, Surface,
};
use crate::combat::GameState;
use crate::game_flow::GameFlow;
use crate::story::{StoryAlert, StoryBroadcast};

#[derive(Resource)]
struct Scene {
    name: String,
    output: PathBuf,
    deadline: Instant,
    started: Option<Instant>,
    started_unix_seconds: Option<f64>,
    transition_unix_seconds: Option<f64>,
    beat: usize,
    decoded: usize,
    decoded_samples: u64,
    decoded_peak: f32,
    peak_sinks: usize,
    transitioned: bool,
    finished: bool,
}

pub fn run() {
    let name =
        std::env::var("DEFEND_TAIPEI_AUDIO_QA_SCENARIO").unwrap_or_else(|_| "showcase".into());
    let output = std::env::var_os("DEFEND_TAIPEI_AUDIO_QA_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("audio-qa-scene"));
    std::fs::create_dir_all(&output).expect("audio QA output directory");
    let mut settings = AudioSettings {
        ambience: 0.0,
        radio: 0.0,
        ..default()
    };
    match name.as_str() {
        "muted" => settings.muted = true,
        "effects_zero" => settings.effects = 0.0,
        "ambience" => {
            settings.effects = 0.0;
            settings.ambience = 0.7;
        }
        "ambience_zero" => settings.effects = 0.0,
        "radio" => {
            settings.effects = 0.0;
            settings.radio = 0.8;
        }
        "radio_zero" => settings.effects = 0.0,
        "resume" | "live_muted" | "live_paused" | "live_background" => {
            settings.effects = 0.0;
            settings.ambience = 0.7;
        }
        "armor_engine"
        | "armor_engine_live_muted"
        | "armor_engine_live_paused"
        | "armor_engine_live_background" => {
            settings.effects = 0.0;
            settings.ambience = 0.7;
        }
        "armor_engine_zero" => settings.effects = 0.0,
        "showcase" | "paused" | "title_preview" => {}
        _ => panic!("Unknown audio QA scenario: {name}"),
    }
    let flow = if name == "title_preview" {
        GameFlow::Title
    } else {
        GameFlow::Playing
    };
    let paused = name == "paused" || name == "resume";
    let rain = if name.starts_with("ambience") || name.starts_with("live_") || name == "resume" {
        1.0
    } else {
        0.0
    };
    let engine_distance = name.starts_with("armor_engine").then_some(12.0);
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16))),
        bevy::log::LogPlugin::default(),
        StatesPlugin,
        TransformPlugin,
        AssetPlugin {
            file_path: "../assets".into(),
            ..default()
        },
        bevy::audio::AudioPlugin::default(),
    ))
    .insert_state(flow)
    .init_resource::<GameState>()
    .insert_resource(crate::ui::PauseOpen(paused))
    .insert_resource(settings)
    .insert_resource(audio::Ambience { rain, ..default() })
    .insert_resource(audio::AudioScheduleControl { automatic: false })
    .insert_resource(audio::engine::ArmorEngineFixture(engine_distance))
    .insert_resource(Scene {
        name,
        output,
        deadline: Instant::now() + Duration::from_secs(50),
        started: None,
        started_unix_seconds: None,
        transition_unix_seconds: None,
        beat: 0,
        decoded: 0,
        decoded_samples: 0,
        decoded_peak: 0.0,
        peak_sinks: 0,
        transitioned: false,
        finished: false,
    })
    .add_message::<WindowFocused>()
    .add_message::<AppLifecycle>()
    .add_message::<StoryAlert>()
    .add_message::<StoryBroadcast>()
    .add_plugins(audio::AudioPlugin)
    .add_systems(Update, drive_scene.before(crate::ui::settings::SettingsSet));
    let result = app.run();
    if result.is_error() {
        std::process::exit(1);
    }
}

fn showcase() -> Vec<SfxEvent> {
    use SfxEvent::*;
    vec![
        Gunshot {
            kind: GunKind::Pistol,
            dist: 0.0,
            indoor: false,
        },
        Gunshot {
            kind: GunKind::Rifle,
            dist: 4.0,
            indoor: true,
        },
        Gunshot {
            kind: GunKind::Sniper,
            dist: 90.0,
            indoor: false,
        },
        Gunshot {
            kind: GunKind::Mg,
            dist: 12.0,
            indoor: false,
        },
        Gunshot {
            kind: GunKind::Shotgun,
            dist: 1.0,
            indoor: true,
        },
        ShellBoom {
            dist: 30.0,
            indoor: false,
        },
        Footstep {
            surface: Surface::Tile,
            dist: 0.0,
            heavy: false,
            run: false,
        },
        Footstep {
            surface: Surface::Asphalt,
            dist: 0.0,
            heavy: false,
            run: true,
        },
        Footstep {
            surface: Surface::Wood,
            dist: 0.0,
            heavy: true,
            run: false,
        },
        Footstep {
            surface: Surface::Metro,
            dist: 0.0,
            heavy: false,
            run: false,
        },
        Door {
            open: true,
            dist: 0.0,
        },
        Door {
            open: false,
            dist: 0.0,
        },
        Rummage,
        Pickup,
        Equip,
        Reload,
        DryFire,
        Consume { drink: true },
        Consume { drink: false },
        Heal,
        Cook,
        Water,
        Swish {
            dist: 0.0,
            heavy: false,
        },
        MeleeHit {
            dist: 0.0,
            blunt: true,
        },
        Barricade {
            remove: false,
            dist: 0.0,
        },
        Barricade {
            remove: true,
            dist: 0.0,
        },
        Build,
        Ui { confirm: true },
    ]
}

fn decode_manifest(world: &World) -> Result<(usize, u64, f32), String> {
    let server = world.resource::<AssetServer>();
    let assets = world.resource::<Assets<AudioSource>>();
    let mut count = 0;
    let mut total_samples = 0_u64;
    let mut peak = 0.0_f32;
    for spec in audio::audio_manifest() {
        let handle: Handle<AudioSource> = server.load(spec.path.clone());
        let source = assets
            .get(&handle)
            .ok_or_else(|| format!("Missing {}", spec.path))?;
        let mut samples = 0_u64;
        let mut energy = 0.0_f64;
        for sample in source.decoder() {
            samples += 1;
            if samples > 2_000_000 || !sample.is_finite() || sample.abs() > 0.9 {
                return Err(format!("Invalid decoded samples: {}", spec.path));
            }
            peak = peak.max(sample.abs());
            energy += f64::from(sample) * f64::from(sample);
        }
        if samples == 0 || energy == 0.0 {
            return Err(format!("Invalid decoded samples: {}", spec.path));
        }
        total_samples += samples;
        count += 1;
    }
    Ok((count, total_samples, peak))
}

fn drive_scene(world: &mut World) {
    if world.resource::<Scene>().finished {
        return;
    }
    if Instant::now() > world.resource::<Scene>().deadline {
        finish(world, Some("Audio QA timed out".into()));
        return;
    }
    let sinks = world.query::<&AudioSink>().iter(world).count();
    world.resource_mut::<Scene>().peak_sinks = world.resource::<Scene>().peak_sinks.max(sinks);
    if world.resource::<Scene>().started.is_none() {
        let diagnostics = world.resource::<AudioDiagnostics>();
        if diagnostics.total_clips == 0
            || diagnostics.ready_clips != diagnostics.total_clips
            || sinks == 0
        {
            return;
        }
        match decode_manifest(world) {
            Ok((decoded, decoded_samples, decoded_peak)) => {
                let mut scene = world.resource_mut::<Scene>();
                scene.decoded = decoded;
                scene.decoded_samples = decoded_samples;
                scene.decoded_peak = decoded_peak;
                scene.started = Some(Instant::now());
                scene.started_unix_seconds = Some(unix_seconds());
            }
            Err(error) => finish(world, Some(error)),
        }
        return;
    }
    let (name, elapsed, beat) = {
        let scene = world.resource::<Scene>();
        (
            scene.name.clone(),
            scene.started.unwrap().elapsed().as_secs_f32(),
            scene.beat,
        )
    };
    if elapsed >= 1.5 && !world.resource::<Scene>().transitioned {
        match name.as_str() {
            "resume" => world.resource_mut::<crate::ui::PauseOpen>().0 = false,
            "live_muted" | "armor_engine_live_muted" => {
                world.resource_mut::<AudioSettings>().muted = true
            }
            "live_paused" | "armor_engine_live_paused" => {
                world.resource_mut::<crate::ui::PauseOpen>().0 = true
            }
            "live_background" | "armor_engine_live_background" => {
                world.write_message(WindowFocused {
                    window: Entity::PLACEHOLDER,
                    focused: false,
                });
            }
            _ => {}
        }
        if matches!(
            name.as_str(),
            "resume"
                | "live_muted"
                | "live_paused"
                | "live_background"
                | "armor_engine_live_muted"
                | "armor_engine_live_paused"
                | "armor_engine_live_background"
        ) {
            world.resource_mut::<Scene>().transition_unix_seconds = Some(unix_seconds());
        }
        world.resource_mut::<Scene>().transitioned = true;
    }
    let events = showcase();
    let interval = if name == "showcase" { 0.7 } else { 0.6 };
    let duration = if name == "showcase" {
        events.len() as f32 * interval + 1.5
    } else {
        4.5
    };
    if elapsed >= duration {
        let error = if name == "showcase"
            && world.resource::<AudioDiagnostics>().recorded_started < 6
        {
            Some("Recorded gun/boom dispatches were not exercised".into())
        } else if name.starts_with("armor_engine")
            && world
                .resource::<AudioDiagnostics>()
                .started
                .get("armor_engine")
                .copied()
                .unwrap_or(0)
                != u64::from(name != "armor_engine_zero")
        {
            Some("Armor engine fixture did not keep exactly one bounded voice (or zero at zero ambience)".into())
        } else {
            None
        };
        finish(world, error);
        return;
    }
    if elapsed >= beat as f32 * interval {
        let event = match name.as_str() {
            "showcase" => events.get(beat).copied(),
            "ambience"
            | "ambience_zero"
            | "resume"
            | "live_muted"
            | "live_paused"
            | "live_background"
            | "armor_engine"
            | "armor_engine_zero"
            | "armor_engine_live_muted"
            | "armor_engine_live_paused"
            | "armor_engine_live_background" => None,
            "title_preview" => Some(SfxEvent::Preview {
                category: AudioCategory::Effects,
            }),
            "radio" | "radio_zero" => Some(SfxEvent::Radio { dist: 0.0 }),
            _ => Some(SfxEvent::Gunshot {
                kind: GunKind::Pistol,
                dist: 0.0,
                indoor: false,
            }),
        };
        if let Some(event) = event {
            world.write_message(event);
        }
        world.resource_mut::<Scene>().beat += 1;
    }
}

fn unix_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after Unix epoch")
        .as_secs_f64()
}

fn finish(world: &mut World, error: Option<String>) {
    let scene = world.resource::<Scene>();
    let finished_unix_seconds = unix_seconds();
    let report = serde_json::json!({
        "success": error.is_none(), "error": error, "scenario": scene.name,
        "decoded_clips": scene.decoded, "peak_audio_sinks": scene.peak_sinks,
        "decoded_samples": scene.decoded_samples, "decoded_peak": scene.decoded_peak,
        "runtime_seconds": scene.started.map(|start| start.elapsed().as_secs_f64()),
        "started_unix_seconds": scene.started_unix_seconds,
        "transition_unix_seconds": scene.transition_unix_seconds,
        "finished_unix_seconds": finished_unix_seconds,
        "diagnostics": world.resource::<AudioDiagnostics>(),
        "scope": "Actual Bevy asset decoding and AudioSink dispatch; PCM output is independently captured by the companion script. No physical-phone claim."
    });
    let write = std::fs::write(
        scene.output.join("audio-scene.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    );
    world.resource_mut::<Scene>().finished = true;
    world.write_message(if error.is_some() || write.is_err() {
        AppExit::error()
    } else {
        AppExit::Success
    });
}
