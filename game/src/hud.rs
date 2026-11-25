//! The HUD reads the same carried inventory, clock and needs used by gameplay.
use bevy::{prelude::*, window::PrimaryWindow};

use crate::combat::{GameState, PlayerWeaponState};
use crate::game_flow::{Campaign, GameFlow};
use crate::i18n::{Language, tr};
use crate::inventory::{Inventory, item};
use crate::survival::{Shelter, ShelterEnvironment};
use crate::ui::{PauseOpen, UiModal};
use crate::worldplay::PlayerNeeds;

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
            .add_systems(PostUpdate, update_hud);
    }
}

fn spawn_hud(mut commands: Commands) {
    commands
        .spawn((
            HudRoot,
            Visibility::Hidden,
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                left: px(12),
                right: px(12),
                top: px(12),
                max_width: px(520),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                padding: UiRect::all(px(8)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.035, 0.045, 0.85)),
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            parent.spawn((
                StatsText,
                Text::new(""),
                TextFont::from_font_size(FontSize::Px(14.0)),
                TextColor(Color::srgb(0.96, 0.95, 0.89)),
                TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
            ));
            parent
                .spawn((
                    Node {
                        height: px(7),
                        width: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.15)),
                ))
                .with_child((
                    HpFill,
                    Node {
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.3, 0.85, 0.4)),
                ));
        });
}

fn update_hud(
    flow: Res<State<GameFlow>>,
    game: Res<GameState>,
    campaign: Res<Campaign>,
    shelter: Res<Shelter>,
    needs: Res<PlayerNeeds>,
    env: Res<ShelterEnvironment>,
    inventory: Res<Inventory>,
    weapon: Res<PlayerWeaponState>,
    lang: Res<Language>,
    modal: Res<UiModal>,
    pause: Res<PauseOpen>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut roots: Query<(&mut Visibility, &mut Node), (With<HudRoot>, Without<HpFill>)>,
    mut stats: Query<(&mut Text, &mut TextFont), With<StatsText>>,
    mut fill: Query<(&mut Node, &mut BackgroundColor), (With<HpFill>, Without<HudRoot>)>,
) {
    let playing = flow.get() == &GameFlow::Playing && !modal.open && !pause.0;
    let compact = windows.iter().next().is_some_and(|w| w.height() < 500.0);
    for (mut visible, mut node) in &mut roots {
        *visible = if playing {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        node.top = px(if campaign.notice_t > 0.0 { 138.0 } else { 12.0 });
    }
    if !playing {
        return;
    }
    let minute = campaign.game_min.max(0.0) as u32;
    let title = tr(*lang, "萬華", "Wanhua");
    let name = item(&inventory.equipped).map_or(tr(*lang, "徒手", "Unarmed"), |d| d.name(*lang));
    let ammo = item(&inventory.equipped)
        .and_then(|d| d.ammo)
        .map_or(String::new(), |_| {
            format!(" · {} {}", weapon.rounds, tr(*lang, "發", "rounds"))
        });
    let copy = format!(
        "{title} · {} {} · {:02}:{:02} · ×{}\nHP {:.0} · {} {:.0} · {} {:.0} · {} {:.0}\n{name}{ammo} · {:.1} kg · {} {}\n{} · {} · {}",
        tr(*lang, "第", "Day"),
        shelter.day,
        minute / 60 % 24,
        minute % 60,
        if campaign.sleeping {
            20
        } else {
            campaign.speed
        },
        game.hp,
        tr(*lang, "飽足", "Food"),
        needs.hunger,
        tr(*lang, "水分", "Water"),
        needs.thirst,
        tr(*lang, "疲勞", "Fatigue"),
        needs.fatigue,
        inventory.weight(),
        tr(*lang, "擊倒", "Kills"),
        game.kills,
        if env.power {
            tr(*lang, "有電", "Power on")
        } else {
            tr(*lang, "停電", "Blackout")
        },
        if env.water {
            tr(*lang, "供水中", "Water on")
        } else {
            tr(*lang, "停水", "Water cut")
        },
        if campaign.sleeping {
            tr(*lang, "睡眠中，移動可起身", "Sleeping; move to wake")
        } else if game.bleed {
            tr(*lang, "流血，需要止血", "Bleeding; use a bandage")
        } else if env.raining {
            tr(*lang, "下雨", "Rain")
        } else {
            tr(*lang, "陰晴", "Dry")
        },
    );
    for (mut text, mut font) in &mut stats {
        if text.0 != copy {
            text.0 = copy.clone();
        }
        font.font_size = FontSize::Px(if compact { 12.0 } else { 14.0 });
    }
    for (mut node, mut color) in &mut fill {
        let fraction = (game.hp / 100.0).clamp(0.0, 1.0);
        node.width = percent(fraction * 100.0);
        color.0 = Color::srgb(0.9 - 0.6 * fraction, 0.25 + 0.6 * fraction, 0.3);
    }
}
