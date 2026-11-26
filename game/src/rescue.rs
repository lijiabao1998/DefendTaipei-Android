//! v24.8 樓上有人在求救 — every morning a text message from someone trapped on an
//! upper floor (Web `rescueTick`, `rescueFloor`, `rescueTalk` in `src/core/main.ts`).
//!
//! At 09:00 a message arrives: a family is on a given floor of a walk-up near a
//! named street, the mother cannot manage the stairs. The rough spot (within
//! 15 m of the building) is marked on the survivor map. The family really is on
//! that floor of that building: climbing to it finds them. Talking to them sends
//! them to the shelter after dark — with a survivor who had not arrived yet —
//! raises karma and every survivor's mood, and they press two cans and a bandage
//! into the player's hands. If nobody comes, the next morning the message
//! "never answered again", morale drops, and a new one arrives.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::dialogue::{Dialogue, DialogueCard, NpcKey, NpcSpot, NpcSpots, TalkTo};
use crate::game_flow::{Campaign, GameFlow};
use crate::human::anim::AnimState;
use crate::human::rig::RigOpts;
use crate::human::{Driver, Humanoid, RigAnim};
use crate::i18n::{Language, tr};
use crate::inventory::Inventory;
use crate::map::upstairs::plan_floor;
use crate::player::Player;
use crate::survival::Shelter;
use crate::upstairs::Upstairs;
use crate::worldmap::{MapProgress, NoteKind};
use crate::worldplay::{WorldNotice, WorldSet};

/// The day's call for help (`surv.resc`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rescue {
    pub building: u32,
    pub floor: u8,
    pub day: u32,
    pub x: f32,
    pub z: f32,
    pub street: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub failed: bool,
}

/// Saved: the current call, if any.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RescueState {
    pub call: Option<Rescue>,
}

impl RescueState {
    pub fn is_valid(&self) -> bool {
        self.call.as_ref().is_none_or(|r| {
            (r.building as usize) < crate::map::world().blds.len()
                && (2..=crate::map::upstairs::MAX_FLOOR).contains(&r.floor)
                && r.x.is_finite()
                && r.z.is_finite()
                && r.street.len() <= 256
        })
    }

    /// The live call (not answered and not gone quiet).
    pub fn open(&self) -> Option<&Rescue> {
        self.call.as_ref().filter(|r| !r.done && !r.failed)
    }
}

/// `floorZh`: 二樓, 三樓 … / floor 2.
pub fn floor_words(f: u8, lang: Language) -> String {
    const ZH: [&str; 15] = [
        "", "一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "十一", "十二", "十三",
        "十四",
    ];
    match lang {
        Language::ZhTw => format!("{}樓", ZH.get(f as usize).copied().unwrap_or("")),
        Language::En if f == 1 => "ground floor".into(),
        Language::En => format!("floor {f}"),
    }
}

/// Picks the day's call: a walk-up of three or more storeys with stairs, not
/// the player's home, 80–320 m from `from`; a floor from 2 to the top; the
/// nearest named street; a marked spot within 15 m. Seeded by the day and karma.
pub fn pick_call(day: u32, karma: i32, from: Vec2, home: Option<Vec2>) -> Option<Rescue> {
    let w = crate::map::world();
    let home_b = home.and_then(|h| w.bld_at(h)).map(|b| b.id);
    let cand: Vec<_> = w
        .stairs
        .iter()
        .filter(|s| {
            let b = &w.blds[s.building as usize];
            let d = b.c.distance(from);
            s.floors >= 3 && Some(b.id) != home_b && d > 80.0 && d < 320.0
        })
        .collect();
    if cand.is_empty() {
        return None;
    }
    let mut rs = (day.wrapping_add(1).wrapping_mul(2_654_435_761))
        ^ (karma.wrapping_add(7) as u32).wrapping_mul(40_503);
    let mut r = || {
        rs = rs.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (f64::from(rs) / 4_294_967_296.0) as f32
    };
    let s = cand[(r() * cand.len() as f32) as usize % cand.len()];
    let b = &w.blds[s.building as usize];
    let floor = 2 + ((r() * f32::from(s.floors - 1)) as u8).min(s.floors - 2);
    let street = w
        .roads
        .iter()
        .filter(|q| !q.name.is_empty())
        .flat_map(|q| q.p.iter().map(move |a| (a.distance(b.c), &q.name)))
        .min_by(|a, z| a.0.total_cmp(&z.0))
        .map(|(_, n)| n.clone())
        .unwrap_or_default();
    let x = b.c.x + (r() - 0.5) * 30.0;
    let z = b.c.y + (r() - 0.5) * 30.0;
    Some(Rescue {
        building: b.id,
        floor,
        day,
        x,
        z,
        street,
        done: false,
        failed: false,
    })
}

/// The family member waiting upstairs.
#[derive(Component)]
struct FamilyRig {
    building: u32,
    floor: u8,
    pos: Vec2,
}

pub struct RescuePlugin;

impl Plugin for RescuePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RescueState>()
            .add_systems(OnEnter(GameFlow::Prologue), |mut s: ResMut<RescueState>| {
                *s = RescueState::default();
            })
            .add_systems(
                Update,
                (morning_call, place_family, publish_family, talk_family)
                    .chain()
                    .in_set(WorldSet)
                    .run_if(in_state(GameFlow::Playing)),
            );
    }
}

#[allow(clippy::too_many_arguments)]
fn morning_call(
    lang: Res<Language>,
    upstairs: Res<Upstairs>,
    (mut state, mut shelter): (ResMut<RescueState>, ResMut<Shelter>),
    mut campaign: ResMut<Campaign>,
    mut notice: ResMut<WorldNotice>,
    mut map: ResMut<MapProgress>,
    player: Query<&Transform, With<Player>>,
) {
    let hour = (campaign.game_min / 60.0) % 24.0;
    let day = (campaign.game_min / 1440.0).floor() as u32;
    if !(9.0..=9.5).contains(&hour) || state.call.as_ref().is_some_and(|r| r.day == day) {
        return;
    }
    let lang = *lang;
    if let Some(r) = state.call.as_mut()
        && !r.done
        && !r.failed
    {
        r.failed = true;
        notice.text = match lang {
            Language::ZhTw => format!("{}那則求救簡訊，再也沒有回覆了。", r.street),
            Language::En => format!("The message from near {} never answered again.", r.street),
        };
        for g in &mut shelter.group {
            g.mood = (g.mood - 4.0).max(0.0);
        }
    }
    // where the player is on the map (the stairwell while upstairs)
    let from = upstairs
        .landing()
        .or_else(|| player.single().ok().map(|t| t.translation.xz()));
    let Some(from) = from else { return };
    let home = shelter.home.map(Vec2::from_array);
    let Some(call) = pick_call(day, shelter.karma, from, home) else {
        return;
    };
    map.reveal(Vec2::new(call.x, call.z), 20.0);
    map.add_note(Vec2::new(call.x, call.z), NoteKind::Danger);
    campaign.notice = match lang {
        Language::ZhTw => format!(
            "求救簡訊：「我們在{}附近一棟公寓的{}，媽媽腳受傷下不了樓梯……拜託。」（大概位置已標上地圖）",
            call.street,
            floor_words(call.floor, lang)
        ),
        Language::En => format!(
            "Text message: “We are on {} of a walk-up near {}. Mum hurt her leg and we cannot get down the stairs. Please…” (rough spot marked on your map)",
            floor_words(call.floor, lang),
            call.street
        ),
    };
    campaign.notice_t = 10.0;
    // (accelerated time stops for the message)
    campaign.speed = 1;
    campaign.sleeping = false;
    state.call = Some(call);
}

/// `rescueFloor`: on the right floor, the mother is in the room away from the stairs.
fn place_family(
    mut commands: Commands,
    lang: Res<Language>,
    upstairs: Res<Upstairs>,
    state: Res<RescueState>,
    mut notice: ResMut<WorldNotice>,
    rigs: Query<(Entity, &FamilyRig)>,
) {
    let want = state
        .open()
        .filter(|r| upstairs.floor() == Some((r.building, r.floor)));
    for (e, rig) in &rigs {
        if want.is_none_or(|r| r.building != rig.building || r.floor != rig.floor) {
            commands.entity(e).despawn();
        }
    }
    let Some(r) = want else { return };
    if rigs
        .iter()
        .any(|(_, rig)| rig.building == r.building && rig.floor == r.floor)
    {
        return;
    }
    let w = crate::map::world();
    let Some(stair) = crate::map::upstairs::stair_of(w, r.building) else {
        return;
    };
    let b = &w.blds[r.building as usize];
    let fixtures = plan_floor(w, b, stair, r.floor);
    // somewhere in the room away from the stairs
    let door = b
        .door
        .unwrap_or(crate::map::data::Door { p: b.c, n: Vec2::X });
    let u = door.tangent();
    let inward = -door.n;
    let mut at = None;
    'find: for v in [3.0, 5.0, 2.0, 7.0, 1.5] {
        for side in [0.0, 1.5, -1.5] {
            let q = door.p + inward * v + u * side;
            if crate::map::data::pip(q, &b.pts)
                && q.distance(stair.landing) > 1.5
                && fixtures.iter().all(|f| !f.contains(q, 0.5))
            {
                at = Some(q);
                break 'find;
            }
        }
    }
    let at = at.unwrap_or(stair.landing + Vec2::new(stair.dir.y, -stair.dir.x) * 1.2);
    let y = stair.floor_y(r.floor);
    commands.spawn((
        FamilyRig {
            building: r.building,
            floor: r.floor,
            pos: at,
        },
        Humanoid {
            opts: RigOpts {
                female: true,
                top: 0x9a6a8a,
                bottom: 0x3a3f4a,
                shoes: 0x222222,
                soldier: false,
                jacket: None,
                pack: None,
                outfit: "",
                gun: "",
            },
            driver: Driver::Ally,
        },
        Transform::from_xyz(at.x, y, at.y),
        Visibility::default(),
    ));
    notice.text = tr(
        *lang,
        "樓上有人：「是誰？……拜託，是來救我們的嗎？」",
        "Someone up here: “Who is it?… Please, have you come to help us?”",
    )
    .into();
}

fn publish_family(
    lang: Res<Language>,
    mut spots: ResMut<NpcSpots>,
    mut rigs: Query<(&FamilyRig, Option<&mut RigAnim>)>,
) {
    let mut list = Vec::new();
    for (rig, anim) in &mut rigs {
        if let Some(mut a) = anim {
            a.state = AnimState::Idle;
            a.speed = 0.0;
        }
        list.push(NpcSpot {
            key: NpcKey::Family,
            pos: rig.pos,
            reach: 2.4,
            label: tr(*lang, "交談", "Talk").into(),
            detail: tr(*lang, "被困在樓上的一家人", "A family trapped upstairs").into(),
            through: true,
        });
    }
    spots.publish(|k| k == NpcKey::Family, list);
}

#[allow(clippy::too_many_arguments)]
fn talk_family(
    mut talks: MessageReader<TalkTo>,
    lang: Res<Language>,
    (mut state, mut shelter): (ResMut<RescueState>, ResMut<Shelter>),
    mut inventory: ResMut<Inventory>,
    mut map: ResMut<MapProgress>,
    mut dialogue: ResMut<Dialogue>,
    mut notice: ResMut<WorldNotice>,
    mut commands: Commands,
    rigs: Query<Entity, With<FamilyRig>>,
) {
    for TalkTo(key) in talks.read() {
        if *key != NpcKey::Family {
            continue;
        }
        let Some(r) = state.call.as_mut().filter(|r| !r.done && !r.failed) else {
            continue;
        };
        r.done = true;
        let lang = *lang;
        let away = shelter.group.iter().position(|g| !g.here && !g.dead);
        if let Some(i) = away {
            shelter.group[i].here = true;
        }
        shelter.karma += 1;
        for g in &mut shelter.group {
            g.mood = (g.mood + 6.0).min(100.0);
        }
        inventory.add("can", 2);
        inventory.add("bandage", 1);
        let spot = Vec2::new(r.x, r.z);
        map.remove_near(spot, 3.0);
        map.add_note(spot, NoteKind::Safe);
        let body = match away.map(|i| &shelter.group[i]) {
            Some(g) => match lang {
                Language::ZhTw => format!(
                    "她們今晚會自己摸黑到你的避難所。一起躲在這裡的還有{}——{}。臨走前，她們塞給你兩個罐頭和一卷繃帶。",
                    g.name(lang),
                    g.role(lang)
                ),
                Language::En => format!(
                    "They will make their way to your shelter tonight. With them comes {} — “{}”. Before you leave they press two cans and a bandage into your hands.",
                    g.name(lang),
                    g.role(lang)
                ),
            },
            None => tr(
                lang,
                "「謝謝……我們知道避難所在哪，晚上自己過去。」臨走前，她們塞給你兩個罐頭和一卷繃帶。",
                "“Thank you… We know where the shelter is; we will get there tonight.” Before you leave they press two cans and a bandage into your hands.",
            )
            .into(),
        };
        dialogue.0 = Some(DialogueCard {
            title: tr(lang, "被困在樓上的一家人", "A family trapped upstairs").into(),
            body,
            options: Vec::new(),
        });
        notice.text = tr(
            lang,
            "救出了一家人（避難所大家的心情好了一點）",
            "You got a family out (spirits lift at the shelter)",
        )
        .into();
        for e in &rigs {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_names_a_real_walk_up_floor_and_street() {
        let w = crate::map::world();
        let r = pick_call(3, 0, w.spawn, None).expect("a walk-up in range");
        let s = crate::map::upstairs::stair_of(w, r.building).expect("stairs");
        assert!(s.floors >= 3 && (2..=s.floors).contains(&r.floor));
        let d = w.blds[r.building as usize].c.distance(w.spawn);
        assert!(d > 80.0 && d < 320.0);
        assert!(!r.street.is_empty());
        assert!(Vec2::new(r.x, r.z).distance(w.blds[r.building as usize].c) <= 22.0);
        // seeded by day and karma
        assert_eq!(pick_call(3, 0, w.spawn, None), Some(r.clone()));
        assert!(RescueState { call: Some(r) }.is_valid());
    }

    #[test]
    fn floor_words_follow_the_web() {
        assert_eq!(floor_words(3, Language::ZhTw), "三樓");
        assert_eq!(floor_words(12, Language::ZhTw), "十二樓");
        assert_eq!(floor_words(1, Language::En), "ground floor");
        assert_eq!(floor_words(4, Language::En), "floor 4");
    }
}
