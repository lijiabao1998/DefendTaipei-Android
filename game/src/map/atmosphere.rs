//! Bounded native version of the Web alley / lit-window / rain presentation.
//!
//! All lamps share the existing city materials; only six shadowless spill lights follow
//! the player. Rain has one reusable mesh, capped at 240 streaks, and does not fall through
//! roofs or arcades. Weather, power and the clock remain owned by the gameplay systems.

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use super::{street::PropKind, textures::MapMaterials, world};
use crate::{
    game_flow::{Campaign, GameFlow},
    player::Player,
    survival::ShelterEnvironment,
    ui::PauseOpen,
    worldplay::{ScavengeState, WorldSet},
};

const STREET_LIGHTS: usize = 4;
const SIGN_LIGHTS: usize = 2;
const LIGHT_COUNT: usize = STREET_LIGHTS + SIGN_LIGHTS;
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
    let night_ambient = if power { 120.0 } else { 65.0 };
    LightingProfile {
        daylight,
        night,
        lamp_strength: if power { night } else { 0.0 },
        ambient: (night_ambient + (600.0 - night_ambient) * daylight) * (1.0 - rain * 0.24),
        sun: (120.0 + 11880.0 * daylight) * (1.0 - rain * 0.65),
        wetness: wet.clamp(0.0, 1.0),
    }
}

#[derive(Clone, Copy)]
struct LightSite {
    p: Vec3,
    sign: bool,
    color: Color,
}

#[derive(Resource)]
struct CityLightSites(Vec<LightSite>);

#[derive(Component)]
struct CitySpill(usize);

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
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (update_materials, update_lights, update_rain)
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
    let w = world();
    let mut sites = Vec::new();
    for p in &w.props {
        if p.kind == PropKind::Lamp {
            let offset = Quat::from_rotation_y(p.ang) * Vec3::new(0.0, 4.72, 1.1);
            sites.push(LightSite {
                p: Vec3::new(p.p.x, 0.0, p.p.y) + offset,
                sign: false,
                color: Color::srgb(1.0, 0.87, 0.66),
            });
        }
    }
    for b in &w.blds {
        if let Some(s) = b.sign {
            let normal = Vec3::new(s.ry.sin(), 0.0, s.ry.cos());
            sites.push(LightSite {
                p: s.p + normal * 0.48 - Vec3::Y * 0.25,
                sign: true,
                color: match s.idx % 4 {
                    0 => Color::srgb(1.0, 0.65, 0.38),
                    1 => Color::srgb(0.68, 0.86, 0.9),
                    2 => Color::srgb(0.9, 0.78, 0.52),
                    _ => Color::srgb(0.65, 0.88, 0.65),
                },
            });
        }
    }
    commands.insert_resource(CityLightSites(sites));
    for index in 0..LIGHT_COUNT {
        commands.spawn((
            CitySpill(index),
            PointLight {
                intensity: 0.0,
                range: 10.0,
                radius: 0.25,
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
    if let Some(mut m) = materials.get_mut(&city.lamp) {
        let strength = p.lamp_strength;
        let glass = 0.28 + 0.46 * strength;
        m.base_color = Color::srgb(glass, glass, glass);
        m.emissive = LinearRgba::rgb(strength * 2.0, strength * 1.65, strength * 1.2);
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

fn nearest_sites(sites: &[LightSite], p: Vec2, sign: bool, limit: usize) -> Vec<usize> {
    let mut candidates: Vec<(usize, f32)> = sites
        .iter()
        .enumerate()
        .filter(|(_, s)| s.sign == sign)
        .map(|(i, s)| (i, s.p.xz().distance_squared(p)))
        .filter(|(_, d)| *d < 32.0 * 32.0)
        .collect();
    candidates.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    candidates.truncate(limit);
    candidates.into_iter().map(|(i, _)| i).collect()
}

#[allow(clippy::too_many_arguments)]
fn update_lights(
    time: Res<Time>,
    flow: Res<State<GameFlow>>,
    campaign: Res<Campaign>,
    env: Res<ShelterEnvironment>,
    sites: Res<CityLightSites>,
    player: Query<&Transform, (With<Player>, Without<CitySpill>)>,
    mut lights: Query<(&CitySpill, &mut PointLight, &mut Transform), Without<Player>>,
    mut timers: ResMut<AtmosphereTimers>,
    mut evidence: ResMut<AtmosphereState>,
) {
    // Power loss is applied immediately, even between the half-second spatial refreshes.
    let strength = if session(flow.get()) {
        lighting_profile(campaign.game_min, env.power, 0.0, 0.0).lamp_strength
    } else {
        0.0
    };
    if strength <= 0.001 {
        for (_, mut light, _) in &mut lights {
            light.intensity = 0.0;
        }
        evidence.active_lights = 0;
        timers.lights = 0.0;
        return;
    }
    timers.lights -= time.delta_secs();
    if timers.lights > 0.0 {
        return;
    }
    timers.lights = 0.5;
    let Ok(player) = player.single() else { return };
    let p = player.translation.xz();
    let street = nearest_sites(&sites.0, p, false, STREET_LIGHTS);
    let signs = nearest_sites(&sites.0, p, true, SIGN_LIGHTS);
    evidence.active_lights = 0;
    for (slot, mut light, mut transform) in &mut lights {
        let site = if slot.0 < STREET_LIGHTS {
            street.get(slot.0)
        } else {
            signs.get(slot.0 - STREET_LIGHTS)
        };
        if let Some(&index) = site {
            let site = sites.0[index];
            transform.translation = site.p;
            light.color = site.color;
            light.range = if site.sign { 6.0 } else { 11.5 };
            light.intensity = strength * if site.sign { 360.0 } else { 1650.0 };
            evidence.active_lights += 1;
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
    fn lamp_selection_has_bounded_pools_and_no_remote_sources() {
        let sites: Vec<_> = (0..30)
            .map(|i| LightSite {
                p: Vec3::new(i as f32 * 3.0, 4.7, 0.0),
                sign: i % 3 == 0,
                color: Color::WHITE,
            })
            .collect();
        let street = nearest_sites(&sites, Vec2::ZERO, false, STREET_LIGHTS);
        let signs = nearest_sites(&sites, Vec2::ZERO, true, SIGN_LIGHTS);
        assert_eq!(street, vec![1, 2, 4, 5]);
        assert_eq!(signs, vec![0, 3]);
        assert!(nearest_sites(&sites, Vec2::splat(1000.0), false, STREET_LIGHTS).is_empty());
        assert_eq!(street.len() + signs.len(), LIGHT_COUNT);
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
