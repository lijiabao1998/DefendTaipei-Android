//! v21 萬華 areas and v24.14 奪回 (Web `src/core/main.ts`: `SECTORS`, `sectorOf`,
//! `checkSectors`, `reclaimParty`, `reclaimDawn`; `src/systems/reclaim.ts`: `reclaimTitle`).
//!
//! Every street patrol belongs to the area whose anchor is nearest its starting
//! point. A core area is taken back when every soldier assigned to it is dead —
//! night patrols included, exactly as `checkSectors` counts them. The moment an
//! area falls the neighbours come out: a title, firecrackers and the temple bell,
//! food pressed into the player's hands, karma and shelter morale. Each later
//! 06:00 the reclaimed areas send rice (one per two) and water (one per three).
//!
//! The nine outer areas are streamed district tiles in the Web build. Those tiles
//! are not ported, so their garrisons cannot be fought here yet; their names stay
//! in the campaign list so the ending keeps the Web's fifteen-area rule.

use bevy::prelude::*;

use crate::audio::SfxEvent;
use crate::game_flow::{Campaign, GameFlow};
use crate::i18n::{Language, tr};
use crate::inventory::{Inventory, item};
use crate::player::Player;
use crate::save::LoadCompleted;
use crate::story::{DistrictReclaimed, WANHUA_DISTRICTS};
use crate::survival::Shelter;
use crate::ui::{PauseOpen, UiModal};
use crate::worldplay::{ScavengeState, WorldNotice, WorldSet};

/// One named area: Traditional Chinese, English, and its anchor (x, z).
#[derive(Clone, Copy, Debug)]
pub struct Sector {
    pub zh: &'static str,
    pub en: &'static str,
    pub at: Vec2,
}

impl Sector {
    pub fn name(&self, lang: Language) -> &'static str {
        tr(lang, self.zh, self.en)
    }
}

const fn s(zh: &'static str, en: &'static str, x: f32, z: f32) -> Sector {
    Sector {
        zh,
        en,
        at: Vec2::new(x, z),
    }
}

/// The fifteen campaign areas in the Web order (`SECTORS[0..NSEC]`).
pub const SECTORS: [Sector; 15] = [
    s("西門町", "Ximending", 330.0, -230.0),
    s("中華路", "Zhonghua Road", 420.0, 260.0),
    s("康定路", "Kangding Road", -40.0, 60.0),
    s("龍山寺", "Longshan Temple", -260.0, 360.0),
    s("貴陽街", "Guiyang Street", -430.0, -220.0),
    s("華西街", "Huaxi Street", -480.0, 330.0),
    s("西園路", "Xiyuan Road", -600.0, 1100.0),
    s("華江", "Huajiang", -1300.0, 820.0),
    s("東園", "Dongyuan", -1000.0, 1550.0),
    s("萬大路", "Wanda Road", -200.0, 1430.0),
    s("南機場", "Nanjichang", 150.0, 1320.0),
    s("青年公園", "Youth Park", 300.0, 1990.0),
    s("加蚋仔", "Kalah", -1000.0, 2120.0),
    s("馬場町", "Machangding", 420.0, 2350.0),
    s("雙園", "Shuangyuan", -1300.0, 2560.0),
];

/// v21 hand-built core: the original 1.1 × 1.0 km box (西門町–龍山寺).
pub const CORE_MIN: Vec2 = Vec2::new(-555.0, -497.0);
pub const CORE_MAX: Vec2 = Vec2::new(555.0, 497.0);

pub fn in_core(p: Vec2) -> bool {
    p.x > CORE_MIN.x && p.x < CORE_MAX.x && p.y > CORE_MIN.y && p.y < CORE_MAX.y
}

/// `sectorOf`: the campaign area of a point is its nearest anchor.
pub fn sector_of(p: Vec2) -> usize {
    SECTORS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            a.at.distance_squared(p)
                .total_cmp(&b.at.distance_squared(p))
        })
        .map_or(0, |(i, _)| i)
}

/// `outerSector`: areas whose anchor lies outside the hand-built core.
pub fn outer_sector(i: usize) -> bool {
    !in_core(SECTORS[i].at)
}

/// Street patrols by stable id and starting point, published by the world planner.
#[derive(Resource, Default, Clone, Debug)]
pub struct PatrolRoster(pub Vec<(u32, Vec2)>);

/// Per-area soldier counts for the HUD and checks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectorCount {
    pub total: u32,
    pub alive: u32,
}

/// What the HUD shows about the area the player stands in.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct SectorStatus {
    pub here: usize,
    pub reclaimed_here: bool,
    pub reclaimed: usize,
    pub counts: [SectorCount; 15],
}

impl SectorStatus {
    /// Occupation soldiers still alive in the campaign areas.
    pub fn alive(&self) -> u32 {
        self.counts.iter().map(|c| c.alive).sum()
    }
}

pub fn count_sectors(roster: &PatrolRoster, dead: &[u32]) -> [SectorCount; 15] {
    let mut counts = [SectorCount::default(); 15];
    for (id, at) in &roster.0 {
        let c = &mut counts[sector_of(*at)];
        c.total += 1;
        if !dead.contains(id) {
            c.alive += 1;
        }
    }
    counts
}

/// Areas whose soldiers are all down and are not yet in the reclaimed list.
pub fn newly_liberated(counts: &[SectorCount; 15], reclaimed: &[String]) -> Vec<usize> {
    (0..SECTORS.len())
        .filter(|&i| {
            counts[i].total > 0
                && counts[i].alive == 0
                && !reclaimed.iter().any(|n| n == SECTORS[i].zh)
        })
        .collect()
}

const RECLAIM_LINES: [(&str, &str); 4] = [
    (
        "鐵捲門一扇一扇拉起來。有人把藏了好幾天的國旗掛上陽台。",
        "One by one the shutters roll up. Someone hangs out a flag hidden for days.",
    ),
    (
        "巷口的阿伯點起一串鞭炮，這一次沒有人叫他小聲一點。",
        "An old man at the corner lights a string of firecrackers. Nobody tells him to keep it down.",
    ),
    (
        "街坊端著熱湯走出來，塞給你就走，不肯收錢。",
        "Neighbours come out with hot soup, press it on you and will not take money.",
    ),
    (
        "廟裡的鐘響了。這條街，是我們自己的了。",
        "The temple bell rings. This street is ours again.",
    ),
];

const RECLAIM_GIFTS: [&[(&str, u32)]; 4] = [
    &[("bento", 2), ("water", 2), ("bandage", 1)],
    &[("hotnoodle", 2), ("boiled", 2), ("can", 1)],
    &[("mantou", 3), ("driedtofu", 2), ("water", 1)],
    &[("roufong", 1), ("can", 2), ("bandage", 2)],
];

/// The gift an area's neighbours press on the player (`RECLAIM_GIFTS[i % 4]`).
pub fn reclaim_gift(i: usize) -> &'static [(&'static str, u32)] {
    RECLAIM_GIFTS[i % RECLAIM_GIFTS.len()]
}

/// `reclaimDawn` amounts for a number of reclaimed areas: (rice, water).
pub fn dawn_supplies(reclaimed: usize) -> (u32, u32) {
    ((reclaimed / 2) as u32, (reclaimed / 3) as u32)
}

/// `reclaimDawn`: adds the neighbours' food to the shelter stash before the
/// day's meals and returns the journal line, if anything arrived.
pub fn reclaim_dawn(shelter: &mut Shelter, reclaimed: usize, lang: Language) -> Option<String> {
    let (rice, water) = dawn_supplies(reclaimed);
    if rice == 0 && water == 0 {
        return None;
    }
    shelter.stash.add("rice", rice);
    shelter.stash.add("water", water);
    Some(match lang {
        Language::ZhTw => format!("奪回街區的街坊送來吃的到避難所：米×{rice}、水×{water}"),
        Language::En => format!(
            "Neighbours from the reclaimed districts left food at the shelter: rice ×{rice}, water ×{water}"
        ),
    })
}

/// An area was just taken back with the player at `at` (the neighbours gather
/// there; also replayed silently after Continue to put the crowd back).
#[derive(Message, Clone, Copy, Debug)]
pub struct ReclaimParty {
    pub sector: usize,
    pub at: Vec2,
}

/// The title card of the moment an area is taken back (`reclaimTitle`).
#[derive(Resource, Default, Clone, Debug)]
pub struct ReclaimTitle {
    pub title: String,
    pub line: String,
    /// Seconds since it appeared; `None` when no card is up.
    pub age: Option<f32>,
}

const TITLE_SECS: f32 = 8.4;

impl ReclaimTitle {
    /// The Web fades in over 1.6 s, holds until 6.5 s, fades out by 8.1 s.
    pub fn alpha(&self) -> f32 {
        let Some(t) = self.age else { return 0.0 };
        if t < 1.6 {
            t / 1.6
        } else if t < 6.5 {
            1.0
        } else {
            (1.0 - (t - 6.5) / 1.6).clamp(0.0, 1.0)
        }
    }
}

/// First check after a new run or Continue is silent: areas already taken in
/// the save are not celebrated again (`checkSectors(true)` at boot).
#[derive(Resource, Default)]
struct SectorSync {
    synced: bool,
}

#[derive(Component)]
struct TitleRoot;
#[derive(Component)]
struct TitleHeading;
#[derive(Component)]
struct TitleLine;

pub struct SectorPlugin;

impl Plugin for SectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PatrolRoster>()
            .init_resource::<SectorStatus>()
            .init_resource::<ReclaimTitle>()
            .init_resource::<SectorSync>()
            .add_message::<ReclaimParty>()
            .add_systems(Startup, spawn_title)
            .add_systems(OnEnter(GameFlow::Prologue), reset_run)
            .add_systems(PreUpdate, resync_on_load)
            .add_systems(
                Update,
                check_sectors
                    .in_set(WorldSet)
                    .run_if(in_state(GameFlow::Playing)),
            )
            .add_systems(PostUpdate, show_title);
    }
}

fn reset_run(mut sync: ResMut<SectorSync>, mut title: ResMut<ReclaimTitle>) {
    sync.synced = false;
    *title = ReclaimTitle::default();
}

fn resync_on_load(
    mut loaded: MessageReader<LoadCompleted>,
    mut sync: ResMut<SectorSync>,
    mut title: ResMut<ReclaimTitle>,
) {
    if loaded.read().count() > 0 {
        sync.synced = false;
        *title = ReclaimTitle::default();
    }
}

#[allow(clippy::too_many_arguments)]
fn check_sectors(
    time: Res<Time>,
    roster: Res<PatrolRoster>,
    progress: Res<ScavengeState>,
    lang: Res<Language>,
    pause: Res<PauseOpen>,
    mut sync: ResMut<SectorSync>,
    mut campaign: ResMut<Campaign>,
    mut status: ResMut<SectorStatus>,
    mut title: ResMut<ReclaimTitle>,
    mut inventory: ResMut<Inventory>,
    mut shelter: ResMut<Shelter>,
    mut notice: ResMut<WorldNotice>,
    mut reclaimed: MessageWriter<DistrictReclaimed>,
    mut out: (MessageWriter<SfxEvent>, MessageWriter<ReclaimParty>),
    player: Query<&Transform, With<Player>>,
) {
    let sfx = &mut out.0;
    if let Some(age) = title.age.as_mut()
        && !pause.0
    {
        *age += time.delta_secs();
        if *age > TITLE_SECS {
            title.age = None;
        }
    }
    let counts = count_sectors(&roster, &progress.dead);
    let fresh = newly_liberated(&counts, &campaign.reclaimed);
    let silent = !sync.synced;
    sync.synced = true;
    for &i in &fresh {
        let sector = SECTORS[i];
        campaign.reclaimed.push(sector.zh.to_owned());
        reclaimed.write(DistrictReclaimed {
            name: sector.zh.to_owned(),
        });
        if silent {
            continue;
        }
        crate::device::request_haptic(35);
        let (zh, en) = RECLAIM_LINES[i % RECLAIM_LINES.len()];
        *title = ReclaimTitle {
            title: match *lang {
                Language::ZhTw => format!("★ {} 奪回來了", sector.zh),
                Language::En => format!("★ {} is ours again", sector.en),
            },
            line: tr(*lang, zh, en).to_owned(),
            age: Some(0.0),
        };
        sfx.write(SfxEvent::Firecrackers);
        sfx.write(SfxEvent::Temple { dist: 30.0 });
        // the neighbours come out around the player, wherever they are
        let here = player.single().map_or(Vec2::ZERO, |p| p.translation.xz());
        campaign
            .reclaim_at
            .push((sector.zh.to_owned(), here.to_array()));
        out.1.write(ReclaimParty {
            sector: i,
            at: here,
        });
        let gift = reclaim_gift(i);
        let mut names = Vec::new();
        for (key, n) in gift {
            inventory.add(key, *n);
            let name = item(key).map_or(*key, |d| d.name(*lang));
            names.push(match *lang {
                Language::ZhTw => format!("{name}×{n}"),
                Language::En => format!("{name} ×{n}"),
            });
        }
        notice.text = match *lang {
            Language::ZhTw => format!("街坊把東西塞到你手上：{}", names.join("、")),
            Language::En => format!(
                "The neighbours press things into your hands: {}",
                names.join(", ")
            ),
        };
        shelter.karma += 1;
        for g in &mut shelter.group {
            g.mood = (g.mood + 8.0).min(100.0);
        }
    }
    let here = player.single().map_or(0, |p| sector_of(p.translation.xz()));
    let next = SectorStatus {
        here,
        reclaimed_here: campaign.reclaimed.iter().any(|n| n == SECTORS[here].zh),
        reclaimed: campaign
            .reclaimed
            .iter()
            .filter(|n| WANHUA_DISTRICTS.contains(&n.as_str()))
            .count(),
        counts,
    };
    status.set_if_neq(next);
}

fn spawn_title(mut commands: Commands) {
    commands
        .spawn((
            TitleRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Percent(22.0),
                padding: UiRect::horizontal(Val::Px(16.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(30),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((
                TitleHeading,
                Text::new(""),
                TextFont::from_font_size(FontSize::Px(44.0)),
                TextColor(Color::srgb(1.0, 0.914, 0.659)),
                TextLayout::new(Justify::Center, LineBreak::WordOrCharacter),
                TextShadow::default(),
                Pickable::IGNORE,
            ));
            root.spawn((
                TitleLine,
                Text::new(""),
                TextFont::from_font_size(FontSize::Px(19.0)),
                TextColor(Color::WHITE),
                TextLayout::new(Justify::Center, LineBreak::WordOrCharacter),
                TextShadow::default(),
                Pickable::IGNORE,
            ));
        });
}

#[allow(clippy::type_complexity)]
fn show_title(
    title: Res<ReclaimTitle>,
    flow: Res<State<GameFlow>>,
    modal: Res<UiModal>,
    windows: Query<&Window>,
    mut root: Query<&mut Visibility, With<TitleRoot>>,
    mut heading: Query<
        (&mut Text, &mut TextColor, &mut TextFont),
        (With<TitleHeading>, Without<TitleLine>),
    >,
    mut line: Query<
        (&mut Text, &mut TextColor, &mut TextFont),
        (With<TitleLine>, Without<TitleHeading>),
    >,
) {
    let alpha = title.alpha();
    let shown = alpha > 0.0 && flow.get() == &GameFlow::Playing && !modal.open;
    for mut v in &mut root {
        v.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    if !shown {
        return;
    }
    // clamp(30px, 6vw, 58px) and clamp(15px, 2.4vw, 21px)
    let vw = windows.iter().next().map_or(1280.0, |w| w.width()) / 100.0;
    let big = (6.0 * vw).clamp(30.0, 58.0);
    let small = (2.4 * vw).clamp(15.0, 21.0);
    for (mut text, mut color, mut font) in &mut heading {
        if text.0 != title.title {
            text.0.clone_from(&title.title);
        }
        color.0 = Color::srgba(1.0, 0.914, 0.659, alpha);
        font.font_size = FontSize::Px(big);
    }
    for (mut text, mut color, mut font) in &mut line {
        if text.0 != title.line {
            text.0.clone_from(&title.line);
        }
        color.0 = Color::srgba(1.0, 1.0, 1.0, alpha);
        font.font_size = FontSize::Px(small);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sector_names_are_the_campaign_list_in_order() {
        let names: Vec<_> = SECTORS.iter().map(|s| s.zh).collect();
        assert_eq!(names, WANHUA_DISTRICTS);
    }

    #[test]
    fn six_anchors_lie_in_the_core_and_nine_outside() {
        let core: Vec<_> = (0..15).filter(|&i| !outer_sector(i)).collect();
        assert_eq!(core, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn nearest_anchor_decides_the_area() {
        // 峨眉街 (Ximending) and 華西街 from the bundled OSM data.
        assert_eq!(SECTORS[sector_of(Vec2::new(228.0, -322.0))].zh, "西門町");
        assert_eq!(SECTORS[sector_of(Vec2::new(-460.0, 266.0))].zh, "華西街");
        assert_eq!(SECTORS[sector_of(Vec2::new(-85.0, 41.0))].zh, "康定路");
    }

    #[test]
    fn an_area_falls_only_when_every_soldier_in_it_is_dead() {
        let roster = PatrolRoster(vec![
            (1, Vec2::new(330.0, -230.0)),
            (2, Vec2::new(320.0, -240.0)),
            (3, Vec2::new(-40.0, 60.0)),
        ]);
        let counts = count_sectors(&roster, &[1]);
        assert_eq!(counts[0], SectorCount { total: 2, alive: 1 });
        assert!(newly_liberated(&counts, &[]).is_empty());
        let counts = count_sectors(&roster, &[1, 2]);
        assert_eq!(newly_liberated(&counts, &[]), vec![0]);
        assert!(newly_liberated(&counts, &["西門町".into()]).is_empty());
        // An area with no soldiers is never "taken" (`total > 0`).
        assert!(!newly_liberated(&counts, &[]).contains(&4));
    }

    #[test]
    fn dawn_supplies_follow_the_web_ratios() {
        assert_eq!(dawn_supplies(0), (0, 0));
        assert_eq!(dawn_supplies(1), (0, 0));
        assert_eq!(dawn_supplies(2), (1, 0));
        assert_eq!(dawn_supplies(3), (1, 1));
        assert_eq!(dawn_supplies(6), (3, 2));
        let mut shelter = Shelter::default();
        let rice = shelter.stash.count("rice");
        assert!(reclaim_dawn(&mut shelter, 1, Language::ZhTw).is_none());
        let line = reclaim_dawn(&mut shelter, 6, Language::ZhTw).unwrap();
        assert_eq!(shelter.stash.count("rice"), rice + 3);
        assert!(line.contains("米×3") && line.contains("水×2"));
    }

    #[test]
    fn gifts_are_real_items() {
        for i in 0..4 {
            for (key, n) in reclaim_gift(i) {
                assert!(item(key).is_some(), "{key}");
                assert!(*n > 0);
            }
        }
    }

    #[test]
    fn title_fades_like_the_web_card() {
        let mut t = ReclaimTitle::default();
        assert_eq!(t.alpha(), 0.0);
        t.age = Some(0.8);
        assert!((t.alpha() - 0.5).abs() < 1e-5);
        t.age = Some(3.0);
        assert_eq!(t.alpha(), 1.0);
        t.age = Some(8.2);
        assert_eq!(t.alpha(), 0.0);
    }
}
