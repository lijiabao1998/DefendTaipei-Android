//! Reproducible CPU mesh workload and world-identity audit for the exterior waves.
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
use std::{collections::BTreeMap, fmt::Debug, mem::size_of, time::Instant};

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
    let mut temple_geometry = MeshTotals::default();
    for b in world
        .blds
        .iter()
        .filter(|b| b.c.distance_squared(position) < map::DETAIL_IN * map::DETAIL_IN)
    {
        lazy_buildings += 1;
        lazy_detail.measure(&map::detail::near_detail(b, kit));
        lazy_detail.measure(&map::detail::near_roof(b));
        temple_geometry.measure(&map::detail::temple_frontage(b));
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
        "wave5_near_temple_added_geometry": temple_geometry,
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

fn commercial_features(world: &WorldData) -> Value {
    use defend_taipei::map::commercial::{self, PanelFinish};

    let mut families = BTreeMap::<&str, usize>::new();
    let mut panels = [0usize; 3];
    let mut frontages = 0;
    let mut arcaded = 0;
    let mut canopies = 0;
    let mut striped_canopies = 0;
    let mut blades = 0;
    let mut max_vertices = [0usize; 2];
    let mut total_vertices = [0usize; 2];
    for b in &world.blds {
        let Some(plan) = commercial::plan(b, world) else {
            continue;
        };
        frontages += 1;
        *families.entry(plan.family.name()).or_default() += 1;
        arcaded += usize::from(plan.arcaded);
        canopies += usize::from(plan.canopy.is_some());
        striped_canopies += usize::from(plan.canopy.is_some_and(|canopy| canopy.striped));
        blades += usize::from(plan.blade.is_some());
        for panel in &plan.panels {
            panels[match panel.finish {
                PanelFinish::Glazed => 0,
                PanelFinish::Shutter => 1,
                PanelFinish::Timber => 2,
            }] += 1;
        }
        for (index, lod) in [Lod::Near, Lod::Mid].into_iter().enumerate() {
            let mesh = commercial::geometry(b, &plan, lod);
            let vertices = mesh.base.vcount() + mesh.shell.vcount();
            max_vertices[index] = max_vertices[index].max(vertices);
            total_vertices[index] += vertices;
        }
    }
    json!({
        "scope": "Render-only business identities from existing signs; closed frontage panels are not new doors or loot fixtures. Assembly vertices below exclude the old geometry they replace; use resident totals for the actual net cost.",
        "original_signed_buildings": world.blds.iter().filter(|b| b.sign.is_some()).count(),
        "planned_commercial_frontages": frontages,
        "family_frontages": families,
        "arcaded_frontages": arcaded,
        "admitted_panels": {"glazed": panels[0], "shutter": panels[1], "timber": panels[2]},
        "admitted_exposed_canopies": canopies,
        "striped_canopies": striped_canopies,
        "admitted_matching_projecting_signs": blades,
        "commercial_vertices_max": {"near": max_vertices[0], "mid": max_vertices[1]},
        "all_world_commercial_vertices_if_same_lod": {"near": total_vertices[0], "mid": total_vertices[1]},
    })
}

fn street_features(world: &WorldData) -> Value {
    use defend_taipei::map::street::{self, PropKind};

    let kind_name = |kind| match kind {
        PropKind::Lamp => "lamp",
        PropKind::Scooter => "scooter",
        PropKind::Car => "car",
        PropKind::Taxi => "taxi",
        PropKind::Truck => "truck",
        PropKind::Tree(_) => "tree",
    };
    let markings = street::road_marking_plans(world);
    let parking = street::parking_plans(world);
    let mut marking_meshes = MeshTotals::default();
    let mut parking_meshes = MeshTotals::default();
    let mut parking_by_kind = BTreeMap::<&str, usize>::new();
    let mut original_props = BTreeMap::<&str, usize>::new();
    let mut prop_meshes = BTreeMap::<&str, MeshTotals>::new();
    let mut prop_vertex_bounds = BTreeMap::<&str, [usize; 2]>::new();
    for plan in &markings {
        marking_meshes.measure(&street::road_marking_geometry(plan));
    }
    for plan in &parking {
        parking_meshes.measure(&street::parking_geometry(plan));
        *parking_by_kind
            .entry(kind_name(world.props[plan.prop_index].kind))
            .or_default() += 1;
    }
    for prop in &world.props {
        let name = kind_name(prop.kind);
        *original_props.entry(name).or_default() += 1;
        if matches!(prop.kind, PropKind::Tree(_)) {
            continue;
        }
        let geometry = street::prop_geometry(prop);
        let vertices = geometry.vcount();
        prop_meshes.entry(name).or_default().measure(&geometry);
        let bounds = prop_vertex_bounds.entry(name).or_insert([usize::MAX, 0]);
        bounds[0] = bounds[0].min(vertices);
        bounds[1] = bounds[1].max(vertices);
    }
    let parked_props: usize = world
        .props
        .iter()
        .filter(|prop| {
            matches!(
                prop.kind,
                PropKind::Scooter | PropKind::Car | PropKind::Taxi | PropKind::Truck
            )
        })
        .count();
    json!({
        "scope": "Inherited Wave 3 street-geometry inventory at original road/prop identities. Assembly material counts are summed before chunk batching, not resident draw calls. Trees and collision records are unchanged. Wave 4 light components, selection and emission are audited separately; resident totals below measure geometry/material-batch cost.",
        "original_props": original_props,
        "planned_road_segments": markings.len(),
        "segments_with_junction_clearance": markings.iter().filter(|plan| plan.clear_start > 0.71 || plan.clear_end > 0.71).count(),
        "junction_crossings_and_stop_lines": markings.iter().map(|plan| plan.approaches.len()).sum::<usize>(),
        "junction_approach_arrows": markings.iter().flat_map(|plan| &plan.approaches).filter(|approach| approach.has_arrow).count(),
        "admitted_parking_bays": parking.len(),
        "parking_bays_by_kind": parking_by_kind,
        "existing_parked_props_without_admitted_bays": parked_props - parking.len(),
        "all_world_marking_assemblies_if_near_or_mid": marking_meshes,
        "all_world_parking_assemblies_if_near_or_mid": parking_meshes,
        "all_world_affected_prop_assemblies_if_near_or_mid": prop_meshes,
        "per_affected_prop_vertex_bounds_min_max": prop_vertex_bounds,
    })
}

fn furnishing_features(world: &WorldData) -> Value {
    use defend_taipei::map::{interiors, street::furnishings};
    let plans = furnishings::plans(world);
    let mut utility_meshes = MeshTotals::default();
    let mut signal_meshes = MeshTotals::default();
    let mut max_utility_vertices = 0;
    let mut max_signal_vertices = 0;
    for site in &plans.utilities {
        let geometry = furnishings::utility_geometry(site);
        max_utility_vertices = max_utility_vertices.max(geometry.vcount());
        utility_meshes.measure(&geometry);
    }
    for site in &plans.signals {
        let geometry = furnishings::signal_geometry(site);
        max_signal_vertices = max_signal_vertices.max(geometry.vcount());
        signal_meshes.measure(&geometry);
    }
    let carts: Vec<_> = world
        .interiors
        .iter()
        .filter(|f| interiors::market_cart_fixture(&world.blds[f.building as usize], f.kind))
        .collect();
    let temples: Vec<_> = world
        .blds
        .iter()
        .filter(|b| map::detail::temple_frontage(b).vcount() > 0)
        .collect();
    let mut temple_meshes = MeshTotals::default();
    let mut max_temple_vertices = 0;
    for building in &temples {
        let geometry = map::detail::temple_frontage(building);
        max_temple_vertices = max_temple_vertices.max(geometry.vcount());
        temple_meshes.measure(&geometry);
    }
    let mut utility_targets: Vec<_> = plans
        .utilities
        .iter()
        .filter(|site| site.cable_to.is_some())
        .collect();
    utility_targets.sort_by(|a, b| {
        a.position
            .distance_squared(world.spawn)
            .total_cmp(&b.position.distance_squared(world.spawn))
    });
    let mut signal_targets: Vec<_> = plans.signals.iter().collect();
    signal_targets.sort_by(|a, b| {
        a.position
            .distance_squared(world.spawn)
            .total_cmp(&b.position.distance_squared(world.spawn))
    });
    json!({
        "scope": "Render-only native adaptation of Web decay.ts/alley.ts/roads.ts. Utility equipment and signal heads reuse existing lamp anchors above head height; no extra ground obstacles. Market carts replace existing kitchen visual meshes within exactly their original collision/loot bounds. Native placement is deterministic and is not a claim of surveyed street furniture positions.",
        "utility_anchors": plans.utilities.len(),
        "transformer_housings": plans.utilities.iter().filter(|site| site.transformer).count(),
        "admitted_same_curb_cable_spans": plans.utilities.iter().filter(|site| site.cable_to.is_some()).count(),
        "signal_heads": plans.signals.len(),
        "signal_axes": [plans.signals.iter().filter(|site| site.axis == 0).count(), plans.signals.iter().filter(|site| site.axis == 1).count()],
        "shared_signal_materials": 6,
        "additional_actual_light_entities": 0,
        "utility_site_fingerprint": fingerprint(&plans.utilities),
        "signal_site_fingerprint": fingerprint(&plans.signals),
        "all_world_utility_assemblies_if_near_or_mid": utility_meshes,
        "all_world_signal_assemblies_if_near_or_mid": signal_meshes,
        "per_utility_max_vertices": max_utility_vertices,
        "per_signal_max_vertices": max_signal_vertices,
        "market_cart_fixtures": carts.len(),
        "market_cart_body_vertices": interiors::market_cart_mesh().vcount(),
        "named_temple_frontages": temples.len(),
        "all_world_temple_ornaments_if_near": temple_meshes,
        "per_temple_max_vertices": max_temple_vertices,
        "temple_render_overrides": temples.iter().map(|b| json!({
            "building":b.id,"original_name":b.name,"stored_sign_index":b.sign.map(|sign|sign.idx),
            "plaque_style":"unbranded_dark_temple","displayed_shop_label":null,
            "geometry_material_counts_preserved":true,
        })).collect::<Vec<_>>(),
        "qa_targets": {
            "utility": utility_targets.iter().take(3).map(|site| json!({"prop_index":site.prop_index,"position":[site.position.x,site.position.y],"cable_to":site.cable_to.map(|p| [p.x,p.y])})).collect::<Vec<_>>(),
            "signals": signal_targets.iter().take(3).map(|site| json!({"prop_index":site.prop_index,"position":[site.position.x,site.position.y],"direction":[site.direction.x,site.direction.y],"axis":site.axis})).collect::<Vec<_>>(),
            "market_carts": carts.iter().take(3).map(|f| json!({"id":f.id,"building":f.building,"position":[f.pos.x,f.pos.y],"approach":[f.approach.x,f.approach.y]})).collect::<Vec<_>>(),
            "temples": temples.iter().map(|b| json!({"building":b.id,"name":b.name,"door":b.door.map(|d| [d.p.x,d.p.y])})).collect::<Vec<_>>(),
        },
    })
}

fn main() {
    let started = Instant::now();
    let world = map::world();
    let world_load_ms = started.elapsed().as_secs_f64() * 1000.0;
    let kit = WindowKit::new();
    // Report the one-time render plan construction before inventory/position
    // generation warms its global cache. This includes the first inventory copy;
    // subsequent chunk timing must not silently appear to include this work.
    let street_plan_started = Instant::now();
    let _ = map::street::road_marking_plans(world);
    let street_plan_initialization_ms = street_plan_started.elapsed().as_secs_f64() * 1000.0;
    let mut result = json!({
        "variant": "wave5_street_detail",
        "baseline_commit": "70dd4659a62cb371cb626a94df31b6716118f7e6",
        "wave3_baseline_commit": "3418a19b2b1e2a4c0843ba59faead5a00af672a6",
        "wave2_baseline_commit": "230c82d85b47ea47c9411721bf68f7a7f0528d76",
        "wave1_baseline_commit": "6d0bbd55964d802e2ddaae886baf9ca8dc17dcb0",
        "original_baseline_commit": "c17292889e884f8a13d4e14598a20b64dc490f81",
        "scope": "Planned resident CPU-generated mesh payload after all world chunks settle at current Near<60m/Mid<260m thresholds plus initial lazy exterior detail within70m. Wave 5 preserves the complete world identity and allows at most 3% additional geometry/payload and 2% additional submitted material meshes for bounded street equipment/temple detail versus Wave 4. These are not GPU-visible draw calls, GPU memory, frame time or measured phone FPS. Payload excludes allocations, ECS, light components, materials/textures, actors, dynamic doors, interior runtime meshes, driveable vehicles, bike docks, enemy armor, rain and UI. Existing static parked props remain included. See lighting_workload and vehicle_workload plus actual native QA for separate light-selection, dynamic-vehicle and runtime evidence. CPU time is one development-build pass, not a benchmark.",
        "world_identity": world_identity(world),
        "world_load_ms": world_load_ms,
        "street_plan_initialization_ms": street_plan_initialization_ms,
        "timing_scope": "Street plan initialization is measured once before all inventories/chunks and includes the first plan-list copy. Position chunk timings use this warmed render-only cache. These development-build wall times are not a performance benchmark.",
        "features": features(world),
        "commercial_features": commercial_features(world),
        "street_features": street_features(world),
        "furnishing_features": furnishing_features(world),
        "positions": [
            position_workload(world, "spawn", world.spawn, &kit),
            position_workload(world, "existing_04a_streetscape", Vec2::new(161.94, -236.84), &kit),
        ],
    });
    let baseline: Value =
        serde_json::from_str(include_str!("../../docs/street-detail-wave4-baseline.json"))
            .expect("checked-in digest-verified CI81 wave-four audit JSON");
    let wave3: Value =
        serde_json::from_str(include_str!("../../docs/lighting-wave3-baseline.json"))
            .expect("checked-in CI79 wave-three audit JSON");
    let wave2: Value = serde_json::from_str(include_str!("../../docs/streets-wave2-baseline.json"))
        .expect("checked-in CI77 wave-two audit JSON");
    let wave1: Value =
        serde_json::from_str(include_str!("../../docs/commercial-wave1-baseline.json"))
            .expect("checked-in CI75 wave-one audit JSON");
    let original: Value = serde_json::from_str(include_str!("../../docs/exterior-baseline.json"))
        .expect("checked-in original exterior audit JSON");
    let identity_matches = result["world_identity"] == baseline["world_identity"]
        && result["world_identity"] == wave2["world_identity"]
        && result["world_identity"] == wave1["world_identity"];
    let original_identity_matches = result["world_identity"] == original["world_identity"];
    let architecture_matches = result["features"] == baseline["features"]
        && result["features"] == wave2["features"]
        && result["features"] == wave1["features"];
    let commerce_matches = result["commercial_features"] == baseline["commercial_features"]
        && result["commercial_features"] == wave2["commercial_features"];
    // The only excluded field is explanatory scope prose; every inventory count,
    // source-kind count and complete assembly-cost record remains exact.
    let mut street_counts = result["street_features"].clone();
    let mut baseline_street_counts = baseline["street_features"].clone();
    street_counts.as_object_mut().unwrap().remove("scope");
    baseline_street_counts
        .as_object_mut()
        .unwrap()
        .remove("scope");
    let streets_match = street_counts == baseline_street_counts;
    let mut comparisons = Vec::new();
    let mut original_comparisons = Vec::new();
    let mut geometry_budget_passed = true;
    let mut material_budget_passed = true;
    for reference in [&baseline, &wave3, &wave2, &wave1, &original] {
        assert_eq!(
            result["positions"].as_array().unwrap().len(),
            reference["positions"].as_array().unwrap().len(),
            "every original audit position must remain represented"
        );
    }
    for (index, current) in result["positions"].as_array().unwrap().iter().enumerate() {
        let previous = &baseline["positions"][index];
        let after_commerce = &wave2["positions"][index];
        let after_architecture = &wave1["positions"][index];
        let before_architecture = &original["positions"][index];
        assert_eq!(current["name"], previous["name"]);
        assert_eq!(current["name"], after_commerce["name"]);
        assert_eq!(current["name"], after_architecture["name"]);
        assert_eq!(current["name"], before_architecture["name"]);
        // The engine's positions are f32. Decimal JSON round-tripping through
        // CI and Python can differ by one f64 ULP without changing either engine
        // coordinate; compare the exact f32 bits used by the streaming planner.
        let coordinate_bits = |value: &Value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|coordinate| (coordinate.as_f64().unwrap() as f32).to_bits())
                .collect::<Vec<_>>()
        };
        for reference in [
            previous,
            after_commerce,
            after_architecture,
            before_architecture,
        ] {
            assert_eq!(
                coordinate_bits(&current["position"]),
                coordinate_bits(&reference["position"])
            );
            assert_eq!(current["chunks"], reference["chunks"]);
        }
        assert_eq!(current["near_buildings"], previous["near_buildings"]);
        assert_eq!(
            current["initial_lazy_detail_buildings_within70m"],
            previous["initial_lazy_detail_buildings_within70m"]
        );
        for metric in [
            "vertices",
            "indices",
            "triangles",
            "attribute_index_payload_bytes",
        ] {
            assert_eq!(
                current["lazy_detail"][metric].as_u64().unwrap(),
                previous["lazy_detail"][metric].as_u64().unwrap()
                    + current["wave5_near_temple_added_geometry"][metric]
                        .as_u64()
                        .unwrap(),
                "only the measured named-temple detail may change the inherited lazy geometry"
            );
        }
        let mut changes = serde_json::Map::new();
        let mut original_changes = serde_json::Map::new();
        // Keep every previous wave visible so the incremental equipment budget
        // cannot hide the cumulative architecture/commercial/street cost.
        for (metric, maximum_baseline_percent) in [
            ("vertices", 103),
            ("indices", 103),
            ("triangles", 103),
            ("material_meshes", 102),
            ("attribute_index_payload_bytes", 103),
        ] {
            let before = previous["planned_resident_total"][metric].as_u64().unwrap();
            let after = current["planned_resident_total"][metric].as_u64().unwrap();
            let limit = before * maximum_baseline_percent / 100;
            let geometry = metric != "material_meshes";
            let passes = after >= before && after <= limit;
            if geometry {
                geometry_budget_passed &= passes;
            } else {
                material_budget_passed &= passes;
            }
            changes.insert(
                metric.to_owned(),
                json!({
                    "baseline": before,
                    "current": after,
                    "budget_limit": limit,
                    "budget_passed": passes,
                    "maximum_baseline_percent": maximum_baseline_percent,
                    "required_relation": "at_least_baseline_and_at_most_limit",
                    "change_percent": (after as f64 / before as f64 - 1.0) * 100.0,
                }),
            );
            let original_before = before_architecture["planned_resident_total"][metric]
                .as_u64()
                .unwrap();
            original_changes.insert(
                metric.to_owned(),
                json!({
                    "original_baseline": original_before,
                    "wave1": after_architecture["planned_resident_total"][metric],
                    "wave2": after_commerce["planned_resident_total"][metric],
                    "wave3": wave3["positions"][index]["planned_resident_total"][metric],
                    "wave4": before,
                    "current": after,
                    "cumulative_change_percent": (after as f64 / original_before as f64 - 1.0) * 100.0,
                }),
            );
        }
        comparisons.push(json!({"name": current["name"], "metrics": changes}));
        original_comparisons.push(json!({"name": current["name"], "metrics": original_changes}));
    }
    result["baseline_comparison"] = json!({
        "reference": "Verified wave-four CI81 resident geometry at merged main 70dd465; original archive/member digests and source/tested/main provenance in street-detail-wave4-baseline.json",
        "world_identity_matches": identity_matches,
        "wave1_feature_and_facade_vertex_counts_match": architecture_matches,
        "wave2_commercial_feature_and_vertex_counts_match": commerce_matches,
        "wave3_street_inventory_and_assembly_counts_match": streets_match,
        "street_comparison_excludes_only_explanatory_scope_text": true,
        "geometry_budget_passed_without_materials": geometry_budget_passed,
        "material_budget_passed": material_budget_passed,
        "geometry_budget_passed": geometry_budget_passed && material_budget_passed,
        "positions": comparisons,
    });
    result["original_baseline_comparison"] = json!({
        "reference": "Pre-architecture main c172928; cumulative costs retain Waves 1, 2, 3 and 4 separately from the Wave 5 bounded equipment allowance",
        "world_identity_matches": original_identity_matches,
        "positions": original_comparisons,
    });
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
    if std::env::args().any(|arg| arg == "--check") {
        assert!(
            identity_matches && original_identity_matches,
            "exterior-only work changed the baseline world/save identity"
        );
        assert!(
            architecture_matches,
            "street detail work changed the verified wave-one feature or facade vertex counts"
        );
        assert!(
            commerce_matches,
            "street detail work changed the verified wave-two commercial feature or vertex counts"
        );
        assert!(
            streets_match,
            "street detail work changed the verified wave-three street geometry inventory"
        );
        assert!(
            geometry_budget_passed,
            "street detail work exceeded the 3% vertex/index/payload allowance"
        );
        assert!(
            material_budget_passed,
            "street detail workload exceeded the 2% shared-material-batch allowance; inspect the JSON comparison"
        );
    }
}
