# DefendTaipei-Android · 保衛台北

A native **Rust + Bevy 0.20.1** port of [Defend Taipei](https://github.com/lijiabao1998/defendtaipei). Desktop and Android run the same game crate. The source reference is Web **v24.42.0**, pinned to [`b9b0a27`](https://github.com/lijiabao1998/defendtaipei/tree/b9b0a2736642b37dd45925a4719dfef08b8f36b4).

## Current playable scope

The Wanhua core now connects exploration, combat, carried supplies, a shared shelter, daily survival and persistent saves. This is a playable native survival slice; it does **not** yet contain every district or every system from the Web game. See [SHELTER-PARITY.md](docs/SHELTER-PARITY.md) for gameplay boundaries, [STREET-TITLE-PARITY.md](docs/STREET-TITLE-PARITY.md) for the first street/title wave, [Wanhua exterior wave 1](docs/WANHUA-EXTERIOR-WAVE1.md) for building architecture, and the follow-up references for [interactive doors](docs/DOORS-PARITY.md), [survivor map](docs/SURVIVOR-MAP-PARITY.md), [HUD and reports](docs/HUD-PARITY.md), [city atmosphere](docs/CITY-ATMOSPHERE-PARITY.md), [furnished interiors](docs/INTERIOR-PARITY.md), [selective scavenging](docs/SCAVENGE-PARITY.md), [audio](docs/AUDIO-PARITY.md), [settings](docs/SETTINGS-PARITY.md), [container aging](docs/CONTAINER-AGING-PARITY.md), and [Android performance controls](docs/ANDROID-PERFORMANCE.md).

- **World:** the original Wanhua road/building data, streamed geometry, interior collision, cover, fading roofs/walls, and an isometric camera that adapts to landscape and portrait. Worn asphalt, filled brick paving, cracked concrete, curb seams, drains, four arcade floor palettes, stained façades, utilities, shallow awnings, service cables and potted plants make the streets distinct. Red home gates and green shop doors can be opened and closed; closed doors block movement, sight and shots, and their state survives streaming and Continue. A hammer and one plank can barricade an entrance; dismantling returns the plank, and hammering can draw patrols.
- **Interiors:** eleven furniture types form deterministic store, medical, police, food-shop and home layouts: stocked shelves, refrigerators, counters, cabinets, wardrobes, kitchen units, family shrines, beds, sofas and tables. Solid furniture shares movement, sight and height-aware projectile collision. Placement preserves a clear route from the entrance to every searchable fixture; stock meshes show the actual saved leftovers. A bounded near layer renders at most ten furnished rooms. Wheel and phone pinch zoom share a 0.5×–3.2× range for close inspection.
- **Building exteriors:** distinct townhouse, walkup and shophouse facade rhythms, projecting tiled balconies, deeper masonry window reveals, and upper-level rear service stairs where the whole assembly fits. Stable rooftop reservations separate tin sheds, roof-access headhouses, existing tanks and utilities. Balcony and roof masses persist at Mid LOD, while rail and appliance detail stays close-range. Added architecture follows the existing roof/wall cutaway. Upper service stairs and roof doors are visual architecture; ground-floor entrances remain the playable route.
- **Survivor map:** a paper map drawn from the same Wanhua streets and footprints, with real street names, 38 m exploration, four annotation types, drag/pinch/scroll, center/whole-core controls, a compass and scale. Discovery and marks persist in the save. Unknown geography remains covered; this map does not expose enemies or unimplemented districts.
- **Survivors and combat:** six selectable roles, gender selection, procedural character rigs, 64 patrol routes, night patrols, line of sight, noise, investigation, cover and attacks. Equipped weapons use the real inventory and consume their matching ammunition.
- **Supplies:** 126 item definitions and 18 loot tables from the pinned Web source. Open a named nearby fixture or fallen-soldier cache, inspect its actual stacks, then take one item, a stack, or everything that fits the Web's 15 kg limit. Leftovers persist and do not reroll on reload. After power loss, opened refrigerators/counters/stalls continue to age on saved six-game-hour intervals, conserving item counts when perishable food becomes rotten. Equip, eat, drink, treat wounds, unload ammunition and cook supported food. Search panels recheck distance and sight before every transfer.
- **Shelter:** shared food/water/medicine deposits, five construction upgrades, survivor meals and illness, morale, arrivals, raids, rain collection, planters, once-per-day prayer, moral choices and a shelter-dependent ending paragraph. Daily settlement occurs when crossing 06:00.
- **World survival:** hunger, thirst, fatigue, sleep, 1×/3×/20× time, rain, power/water cutoffs, food spoilage, a refillable home water fixture and the reachable Longshan barter stall. Smooth dusk/dawn, local street/sign lights, visible rain and wet road materials respond to the actual saved environment; power loss switches the city lighting off. A new run can use one, two or four real hours per game day at 1× speed; Continue preserves that run’s saved duration.
- **Flow and presentation:** Home → survivor selection → departure briefing → prologue, with cached portraits made from the actual native character rigs, starting-kit details, Controls, About and save-aware Continue. Pause, inventory, shelter, barter, journal and ending screens support portrait and landscape. Traditional Chinese/English controls and item labels, a bundled CJK font, dictionary-based line breaking, environmental audio and alerts are included. Original story broadcasts remain in Chinese.
- **Sound and settings:** 104 compact OGG assets include 45 variants of the Web’s 12 recorded gunshots and three blasts, alongside 59 synthetic effects. Actual-motion footsteps, doors, scavenging, supplies, shelter actions, indoor/outdoor rain, power-aware home hum and scheduled city cues use separate effects, ambience and radio buses. Home and Pause offer live master/category sliders, mute, auditions, language and next-run day duration. Preferences persist independently of the game save. Broadcasts use radio texture; spoken narration and a music soundtrack are not implemented.
- **Device performance:** Auto/Battery/Balanced/Quality profiles, requested 30/60/90/120 FPS caps resolved against reported display modes, and 70/85/100% scene scaling with full-resolution UI. Local Android RAM/PSS, refresh, thermal and power-mode observations drive conservative protections. Diagnostics can record sixty active gameplay seconds after warmup and export a JSON report through Android’s chooser. The app cap and observed frame intervals are distinct from GPU/presentation timing; no phone-specific FPS benefit is claimed. See [ANDROID-PERFORMANCE.md](docs/ANDROID-PERFORMANCE.md) for the policy, limits and physical-phone test procedure.
- **HUD and reports:** four live health/food/water/energy meters, condition warnings, equipment/load/utilities, a shared nearest-action prompt, item cards with real category and weight data, and a radio journal containing only reports already received. Tapping an active broadcast opens its history.
- **Persistence:** one validated, atomically replaced save; autosave every 20 real seconds during play and on app background/close; manual Save and Save and return to menu. Continue restores the role, position, inventory, shelter, clock, weather, world loot history and partial shelf stock, closed/barricaded doors, map discovery and annotations. Older native saves default to their previous open doors and an unexplored map. Previously searched buildings do not gain extra stock when upgrading to the new fixtures; a saved position overlapping newly added furniture moves only to a nearby clear point in the same building. Starting a new run or dying clears the previous run, matching the original game's permanent-death rule.

## Play on Android

Open a successful run of [GitHub Actions CI](https://github.com/lijiabao1998/DefendTaipei-Android/actions/workflows/ci.yml), then download the **DefendTaipei-arm64-debug-apk** artifact. Unzip and install `DefendTaipei-arm64-debug.apk`.

The phone build requires **Android 12 / API 31 or newer**, **arm64-v8a**, and **Vulkan**. It is a testing APK signed with a fixed, explicitly public AOSP development key. Both ABI builds verify the expected certificate in CI. This key is only for development; release signing remains separate. See [TEST-SIGNING.md](docs/TEST-SIGNING.md). The x86_64 artifact is for the CI emulator. Physical-phone screenshots are available and inform the visual work, but hardware frame rate, thermal behavior and compatibility across devices have not been measured.

### Updating an older testing APK

Older CI builds used a different random debug certificate on each runner. This build establishes one stable development signer, so those older installations need a one-time migration. Save in the old game first, then use the [save-preserving ADB update helper](docs/ANDROID-TEST-UPDATES.md) if Android reports a signature conflict. The helper requires an explicit migration flag and a validated local backup before uninstalling anything; failed restores retain that backup for recovery. Future builds with the same fixed signer can install normally with their existing save.

### First run

1. From Home, choose **新遊戲 / New game**, select one of the six survivors and a gender, then choose **下一步 / Next**. Review the survivor, starting supplies and departure information, then press **走進萬華 / Depart** to create the run. Returning from selection or the briefing keeps the existing save; departing replaces it. The following prologue can be skipped. **設定 / Settings**, **操作說明 / Controls** and **製作名單 / About** are available from Home. Set audio, language, the next run’s day duration and performance preferences before departure; settings are also accessible from Pause.
2. You begin at your shelter. Open **避難所 / Shelter** to deposit provisions or inspect construction costs. Grey buttons indicate an unmet cost, range or daily-use requirement.
3. Use **背包 / Bag** to equip a weapon or consume supplies. Guns require matching ammunition. Approach a visible shelf or cabinet and press **互動 / Interact**, then choose **拿 1 / Take 1**, **整疊 / Stack**, or **拿取可攜帶物資 / Take what fits**. Brown crates still mark outdoor drops and unfurnished fallback supplies. Near a doorway, the bag also offers **封門 / Board** or **拆封 / Remove**; the technician starts with the necessary hammer and planks. The blue home fixture supplies one bottle of water, then waits more than 90 game minutes before another refill. It stops working after the water cutoff.
4. Explore and return supplies to the shelter before the next morning. The **日誌 / Journal** records daily events. Sleep is available indoors when tired and safe; nearby danger cancels accelerated time.
5. Use **儲存 / Save**, or **暫停 → 儲存並返回主選單 / Pause → Save and return to menu**. Home's **繼續遊戲 / Continue** button becomes available after a valid save exists.

### Controls

| Action | Desktop | Phone |
| --- | --- | --- |
| Move | WASD / arrow keys; Shift to run | Left movement pad; full deflection runs |
| Attack | Hold left mouse button away from UI | Attack pad |
| Crouch / retreat | Click the corresponding touch control | Crouch pad / retreat gesture |
| Open door / inspect supplies | E | Bottom **互動 / Interact** button |
| Barricade / remove boards | B near a door | Bag's contextual **封門 / 拆封** action |
| Inventory / shelter / barter | I / C / T | Bottom toolbar or panel tabs |
| Survivor map | M; Esc closes | Map button; drag / pinch; annotation tools |
| Sleep / wake | Z, or inventory action | Inventory action |
| Time scale | 1 / 2 / 3 for 1× / 3× / 20× | Inventory actions |
| Pause / close panel | Esc | Pause / Close buttons |
| Camera zoom | Mouse wheel | Two-finger pinch |
| Performance overlay | F3; Settings → Performance → Diagnostics | Three-finger tap; Settings → Performance → Diagnostics |

UI presses remain captured until release, so holding a menu button cannot also fire or move the character. The map, inventory and shelter panels block movement without pausing the world; use Pause when you need to stop the clock. Backgrounding the app pauses simulation. Both orientations are supported; long panels paginate at short screen heights.

## Build on desktop

Install Rust **1.97.1 or newer**. Linux also needs audio/input/window development libraries:

```sh
sudo apt-get install libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev pkg-config
cargo run -p defend_taipei
# Optimized game build:
cargo run -p defend_taipei --release
```

Windows and macOS use the same game crate. Save files use the platform application-data directory; Android uses the app's private internal files directory. `DEFEND_TAIPEI_SAVE_DIR` selects an isolated desktop save directory for development or QA.

## Build the Android APK

Prerequisites: Android SDK platform 34, Android NDK, JDK 17, `cargo-ndk`, and the Rust Android target. CI selects the hosted runner's installed NDK.

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk
# Set ANDROID_HOME and ANDROID_NDK_HOME to your SDK and NDK installations.
cargo ndk -t arm64-v8a -P 31 -o android/app/src/main/jniLibs \
  build --release -p defend_taipei_android
cd android
bash ../scripts/gradle-assemble.sh assembleDebug
# android/app/build/outputs/apk/debug/app-debug.apk
```

The shared Rust library is packaged with Android GameActivity. The activity follows the orientation sensor. MSAA and shadow maps are disabled on Android. No external-storage permission is needed for saves.

## Verification

```sh
cargo fmt --all -- --check
bash scripts/test-gradle-assemble.sh
python3 -m unittest discover -s scripts/tests -p 'test_*.py' -v
CARGO_PROFILE_DEV_DEBUG=0 cargo clippy --workspace --all-targets -- -D warnings
CARGO_PROFILE_DEV_DEBUG=0 cargo test -p defend_taipei --lib
```

CI also builds both APK ABIs, verifies their development signatures and runs six end-to-end checks:

- **Native GPU QA:** `cargo run -p defend_taipei -- --qa` renders actual Bevy screenshots: Home at three sizes, portrait/landscape and paused audio/game/performance settings, survivor/departure pages, prologue, streets, closed/open/boarded doors, furnished shops and homes, selective/partial/empty loot panels, survivor maps in both orientations, rain/night/blackout, warning HUD, shelter, inventory, radio history, pause, barter and Save → Menu → Continue. It uses an offscreen GPU texture, does not require a display server, and fails if the restored run differs. CI requires a successful `qa-report.json` with every expected capture and assertion, including native UI versus scaled-scene dimensions. Outputs are in the **native-qa** artifact. Weather and barricade tools use explicit fixtures; door and pickup actions use actual gameplay messages. The run checks material conservation, exact shelf/bag contents and persisted barricades/leftovers. On Linux, install `mesa-vulkan-drivers` for software Vulkan when no GPU is available. `DEFEND_TAIPEI_QA_OUT` selects the output directory. This uses a fixed simulation timestep and is **not a performance benchmark**.
- **Native audio output:** `scripts/audio_capture.py` captures the running Bevy application from an isolated PulseAudio monitor. Thirteen scenarios exercise recorded and procedural effects, independent buses, silence, title auditions, rain, live mute/pause/focus-loss transitions and resume. Every bundled clip is fully decoded and checked; WAVs and measured PCM windows are published in **native-audio-qa**. Asset readiness and dispatch counters alone cannot satisfy the audible-output assertions.
- **Android gameplay smoke:** the **emulator-smoke** artifact contains logcat, screenshots and actual private save JSON from touch-driven map marking, held-pointer capture, deposit, prayer, disabled construction, save, menu, continue and map/panel rotation checks. `scripts/emulator-gameplay-smoke.sh` clears the disposable emulator's app data and refuses to run on a physical phone. Its indoor extension uses checked position/stock fixtures, then exercises actual touch pickup, full/partial/empty stock, barricade spending/refunds, door opening and cold-launch Continue against real private save bytes. A controlled outage/clock/RNG fixture verifies actual refrigerator conversion, unchanged dry goods and ordinary shelves, and no re-roll after cold Continue. A provenance JSON identifies the injected fixtures and suppressed patrols; these controlled stages do not test walking to the locations or combat.
- **Android audio/settings smoke:** actual short taps and slider drags on the emulator exercise all audio buses, mute, reset, language, next-run day duration, rotation, cold restart and ordinary same-signer replacement. Nine PCM captures must match audible/silent expectations, while the existing game save remains byte-for-byte unchanged. A separate host preflight requires an actual 48 kHz stereo emulator stream on the dedicated 48 kHz capture sink before gameplay checks begin; it does not replace PCM verification. Slider endpoints require exact zero/full-scale readback, and the effective Android media-volume range is recorded. Preferences JSON, routing diagnostics, layout logs and four settings screenshots are included in **emulator-smoke**. This is emulator-output verification, not physical-phone loudness or latency measurement.
- **Android performance smoke:** actual taps configure a fixed profile/cap/scene scale, verify full preference restoration after a cold launch, rotate the paused settings UI, and resume the saved game for a real sixty-second active-gameplay capture. Report assertions check coherent measured intervals, current device snapshots and native UI/scaled-scene sizes without an emulator FPS threshold. The Android chooser opens with a report-only content URI; the test does not choose a recipient or send the file. Where supported, injected emulator thermal status checks protection and recovery. Reports, PNGs and runtime evidence are in `emulator-performance-qa/` inside **emulator-smoke**. This follows audio verification and precedes the independent APK update checks.
- **Android update smoke:** after gameplay, `scripts/emulator-update-smoke.sh` uses the emulator's actual save to verify an ordinary same-signer update, migration to an ephemeral test signer, and migration back to the delivered signer. Every step must preserve the exact original save bytes. Verified backups and evidence are included in **emulator-smoke**; the temporary signing key is deleted and is never uploaded. The helper also has mocked failure-path regressions covering invalid APKs/saves, device ambiguity, backup failures and recovery after failed installation.

These are verification gates; use the Actions result for the revision you are installing to check their outcome. The Gradle helper retries only recognized HTTP 429/502/503/504 dependency-download failures, up to three attempts, and preserves build failures. To launch a directly built desktop binary, set `BEVY_ASSET_ROOT` to the repository's `game` directory. The usual `cargo run` path supplies the crate's asset root automatically.

## Code map

| Path | Responsibility |
| --- | --- |
| `game/src/map/`, `game/src/chunk/` | Wanhua data, geometry, collision, streaming and mesh utilities |
| `game/src/combat/`, `game/src/human/`, `game/src/input/` | Combat, character rigs and input |
| `game/src/inventory.rs`, `game/src/inventory/catalog.rs` | Item behavior and generated source catalog |
| `game/src/survival.rs` | Shelter rules and daily settlement |
| `game/src/worldplay.rs` | World interactions, loot, vitals, weather and utility integration |
| `game/src/game_flow.rs`, `game/src/story.rs`, `game/src/ui/` | Campaign flow, story data and responsive screens |
| `game/src/ui/title.rs`, `game/src/ui/portrait.rs`, `game/src/ui/layout.rs` | Home, survivor selection, briefing, cached rig portraits and screen geometry |
| `game/src/save.rs`, `game/src/qa.rs` | Save lifecycle and native visual QA |
| `game/src/audio.rs`, `game/src/audio_qa.rs`, `assets/audio/` | Bounded playback, Web-derived recordings and actual output fixtures |
| `game/src/settings.rs`, `game/src/ui/settings.rs` | Independent preferences and responsive settings |
| `game/src/performance/`, `game/src/device.rs`, `game/src/perf.rs` | Device policy, scene scaling, local diagnostics, frame capture and overlay |
| `game/src/worldplay/aging.rs` | Persisted opened-container spoilage |
| `game/tests/fixtures/`, `tools/generate_*.cjs` | Original TypeScript reference fixtures and generators |
| `mobile/`, `android/`, `.github/workflows/ci.yml` | Android entry point, APK packaging and CI |

## Remaining Web-port work

The current playable bounds cover the Wanhua core. Other Taipei districts, the outside-core Dongyuan trade location, vehicles and armor combat, full teammate/rescue systems, metro/inter-floor traversal, and the boat-informant path are not implemented. District-reclamation events have a handler but no complete native capture loop. The active extraction route is the timed helicopter site; instant keyboard evacuation shortcuts have been removed.

Container/furniture and patrol placement use a native deterministic stream, not the Web renderer's interleaved random calls. The native per-fixture pickup panel supports selective items and partial stacks; its layouts and procedural furnishings differ from the Web renderer. Doors support open/close and barricades, but not damage or enemy door-opening AI. Building locks are not defined by the pinned Web source; its lockpicking skill belongs to the unported vehicle system. The map does not yet include liberation overlays, radio-derived enemy intelligence, or item-triggered district reveals. Native settings cover audio, language, day length and the device-performance controls described above. These profiles are implemented for the native renderer; vibration, custom bindings and teammate selection remain unported. Some weapon silhouettes and environmental visuals remain simplified. The visual work does not establish full Web parity or measured phone FPS.

Map data attribution: © OpenStreetMap contributors, ODbL. The bundled Noto font license is included under `assets/fonts/`. Audio CC0 attribution and source checksums are included in [assets/audio/CREDITS.md](assets/audio/CREDITS.md) and its manifest.
