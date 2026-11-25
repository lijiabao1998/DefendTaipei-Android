//! Pinch-to-zoom. Virtual sticks live in [`crate::input`]; this only skips the
//! touch ids those sticks have already claimed.

use bevy::prelude::*;

use crate::camera::CameraZoom;
use crate::input::{InputSet, TouchControl};

#[derive(Resource, Default)]
struct Pinch {
    previous: Option<f32>,
}

pub struct TouchPlugin;

impl Plugin for TouchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pinch>()
            .add_systems(Update, pinch_zoom.after(InputSet));
    }
}

/// Two fingers that are not on a stick or button pinch to zoom.
fn pinch_zoom(
    touches: Res<Touches>,
    control: Res<TouchControl>,
    mut pinch: ResMut<Pinch>,
    mut zoom: ResMut<CameraZoom>,
) {
    let mut free = touches.iter().filter(|touch| !control.claims(touch.id()));
    let (Some(a), Some(b)) = (free.next(), free.next()) else {
        pinch.previous = None;
        return;
    };
    let distance = a.position().distance(b.position());
    if let Some(previous) = pinch.previous
        && previous > 1.0
    {
        zoom.0 = (zoom.0 * distance / previous).clamp(0.5, 2.2);
    }
    pinch.previous = Some(distance);
}
