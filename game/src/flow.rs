//! Game flow: Title → Playing → Won / Lost → (restart) → Playing.
//!
//! UI (title / end screens, lijiabao3) only needs to:
//! - read `State<GameFlow>` to decide what to show, and
//! - write [`StartGame`] (on Title) or [`RestartGame`] (on Won / Lost).
//!
//! Combat moves Playing → Won when every wave is cleared and Playing → Lost when
//! the player dies. Combat / wave systems only run in `GameFlow::Playing`.

use bevy::prelude::*;

use crate::combat::ResetCombat;

/// Top-level game flow state.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameFlow {
    #[default]
    Title,
    Playing,
    Won,
    Lost,
}

/// UI button: start the run (only acted on in `GameFlow::Title`).
#[derive(Message, Clone, Copy, Debug, Default, PartialEq)]
pub struct StartGame;

/// UI button: play again (only acted on in `GameFlow::Won` / `GameFlow::Lost`).
/// Resets combat + waves, then enters `Playing`.
#[derive(Message, Clone, Copy, Debug, Default, PartialEq)]
pub struct RestartGame;

/// TEMPORARY: until the title / end screens exist, skip the title screen and
/// restart automatically after a win or loss so the APK stays playable.
/// Set to `false` once lijiabao3's screens send `StartGame` / `RestartGame`.
pub const AUTO_FLOW_TEMP: bool = true;
/// TEMPORARY (with `AUTO_FLOW_TEMP`): seconds on the end banner before restarting.
pub const AUTO_RESTART_SECS_TEMP: f32 = 5.0;
/// TEMPORARY (with `AUTO_FLOW_TEMP`): taps / clicks / R before this are ignored,
/// so a finger still on the screen does not restart instantly.
const RESTART_GRACE_SECS_TEMP: f32 = 0.6;

#[derive(Resource, Default)]
struct EndTimer(f32);

pub struct FlowPlugin;

impl Plugin for FlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameFlow>()
            .init_resource::<EndTimer>()
            .add_message::<StartGame>()
            .add_message::<RestartGame>()
            .add_systems(OnExit(GameFlow::Won), reset_end_timer)
            .add_systems(OnExit(GameFlow::Lost), reset_end_timer)
            .add_systems(
                Update,
                (
                    auto_flow_temp.run_if(|| AUTO_FLOW_TEMP),
                    start_game.run_if(in_state(GameFlow::Title)),
                    restart_game.run_if(at_end_screen),
                )
                    .chain()
                    .before(crate::combat::CombatSet),
            );
    }
}

fn at_end_screen(flow: Res<State<GameFlow>>) -> bool {
    matches!(flow.get(), GameFlow::Won | GameFlow::Lost)
}

fn start_game(mut starts: MessageReader<StartGame>, mut next: ResMut<NextState<GameFlow>>) {
    if starts.read().count() > 0 {
        next.set(GameFlow::Playing);
    }
}

fn restart_game(
    mut restarts: MessageReader<RestartGame>,
    mut reset: MessageWriter<ResetCombat>,
    mut next: ResMut<NextState<GameFlow>>,
) {
    if restarts.read().count() > 0 {
        reset.write(ResetCombat);
        next.set(GameFlow::Playing);
    }
}

fn reset_end_timer(mut t: ResMut<EndTimer>) {
    t.0 = 0.0;
}

/// TEMPORARY driver behind `AUTO_FLOW_TEMP`: Title → Playing at once; on Won /
/// Lost, restart on tap / click / R or after `AUTO_RESTART_SECS_TEMP`.
fn auto_flow_temp(
    time: Res<Time>,
    flow: Res<State<GameFlow>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    touches: Option<Res<Touches>>,
    mut timer: ResMut<EndTimer>,
    mut start: MessageWriter<StartGame>,
    mut restart: MessageWriter<RestartGame>,
) {
    match flow.get() {
        GameFlow::Title => {
            start.write(StartGame);
        }
        GameFlow::Playing => {}
        GameFlow::Won | GameFlow::Lost => {
            timer.0 += time.delta_secs();
            let pressed = keys.is_some_and(|k| {
                k.any_just_pressed([KeyCode::KeyR, KeyCode::Space, KeyCode::Enter])
            }) || mouse.is_some_and(|m| m.just_pressed(MouseButton::Left))
                || touches.is_some_and(|t| t.any_just_pressed());
            if timer.0 >= AUTO_RESTART_SECS_TEMP || (pressed && timer.0 > RESTART_GRACE_SECS_TEMP) {
                restart.write(RestartGame);
            }
        }
    }
}
