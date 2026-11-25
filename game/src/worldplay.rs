//! Playable links between the native map, carried items, shelter, clock and saves.
//!
//! Rules and item weights come from web v24.42.0. The native core has its own
//! deterministic placement stream; it does not reproduce the web renderer's
//! interleaved random calls or the districts outside the Wanhua core.

use std::collections::HashSet;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::audio::{Ambience, SfxEvent};
use crate::combat::{
    self, CombatEnv, Enemy, EnemyDied, EnemyKind, GameState, PlayerDied, SolState,
};
use crate::doors::{DoorPrompt, DoorTarget};
use crate::game_flow::{Campaign, FlowSet, GameFlow};
use crate::i18n::{Language, tr};
use crate::inventory::{Inventory, LOOT, UseResult, Vitals, item};
use crate::map::{EvacSites, Obstacles, world};
use crate::player::Player;
use crate::roles::SelectedRole;
use crate::save::LoadCompleted;
use crate::story::{EvacComplete, EvacRoute, POWER_OFF, WATER_OFF, evac_open};
use crate::survival::{
    MoralChoice, Shelter, ShelterEnvironment, ShelterSet, Stash, is_night, trade_at,
};
use crate::ui::{OpenPanel, PanelKind, PauseOpen, UiModal};

#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerNeeds {
    pub hunger: f32,
    pub thirst: f32,
    pub fatigue: f32,
}

impl Default for PlayerNeeds {
    fn default() -> Self {
        Self {
            hunger: 85.0,
            thirst: 85.0,
            fatigue: 10.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherState {
    pub next: f32,
    pub until: f32,
    pub amount: f32,
    pub intensity: f32,
    pub wet: f32,
    pub seed: u32,
}

impl Default for WeatherState {
    fn default() -> Self {
        Self {
            next: 15.0 * 60.0,
            until: 0.0,
            amount: 0.0,
            intensity: 1.0,
            wet: 0.0,
            seed: 20_261_024,
        }
    }
}

/// Saved world deltas. An empty recorded stack stays empty across a reload.
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScavengeState {
    pub searched: Vec<String>,
    pub asked: Vec<String>,
    pub night_seen: Vec<String>,
    pub loot: Vec<(String, Stash)>,
    pub dead: Vec<u32>,
    pub drops: Vec<(String, [f32; 2])>,
    /// Per-fixture next refill minute, persisted so Continue cannot bypass it.
    pub taps: Vec<(String, f32)>,
    /// Building IDs with closed entrances. Missing in older native saves means
    /// their previously open passages stay open; new games set their own defaults.
    pub closed_doors: Vec<u32>,
    pub weather: WeatherState,
}

#[derive(Resource, Default)]
pub struct WorldNotice {
    pub text: String,
}

/// Both the HUD and Interact use the same reachable target and priority.
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct InteractionHint {
    pub label: String,
    pub detail: String,
}

#[derive(Resource, Default)]
pub struct MoralPrompt(pub Option<String>);

#[derive(Message, Clone, Debug)]
pub enum WorldAction {
    Interact,
    FillWater,
    Use(String),
    Sleep,
    Speed(u8),
    Moral(MoralChoice),
}

#[derive(Clone, Debug)]
struct Container {
    id: String,
    pos: Vec2,
    kind: &'static str,
    cabinet: bool,
}

#[derive(Clone)]
struct Patrol {
    id: u32,
    route: Vec<Vec2>,
}

#[derive(Resource)]
struct WorldCatalog {
    containers: Vec<Container>,
    patrols: Vec<Patrol>,
    home: Vec2,
}

#[derive(Resource, Default)]
struct FrameProgress {
    minute: Option<f32>,
    day: u32,
}

#[derive(Resource)]
struct LootArt {
    mesh: Handle<Mesh>,
    full: Handle<StandardMaterial>,
    empty: Handle<StandardMaterial>,
    water: Handle<StandardMaterial>,
}

const HOME_TAP_ID: &str = "tap_home";

#[derive(Component)]
struct LootMarker(String);

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorldSet;

/// Intent producers run before pause, the game clock and simulation.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorldInputSet;

pub struct WorldPlayPlugin;

impl Plugin for WorldPlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inventory>()
            .init_resource::<PlayerNeeds>()
            .init_resource::<ScavengeState>()
            .init_resource::<WorldNotice>()
            .init_resource::<MoralPrompt>()
            .init_resource::<InteractionHint>()
            .init_resource::<DoorPrompt>()
            .init_resource::<FrameProgress>()
            .add_message::<WorldAction>()
            .configure_sets(Update, WorldSet.after(ShelterSet).after(combat::CombatSet))
            .add_systems(Startup, (plan_world, setup_loot_art))
            // Continue is serviced in Last. Restore the world before the next
            // clock/dawn update, including when loading just before 06:00.
            .add_systems(PreUpdate, (restore_world, pause_on_background))
            .add_systems(
                Update,
                weather_before_dawn.after(FlowSet).before(ShelterSet),
            )
            .add_systems(
                OnEnter(GameFlow::Prologue),
                reset_new_run.after(crate::survival::reset),
            )
            .add_systems(
                Update,
                (keyboard_actions, danger_speed)
                    .chain()
                    .in_set(WorldInputSet)
                    .before(FlowSet)
                    .after(crate::input::InputSet),
            )
            .add_systems(
                Update,
                (
                    apply_actions,
                    tick_needs_weather,
                    remember_deaths,
                    update_environment,
                    update_evac,
                    stream_loot,
                    update_interaction_hint,
                )
                    .chain()
                    .in_set(WorldSet),
            );
    }
}

/// JS's signed 32-bit string hash (`(h << 5) - h + charCodeAt`).
pub fn id_hash(id: &str) -> u32 {
    id.encode_utf16()
        .fold(0_i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
        .unsigned_abs()
}

fn next(seed: &mut u32) -> f64 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    f64::from(*seed) / 4_294_967_296.0
}

fn plan_world(mut commands: Commands) {
    let map = world();
    let home = map
        .blds
        .iter()
        .filter_map(|b| b.door.map(|d| (b, d)))
        .min_by(|(_, a), (_, b)| {
            a.p.distance_squared(map.spawn)
                .total_cmp(&b.p.distance_squared(map.spawn))
        })
        .map_or(map.spawn, |(b, door)| {
            let p = door.p - door.n * 1.4;
            if crate::map::data::pip(p, &b.pts) {
                p
            } else {
                b.c
            }
        });
    let mut containers: Vec<_> = map
        .blds
        .iter()
        .filter_map(|b| {
            let door = b.door?;
            let mut pos = door.p - door.n * 2.7;
            if !crate::map::data::pip(pos, &b.pts) {
                pos = b.c;
            }
            let kind = match b.kind {
                "pharmacy" | "hospital" | "clinic" => "pharmacy",
                "store" | "convenience" | "market" | "food" | "restaurant" => "store",
                "police" => "armory",
                _ => "home",
            };
            Some(Container {
                id: format!("c{}", b.id),
                pos,
                kind,
                cabinet: kind == "home",
            })
        })
        .collect();
    // The native map supplies door/interior geometry, not the web's furniture.
    // A visible water fixture is placed at the home spawn; its refill rule is
    // the original sink/water-dispenser rule (one bottle per 90 game minutes).
    containers.push(Container {
        id: HOME_TAP_ID.into(),
        pos: home,
        kind: "tap",
        cabinet: false,
    });
    let roads: Vec<_> = map
        .roads
        .iter()
        .filter(|r| r.p.len() >= 3 && r.t != "footway")
        .collect();
    let mut seed = 20_261_024_u32;
    let patrols = (0..64)
        .filter_map(|id| {
            if roads.is_empty() {
                return None;
            }
            let road = roads[(next(&mut seed) * roads.len() as f64) as usize];
            let start = (next(&mut seed) * (road.p.len() - 1) as f64) as usize;
            let route: Vec<_> =
                road.p[start.saturating_sub(3)..(start + 4).min(road.p.len())].to_vec();
            Some(Patrol { id, route })
        })
        .collect();
    commands.insert_resource(WorldCatalog {
        containers,
        patrols,
        home,
    });
}

fn setup_loot_art(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(LootArt {
        mesh: meshes.add(Cuboid::new(0.85, 0.65, 0.55)),
        full: materials.add(StandardMaterial {
            base_color: Color::srgb(0.59, 0.40, 0.18),
            emissive: LinearRgba::rgb(0.06, 0.035, 0.005),
            ..default()
        }),
        empty: materials.add(Color::srgb(0.22, 0.23, 0.22)),
        water: materials.add(Color::srgb(0.12, 0.48, 0.66)),
    });
}

fn spawn_patrols(commands: &mut Commands, catalog: &WorldCatalog, dead: &[u32]) {
    for patrol in &catalog.patrols {
        if dead.contains(&patrol.id) {
            continue;
        }
        let kind = if patrol.id >= 48 {
            EnemyKind::NightPatrol
        } else {
            EnemyKind::Patrol
        };
        combat::spawn_enemy_with_route(
            commands,
            patrol.id,
            patrol.route[0],
            kind,
            patrol.route.clone(),
        );
    }
}

fn reset_new_run(
    mut commands: Commands,
    catalog: Res<WorldCatalog>,
    selected: Res<SelectedRole>,
    mut inventory: ResMut<Inventory>,
    mut needs: ResMut<PlayerNeeds>,
    mut progress: ResMut<ScavengeState>,
    mut shelter: ResMut<Shelter>,
    mut frame: ResMut<FrameProgress>,
    campaign: Res<Campaign>,
    mut prompt: ResMut<MoralPrompt>,
    mut notice: ResMut<WorldNotice>,
    mut player: Query<&mut Transform, With<Player>>,
    old: Query<Entity, With<Enemy>>,
    markers: Query<Entity, With<LootMarker>>,
    obstacles: Res<Obstacles>,
) {
    *inventory = Inventory::from_role(selected.role());
    *needs = PlayerNeeds::default();
    *progress = ScavengeState::default();
    obstacles
        .doors()
        .replace_closed(&obstacles.doors().all_ids());
    // The player's shelter has an open exit on a fresh run. Every other door
    // starts closed, as in the Web game, without modifying the world planner.
    if let Some(home) = world()
        .blds
        .iter()
        .filter_map(|b| b.door.map(|d| (b.id, d)))
        .min_by(|(_, a), (_, b)| {
            a.p.distance_squared(world().spawn)
                .total_cmp(&b.p.distance_squared(world().spawn))
        })
    {
        obstacles.doors().set_open(home.0, true);
    }
    crate::doors::protect_position(&obstacles, catalog.home);
    progress.closed_doors = obstacles.doors().closed_ids();
    shelter.home = Some(catalog.home.to_array());
    *frame = FrameProgress {
        minute: Some(campaign.game_min),
        day: shelter.day,
    };
    prompt.0 = None;
    notice.text.clear();
    for mut transform in &mut player {
        transform.translation = Vec3::new(catalog.home.x, 0.0, catalog.home.y);
    }
    for entity in &old {
        commands.entity(entity).despawn();
    }
    for entity in &markers {
        commands.entity(entity).despawn();
    }
    spawn_patrols(&mut commands, &catalog, &[]);
}

fn restore_world(
    mut commands: Commands,
    mut loaded: MessageReader<LoadCompleted>,
    catalog: Res<WorldCatalog>,
    progress: Res<ScavengeState>,
    shelter: Res<Shelter>,
    mut frame: ResMut<FrameProgress>,
    mut campaign: ResMut<Campaign>,
    mut prompt: ResMut<MoralPrompt>,
    old: Query<Entity, With<Enemy>>,
    markers: Query<Entity, With<LootMarker>>,
) {
    if loaded.read().count() == 0 {
        return;
    }
    for entity in &old {
        commands.entity(entity).despawn();
    }
    for entity in &markers {
        commands.entity(entity).despawn();
    }
    campaign.sleeping = false;
    campaign.speed = 1;
    prompt.0 = None;
    *frame = FrameProgress {
        minute: Some(campaign.game_min),
        day: shelter.day,
    };
    spawn_patrols(&mut commands, &catalog, &progress.dead);
}

fn pause_on_background(
    flow: Res<State<GameFlow>>,
    mut pause: ResMut<PauseOpen>,
    mut focus: MessageReader<bevy::window::WindowFocused>,
    mut lifecycle: MessageReader<bevy::window::AppLifecycle>,
) {
    let unfocused = focus.read().any(|event| !event.focused);
    let suspending = lifecycle.read().any(|event| {
        matches!(
            event,
            bevy::window::AppLifecycle::WillSuspend | bevy::window::AppLifecycle::Suspended
        )
    });
    if flow.get() == &GameFlow::Playing && (unfocused || suspending) {
        pause.0 = true;
    }
}

fn keyboard_actions(
    keys: Res<ButtonInput<KeyCode>>,
    flow: Res<State<GameFlow>>,
    modal: Res<UiModal>,
    pause: Res<PauseOpen>,
    mut actions: MessageWriter<WorldAction>,
    mut pauses: MessageWriter<crate::ui::TogglePause>,
) {
    if flow.get() != &GameFlow::Playing || modal.blocks_input() {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        pauses.write(crate::ui::TogglePause);
    }
    if pause.0 {
        return;
    }
    if keys.just_pressed(KeyCode::KeyE) {
        actions.write(WorldAction::Interact);
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        actions.write(WorldAction::Sleep);
    }
    for (key, speed) in [
        (KeyCode::Digit1, 1),
        (KeyCode::Digit2, 3),
        (KeyCode::Digit3, 20),
    ] {
        if keys.just_pressed(key) {
            actions.write(WorldAction::Speed(speed));
        }
    }
}

fn danger_near(pos: Vec2, enemies: &Query<&Enemy>, night: bool) -> bool {
    enemies.iter().any(|enemy| {
        let s = &enemy.0;
        s.alive()
            && s.active(night)
            && (s.state == SolState::Combat
                || s.pos.distance(pos)
                    < if s.state == SolState::Investigate {
                        40.0
                    } else {
                        18.0
                    })
    })
}

fn danger_speed(
    flow: Res<State<GameFlow>>,
    mut campaign: ResMut<Campaign>,
    game: Res<GameState>,
    player: Query<&Transform, With<Player>>,
    enemies: Query<&Enemy>,
    input: Res<crate::input::PlayerInput>,
    mut notice: ResMut<WorldNotice>,
    lang: Res<Language>,
) {
    if flow.get() != &GameFlow::Playing {
        return;
    }
    let Ok(transform) = player.single() else {
        return;
    };
    let danger = danger_near(
        transform.translation.xz(),
        &enemies,
        is_night(f64::from(campaign.game_min)),
    );
    if (campaign.speed > 1 || campaign.sleeping) && (danger || game.stun > 0.0) {
        campaign.speed = 1;
        campaign.sleeping = false;
        notice.text = tr(
            *lang,
            "附近有危險，恢復正常速度。",
            "Danger nearby. Normal speed restored.",
        )
        .into();
    }
    if campaign.sleeping && (input.r#move.length_squared() > 0.0 || input.fire) {
        campaign.sleeping = false;
    }
}

fn nearest_container(
    pos: Vec2,
    catalog: &WorldCatalog,
    progress: &ScavengeState,
    obstacles: Option<&Obstacles>,
) -> Option<Container> {
    let building = world().bld_at(pos).map(|b| b.id);
    let mut best = catalog
        .containers
        .iter()
        .filter(|c| {
            c.pos.distance(pos) < 2.2
                && world().bld_at(c.pos).map(|b| b.id) == building
                && obstacles.is_none_or(|obs| obs.los(pos, c.pos))
        })
        .min_by(|a, b| {
            a.pos
                .distance_squared(pos)
                .total_cmp(&b.pos.distance_squared(pos))
        })
        .cloned();
    for (id, p) in &progress.drops {
        let q = Vec2::from_array(*p);
        if q.distance(pos) < 2.2
            && world().bld_at(q).map(|b| b.id) == building
            && obstacles.is_none_or(|obs| obs.los(pos, q))
            && best
                .as_ref()
                .is_none_or(|b| q.distance_squared(pos) < b.pos.distance_squared(pos))
        {
            best = Some(Container {
                id: id.clone(),
                pos: q,
                kind: "crate",
                cabinet: false,
            });
        }
    }
    best
}

#[derive(Clone)]
enum InteractionTarget {
    Door(DoorTarget),
    Container(Container),
    Trade,
    Shelter,
    None,
}

fn interaction_target(
    pos: Vec2,
    catalog: &WorldCatalog,
    progress: &ScavengeState,
    shelter: &Shelter,
    obstacles: &Obstacles,
) -> InteractionTarget {
    if trade_at(pos.to_array()).is_some() {
        return InteractionTarget::Trade;
    }
    let door = crate::doors::nearest_door(pos, obstacles);
    let container = nearest_container(pos, catalog, progress, Some(obstacles));
    if let Some(door) = door
        && container
            .as_ref()
            .is_none_or(|c| c.pos.distance(pos) + 0.12 >= door.distance)
    {
        return InteractionTarget::Door(door);
    }
    if let Some(container) = container {
        InteractionTarget::Container(container)
    } else if shelter.at_home(pos.to_array()) {
        InteractionTarget::Shelter
    } else {
        InteractionTarget::None
    }
}

#[allow(clippy::too_many_arguments)]
fn update_interaction_hint(
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    modal: Res<UiModal>,
    player: Query<&Transform, With<Player>>,
    obstacles: Res<Obstacles>,
    catalog: Res<WorldCatalog>,
    progress: Res<ScavengeState>,
    shelter: Res<Shelter>,
    lang: Res<Language>,
    mut hint: ResMut<InteractionHint>,
    mut door_prompt: ResMut<DoorPrompt>,
) {
    door_prompt.0 = None;
    let mut next = InteractionHint::default();
    if flow.get() == &GameFlow::Playing
        && !pause.0
        && !modal.blocks_input()
        && let Ok(player) = player.single()
    {
        let target = interaction_target(
            player.translation.xz(),
            &catalog,
            &progress,
            &shelter,
            &obstacles,
        );
        let (label, detail) = match target {
            InteractionTarget::Door(door) => {
                door_prompt.0 = Some(door);
                (
                    tr(
                        *lang,
                        if door.open { "關門" } else { "開門" },
                        if door.open { "Close door" } else { "Open door" },
                    ),
                    tr(*lang, "建築入口 · E 互動", "Entrance · E to interact"),
                )
            }
            InteractionTarget::Container(c) if c.id == HOME_TAP_ID => (
                tr(*lang, "裝水", "Fill water"),
                tr(*lang, "避難所飲水設備", "Shelter water fixture"),
            ),
            InteractionTarget::Container(c) if progress.searched.contains(&c.id) => (
                tr(*lang, "查看", "Inspect"),
                tr(*lang, "已搜尋的物資", "Previously searched supplies"),
            ),
            InteractionTarget::Container(c) if c.cabinet => (
                tr(*lang, "搜尋", "Search"),
                tr(
                    *lang,
                    "屋內櫥櫃 · E 互動",
                    "Household cabinet · E to interact",
                ),
            ),
            InteractionTarget::Container(_) => (
                tr(*lang, "搜尋", "Search"),
                tr(
                    *lang,
                    "附近物資 · E 互動",
                    "Nearby supplies · E to interact",
                ),
            ),
            InteractionTarget::Trade => (
                tr(*lang, "交易", "Barter"),
                tr(*lang, "龍山交易攤", "Longshan barter stall"),
            ),
            InteractionTarget::Shelter => (
                tr(*lang, "避難所", "Shelter"),
                tr(*lang, "你已回到家中", "You are back at home"),
            ),
            InteractionTarget::None => (
                tr(*lang, "互動", "Interact"),
                tr(*lang, "走近門口或物資", "Approach an entrance or supplies"),
            ),
        };
        next.label = label.into();
        next.detail = detail.into();
    }
    hint.set_if_neq(next);
}

fn roll_loot(id: &str, kind: &str, minute: f32) -> Stash {
    let mut seed = id_hash(id).wrapping_add(20_261_024);
    roll_loot_with(kind, minute, &mut seed)
}

/// `main.ts` rollLoot + spoilRolled, using the native container's stable stream.
fn roll_loot_with(kind: &str, minute: f32, seed: &mut u32) -> Stash {
    let table = LOOT
        .iter()
        .find(|table| table.kind == kind)
        .or_else(|| LOOT.iter().find(|table| table.kind == "home"))
        .expect("the original catalog contains home loot");
    let mut stack = Stash::default();
    let count = 1 + (next(seed) * 3.0) as usize;
    let total: f64 = table
        .entries
        .iter()
        .map(|entry| f64::from(entry.weight))
        .sum();
    for _ in 0..count {
        let mut choice = next(seed) * total;
        for entry in table.entries {
            choice -= f64::from(entry.weight);
            if choice <= 0.0 {
                stack.add(entry.item, 1 + (next(seed) * f64::from(entry.max)) as u32);
                break;
            }
        }
    }
    if kind == "armory" {
        if ["t75p", "t65k2", "t93", "huntsg"]
            .iter()
            .all(|id| stack.count(id) == 0)
        {
            stack.add("t75p", 1);
        }
        for (gun, ammo, count) in [
            ("t75p", "a9", 12),
            ("t65k2", "a556n", 30),
            ("t93", "a762n", 10),
            ("huntsg", "a12", 4),
        ] {
            if stack.count(gun) > 0 && stack.count(ammo) == 0 {
                stack.add(ammo, count);
            }
        }
    }
    if minute >= (POWER_OFF + 1440) as f32 {
        let fresh: Vec<_> = stack
            .stacks()
            .iter()
            .filter(|(id, _)| item(id).is_some_and(|definition| definition.perish))
            .cloned()
            .collect();
        for (id, count) in fresh {
            stack.take(&id, count);
            if next(seed) < 0.75 {
                stack.add("rotten", count.min(2));
            }
        }
        stack.remove_empty();
    }
    stack
}

#[derive(Debug, PartialEq, Eq)]
enum TapError {
    TooFar,
    WaterOff,
    Cooldown,
}

impl TapError {
    fn message(&self, lang: Language) -> &'static str {
        match self {
            Self::TooFar => tr(
                lang,
                "請走近避難所的藍色飲水設備。",
                "Move closer to the blue water fixture in your shelter.",
            ),
            Self::WaterOff => tr(
                lang,
                "水龍頭只滴了幾滴，已經停水。",
                "The tap only drips. The water supply is off.",
            ),
            Self::Cooldown => tr(
                lang,
                "這裡剛裝過水，過一陣子再來。",
                "You just filled water here. Come back later.",
            ),
        }
    }
}

fn fill_water(
    pos: Vec2,
    catalog: &WorldCatalog,
    progress: &mut ScavengeState,
    inventory: &mut Inventory,
    minute: f32,
    water: bool,
    obstacles: Option<&Obstacles>,
) -> Result<(), TapError> {
    let fixture = nearest_container(pos, catalog, progress, obstacles)
        .filter(|container| container.id == HOME_TAP_ID)
        .ok_or(TapError::TooFar)?;
    if !water {
        return Err(TapError::WaterOff);
    }
    if let Some((_, next)) = progress.taps.iter_mut().find(|(id, _)| id == &fixture.id) {
        // The web comparison is strictly >, not >=.
        if minute <= *next {
            return Err(TapError::Cooldown);
        }
        *next = minute + 90.0;
    } else {
        progress.taps.push((fixture.id.clone(), minute + 90.0));
    }
    let index = progress
        .loot
        .iter()
        .position(|(id, _)| id == &fixture.id)
        .unwrap_or_else(|| {
            progress.loot.push((fixture.id.clone(), Stash::default()));
            progress.loot.len() - 1
        });
    progress.loot[index].1.add("water", 1);
    loot_container(&fixture, progress, inventory, false, minute);
    Ok(())
}

fn loot_container(
    c: &Container,
    progress: &mut ScavengeState,
    inventory: &mut Inventory,
    night: bool,
    minute: f32,
) -> u32 {
    let index = progress
        .loot
        .iter()
        .position(|(id, _)| id == &c.id)
        .unwrap_or_else(|| {
            progress.loot.push((
                c.id.clone(),
                if c.id == HOME_TAP_ID {
                    Stash::default()
                } else {
                    roll_loot(&c.id, c.kind, minute)
                },
            ));
            progress.loot.len() - 1
        });
    if !progress.night_seen.contains(&c.id) {
        progress.night_seen.push(c.id.clone());
        let hash = id_hash(&c.id);
        if c.id != HOME_TAP_ID && night && hash.is_multiple_of(3) {
            progress.loot[index].1.add(
                ["cig", "battery", "can", "rice", "pill"][(hash % 5) as usize],
                1,
            );
        }
    }
    let mut total = 0;
    for (key, count) in progress.loot[index].1.stacks() {
        inventory.add(key, *count);
        total += count;
    }
    progress.loot[index].1 = Stash::default();
    if !progress.searched.contains(&c.id) {
        progress.searched.push(c.id.clone());
    }
    total
}

fn apply_actions(
    mut actions: MessageReader<WorldAction>,
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    catalog: Res<WorldCatalog>,
    player: Query<&Transform, With<Player>>,
    enemies: Query<&Enemy>,
    states: (
        ResMut<Campaign>,
        ResMut<ScavengeState>,
        ResMut<Inventory>,
        ResMut<Shelter>,
        ResMut<PlayerNeeds>,
        ResMut<GameState>,
    ),
    mut prompt: ResMut<MoralPrompt>,
    mut notice: ResMut<WorldNotice>,
    mut panels: MessageWriter<OpenPanel>,
    mut sound: MessageWriter<SfxEvent>,
    lang: Res<Language>,
    selected: Res<SelectedRole>,
    env: Res<ShelterEnvironment>,
    obstacles: Res<Obstacles>,
    civilians: Query<&Transform, With<crate::human::Civilian>>,
) {
    let (mut campaign, mut progress, mut inventory, mut shelter, mut needs, mut game) = states;
    if flow.get() != &GameFlow::Playing || pause.0 || game.hp <= 0.0 || game.over {
        actions.clear();
        return;
    }
    let Ok(transform) = player.single() else {
        actions.clear();
        return;
    };
    let pos = transform.translation.xz();
    let night = is_night(f64::from(campaign.game_min));
    for action in actions.read() {
        match action {
            WorldAction::FillWater => {
                notice.text = match fill_water(
                    pos,
                    &catalog,
                    &mut progress,
                    &mut inventory,
                    campaign.game_min,
                    env.water,
                    Some(&obstacles),
                ) {
                    Ok(()) => tr(
                        *lang,
                        "裝滿了一瓶飲用水。",
                        "Filled a bottle of drinking water.",
                    )
                    .into(),
                    Err(error) => error.message(*lang).into(),
                };
            }
            WorldAction::Interact => {
                if prompt.0.is_some() {
                    continue;
                }
                let target = interaction_target(pos, &catalog, &progress, &shelter, &obstacles);
                if let InteractionTarget::Trade = target {
                    panels.write(OpenPanel(PanelKind::Trade));
                    continue;
                }
                if let InteractionTarget::Door(target) = target {
                    let door = world().blds[target.id as usize].door.unwrap();
                    let occupants = std::iter::once((pos, crate::player::PLAYER_RADIUS))
                        .chain(
                            enemies
                                .iter()
                                .filter(|e| e.0.alive())
                                .map(|e| (e.0.pos, 0.4)),
                        )
                        .chain(civilians.iter().map(|c| (c.translation.xz(), 0.35)));
                    if target.open && !crate::doors::can_close(door, occupants) {
                        notice.text = tr(
                            *lang,
                            "門口有人，先退開再關門。",
                            "The doorway is occupied. Step away before closing it.",
                        )
                        .into();
                    } else {
                        obstacles.doors().set_open(target.id, !target.open);
                        progress.closed_doors = obstacles.doors().closed_ids();
                        notice.text = tr(
                            *lang,
                            if target.open {
                                "關上了門。"
                            } else {
                                "打開了門。"
                            },
                            if target.open {
                                "Door closed."
                            } else {
                                "Door opened."
                            },
                        )
                        .into();
                        sound.write(SfxEvent::Clink {
                            dist: target.distance,
                        });
                    }
                    continue;
                }
                if let InteractionTarget::Container(c) = target {
                    if c.id == HOME_TAP_ID {
                        notice.text = match fill_water(
                            pos,
                            &catalog,
                            &mut progress,
                            &mut inventory,
                            campaign.game_min,
                            env.water,
                            Some(&obstacles),
                        ) {
                            Ok(()) => tr(
                                *lang,
                                "裝滿了一瓶飲用水。",
                                "Filled a bottle of drinking water.",
                            )
                            .into(),
                            Err(error) => error.message(*lang).into(),
                        };
                        continue;
                    }
                    if c.cabinet
                        && id_hash(&c.id).is_multiple_of(5)
                        && !progress.asked.contains(&c.id)
                    {
                        prompt.0 = Some(c.id);
                        continue;
                    }
                    let total =
                        loot_container(&c, &mut progress, &mut inventory, night, campaign.game_min);
                    notice.text = if total > 0 {
                        format!(
                            "{} {total}",
                            tr(*lang, "物資已放入背包：", "Supplies collected:")
                        )
                    } else {
                        tr(*lang, "這裡已經沒有物資。", "No supplies left here.").into()
                    };
                    sound.write(SfxEvent::Rummage);
                } else if matches!(target, InteractionTarget::Shelter) {
                    panels.write(OpenPanel(PanelKind::Shelter));
                } else {
                    notice.text = tr(
                        *lang,
                        "走近門口、屋內櫥櫃、遺留物或交易點再互動。",
                        "Move closer to an entrance, cabinet, dropped supplies or trader.",
                    )
                    .into();
                }
            }
            WorldAction::Moral(choice) => {
                let Some(id) = prompt.0.as_ref() else {
                    continue;
                };
                let Some(c) = catalog.containers.iter().find(|c| &c.id == id) else {
                    prompt.0 = None;
                    continue;
                };
                if c.pos.distance(pos) >= 2.2 {
                    prompt.0 = None;
                    continue;
                }
                match shelter.moral(*choice, &mut inventory, *lang) {
                    Ok(message) => {
                        if !progress.asked.contains(&c.id) {
                            progress.asked.push(c.id.clone());
                        }
                        notice.text = message;
                        if *choice == MoralChoice::Take {
                            if !progress.night_seen.contains(&c.id) {
                                progress.night_seen.push(c.id.clone());
                            }
                            loot_container(
                                c,
                                &mut progress,
                                &mut inventory,
                                night,
                                campaign.game_min,
                            );
                        }
                        prompt.0 = None;
                    }
                    Err(error) => notice.text = error.message(*lang).into(),
                }
            }
            WorldAction::Use(id) => {
                let mut vitals = Vitals {
                    hp: game.hp,
                    hunger: needs.hunger,
                    thirst: needs.thirst,
                    fatigue: needs.fatigue,
                    bleeding: game.bleed,
                };
                let risk = next(&mut progress.weather.seed) as f32;
                let medic = if selected.role().skills.medic > 0 {
                    1.5
                } else {
                    1.0
                };
                match inventory.use_item(id, &mut vitals, medic, risk) {
                    Ok(result) => {
                        game.hp = vitals.hp;
                        game.bleed = vitals.bleeding;
                        needs.hunger = vitals.hunger;
                        needs.thirst = vitals.thirst;
                        needs.fatigue = vitals.fatigue;
                        notice.text = match result {
                            UseResult::Cooked(out) => format!(
                                "{} {}",
                                tr(*lang, "煮好了：", "Cooked:"),
                                item(out).unwrap().name(*lang)
                            ),
                            UseResult::Equipped(on) => tr(
                                *lang,
                                if on { "已裝備。" } else { "已收起。" },
                                if on { "Equipped." } else { "Unequipped." },
                            )
                            .into(),
                            UseResult::Unloaded { count, .. } => {
                                format!("{} {count}", tr(*lang, "已卸出子彈：", "Rounds unloaded:"))
                            }
                            UseResult::Consumed { upset: true } => tr(
                                *lang,
                                "吃壞肚子，失去體力和水分。",
                                "An upset stomach costs health and water.",
                            )
                            .into(),
                            UseResult::Consumed { upset: false } => {
                                tr(*lang, "已使用。", "Used.").into()
                            }
                        };
                    }
                    Err(error) => notice.text = error.message(*lang).into(),
                }
            }
            WorldAction::Sleep => {
                if campaign.sleeping {
                    campaign.sleeping = false;
                    notice.text = tr(*lang, "你醒了。", "You wake up.").into();
                } else if world().bld_at(pos).is_none() {
                    notice.text =
                        tr(*lang, "只能在室內睡覺。", "You can only sleep indoors.").into();
                } else if danger_near(pos, &enemies, night) {
                    notice.text = tr(
                        *lang,
                        "附近有佔領軍，現在睡太危險。",
                        "Occupation troops nearby. Too dangerous to sleep.",
                    )
                    .into();
                } else if needs.fatigue < 20.0 {
                    notice.text = tr(*lang, "你還不累。", "You are not tired yet.").into();
                } else {
                    campaign.sleeping = true;
                    notice.text = tr(
                        *lang,
                        "你閉上眼睛，時間快轉 20 倍。",
                        "You close your eyes. Time advances at 20x.",
                    )
                    .into();
                }
            }
            WorldAction::Speed(speed) => {
                if [1, 3, 20].contains(speed) {
                    campaign.sleeping = false;
                    campaign.speed = if danger_near(pos, &enemies, night) {
                        1
                    } else {
                        *speed
                    };
                }
            }
        }
    }
}

fn regen_rate(needs: &PlayerNeeds, bleeding: bool) -> f32 {
    if bleeding || needs.hunger <= 50.0 || needs.thirst <= 50.0 {
        0.0
    } else if needs.hunger >= 80.0 && needs.thirst >= 80.0 {
        18.0
    } else {
        6.0
    }
}

fn weather_before_dawn(
    time: Res<Time>,
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    frame: Res<FrameProgress>,
    campaign: Res<Campaign>,
    mut progress: ResMut<ScavengeState>,
    mut env: ResMut<ShelterEnvironment>,
    mut notice: ResMut<WorldNotice>,
    lang: Res<Language>,
) {
    if flow.get() != &GameFlow::Playing || pause.0 {
        return;
    }
    let minute = campaign.game_min;
    let gm = ((minute - frame.minute.unwrap_or(minute)) / 60.0).max(0.0);
    let weather = &mut progress.weather;
    if minute >= weather.next && minute >= weather.until {
        let storm = (13.0..19.0).contains(&(minute / 60.0 % 24.0));
        weather.intensity = if storm {
            0.82 + next(&mut weather.seed) as f32 * 0.18
        } else {
            0.3 + next(&mut weather.seed) as f32 * 0.25
        };
        weather.until = minute
            + 60.0
                * if storm {
                    1.0 + next(&mut weather.seed) as f32 * 1.5
                } else {
                    2.0 + next(&mut weather.seed) as f32 * 4.0
                };
        weather.next = weather.until + 60.0 * (8.0 + next(&mut weather.seed) as f32 * 28.0);
        notice.text = tr(
            *lang,
            "台北下起雨，集雨設施會收集雨水。",
            "Rain begins in Taipei. Rain collectors will gather water.",
        )
        .into();
    }
    let raining = minute < weather.until;
    weather.amount += ((if raining { weather.intensity } else { 0.0 }) - weather.amount)
        * (time.delta_secs() * 0.5).min(1.0);
    weather.wet = (weather.wet
        + if raining {
            gm * 1.5 * (0.4 + 0.6 * weather.intensity)
        } else {
            -gm * 0.25
        })
    .clamp(0.0, 1.0);
    env.raining = weather.amount > 0.2;
    if minute >= POWER_OFF as f32 {
        env.power = false;
    }
    if minute >= WATER_OFF as f32 {
        env.water = false;
    }
}

fn tick_needs_weather(
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    mut frame: ResMut<FrameProgress>,
    mut campaign: ResMut<Campaign>,
    mut needs: ResMut<PlayerNeeds>,
    selected: Res<SelectedRole>,
    mut game: ResMut<GameState>,
    mut shelter: ResMut<Shelter>,
    mut inventory: ResMut<Inventory>,
    mut notice: ResMut<WorldNotice>,
    mut died: MessageWriter<PlayerDied>,
    lang: Res<Language>,
) {
    if flow.get() != &GameFlow::Playing || pause.0 || game.over {
        return;
    }
    let minute = campaign.game_min;
    let previous = frame.minute.replace(minute).unwrap_or(minute);
    let gm = ((minute - previous) / 60.0).max(0.0);
    needs.hunger = (needs.hunger - gm * 2.2).clamp(0.0, 100.0);
    needs.thirst = (needs.thirst - gm * 3.2).clamp(0.0, 100.0);
    needs.fatigue = (needs.fatigue
        + if campaign.sleeping {
            -gm * 14.0 * selected.role().skills.rest
        } else {
            gm * 2.6
        })
    .clamp(0.0, 100.0);
    if campaign.sleeping && needs.fatigue <= 0.0 {
        campaign.sleeping = false;
        notice.text = tr(*lang, "你醒了。", "You wake up.").into();
    }
    if needs.hunger <= 0.0 || needs.thirst <= 0.0 {
        game.hp -= gm * 8.0;
    }
    game.hp = (game.hp + gm * regen_rate(&needs, game.bleed)).min(100.0);
    if game.hp <= 0.0 {
        game.hp = 0.0;
        game.over = true;
        died.write(PlayerDied);
    }
    if shelter.day != frame.day {
        frame.day = shelter.day;
        if minute >= (POWER_OFF + 1440) as f32 {
            let spoiled = inventory.spoil() + shelter.stash.spoil();
            if spoiled > 0 {
                notice.text = format!(
                    "{} {spoiled}",
                    tr(
                        *lang,
                        "停電後新鮮食物腐壞：",
                        "Fresh food spoiled after the blackout:"
                    )
                );
            }
        }
    }
}

fn remember_deaths(mut died: MessageReader<EnemyDied>, mut progress: ResMut<ScavengeState>) {
    for death in died.read() {
        if progress.dead.contains(&death.id) {
            continue;
        }
        progress.dead.push(death.id);
        let id = format!("body:{}", death.id);
        let mut seed = id_hash(&id).wrapping_add(20_261_024);
        let items = crate::inventory::soldier_drop(death.id, &mut || next(&mut seed));
        progress.loot.push((id.clone(), items));
        progress.drops.push((id.clone(), death.pos.xz().to_array()));
        progress.night_seen.push(id);
    }
}

fn update_environment(
    campaign: Res<Campaign>,
    shelter: Res<Shelter>,
    progress: Res<ScavengeState>,
    player: Query<&Transform, With<Player>>,
    mut combat: ResMut<CombatEnv>,
    mut ambience: ResMut<Ambience>,
) {
    let night = is_night(f64::from(campaign.game_min));
    combat.night = night;
    combat.rain = progress.weather.amount;
    ambience.rain = progress.weather.amount;
    ambience.night = night;
    if let Ok(p) = player.single() {
        ambience.indoor = world().bld_at(p.translation.xz()).is_some();
        ambience.home = shelter.at_home(p.translation.xz().to_array());
    }
}

fn update_evac(
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    game: Res<GameState>,
    mut campaign: ResMut<Campaign>,
    mut sites: ResMut<EvacSites>,
    player: Query<&Transform, With<Player>>,
    mut completed: MessageWriter<EvacComplete>,
) {
    sites.heli_open =
        flow.get() == &GameFlow::Playing && evac_open(campaign.game_min, campaign.evac_taken);
    // Boat contact/informant progression is not yet native; its marker must not promise an available extraction.
    sites.boat_visible = false;
    if !sites.heli_open || pause.0 || game.over || game.hp <= 0.0 {
        return;
    }
    if let Ok(p) = player.single()
        && p.translation.xz().distance(sites.heli) < EvacSites::HELI_RADIUS
    {
        campaign.evac_taken = true;
        completed.write(EvacComplete {
            route: EvacRoute::Helicopter,
        });
    }
}

fn stream_loot(
    mut commands: Commands,
    catalog: Res<WorldCatalog>,
    progress: Res<ScavengeState>,
    art: Res<LootArt>,
    player: Query<&Transform, With<Player>>,
    markers: Query<(Entity, &LootMarker, &Transform)>,
) {
    let Ok(p) = player.single() else {
        return;
    };
    let pos = p.translation.xz();
    let mut visible: HashSet<String> = HashSet::new();
    for (entity, marker, tf) in &markers {
        if tf.translation.xz().distance(pos) > 75.0 {
            commands.entity(entity).despawn();
        } else {
            visible.insert(marker.0.clone());
            let empty = progress.searched.contains(&marker.0);
            commands
                .entity(entity)
                .insert(MeshMaterial3d(if marker.0 == HOME_TAP_ID {
                    art.water.clone()
                } else if empty {
                    art.empty.clone()
                } else {
                    art.full.clone()
                }));
        }
    }
    let mut spawn = |id: &str, q: Vec2| {
        if q.distance(pos) < 65.0 && !visible.contains(id) {
            commands.spawn((
                LootMarker(id.to_string()),
                Mesh3d(art.mesh.clone()),
                MeshMaterial3d(if id == HOME_TAP_ID {
                    art.water.clone()
                } else if progress.searched.iter().any(|s| s == id) {
                    art.empty.clone()
                } else {
                    art.full.clone()
                }),
                Transform::from_xyz(q.x, 0.325, q.y),
                Visibility::default(),
            ));
        }
    };
    for c in &catalog.containers {
        spawn(&c.id, c.pos);
    }
    for (id, p) in &progress.drops {
        spawn(id, Vec2::from_array(*p));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_arcade_door_blocks_water_and_nearest_target_tracks_the_real_action() {
        let map = world();
        let obstacles = Obstacles::new(map.clone());
        let (id, door) = map
            .blds
            .iter()
            .filter(|b| b.arcade.is_some())
            .filter_map(|b| b.door.map(|d| (b.id, d)))
            .find(|(id, d)| {
                let outside = d.p + d.n * 0.6;
                let inside = d.p - d.n * 1.4;
                map.bld_at(outside).is_some_and(|b| b.id == *id)
                    && map.bld_at(inside).is_some_and(|b| b.id == *id)
                    && obstacles.los(outside, inside)
            })
            .expect("a recessed arcade entrance with a clear water-fixture ray");
        let fixture = door.p - door.n * 1.4;
        let pos = door.p + door.n * 0.6;
        let catalog = WorldCatalog {
            containers: vec![Container {
                id: HOME_TAP_ID.into(),
                pos: fixture,
                kind: "tap",
                cabinet: false,
            }],
            patrols: vec![],
            home: fixture,
        };
        let mut progress = ScavengeState::default();
        let mut inventory = Inventory::default();
        let shelter = Shelter::default();
        obstacles.doors().set_open(id, false);
        assert!(
            matches!(interaction_target(pos, &catalog, &progress, &shelter, &obstacles), InteractionTarget::Door(d) if d.id == id && !d.open)
        );
        assert_eq!(
            fill_water(
                pos,
                &catalog,
                &mut progress,
                &mut inventory,
                500.0,
                true,
                Some(&obstacles)
            ),
            Err(TapError::TooFar)
        );
        assert_eq!(inventory.count("water"), 0);
        obstacles.doors().set_open(id, true);
        let inside = door.p - door.n * 1.1;
        assert!(
            matches!(interaction_target(inside, &catalog, &progress, &shelter, &obstacles), InteractionTarget::Container(c) if c.id == HOME_TAP_ID)
        );
        fill_water(
            inside,
            &catalog,
            &mut progress,
            &mut inventory,
            500.0,
            true,
            Some(&obstacles),
        )
        .unwrap();
        assert_eq!(inventory.count("water"), 1);
    }

    #[test]
    fn loading_before_dawn_restores_baseline_before_daily_spoilage() {
        use crate::game_flow::GameFlowPlugin;
        use crate::survival::{DawnTracker, ShelterPlugin};
        use bevy::{state::app::StatesPlugin, time::TimeUpdateStrategy};
        use std::time::Duration;

        // Use the actual plugin schedules without a window or renderer. The
        // resources/messages below stand in for presentation and input plugins.
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)))
            .init_resource::<Language>()
            .init_resource::<SelectedRole>()
            .init_resource::<GameState>()
            .init_resource::<CombatEnv>()
            .init_resource::<Ambience>()
            .init_resource::<GlobalAmbientLight>()
            .insert_resource(Obstacles::new(world().clone()))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::input::PlayerInput>()
            .init_resource::<PauseOpen>()
            .init_resource::<UiModal>()
            .insert_resource(world().plan_evac())
            .add_message::<combat::ResetCombat>()
            .add_message::<PlayerDied>()
            .add_message::<EnemyDied>()
            .add_message::<LoadCompleted>()
            .add_message::<OpenPanel>()
            .add_message::<crate::ui::TogglePause>()
            .add_message::<SfxEvent>()
            .add_message::<bevy::window::WindowFocused>()
            .add_message::<bevy::window::AppLifecycle>()
            .add_plugins((GameFlowPlugin, ShelterPlugin, WorldPlayPlugin));
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(Duration::from_secs(1));
        app.world_mut().spawn((Player, Transform::default()));
        app.update();

        // SavePlugin::restore finishes in Last. These are the restored values
        // and notification observed by the following frame's real schedules.
        let saved_minute = 4.0 * 1440.0 + 360.0 - 0.01;
        app.insert_resource(Campaign {
            game_min: saved_minute,
            ..default()
        });
        app.insert_resource(DawnTracker::from_minutes(saved_minute));
        app.world_mut().resource_mut::<Shelter>().day = 4;
        app.world_mut().resource_mut::<Inventory>().add("bento", 3);
        app.world_mut().resource_mut::<GameState>().over = false;
        app.world_mut()
            .resource_mut::<NextState<GameFlow>>()
            .set(GameFlow::Playing);
        app.world_mut().write_message(LoadCompleted);
        app.update();

        assert_eq!(app.world().resource::<Shelter>().day, 5);
        let inventory = app.world().resource::<Inventory>();
        assert_eq!(inventory.count("bento"), 1);
        assert_eq!(inventory.count("rotten"), 2);
        app.update();
        assert_eq!(app.world().resource::<Shelter>().day, 5);
        let inventory = app.world().resource::<Inventory>();
        assert_eq!(inventory.count("bento"), 1);
        assert_eq!(inventory.count("rotten"), 2);
    }

    #[test]
    fn tap_requires_nearby_fixture_running_water_and_saved_strict_cooldown() {
        let fixture = Container {
            id: HOME_TAP_ID.into(),
            pos: Vec2::ZERO,
            kind: "tap",
            cabinet: false,
        };
        let catalog = WorldCatalog {
            containers: vec![fixture],
            patrols: Vec::new(),
            home: Vec2::ZERO,
        };
        let mut progress = ScavengeState::default();
        let mut inventory = Inventory::default();
        assert_eq!(
            fill_water(
                Vec2::new(3.0, 0.0),
                &catalog,
                &mut progress,
                &mut inventory,
                440.0,
                true,
                None,
            ),
            Err(TapError::TooFar)
        );
        assert_eq!(
            fill_water(
                Vec2::ZERO,
                &catalog,
                &mut progress,
                &mut inventory,
                440.0,
                false,
                None,
            ),
            Err(TapError::WaterOff)
        );
        fill_water(
            Vec2::ZERO,
            &catalog,
            &mut progress,
            &mut inventory,
            440.0,
            true,
            None,
        )
        .unwrap();
        assert_eq!(inventory.count("water"), 1);
        let bytes = serde_json::to_vec(&progress).unwrap();
        let mut loaded: ScavengeState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            fill_water(
                Vec2::ZERO,
                &catalog,
                &mut loaded,
                &mut inventory,
                530.0,
                true,
                None,
            ),
            Err(TapError::Cooldown)
        );
        fill_water(
            Vec2::ZERO,
            &catalog,
            &mut loaded,
            &mut inventory,
            530.1,
            true,
            None,
        )
        .unwrap();
        assert_eq!(inventory.count("water"), 2);
        assert_eq!(
            fill_water(
                Vec2::ZERO,
                &catalog,
                &mut loaded,
                &mut inventory,
                WATER_OFF as f32,
                false,
                None,
            ),
            Err(TapError::WaterOff)
        );
        assert_eq!(inventory.count("water"), 2);
    }

    #[test]
    fn original_loot_and_late_spoilage_match_source_fixtures() {
        #[derive(Deserialize)]
        struct Fixture {
            cases: Vec<Case>,
        }
        #[derive(Deserialize)]
        struct Case {
            kind: String,
            minute: f32,
            initial_seed: u32,
            items: Stash,
            final_seed: u32,
        }
        let fixture: Fixture =
            serde_json::from_str(include_str!("../tests/fixtures/world-loot-reference.json"))
                .unwrap();
        assert_eq!(fixture.cases.len(), 456);
        for case in fixture.cases {
            let mut seed = case.initial_seed;
            let actual = roll_loot_with(&case.kind, case.minute, &mut seed);
            assert_eq!(
                actual, case.items,
                "{} at {} with seed {}",
                case.kind, case.minute, case.initial_seed
            );
            assert_eq!(
                seed, case.final_seed,
                "RNG call order: {} at {} with seed {}",
                case.kind, case.minute, case.initial_seed
            );
        }
    }

    #[test]
    fn original_initial_needs_and_healing_thresholds() {
        let mut needs = PlayerNeeds::default();
        assert_eq!(
            (needs.hunger, needs.thirst, needs.fatigue),
            (85.0, 85.0, 10.0)
        );
        assert_eq!(regen_rate(&needs, false), 18.0);
        needs.hunger = 79.0;
        assert_eq!(regen_rate(&needs, false), 6.0);
        needs.thirst = 50.0;
        assert_eq!(regen_rate(&needs, false), 0.0);
        needs.thirst = 100.0;
        assert_eq!(regen_rate(&needs, true), 0.0);
    }

    #[test]
    fn looted_container_cannot_refill_after_serialization() {
        let c = Container {
            id: "cabinet-12".into(),
            pos: Vec2::ZERO,
            kind: "home",
            cabinet: true,
        };
        let mut state = ScavengeState::default();
        let mut bag = Inventory::default();
        loot_container(&c, &mut state, &mut bag, true, 440.0);
        let encoded = serde_json::to_vec(&state).unwrap();
        let mut loaded: ScavengeState = serde_json::from_slice(&encoded).unwrap();
        let before = bag.clone();
        assert_eq!(loot_container(&c, &mut loaded, &mut bag, true, 440.0), 0);
        assert_eq!(bag, before);
    }

    #[test]
    fn loot_is_deterministic_and_catalogued() {
        for kind in ["home", "store", "pharmacy", "armory"] {
            for id in 0..64 {
                let key = format!("c{id}");
                let a = roll_loot(&key, kind, 440.0);
                assert_eq!(a, roll_loot(&key, kind, 440.0));
                assert!(
                    a.stacks()
                        .iter()
                        .all(|(id, count)| item(id).is_some() && *count > 0)
                );
            }
        }
    }
}
