//! Chunk / LOD / tile streaming.
//!
//! Faithful port of the web game's `src/core/chunk.ts`, `src/core/compact.ts`,
//! and the 256 m streaming core in `src/map/tiles/` (`grid.ts`, `lod.ts`,
//! `manager.ts`, `mid.ts`).
//!
//! Streaming is **off** until [`ChunkConfig::enabled`] is set, or
//! `DEFEND_MAPTILES=1`. The colored debug tiles are a separate switch
//! (`--chunk-debug` / `DEFEND_CHUNK_DEBUG=1`) and are not part of a normal launch.
//!
//! # Map provider
//!
//! ```ignore
//! use defend_taipei::chunk::{
//!     ChunkAppExt, ChunkConfig, CompactMaterial, Lod, TileCoord, TileCtx, TileHandle,
//!     TileLoadError, TileProvider,
//! };
//!
//! struct WanhuaTiles;
//!
//! impl TileProvider for WanhuaTiles {
//!     fn has(&self, _coord: TileCoord) -> bool { true }
//!
//!     fn load(
//!         &mut self,
//!         coord: TileCoord,
//!         lod: Lod,
//!         ctx: &mut TileCtx,
//!     ) -> Result<TileHandle, TileLoadError> {
//!         let _ = (coord, lod);
//!         let mesh = ctx.add_mesh(bevy::prelude::Mesh::from(bevy::prelude::Cuboid::default()), &CompactMaterial::default());
//!         let root = ctx.commands.spawn(bevy::prelude::Mesh3d(mesh)).id();
//!         Ok(TileHandle::entity(root))
//!     }
//!
//!     fn unload(&mut self, _coord: TileCoord, handle: TileHandle, ctx: &mut TileCtx) {
//!         if let Some(root) = handle.root {
//!             ctx.commands.entity(root).despawn();
//!         }
//!     }
//! }
//!
//! fn install(app: &mut bevy::prelude::App) {
//!     app.register_tile_provider(WanhuaTiles)
//!         .insert_resource(ChunkConfig {
//!             enabled: true,
//!             ..ChunkConfig::original()
//!         });
//! }
//! ```
//!
//! Distances and the per-frame budget live on [`ChunkConfig`] (near 500 m, mid
//! 2 km, drop 4 km, hysteresis 32 m, budget 4). The focus is the player unless
//! [`StreamFocus`] is set.
//!
//! To spawn from a system that needs queries, leave the provider empty, enable
//! streaming, and use [`TileMailbox`]: drain `requests`, push `finished` with
//! the same `generation`, and drain `unloads`. Order that system after
//! [`ChunkSet`]. [`TileLoaded`] / [`TileUnloaded`] are the bus events.
//!
//! # Mesh helpers the map calls itself
//!
//! The streamer does not walk the scene.
//!
//! - [`chunkify_mesh`] / [`chunkify_instances`] — `chunk.ts`. 60 m cells, only
//!   meshes of at least 12 000 triangles spanning 100 m.
//! - [`compact_mesh`] — `compact.ts`. Uv dropped when unused, normals packed to
//!   `Snorm8x4`, colours to `Unorm16x4`. `DEFEND_NOCOMPACT=1` and
//!   `DEFEND_NORELEASE=1` match `?nocompact=1` and `?norelease=1`.
//!
//! `src/map/tiles/legacy.ts`, `transaction.ts`, and `nav-ownership.ts` are the
//! inactive ownership experiments. They are not this runtime, and collision
//! ownership stays with the map.

mod chunkify;
mod compact;
mod config;
mod debug;
mod grid;
mod lod;
mod manager;
mod mid;
mod provider;

use bevy::prelude::*;

pub use chunkify::{
    ChunkOptions, ChunkStats, ChunkedMesh, InstanceCloud, InstancePiece, chunkify_instances,
    chunkify_mesh,
};
pub use compact::{
    CompactMaterial, CompactOptions, CompactOutcome, CompactSkip, CompactStats, MIN_VERTS,
    ReleaseStats, SMALL_BYTES, compact_mesh, discard_mesh, free_now,
};
pub use config::{ChunkConfig, DEFAULT_BUDGET, debug_requested, requested_resolution};
pub use grid::{
    TILE, TileBounds, TileCoord, key_of, parse_key, tile_bounds, tile_center, tile_dist, tile_of,
};
pub use lod::{LOD_DROP, LOD_HYST, LOD_MID, LOD_NEAR, Lod, LodThresholds, lod_for, lod_for_with};
pub use manager::{LoadResult, TileEntry, TileEvent, TileHooks, TileManager};
pub use mid::{Footprint, far_terrain, far_terrain_sampled, mid_buildings};
pub use provider::{
    ChunkAppExt, ChunkFrameStats, StreamFocus, StreamState, TileCtx, TileHandle, TileLoadError,
    TileLoadFinished, TileLoadRequest, TileLoaded, TileMailbox, TileProvider, TileProviderSlot,
    TileUnloadRequest, TileUnloaded,
};

/// Systems that stream tiles. Map systems that read [`TileMailbox`] should run
/// after this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkSet;

pub struct ChunkPlugin;

impl Plugin for ChunkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChunkConfig>()
            .init_resource::<TileProviderSlot>()
            .init_resource::<StreamState>()
            .init_resource::<ChunkFrameStats>()
            .init_resource::<CompactStats>()
            .init_resource::<ReleaseStats>()
            .init_resource::<StreamFocus>()
            .init_resource::<TileMailbox>()
            .add_message::<TileLoaded>()
            .add_message::<TileUnloaded>()
            .configure_sets(Update, ChunkSet.after(crate::player::MoveSet))
            .add_systems(PreStartup, provider::apply_env_config)
            .add_systems(Startup, debug::setup_debug_view)
            .add_systems(
                Update,
                (
                    provider::stream_tiles,
                    debug::suppress_game_camera,
                    debug::fit_debug_camera,
                    debug::update_legend,
                    debug::capture_debug_shot,
                )
                    .chain()
                    .in_set(ChunkSet),
            );
    }
}
