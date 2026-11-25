# Wanhua frontage, rain and powered night lighting

This wave extends the previous street-wear work with everyday frontage details and a
visible response to the existing clock, weather and electrical supply. The reference
remains Web **v24.42.0**, commit
[`b9b0a2736642b37dd45925a4719dfef08b8f36b4`](https://github.com/lijiabao1998/defendtaipei/tree/b9b0a2736642b37dd45925a4719dfef08b8f36b4).

## Reference map

| Web source | Native implementation |
| --- | --- |
| [alley.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/alley.ts): shallow shop awnings, service cables, domestic plants and bounded pools of nearby lights | `map/detail.rs`: frontage-life geometry; `map/atmosphere.rs`: local light pool |
| [litwin.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/litwin.ts): night/power dependent occupied windows | Existing native window panes and lamp material now follow the gameplay clock and power supply |
| [rain.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/systems/rain.ts): local visible rain and wet ground | `map/atmosphere.rs`: one reusable rain mesh and shared surface wetness |
| [lamps.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/core/lamps.ts): bounded street-lamp rendering | Existing streamed native lamp meshes plus four nearby shadowless spill lights |

## Frontage life

At the existing near-detail distance, a building can receive at most two decorated
solid frontage spans. Short sagging service cables attach to the same building; they
do not jump between independently streamed chunks. Exposed shop houses can carry
shallow worn green, red-brown or grey corrugated awnings, with a drip edge and diagonal
brackets. Buildings with an existing arcade ceiling do not receive a second ceiling.

Small terracotta or grey-green pots with long leaves sit against the wall only when
there is a clear walking strip outside them. Placement rejects the carriageway,
another building's footprint, nearby walls and the enlarged doorway corridor used by
the moving doors. Pots are intentionally distinct from the brown searchable containers
and blue water fixture; these are passive visual details, not new loot interactions.

The layer uses the existing vertex-colour material and, for awnings, the existing tin
material. Leaves join the vertex-colour batch rather than introducing a foliage draw
for every building. Geometry is bounded by two spans and tested below 2,200 vertices
per building. Existing near-detail unloading and building cutaway also remove these
details. No new global collision segments are introduced by frontage dressing.

## Time, power and rain

`map::atmosphere::AtmospherePlugin` owns scene lighting after the gameplay `WorldSet`.
`worldplay::update_environment` continues to update the combat and audio environment;
it no longer writes the sun or global ambient light. This avoids competing lighting
systems.

- Dawn blends from 05:30 to 07:00 and dusk from 17:30 to 19:30. These are rendering
  transitions; patrol and survival rules retain their existing gameplay night times.
- The existing shared window/lamp and horizontal/vertical sign materials become
  emissive with darkness while electricity is available. Cutting power immediately
  disables that emission and all pooled spill lights. Parked vehicles' headlamp
  housings use passive vertex-colour geometry, so municipal power does not switch
  every parked car's headlights on.
- A fixed pool of **four street lights and two sign lights** is repositioned to nearby
  actual lamp/sign locations every half-second. Selection is deterministic, limited
  to 32 metres, and adds no entities as the player travels. Both shadow maps and
  contact shadows are disabled. The lamps add local illumination, not new gameplay
  sight or stealth rules.
- The existing saved weather amount controls visible rain. One reusable mesh draws
  at most **240 short streaks**, refreshed at up to 30 Hz. Positions under a building
  footprint, including an arcade, are excluded. The mesh is hidden when there is no
  rain or no active run. Pausing freezes its fall animation.
- The existing saved wetness darkens asphalt, paving and curbs and lowers their
  roughness. Rain reduces daylight intensity. There are no screen-space reflections,
  volumetric fog, per-drop entities, rain shadows or additional GPU feature requirements.

Rain is a local presentation effect, not an exact reproduction of the Web particle
stream. The six spill lights are deliberately shadowless and do not establish correct
light occlusion through every wall. This work also does not add the Web alley's entire
market/cart/temple scene, point-light pool sizes, puddle simulations or per-window
emissive atlas. Existing geometry and shared materials keep this bounded on mobile;
measured physical-phone performance remains a separate check.

## Verification

Unit checks cover smooth day/dusk/night transitions, immediate power-off behavior,
rain/wet daylight reduction, deterministic and range-limited light selection, bounded
rain geometry and roof exclusion. The frontage check builds real nearby world geometry
and checks mesh budgets, carriageway separation and the entire moving-door corridor.

Native QA can inspect `AtmosphereState` for actual submitted `active_lights` and
`rain_streaks`, along with `night`, `power` and `wetness`. Dedicated day, rain, powered
night and blackout captures validate presentation with the actual renderer. See the
CI run for the revision being installed for final pass/fail results.
