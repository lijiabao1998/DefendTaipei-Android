//! 256 m × 256 m tile grid. Port of `src/map/tiles/grid.ts`.
//!
//! World units are metres on the x/z plane. Indices use `floor`, so a negative
//! coordinate and an exact positive boundary belong to the next tile toward +∞.
//! The string key is `"tx,tz"`, the web `TileKey`.

/// Tile edge length in metres. Original `TILE`.
pub const TILE: f32 = 256.0;

/// Integer tile coordinate. The web code stores this as `"tx,tz"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileCoord {
    pub tx: i32,
    pub tz: i32,
}

impl TileCoord {
    pub const fn new(tx: i32, tz: i32) -> Self {
        Self { tx, tz }
    }
}

impl std::fmt::Display for TileCoord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{},{}", self.tx, self.tz)
    }
}

/// `tileOf`.
pub fn tile_of(x: f32, z: f32) -> TileCoord {
    TileCoord::new((x / TILE).floor() as i32, (z / TILE).floor() as i32)
}

/// `keyOf`.
pub fn key_of(coord: TileCoord) -> String {
    coord.to_string()
}

/// `parseKey`. `None` when the string is not two integers.
pub fn parse_key(key: &str) -> Option<TileCoord> {
    let (a, b) = key.split_once(',')?;
    Some(TileCoord::new(a.parse().ok()?, b.parse().ok()?))
}

/// Half-open world bounds: `[min, max)` on x and z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileBounds {
    pub min_x: f32,
    pub min_z: f32,
    pub max_x: f32,
    pub max_z: f32,
}

/// `tileBounds`.
pub fn tile_bounds(coord: TileCoord) -> TileBounds {
    TileBounds {
        min_x: coord.tx as f32 * TILE,
        min_z: coord.tz as f32 * TILE,
        max_x: (coord.tx + 1) as f32 * TILE,
        max_z: (coord.tz + 1) as f32 * TILE,
    }
}

/// Centre of the tile on the x/z plane.
pub fn tile_center(coord: TileCoord) -> [f32; 2] {
    let b = tile_bounds(coord);
    [(b.min_x + b.max_x) * 0.5, (b.min_z + b.max_z) * 0.5]
}

/// `tileDist`. Metres from `(x, z)` to the nearest point of the tile, or 0
/// inside. The max edge counts as inside, matching
/// `Math.max(min - p, 0, p - max)`.
pub fn tile_dist(coord: TileCoord, x: f32, z: f32) -> f32 {
    let b = tile_bounds(coord);
    let dx = (b.min_x - x).max(0.0).max(x - b.max_x);
    let dz = (b.min_z - z).max(0.0).max(z - b.max_z);
    dx.hypot(dz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_coordinates_use_floor() {
        assert_eq!(tile_of(-0.01, -256.0), TileCoord::new(-1, -1));
    }

    #[test]
    fn exact_positive_boundary_enters_next_tile() {
        assert_eq!(tile_of(256.0, 0.0), TileCoord::new(1, 0));
    }

    #[test]
    fn tile_keys_round_trip_signed_coordinates() {
        let coord = TileCoord::new(-2, 3);
        assert_eq!(parse_key(&key_of(coord)), Some(coord));
        assert_eq!(key_of(coord), "-2,3");
    }

    #[test]
    fn signed_tile_bounds() {
        let b = tile_bounds(TileCoord::new(-1, 0));
        assert_eq!(
            (b.min_x, b.max_x, b.min_z, b.max_z),
            (-256.0, 0.0, 0.0, 256.0)
        );
    }

    #[test]
    fn distance_uses_nearest_tile_edge() {
        let origin = TileCoord::new(0, 0);
        assert_eq!(tile_dist(origin, 256.0, 100.0), 0.0);
        assert_eq!(tile_dist(origin, 259.0, 260.0), 5.0);
    }
}
