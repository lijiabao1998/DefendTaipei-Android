//! Three LOD levels for streamed tiles. Port of `src/map/tiles/lod.ts`.
//!
//! - near (~0–500 m): full buildings and roads
//! - mid (~500 m–2 km): extruded boxes and a merged road mesh
//! - far (~2 km–4 km): one low-poly terrain mesh per tile
//! - beyond 4 km: the tile is dropped
//!
//! A tile that already has a level keeps it for an extra [`LOD_HYST`] metres so
//! a point sitting on a boundary does not flip every frame.

/// `Lod`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lod {
    Near,
    Mid,
    Far,
}

impl Lod {
    /// Bus payload `tileLoaded.lod`: 0 near / 1 mid / 2 far. Original `LOD_NUM`.
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Near => 0,
            Self::Mid => 1,
            Self::Far => 2,
        }
    }
}

/// `LOD_NEAR`, metres.
pub const LOD_NEAR: f32 = 500.0;
/// `LOD_MID`, metres.
pub const LOD_MID: f32 = 2000.0;
/// `LOD_DROP`, metres. Tiles farther than this are not loaded.
pub const LOD_DROP: f32 = 4000.0;
/// `LOD_HYST`, metres.
pub const LOD_HYST: f32 = 32.0;

/// Distances used by [`lod_for_with`]. Defaults are the web constants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodThresholds {
    pub near: f32,
    pub mid: f32,
    pub drop: f32,
    pub hyst: f32,
}

impl LodThresholds {
    pub const ORIGINAL: Self = Self {
        near: LOD_NEAR,
        mid: LOD_MID,
        drop: LOD_DROP,
        hyst: LOD_HYST,
    };
}

impl Default for LodThresholds {
    fn default() -> Self {
        Self::ORIGINAL
    }
}

/// `lodFor(dist, prev)` with the original distances.
pub fn lod_for(dist: f32, prev: Option<Lod>) -> Option<Lod> {
    lod_for_with(dist, prev, LodThresholds::ORIGINAL)
}

/// `lodFor` with configurable distances. `None` means the tile is past the drop
/// radius. Passing `prev` widens the band the tile is already in by `hyst`.
pub fn lod_for_with(dist: f32, prev: Option<Lod>, t: LodThresholds) -> Option<Lod> {
    let h = if prev.is_some() { t.hyst } else { 0.0 };
    let near = if prev == Some(Lod::Near) {
        t.near + h
    } else {
        t.near - h
    };
    let mid = if prev == Some(Lod::Mid) || prev == Some(Lod::Near) {
        t.mid + h
    } else {
        t.mid - h
    };
    if dist <= near {
        Some(Lod::Near)
    } else if dist <= mid {
        Some(Lod::Mid)
    } else if dist <= t.drop + h {
        Some(Lod::Far)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_lod_thresholds() {
        assert_eq!(lod_for(500.0, None), Some(Lod::Near));
        assert_eq!(lod_for(501.0, None), Some(Lod::Mid));
        assert_eq!(lod_for(2001.0, None), Some(Lod::Far));
    }

    #[test]
    fn near_retains_hysteresis_outward() {
        assert_eq!(lod_for(532.0, Some(Lod::Near)), Some(Lod::Near));
        assert_eq!(lod_for(533.0, Some(Lod::Near)), Some(Lod::Mid));
    }

    #[test]
    fn mid_retains_hysteresis_inward() {
        assert_eq!(lod_for(469.0, Some(Lod::Mid)), Some(Lod::Mid));
        assert_eq!(lod_for(468.0, Some(Lod::Mid)), Some(Lod::Near));
    }

    #[test]
    fn mid_retains_hysteresis_outward() {
        assert_eq!(lod_for(2032.0, Some(Lod::Mid)), Some(Lod::Mid));
        assert_eq!(lod_for(2033.0, Some(Lod::Mid)), Some(Lod::Far));
    }

    #[test]
    fn far_retains_hysteresis_inward() {
        assert_eq!(lod_for(1969.0, Some(Lod::Far)), Some(Lod::Far));
        assert_eq!(lod_for(1968.0, Some(Lod::Far)), Some(Lod::Mid));
    }

    #[test]
    fn drop_boundary_honors_hysteresis() {
        assert_eq!(lod_for(4000.0, None), Some(Lod::Far));
        assert_eq!(lod_for(4001.0, None), None);
        assert_eq!(lod_for(4032.0, Some(Lod::Far)), Some(Lod::Far));
        assert_eq!(lod_for(4033.0, Some(Lod::Far)), None);
    }

    #[test]
    fn configured_distances_replace_the_defaults() {
        let t = LodThresholds {
            near: 10.0,
            mid: 20.0,
            drop: 30.0,
            hyst: 1.0,
        };
        assert_eq!(lod_for_with(10.0, None, t), Some(Lod::Near));
        assert_eq!(lod_for_with(11.0, None, t), Some(Lod::Mid));
        assert_eq!(lod_for_with(11.0, Some(Lod::Near), t), Some(Lod::Near));
        assert_eq!(lod_for_with(31.0, Some(Lod::Far), t), Some(Lod::Far));
        assert_eq!(lod_for_with(32.0, Some(Lod::Far), t), None);
    }
}
