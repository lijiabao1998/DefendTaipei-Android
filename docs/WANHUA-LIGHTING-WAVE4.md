# Wanhua exterior wave 4 — directional city lighting

## Scope and source

This wave follows the completed architectural, commercial and street-object waves.
It improves the relationship between existing light fixtures and the surfaces they
illuminate: road-facing street lights, outward-facing signs and covered arcade
walkways. Existing occupied-room panes have their own softer glow response.

The baseline is merged main `3418a19b2b1e2a4c0843ba59faead5a00af672a6`
([PR #24](https://github.com/lijiabao1998/DefendTaipei-Android/pull/24)).
Its source head `00ddb75fa4fee7f9dba9b8d6d258907b3acd1e63` and tested synthetic
merge `526f1873a634d9980947209dc6cbbd50701a7a9d` share the same tree
`739855a63872a46b1f174a2570d057d718c9dcdb`. The verified
[CI79](https://github.com/lijiabao1998/DefendTaipei-Android/actions/runs/45929516204)
native artifact is the geometry and visual baseline. Its original ZIP is artifact
`15168938357`, SHA-256
`7d99d776c8372733d7e9a9694e529cc60e06b4d4b2a99fa0a2d76de17bdd3577`.
`lighting-wave3-baseline.json` records the extracted workload and its provenance.

This is a rendering change. It introduces no new gameplay light switches, stealth
rules, collision, save fields, driveable vehicles or additional districts.

## Original fixtures, bounded direct light

The preceding version already planned 2,010 street-lamp sites and 603 sign sites.
There are also 464 buildings with arcade geometry; their existing ceiling-fixture
bays now provide arcade light candidates. A site is a deterministic render plan
derived from an original prop, building sign or arcade bay. It is not an additional
light entity or a newly spawned gameplay fixture.

Street/sign fixtures without a valid exterior receiver are omitted only from the
direct-light catalogue. Their original models and material emission remain. The
workload reports original fixture counts and admitted spill sites separately.
The measured catalogue contains **3,335** sites: **2,008 street lights, 506 signs
and 821 arcade fixtures**. The two remaining street lamps and 97 remaining signs
retain their original models/emission but have no admitted exterior spill receiver.

The city still allocates exactly **six reusable local light entities**:

| Pool | Allocation | Source and direction |
| --- | ---: | --- |
| Street | 4 | Original lamp heads, aimed down toward their road-side receivers |
| Frontage | 2 | Existing sign faces and arcade ceiling fixtures, aimed outward/down or down into the covered route |

When eligible signs and arcade fixtures are both nearby, the frontage pool keeps
one of each kind. If a kind is absent, the other can use the available slot.
All six lights are shadowless `SpotLight` components. The separate evacuation
flare remains its original `PointLight`, with its existing gameplay behavior.
Neither the flare nor the directional sun is counted as a city-pool slot.

The point-to-spot change itself is not treated as an automatic brightness gain.
Bevy 0.20.1 converts both component intensities with the same `intensity / (4π)`
factor. Actual receiver pixels are checked to establish useful local illumination.
The light aim is derived from the fixture's real orientation, not from the camera.

These are deliberately shadowless lights. Direction and source selection reduce
unhelpful backward illumination, but they do not prove physically correct light
occlusion through every building, vehicle or roof. No shadow maps, contact shadows,
bloom, volumetric fog or screen-space reflections are added by this wave.

## Stable selection and power transitions

A spatial index limits candidate discovery to nearby cells. New sources are
admitted within 32 metres; an assigned source can remain out to 36 metres to avoid
chattering at the admission edge. The audit reports those retained outer sources
explicitly. Candidate visits, source identities and allocated lights are distinct
measurements.

The audit's 3,608 queries include all 3,335 source neighbourhoods, fixed scene
anchors, a movement route, stationary repeats and cross-city jumps. Every cold
query is compared with an independent full-catalogue selection. In this sample the
maximum was 25 visited cells, 59 source evaluations and six selected slots; 59 is
an observed sample maximum, not a universal candidate-count guarantee. All 3,335
catalogue identities are selected by at least one source-neighbourhood query.

Stable IDs identify `Street(prop_index)`, `Sign(building_id)` and
`Arcade(building_id, bay_index)`. A clearly closer replacement is required before
an otherwise valid source is displaced. Retained sources keep their slots rather
than being exchanged whenever two distance ranks swap.

A normal source handoff fades the old light out, changes its position only while
dark, then fades the replacement in. The implementation uses a short 180 ms fade
for each side of the handoff. Spatial selection is refreshed at bounded intervals;
the six light intensities can still follow the day/night response every frame.

Power loss, daylight and leaving the live game turn off the actual pool
immediately. Power restoration re-enables the appropriate original sources.
The existing clock, utility state, dawn/dusk curves, weather and save systems
remain the authority; no competing electrical or time system is introduced.

## Three shared glow roles

The former `Mat::Lamp` contained three unrelated uses. The existing meshes now
select the appropriate shared material:

| Material | Existing geometry | Response |
| --- | --- | --- |
| `Lamp` | Street-lamp heads | Warm fixture glow |
| `WindowGlow` | Originally occupied-room panes | Softer warm window glow |
| `ArcadeGlow` | Existing arcade ceiling fixtures | More neutral/cool fluorescent glow |

Horizontal and projecting signs retain their existing emissive textures. Every
city-powered emission reaches zero during blackout. The original window-selection
random sequence, occupied/broken/boarded decisions, pane positions, curtains,
frames and cages are preserved. Parked vehicles remain unlit scenery/cover under
the existing rules; their headlights do not switch on with municipal power.

Two additional shared material handles do not mean two additional draw calls.
Splitting a previously combined chunk batch can add a submitted mesh in each
affected chunk. The workload check measures that actual increase separately.

## Geometry and world compatibility

The complete world fingerprint remains required to equal `3b33c3a798c7decd`:
2,677 buildings and original entrances, 615 roads, 3,356 street props,
74,337 collision segments and 14,378 interior fixtures. The first three waves'
feature plans, source positions, orientations and seeds remain unchanged.

At the same exact engine `f32` audit coordinates, Wave 4 requires the following
Wave 3 geometry totals to remain **exact**, not merely within a larger allowance:

| Planned resident metric | Original spawn | Existing `04a` streetscape |
| --- | ---: | ---: |
| Vertices | 3,040,724 | 3,040,315 |
| Indices | 6,454,764 | 6,440,727 |
| Triangles | 2,151,588 | 2,146,909 |
| Attribute/index payload bytes | 171,773,808 | 171,698,028 |
| Wave 3 submitted material-mesh baseline | 3,346 | 4,303 |
| Wave 4 submitted material meshes | 3,354 | 4,309 |

Submitted material meshes may increase by at most **1%** for the glow split.
The measured increase is eight meshes at spawn (**0.2391%**) and six at the
existing streetscape (**0.1394%**), with all four geometry metrics unchanged.
Original, Wave 1, Wave 2 and Wave 3 values remain visible in cumulative comparisons.
Near/Mid/Far thresholds, initial lazy-detail admission and building cutaway remain
unchanged. These are CPU-generated mesh payloads, not total GPU memory, visible
draw calls or measured frame times.

`lighting_workload` separately checks the original-world light catalogue, indexed
candidate selection, retention bounds and the six-slot allocation contract.
CPU timing is recorded as a development-build observation, not a hardware-independent
performance threshold. Runtime QA independently inspects real light components.
Exact geometry counts alone cannot establish unchanged lighting GPU cost.

## Actual rendering verification

The original 62 native capture names, relative order and assertions remain. Six
new same-view captures produce a full total of **68**:

| State | Required evidence |
| --- | --- |
| Day | Existing geometry and camera; city direct light and emission off |
| Dusk | The original smooth time transition with partial fixture response |
| Night | Real bounded spotlights, source transforms/aim and illuminated receiver pixels |
| Night without direct city spill | Diagnostic A/B with unchanged clock, power, ambient, sun and material emission; only actual city-pool intensity is disabled |
| Blackout | City-pool intensity and powered material emission off under actual outage state |
| Restored | The same original sources, normal direct-light parameters and visible receiver illumination return |

The no-spill capture is explicitly a **QA diagnostic**, not normal blackout
gameplay. It is necessary because the pre-existing blackout also reduces global
ambient brightness. Comparing only normal night against blackout would therefore
not isolate illumination from the city light pool.

The reports retain real light component parameters, source IDs, projected receiver
coordinates and pixel RGB/luminance. The image gate compares the same non-emissive
receivers between normal night and the no-spill control; glowing sign/window
pixels cannot substitute for direct light reaching a road or arcade floor.
The comparison measures the contribution of the whole six-slot city pool. A
receiver can fall within overlapping cones; recorded coverage does not turn the
paired image into an isolated measurement of each individual emitter. Probe
sources also need actual main-camera cluster visibility, and original building
and prop triangles screen out covered ground positions.
Recovered power is compared with the original powered night. Metadata visibility
does not establish that every part of every fixture is unoccluded; original PNGs
remain part of visual review.

An explicitly marked lighting-only development subset supports iteration and
cannot satisfy the full CI gate. The existing 30-minute internal native deadline,
31-minute external command limit and all per-stage checks remain. CI79's previous
full 62-image/report gate took approximately 21 minutes 23 seconds under software
Vulkan; elapsed QA duration is not a phone performance benchmark.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p defend_taipei --lib
cargo run -p defend_taipei --example exterior_workload -- --check
cargo run -p defend_taipei --example lighting_workload -- --check
cargo run -p defend_taipei -- --qa
```

The complete CI retains actual native PCM audio, ARM64 APK/signature validation,
Android gameplay/PCM/max-volume, settings/rotation/cold Continue/thermal-policy
checks and the three save-preserving update paths. Native null-audio screenshots
do not replace the independent audio or Android gates.

## Remaining boundaries

Citywide coverage describes a world-derived source plan and lighting that follows
the player throughout the existing Wanhua map, not thousands of simultaneous
dynamic lights. The renderer does not provide per-window simulated households,
global illumination or exact real-world surveyed lighting. Other districts and
the unimplemented gameplay listed in the README remain separate work. Physical
phone frame rate, display presentation, heat and battery cost still require actual
measurements on that device.
