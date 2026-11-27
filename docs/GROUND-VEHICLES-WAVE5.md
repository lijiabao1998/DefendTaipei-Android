# Wave 5 ground vehicles

## Source and scope

The reference is `lijiabao1998/defendtaipei` Web v24.42.0, commit
`b9b0a2736642b37dd45925a4719dfef08b8f36b4`.

Two different Web systems must not be confused:

* `src/systems/vehicles.ts` produces locked or stripped street scenery. Its
  `enter()` deliberately returns false. The native world's original 431
  scooters, 22 cars, 9 taxis and 7 trucks retain their existing identities,
  position, appearance and static collision.
* `src/systems/drive.ts` produces a separate playable fleet. Wave 5 adds that
  system: scooter, motorcycle, electric scooter, delivery scooter, Smile Bike,
  basket bicycle, mini truck, taxi, sedan and van. Bus and garbage truck are
  non-driveable cover, as their source `drive` flags specify. Captured and
  abandoned ZBL-08, ZBD-05, ZTD-05 and Mengshi vehicles use the same drive rules.
* The UH-60 definition in `drive.ts` is a flying vehicle and is outside this
  ground-transport milestone. This change does not claim flight or passenger
  system parity.

All 16 ground specifications are directly checked against executed original
TypeScript, including speed, acceleration, steering, noise, fuel capacity/burn,
dimensions, electric/pedal flags, seat height and nominal cargo. The original
uses cargo only as metadata and for teammate seat counts; this wave does not
invent a vehicle storage or van sleeping system.

## Playable behavior

The same directional input as walking steers the vehicle. It accelerates toward
the requested direction, reverses when commanded behind it at less than 2 m/s,
coasts/brakes when the stick is released, and loses speed when tires or engine
are damaged. The courier's riding multiplier applies only to two-wheelers.
Fuel burn and engine noise follow the original tuning; noise reaches the
existing soldier investigation system. Parked electric scooters trickle-charge
only while grid power is available. A courier starts with an owned, healthy
delivery scooter with four litres of fuel beside the start, using a nearby
clear placement if the original offset is obstructed in the native world.

Interact follows the source's useful transaction order: add available fuel,
perform available repairs, then attempt boarding. A successful transaction
requires another interaction before boarding. Unavailable or unnecessary
maintenance does not trap the player in an endlessly repeated full-tank message.
The inventory's gasoline, hose, patch and car-battery actions target that exact
item and recheck distance, wall visibility, vehicle identity and inventory
contents at execution time. An inventory action can complete while its own
inventory panel is open; driving, boarding and mounted combat remain blocked
by modal panels and pause.

Locked ignitions require the handyman's lockpicking skill. Wiring takes 2.5 s
with repair skill, otherwise 4 s; remain beside the vehicle and interact again
after completion. Moving away cancels the work. Docked bikes require a hammer
or pipe and release with the original 26 m investigation noise. Empty fuel,
flat starter battery and broken engine block entry. Engine tools are retained;
tire patches and replacement batteries are consumed exactly once when needed.

The driver remains at ground level for camera, map and world interactions;
the character renderer handles seating and cabin visibility. Exit checks both
sides and the front/rear for a safe, reachable standing position, so a blocked
side cannot teleport the player through a wall.

## Native corrections and compatibility

The original renderer interleaves random calls in geometry and gameplay, and
its `sort(() => random - .5)` shuffle is engine-dependent. Native placement
uses explicit deterministic shuffling, source road-width/distance rules and
collision/doorway clearance checks. Exact Web spawn coordinates and RNG
streams are not claimed. Original static road/building/prop data are unchanged.

Live civilian and hostile armor hulls share a collision provider with walking,
NPC movement, line of sight, projectile-height queries and cover detection.
Publishers own separate groups; resetting one does not remove another group's
cover. The provider is shared by cloned combat-world handles. Driving ignores
its own hull and uses short travel substeps; its rectangular perimeter, rather
than only its center, rejects walls, stationary scenery, other vehicles and
map edges. Projectile rays originating in a vehicle can leave that hull.

Three intentional bounds differ from the permissive Web inventory code:

1. Siphoning obeys the existing native 15 kg pickup limit and leaves uncarried
   fuel in the tank.
2. Gasoline is a whole-litre item. Refueling transfers only whole litres that
   fit, preserving fuel and inventory without Web's `ceil` over-capacity edge
   case. A fractional remainder below one litre is considered full for that
   transfer.
3. Driving and safe exit also respect the native world's live doors, scenery
   and map edges. A damaged engine below the entry threshold cannot continue
   accelerating after it has failed.

Handheld AT4 ammunition has one additional inventory-conservation correction:
firing consumes exactly one carried tube and clears the equipped weapon. Any
remaining tubes stay in the pack and require re-equipping before another shot.
The pinned Web implementation deletes the entire `at4` inventory key after a
shot, which would discard every tube in a stack. Native ECS regressions cover
one-tube consumption, the empty-handed interval and explicit re-equipping.

The fleet serializes stable ID, kind, position, heading, speed, fuel, starter
battery, tires, engine, key, dock state, occupied vehicle, turret direction and
mounted-gun ammunition/timers. Validation rejects nonfinite/out-of-range data,
duplicate IDs, impossible occupancy and invalid weapon loadouts. Missing fleet
data in an old native save initialize the deterministic fleet; existing
inventory and scavenging data are preserved. Hotwiring, held input and effect
timers are transient and reset on load.

## Hostile armor and captured weapons

The four hostile patrols and three burned wrecks come from the pinned
`src/systems/armor.ts`. Mengshi patrols have 40 HP and move at 7 m/s; the other
three types have 100 HP and move at 4.5 m/s. Routes reverse at their endpoints,
with the source four-second stop. Spotting, the six-second alert stop, nearby
infantry investigation and mounted machine-gun cadence/damage retain the
original rules. Small arms ricochet; an anti-armor hit that leaves 1–30 HP
causes the crew to abandon the vehicle. A hit that reaches zero destroys it.
Capture transfers the same kind, position and heading exactly once into the
playable fleet with 40% engine condition. The abandoned hostile hull stops
rendering/colliding when its playable replacement takes ownership.
The original early engine warning appears once below 60 m and rearms only
beyond 100 m. It uses the pinned wording and yields to a new spotting or combat
message; its transient heard flag resets on load or a new game.

The exact ground loadouts and turret speeds are:

| Vehicle | Mounted weapons, in switching order | Traverse speed |
| --- | --- | --- |
| ZBL-08 | 30 mm autocannon; 7.62 mm coaxial MG | 1.1 rad/s |
| ZBD-05 | 30 mm autocannon; 7.62 mm coaxial MG | 1.1 rad/s |
| ZTD-05 | 105 mm gun; 7.62 mm coaxial MG | 0.7 rad/s |
| Mengshi | 12.7 mm heavy MG | 1.8 rad/s |

| Weapon | Ready capacity | Nominal reserve | Reload | Shot interval | Infantry damage | Anti-armor damage | Range / splash radius |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 7.62 mm coaxial MG | 250 | 1,750 | 6 s | 0.086 s | 58 | 0 | 70 m / 0 m |
| 30 mm autocannon | 200 | 200 | 9 s | 0.2 s | 150 | 6 | 90 m / 2.4 m |
| 105 mm gun | 8 | 37 | 22 s | 7.5 s | 400 | 70 | 120 m / 8 m |
| 12.7 mm heavy MG | 60 | 240 | 7 s | 0.11 s | 110 | 1 | 85 m / 0 m |

These are the original game's tuning values. Newly spawned vehicles start
with full ready ammunition and 60–100% of nominal reserves, as in `drive.ts`.
The original per-weapon heating, cooling, jam recovery, automatic empty reload,
manual partial reload and round conservation are checked against executed
`avgun.ts`. Reload success is logged and sounded only when it actually starts;
completion is emitted only when the timer reaches zero and rounds are loaded.

A shot follows the current barrel direction while the turret traverses toward
aim. Static walls and intervening cover stop the shot, nearer infantry take
precedence over a hull behind them, and the selected hull ID receives the hit.
That identity matters when hull centers are close together. Splash checks the
path from just in front of the impact, so a wall shelters soldiers behind it.
Dynamic hull queries allow a shot to leave its carrier and reach an occupied
target; a hull between shooter and target still provides cover.

An occupied armored vehicle passes through exactly 10% of vehicle/combat
health damage and clears bleeding. The shield begins before driving impacts
and ends before the death check, including a raw hit larger than the driver's
remaining HP. On foot and in civilian vehicles the damage stays unshielded.
Hostile ramming uses the source radius `3 + vehicle length × 0.3`, removes
0.5 engine condition, sets tires to 0.2, stops the vehicle, and deals 35 HP on
two wheels or 15 HP in a car before shielding. The open-space shove is half
the separation vector. Native corrections bound one impact to each contact,
require separation plus a one-second cooldown before another impact, and
sweep the entire displaced hull against walls and unrelated vehicles.
Contact latches and shield snapshots reset on a loaded game.

## Engine audio and workload accounting

The armor engine is a real looping ambience voice, generated from the pinned
38 Hz sawtooth plus 57 Hz square oscillator recipe through a 220 Hz low-pass
filter. One shared voice follows the nearest active hostile armor within
90 m; the occupied, fueled, working armored vehicle also supplies that voice
as native driving feedback. It uses the existing ambience gain, master mute,
pause/world-scope and foreground lifecycle. Civilian drive noise remains the
source's AI investigation signal; `drive.ts` supplies no civilian engine-audio
recipe. The engine has five dedicated real-output QA cases, alongside the
existing 13 baseline cases: audible output, zero ambience, live mute, live
pause and live background transition. PCM captures, rather than a started-event
counter, determine audible-versus-silent results.

`vehicle_workload` counts the sixteen-kind playable renderer's mesh pool,
the hostile renderer's separately uploaded four-kind pool, and the actual
shared docking-post mesh. The duplicated armor meshes are included in the
combined attribute/index payload. Distance-eligible instances are reported
separately for the 220 m playable/dock radius and the hostile renderer's 120 m
spawn / 140 m retention radii. These counts add to `exterior_workload`; they
are not measured GPU memory, draw calls, FPS, thermals or process memory.

## Reference and regression checks

`tools/generate_vehicle_reference.cjs` requires the exact pinned Web checkout.
It executes unmodified `SPEC` and `DriveSystem.update` functions with renderer
dependencies mocked. Its checked-in fixture contains all 16 ground specs and
141 motion snapshots covering acceleration, steering, braking, courier skill,
damaged tires, reverse, empty fuel, impacts and pedal transport.

Native tests compare those values and additionally cover deterministic fleet
placement, six abandoned armor vehicles, unchanged static-world counts, fuel
conservation, exact-item repairs, required wiring time, dock tools, invalid
save rejection, shared collision/visibility/cover and safe exit. Native and
Android gameplay captures are separate runtime evidence; these pure tests do
not establish renderer FPS or physical-phone performance.

Armor ECS regressions additionally drive the actual hostile-fire, spawn,
ramming, mounted-fire and capture systems. They check 10% protected damage
against real hit messages, unprotected civilian/on-foot damage, first-wall
and blast obstruction, exact front-hull identity, capture only once, bounded
ram contact damage and a full-hull shove stopped by a static wall.
