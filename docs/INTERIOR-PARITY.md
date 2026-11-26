# Native Wanhua interiors and scavenging

Reference: Web v24.42.0 (`b9b0a2736642b37dd45925a4719dfef08b8f36b4`),
`src/map/interiors.ts` (`furnish`, fridge/gondola/wallshelf/counter/wardrobe/kitchen,
shrine/woodsofa/bed/table fixtures). The native world uses its own deterministic
placement stream and existing OSM Wanhua footprints.

## Implemented

- Homes have wooden wardrobes, kitchen cupboards and sinks, family shrines,
  wooden sofas, beds, dining tables and red stools when the room can fit them.
  Blue-and-white slippers and a threshold mat give the entrance a domestic scale.
- Stores combine shelves with price strips, drink refrigerators, checkout counters
  and registers. Pharmacies use green medicine displays. Police buildings use
  equipment lockers. Fixtures are procedural coloured meshes, with no downloaded
  brand images or new bitmap textures.
- Each accepted piece occupies real collision geometry. The same indexed segments
  stop player/NPC walking, melee and line of sight; firearm ray height allows shots
  above low furniture. Furniture uses the exact visual footprint and height.
- Each container exposes a safe front approach point. Placement samples entire
  fixture edges against the footprint and existing indexed walls, rejects overlapping
  OSM ownership, checks separation and preserves a direct
  player-capsule route from the entrance to every approach using analytic swept
  rectangle checks. It never fills a narrow
  or irregular room by forcing furniture through its walls or doorway. Such rooms
  retain the original fallback crate.
- The first loot fixture in a building retains the original native `c{id}` ID.
  Additional fixtures use `x{building}_{layout-slot}` IDs independent of camera,
  render order and streaming. `WorldData.interiors` is the single catalogue used by
  collision, rendered furniture and the scavenging panel. Legacy saved player
  coordinates remain exact unless new furniture overlaps them; those positions
  move to the nearest clear point in the same room before door safety checks.
- Open shelves and refrigerators show products from the same deterministic stock
  preview and saved leftovers used by scavenging. An existing empty stock record
  remains empty. Taking the last item removes its visible products. Products are
  generic bottle/can/carton shapes, capped at 18 units per fixture; this display is
  a bounded illustration of remaining types, not a literal inventory counter.
- Furniture streams in at 44 metres, limited to 10 nearest rooms and at most 7
  furnishings per room. Each live room uses one body batch and one stock batch.
  Selection/stock refresh runs at most four times per second; bodies are rebuilt
  only on streaming, stock only when its ledger fingerprint changes. Unloaded
  entities release their owned mesh handles.

## Deliberate scope

The current native building classification is mainly home/store/pharmacy/police.
This does not reproduce every Web shop subclass, supermarket plan, breakfast-shop
menu, night-market stall, room partition, power-emitting appliance or item-card
sprite. Domestic shrines are furniture/loot points; prayer remains the existing
shelter mechanic. Kitchen sinks in this furniture layer are not additional water
sources: the existing home water fixture and its persisted cooldown remain the
water interaction. Cupboards remain closed visually; their contents are read in
the scavenging panel. Doors and door controls are documented separately.

## Validation

The focused tests check real Wanhua footprints, deterministic fixture IDs and
positions, preserved routes, bounds and entry clearance, physical movement/LOS
and height-aware shots, bounded meshes/product display and fully empty stock.
Native QA and Android smoke results for a delivered build are recorded with that
build's verification artifacts; this document does not claim physical-device
frame-rate, thermal or memory measurements.
