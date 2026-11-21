//! Sound effects and the Wanhua street bed.
//!
//! Files are under `assets/audio/` and listed in `assets/LICENSES.md`. Paths
//! below are relative to the `assets/` folder.
//!
//! Other modules trigger one-shots by writing [`SfxEvent`]. The placeholder
//! combat code already does this for shots, enemy deaths, and player
//! damage. Footsteps also play on their own while [`MoveInput`] is held and
//! the player is alive; [`SfxEvent::Footstep`] plays a single extra step.

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::combat::GameState;
use crate::player::{MoveInput, MoveSet};

pub const GUNSHOT: &str = "audio/gunshot.ogg";
pub const FOOTSTEPS: [&str; 3] = [
    "audio/footstep_01.ogg",
    "audio/footstep_02.ogg",
    "audio/footstep_03.ogg",
];
pub const ENEMY_DEATH: &str = "audio/enemy_death.ogg";
pub const PLAYER_HURT: &str = "audio/player_hurt.ogg";
pub const PLAYER_DEATH: &str = "audio/player_death.ogg";
pub const AMBIENT_CITY: &str = "audio/ambient_city.ogg";
pub const BGM: &str = "audio/bgm.ogg";

const STEP_SECONDS: f32 = 0.36;
const HURT_GAP: f32 = 0.28;

/// One-shot requests. Registering and playback live in [`AudioPlugin`].
#[derive(Message, Clone, Copy, Debug)]
pub enum SfxEvent {
    Gunshot,
    Footstep,
    EnemyDeath,
    PlayerHurt,
    PlayerDeath,
}

#[derive(Resource)]
struct Sfx {
    gunshot: Handle<AudioSource>,
    footsteps: [Handle<AudioSource>; 3],
    enemy_death: Handle<AudioSource>,
    player_hurt: Handle<AudioSource>,
    player_death: Handle<AudioSource>,
    ambient: Handle<AudioSource>,
    bgm: Handle<AudioSource>,
}

#[derive(Resource)]
struct Steps {
    timer: Timer,
    next: usize,
    hurt_gap: f32,
}

impl Default for Steps {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(STEP_SECONDS, TimerMode::Repeating),
            next: 0,
            hurt_gap: 0.0,
        }
    }
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SfxEvent>()
            .init_resource::<Steps>()
            .add_systems(Startup, start_audio)
            .add_systems(Update, (play_footsteps, play_events).chain().after(MoveSet));
    }
}

fn start_audio(mut commands: Commands, asset_server: Res<AssetServer>) {
    let sfx = Sfx {
        gunshot: asset_server.load(GUNSHOT),
        footsteps: FOOTSTEPS.map(|path| asset_server.load(path)),
        enemy_death: asset_server.load(ENEMY_DEATH),
        player_hurt: asset_server.load(PLAYER_HURT),
        player_death: asset_server.load(PLAYER_DEATH),
        ambient: asset_server.load(AMBIENT_CITY),
        bgm: asset_server.load(BGM),
    };
    commands.spawn((
        AudioPlayer::new(sfx.ambient.clone()),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.5)),
    ));
    commands.spawn((
        AudioPlayer::new(sfx.bgm.clone()),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.28)),
    ));
    commands.insert_resource(sfx);
}

fn play_events(
    mut commands: Commands,
    time: Res<Time>,
    sfx: Res<Sfx>,
    mut steps: ResMut<Steps>,
    mut events: MessageReader<SfxEvent>,
) {
    steps.hurt_gap = (steps.hurt_gap - time.delta_secs()).max(0.0);
    for event in events.read() {
        match event {
            SfxEvent::Gunshot => spawn_oneshot(&mut commands, sfx.gunshot.clone(), 0.7),
            SfxEvent::Footstep => {
                spawn_oneshot(&mut commands, next_step(&sfx, &mut steps), 0.5);
            }
            SfxEvent::EnemyDeath => spawn_oneshot(&mut commands, sfx.enemy_death.clone(), 0.65),
            SfxEvent::PlayerHurt => {
                if steps.hurt_gap > 0.0 {
                    continue;
                }
                steps.hurt_gap = HURT_GAP;
                spawn_oneshot(&mut commands, sfx.player_hurt.clone(), 0.6);
            }
            SfxEvent::PlayerDeath => spawn_oneshot(&mut commands, sfx.player_death.clone(), 0.75),
        }
    }
}

fn play_footsteps(
    time: Res<Time>,
    input: Res<MoveInput>,
    state: Res<GameState>,
    mut steps: ResMut<Steps>,
    mut events: MessageWriter<SfxEvent>,
) {
    let moving = !state.over && input.combined().length_squared() > 0.02;
    if !moving {
        steps.timer.reset();
        return;
    }
    steps.timer.tick(time.delta());
    if steps.timer.just_finished() {
        events.write(SfxEvent::Footstep);
    }
}

fn next_step(sfx: &Sfx, steps: &mut Steps) -> Handle<AudioSource> {
    let handle = sfx.footsteps[steps.next % sfx.footsteps.len()].clone();
    steps.next = steps.next.wrapping_add(1);
    handle
}

fn spawn_oneshot(commands: &mut Commands, source: Handle<AudioSource>, volume: f32) {
    commands.spawn((
        AudioPlayer::new(source),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)),
    ));
}
