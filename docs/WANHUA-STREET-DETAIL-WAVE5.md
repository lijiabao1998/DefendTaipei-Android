# Wave 5 street detail and material presentation

This is the street presentation part of Wave 5. Vehicle rules and gameplay belong to the separate native vehicle module. The original reference remains Web v24.42.0, commit `b9b0a2736642b37dd45925a4719dfef08b8f36b4`.

## Inherited work and actual additions

The prior native waves already include ceramic facades, stained and peeling plaster, rust, weathered tin, facade meters/pipes, short sagging wires, doorway plants, commercial panels/signs/canopies, road markings, and detailed static parked vehicles. These are retained and are not counted as new Wave 5 features.

Wave 5 adds utility crossarms, ceramic insulators, transformer housings, longer admitted overhead cable spans, powered traffic signal heads, distinct chipped ceramic corners/mineral joints and rusted sheet fasteners. It also changes the visual form of eligible existing food-shop kitchen fixtures to stainless pushcarts with empty crate stacks and a recessed refuse bucket. The actual saved contents appear on the counter and disappear when removed. Existing named temple entrance flanks gain red lanterns and lacquer/gold trim.

No new standalone ground cart, bin, transformer cabinet, temple building, supply container or point light is created by this presentation layer. This preserves the existing collision and loot contract instead of placing solid-looking uncollidable objects across a route. The market carts remain the same accessible kitchen fixture, with the same stable ID and loot kind. Temple accents use existing names ending in 寺, 宮 or 廟; toilets, libraries, ruins and bell towers do not match that rule.

## Source and native adaptation

- Web `src/map/decay.ts`, utility-pole section: crossarms, insulators, transformer/junction equipment and sagging wires.
- Web `src/map/alley.ts`, night-market cart section: stainless pushcart silhouette, wheels, counter and supplies.
- Web `src/map/roads.ts`, `signal`, `update` and `green`: signal housing, lens colors, shared materials and exact 30-second cycle.
- Web `src/map/clutter.ts`, temple and market furniture: red/gold shrine and steel market furnishings.

The native core's original map is kept intact. Equipment uses existing curb lamp anchors, extends their upper shafts, and leaves all added parts above 3.7 m. This is a deterministic native adaptation of the source visual vocabulary, not a claim that the added street furniture has surveyed positions or exact Web random placement. The original lamp models, all street props, doors, stored shop-sign assignments, fixtures and saved identities remain unchanged.

Utility candidates use a private integer hash over stable prop index and coordinate bits. Full crossarm/transformer envelopes reject nearby buildings. Cable spans connect matching curb orientations, remain no longer than 35 m, and are sampled conservatively against building footprints/heights. A destination must itself have a utility assembly, and one source owns each emitted span. Equipment is batched into existing metal and vertex-color materials, emitted at Near/Mid and omitted at Far. Cache use is limited to the immutable global world; independent test worlds rebuild their own plans.

Signals select safe original curb lamps close to already admitted junction approaches. They do not create new pole collision or change road/vehicle right-of-way rules by inference. Their six shared lens materials add no light entities, leaving the Wave 4 pool at six city spot lights.

## Signal behavior

| Phase in real seconds | Axis A | Axis B |
|---|---|---|
| 0 to under 12 | Green | Red |
| 12 to under 15 | Amber | Red |
| 15 to under 27 | Red | Green |
| 27 to under 30 | Red | Amber |

Power loss sets all six material emissions to zero immediately. Power restoration uses the current phase. The phase advances only during an active, unpaused session. The original source `green` helper permits axis A before 13 seconds and axis B from 15 to under 28 seconds; that one-second amber allowance is preserved and separately tested. It returns permission when power is absent, matching the source.

Desktop renderer QA may set `SignalPhase.qa_fixed` to hold the same signal phase across controlled lighting A/B and blackout/restored captures. That override is absent from Android builds and never changes campaign time, power or the production light pool.

## Fixture compatibility

Pushcart treatment applies only to an existing `Kitchen` fixture whose original building sign is one of the source food-sign cells (0, 2, 6 or 14). The body remains inside the kitchen's original 1.7 × 1.0 × 0.65 m collision envelope. All fixture planning, collision segments, labels, approach points, inventory stock, aging, search and saved IDs remain unchanged.

Crate shells are empty structural detail. Up to five visible products are derived from the actual deterministic or saved stock ledger; an empty stock record yields no displayed products. Existing normal shelf/fridge/medicine stock rendering remains unchanged. The rendering code does not grant or reroll items.

## Named temple presentation correction

The first actual Temple GPU capture selected original building 128, 台北天后宮, but its inherited generic shop fascia and striped canopy made the entrance read as a shop. Named temples now override that inherited presentation with plain dark plaques and vermilion/gold surfaces. The existing atlas contains no original temple names, so no invented lettering is added. Exact names remain in map data and QA evidence; this is an entrance treatment, not a reconstructed historic temple building.

Main, projecting and secondary plaques sample a verified blank background corner within their existing atlas cell. Fallback signs and any legacy vertical sign are also unbranded. This keeps every original stored sign index, geometry envelope, vertex/index count and material slot; ordinary shops retain their complete text. Inherited commercial family/striped-layout inventory counts therefore describe the unchanged source plans. The separate Wave 5 temple override inventory records the effective religious-frontage presentation.

Lanterns under an admitted canopy hang from its two flanks with their upper rods reaching the existing canopy. Their bodies clear the plaque edges and remain above the walkable doorway height. The original road/building envelope checks still reject a lantern over another lot or carriageway. Entrance trim stays on the original wall flanks. No ground obstacle, doorway, fixture or loot identity changes.

## Cost and validation

`cargo run -p defend_taipei --example exterior_workload -- --check` retains the original spawn and 04a street positions and all previous cumulative comparisons. The Wave 4 baseline in `street-detail-wave4-baseline.json` comes from the digest-verified CI81 native archive and records the source, tested merge, main tree, archive and workload digests.

The acceptance ceiling is at most 3% additional planned resident geometry/index payload and 2% additional submitted material meshes over Wave 4, with no baseline reductions used to conceal removed geometry. Individual utility assemblies are bounded to 2,400 vertices and signal assemblies to 700. Only measured temple ornament geometry may change the inherited lazy-detail geometry. World identity, Wave 1 architecture, Wave 2 commerce and Wave 3 street inventories remain exact. Texture dimensions are unchanged; six small shared signal materials are the only new material handles.

The report includes exact utility, span, transformer, signal and cart counts; site fingerprints; source geometry inventories; and real world QA targets for utilities, signals, carts and temples. Submitted CPU mesh payload is not measured GPU memory, draw calls, phone FPS, thermal behavior or battery consumption. Dynamic interior runtime meshes, including the replacement cart bodies, remain explicitly outside that exterior resident accounting. New driveable vehicles, their wheels/turrets, bike docks and enemy armored vehicles are also separate runtime meshes; their costs belong to `vehicle_workload`, not this 3% street-detail allowance. Existing static parked props remain included in both the baseline and current exterior totals.

Meaningful regression tests cover the original signal boundary/blackout rules, anchor and cable validity, deterministic planning, finite bounded overhead geometry, temple doorway clearance, and cart body/stock containment and empty-stock removal. Root integration runs the full native/Android gates and captures the actual renderer.

The initial local workload pass reports 376 utility anchors, 123 transformer housings, 120 admitted cable spans, 40 signal heads (13 axis A / 27 axis B), 79 existing fixtures with cart treatment, and 7 named temple frontages. Maximum assembly vertices are 1,038 per utility, 282 per signal and 1,188 per cart.

The recorded local `wave5-exterior-workload.json` gives the following planned resident totals. Every declared budget and inherited identity check passes in that report. These measurements remain subject to the final CI artifact for the delivered commit.

| Position and metric | Wave 4 baseline | Wave 5 street detail | Added | Change |
|---|---:|---:|---:|---:|
| Spawn vertices | 3,040,724 | 3,117,746 | 77,022 | 2.533% |
| Spawn triangles | 2,151,588 | 2,202,204 | 50,616 | 2.352% |
| Spawn material meshes | 3,354 | 3,372 | 18 | 0.537% |
| Spawn mesh attribute/index bytes | 171,773,808 | 176,078,256 | 4,304,448 | 2.506% |
| 04a vertices | 3,040,315 | 3,108,289 | 67,974 | 2.236% |
| 04a triangles | 2,146,909 | 2,191,741 | 44,832 | 2.088% |
| 04a material meshes | 4,309 | 4,324 | 15 | 0.348% |
| 04a mesh attribute/index bytes | 171,698,028 | 175,498,764 | 3,800,736 | 2.214% |

The unchanged original-world fingerprint is `3b33c3a798c7decd`; the utility and signal site fingerprints are `958ef041eba2f4dc` and `305592f07ab5f7f1`. Whole-map utility assemblies total 206,400 vertices / 133,656 triangles, and signal assemblies total 11,280 vertices / 8,160 triangles before chunk residency is applied. Whole-map inventories are not additional resident totals. Neither existing measurement position contains a new near temple ornament, so the report also audits temple assemblies separately rather than implying those two views cover their cost.

## Actual renderer evidence contract

The desktop `qa/wave5_exterior.rs` scenes select real production utility/cable plans with an actual nearby signal head, a food-sign kitchen fixture with actual nonempty stock, and an exposed named temple entrance. They use the normal isometric camera and gameplay zoom range; they never create independent display props. The temple selector rejects intact arcades and positions that would trigger the normal facade cutaway, so the selected ornaments must remain on the real visible exterior.

Utility and temple assertions bind their feature vertices to the production chunk/facade material batches, compare the retained metadata from the actual GPU upload, require those entities in the main camera's visible set, and project the complete subject inside the viewport. Temple evidence also requires the actual fascia upload to have the constant blank UV coordinates and reports `generic_shop_name_suppressed`, `displayed_shop_label: null`, and the exact original temple name. The retained flat-UV field exists only in desktop builds. Market assertions compare actual room and stock upload metadata with the exact production assembly and current saved/preview stock, require both body and goods in the main view, and verify the normal building roof cutaway. `InteriorGeometry` is a separate metadata component, so these checks do not alter previous `MapMeshGeometry` totals.

These are upload, ownership, main-view and framing checks with conservative source-envelope screening. They do not label counts alone as pixel visibility or claim complete per-pixel occlusion testing. The raw GPU screenshots still require visual inspection before delivery.
