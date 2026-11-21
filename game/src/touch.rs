//! Touch controls: a floating virtual joystick on the left side of the screen
//! (works in landscape and portrait) and two-finger pinch zoom.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::camera::CameraZoom;
use crate::player::MoveInput;

/// Joystick travel radius in logical pixels.
const RADIUS: f32 = 60.0;
const BASE: f32 = 132.0;
const KNOB: f32 = 58.0;
/// Below this deflection the stick is ignored (matches the web build's 0.12 dead zone).
const DEAD_ZONE: f32 = 0.12;

#[derive(Resource, Default)]
struct Stick {
    touch: Option<u64>,
    origin: Vec2,
    pinch: Option<f32>,
}

#[derive(Component)]
struct StickBase;

#[derive(Component)]
struct StickKnob;

pub struct TouchPlugin;

impl Plugin for TouchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stick>()
            .add_systems(Startup, spawn_stick_ui)
            .add_systems(
                Update,
                (touch_stick, pinch_zoom).before(crate::player::move_system_set()),
            );
    }
}

fn spawn_stick_ui(mut commands: Commands) {
    commands
        .spawn((
            StickBase,
            Node {
                position_type: PositionType::Absolute,
                width: px(BASE),
                height: px(BASE),
                border: UiRect::all(px(3)),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.5)),
            Visibility::Hidden,
            GlobalZIndex(10),
        ))
        .with_child((
            StickKnob,
            Node {
                width: px(KNOB),
                height: px(KNOB),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
        ));
}

fn touch_stick(
    touches: Res<Touches>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut stick: ResMut<Stick>,
    mut input: ResMut<MoveInput>,
    mut base: Single<(&mut Node, &mut Visibility), (With<StickBase>, Without<StickKnob>)>,
    mut knob: Single<&mut UiTransform, With<StickKnob>>,
) {
    // Release
    if let Some(id) = stick.touch
        && touches.get_pressed(id).is_none()
    {
        stick.touch = None;
    }
    // Grab: first new touch on the left 55% of the screen becomes the stick
    if stick.touch.is_none() {
        let limit = window.width() * 0.55;
        if let Some(t) = touches.iter_just_pressed().find(|t| t.position().x < limit) {
            stick.touch = Some(t.id());
            stick.origin = t.position();
        }
    }

    let (node, vis) = &mut *base;
    match stick.touch.and_then(|id| touches.get_pressed(id)) {
        Some(t) => {
            let mut d = t.position() - stick.origin;
            if d.length() > RADIUS {
                d = d.normalize() * RADIUS;
            }
            let v = d / RADIUS;
            // Screen y grows downwards; MoveInput y is "up".
            input.stick = if v.length() > DEAD_ZONE {
                Vec2::new(v.x, -v.y)
            } else {
                Vec2::ZERO
            };
            node.left = px(stick.origin.x - BASE * 0.5);
            node.top = px(stick.origin.y - BASE * 0.5);
            **vis = Visibility::Inherited;
            knob.translation = Val2::px(d.x, d.y);
        }
        None => {
            input.stick = Vec2::ZERO;
            **vis = Visibility::Hidden;
            knob.translation = Val2::px(0.0, 0.0);
        }
    }
}

/// Two fingers that are not the joystick pinch to zoom.
fn pinch_zoom(touches: Res<Touches>, mut stick: ResMut<Stick>, mut zoom: ResMut<CameraZoom>) {
    let stick_id = stick.touch;
    let mut free = touches.iter().filter(|t| Some(t.id()) != stick_id);
    let (Some(a), Some(b)) = (free.next(), free.next()) else {
        stick.pinch = None;
        return;
    };
    let dist = a.position().distance(b.position());
    if let Some(prev) = stick.pinch
        && prev > 1.0
    {
        zoom.0 = (zoom.0 * dist / prev).clamp(0.5, 2.2);
    }
    stick.pinch = Some(dist);
}
