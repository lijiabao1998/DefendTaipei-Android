//! Ground armour and captured mounted weapons, ported from the pinned Web
//! `systems/armor.ts`, `systems/avgun.ts` and `core/main.ts`.
//! All figures here are the original game's tuning, not real equipment guidance.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::vehicles::VehicleKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountedWeapon {
    Coax762,
    Autocannon30,
    Cannon105,
    Heavy127,
}

#[derive(Clone, Copy, Debug)]
pub struct MountedWeaponSpec {
    pub name_zh: &'static str,
    pub name_en: &'static str,
    pub cooldown: f32,
    pub ready: u32,
    pub reserve: u32,
    pub reload: f32,
    pub heat: f32,
    pub cool: f32,
    pub damage: f32,
    pub splash: f32,
    pub range: f32,
    pub anti_armor: f32,
    pub noise: f32,
}

impl MountedWeapon {
    pub fn spec(self) -> MountedWeaponSpec {
        match self {
            Self::Coax762 => MountedWeaponSpec {
                name_zh: "7.62 毫米同軸機槍",
                name_en: "7.62 mm coaxial MG",
                cooldown: 0.086,
                ready: 250,
                reserve: 1750,
                reload: 6.0,
                heat: 0.55,
                cool: 1.75,
                damage: 58.0,
                splash: 0.0,
                range: 70.0,
                anti_armor: 0.0,
                noise: 70.0,
            },
            Self::Autocannon30 => MountedWeaponSpec {
                name_zh: "30 毫米機砲",
                name_en: "30 mm autocannon",
                cooldown: 0.2,
                ready: 200,
                reserve: 200,
                reload: 9.0,
                heat: 1.6,
                cool: 2.5,
                damage: 150.0,
                splash: 2.4,
                range: 90.0,
                anti_armor: 6.0,
                noise: 95.0,
            },
            Self::Cannon105 => MountedWeaponSpec {
                name_zh: "105 毫米砲",
                name_en: "105 mm gun",
                cooldown: 7.5,
                ready: 8,
                reserve: 37,
                reload: 22.0,
                heat: 6.0,
                cool: 0.5,
                damage: 400.0,
                splash: 8.0,
                range: 120.0,
                anti_armor: 70.0,
                noise: 150.0,
            },
            Self::Heavy127 => MountedWeaponSpec {
                name_zh: "12.7 毫米重機槍",
                name_en: "12.7 mm heavy MG",
                cooldown: 0.11,
                ready: 60,
                reserve: 240,
                reload: 7.0,
                heat: 0.7,
                cool: 1.9,
                damage: 110.0,
                splash: 0.0,
                range: 85.0,
                anti_armor: 1.0,
                noise: 85.0,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountedGun {
    pub weapon: MountedWeapon,
    pub ready: u32,
    pub reserve: u32,
    pub heat: f32,
    pub jammed: bool,
    pub reload_remaining: f32,
    pub cooldown_remaining: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountedGunState {
    pub guns: Vec<MountedGun>,
    pub selected: usize,
}

fn loadout(kind: VehicleKind) -> &'static [MountedWeapon] {
    use MountedWeapon::*;
    match kind {
        VehicleKind::Zbl08 | VehicleKind::Zbd05 => &[Autocannon30, Coax762],
        VehicleKind::Ztd05 => &[Cannon105, Coax762],
        VehicleKind::Mengshi => &[Heavy127],
        _ => &[],
    }
}

pub fn traverse_speed(kind: VehicleKind) -> f32 {
    match kind {
        VehicleKind::Zbl08 | VehicleKind::Zbd05 => 1.1,
        VehicleKind::Ztd05 => 0.7,
        VehicleKind::Mengshi => 1.8,
        _ => 0.0,
    }
}

impl MountedGunState {
    pub fn new(kind: VehicleKind) -> Self {
        Self {
            guns: loadout(kind)
                .iter()
                .map(|&weapon| {
                    let s = weapon.spec();
                    MountedGun {
                        weapon,
                        ready: s.ready,
                        reserve: s.reserve,
                        heat: 0.0,
                        jammed: false,
                        reload_remaining: 0.0,
                        cooldown_remaining: 0.0,
                    }
                })
                .collect(),
            selected: 0,
        }
    }

    pub fn validate(&self, kind: VehicleKind) -> bool {
        let want = loadout(kind);
        self.guns.len() == want.len()
            && !want.is_empty()
            && self.selected < want.len()
            && self.guns.iter().zip(want).all(|(g, weapon)| {
                let s = weapon.spec();
                g.weapon == *weapon
                    && g.ready <= s.ready
                    && g.reserve <= s.reserve + s.ready
                    && g.ready + g.reserve <= s.ready + s.reserve
                    && finite(g.heat, 0.0, 100.0)
                    && finite(g.reload_remaining, 0.0, s.reload)
                    && finite(g.cooldown_remaining, 0.0, s.cooldown)
                    && (!g.jammed || g.heat > 45.0)
            })
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        if !dt.is_finite() || dt < 0.0 {
            return false;
        }
        for gun in &mut self.guns {
            let spec = gun.weapon.spec();
            gun.cooldown_remaining = (gun.cooldown_remaining - dt).max(0.0);
            gun.heat = (gun.heat - spec.cool * dt).max(0.0);
            if gun.jammed && gun.heat <= 45.0 {
                gun.jammed = false;
            }
            if gun.reload_remaining > 0.0 {
                gun.reload_remaining = (gun.reload_remaining - dt).max(0.0);
                if gun.reload_remaining == 0.0 {
                    let n = spec.ready.min(gun.reserve);
                    gun.ready = n;
                    gun.reserve -= n;
                }
            }
            if gun.ready == 0 && gun.reload_remaining == 0.0 && gun.reserve > 0 {
                gun.reload_remaining = spec.reload;
            }
        }
        self.can_fire()
    }

    pub fn can_fire(&self) -> bool {
        self.guns.get(self.selected).is_some_and(|g| {
            g.ready > 0 && g.reload_remaining <= 0.0 && !g.jammed && g.cooldown_remaining <= 0.0
        })
    }

    pub fn fire(&mut self) -> Option<MountedWeaponSpec> {
        if !self.can_fire() {
            return None;
        }
        let gun = &mut self.guns[self.selected];
        let spec = gun.weapon.spec();
        gun.ready -= 1;
        gun.cooldown_remaining = spec.cooldown;
        gun.heat = (gun.heat + spec.heat).min(100.0);
        gun.jammed |= gun.heat >= 100.0;
        Some(spec)
    }

    pub fn reload(&mut self) -> bool {
        let Some(gun) = self.guns.get_mut(self.selected) else {
            return false;
        };
        let spec = gun.weapon.spec();
        if gun.reload_remaining > 0.0 || gun.reserve == 0 || gun.ready >= spec.ready {
            return false;
        }
        gun.reserve += gun.ready;
        gun.ready = 0;
        gun.reload_remaining = spec.reload;
        true
    }

    pub fn switch(&mut self) {
        if !self.guns.is_empty() {
            self.selected = (self.selected + 1) % self.guns.len();
        }
    }
}

fn finite(value: f32, min: f32, max: f32) -> bool {
    value.is_finite() && (min..=max).contains(&value)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmorState {
    pub id: u32,
    pub kind: VehicleKind,
    pub route: Vec<[f32; 2]>,
    pub segment: usize,
    pub t: f32,
    pub direction: i8,
    pub pos: [f32; 2],
    /// +Z-forward bearing, shared with drive.ts (the original armour mesh is +X).
    pub heading: f32,
    pub turret_yaw: f32,
    pub hp: f32,
    pub destroyed: bool,
    pub abandoned: bool,
    pub captured_id: Option<u32>,
    pub fire_cd: f32,
    pub spot: f32,
    pub alerted: bool,
    pub stop: f32,
}

impl ArmorState {
    pub fn new(id: u32, kind: VehicleKind, route: Vec<Vec2>, segment: usize) -> Self {
        let segment = segment.min(route.len().saturating_sub(2));
        let pos = route.get(segment).copied().unwrap_or_default();
        let direction = route.get(segment + 1).copied().unwrap_or(pos + Vec2::Y) - pos;
        Self {
            id,
            kind,
            route: route.iter().map(|p| p.to_array()).collect(),
            segment,
            t: 0.0,
            direction: 1,
            pos: pos.to_array(),
            heading: direction.x.atan2(direction.y),
            turret_yaw: 0.0,
            hp: if kind == VehicleKind::Mengshi {
                40.0
            } else {
                100.0
            },
            destroyed: false,
            abandoned: false,
            captured_id: None,
            fire_cd: 0.0,
            spot: 0.0,
            alerted: false,
            stop: 0.0,
        }
    }

    pub fn position(&self) -> Vec2 {
        Vec2::from_array(self.pos)
    }
    pub fn active(&self) -> bool {
        !self.destroyed && !self.abandoned
    }
    fn collider(&self) -> crate::map::collide::DynamicCollider {
        let (length, width) = match self.kind {
            VehicleKind::Mengshi => (4.9, 2.2),
            VehicleKind::Zbl08 => (8.0, 3.0),
            _ => (9.5, 3.4),
        };
        crate::map::collide::DynamicCollider {
            id: 0x1_0000_0000 + u64::from(self.id),
            corners: crate::vehicles::footprint(self.position(), self.heading, length, width),
            height: 2.5,
        }
    }
    pub fn patrol(&mut self, dt: f32) {
        if !self.active() || self.route.len() < 2 {
            return;
        }
        if self.stop > 0.0 {
            self.stop = (self.stop - dt).max(0.0);
            return;
        }
        let p = Vec2::from_array(self.route[self.segment]);
        let q = Vec2::from_array(self.route[self.segment + 1]);
        let speed = if self.kind == VehicleKind::Mengshi {
            7.0
        } else {
            4.5
        };
        self.t += self.direction as f32 * speed * if self.alerted { 0.4 } else { 1.0 } * dt
            / p.distance(q).max(1.0);
        if self.t > 1.0 {
            self.segment += 1;
            self.t = 0.0;
            if self.segment >= self.route.len() - 1 {
                self.segment = self.route.len() - 2;
                self.t = 1.0;
                self.direction = -1;
                self.stop = 4.0;
            }
        } else if self.t < 0.0 {
            if self.segment == 0 {
                self.t = 0.0;
                self.direction = 1;
                self.stop = 4.0;
            } else {
                self.segment -= 1;
                self.t = 1.0;
            }
        }
        let a = Vec2::from_array(self.route[self.segment]);
        let b = Vec2::from_array(self.route[self.segment + 1]);
        self.pos = a.lerp(b, self.t).to_array();
        let direction = (q - p) * self.direction as f32;
        self.heading +=
            angle_delta(direction.x.atan2(direction.y), self.heading) * (dt * 2.0).min(1.0);
    }
}

fn angle_delta(want: f32, current: f32) -> f32 {
    let d = want - current;
    d.sin().atan2(d.cos())
}

#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmorFleet {
    pub units: Vec<ArmorState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmorImpact {
    Miss,
    Ricochet,
    Damaged,
    Abandoned,
    Destroyed,
}

impl Default for ArmorFleet {
    fn default() -> Self {
        Self::planned(crate::map::world())
    }
}

impl ArmorFleet {
    pub fn publish_colliders(&self, obstacles: &crate::map::Obstacles) {
        obstacles.replace_dynamic(
            2,
            self.units
                .iter()
                .filter(|a| !a.abandoned)
                .map(ArmorState::collider)
                .collect(),
        );
    }

    pub fn planned(world: &crate::map::WorldData) -> Self {
        let length =
            |r: &crate::map::data::Road| r.p.windows(2).map(|s| s[0].distance(s[1])).sum::<f32>();
        let mut roads: Vec<_> = world
            .roads
            .iter()
            .filter(|r| {
                (r.t.starts_with("primary")
                    || r.t.starts_with("secondary")
                    || r.t.starts_with("trunk")
                    || [
                        "西園路",
                        "環河南路",
                        "和平西路",
                        "康定路",
                        "中華路",
                        "艋舺大道",
                    ]
                    .iter()
                    .any(|name| r.name.contains(name)))
                    && r.p.len() >= 2
                    && r.w >= 9.0
                    && length(r) > 70.0
                    && r.p.iter().any(|p| p.distance(world.spawn) < 420.0)
            })
            .collect();
        roads.sort_by(|a, b| length(b).total_cmp(&length(a)));
        let kinds = [
            VehicleKind::Zbl08,
            VehicleKind::Zbd05,
            VehicleKind::Mengshi,
            VehicleKind::Ztd05,
        ];
        let mut units = Vec::new();
        let mut used = std::collections::HashSet::new();
        for road in &roads {
            if units.len() == kinds.len() {
                break;
            }
            let key = (
                road.name.as_str(),
                road.p[0].x.to_bits(),
                road.p[0].y.to_bits(),
            );
            if !used.insert(key) || road.p.iter().any(|p| p.distance(world.spawn) < 30.0) {
                continue;
            }
            units.push(ArmorState::new(
                units.len() as u32,
                kinds[units.len()],
                road.p.clone(),
                road.p.len() / 3,
            ));
        }
        // The three burnt roadside hulls from setupArmor are durable scenery.
        for road in roads.iter().rev() {
            let count = units.iter().filter(|u| u.destroyed).count();
            if count >= 3 {
                break;
            }
            let q = road.p[road.p.len() / 2];
            if q.distance(world.spawn) < 25.0 {
                continue;
            }
            let q2 = road
                .p
                .get(road.p.len() / 2 + 1)
                .copied()
                .unwrap_or(road.p[0]);
            let d = (q2 - q).normalize_or_zero();
            let p = q + Vec2::new(-d.y, d.x) * (road.w / 2.0 - 2.2);
            let kind = [VehicleKind::Zbd05, VehicleKind::Mengshi, VehicleKind::Zbl08][count];
            let mut wreck = ArmorState::new(100 + count as u32, kind, vec![p, p + d], 0);
            wreck.heading += 0.3;
            wreck.destroyed = true;
            wreck.hp = 0.0;
            units.push(wreck);
        }
        Self { units }
    }

    pub fn validate(&self) -> bool {
        let world = crate::map::world();
        let valid_point = |p: [f32; 2]| {
            p.iter().all(|v| v.is_finite())
                && Vec2::from_array(p).cmpge(world.min - 256.0).all()
                && Vec2::from_array(p).cmple(world.max + 256.0).all()
        };
        self.units.len() <= 128
            && self
                .units
                .iter()
                .map(|a| a.id)
                .collect::<std::collections::HashSet<_>>()
                .len()
                == self.units.len()
            && self.units.iter().all(|a| {
                a.id < 100_000
                    && !loadout(a.kind).is_empty()
                    && a.route.len() >= 2
                    && a.route.len() <= 10_000
                    && a.segment < a.route.len().saturating_sub(1)
                    && a.route.iter().all(|p| valid_point(*p))
                    && valid_point(a.pos)
                    && finite(a.t, 0.0, 1.0)
                    && matches!(a.direction, -1 | 1)
                    && a.heading.is_finite()
                    && a.turret_yaw.is_finite()
                    && finite(
                        a.hp,
                        0.0,
                        if a.kind == VehicleKind::Mengshi {
                            40.0
                        } else {
                            100.0
                        },
                    )
                    && (a.destroyed == (a.hp == 0.0))
                    && !(a.destroyed && a.abandoned)
                    && (!a.abandoned || a.hp <= 30.0)
                    && (a.captured_id.is_none() || a.abandoned)
                    && finite(a.fire_cd, 0.0, 0.12)
                    && finite(a.spot, 0.0, 1_000_000.0)
                    && finite(a.stop, 0.0, 6.0)
            })
    }

    pub fn hit_at(&mut self, point: Vec2, damage: f32, anti_armor: bool) -> ArmorImpact {
        let Some(a) = self
            .units
            .iter_mut()
            .find(|a| a.active() && a.position().distance(point) <= 4.0)
        else {
            return ArmorImpact::Miss;
        };
        Self::damage_unit(a, damage, anti_armor)
    }

    fn hit_id(&mut self, id: u32, damage: f32, anti_armor: bool) -> ArmorImpact {
        let Some(a) = self.units.iter_mut().find(|a| a.id == id && a.active()) else {
            return ArmorImpact::Miss;
        };
        Self::damage_unit(a, damage, anti_armor)
    }

    fn damage_unit(a: &mut ArmorState, damage: f32, anti_armor: bool) -> ArmorImpact {
        if !damage.is_finite() || damage <= 0.0 {
            return ArmorImpact::Miss;
        }
        if !anti_armor {
            return ArmorImpact::Ricochet;
        }
        a.hp = (a.hp - damage).max(0.0);
        if a.hp == 0.0 {
            a.destroyed = true;
            ArmorImpact::Destroyed
        } else if a.hp <= 30.0 {
            a.abandoned = true;
            ArmorImpact::Abandoned
        } else {
            ArmorImpact::Damaged
        }
    }
}

#[derive(Resource, Default)]
struct ArmorShield {
    hp_before: Option<f32>,
}

#[derive(Resource, Default)]
struct ArmorProximity {
    heard: bool,
}

impl ArmorProximity {
    fn observe(&mut self, nearest: f32, urgent: bool) -> bool {
        if nearest > 100.0 {
            self.heard = false;
        }
        if nearest < 60.0 && !self.heard {
            self.heard = true;
            return !urgent;
        }
        false
    }
}

/// Native collision substeps must not turn one contact into damage every frame.
/// The Web ram values are unchanged; a pair rearms after separating and at least
/// one second has elapsed. These contacts are transient, never part of a save.
#[derive(Resource, Default)]
struct ArmorRamContacts {
    current: Option<u32>,
    contacts: std::collections::HashMap<u32, RamContact>,
}

#[derive(Default)]
struct RamContact {
    touching: bool,
    cooldown: f32,
}

/// Desktop renderer captures hold patrols still while comparing identical views.
#[derive(Resource, Default)]
pub(crate) struct ArmorQaFreeze(pub bool);

#[derive(Resource, Default)]
struct ArmorArt {
    meshes: std::collections::HashMap<VehicleKind, [Handle<Mesh>; 3]>,
    normal: Handle<StandardMaterial>,
    burned: Handle<StandardMaterial>,
}

#[derive(Component)]
struct ArmorVisual {
    id: u32,
    destroyed: bool,
}

#[derive(Component)]
struct ArmorTurret(u32);

pub struct ArmorPlugin;

impl Plugin for ArmorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ArmorFleet>()
            .init_resource::<ArmorShield>()
            .init_resource::<ArmorProximity>()
            .init_resource::<ArmorRamContacts>()
            .init_resource::<ArmorQaFreeze>()
            .add_systems(Startup, setup_art)
            .add_systems(PreUpdate, clear_loaded_contacts)
            .add_systems(OnEnter(crate::game_flow::GameFlow::Prologue), reset_armor)
            .add_systems(
                Update,
                shield_begin
                    .after(crate::player::MoveSet)
                    .before(crate::vehicles::VehicleSet)
                    .before(super::CombatSet),
            )
            .add_systems(
                Update,
                (
                    handle_armor_requests,
                    armor_tick,
                    armor_ramming,
                    mounted_fire,
                    transfer_abandoned,
                    shield_end,
                )
                    .chain()
                    .after(super::player_fire)
                    .before(super::player_vitals)
                    .in_set(super::CombatSet),
            )
            .add_systems(PostUpdate, sync_art.before(TransformSystems::Propagate));
    }
}

fn reset_armor(
    mut fleet: ResMut<ArmorFleet>,
    mut shield: ResMut<ArmorShield>,
    mut contacts: ResMut<ArmorRamContacts>,
    mut proximity: ResMut<ArmorProximity>,
) {
    *fleet = ArmorFleet::default();
    *shield = ArmorShield::default();
    *contacts = ArmorRamContacts::default();
    *proximity = ArmorProximity::default();
}

fn clear_loaded_contacts(
    mut loaded: MessageReader<crate::save::LoadCompleted>,
    mut contacts: ResMut<ArmorRamContacts>,
    mut shield: ResMut<ArmorShield>,
    mut proximity: ResMut<ArmorProximity>,
) {
    if loaded.read().count() > 0 {
        *contacts = ArmorRamContacts::default();
        *shield = ArmorShield::default();
        *proximity = ArmorProximity::default();
    }
}

fn shield_begin(
    state: Res<super::GameState>,
    fleet: Res<crate::vehicles::VehicleFleet>,
    mut shield: ResMut<ArmorShield>,
) {
    shield.hp_before = fleet
        .current()
        .filter(|v| v.gun.is_some())
        .map(|_| state.hp);
}

fn shield_end(mut state: ResMut<super::GameState>, shield: Res<ArmorShield>) {
    if let Some(hp) = shield.hp_before {
        if state.hp < hp {
            state.hp = hp - (hp - state.hp) * 0.1;
        }
        state.bleed = false;
    }
}

fn handle_armor_requests(
    mut requests: MessageReader<super::SpawnEnemy>,
    mut fleet: ResMut<ArmorFleet>,
) {
    for request in requests.read() {
        let kind = match request.kind {
            super::EnemyKind::Zbl08 => VehicleKind::Zbl08,
            super::EnemyKind::Zbd05 => VehicleKind::Zbd05,
            super::EnemyKind::Mengshi => VehicleKind::Mengshi,
            super::EnemyKind::Ztd05 => VehicleKind::Ztd05,
            _ => continue,
        };
        if fleet.units.len() >= 128 {
            continue;
        }
        let id = fleet
            .units
            .iter()
            .map(|a| a.id)
            .max()
            .unwrap_or(100)
            .checked_add(1)
            .filter(|id| *id < 100_000)
            .unwrap_or_else(|| {
                (0..128)
                    .find(|id| fleet.units.iter().all(|a| a.id != *id))
                    .expect("Fewer than 128 units leave a free bounded armor ID")
            });
        let p = request.pos.xz();
        fleet
            .units
            .push(ArmorState::new(id, kind, vec![p, p + Vec2::X * 12.0], 0));
    }
}

#[derive(bevy::ecs::system::SystemParam)]
struct ArmorContext<'w> {
    time: Res<'w, Time>,
    flow: Res<'w, State<crate::game_flow::GameFlow>>,
    pause: Res<'w, crate::ui::PauseOpen>,
    modal: Res<'w, crate::ui::UiModal>,
    world: Res<'w, super::CombatWorldRes>,
    env: Res<'w, super::CombatEnv>,
    rng: ResMut<'w, super::CombatRng>,
    state: ResMut<'w, super::GameState>,
    hits: MessageWriter<'w, super::PlayerHit>,
    sound: MessageWriter<'w, crate::audio::SfxEvent>,
    calls: MessageWriter<'w, super::Callout>,
    lang: Res<'w, crate::i18n::Language>,
    obstacles: Res<'w, crate::map::Obstacles>,
    notice: ResMut<'w, crate::worldplay::WorldNotice>,
}

impl ArmorContext<'_> {
    fn live(&self) -> bool {
        *self.flow.get() == crate::game_flow::GameFlow::Playing && !self.pause.0 && !self.state.over
    }
}

fn armor_tick(
    mut ctx: ArmorContext,
    mut fleet: ResMut<ArmorFleet>,
    player: Query<&Transform, With<crate::player::Player>>,
    mut soldiers: Query<&mut super::Enemy>,
    mut commands: Commands,
    qa: Res<ArmorQaFreeze>,
    mut proximity: ResMut<ArmorProximity>,
) {
    if !ctx.live() || qa.0 {
        return;
    }
    let Ok(player) = player.single() else {
        return;
    };
    let p = player.translation.xz();
    let dt = ctx.time.delta_secs();
    let mut urgent = ctx.state.stun > 0.0
        || (ctx.notice.is_changed() && !ctx.notice.text.is_empty())
        || soldiers.iter_mut().any(|s| {
            s.0.alive()
                && s.0.active(ctx.env.night)
                && s.0.state == super::SolState::Combat
                && s.0.pos.distance(p) < 60.0
        });
    for a in &mut fleet.units {
        if !a.active() {
            continue;
        }
        a.patrol(dt);
        let from = a.position();
        let seen = from.distance(p) < 38.0
            && ctx.world.0.building_at(p).is_none()
            && ctx.world.0.shot_ray(from, p, 2.8, 1.2, 4.0) >= 1.0;
        if seen {
            a.spot += dt;
            let want = (p.x - from.x).atan2(p.y - from.y) - a.heading;
            a.turret_yaw += angle_delta(want, a.turret_yaw) * (dt * 3.0).min(1.0);
            if !a.alerted && a.spot > 0.6 {
                a.alerted = true;
                a.stop = 6.0;
                let mut candidates: Vec<_> = soldiers
                    .iter_mut()
                    .filter(|s| s.0.alive() && s.0.post.is_none())
                    .map(|s| (s.0.id, s.0.pos.distance_squared(from)))
                    .collect();
                candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
                for mut s in &mut soldiers {
                    if candidates.iter().take(3).any(|(id, _)| *id == s.0.id) {
                        s.0.state = super::SolState::Investigate;
                        s.0.target = Some(p);
                        s.0.timer = 300.0;
                    }
                }
                let text = crate::i18n::tr(
                    *ctx.lang,
                    "裝甲巡邏發現你！快躲進騎樓或巷子。",
                    "Armour spotted you! Get under an arcade or into an alley.",
                );
                ctx.notice.text = text.into();
                ctx.calls.write(super::Callout {
                    pos: Vec3::new(from.x, 2.8, from.y),
                    text,
                    audible: true,
                });
            }
            urgent |= a.alerted;
            a.fire_cd = (a.fire_cd - dt).max(0.0);
            if a.alerted && a.fire_cd <= 0.0 {
                a.fire_cd = 0.12;
                super::tracer(
                    &mut commands,
                    Vec3::new(from.x, 2.8, from.y),
                    Vec3::new(p.x, 1.2, p.y),
                );
                ctx.sound.write(crate::audio::SfxEvent::Gunshot {
                    kind: crate::audio::GunKind::Mg,
                    dist: from.distance(p),
                    indoor: false,
                });
                if ctx.rng.0.next_f32() < 0.13 {
                    let damage = 7.0 + ctx.rng.0.next_f32() * 6.0;
                    ctx.state.hp -= damage;
                    ctx.state.stun = 0.25;
                    let bleed = !ctx.state.bleed && ctx.rng.0.next_f32() < 0.35;
                    ctx.state.bleed |= bleed;
                    ctx.hits.write(super::PlayerHit {
                        dmg: damage,
                        cause: super::HitCause::Rifle,
                        bleed,
                    });
                }
            }
        } else {
            a.spot = (a.spot - dt).max(0.0);
            if a.spot == 0.0 && a.alerted && ctx.rng.0.next_f32() < dt * 0.05 {
                a.alerted = false;
            }
        }
    }
    let nearest = fleet
        .units
        .iter()
        .filter(|a| a.active())
        .map(|a| a.position().distance(p))
        .fold(f32::INFINITY, f32::min);
    if proximity.observe(nearest, urgent || ctx.state.stun > 0.0) {
        ctx.notice.text = crate::i18n::tr(
            *ctx.lang,
            "聽見履帶與柴油引擎聲……裝甲車在附近大馬路上",
            "Tracks and a diesel engine... An armored vehicle is on a nearby main road.",
        )
        .into();
    }
    fleet.publish_colliders(&ctx.obstacles);
}

fn armor_ramming(
    mut ctx: ArmorContext,
    armor: Res<ArmorFleet>,
    mut vehicles: ResMut<crate::vehicles::VehicleFleet>,
    mut contacts: ResMut<ArmorRamContacts>,
    mut player: Query<&mut Transform, With<crate::player::Player>>,
    qa: Res<ArmorQaFreeze>,
) {
    if !ctx.live() || qa.0 {
        return;
    }
    let Some(vehicle) = vehicles.current_mut() else {
        *contacts = ArmorRamContacts::default();
        return;
    };
    let Ok(mut player) = player.single_mut() else {
        return;
    };
    if contacts.current != Some(vehicle.id) {
        contacts.current = Some(vehicle.id);
        contacts.contacts.clear();
    }
    let dt = ctx.time.delta_secs().clamp(0.0, 1.0);
    for contact in contacts.contacts.values_mut() {
        contact.cooldown = (contact.cooldown - dt).max(0.0);
    }
    contacts.contacts.retain(|id, contact| {
        (contact.touching || contact.cooldown > 0.0)
            && armor.units.iter().any(|a| a.id == *id && a.active())
    });
    for unit in armor.units.iter().filter(|a| a.active()) {
        let separation = vehicle.position() - unit.position();
        let distance = separation.length();
        let radius = 3.0 + vehicle.spec().length * 0.3;
        if distance >= radius + 0.5 {
            if let Some(contact) = contacts.contacts.get_mut(&unit.id) {
                contact.touching = false;
            }
            continue;
        }
        if distance >= radius {
            continue;
        }
        let contact = contacts.contacts.entry(unit.id).or_default();
        if contact.touching {
            continue;
        }
        contact.touching = true;
        if contact.cooldown > 0.0 {
            continue;
        }
        contact.cooldown = 1.0;

        // drive.ts: one ram removes half the engine condition, leaves 20%
        // tyres, stops the vehicle, and deals 35 HP on two wheels or 15 in a car.
        vehicle.engine = (vehicle.engine - 0.5).max(0.0);
        vehicle.tyre = 0.2;
        vehicle.speed = 0.0;
        let damage = if vehicle.spec().two { 35.0 } else { 15.0 };
        ctx.state.hp -= damage;
        ctx.hits.write(super::PlayerHit {
            dmg: damage,
            cause: super::HitCause::RifleButt,
            bleed: false,
        });
        ctx.sound.write(crate::audio::SfxEvent::MeleeHit {
            dist: 0.0,
            blunt: true,
        });
        ctx.notice.text = crate::i18n::tr(
            *ctx.lang,
            "裝甲車直接撞上來！",
            "An armored vehicle rams you!",
        )
        .into();

        // Preserve the original half-separation shove in open space. Sweep the
        // complete hull so a long vehicle cannot be pushed through a thin wall.
        let shove = separation * 0.5;
        let steps = (shove.length() / 0.25).ceil().max(1.0) as u32;
        let step = shove / steps as f32;
        let ignored = [u64::from(vehicle.id), 0x1_0000_0000 + u64::from(unit.id)];
        for _ in 0..steps {
            let next = vehicle.position() + step;
            if ctx
                .obstacles
                .footprint_blocked_ignoring(vehicle.corners_at(next, vehicle.heading), &ignored)
            {
                break;
            }
            vehicle.pos = next.to_array();
        }
        player.translation = Vec3::new(vehicle.pos[0], 0.0, vehicle.pos[1]);
        println!(
            "DTARMOR action=ram state={}",
            serde_json::json!({"armor_id":unit.id,"vehicle_id":vehicle.id,"damage":damage,"engine":vehicle.engine,"tyre":vehicle.tyre,"position":vehicle.pos})
        );
    }
    vehicles.publish_colliders(&ctx.obstacles);
}

fn transfer_abandoned(
    mut fleet: ResMut<ArmorFleet>,
    mut vehicles: ResMut<crate::vehicles::VehicleFleet>,
    mut notice: ResMut<crate::worldplay::WorldNotice>,
    lang: Res<crate::i18n::Language>,
) {
    for unit in &mut fleet.units {
        if unit.abandoned && unit.captured_id.is_none() {
            unit.captured_id =
                Some(vehicles.add_captured(unit.kind, unit.position(), unit.heading));
            notice.text = crate::i18n::tr(
                *lang,
                "乘員棄車逃了！這台裝甲車還能開，也可以繼續修復。",
                "The crew bailed out! This armoured vehicle can still be driven and repaired.",
            )
            .into();
        }
    }
}

fn mounted_fire(
    mut commands: Commands,
    mut ctx: ArmorContext,
    input: Res<super::WeaponInput>,
    mut vehicles: ResMut<crate::vehicles::VehicleFleet>,
    mut armor: ResMut<ArmorFleet>,
    mut enemies: Query<(Entity, &mut super::Enemy, &Transform)>,
    mut died: MessageWriter<super::EnemyDied>,
    fx: Option<Res<super::standin::CombatFxAssets>>,
) {
    if !ctx.live() {
        return;
    }
    let Some(v) = vehicles.current_mut() else {
        return;
    };
    let position = v.position();
    let Some(gun) = &mut v.gun else {
        return;
    };
    let dt = ctx.time.delta_secs();
    let reloading = gun.guns.iter().enumerate().fold(0_u8, |mask, (i, g)| {
        mask | (u8::from(g.reload_remaining > 0.0) << i)
    });
    let can = gun.tick(dt);
    if gun
        .guns
        .iter()
        .enumerate()
        .any(|(i, g)| reloading & (1 << i) != 0 && g.reload_remaining == 0.0 && g.ready > 0)
    {
        println!(
            "DTVEHICLE action=reload_complete state={}",
            serde_json::json!({"vehicle_id":v.id,"selected":gun.selected,"guns":gun.guns})
        );
    }
    if ctx.modal.blocks_input() {
        return;
    }
    let target = if input.aim.length_squared() > 0.0 {
        Some(position + input.aim * 60.0)
    } else {
        enemies
            .iter()
            .filter(|(_, s, _)| {
                s.0.alive()
                    && s.0.active(ctx.env.night)
                    && s.0.pos.distance(position) < 60.0
                    && ctx.world.0.shot_ray(position, s.0.pos, 2.0, 1.2, 5.0) >= 1.0
            })
            .map(|(_, s, _)| s.0.pos)
            .chain(
                armor
                    .units
                    .iter()
                    .filter(|a| a.active() && a.position().distance(position) < 60.0)
                    .map(|a| a.position()),
            )
            .min_by(|a, b| {
                a.distance_squared(position)
                    .total_cmp(&b.distance_squared(position))
            })
    };
    if let Some(target) = target {
        let want = (target.x - position.x).atan2(target.y - position.y) - v.heading;
        let d = angle_delta(want, v.turret_yaw);
        v.turret_yaw += d.signum() * d.abs().min(traverse_speed(v.kind) * dt);
    }
    if !input.fire || !can {
        return;
    }
    let Some(weapon) = gun.fire() else {
        return;
    };
    let yaw = v.heading + v.turret_yaw;
    let direction = Vec2::new(yaw.sin(), yaw.cos());
    let origin = position + direction * (v.spec().length * 0.5);
    let mut reach = weapon.range
        * ctx
            .world
            .0
            .shot_ray(origin, origin + direction * weapon.range, 1.9, 1.2, 1.5);
    let mut hit = None;
    let mut actor_reach = weapon.range;
    for (entity, s, _) in &enemies {
        let to = s.0.pos - origin;
        let along = to.dot(direction);
        if s.0.alive()
            && s.0.active(ctx.env.night)
            && along >= 0.0
            && along <= reach
            && (to - direction * along).length() < 0.75
        {
            hit = Some(entity);
            reach = along;
            actor_reach = along;
        }
    }
    let mut hull = None;
    for a in &armor.units {
        let to = a.position() - origin;
        let along = to.dot(direction);
        if a.active()
            && along >= 0.0
            && along <= actor_reach
            && (to - direction * along).length() < 2.2
        {
            // The dynamic hull itself may be the first collision: its centre is
            // inside that surface, unlike an infantry ray target.
            if ctx
                .obstacles
                .segment_hit_except(
                    origin,
                    origin + direction * along,
                    1.9,
                    1.9 + (1.2 - 1.9) * (along / weapon.range),
                    Some(0x1_0000_0000 + u64::from(a.id)),
                )
                .is_none()
            {
                reach = along;
                actor_reach = along;
                hit = None;
                hull = Some(a.id);
            }
        }
    }
    let end = origin + direction * reach;
    for (entity, mut enemy, tf) in &mut enemies {
        let distance = enemy.0.pos.distance(end);
        let direct = hit == Some(entity);
        let splash = !direct
            && weapon.splash > 0.0
            && distance < weapon.splash
            && ctx
                .world
                .0
                .shot_ray(end - direction * 0.3, enemy.0.pos, 1.0, 1.0, 0.0)
                >= 1.0;
        if !enemy.0.alive() || (!direct && !splash) {
            continue;
        }
        let damage = if direct {
            weapon.damage
        } else {
            weapon.damage * 0.5 * (1.0 - distance / weapon.splash)
        };
        if super::soldier::damage(&mut enemy.0, damage, position) {
            ctx.state.kills += 1;
            commands.entity(entity).insert(super::Body {
                kind: enemy.0.kind,
                t: super::tuning::BODY_SECS,
            });
            died.write(super::EnemyDied {
                id: enemy.0.id,
                kind: enemy.0.kind,
                pos: tf.translation,
            });
        }
    }
    if let Some(id) = hull
        && weapon.anti_armor > 0.0
        && armor.hit_id(id, weapon.anti_armor, true) == ArmorImpact::Destroyed
    {
        ctx.state.kills += 1;
    }
    println!(
        "DTARMOR action=mounted_fire state={}",
        serde_json::json!({"vehicle_id":v.id,"selected":v.gun.as_ref().map(|g|g.selected),"gun":v.gun,"hull_hit":hull.is_some(),"hull_id":hull,"impact":end.to_array()})
    );
    if weapon.splash > 0.0
        && let Some(fx) = fx.as_deref()
    {
        super::standin::spawn_blast(&mut commands, fx, end);
    }
    super::tracer(
        &mut commands,
        Vec3::new(origin.x, 1.9, origin.y),
        Vec3::new(end.x, 0.8, end.y),
    );
    ctx.sound.write(crate::audio::SfxEvent::Gunshot {
        kind: if weapon.splash > 0.0 {
            crate::audio::GunKind::At
        } else {
            crate::audio::GunKind::Mg
        },
        dist: 0.0,
        indoor: false,
    });
    super::soldier::noise(
        enemies.iter_mut().map(|(_, e, _)| &mut e.into_inner().0),
        position,
        weapon.noise,
        ctx.env.rain,
        ctx.env.night,
        &mut ctx.rng.0,
    );
}

fn setup_art(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut art = ArmorArt {
        normal: materials.add(StandardMaterial {
            perceptual_roughness: 0.85,
            metallic: 0.15,
            ..default()
        }),
        burned: materials.add(StandardMaterial {
            base_color: Color::srgb(0.19, 0.15, 0.13),
            perceptual_roughness: 1.0,
            ..default()
        }),
        ..default()
    };
    for kind in [
        VehicleKind::Zbl08,
        VehicleKind::Zbd05,
        VehicleKind::Ztd05,
        VehicleKind::Mengshi,
    ] {
        let model = crate::vehicles::visual::model(kind);
        art.meshes.insert(
            kind,
            [
                meshes.add(model.body.into_mesh()),
                meshes.add(model.wheels.into_mesh()),
                meshes.add(model.turret.into_mesh()),
            ],
        );
    }
    commands.insert_resource(art);
}

fn sync_art(
    mut commands: Commands,
    fleet: Res<ArmorFleet>,
    player: Query<
        &Transform,
        (
            With<crate::player::Player>,
            Without<ArmorVisual>,
            Without<ArmorTurret>,
        ),
    >,
    art: Res<ArmorArt>,
    mut visuals: Query<(Entity, &ArmorVisual, &mut Transform), Without<ArmorTurret>>,
    mut turrets: Query<(&ArmorTurret, &mut Transform), Without<ArmorVisual>>,
    obstacles: Res<crate::map::Obstacles>,
) {
    let Ok(player) = player.single() else {
        return;
    };
    let position = player.translation.xz();
    let mut present = std::collections::HashSet::new();
    for (entity, marker, mut transform) in &mut visuals {
        let Some(unit) = fleet.units.iter().find(|u| {
            u.id == marker.id
                && !u.abandoned
                && u.position().distance(position) < 140.0
                && u.destroyed == marker.destroyed
        }) else {
            commands.entity(entity).despawn();
            continue;
        };
        transform.translation = Vec3::new(unit.pos[0], 0.0, unit.pos[1]);
        transform.rotation = Quat::from_rotation_y(unit.heading);
        present.insert(unit.id);
    }
    for unit in fleet.units.iter().filter(|a| {
        !a.abandoned && a.position().distance(position) < 120.0 && !present.contains(&a.id)
    }) {
        let meshes = &art.meshes[&unit.kind];
        let material = if unit.destroyed {
            &art.burned
        } else {
            &art.normal
        };
        commands
            .spawn((
                ArmorVisual {
                    id: unit.id,
                    destroyed: unit.destroyed,
                },
                Transform::from_xyz(unit.pos[0], 0.0, unit.pos[1])
                    .with_rotation(Quat::from_rotation_y(unit.heading)),
                Visibility::default(),
            ))
            .with_children(|children| {
                children.spawn((
                    Mesh3d(meshes[0].clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                ));
                children.spawn((
                    Mesh3d(meshes[1].clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                ));
                children.spawn((
                    ArmorTurret(unit.id),
                    Mesh3d(meshes[2].clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                ));
            });
    }
    for (turret, mut transform) in &mut turrets {
        if let Some(unit) = fleet.units.iter().find(|a| a.id == turret.0) {
            transform.rotation = Quat::from_euler(
                EulerRot::YXZ,
                unit.turret_yaw,
                0.0,
                if unit.destroyed { 0.25 } else { 0.0 },
            );
        }
    }
    fleet.publish_colliders(&obstacles);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::message::{MessageCursor, Messages};
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;

    const TEST_ORIGIN: Vec2 = Vec2::new(10_000.0, 10_000.0);

    // The production map owns private spatial grids. Keep its real query code
    // and place these isolated fixtures outside its streets, inside test bounds.
    fn test_obstacles(wall_z: Option<f32>) -> crate::map::Obstacles {
        fn terrain(wall_z: Option<f32>) -> Arc<crate::map::WorldData> {
            let mut world = crate::map::WorldData::load();
            world.min = TEST_ORIGIN - Vec2::splat(200.0);
            world.max = TEST_ORIGIN + Vec2::splat(200.0);
            if let Some(z) = wall_z {
                world.add_seg(crate::map::data::Seg {
                    a: TEST_ORIGIN + Vec2::new(-20.0, z),
                    b: TEST_ORIGIN + Vec2::new(20.0, z),
                    h: None,
                    bld: None,
                });
            }
            Arc::new(world)
        }
        static OPEN: OnceLock<Arc<crate::map::WorldData>> = OnceLock::new();
        crate::map::Obstacles::new(match wall_z {
            Some(z) => terrain(Some(z)),
            None => OPEN.get_or_init(|| terrain(None)).clone(),
        })
    }

    fn ecs_app(kind: VehicleKind, obstacles: crate::map::Obstacles) -> App {
        let mut vehicle =
            crate::vehicles::VehicleState::new(7, kind, TEST_ORIGIN, 0.0, &mut || 0.5);
        vehicle.engine = 1.0;
        vehicle.tyre = 1.0;
        vehicle.speed = 10.0;
        let vehicles = crate::vehicles::VehicleFleet {
            vehicles: vec![vehicle],
            current: Some(7),
        };
        vehicles.publish_colliders(&obstacles);
        let mut app = App::new();
        app.init_resource::<Time>()
            .insert_resource(State::new(crate::game_flow::GameFlow::Playing))
            .init_resource::<crate::ui::PauseOpen>()
            .init_resource::<crate::ui::UiModal>()
            .init_resource::<crate::combat::GameState>()
            .init_resource::<crate::combat::CombatEnv>()
            .init_resource::<crate::combat::WeaponInput>()
            .insert_resource(crate::combat::CombatRng(crate::city::Lcg(0)))
            .insert_resource(crate::combat::CombatWorldRes(Box::new(obstacles.clone())))
            .insert_resource(obstacles)
            .insert_resource(vehicles)
            .insert_resource(ArmorFleet { units: vec![] })
            .init_resource::<ArmorShield>()
            .init_resource::<ArmorProximity>()
            .init_resource::<ArmorRamContacts>()
            .init_resource::<ArmorQaFreeze>()
            .init_resource::<crate::worldplay::WorldNotice>()
            .init_resource::<crate::i18n::Language>()
            .add_message::<crate::combat::SpawnEnemy>()
            .add_message::<crate::combat::EnemyDied>()
            .add_message::<crate::combat::PlayerHit>()
            .add_message::<crate::combat::PlayerDied>()
            .add_message::<crate::combat::Callout>()
            .add_message::<crate::audio::SfxEvent>()
            .add_message::<crate::save::LoadCompleted>();
        app.world_mut().spawn((
            crate::player::Player,
            Transform::from_xyz(TEST_ORIGIN.x, 0.0, TEST_ORIGIN.y),
        ));
        app
    }

    fn step(app: &mut App, secs: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(secs));
        app.update();
    }

    fn hostile(app: &mut App, id: u32, kind: VehicleKind, offset: Vec2) {
        let p = TEST_ORIGIN + offset;
        let mut unit = ArmorState::new(id, kind, vec![p, p + Vec2::Y * 40.0], 0);
        unit.stop = 6.0;
        app.world_mut()
            .resource_mut::<ArmorFleet>()
            .units
            .push(unit);
        app.world()
            .resource::<ArmorFleet>()
            .publish_colliders(app.world().resource::<crate::map::Obstacles>());
    }

    fn infantry(app: &mut App, id: u32, offset: Vec2) -> Entity {
        let p = TEST_ORIGIN + offset;
        app.world_mut()
            .spawn((
                crate::combat::Enemy(crate::combat::Soldier::new(
                    id,
                    crate::combat::EnemyKind::Patrol,
                    p,
                    vec![],
                )),
                Transform::from_xyz(p.x, 0.0, p.y),
            ))
            .id()
    }

    #[test]
    fn ecs_armor_fire_reaches_occupied_hull_and_shield_applies_exact_tenth_before_death() {
        for (kind, occupied, fraction, hp) in [
            (VehicleKind::Zbl08, true, 0.1, 1.0),
            (VehicleKind::Sedan, true, 1.0, 20.0),
            (VehicleKind::Zbl08, false, 1.0, 20.0),
        ] {
            let mut app = ecs_app(kind, test_obstacles(None));
            if !occupied {
                app.world_mut()
                    .resource_mut::<crate::vehicles::VehicleFleet>()
                    .current = None;
            }
            app.world_mut()
                .resource_mut::<crate::combat::GameState>()
                .hp = hp;
            app.world_mut()
                .resource_mut::<crate::combat::GameState>()
                .bleed = fraction < 1.0;
            hostile(&mut app, 1, VehicleKind::Mengshi, Vec2::Y * 20.0);
            app.world_mut().resource_mut::<ArmorFleet>().units[0].alerted = true;
            app.add_systems(
                Update,
                (
                    shield_begin,
                    armor_tick,
                    shield_end,
                    crate::combat::player_vitals,
                )
                    .chain(),
            );
            step(&mut app, 0.01);
            let mut cursor = MessageCursor::<crate::combat::PlayerHit>::default();
            let hits: Vec<_> = cursor
                .read(app.world().resource::<Messages<crate::combat::PlayerHit>>())
                .collect();
            assert_eq!(
                hits.len(),
                1,
                "A real deterministic hostile MG round must hit"
            );
            let state = app.world().resource::<crate::combat::GameState>();
            assert!((state.hp - (hp - hits[0].dmg * fraction)).abs() < 0.0001);
            assert!(!state.over, "Shielding must precede the death check");
            assert!(!state.bleed);
        }
    }

    #[test]
    fn ecs_spawn_requests_route_four_armor_types_once_without_spawning_infantry_duplicates() {
        let mut app = ecs_app(VehicleKind::Sedan, test_obstacles(None));
        app.init_resource::<crate::combat::AiTimers>().add_systems(
            Update,
            (crate::combat::handle_spawn_requests, handle_armor_requests).chain(),
        );
        for kind in [
            crate::combat::EnemyKind::Zbl08,
            crate::combat::EnemyKind::Zbd05,
            crate::combat::EnemyKind::Mengshi,
            crate::combat::EnemyKind::Ztd05,
            crate::combat::EnemyKind::Patrol,
        ] {
            app.world_mut().write_message(crate::combat::SpawnEnemy {
                pos: Vec3::new(TEST_ORIGIN.x, 0.0, TEST_ORIGIN.y),
                kind,
            });
        }
        step(&mut app, 0.01);
        step(&mut app, 0.01);
        let fleet = app.world().resource::<ArmorFleet>();
        assert_eq!(fleet.units.len(), 4);
        assert_eq!(
            fleet.units.iter().map(|a| a.kind).collect::<Vec<_>>(),
            vec![
                VehicleKind::Zbl08,
                VehicleKind::Zbd05,
                VehicleKind::Mengshi,
                VehicleKind::Ztd05
            ]
        );
        assert_eq!(
            fleet
                .units
                .iter()
                .map(|a| a.id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            4
        );
        let world = app.world_mut();
        assert_eq!(
            world.query::<&crate::combat::Enemy>().iter(world).count(),
            1
        );
    }

    #[test]
    fn ecs_ramming_uses_source_damage_and_only_rearms_after_separation_and_cooldown() {
        for (kind, raw, protected) in [
            (VehicleKind::Scooter, 35.0, 1.0),
            (VehicleKind::Sedan, 15.0, 1.0),
            (VehicleKind::Ztd05, 15.0, 0.1),
        ] {
            let mut app = ecs_app(kind, test_obstacles(None));
            hostile(&mut app, 1, VehicleKind::Mengshi, Vec2::ZERO);
            app.add_systems(Update, (shield_begin, armor_ramming, shield_end).chain());
            let mut cursor = MessageCursor::<crate::combat::PlayerHit>::default();
            step(&mut app, 0.01);
            let vehicle = app
                .world()
                .resource::<crate::vehicles::VehicleFleet>()
                .current()
                .unwrap();
            assert_eq!(
                (vehicle.engine, vehicle.tyre, vehicle.speed),
                (0.5, 0.2, 0.0)
            );
            assert!(
                (app.world().resource::<crate::combat::GameState>().hp - (100.0 - raw * protected))
                    .abs()
                    < 0.0001
            );
            assert_eq!(
                cursor
                    .read(app.world().resource::<Messages<crate::combat::PlayerHit>>())
                    .count(),
                1
            );
            for _ in 0..20 {
                step(&mut app, 0.1);
                assert_eq!(
                    cursor
                        .read(app.world().resource::<Messages<crate::combat::PlayerHit>>())
                        .count(),
                    0
                );
            }
            app.world_mut().resource_mut::<ArmorFleet>().units[0].pos =
                (TEST_ORIGIN + Vec2::X * 20.0).into();
            step(&mut app, 0.01);
            app.world_mut().resource_mut::<ArmorFleet>().units[0].pos = TEST_ORIGIN.into();
            step(&mut app, 0.01);
            assert_eq!(
                cursor
                    .read(app.world().resource::<Messages<crate::combat::PlayerHit>>())
                    .count(),
                1
            );
            assert_eq!(
                app.world()
                    .resource::<crate::vehicles::VehicleFleet>()
                    .current()
                    .unwrap()
                    .engine,
                0.0
            );
        }
    }

    #[test]
    fn ecs_ram_shove_sweeps_full_hull_and_respects_static_wall() {
        for wall in [None, Some(3.0)] {
            let obstacles = test_obstacles(wall);
            let mut app = ecs_app(VehicleKind::Sedan, obstacles.clone());
            hostile(&mut app, 1, VehicleKind::Mengshi, -Vec2::Y * 4.0);
            app.add_systems(Update, armor_ramming);
            step(&mut app, 0.01);
            let vehicle = app
                .world()
                .resource::<crate::vehicles::VehicleFleet>()
                .current()
                .unwrap();
            let moved = vehicle.position().y - TEST_ORIGIN.y;
            if wall.is_some() {
                assert!(
                    moved > 0.0 && moved < 0.8,
                    "The front bumper must stop before the wall: {moved}"
                );
            } else {
                assert!(
                    (moved - 2.0).abs() < 0.001,
                    "The source shove is half the separation"
                );
            }
            assert!(!obstacles.footprint_blocked_ignoring(
                vehicle.corners_at(vehicle.position(), 0.0),
                &[7, 0x1_0000_0001]
            ));
            let world = app.world_mut();
            let transform = world
                .query_filtered::<&Transform, With<crate::player::Player>>()
                .single(world)
                .unwrap();
            assert_eq!(transform.translation.xz(), TEST_ORIGIN + Vec2::Y * moved);
        }
    }

    #[test]
    fn ecs_mounted_first_wall_blocks_hull_direct_hit_and_splash_behind_it() {
        let mut app = ecs_app(VehicleKind::Ztd05, test_obstacles(Some(10.0)));
        let soldier = infantry(&mut app, 1, Vec2::Y * 14.0);
        hostile(&mut app, 1, VehicleKind::Ztd05, Vec2::Y * 20.0);
        *app.world_mut().resource_mut::<crate::combat::WeaponInput>() =
            crate::combat::WeaponInput {
                fire: true,
                aim: Vec2::Y,
                ..default()
            };
        app.add_systems(Update, mounted_fire);
        step(&mut app, 0.01);
        assert_eq!(
            app.world()
                .get::<crate::combat::Enemy>(soldier)
                .unwrap()
                .0
                .hp,
            crate::combat::tuning::SOLDIER_HP
        );
        assert_eq!(app.world().resource::<ArmorFleet>().units[0].hp, 100.0);
        assert_eq!(
            app.world()
                .resource::<crate::vehicles::VehicleFleet>()
                .current()
                .unwrap()
                .gun
                .as_ref()
                .unwrap()
                .guns[0]
                .ready,
            7
        );
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<&Transform, With<crate::combat::standin::TracerFx>>()
                .iter(world)
                .count(),
            1
        );
    }

    #[test]
    fn ecs_mounted_round_hits_nearer_infantry_before_an_offset_hull() {
        let mut app = ecs_app(VehicleKind::Ztd05, test_obstacles(None));
        app.world_mut()
            .resource_mut::<crate::vehicles::VehicleFleet>()
            .current_mut()
            .unwrap()
            .gun
            .as_mut()
            .unwrap()
            .selected = 1;
        let soldier = infantry(&mut app, 1, Vec2::Y * 14.0);
        hostile(&mut app, 1, VehicleKind::Zbl08, Vec2::new(1.8, 17.0));
        *app.world_mut().resource_mut::<crate::combat::WeaponInput>() =
            crate::combat::WeaponInput {
                fire: true,
                aim: Vec2::Y,
                ..default()
            };
        app.add_systems(Update, mounted_fire);
        step(&mut app, 0.01);
        assert!(
            (app.world()
                .get::<crate::combat::Enemy>(soldier)
                .unwrap()
                .0
                .hp
                - (crate::combat::tuning::SOLDIER_HP - MountedWeapon::Coax762.spec().damage))
                .abs()
                < 0.001
        );
        assert_eq!(app.world().resource::<ArmorFleet>().units[0].hp, 100.0);
    }

    #[test]
    fn ecs_mounted_hull_hit_selects_front_long_hull_and_capture_transfers_once() {
        let mut app = ecs_app(VehicleKind::Ztd05, test_obstacles(None));
        // Reverse insertion order and overlapping centers exercise exact target
        // identity, not the first unit found within a four-metre hit radius.
        hostile(&mut app, 2, VehicleKind::Ztd05, Vec2::new(1.8, 23.0));
        hostile(&mut app, 1, VehicleKind::Ztd05, Vec2::Y * 20.0);
        *app.world_mut().resource_mut::<crate::combat::WeaponInput>() =
            crate::combat::WeaponInput {
                fire: true,
                aim: Vec2::Y,
                ..default()
            };
        app.add_systems(Update, (mounted_fire, transfer_abandoned).chain());
        step(&mut app, 0.01);
        let fleet = app.world().resource::<ArmorFleet>();
        assert_eq!(fleet.units[0].hp, 100.0);
        assert_eq!(fleet.units[1].hp, 30.0);
        let captured_id = fleet.units[1]
            .captured_id
            .expect("An actual cannon hit must transfer the bailed-out hull");
        let captured = app
            .world()
            .resource::<crate::vehicles::VehicleFleet>()
            .get(captured_id)
            .unwrap();
        assert_eq!(captured.kind, VehicleKind::Ztd05);
        assert_eq!(captured.engine, 0.4);
        assert_eq!(captured.position(), TEST_ORIGIN + Vec2::Y * 20.0);
        assert_eq!(captured.heading, fleet.units[1].heading);
        assert!(captured.gun.as_ref().unwrap().validate(captured.kind));
        app.world_mut()
            .resource_mut::<crate::combat::WeaponInput>()
            .fire = false;
        for _ in 0..5 {
            step(&mut app, 0.1);
        }
        assert_eq!(
            app.world()
                .resource::<crate::vehicles::VehicleFleet>()
                .vehicles
                .len(),
            2
        );
        assert_eq!(
            app.world().resource::<ArmorFleet>().units[1].captured_id,
            Some(captured_id)
        );
    }

    #[test]
    fn ecs_load_clears_transient_ram_contacts_and_shield_snapshot() {
        let mut app = ecs_app(VehicleKind::Sedan, test_obstacles(None));
        app.world_mut().resource_mut::<ArmorRamContacts>().current = Some(7);
        app.world_mut()
            .resource_mut::<ArmorRamContacts>()
            .contacts
            .insert(
                1,
                RamContact {
                    touching: true,
                    cooldown: 1.0,
                },
            );
        app.world_mut().resource_mut::<ArmorShield>().hp_before = Some(100.0);
        app.world_mut().resource_mut::<ArmorProximity>().heard = true;
        app.add_systems(Update, clear_loaded_contacts);
        app.world_mut().write_message(crate::save::LoadCompleted);
        step(&mut app, 0.01);
        let contacts = app.world().resource::<ArmorRamContacts>();
        assert_eq!(contacts.current, None);
        assert!(contacts.contacts.is_empty());
        assert_eq!(app.world().resource::<ArmorShield>().hp_before, None);
        assert!(!app.world().resource::<ArmorProximity>().heard);
    }

    #[test]
    fn ecs_engine_warning_uses_strict_distance_thresholds_and_preserves_spotted_priority() {
        let mut app = ecs_app(VehicleKind::Sedan, test_obstacles(None));
        hostile(&mut app, 1, VehicleKind::Mengshi, Vec2::Y * 60.0);
        app.add_systems(Update, armor_tick);
        let set_distance = |app: &mut App, distance: f32| {
            let mut fleet = app.world_mut().resource_mut::<ArmorFleet>();
            fleet.units[0].pos = (TEST_ORIGIN + Vec2::Y * distance).into();
            fleet.units[0].stop = 6.0;
        };
        step(&mut app, 0.01);
        assert!(!app.world().resource::<ArmorProximity>().heard);
        assert!(
            app.world()
                .resource::<crate::worldplay::WorldNotice>()
                .text
                .is_empty()
        );
        set_distance(&mut app, 59.99);
        step(&mut app, 0.01);
        let warning = "聽見履帶與柴油引擎聲……裝甲車在附近大馬路上";
        assert_eq!(
            app.world().resource::<crate::worldplay::WorldNotice>().text,
            warning
        );
        app.world_mut()
            .resource_mut::<crate::worldplay::WorldNotice>()
            .text = "keep current message".into();
        step(&mut app, 0.01);
        assert_eq!(
            app.world().resource::<crate::worldplay::WorldNotice>().text,
            "keep current message"
        );
        set_distance(&mut app, 100.0);
        step(&mut app, 0.01);
        assert!(app.world().resource::<ArmorProximity>().heard);
        set_distance(&mut app, 100.01);
        step(&mut app, 0.01);
        assert!(!app.world().resource::<ArmorProximity>().heard);
        app.world_mut()
            .resource_mut::<crate::worldplay::WorldNotice>()
            .text
            .clear();
        set_distance(&mut app, 59.99);
        step(&mut app, 0.01);
        assert_eq!(
            app.world().resource::<crate::worldplay::WorldNotice>().text,
            warning
        );

        // An actual new spotting event in the same update is more urgent than
        // the early engine hint and must be the displayed message.
        app.world_mut().resource_mut::<ArmorProximity>().heard = false;
        app.world_mut().resource_mut::<ArmorFleet>().units[0].spot = 0.6;
        set_distance(&mut app, 20.0);
        step(&mut app, 0.01);
        assert!(app.world().resource::<ArmorProximity>().heard);
        assert_eq!(
            app.world().resource::<crate::worldplay::WorldNotice>().text,
            "裝甲巡邏發現你！快躲進騎樓或巷子。"
        );
    }

    #[test]
    fn armor_save_validation_rejects_extreme_ids_and_segments_without_overflow() {
        let baseline = ArmorFleet::default();
        assert!(baseline.validate());
        for id in [100_000, u32::MAX] {
            let mut invalid = baseline.clone();
            invalid.units[0].id = id;
            assert!(!invalid.validate());
        }
        let mut invalid = baseline.clone();
        invalid.units[0].segment = usize::MAX;
        assert!(!invalid.validate());
        invalid.units[0].segment = invalid.units[0].route.len() - 1;
        assert!(!invalid.validate());
    }

    #[test]
    fn original_typescript_mounted_weapon_reference() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/armor-reference.json"))
                .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let kind = match case["kind"].as_str().unwrap() {
                "zbl08" => VehicleKind::Zbl08,
                "zbd05" => VehicleKind::Zbd05,
                "ztd05" => VehicleKind::Ztd05,
                "mengshi" => VehicleKind::Mengshi,
                _ => unreachable!(),
            };
            let scenario = case["scenario"].as_str().unwrap();
            let mut state = MountedGunState::new(kind);
            for index in 0..240 {
                if scenario == "partial_reload" && index == 40 {
                    state.reload();
                }
                if scenario == "alternate" && index == 60 {
                    state.switch();
                }
                let can = state.tick(0.125);
                let fired = can && (scenario != "cool" || index < 120);
                if fired {
                    assert!(state.fire().is_some());
                }
                if let Some(expected) = case["steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|s| s["index"] == index)
                {
                    assert_eq!(
                        fired,
                        expected["fired"].as_bool().unwrap(),
                        "{kind:?}/{scenario}/{index}"
                    );
                    assert_eq!(
                        state.selected,
                        expected["selected"].as_u64().unwrap() as usize
                    );
                    for (g, e) in state.guns.iter().zip(expected["guns"].as_array().unwrap()) {
                        assert_eq!(
                            g.ready,
                            e["ready"].as_u64().unwrap() as u32,
                            "{kind:?}/{scenario}/{index}"
                        );
                        assert_eq!(g.reserve, e["reserve"].as_u64().unwrap() as u32);
                        assert_eq!(g.jammed, e["jammed"].as_bool().unwrap());
                        for (actual, key) in [
                            (g.heat, "heat"),
                            (g.reload_remaining, "reload_remaining"),
                            (g.cooldown_remaining, "cooldown_remaining"),
                        ] {
                            assert!(
                                (f64::from(actual) - e[key].as_f64().unwrap()).abs() < 0.002,
                                "{kind:?}/{scenario}/{index}/{key}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn armor_immunity_bailout_and_destruction_are_distinct_and_single_use() {
        let mut fleet = ArmorFleet {
            units: vec![ArmorState::new(
                0,
                VehicleKind::Zbl08,
                vec![Vec2::ZERO, Vec2::X * 10.0],
                0,
            )],
        };
        assert_eq!(
            fleet.hit_at(Vec2::ZERO, 1000.0, false),
            ArmorImpact::Ricochet
        );
        assert_eq!(fleet.units[0].hp, 100.0);
        assert_eq!(fleet.hit_at(Vec2::ZERO, 70.0, true), ArmorImpact::Abandoned);
        assert_eq!(fleet.hit_at(Vec2::ZERO, 100.0, true), ArmorImpact::Miss);
        assert!(fleet.units[0].abandoned && !fleet.units[0].destroyed);
        let mut unit = ArmorState::new(
            1,
            VehicleKind::Mengshi,
            vec![Vec2::X * 20.0, Vec2::X * 30.0],
            0,
        );
        unit.hp = 40.0;
        fleet.units.push(unit);
        assert_eq!(
            fleet.hit_at(Vec2::X * 20.0, 40.0, true),
            ArmorImpact::Destroyed
        );
        assert_eq!(fleet.hit_at(Vec2::X * 20.0, 40.0, true), ArmorImpact::Miss);
    }

    #[test]
    fn armor_patrol_reverses_and_waits_at_route_ends() {
        let mut unit = ArmorState::new(0, VehicleKind::Zbl08, vec![Vec2::ZERO, Vec2::X], 0);
        unit.patrol(0.5);
        assert_eq!((unit.direction, unit.t, unit.stop), (-1, 1.0, 4.0));
        let pos = unit.pos;
        unit.patrol(3.0);
        assert_eq!(unit.pos, pos);
        assert_eq!(unit.stop, 1.0);
    }

    #[test]
    fn default_armor_plan_is_valid_and_avoids_start_street() {
        let fleet = ArmorFleet::default();
        assert!(fleet.validate());
        assert_eq!(fleet.units.iter().filter(|a| a.active()).count(), 4);
        assert_eq!(fleet.units.iter().filter(|a| a.destroyed).count(), 3);
        assert!(fleet.units.iter().filter(|a| a.active()).all(|a| {
            a.route
                .iter()
                .all(|p| Vec2::from_array(*p).distance(crate::map::world().spawn) >= 30.0)
        }));
    }

    #[test]
    fn mounted_reload_conserves_rounds_and_rejects_early_fire() {
        let mut state = MountedGunState::new(VehicleKind::Ztd05);
        assert!(state.fire().is_some());
        assert!(state.fire().is_none());
        assert!(state.reload());
        assert_eq!((state.guns[0].ready, state.guns[0].reserve), (0, 44));
        assert!(!state.tick(21.9));
        assert!(state.tick(0.11));
        assert_eq!((state.guns[0].ready, state.guns[0].reserve), (8, 36));
        assert!(state.validate(VehicleKind::Ztd05));
    }

    #[test]
    fn mounted_heat_locks_until_original_cooling_threshold() {
        let mut state = MountedGunState::new(VehicleKind::Zbl08);
        state.guns[0].heat = 99.0;
        assert!(state.fire().is_some());
        assert!(state.guns[0].jammed);
        assert!(!state.tick(21.9));
        assert!(state.tick(0.1));
        assert!(!state.guns[0].jammed);
    }

    #[test]
    fn mounted_save_rejects_wrong_loadout_and_round_creation() {
        let mut state = MountedGunState::new(VehicleKind::Mengshi);
        assert!(!state.validate(VehicleKind::Ztd05));
        state.guns[0].reserve += 1;
        assert!(!state.validate(VehicleKind::Mengshi));
    }
}
