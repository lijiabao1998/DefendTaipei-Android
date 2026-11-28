//! Versioned native saves, shared by desktop and Android.
//!
//! A save is validated before touching the live world. Writes go to a sibling
//! temporary file, are flushed, and replace the previous slot atomically. Android
//! uses GameActivity's private internal data directory; no storage permission is
//! needed. The web game's 20-real-second autosave and deletion on death/new game
//! are preserved. `ContinueGame` bypasses the new-game/prologue reset path.

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use bevy::{
    app::AppExit,
    prelude::*,
    window::{AppLifecycle, WindowCloseRequested, WindowFocused},
};
use serde::{Deserialize, Serialize};

use crate::combat::{Body, Enemy, Fx, GameState};
use crate::game_flow::{Campaign, Ending, GameFlow, RestartGame};
use crate::i18n::{Language, tr};
use crate::inventory::Inventory;
use crate::player::{MoveInput, Player};
use crate::roles::{Gender, ROLES, SelectedRole};
use crate::story::{PRO_NEWS, WANHUA_DISTRICTS};
use crate::survival::{DawnTracker, Shelter, ShelterEnvironment};
use crate::worldmap::MapProgress;
use crate::worldplay::{PlayerNeeds, ScavengeState};

const SAVE_VERSION: u32 = 1;
const SAVE_NAME: &str = "defendtaipei-native-v1.json";
const MAX_SAVE_BYTES: usize = 4 * 1024 * 1024;
const AUTOSAVE_SECONDS: f32 = 20.0;
const MAX_WORLD_ENTRIES: usize = 100_000;

#[derive(Message, Clone, Copy, Debug)]
pub struct SaveGame;

#[derive(Message, Clone, Copy, Debug)]
pub struct ContinueGame;

/// Save first, then return to the title. A failed write keeps the current run.
#[derive(Message, Clone, Copy, Debug)]
pub struct SaveAndMenu;

/// Resources and the player transform have been restored. World providers use
/// this to rebuild patrols and container visuals from the saved world deltas.
#[derive(Message, Clone, Copy, Debug)]
pub struct LoadCompleted;

#[derive(Resource, Default, Debug)]
pub struct SaveStatus {
    /// True only if the slot was read and passed schema/value validation.
    pub available: bool,
    pub error: Option<String>,
    pub notice: String,
}

impl SaveStatus {
    pub fn message(&self) -> &str {
        self.error.as_deref().unwrap_or(&self.notice)
    }
}

#[derive(Resource, Default)]
struct SaveLocation(Option<PathBuf>);

#[derive(Resource, Default)]
struct SaveRequests {
    elapsed: f32,
    save: bool,
    load: bool,
    menu: bool,
    manual: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveFile {
    version: u32,
    campaign: Campaign,
    player: PlayerSnapshot,
    role_id: String,
    female: bool,
    inventory: Inventory,
    shelter: Shelter,
    environment: ShelterEnvironment,
    needs: PlayerNeeds,
    world: ScavengeState,
    #[serde(default)]
    map: MapProgress,
    #[serde(default)]
    vehicles: crate::vehicles::VehicleFleet,
    #[serde(default)]
    armor: crate::combat::armor::ArmorFleet,
    /// v24.3 the ROC remnant positions (absent in older native saves).
    #[serde(default)]
    remnants: crate::remnant::RemnantState,
    /// v24.4 the JSDF liaison, radio cooldowns and airdrops.
    #[serde(default)]
    japan: crate::japan::JapanState,
    /// v24.8 the day's call for help from upstairs.
    #[serde(default)]
    rescue: crate::rescue::RescueState,
    /// v22.5 cleared sniper posts.
    #[serde(default)]
    snipers: crate::sniper::SniperState,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerSnapshot {
    hp: f32,
    bleed: bool,
    stun: f32,
    kills: u32,
    survived: f32,
    position: [f32; 3],
    rotation: [f32; 4],
}

#[derive(Debug)]
enum SaveError {
    Io(io::Error),
    Json(serde_json::Error),
    Invalid(&'static str),
    Version(u32),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "Invalid or incomplete save JSON: {err}"),
            Self::Invalid(reason) => write!(f, "{reason}"),
            Self::Version(version) => write!(f, "Unsupported save version: {version}"),
        }
    }
}

impl From<io::Error> for SaveError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for SaveError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl SaveFile {
    fn validate(&self) -> Result<(), SaveError> {
        if self.version != SAVE_VERSION {
            return Err(SaveError::Version(self.version));
        }
        let capture_ids: Vec<_> = self
            .armor
            .units
            .iter()
            .filter_map(|unit| unit.captured_id)
            .collect();
        if capture_ids.iter().collect::<HashSet<_>>().len() != capture_ids.len()
            || !self.vehicles.validate()
            || !self.armor.validate()
            || self.armor.units.iter().any(|unit| {
                unit.captured_id
                    .is_some_and(|id| self.vehicles.get(id).is_none_or(|v| v.kind != unit.kind))
            })
        {
            return Err(SaveError::Invalid("Invalid vehicle or armour state"));
        }
        let player = &self.player;
        if !finite_range(player.hp, f32::MIN_POSITIVE, 100.0)
            || !finite_range(player.stun, 0.0, 3600.0)
            || !finite_range(player.survived, 0.0, 1_000_000_000.0)
            || player
                .position
                .iter()
                .any(|n| !n.is_finite() || n.abs() > 1_000_000.0)
            || player.rotation.iter().any(|n| !n.is_finite())
            || !(0.99..=1.01).contains(&player.rotation.iter().map(|n| n * n).sum::<f32>())
        {
            return Err(SaveError::Invalid("Invalid player state"));
        }
        if !ROLES.iter().any(|role| role.id == self.role_id) {
            return Err(SaveError::Invalid("Unknown survivor role"));
        }
        let map = crate::map::world();
        let position = Vec2::new(player.position[0], player.position[2]);
        if !position.cmpge(map.min - 0.1).all()
            || !position.cmple(map.max + 0.1).all()
            || !finite_range(player.position[1], -1.0, 1000.0)
        {
            return Err(SaveError::Invalid("Player is outside the playable map"));
        }
        let campaign = &self.campaign;
        if !finite_range(campaign.game_min, 0.0, 1_000_000_000.0)
            || !campaign.clock_remainder.is_finite()
            || campaign.clock_remainder.abs() > 64.0
            || !(0.0..=1_000_000_000.0)
                .contains(&(f64::from(campaign.game_min) + campaign.clock_remainder))
            || !matches!(campaign.day_hours, 1 | 2 | 4)
            || !matches!(campaign.speed, 1 | 3 | 20)
            || !finite_range(campaign.prologue_t, 0.0, 1_000_000.0)
            || campaign.news_i > PRO_NEWS.len()
            || !finite_range(campaign.notice_t, 0.0, 3600.0)
            || campaign.notice.len() > 16_384
            || campaign.reclaimed.len() > WANHUA_DISTRICTS.len()
            || campaign.reclaim_at.len() > WANHUA_DISTRICTS.len()
            || campaign.reclaim_at.iter().any(|(name, p)| {
                !WANHUA_DISTRICTS.contains(&name.as_str())
                    || p.iter().any(|n| !n.is_finite() || n.abs() > 1_000_000.0)
            })
            || !unique_strings(&campaign.reclaimed, 128)
            || campaign
                .reclaimed
                .iter()
                .any(|name| !WANHUA_DISTRICTS.contains(&name.as_str()))
            || campaign.fired.len() > 1000
            || !unique_strings(&campaign.fired, 128)
        {
            return Err(SaveError::Invalid("Invalid campaign progress"));
        }
        if !self.inventory.is_valid() || !self.shelter.is_valid() || !self.environment.is_valid() {
            return Err(SaveError::Invalid("Invalid inventory or shelter state"));
        }
        if !self.map.validate() {
            return Err(SaveError::Invalid("Invalid map exploration or notes"));
        }
        if !self.remnants.is_valid() {
            return Err(SaveError::Invalid("Invalid ROC remnant state"));
        }
        if !self.japan.is_valid() {
            return Err(SaveError::Invalid("Invalid JSDF support state"));
        }
        if !self.rescue.is_valid() {
            return Err(SaveError::Invalid("Invalid rescue state"));
        }
        if !self.snipers.is_valid() {
            return Err(SaveError::Invalid("Invalid sniper state"));
        }
        if [self.needs.hunger, self.needs.thirst, self.needs.fatigue]
            .iter()
            .any(|value| !finite_range(*value, 0.0, 100.0))
        {
            return Err(SaveError::Invalid("Invalid survival needs"));
        }
        let world = &self.world;
        let aging = &world.container_aging;
        if !aging.elapsed_hours.is_finite()
            || aging.elapsed_hours < 0.0
            || aging.elapsed_hours > f64::from(campaign.game_min) / 60.0 + 0.001
            || aging
                .minute
                .is_some_and(|minute| !finite_range(minute, 0.0, campaign.game_min))
            || (aging.minute.is_none() && aging.elapsed_hours != 0.0)
        {
            return Err(SaveError::Invalid("Invalid container aging progress"));
        }
        if world.fixtures_version > 1 {
            return Err(SaveError::Invalid("Unsupported indoor fixtures version"));
        }
        let closed_doors: HashSet<_> = world.closed_doors.iter().copied().collect();
        if world.closed_doors.len() > map.blds.len()
            || closed_doors.len() != world.closed_doors.len()
            || world.closed_doors.iter().any(|id| {
                map.blds
                    .get(*id as usize)
                    .is_none_or(|b| b.id != *id || b.door.is_none())
            })
        {
            return Err(SaveError::Invalid("Invalid saved doors"));
        }
        if world.barricaded_doors.len() > map.blds.len()
            || world.barricaded_doors.iter().collect::<HashSet<_>>().len()
                != world.barricaded_doors.len()
            || world
                .barricaded_doors
                .iter()
                .any(|id| !closed_doors.contains(id))
        {
            return Err(SaveError::Invalid("Invalid saved barricades"));
        }
        let loot_ids: HashSet<&str> = world.loot.iter().map(|(id, _)| id.as_str()).collect();
        if [&world.searched, &world.asked, &world.night_seen]
            .iter()
            .any(|ids| ids.len() > MAX_WORLD_ENTRIES || !unique_strings(ids, 256))
            || world.loot.len() > MAX_WORLD_ENTRIES
            || world
                .loot
                .iter()
                .any(|(id, items)| id.is_empty() || id.len() > 256 || !items.is_valid())
            || loot_ids.len() != world.loot.len()
            || world.dead.len() > MAX_WORLD_ENTRIES
            || world.dead.iter().collect::<HashSet<_>>().len() != world.dead.len()
            || world.drops.len() > MAX_WORLD_ENTRIES
            || world
                .drops
                .iter()
                .map(|(id, _)| id)
                .collect::<HashSet<_>>()
                .len()
                != world.drops.len()
            || world.drops.iter().any(|(id, point)| {
                id.is_empty()
                    || id.len() > 256
                    || !point.iter().all(|n| n.is_finite())
                    || !Vec2::from_array(*point).cmpge(map.min - 256.0).all()
                    || !Vec2::from_array(*point).cmple(map.max + 256.0).all()
                    || !loot_ids.contains(id.as_str())
            })
        {
            return Err(SaveError::Invalid("Invalid world progress"));
        }
        if world.taps.len() > 1
            || world
                .taps
                .iter()
                .any(|(id, next)| id != "tap_home" || !finite_range(*next, 0.0, 1_000_000_000.0))
        {
            return Err(SaveError::Invalid("Invalid water fixture cooldown"));
        }
        let weather = &world.weather;
        if !finite_range(weather.next, 0.0, 1_000_000_000.0)
            || !finite_range(weather.until, 0.0, 1_000_000_000.0)
            || [weather.amount, weather.intensity, weather.wet]
                .iter()
                .any(|value| !finite_range(*value, 0.0, 1.0))
        {
            return Err(SaveError::Invalid("Invalid saved weather"));
        }
        Ok(())
    }
}

fn finite_range(value: f32, min: f32, max: f32) -> bool {
    value.is_finite() && (min..=max).contains(&value)
}

fn unique_strings(values: &[String], max_length: usize) -> bool {
    values
        .iter()
        .all(|value| !value.is_empty() && value.len() <= max_length)
        && values.iter().collect::<HashSet<_>>().len() == values.len()
}

fn decode(bytes: &[u8]) -> Result<SaveFile, SaveError> {
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(SaveError::Invalid("Save exceeds size limit"));
    }
    // Read the version before the schema, so a future format is clearly reported
    // and is never silently interpreted as a new game or overwritten on load.
    #[derive(Deserialize)]
    struct Header {
        version: u32,
    }
    let header: Header = serde_json::from_slice(bytes)?;
    if header.version != SAVE_VERSION {
        return Err(SaveError::Version(header.version));
    }
    let save: SaveFile = serde_json::from_slice(bytes)?;
    save.validate()?;
    Ok(save)
}

fn read_slot(path: &Path) -> Result<SaveFile, SaveError> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_SAVE_BYTES as u64 {
        return Err(SaveError::Invalid("Save exceeds size limit"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_SAVE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    decode(&bytes)
}

fn write_slot(path: &Path, save: &SaveFile) -> Result<(), SaveError> {
    save.validate()?;
    let bytes = serde_json::to_vec(save)?;
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(SaveError::Invalid("Save exceeds size limit"));
    }
    let parent = path
        .parent()
        .ok_or(SaveError::Invalid("Missing save directory"))?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension("tmp");
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        // Flush the directory entry as well on Unix (including Android).
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(SaveError::Io)
}

fn remove_slot(path: &Path) -> Result<(), SaveError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(target_os = "android")]
fn save_path() -> Result<PathBuf, SaveError> {
    // ANDROID_APP is populated by #[bevy_main] before the Bevy App is built.
    bevy::android::ANDROID_APP
        .get()
        .and_then(|app| app.internal_data_path())
        .map(|directory| directory.join(SAVE_NAME))
        .ok_or(SaveError::Invalid(
            "Android internal data directory is unavailable",
        ))
}

#[cfg(not(target_os = "android"))]
fn save_path() -> Result<PathBuf, SaveError> {
    if let Some(path) = std::env::var_os("DEFEND_TAIPEI_SAVE_DIR").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path).join(SAVE_NAME));
    }
    // The scripted QA run starts a new game and clears its slot. Keep that
    // disposable run separate even when invoked without a save-dir override.
    if std::env::args().any(|arg| arg == "--qa") {
        let output = std::env::var_os("DEFEND_TAIPEI_QA_OUT")
            .map_or_else(|| PathBuf::from("qa-artifacts"), PathBuf::from);
        return Ok(output.join("save").join(SAVE_NAME));
    }
    #[cfg(target_os = "windows")]
    let directory = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let directory = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let directory = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    directory
        .map(|directory| directory.join("DefendTaipei").join(SAVE_NAME))
        .ok_or(SaveError::Invalid("User data directory is unavailable"))
}

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SaveStatus>()
            .init_resource::<SaveLocation>()
            .init_resource::<SaveRequests>()
            .add_message::<SaveGame>()
            .add_message::<ContinueGame>()
            .add_message::<SaveAndMenu>()
            .add_message::<LoadCompleted>()
            .add_systems(Startup, discover_slot)
            .add_systems(OnEnter(GameFlow::Prologue), clear_slot)
            .add_systems(OnEnter(GameFlow::GameOver(Ending::Death)), clear_slot)
            // Capture after every gameplay system, including that frame's dawn,
            // inventory actions and damage. Load before the next state transition.
            .add_systems(Last, (collect_requests, service_requests).chain());
    }
}

fn discover_slot(mut location: ResMut<SaveLocation>, mut status: ResMut<SaveStatus>) {
    // A supplied location also lets headless integration tests use isolated
    // directories without changing process-wide environment variables.
    let path = match location.0.clone().map(Ok).unwrap_or_else(save_path) {
        Ok(path) => path,
        Err(err) => {
            status.error = Some(err.to_string());
            return;
        }
    };
    match read_slot(&path) {
        Ok(_) => status.available = true,
        Err(SaveError::Io(err)) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => status.error = Some(err.to_string()),
    }
    location.0 = Some(path);
}

fn clear_slot(
    location: Res<SaveLocation>,
    mut status: ResMut<SaveStatus>,
    mut requests: ResMut<SaveRequests>,
) {
    *requests = SaveRequests::default();
    status.available = false;
    status.notice.clear();
    status.error = location
        .0
        .as_deref()
        .and_then(|path| remove_slot(path).err())
        .map(|err| err.to_string());
}

fn collect_requests(
    time: Res<Time<Real>>,
    flow: Res<State<GameFlow>>,
    next: Res<NextState<GameFlow>>,
    mut saves: MessageReader<SaveGame>,
    mut loads: MessageReader<ContinueGame>,
    mut menus: MessageReader<SaveAndMenu>,
    mut lifecycle: MessageReader<AppLifecycle>,
    mut focus: MessageReader<WindowFocused>,
    mut closes: MessageReader<WindowCloseRequested>,
    mut exits: MessageReader<AppExit>,
    mut requests: ResMut<SaveRequests>,
) {
    let manual = saves.read().count() > 0;
    let load = loads.read().count() > 0;
    let menu = menus.read().count() > 0;
    let background = lifecycle
        .read()
        .any(|event| matches!(event, AppLifecycle::WillSuspend | AppLifecycle::Suspended));
    let focus_lost = focus.read().any(|event| !event.focused);
    let close_requested = closes.read().count() > 0;
    let exiting = exits.read().count() > 0;
    let successful_ending = match &*next {
        NextState::Pending(GameFlow::GameOver(ending))
        | NextState::PendingIfDifferent(GameFlow::GameOver(ending)) => *ending != Ending::Death,
        _ => false,
    };
    requests.load |= load && matches!(flow.get(), GameFlow::Title);
    if matches!(flow.get(), GameFlow::Playing) {
        requests.elapsed += time.delta_secs();
        requests.manual |= manual || menu;
        requests.menu |= menu;
        requests.save |= manual
            || menu
            || background
            || focus_lost
            || close_requested
            || exiting
            || successful_ending
            || requests.elapsed >= AUTOSAVE_SECONDS;
    } else {
        requests.elapsed = 0.0;
    }
}

fn snapshot(world: &mut World) -> Result<SaveFile, SaveError> {
    let transform = world
        .query_filtered::<&Transform, With<Player>>()
        .iter(world)
        .next()
        .ok_or(SaveError::Invalid("Player is not ready"))?;
    let state = world.resource::<GameState>();
    let player = PlayerSnapshot {
        hp: state.hp,
        bleed: state.bleed,
        stun: state.stun,
        kills: state.kills,
        survived: state.survived,
        position: transform.translation.to_array(),
        rotation: transform.rotation.to_array(),
    };
    // v23.4 a save made upstairs records the ground-floor landing
    let position = world
        .get_resource::<crate::upstairs::Upstairs>()
        .and_then(|u| u.landing())
        .map_or(player.position, |p| [p.x, 0.0, p.y]);
    let player = PlayerSnapshot { position, ..player };
    let selected = world.resource::<SelectedRole>();
    let save = SaveFile {
        version: SAVE_VERSION,
        campaign: world.resource::<Campaign>().clone(),
        player,
        role_id: selected.id.to_owned(),
        female: selected.gender == Gender::Female,
        inventory: world.resource::<Inventory>().clone(),
        shelter: world.resource::<Shelter>().clone(),
        environment: *world.resource::<ShelterEnvironment>(),
        needs: world.resource::<PlayerNeeds>().clone(),
        world: world.resource::<ScavengeState>().clone(),
        map: world
            .get_resource::<MapProgress>()
            .cloned()
            .unwrap_or_default(),
        vehicles: world
            .get_resource::<crate::vehicles::VehicleFleet>()
            .cloned()
            .unwrap_or_default(),
        armor: world
            .get_resource::<crate::combat::armor::ArmorFleet>()
            .cloned()
            .unwrap_or_default(),
        remnants: world
            .get_resource::<crate::remnant::RemnantState>()
            .cloned()
            .unwrap_or_default(),
        japan: world
            .get_resource::<crate::japan::JapanState>()
            .cloned()
            .unwrap_or_default(),
        rescue: world
            .get_resource::<crate::rescue::RescueState>()
            .cloned()
            .unwrap_or_default(),
        snipers: world
            .get_resource::<crate::sniper::SniperState>()
            .cloned()
            .unwrap_or_default(),
    };
    save.validate()?;
    Ok(save)
}

fn restore(world: &mut World, mut save: SaveFile) -> Result<(), SaveError> {
    save.validate()?;
    let player_entity = world
        .query_filtered::<Entity, With<Player>>()
        .iter(world)
        .next()
        .ok_or(SaveError::Invalid("Player is not ready"))?;
    let mut transform = Transform::from_translation(Vec3::from_array(save.player.position))
        .with_rotation(Quat::from_array(save.player.rotation).normalize());
    let restored_floor =
        crate::map::interiors::restore_position(crate::map::world(), transform.translation.xz());
    transform.translation.x = restored_floor.x;
    transform.translation.z = restored_floor.y;
    world.entity_mut(player_entity).insert(transform);
    let transients: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<Enemy>, With<Body>, With<Fx>)>>()
        .iter(world)
        .collect();
    for entity in transients {
        world.despawn(entity);
    }
    crate::combat::clear_transient_combat(world);
    let role = ROLES
        .iter()
        .find(|role| role.id == save.role_id)
        .expect("validated role");
    world.insert_resource(SelectedRole {
        id: role.id,
        gender: if save.female {
            Gender::Female
        } else {
            Gender::Male
        },
    });
    world.insert_resource(GameState {
        hp: save.player.hp,
        bleed: save.player.bleed,
        stun: save.player.stun,
        kills: save.player.kills,
        survived: save.player.survived,
        over: false,
    });
    world.insert_resource(DawnTracker::from_minutes(save.campaign.game_min));
    // The web save persists day length, but never resumes a sleeping animation
    // or accelerated controls after reopening the game.
    save.campaign.speed = 1;
    save.campaign.sleeping = false;
    world.insert_resource(save.campaign);
    world.insert_resource(save.inventory);
    world.insert_resource(save.shelter);
    world.insert_resource(save.environment);
    world.insert_resource(save.needs);
    let recovered_planks = if let Some(obstacles) = world.get_resource::<crate::map::Obstacles>() {
        obstacles
            .doors()
            .replace_state(&save.world.closed_doors, &save.world.barricaded_doors);
        let recovered = crate::doors::protect_position(obstacles, transform.translation.xz());
        save.world.closed_doors = obstacles.doors().closed_ids();
        save.world.barricaded_doors = obstacles.doors().barricaded_ids();
        recovered
    } else {
        0
    };
    if recovered_planks > 0 {
        world
            .resource_mut::<Inventory>()
            .add("plank", recovered_planks);
    }
    world.insert_resource(save.world);
    world.insert_resource(save.map);
    // Resume with the physical vehicle intact and its throttle released. A
    // persisted driving seat is retained; a background input never accelerates it.
    if let Some(vehicle) = save.vehicles.current_mut() {
        vehicle.speed = 0.0;
        let position = vehicle.position();
        if let Some(mut player) = world.get_mut::<Transform>(player_entity) {
            player.translation = Vec3::new(position.x, 0.0, position.y);
            player.rotation = Quat::from_rotation_y(vehicle.heading + std::f32::consts::PI);
        }
    }
    world.insert_resource(save.vehicles);
    world.insert_resource(save.armor);
    world.insert_resource(save.remnants);
    world.insert_resource(save.japan);
    world.insert_resource(save.rescue);
    world.insert_resource(save.snipers);
    if let Some(mut input) = world.get_resource_mut::<MoveInput>() {
        *input = MoveInput::default();
    }
    if let Some(mut input) = world.get_resource_mut::<crate::input::PlayerInput>() {
        *input = crate::input::PlayerInput::default();
    }
    if let Some(mut stance) = world.get_resource_mut::<crate::combat::PlayerStance>() {
        *stance = crate::combat::PlayerStance::default();
    }
    world
        .resource_mut::<NextState<GameFlow>>()
        .set(GameFlow::Playing);
    world.write_message(LoadCompleted);
    Ok(())
}

pub(crate) fn service_requests(world: &mut World) {
    let (load, save, menu, manual) = {
        let mut requests = world.resource_mut::<SaveRequests>();
        let actions = (requests.load, requests.save, requests.menu, requests.manual);
        requests.load = false;
        requests.save = false;
        requests.menu = false;
        requests.manual = false;
        if actions.0 || actions.1 {
            requests.elapsed = 0.0;
        }
        actions
    };
    if !load && !save {
        return;
    }
    let lang = *world.resource::<Language>();
    let path = match world.resource::<SaveLocation>().0.clone() {
        Some(path) => path,
        None => {
            world.resource_mut::<SaveStatus>().error =
                Some(tr(lang, "無法取得存檔目錄。", "Save directory is unavailable.").into());
            return;
        }
    };
    if load {
        let result = read_slot(&path).and_then(|save| restore(world, save));
        let mut status = world.resource_mut::<SaveStatus>();
        match result {
            Ok(()) => {
                status.available = true;
                status.error = None;
                status.notice = tr(lang, "已載入進度。", "Progress restored.").into();
            }
            Err(err) => {
                status.available = false;
                status.notice.clear();
                status.error = Some(format!(
                    "{} {err}",
                    tr(lang, "無法載入存檔：", "Could not load save:")
                ));
            }
        }
        return;
    }
    if !matches!(world.resource::<State<GameFlow>>().get(), GameFlow::Playing)
        || world.resource::<GameState>().hp <= 0.0
    {
        return;
    }
    let result = snapshot(world).and_then(|save| write_slot(&path, &save));
    let successful = result.is_ok();
    {
        let mut status = world.resource_mut::<SaveStatus>();
        match result {
            Ok(()) => {
                status.available = true;
                status.error = None;
                if manual {
                    status.notice = tr(lang, "進度已儲存。", "Progress saved.").into();
                }
            }
            Err(err) => {
                status.notice.clear();
                status.error = Some(format!(
                    "{} {err}",
                    tr(lang, "儲存失敗：", "Could not save:")
                ));
            }
        }
    }
    if successful && menu {
        world.write_message(RestartGame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{PlayerDied, ResetCombat};
    use crate::game_flow::{GameFlowPlugin, SkipPrologue, StartGame};
    use crate::survival::{ShelterPlugin, Upgrade};
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn sample() -> SaveFile {
        SaveFile {
            version: SAVE_VERSION,
            campaign: Campaign {
                game_min: 1800.0,
                ..default()
            },
            player: PlayerSnapshot {
                hp: 73.0,
                bleed: true,
                stun: 0.0,
                kills: 2,
                survived: 1800.0,
                position: [147.3, 0.0, -135.5],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            role_id: "office".into(),
            female: true,
            inventory: Inventory::default(),
            shelter: Shelter::default(),
            environment: ShelterEnvironment::default(),
            needs: PlayerNeeds::default(),
            world: ScavengeState::default(),
            map: MapProgress::default(),
            vehicles: crate::vehicles::VehicleFleet::default(),
            armor: crate::combat::armor::ArmorFleet::default(),
            remnants: crate::remnant::RemnantState::default(),
            japan: crate::japan::JapanState::default(),
            rescue: crate::rescue::RescueState::default(),
            snipers: crate::sniper::SniperState::default(),
        }
    }

    #[test]
    fn legacy_save_adds_deterministic_ground_transport_without_changing_supplies() {
        let sample = sample();
        let expected = serde_json::to_value(&sample.vehicles).unwrap();
        let mut legacy = serde_json::to_value(&sample).unwrap();
        legacy.as_object_mut().unwrap().remove("vehicles");
        legacy.as_object_mut().unwrap().remove("armor");
        let loaded = decode(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(loaded.vehicles).unwrap(), expected);
        assert_eq!(loaded.inventory, sample.inventory);
        assert!(loaded.armor.validate());
    }

    #[test]
    fn vehicle_save_keeps_fuel_damage_and_mounted_ammunition_and_rejects_duplicate_capture() {
        let mut saved = sample();
        let position = crate::map::world().spawn;
        let id = saved
            .vehicles
            .add_captured(crate::vehicles::VehicleKind::Ztd05, position, 0.0);
        saved.vehicles.current = Some(id);
        let vehicle = saved.vehicles.get_mut(id).unwrap();
        vehicle.fuel = 17.25;
        vehicle.engine = 0.4;
        vehicle.gun.as_mut().unwrap().fire().unwrap();
        let encoded = serde_json::to_vec(&saved).unwrap();
        let loaded = decode(&encoded).unwrap();
        assert_eq!(
            serde_json::to_value(&loaded.vehicles).unwrap(),
            serde_json::to_value(&saved.vehicles).unwrap()
        );
        saved.armor.units[0].abandoned = true;
        saved.armor.units[0].hp = 30.0;
        saved.armor.units[0].captured_id = Some(99_999);
        assert!(saved.validate().is_err());
        saved.armor.units[0].kind = crate::vehicles::VehicleKind::Ztd05;
        saved.armor.units[0].captured_id = Some(id);
        assert!(saved.validate().is_ok());
        let mut duplicate = saved.armor.units[0].clone();
        duplicate.id = 99_998;
        saved.armor.units.push(duplicate);
        assert!(
            saved.validate().is_err(),
            "Two abandoned hulls must not map to one captured vehicle"
        );
    }

    #[test]
    fn container_aging_roundtrip_preserves_clock_backlog_and_rng() {
        let mut saved = sample();
        saved.world.container_aging.minute = Some(1800.0);
        saved.world.container_aging.elapsed_hours = 12.5;
        saved.world.container_aging.seed = 1234567;
        saved.validate().unwrap();
        let decoded: SaveFile =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.world.container_aging.minute, Some(1800.0));
        assert_eq!(decoded.world.container_aging.elapsed_hours, 12.5);
        assert_eq!(decoded.world.container_aging.seed, 1234567);
    }

    #[test]
    fn legacy_save_has_no_retroactive_container_aging() {
        let mut json = serde_json::to_value(sample()).unwrap();
        json["world"]
            .as_object_mut()
            .unwrap()
            .remove("container_aging");
        let decoded: SaveFile = serde_json::from_value(json).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.world.container_aging.minute, None);
        assert_eq!(decoded.world.container_aging.elapsed_hours, 0.0);
    }

    #[test]
    fn save_rejects_invalid_container_aging_without_touching_live_world() {
        for minute in [f32::NAN, f32::INFINITY, -1.0, 1800.5] {
            let mut saved = sample();
            saved.world.container_aging.minute = Some(minute);
            assert!(saved.validate().is_err());
        }
        for hours in [f64::NAN, f64::INFINITY, -0.01, 31.0] {
            let mut saved = sample();
            saved.world.container_aging.minute = Some(1800.0);
            saved.world.container_aging.elapsed_hours = hours;
            assert!(saved.validate().is_err());
        }
        let mut saved = sample();
        saved.world.container_aging.elapsed_hours = 1.0;
        assert!(saved.validate().is_err());
    }

    struct TempSlot(PathBuf);

    impl TempSlot {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            Self(std::env::temp_dir().join(format!(
                "defendtaipei-save-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
        }

        fn path(&self) -> PathBuf {
            self.0.join(SAVE_NAME)
        }
    }

    impl Drop for TempSlot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn roundtrip_preserves_resources_and_ordered_stash() {
        let mut save = sample();
        save.shelter.stash.add("plank", 3);
        save.world.searched.push("cabinet-1".into());
        save.world.dead.push(7);
        let bytes = serde_json::to_vec(&save).unwrap();
        let restored = decode(&bytes).unwrap();
        assert_eq!(restored.player.hp, 73.0);
        assert!(restored.player.bleed);
        assert!(restored.female);
        assert_eq!(restored.campaign.game_min, 1800.0);
        assert_eq!(restored.shelter.stash.stacks(), save.shelter.stash.stacks());
        assert_eq!(restored.world.searched, ["cabinet-1"]);
        assert_eq!(restored.world.dead, [7]);
    }

    #[test]
    fn corrupt_and_future_saves_are_rejected_without_fallback() {
        assert!(decode(b"{\"version\":1,").is_err());
        assert!(matches!(
            decode(b"{\"version\":2}"),
            Err(SaveError::Version(2))
        ));
        assert!(decode(&vec![b' '; MAX_SAVE_BYTES + 1]).is_err());
        let mut save = sample();
        save.player.hp = 0.0;
        assert!(save.validate().is_err());
        save.player.hp = 100.0;
        save.player.position[0] = f32::NAN;
        assert!(save.validate().is_err());
    }

    #[test]
    fn duplicate_world_ids_and_unknown_roles_are_rejected() {
        let mut save = sample();
        save.world.searched = vec!["cabinet-1".into(), "cabinet-1".into()];
        assert!(save.validate().is_err());
        save.world.searched.clear();
        save.role_id = "unknown-role".into();
        assert!(save.validate().is_err());
    }

    #[test]
    fn legacy_saves_without_map_and_door_fields_preserve_their_open_passages() {
        let mut json = serde_json::to_value(sample()).unwrap();
        json.as_object_mut().unwrap().remove("map");
        json["world"]
            .as_object_mut()
            .unwrap()
            .remove("closed_doors");
        json["world"]
            .as_object_mut()
            .unwrap()
            .remove("barricaded_doors");
        json["world"]
            .as_object_mut()
            .unwrap()
            .remove("fixtures_version");
        let saved = decode(&serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(saved.player.hp, 73.0);
        assert_eq!(saved.campaign.game_min, 1800.0);
        assert_eq!(saved.map, MapProgress::default());
        assert!(saved.world.closed_doors.is_empty());
        assert!(saved.world.barricaded_doors.is_empty());
        assert_eq!(saved.world.fixtures_version, 0);
    }

    #[test]
    fn partial_fixture_loot_and_carried_items_survive_disk_continue_without_refill() {
        let mut saved = sample();
        saved.world.fixtures_version = 1;
        saved.inventory.add("water", 2);
        saved.inventory.add("bandage", 1);
        let mut leftovers = crate::survival::Stash::default();
        leftovers.add("water", 1);
        leftovers.add("bandage", 3);
        let fixture_ids: Vec<_> = crate::map::world()
            .interiors
            .iter()
            .filter(|f| f.loot.is_some())
            .take(2)
            .map(|f| f.id.clone())
            .collect();
        assert_eq!(fixture_ids.len(), 2);
        saved.world.searched = fixture_ids.clone();
        saved.world.loot = vec![
            (fixture_ids[0].clone(), leftovers.clone()),
            (fixture_ids[1].clone(), crate::survival::Stash::default()),
        ];
        let slot = TempSlot::new();
        write_slot(&slot.path(), &saved).unwrap();
        let mut world = World::new();
        world.spawn((Player, Transform::default()));
        world.insert_resource(NextState::<GameFlow>::Unchanged);
        world.init_resource::<Messages<LoadCompleted>>();
        for _ in 0..2 {
            restore(&mut world, read_slot(&slot.path()).unwrap()).unwrap();
            let progress = world.resource::<ScavengeState>();
            assert_eq!(progress.fixtures_version, 1);
            assert_eq!(progress.loot[0].1, leftovers);
            assert!(progress.loot[1].1.stacks().is_empty());
            let bag = world.resource::<Inventory>();
            assert_eq!(bag.count("water") + progress.loot[0].1.count("water"), 3);
            assert_eq!(
                bag.count("bandage") + progress.loot[0].1.count("bandage"),
                4
            );
            let roundtrip = snapshot(&mut world).unwrap();
            write_slot(&slot.path(), &roundtrip).unwrap();
        }
        let mut saved = sample();
        saved.world.fixtures_version = 2;
        assert!(saved.validate().is_err());
    }

    #[test]
    fn nonempty_exploration_and_note_survive_real_save_snapshot_and_continue() {
        let mut saved = sample();
        let point = Vec2::new(saved.player.position[0], saved.player.position[2]);
        saved.map.reveal(point, 38.0);
        assert!(saved.map.add_note(
            point + Vec2::new(12.0, -8.0),
            crate::worldmap::NoteKind::Supplies
        ));
        let expected = saved.map.clone();
        let slot = TempSlot::new();
        write_slot(&slot.path(), &saved).unwrap();
        let mut world = World::new();
        world.spawn((Player, Transform::default()));
        world.insert_resource(NextState::<GameFlow>::Unchanged);
        world.init_resource::<Messages<LoadCompleted>>();
        restore(&mut world, read_slot(&slot.path()).unwrap()).unwrap();
        assert_eq!(world.resource::<MapProgress>(), &expected);
        assert_eq!(snapshot(&mut world).unwrap().map, expected);
    }

    #[test]
    fn door_deltas_roundtrip_and_reject_unknown_or_duplicate_buildings() {
        let id = crate::map::world()
            .blds
            .iter()
            .find(|b| b.door.is_some())
            .unwrap()
            .id;
        let mut saved = sample();
        saved.world.closed_doors = vec![id];
        let loaded = decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(loaded.world.closed_doors, vec![id]);
        saved.world.closed_doors.push(id);
        assert!(saved.validate().is_err());
        saved.world.closed_doors = vec![u32::MAX];
        assert!(saved.validate().is_err());
    }

    #[test]
    fn barricade_save_rejects_duplicate_unknown_and_open_barricades() {
        let id = crate::map::world()
            .blds
            .iter()
            .find(|b| b.door.is_some())
            .unwrap()
            .id;
        let mut saved = sample();
        saved.world.closed_doors = vec![id];
        saved.world.barricaded_doors = vec![id];
        assert!(saved.validate().is_ok());
        saved.world.barricaded_doors.push(id);
        assert!(saved.validate().is_err());
        saved.world.barricaded_doors = vec![u32::MAX];
        assert!(saved.validate().is_err());
        saved.world.barricaded_doors = vec![id];
        saved.world.closed_doors.clear();
        assert!(saved.validate().is_err());
    }

    #[test]
    fn barricade_and_spent_plank_survive_disk_continue_and_unloaded_views() {
        let map = crate::map::world().clone();
        let (id, door) = map
            .blds
            .iter()
            .find_map(|b| b.door.map(|d| (b.id, d)))
            .unwrap();
        let mut saved = sample();
        saved.world.closed_doors = vec![id];
        saved.world.barricaded_doors = vec![id];
        saved.inventory = Inventory::default();
        saved.inventory.add("hammer", 1);
        saved.inventory.add("plank", 1);
        saved.player.position = [door.p.x + door.n.x, 0.0, door.p.y + door.n.y];
        let slot = TempSlot::new();
        write_slot(&slot.path(), &saved).unwrap();
        let mut world = World::new();
        world.spawn((Player, Transform::default()));
        world.insert_resource(NextState::<GameFlow>::Unchanged);
        world.init_resource::<Messages<LoadCompleted>>();
        world.insert_resource(crate::map::Obstacles::new(map));
        for _ in 0..2 {
            restore(&mut world, read_slot(&slot.path()).unwrap()).unwrap();
            let obstacles = world.resource::<crate::map::Obstacles>();
            assert!(obstacles.doors().is_barricaded(id));
            assert!(!obstacles.doors().set_open(id, true));
            assert!(!obstacles.los(door.p + door.n, door.p - door.n));
            assert_eq!(world.resource::<Inventory>().count("plank"), 1);
            assert_eq!(world.resource::<Inventory>().count("hammer"), 1);
            assert_eq!(world.resource::<ScavengeState>().barricaded_doors, vec![id]);
        }
    }

    #[test]
    fn threshold_repair_refunds_removed_barricade_once_without_teleporting() {
        let map = crate::map::world().clone();
        let (id, door) = map
            .blds
            .iter()
            .find_map(|b| b.door.map(|d| (b.id, d)))
            .unwrap();
        let mut saved = sample();
        saved.world.closed_doors = vec![id];
        saved.world.barricaded_doors = vec![id];
        saved.inventory = Inventory::default();
        saved.inventory.add("plank", 1);
        saved.player.position = [door.p.x, 0.0, door.p.y];
        let mut world = World::new();
        world.spawn((Player, Transform::default()));
        world.insert_resource(NextState::<GameFlow>::Unchanged);
        world.init_resource::<Messages<LoadCompleted>>();
        world.insert_resource(crate::map::Obstacles::new(map));
        restore(&mut world, saved).unwrap();
        assert_eq!(world.resource::<Inventory>().count("plank"), 2);
        assert!(
            world
                .resource::<ScavengeState>()
                .barricaded_doors
                .is_empty()
        );
        assert!(world.resource::<ScavengeState>().closed_doors.is_empty());
        assert_eq!(
            world
                .query_filtered::<&Transform, With<Player>>()
                .single(&world)
                .unwrap()
                .translation
                .xz(),
            door.p
        );
        let repaired = snapshot(&mut world).unwrap();
        restore(&mut world, repaired).unwrap();
        assert_eq!(world.resource::<Inventory>().count("plank"), 2);
    }

    #[test]
    fn legacy_continue_repairs_new_furniture_overlap_but_preserves_clear_floor() {
        let map = crate::map::world().clone();
        let fixture = map
            .interiors
            .iter()
            .find(|f| f.loot.is_some())
            .unwrap()
            .clone();
        let mut world = World::new();
        world.spawn((Player, Transform::default()));
        world.insert_resource(NextState::<GameFlow>::Unchanged);
        world.init_resource::<Messages<LoadCompleted>>();
        world.insert_resource(crate::map::Obstacles::new(map.clone()));
        for original in [fixture.pos, fixture.approach] {
            let mut saved = sample();
            saved.player.position = [original.x, 0.0, original.y];
            assert_eq!(saved.world.fixtures_version, 0);
            restore(&mut world, saved).unwrap();
            let restored = world
                .query_filtered::<&Transform, With<Player>>()
                .single(&world)
                .unwrap()
                .translation
                .xz();
            assert_eq!(map.bld_at(restored).map(|b| b.id), Some(fixture.building));
            assert!(map.free(restored, crate::player::PLAYER_RADIUS));
            if original == fixture.approach {
                assert_eq!(
                    restored, original,
                    "already clear positions must remain bit-for-bit unchanged"
                );
            } else {
                assert!(restored.distance(original) > crate::player::PLAYER_RADIUS);
                assert!(
                    restored.distance(original) < 4.0,
                    "repair should stay on nearby room floor"
                );
            }
            let repaired = snapshot(&mut world).unwrap();
            restore(&mut world, repaired).unwrap();
            assert_eq!(
                world
                    .query_filtered::<&Transform, With<Player>>()
                    .single(&world)
                    .unwrap()
                    .translation
                    .xz(),
                restored
            );
        }
    }

    #[test]
    fn continue_restores_shared_door_collision_and_protects_a_saved_threshold() {
        let map = crate::map::world().clone();
        let (id, door) = map
            .blds
            .iter()
            .find_map(|b| b.door.map(|d| (b.id, d)))
            .unwrap();
        let mut saved = sample();
        saved.world.closed_doors = vec![id];
        saved.player.position = [door.p.x + door.n.x, 0.0, door.p.y + door.n.y];
        let mut world = World::new();
        world.spawn((Player, Transform::default()));
        world.insert_resource(NextState::<GameFlow>::Unchanged);
        world.init_resource::<Messages<LoadCompleted>>();
        world.insert_resource(crate::map::Obstacles::new(map));
        restore(&mut world, saved).unwrap();
        assert!(
            !world
                .resource::<crate::map::Obstacles>()
                .doors()
                .is_open(id)
        );
        let mut saved = sample();
        saved.world.closed_doors = vec![id];
        saved.player.position = [door.p.x, 0.0, door.p.y];
        restore(&mut world, saved).unwrap();
        assert!(
            world
                .resource::<crate::map::Obstacles>()
                .doors()
                .is_open(id)
        );
        assert!(world.resource::<ScavengeState>().closed_doors.is_empty());
        let pos = world
            .query_filtered::<&Transform, With<Player>>()
            .single(&world)
            .unwrap()
            .translation
            .xz();
        assert_eq!(pos, door.p, "Continue must not teleport the player");
    }

    #[test]
    fn atomic_save_replaces_previous_and_keeps_it_on_validation_failure() {
        let slot = TempSlot::new();
        let mut save = sample();
        write_slot(&slot.path(), &save).unwrap();
        save.player.hp = 55.0;
        write_slot(&slot.path(), &save).unwrap();
        assert_eq!(read_slot(&slot.path()).unwrap().player.hp, 55.0);
        assert!(!slot.path().with_extension("tmp").exists());
        save.player.hp = -1.0;
        assert!(write_slot(&slot.path(), &save).is_err());
        assert_eq!(read_slot(&slot.path()).unwrap().player.hp, 55.0);
    }

    #[test]
    fn interrupted_temp_file_does_not_replace_last_complete_save() {
        let slot = TempSlot::new();
        write_slot(&slot.path(), &sample()).unwrap();
        fs::write(slot.path().with_extension("tmp"), b"{\"version\":1").unwrap();
        assert_eq!(read_slot(&slot.path()).unwrap().player.hp, 73.0);
        remove_slot(&slot.path()).unwrap();
        assert!(
            matches!(read_slot(&slot.path()), Err(SaveError::Io(err)) if err.kind() == io::ErrorKind::NotFound)
        );
    }

    #[test]
    fn restore_is_atomic_and_bypasses_new_game_resets() {
        let mut world = World::new();
        let player = world.spawn((Player, Transform::default())).id();
        world.insert_resource(NextState::<GameFlow>::Unchanged);
        world.init_resource::<Messages<LoadCompleted>>();
        restore(&mut world, sample()).unwrap();
        assert_eq!(
            world.get::<Transform>(player).unwrap().translation,
            Vec3::new(147.3, 0.0, -135.5)
        );
        assert_eq!(world.resource::<GameState>().hp, 73.0);
        assert_eq!(world.resource::<SelectedRole>().gender, Gender::Female);
        assert_eq!(world.resource::<Campaign>().game_min, 1800.0);
        assert!(matches!(
            world.resource::<NextState<GameFlow>>(),
            NextState::Pending(GameFlow::Playing)
        ));
        assert_eq!(world.resource::<Messages<LoadCompleted>>().len(), 1);
        assert!(!world.resource_mut::<DawnTracker>().crossed(1800.0));
        assert!(world.resource_mut::<DawnTracker>().crossed(3240.0));
        let mut invalid = sample();
        invalid.player.hp = -1.0;
        assert!(restore(&mut world, invalid).is_err());
        assert_eq!(world.resource::<GameState>().hp, 73.0);
    }

    fn world_with_save_request(path: PathBuf) -> World {
        let mut world = World::new();
        world.spawn((Player, Transform::default()));
        world.insert_resource(NextState::<GameFlow>::Unchanged);
        world.init_resource::<Messages<LoadCompleted>>();
        world.init_resource::<Messages<RestartGame>>();
        restore(&mut world, sample()).unwrap();
        world.insert_resource(State::new(GameFlow::Playing));
        world.insert_resource(Language::En);
        world.insert_resource(SaveLocation(Some(path)));
        world.init_resource::<SaveStatus>();
        world.insert_resource(SaveRequests {
            save: true,
            menu: true,
            manual: true,
            ..default()
        });
        world
    }

    #[test]
    fn menu_request_saves_live_state_before_requesting_restart() {
        let slot = TempSlot::new();
        let mut world = world_with_save_request(slot.path());
        service_requests(&mut world);
        assert_eq!(read_slot(&slot.path()).unwrap().campaign.game_min, 1800.0);
        assert_eq!(world.resource::<Campaign>().game_min, 1800.0);
        assert_eq!(world.resource::<Messages<RestartGame>>().len(), 1);
        assert!(world.resource::<SaveStatus>().available);
    }

    #[test]
    fn failed_menu_save_keeps_run_and_does_not_request_restart() {
        let slot = TempSlot::new();
        fs::create_dir_all(&slot.0).unwrap();
        let blocked_directory = slot.0.join("not-a-directory");
        fs::write(&blocked_directory, b"block").unwrap();
        let mut world = world_with_save_request(blocked_directory.join(SAVE_NAME));
        service_requests(&mut world);
        assert_eq!(world.resource::<Campaign>().game_min, 1800.0);
        assert_eq!(world.resource::<GameState>().hp, 73.0);
        assert_eq!(world.resource::<Messages<RestartGame>>().len(), 0);
        assert!(world.resource::<SaveStatus>().error.is_some());
    }

    fn save_app(path: PathBuf) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_resource(SaveLocation(Some(path)))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)))
            .init_resource::<Language>()
            .init_resource::<GameState>()
            .init_resource::<SelectedRole>()
            .init_resource::<Inventory>()
            .init_resource::<PlayerNeeds>()
            .init_resource::<ScavengeState>()
            .add_message::<PlayerDied>()
            .add_message::<ResetCombat>()
            .add_message::<AppLifecycle>()
            .add_message::<WindowFocused>()
            .add_message::<WindowCloseRequested>()
            .add_plugins((GameFlowPlugin, ShelterPlugin, SavePlugin));
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(Duration::from_secs(1));
        app.world_mut()
            .spawn((Player, Transform::from_xyz(147.3, 0.0, -135.5)));
        app
    }

    #[test]
    fn start_autosave_and_continue_preserve_dawn_and_shelter_actions() {
        let slot = TempSlot::new();
        // A new game must discard the previous run before its first autosave.
        write_slot(&slot.path(), &sample()).unwrap();
        let mut app = save_app(slot.path());
        app.update();
        assert!(app.world().resource::<SaveStatus>().available);
        app.world_mut().write_message(StartGame);
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<State<GameFlow>>().get(),
            &GameFlow::Prologue
        );
        assert!(!slot.path().exists());
        assert!(!app.world().resource::<SaveStatus>().available);
        app.world_mut().resource_mut::<Campaign>().game_min = 1798.0;
        app.world_mut()
            .resource_scope(|world, mut shelter: Mut<Shelter>| {
                shelter.home = Some([147.3, -135.5]);
                let mut inventory = world.resource_mut::<Inventory>();
                inventory.add("plank", 3);
                inventory.add("can", 4);
                inventory.add("water", 4);
                shelter
                    .build(Upgrade::Boards, &mut inventory, [147.3, -135.5])
                    .unwrap();
                assert_eq!(shelter.deposit(&mut inventory, [147.3, -135.5]).unwrap(), 8);
            });
        app.world_mut().write_message(SkipPrologue);
        app.update();
        // No wall-clock sleeps: Time<Real> advances by one second per update.
        for _ in 0..21 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<State<GameFlow>>().get(),
            &GameFlow::Playing
        );
        assert!(app.world().resource::<SaveStatus>().available);
        let autosaved = read_slot(&slot.path()).unwrap();
        assert_eq!(autosaved.shelter.day, 2);
        assert!(autosaved.shelter.has(Upgrade::Boards));
        assert_eq!(autosaved.inventory.count("plank"), 0);
        assert_eq!(autosaved.inventory.count("can"), 0);
        assert_eq!(autosaved.shelter.stash.count("can"), 2);

        app.world_mut().write_message(SaveAndMenu);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<State<GameFlow>>().get(),
            &GameFlow::Title
        );
        let saved = read_slot(&slot.path()).unwrap();
        app.world_mut().resource_mut::<Shelter>().day = 99;
        app.world_mut()
            .resource_mut::<Shelter>()
            .stash
            .add("can", 100);
        app.world_mut().write_message(ContinueGame);
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<State<GameFlow>>().get(),
            &GameFlow::Playing
        );
        assert_eq!(app.world().resource::<Shelter>().day, saved.shelter.day);
        assert_eq!(app.world().resource::<Shelter>().stash, saved.shelter.stash);
        assert!(app.world().resource::<Shelter>().has(Upgrade::Boards));
        assert_eq!(app.world().resource::<Inventory>().count("plank"), 0);
        assert!(app.world().resource::<Campaign>().game_min >= saved.campaign.game_min);
    }

    #[test]
    fn pending_evacuation_saves_final_progress_and_death_removes_slot() {
        let slot = TempSlot::new();
        let mut app = save_app(slot.path());
        app.update();
        restore(app.world_mut(), sample()).unwrap();
        app.update();
        app.world_mut().resource_mut::<Campaign>().evac_taken = true;
        app.world_mut()
            .resource_mut::<NextState<GameFlow>>()
            .set(GameFlow::GameOver(Ending::EvacBoat));
        // Last sees a pending ending before the state transition can freeze play.
        app.world_mut().run_schedule(Last);
        assert!(read_slot(&slot.path()).unwrap().campaign.evac_taken);
        app.world_mut()
            .resource_mut::<NextState<GameFlow>>()
            .set(GameFlow::GameOver(Ending::Death));
        app.update();
        assert!(!slot.path().exists());
        assert!(!app.world().resource::<SaveStatus>().available);
    }

    #[test]
    fn android_background_event_saves_without_waiting_for_autosave() {
        let slot = TempSlot::new();
        let mut app = save_app(slot.path());
        app.update();
        restore(app.world_mut(), sample()).unwrap();
        app.update();
        assert!(!slot.path().exists());
        app.world_mut().resource_mut::<GameState>().hp = 61.0;
        app.world_mut().write_message(AppLifecycle::WillSuspend);
        app.world_mut().run_schedule(Last);
        assert_eq!(read_slot(&slot.path()).unwrap().player.hp, 61.0);
        assert!(app.world().resource::<SaveStatus>().available);
    }

    #[test]
    fn weather_and_drop_coordinates_must_be_valid() {
        let mut save = sample();
        save.world.weather.intensity = 2.0;
        assert!(save.validate().is_err());
        save.world.weather.intensity = 0.7;
        save.world.drops.push(("body:7".into(), [147.3, -135.5]));
        assert!(save.validate().is_err());
        save.world.loot.push(("body:7".into(), default()));
        assert!(save.validate().is_ok());
        save.world.drops[0].1[0] = f32::INFINITY;
        assert!(save.validate().is_err());
    }

    #[test]
    fn tap_cooldowns_reject_unknown_duplicate_and_invalid_values() {
        let mut save = sample();
        save.world.taps.push(("tap_home".into(), 750.0));
        assert!(save.validate().is_ok());
        for value in [-1.0, f32::INFINITY, f32::NAN, 1_000_000_128.0] {
            save.world.taps[0].1 = value;
            assert!(save.validate().is_err());
        }
        save.world.taps[0].1 = 750.0;
        save.world.taps[0].0 = "unknown_tap".into();
        assert!(save.validate().is_err());
        save.world.taps[0].0 = "tap_home".into();
        save.world.taps.push(("tap_home".into(), 800.0));
        assert!(save.validate().is_err());
    }

    #[test]
    fn clock_carry_must_be_finite_bounded_and_keep_time_nonnegative() {
        let mut save = sample();
        save.campaign.clock_remainder = 0.00001;
        assert!(save.validate().is_ok());
        for remainder in [f64::NAN, f64::INFINITY, -65.0, 65.0] {
            save.campaign.clock_remainder = remainder;
            assert!(save.validate().is_err());
        }
        save.campaign.game_min = 0.0;
        save.campaign.clock_remainder = -0.00001;
        assert!(save.validate().is_err());
        save.campaign.game_min = 1_000_000_000.0;
        save.campaign.clock_remainder = 0.1;
        assert!(save.validate().is_err());
    }

    #[test]
    fn earlier_v1_saves_default_new_clock_and_fixture_fields() {
        let mut json = serde_json::to_value(sample()).unwrap();
        json["campaign"]
            .as_object_mut()
            .unwrap()
            .remove("clock_remainder");
        json["world"].as_object_mut().unwrap().remove("taps");
        let restored = decode(&serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(restored.campaign.clock_remainder, 0.0);
        assert!(restored.world.taps.is_empty());
    }
}
