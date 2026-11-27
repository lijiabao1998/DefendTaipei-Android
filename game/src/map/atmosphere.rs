//! Bounded native version of the Web alley / lit-window / rain presentation.
//!
//! Street lamps, arcade fixtures and occupied windows use separate shared emission.
//! Six shadowless spot lights follow original world sources, with indexed selection and
//! dark handoffs. Rain has one reusable mesh, capped at 240 streaks, and does not fall
//! through roofs or arcades. Gameplay still owns weather, power and the clock.

pub mod lighting;
pub use lighting::{
    CITY_LIGHT_COUNT, FRONTAGE_LIGHT_COUNT, LIGHT_ENTER_RADIUS, LIGHT_EXIT_RADIUS, LIGHT_GRID_CELL,
    LIGHT_RETAIN_ADVANTAGE, LightKind, LightSelection, LightSite, LightSiteIndex, LightSource,
    STREET_LIGHT_COUNT, arcade_lamp_count, arcade_lamp_position, plan_city_lights,
};

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use super::{textures::MapMaterials, world};
use crate::{
    game_flow::{Campaign, GameFlow},
    player::Player,
    survival::ShelterEnvironment,
    ui::PauseOpen,
    worldplay::{ScavengeState, WorldSet},
};

const LIGHT_REFRESH_SECONDS: f32 = 0.5;
const LIGHT_FADE_SECONDS: f32 = 0.18;
pub const MAX_RAIN_STREAKS: usize = 240;
const RAIN_RADIUS: f32 = 28.0;

/// Render-state evidence used by native QA. These are actual submitted scene counts.
#[derive(Resource, Default, Debug)]
pub struct AtmosphereState {
    pub night: f32,
    pub power: bool,
    pub wetness: f32,
    pub active_lights: usize,
    pub rain_streaks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightingProfile {
    pub daylight: f32,
    pub night: f32,
    pub lamp_strength: f32,
    pub ambient: f32,
    pub sun: f32,
    pub wetness: f32,
}

fn smooth(x: f32) -> f32 {
    let k = x.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// Dawn 05:30–07:00, dusk 17:30–19:30; the simulation still owns its own night rules.
pub fn lighting_profile(minute: f32, power: bool, rain: f32, wet: f32) -> LightingProfile {
    let minute = minute.rem_euclid(1440.0);
    let daylight = smooth((minute - 330.0) / 90.0) * (1.0 - smooth((minute - 1050.0) / 120.0));
    let night = 1.0 - daylight;
    let rain = rain.clamp(0.0, 1.0);
    let night_ambient = if power { 260.0 } else { 95.0 };
    LightingProfile {
        daylight,
        night,
        lamp_strength: if power { night } else { 0.0 },
        ambient: (night_ambient + (600.0 - night_ambient) * daylight) * (1.0 - rain * 0.24),
        sun: (220.0 + 11780.0 * daylight) * (1.0 - rain * 0.65),
        wetness: wet.clamp(0.0, 1.0),
    }
}

#[derive(Resource)]
pub(crate) struct CityLightSites {
    pub sites: Vec<LightSite>,
    index: LightSiteIndex,
}

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct CitySpill {
    pub slot: usize,
    pub site: Option<u64>,
    /// Handoff gain only. Actual intensity also includes the night/power profile.
    pub gain: f32,
    target: Option<u64>,
}

impl CitySpill {
    fn new(slot: usize) -> Self {
        Self {
            slot,
            site: None,
            gain: 0.0,
            target: None,
        }
    }

    fn advance(&mut self, delta: f32) {
        let step = delta.max(0.0) / LIGHT_FADE_SECONDS;
        if self.site != self.target {
            self.gain = (self.gain - step).max(0.0);
            if self.gain == 0.0 {
                // Reposition only while dark, including one zero-intensity
                // submitted frame before the new source begins to fade in.
                self.site = self.target;
            }
        } else if self.site.is_some() {
            self.gain = (self.gain + step).min(1.0);
        } else {
            self.gain = 0.0;
        }
    }

    fn extinguish(&mut self, keep_identity: bool) {
        self.gain = 0.0;
        if !keep_identity {
            self.site = None;
        }
        self.target = self.site;
    }

    fn retire_distant(&mut self, sites: &CityLightSites, player: Vec2) -> bool {
        let nearby = |id| {
            sites.index.index_of(id).is_some_and(|index| {
                sites.sites[index].position.xz().distance(player) <= LIGHT_EXIT_RADIUS
            })
        };
        if self.site.is_some_and(|id| !nearby(id)) {
            self.extinguish(false);
            return true;
        }
        if self.target != self.site && self.target.is_some_and(|id| !nearby(id)) {
            self.target = self.site;
            return true;
        }
        false
    }
}

#[derive(Component)]
struct RainField;

#[derive(Resource)]
struct RainArt {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
struct AtmosphereTimers {
    lights: f32,
    rain: f32,
    elapsed: f32,
}

pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AtmosphereState>()
            .init_resource::<AtmosphereTimers>()
            .init_resource::<super::street::furnishings::SignalPhase>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    update_materials,
                    update_lights,
                    update_rain,
                    super::street::furnishings::update_signals,
                )
                    .chain()
                    .after(WorldSet),
            );
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let sites = plan_city_lights(world());
    let index = LightSiteIndex::new(&sites);
    commands.insert_resource(CityLightSites { sites, index });
    for slot in 0..CITY_LIGHT_COUNT {
        commands.spawn((
            CitySpill::new(slot),
            SpotLight {
                intensity: 0.0,
                range: 10.0,
                radius: 0.12,
                shadow_maps_enabled: false,
                contact_shadows_enabled: false,
                ..default()
            },
            Transform::default(),
        ));
    }
    let mesh = meshes.add(rain_mesh(Vec::new()));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.72, 0.79, 0.82, 0.38),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        RainField,
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        NoFrustumCulling,
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.insert_resource(RainArt { mesh, material });
}

fn session(flow: &GameFlow) -> bool {
    matches!(flow, GameFlow::Prologue | GameFlow::Playing)
}

#[allow(clippy::too_many_arguments)]
fn update_materials(
    flow: Res<State<GameFlow>>,
    campaign: Res<Campaign>,
    env: Res<ShelterEnvironment>,
    progress: Res<ScavengeState>,
    city: Res<MapMaterials>,
    rain: Res<RainArt>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut sun: Query<&mut DirectionalLight>,
    mut evidence: ResMut<AtmosphereState>,
    mut previous: Local<Option<(LightingProfile, bool)>>,
) {
    let live = session(flow.get());
    let p = if live {
        lighting_profile(
            campaign.game_min,
            env.power,
            progress.weather.amount,
            progress.weather.wet,
        )
    } else {
        lighting_profile(480.0, true, 0.0, 0.0)
    };
    let power = !live || env.power;
    if *previous == Some((p, power)) {
        return;
    }
    *previous = Some((p, power));
    evidence.night = p.night;
    evidence.power = power;
    evidence.wetness = p.wetness;
    ambient.brightness = p.ambient;
    ambient.color = Color::srgb(
        0.55 + 0.26 * p.daylight,
        0.65 + 0.24 * p.daylight,
        0.84 + 0.16 * p.daylight,
    );
    for mut light in &mut sun {
        light.illuminance = p.sun;
        light.color = Color::srgb(
            0.58 + 0.42 * p.daylight,
            0.67 + 0.26 * p.daylight,
            0.9 - 0.1 * p.daylight,
        );
    }
    for (handle, rgb) in [
        (&city.lamp, [2.4, 1.85, 1.1]),
        (&city.window_glow, [0.85, 0.58, 0.30]),
        (&city.arcade_glow, [1.45, 1.6, 1.75]),
    ] {
        if let Some(mut m) = materials.get_mut(handle) {
            let strength = p.lamp_strength;
            // Keep the previous daylight base. Independent shared emission adds
            // neither geometry nor a light entity for each occupied window.
            let glass = 0.28 + 0.46 * strength;
            m.base_color = Color::srgb(glass, glass, glass);
            m.emissive = LinearRgba::rgb(strength * rgb[0], strength * rgb[1], strength * rgb[2]);
        }
    }
    for (handle, gain) in [(&city.signs, 0.95), (&city.vsigns, 1.2)] {
        if let Some(mut m) = materials.get_mut(handle) {
            let k = p.lamp_strength * gain;
            m.emissive = LinearRgba::rgb(k, k, k);
        }
    }
    for (handle, dry, change, darken) in [
        (&city.road, 0.95, 0.42, 0.16),
        (&city.walk, 0.9, 0.27, 0.12),
        (&city.curb, 0.85, 0.24, 0.1),
        (&city.ground, 1.0, 0.22, 0.11),
        (&city.marking, 0.8, 0.27, 0.0),
    ] {
        if let Some(mut m) = materials.get_mut(handle) {
            m.perceptual_roughness = dry - change * p.wetness;
            let k = 1.0 - darken * p.wetness;
            m.base_color = Color::srgb(k, k, k);
        }
    }
    if let Some(mut m) = materials.get_mut(&rain.material) {
        let k = 0.35 + 0.65 * p.daylight;
        m.base_color = Color::srgba(0.72 * k, 0.79 * k, 0.82 * k, 0.38);
    }
}

/// A stable up vector also handles the straight-down arcade cones. Using Y as
/// both direction and up would make that rotation undefined.
fn source_transform(site: &LightSite) -> Transform {
    let up = if site.direction.dot(Vec3::Y).abs() > 0.95 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    Transform::from_translation(site.position).looking_to(site.direction, up)
}

#[allow(clippy::too_many_arguments)]
fn update_lights(
    time: Res<Time>,
    flow: Res<State<GameFlow>>,
    campaign: Res<Campaign>,
    env: Res<ShelterEnvironment>,
    sites: Res<CityLightSites>,
    player: Query<&Transform, (With<Player>, Without<CitySpill>)>,
    mut lights: Query<(&mut CitySpill, &mut SpotLight, &mut Transform), Without<Player>>,
    mut timers: ResMut<AtmosphereTimers>,
    mut evidence: ResMut<AtmosphereState>,
) {
    let live = session(flow.get());
    let strength = if live {
        lighting_profile(campaign.game_min, env.power, 0.0, 0.0).lamp_strength
    } else {
        0.0
    };
    let player = player
        .single()
        .ok()
        .map(|transform| transform.translation.xz())
        .filter(|position| position.is_finite());
    // Supply loss is immediate, independently of both the spatial timer and
    // handoff fade. Existing source identities can restore after a blackout.
    if strength <= 0.001 || player.is_none() {
        for (mut slot, mut light, _) in &mut lights {
            slot.extinguish(live && player.is_some());
            light.intensity = 0.0;
        }
        evidence.active_lights = 0;
        timers.lights = 0.0;
        return;
    }
    let player = player.expect("active lighting has a player");
    // Fixed-pool ID lookups also enforce locality between index refreshes. A
    // teleport must not leave the previous neighborhood lit for another tick.
    for (mut slot, mut light, _) in &mut lights {
        if slot.retire_distant(&sites, player) {
            light.intensity = 0.0;
            timers.lights = 0.0;
        }
    }
    timers.lights -= time.delta_secs();
    if timers.lights <= 0.0 {
        timers.lights = LIGHT_REFRESH_SECONDS;
        let mut current = [None; CITY_LIGHT_COUNT];
        for (slot, _, _) in &lights {
            current[slot.slot] = slot.site;
        }
        let desired = sites.index.select(&sites.sites, player, &current);
        for (mut slot, _, _) in &mut lights {
            slot.target = desired[slot.slot];
        }
    }
    evidence.active_lights = 0;
    for (mut slot, mut light, mut transform) in &mut lights {
        slot.advance(time.delta_secs());
        let source = slot.site.and_then(|id| sites.index.index_of(id));
        if let Some(index) = source {
            let site = &sites.sites[index];
            // The assignment only changes at gain zero. In normal operation
            // these values are stable; only six bounded intensities ramp.
            *transform = source_transform(site);
            light.color = site.color;
            light.range = site.range;
            light.inner_angle = site.inner_angle;
            light.outer_angle = site.outer_angle;
            light.intensity = strength * site.intensity * slot.gain;
            if light.intensity > 0.0 {
                evidence.active_lights += 1;
            }
        } else {
            light.intensity = 0.0;
        }
    }
}

fn hash(index: usize, salt: u32) -> f32 {
    let mut n = (index as u32).wrapping_mul(747_796_405).wrapping_add(salt);
    n = ((n >> ((n >> 28) + 4)) ^ n).wrapping_mul(277_803_737);
    ((n >> 22) ^ n) as f32 / u32::MAX as f32
}

/// Stable local rain, no particles under roof/arcade footprints. Vertices carry real
/// world coordinates so the single draw does not need one entity per drop.
fn rain_positions(center: Vec2, elapsed: f32, amount: f32) -> Vec<[f32; 3]> {
    let count = (MAX_RAIN_STREAKS as f32 * amount.clamp(0.0, 1.0)).round() as usize;
    let mut vertices = Vec::with_capacity(count * 4);
    let w = world();
    let right = Vec3::new(0.707, 0.0, -0.707) * 0.018;
    for i in 0..count {
        let x = (hash(i, 517) * 2.0 - 1.0) * RAIN_RADIUS;
        let z = (hash(i, 931) * 2.0 - 1.0) * RAIN_RADIUS;
        let y =
            (hash(i, 227) * 13.0 - elapsed * (9.0 + hash(i, 761) * 3.0)).rem_euclid(13.0) + 0.35;
        let p = Vec3::new(center.x + x + y * 0.08, y, center.y + z - y * 0.04);
        if w.bld_at(p.xz()).is_some() {
            continue;
        }
        let tail = p + Vec3::new(0.055, 0.68 + hash(i, 349) * 0.34, -0.025);
        vertices.extend([
            (p - right).to_array(),
            (p + right).to_array(),
            (tail - right).to_array(),
            (tail + right).to_array(),
        ]);
    }
    vertices
}

fn rain_mesh(positions: Vec<[f32; 3]>) -> Mesh {
    let count = positions.len();
    let mut indices = Vec::with_capacity(count / 4 * 6);
    for b in (0..count as u32).step_by(4) {
        indices.extend([b, b + 1, b + 2, b + 1, b + 3, b + 2]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.707, 0.0, 0.707]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; count])
    .with_inserted_indices(Indices::U32(indices))
}

#[allow(clippy::too_many_arguments)]
fn update_rain(
    time: Res<Time>,
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    progress: Res<ScavengeState>,
    art: Res<RainArt>,
    player: Query<&Transform, With<Player>>,
    mut rain: Query<&mut Visibility, With<RainField>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut timers: ResMut<AtmosphereTimers>,
    mut evidence: ResMut<AtmosphereState>,
) {
    let amount = if session(flow.get()) {
        progress.weather.amount
    } else {
        0.0
    };
    if amount <= 0.01 {
        for mut visibility in &mut rain {
            *visibility = Visibility::Hidden;
        }
        evidence.rain_streaks = 0;
        timers.rain = 0.0;
        return;
    }
    if !pause.0 {
        timers.elapsed += time.delta_secs();
    }
    timers.rain -= time.delta_secs();
    if timers.rain > 0.0 {
        return;
    }
    timers.rain = 1.0 / 30.0;
    let Ok(player) = player.single() else { return };
    let positions = rain_positions(player.translation.xz(), timers.elapsed, amount);
    evidence.rain_streaks = positions.len() / 4;
    if let Some(mut mesh) = meshes.get_mut(&art.mesh) {
        *mesh = rain_mesh(positions);
    }
    for mut visibility in &mut rain {
        *visibility = Visibility::Visible;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_night_power_and_wetness_follow_game_state() {
        let day = lighting_profile(12.0 * 60.0, true, 0.0, 0.0);
        let dusk = lighting_profile(18.5 * 60.0, true, 0.0, 0.0);
        let night = lighting_profile(22.0 * 60.0, true, 0.0, 0.0);
        let blackout = lighting_profile(22.0 * 60.0, false, 0.0, 0.0);
        let wet = lighting_profile(12.0 * 60.0, true, 0.8, 1.0);
        assert_eq!(day.lamp_strength, 0.0);
        assert_eq!(night.lamp_strength, 1.0);
        assert_eq!(blackout.lamp_strength, 0.0);
        assert!(day.ambient > dusk.ambient && dusk.ambient > night.ambient);
        assert!(night.ambient > blackout.ambient);
        assert!(wet.sun < day.sun && wet.ambient < day.ambient);
        assert_eq!(wet.wetness, 1.0);
        assert_eq!(
            lighting_profile(3.0 * 1440.0 + 720.0, true, 0.0, 0.0).sun,
            day.sun
        );
        for minute in 1..1440 {
            let a = lighting_profile(minute as f32 - 0.01, true, 0.0, 0.0);
            let b = lighting_profile(minute as f32 + 0.01, true, 0.0, 0.0);
            assert!(
                (a.ambient - b.ambient).abs() < 0.2,
                "abrupt lighting at {minute}"
            );
        }
    }

    #[test]
    fn light_handoffs_reposition_only_while_dark_and_power_loss_is_immediate() {
        let mut slot = CitySpill::new(0);
        slot.target = Some(11);
        slot.advance(1.0 / 60.0);
        assert_eq!(slot.site, Some(11));
        assert_eq!(slot.gain, 0.0);
        slot.advance(LIGHT_FADE_SECONDS);
        assert_eq!(slot.gain, 1.0);
        slot.target = Some(12);
        slot.advance(LIGHT_FADE_SECONDS * 0.5);
        assert_eq!(slot.site, Some(11));
        assert!((slot.gain - 0.5).abs() < 0.00001);
        slot.advance(LIGHT_FADE_SECONDS);
        assert_eq!(slot.site, Some(12));
        assert_eq!(slot.gain, 0.0);
        slot.advance(LIGHT_FADE_SECONDS);
        assert_eq!(slot.gain, 1.0);
        slot.extinguish(true);
        assert_eq!(slot.gain, 0.0);
        assert_eq!(slot.site, Some(12));
        slot.advance(LIGHT_FADE_SECONDS);
        assert_eq!(slot.gain, 1.0);
        slot.extinguish(false);
        assert_eq!(slot.gain, 0.0);
        assert_eq!(slot.site, None);
        assert_eq!(slot.target, None);
    }

    #[test]
    fn every_spot_rotation_points_to_its_receiver_including_vertical_arcade_lights() {
        for site in plan_city_lights(world()) {
            let transform = source_transform(&site);
            assert!(transform.rotation.is_finite());
            assert!((transform.rotation * Vec3::NEG_Z).distance(site.direction) < 0.00001);
            assert_eq!(transform.translation, site.position);
        }
    }

    #[test]
    fn crossing_the_exit_radius_retires_active_and_pending_sources_immediately() {
        let planned = plan_city_lights(world());
        let first = planned[0];
        let sites = CityLightSites {
            index: LightSiteIndex::new(&planned),
            sites: planned,
        };
        let mut slot = CitySpill::new(0);
        slot.site = Some(first.id);
        slot.target = slot.site;
        slot.gain = 1.0;
        let inside = first.position.xz() + Vec2::X * (LIGHT_EXIT_RADIUS - 0.1);
        assert!(!slot.retire_distant(&sites, inside));
        assert_eq!(slot.gain, 1.0);
        let outside = first.position.xz() + Vec2::X * (LIGHT_EXIT_RADIUS + 0.1);
        assert!(slot.retire_distant(&sites, outside));
        assert_eq!(slot.gain, 0.0);
        assert_eq!(slot.site, None);
        assert_eq!(slot.target, None);
        slot.target = Some(first.id);
        assert!(slot.retire_distant(&sites, outside));
        assert_eq!(slot.target, None);
    }

    #[test]
    fn rain_is_deterministic_bounded_and_excludes_roofs() {
        let center = world().spawn;
        assert!(rain_positions(center, 0.0, 0.0).is_empty());
        let a = rain_positions(center, 0.0, 1.0);
        let b = rain_positions(center, 0.0, 1.0);
        let c = rain_positions(center, 0.2, 1.0);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.len() >= 40 && a.len() <= MAX_RAIN_STREAKS * 4);
        for drop in a.as_chunks::<4>().0 {
            let p = (Vec3::from(drop[0]) + Vec3::from(drop[1])) / 2.0;
            assert!(p.is_finite());
            assert!(world().bld_at(p.xz()).is_none());
            assert!(p.xz().distance(center) < RAIN_RADIUS * 1.5);
        }
    }
}
