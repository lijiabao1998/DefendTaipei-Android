//! Heads-up display: HP bar, kills / wave / time, game-over banner, control hint.

use bevy::prelude::*;

use crate::combat::GameState;
use crate::game_flow::Campaign;
use crate::story::WANHUA_DISTRICTS;

#[derive(Component)]
struct StatsText;

#[derive(Component)]
struct HpFill;

#[derive(Component)]
struct Banner;

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

    // Top-left panel
    commands
        .spawn((
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
                Text::new("DEFEND TAIPEI  |  Wanhua"),
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

    // Bottom hint
    let hint = if cfg!(target_os = "android") {
        "Drag left side to move  |  pinch to zoom  |  auto-fire"
    } else {
        "WASD / arrows to move  |  wheel to zoom  |  auto-fire"
    };
    commands.spawn((
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

    // Centered game-over banner
    commands
        .spawn((
            Banner,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(20),
        ))
        .with_child((
            Node {
                padding: UiRect::all(px(18)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            Text::new(""),
            font(24.0),
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Center),
        ));
}

fn update_hud(
    state: Res<GameState>,
    campaign: Res<Campaign>,
    mut stats: Single<&mut Text, (With<StatsText>, Without<Banner>)>,
    mut fill: Single<(&mut Node, &mut BackgroundColor), With<HpFill>>,
    mut banner: Single<&mut Visibility, With<Banner>>,
) {
    if !state.is_changed() && !campaign.is_changed() {
        return;
    }
    stats.0 = format!(
        "HP {:>3.0}   擊倒 {}   已奪回 {}/{}",
        state.hp,
        state.kills,
        campaign.reclaimed.len(),
        WANHUA_DISTRICTS.len()
    );
    let frac = (state.hp / 100.0).clamp(0.0, 1.0);
    fill.0.width = percent(frac * 100.0);
    fill.1.0 = Color::srgb(0.9 - 0.6 * frac, 0.25 + 0.6 * frac, 0.3);
    // Title, prologue, and endings are drawn by GameFlow. This banner stays hidden.
    **banner = Visibility::Hidden;
}
