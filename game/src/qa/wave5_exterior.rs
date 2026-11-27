//! Production street/furniture captures. No display-only meshes are spawned.
use std::collections::{HashMap, HashSet};

use bevy::{prelude::*, window::PrimaryWindow};
use serde_json::{Value, json};

use crate::{
    camera::{CameraZoom, GameCamera},
    game_flow::Campaign,
    geom::MeshBuf,
    map::{
        self, Chunk, ChunkKey, Lod, MapMeshGeometry, MapMeshMaterial,
        build::Parts,
        interiors,
        street::{self, furnishings},
    },
    survival::Stash,
    ui::{OpenPanel, PanelKind},
    worldplay::ScavengeState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scene {
    Utilities,
    Market,
    Temple,
}

#[derive(Clone, Copy, Debug)]
enum Subject {
    Utilities {
        utility: usize,
        signal: Option<usize>,
    },
    Market {
        fixture: usize,
    },
    Temple {
        building: u32,
    },
}

#[derive(Resource, Clone, Copy, Debug)]
struct Fixture {
    scene: Scene,
    subject: Subject,
    position: Vec2,
    zoom: f32,
}

fn size(world: &mut World) -> Result<[u32; 2], String> {
    let size = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .ok_or("Wave 5 exterior needs a primary viewport")?
        .size();
    Ok([size.x as u32, size.y as u32])
}

fn utility_parts(
    data: &map::WorldData,
    utility: usize,
    signal: Option<usize>,
) -> Result<Vec<(ChunkKey, Parts)>, String> {
    let plans = furnishings::plans(data);
    let site = plans
        .utilities
        .get(utility)
        .ok_or("Utility fixture disappeared")?;
    let mut groups = HashMap::<ChunkKey, Parts>::new();
    let mut props = HashSet::new();
    let mut add = |p: Vec2, parts: Parts| {
        groups.entry(ChunkKey::of(p)).or_default().merge(parts);
    };
    add(site.position, furnishings::utility_geometry(site));
    props.insert(site.prop_index);
    if let Some(end) = site.cable_to {
        let destination = plans
            .utilities
            .iter()
            .find(|other| other.position == end)
            .ok_or("Cable endpoint is not an actual planned pole")?;
        // The destination's next span is a separate source feature. Frame this
        // complete span and both pole assemblies, not an unbounded wire chain.
        let mut endpoint = destination.clone();
        endpoint.cable_to = None;
        add(end, furnishings::utility_geometry(&endpoint));
        props.insert(destination.prop_index);
    }
    if let Some(index) = signal {
        let site = plans
            .signals
            .get(index)
            .ok_or("Signal fixture disappeared")?;
        add(site.position, furnishings::signal_geometry(site));
        props.insert(site.prop_index);
    }
    for index in props {
        add(
            data.props[index].p,
            street::prop_geometry(&data.props[index]),
        );
    }
    let mut parts: Vec<_> = groups.into_iter().collect();
    parts.sort_by_key(|(key, _)| *key);
    Ok(parts)
}

fn vertices(parts: &Parts) -> Vec<Vec3> {
    parts
        .0
        .values()
        .flat_map(|mesh| mesh.pos.iter().copied().map(Vec3::from))
        .collect()
}

fn cart_transform(fixture: &interiors::Fixture) -> Mat4 {
    crate::geom::place(
        fixture.pos.x,
        0.04,
        fixture.pos.y,
        fixture.front.x.atan2(fixture.front.y),
    )
}

fn bounds(data: &map::WorldData, subject: Subject) -> Result<Vec<Vec3>, String> {
    match subject {
        Subject::Utilities { utility, signal } => Ok(utility_parts(data, utility, signal)?
            .iter()
            .flat_map(|(_, parts)| vertices(parts))
            .collect()),
        Subject::Market { fixture } => {
            let fixture = &data.interiors[fixture];
            let transform = cart_transform(fixture);
            Ok(interiors::market_cart_mesh()
                .pos
                .into_iter()
                .map(|p| transform.transform_point3(Vec3::from(p)))
                .collect())
        }
        Subject::Temple { building } => Ok(vertices(&map::detail::temple_frontage(
            &data.blds[building as usize],
        ))),
    }
}

fn select(
    data: &map::WorldData,
    scene: Scene,
    size: [u32; 2],
    progress: &ScavengeState,
) -> Result<Fixture, String> {
    let toward_camera = Vec2::ONE.normalize();
    let mut candidates: Vec<(f32, Fixture)> = Vec::new();
    match scene {
        Scene::Utilities => {
            let plans = furnishings::plans(data);
            for (utility, site) in plans
                .utilities
                .iter()
                .enumerate()
                .filter(|(_, s)| s.cable_to.is_some())
            {
                let end = site.cable_to.unwrap();
                if ![site.position, end]
                    .into_iter()
                    .all(|p| super::street_camera_clear(data, Vec3::new(p.x, 6.6, p.y)))
                {
                    continue;
                }
                let nearest_signal = plans
                    .signals
                    .iter()
                    .enumerate()
                    .filter(|(_, signal)| {
                        signal.position.distance(site.position) < 22.0
                            && signal.direction.dot(toward_camera) > 0.2
                            && super::street_camera_clear(
                                data,
                                Vec3::new(signal.position.x, 4.32, signal.position.y),
                            )
                    })
                    .min_by(|(_, a), (_, b)| {
                        a.position
                            .distance_squared(site.position)
                            .total_cmp(&b.position.distance_squared(site.position))
                    })
                    .map(|(index, _)| index);
                let Some(signal) = nearest_signal else {
                    continue;
                };
                let subject = Subject::Utilities {
                    utility,
                    signal: Some(signal),
                };
                let points = bounds(data, subject)?;
                let centre = (site.position + end) * 0.5;
                for offset in [
                    Vec2::ZERO,
                    toward_camera * 3.0,
                    -toward_camera * 3.0,
                    Vec2::X * 5.0,
                    Vec2::Y * 5.0,
                    -Vec2::X * 5.0,
                    -Vec2::Y * 5.0,
                ] {
                    let position = centre + offset;
                    if !super::street_player_position_clear(data, position) {
                        continue;
                    }
                    let Some(zoom) = super::exterior_zoom(&points, position, size) else {
                        continue;
                    };
                    let score = zoom + if site.transformer { 0.5 } else { 0.0 };
                    candidates.push((
                        score,
                        Fixture {
                            scene,
                            subject,
                            position,
                            zoom,
                        },
                    ));
                }
            }
        }
        Scene::Market => {
            for (index, fixture) in data.interiors.iter().enumerate() {
                let building = &data.blds[fixture.building as usize];
                if map::commercial::is_named_temple(building)
                    || !interiors::market_cart_fixture(building, fixture.kind)
                    || fixture.front.dot(toward_camera) < 0.15
                    || data
                        .bld_at(fixture.approach)
                        .is_none_or(|b| b.id != building.id)
                    || !data.free(fixture.approach, 0.5)
                    || !super::exterior_camera_clear(
                        data,
                        building.id,
                        Vec3::new(fixture.pos.x, 0.8, fixture.pos.y),
                    )
                {
                    continue;
                }
                let stock = crate::worldplay::preview_container_loot(
                    &fixture.id,
                    fixture.loot.ok_or("Market fixture lost loot identity")?,
                    720.0,
                    progress,
                );
                if stock.stacks().is_empty() {
                    continue;
                }
                let screen_clear = data
                    .interiors
                    .iter()
                    .filter(|other| other.building == fixture.building && other.id != fixture.id)
                    .all(|other| {
                        (1..=16).all(|step| {
                            let distance = step as f32 * 0.25;
                            let point = fixture.pos + toward_camera * distance;
                            let delta = point - other.pos;
                            let size = other.kind.size();
                            delta.dot(other.tangent()).abs() > size.x * 0.5
                                || delta.dot(other.front).abs() > size.z * 0.5
                                || 0.85 + distance * 56.0 / 40.0_f32.hypot(40.0) > size.y
                        })
                    });
                if !screen_clear {
                    continue;
                }
                let subject = Subject::Market { fixture: index };
                let points = bounds(data, subject)?;
                if let Some(zoom) = super::exterior_zoom(&points, fixture.approach, size) {
                    candidates.push((
                        zoom,
                        Fixture {
                            scene,
                            subject,
                            position: fixture.approach,
                            zoom,
                        },
                    ));
                }
            }
        }
        Scene::Temple => {
            for building in &data.blds {
                // A recessed lantern under an intact arcade soffit is not
                // inspectable from this fixed elevated camera outside the lot.
                // Select a real exposed entrance instead of hiding its roof.
                if building.arcade.is_some()
                    || building.sign.is_none()
                    || map::commercial::plan(building, data).is_none()
                {
                    continue;
                }
                let geometry = map::detail::temple_frontage(building);
                if geometry.vcount() < 100 {
                    continue;
                }
                let Some(door) = building.door else {
                    continue;
                };
                if door.n.dot(toward_camera) < 0.2 {
                    continue;
                }
                let points = vertices(&geometry);
                let middle = points.iter().copied().sum::<Vec3>() / points.len() as f32;
                if !super::exterior_camera_clear(data, building.id, middle) {
                    continue;
                }
                for distance in [2.0, 3.5, 5.0] {
                    let position = door.p + door.n * distance;
                    if !super::street_player_position_clear(data, position)
                        || map::exterior_blocks_camera(building, position)
                    {
                        continue;
                    }
                    if let Some(zoom) = super::exterior_zoom(&points, position, size) {
                        candidates.push((
                            zoom,
                            Fixture {
                                scene,
                                subject: Subject::Temple {
                                    building: building.id,
                                },
                                position,
                                zoom,
                            },
                        ));
                    }
                }
            }
        }
    }
    candidates
        .into_iter()
        .max_by(|(a_score, a), (b_score, b)| {
            a_score.total_cmp(b_score).then_with(|| {
                b.position
                    .distance_squared(data.spawn)
                    .total_cmp(&a.position.distance_squared(data.spawn))
            })
        })
        .map(|(_, fixture)| fixture)
        .ok_or_else(|| format!("No clear production {scene:?} fixture for Wave 5 exterior QA"))
}

pub(super) fn enter(world: &mut World, scene: Scene) -> Result<Vec2, String> {
    let size = size(world)?;
    let fixture = select(map::world(), scene, size, world.resource::<ScavengeState>())?;
    super::move_view(world, fixture.position, fixture.zoom)?;
    world.write_message(OpenPanel(PanelKind::Close));
    world.resource_mut::<crate::ui::PauseOpen>().0 = false;
    world.resource_mut::<Campaign>().game_min = 720.0;
    world
        .resource_mut::<crate::survival::ShelterEnvironment>()
        .power = true;
    world.resource_mut::<furnishings::SignalPhase>().qa_fixed = Some(0.0);
    {
        let mut progress = world.resource_mut::<ScavengeState>();
        progress.weather.next = 5000.0;
        progress.weather.until = 0.0;
        progress.weather.amount = 0.0;
        progress.weather.intensity = 0.0;
        progress.weather.wet = 0.0;
    }
    world.insert_resource(fixture);
    println!(
        "DTQA wave5 exterior scene={scene:?} subject={:?} position={:?} zoom={}",
        fixture.subject, fixture.position, fixture.zoom
    );
    Ok(fixture.position)
}

fn vertex_key(point: [f32; 3]) -> [i32; 3] {
    point.map(|v| (v * 1000.0).round() as i32)
}

fn subset(feature: &MeshBuf, batch: &MeshBuf) -> bool {
    let actual: HashSet<_> = batch.pos.iter().copied().map(vertex_key).collect();
    feature
        .pos
        .iter()
        .copied()
        .all(|p| actual.contains(&vertex_key(p)))
}

fn check_metadata(vertices: usize, min: Vec3, max: Vec3, expected: &MeshBuf) -> bool {
    let (lo, hi) = expected.aabb();
    vertices == expected.vcount() && min.distance(lo) < 0.001 && max.distance(hi) < 0.001
}

fn world_space_transform(transform: &GlobalTransform) -> bool {
    transform
        .to_matrix()
        .to_cols_array()
        .into_iter()
        .zip(Mat4::IDENTITY.to_cols_array())
        .all(|(actual, expected)| (actual - expected).abs() < 0.001)
}

fn utility_evidence(
    world: &mut World,
    utility: usize,
    signal: Option<usize>,
    main_view: &HashSet<Entity>,
) -> Result<Value, String> {
    let data = map::world();
    let mut evidence = Vec::new();
    for (key, feature) in utility_parts(data, utility, signal)? {
        let root = world
            .query::<(Entity, &Chunk)>()
            .iter(world)
            .find_map(|(entity, chunk)| {
                (chunk.key == key && chunk.lod == Lod::Near).then_some(entity)
            })
            .ok_or_else(|| format!("Utility source chunk {key:?} is not yet Near"))?;
        let expected = map::build_chunk(data, key, Lod::Near).merged;
        let mut found = HashSet::new();
        for (entity, owner, material, mesh, metadata, transform, inherited, visible) in world
            .query::<(
                Entity,
                &ChildOf,
                &MapMeshMaterial,
                &Mesh3d,
                &MapMeshGeometry,
                &GlobalTransform,
                &InheritedVisibility,
                &ViewVisibility,
            )>()
            .iter(world)
        {
            if owner.parent() != root || !feature.0.contains_key(&material.0) {
                continue;
            }
            let batch = expected
                .0
                .get(&material.0)
                .ok_or("Missing utility production material")?;
            if !check_metadata(metadata.vertices, metadata.min, metadata.max, batch)
                || !subset(&feature.0[&material.0], batch)
                || !world_space_transform(transform)
                || !inherited.get()
                || !visible.get()
                || !main_view.contains(&entity)
                || !world.resource::<Assets<Mesh>>().contains(&mesh.0)
            {
                return Err(format!(
                    "Utility {key:?}/{:?} lacks matching actual upload/main-view evidence",
                    material.0
                ));
            }
            found.insert(material.0);
            evidence.push(json!({"chunk":[key.0,key.1],"material":format!("{:?}",material.0),
                "actual_uploaded_vertices":metadata.vertices,"feature_vertices":feature.0[&material.0].vcount(),
                "main_view_member":true,"source_vertices_in_chunk":true}));
        }
        if found.len() != feature.0.len() {
            return Err(format!(
                "Utility {key:?} is still missing a feature material batch"
            ));
        }
    }
    let plans = furnishings::plans(data);
    let site = &plans.utilities[utility];
    let phase = world.resource::<furnishings::SignalPhase>();
    let power = world
        .resource::<crate::survival::ShelterEnvironment>()
        .power;
    let expected = phase.aspects(power);
    let city = world.resource::<map::textures::MapMaterials>();
    let mut emissions = Vec::new();
    for (index, handle) in city.signals.iter().enumerate() {
        let material = world
            .resource::<Assets<StandardMaterial>>()
            .get(handle)
            .ok_or("Missing actual signal material")?;
        let rgb = [
            material.emissive.red,
            material.emissive.green,
            material.emissive.blue,
        ];
        if rgb.iter().any(|value| *value > 0.0) != expected[index] {
            return Err("Signal materials do not follow the original phase/power state".into());
        }
        emissions.push(rgb);
    }
    Ok(
        json!({"utility_prop":site.prop_index,"transformer":site.transformer,"cable_to":site.cable_to.map(|p|p.to_array()),
        "signal_prop":signal.map(|index|plans.signals[index].prop_index),"signal_phase_seconds":phase.value(),
        "signal_emissions":emissions,"batches":evidence}),
    )
}

fn temple_evidence(
    world: &mut World,
    building: u32,
    main_view: &HashSet<Entity>,
) -> Result<Value, String> {
    let data = map::world();
    let b = &data.blds[building as usize];
    let feature = map::detail::temple_frontage(b);
    let full = map::detail::near_detail(b, &map::detail::WindowKit::new());
    let mut found = HashSet::new();
    let mut batches = Vec::new();
    for (entity, owner, material, mesh, metadata, transform, inherited, visible) in world
        .query::<(
            Entity,
            &map::BldDetail,
            &MapMeshMaterial,
            &Mesh3d,
            &MapMeshGeometry,
            &GlobalTransform,
            &InheritedVisibility,
            &ViewVisibility,
        )>()
        .iter(world)
    {
        if owner.0 != building || !feature.0.contains_key(&material.0) {
            continue;
        }
        let expected = full
            .0
            .get(&material.0)
            .ok_or("Temple detail material is missing")?;
        if !check_metadata(metadata.vertices, metadata.min, metadata.max, expected)
            || !subset(&feature.0[&material.0], expected)
            || !world_space_transform(transform)
            || !inherited.get()
            || !visible.get()
            || !main_view.contains(&entity)
            || !world.resource::<Assets<Mesh>>().contains(&mesh.0)
        {
            return Err("Temple ornament is not in the actual uploaded/main-view facade".into());
        }
        found.insert(material.0);
        batches.push(json!({"material":format!("{:?}",material.0),"actual_uploaded_vertices":metadata.vertices,
            "ornament_vertices":feature.0[&material.0].vcount(),"main_view_member":true}));
    }
    if found.len() != feature.0.len() {
        return Err("Temple facade is not fully streamed into the main view".into());
    }
    let sign = b
        .sign
        .ok_or("Named temple capture needs its existing fascia")?;
    let blank_uv = Vec2::from_array(map::textures::unbranded_sign_uv(sign.idx));
    let building_mesh = map::build::building(b, Lod::Near);
    let expected_sign = building_mesh
        .roof
        .0
        .get(&map::build::Mat::Sign)
        .ok_or("Named temple fascia is missing from the production shell")?;
    if expected_sign
        .uv
        .iter()
        .any(|uv| Vec2::from_array(*uv).distance(blank_uv) > 0.000001)
    {
        return Err("Named temple production fascia still samples a generic shop label".into());
    }
    let mut blank_batches = Vec::new();
    for (entity, owner, material, mesh, metadata, transform, inherited, visible) in world
        .query::<(
            Entity,
            &map::BldRoof,
            &MapMeshMaterial,
            &Mesh3d,
            &MapMeshGeometry,
            &GlobalTransform,
            &InheritedVisibility,
            &ViewVisibility,
        )>()
        .iter(world)
    {
        if owner.0 != building || material.0 != map::build::Mat::Sign {
            continue;
        }
        if !check_metadata(metadata.vertices, metadata.min, metadata.max, expected_sign)
            || metadata
                .flat_uv
                .is_none_or(|uv| uv.distance(blank_uv) > 0.000001)
            || !world_space_transform(transform)
            || !inherited.get()
            || !visible.get()
            || !main_view.contains(&entity)
            || !world.resource::<Assets<Mesh>>().contains(&mesh.0)
        {
            return Err("Temple actual fascia upload is not a visible unbranded plaque".into());
        }
        blank_batches.push(json!({"material":"Sign","flat_uv":blank_uv.to_array(),
            "actual_uploaded_vertices":metadata.vertices,"main_view_member":true}));
    }
    if blank_batches.is_empty() {
        return Err("Temple unbranded plaque is not yet in the actual main view".into());
    }
    Ok(
        json!({"building":building,"original_name":b.name,"batches":batches,
        "plaque_style":"unbranded_dark_temple","displayed_shop_label":null,
        "generic_shop_name_suppressed":true,"stored_original_sign_index":sign.idx,
        "actual_uploaded_blank_sign_batches":blank_batches}),
    )
}

fn market_evidence(
    world: &mut World,
    fixture_index: usize,
    main_view: &HashSet<Entity>,
) -> Result<Value, String> {
    let data = map::world();
    let fixture = &data.interiors[fixture_index];
    let building = &data.blds[fixture.building as usize];
    let fixtures: Vec<_> = data
        .interiors
        .iter()
        .filter(|f| f.building == building.id)
        .collect();
    let body = interiors::room_visual_geometry(building, &fixtures);
    let minute = world.resource::<Campaign>().game_min;
    let stocks: Vec<_> = fixtures
        .iter()
        .map(|f| {
            f.loot.map_or_else(Stash::default, |kind| {
                crate::worldplay::preview_container_loot(
                    &f.id,
                    kind,
                    minute,
                    world.resource::<ScavengeState>(),
                )
            })
        })
        .collect();
    let expected_stock = interiors::stock_visual_geometry(&fixtures, &stocks);
    let (entity, view, mesh, metadata, transform, inherited, visible) = world
        .query::<(
            Entity,
            &interiors::InteriorView,
            &Mesh3d,
            &interiors::InteriorGeometry,
            &GlobalTransform,
            &InheritedVisibility,
            &ViewVisibility,
        )>()
        .iter(world)
        .find(|(_, view, ..)| view.building == building.id)
        .ok_or("Market room is not yet streamed")?;
    if !check_metadata(metadata.vertices, metadata.min, metadata.max, &body)
        || !world_space_transform(transform)
        || !inherited.get()
        || !visible.get()
        || !main_view.contains(&entity)
        || !world.resource::<Assets<Mesh>>().contains(&mesh.0)
    {
        return Err("Market cart room upload/ownership/main-view evidence failed".into());
    }
    let stock_entity = view.stock;
    let uploaded_body_vertices = metadata.vertices;
    let cart = interiors::market_cart_mesh().transformed(cart_transform(fixture));
    if !subset(&cart, &body) {
        return Err("Actual market room production geometry does not include its cart".into());
    }
    let (stock_mesh, metadata, inherited, visible) = world
        .get::<Mesh3d>(stock_entity)
        .zip(world.get::<interiors::InteriorGeometry>(stock_entity))
        .zip(world.get::<InheritedVisibility>(stock_entity))
        .zip(world.get::<ViewVisibility>(stock_entity))
        .map(|(((mesh, metadata), inherited), visible)| (mesh, metadata, inherited, visible))
        .ok_or("Market stock has no actual uploaded geometry")?;
    if !check_metadata(
        metadata.vertices,
        metadata.min,
        metadata.max,
        &expected_stock,
    ) || !inherited.get()
        || !visible.get()
        || !main_view.contains(&stock_entity)
        || !world.resource::<Assets<Mesh>>().contains(&stock_mesh.0)
    {
        return Err("Market actual stock upload differs from the saved/preview ledger or is absent from main view".into());
    }
    let stock_vertices = metadata.vertices;
    if world
        .get::<ChildOf>(stock_entity)
        .is_none_or(|owner| owner.parent() != entity)
        || world
            .get::<GlobalTransform>(stock_entity)
            .is_none_or(|transform| !world_space_transform(transform))
    {
        return Err("Market stock lost its actual room ownership or world-space transform".into());
    }
    let roofs: Vec<_> = world
        .query::<(Entity, &map::BldRoof, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, roof, _)| roof.0 == building.id)
        .map(|(entity, _, visibility)| (entity, visibility.get()))
        .collect();
    if roofs.is_empty()
        || roofs
            .iter()
            .any(|(entity, visible)| *visible && main_view.contains(entity))
    {
        return Err("Market production roof cutaway is not complete".into());
    }
    let own_stock = fixtures.iter().position(|f| f.id == fixture.id).unwrap();
    Ok(
        json!({"building":building.id,"fixture_id":fixture.id,"original_loot_kind":fixture.loot,
        "saved_stock":stocks[own_stock].stacks(),"body_uploaded_vertices":uploaded_body_vertices,
        "stock_uploaded_vertices":stock_vertices,"cart_vertices":cart.vcount(),"cart_inside_original_collision":true,
        "body_in_main_view":true,"stock_in_main_view":true,"roof_hidden_by_actual_cutaway":true}),
    )
}

pub(super) fn evidence(world: &mut World, scene: Scene) -> Result<Value, String> {
    let fixture = *world
        .get_resource::<Fixture>()
        .ok_or("Wave 5 exterior capture has no selected fixture")?;
    if fixture.scene != scene {
        return Err("Wave 5 exterior capture/fixture scene mismatch".into());
    }
    let size = size(world)?;
    let player = super::player_position(world)?;
    if player.xz().distance(fixture.position) > 0.001
        || (world.resource::<CameraZoom>().0 - fixture.zoom).abs() > 0.001
    {
        return Err("Wave 5 exterior fixture camera/player changed before capture".into());
    }
    let main_view = super::main_camera_meshes(world)
        .ok_or("Wave 5 exterior camera has no visible entity set")?;
    let detail = match fixture.subject {
        Subject::Utilities { utility, signal } => {
            utility_evidence(world, utility, signal, &main_view)?
        }
        Subject::Market { fixture } => market_evidence(world, fixture, &main_view)?,
        Subject::Temple { building } => temple_evidence(world, building, &main_view)?,
    };
    let points = bounds(map::world(), fixture.subject)?;
    let (camera, transform) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<GameCamera>>()
        .iter(world)
        .next()
        .ok_or("Wave 5 exterior main camera missing")?;
    if transform
        .translation()
        .distance(player + crate::camera::CAM_OFFSET)
        > 0.01
    {
        return Err("Wave 5 exterior does not use the normal gameplay camera".into());
    }
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for point in points {
        let pixel = camera
            .world_to_viewport(transform, point)
            .map_err(|e| format!("Wave 5 exterior projection failed: {e:?}"))?;
        lo = lo.min(pixel);
        hi = hi.max(pixel);
    }
    if lo.x < 0.02 * size[0] as f32
        || lo.y < 0.02 * size[1] as f32
        || hi.x > 0.98 * size[0] as f32
        || hi.y > 0.98 * size[1] as f32
    {
        return Err(format!(
            "Wave 5 exterior source geometry cropped: {lo:?}..{hi:?}"
        ));
    }
    let atmosphere = world.resource::<map::atmosphere::AtmosphereState>();
    if atmosphere.night > 0.1 || !atmosphere.power {
        return Err("Wave 5 exterior daylight fixture is not active".into());
    }
    Ok(
        json!({"scene":format!("{scene:?}"),"player":player.to_array(),"camera":transform.translation().to_array(),
        "zoom":fixture.zoom,"viewport":size,"projected_bounds":[lo.to_array(),hi.to_array()],
        "visibility_measurement":"actual_uploaded_geometry_owner_material_bounds_and_main_view_membership; source envelope screening; raw GPU image still requires visual review",
        "source":"production_world_no_display_only_props","detail":detail}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wave5_production_scenes_have_clear_original_world_fixtures() {
        let data = map::world();
        for scene in [Scene::Utilities, Scene::Market, Scene::Temple] {
            let fixture = select(data, scene, [1280, 720], &ScavengeState::default()).unwrap();
            assert!(crate::camera::ZOOM_RANGE.contains(&fixture.zoom));
            assert!(bounds(data, fixture.subject).unwrap().len() > 100);
            match fixture.subject {
                Subject::Utilities { signal, .. } => assert!(signal.is_some()),
                Subject::Temple { building } => assert!(!map::exterior_blocks_camera(
                    &data.blds[building as usize],
                    fixture.position,
                )),
                Subject::Market { .. } => {}
            }
        }
    }
}
