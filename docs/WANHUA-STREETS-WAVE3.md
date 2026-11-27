# Wanhua exterior wave 3 — roads and street objects

## Scope

This wave follows the merged architectural and commercial waves. It improves the
existing road markings and the parked vehicles and lamps already in the Wanhua
world. It does not add driveable vehicles, new interactions, collision obstacles,
traffic simulation or citywide lighting. The OSM roads remain the source of road
positions and widths; the markings are a procedural visual treatment, not a survey
of the present-day markings at every Taipei address.

The preceding source is main `230c82d85b47ea47c9411721bf68f7a7f0528d76` (PR #23).
Its CI77 native artifact is the reproducible baseline. The baseline's tested
synthetic merge, source head and merged main have the same source tree
`e985dda817fe4c00a2af5a6b09c198d38e803d37`.

## Road layout and street objects

Road marking plans derive junction clearance from connected roads. Longitudinal
lines stop before the approach treatment, and eligible approaches receive a
crossing, stop line and direction arrow as a coordinated assembly. Short or
geometrically unsuitable approaches are omitted instead of forcing overlapping
symbols into them.

Parking outlines derive from the existing parked props. Admission checks the
whole proposed footprint, entrance corridors, junction approaches and road-edge
space. Red and yellow curb sections keep their existing appearance and do not
receive parking outlines. A rejected outline does not move or delete its vehicle.
The original parked objects are not newly available loot or transport.

Scooters, cars, taxis, trucks and street lamps retain their original positions,
orientations and seeds. Their geometry is generated in the existing shared
material batches. Vehicle glazing is placed on exposed surfaces rather than
inside the opaque body shell. Lamps retain the existing power/lighting behavior;
this wave does not introduce additional light entities.

The deterministic inventory contains 61 crossing/stop-line approaches and 49
approach arrows. Of the original 469 parked vehicles, 73 receive eligible bays:
72 scooters and one truck. The other 396 remain in place without an invented bay.
The original prop inventory is 2,010 lamps, 431 scooters, 22 cars, nine taxis,
seven trucks and 877 unchanged trees. These counts describe existing objects and
admitted paint, not newly spawned gameplay entities.

## World and save compatibility

The full world identity must remain `3b33c3a798c7decd`, including all original
building records, roads, street prop records, collision segments and interior
fixtures. This includes 2,677 buildings and doors, 615 roads, 3,356 street props,
74,337 collision segments and 14,378 interior fixtures. Door, barricade, inventory,
loot and save identifiers remain the same.

The original architecture and commercial plans and their complete Near/Mid
vertex totals are checked against the verified earlier waves. Initial lazy
building detail must remain exactly the same at both audit positions. Road and
prop geometry remains attached to the source segment midpoint or prop centre's
chunk. The original Near/Mid/Far thresholds and the building cutaway behavior are
retained.

## Bounded cost with an attributable baseline

`docs/streets-wave2-baseline.json` is extracted from the verified CI77 artifact
`15158096594`, run `45902044839`. The archive SHA-256 is
`13203dcfa3ffb514a93f2ed3e73c76763f92cc38aa4cc7e314e29d21a86d0f53`.
The checked-in baseline also records the original workload file's digest and all
source revisions. Timing from that CI runner is not used as a performance promise.
The one-time street-plan initialization is recorded separately before any feature
inventory warms its cache; later position chunk timings use that warmed cache.

The workload check uses the same original spawn and `04a` streetscape coordinates
and the same exact engine `f32` positions. Its Wave 3 limits are 105% of Wave 2 for
vertices, triangles and attribute/index payload, and 101% for submitted material
meshes. It also retains the original pre-architecture and Wave 1 totals so that
the cumulative cost is visible.

The reproducible Wave 3 totals are:

| Planned resident metric | Original spawn | Existing `04a` streetscape |
| --- | ---: | ---: |
| Vertices | 3,040,724 | 3,040,315 |
| Triangles | 2,151,588 | 2,146,909 |
| Submitted material meshes | 3,346 | 4,303 |
| Attribute/index payload bytes | 171,773,808 | 171,698,028 |
| Payload increase over Wave 2 | 1.0484% | 0.9653% |
| Additional material meshes over Wave 2 | 2 | 2 |

These are aggregate planned resident mesh counts after streaming settles. They
are not visible draw calls, GPU memory, measured frame times or physical-phone
FPS. Payload excludes allocator overhead, ECS, textures, materials, actors,
dynamic door geometry, runtime interior meshes, rain and UI. Per-feature mesh
counts describe the generated replacement assemblies; the resident comparison
is the authoritative net change.

## Verification

The full native GPU suite preserves the previous 58 captures in their original
relative order and adds four street scenes. The added fixtures use original road
segments and prop records, collision-clear player locations, actual uploaded
chunk meshes and the real main camera. The reports distinguish geometric bounds
and camera visibility from final pixel occlusion; the PNGs must also be reviewed.
Existing Save → Menu → Continue, architecture/cutaway and commercial day/night/
blackout assertions remain required.

Fixture placement additionally rejects the complete original car, taxi, truck
and scooter footprints with a player-radius margin. A point inside a closed
vehicle perimeter can be farther than the collision radius from every edge;
the ordinary segment-distance query alone is therefore insufficient to validate
a teleported QA camera. The production collision records are not changed.

| New capture | Required original-world subject |
| --- | --- |
| `04d23-streets-junction-landscape.png` | A connected-road approach with a crossing, inbound stop line and arrow. |
| `04d24-streets-scooters-portrait.png` | At least two original scooters, two admitted parking bays and one original nearby lamp. |
| `04d25-streets-taxi-landscape.png` | One original taxi with its actual uploaded vehicle materials and geometry. |
| `04d26-streets-truck-landscape.png` | One original truck with its actual uploaded cab and cargo-body geometry. |

White marking probes pair a projected paint sample with adjacent unpainted
asphalt and record both pixels, their coordinates and luminance. The check rejects
neutral asphalt and uniform bright pavement, and excludes samples hidden by the
selected prop geometry. It is a deliberately limited paint check; it does not
claim pixel-level verification of every vehicle panel or every scene occluder.
The actual vehicle PNGs remain part of visual review.

The full 62-capture suite has a 30-minute internal deadline with one additional
minute for the shell to collect failure evidence. The preceding 58-capture CI77
run already took approximately 24 minutes under software Vulkan. The added time
allows four additional actual rendered scenes; it does not skip previous stages
or reduce their assertions. A street-only development run is visual iteration,
not a replacement for the full CI run.

For local iteration, `DEFEND_TAIPEI_QA_STREETS_ONLY=1` selects five startup
captures followed by the four new street scenes. Its report must identify itself
as `streets_subset` with nine expected captures; CI only accepts `full` with 62.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p defend_taipei --lib
cargo run -p defend_taipei --example exterior_workload -- --check
cargo run -p defend_taipei -- --qa
```

CI also retains native PCM audio, Android gameplay and PCM audio, media-volume
maximum verification, performance settings/cold Continue/rotation and the three
save-preserving APK update paths. Local null-audio software-Vulkan screenshots
do not stand in for these audio and Android gates.

## Remaining boundaries

This is a targeted street rendering wave. It does not establish full parity with
the original Web game or a one-to-one reconstruction of present-day Wanhua street
furniture. Citywide lighting, other districts and the existing gameplay boundaries
listed in the README remain separate work. Physical-device frame rate, temperature
and battery cost require measurement on that device.
