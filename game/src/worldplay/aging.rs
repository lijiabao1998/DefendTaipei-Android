//! Opened-container aging from Web v24.42.0 `core/main.ts::updateStory`.
//! Its `gm` is game HOURS: while power is off, every >6 hours each present
//! listed food stack in a 冷藏/櫃台/攤 container has a 15% chance to turn one
//! portion into rotten food. Daily carried-food spoilage is a different rule.
//!
//! Native saves retain the elapsed clock and a separate deterministic RNG.
//! Fractional overshoot is retained; large jumps catch up in bounded batches.
//! A legacy save starts its clock when loaded, without retroactively removing
//! supplies. Unopened loot keeps the existing late-roll spoilage rule.

use std::collections::HashSet;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{ScavengeSession, ScavengeState, WorldCatalog, WorldNotice, next};
use crate::combat::GameState;
use crate::game_flow::{Campaign, GameFlow};
use crate::i18n::{Language, tr};
use crate::story::POWER_OFF;
use crate::survival::{ShelterEnvironment, Stash};
use crate::ui::PauseOpen;

const INTERVAL_HOURS: f64 = 6.0;
const MAX_INTERVALS_PER_FRAME: u32 = 64;
const PERISH: [&str; 9] = [
    "bento", "rice", "oden", "soymilk", "milktea", "danbing", "chicken", "sausage", "egg",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ContainerAging {
    pub minute: Option<f32>,
    pub elapsed_hours: f64,
    pub seed: u32,
}

impl Default for ContainerAging {
    fn default() -> Self {
        Self {
            minute: None,
            elapsed_hours: 0.0,
            seed: 0x51a7_e001,
        }
    }
}

impl ContainerAging {
    fn advance(&mut self, minute: f32, power: bool) -> u32 {
        if !minute.is_finite() || minute < 0.0 {
            return 0;
        }
        let Some(previous) = self.minute.replace(minute) else {
            return 0;
        };
        if minute < previous {
            // A new/loaded run normally replaces this whole state. Be defensive
            // about an explicit developer rewind without replaying prior time.
            self.elapsed_hours = 0.0;
            return 0;
        }
        if power {
            return 0;
        }
        let previous = if previous < POWER_OFF as f32 && minute >= POWER_OFF as f32 {
            POWER_OFF as f32
        } else {
            previous
        };
        self.elapsed_hours += f64::from(minute - previous) / 60.0;
        // Preserve the source's strict >6 boundary. ceil(x)-1 yields zero at
        // exactly 6 h and one at exactly 12 h; fractional time is never discarded.
        let intervals = ((self.elapsed_hours / INTERVAL_HOURS).ceil() - 1.0)
            .clamp(0.0, f64::from(MAX_INTERVALS_PER_FRAME)) as u32;
        self.elapsed_hours -= f64::from(intervals) * INTERVAL_HOURS;
        intervals
    }
}

fn eligible_label(label: &str) -> bool {
    ["冷藏", "櫃台", "攤"]
        .iter()
        .any(|term| label.contains(term))
}

fn spoil_one_interval(stock: &mut Stash, random: &mut impl FnMut() -> f64) -> u32 {
    let mut converted = 0;
    for id in PERISH {
        if stock.count(id) > 0 && random() < 0.15 && stock.take(id, 1) {
            converted += 1;
        }
    }
    if converted > 0 {
        stock.add("rotten", converted);
        stock.remove_empty();
    }
    converted
}

pub(super) fn tick_container_aging(
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    game: Res<GameState>,
    campaign: Res<Campaign>,
    environment: Res<ShelterEnvironment>,
    catalog: Res<WorldCatalog>,
    mut progress: ResMut<ScavengeState>,
    session: Res<ScavengeSession>,
    mut notice: ResMut<WorldNotice>,
    lang: Res<Language>,
) {
    if flow.get() != &GameFlow::Playing || pause.0 || game.over || game.hp <= 0.0 {
        return;
    }
    let intervals = progress
        .container_aging
        .advance(campaign.game_min, environment.power);
    if intervals == 0 {
        return;
    }
    let eligible: HashSet<_> = catalog
        .containers
        .iter()
        .filter(|container| eligible_label(container.label_zh))
        .map(|container| container.id.as_str())
        .collect();
    let selected = session.target.as_ref().map(|target| target.id.as_str());
    let ScavengeState {
        loot,
        container_aging,
        ..
    } = &mut *progress;
    let mut total = 0;
    let mut visible = 0;
    for _ in 0..intervals {
        for (id, stock) in &mut *loot {
            if !eligible.contains(id.as_str()) {
                continue;
            }
            let converted = spoil_one_interval(stock, &mut || next(&mut container_aging.seed));
            total += converted;
            if selected == Some(id.as_str()) {
                visible += converted;
            }
        }
    }
    if visible > 0 {
        notice.text = format!(
            "{} {visible}",
            tr(
                *lang,
                "停電後容器內食物腐壞：",
                "Food in this container spoiled after the blackout:"
            )
        );
    }
    if total > 0 {
        println!("DTLOOT aging converted={total} intervals={intervals}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_aging_fixture_targets_real_reachable_cold_storage() {
        let map = crate::map::world();
        let cold = map
            .interiors
            .iter()
            .find(|f| f.building == 814 && f.kind == crate::map::interiors::FixtureKind::Fridge)
            .unwrap();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_systems(Startup, super::super::plan_world);
        app.update();
        let obstacles = crate::map::Obstacles::new(map.clone());
        obstacles.doors().set_open(cold.building, true);
        assert!(map.free(cold.approach, crate::player::PLAYER_RADIUS));
        assert_eq!(
            crate::map::interiors::restore_position(map, cold.approach),
            cold.approach
        );
        let target = super::super::interaction_target(
            cold.approach,
            app.world().resource::<WorldCatalog>(),
            &ScavengeState::default(),
            &crate::survival::Shelter::default(),
            &obstacles,
            &crate::dialogue::NpcSpots::default(),
        );
        assert!(matches!(target, super::super::InteractionTarget::Container(c) if c.id == cold.id));
        let actual = serde_json::json!({
            "version": 1,
            "container": { "id": cold.id, "building": cold.building, "approach": cold.approach.to_array() },
            "control_id": "x814_3",
            "game_min": POWER_OFF + 361,
            "power_off": POWER_OFF,
        });
        let descriptor: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/android-aging-smoke.json"
        ))
        .unwrap();
        assert_eq!(
            actual, descriptor,
            "Android aging fixture must match actual geometry and timeline"
        );
        let control = map.interiors.iter().find(|f| f.id == "x814_3").unwrap();
        assert_eq!(control.kind, crate::map::interiors::FixtureKind::Shelf);
        assert!(!eligible_label(control.label_zh));
    }

    #[test]
    fn clock_obeys_power_six_hour_boundary_and_carries_overshoot() {
        let cut = POWER_OFF as f32;
        let mut clock = ContainerAging::default();
        assert_eq!(clock.advance(cut - 30.0, true), 0);
        assert_eq!(clock.advance(cut + 30.0, false), 0);
        assert_eq!(clock.elapsed_hours, 0.5);
        assert_eq!(clock.advance(cut + 360.0, false), 0);
        assert_eq!(clock.elapsed_hours, 6.0);
        assert_eq!(clock.advance(cut + 361.0, false), 1);
        assert!((clock.elapsed_hours - 1.0 / 60.0).abs() < 1e-9);
        assert_eq!(clock.advance(cut + 721.0, false), 1);
        assert!((clock.elapsed_hours - 1.0 / 60.0).abs() < 1e-9);
        let remainder = clock.elapsed_hours;
        assert_eq!(clock.advance(cut + 2000.0, true), 0);
        assert_eq!(clock.elapsed_hours, remainder);
    }

    #[test]
    fn opened_food_uses_exact_web_list_chance_and_single_portions() {
        let mut stock = Stash::default();
        for id in PERISH {
            stock.add(id, 3);
        }
        stock.add("can", 4);
        stock.add("water", 2);
        stock.add("rotten", 2);
        let mut calls = 0;
        assert_eq!(
            spoil_one_interval(&mut stock, &mut || {
                calls += 1;
                0.149_999
            }),
            9
        );
        assert_eq!(calls, 9);
        assert!(PERISH.iter().all(|id| stock.count(id) == 2));
        assert_eq!(stock.count("rotten"), 11);
        assert_eq!(stock.count("can"), 4);
        assert_eq!(stock.count("water"), 2);
        assert_eq!(spoil_one_interval(&mut stock, &mut || 0.15), 0);
        assert_eq!(
            spoil_one_interval(&mut Stash::default(), &mut || panic!(
                "empty stock must not draw"
            )),
            0
        );
        assert!(eligible_label("飲料冷藏櫃"));
        assert!(eligible_label("結帳櫃台"));
        assert!(eligible_label("路邊攤"));
        assert!(!eligible_label("木製衣櫃"));
        assert!(
            !eligible_label("廚房冰箱"),
            "source matches labels, not generic perish flags"
        );
    }

    #[test]
    fn legacy_save_starts_now_and_mid_interval_reload_preserves_outcome() {
        let cut = POWER_OFF as f32;
        let mut legacy: ScavengeState = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.container_aging.advance(cut + 3000.0, false), 0);
        assert_eq!(legacy.container_aging.elapsed_hours, 0.0);
        let mut stock = Stash::default();
        stock.add("bento", 5);
        stock.add("soymilk", 4);
        legacy.loot.push(("test".into(), stock));
        legacy.container_aging.advance(cut + 3150.0, false);
        let saved = serde_json::to_string(&legacy).unwrap();
        let mut loaded: ScavengeState = serde_json::from_str(&saved).unwrap();
        for now in [cut + 3361.0, cut + 3721.0, cut + 4081.0, cut + 4441.0] {
            for state in [&mut legacy, &mut loaded] {
                let n = state.container_aging.advance(now, false);
                for _ in 0..n {
                    spoil_one_interval(&mut state.loot[0].1, &mut || {
                        next(&mut state.container_aging.seed)
                    });
                }
            }
            assert_eq!(
                serde_json::to_string(&legacy).unwrap(),
                serde_json::to_string(&loaded).unwrap()
            );
            assert_eq!(
                loaded.container_aging.advance(now, false),
                0,
                "same saved minute cannot age twice"
            );
        }
    }

    #[test]
    fn long_jump_catches_up_without_unbounded_frame_work() {
        let mut clock = ContainerAging::default();
        let cut = POWER_OFF as f32;
        clock.advance(cut, false);
        assert_eq!(clock.advance(cut + 360.0 * 130.0 + 1.0, false), 64);
        assert!(clock.elapsed_hours > 6.0);
        assert_eq!(clock.advance(cut + 360.0 * 130.0 + 1.0, false), 64);
        assert_eq!(clock.advance(cut + 360.0 * 130.0 + 1.0, false), 2);
        assert_eq!(clock.advance(cut + 360.0 * 130.0 + 1.0, false), 0);
        assert!((clock.elapsed_hours - 1.0 / 60.0).abs() < 1e-9);
    }

    #[test]
    fn running_world_ages_only_opened_matching_stock_and_pause_preserves_clock() {
        use super::super::{Container, LootTarget};
        use bevy::state::app::StatesPlugin;
        let cut = POWER_OFF as f32;
        let mut progress = ScavengeState::default();
        progress.container_aging.minute = Some(cut);
        progress.container_aging.seed = 1972; // first LCG draw =0.000321359 <0.15
        for id in ["cold", "shelf", "body:7"] {
            let mut stock = Stash::default();
            stock.add("bento", 2);
            stock.add("can", 4);
            progress.loot.push((id.into(), stock));
        }
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(GameFlow::Playing)
            .init_resource::<PauseOpen>()
            .init_resource::<GameState>()
            .insert_resource(Campaign {
                game_min: cut + 361.0,
                ..default()
            })
            .insert_resource(ShelterEnvironment {
                power: false,
                ..default()
            })
            .insert_resource(progress)
            .insert_resource(WorldCatalog {
                containers: [("cold", "冷藏櫃"), ("shelf", "貨架"), ("unopened", "櫃台")]
                    .into_iter()
                    .map(|(id, label)| Container {
                        id: id.into(),
                        pos: Vec2::ZERO,
                        kind: "fridge",
                        cabinet: false,
                        furnished: true,
                        label_zh: label,
                        label_en: label,
                    })
                    .collect(),
                patrols: vec![],
                home: Vec2::ZERO,
            })
            .insert_resource(ScavengeSession {
                target: Some(LootTarget {
                    id: "cold".into(),
                    label_zh: "冷藏櫃".into(),
                    label_en: "Fridge".into(),
                }),
                reachable: true,
            })
            .init_resource::<WorldNotice>()
            .init_resource::<Language>()
            .add_systems(Update, tick_container_aging);
        app.update();
        let state = app.world().resource::<ScavengeState>();
        assert_eq!(
            state.loot.len(),
            3,
            "unopened container must not be rolled by aging"
        );
        assert_eq!(state.loot[0].1.count("bento"), 1);
        assert_eq!(state.loot[0].1.count("rotten"), 1);
        assert_eq!(state.loot[0].1.count("can"), 4);
        assert_eq!(state.loot[1].1.count("bento"), 2);
        assert_eq!(state.loot[2].1.count("bento"), 2);
        assert!(app.world().resource::<WorldNotice>().text.contains("腐壞"));
        let saved = serde_json::to_string(state).unwrap();
        app.insert_resource(serde_json::from_str::<ScavengeState>(&saved).unwrap());
        app.update();
        assert_eq!(
            serde_json::to_string(app.world().resource::<ScavengeState>()).unwrap(),
            saved
        );
        app.world_mut().resource_mut::<PauseOpen>().0 = true;
        app.world_mut().resource_mut::<Campaign>().game_min = cut + 721.0;
        app.update();
        assert_eq!(
            serde_json::to_string(app.world().resource::<ScavengeState>()).unwrap(),
            saved
        );
        app.world_mut().resource_mut::<PauseOpen>().0 = false;
        app.world_mut().resource_mut::<ShelterEnvironment>().power = true;
        app.update();
        let after = app.world().resource::<ScavengeState>();
        assert_eq!(after.loot[0].1.count("rotten"), 1);
        assert_eq!(
            after.container_aging.seed,
            serde_json::from_str::<ScavengeState>(&saved)
                .unwrap()
                .container_aging
                .seed
        );
        assert_eq!(after.container_aging.minute, Some(cut + 721.0));
    }
}
