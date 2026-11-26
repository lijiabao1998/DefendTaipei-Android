# Selective scavenging and persistent fixture stock

Reference: Web v24.42.0, commit
[`b9b0a2736642b37dd45925a4719dfef08b8f36b4`](https://github.com/lijiabao1998/defendtaipei/tree/b9b0a2736642b37dd45925a4719dfef08b8f36b4).
The native UI follows `src/core/main.ts`'s `renderPanel` / `take` flow and the
fixture names and loot tables in `src/map/interiors.ts`.

## Playable behavior

- Interact opens the nearest reachable shelf, refrigerator, cabinet or dropped
  supply container. Opening does not move any items into the player's bag.
- Each card shows the actual item name, count, total stack weight, category and
  count already carried. Take 1, Stack and Take what fits all modify the same
  saved stock used by the furniture's visible products.
- The player's pickup limit is **15 kg**, matching Web `take()`. Carry skill
  still changes the separate `12 kg × carry` movement penalty threshold.
  Native Stack / Take what fits accept the portion that fits, leaving all other
  units in that same fixture; Web's whole-stack operation rejects an overweight
  stack instead. Take-all continues to lighter items when a heavy stack will
  not fit. Other existing grants, such as a paid barricade refund, may exceed
  this pickup limit; they do not destroy items.
- Every transfer rechecks the active container ID, distance under 2.2 metres,
  building identity, and line of sight. A closed door, a barricade, moving away,
  closing the panel, or death cannot be bypassed by an old button action.
- The world clock remains live while browsing. The modal captures movement and
  combat input. Close, Escape, switching to Bag, map/pause transitions and
  orientation changes use the shared native panel lifecycle.
- Empty, full-bag and out-of-reach states disable pickup actions but keep Close,
  Bag and Save available. Phone portrait and short landscape use bounded pages;
  each visible item has both single-item and stack controls.
- Water filling checks available carrying capacity before spending its existing
  90-game-minute refill cooldown. A full bag does not lose that refill.
- Home moral choices are keyed to the building (`c{id}`), so its additional
  cabinets do not repeat the same moral prompt merely because another shelf was selected.

## Saved state and upgrades

`ScavengeState.loot` is the authoritative stock record. First search freezes the
initial deterministic roll (including the one-time night bonus); later searches
never reroll, apply another night bonus, or turn an empty record into new goods.
Opened fixture leftovers currently remain fixed after their first search. Web
also ages refrigerator/counter/stall contents periodically after a power cut;
that periodic fixture-stock decay is not implemented in this wave. Initial late
search spoilage and carried/shelter stock spoilage remain active.

Partial counts and empty records are included in the existing atomic native save,
20-real-second autosave, background save, and manual Save / Continue path.

The first furnished loot point keeps the old `c{id}` identifier. New points have
stable `x{id}_{slot}` identifiers independent of mesh streaming. Old saves have
`fixtures_version = 0`: on Continue, new points in any previously opened building
are recorded empty, while the exact old `c{id}` leftovers remain unchanged.
Previously unsearched buildings receive normal new fixtures. A successful
migration records version 1 and is idempotent. Fresh runs start at version 1.

Fallback supply boxes remain only where no suitable furniture layout fits.
Physical furniture replaces those boxes in furnished rooms; the marker pass uses
a precomputed per-container flag rather than scanning all furniture for each
container every frame.

## Verification

Regression tests cover unit conservation during partial transfers, capacity and
light-item fallback, repeated night searches, reloads, no refill of empty stock,
legacy fixture migration, actual Interact opening without automatic collection,
closed-door and distance rejection, stale panel rejection, and water cooldown
preservation. Layout tests cover 320×640, 390×844, 640×360 and 844×390 in both
languages, with every visible pickup/close/navigation target inside the safe
viewport and no overlapping buttons. The save module separately exercises real
disk save and Continue with partially emptied fixture stock and carried items.

The native layout and random placement stream remain independent of Web's
interleaved renderer RNG. This wave does not implement teammate bag exchanges,
item drag-and-drop, floor switching or all Web district interiors.
