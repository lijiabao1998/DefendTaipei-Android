# Stopped traffic and reclaimed streets

References: [Web `src/map/wreckage.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/wreckage.ts) and [Web `src/systems/reclaim.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/systems/reclaim.ts). Native implementation: [`map/wreckage.rs`](../game/src/map/wreckage.rs), [`reclaim_street.rs`](../game/src/reclaim_street.rs) and [`tools/generate_orders.py`](../tools/generate_orders.py).

## v24.5 stopped traffic

- On every road at least 6 m wide, rows of 4–9 scooters stand nose-in along a kerb (12% knocked over). Residential lanes have cars parked along one kerb (4% burnt out, some taxis); narrow lanes have a dropped scooter or two.
- Roads 9 m and wider have abandoned traffic in both directions every 9–24 m: cars and taxis, buses on roads 12 m and wider, 18% slewed in a crash, 9% burnt out with a scorch mark, glass and paper around each. One road segment in three over 30 m long has a barrier line (water-filled red/white or concrete) across a lane. Litter lies along every road.
- Cars are 1.45 m cover, buses 2.8 m and barriers 1 m, in the shared world collision for movement, sight and shots. Doorways, the start and the fortified corners and checkpoints are kept clear.
- Rendering merges each 80 m cell into one vertex-coloured mesh for vehicles and barriers, one for litter and one for scorch marks, so off-screen cells are culled whole. The plan uses its own fixed random stream; the Web's session random is not reproducible.

## v24.14 reclaimed streets

- While a core area is occupied, about a fifth of the street-facing walls of buildings over 6.5 m carry a vertical red order banner (up to 150 per area), and roads 9 m and wider are crossed every 70–140 m by a red curfew notice between two poles. The texts are the Web's invented orders, baked once into `assets/textures/orders.png` with Noto Sans CJK.
- When the area is reclaimed the banners come down. ROC flags hang from windows (up to 320 per area), big flags fly on the roofs of some taller buildings, and about 110 groups of two to five neighbours stand along its streets among red firecracker paper; a third of them wave a small flag. The flags and neighbours are built the first time the area is reclaimed.
- At the moment an area falls, up to sixteen groups gather around the player. Where that happened is saved, so after Continue the crowd stands in the same place without a second party.

## Boundaries

Flags and people are static meshes: the Web's shader sway is not ported. Banners do not fade with the occlusion cutaway of the building they hang on.

## Checks

Unit tests cover the counts of vehicles, scooters, litter and barriers, the clear start and corners, determinism, vehicle cover, the per-area banners, flags and neighbours, one neighbour's geometry and the atlas cells.
