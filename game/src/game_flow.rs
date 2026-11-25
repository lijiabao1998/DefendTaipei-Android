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
    ALL_CLEAR_ID, AM765_STATION, AlertKind, DistrictReclaimed, EvacComplete, EvacRoute, PRO_NEWS,
    RunStoryEvent, StoryAlert, StoryBroadcast, StoryEmit, WANHUA_DISTRICTS, event_by_id,
    script_by_id,
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
    /// Phone / radio line. The prologue banner reads it; nothing draws it in play.
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
                    apply_flow,
                    prologue,
                    story_events,
                    reclaim,
                    sync_over,
                    sync_prologue_banner,
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
        resets.write(ResetCombat);
        next.set(GameFlow::Prologue);
    }
    if matches!(state.get(), GameFlow::Prologue) && !skips.is_empty() {
        skips.clear();
        next.set(GameFlow::Playing);
    } else {
        skips.clear();
    }
    if matches!(state.get(), GameFlow::Playing | GameFlow::Prologue) && !deaths.is_empty() {
        deaths.clear();
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
        campaign.notice =
            "⚠️ 國家級警報\n飛彈空襲警報！共軍部隊於淡水河沿岸登陸，請立即就近避難，遠離窗戶。"
                .into();
        campaign.notice_t = 8.0;
    } else if campaign.raid && campaign.prologue_t > 16.0 {
        next.set(GameFlow::Playing);
        // The phone card belongs to the prologue. Playing must not inherit it.
        campaign.notice.clear();
        campaign.notice_t = 0.0;
    }
    campaign.notice_t = (campaign.notice_t - time.delta_secs()).max(0.0);
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
        if campaign.fired.iter().any(|id| id == event.id) {
            continue;
        }
        campaign.fired.push(event.id.to_string());
        campaign.notice = event.text.to_string();
        campaign.notice_t = 8.0;
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
        if event.id == ALL_CLEAR_ID {
            campaign.notice_t = 10.0;
        }
    }
}

fn reclaim(
    state: Res<State<GameFlow>>,
    mut next: ResMut<NextState<GameFlow>>,
    mut campaign: ResMut<Campaign>,
    mut taken: MessageReader<DistrictReclaimed>,
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
