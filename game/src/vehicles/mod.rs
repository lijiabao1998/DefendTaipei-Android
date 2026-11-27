//! Drivable ground transport, separate from the original locked/stripped street
//! scenery. Rules are ported from Web v24.42.0 drive.ts; static world IDs remain.
mod planning;
mod spec;
mod state;
#[cfg(test)]
mod tests;
pub(crate) mod ui;
pub mod visual;

pub use spec::{VehicleKind, VehicleSpec};
pub use state::{
    Hotwire, VehicleFleet, VehicleOutcome, VehicleRuntime, VehicleState, angle, drive_step, enter,
    footprint, refuel, repair, siphon,
};

use crate::combat::{CombatEnv, Enemy, GameState, PlayerHit};
use crate::game_flow::{Campaign, GameFlow};
use crate::i18n::{Language, tr};
use crate::inventory::Inventory;
use crate::map::Obstacles;
use crate::player::{MoveInput, Player};
use crate::roles::SelectedRole;
use crate::worldplay::WorldNotice;
use bevy::{ecs::system::SystemParam, prelude::*};

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct VehicleSet;

#[derive(Message, Clone, Debug)]
pub enum VehicleAction {
    Interact,
    Enter(u32),
    Exit,
    Siphon(u32),
    Refuel(u32),
    Repair(u32),
    UseItem { vehicle: u32, item: String },
    Brake,
    ReloadGun,
    SwitchGun,
}

pub struct VehiclePlugin;
impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VehicleFleet>()
            .init_resource::<VehicleRuntime>()
            .init_resource::<visual::VehicleArt>()
            .add_message::<VehicleAction>()
            .configure_sets(
                Update,
                VehicleSet
                    .after(crate::player::MoveSet)
                    .before(crate::combat::CombatSet),
            )
            .add_systems(Startup, (visual::setup_art, ui::spawn))
            .add_systems(OnEnter(GameFlow::Prologue), reset_fleet)
            .add_systems(PreUpdate, clear_loaded_transients)
            .add_systems(Update, tick.in_set(VehicleSet))
            .add_systems(Update, apply_actions.after(crate::worldplay::WorldSet))
            .add_systems(Update, ui::button_actions.before(crate::input::InputSet))
            .add_systems(
                PostUpdate,
                (visual::sync_visuals, ui::update)
                    .before(TransformSystems::Propagate)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate)
                    .before(bevy::ui::UiSystems::Prepare),
            );
    }
}

fn clear_loaded_transients(
    mut loaded: MessageReader<crate::save::LoadCompleted>,
    mut runtime: ResMut<VehicleRuntime>,
    fleet: Res<VehicleFleet>,
    obstacles: Res<Obstacles>,
) {
    if loaded.read().count() > 0 {
        *runtime = VehicleRuntime::default();
    }
    fleet.publish_colliders(&obstacles);
}

fn reset_fleet(
    mut fleet: ResMut<VehicleFleet>,
    mut runtime: ResMut<VehicleRuntime>,
    selected: Res<SelectedRole>,
    obstacles: Res<Obstacles>,
) {
    *fleet = VehicleFleet::default();
    *runtime = VehicleRuntime::default();
    fleet.publish_colliders(&obstacles);
    if selected.role().id == "courier" {
        let start = crate::map::world().spawn;
        let mut rng = crate::map::data::Park(125977);
        'search: for radius in [1.4, 2.5, 4.0, 6.0, 9.0, 12.0] {
            for i in 0..24 {
                let a = i as f32 * std::f32::consts::TAU / 24.0;
                let p = start + Vec2::from_angle(a) * radius;
                let mut v = VehicleState::new(
                    fleet.vehicles.len() as u32,
                    VehicleKind::Delivery,
                    p,
                    0.0,
                    &mut || rng.f(),
                );
                if obstacles.footprint_blocked(v.corners_at(p, 0.0), None) {
                    continue;
                }
                v.key = true;
                v.fuel = 4.0;
                v.tyre = 1.0;
                v.engine = 1.0;
                v.battery = 1.0;
                fleet.vehicles.push(v);
                break 'search;
            }
        }
    }
    fleet.publish_colliders(&obstacles);
}

#[derive(SystemParam)]
struct Context<'w> {
    flow: Res<'w, State<GameFlow>>,
    pause: Res<'w, crate::ui::PauseOpen>,
    modal: Res<'w, crate::ui::UiModal>,
    campaign: Res<'w, Campaign>,
    obstacles: Res<'w, Obstacles>,
    selected: Res<'w, SelectedRole>,
    lang: Res<'w, Language>,
    env: Res<'w, CombatEnv>,
    utilities: Res<'w, crate::survival::ShelterEnvironment>,
}
impl Context<'_> {
    fn active(&self, state: &GameState) -> bool {
        self.flow.get() == &GameFlow::Playing
            && !self.pause.0
            && !self.modal.blocks_input()
            && !self.campaign.sleeping
            && !state.over
            && state.hp > 0.0
    }
}
struct NoiseRng(crate::city::Lcg);
impl Default for NoiseRng {
    fn default() -> Self {
        Self(crate::city::Lcg(0x76656869636c65))
    }
}

#[allow(clippy::too_many_arguments)]
fn tick(
    time: Res<Time>,
    context: Context,
    mut fleet: ResMut<VehicleFleet>,
    mut runtime: ResMut<VehicleRuntime>,
    input: Res<MoveInput>,
    command: Res<crate::input::PlayerInput>,
    mut game: ResMut<GameState>,
    mut player: Query<&mut Transform, With<Player>>,
    mut enemies: Query<&mut Enemy>,
    mut notice: ResMut<WorldNotice>,
    mut hits: MessageWriter<PlayerHit>,
    mut sound: MessageWriter<crate::audio::SfxEvent>,
    mut rng: Local<NoiseRng>,
) {
    if !context.active(&game) {
        return;
    }
    let Ok(mut player) = player.single_mut() else {
        return;
    };
    let dt = time.delta_secs().clamp(0.0, 1.0);
    if let Some(wire) = runtime.hotwire {
        if !fleet.reachable(wire.id, player.translation.xz(), &context.obstacles) {
            runtime.hotwire = None;
        } else if let Some(wire) = &mut runtime.hotwire {
            let previous = wire.remaining;
            wire.remaining = (wire.remaining - dt).max(0.0);
            if previous > 0.0 && wire.remaining == 0.0 {
                notice.text = tr(
                    *context.lang,
                    "接線完成，再互動即可發動。",
                    "Wiring complete. Interact again to start.",
                )
                .into();
            }
        }
    }
    let current = fleet.current;
    for v in &mut fleet.vehicles {
        if v.spec().electric && current != Some(v.id) && context.utilities.power {
            v.fuel = (v.fuel + dt * 0.0005).min(v.spec().fuel_capacity);
        }
    }
    let Some(v) = fleet.current_mut() else {
        return;
    };
    let want = if command.vehicle_brake {
        Vec2::ZERO
    } else {
        crate::player::screen_to_ground(input.combined())
    };
    let before = v.position();
    let fuel_before = v.fuel;
    // At most 25 cm of travel per substep, including reverse and a slow frame.
    let steps = (dt * v.spec().vmax.max(v.speed.abs()) * 1.3 / 0.25)
        .ceil()
        .max(1.0) as u32;
    let step = dt / steps as f32;
    let mut impact = 0.0_f32;
    for _ in 0..steps {
        let id = v.id;
        impact = impact.max(drive_step(
            v,
            want,
            step,
            context.selected.role().skills.ride,
            |corners| {
                context
                    .obstacles
                    .footprint_blocked(corners, Some(id as u64))
            },
        ));
    }
    runtime.distance_driven += v.position().distance(before);
    runtime.fuel_used += (fuel_before - v.fuel).max(0.0);
    if impact > 6.0 {
        let damage = impact * if v.spec().two { 1.5 } else { 0.4 };
        // Armour shielding runs after collision/combat; clamp only in vitals so
        // a raw lethal impact can still be reduced to its exact protected share.
        game.hp -= damage;
        hits.write(PlayerHit {
            dmg: damage,
            cause: crate::combat::HitCause::RifleButt,
            bleed: false,
        });
        sound.write(crate::audio::SfxEvent::MeleeHit {
            dist: 0.0,
            blunt: true,
        });
        runtime.collisions += 1;
        notice.text = format!(
            "{} · {} {:.0}%",
            tr(
                *context.lang,
                "碰撞造成車輛損壞",
                "Collision damaged the vehicle"
            ),
            tr(*context.lang, "引擎", "Engine"),
            v.engine * 100.0
        );
    }
    runtime.noise_timer += dt;
    if (runtime.noise_timer > 0.6 && v.spec().noise > 0.0) || impact > 6.0 {
        runtime.noise_timer = 0.0;
        let radius = if impact > 6.0 {
            30.0
        } else {
            v.spec().noise * (0.5 + 0.5 * (v.speed.abs() / v.spec().vmax.max(1.0)).min(1.0))
        };
        crate::combat::soldier::noise(
            enemies.iter_mut().map(|e| &mut e.into_inner().0),
            v.position(),
            radius,
            context.env.rain,
            context.env.night,
            &mut rng.0,
        );
    }
    if fuel_before > 0.0 && v.fuel <= 0.0 {
        notice.text = tr(
            *context.lang,
            "能源耗盡，車輛正在減速。",
            "Out of energy. The vehicle is slowing down.",
        )
        .into();
    }
    player.translation = Vec3::new(v.pos[0], 0.0, v.pos[1]);
    player.rotation = Quat::from_rotation_y(v.heading + std::f32::consts::PI);
    fleet.publish_colliders(&context.obstacles);
}

#[allow(clippy::too_many_arguments)]
fn apply_actions(
    mut actions: MessageReader<VehicleAction>,
    context: Context,
    mut fleet: ResMut<VehicleFleet>,
    mut runtime: ResMut<VehicleRuntime>,
    mut bag: ResMut<Inventory>,
    mut player: Query<&mut Transform, With<Player>>,
    game: Res<GameState>,
    mut notice: ResMut<WorldNotice>,
    mut enemies: Query<&mut Enemy>,
    mut sounds: MessageWriter<crate::audio::SfxEvent>,
    mut rng: Local<NoiseRng>,
) {
    for action in actions.read() {
        // These are already semantic actions emitted by UI/key routing. A
        // swallowed pointer press must not cancel the button it just activated.
        let inventory_action = matches!(action, VehicleAction::UseItem { .. });
        let can_act = context.flow.get() == &GameFlow::Playing
            && !context.pause.0
            && !context.campaign.sleeping
            && !game.over
            && game.hp > 0.0
            && (!context.modal.open || inventory_action);
        if !can_act {
            continue;
        }
        let Ok(mut player) = player.single_mut() else {
            continue;
        };
        let pos = player.translation.xz();
        if matches!(action, VehicleAction::Brake) {
            if let Some(v) = fleet.current_mut() {
                v.speed = 0.0;
            }
            continue;
        }
        if matches!(action, VehicleAction::ReloadGun | VehicleAction::SwitchGun) {
            if let Some(gun) = fleet.current_mut().and_then(|v| v.gun.as_mut()) {
                if matches!(action, VehicleAction::ReloadGun) {
                    if !gun.reload() {
                        notice.text = tr(
                            *context.lang,
                            "目前無法裝填：彈藥已滿、正在裝填，或沒有備彈。",
                            "Cannot reload: already full, reloading, or out of spare ammunition.",
                        )
                        .into();
                        continue;
                    }
                    sounds.write(crate::audio::SfxEvent::Reload);
                } else {
                    let before = gun.selected;
                    gun.switch();
                    if gun.selected == before {
                        continue;
                    }
                    sounds.write(crate::audio::SfxEvent::Equip);
                }
                println!(
                    "DTVEHICLE action={} state={}",
                    if matches!(action, VehicleAction::ReloadGun) {
                        "reload"
                    } else {
                        "switch"
                    },
                    serde_json::to_string(&*gun).unwrap_or_default()
                );
            }
            continue;
        }
        if matches!(action, VehicleAction::Exit)
            || (matches!(action, VehicleAction::Interact) && fleet.current.is_some())
        {
            let Some(v) = fleet.current() else {
                continue;
            };
            if let Some(exit) = exit_position(v, &context.obstacles) {
                println!(
                    "DTVEHICLE action=exit state={}",
                    serde_json::json!({"id":v.id,"kind":v.kind,"position":[exit.x,exit.y]})
                );
                player.translation = Vec3::new(exit.x, 0.0, exit.y);
                if let Some(v) = fleet.current_mut() {
                    v.speed = 0.0;
                }
                fleet.current = None;
                runtime.exited += 1;
                notice.text = tr(*context.lang, "已下車。", "Exited the vehicle.").into();
                sounds.write(crate::audio::SfxEvent::Door {
                    open: true,
                    dist: 0.0,
                });
            } else {
                notice.text = tr(
                    *context.lang,
                    "車旁沒有安全落腳處，移動到空地再下車。",
                    "No safe exit beside the vehicle. Move to an open area first.",
                )
                .into();
            }
            continue;
        }
        let id = match action {
            VehicleAction::Enter(id)
            | VehicleAction::Siphon(id)
            | VehicleAction::Refuel(id)
            | VehicleAction::Repair(id) => Some(*id),
            VehicleAction::UseItem { vehicle, .. } => Some(*vehicle),
            VehicleAction::Interact => fleet.nearest(pos, 2.6).map(|v| v.id),
            _ => None,
        };
        let Some(id) = id else {
            continue;
        };
        if fleet.current.is_some() || !fleet.reachable(id, pos, &context.obstacles) {
            notice.text = tr(
                *context.lang,
                "請先下車並靠近車輛。",
                "Exit first and stand beside the vehicle.",
            )
            .into();
            continue;
        }
        let skills = context.selected.role().skills;
        let current_before = fleet.current;
        let before = fleet.get(id).unwrap().clone();
        let mut outcome = match action {
            VehicleAction::Siphon(_) => siphon(fleet.get_mut(id).unwrap(), &mut bag, *context.lang),
            VehicleAction::Refuel(_) => refuel(fleet.get_mut(id).unwrap(), &mut bag, *context.lang),
            VehicleAction::Repair(_) => {
                repair(fleet.get_mut(id).unwrap(), &mut bag, skills, *context.lang)
            }
            VehicleAction::UseItem { item, .. } => use_item(
                fleet.get_mut(id).unwrap(),
                &mut bag,
                item,
                skills,
                *context.lang,
            ),
            VehicleAction::Enter(_) => {
                enter(&mut fleet, &mut runtime, id, &bag, skills, *context.lang)
            }
            _ => {
                let v = fleet.get_mut(id).unwrap();
                let fuel = refuel(v, &mut bag, *context.lang);
                if fuel.changed {
                    fuel
                } else {
                    let fix = repair(v, &mut bag, skills, *context.lang);
                    if fix.changed {
                        fix
                    } else {
                        enter(&mut fleet, &mut runtime, id, &bag, skills, *context.lang)
                    }
                }
            }
        };
        if matches!(action, VehicleAction::Interact)
            && !outcome.changed
            && runtime.hotwire.is_none()
            && bag.count("hose") > 0
        {
            let attempt = siphon(fleet.get_mut(id).unwrap(), &mut bag, *context.lang);
            if attempt.changed {
                outcome = attempt;
            }
        }
        if outcome.noise > 0.0 {
            crate::combat::soldier::noise(
                enemies.iter_mut().map(|e| &mut e.into_inner().0),
                pos,
                outcome.noise,
                context.env.rain,
                context.env.night,
                &mut rng.0,
            );
        }
        if outcome.changed {
            sounds.write(
                if current_before != fleet.current && fleet.current.is_some() {
                    crate::audio::SfxEvent::Door {
                        open: false,
                        dist: 0.0,
                    }
                } else {
                    crate::audio::SfxEvent::Clink { dist: 0.0 }
                },
            );
        }
        let after = fleet.get(id).unwrap();
        let semantic = if current_before != Some(id) && fleet.current == Some(id) {
            "enter"
        } else if after.fuel > before.fuel {
            "refuel"
        } else if after.fuel < before.fuel {
            "siphon"
        } else if after.tyre > before.tyre
            || after.battery > before.battery
            || after.engine > before.engine
        {
            "repair"
        } else {
            "inspect"
        };
        println!(
            "DTVEHICLE action={semantic} state={}",
            serde_json::json!({"id":id,"kind":after.kind,"current":fleet.current,"fuel":after.fuel,"battery":after.battery,"tyre":after.tyre,"engine":after.engine,"changed":outcome.changed,"gas_items":bag.count("gas"),"patch_items":bag.count("patch"),"battery_items":bag.count("carbat")})
        );
        notice.text = outcome.text;
        if let Some(v) = fleet.current() {
            player.translation = Vec3::new(v.pos[0], 0.0, v.pos[1]);
        }
        fleet.publish_colliders(&context.obstacles);
    }
}

pub fn use_item(
    v: &mut VehicleState,
    bag: &mut Inventory,
    id: &str,
    skills: crate::roles::Skills,
    lang: Language,
) -> VehicleOutcome {
    match id {
        "gas" => refuel(v, bag, lang),
        "hose" => siphon(v, bag, lang),
        "patch" if v.tyre < 0.9 && bag.take("patch", 1) => {
            v.tyre = 1.0;
            VehicleOutcome {
                text: tr(lang, "輪胎已修補。", "Tire patched.").into(),
                noise: 0.0,
                changed: true,
            }
        }
        "carbat" if !v.spec().pedal && v.battery < 0.5 && bag.take("carbat", 1) => {
            v.battery = 1.0;
            VehicleOutcome {
                text: tr(lang, "電瓶已更換。", "Battery replaced.").into(),
                noise: 0.0,
                changed: true,
            }
        }
        "hammer" | "pipe" => repair(v, bag, skills, lang),
        _ => VehicleOutcome {
            text: tr(
                lang,
                "目前不需要這項維修，用品已保留。",
                "That repair is not needed; the item was kept.",
            )
            .into(),
            noise: 0.0,
            changed: false,
        },
    }
}

pub fn exit_position(v: &VehicleState, obstacles: &Obstacles) -> Option<Vec2> {
    let s = v.spec();
    let right = Vec2::new(v.heading.cos(), -v.heading.sin());
    let forward = Vec2::new(v.heading.sin(), v.heading.cos());
    let side = if s.two { 0.8 } else { s.width * 0.5 + 0.6 };
    [
        right * side,
        -right * side,
        -forward * (s.length * 0.5 + 0.65),
        forward * (s.length * 0.5 + 0.65),
    ]
    .into_iter()
    .map(|offset| v.position() + offset)
    .find(|&p| {
        !obstacles.blocked_except(p, crate::player::PLAYER_RADIUS, Some(v.id as u64))
            && obstacles
                .segment_hit_except(v.position(), p, 0.6, 0.6, Some(v.id as u64))
                .is_none()
    })
}
