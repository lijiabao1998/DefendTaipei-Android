//! Reproducible Wave 4 light-catalogue and spatial-selection audit.
//!
//! This CPU-only audit does not instantiate an ECS scene or measure visible pixels,
//! GPU time or device FPS. Native QA separately inspects actual pooled SpotLights,
//! shared materials and rendered evidence. `exterior_workload` audits exact geometry.
use bevy::{
    math::Vec3Swizzles,
    prelude::{Quat, Vec2, Vec3},
};
use defend_taipei::map::{
    self,
    atmosphere::{
        CITY_LIGHT_COUNT, FRONTAGE_LIGHT_COUNT, LIGHT_ENTER_RADIUS, LIGHT_EXIT_RADIUS,
        LIGHT_GRID_CELL, LIGHT_RETAIN_ADVANTAGE, LightKind, LightSite, LightSiteIndex, LightSource,
        STREET_LIGHT_COUNT, arcade_lamp_count, arcade_lamp_position, lighting_profile,
        plan_city_lights,
    },
    data::WorldData,
    street::PropKind,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    time::Instant,
};

struct Fingerprint(u64);
impl Fingerprint {
    fn new() -> Self {
        Self(0xcbf29ce484222325)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
    fn debug(&mut self, value: &impl Debug) {
        let text = format!("{value:?}");
        self.bytes(&(text.len() as u64).to_le_bytes());
        self.bytes(text.as_bytes());
    }
    fn finish(self) -> String {
        format!("{:016x}", self.0)
    }
}
fn collection_fingerprint<T: Debug>(items: &[T]) -> String {
    let mut hash = Fingerprint::new();
    hash.bytes(&(items.len() as u64).to_le_bytes());
    for item in items {
        hash.debug(item);
    }
    hash.finish()
}
fn world_fingerprint(world: &WorldData) -> String {
    // Same documented, length-delimited full-record identity as exterior_workload.
    let components = [
        ("buildings", collection_fingerprint(&world.blds)),
        ("roads", collection_fingerprint(&world.roads)),
        ("collision_segments", collection_fingerprint(&world.segs)),
        ("street_props", collection_fingerprint(&world.props)),
        (
            "interior_fixtures",
            collection_fingerprint(&world.interiors),
        ),
    ];
    let mut hash = Fingerprint::new();
    hash.debug(&world.min);
    hash.debug(&world.max);
    hash.debug(&world.spawn);
    for (name, value) in components {
        hash.debug(&name);
        hash.debug(&value.as_str());
    }
    hash.finish()
}

fn kind_name(kind: LightKind) -> &'static str {
    match kind {
        LightKind::Street => "street",
        LightKind::Sign => "sign",
        LightKind::Arcade => "arcade",
    }
}
fn source_json(source: LightSource) -> Value {
    match source {
        LightSource::Street { prop_index } => json!({"prop_index": prop_index}),
        LightSource::Sign { building_id } => json!({"building_id": building_id}),
        LightSource::Arcade {
            building_id,
            bay_index,
        } => json!({"building_id": building_id, "bay_index": bay_index}),
    }
}
fn site_json(site: &LightSite) -> Value {
    let color = site.color.to_srgba();
    json!({
        // String form remains exact in JavaScript/JSON clients above 2^53.
        "id": site.id.to_string(),
        "kind": kind_name(site.kind),
        "source": source_json(site.source),
        "position": site.position.to_array(),
        "direction": site.direction.to_array(),
        "receiver": site.receiver.to_array(),
        "color_srgba": [color.red, color.green, color.blue, color.alpha],
        "range": site.range,
        "intensity_lumens_at_full_strength": site.intensity,
        "inner_angle_radians": site.inner_angle,
        "outer_angle_radians": site.outer_angle,
    })
}

fn validate_catalogue(world: &WorldData, sites: &[LightSite]) -> Value {
    let mut by_kind = BTreeMap::<&str, usize>::from([("street", 0), ("sign", 0), ("arcade", 0)]);
    let mut ids = BTreeSet::new();
    let buildings: BTreeMap<_, _> = world.blds.iter().map(|b| (b.id, b)).collect();
    for site in sites {
        assert!(ids.insert(site.id), "duplicate source identity");
        assert_eq!(site.id, site.source.id());
        assert_eq!(site.kind, site.source.kind());
        *by_kind.get_mut(kind_name(site.kind)).unwrap() += 1;
        assert!(site.position.is_finite() && site.receiver.is_finite());
        assert!(site.direction.is_finite());
        assert!((site.direction.length() - 1.0).abs() < 0.00001);
        assert!(site.direction.y < -0.7);
        assert!(site.range.is_finite() && site.intensity.is_finite());
        assert!(site.range > 0.0 && site.intensity > 0.0);
        assert!(site.position.distance(site.receiver) < site.range);
        assert!(site.inner_angle > 0.0 && site.inner_angle < site.outer_angle);
        assert!(site.outer_angle < std::f32::consts::FRAC_PI_2);
        let toward_receiver = (site.receiver - site.position).normalize();
        assert!(site.direction.dot(toward_receiver) >= site.outer_angle.cos());
        match site.source {
            LightSource::Street { prop_index } => {
                let prop = world.props.get(prop_index).expect("original street source");
                assert_eq!(prop.kind, PropKind::Lamp);
                let expected = Vec3::new(prop.p.x, 0.0, prop.p.y)
                    + Quat::from_rotation_y(prop.ang) * Vec3::new(0.0, 4.72, 1.1);
                assert_eq!(site.position, expected);
                assert!(world.bld_at(site.receiver.xz()).is_none());
                assert!(world.road_dist(site.receiver.xz()) < 0.1);
            }
            LightSource::Sign { building_id } => {
                let building = buildings.get(&building_id).expect("original sign building");
                let sign = building.sign.expect("original sign identity");
                let normal = Vec3::new(sign.ry.sin(), 0.0, sign.ry.cos());
                assert_eq!(site.position, sign.p + normal * 0.48 - Vec3::Y * 0.25);
                assert!(world.bld_at(site.receiver.xz()).is_none());
            }
            LightSource::Arcade {
                building_id,
                bay_index,
            } => {
                let building = buildings
                    .get(&building_id)
                    .expect("original arcade building");
                let fixture = arcade_lamp_position(building, bay_index)
                    .expect("source bay belongs to an existing arcade lamp");
                assert_eq!(site.position, fixture - Vec3::Y * 0.06);
                assert_eq!(site.receiver.xz(), fixture.xz());
                assert_eq!(site.receiver.y, 0.065);
            }
        }
    }
    let original_street = world
        .props
        .iter()
        .filter(|p| p.kind == PropKind::Lamp)
        .count();
    let original_sign = world.blds.iter().filter(|b| b.sign.is_some()).count();
    let original_arcade = world.blds.iter().map(arcade_lamp_count).sum::<usize>();
    assert!(by_kind["street"] <= original_street);
    assert!(by_kind["sign"] <= original_sign);
    assert_eq!(by_kind["arcade"], original_arcade);
    json!({
        "admitted_spill_sites": sites.len(),
        "admitted_by_kind": by_kind,
        "original_fixture_models": {
            "street": original_street, "sign": original_sign, "arcade": original_arcade,
        },
        "original_models_without_admitted_exterior_spill": {
            "street": original_street - by_kind["street"],
            "sign": original_sign - by_kind["sign"],
        },
        "source_identity_alignment_passed": true,
        "unique_stable_ids": true,
        "finite_downward_cones_with_receivers_inside_range": true,
        "scope": "Spill-site admission is distinct from original fixture geometry/emission. Exterior candidates with no valid receiver are omitted only from the spill catalogue; original models and world records remain unchanged. No claim that every catalogue site is instantiated simultaneously.",
    })
}

fn grid_cell(p: Vec2) -> (i32, i32) {
    (
        (p.x / LIGHT_GRID_CELL).floor() as i32,
        (p.y / LIGHT_GRID_CELL).floor() as i32,
    )
}

/// Independent full-scan reference for cold selection, including real-map ties.
fn cold_reference(sites: &[LightSite], p: Vec2) -> BTreeSet<u64> {
    let mut candidates: Vec<_> = sites
        .iter()
        .map(|site| (site.position.xz().distance(p), site))
        .filter(|(distance, _)| *distance <= LIGHT_ENTER_RADIUS)
        .collect();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
    let has_sign = candidates
        .iter()
        .any(|(_, site)| site.kind == LightKind::Sign);
    let has_arcade = candidates
        .iter()
        .any(|(_, site)| site.kind == LightKind::Arcade);
    let mut result = BTreeSet::new();
    for (kind, limit) in [
        (LightKind::Street, STREET_LIGHT_COUNT),
        (
            LightKind::Sign,
            if has_arcade { 1 } else { FRONTAGE_LIGHT_COUNT },
        ),
        (
            LightKind::Arcade,
            if has_sign { 1 } else { FRONTAGE_LIGHT_COUNT },
        ),
    ] {
        result.extend(
            candidates
                .iter()
                .filter(|(_, site)| site.kind == kind)
                .take(limit)
                .map(|(_, site)| site.id),
        );
    }
    result
}

#[derive(Default)]
struct QueryTotals {
    queries: usize,
    cold_reference_checks: usize,
    visited_cells: usize,
    visited_sites: usize,
    max_visited_cells: usize,
    max_visited_sites: usize,
    max_eligible_sites: usize,
    max_active_slots: usize,
    max_retained_outside_enter_radius: usize,
    timing_ms: Vec<f64>,
}
struct AuditRun<'a> {
    sites: &'a [LightSite],
    index: &'a LightSiteIndex,
    by_id: BTreeMap<u64, usize>,
    totals: QueryTotals,
}
impl<'a> AuditRun<'a> {
    fn new(sites: &'a [LightSite], index: &'a LightSiteIndex) -> Self {
        Self {
            sites,
            index,
            by_id: sites
                .iter()
                .enumerate()
                .map(|(i, site)| (site.id, i))
                .collect(),
            totals: QueryTotals::default(),
        }
    }

    fn query(
        &mut self,
        p: Vec2,
        current: &[Option<u64>; CITY_LIGHT_COUNT],
    ) -> ([Option<u64>; CITY_LIGHT_COUNT], Value) {
        let start = Instant::now();
        let selected = self.index.select_with_stats(self.sites, p, current);
        let selection_ms = start.elapsed().as_secs_f64() * 1000.0;
        // Full scans below are independent verification work, outside the timed
        // indexed call. They are not silently charged to/removed from the selector.
        let lo = grid_cell(p - LIGHT_ENTER_RADIUS);
        let hi = grid_cell(p + LIGHT_ENTER_RADIUS);
        let expected_cells = ((hi.0 - lo.0 + 1) * (hi.1 - lo.1 + 1)) as usize;
        let cell_candidates = self
            .sites
            .iter()
            .filter(|site| {
                let cell = grid_cell(site.position.xz());
                (lo.0..=hi.0).contains(&cell.0) && (lo.1..=hi.1).contains(&cell.1)
            })
            .count();
        let incumbent_lookups = current
            .iter()
            .flatten()
            .filter(|id| self.by_id.contains_key(id))
            .count();
        let eligible = self
            .sites
            .iter()
            .filter(|site| {
                let distance = site.position.xz().distance(p);
                distance <= LIGHT_ENTER_RADIUS
                    || (current.contains(&Some(site.id)) && distance <= LIGHT_EXIT_RADIUS)
            })
            .count();
        assert_eq!(selected.visited_cells, expected_cells);
        assert!(selected.visited_cells <= 25);
        assert_eq!(selected.visited_sites, cell_candidates + incumbent_lookups);
        assert!(selected.visited_sites <= self.sites.len() + CITY_LIGHT_COUNT);
        assert_eq!(selected.eligible_sites, eligible);
        let mut unique = BTreeSet::new();
        let mut actual_retained = 0;
        let mut slots = Vec::new();
        for (slot, id) in selected.slots.iter().enumerate() {
            let Some(id) = id else { continue };
            assert!(unique.insert(*id), "source assigned to multiple city slots");
            let site = &self.sites[*self
                .by_id
                .get(id)
                .expect("selected source is in original catalogue")];
            assert_eq!(site.kind == LightKind::Street, slot < STREET_LIGHT_COUNT);
            let distance = site.position.xz().distance(p);
            let was_current = current.contains(&Some(*id));
            let bound = if was_current {
                LIGHT_EXIT_RADIUS
            } else {
                LIGHT_ENTER_RADIUS
            };
            assert!(
                distance <= bound,
                "source exceeds its admission/retention radius"
            );
            if distance > LIGHT_ENTER_RADIUS {
                assert!(was_current);
                actual_retained += 1;
            }
            slots.push(json!({"slot": slot, "id": id.to_string(), "kind": kind_name(site.kind), "distance": distance, "incumbent": was_current}));
        }
        assert_eq!(selected.retained_outside_enter_radius, actual_retained);
        assert!(unique.len() <= CITY_LIGHT_COUNT);
        if current.iter().all(Option::is_none) {
            assert_eq!(
                unique,
                cold_reference(self.sites, p),
                "indexed selection differs from independent full-catalogue reference"
            );
            self.totals.cold_reference_checks += 1;
        }
        let totals = &mut self.totals;
        totals.queries += 1;
        totals.visited_cells += selected.visited_cells;
        totals.visited_sites += selected.visited_sites;
        totals.max_visited_cells = totals.max_visited_cells.max(selected.visited_cells);
        totals.max_visited_sites = totals.max_visited_sites.max(selected.visited_sites);
        totals.max_eligible_sites = totals.max_eligible_sites.max(selected.eligible_sites);
        totals.max_active_slots = totals.max_active_slots.max(unique.len());
        totals.max_retained_outside_enter_radius = totals
            .max_retained_outside_enter_radius
            .max(actual_retained);
        totals.timing_ms.push(selection_ms);
        let row = json!({
            "position": p.to_array(), "selected_slots": slots,
            "visited_cells": selected.visited_cells, "visited_sites": selected.visited_sites,
            "eligible_sites": selected.eligible_sites, "retained_outside_enter_radius": actual_retained,
            "selection_cpu_ms": selection_ms,
        });
        (selected.slots, row)
    }
}

fn timing_summary(samples: &[f64]) -> Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let sum = sorted.iter().sum::<f64>();
    json!({
        "samples": sorted.len(), "sum_ms": sum, "mean_ms": sum / sorted.len().max(1) as f64,
        "p95_ms": sorted.get((sorted.len() * 95 / 100).min(sorted.len().saturating_sub(1))),
        "max_ms": sorted.last(),
    })
}

fn main() {
    let start = Instant::now();
    let world = map::world();
    let world_load_ms = start.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let sites = plan_city_lights(world);
    let catalogue_build_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let index = LightSiteIndex::new(&sites);
    let index_build_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert!(!sites.is_empty());
    assert_eq!(CITY_LIGHT_COUNT, 6);
    assert_eq!(STREET_LIGHT_COUNT, 4);
    assert_eq!(FRONTAGE_LIGHT_COUNT, 2);
    let inventory = validate_catalogue(world, &sites);
    assert_eq!(
        sites,
        plan_city_lights(world),
        "catalogue must be deterministic"
    );
    let baseline: Value =
        serde_json::from_str(include_str!("../../docs/lighting-wave3-baseline.json"))
            .expect("digest-verified CI79 world source");
    let identity = world_fingerprint(world);
    assert_eq!(
        identity,
        baseline["world_identity"]["fingerprint"].as_str().unwrap()
    );
    let catalogue: Vec<_> = sites.iter().map(site_json).collect();
    let mut fingerprint = Fingerprint::new();
    fingerprint.bytes(&serde_json::to_vec(&catalogue).unwrap());

    let positions = [
        ("spawn", world.spawn),
        ("existing_04a_streetscape", Vec2::new(161.94, -236.84)),
        ("wave3_junction", Vec2::new(69.619415, 327.71036)),
        ("wave3_scooters", Vec2::new(-367.21985, 27.129354)),
        ("wave3_taxi", Vec2::new(59.768127, -252.9495)),
        ("wave3_truck", Vec2::new(548.0494, -138.46393)),
        ("existing_commercial_food", Vec2::new(109.41663, -365.97858)),
        ("outside_world", world.min - Vec2::splat(1000.0)),
    ];
    let mut audit = AuditRun::new(&sites, &index);
    let mut samples = Vec::new();
    let mut stationary_repeats = 0;
    for (name, p) in positions {
        let (slots, mut row) = audit.query(p, &[None; CITY_LIGHT_COUNT]);
        row["name"] = json!(name);
        for _ in 0..16 {
            let (repeated, _) = audit.query(p, &slots);
            assert_eq!(
                repeated, slots,
                "stationary source-to-slot assignment changed"
            );
            stationary_repeats += 1;
        }
        samples.push(row);
    }

    let sweep_start = audit.totals.timing_ms.len();
    let mut sweep_selected = BTreeSet::new();
    for site in &sites {
        let (slots, _) = audit.query(site.position.xz(), &[None; CITY_LIGHT_COUNT]);
        sweep_selected.extend(slots.into_iter().flatten());
    }
    let sweep_timing = timing_summary(&audit.totals.timing_ms[sweep_start..]);
    let never_selected_at_source_anchors: Vec<_> = sites
        .iter()
        .filter(|site| !sweep_selected.contains(&site.id))
        .map(|site| site.id.to_string())
        .collect();

    let mut moving_slots = [None; CITY_LIGHT_COUNT];
    let mut moving_rows = Vec::new();
    let mut assignment_changes = 0;
    for step in 0..=128 {
        let p = world.spawn + Vec2::new(step as f32 * 0.5 - 32.0, 0.0);
        let (next, mut row) = audit.query(p, &moving_slots);
        assignment_changes += next
            .iter()
            .zip(moving_slots)
            .filter(|(a, b)| **a != *b)
            .count();
        moving_slots = next;
        row["step"] = json!(step);
        moving_rows.push(row);
    }
    let mut teleports = Vec::new();
    for (name, p) in positions {
        let (next, mut row) = audit.query(p, &moving_slots);
        moving_slots = next;
        row["name"] = json!(name);
        teleports.push(row);
    }
    assert!(
        moving_slots.iter().all(Option::is_none),
        "far teleport retained a city light"
    );

    let profiles: Vec<_> = [
        ("day", 720.0, true), ("dusk", 1110.0, true), ("night", 1320.0, true),
        ("blackout", 1320.0, false), ("power_restored", 1320.0, true),
    ].into_iter().map(|(name, minute, power)| {
        let p = lighting_profile(minute, power, 0.0, 0.0);
        json!({"name": name, "minute": minute, "power": power, "daylight": p.daylight, "night": p.night,
            "lamp_strength": p.lamp_strength, "ambient": p.ambient, "sun": p.sun})
    }).collect();
    let totals = &audit.totals;
    let result = json!({
        "variant": "wave4_lighting",
        "success": true,
        "source_baseline": {
            "main_commit": baseline["source_commit"], "source_head": baseline["source_head"],
            "tested_merge": baseline["source_tested_merge"], "source_tree": baseline["source_tree"],
            "run": baseline["source_run"], "artifact": baseline["source_artifact"],
            "artifact_sha256": baseline["source_artifact_sha256"], "workload_sha256": baseline["source_workload_sha256"],
        },
        "scope": "CPU render-plan catalogue and indexed selection on the actual unchanged world. The sweep queries source neighbourhoods; it does not claim the player walked to every source. No geometry or ECS light entities are generated here. exterior_workload verifies geometry; native GPU QA independently verifies actual SpotLight allocation, shadow flags, emission, power transitions and pixels. These times are local development-build diagnostics, not GPU time or phone FPS.",
        "world_fingerprint": identity,
        "world_identity_matches_verified_wave3": true,
        "catalogue": inventory,
        "catalogue_fingerprint": fingerprint.finish(),
        "catalogue_deterministic": true,
        "light_sites": catalogue,
        "city_pool_contract": {
            "fixed_maximum_city_slots": CITY_LIGHT_COUNT, "street_slots": STREET_LIGHT_COUNT,
            "sign_or_arcade_slots": FRONTAGE_LIGHT_COUNT, "planned_component_kind": "SpotLight",
            "expected_shadow_maps_enabled": false, "expected_contact_shadows_enabled": false,
            "excludes": ["independent evacuation flare", "directional sun/moon", "global ambient"],
            "runtime_allocation_observed_by_this_cpu_audit": false,
        },
        "spatial_policy": {
            "anchor": "site.position.xz distance to player.xz", "cell_metres": LIGHT_GRID_CELL,
            "enter_radius_metres": LIGHT_ENTER_RADIUS, "exit_radius_metres": LIGHT_EXIT_RADIUS,
            "retention_advantage_metres": LIGHT_RETAIN_ADVANTAGE,
            "maximum_queried_cells": 25, "maximum_additional_incumbent_lookups": CITY_LIGHT_COUNT,
            "tie_break": "ascending stable source ID", "frontage_quota": "one sign and one arcade when both are eligible; otherwise borrow the missing family slot",
        },
        "named_samples": samples,
        "full_catalogue_source_neighbourhood_sweep": {
            "query_count": sites.len(), "unique_sources_selected": sweep_selected.len(),
            "not_selected_at_any_source_anchor": never_selected_at_source_anchors,
            "cold_selection_matches_independent_full_scan": true,
            "selection_cpu_ms": sweep_timing,
        },
        "stationary_exact_slot_repeats": stationary_repeats,
        "walk_route": {"step_metres": 0.5, "samples": moving_rows, "slot_assignment_changes": assignment_changes},
        "teleport_samples": teleports,
        "clock_profiles": profiles,
        "query_totals": {
            "queries": totals.queries, "cold_full_scan_reference_checks": totals.cold_reference_checks,
            "visited_cells": totals.visited_cells, "visited_sites": totals.visited_sites,
            "max_visited_cells": totals.max_visited_cells, "max_visited_sites": totals.max_visited_sites,
            "max_eligible_sites": totals.max_eligible_sites, "max_active_slots": totals.max_active_slots,
            "max_retained_outside_enter_radius": totals.max_retained_outside_enter_radius,
        },
        "checks": {
            "source_identities_and_fixture_positions": true, "receiver_bounds_and_cones": true,
            "cold_grid_queries_match_independent_full_catalogue_reference": true,
            "candidate_counts_match_independent_grid_and_id_lookup_counts": true,
            "unique_type_correct_bounded_slots": true, "stationary_slot_identity_stable": true,
            "far_teleport_clears_stale_sources": true,
        },
        "timings": {
            "world_load_ms": world_load_ms, "catalogue_build_ms": catalogue_build_ms, "index_build_ms": index_build_ms,
            "selection_cpu_ms": timing_summary(&totals.timing_ms),
            "scope": "First catalogue and index construction are timed before any query. Selection timings include only select_with_stats; independent full-scan/statistics validation and JSON generation occur after its timer. No absolute wall-time threshold is used.",
        },
    });
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}
