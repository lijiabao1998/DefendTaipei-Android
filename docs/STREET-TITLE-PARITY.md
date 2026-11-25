# Streets, entrances and title flow

**Historical scope:** this records PR #17's first street/title wave. The next wave replaces fixed-open leaves with [interactive persisted doors](DOORS-PARITY.md), adds the [survivor map](SURVIVOR-MAP-PARITY.md), [expanded HUD/report history](HUD-PARITY.md), and [weather/power-responsive atmosphere](CITY-ATMOSPHERE-PARITY.md). Its native QA expands the 18 captures below to 28. Statements about fixed-open doors below describe PR #17, not the current build.

This document describes the native visual work prompted by physical-phone screenshots showing overly clean paving and walls, conspicuous colored asphalt patches, missing visible doors and a sparse start screen. It records the implemented scope and the checks required for this revision. It does not certify pixel-for-pixel Web parity or a hardware frame-rate target.

The reference is **Defend Taipei Web v24.42.0**, pinned to [`b9b0a2736642b37dd45925a4719dfef08b8f36b4`](https://github.com/lijiabao1998/defendtaipei/tree/b9b0a2736642b37dd45925a4719dfef08b8f36b4). The native changes use procedural textures, meshes and UI from this repository. No generated bitmap artwork is required.

## Source and implementation map

All Web links below are pinned to that source revision. Native links point to files in this checkout.

| Area | Original Web reference | Native implementation |
| --- | --- | --- |
| Asphalt, wall and ground materials | [textures.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/textures.ts) | [map/textures.rs](../game/src/map/textures.rs), [map/canvas.rs](../game/src/map/canvas.rs) |
| Paver scale, facility course, arcade palettes and concrete wear | [paving.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/paving.ts), [arcade.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/arcade.ts) | [map/textures.rs](../game/src/map/textures.rs), [map/street.rs](../game/src/map/street.rs), [map/build.rs](../game/src/map/build.rs) |
| Road edges, accumulated dirt, street and façade detail | [roads.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/roads.ts), [decay.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/decay.ts), [alley.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/alley.ts) | [map/street.rs](../game/src/map/street.rs), [map/detail.rs](../game/src/map/detail.rs) |
| Entrance frames and gate appearance | [doors.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/doors.ts) | [map/build.rs](../game/src/map/build.rs), [map/data.rs](../game/src/map/data.rs) |
| Home, survivor selection and departure review | [core/main.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/core/main.ts), [ui/style.css](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/ui/style.css) | [ui/title.rs](../game/src/ui/title.rs), [ui/layout.rs](../game/src/ui/layout.rs), [game_flow.rs](../game/src/game_flow.rs) |
| Cached portraits of the actual role rig | [ui/portrait.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/ui/portrait.ts) | [ui/portrait.rs](../game/src/ui/portrait.rs), [human/rig.rs](../game/src/human/rig.rs) |
| Controls and credits | [ui/menu.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/ui/menu.ts) | [ui/title.rs](../game/src/ui/title.rs) |

## Streets and exterior surfaces

**Asphalt repairs use neutral tones.** The previous native patch colors varied their red, green and blue components independently, which could produce large green or purple rectangles. Repairs now share a correlated neutral value, have irregular outlines and cut edges, and sit over aggregate, fine cracks and restrained stains. The material remains a shared repeating texture; it is not a unique scan of each real street.

**Sidewalk pavers fill their cells correctly.** Paired 10 × 20 cm pavers alternate direction within filled 20 × 20 cm cells. This removes the old overlapping pattern's regularly spaced dark holes. Chips, damaged joints, cracks and grime break up the paving. A 30 cm facility course follows the physical curb and has its own seams, rather than appearing as a stripe inside every texture repeat. The native pattern is an alternating paired-brick layout; it is not a claim to reproduce every Web paving pattern exactly.

**Concrete lots and arcades have different wear.** Ground slabs include color variation, branching cracks, joint moss and localized tile/brick repairs. Arcade floors use four shared palettes drawn from the Web paving styles: maroon ceramic, beige/brown checker, grey slate, and green/white mosaic. Edge wear and tracked-in grime accompany the changed floor and column roughness. Shared wall textures add broad damp areas, lime streaks, peeling render, cracked joints and damaged tiles.

**Street dressing follows the road edge.** Gutter stains, flat drain grates, utility covers, curb scuffs and small paper/grit/grass clusters are placed from road geometry. They avoid the clear area at connected junction ends and zebra crossings. Decoration checks existing entrance corridors, including recessed arcade approaches, and existing walkability before placing sidewalk litter. These are visual details; they do not add collision obstacles or alter gameplay state.

The detail budget is bounded by fixed placement intervals, small meshes and the existing distance tiers. A drain uses flat bars instead of a set of cuboids; an individual litter cluster remains under 10 cm high. Door-clearance queries first filter doors against each complete road segment's bounds, expanded by half its road width plus 13 m. Filtering uses the complete segment because a segment can extend well beyond the chunk that owns its midpoint.

## Visible entrances and façade utilities

Existing planned entrances now carry painted jambs, a lintel, a shallow sill and gate leaves folded against adjacent solid wall. Home entrances use red paint; other entrances use an ochre frame with darker green leaves. Near detail adds metal handles, chipped kickplates, a mat and a stable fictional address plaque where the wall has enough room. Mid-distance entrances retain their recognizable frame and gate straps; the extra entrance mesh is omitted at the far tier.

Gate leaves are fitted to available wall space beside each opening. They do not bridge the doorway, extend around an unsupported corner or change the collision planner's door position. The main opening remains the existing 1.6 m opening. Tests check the actual rendered passage and collision clearance rather than assuming that every original map lot has a reachable entrance.

Near façades also gain lower-wall ceramic panels, drainpipes, meter boxes, damaged brick/render, faded notices and window runoff. Arcade shutter rails and upper housings distinguish the recessed shopfronts. These use existing material families and are placed on solid wall spans outside the doorway.

**Door behavior remains limited:** the new doors are permanently shown open. There is no open/close/lock state, animation, lockpick action, door inventory or new saved door state. The visual work makes the existing entrance visible; it does not port the Web door interaction system.

## First run and title navigation

The native sequence is **Home → Survivor → Briefing → Prologue → Playing**.

1. **Home** shows the title over a subdued plan of the actual Wanhua road data. It offers **新遊戲 / New game**, **繼續遊戲 / Continue**, **操作說明 / Controls**, **製作名單 / About**, and Traditional Chinese/English selection. Continue is disabled unless a validated save is available.
2. **Survivor** presents six roles, male/female selection, the selected survivor's information and starting supplies. Choose **下一步 / Next** to review the run. Selection and Back do not start a run or discard the current save.
3. **Briefing** shows the selected survivor, supplies, the one-real-hour day at 1× speed, shelter guidance and the save/permanent-death notice. **走進萬華 / Depart** is the action that creates the new run and replaces the previous save. Back returns to selection.
4. **Prologue** runs the existing opening sequence and provides Skip. Play then begins at the shelter. The shelter, supply, save and daily-survival scope is documented in [SHELTER-PARITY.md](SHELTER-PARITY.md).

Controls is a three-page guide to movement/combat, supplies/shelter, and saving/returning. About contains authorship, native-engine, map and font credits. The screens adapt their card grids, text and button positions for portrait and short landscape viewports. Pointer input, keyboard navigation and automated QA feed the same guarded title actions.

Portraits come from `human::rig::make_rig`, using each role's actual outfit and selected gender. A small CPU rasterizer produces transparent 160 × 208 portraits and caches them by role and gender, for at most twelve images. They do not require six continuously running portrait cameras or animated rigs. The title tree is rebuilt when its viewport, page, survivor, language or save-status render key changes.

The native flow preserves the Web distinction between choosing a character and committing to departure. It does not include the Web's full Preferences or Settings screens, configurable day length, multiplayer or teammate selection. The native day remains one real hour at 1×; prologue skipping occurs on the prologue screen.

## Performance approach

Surface maps are generated once on the CPU and shared, with the existing mip chain and repeat sampling. Added static geometry is merged into the existing material batches for streamed chunks/buildings. Close-range utility and gate detail uses the existing distance tiers. The work does not add an entity or update system for each stain, paving joint, drain bar or notice, and does not change world/save keys or gameplay randomness.

These choices constrain allocation, geometry and per-frame work; they are not measured FPS improvements. Physical-phone screenshots inform the visual review, but no instrumented hardware frame-rate, frame-time, memory, thermal or broad GPU-compatibility result is asserted here. Software-Vulkan QA and emulator smoke tests serve different purposes from a physical-device performance test.

## Verification gates

Run these from the repository root:

```sh
cargo fmt --all -- --check
bash scripts/test-gradle-assemble.sh
CARGO_PROFILE_DEV_DEBUG=0 cargo clippy --workspace --all-targets -- -D warnings
CARGO_PROFILE_DEV_DEBUG=0 cargo test -p defend_taipei --lib
CARGO_PROFILE_DEV_DEBUG=0 cargo run -p defend_taipei -- --qa
```

The relevant unit-test gates cover neutral asphalt values, deterministic finite portraits, menu navigation and save guards, responsive hit regions, bounded decorative meshes, door-approach clearance, full-length road-segment door filtering, visible entrance passage and façade placement outside openings. Existing gameplay/save tests remain part of the same library suite. This document specifies the gates; it does not substitute for the result of a run on the current revision or claim an exact passing test count.

The [CI workflow](../.github/workflows/ci.yml) also packages arm64 and x86_64 APKs, checks the QA report, and exercises the disposable Android emulator through touch, save/continue and orientation changes. The [Gradle helper](../scripts/gradle-assemble.sh) retries only recognized HTTP 429/502/503/504 download failures, up to three attempts; it does not retry arbitrary compiler failures. Its shell regression test checks bounded retries and preserved failure status.

### Native capture coverage

[game/src/qa.rs](../game/src/qa.rs) defines **18 actual native Bevy captures**. They render the running game to an offscreen GPU image; they are not UI mockups or exported Web frames. `DEFEND_TAIPEI_QA_OUT` selects the destination, and CI publishes the images, `qa-report.json` and execution log in the **native-qa** artifact.

| Capture | Viewport | Coverage |
| --- | --- | --- |
| `01-title-landscape.png` | 1280 × 720 | Home |
| `02-title-portrait.png` | 720 × 1280 | Home |
| `02a-title-small-portrait.png` | 320 × 640 | Compact Home |
| `02b-survivor-portrait.png` | 390 × 844 | Survivor cards and selected profile |
| `02c-survivor-small-landscape.png` | 800 × 360 | Compact survivor selection |
| `02d-briefing-small-landscape.png` | 800 × 360 | Compact departure briefing |
| `02e-briefing-portrait.png` | 390 × 844 | Departure briefing |
| `03-prologue.png` | 1280 × 720 | Opening after Depart |
| `04-playing.png` | 1280 × 720 | Gameplay after Skip |
| `04a-streetscape-portrait.png` | 390 × 844 | Reachable sidewalk, road and street detail |
| `04b-entrance-landscape.png` | 1280 × 720 | Reachable building entrance |
| `05-shelter-landscape.png` | 1280 × 720 | Shelter |
| `06-shelter-small-landscape.png` | 800 × 360 | Compact shelter |
| `07-shelter-portrait.png` | 720 × 1280 | Shelter |
| `08-inventory-portrait.png` | 720 × 1280 | Inventory |
| `09-barter-portrait.png` | 720 × 1280 | Barter |
| `10-title-with-save.png` | 1280 × 720 | Save and return to Home |
| `11-continued.png` | 1280 × 720 | Continue restored run |

The continuation check compares the saved day, shelter stockpile, carried inventory, selected survivor, position and campaign clock. CI requires a successful report containing all 18 captures. The fixed QA timestep and software Vulkan option make these reproducible functional/rendering checks, not phone performance measurements. Controls and About are not separate capture stages in this list.

## Remaining visual scope

The shared procedural maps, native deterministic placement and Bevy lighting differ from the Web renderer. Some environmental density, individual street objects, weapon silhouettes and building details remain simplified. This wave does not extend playable districts, add enterable floors or port the interactive Web door system. For the wider gameplay boundary, see [SHELTER-PARITY.md](SHELTER-PARITY.md) and the [README](../README.md#remaining-web-port-work).
