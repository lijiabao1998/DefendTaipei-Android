# Native shelter and survival integration

## Reference and scope

The authoritative reference is Web v24.42.0 at commit [`b9b0a2736642b37dd45925a4719dfef08b8f36b4`](https://github.com/lijiabao1998/defendtaipei/tree/b9b0a2736642b37dd45925a4719dfef08b8f36b4). This document describes the native Wanhua survival loop, not all Web-game parity.

| Original source | Native implementation |
| --- | --- |
| [`src/systems/survival.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/systems/survival.ts) | `game/src/survival.rs`: roster, stash order, dawn settlement, upgrades, prayer, morality, barter and ending text |
| [`src/systems/items.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/systems/items.ts) | `game/src/inventory/catalog.rs`: 126 item records and 18 loot tables; `inventory.rs`: consumption, equipment, cooking and spoilage |
| [`src/core/main.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/core/main.ts) | `worldplay.rs`, `game_flow.rs`, `save.rs`: clock, interactions, weather, utilities, needs and persistent run lifecycle |
| Web soldier-drop rules | `inventory::soldier_drop` and `combat::EnemyDied`: deterministic death containers and the original type/weight rules |

## Integrated behavior

A new run initializes the chosen role's carried inventory, empty hands, needs at 85 food / 85 water / 10 fatigue, the shelter roster and shared stock, a native home and patrol world. Continue restores the existing run directly and does not enter the new-game prologue.

The campaign clock advances at 0.4 game minutes per real second for the default one-hour day. Speeds are 1×, 3× and 20×; sleep uses 20×. A persisted f64 remainder prevents f32 accumulation from freezing during long runs. Pause and background suspension stop the clock, movement and combat in the same frame.

Dawn is detected with `floor((gameMin - 360) / 1440)`. Weather updates before dawn settlement, then player needs and refrigeration spoilage follow it. The restored dawn baseline is set from the saved clock, preventing another meal, raid or plant harvest immediately after Continue.

The shelter panel supports food/drink/medicine deposits, all five upgrade purchases, once-per-day prayer, survivor status, shared stock and a journal. Transactions check distance, item counts and already-built state before mutating inventory. The reachable Longshan stall uses the original price curve and item values. Moral prompts commit their asked flag only after an accepted choice, so closing the app while a prompt is open cannot skip the choice.

Inventory use updates actual health, bleeding, food, water and fatigue. Combat reads the equipped item and actual ammunition; there is no free gameplay ammunition fallback. Night loot, police-armory guaranteed guns/ammunition, spoiled late searches, fallen-soldier loot and container depletion are included. The native home has one visible blue water fixture: it adds a bottle only with running water, a nearby same-building player and a strict `gameMin > nextRefill` check. Its 90-minute cooldown is saved.

Power cuts at day 3, 19:00; water cuts at day 5, 07:00. Rain feeds the shelter adapter and world ambience. Fresh carried/shared food spoils on later dawns after the original refrigeration threshold. Indoor sleep checks fatigue and nearby danger; hunger, thirst, fatigue and healing use the original rates and thresholds. Fatal damage takes priority over evacuation in the same frame.

The real helicopter position and original availability window drive extraction. An ending includes the shelter's final condition. The native game does not expose the old H/B instant-success shortcuts or advertise a boat route whose informant progression is not implemented.

## Persistence

`game/src/save.rs` stores a versioned JSON snapshot of campaign, role/gender, player transform/vitals, inventory/equipment, shelter, utility state, needs, weather, searched containers, moral choices, refill timers and killed-soldier drops.

Writes go to a sibling temporary file, are flushed, then atomically renamed. Reads validate schema version, file size, finite numeric ranges, item/role IDs, unique world keys and playable player coordinates before changing the live world. Invalid data leaves the current run unchanged and displays an error. Earlier version-1 files default newly added optional fields.

Autosave runs every 20 real seconds while Playing and on background/close. Manual Save and Save and return to menu share the same validated writer; menu transition waits for a successful write. A new game or permanent death clears the old run. Continue clears transient attacks/inputs and replaces enemy/loot markers so an old scene cannot duplicate soldiers or leave ghost crates. Android stores the file under its private internal data path; desktop QA can use an isolated `DEFEND_TAIPEI_SAVE_DIR`.

## Evidence and regression gates

- Original TypeScript is executed by the fixture generators, rather than rewriting expected output in Rust. The shelter fixture covers four scenarios, 62 dawns and five barter price bands, comparing complete state, log order, ending and RNG progression.
- Item catalog generation captures every original field used by the native inventory. Soldier-drop fixtures cover all 64 native soldier IDs and the final RNG state. Loot fixtures cover all 18 table types before and at the spoilage threshold.
- Runtime tests exercise inventory transactions, empty-ammo behavior, paused attacks, collision, shelter costs, corrupt-save rejection, new-run/death clearing, autosave and Save → Menu → Continue across dawn.
- UI tests cover portrait/landscape panel bounds, disabled actions, press-and-hold capture, Continue routing and a real same-frame pause input through the flow and input plugins.
- `--qa` captures eighteen actual Bevy GPU-rendered screens, including the title, survivor selection, departure review, street and entrance views, and asserts preserved role, position, inventory, shared stock, day and clock after Save → Menu → Continue. Its fixed timestep makes screenshots reproducible; its displayed FPS must not be used as hardware performance evidence.
- CI builds an ARM64 phone APK and an x86_64 emulator APK. The emulator script uses real touch input, checks resulting private save JSON and captures both orientations. Failure remains a failed job; it is not hidden with `continue-on-error`.

See the exact commit's [Actions run](https://github.com/lijiabao1998/DefendTaipei-Android/actions/workflows/ci.yml) for completed results and artifacts. User-provided phone screenshots show the earlier native build running; a measured device performance and compatibility pass is still outstanding. The current street, entrance and title work is described separately in [STREET-TITLE-PARITY.md](STREET-TITLE-PARITY.md).

## Explicit differences and remaining work

The Wanhua core uses the original road/building geometry, but native cabinet, home-furniture and patrol placement are independently seeded. The Web renderer interleaves many unrelated RNG calls, so matching a cabinet ID does not guarantee identical placement or contents across engines. The loot rules are compared with equal RNG input.

The native interaction transfers an entire nearby container to the carried bag. The Web per-item pickup UI, locked-door/furniture traversal, full building floors, all Taipei districts, armor/vehicle systems, AI teammates/rescues, metro, boat contact and complete district-recapture loop remain outside this integration. Dongyuan's original trade coordinates are retained but lie outside the native playable bounds. Anti-armor item records exist in the catalog, but their combat interaction is disabled until armor targets are ported.

Weather drives lighting, audio, utility and shelter outcomes; rain-particle/flood visuals and exact Web character/weapon meshes are not claimed as complete visual parity. English labels are provided for the new interaction surfaces; original radio and story copy remains Chinese.
