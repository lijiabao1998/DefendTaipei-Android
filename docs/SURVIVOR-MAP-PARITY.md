# Survivor map — Wanhua native scope

Reference: Web v24.42.0 [`src/map/map.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/map.ts). The native map is implemented in `game/src/worldmap/`, using the very same planned streets and building footprints as the 3D world. It adds no remote tile service or separately authored geography.

## Implemented

- Paper palette with residential, shop, medical, temple and police building colours; original OSM road labels, player direction, home, discovered barter and the currently active helicopter extraction point. No enemy positions are revealed.
- Exploration extends 38 m around the player, following the Web radius. A fixed 10 m grid covers the 1.12 × 1.00 km core. The containing cell is always revealed, including near cell edges. Unexplored cells are opaque; previously explored cells remain visible.
- Drag to pan, mouse wheel or two-finger pinch to zoom, plus zoom buttons, player center and whole-core view. Controls reflow for portrait and short landscape screens. The map wheel never changes the gameplay camera zoom.
- Four player annotation categories: Supplies, Danger, Safe and Evacuation. Select a tool, then tap the paper. Erase deletes the nearest own mark. Marks are limited to 128, reject nonfinite/out-of-bounds positions and deduplicate nearby repeated marks of the same kind.
- `M` or the dedicated Map button opens/closes the panel; Escape closes only the map. All map presses are captured before player movement/fire, including the opening frame and a held pointer carried through closing. An opening finger cannot create a mark when a mark tool was previously selected. Android touches are processed from the ordered raw event stream, including complete DOWN/UP taps between slow frames and multiple taps reusing pointer 0. Canceled touches, out-and-back drags, pinches and unfinished gestures across rotation cannot create annotations.
- Opening the map blocks gameplay input but does **not** pause the clock. The footer states this; Pause remains the way to stop the world.

## Persistence and cost

`MapProgress` lives in the validated native save under `map`; missing fields from older saves default to empty discovery and notes. Mark edits request an immediate save, while discovery follows normal autosave/background/manual saving. New-game activation clears exploration; Continue restores it without the new-game reset path.

The paper texture is created only while open, reuses one image, caps its dimensions to 1000 × 800, and redraws at most ten times per second during gestures. Bevy change ticks detect exploration edits without cloning the discovered-cell tree every frame. Geometry is clipped to the paper bounds before rasterization; street labels are limited and avoid overlap. Hidden maps retain no per-frame raster work.

The bundled font subset includes OSM road-name characters, and its glyph coverage test checks those labels as well as Rust UI strings.

## Verification and boundaries

Unit and integration checks cover discovery edges/monotonicity, bounded notes, save roundtrip, coordinate transforms/zoom anchoring, screen/control bounds, clipping long roads and large footprints, map input capture and Escape behavior. Full schedule regressions cover a one-frame open/tool/mark sequence, coalesced short taps, cancellation, drag return, pinch release, rotation and held opening pointers. The former snapshot-only code was verified to fail the coalesced-touch regression before the ordered-event fix. Native GPU QA captures portrait and short-landscape map views, then checks map notes/discovery and door state after Save → Menu → Continue. Android smoke selects the tool and creates a mark with ordinary short `adb input tap` events, checks held-pointer reopening, compares actual private save JSON and rotates the restored map.

This does not implement the Web game's other districts, liberation/occupation overlays, enemy intel, readable-map item reveals, metro maps, text annotation editing or the courier's fully revealed map perk. The rendering and exploration are native equivalents over the current core, not a claim of complete Web feature parity.

Map data © OpenStreetMap contributors, ODbL.
