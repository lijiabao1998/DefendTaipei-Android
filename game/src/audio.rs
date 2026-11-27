//! Offline-rendered Web audio with recorded CC0 shots, lifecycle-safe buses,
//! displacement-driven footsteps and Taipei environmental schedules.

//! The asset manifest records source hashes and decoded sample measurements.
//! Diagnostics count dispatches, never claim that a speaker produced sound.

pub(crate) mod engine;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use bevy::audio::Volume;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::{Enemy, GameState};
use crate::game_flow::{Campaign, GameFlow};
use crate::player::{MoveSet, Player};
use crate::story::{self, StoryAlert, StoryBroadcast};

pub const RAIN: &str = "audio/rain.ogg";
pub const ROOM: &str = "audio/room.ogg";
pub const MAX_VOICES: usize = 24;
const MAX_PENDING: usize = 32;

#[derive(Resource, Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioSettings {
    pub master: f32,
    pub effects: f32,
    pub ambience: f32,
    pub radio: f32,
    pub muted: bool,
}
impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master: 0.8,
            effects: 1.0,
            ambience: 0.7,
            radio: 0.8,
            muted: false,
        }
    }
}
impl AudioSettings {
    /// Invalid persisted values get safe defaults; valid values saturate at 0..1.
    pub fn sanitize(mut self) -> Self {
        let defaults = Self::default();
        for (value, fallback) in [
            (&mut self.master, defaults.master),
            (&mut self.effects, defaults.effects),
            (&mut self.ambience, defaults.ambience),
            (&mut self.radio, defaults.radio),
        ] {
            *value = if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                fallback
            };
        }
        self
    }
    pub fn ambience_gain(&self) -> f32 {
        self.gain(AudioCategory::Ambience)
    }
    pub fn radio_gain(&self) -> f32 {
        self.gain(AudioCategory::Radio)
    }
    pub fn gain(&self, category: AudioCategory) -> f32 {
        let s = self.sanitize();
        if s.muted {
            return 0.0;
        }
        s.master
            * match category {
                AudioCategory::Effects => s.effects,
                AudioCategory::Ambience => s.ambience,
                AudioCategory::Radio => s.radio,
            }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum AudioCategory {
    Effects,
    Ambience,
    Radio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GunKind {
    Pistol,
    Rifle,
    Sniper,
    Mg,
    Shotgun,
    At,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Tile,
    Asphalt,
    Wood,
    Metro,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Test,
    Alert,
    Clear,
}

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
    Broadcast,
    Door {
        open: bool,
        dist: f32,
    },
    Barricade {
        remove: bool,
        dist: f32,
    },
    Pickup,
    Equip,
    Consume {
        drink: bool,
    },
    Heal,
    Cook,
    Water,
    Build,
    Ui {
        confirm: bool,
    },
    Denied,
    Reload,
    DryFire,
    GarbageTruck {
        dist: f32,
        indoor: bool,
    },
    Temple {
        dist: f32,
    },
    Firecrackers,
    Thunder {
        indoor: bool,
    },
    JetPass {
        dist: f32,
    },
    Rotor {
        dist: f32,
    },
    /// A harmless category audition allowed in Settings, including title/pause.
    Preview {
        category: AudioCategory,
    },
    /// These synthesized distant cues belong to ambience, not enemy simulation.
    Street {
        kind: StreetKind,
        indoor: bool,
    },
    Shelter {
        kind: ShelterKind,
    },
}
#[derive(Clone, Copy, Debug)]
pub enum StreetKind {
    Explosion,
    Loudspeaker,
    Dog,
    Scooter,
}
#[derive(Clone, Copy, Debug)]
pub enum ShelterKind {
    Cup,
    Clock,
    Chair,
    Pot,
}

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct Ambience {
    pub rain: f32,
    pub indoor: bool,
    pub home: bool,
    pub night: bool,
    pub metro: bool,
    #[allow(dead_code)] // Reserved Web AmbState field; no invented combat music.
    pub combat: bool,
}

/// The real-output QA fixture disables automatic atmosphere to isolate its
/// selected source. Normal games always default to the complete scheduler.
/// This does not bypass asset decoding, playback, mixing, beds or lifecycle.
#[derive(Resource, Clone, Copy, Debug)]
pub struct AudioScheduleControl {
    pub automatic: bool,
}

impl Default for AudioScheduleControl {
    fn default() -> Self {
        Self { automatic: true }
    }
}

/// Every path is relative to the Bevy asset root. Ready means source bytes
/// loaded; only an output capture can prove device playback and decoding.
#[derive(Clone, Debug, Deserialize)]
pub struct AudioClipSpec {
    pub path: String,
    pub duration: f32,
}
pub fn audio_manifest() -> &'static [AudioClipSpec] {
    static CLIPS: OnceLock<Vec<AudioClipSpec>> = OnceLock::new();
    CLIPS.get_or_init(|| {
        #[derive(Deserialize)]
        struct Manifest {
            clips: Vec<AudioClipSpec>,
        }
        serde_json::from_str::<Manifest>(include_str!("../../assets/audio/manifest.json"))
            .expect("validated committed audio manifest")
            .clips
    })
}

#[derive(Resource, Default, Debug, Serialize)]
pub struct AudioDiagnostics {
    pub requested: BTreeMap<String, u64>,
    pub started: BTreeMap<String, u64>,
    pub suppressed: u64,
    pub dropped: u64,
    pub recorded_started: u64,
    pub fallback_started: u64,
    pub active_voices: usize,
    pub active_sinks: usize,
    pub ready_clips: usize,
    pub total_clips: usize,
    pub pending: usize,
    pub foreground: bool,
    pub world_active: bool,
    pub mix_scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    World,
    Interface,
    Ending,
}

/// Public for the audio-output QA harness. A tagged entity is only a dispatch:
/// the accompanying AudioSink/captured PCM is the playback evidence.
#[derive(Component, Debug)]
pub struct AudioVoice {
    pub cue: String,
    pub category: AudioCategory,
    pub base_gain: f32,
    pub remaining: f32,
    #[allow(dead_code)] // Public trace fields consumed by optional output QA.
    pub is_preview: bool,
    #[allow(dead_code)]
    pub recorded: bool,
    scope: Scope,
    looping: bool,
}

#[derive(Component, Clone, Copy)]
enum Bed {
    Rain,
    IndoorRain,
    Room,
}

#[derive(Resource)]
struct Clips {
    entries: HashMap<String, (Handle<AudioSource>, f32)>,
}
impl Clips {
    fn get(&self, name: &str) -> Option<&(Handle<AudioSource>, f32)> {
        self.entries.get(name)
    }
    fn loaded(&self, name: &str, assets: &Assets<AudioSource>) -> bool {
        self.get(name).is_some_and(|(h, _)| assets.contains(h.id()))
    }
}

#[derive(Clone)]
struct VoicePlan {
    cue: String,
    clip: String,
    category: AudioCategory,
    gain: f32,
    speed: f32,
    delay: f32,
    scope: Scope,
    preview: bool,
    recorded: bool,
    fallback: bool,
}
impl VoicePlan {
    fn new(cue: &str, clip: &str, category: AudioCategory, gain: f32) -> Self {
        Self {
            cue: cue.into(),
            clip: clip.into(),
            category,
            gain,
            speed: 1.0,
            delay: 0.0,
            scope: Scope::World,
            preview: false,
            recorded: false,
            fallback: false,
        }
    }
}

#[derive(Default)]
struct StepTrack {
    position: Option<Vec2>,
    travelled: f32,
}
impl StepTrack {
    /// Actual movement, not joystick intent. A long jump is a restore/teleport.
    fn advance(&mut self, pos: Vec2, dt: f32, running: bool, enabled: bool) -> bool {
        let previous = self.position.replace(pos);
        if !enabled || !pos.is_finite() || !dt.is_finite() || dt <= 0.0 {
            self.travelled = 0.0;
            return false;
        }
        let Some(previous) = previous else {
            return false;
        };
        let distance = pos.distance(previous);
        if !distance.is_finite() || distance > 9.0 * dt.min(1.0) + 0.15 {
            self.travelled = 0.0;
            return false;
        }
        self.travelled += distance;
        let stride = if running { 1.7 } else { 1.25 };
        if self.travelled >= stride {
            self.travelled %= stride;
            true
        } else {
            false
        }
    }
}

#[derive(Resource)]
struct Runtime {
    foreground: bool,
    focused: bool,
    app_running: bool,
    world_active: bool,
    last_flow: Option<GameFlow>,
    rng: u32,
    player_step: StepTrack,
    enemy_steps: HashMap<u32, StepTrack>,
    next_street: f32,
    next_home: f32,
    next_truck: f32,
    next_temple: f32,
    next_thunder: f32,
    next_rotor: f32,
    temple_scan: f32,
    temple_distance: f32,
    siren_t: f32,
    siren_on: bool,
    hurt_gap: f32,
    ui_gap: f32,
    pending: Vec<VoicePlan>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            foreground: true,
            focused: true,
            app_running: true,
            world_active: false,
            last_flow: None,
            rng: 0x47ac_2031,
            player_step: StepTrack::default(),
            enemy_steps: HashMap::new(),
            next_street: 4.0,
            next_home: 3.0,
            next_truck: 0.0,
            next_temple: 4.0,
            next_thunder: 8.0,
            next_rotor: 0.0,
            temple_scan: 0.0,
            temple_distance: f32::INFINITY,
            siren_t: 0.0,
            siren_on: false,
            hurt_gap: 0.0,
            ui_gap: 0.0,
            pending: Vec::new(),
        }
    }
}
impl Runtime {
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / 16_777_216.0
    }
}

pub struct AudioPlugin;
impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SfxEvent>()
            .init_resource::<AudioSettings>()
            .init_resource::<AudioDiagnostics>()
            .init_resource::<Ambience>()
            .init_resource::<AudioScheduleControl>()
            .init_resource::<Runtime>()
            .add_systems(Startup, load_audio)
            .add_systems(
                Update,
                (
                    audio_lifecycle,
                    footsteps,
                    story_cues,
                    environment,
                    dispatch,
                    siren,
                    engine::update,
                    mix_and_cleanup,
                )
                    .chain()
                    .after(MoveSet)
                    .after(crate::worldplay::WorldSet)
                    .after(crate::ui::settings::SettingsSet),
            );
    }
}
fn load_audio(mut commands: Commands, server: Res<AssetServer>) {
    let mut entries = HashMap::new();
    for spec in audio_manifest() {
        let handle = server.load(spec.path.clone());
        entries.insert(
            spec.path.trim_start_matches("audio/").to_owned(),
            (handle, spec.duration),
        );
    }
    for (bed, name) in [
        (Bed::Rain, RAIN.trim_start_matches("audio/")),
        (Bed::IndoorRain, "rain_indoor.ogg"),
        (Bed::Room, ROOM.trim_start_matches("audio/")),
    ] {
        if let Some((handle, _)) = entries.get(name) {
            commands.spawn((
                bed,
                BedMix::default(),
                AudioPlayer::new(handle.clone()),
                PlaybackSettings::LOOP
                    .with_volume(Volume::Linear(0.0))
                    .paused(),
            ));
        }
    }
    commands.insert_resource(Clips { entries });
}

fn audio_lifecycle(
    flow: Res<State<GameFlow>>,
    game: Res<GameState>,
    pause: Option<Res<crate::ui::PauseOpen>>,
    mut focus: MessageReader<bevy::window::WindowFocused>,
    mut lifecycle: MessageReader<bevy::window::AppLifecycle>,
    mut rt: ResMut<Runtime>,
) {
    for e in focus.read() {
        rt.focused = e.focused;
    }
    for e in lifecycle.read() {
        match e {
            bevy::window::AppLifecycle::WillSuspend | bevy::window::AppLifecycle::Suspended => {
                rt.app_running = false
            }
            bevy::window::AppLifecycle::Running => rt.app_running = true,
            _ => {}
        }
    }
    rt.foreground = rt.focused && rt.app_running;
    let in_session = matches!(flow.get(), GameFlow::Playing | GameFlow::Prologue);
    rt.world_active = rt.foreground && in_session && game.hp > 0.0 && !pause.is_some_and(|p| p.0);
    if rt.last_flow.as_ref() != Some(flow.get()) {
        rt.pending.clear();
        rt.player_step = StepTrack::default();
        rt.enemy_steps.clear();
        if !in_session || matches!(flow.get(), GameFlow::Prologue) {
            rt.siren_on = false;
            rt.siren_t = 0.0;
            rt.next_street = 4.0;
            rt.next_home = 3.0;
            rt.next_truck = 0.0;
            rt.next_temple = 4.0;
            rt.next_thunder = 8.0;
            rt.next_rotor = 0.0;
        }
        rt.last_flow = Some(flow.get().clone());
    }
    if !rt.world_active {
        rt.pending.clear();
    }
}

pub fn surface_at(position: Vec2, metro: bool) -> Surface {
    if metro {
        return Surface::Metro;
    }
    let world = crate::map::world();
    if let Some(building) = world.bld_at(position) {
        if building.kind == "home" {
            Surface::Wood
        } else {
            Surface::Tile
        }
    } else if world.road_dist(position) <= 0.0 {
        Surface::Asphalt
    } else {
        Surface::Tile
    }
}
fn footsteps(
    time: Res<Time>,
    flow: Res<State<GameFlow>>,
    amb: Res<Ambience>,
    player: Query<&Transform, With<Player>>,
    enemies: Query<&Enemy>,
    stance: Option<Res<crate::input::PlayerInput>>,
    inventory: Option<Res<crate::inventory::Inventory>>,
    mut rt: ResMut<Runtime>,
    mut events: MessageWriter<SfxEvent>,
    vehicles: Option<Res<crate::vehicles::VehicleFleet>>,
) {
    let Ok(player) = player.single() else { return };
    let pos = player.translation.xz();
    let dt = time.delta_secs();
    let active = rt.world_active && flow.get() == &GameFlow::Playing;
    let velocity = rt
        .player_step
        .position
        .map_or(0.0, |p| p.distance(pos) / dt.max(0.001));
    let run = velocity > 4.2;
    if rt.player_step.advance(
        pos,
        dt,
        run,
        active && vehicles.is_none_or(|v| v.current.is_none()),
    ) {
        let crouched = stance.as_ref().is_some_and(|s| s.crouch);
        // Crouching still makes quiet footsteps; reduce this event's gain in dispatch.
        let heavy = inventory.as_ref().is_some_and(|i| i.weight() > 12.0);
        events.write(SfxEvent::Footstep {
            surface: surface_at(pos, amb.metro),
            dist: if crouched { 3.0 } else { 0.0 },
            heavy,
            run,
        });
    }
    let mut nearby = HashSet::new();
    for enemy in &enemies {
        let s = &enemy.0;
        let dist = pos.distance(s.pos);
        if !active || !s.alive() || !s.active(amb.night) || dist > 22.0 {
            continue;
        }
        nearby.insert(s.id);
        let track = rt.enemy_steps.entry(s.id).or_default();
        if track.advance(s.pos, dt, s.moved > 4.0, active) {
            events.write(SfxEvent::Footstep {
                surface: surface_at(s.pos, amb.metro),
                dist,
                heavy: true,
                run: s.moved > 4.0,
            });
        }
    }
    rt.enemy_steps.retain(|id, _| nearby.contains(id));
}

fn story_cues(
    mut alerts: MessageReader<StoryAlert>,
    mut lines: MessageReader<StoryBroadcast>,
    mut events: MessageWriter<SfxEvent>,
) {
    for alert in alerts.read() {
        events.write(SfxEvent::Alert {
            kind: match alert.kind {
                story::AlertKind::Test => AlertKind::Test,
                story::AlertKind::Alert => AlertKind::Alert,
                story::AlertKind::Clear => AlertKind::Clear,
            },
        });
    }
    for _ in lines.read() {
        events.write(SfxEvent::Broadcast);
    }
}

/// The Web's evening garbage-truck window, ending at the saved power cutoff.
pub fn truck_distance(game_min: f32) -> Option<f32> {
    if !game_min.is_finite() || game_min < 0.0 || game_min >= story::POWER_OFF as f32 {
        return None;
    }
    let minute = game_min.rem_euclid(1440.0);
    if !(1140.0..1152.0).contains(&minute) {
        return None;
    }
    Some(20.0 + ((minute - 1140.0) / 12.0 - 0.5).abs() * 360.0)
}
/// Native shop typing collapses many OSM temples to homes. Preserve named
/// temple geometry and the already playable Longshan courtyard landmark.
pub fn temple_distance(position: Vec2) -> f32 {
    if !position.is_finite() {
        return f32::INFINITY;
    }
    let landmark = Vec2::from_array(crate::survival::TRADE_SPOTS[0].pos);
    crate::map::world()
        .blds
        .iter()
        .filter(|b| b.kind == "temple" || b.name.contains('寺') || b.name.contains('宮'))
        .map(|b| b.c.distance(position))
        .fold(landmark.distance(position), f32::min)
}
fn environment(
    time: Res<Time>,
    control: Res<AudioScheduleControl>,
    campaign: Option<Res<Campaign>>,
    evac: Option<Res<crate::map::EvacSites>>,
    amb: Res<Ambience>,
    player: Query<&Transform, With<Player>>,
    mut rt: ResMut<Runtime>,
    mut events: MessageWriter<SfxEvent>,
) {
    if !control.automatic || !rt.world_active || amb.metro {
        return;
    }
    let dt = time.delta_secs().clamp(0.0, 1.0);
    rt.next_street -= dt;
    if rt.next_street <= 0.0 {
        rt.next_street = 6.0 + rt.random() * 12.0;
        let pick = rt.random();
        let kind = if pick < 0.22 {
            StreetKind::Explosion
        } else if pick < 0.4 && !amb.night {
            StreetKind::Loudspeaker
        } else if pick < 0.6 {
            StreetKind::Dog
        } else if pick < 0.8 && !amb.night {
            StreetKind::Scooter
        } else {
            StreetKind::Explosion
        };
        events.write(SfxEvent::Street {
            kind,
            indoor: amb.indoor,
        });
    }
    if amb.home {
        rt.next_home -= dt;
        if rt.next_home <= 0.0 {
            rt.next_home = 4.0 + rt.random() * 7.0;
            let pick = rt.random();
            events.write(SfxEvent::Shelter {
                kind: if pick < 0.3 {
                    ShelterKind::Cup
                } else if pick < 0.55 {
                    ShelterKind::Clock
                } else if pick < 0.8 {
                    ShelterKind::Chair
                } else {
                    ShelterKind::Pot
                },
            });
        }
    }
    if let Some(campaign) = campaign {
        if let Some(dist) = truck_distance(campaign.game_min) {
            rt.next_truck -= dt;
            if rt.next_truck <= 0.0 {
                rt.next_truck = 6.8;
                events.write(SfxEvent::GarbageTruck {
                    dist,
                    indoor: amb.indoor,
                });
            }
        } else {
            rt.next_truck = 0.0;
        }
    }
    rt.temple_scan -= dt;
    if rt.temple_scan <= 0.0 {
        rt.temple_scan = 2.0;
        rt.temple_distance = player
            .single()
            .ok()
            .map_or(f32::INFINITY, |p| temple_distance(p.translation.xz()));
    }
    if rt.temple_distance < 70.0 {
        rt.next_temple -= dt;
        if rt.next_temple <= 0.0 {
            rt.next_temple = 18.0 + rt.random() * 14.0;
            events.write(SfxEvent::Temple {
                dist: rt.temple_distance,
            });
        }
    }
    if let Some(evac) = evac
        && evac.heli_open
        && let Ok(player) = player.single()
        && player.translation.xz().distance(evac.heli) <= 300.0
    {
        rt.next_rotor -= dt;
        if rt.next_rotor <= 0.0 {
            rt.next_rotor = 1.25;
            events.write(SfxEvent::Rotor {
                dist: player.translation.xz().distance(evac.heli),
            });
        }
    } else {
        rt.next_rotor = 0.0;
    }
    if amb.rain > 0.6 {
        rt.next_thunder -= dt;
        if rt.next_thunder <= 0.0 {
            rt.next_thunder = 20.0 + rt.random() * 25.0;
            events.write(SfxEvent::Thunder { indoor: amb.indoor });
        }
    }
}

fn attenuation(dist: f32, reference: f32, cutoff: f32) -> Option<f32> {
    if !dist.is_finite() || dist > cutoff {
        return None;
    }
    Some(1.0 / (1.0 + dist.max(0.0) / reference))
}
fn gun_name(kind: GunKind) -> &'static str {
    match kind {
        GunKind::Pistol => "pistol",
        GunKind::Rifle => "rifle",
        GunKind::Sniper => "sniper",
        GunKind::Mg => "mg",
        GunKind::Shotgun => "shotgun",
        GunKind::At => "at",
    }
}
fn step_name(surface: Surface) -> &'static str {
    match surface {
        Surface::Tile => "tile",
        Surface::Asphalt => "asphalt",
        Surface::Wood => "wood",
        Surface::Metro => "metro",
    }
}
fn resolve(
    event: SfxEvent,
    clips: &Clips,
    assets: &Assets<AudioSource>,
    rt: &mut Runtime,
) -> Option<VoicePlan> {
    use AudioCategory::{Ambience as A, Effects as E, Radio as R};
    let mut p = match event {
        SfxEvent::Gunshot { kind, dist, indoor } => {
            let name = gun_name(kind);
            let reference = if kind == GunKind::Sniper { 22.0 } else { 14.0 };
            let gain = attenuation(dist, reference, 260.0)?;
            let far = dist > 70.0 && kind != GunKind::Pistol && kind != GunKind::At;
            let base = if far { "far" } else { name };
            let count = if far {
                1
            } else {
                match kind {
                    GunKind::Rifle => 3,
                    GunKind::At => 0,
                    _ => 2,
                }
            };
            let variant = if indoor {
                "room"
            } else if dist > 25.0 {
                "far"
            } else {
                "dry"
            };
            let selected = (rt.random() * count.max(1) as f32) as usize;
            let recording = (0..count)
                .map(|i| format!("rec_{base}{}_{variant}.ogg", 1 + (selected + i) % count))
                .find(|path| clips.loaded(path, assets));
            let mut p = VoicePlan::new(
                &format!("gun_{name}"),
                &recording
                    .clone()
                    .unwrap_or_else(|| format!("gun_{name}.ogg")),
                E,
                gain * match kind {
                    GunKind::Pistol => 0.52,
                    GunKind::Rifle => 0.62,
                    GunKind::Sniper => 0.8,
                    GunKind::Mg => 0.58,
                    GunKind::Shotgun => 0.72,
                    GunKind::At => 0.85,
                },
            );
            p.recorded = recording.is_some();
            p.fallback = count > 0 && recording.is_none();
            p.speed =
                (0.93 + rt.random() * 0.13) * if kind == GunKind::Sniper { 0.94 } else { 1.0 };
            p
        }
        SfxEvent::ShellBoom { dist, indoor } => {
            let gain = attenuation(dist, 25.0, 600.0)?;
            let variant = if indoor {
                "room"
            } else if dist > 25.0 {
                "far"
            } else {
                "dry"
            };
            let chosen = (rt.random() * 3.0) as usize;
            let recording = (0..3)
                .map(|i| format!("rec_boom{}_{variant}.ogg", 1 + (chosen + i) % 3))
                .find(|p| clips.loaded(p, assets));
            let mut p = VoicePlan::new(
                "shell_boom",
                &recording.clone().unwrap_or_else(|| "boom.ogg".into()),
                E,
                gain * 0.85,
            );
            p.recorded = recording.is_some();
            p.fallback = recording.is_none();
            p.delay = (dist.max(0.0) / 343.0).min(1.2);
            p.speed = 0.88 + rt.random() * 0.17;
            p
        }
        SfxEvent::Footstep {
            surface,
            dist,
            heavy,
            run,
        } => {
            let gain = attenuation(dist, 3.0, 22.0)?;
            let name = step_name(surface);
            let mut p = VoicePlan::new(
                &format!("step_{name}{}", if heavy { "_heavy" } else { "" }),
                &format!("step_{name}.ogg"),
                E,
                gain * (if run { 0.25 } else { 0.16 }) * if heavy { 1.4 } else { 1.0 },
            );
            p.speed = 0.93 + rt.random() * 0.15;
            p
        }
        SfxEvent::Hurt { dist } => {
            if rt.hurt_gap > 0.0 {
                return None;
            }
            let gain = attenuation(dist, 4.0, 40.0)?;
            rt.hurt_gap = 0.28;
            VoicePlan::new("hurt", "hurt.ogg", E, gain * 0.5)
        }
        SfxEvent::Death { dist } => {
            let mut p = VoicePlan::new(
                "death",
                "death.ogg",
                E,
                attenuation(dist, 5.0, 50.0)? * 0.65,
            );
            p.scope = Scope::Ending;
            p
        }
        SfxEvent::Swish { dist, heavy } => VoicePlan::new(
            "swish",
            if heavy {
                "swish_heavy.ogg"
            } else {
                "swish.ogg"
            },
            E,
            attenuation(dist, 4.0, 18.0)? * if heavy { 0.45 } else { 0.3 },
        ),
        SfxEvent::MeleeHit { dist, blunt } => VoicePlan::new(
            "melee_hit",
            if blunt {
                "melee_blunt.ogg"
            } else {
                "melee.ogg"
            },
            E,
            attenuation(dist, 4.0, 16.0)? * 0.55,
        ),
        SfxEvent::Radio { dist } => {
            VoicePlan::new("radio", "radio.ogg", R, attenuation(dist, 6.0, 40.0)? * 0.4)
        }
        SfxEvent::Clink { dist } => VoicePlan::new(
            "clink",
            "clink.ogg",
            E,
            attenuation(dist, 5.0, 30.0)? * 0.35,
        ),
        SfxEvent::Rummage => VoicePlan::new("rummage", "rummage.ogg", E, 0.32),
        SfxEvent::Whistle { dist } => VoicePlan::new(
            "shell_whistle",
            "whistle.ogg",
            E,
            attenuation(dist, 30.0, 200.0)? * 0.4,
        ),
        SfxEvent::Alert { kind } => {
            rt.siren_on = kind == AlertKind::Alert;
            rt.siren_t = 0.0;
            match kind {
                AlertKind::Test => VoicePlan::new("alert_test", "alert_test.ogg", R, 0.32),
                AlertKind::Clear => VoicePlan::new("all_clear", "all_clear.ogg", R, 0.4),
                AlertKind::Alert => return None,
            }
        }
        SfxEvent::Broadcast => VoicePlan::new("broadcast", "radio_bed.ogg", R, 0.15),
        SfxEvent::Door { open, dist } => VoicePlan::new(
            if open { "door_open" } else { "door_close" },
            if open {
                "door_open.ogg"
            } else {
                "door_close.ogg"
            },
            E,
            attenuation(dist, 4.0, 24.0)? * 0.4,
        ),
        SfxEvent::Barricade { remove, dist } => VoicePlan::new(
            if remove { "unboard" } else { "barricade" },
            if remove {
                "unboard.ogg"
            } else {
                "barricade.ogg"
            },
            E,
            attenuation(dist, 5.0, 30.0)? * 0.48,
        ),
        SfxEvent::Pickup => VoicePlan::new("pickup", "pickup.ogg", E, 0.3),
        SfxEvent::Equip => VoicePlan::new("equip", "equip.ogg", E, 0.32),
        SfxEvent::Consume { drink } => VoicePlan::new(
            if drink { "drink" } else { "eat" },
            if drink { "drink.ogg" } else { "eat.ogg" },
            E,
            0.3,
        ),
        SfxEvent::Heal => VoicePlan::new("heal", "heal.ogg", E, 0.3),
        SfxEvent::Cook => VoicePlan::new("cook", "cook.ogg", E, 0.3),
        SfxEvent::Water => VoicePlan::new("water", "water.ogg", E, 0.32),
        SfxEvent::Build => VoicePlan::new("build", "build.ogg", E, 0.4),
        SfxEvent::Reload => VoicePlan::new("reload", "reload.ogg", E, 0.35),
        SfxEvent::DryFire => VoicePlan::new("dry_fire", "dry_fire.ogg", E, 0.25),
        SfxEvent::Ui { confirm } => {
            if rt.ui_gap > 0.0 {
                return None;
            }
            rt.ui_gap = 0.06;
            let mut p = VoicePlan::new(
                if confirm { "ui_confirm" } else { "ui_open" },
                if confirm {
                    "ui_confirm.ogg"
                } else {
                    "ui_open.ogg"
                },
                E,
                0.22,
            );
            p.scope = Scope::Interface;
            p
        }
        SfxEvent::Denied => {
            if rt.ui_gap > 0.0 {
                return None;
            }
            rt.ui_gap = 0.12;
            let mut p = VoicePlan::new("denied", "denied.ogg", E, 0.22);
            p.scope = Scope::Interface;
            p
        }
        SfxEvent::GarbageTruck { dist, indoor } => {
            if !dist.is_finite() || dist > 220.0 {
                return None;
            }
            VoicePlan::new(
                "garbage_truck",
                "garbage_truck.ogg",
                A,
                (0.02_f32.max(0.16 * (1.0 - dist.max(0.0) / 220.0)))
                    * if indoor { 0.45 } else { 1.0 },
            )
        }
        SfxEvent::Temple { dist } => {
            if !dist.is_finite() || dist >= 70.0 {
                return None;
            }
            VoicePlan::new(
                "temple",
                "temple.ogg",
                A,
                0.18 * (1.0 - dist.max(0.0) / 70.0),
            )
        }
        SfxEvent::Firecrackers => VoicePlan::new("firecrackers", "firecrackers.ogg", A, 0.4),
        SfxEvent::Thunder { indoor } => VoicePlan::new(
            "thunder",
            "thunder.ogg",
            A,
            if indoor { 0.25 } else { 0.45 },
        ),
        SfxEvent::JetPass { dist } => {
            if !dist.is_finite() || dist > 400.0 {
                return None;
            }
            VoicePlan::new(
                "jet",
                "jet.ogg",
                A,
                0.5 * (1.0 - dist.max(0.0) / 400.0).max(0.15),
            )
        }
        SfxEvent::Rotor { dist } => {
            if !dist.is_finite() || dist > 300.0 {
                return None;
            }
            VoicePlan::new(
                "rotor",
                "rotor.ogg",
                A,
                0.35 * (1.0 - dist.max(0.0) / 300.0).max(0.1),
            )
        }
        SfxEvent::Preview { category } => {
            let (cue, clip) = match category {
                E => ("preview_effects", "ui_confirm.ogg"),
                A => ("preview_ambience", "shelter_cup.ogg"),
                R => ("preview_radio", "radio.ogg"),
            };
            let mut p = VoicePlan::new(cue, clip, category, 0.35);
            p.scope = Scope::Interface;
            p.preview = true;
            p
        }
        SfxEvent::Street { kind, indoor } => {
            let (cue, clip, gain) = match kind {
                StreetKind::Explosion => ("amb_explosion", "amb_explosion.ogg", 0.35),
                StreetKind::Loudspeaker => ("amb_loudspeaker", "amb_chime.ogg", 0.18),
                StreetKind::Dog => ("amb_dog", "amb_dog.ogg", 0.2),
                StreetKind::Scooter => ("amb_scooter", "amb_scooter.ogg", 0.16),
            };
            VoicePlan::new(cue, clip, A, gain * if indoor { 0.4 } else { 1.0 })
        }
        SfxEvent::Shelter { kind } => {
            let (cue, clip) = match kind {
                ShelterKind::Cup => ("shelter_cup", "shelter_cup.ogg"),
                ShelterKind::Clock => ("shelter_clock", "shelter_clock.ogg"),
                ShelterKind::Chair => ("shelter_chair", "shelter_chair.ogg"),
                ShelterKind::Pot => ("shelter_pot", "shelter_pot.ogg"),
            };
            VoicePlan::new(cue, clip, A, 0.12)
        }
    };
    if !p.speed.is_finite() || !p.gain.is_finite() {
        return None;
    }
    p.gain = p.gain.clamp(0.0, 1.0);
    Some(p)
}

fn requested_name(event: SfxEvent) -> &'static str {
    match event {
        SfxEvent::Gunshot { .. } => "gunshot",
        SfxEvent::Footstep { .. } => "footstep",
        SfxEvent::Hurt { .. } => "hurt",
        SfxEvent::Death { .. } => "death",
        SfxEvent::Swish { .. } => "swish",
        SfxEvent::MeleeHit { .. } => "melee_hit",
        SfxEvent::Radio { .. } => "radio",
        SfxEvent::Clink { .. } => "clink",
        SfxEvent::Rummage => "rummage",
        SfxEvent::Whistle { .. } => "shell_whistle",
        SfxEvent::ShellBoom { .. } => "shell_boom",
        SfxEvent::Alert { .. } => "alert",
        SfxEvent::Broadcast => "broadcast",
        SfxEvent::Door { .. } => "door",
        SfxEvent::Barricade { .. } => "barricade",
        SfxEvent::Pickup => "pickup",
        SfxEvent::Equip => "equip",
        SfxEvent::Consume { .. } => "consume",
        SfxEvent::Heal => "heal",
        SfxEvent::Cook => "cook",
        SfxEvent::Water => "water",
        SfxEvent::Build => "build",
        SfxEvent::Ui { .. } => "ui",
        SfxEvent::Denied => "denied",
        SfxEvent::Reload => "reload",
        SfxEvent::DryFire => "dry_fire",
        SfxEvent::GarbageTruck { .. } => "garbage_truck",
        SfxEvent::Temple { .. } => "temple",
        SfxEvent::Firecrackers => "firecrackers",
        SfxEvent::Thunder { .. } => "thunder",
        SfxEvent::JetPass { .. } => "jet",
        SfxEvent::Rotor { .. } => "rotor",
        SfxEvent::Preview { .. } => "preview",
        SfxEvent::Street { .. } => "street",
        SfxEvent::Shelter { .. } => "shelter",
    }
}
fn event_scope(event: SfxEvent) -> Scope {
    match event {
        SfxEvent::Ui { .. } | SfxEvent::Denied | SfxEvent::Preview { .. } => Scope::Interface,
        SfxEvent::Death { .. } => Scope::Ending,
        _ => Scope::World,
    }
}
fn allowed(scope: Scope, foreground: bool, world_active: bool, flow: &GameFlow) -> bool {
    foreground
        && match scope {
            Scope::World => world_active,
            Scope::Interface => true,
            Scope::Ending => !matches!(flow, GameFlow::Title),
        }
}

fn dispatch(
    mut commands: Commands,
    time: Res<Time>,
    clips: Res<Clips>,
    assets: Res<Assets<AudioSource>>,
    settings: Res<AudioSettings>,
    flow: Res<State<GameFlow>>,
    voices: Query<&AudioVoice>,
    mut rt: ResMut<Runtime>,
    mut diag: ResMut<AudioDiagnostics>,
    mut events: MessageReader<SfxEvent>,
) {
    let dt = time.delta_secs().clamp(0.0, 1.0);
    rt.hurt_gap = (rt.hurt_gap - dt).max(0.0);
    rt.ui_gap = (rt.ui_gap - dt).max(0.0);
    for &event in events.read() {
        *diag
            .requested
            .entry(requested_name(event).into())
            .or_default() += 1;
        if !allowed(
            event_scope(event),
            rt.foreground,
            rt.world_active,
            flow.get(),
        ) {
            diag.suppressed += 1;
            continue;
        }
        let Some(plan) = resolve(event, &clips, &assets, &mut rt) else {
            continue;
        };
        if settings.gain(plan.category) <= 0.0 {
            diag.suppressed += 1;
            continue;
        }
        if rt.pending.len() >= MAX_PENDING {
            diag.dropped += 1;
            continue;
        }
        rt.pending.push(plan);
    }
    let mut pending = std::mem::take(&mut rt.pending);
    let mut live = voices.iter().count();
    for mut plan in pending.drain(..) {
        if !allowed(plan.scope, rt.foreground, rt.world_active, flow.get())
            || settings.gain(plan.category) <= 0.0
        {
            diag.suppressed += 1;
            continue;
        }
        plan.delay -= dt;
        if plan.delay > 0.0 {
            rt.pending.push(plan);
            continue;
        }
        if live >= MAX_VOICES || !clips.loaded(&plan.clip, &assets) {
            diag.dropped += 1;
            continue;
        }
        let Some((handle, duration)) = clips.get(&plan.clip) else {
            diag.dropped += 1;
            continue;
        };
        // Start silent; mix_and_cleanup applies the final gain before Bevy queues
        // playback in PostUpdate. This avoids a one-frame mute/background leak.
        commands.spawn((
            AudioVoice {
                cue: plan.cue.clone(),
                category: plan.category,
                base_gain: plan.gain,
                remaining: duration / plan.speed + 0.5,
                is_preview: plan.preview,
                recorded: plan.recorded,
                scope: plan.scope,
                looping: false,
            },
            AudioPlayer::new(handle.clone()),
            PlaybackSettings::DESPAWN
                .with_speed(plan.speed)
                .with_volume(Volume::Linear(0.0)),
        ));
        live += 1;
        *diag.started.entry(plan.cue).or_default() += 1;
        diag.recorded_started += u64::from(plan.recorded);
        diag.fallback_started += u64::from(plan.fallback);
    }
}

/// Three 40-second cycles: 15 seconds on, 5 off, 5 on, 5 off, 5 on, 5 off.
pub fn siren_window(t: f32) -> bool {
    if !t.is_finite() || !(0.0..120.0).contains(&t) {
        return false;
    }
    let local = t % 40.0;
    (0.0..15.0).contains(&local) || (20.0..25.0).contains(&local) || (30.0..35.0).contains(&local)
}
fn siren(
    mut commands: Commands,
    time: Res<Time>,
    clips: Res<Clips>,
    assets: Res<Assets<AudioSource>>,
    settings: Res<AudioSettings>,
    amb: Res<Ambience>,
    mut rt: ResMut<Runtime>,
    voices: Query<(Entity, &AudioVoice)>,
    mut diag: ResMut<AudioDiagnostics>,
) {
    if rt.world_active && rt.siren_on {
        rt.siren_t += time.delta_secs().clamp(0.0, 1.0);
        if rt.siren_t >= 120.0 {
            rt.siren_on = false;
        }
    }
    let want =
        rt.world_active && rt.siren_on && siren_window(rt.siren_t) && settings.radio_gain() > 0.0;
    let existing: Vec<Entity> = voices
        .iter()
        .filter_map(|(e, v)| (v.cue == "siren").then_some(e))
        .collect();
    if !want {
        for entity in existing {
            commands.entity(entity).despawn();
        }
    } else if existing.is_empty()
        && voices.iter().count() < MAX_VOICES
        && clips.loaded("siren.ogg", &assets)
    {
        let (handle, _) = clips.get("siren.ogg").expect("manifest includes siren");
        commands.spawn((
            AudioVoice {
                cue: "siren".into(),
                category: AudioCategory::Radio,
                base_gain: 0.3 * if amb.indoor { 0.55 } else { 1.0 },
                remaining: f32::INFINITY,
                is_preview: false,
                recorded: false,
                scope: Scope::World,
                looping: true,
            },
            AudioPlayer::new(handle.clone()),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
        ));
        *diag.started.entry("siren".into()).or_default() += 1;
    }
}

#[derive(Component, Default)]
struct BedMix(f32);

/// Outdoor rain and filtered indoor rain are crossfaded, never both at full
/// level. The domestic electrical hum stops at the real saved power cutoff.
pub fn bed_levels(amb: &Ambience, active: bool, power: bool) -> [f32; 3] {
    if !active {
        return [0.0; 3];
    }
    let rain = if amb.rain.is_finite() {
        amb.rain.clamp(0.0, 1.0)
    } else {
        0.0
    };
    [
        if !amb.metro && !amb.indoor {
            rain * 0.35
        } else {
            0.0
        },
        if !amb.metro && amb.indoor {
            rain * 0.12
        } else {
            0.0
        },
        if amb.home && power { 0.05 } else { 0.0 },
    ]
}
fn mix_and_cleanup(
    mut commands: Commands,
    time: Res<Time>,
    flow: Res<State<GameFlow>>,
    amb: Res<Ambience>,
    campaign: Option<Res<Campaign>>,
    settings: Res<AudioSettings>,
    clips: Res<Clips>,
    assets: Res<Assets<AudioSource>>,
    rt: Res<Runtime>,
    mut diag: ResMut<AudioDiagnostics>,
    mut voices: Query<
        (
            Entity,
            &mut AudioVoice,
            &mut PlaybackSettings,
            Option<&mut AudioSink>,
        ),
        Without<Bed>,
    >,
    mut beds: Query<
        (
            &Bed,
            &mut BedMix,
            &mut PlaybackSettings,
            Option<&mut AudioSink>,
        ),
        Without<AudioVoice>,
    >,
) {
    let dt = time.delta_secs().clamp(0.0, 1.0);
    let power = campaign.is_none_or(|c| c.game_min < story::POWER_OFF as f32);
    let targets = bed_levels(&amb, rt.world_active, power);
    let mut total = 0.0;
    let mut active = 0;
    for (entity, mut voice, _, sink) in &mut voices {
        if !voice.looping {
            voice.remaining -= dt;
        }
        let valid = allowed(voice.scope, rt.foreground, rt.world_active, flow.get())
            && settings.gain(voice.category) > 0.0
            && voice.remaining > 0.0;
        if !valid {
            if let Some(sink) = sink {
                sink.stop();
            }
            commands.entity(entity).despawn();
            continue;
        }
        if voice.cue == "siren" {
            voice.base_gain = 0.3 * if amb.indoor { 0.55 } else { 1.0 };
        }
        total += voice.base_gain * settings.gain(voice.category);
        active += 1;
    }
    for (bed, mut mix, _, _) in &mut beds {
        let target = targets[match bed {
            Bed::Rain => 0,
            Bed::IndoorRain => 1,
            Bed::Room => 2,
        }];
        if !rt.world_active || settings.ambience_gain() <= 0.0 {
            mix.0 = 0.0;
        } else {
            mix.0 += (target - mix.0) * (1.0 - (-dt / 0.35).exp());
        }
        total += mix.0 * settings.ambience_gain();
    }
    // A deterministic aggregate gain budget leaves headroom for every active
    // stream. It replaces Web's compressor, without claiming equivalent DSP.
    let scale = if total > 0.85 { 0.85 / total } else { 1.0 };
    let mut sinks = 0;
    for (_, voice, mut playback, sink) in &mut voices {
        let valid = allowed(voice.scope, rt.foreground, rt.world_active, flow.get())
            && voice.remaining > 0.0;
        let gain = if valid {
            voice.base_gain * settings.gain(voice.category) * scale
        } else {
            0.0
        };
        playback.volume = Volume::Linear(gain);
        playback.paused = gain <= 0.0;
        if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(gain));
            if gain > 0.0 {
                sink.play();
                sinks += 1;
            } else {
                sink.pause();
            }
        }
    }
    for (_, mix, mut playback, sink) in &mut beds {
        let gain = mix.0 * settings.ambience_gain() * scale;
        playback.volume = Volume::Linear(gain);
        playback.paused = gain <= 0.00001;
        if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(gain));
            if gain > 0.00001 {
                sink.play();
                sinks += 1;
            } else {
                sink.pause();
            }
        }
    }
    diag.active_voices = active;
    diag.active_sinks = sinks;
    diag.ready_clips = clips
        .entries
        .values()
        .filter(|(h, _)| assets.contains(h.id()))
        .count();
    diag.total_clips = clips.entries.len();
    diag.pending = rt.pending.len();
    diag.foreground = rt.foreground;
    diag.world_active = rt.world_active;
    diag.mix_scale = scale;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_settings_are_finite_and_each_bus_is_independent() {
        let s = AudioSettings {
            master: f32::NAN,
            effects: -1.0,
            ambience: 2.0,
            radio: f32::INFINITY,
            muted: false,
        }
        .sanitize();
        assert_eq!(
            s,
            AudioSettings {
                master: 0.8,
                effects: 0.0,
                ambience: 1.0,
                radio: 0.8,
                muted: false
            }
        );
        assert_eq!(s.gain(AudioCategory::Effects), 0.0);
        assert_eq!(s.ambience_gain(), 0.8);
        assert!((s.radio_gain() - 0.64).abs() < 1e-6);
        let silent = AudioSettings { muted: true, ..s };
        assert_eq!(
            [
                silent.gain(AudioCategory::Effects),
                silent.ambience_gain(),
                silent.radio_gain()
            ],
            [0.0; 3]
        );
        let fallback: AudioSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(fallback, AudioSettings::default());
    }
    #[test]
    fn blocked_motion_restores_and_teleports_cannot_make_steps() {
        let mut step = StepTrack::default();
        for _ in 0..50 {
            assert!(!step.advance(Vec2::ZERO, 1.0 / 60.0, false, true));
        }
        assert!(!step.advance(Vec2::new(100.0, 0.0), 1.0 / 60.0, false, true));
        for _ in 0..30 {
            assert!(!step.advance(Vec2::new(100.0, 0.0), 1.0 / 60.0, false, true));
        }
        let mut steps = 0;
        for i in 1..=30 {
            steps += usize::from(step.advance(
                Vec2::new(100.0 + i as f32 * 0.05, 0.0),
                1.0 / 60.0,
                false,
                true,
            ));
        }
        assert_eq!(steps, 1);
        assert!(!step.advance(Vec2::new(102.0, 0.0), 1.0 / 60.0, false, false));
        assert!(!step.advance(Vec2::new(102.05, 0.0), 1.0 / 60.0, false, true));
    }
    #[test]
    fn siren_uses_all_three_cycles_and_handles_invalid_time() {
        for cycle in 0..3 {
            let base = cycle as f32 * 40.0;
            assert!(siren_window(base));
            assert!(siren_window(base + 14.99));
            assert!(!siren_window(base + 15.0));
            assert!(siren_window(base + 20.0));
            assert!(!siren_window(base + 25.0));
            assert!(siren_window(base + 30.0));
            assert!(!siren_window(base + 35.0));
        }
        for t in [-1.0, 120.0, 200.0, f32::NAN, f32::INFINITY] {
            assert!(!siren_window(t));
        }
    }
    #[test]
    fn world_lifecycle_stops_game_audio_but_keeps_settings_preview() {
        assert!(allowed(Scope::World, true, true, &GameFlow::Playing));
        assert!(!allowed(Scope::World, true, false, &GameFlow::Playing));
        assert!(allowed(Scope::Interface, true, false, &GameFlow::Title));
        assert!(!allowed(Scope::Interface, false, false, &GameFlow::Title));
        assert!(!allowed(Scope::Ending, true, false, &GameFlow::Title));
    }
    #[test]
    fn evening_window_uses_game_clock_and_actual_blackout() {
        assert_eq!(truck_distance(1139.99), None);
        assert_eq!(truck_distance(1140.0), Some(200.0));
        assert_eq!(truck_distance(1146.0), Some(20.0));
        assert_eq!(truck_distance(1152.0), None);
        assert_eq!(truck_distance(story::POWER_OFF as f32), None);
        assert_eq!(truck_distance(f32::NAN), None);
        assert_eq!(truck_distance(1440.0 + 1146.0), Some(20.0));
    }
    #[test]
    fn indoor_rain_is_filtered_bus_and_home_hum_obeys_power() {
        let amb = Ambience {
            rain: 1.0,
            home: true,
            ..default()
        };
        assert_eq!(bed_levels(&amb, true, true), [0.35, 0.0, 0.05]);
        let indoor = Ambience {
            indoor: true,
            ..amb
        };
        assert_eq!(bed_levels(&indoor, true, true), [0.0, 0.12, 0.05]);
        assert_eq!(bed_levels(&indoor, true, false), [0.0, 0.12, 0.0]);
        assert_eq!(bed_levels(&indoor, false, true), [0.0; 3]);
        assert_eq!(
            bed_levels(
                &Ambience {
                    metro: true,
                    ..indoor
                },
                true,
                true
            ),
            [0.0, 0.0, 0.05]
        );
    }
    #[test]
    fn attenuation_rejects_nonfinite_and_cutoff_sources() {
        assert_eq!(attenuation(f32::NAN, 3.0, 22.0), None);
        assert_eq!(attenuation(23.0, 3.0, 22.0), None);
        assert_eq!(attenuation(-3.0, 3.0, 22.0), Some(1.0));
        assert_eq!(attenuation(3.0, 3.0, 22.0), Some(0.5));
    }
    #[test]
    fn committed_manifest_has_unique_finite_playable_clips() {
        let mut names = HashSet::new();
        for clip in audio_manifest() {
            assert!(clip.path.starts_with("audio/") && clip.path.ends_with(".ogg"));
            assert!(clip.duration.is_finite() && clip.duration > 0.0 && clip.duration < 12.0);
            assert!(names.insert(&clip.path));
        }
        assert!(names.contains(&"audio/rec_pistol1_dry.ogg".to_string()));
        assert!(names.contains(&"audio/rec_boom3_room.ogg".to_string()));
        assert!(names.contains(&"audio/garbage_truck.ogg".to_string()));
    }

    fn fixture_clips(names: &[&str]) -> (Clips, Assets<AudioSource>) {
        let mut assets = Assets::<AudioSource>::default();
        let mut entries = HashMap::new();
        for name in names {
            let handle = assets.add(AudioSource {
                bytes: vec![0_u8; 8].into(),
            });
            entries.insert((*name).to_string(), (handle, 1.0));
        }
        (Clips { entries }, assets)
    }
    #[test]
    fn recording_choice_uses_real_ready_assets_and_falls_back_when_absent() {
        let (clips, assets) = fixture_clips(&["rec_pistol1_room.ogg", "rec_far1_far.ogg"]);
        let mut rt = Runtime::default();
        let indoor = resolve(
            SfxEvent::Gunshot {
                kind: GunKind::Pistol,
                dist: 0.0,
                indoor: true,
            },
            &clips,
            &assets,
            &mut rt,
        )
        .unwrap();
        assert_eq!(indoor.clip, "rec_pistol1_room.ogg");
        assert!(indoor.recorded && !indoor.fallback);
        let distant = resolve(
            SfxEvent::Gunshot {
                kind: GunKind::Rifle,
                dist: 90.0,
                indoor: false,
            },
            &clips,
            &assets,
            &mut rt,
        )
        .unwrap();
        assert_eq!(distant.clip, "rec_far1_far.ogg");
        let missing = resolve(
            SfxEvent::Gunshot {
                kind: GunKind::Rifle,
                dist: 0.0,
                indoor: false,
            },
            &clips,
            &assets,
            &mut rt,
        )
        .unwrap();
        assert_eq!(missing.clip, "gun_rifle.ogg");
        assert!(!missing.recorded && missing.fallback);
        assert!(
            resolve(
                SfxEvent::Gunshot {
                    kind: GunKind::Rifle,
                    dist: 261.0,
                    indoor: false
                },
                &clips,
                &assets,
                &mut rt
            )
            .is_none()
        );
    }
    #[test]
    fn delayed_blast_queue_and_voice_budget_remain_bounded_without_a_device() {
        let (clips, assets) = fixture_clips(&["boom.ogg"]);
        let mut app = App::new();
        app.init_resource::<Time>()
            .insert_resource(State::new(GameFlow::Playing))
            .insert_resource(clips)
            .insert_resource(assets)
            .init_resource::<AudioSettings>()
            .init_resource::<AudioDiagnostics>()
            .insert_resource(Runtime {
                world_active: true,
                ..default()
            })
            .add_message::<SfxEvent>()
            .add_systems(Update, dispatch);
        for _ in 0..100 {
            app.world_mut()
                .resource_mut::<Messages<SfxEvent>>()
                .write(SfxEvent::ShellBoom {
                    dist: 600.0,
                    indoor: false,
                });
        }
        app.update();
        assert_eq!(app.world().resource::<Runtime>().pending.len(), MAX_PENDING);
        assert_eq!(app.world().resource::<AudioDiagnostics>().dropped, 68);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(1));
        app.update();
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&AudioVoice>()
                .iter(app.world())
                .count(),
            MAX_VOICES
        );
        assert!(app.world().resource::<Runtime>().pending.is_empty());
        assert_eq!(app.world().resource::<AudioDiagnostics>().dropped, 76);
    }
    #[test]
    fn a_siren_without_a_sink_does_not_spawn_once_per_frame() {
        let (clips, assets) = fixture_clips(&["siren.ogg"]);
        let mut app = App::new();
        app.init_resource::<Time>()
            .insert_resource(clips)
            .insert_resource(assets)
            .init_resource::<AudioSettings>()
            .init_resource::<AudioDiagnostics>()
            .init_resource::<Ambience>()
            .insert_resource(Runtime {
                world_active: true,
                siren_on: true,
                ..default()
            })
            .add_systems(Update, siren);
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(
            app.world_mut()
                .query::<&AudioVoice>()
                .iter(app.world())
                .count(),
            1
        );
        app.world_mut().resource_mut::<AudioSettings>().radio = 0.0;
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&AudioVoice>()
                .iter(app.world())
                .count(),
            0
        );
    }
    #[test]
    fn sinkless_one_shots_expire_and_background_clears_interface_voices() {
        let (clips, assets) = fixture_clips(&[]);
        let mut app = App::new();
        app.init_resource::<Time>()
            .insert_resource(clips)
            .insert_resource(assets)
            .insert_resource(State::new(GameFlow::Playing))
            .init_resource::<AudioSettings>()
            .init_resource::<AudioDiagnostics>()
            .init_resource::<Ambience>()
            .insert_resource(Runtime {
                world_active: true,
                ..default()
            })
            .add_systems(Update, mix_and_cleanup);
        let voice = |scope, remaining| AudioVoice {
            cue: "test".into(),
            category: AudioCategory::Effects,
            base_gain: 0.3,
            remaining,
            is_preview: false,
            recorded: false,
            scope,
            looping: false,
        };
        app.world_mut()
            .spawn((voice(Scope::World, 0.1), PlaybackSettings::DESPAWN));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(1));
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&AudioVoice>()
                .iter(app.world())
                .count(),
            0
        );
        app.world_mut()
            .spawn((voice(Scope::Interface, 5.0), PlaybackSettings::DESPAWN));
        app.world_mut().resource_mut::<Runtime>().foreground = false;
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&AudioVoice>()
                .iter(app.world())
                .count(),
            0
        );
    }

    #[test]
    fn lifecycle_running_does_not_override_a_lost_window_focus() {
        let mut app = App::new();
        app.insert_resource(State::new(GameFlow::Playing))
            .init_resource::<GameState>()
            .init_resource::<Runtime>()
            .add_message::<bevy::window::WindowFocused>()
            .add_message::<bevy::window::AppLifecycle>()
            .add_systems(Update, audio_lifecycle);
        app.world_mut()
            .resource_mut::<Messages<bevy::window::WindowFocused>>()
            .write(bevy::window::WindowFocused {
                window: Entity::PLACEHOLDER,
                focused: false,
            });
        app.world_mut()
            .resource_mut::<Messages<bevy::window::AppLifecycle>>()
            .write(bevy::window::AppLifecycle::Running);
        app.update();
        assert!(!app.world().resource::<Runtime>().foreground);
        app.world_mut()
            .resource_mut::<Messages<bevy::window::WindowFocused>>()
            .write(bevy::window::WindowFocused {
                window: Entity::PLACEHOLDER,
                focused: true,
            });
        app.update();
        assert!(app.world().resource::<Runtime>().foreground);
        app.world_mut()
            .resource_mut::<Messages<bevy::window::AppLifecycle>>()
            .write(bevy::window::AppLifecycle::Suspended);
        app.update();
        assert!(!app.world().resource::<Runtime>().foreground);
    }
    #[test]
    fn longshan_landmark_stays_audible_despite_native_shop_typing() {
        let landmark = Vec2::from_array(crate::survival::TRADE_SPOTS[0].pos);
        assert_eq!(temple_distance(landmark), 0.0);
        assert!(temple_distance(landmark + Vec2::X * 40.0) <= 40.0);
        assert!(temple_distance(Vec2::splat(f32::NAN)).is_infinite());
    }
}
