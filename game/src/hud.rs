//! Heads-up display: HP bar, kills, reclaimed districts, control hint.
//! Title, prologue, and endings are drawn by the UI plugin. Game flow owns the state.
//!
//! Ammo and weapon are slots until combat publishes that resource. Time reads
//! [`GameState::survived`].

use bevy::prelude::*;

use crate::combat::GameState;
use crate::game_flow::{Campaign, GameFlow};
use crate::story::WANHUA_DISTRICTS;

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct StatsText;

#[derive(Component)]
struct HpFill;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, update_hud);
    }
}

fn spawn_hud(mut commands: Commands, asset_server: Res<AssetServer>) {
    let face = asset_server.load("fonts/NotoSansTC-Subset.otf");
    let font = |size: f32| TextFont {
        font: bevy::text::FontSource::Handle(face.clone()),
        font_size: FontSize::Px(size),
        ..default()
    };

    commands
        .spawn((
            HudRoot,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                left: px(12),
                top: px(12),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                padding: UiRect::all(px(8)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("保衛台北  |  萬華"),
                font(16.0),
                TextColor(Color::srgb(1.0, 0.85, 0.3)),
            ));
            p.spawn((
                Node {
                    width: px(180),
                    height: px(10),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.15)),
            ))
            .with_child((
                HpFill,
                Node {
                    width: percent(100),
                    height: percent(100),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.3, 0.85, 0.4)),
            ));
            p.spawn((
                StatsText,
                Text::new(""),
                font(15.0),
                TextColor(Color::WHITE),
            ));
        });

    let hint = if cfg!(target_os = "android") {
        "左側搖桿移動・直向「打」射擊・「蹲」長按撤退"
    } else {
        "WASD／左側搖桿移動・直向「打」射擊・「蹲」長按撤退"
    };
    commands.spawn((
        HudRoot,
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            right: px(12),
            bottom: px(10),
            ..default()
        },
        Text::new(hint),
        TextLayout::new(Justify::Right, LineBreak::WordOrCharacter),
        font(13.0),
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.75)),
    ));
}

fn update_hud(
    flow: Res<State<GameFlow>>,
    state: Res<GameState>,
    campaign: Res<Campaign>,
    mut roots: Query<&mut Visibility, With<HudRoot>>,
    mut stats: Single<&mut Text, With<StatsText>>,
    mut fill: Single<(&mut Node, &mut BackgroundColor), With<HpFill>>,
) {
    let playing = matches!(flow.get(), GameFlow::Playing);
    let shown = if playing {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut roots {
        if *visibility != shown {
            *visibility = shown;
        }
    }
    if !playing || (!state.is_changed() && !campaign.is_changed() && !flow.is_changed()) {
        return;
    }
    let hint = campaign.reclaimed.last().map(String::as_str).unwrap_or("—");
    let total = state.survived.max(0.0) as u32;
    let clock = format!("{:02}:{:02}", total / 60, total % 60);
    stats.0 = format!(
        "HP {:>3.0}   擊倒 {}   已奪回 {}/{}   {}\n彈藥 —　武器 —　{clock}",
        state.hp,
        state.kills,
        campaign.reclaimed.len(),
        WANHUA_DISTRICTS.len(),
        hint
    );
    let frac = (state.hp / 100.0).clamp(0.0, 1.0);
    fill.0.width = percent(frac * 100.0);
    fill.1.0 = Color::srgb(0.9 - 0.6 * frac, 0.25 + 0.6 * frac, 0.3);
}
