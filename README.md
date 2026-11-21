# DefendTaipei-Android

Native Android version of [Defend Taipei](https://github.com/lijiabao1998/defendtaipei), a Taipei urban survival game.

The original game is a TypeScript + Three.js web title (currently v24.42). It is set across a map of Taipei's 12 districts and includes AI teammates.

This repository is a planned native Android port and rewrite. Nothing is playable yet.

## Why native

The web build runs in a mobile browser, which adds a JavaScript, WebGL, and browser layer between the game and the device. A native Android build can talk more directly to the CPU, GPU, and memory, which should improve performance, battery use, and heat on phones—especially on a large map like Taipei's 12 districts.

## Planned stack

- **Engine:** Godot 4, with gameplay written in GDScript
- **Hot paths (if needed):** Rust GDExtension for work such as pathfinding and large unit counts
- **Android export:** Godot's Android export via NDK and Vulkan
- **Builds:** APK produced by GitHub Actions

## Status

Planning. This repository has no Godot project, no playable build, and no CI yet.
