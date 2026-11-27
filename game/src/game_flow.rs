//! Game flow ported from the web build's opening, death, and 奪回萬華 ending.
//!
//! [`GameFlow`] is the only game-state enum. Title → prologue → playing, then
//! death, reclaiming all fifteen 萬華 districts, or an evacuation. Screens and
//! pointer input belong to [`crate::ui`]. This module only advances state and
//! writes story messages. The one visual it still owns is the prologue phone
//! banner, despawned on leaving [`GameFlow::Prologue`].

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::{GameState, PlayerDied, ResetCombat};
use crate::story::{
    ALL_CLEAR_ID, AM765_STATION, AlertKind, DistrictReclaimed, EvacComplete, EvacRoute,
    GAME_MINUTES_PER_REAL_SECOND, PRO_NEWS, PROLOGUE_START_MIN, RunStoryEvent, StoryAlert,
    StoryBroadcast, StoryEmit, TIMELINE, WANHUA_DISTRICTS, event_by_id, script_by_id,
};

/// Single source of truth for where the session is.
#[derive(States, Clone, PartialEq, Eq, Hash, Debug)]
pub enum GameFlow {
    Title,
    Prologue,
    Playing,
    GameOver(Ending),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Ending {
    /// `die()` — 你陣亡了.
    Death,
    /// Every 萬華 district taken back — 奪回萬華.
    Reclaimed,
    /// Helicopter extraction to the carrier.
    EvacHelicopter,
    /// Boat extraction down the Tamsui.
    EvacBoat,
}

/// Title button / Enter. Starts the prologue.
#[derive(Message, Clone, Copy, Debug)]
pub struct StartGame;

/// Death or ending screen back to the title. Also the original reload.
#[derive(Message, Clone, Copy, Debug)]
pub struct RestartGame;

/// 跳過開場. Prologue only.
#[derive(Message, Clone, Copy, Debug)]
pub struct SkipPrologue;

#[derive(Resource, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Campaign {
    pub reclaimed: Vec<String>,
    pub(crate) prologue_t: f32,
    pub(crate) news_i: usize,
    pub(crate) raid: bool,
    /// Family-group message during the air raid (`updatePrologue` stage 2).
    pub(crate) family: bool,
    /// In-game minutes from 00:00 on day 1. The prologue pins this to 07:20.
    pub game_min: f32,
    /// Fraction lost when the public f32 clock is rounded. Persisting this carry
    /// lets a frame advance time even after one frame is smaller than a clock ULP.
    pub clock_remainder: f64,
    /// One in-game day lasts 1, 2, or 4 real hours.
    pub day_hours: u8,
    /// Original speed controls: 1, 3, or 20.
    pub speed: u8,
    pub sleeping: bool,
    /// Helicopter or boat already taken. Closes the window the way `UT.ev.ex` does.
    pub(crate) evac_taken: bool,
    /// Phone / radio line. The prologue card and the in-play card read it.
    pub(crate) notice: String,
    pub(crate) notice_t: f32,
    pub(crate) fired: Vec<String>,
}

impl Default for Campaign {
    fn default() -> Self {
        Self {
            reclaimed: Vec::new(),
            prologue_t: 0.0,
            news_i: 0,
            raid: false,
            family: false,
            game_min: 0.0,
            clock_remainder: 0.0,
            day_hours: 1,
            speed: 1,
            sleeping: false,
            evac_taken: false,
            notice: String::new(),
            notice_t: 0.0,
            fired: Vec::new(),
        }
    }
}

/// Prologue phone card. [`DespawnOnExit`] removes it when play (or any other state) begins.
#[derive(Component)]
struct PrologueBanner;

#[derive(Component)]
struct PrologueBannerText;

/// In-play phone / radio card. Despawns on leaving [`GameFlow::Playing`], so an ending is only the UI screen.
#[derive(Component)]
struct PlayingBanner;

#[derive(Component)]
struct PlayingBannerText;

#[derive(Component)]
struct PlayingBannerFooter;

/// Ordered clock/story stage. Shelter dawn and world vitals follow it.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlowSet;

pub struct GameFlowPlugin;

impl Plugin for GameFlowPlugin {
    fn build(&self, app: &mut App) {
        app.insert_state(GameFlow::Title)
            .init_resource::<Campaign>()
            .add_message::<StartGame>()
            .add_message::<RestartGame>()
            .add_message::<SkipPrologue>()
            .add_message::<RunStoryEvent>()
            .add_message::<StoryAlert>()
            .add_message::<StoryBroadcast>()
            .add_message::<DistrictReclaimed>()
            .add_message::<EvacComplete>()
            .configure_sets(
                Update,
                FlowSet
                    .after(crate::input::InputSet)
                    .before(crate::combat::CombatSet),
            )
            .add_systems(
                Update,
                (
                    apply_flow,
                    prologue,
                    advance_story,
                    story_events,
                    reclaim,
                    sync_over,
                    sync_prologue_banner,
                )
                    .chain()
                    .in_set(FlowSet),
            )
            .add_systems(
                PostUpdate,
                sync_playing_banner
                    .after(crate::hud::HudSet)
                    .before(bevy::ui::UiSystems::Prepare),
            );
    }
}

fn apply_flow(
    state: Res<State<GameFlow>>,
    mut next: ResMut<NextState<GameFlow>>,
    mut campaign: ResMut<Campaign>,
    mut resets: MessageWriter<ResetCombat>,
    mut alerts: MessageWriter<StoryAlert>,
    mut starts: MessageReader<StartGame>,
    mut restarts: MessageReader<RestartGame>,
    mut skips: MessageReader<SkipPrologue>,
    mut deaths: MessageReader<PlayerDied>,
    mut evacs: MessageReader<EvacComplete>,
    preferences: Option<Res<crate::settings::GamePreferences>>,
) {
    for _ in restarts.read() {
        *campaign = Campaign::default();
        resets.write(ResetCombat);
        next.set(GameFlow::Title);
    }
    if !matches!(state.get(), GameFlow::Title) {
        starts.clear();
    } else if !starts.is_empty() {
        starts.clear();
        *campaign = Campaign::default();
        campaign.day_hours = preferences.as_ref().map_or(1, |p| {
            if matches!(p.day_hours, 1 | 2 | 4) {
                p.day_hours
            } else {
                1
            }
        });
        campaign.game_min = PROLOGUE_START_MIN;
        resets.write(ResetCombat);
        next.set(GameFlow::Prologue);
    }
    if matches!(state.get(), GameFlow::Prologue) && !skips.is_empty() {
        skips.clear();
        campaign.notice.clear();
        campaign.notice_t = 0.0;
        next.set(GameFlow::Playing);
    } else {
        skips.clear();
    }
    if matches!(state.get(), GameFlow::Playing | GameFlow::Prologue) && !deaths.is_empty() {
        deaths.clear();
        // Damage and extraction can arrive in the same frame. A fatal hit must
        // never be overwritten by the later evacuation message.
        evacs.clear();
        campaign.notice_t = 0.0;
        next.set(GameFlow::GameOver(Ending::Death));
        return;
    } else {
        deaths.clear();
    }
    for evac in evacs.read() {
        if matches!(state.get(), GameFlow::Playing) {
            let ending = match evac.route {
                EvacRoute::Helicopter => Ending::EvacHelicopter,
                EvacRoute::Boat => Ending::EvacBoat,
            };
            alerts.write(StoryAlert {
                kind: AlertKind::Clear,
            });
            campaign.notice_t = 0.0;
            next.set(GameFlow::GameOver(ending));
        }
    }
}

fn prologue(
    time: Res<Time>,
    state: Res<State<GameFlow>>,
    mut next: ResMut<NextState<GameFlow>>,
    mut campaign: ResMut<Campaign>,
    mut alerts: MessageWriter<StoryAlert>,
) {
    if !matches!(state.get(), GameFlow::Prologue) {
        return;
    }
    campaign.prologue_t += time.delta_secs();
    let t = campaign.prologue_t;
    while campaign.news_i < PRO_NEWS.len() && t >= PRO_NEWS[campaign.news_i].0 {
        let (_, who, text) = PRO_NEWS[campaign.news_i];
        campaign.notice = format!("{who}\n{text}");
        campaign.notice_t = 6.0;
        campaign.news_i += 1;
    }
    // No commute target on this slice: the raid starts after the last push, as stage 2 of the original.
    if !campaign.raid && (campaign.news_i >= PRO_NEWS.len() || t > 75.0) {
        campaign.raid = true;
        campaign.prologue_t = 0.0;
        alerts.write(StoryAlert {
            kind: AlertKind::Alert,
        });
        campaign.notice = "國家級警報\n飛彈空襲警報！共軍部隊於淡水河沿岸登陸，請立即就近避難，遠離窗戶。\n\n防空廣播：「這不是演習！這不是演習！請民眾立即進入地下室或避難處所……」".into();
        campaign.notice_t = 8.0;
    } else if campaign.raid && !campaign.family && campaign.prologue_t > 5.0 {
        campaign.family = true;
        campaign.notice = "家族群組\n你在哪？？西園路那邊有軍車開進來了！不要出門！\n\n街口傳來履帶聲——穿著陌生迷彩的士兵封鎖了路口。".into();
        campaign.notice_t = 8.0;
    } else if campaign.raid && campaign.prologue_t > 16.0 {
        next.set(GameFlow::Playing);
        // The phone card belongs to the prologue. Playing must not inherit it.
        campaign.notice.clear();
        campaign.notice_t = 0.0;
    }
    campaign.notice_t = (campaign.notice_t - time.delta_secs()).max(0.0);
}

fn deliver(
    campaign: &mut Campaign,
    alerts: &mut MessageWriter<StoryAlert>,
    broadcasts: &mut MessageWriter<StoryBroadcast>,
    event: &crate::story::StoryEvent,
) {
    if campaign.fired.iter().any(|id| id == event.id) {
        return;
    }
    campaign.fired.push(event.id.to_string());
    campaign.notice = event.text.to_string();
    campaign.notice_t = if event.id == ALL_CLEAR_ID { 10.0 } else { 8.0 };
    for step in event.emits {
        match step {
            StoryEmit::Broadcast(id) => {
                if let Some(script) = script_by_id(id) {
                    broadcasts.write(StoryBroadcast {
                        id: script.id,
                        station: AM765_STATION,
                        text: script.text,
                    });
                }
            }
            StoryEmit::AlertTest => {
                alerts.write(StoryAlert {
                    kind: AlertKind::Test,
                });
            }
            StoryEmit::Alert => {
                alerts.write(StoryAlert {
                    kind: AlertKind::Alert,
                });
            }
            StoryEmit::AlertClear => {
                alerts.write(StoryAlert {
                    kind: AlertKind::Clear,
                });
            }
        }
    }
}

/// `updateStory`: each `STORY` row once, when `gameMin` reaches it. 📻 lines also start the radio bed.
/// Catalog-only events remain explicit requests, matching the web build.
fn advance_story(
    time: Res<Time>,
    state: Res<State<GameFlow>>,
    mut game: ResMut<GameState>,
    mut campaign: ResMut<Campaign>,
    mut broadcasts: MessageWriter<StoryBroadcast>,
    pause: Option<Res<crate::ui::PauseOpen>>,
) {
    if pause.is_some_and(|p| p.0) || !matches!(state.get(), GameFlow::Playing) {
        return;
    }
    let dt = time.delta_secs();
    let scale = if campaign.sleeping {
        20.0
    } else {
        f32::from(campaign.speed)
    };
    let exact = f64::from(campaign.game_min)
        + campaign.clock_remainder
        + f64::from(dt) * f64::from(scale) * f64::from(GAME_MINUTES_PER_REAL_SECOND)
            / f64::from(campaign.day_hours);
    campaign.game_min = exact as f32;
    campaign.clock_remainder = exact - f64::from(campaign.game_min);
    game.survived = campaign.game_min;
    campaign.notice_t = (campaign.notice_t - dt).max(0.0);
    for line in TIMELINE {
        if campaign.game_min < line.minute as f32 || campaign.fired.iter().any(|id| id == line.id) {
            continue;
        }
        campaign.fired.push(line.id.to_string());
        campaign.notice = format!("{}\n{}", line.who, line.text);
        campaign.notice_t = 7.0;
        if line.radio {
            broadcasts.write(StoryBroadcast {
                id: line.id,
                station: line.who,
                text: line.text,
            });
        }
    }
}

fn story_events(
    mut campaign: ResMut<Campaign>,
    mut runs: MessageReader<RunStoryEvent>,
    mut alerts: MessageWriter<StoryAlert>,
    mut broadcasts: MessageWriter<StoryBroadcast>,
) {
    for run in runs.read() {
        let Some(event) = event_by_id(&run.id) else {
            campaign.notice = format!("沒有這個故事事件：{}", run.id);
            campaign.notice_t = 4.0;
            continue;
        };
        deliver(&mut campaign, &mut alerts, &mut broadcasts, event);
    }
}

fn reclaim(
    state: Res<State<GameFlow>>,
    mut next: ResMut<NextState<GameFlow>>,
    mut campaign: ResMut<Campaign>,
    mut taken: MessageReader<DistrictReclaimed>,
    mut alerts: MessageWriter<StoryAlert>,
    mut broadcasts: MessageWriter<StoryBroadcast>,
) {
    for district in taken.read() {
        if WANHUA_DISTRICTS.contains(&district.name.as_str())
            && !campaign.reclaimed.iter().any(|name| name == &district.name)
        {
            campaign.reclaimed.push(district.name.clone());
            campaign.notice = format!("已奪回{}", district.name);
            campaign.notice_t = 5.0;
        }
    }
    if matches!(state.get(), GameFlow::Playing)
        && campaign.reclaimed.len() >= WANHUA_DISTRICTS.len()
    {
        if let Some(event) = event_by_id(ALL_CLEAR_ID) {
            deliver(&mut campaign, &mut alerts, &mut broadcasts, event);
        }
        campaign.notice_t = 0.0;
        next.set(GameFlow::GameOver(Ending::Reclaimed));
    }
}

fn sync_over(
    state: Res<State<GameFlow>>,
    pause: Option<Res<crate::ui::PauseOpen>>,
    mut game: ResMut<GameState>,
) {
    let live = matches!(state.get(), GameFlow::Prologue | GameFlow::Playing)
        && !pause.is_some_and(|p| p.0);
    if !live {
        game.over = true;
    } else if game.hp > 0.0 {
        game.over = false;
    }
}

/// Draws the current prologue phone line. The entity is marked [`DespawnOnExit`]
/// so leaving the prologue cannot leave the card on screen.
fn sync_prologue_banner(
    mut commands: Commands,
    state: Res<State<GameFlow>>,
    campaign: Res<Campaign>,
    card: Option<Single<&mut Visibility, With<PrologueBanner>>>,
    body: Option<Single<&mut Text, With<PrologueBannerText>>>,
) {
    if !matches!(state.get(), GameFlow::Prologue) {
        return;
    }
    let show = campaign.notice_t > 0.0 && !campaign.notice.is_empty();
    let Some(mut card) = card else {
        commands
            .spawn((
                PrologueBanner,
                DespawnOnExit(GameFlow::Prologue),
                Node {
                    position_type: PositionType::Absolute,
                    top: px(12),
                    left: px(16),
                    right: px(16),
                    padding: UiRect::all(px(10)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.05, 0.07, 0.11)),
                GlobalZIndex(70),
                Visibility::Hidden,
            ))
            .with_child((
                PrologueBannerText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::srgb(0.86, 0.93, 1.0)),
                TextLayout::justify(Justify::Left),
            ));
        return;
    };
    **card = if show {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if show && let Some(mut body) = body {
        body.0 = campaign.notice.clone();
    }
}

/// The phone/radio banner shares the HUD geometry and opens the journal on tap.
/// The full original report is available in that journal even on a short phone.
fn sync_playing_banner(
    mut commands: Commands,
    state: Res<State<GameFlow>>,
    campaign: Res<Campaign>,
    modal: Option<Res<crate::ui::UiModal>>,
    pause: Option<Res<crate::ui::PauseOpen>>,
    lang: Option<Res<crate::i18n::Language>>,
    geometry: Option<Res<crate::hud::HudGeometry>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    card: Option<Single<(&mut Visibility, &mut Node), With<PlayingBanner>>>,
    body: Option<
        Single<
            (&mut Text, &mut Node),
            (
                With<PlayingBannerText>,
                Without<PlayingBannerFooter>,
                Without<PlayingBanner>,
            ),
        >,
    >,
    footer: Option<Single<&mut Text, (With<PlayingBannerFooter>, Without<PlayingBannerText>)>>,
) {
    if !matches!(state.get(), GameFlow::Playing) {
        return;
    }
    let viewport = windows.iter().next().map_or(Vec2::new(1280.0, 720.0), |w| {
        Vec2::new(w.width(), w.height())
    });
    let rect = geometry
        .as_ref()
        .and_then(|g| g.banner)
        .unwrap_or_else(|| crate::hud::survival_hud_layout(viewport, false).banner);
    let footer_copy = crate::i18n::tr(
        lang.as_ref().map_or(crate::i18n::Language::ZhTw, |l| **l),
        "點一下查看通報與日誌",
        "Tap to read reports and journal",
    );
    let Some(mut card) = card else {
        commands
            .spawn((
                PlayingBanner,
                DespawnOnExit(GameFlow::Playing),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(rect.min.x),
                    top: px(rect.min.y),
                    width: px(rect.width()),
                    height: px(rect.height()),
                    padding: UiRect::all(px(8)),
                    border: UiRect::left(px(3)),
                    border_radius: BorderRadius::all(px(5)),
                    overflow: Overflow::clip(),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.07, 0.10, 0.94)),
                BorderColor::all(Color::srgb(0.52, 0.64, 0.77)),
                GlobalZIndex(25),
                Visibility::Hidden,
                Pickable::IGNORE,
            ))
            .with_children(|parent| {
                parent.spawn((
                    PlayingBannerText,
                    Node {
                        width: percent(100),
                        height: px(rect.height() - 35.0),
                        overflow: Overflow::clip(),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Text::new(""),
                    TextFont::from_font_size(FontSize::Px(12.0)),
                    TextColor(Color::srgb(0.86, 0.93, 1.0)),
                    TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                ));
                parent.spawn((
                    PlayingBannerFooter,
                    Text::new(footer_copy),
                    TextFont::from_font_size(FontSize::Px(10.0)),
                    TextColor(Color::srgb(0.75, 0.69, 0.50)),
                ));
            });
        return;
    };
    let show = campaign.notice_t > 0.0
        && !campaign.notice.is_empty()
        && !modal.is_some_and(|m| m.open)
        && !pause.is_some_and(|p| p.0);
    *card.0 = if show {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    card.1.left = px(rect.min.x);
    card.1.top = px(rect.min.y);
    card.1.width = px(rect.width());
    card.1.height = px(rect.height());
    if let Some(mut body) = body {
        body.1.height = px((rect.height() - 35.0).max(15.0));
        if show {
            body.0.0 = campaign.notice.clone();
        }
    }
    if let Some(mut footer) = footer {
        footer.0 = footer_copy.to_owned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_run_clock_carries_sub_ulp_frames_and_survives_serialization() {
        // These are the exact f32 magnitudes at which the former clock stopped
        // advancing at 60 FPS with a four-hour or one-hour day respectively.
        for (minute, day_hours) in [(32_768.0, 4), (131_072.0, 1)] {
            let mut app = App::new();
            app.init_resource::<Time>()
                .insert_resource(State::new(GameFlow::Playing))
                .insert_resource(Campaign {
                    game_min: minute,
                    day_hours,
                    ..default()
                })
                .init_resource::<GameState>()
                .add_message::<StoryBroadcast>()
                .add_systems(Update, advance_story);
            let step = std::time::Duration::from_nanos(16_666_667);
            let mut expected = f64::from(minute);
            for frame in 0..120 {
                app.world_mut().resource_mut::<Time>().advance_by(step);
                let dt = app.world().resource::<Time>().delta_secs();
                expected +=
                    f64::from(dt) * f64::from(GAME_MINUTES_PER_REAL_SECOND) / f64::from(day_hours);
                app.update();
                if frame == 60 {
                    let encoded = serde_json::to_vec(app.world().resource::<Campaign>()).unwrap();
                    app.insert_resource(serde_json::from_slice::<Campaign>(&encoded).unwrap());
                }
            }
            let clock = app.world().resource::<Campaign>();
            assert!(clock.game_min > minute, "day_hours={day_hours}");
            assert!(
                (f64::from(clock.game_min) + clock.clock_remainder - expected).abs() < 1e-8,
                "day_hours={day_hours}: {} + {} vs {expected}",
                clock.game_min,
                clock.clock_remainder
            );
        }
    }

    #[test]
    fn day_length_preference_is_copied_only_when_departing() {
        let mut app = App::new();
        app.insert_resource(State::new(GameFlow::Title))
            .init_resource::<NextState<GameFlow>>()
            .init_resource::<Campaign>()
            .insert_resource(crate::settings::GamePreferences { day_hours: 4 })
            .add_message::<ResetCombat>()
            .add_message::<StoryAlert>()
            .add_message::<StartGame>()
            .add_message::<RestartGame>()
            .add_message::<SkipPrologue>()
            .add_message::<PlayerDied>()
            .add_message::<EvacComplete>()
            .add_systems(Update, apply_flow);
        app.world_mut().write_message(StartGame);
        app.update();
        assert_eq!(app.world().resource::<Campaign>().day_hours, 4);
        app.insert_resource(State::new(GameFlow::Playing));
        app.world_mut()
            .resource_mut::<crate::settings::GamePreferences>()
            .day_hours = 1;
        app.update();
        assert_eq!(app.world().resource::<Campaign>().day_hours, 4);
    }
    #[test]
    fn fatal_hit_wins_over_an_evacuation_in_the_same_frame() {
        let mut app = App::new();
        app.insert_resource(State::new(GameFlow::Playing))
            .init_resource::<NextState<GameFlow>>()
            .init_resource::<Campaign>()
            .add_message::<ResetCombat>()
            .add_message::<StoryAlert>()
            .add_message::<StartGame>()
            .add_message::<RestartGame>()
            .add_message::<SkipPrologue>()
            .add_message::<PlayerDied>()
            .add_message::<EvacComplete>()
            .add_systems(Update, apply_flow);
        app.world_mut().write_message(PlayerDied);
        app.world_mut().write_message(EvacComplete {
            route: EvacRoute::Helicopter,
        });
        app.update();
        assert!(matches!(
            app.world().resource::<NextState<GameFlow>>(),
            NextState::Pending(GameFlow::GameOver(Ending::Death))
        ));
        // The queued extraction was consumed rather than delayed to the next
        // update, where it could replace the pending death transition.
        app.update();
        assert!(matches!(
            app.world().resource::<NextState<GameFlow>>(),
            NextState::Pending(GameFlow::GameOver(Ending::Death))
        ));
    }
}
