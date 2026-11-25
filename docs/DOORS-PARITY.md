# Interactive native entrances

Reference: [Web `src/map/doors.ts` at v24.42.0](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/doors.ts). Native implementation: [`doors.rs`](../game/src/doors.rs), [`map/collide.rs`](../game/src/map/collide.rs), [`worldplay.rs`](../game/src/worldplay.rs) and [`save.rs`](../game/src/save.rs).

## Player-facing behavior

- Near an entrance, the contextual action becomes **開門 / Open door** or **關門 / Close door**. The same reachable target drives the HUD, brass ground ring and Interact action. A closer reachable cabinet or water fixture takes priority; a closed door prevents reaching supplies through the leaf.
- New games begin with closed street entrances and an open home exit. Red home gates and green shop leaves swing outward over 0.4 seconds. A door cannot close while the player or a living NPC occupies its aperture.
- A closed entrance blocks player and NPC movement, sight and chest-height shots. Door height remains 2.4 m. This is a deliberate native extension: the pinned Web leaf code blocks only the player, leaving soldiers' original pathing unchanged.
- Doors are identified by the existing building IDs. Opening, closing, streaming out and returning do not change their state. Save and Continue preserve the closed IDs. Older native saves lacking this field retain their formerly open passages; a restored character already occupying an aperture keeps their exact position and that aperture opens safely.

## Rendering and collision bounds

Static frames, sills and address plates remain part of the building chunks. Live leaves use four shared meshes and two shared materials; at most 24 doors within 48 m are present. They are created only when their building's Near or Mid owner chunk exists, fade with its walls, and shorten with the interior cutaway. Supporting Mid owners prevents a large footprint's distant centroid from hiding a nearby entrance leaf. LOD changes never change collision state. The closed-door collision index uses 8 m cells shared by cheap `Obstacles` clones, including the boxed combat world; queries do not scan every city door.

Movement uses circle substeps so a long frame cannot jump over the closed aperture. The ring and leaf swing have no separate gameplay collision. The state becomes open or closed immediately when the interaction succeeds.

## Boundaries and checks

This subset does not add locking, lockpicking, barricades, damage to doors or enemy door-opening AI. New art is procedural mesh geometry. Existing world planning, IDs, item RNG and save version stay unchanged.

Unit checks cover shared movement/LOS/shot collision, large-step crossing, open-leaf passage width, occupied-threshold refusal, target selection on both sides, water access behind a closed arcade door, bounded visual geometry, saved state independent of render slots, legacy save fields, invalid IDs and safe Continue at a threshold. End-to-end screenshots and Android smoke results should be read from the CI run for the installed revision.
