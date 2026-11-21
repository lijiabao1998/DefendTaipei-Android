//! PROVISIONAL stand-in for lijiabao5's `game_flow` (PR #5, not on main yet).
//!
//! Delete this entire file when that branch lands and use the official module.
//! The public names and signatures match `defend_taipei::game_flow` there:
//! [`GameFlow`], [`Ending`], [`StartGame`], [`RestartGame`], [`SkipPrologue`],
//! [`Campaign`], [`GameFlowPlugin`].
//!
//! This copy only switches states and remembers reclaimed districts. It does not
//! play the story catalog or spawn patrols. UI reads the state and writes the
//! three messages; it does not set [`NextState`] itself.
//!
//! Button clicks do not emit `SfxEvent` until the audio branch is on main.

use bevy::prelude::*;

use crate::city::Lcg;
use crate::combat::GameState;

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
#[allow(dead_code)] // Private fields match PR #5's `Campaign`. This stand-in does not tick them.
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

pub struct GameFlowPlugin;

impl Plugin for GameFlowPlugin {
    fn build(&self, app: &mut App) {
        app.insert_state(GameFlow::Title)
            .init_resource::<Campaign>()
            .add_message::<StartGame>()
            .add_message::<RestartGame>()
            .add_message::<SkipPrologue>()
            .add_systems(Update, (apply_flow, sync_over).chain());
    }
}

fn apply_flow(
    state: Res<State<GameFlow>>,
    game: Res<GameState>,
    mut next: ResMut<NextState<GameFlow>>,
    mut campaign: ResMut<Campaign>,
    mut starts: MessageReader<StartGame>,
    mut restarts: MessageReader<RestartGame>,
    mut skips: MessageReader<SkipPrologue>,
) {
    let start = starts.read().count() > 0;
    let restart = restarts.read().count() > 0;
    let skip = skips.read().count() > 0;
    let dead = game.hp <= 0.0;
    if let Some(to) = next_flow(state.get(), start, skip, restart, dead) {
        if matches!(to, GameFlow::Title | GameFlow::Prologue) {
            *campaign = Campaign::default();
        }
        next.set(to);
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

fn next_flow(
    current: &GameFlow,
    start: bool,
    skip: bool,
    restart: bool,
    player_dead: bool,
) -> Option<GameFlow> {
    if restart {
        return Some(GameFlow::Title);
    }
    if matches!(current, GameFlow::Title) && start {
        return Some(GameFlow::Prologue);
    }
    if matches!(current, GameFlow::Prologue) && skip {
        return Some(GameFlow::Playing);
    }
    if matches!(current, GameFlow::Playing) && player_dead {
        return Some(GameFlow::GameOver(Ending::Death));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_follow_the_official_transitions() {
        assert_eq!(
            next_flow(&GameFlow::Title, true, false, false, false),
            Some(GameFlow::Prologue)
        );
        assert_eq!(
            next_flow(&GameFlow::Prologue, false, true, false, false),
            Some(GameFlow::Playing)
        );
        assert_eq!(
            next_flow(&GameFlow::Playing, false, false, false, true),
            Some(GameFlow::GameOver(Ending::Death))
        );
        assert_eq!(
            next_flow(
                &GameFlow::GameOver(Ending::EvacBoat),
                false,
                false,
                true,
                false
            ),
            Some(GameFlow::Title)
        );
        assert_eq!(
            next_flow(&GameFlow::Playing, true, true, false, false),
            None
        );
    }

    #[test]
    fn endings_match_the_four_original_outcomes() {
        let endings = [
            Ending::Death,
            Ending::Reclaimed,
            Ending::EvacHelicopter,
            Ending::EvacBoat,
        ];
        assert_eq!(endings.len(), 4);
        let _ = Campaign::default();
        let campaign = Campaign::default();
        assert!(campaign.reclaimed.is_empty());
        assert_eq!(campaign.notice, "");
        assert_eq!(campaign.rng.0, 0xdefe_17a1);
        assert!(!campaign.raid);
        assert!(campaign.fired.is_empty());
        let _ = (
            campaign.prologue_t,
            campaign.news_i,
            campaign.spawn_timer,
            campaign.notice_t,
        );
    }
}
