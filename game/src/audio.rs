//! Port of the web build's procedural audio (`src/audio/audio.ts`,
//! `src/story/broadcasts/broadcast.ts`).
//!
//! The browser game synthesizes cues in WebAudio and, for guns, falls back to
//! that synthesis when the optional recordings are missing. These OGG files are
//! that fallback, rendered ahead of time. Distance uses the original
//! `1 / (1 + dist / reference)` curve and the same cutoff distances. Indoor
//! shots are quieter, standing in for the original low-pass. The 萬安 alert
//! schedule (three cycles of 15 s + 5 s + 5 s, with 5 s gaps) loops `siren.ogg`.
//! The all-clear is the web build's already-shortened tone. Speech synthesis
//! is not available here: [`SfxEvent::Broadcast`] plays the radio-bed hiss only.
//!
//! Other modules write [`SfxEvent`]. The placeholder combat code already does
//! that for shots, player damage, and deaths.

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::combat::GameState;
use crate::game_flow::GameFlow;
use crate::player::{MoveInput, MoveSet};
use crate::story::{self, StoryAlert, StoryBroadcast};

pub const RAIN: &str = "audio/rain.ogg";
pub const ROOM: &str = "audio/room.ogg";

/// Matches `GunKind` in `src/audio/audio.ts`.
/// Variants other than rifle are the public contract for firearms code.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GunKind {
    Pistol,
    Rifle,
    Sniper,
    Mg,
    Shotgun,
    At,
}

/// Matches `Surface` in `src/audio/audio.ts`.
/// Streets use asphalt; interiors and the metro send the other variants.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Tile,
    Asphalt,
    Wood,
    Metro,
}

/// `alert` bus payload from `src/core/bus.ts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Test,
    Alert,
    Clear,
}

/// One-shot and siren requests. [`AudioPlugin`] registers this message.
/// Placeholder combat sends shots, hurt, and death. The rest is for firearms,
/// story, and scavenging once those modules land.
#[allow(dead_code)]
#[derive(Message, Clone, Copy, Debug)]
pub enum SfxEvent {
    Gunshot {
        kind: GunKind,
        dist: f32,
        indoor: bool,
    },
    Footstep {
        surface: Surface,
        dist: f32,
        heavy: bool,
        run: bool,
    },
    Hurt {
        dist: f32,
    },
    Death {
        dist: f32,
    },
    Swish {
        dist: f32,
        heavy: bool,
    },
    MeleeHit {
        dist: f32,
        blunt: bool,
    },
    Radio {
        dist: f32,
    },
    Clink {
        dist: f32,
    },
    Rummage,
    Whistle {
        dist: f32,
    },
    ShellBoom {
        dist: f32,
        indoor: bool,
    },
    Alert {
        kind: AlertKind,
    },
    /// Radio-bed hiss. The Chinese copy is owned by story/UI, not spoken here.
    Broadcast,
}

/// The web build's `AmbState`, written by whoever knows the weather and the room.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Ambience {
    pub rain: f32,
    pub indoor: bool,
    pub home: bool,
    pub night: bool,
    pub metro: bool,
    /// Kept for the web build's `AmbState`. Nothing in this slice reads it yet.
    #[allow(dead_code)]
    pub combat: bool,
}

impl Default for Ambience {
    fn default() -> Self {
        Self {
            rain: 0.0,
            indoor: false,
            home: false,
            night: false,
            metro: false,
            combat: false,
        }
    }
}

#[derive(Resource)]
struct Clips {
    guns: [Handle<AudioSource>; 6],
    steps: [Handle<AudioSource>; 4],
    hurt: Handle<AudioSource>,
    death: Handle<AudioSource>,
    swish: Handle<AudioSource>,
    melee: Handle<AudioSource>,
    radio: Handle<AudioSource>,
    clink: Handle<AudioSource>,
    rummage: Handle<AudioSource>,
    alert_test: Handle<AudioSource>,
    siren: Handle<AudioSource>,
    all_clear: Handle<AudioSource>,
    whistle: Handle<AudioSource>,
    boom: Handle<AudioSource>,
    explosion: Handle<AudioSource>,
    dog: Handle<AudioSource>,
    scooter: Handle<AudioSource>,
    chime: Handle<AudioSource>,
    radio_bed: Handle<AudioSource>,
}

#[derive(Component)]
struct RainBed;

#[derive(Component)]
struct RoomBed;

#[derive(Component)]
struct SirenVoice;

#[derive(Resource)]
struct Mix {
    step: Timer,
    running: bool,
    hurt_gap: f32,
    /// Seconds until the next street one-shot. Starts at 4, as in `ambience()`.
    next_street: f32,
    siren_t: f32,
    siren_on: bool,
    siren_kind: AlertKind,
}

impl Default for Mix {
    fn default() -> Self {
        Self {
            step: Timer::from_seconds(0.42, TimerMode::Repeating),
            running: false,
            hurt_gap: 0.0,
            next_street: 4.0,
            siren_t: 0.0,
            siren_on: false,
            siren_kind: AlertKind::Alert,
        }
    }
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SfxEvent>()
            .init_resource::<Ambience>()
            .init_resource::<Mix>()
            .add_systems(Startup, start_beds)
            .add_systems(
                Update,
                (
                    footsteps_while_moving,
                    story_cues,
                    play_events,
                    street_life,
                    drive_siren,
                    mix_beds,
                )
                    .chain()
                    .after(MoveSet),
            );
    }
}

fn load(server: &AssetServer, path: &str) -> Handle<AudioSource> {
    server.load(path.to_string())
}

fn start_beds(mut commands: Commands, asset_server: Res<AssetServer>) {
    let guns = [
        load(&asset_server, "audio/gun_pistol.ogg"),
        load(&asset_server, "audio/gun_rifle.ogg"),
        load(&asset_server, "audio/gun_sniper.ogg"),
        load(&asset_server, "audio/gun_mg.ogg"),
        load(&asset_server, "audio/gun_shotgun.ogg"),
        load(&asset_server, "audio/gun_at.ogg"),
    ];
    let steps = [
        load(&asset_server, "audio/step_tile.ogg"),
        load(&asset_server, "audio/step_asphalt.ogg"),
        load(&asset_server, "audio/step_wood.ogg"),
        load(&asset_server, "audio/step_metro.ogg"),
    ];
    let clips = Clips {
        guns,
        steps,
        hurt: load(&asset_server, "audio/hurt.ogg"),
        death: load(&asset_server, "audio/death.ogg"),
        swish: load(&asset_server, "audio/swish.ogg"),
        melee: load(&asset_server, "audio/melee.ogg"),
        radio: load(&asset_server, "audio/radio.ogg"),
        clink: load(&asset_server, "audio/clink.ogg"),
        rummage: load(&asset_server, "audio/rummage.ogg"),
        alert_test: load(&asset_server, "audio/alert_test.ogg"),
        siren: load(&asset_server, "audio/siren.ogg"),
        all_clear: load(&asset_server, "audio/all_clear.ogg"),
        whistle: load(&asset_server, "audio/whistle.ogg"),
        boom: load(&asset_server, "audio/boom.ogg"),
        explosion: load(&asset_server, "audio/amb_explosion.ogg"),
        dog: load(&asset_server, "audio/amb_dog.ogg"),
        scooter: load(&asset_server, "audio/amb_scooter.ogg"),
        chime: load(&asset_server, "audio/amb_chime.ogg"),
        radio_bed: load(&asset_server, "audio/radio_bed.ogg"),
    };
    commands.spawn((
        RainBed,
        AudioPlayer::new(load(&asset_server, RAIN)),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
    ));
    commands.spawn((
        RoomBed,
        AudioPlayer::new(load(&asset_server, ROOM)),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
    ));
    commands.insert_resource(clips);
}

fn attenuate(dist: f32, reference: f32, max_dist: f32) -> Option<f32> {
    if dist > max_dist {
        return None;
    }
    Some(1.0 / (1.0 + dist.max(0.0) / reference))
}

fn play(commands: &mut Commands, source: Handle<AudioSource>, volume: f32) {
    let volume = volume.clamp(0.0, 1.0);
    if volume < 0.001 {
        return;
    }
    commands.spawn((
        AudioPlayer::new(source),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)),
    ));
}

fn gun_index(kind: GunKind) -> usize {
    match kind {
        GunKind::Pistol => 0,
        GunKind::Rifle => 1,
        GunKind::Sniper => 2,
        GunKind::Mg => 3,
        GunKind::Shotgun => 4,
        GunKind::At => 5,
    }
}

fn step_index(surface: Surface) -> usize {
    match surface {
        Surface::Tile => 0,
        Surface::Asphalt => 1,
        Surface::Wood => 2,
        Surface::Metro => 3,
    }
}

fn in_session(flow: &GameFlow) -> bool {
    matches!(flow, GameFlow::Prologue | GameFlow::Playing)
}

/// Story alerts and 北廣 lines become the siren, the test chime, or the radio bed.
fn story_cues(
    mut alerts: MessageReader<StoryAlert>,
    mut lines: MessageReader<StoryBroadcast>,
    mut sfx: MessageWriter<SfxEvent>,
) {
    for alert in alerts.read() {
        let kind = match alert.kind {
            story::AlertKind::Test => AlertKind::Test,
            story::AlertKind::Alert => AlertKind::Alert,
            story::AlertKind::Clear => AlertKind::Clear,
        };
        sfx.write(SfxEvent::Alert { kind });
    }
    for _line in lines.read() {
        sfx.write(SfxEvent::Broadcast);
    }
}

fn footsteps_while_moving(
    time: Res<Time>,
    input: Res<MoveInput>,
    state: Res<GameState>,
    flow: Res<State<GameFlow>>,
    mut mix: ResMut<Mix>,
    mut events: MessageWriter<SfxEvent>,
) {
    let stick = input.combined().length();
    let moving = in_session(flow.get()) && !state.over && stick > 0.15;
    if !moving {
        mix.step.reset();
        return;
    }
    // A full push is a sprint, matching the joystick ("push all the way to sprint").
    let run = stick > 0.92;
    if run != mix.running {
        mix.running = run;
        mix.step = Timer::from_seconds(if run { 0.28 } else { 0.42 }, TimerMode::Repeating);
    }
    mix.step.tick(time.delta());
    if mix.step.just_finished() {
        events.write(SfxEvent::Footstep {
            surface: Surface::Asphalt,
            dist: 0.0,
            heavy: false,
            run,
        });
    }
}

fn play_events(
    mut commands: Commands,
    time: Res<Time>,
    clips: Res<Clips>,
    mut mix: ResMut<Mix>,
    mut events: MessageReader<SfxEvent>,
) {
    mix.hurt_gap = (mix.hurt_gap - time.delta_secs()).max(0.0);
    for event in events.read() {
        match *event {
            SfxEvent::Gunshot { kind, dist, indoor } => {
                let reference = if kind == GunKind::Pistol { 5.0 } else { 9.0 };
                let Some(gain) = attenuate(dist, reference, 260.0) else {
                    continue;
                };
                let loud = match kind {
                    GunKind::Pistol => 0.7,
                    GunKind::Rifle => 0.85,
                    GunKind::Sniper => 1.0,
                    GunKind::Mg => 0.8,
                    GunKind::Shotgun => 0.95,
                    GunKind::At => 1.0,
                };
                let indoor_scale = if indoor { 0.65 } else { 1.0 };
                play(
                    &mut commands,
                    clips.guns[gun_index(kind)].clone(),
                    loud * gain * indoor_scale,
                );
            }
            SfxEvent::Footstep {
                surface,
                dist,
                heavy,
                run,
            } => {
                let Some(gain) = attenuate(dist, 3.0, 22.0) else {
                    continue;
                };
                let mut volume = (if run { 0.55 } else { 0.4 }) * gain;
                if heavy {
                    volume *= 1.4;
                }
                play(
                    &mut commands,
                    clips.steps[step_index(surface)].clone(),
                    volume,
                );
            }
            SfxEvent::Hurt { dist } => {
                if mix.hurt_gap > 0.0 {
                    continue;
                }
                let Some(gain) = attenuate(dist, 4.0, 40.0) else {
                    continue;
                };
                mix.hurt_gap = 0.28;
                play(&mut commands, clips.hurt.clone(), 0.7 * gain);
            }
            SfxEvent::Death { dist } => {
                let Some(gain) = attenuate(dist, 5.0, 50.0) else {
                    continue;
                };
                play(&mut commands, clips.death.clone(), 0.8 * gain);
            }
            SfxEvent::Swish { dist, heavy } => {
                let Some(gain) = attenuate(dist, 4.0, 18.0) else {
                    continue;
                };
                play(
                    &mut commands,
                    clips.swish.clone(),
                    (if heavy { 0.6 } else { 0.45 }) * gain,
                );
            }
            SfxEvent::MeleeHit { dist, blunt: _ } => {
                let Some(gain) = attenuate(dist, 4.0, 16.0) else {
                    continue;
                };
                play(&mut commands, clips.melee.clone(), 0.7 * gain);
            }
            SfxEvent::Radio { dist } => {
                let Some(gain) = attenuate(dist, 6.0, 40.0) else {
                    continue;
                };
                play(&mut commands, clips.radio.clone(), 0.45 * gain);
            }
            SfxEvent::Clink { dist } => {
                let Some(gain) = attenuate(dist, 5.0, 30.0) else {
                    continue;
                };
                play(&mut commands, clips.clink.clone(), 0.5 * gain);
            }
            SfxEvent::Rummage => play(&mut commands, clips.rummage.clone(), 0.4),
            SfxEvent::Whistle { dist } => {
                let Some(gain) = attenuate(dist, 30.0, 200.0) else {
                    continue;
                };
                play(&mut commands, clips.whistle.clone(), 0.45 * gain);
            }
            SfxEvent::ShellBoom { dist, indoor } => {
                let Some(gain) = attenuate(dist, 25.0, 600.0) else {
                    continue;
                };
                play(
                    &mut commands,
                    clips.boom.clone(),
                    0.9 * gain * if indoor { 0.6 } else { 1.0 },
                );
            }
            SfxEvent::Alert { kind } => {
                mix.siren_kind = kind;
                mix.siren_t = 0.0;
                mix.siren_on = !matches!(kind, AlertKind::Test);
                if kind == AlertKind::Test {
                    play(&mut commands, clips.alert_test.clone(), 0.45);
                }
                if kind == AlertKind::Clear {
                    play(&mut commands, clips.all_clear.clone(), 0.5);
                }
            }
            SfxEvent::Broadcast => play(&mut commands, clips.radio_bed.clone(), 0.35),
        }
    }
}

/// Random far street cues from `ambience()`: blast, loudspeaker, dog, scooter.
fn street_life(
    mut commands: Commands,
    time: Res<Time>,
    clips: Res<Clips>,
    amb: Res<Ambience>,
    flow: Res<State<GameFlow>>,
    mut mix: ResMut<Mix>,
) {
    // Title and the ending screens stay quiet. The morning commute and the fight do not.
    if !in_session(flow.get()) || amb.metro {
        return;
    }
    mix.next_street -= time.delta_secs();
    if mix.next_street > 0.0 {
        return;
    }
    mix.next_street = 6.0 + (mix.siren_t.sin().abs() * 12.0);
    let far = if amb.indoor { 0.4 } else { 1.0 };
    let pick = (mix.next_street * 10.0) as i32 % 4;
    let (clip, volume) = match pick {
        0 => (clips.explosion.clone(), 0.45 * far),
        1 if !amb.night => (clips.chime.clone(), 0.3 * far),
        2 => (clips.dog.clone(), 0.35 * far),
        _ if !amb.night => (clips.scooter.clone(), 0.28 * far),
        _ => (clips.explosion.clone(), 0.3 * far),
    };
    play(&mut commands, clip, volume);
}

fn siren_window(t: f32) -> bool {
    // Three cycles of 15 on, 5 gap, 5 on, 5 gap, 5 on, 5 gap. From broadcast.ts.
    let cycle = 40.0;
    let local = t % cycle;
    if t >= cycle * 3.0 {
        return false;
    }
    (0.0..15.0).contains(&local) || (20.0..25.0).contains(&local) || (30.0..35.0).contains(&local)
}

fn drive_siren(
    mut commands: Commands,
    time: Res<Time>,
    clips: Res<Clips>,
    amb: Res<Ambience>,
    flow: Res<State<GameFlow>>,
    mut mix: ResMut<Mix>,
    mut voices: Query<(Entity, &mut AudioSink), With<SirenVoice>>,
) {
    // The 萬安 rotor belongs to the prologue raid and the fight, not the title or an ending.
    if !in_session(flow.get()) {
        mix.siren_on = false;
    }
    if !mix.siren_on || mix.siren_kind != AlertKind::Alert {
        for (entity, _) in voices.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }
    mix.siren_t += time.delta_secs();
    let want = siren_window(mix.siren_t);
    let indoor = if amb.indoor { 0.55 } else { 1.0 };
    if voices.is_empty() && want {
        commands.spawn((
            SirenVoice,
            AudioPlayer::new(clips.siren.clone()),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(0.32 * indoor)),
        ));
        return;
    }
    if !want {
        for (entity, _) in voices.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }
    for (_, mut sink) in &mut voices {
        sink.set_volume(Volume::Linear(0.32 * indoor));
    }
    if mix.siren_t > 40.0 * 3.0 + 4.0 {
        mix.siren_on = false;
    }
}

fn mix_beds(
    amb: Res<Ambience>,
    mut rain: Query<&mut AudioSink, (With<RainBed>, Without<RoomBed>)>,
    mut room: Query<&mut AudioSink, (With<RoomBed>, Without<RainBed>)>,
) {
    let rain_v = if amb.metro {
        0.0
    } else {
        amb.rain * if amb.indoor { 0.12 } else { 0.35 }
    };
    for mut sink in &mut rain {
        sink.set_volume(Volume::Linear(rain_v));
    }
    let room_v = if amb.home { 0.05 } else { 0.0 };
    for mut sink in &mut room {
        sink.set_volume(Volume::Linear(room_v));
    }
}
