//! Reproducible added ground-transport workload. Counts describe generated
//! geometry and distance-eligible instances, not GPU timings or phone FPS.
use bevy::prelude::Vec2;
use defend_taipei::{
    combat::armor::ArmorFleet,
    map,
    vehicles::{VehicleFleet, VehicleKind, visual},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    mem::size_of,
};

#[derive(Clone, Copy, Default, Serialize)]
struct MeshCost {
    meshes: usize,
    vertices: usize,
    indices: usize,
    triangles: usize,
    attribute_index_payload_bytes: usize,
}

impl MeshCost {
    fn part(pos: usize, nrm: usize, uv: usize, col: usize, idx: usize) -> Self {
        Self {
            meshes: usize::from(idx > 0),
            vertices: pos,
            indices: idx,
            triangles: idx / 3,
            attribute_index_payload_bytes: pos * size_of::<[f32; 3]>()
                + nrm * size_of::<[f32; 3]>()
                + uv * size_of::<[f32; 2]>()
                + col * size_of::<[f32; 4]>()
                + idx * size_of::<u32>(),
        }
    }
    fn add(&mut self, other: Self) {
        self.meshes += other.meshes;
        self.vertices += other.vertices;
        self.indices += other.indices;
        self.triangles += other.triangles;
        self.attribute_index_payload_bytes += other.attribute_index_payload_bytes;
    }
}

fn main() {
    let world = map::world();
    let fleet = VehicleFleet::planned(world);
    let armor = ArmorFleet::planned(world);
    assert!(fleet.validate());
    assert!(armor.validate());
    let mut counts = BTreeMap::<String, usize>::new();
    for v in &fleet.vehicles {
        *counts
            .entry(
                serde_json::to_value(v.kind)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string(),
            )
            .or_default() += 1;
    }
    let mut models = Vec::new();
    let mut costs = HashMap::new();
    let mut vehicle_pool = MeshCost::default();
    for kind in VehicleKind::ALL {
        let model = visual::model(kind);
        let mut cost = MeshCost::default();
        for part in [&model.body, &model.wheels, &model.turret] {
            cost.add(MeshCost::part(
                part.pos.len(),
                part.nrm.len(),
                part.uv.len(),
                part.col.len(),
                part.idx.len(),
            ));
        }
        vehicle_pool.add(cost);
        costs.insert(kind, cost);
        models.push(serde_json::json!({"kind":kind,"cost":cost}));
    }
    // ArmorArt independently uploads these four models. They are additional
    // allocations even though their geometry matches VehicleArt's four kinds.
    let mut armor_pool = MeshCost::default();
    for kind in [
        VehicleKind::Zbl08,
        VehicleKind::Zbd05,
        VehicleKind::Ztd05,
        VehicleKind::Mengshi,
    ] {
        armor_pool.add(costs[&kind]);
    }
    let post = visual::docking_post();
    let dock_pool = MeshCost::part(
        post.pos.len(),
        post.nrm.len(),
        post.uv.len(),
        post.col.len(),
        post.idx.len(),
    );
    let mut total_pool = vehicle_pool;
    total_pool.add(armor_pool);
    total_pool.add(dock_pool);

    let near: Vec<_> = fleet
        .vehicles
        .iter()
        .filter(|v| v.position().distance(world.spawn) < 220.0)
        .collect();
    let armor_near: Vec<_> = armor
        .units
        .iter()
        .filter(|a| !a.abandoned && a.position().distance(world.spawn) < 120.0)
        .collect();
    let dock_positions: Vec<_> = fleet
        .vehicles
        .iter()
        .filter(|v| v.docked)
        .map(|v| v.position() + Vec2::new(v.heading.sin(), v.heading.cos()) * 0.7)
        .collect();
    let near_docks = dock_positions
        .iter()
        .filter(|p| p.distance(world.spawn) < 220.0)
        .count();
    let mut vehicle_instances = MeshCost::default();
    for vehicle in &near {
        vehicle_instances.add(costs[&vehicle.kind]);
    }
    let mut armor_instances = MeshCost::default();
    for unit in &armor_near {
        armor_instances.add(costs[&unit.kind]);
    }
    let mut total_instances = vehicle_instances;
    total_instances.add(armor_instances);
    for _ in 0..near_docks {
        total_instances.add(dock_pool);
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({
        "source":"pinned Web v24.42.0 ground rules; native deterministic placement",
        "fleet_count":fleet.vehicles.len(),"counts":counts,"abandoned_armor":fleet.vehicles.iter().filter(|v|v.spec().armored).count(),
        "shared_models":models,"shared_model_meshes":vehicle_pool.meshes,"shared_model_vertices":vehicle_pool.vertices,"shared_model_indices":vehicle_pool.indices,
        "shared_model_payload_bytes":vehicle_pool.attribute_index_payload_bytes,
        "vehicle_renderer":{"model_pool":vehicle_pool,"materials":1,"spawn_radius_metres":220,"spawn_distance_eligible_vehicles":near.len(),"spawn_instance_geometry":vehicle_instances},
        "hostile_armor_renderer":{"model_pool":armor_pool,"materials":2,"duplicates_vehicle_pool_models":true,"units":armor.units.len(),"active_patrols":armor.units.iter().filter(|a|a.active()).count(),"burned_wrecks":armor.units.iter().filter(|a|a.destroyed).count(),"spawn_radius_metres":120,"retention_radius_metres":140,"spawn_distance_eligible_units":armor_near.len(),"spawn_instance_geometry":armor_instances},
        "docking_posts":{"model_pool":dock_pool,"posts":dock_positions.len(),"spawn_radius_metres":220,"spawn_distance_eligible_posts":near_docks},
        "total_uploaded_model_geometry":total_pool,
        "total_spawn_distance_eligible_instance_geometry":total_instances,
        "unchanged_static_world":{"buildings":world.blds.len(),"roads":world.roads.len(),"props":world.props.len(),"segments":world.segs.len()},
        "limits":"Adds to exterior_workload. Includes the separately uploaded hostile armor model pool and bike-dock mesh. Attribute/index payload excludes allocation alignment, ECS/transform buffers, materials, textures, drivers and engine overhead. Distance eligibility precedes frustum/occlusion tests; instance totals are not draw calls. No measured GPU memory, timing, FPS, thermal or total-process-memory claim."
    })).unwrap());
}
