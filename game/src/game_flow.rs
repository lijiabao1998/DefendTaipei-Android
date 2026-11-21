//! Game flow ported from the web build's opening, death, and 奪回萬華 ending.
//!
//! [`GameFlow`] is the only game-state enum. Title → prologue → playing, then
//! death, reclaiming all fifteen 萬華 districts, or an evacuation. The data-driven
//! catalog in [`crate::story`] is not auto-played; send [`RunStoryEvent`].

use bevy::prelude::*;

use crate::city::{Lcg, MAP_HALF, Obstacles};
use crate::combat::{Enemy, EnemyKind, GameState, PlayerDied, ResetCombat, SpawnEnemy};
use crate::player::Player;
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
    spawn_timer: f32,
    rng: Lcg,
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
            spawn_timer: 1.5,
            rng: Lcg(0xdefe_17a1),
            notice: String::new(),
            notice_t: 0.0,
            fired: Vec::new(),
        }
    }
}

#[derive(Component)]
struct FlowRoot;

#[derive(Component)]
struct FlowBody;

#[derive(Component)]
enum FlowButton {
    Start,
    Skip,
    Menu,
}

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
            .add_systems(Startup, spawn_overlay)
            .add_systems(
                Update,
                (
                    read_keys,
                    read_buttons,
                    apply_flow,
                    prologue,
                    occupation_pressure,
                    story_events,
                    reclaim,
                    sync_over,
                    refresh_overlay,
                )
                    .chain(),
            );
    }
}

fn font(size: f32, face: Handle<Font>) -> TextFont {
    TextFont {
        font: bevy::text::FontSource::Handle(face),
        font_size: FontSize::Px(size),
        ..default()
    }
}

fn spawn_overlay(mut commands: Commands, asset_server: Res<AssetServer>) {
    let face = asset_server.load("fonts/NotoSansTC-Subset.otf");
    commands
        .spawn((
            FlowRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                padding: UiRect::all(px(24)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.82)),
            GlobalZIndex(30),
        ))
        .with_children(|parent| {
            parent.spawn((
                FlowBody,
                Text::new(""),
                font(22.0, face.clone()),
                TextColor(Color::WHITE),
                TextLayout::justify(Justify::Center),
            ));
            parent
                .spawn((
                    bevy::ui_widgets::Button,
                    FlowButton::Start,
                    Node {
                        padding: UiRect::axes(px(18), px(10)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.75, 0.55, 0.15)),
                ))
                .with_child((
                    Text::new("新遊戲"),
                    font(18.0, face.clone()),
                    TextColor(Color::BLACK),
                ));
            parent
                .spawn((
                    bevy::ui_widgets::Button,
                    FlowButton::Skip,
                    Node {
                        padding: UiRect::axes(px(18), px(10)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.25, 0.28, 0.32)),
                ))
                .with_child((
                    Text::new("跳過開場 ⏭"),
                    font(16.0, face.clone()),
                    TextColor(Color::WHITE),
                ));
            parent
                .spawn((
                    bevy::ui_widgets::Button,
                    FlowButton::Menu,
                    Node {
                        padding: UiRect::axes(px(18), px(10)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.45, 0.16, 0.14)),
                ))
                .with_child((
                    Text::new("返回主選單"),
                    font(16.0, face),
                    TextColor(Color::WHITE),
                ));
        });
}

fn read_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    state: Res<State<GameFlow>>,
    mut start: MessageWriter<StartGame>,
    mut restart: MessageWriter<RestartGame>,
    mut skip: MessageWriter<SkipPrologue>,
) {
    let enter = keys.any_just_pressed([KeyCode::Enter, KeyCode::Space]);
    let click = mouse.just_pressed(MouseButton::Left);
    match state.get() {
        GameFlow::Title if enter || click => {
            start.write(StartGame);
        }
        GameFlow::Prologue if enter || click || keys.just_pressed(KeyCode::Escape) => {
            skip.write(SkipPrologue);
        }
        GameFlow::GameOver(_)
            if enter || click || keys.any_just_pressed([KeyCode::KeyR, KeyCode::Escape]) =>
        {
            restart.write(RestartGame);
        }
        _ => {}
    }
}

fn read_buttons(
    buttons: Query<&FlowButton, Added<bevy::ui::Pressed>>,
    mut start: MessageWriter<StartGame>,
    mut restart: MessageWriter<RestartGame>,
    mut skip: MessageWriter<SkipPrologue>,
) {
    for button in &buttons {
        match button {
            FlowButton::Start => {
                start.write(StartGame);
            }
            FlowButton::Skip => {
                skip.write(SkipPrologue);
            }
            FlowButton::Menu => {
                restart.write(RestartGame);
            }
        }
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
        campaign.notice =
            "解放軍已攻入萬華，街上到處是哨兵與裝甲車。從現在起，活下去，一區一區奪回來。".into();
        campaign.notice_t = 8.0;
    }
    campaign.notice_t = (campaign.notice_t - time.delta_secs()).max(0.0);
}

/// Stand-in for `spawnSoldiers`: patrols keep arriving on the street ring.
/// The win condition is districts, not clearing these.
fn occupation_pressure(
    time: Res<Time>,
    state: Res<State<GameFlow>>,
    obstacles: Res<Obstacles>,
    mut game: ResMut<GameState>,
    mut campaign: ResMut<Campaign>,
    player: Single<&Transform, With<Player>>,
    enemies: Query<(), With<Enemy>>,
    mut spawns: MessageWriter<SpawnEnemy>,
) {
    if !matches!(state.get(), GameFlow::Playing) || game.over {
        return;
    }
    game.survived += time.delta_secs();
    campaign.spawn_timer -= time.delta_secs();
    if campaign.spawn_timer > 0.0 || enemies.iter().count() >= 14 {
        return;
    }
    campaign.spawn_timer = 1.6;
    let origin = player.translation.xz();
    for _ in 0..10 {
        let angle = campaign.rng.range(0.0, std::f32::consts::TAU);
        let radius = campaign.rng.range(26.0, 34.0);
        let point = origin + Vec2::from_angle(angle) * radius;
        if point.abs().max_element() > MAP_HALF - 1.0
            || obstacles.0.iter().any(|o| o.contains(point, 0.8))
        {
            continue;
        }
        let night = campaign.rng.range(0.0, 1.0) > 0.75;
        spawns.write(SpawnEnemy {
            pos: Vec3::new(point.x, 0.9, point.y),
            kind: if night {
                EnemyKind::NightPatrol
            } else {
                EnemyKind::Patrol
            },
        });
        break;
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

fn refresh_overlay(
    state: Res<State<GameFlow>>,
    game: Res<GameState>,
    campaign: Res<Campaign>,
    mut root: Single<(&mut Visibility, &mut BackgroundColor), With<FlowRoot>>,
    mut body: Single<&mut Text, With<FlowBody>>,
    mut buttons: Query<(&FlowButton, &mut Visibility), Without<FlowRoot>>,
) {
    let (root_vis, background) = &mut *root;
    let playing = matches!(state.get(), GameFlow::Playing);
    **root_vis = Visibility::Inherited;
    background.0 = if playing {
        Color::srgba(0.0, 0.0, 0.0, 0.0)
    } else {
        Color::srgba(0.05, 0.06, 0.08, 0.82)
    };
    let day = (game.survived / 1440.0).floor() as i32 + 1;
    body.0 = match state.get() {
        GameFlow::Title => "捍衛台北\n萬華\n\n新遊戲開始序章。Enter 也可以。".into(),
        GameFlow::Prologue => {
            if campaign.notice_t > 0.0 {
                format!("07:20　鬧鐘響了\n\n{}", campaign.notice)
            } else {
                "07:20　鬧鐘響了\n今天也要去西門町\n跟著路上的人流。Enter 跳過開場。".into()
            }
        }
        GameFlow::Playing => {
            if campaign.notice_t > 0.0 {
                campaign.notice.clone()
            } else {
                String::new()
            }
        }
        GameFlow::GameOver(Ending::Death) => format!(
            "你陣亡了\n這就是你的結局。\n你在淪陷的台北撐過了 {day} 天，擊倒 {} 名佔領軍，奪回了 {} 個街區。\n\n重新開始（新的倖存者）",
            game.kills,
            campaign.reclaimed.len()
        ),
        GameFlow::GameOver(Ending::Reclaimed) => format!(
            "奪回萬華\n結局：國軍與萬華的街坊並肩，把佔領軍趕出了艋舺。\n存活 {day} 天　奪回街區 {} / {}\n擊倒佔領軍 {}",
            campaign.reclaimed.len(),
            WANHUA_DISTRICTS.len(),
            game.kills
        ),
        GameFlow::GameOver(Ending::EvacHelicopter) => {
            "您已成功撤離至 福特號航空母艦（USS Gerald R. Ford, CVN-78）\n撤離路線：直升機・河濱撤離區".into()
        }
        GameFlow::GameOver(Ending::EvacBoat) => {
            "您已成功撤離至 福特號航空母艦（USS Gerald R. Ford, CVN-78）\n撤離路線：小艇・淡水河出海".into()
        }
    };
    if playing && campaign.notice_t <= 0.0 {
        **root_vis = Visibility::Hidden;
    }
    for (button, mut visibility) in &mut buttons {
        *visibility = match (state.get(), button) {
            (GameFlow::Title, FlowButton::Start) => Visibility::Inherited,
            (GameFlow::Prologue, FlowButton::Skip) => Visibility::Inherited,
            (GameFlow::GameOver(_), FlowButton::Menu) => Visibility::Inherited,
            _ => Visibility::Hidden,
        };
    }
}
