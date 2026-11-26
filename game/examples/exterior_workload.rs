//! Reproducible CPU mesh workload and world-identity audit for exterior wave 1.
//! Run `cargo run -p defend_taipei --example exterior_workload -- --check`.
//! These counts describe planned resident geometry, never measured phone FPS.
use bevy::prelude::Vec2;
use defend_taipei::map::{
    self, ChunkKey,
    build::{Lod, Parts},
    data::WorldData,
    detail::WindowKit,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{fmt::Debug, mem::size_of, time::Instant};

#[derive(Clone, Default, Serialize)]
struct MeshTotals {
    vertices: usize,
    indices: usize,
    triangles: usize,
    material_meshes: usize,
    attribute_index_payload_bytes: usize,
}

impl MeshTotals {
    fn measure(&mut self, parts: &Parts) {
        for mesh in parts.0.values().filter(|mesh| !mesh.is_empty()) {
            self.vertices += mesh.pos.len();
            self.indices += mesh.idx.len();
            self.triangles += mesh.idx.len() / 3;
            self.material_meshes += 1;
            self.attribute_index_payload_bytes += mesh.pos.len() * size_of::<[f32; 3]>()
                + mesh.nrm.len() * size_of::<[f32; 3]>()
                + mesh.uv.len() * size_of::<[f32; 2]>()
                + mesh.col.len() * size_of::<[f32; 4]>()
                + mesh.idx.len() * size_of::<u32>();
        }
    }
    fn plus(&mut self, other: &Self) {
        self.vertices += other.vertices;
        self.indices += other.indices;
        self.triangles += other.triangles;
        self.material_meshes += other.material_meshes;
        self.attribute_index_payload_bytes += other.attribute_index_payload_bytes;
    }
}

struct Fingerprint(u64);
impl Fingerprint {
    fn new() -> Self {
        Self(0xcbf29ce484222325)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= *byte as u64;
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
    fn debug(&mut self, value: &impl Debug) {
        let value = format!("{value:?}");
        self.bytes(&(value.len() as u64).to_le_bytes());
        self.bytes(value.as_bytes());
    }
    fn finish(&self) -> String {
        format!("{:016x}", self.0)
    }
}
fn fingerprint<T: Debug>(items: &[T]) -> String {
    let mut hash = Fingerprint::new();
    hash.bytes(&(items.len() as u64).to_le_bytes());
    for item in items {
        hash.debug(item);
    }
    hash.finish()
}

fn world_identity(world: &WorldData) -> Value {
    // All of these collection entries are structs/enums/vectors with deterministic
    // Debug formatting, never HashMaps or ECS entities. Same compiler, same format.
    // The buildings include IDs, complete footprint, heights, kind, doors, arcade,
    // windows, signs, materials, roof class, tank, shack flag and both original seeds.
    let components = json!({
        "buildings": fingerprint(&world.blds),
        "roads": fingerprint(&world.roads),
        "collision_segments": fingerprint(&world.segs),
        "street_props": fingerprint(&world.props),
        "interior_fixtures": fingerprint(&world.interiors),
    });
    let mut combined = Fingerprint::new();
    combined.debug(&world.min);
    combined.debug(&world.max);
    combined.debug(&world.spawn);
    for name in [
        "buildings",
        "roads",
        "collision_segments",
        "street_props",
        "interior_fixtures",
    ] {
        combined.debug(&name);
        combined.debug(&components[name].as_str().unwrap());
    }
    json!({
        "algorithm": "FNV-1a 64, length-delimited deterministic Debug records; combined digest includes bounds/spawn and labeled component hashes",
        "fingerprint": combined.finish(),
        "component_fingerprints": components,
        "buildings": world.blds.len(),
        "doors": world.blds.iter().filter(|b| b.door.is_some()).count(),
        "arcades": world.blds.iter().filter(|b| b.arcade.is_some()).count(),
        "windows": world.blds.iter().map(|b| b.wins.len()).sum::<usize>(),
        "roads": world.roads.len(),
        "collision_segments": world.segs.len(),
        "street_props": world.props.len(),
        "interior_fixtures": world.interiors.len(),
        "loot_fixtures": world.interiors.iter().filter(|f| f.loot.is_some()).count(),
        "spawn": [world.spawn.x, world.spawn.y],
        "bounds": [world.min.x, world.min.y, world.max.x, world.max.y],
    })
}

fn position_workload(world: &WorldData, name: &str, position: Vec2, kit: &WindowKit) -> Value {
    let start = Instant::now();
    let mut chunk_meshes = MeshTotals::default();
    let mut lod_chunks = [0usize; 3];
    let mut near_buildings = 0usize;
    for key in ChunkKey::all(world) {
        let lod = map::want_lod(key, position);
        let index = match lod {
            Lod::Near => 0,
            Lod::Mid => 1,
            Lod::Far => 2,
        };
        lod_chunks[index] += 1;
        let chunk = map::build_chunk(world, key, lod);
        chunk_meshes.measure(&chunk.merged);
        for (_, building) in &chunk.buildings {
            near_buildings += 1;
            chunk_meshes.measure(&building.walls);
            chunk_meshes.measure(&building.roof);
            chunk_meshes.measure(&building.base);
            chunk_meshes.measure(&building.structure);
        }
    }
    let chunk_build_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let mut lazy_detail = MeshTotals::default();
    let mut lazy_buildings = 0usize;
    for b in world
        .blds
        .iter()
        .filter(|b| b.c.distance_squared(position) < map::DETAIL_IN * map::DETAIL_IN)
    {
        lazy_buildings += 1;
        lazy_detail.measure(&map::detail::near_detail(b, kit));
        lazy_detail.measure(&map::detail::near_roof(b));
    }
    let lazy_build_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut total = chunk_meshes.clone();
    total.plus(&lazy_detail);
    json!({
        "name": name,
        "position": [position.x, position.y],
        "chunks": {"near": lod_chunks[0], "mid": lod_chunks[1], "far": lod_chunks[2]},
        "near_buildings": near_buildings,
        "initial_lazy_detail_buildings_within70m": lazy_buildings,
        "chunk_meshes": chunk_meshes,
        "lazy_detail": lazy_detail,
        "planned_resident_total": total,
        "single_pass_cpu_generation_ms": {"chunks": chunk_build_ms, "lazy_detail": lazy_build_ms},
    })
}

fn features(world: &WorldData) -> Value {
    use defend_taipei::map::{
        exterior::{self, Typology},
        roofs,
    };
    let mut types = [0usize; 4];
    let mut balcony_buildings = 0;
    let mut balcony_stacks = 0;
    let mut service_stairs = 0;
    let mut roofs_with_shacks = 0;
    let mut shacks = 0;
    let mut roof_access = 0;
    let mut roof_utilities = 0;
    let mut max_near = 0;
    let mut max_mid = 0;
    let mut near_sum = 0;
    let mut mid_sum = 0;
    for b in &world.blds {
        let plan = exterior::plan(b, world);
        types[match plan.spec.typology {
            Typology::Townhouse => 0,
            Typology::Walkup => 1,
            Typology::Shophouse => 2,
            Typology::Other => 3,
        }] += 1;
        balcony_buildings += usize::from(!plan.balconies.is_empty());
        balcony_stacks += plan.balconies.len();
        service_stairs += usize::from(plan.stair.is_some());
        let roof = roofs::layout(b);
        roofs_with_shacks += usize::from(!roof.shacks.is_empty());
        shacks += roof.shacks.len();
        roof_access += usize::from(roof.access.is_some());
        roof_utilities += roof.utilities.len();
        let near = exterior::structure(b, world, Lod::Near).vcount();
        let mid = exterior::structure(b, world, Lod::Mid).vcount();
        max_near = max_near.max(near);
        max_mid = max_mid.max(mid);
        near_sum += near;
        mid_sum += mid;
    }
    json!({
        "typology_buildings": {"townhouse": types[0], "walkup": types[1], "shophouse": types[2], "other": types[3]},
        "buildings_with_admitted_balconies": balcony_buildings,
        "admitted_balcony_stacks": balcony_stacks,
        "upper_service_stair_buildings": service_stairs,
        "roofs_with_shacks": roofs_with_shacks,
        "planned_shacks": shacks,
        "roof_access_headhouses": roof_access,
        "planned_roof_utilities": roof_utilities,
        "structure_vertices_max": {"near": max_near, "mid": max_mid},
        "all_world_structure_vertices_if_same_lod": {"near": near_sum, "mid": mid_sum},
    })
}

fn main() {
    let started = Instant::now();
    let world = map::world();
    let world_load_ms = started.elapsed().as_secs_f64() * 1000.0;
    let kit = WindowKit::new();
    let mut result = json!({
        "variant": "wave1",
        "baseline_commit": "c17292889e884f8a13d4e14598a20b64dc490f81",
        "scope": "Planned resident CPU-generated mesh payload after all world chunks settle at current Near<60m/Mid<260m thresholds plus initial lazy exterior detail within70m. Counts are submitted material meshes, not GPU-visible draw calls, GPU memory, frame time or measured phone FPS. Payload excludes allocations, ECS, materials/textures, actors, dynamic doors, interior runtime meshes, rain and UI. CPU time is one development-build pass, not a benchmark.",
        "world_identity": world_identity(world),
        "world_load_ms": world_load_ms,
        "features": features(world),
        "positions": [
            position_workload(world, "spawn", world.spawn, &kit),
            position_workload(world, "existing_04a_streetscape", Vec2::new(161.94, -236.84), &kit),
        ],
    });
    let baseline: Value = serde_json::from_str(include_str!("../../docs/exterior-baseline.json"))
        .expect("checked-in baseline audit JSON");
    let identity_matches = result["world_identity"] == baseline["world_identity"];
    let mut comparisons = Vec::new();
    let mut within_geometry_budget = true;
    for (current, previous) in result["positions"]
        .as_array()
        .unwrap()
        .iter()
        .zip(baseline["positions"].as_array().unwrap())
    {
        assert_eq!(current["name"], previous["name"]);
        let mut changes = serde_json::Map::new();
        for (metric, maximum_baseline_percent) in [
            ("vertices", 145),
            ("triangles", 145),
            ("material_meshes", 150),
            ("attribute_index_payload_bytes", 145),
        ] {
            let before = previous["planned_resident_total"][metric].as_u64().unwrap();
            let after = current["planned_resident_total"][metric].as_u64().unwrap();
            let limit = before * maximum_baseline_percent / 100;
            let passes = after <= limit;
            within_geometry_budget &= passes;
            changes.insert(
                metric.to_owned(),
                json!({
                    "baseline": before,
                    "current": after,
                    "budget_limit": limit,
                    "budget_passed": passes,
                    "maximum_baseline_percent": maximum_baseline_percent,
                    "change_percent": (after as f64 / before as f64 - 1.0) * 100.0,
                }),
            );
        }
        comparisons.push(json!({"name": current["name"], "metrics": changes}));
    }
    result["baseline_comparison"] = json!({
        "world_identity_matches": identity_matches,
        "geometry_budget_passed": within_geometry_budget,
        "positions": comparisons,
    });
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
    if std::env::args().any(|arg| arg == "--check") {
        assert!(
            identity_matches,
            "exterior-only work changed the baseline world/save identity"
        );
        assert!(
            within_geometry_budget,
            "exterior workload exceeded its aggregate geometry/material budget; inspect the JSON comparison"
        );
    }
}
