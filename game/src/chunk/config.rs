//! Off switch and tunable distances. Defaults match the web constants.
//!
//! Streaming stays off until a map provider (or `DEFEND_MAPTILES=1`) enables it,
//! same as `MAP_TILES_ON` (`?maptiles=1`). Colored debug tiles stay off unless
//! `--chunk-debug` / `DEFEND_CHUNK_DEBUG=1` is set, so a normal launch draws no
//! placeholder geometry.

use bevy::prelude::*;

use super::lod::{LOD_DROP, LOD_HYST, LOD_MID, LOD_NEAR, LodThresholds};

/// How many tile loads, including LOD changes, may start in one update.
/// Original `TileManager.budget`.
pub const DEFAULT_BUDGET: u32 = 4;

/// Runtime switches. Environment and argv are applied in `PreStartup`.
///
/// | Web flag | Here |
/// | --- | --- |
/// | `?maptiles=1` | `DEFEND_MAPTILES=1` |
/// | debug view | `--chunk-debug` or `DEFEND_CHUNK_DEBUG=1` |
/// | `?nocompact=1` | `DEFEND_NOCOMPACT=1` |
/// | `?norelease=1` | `DEFEND_NORELEASE=1` |
///
/// `DEFEND_CHUNK_BUDGET`, `DEFEND_LOD_NEAR`, `DEFEND_LOD_MID`, `DEFEND_LOD_DROP`
/// and `DEFEND_LOD_HYST` override the numbers. `DEFEND_CHUNK_SHOT=/path.png`
/// saves one screenshot of the debug view once the radius is resident and then
/// quits. `DEFEND_WINDOW=720x1280` or `--window 720x1280` resizes the desktop
/// window. Both are capture harnesses, not gameplay.
#[derive(Resource, Clone, Debug)]
pub struct ChunkConfig {
    /// Master switch. When false the streamer does no work and spawns nothing.
    pub enabled: bool,
    /// Colored LOD quads and the overview camera. Off in a normal launch.
    pub debug_view: bool,
    pub budget: u32,
    pub near: f32,
    pub mid: f32,
    pub drop: f32,
    pub hyst: f32,
    /// Pack vertex attributes after a tile mesh is built. Original `COMPACT.on`.
    pub compact: bool,
    /// Drop the CPU copy after GPU upload. Original `RELEASE.on`.
    pub release_cpu: bool,
    /// Set only by `DEFEND_CHUNK_SHOT`.
    pub shot_path: Option<String>,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            debug_view: false,
            budget: DEFAULT_BUDGET,
            near: LOD_NEAR,
            mid: LOD_MID,
            drop: LOD_DROP,
            hyst: LOD_HYST,
            compact: true,
            release_cpu: true,
            shot_path: None,
        }
    }
}

impl ChunkConfig {
    pub fn thresholds(&self) -> LodThresholds {
        LodThresholds {
            near: self.near,
            mid: self.mid,
            drop: self.drop,
            hyst: self.hyst,
        }
    }

    /// Original constants, streaming off, debug view off.
    pub fn original() -> Self {
        Self::default()
    }

    /// Layer environment and argv over [`Self::original`].
    pub fn from_env() -> Self {
        let debug = debug_requested();
        let mut cfg = Self::original();
        cfg.debug_view = debug;
        cfg.enabled = debug || env_flag("DEFEND_MAPTILES");
        if let Some(v) = env_u32("DEFEND_CHUNK_BUDGET") {
            cfg.budget = v;
        }
        if let Some(v) = env_f32("DEFEND_LOD_NEAR") {
            cfg.near = v;
        }
        if let Some(v) = env_f32("DEFEND_LOD_MID") {
            cfg.mid = v;
        }
        if let Some(v) = env_f32("DEFEND_LOD_DROP") {
            cfg.drop = v;
        }
        if let Some(v) = env_f32("DEFEND_LOD_HYST") {
            cfg.hyst = v;
        }
        if env_flag("DEFEND_NOCOMPACT") {
            cfg.compact = false;
        }
        if env_flag("DEFEND_NORELEASE") {
            cfg.release_cpu = false;
        }
        if let Ok(path) = std::env::var("DEFEND_CHUNK_SHOT")
            && !path.is_empty()
        {
            cfg.shot_path = Some(path);
            cfg.debug_view = true;
            cfg.enabled = true;
        }
        cfg
    }
}

/// `--chunk-debug` or `DEFEND_CHUNK_DEBUG=1`.
pub fn debug_requested() -> bool {
    env_flag("DEFEND_CHUNK_DEBUG") || std::env::args().any(|arg| arg == "--chunk-debug")
}

/// `DEFEND_WINDOW=720x1280` or `--window 720x1280`. `None` keeps the default window.
pub fn requested_resolution() -> Option<(u32, u32)> {
    if let Ok(raw) = std::env::var("DEFEND_WINDOW")
        && let Some(size) = parse_resolution(&raw)
    {
        return Some(size);
    }
    let args: Vec<String> = std::env::args().collect();
    for pair in args.windows(2) {
        if pair[0] == "--window"
            && let Some(size) = parse_resolution(&pair[1])
        {
            return Some(size);
        }
    }
    None
}

fn parse_resolution(raw: &str) -> Option<(u32, u32)> {
    let (w, h) = raw.split_once(['x', 'X'])?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

fn env_u32(name: &str) -> Option<u32> {
    std::env::var(name).ok()?.parse().ok()
}

fn env_f32(name: &str) -> Option<f32> {
    let value: f32 = std::env::var(name).ok()?.parse().ok()?;
    value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_web_game_and_stay_off() {
        let cfg = ChunkConfig::original();
        assert!(!cfg.enabled);
        assert!(!cfg.debug_view);
        assert_eq!(cfg.budget, 4);
        assert_eq!(cfg.thresholds(), LodThresholds::ORIGINAL);
        assert!(cfg.compact && cfg.release_cpu);
    }

    #[test]
    fn resolution_parser_accepts_portrait_and_landscape() {
        assert_eq!(parse_resolution("1280x720"), Some((1280, 720)));
        assert_eq!(parse_resolution("720X1280"), Some((720, 1280)));
        assert_eq!(parse_resolution("0x10"), None);
        assert_eq!(parse_resolution("nope"), None);
    }
}
