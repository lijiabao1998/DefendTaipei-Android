//! Heads-up display: HP bar, kills / wave / time, game-over banner, control hint.

use bevy::prelude::*;

use crate::combat::GameState;

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

fn spawn_hud(mut commands: Commands) {
    let font = |size: f32| TextFont {
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
            right: px(12),
            bottom: px(10),
            ..default()
        },
        Text::new(hint),
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
    mut stats: Single<&mut Text, (With<StatsText>, Without<Banner>)>,
    mut fill: Single<(&mut Node, &mut BackgroundColor), With<HpFill>>,
    mut banner: Single<(&mut Visibility, &Children), With<Banner>>,
    mut texts: Query<&mut Text, (Without<StatsText>, Without<HpFill>)>,
) {
    if !state.is_changed() {
        return;
    }
    stats.0 = format!(
        "HP {:>3.0}   Kills {}   Wave {}   {:>3.0}s",
        state.hp, state.kills, state.wave, state.survived
    );
    let frac = (state.hp / 100.0).clamp(0.0, 1.0);
    fill.0.width = percent(frac * 100.0);
    fill.1.0 = Color::srgb(0.9 - 0.6 * frac, 0.25 + 0.6 * frac, 0.3);

    let (vis, children) = &mut *banner;
    **vis = if state.over {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if state.over
        && let Some(&child) = children.first()
        && let Ok(mut t) = texts.get_mut(child)
    {
        let again = if cfg!(target_os = "android") {
            "Tap to retry"
        } else {
            "R / click to retry"
        };
        t.0 = format!(
            "Wanhua has fallen\n{:.0}s | {} kills | wave {}\n{again}",
            state.survived, state.kills, state.wave
        );
    }
}
