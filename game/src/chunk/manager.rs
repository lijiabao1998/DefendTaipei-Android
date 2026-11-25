//! Tile manager. Port of `src/map/tiles/manager.ts`.
//!
//! Each update loads tiles around a focus point at the LOD [`lod_for_with`]
//! chooses, nearest first, and unloads the rest. At most `budget` loads start
//! per call (the web default is 4) so a phone frame is not spent building the
//! whole radius. A load may finish later: the previous LOD stays visible until
//! the new one resolves, and a stale or failed load cannot leak the object it
//! built or emit `tileUnloaded` for a tile that never finished loading.
//!
//! `generation` is the web `gen`. Rust 2024 reserves `gen` as a keyword.

use std::collections::{BTreeMap, HashSet};

use super::config::DEFAULT_BUDGET;
use super::grid::{TILE, TileCoord, tile_dist, tile_of};
use super::lod::{Lod, LodThresholds, lod_for_with};

/// What [`TileManager::update`] asked the hooks to build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadResult<T> {
    /// Finished during this call. Original synchronous `load` return.
    Ready(T),
    /// Started, not finished. Call [`TileManager::resolve`] with the generation
    /// passed into [`TileHooks::load`]. Original `Promise`.
    Pending,
    /// Synchronous failure. Original `throw` inside `load`.
    Failed,
}

/// `TileHooks`. The streamer does not know what a tile contains.
pub trait TileHooks<T> {
    /// `has`. False skips the cell (and drops it if it was loaded).
    fn has(&self, _tx: i32, _tz: i32) -> bool {
        true
    }
    /// `load`. `generation` identifies this attempt so a late finish can be ignored.
    fn load(&mut self, tx: i32, tz: i32, lod: Lod, generation: u64) -> LoadResult<T>;
    /// `unload`. Called for the object that is leaving, including a stale one.
    fn unload(&mut self, tx: i32, tz: i32, obj: T);
    fn on_loaded(&mut self, _tx: i32, _tz: i32, _lod: Lod) {}
    fn on_unloaded(&mut self, _tx: i32, _tz: i32) {}
}

/// One streamed cell. `shown` means `tileLoaded` has been emitted, so
/// `tileUnloaded` may be emitted later.
#[derive(Clone, Debug)]
pub struct TileEntry<T> {
    pub lod: Lod,
    pub obj: Option<T>,
    pub generation: u64,
    pub shown: bool,
    pub pending: bool,
}

/// `tileLoaded` / `tileUnloaded` on the web bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileEvent {
    Loaded { tx: i32, tz: i32, lod: u8 },
    Unloaded { tx: i32, tz: i32 },
}

/// `TileManager`.
pub struct TileManager<T> {
    pub tiles: BTreeMap<TileCoord, TileEntry<T>>,
    /// Max load starts per [`Self::update`].
    pub budget: u32,
    pub thresholds: LodThresholds,
    /// Appended on every real load and unload. Not cleared by `update`.
    pub events: Vec<TileEvent>,
    /// Starts performed by the most recent `update`. Failed attempts count.
    pub last_starts: u32,
    generation: u64,
}

impl<T> Default for TileManager<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> TileManager<T> {
    pub fn new() -> Self {
        Self {
            tiles: BTreeMap::new(),
            budget: DEFAULT_BUDGET,
            thresholds: LodThresholds::ORIGINAL,
            events: Vec::new(),
            last_starts: 0,
            generation: 0,
        }
    }

    /// `update`. `x` / `z` are the focus in metres (player or camera).
    pub fn update(&mut self, x: f32, z: f32, hooks: &mut impl TileHooks<T>) {
        let center = tile_of(x, z);
        let reach = self.thresholds.drop + self.thresholds.hyst;
        let r = (reach / TILE).ceil() as i32;
        let mut want = Vec::new();
        let mut seen = HashSet::new();
        for tz in (center.tz - r)..=(center.tz + r) {
            for tx in (center.tx - r)..=(center.tx + r) {
                if !hooks.has(tx, tz) {
                    continue;
                }
                let coord = TileCoord::new(tx, tz);
                let prev = self.tiles.get(&coord).map(|entry| entry.lod);
                let dist = tile_dist(coord, x, z);
                let Some(lod) = lod_for_with(dist, prev, self.thresholds) else {
                    continue;
                };
                seen.insert(coord);
                want.push(Want { coord, lod, dist });
            }
        }
        let gone: Vec<TileCoord> = self
            .tiles
            .keys()
            .copied()
            .filter(|coord| !seen.contains(coord))
            .collect();
        for coord in gone {
            self.drop_tile(coord, hooks);
        }
        want.sort_by(|a, b| a.dist.total_cmp(&b.dist).then(a.coord.cmp(&b.coord)));
        let mut started = 0u32;
        for item in want {
            if self
                .tiles
                .get(&item.coord)
                .is_some_and(|entry| entry.lod == item.lod)
            {
                continue;
            }
            if started >= self.budget {
                break;
            }
            started += 1;
            self.start(item.coord, item.lod, hooks);
        }
        self.last_starts = started;
    }

    /// Finish a [`LoadResult::Pending`] attempt. A generation that is no longer
    /// current unloads `Ok` and ignores `Err` (the newer entry owns the previous object).
    pub fn resolve(
        &mut self,
        tx: i32,
        tz: i32,
        generation: u64,
        result: Result<T, ()>,
        hooks: &mut impl TileHooks<T>,
    ) {
        let coord = TileCoord::new(tx, tz);
        let current = self
            .tiles
            .get(&coord)
            .is_some_and(|entry| entry.generation == generation);
        match result {
            Ok(obj) if current => self.finish_ready(coord, generation, obj, hooks),
            Ok(obj) => hooks.unload(tx, tz, obj),
            Err(()) if current => self.drop_tile(coord, hooks),
            Err(()) => {}
        }
    }

    /// `clear`.
    pub fn clear(&mut self, hooks: &mut impl TileHooks<T>) {
        let coords: Vec<TileCoord> = self.tiles.keys().copied().collect();
        for coord in coords {
            self.drop_tile(coord, hooks);
        }
    }

    fn start(&mut self, coord: TileCoord, lod: Lod, hooks: &mut impl TileHooks<T>) {
        self.generation += 1;
        let generation = self.generation;
        let old = self.tiles.remove(&coord);
        let (obj, shown) = match old {
            Some(entry) => (entry.obj, entry.shown),
            None => (None, false),
        };
        self.tiles.insert(
            coord,
            TileEntry {
                lod,
                obj,
                generation,
                shown,
                pending: true,
            },
        );
        match hooks.load(coord.tx, coord.tz, lod, generation) {
            LoadResult::Ready(obj) => self.finish_ready(coord, generation, obj, hooks),
            LoadResult::Pending => {}
            LoadResult::Failed => self.drop_tile(coord, hooks),
        }
    }

    fn finish_ready(
        &mut self,
        coord: TileCoord,
        generation: u64,
        obj: T,
        hooks: &mut impl TileHooks<T>,
    ) {
        enum Next<T> {
            Stale(T),
            Done { lod: Lod, old: Option<T> },
        }
        let next = match self.tiles.get_mut(&coord) {
            Some(entry) if entry.generation == generation => {
                let old = entry.obj.take();
                entry.obj = Some(obj);
                entry.shown = true;
                entry.pending = false;
                Next::Done {
                    lod: entry.lod,
                    old,
                }
            }
            _ => Next::Stale(obj),
        };
        match next {
            Next::Stale(obj) => hooks.unload(coord.tx, coord.tz, obj),
            Next::Done { lod, old } => {
                if let Some(old) = old {
                    hooks.unload(coord.tx, coord.tz, old);
                }
                hooks.on_loaded(coord.tx, coord.tz, lod);
                self.events.push(TileEvent::Loaded {
                    tx: coord.tx,
                    tz: coord.tz,
                    lod: lod.as_u8(),
                });
            }
        }
    }

    fn drop_tile(&mut self, coord: TileCoord, hooks: &mut impl TileHooks<T>) {
        let Some(entry) = self.tiles.remove(&coord) else {
            return;
        };
        if let Some(obj) = entry.obj {
            hooks.unload(coord.tx, coord.tz, obj);
        }
        if !entry.shown {
            return;
        }
        hooks.on_unloaded(coord.tx, coord.tz);
        self.events.push(TileEvent::Unloaded {
            tx: coord.tx,
            tz: coord.tz,
        });
    }
}

struct Want {
    coord: TileCoord,
    lod: Lod,
    dist: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lod_name(lod: Lod) -> &'static str {
        match lod {
            Lod::Near => "near",
            Lod::Mid => "mid",
            Lod::Far => "far",
        }
    }

    struct Mem {
        reject_pending: bool,
        live: HashSet<String>,
        pending_fail: Vec<(i32, i32, u64)>,
    }

    impl Mem {
        fn new() -> Self {
            Self {
                reject_pending: false,
                live: HashSet::new(),
                pending_fail: Vec::new(),
            }
        }
    }

    impl TileHooks<String> for Mem {
        fn load(&mut self, tx: i32, tz: i32, lod: Lod, generation: u64) -> LoadResult<String> {
            if self.reject_pending {
                self.pending_fail.push((tx, tz, generation));
                return LoadResult::Pending;
            }
            let key = format!("{tx},{tz},{}", lod_name(lod));
            self.live.insert(key.clone());
            LoadResult::Ready(key)
        }

        fn unload(&mut self, _tx: i32, _tz: i32, obj: String) {
            self.live.remove(&obj);
        }
    }

    fn leaked(mem: &Mem, tiles: &BTreeMap<TileCoord, TileEntry<String>>) -> Vec<String> {
        mem.live
            .iter()
            .filter(|key| {
                let mut parts = key.split(',');
                let tx: i32 = parts.next().unwrap().parse().unwrap();
                let tz: i32 = parts.next().unwrap().parse().unwrap();
                !tiles.contains_key(&TileCoord::new(tx, tz))
            })
            .cloned()
            .collect()
    }

    #[test]
    fn loads_all_three_lods_and_clears() {
        let mut mem = Mem::new();
        let mut mgr = TileManager::new();
        mgr.budget = 1_000_000;
        mgr.update(0.0, 0.0, &mut mem);
        assert!(mgr.events.contains(&TileEvent::Loaded {
            tx: 0,
            tz: 0,
            lod: 0
        }));
        assert!(
            mgr.events
                .iter()
                .any(|event| matches!(event, TileEvent::Loaded { lod: 1, .. }))
        );
        assert!(
            mgr.events
                .iter()
                .any(|event| matches!(event, TileEvent::Loaded { lod: 2, .. }))
        );
        assert_eq!(mem.live.len(), mgr.tiles.len());
        mgr.clear(&mut mem);
        assert!(mem.live.is_empty());
        assert!(mgr.tiles.is_empty());
    }

    #[test]
    fn failed_async_loads_do_not_leak_and_emit_unload() {
        let mut mem = Mem::new();
        let mut mgr = TileManager::new();
        mgr.budget = 1_000_000;
        mgr.update(0.0, 0.0, &mut mem);
        mem.reject_pending = true;
        mgr.update(1500.0, 0.0, &mut mem);
        let pending = std::mem::take(&mut mem.pending_fail);
        for (tx, tz, generation) in pending {
            mgr.resolve(tx, tz, generation, Err(()), &mut mem);
        }
        let leaked = leaked(&mem, &mgr.tiles);
        assert!(leaked.is_empty(), "leaked objects: {leaked:?}");
        assert!(
            mgr.events
                .iter()
                .any(|event| matches!(event, TileEvent::Unloaded { .. }))
        );
    }

    #[test]
    fn never_loaded_tiles_do_not_emit_unload() {
        struct First {
            pending: Vec<(i32, i32, u64)>,
        }
        impl TileHooks<String> for First {
            fn load(&mut self, tx: i32, tz: i32, _lod: Lod, generation: u64) -> LoadResult<String> {
                self.pending.push((tx, tz, generation));
                LoadResult::Pending
            }
            fn unload(&mut self, _tx: i32, _tz: i32, _obj: String) {}
        }
        let mut hooks = First {
            pending: Vec::new(),
        };
        let mut mgr = TileManager::new();
        mgr.budget = 1_000_000;
        mgr.update(0.0, 0.0, &mut hooks);
        let pending = std::mem::take(&mut hooks.pending);
        mgr.update(100_000.0, 0.0, &mut hooks);
        for (tx, tz, generation) in pending {
            let result = if tx == 0 && tz == 0 {
                Err(())
            } else {
                Ok(format!("{tx},{tz}"))
            };
            mgr.resolve(tx, tz, generation, result, &mut hooks);
        }
        assert!(
            mgr.events
                .iter()
                .all(|event| !matches!(event, TileEvent::Unloaded { .. })),
            "events: {:?}",
            mgr.events
        );
    }

    #[test]
    fn synchronous_lod_failure_releases_previous_tile_and_can_retry() {
        struct OnlyOrigin {
            throw_sync: bool,
            live: HashSet<String>,
        }
        impl TileHooks<String> for OnlyOrigin {
            fn has(&self, tx: i32, tz: i32) -> bool {
                tx == 0 && tz == 0
            }
            fn load(
                &mut self,
                _tx: i32,
                _tz: i32,
                lod: Lod,
                _generation: u64,
            ) -> LoadResult<String> {
                if self.throw_sync {
                    return LoadResult::Failed;
                }
                let name = lod_name(lod).to_string();
                self.live.insert(name.clone());
                LoadResult::Ready(name)
            }
            fn unload(&mut self, _tx: i32, _tz: i32, obj: String) {
                self.live.remove(&obj);
            }
        }
        let mut hooks = OnlyOrigin {
            throw_sync: false,
            live: HashSet::new(),
        };
        let mut mgr = TileManager::new();
        mgr.update(0.0, 0.0, &mut hooks);
        hooks.throw_sync = true;
        mgr.update(1000.0, 0.0, &mut hooks);
        assert!(mgr.tiles.is_empty());
        assert!(hooks.live.is_empty());
        assert_eq!(
            mgr.events,
            vec![
                TileEvent::Loaded {
                    tx: 0,
                    tz: 0,
                    lod: 0
                },
                TileEvent::Unloaded { tx: 0, tz: 0 },
            ]
        );
        hooks.throw_sync = false;
        mgr.update(1000.0, 0.0, &mut hooks);
        assert!(hooks.live.contains("mid"));
        assert_eq!(
            *mgr.events.last().unwrap(),
            TileEvent::Loaded {
                tx: 0,
                tz: 0,
                lod: 1
            }
        );
    }

    #[test]
    fn budget_starts_nearest_tiles_and_spreads_the_rest() {
        struct Order {
            keys: Vec<TileCoord>,
        }
        impl TileHooks<()> for Order {
            fn has(&self, tx: i32, tz: i32) -> bool {
                (0..3).contains(&tx) && (0..3).contains(&tz)
            }
            fn load(&mut self, tx: i32, tz: i32, _lod: Lod, _generation: u64) -> LoadResult<()> {
                self.keys.push(TileCoord::new(tx, tz));
                LoadResult::Ready(())
            }
            fn unload(&mut self, _tx: i32, _tz: i32, _obj: ()) {}
        }
        let mut hooks = Order { keys: Vec::new() };
        let mut mgr = TileManager::new();
        mgr.budget = 2;
        mgr.update(10.0, 10.0, &mut hooks);
        assert_eq!(mgr.last_starts, 2);
        assert_eq!(hooks.keys.len(), 2);
        assert_eq!(hooks.keys[0], TileCoord::new(0, 0));
        mgr.update(10.0, 10.0, &mut hooks);
        assert_eq!(mgr.last_starts, 2);
        assert_eq!(hooks.keys.len(), 4);
        for _ in 0..10 {
            mgr.update(10.0, 10.0, &mut hooks);
        }
        assert_eq!(hooks.keys.len(), 9);
        assert_eq!(mgr.last_starts, 0);
        assert_eq!(mgr.tiles.len(), 9);
    }

    #[test]
    fn loaded_level_keeps_hysteresis_until_the_far_side() {
        struct OnlyOrigin;
        impl TileHooks<Lod> for OnlyOrigin {
            fn has(&self, tx: i32, tz: i32) -> bool {
                tx == 0 && tz == 0
            }
            fn load(&mut self, _tx: i32, _tz: i32, lod: Lod, _generation: u64) -> LoadResult<Lod> {
                LoadResult::Ready(lod)
            }
            fn unload(&mut self, _tx: i32, _tz: i32, _obj: Lod) {}
        }
        let mut hooks = OnlyOrigin;
        let mut mgr = TileManager::new();
        mgr.update(256.0 + 500.0, 0.0, &mut hooks);
        assert_eq!(mgr.tiles[&TileCoord::new(0, 0)].lod, Lod::Near);
        mgr.update(256.0 + 532.0, 0.0, &mut hooks);
        assert_eq!(mgr.tiles[&TileCoord::new(0, 0)].lod, Lod::Near);
        assert_eq!(mgr.last_starts, 0);
        mgr.update(256.0 + 533.0, 0.0, &mut hooks);
        assert_eq!(mgr.tiles[&TileCoord::new(0, 0)].lod, Lod::Mid);
    }

    #[test]
    fn steady_state_update_stays_within_a_frame() {
        struct Nop;
        impl TileHooks<()> for Nop {
            fn load(&mut self, _tx: i32, _tz: i32, _lod: Lod, _generation: u64) -> LoadResult<()> {
                LoadResult::Ready(())
            }
            fn unload(&mut self, _tx: i32, _tz: i32, _obj: ()) {}
        }
        let mut hooks = Nop;
        let mut mgr = TileManager::new();
        mgr.budget = 1_000_000;
        mgr.update(0.0, 0.0, &mut hooks);
        let loaded = mgr.tiles.len();
        mgr.budget = DEFAULT_BUDGET;
        let start = std::time::Instant::now();
        let rounds = 50;
        for _ in 0..rounds {
            mgr.update(0.0, 0.0, &mut hooks);
            assert_eq!(mgr.last_starts, 0);
        }
        let us = start.elapsed().as_micros() / rounds;
        eprintln!("chunk steady-state update: {us} µs for {loaded} tiles");
        assert!(
            us < 5_000,
            "steady-state update took {us} µs across {loaded} tiles"
        );
    }
}
