# DefendTaipei-Android

Native Android version of [Defend Taipei](https://github.com/lijiabao1998/defendtaipei), a Taipei urban survival game.

The original game is a TypeScript + Three.js web title (currently v24.42). It is set across a map of Taipei's 12 districts and includes AI teammates.

This repository is a planned native Android port and rewrite. Nothing is playable yet.

## Why native

The web build runs in a mobile browser, which adds a JavaScript, WebGL, and browser layer between the game and the device. A native Android build can talk more directly to the CPU, GPU, and memory, which should improve performance, battery use, and heat on phones—especially on a large map like Taipei's 12 districts.

## Planned stack

- **Engine:** [Bevy](https://bevy.org) 0.20.1 (latest stable on [crates.io](https://crates.io/crates/bevy) as of 2026-10-28), written in Rust and using Bevy's ECS (entity-component-system) architecture.
- **Rendering:** [wgpu](https://wgpu.rs) on Vulkan.
- **Android build:** Follow Bevy 0.20.1's official Android path: compile the Rust library with [`cargo-ndk`](https://crates.io/crates/cargo-ndk) for the Android NDK target (for example `cargo ndk -t arm64-v8a -P 26 -o <app>/src/main/jniLibs build`), then package the APK with a Gradle / Android Studio project (`./gradlew build`). Bevy's default Android activity is GameActivity. [`cargo-apk` is deprecated and does not support GameActivity](https://github.com/bevyengine/bevy/blob/v0.20.1/examples/README.md#about-cargo-apk); `xbuild` is not a supported Bevy Android path. See Bevy's [Android example docs](https://github.com/bevyengine/bevy/blob/v0.20.1/examples/README.md#android).
- **Desktop:** The same codebase should also build for Windows, Linux, and macOS, for development and testing.
- **Orientation:** Both portrait and landscape. Landscape is the primary layout for gameplay; portrait is for the map and menus. The game should handle window resize and orientation changes.
- **CI:** APKs built by GitHub Actions.

## Status

Planning. This repository has no Bevy project, no playable build, and no CI yet.
