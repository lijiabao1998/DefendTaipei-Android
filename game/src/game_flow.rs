//! Game flow ported from the web build's opening, death, and 奪回萬華 ending.
//!
//! [`GameFlow`] is the only game-state enum. Title → prologue → playing, then
//! death, reclaiming all fifteen 萬華 districts, or an evacuation. Screens and
//! pointer input belong to [`crate::ui`]. This module only advances state and
//! writes story messages. The one visual it still owns is the prologue phone
//! banner, despawned on leaving [`GameFlow::Prologue`].

use bevy::prelude::*;

use crate::combat::{GameState, PlayerDied, ResetCombat};
use crate::story::{
    ALL_CLEAR_ID, AM765_STATION, AlertKind, CATALOG_AT, DistrictReclaimed, EVAC_STANDIN,
    EvacComplete, EvacRoute, GAME_MINUTES_PER_REAL_SECOND, PRO_NEWS, PROLOGUE_START_MIN,
    RunStoryEvent, StoryAlert, StoryBroadcast, StoryEmit, TIMELINE, WANHUA_DISTRICTS, evac_open,
    event_by_id, script_by_id,
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

#[derive(Resource)]
pub struct Campaign {
    pub reclaimed: Vec<String>,
    prologue_t: f32,
    news_i: usize,
    raid: bool,
    /// Family-group message during the air raid (`updatePrologue` stage 2).
    family: bool,
    /// In-game minutes from 00:00 on day 1. The prologue pins this to 07:20.
    pub game_min: f32,
    /// Helicopter or boat already taken. Closes the window the way `UT.ev.ex` does.
    evac_taken: bool,
    /// Phone / radio line. The prologue card and the in-play card read it.
    notice: String,
    notice_t: f32,
    fired: Vec<String>,
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

/// Stand-in for walking into the riverside pad or meeting the boat. Only spawned with the in-play card.
#[derive(Component, Clone, Copy)]
struct EvacChoice(EvacRoute);

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
            .add_systems(
                Update,
                (
                    evac_input,
                    apply_flow,
                    prologue,
                    advance_story,
                    story_events,
                    reclaim,
                    sync_over,
                    sync_prologue_banner,
                    sync_playing_banner,
                )
                    .chain(),
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
        campaign.notice_t = 0.0;
        next.set(GameFlow::GameOver(Ending::Death));
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
        campaign.notice = "⚠️ 國家級警報\n飛彈空襲警報！共軍部隊於淡水河沿岸登陸，請立即就近避難，遠離窗戶。\n\n📢 防空廣播：「這不是演習！這不是演習！請民眾立即進入地下室或避難處所……」".into();
        campaign.notice_t = 8.0;
    } else if campaign.raid && !campaign.family && campaign.prologue_t > 5.0 {
        campaign.family = true;
        campaign.notice = "📱 家族群組\n你在哪？？西園路那邊有軍車開進來了！不要出門！\n\n街口傳來履帶聲——穿著陌生迷彩的士兵封鎖了路口。".into();
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
/// Act-1 catalog ids fire at the minutes in [`CATALOG_AT`].
fn advance_story(
    time: Res<Time>,
    state: Res<State<GameFlow>>,
    mut game: ResMut<GameState>,
    mut campaign: ResMut<Campaign>,
    mut alerts: MessageWriter<StoryAlert>,
    mut broadcasts: MessageWriter<StoryBroadcast>,
) {
    if !matches!(state.get(), GameFlow::Playing) {
        return;
    }
    let dt = time.delta_secs();
    campaign.game_min += dt * GAME_MINUTES_PER_REAL_SECOND;
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
    for &(minute, id) in CATALOG_AT {
        if campaign.game_min < minute as f32 {
            continue;
        }
        if let Some(event) = event_by_id(id) {
            deliver(&mut campaign, &mut alerts, &mut broadcasts, event);
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

fn sync_over(state: Res<State<GameFlow>>, mut game: ResMut<GameState>) {
    let live = matches!(state.get(), GameFlow::Prologue | GameFlow::Playing);
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
                BackgroundColor(Color::srgba(0.03, 0.06, 0.1, 0.92)),
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

fn banner_font() -> TextFont {
    TextFont {
        font_size: FontSize::Px(16.0),
        ..default()
    }
}

/// H / B, or the stand-in buttons, finish an evacuation while the window is open.
fn evac_input(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameFlow>>,
    mut campaign: ResMut<Campaign>,
    mut evacs: MessageWriter<EvacComplete>,
) {
    if !matches!(state.get(), GameFlow::Playing)
        || !evac_open(campaign.game_min, campaign.evac_taken)
    {
        return;
    }
    let route = if keys.just_pressed(KeyCode::KeyB) {
        Some(EvacRoute::Boat)
    } else if keys.just_pressed(KeyCode::KeyH) {
        Some(EvacRoute::Helicopter)
    } else {
        None
    };
    if let Some(route) = route {
        campaign.evac_taken = true;
        evacs.write(EvacComplete { route });
    }
}

fn on_evac_button(
    activate: On<bevy::ui_widgets::Activate>,
    state: Res<State<GameFlow>>,
    choices: Query<&EvacChoice>,
    mut campaign: ResMut<Campaign>,
    mut evacs: MessageWriter<EvacComplete>,
) {
    if !matches!(state.get(), GameFlow::Playing)
        || !evac_open(campaign.game_min, campaign.evac_taken)
    {
        return;
    }
    let Ok(choice) = choices.get(activate.entity) else {
        return;
    };
    campaign.evac_taken = true;
    evacs.write(EvacComplete { route: choice.0 });
}

/// Radio and phone lines during play, plus the stand-in evac controls.
/// [`DespawnOnExit`] drops the card when an ending (or the title) begins.
fn sync_playing_banner(
    mut commands: Commands,
    state: Res<State<GameFlow>>,
    campaign: Res<Campaign>,
    card: Option<Single<&mut Visibility, With<PlayingBanner>>>,
    body: Option<Single<&mut Text, With<PlayingBannerText>>>,
    mut buttons: Query<&mut Visibility, (With<EvacChoice>, Without<PlayingBanner>)>,
) {
    if !matches!(state.get(), GameFlow::Playing) {
        return;
    }
    let Some(mut card) = card else {
        commands
            .spawn((
                PlayingBanner,
                DespawnOnExit(GameFlow::Playing),
                Node {
                    position_type: PositionType::Absolute,
                    top: px(12),
                    left: px(16),
                    right: px(16),
                    padding: UiRect::all(px(10)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.03, 0.06, 0.1, 0.92)),
                GlobalZIndex(70),
                Visibility::Hidden,
            ))
            .with_children(|parent| {
                parent.spawn((
                    PlayingBannerText,
                    Text::new(""),
                    banner_font(),
                    TextColor(Color::srgb(0.86, 0.93, 1.0)),
                    TextLayout::justify(Justify::Left),
                ));
                for (route, label, color) in [
                    (
                        EvacRoute::Helicopter,
                        "H　直升機・河濱撤離區",
                        Color::srgb(0.15, 0.38, 0.24),
                    ),
                    (
                        EvacRoute::Boat,
                        "B　小艇・聯絡人接應",
                        Color::srgb(0.12, 0.28, 0.42),
                    ),
                ] {
                    parent
                        .spawn((
                            bevy::ui_widgets::Button,
                            bevy::ui_widgets::ActivateOnPress,
                            EvacChoice(route),
                            Node {
                                padding: UiRect::axes(px(12), px(8)),
                                ..default()
                            },
                            BackgroundColor(color),
                            Visibility::Hidden,
                        ))
                        .observe(on_evac_button)
                        .with_child((Text::new(label), banner_font(), TextColor(Color::WHITE)));
                }
            });
        return;
    };
    let window = evac_open(campaign.game_min, campaign.evac_taken);
    let show = (campaign.notice_t > 0.0 && !campaign.notice.is_empty()) || window;
    **card = if show {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if let Some(mut body) = body {
        if campaign.notice_t > 0.0 && !campaign.notice.is_empty() {
            body.0 = campaign.notice.clone();
        } else if window {
            body.0 = EVAC_STANDIN.to_string();
        }
    }
    let button_vis = if window {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut buttons {
        *visibility = button_vis;
    }
}
