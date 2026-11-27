//! Deterministic city light locations and a bounded spatial selector.
//!
//! These are render plans derived from original lamps, signs and arcade fixtures.
//! No world record, collision, random stream or saved gameplay state is changed.

use bevy::prelude::*;
use std::collections::HashMap;

use super::super::{
    WorldData,
    data::{ARC, BAY, Bld, COL, GF},
    street::PropKind,
};

pub const STREET_LIGHT_COUNT: usize = 4;
pub const FRONTAGE_LIGHT_COUNT: usize = 2;
pub const CITY_LIGHT_COUNT: usize = STREET_LIGHT_COUNT + FRONTAGE_LIGHT_COUNT;
pub const LIGHT_ENTER_RADIUS: f32 = 32.0;
pub const LIGHT_EXIT_RADIUS: f32 = 36.0;
pub const LIGHT_GRID_CELL: f32 = 16.0;
/// An incumbent can be up to this many metres farther than a new candidate.
pub const LIGHT_RETAIN_ADVANTAGE: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LightKind {
    Street,
    Sign,
    Arcade,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LightSource {
    Street { prop_index: usize },
    Sign { building_id: u32 },
    Arcade { building_id: u32, bay_index: usize },
}

impl LightSource {
    pub fn id(self) -> u64 {
        match self {
            Self::Street { prop_index } => (1_u64 << 56) | prop_index as u64,
            Self::Sign { building_id } => (2_u64 << 56) | u64::from(building_id),
            Self::Arcade {
                building_id,
                bay_index,
            } => {
                assert!(bay_index < 1 << 24);
                (3_u64 << 56) | (u64::from(building_id) << 24) | bay_index as u64
            }
        }
    }

    pub fn kind(self) -> LightKind {
        match self {
            Self::Street { .. } => LightKind::Street,
            Self::Sign { .. } => LightKind::Sign,
            Self::Arcade { .. } => LightKind::Arcade,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightSite {
    pub id: u64,
    pub kind: LightKind,
    pub source: LightSource,
    /// Actual emitter immediately below the original light fixture/sign face.
    pub position: Vec3,
    /// Unit direction of the spot cone; Bevy lights point down local -Z.
    pub direction: Vec3,
    /// An aim point just above road, pavement or arcade floor on the cone's centre line.
    pub receiver: Vec3,
    pub color: Color,
    pub range: f32,
    /// Bevy lumens; these are not measured physical-device lux values.
    pub intensity: f32,
    pub inner_angle: f32,
    pub outer_angle: f32,
}

impl LightSite {
    fn new(source: LightSource, position: Vec3, receiver: Vec3) -> Self {
        let kind = source.kind();
        let (color, range, intensity, inner_angle, outer_angle) = match kind {
            LightKind::Street => (Color::srgb(1.0, 0.82, 0.57), 12.5, 168_000.0, 0.45, 0.78),
            LightKind::Sign => (Color::srgb(1.0, 0.77, 0.56), 8.0, 64_000.0, 0.38, 0.72),
            LightKind::Arcade => (Color::srgb(0.88, 0.95, 1.0), 6.0, 88_000.0, 0.28, 0.48),
        };
        Self {
            id: source.id(),
            kind,
            source,
            position,
            direction: (receiver - position).normalize(),
            receiver,
            color,
            range,
            intensity,
            inner_angle,
            outer_angle,
        }
    }
}

/// Matches the existing lamp-fixture loop in `build::arcade`.
pub fn arcade_lamp_count(building: &Bld) -> usize {
    building
        .arcade
        .map_or(0, |a| ((a.dl - COL) / BAY).round().max(1.0) as usize)
}

/// The existing fixture's geometric centre, without adding or moving geometry.
pub fn arcade_lamp_position(building: &Bld, bay_index: usize) -> Option<Vec3> {
    let a = building.arcade?;
    let bays = arcade_lamp_count(building);
    if bay_index >= bays {
        return None;
    }
    let step = (a.dl - 0.7) / bays as f32;
    let s = 0.35 + (bay_index as f32 + 0.5) * step;
    let p = a.a + a.u * s - a.n * (ARC / 2.0);
    Some(Vec3::new(p.x, GF - 0.24, p.y))
}

/// Plans sources across the entire unchanged world. Only the fixed six pooled
/// lights are instantiated. Invalid exterior receivers are omitted from this
/// spill catalogue; their original lamp/sign meshes remain untouched.
pub fn plan_city_lights(world: &WorldData) -> Vec<LightSite> {
    let mut sites = Vec::new();
    for (prop_index, prop) in world.props.iter().enumerate() {
        if prop.kind != PropKind::Lamp {
            continue;
        }
        let rotation = Quat::from_rotation_y(prop.ang);
        let base = Vec3::new(prop.p.x, 0.0, prop.p.y);
        let position = base + rotation * Vec3::new(0.0, 4.72, 1.1);
        let receiver = [2.2, 1.6, 1.1].into_iter().find_map(|forward| {
            let p = base + rotation * Vec3::new(0.0, 0.035, forward);
            (world.bld_at(p.xz()).is_none() && world.road_dist(p.xz()) < 0.1).then_some(p)
        });
        if let Some(receiver) = receiver {
            sites.push(LightSite::new(
                LightSource::Street { prop_index },
                position,
                receiver,
            ));
        }
    }
    for building in &world.blds {
        if let Some(sign) = building.sign {
            let normal = Vec3::new(sign.ry.sin(), 0.0, sign.ry.cos());
            let position = sign.p + normal * 0.48 - Vec3::Y * 0.25;
            let receiver = [1.55, 1.0, 0.65].into_iter().find_map(|forward| {
                // Sidewalk slabs top out at y=.15. Keep the aiming/sample point
                // above that surface even when the sign faces a raised walk.
                let p = Vec3::new(sign.p.x, 0.155, sign.p.z) + normal * forward;
                world.bld_at(p.xz()).is_none().then_some(p)
            });
            if let Some(receiver) = receiver {
                let mut site = LightSite::new(
                    LightSource::Sign {
                        building_id: building.id,
                    },
                    position,
                    receiver,
                );
                site.color = match sign.idx % 4 {
                    0 => Color::srgb(1.0, 0.65, 0.38),
                    1 => Color::srgb(0.68, 0.86, 0.9),
                    2 => Color::srgb(0.9, 0.78, 0.52),
                    _ => Color::srgb(0.65, 0.88, 0.65),
                };
                sites.push(site);
            }
        }
        for bay_index in 0..arcade_lamp_count(building) {
            let fixture = arcade_lamp_position(building, bay_index)
                .expect("existing arcade bay has a fixture");
            // Existing tile top is y=.06; the receiver is just above it.
            let receiver = Vec3::new(fixture.x, 0.065, fixture.z);
            sites.push(LightSite::new(
                LightSource::Arcade {
                    building_id: building.id,
                    bay_index,
                },
                fixture - Vec3::Y * 0.06,
                receiver,
            ));
        }
    }
    sites
}

#[derive(Clone, Debug, Default)]
pub struct LightSelection {
    pub slots: [Option<u64>; CITY_LIGHT_COUNT],
    /// Includes empty cells. Radius 32 on a 16 m grid visits at most 25.
    pub visited_cells: usize,
    /// Cell candidate evaluations plus up to six incumbent ID-map evaluations.
    pub visited_sites: usize,
    pub eligible_sites: usize,
    /// Selected incumbents beyond admission radius but inside the exit radius.
    pub retained_outside_enter_radius: usize,
}

#[derive(Clone, Debug)]
pub struct LightSiteIndex {
    cells: HashMap<(i32, i32), Vec<usize>>,
    by_id: HashMap<u64, usize>,
    catalog_len: usize,
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    index: usize,
    distance: f32,
}

impl LightSiteIndex {
    pub fn new(sites: &[LightSite]) -> Self {
        let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        let mut by_id = HashMap::new();
        for (index, site) in sites.iter().enumerate() {
            assert!(site.position.is_finite());
            assert!(
                by_id.insert(site.id, index).is_none(),
                "duplicate light identity"
            );
            cells
                .entry(cell(site.position.xz()))
                .or_default()
                .push(index);
        }
        Self {
            cells,
            by_id,
            catalog_len: sites.len(),
        }
    }

    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.by_id.get(&id).copied()
    }

    pub fn select(
        &self,
        sites: &[LightSite],
        player: Vec2,
        current: &[Option<u64>; CITY_LIGHT_COUNT],
    ) -> [Option<u64>; CITY_LIGHT_COUNT] {
        self.select_with_stats(sites, player, current).slots
    }

    pub fn select_with_stats(
        &self,
        sites: &[LightSite],
        player: Vec2,
        current: &[Option<u64>; CITY_LIGHT_COUNT],
    ) -> LightSelection {
        assert_eq!(
            self.catalog_len,
            sites.len(),
            "light index/catalog mismatch"
        );
        let mut result = LightSelection::default();
        if !player.is_finite() {
            return result;
        }
        let min = cell(player - LIGHT_ENTER_RADIUS);
        let max = cell(player + LIGHT_ENTER_RADIUS);
        let mut candidates = Vec::new();
        for x in min.0..=max.0 {
            for z in min.1..=max.1 {
                result.visited_cells += 1;
                if let Some(indices) = self.cells.get(&(x, z)) {
                    for &index in indices {
                        result.visited_sites += 1;
                        let distance = sites[index].position.xz().distance(player);
                        if distance <= LIGHT_ENTER_RADIUS {
                            candidates.push(Candidate { index, distance });
                        }
                    }
                }
            }
        }
        for id in current.iter().flatten() {
            if let Some(index) = self.index_of(*id) {
                result.visited_sites += 1;
                let distance = sites[index].position.xz().distance(player);
                if distance <= LIGHT_EXIT_RADIUS
                    && !candidates.iter().any(|candidate| candidate.index == index)
                {
                    candidates.push(Candidate { index, distance });
                }
            }
        }
        candidates.sort_by(|a, b| {
            a.distance
                .total_cmp(&b.distance)
                .then(sites[a.index].id.cmp(&sites[b.index].id))
        });
        result.eligible_sites = candidates.len();

        let mut chosen = choose_kind(
            sites,
            &candidates,
            current,
            LightKind::Street,
            STREET_LIGHT_COUNT,
        );
        let has_sign = candidates
            .iter()
            .any(|candidate| sites[candidate.index].kind == LightKind::Sign);
        let has_arcade = candidates
            .iter()
            .any(|candidate| sites[candidate.index].kind == LightKind::Arcade);
        // Give each available frontage source family one slot. A missing family
        // lends its slot to the other without growing the fixed pool.
        for (kind, limit) in [
            (
                LightKind::Sign,
                if has_arcade { 1 } else { FRONTAGE_LIGHT_COUNT },
            ),
            (
                LightKind::Arcade,
                if has_sign { 1 } else { FRONTAGE_LIGHT_COUNT },
            ),
        ] {
            chosen.extend(choose_kind(sites, &candidates, current, kind, limit));
        }

        for (slot, incumbent) in current.iter().enumerate() {
            if let Some(id) = incumbent
                && chosen.contains(id)
                && !result.slots.contains(&Some(*id))
                && self.index_of(*id).is_some_and(|index| {
                    (sites[index].kind == LightKind::Street) == (slot < STREET_LIGHT_COUNT)
                })
            {
                result.slots[slot] = Some(*id);
            }
        }
        for id in chosen {
            if result.slots.contains(&Some(id)) {
                continue;
            }
            let index = self.index_of(id).expect("chosen site is indexed");
            let range = if sites[index].kind == LightKind::Street {
                0..STREET_LIGHT_COUNT
            } else {
                STREET_LIGHT_COUNT..CITY_LIGHT_COUNT
            };
            for slot in range {
                if result.slots[slot].is_none() {
                    result.slots[slot] = Some(id);
                    break;
                }
            }
        }
        result.retained_outside_enter_radius = result
            .slots
            .iter()
            .flatten()
            .filter(|id| {
                self.index_of(**id).is_some_and(|index| {
                    sites[index].position.xz().distance(player) > LIGHT_ENTER_RADIUS
                })
            })
            .count();
        result
    }
}

fn cell(p: Vec2) -> (i32, i32) {
    (
        (p.x / LIGHT_GRID_CELL).floor() as i32,
        (p.y / LIGHT_GRID_CELL).floor() as i32,
    )
}

fn choose_kind(
    sites: &[LightSite],
    candidates: &[Candidate],
    current: &[Option<u64>; CITY_LIGHT_COUNT],
    kind: LightKind,
    limit: usize,
) -> Vec<u64> {
    let of_kind: Vec<_> = candidates
        .iter()
        .filter(|candidate| sites[candidate.index].kind == kind)
        .collect();
    let mut chosen: Vec<_> = of_kind.iter().take(limit).copied().collect();
    // Preserve incumbent identities when only near-equal distances changed.
    for id in current.iter().flatten() {
        let Some(incumbent) = of_kind
            .iter()
            .find(|candidate| sites[candidate.index].id == *id)
            .copied()
        else {
            continue;
        };
        if chosen
            .iter()
            .any(|candidate| candidate.index == incumbent.index)
        {
            continue;
        }
        if let Some((replace, worst)) = chosen
            .iter()
            .enumerate()
            .filter(|(_, candidate)| !current.contains(&Some(sites[candidate.index].id)))
            .max_by(|(_, a), (_, b)| a.distance.total_cmp(&b.distance))
            && incumbent.distance <= worst.distance + LIGHT_RETAIN_ADVANTAGE
        {
            chosen[replace] = incumbent;
        }
    }
    chosen
        .into_iter()
        .map(|candidate| sites[candidate.index].id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn site(kind: LightKind, index: usize, x: f32, z: f32) -> LightSite {
        let source = match kind {
            LightKind::Street => LightSource::Street { prop_index: index },
            LightKind::Sign => LightSource::Sign {
                building_id: index as u32,
            },
            LightKind::Arcade => LightSource::Arcade {
                building_id: index as u32,
                bay_index: 0,
            },
        };
        LightSite::new(source, Vec3::new(x, 4.0, z), Vec3::new(x, 0.035, z))
    }

    #[test]
    fn catalogue_uses_real_fixture_identities_and_downward_receivers() {
        let world = super::super::super::world();
        let sites = plan_city_lights(world);
        assert_eq!(sites, plan_city_lights(world));
        let mut ids = HashSet::new();
        for site in sites {
            assert!(ids.insert(site.id));
            assert_eq!(site.id, site.source.id());
            assert_eq!(site.kind, site.source.kind());
            assert!(site.position.is_finite() && site.receiver.is_finite());
            assert!((site.direction.length() - 1.0).abs() < 0.00001);
            assert!(site.direction.y < -0.7);
            assert!(site.position.distance(site.receiver) < site.range);
            assert!(site.inner_angle > 0.0 && site.inner_angle < site.outer_angle);
            assert!(site.outer_angle < std::f32::consts::FRAC_PI_2);
            match site.source {
                LightSource::Street { prop_index } => {
                    let prop = world.props[prop_index];
                    assert_eq!(prop.kind, PropKind::Lamp);
                    let expected = Vec3::new(prop.p.x, 0.0, prop.p.y)
                        + Quat::from_rotation_y(prop.ang) * Vec3::new(0.0, 4.72, 1.1);
                    assert_eq!(site.position, expected);
                    assert!(world.bld_at(site.receiver.xz()).is_none());
                    assert!(world.road_dist(site.receiver.xz()) < 0.1);
                }
                LightSource::Sign { building_id } => {
                    let sign = world.blds[building_id as usize].sign.unwrap();
                    let normal = Vec3::new(sign.ry.sin(), 0.0, sign.ry.cos());
                    assert_eq!(site.position, sign.p + normal * 0.48 - Vec3::Y * 0.25);
                    assert!(world.bld_at(site.receiver.xz()).is_none());
                }
                LightSource::Arcade {
                    building_id,
                    bay_index,
                } => {
                    let building = &world.blds[building_id as usize];
                    let fixture = arcade_lamp_position(building, bay_index).unwrap();
                    assert_eq!(site.position, fixture - Vec3::Y * 0.06);
                    assert_eq!(site.receiver.xz(), fixture.xz());
                    assert_eq!(site.receiver.y, 0.065);
                }
            }
        }
    }

    #[test]
    fn fixed_pool_preserves_frontage_variety_and_incumbent_slots() {
        let mut sites: Vec<_> = (0..5)
            .map(|i| site(LightKind::Street, i, i as f32 + 2.0, 0.0))
            .collect();
        sites.extend([
            site(LightKind::Sign, 0, 4.0, 1.0),
            site(LightKind::Arcade, 0, 1.0, 1.0),
            site(LightKind::Arcade, 1, 1.2, 1.0),
        ]);
        let index = LightSiteIndex::new(&sites);
        let first = index.select(&sites, Vec2::ZERO, &[None; CITY_LIGHT_COUNT]);
        assert!(first.iter().all(Option::is_some));
        assert_eq!(
            first.iter().flatten().collect::<HashSet<_>>().len(),
            CITY_LIGHT_COUNT
        );
        let local: Vec<_> = first[STREET_LIGHT_COUNT..]
            .iter()
            .flatten()
            .map(|id| sites[index.index_of(*id).unwrap()].kind)
            .collect();
        assert!(local.contains(&LightKind::Sign) && local.contains(&LightKind::Arcade));
        assert_eq!(first, index.select(&sites, Vec2::new(0.1, 0.05), &first));
        assert!(
            index
                .select(&sites, Vec2::splat(1000.0), &first)
                .iter()
                .all(Option::is_none)
        );
    }

    #[test]
    fn admission_exit_hysteresis_and_local_index_are_bounded() {
        let mut sites = vec![site(LightKind::Street, 0, 31.8, 0.0)];
        sites.extend((1..100).map(|i| site(LightKind::Street, i, 1000.0 + i as f32, 0.0)));
        let index = LightSiteIndex::new(&sites);
        let initial = index.select_with_stats(&sites, Vec2::ZERO, &[None; CITY_LIGHT_COUNT]);
        assert_eq!(initial.slots[0], Some(sites[0].id));
        assert!(initial.visited_cells <= 25);
        assert_eq!(initial.visited_sites, 1);
        assert_eq!(initial.retained_outside_enter_radius, 0);
        let retained = index.select_with_stats(&sites, Vec2::new(-1.0, 0.0), &initial.slots);
        assert_eq!(retained.slots, initial.slots);
        assert_eq!(retained.retained_outside_enter_radius, 1);
        let fresh = index.select(&sites, Vec2::new(-1.0, 0.0), &[None; CITY_LIGHT_COUNT]);
        assert!(fresh.iter().all(Option::is_none));
        let exited = index.select(&sites, Vec2::new(-5.0, 0.0), &initial.slots);
        assert!(exited.iter().all(Option::is_none));
        let nan = index.select_with_stats(&sites, Vec2::splat(f32::NAN), &initial.slots);
        assert!(nan.slots.iter().all(Option::is_none));
        assert_eq!(nan.visited_sites, 0);
    }

    #[test]
    fn exact_distance_ties_and_family_borrowing_are_deterministic() {
        let sites = vec![
            site(LightKind::Sign, 4, 1.0, 0.0),
            site(LightKind::Sign, 2, -1.0, 0.0),
            site(LightKind::Sign, 3, 0.0, 1.0),
        ];
        let index = LightSiteIndex::new(&sites);
        let selected = index.select(&sites, Vec2::ZERO, &[None; CITY_LIGHT_COUNT]);
        assert!(selected[..STREET_LIGHT_COUNT].iter().all(Option::is_none));
        assert_eq!(selected[4], Some(sites[1].id));
        assert_eq!(selected[5], Some(sites[2].id));
        assert_eq!(selected, index.select(&sites, Vec2::ZERO, &selected));
    }
}
