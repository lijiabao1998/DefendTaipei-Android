//! Survival instruments and contextual prompts, adapted from the Web HUD.
//! Every value and warning comes from the active run; no ornamental status is invented.
use bevy::{ecs::system::SystemParam, prelude::*, window::PrimaryWindow};

use crate::combat::{GameState, PlayerStance, PlayerWeaponState};
use crate::game_flow::{Campaign, GameFlow};
use crate::i18n::{Language, tr};
use crate::input::{HudLayout, Orientation, ScreenRect, orientation, safe_insets};
use crate::inventory::{Inventory, item};
use crate::survival::{Shelter, ShelterEnvironment, is_night};
use crate::ui::{PauseOpen, UiModal};
use crate::worldplay::{InteractionHint, PlayerNeeds, ScavengeState, WorldNotice};

const METER_COLORS: [Color; 4] = [
    Color::srgb(0.80, 0.24, 0.20),
    Color::srgb(0.79, 0.52, 0.17),
    Color::srgb(0.23, 0.54, 0.84),
    Color::srgb(0.56, 0.43, 0.82),
];

#[derive(Component)]
struct HudRoot;
#[derive(Component)]
struct HintRoot;
#[derive(Component)]
struct HintText;
#[derive(Component, Clone, Copy)]
enum HudLabel {
    Clock,
    Meter(usize),
    Gear,
    Utilities,
    Conditions,
}
#[derive(Component)]
struct MeterTrack(usize);
#[derive(Component)]
struct MeterFill(usize);

/// Shared geometry keeps the radio card below the instruments in portrait.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SurvivalHudLayout {
    pub card: ScreenRect,
    pub banner: ScreenRect,
    pub hint: ScreenRect,
}

pub(crate) fn survival_hud_layout(viewport: Vec2, conditions: bool) -> SurvivalHudLayout {
    let safe = safe_insets(viewport);
    let portrait = orientation(viewport) == Orientation::Portrait;
    let width = if portrait {
        (viewport.x - safe.left - safe.right).min(430.0)
    } else {
        (viewport.x * 0.36).clamp(270.0, 320.0)
    };
    let height = if conditions { 194.0 } else { 150.0 };
    let card = ScreenRect::new(safe.left, safe.top, width, height);
    let banner = if portrait {
        ScreenRect::new(card.min.x, card.max.y + 6.0, card.width(), 94.0)
    } else {
        let width = (viewport.x - card.max.x - safe.right - 16.0).clamp(200.0, 430.0);
        ScreenRect::new(
            viewport.x - safe.right - width,
            safe.top + 54.0,
            width,
            100.0,
        )
    };
    let controls = HudLayout::from_viewport(viewport);
    let controls_top = controls
        .sec
        .map_or(controls.joy.min.y, |sec| sec.min.y.min(controls.joy.min.y));
    let hint_w = if portrait {
        (viewport.x - safe.left - safe.right - 80.0).min(360.0)
    } else {
        (viewport.x - card.max.x - safe.right - 12.0).min(460.0)
    };
    let hint = ScreenRect::new(
        if portrait {
            safe.left
        } else {
            card.max.x + 12.0
        },
        controls_top - 48.0,
        hint_w,
        40.0,
    );
    SurvivalHudLayout { card, banner, hint }
}

pub(crate) fn survival_hud_layout_for_controls(
    viewport: Vec2,
    conditions: bool,
    button_percent: u8,
    driving: bool,
) -> SurvivalHudLayout {
    let mut layout = survival_hud_layout(viewport, conditions);
    let controls = HudLayout::for_controls(viewport, button_percent, driving);
    let controls_top = controls
        .sec
        .map_or(controls.joy.min.y, |sec| sec.min.y.min(controls.joy.min.y));
    layout.hint.min.y = controls_top - 48.0;
    layout.hint.max.y = controls_top - 8.0;
    if driving
        && orientation(viewport) == Orientation::Portrait
        && controls.joy.min.y - layout.banner.max.y < 70.0
    {
        // Very short phones retain a one-line report preview and the journal
        // link, leaving an independent mounted-control row above the joystick.
        layout.banner.max.y = layout.banner.min.y + 50.0;
    }
    layout
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HudSet;

#[derive(Resource, Default)]
pub(crate) struct HudGeometry {
    pub viewport: Vec2,
    pub button_percent: u8,
    /// Actual presented instruments, including only active condition rows.
    pub card: Option<ScreenRect>,
    /// The visible, presented radio card. Input consumes this exact rectangle.
    pub banner: Option<ScreenRect>,
}

pub struct HudPlugin;
impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudGeometry>()
            .add_systems(Startup, spawn_hud)
            .add_systems(
                PostUpdate,
                update_hud
                    .in_set(HudSet)
                    .before(crate::vehicles::ui::update)
                    .before(bevy::ui::UiSystems::Prepare),
            );
    }
}

fn text_node(label: HudLabel, size: f32) -> impl Bundle {
    (
        label,
        Node {
            position_type: PositionType::Absolute,
            overflow: Overflow::clip(),
            ..default()
        },
        Text::new(""),
        TextFont::from_font_size(FontSize::Px(size)),
        TextColor(Color::srgb(0.91, 0.89, 0.82)),
        TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
        Pickable::IGNORE,
    )
}

fn spawn_hud(mut commands: Commands) {
    commands
        .spawn((
            HudRoot,
            Visibility::Hidden,
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.047, 0.055, 0.063, 0.87)),
            BorderColor::all(Color::srgba(0.42, 0.39, 0.28, 0.85)),
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            parent.spawn(text_node(HudLabel::Clock, 14.0));
            for (index, meter_color) in METER_COLORS.iter().enumerate() {
                parent.spawn(text_node(HudLabel::Meter(index), 12.0));
                parent
                    .spawn((
                        MeterTrack(index),
                        Node {
                            position_type: PositionType::Absolute,
                            height: px(5),
                            border_radius: BorderRadius::all(px(2)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.14, 0.15, 0.15)),
                        Pickable::IGNORE,
                    ))
                    .with_child((
                        MeterFill(index),
                        Node {
                            width: percent(100),
                            height: percent(100),
                            ..default()
                        },
                        BackgroundColor(*meter_color),
                    ));
            }
            parent.spawn(text_node(HudLabel::Gear, 12.0));
            parent.spawn(text_node(HudLabel::Utilities, 11.0));
            parent.spawn(text_node(HudLabel::Conditions, 12.0));
        });
    commands
        .spawn((
            HintRoot,
            Visibility::Hidden,
            GlobalZIndex(11),
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(px(8), px(4)),
                border: UiRect::left(px(3)),
                border_radius: BorderRadius::all(px(4)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.035, 0.045, 0.05, 0.90)),
            BorderColor::all(Color::srgb(0.84, 0.68, 0.35)),
            Pickable::IGNORE,
        ))
        .with_child((
            HintText,
            Text::new(""),
            TextFont::from_font_size(FontSize::Px(12.0)),
            TextColor(Color::srgb(0.96, 0.88, 0.66)),
            TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
            Pickable::IGNORE,
        ));
}

fn meter_values(hp: f32, needs: &PlayerNeeds) -> [f32; 4] {
    [hp, needs.hunger, needs.thirst, 100.0 - needs.fatigue].map(|value| value.clamp(0.0, 100.0))
}

fn condition_text(
    game: &GameState,
    needs: &PlayerNeeds,
    campaign: &Campaign,
    stance: Option<&PlayerStance>,
    wet: f32,
    lang: Language,
) -> String {
    let mut labels = Vec::new();
    if game.bleed {
        labels.push(tr(lang, "流血", "Bleeding"));
    }
    if game.hp < 30.0 {
        labels.push(tr(lang, "重傷", "Severe injury"));
    }
    if needs.hunger < 25.0 {
        labels.push(tr(lang, "飢餓", "Hungry"));
    }
    if needs.thirst < 25.0 {
        labels.push(tr(lang, "口渴", "Thirsty"));
    }
    if needs.fatigue > 75.0 {
        labels.push(tr(lang, "疲憊", "Exhausted"));
    }
    if campaign.sleeping {
        labels.push(tr(lang, "睡眠中", "Sleeping"));
    }
    // Native WeatherState.wet is ground wetness, not the Web player's soak.
    // Do not label a sheltered survivor as soaked or cold from this value.
    if wet >= 0.25 {
        labels.push(tr(lang, "路面潮濕", "Wet ground"));
    }
    if stance.is_some_and(|s| s.cover) {
        labels.push(tr(lang, "掩體", "In cover"));
    } else if stance.is_some_and(|s| s.sneak) {
        labels.push(tr(lang, "潛行", "Sneaking"));
    }
    labels.join(" · ")
}

#[derive(Default)]
struct NoticeToast {
    last: String,
    remaining: f32,
}

#[derive(SystemParam)]
struct HudWorld<'w> {
    game: Res<'w, GameState>,
    campaign: Res<'w, Campaign>,
    shelter: Res<'w, Shelter>,
    needs: Res<'w, PlayerNeeds>,
    env: Res<'w, ShelterEnvironment>,
    inventory: Res<'w, Inventory>,
    weapon: Res<'w, PlayerWeaponState>,
    stance: Option<Res<'w, PlayerStance>>,
    scavenge: Option<Res<'w, ScavengeState>>,
    hint: Option<Res<'w, InteractionHint>>,
    notice: Option<Res<'w, WorldNotice>>,
    sectors: Option<Res<'w, crate::sectors::SectorStatus>>,
    lang: Res<'w, Language>,
    controls: Option<Res<'w, crate::input::ControlPreferences>>,
    fleet: Option<Res<'w, crate::vehicles::VehicleFleet>>,
}

fn update_hud(
    flow: Res<State<GameFlow>>,
    world: HudWorld,
    modal: Res<UiModal>,
    pause: Res<PauseOpen>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut roots: Query<
        (&mut Visibility, &mut Node),
        (
            With<HudRoot>,
            Without<HintRoot>,
            Without<HudLabel>,
            Without<MeterTrack>,
            Without<MeterFill>,
        ),
    >,
    mut hints: Query<
        (&mut Visibility, &mut Node),
        (
            With<HintRoot>,
            Without<HudRoot>,
            Without<HudLabel>,
            Without<MeterTrack>,
            Without<MeterFill>,
        ),
    >,
    mut labels: Query<
        (&HudLabel, &mut Text, &mut Node, &mut TextColor),
        (
            Without<HudRoot>,
            Without<HintRoot>,
            Without<MeterTrack>,
            Without<MeterFill>,
            Without<HintText>,
        ),
    >,
    mut tracks: Query<
        (&MeterTrack, &mut Node),
        (
            Without<HudRoot>,
            Without<HintRoot>,
            Without<HudLabel>,
            Without<MeterFill>,
        ),
    >,
    mut fills: Query<
        (&MeterFill, &mut Node),
        (
            Without<HudRoot>,
            Without<HintRoot>,
            Without<HudLabel>,
            Without<MeterTrack>,
        ),
    >,
    mut hint_text: Query<&mut Text, (With<HintText>, Without<HudLabel>)>,
    mut geometry: ResMut<HudGeometry>,
    time: Res<Time>,
    mut toast: Local<NoticeToast>,
) {
    let playing = flow.get() == &GameFlow::Playing && !modal.open && !pause.0;
    let viewport = windows.iter().next().map_or(Vec2::new(1280.0, 720.0), |w| {
        Vec2::new(w.width(), w.height())
    });
    let HudWorld {
        game,
        campaign,
        shelter,
        needs,
        env,
        inventory,
        weapon,
        stance,
        scavenge,
        hint,
        notice,
        sectors,
        lang,
        controls,
        fleet,
    } = world;
    let conditions = condition_text(
        &game,
        &needs,
        &campaign,
        stance.as_deref(),
        scavenge.as_ref().map_or(0.0, |s| s.weather.wet),
        *lang,
    );
    let button_percent = controls.as_ref().map_or(100, |prefs| prefs.button_percent);
    let layout = survival_hud_layout_for_controls(
        viewport,
        !conditions.is_empty(),
        button_percent,
        fleet.as_ref().is_some_and(|fleet| fleet.current.is_some()),
    );
    geometry.viewport = viewport;
    geometry.button_percent = button_percent;
    geometry.card = playing.then_some(layout.card);
    geometry.banner = (playing && campaign.notice_t > 0.0 && !campaign.notice.is_empty())
        .then_some(layout.banner);
    for (mut visible, mut node) in &mut roots {
        *visible = if playing {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        place(&mut node, layout.card);
    }
    let notice_text = notice.as_ref().map_or("", |notice| notice.text.as_str());
    if toast.last != notice_text {
        toast.last = notice_text.to_owned();
        toast.remaining = if notice_text.is_empty() { 0.0 } else { 4.5 };
    }
    if flow.get() != &GameFlow::Playing {
        toast.remaining = 0.0;
    }
    let vehicle_occluded = fleet
        .as_ref()
        .and_then(|fleet| fleet.current())
        .is_some_and(|vehicle| {
            crate::vehicles::ui::panel_layout(viewport, Some(&geometry), vehicle.spec().armored)
                .intersects(layout.hint)
        });
    let occluded =
        (campaign.notice_t > 0.0 && layout.banner.intersects(layout.hint)) || vehicle_occluded;
    if playing && !occluded {
        toast.remaining = (toast.remaining - time.delta_secs()).max(0.0);
    }
    let show_toast = toast.remaining > 0.0;
    let hint_available = hint.as_ref().is_some_and(|h| !h.label.is_empty());
    for (mut visible, mut node) in &mut hints {
        *visible = if playing && (hint_available || show_toast) && !occluded {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        place(&mut node, layout.hint);
    }
    if !playing {
        return;
    }
    let values = meter_values(game.hp, &needs);
    let width = layout.card.width() - 20.0;
    let cell_w = (width - 14.0) * 0.5;
    let minute = campaign.game_min.max(0.0) as u32;
    let night = is_night(f64::from(campaign.game_min));
    let curfew = minute / 60 % 24 >= 20 || minute / 60 % 24 < 6;
    let clock = format!(
        "{} {} · {:02}:{:02} · ×{}{}",
        tr(*lang, "第", "Day"),
        shelter.day,
        minute / 60 % 24,
        minute % 60,
        if campaign.sleeping {
            20
        } else {
            campaign.speed
        },
        if curfew {
            tr(*lang, " · 宵禁", " · Curfew")
        } else if night {
            tr(*lang, " · 夜間", " · Night")
        } else {
            ""
        }
    );
    let definition = item(&inventory.equipped);
    let weapon_name = definition.map_or(tr(*lang, "徒手", "Unarmed"), |d| d.name(*lang));
    let ammo = if definition.is_some_and(|d| d.ammo.is_some()) {
        format!(" · {} {}", weapon.rounds, tr(*lang, "發", "rds"))
    } else {
        String::new()
    };
    let gear = format!("{weapon_name}{ammo} · {:.1} kg", inventory.weight());
    let area = sectors.as_deref().map_or_else(
        || tr(*lang, "萬華", "Wanhua").to_owned(),
        |s| {
            let name = crate::sectors::SECTORS[s.here].name(*lang);
            if s.reclaimed_here {
                format!("{name}★")
            } else {
                format!("{name}{}", tr(*lang, "（佔領）", " (occupied)"))
            }
        },
    );
    let utilities = format!(
        "{} · {} · {} · {} {}",
        area,
        if env.power {
            tr(*lang, "有電", "Power")
        } else {
            tr(*lang, "停電", "Blackout")
        },
        if env.water {
            tr(*lang, "供水", "Water on")
        } else {
            tr(*lang, "停水", "Water cut")
        },
        if env.raining {
            tr(*lang, "雨", "Rain")
        } else {
            tr(*lang, "陰晴", "Dry")
        },
        if game.kills > 0 {
            format!("· {} {}", tr(*lang, "擊倒", "Kills"), game.kills)
        } else {
            String::new()
        }
    );
    for (label, mut text, mut node, mut color) in &mut labels {
        let (next, rect, tint) = match *label {
            HudLabel::Clock => (
                clock.clone(),
                ScreenRect::new(10.0, 8.0, width, 21.0),
                Color::srgb(0.96, 0.84, 0.55),
            ),
            HudLabel::Meter(index) => {
                let title = match index {
                    0 => tr(*lang, "生命", "Health"),
                    1 => tr(*lang, "飽足", "Fullness"),
                    2 => tr(*lang, "水分", "Hydration"),
                    _ => tr(*lang, "精神", "Energy"),
                };
                (
                    format!("{title}  {:.0}", values[index]),
                    ScreenRect::new(
                        10.0 + (index % 2) as f32 * (cell_w + 14.0),
                        34.0 + (index / 2) as f32 * 34.0,
                        cell_w,
                        18.0,
                    ),
                    Color::srgb(0.91, 0.89, 0.82),
                )
            }
            HudLabel::Gear => (
                gear.clone(),
                ScreenRect::new(10.0, 105.0, width, 18.0),
                Color::srgb(0.91, 0.89, 0.82),
            ),
            HudLabel::Utilities => (
                utilities.clone(),
                ScreenRect::new(10.0, 128.0, width, 16.0),
                Color::srgb(0.66, 0.70, 0.66),
            ),
            HudLabel::Conditions => (
                conditions.clone(),
                ScreenRect::new(10.0, 150.0, width, 40.0),
                Color::srgb(1.0, 0.61, 0.43),
            ),
        };
        if text.0 != next {
            text.0 = next;
        }
        place(&mut node, rect);
        color.0 = tint;
    }
    for (track, mut node) in &mut tracks {
        let i = track.0;
        place(
            &mut node,
            ScreenRect::new(
                10.0 + (i % 2) as f32 * (cell_w + 14.0),
                54.0 + (i / 2) as f32 * 34.0,
                cell_w,
                5.0,
            ),
        );
    }
    for (meter, mut node) in &mut fills {
        node.width = percent(values[meter.0]);
    }
    let hint_copy = if show_toast {
        toast.last.clone()
    } else {
        hint.map_or(String::new(), |hint| {
            format!("{}\n{}", hint.label, hint.detail)
        })
    };
    for mut text in &mut hint_text {
        if text.0 != hint_copy {
            text.0 = hint_copy.clone();
        }
    }
}

fn place(node: &mut Node, rect: ScreenRect) {
    node.left = px(rect.min.x);
    node.top = px(rect.min.y);
    node.width = px(rect.width());
    node.height = px(rect.height());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_is_inverse_fatigue_and_all_meters_are_clamped() {
        let values = meter_values(
            120.0,
            &PlayerNeeds {
                hunger: -4.0,
                thirst: 45.0,
                fatigue: 82.0,
            },
        );
        assert_eq!(values, [100.0, 0.0, 45.0, 18.0]);
    }

    #[test]
    fn warnings_match_live_thresholds_and_do_not_invent_conditions() {
        let mut game = GameState::default();
        let mut needs = PlayerNeeds {
            hunger: 25.0,
            thirst: 25.0,
            fatigue: 75.0,
        };
        assert_eq!(
            condition_text(&game, &needs, &Campaign::default(), None, 0.0, Language::En),
            ""
        );
        game.bleed = true;
        game.hp = 20.0;
        needs.thirst = 24.0;
        let text = condition_text(&game, &needs, &Campaign::default(), None, 0.3, Language::En);
        assert_eq!(text, "Bleeding · Severe injury · Thirsty · Wet ground");
    }

    #[test]
    fn hud_and_radio_fit_portrait_and_landscape_safe_areas() {
        for viewport in [
            Vec2::new(320.0, 568.0),
            Vec2::new(390.0, 844.0),
            Vec2::new(800.0, 360.0),
            Vec2::new(1280.0, 720.0),
        ] {
            for conditions in [false, true] {
                let layout = survival_hud_layout(viewport, conditions);
                for rect in [layout.card, layout.banner, layout.hint] {
                    assert!(
                        rect.min.x >= 0.0 && rect.min.y >= 0.0,
                        "{viewport:?} {rect:?}"
                    );
                    assert!(
                        rect.max.x <= viewport.x && rect.max.y <= viewport.y,
                        "{viewport:?} {rect:?}"
                    );
                }
                assert!(!layout.card.intersects(layout.banner));
                if viewport.y > viewport.x {
                    assert!(!layout.card.intersects(layout.hint));
                }
            }
        }
    }
    #[test]
    fn live_hud_hides_all_instruments_and_radio_hit_area_behind_modal_and_pause() {
        use bevy::state::app::StatesPlugin;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(GameFlow::Playing)
            .init_resource::<GameState>()
            .insert_resource(Campaign {
                notice: "Test report".to_owned(),
                notice_t: 7.0,
                ..default()
            })
            .init_resource::<Shelter>()
            .init_resource::<PlayerNeeds>()
            .init_resource::<ShelterEnvironment>()
            .init_resource::<Inventory>()
            .init_resource::<PlayerWeaponState>()
            .init_resource::<Language>()
            .init_resource::<UiModal>()
            .init_resource::<PauseOpen>()
            .insert_resource(InteractionHint {
                label: "Open door".to_owned(),
                detail: "Entrance".to_owned(),
            })
            .add_plugins(HudPlugin);
        app.world_mut().spawn((
            Window {
                resolution: (390, 844).into(),
                ..default()
            },
            PrimaryWindow,
        ));
        app.update();
        let visibility = |app: &mut App| {
            let world = app.world_mut();
            let mut query =
                world.query_filtered::<&Visibility, Or<(With<HudRoot>, With<HintRoot>)>>();
            query.iter(world).copied().collect::<Vec<_>>()
        };
        assert_eq!(
            visibility(&mut app),
            vec![Visibility::Inherited, Visibility::Inherited]
        );
        assert!(app.world().resource::<HudGeometry>().banner.is_some());
        app.world_mut().resource_mut::<UiModal>().open = true;
        app.update();
        assert!(
            visibility(&mut app)
                .iter()
                .all(|v| *v == Visibility::Hidden)
        );
        assert!(app.world().resource::<HudGeometry>().banner.is_none());
        app.world_mut().resource_mut::<UiModal>().open = false;
        app.world_mut().resource_mut::<PauseOpen>().0 = true;
        app.update();
        assert!(
            visibility(&mut app)
                .iter()
                .all(|v| *v == Visibility::Hidden)
        );
        assert!(app.world().resource::<HudGeometry>().banner.is_none());

        // Reproduce the compact warning/radio scene that exposed a midpoint
        // toast covering the report. Feedback must occupy the context slot.
        app.world_mut().resource_mut::<PauseOpen>().0 = false;
        app.world_mut().resource_mut::<GameState>().bleed = true;
        app.world_mut().insert_resource(WorldNotice {
            text: "Door closed".to_owned(),
        });
        let mut query = app
            .world_mut()
            .query_filtered::<&mut Window, With<PrimaryWindow>>();
        query
            .single_mut(app.world_mut())
            .unwrap()
            .resolution
            .set_physical_resolution(320, 640);
        app.update();
        let banner = app.world().resource::<HudGeometry>().banner.unwrap();
        let mut query = app.world_mut().query_filtered::<&Node, With<HintRoot>>();
        let node = query.single(app.world()).unwrap();
        let Val::Px(x) = node.left else {
            panic!("hint x must be absolute")
        };
        let Val::Px(y) = node.top else {
            panic!("hint y must be absolute")
        };
        let Val::Px(w) = node.width else {
            panic!("hint width must be absolute")
        };
        let Val::Px(h) = node.height else {
            panic!("hint height must be absolute")
        };
        assert!(!banner.intersects(ScreenRect::new(x, y, w, h)));
        let mut query = app.world_mut().query_filtered::<&Text, With<HintText>>();
        assert_eq!(query.single(app.world()).unwrap().0, "Door closed");

        // The shortest supported phone keeps the report/journal target and
        // mounted buttons distinct; its contextual toast yields that slot.
        app.insert_resource(crate::input::ControlPreferences {
            button_percent: 110,
            ..default()
        });
        let vehicle = crate::vehicles::VehicleState::new(
            9,
            crate::vehicles::VehicleKind::Zbl08,
            Vec2::ZERO,
            0.0,
            &mut || 0.5,
        );
        app.insert_resource(crate::vehicles::VehicleFleet {
            vehicles: vec![vehicle],
            current: Some(9),
        });
        let mut query = app
            .world_mut()
            .query_filtered::<&mut Window, With<PrimaryWindow>>();
        query
            .single_mut(app.world_mut())
            .unwrap()
            .resolution
            .set_physical_resolution(320, 568);
        app.update();
        assert_eq!(
            app.world()
                .resource::<HudGeometry>()
                .banner
                .unwrap()
                .height(),
            50.0
        );
        let mut query = app
            .world_mut()
            .query_filtered::<&Visibility, With<HintRoot>>();
        assert_eq!(*query.single(app.world()).unwrap(), Visibility::Hidden);
    }
}
