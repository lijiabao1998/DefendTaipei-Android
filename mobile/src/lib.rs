//! Android entry point. `#[bevy_main]` generates the `android_main` symbol that
//! GameActivity calls after `System.loadLibrary("defend_taipei_android")`.

use bevy::prelude::*;

#[bevy_main]
fn main() {
    defend_taipei::run();
}
