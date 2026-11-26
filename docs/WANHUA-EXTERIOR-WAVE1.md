# Wanhua exterior wave 1 — building architecture

## Starting point and scope

The baseline is Android main `c17292889e884f8a13d4e14598a20b64dc490f81`, after PR #21. Its actual [CI #72 native GPU images](https://github.com/lijiabao1998/DefendTaipei-Android/actions/runs/45802533773/artifacts/15109786108) already show weathered walls, three-dimensional iron windows, air conditioners, arcade floors and detailed tin roof additions. The visible gap was repeated flat building masses: little balcony depth or owner/apartment rhythm, uniform roof edges, and the entire roof addition disappearing outside the close-detail radius.

Wave 1 reconstructs that architecture on the existing Wanhua lots. It covers the previously agreed building families, balconies, exterior ceramic, roof additions, rear service stairs and window depth. The separate commercial-street, road-object and lighting waves remain distinct follow-ups. This is procedural architectural variety on the existing OSM footprints, not a surveyed reconstruction of every real address.

## Architectural families

`game/src/map/exterior.rs` derives immutable rendering metadata from the current building ID, seeds, storeys, frontage and arcade. It uses its own integer variation hash and never consumes the world planner's RNG.

| Family | Existing-lot selection | Visible structure |
| --- | --- | --- |
| Townhouse | Narrow eligible frontage, small lot and two to four storeys | One wider balcony stack, a narrow owner's cornice, tiled end strips and compact floor bands |
| Walkup | Eligible wider/taller residential form | Separate apartment balcony bays, stronger slab bands, lower solid balcony fronts with open rails; rear service stairs where the complete envelope fits |
| Shophouse | Existing arcade, or a qualifying store/pharmacy frontage | Repeated proprietor bays, shallower balcony projections, tiled bay divisions and stepped coping; the existing shop sign stays visible |
| Other | Single-storey, unusually large/tall, unclassified or unsuitable frontage | Existing building geometry is retained; rooftop planning remains independent |

The facade follows the original doorway's upper-wall plane. An arcade's front wall is offset by its existing 3.6 m recess; no entrance is moved to manufacture a better-looking elevation. A single neutral ceramic texture is tinted in muted cream, rose, jade and stone palettes. Its UV scale is one metre per repeat, with 12.5 × 6.25 cm tile courses. It contains no painted window motif.

Balconies are admitted only when their full projected rectangle clears nearby footprint polygons and the carriageway, including a margin for rails and slab thickness. Narrow spaces receive a shallower balcony or keep their facade bands. Columns, frames, signs and the original ground-floor doorway retain their existing role.

## Roofs and windows

`game/src/map/roofs.rs` plans the complete roof before generating meshes. It aligns to the existing entrance, reserves the original tank first, checks concave footprint notches and edges, and rejects overlapping object envelopes. The same layout supplies both coarse and fine geometry.

- Narrow lots receive a compact rear tin shed where it fits; eligible wide walkups can have two separate owner sheds.
- A roof-access headhouse is added only when its entire footprint fits alongside the other reservations.
- Copings and low parapet/rail combinations replace the single repeated roof edge.
- Solar heaters, compressors, laundry and antennas use independent reserved spaces. Fine shack fittings stay inside the shack's full service/overhang envelope.
- Gable, arched and mono-pitch tin forms reuse the native procedural generator, split into a persistent shell and close-range fittings.
- Window units gain a real 22 cm masonry surround and projecting sill. The lit pane, sliding sash and iron cage remain separate; the surround never places an opaque plate over the lighted opening.

Rear stair assemblies have upper landings, switchback flights, guards, service-access frames and a folded lower ladder. All added structural vertices remain above 2.6 m. These are visible upper-floor escape structures; upper floors and roof access are not newly traversable gameplay. The ground-level collision contract and original interactive doors are preserved.

## Streaming, cutaway and persistence

| Layer | Near | Mid | Far |
| --- | --- | --- | --- |
| Building walls and footprint | Existing per-building geometry | Existing merged masses | Existing simplified masses |
| Balcony/slab/owner structure | Full structural layer | Same primary projections with fewer thin rail/stair parts | Omitted |
| Roof shed, coping and headhouse | Persistent shell | Identical shell and layout | Omitted |
| Windows, gutters, rooftop appliances and small fittings | Existing 70 m admission / 95 m removal layer | Omitted | Omitted |

Facade architecture is generated in `BuildingMeshes.structure`, then merged into the roof by shared material before Near upload. The resulting persistent shell carries `BldStructure` and `BldRoof`; close facade details keep `BldDetail`, and close roof fittings carry `BldRoof` without `BldStructure`. QA counts these three groups once each. The shell hides when the player enters or passes behind that building and returns with its original ceramic/metal materials. It never receives `BldWalls`, whose material replacement and height scaling are specific to the main walls. Mid chunks merge the same shared material batches across buildings.

The new facade layer is limited to three shared materials, at most 5,000 vertices per building at Near and 2,800 at Mid. Wall-mounted strips omit buried faces, balcony parapets retain their visible inner/outer/top faces, steel rails share corner vertices, and stair flights retain their actual stepped top in one connected shell. Small roof rails, gutters, pipes and ridge trim belong to the close-detail layer; open canopies retain structural corner posts at every visible shell LOD. Roof shells are capped at 1,280 vertices and three materials per building; fine fittings are capped at 16,000 vertices and six materials across the whole-world tests. These are workload limits, not a claim about physical-phone FPS.

`WorldData` planning, source footprints, doors, collision segments, building IDs, interior fixture IDs and seeds are unchanged. No save migration is introduced. Existing door, barricade, loot, Continue and APK-update gates remain part of CI.

## Verification

The standard workflow retains the original 47 GPU screenshots and adds six exterior captures (53 total). Original street/entrance views remain comparable with CI #72. The new captures select actual eligible lots and record the building ID, typology, player/camera position, zoom, projected bounds, admitted balcony/stair/roof features, uploaded mesh counts and streaming state.

The capture gate waits for the target's Near chunk and detailed facade/roof entities. It checks actual camera framing, propagated visibility and membership in the game camera's visible-entity list. Outdoor fixture selection shares the exact cutaway obstruction predicate with the renderer, so a concave lot's centroid cannot silently hide the subject. Waiting diagnostics identify the missing condition instead of leaving a blocked scene unexplained. Mesh counts and bounds are recorded before GPU upload, because the production renderer releases CPU vertex arrays after extraction.

The CPU regressions cover actual-world family coverage; deterministic layouts; full neighbor, road, concavity and tank clearance; geometry staying above the ground route; retained Near/Mid silhouettes; finite normals and valid indices; material/vertex budgets; window opening clearance; and hide/restore of mixed architectural materials. The same walkup is then shown from outside and from its real interior in GPU QA.

```sh
cargo fmt --all -- --check
CARGO_PROFILE_DEV_DEBUG=0 cargo clippy --workspace --all-targets -- -D warnings
CARGO_PROFILE_DEV_DEBUG=0 cargo test -p defend_taipei --lib
cargo run -p defend_taipei --example exterior_workload -- --check
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts/tests -p 'test_*.py'
cargo run -p defend_taipei -- --qa
```

`game/examples/exterior_workload.rs` regenerates every chunk at the existing Near/Mid/Far thresholds for the original spawn and `04a` streetscape position, then adds the initial 70 m detail population. Its report compares planned resident attribute/index bytes, vertices, triangles and submitted material meshes with `docs/exterior-baseline.json`. A length-delimited fingerprint verifies the complete building, door, window, collision, road, prop and interior-fixture data, including spawn and map bounds. These are deterministic geometry and compatibility checks; they do not measure visible GPU draw calls, GPU memory or physical-phone frame rate.

For fast visual iteration, `DEFEND_TAIPEI_QA_EXTERIOR_ONLY=1 cargo run -p defend_taipei -- --qa` retains the five normal startup stages and captures the six exterior scenes. Its report explicitly says `exterior_subset` with 11 expected images. CI requires `full`, all 53 images and the Save → Menu → Continue assertions, so a subset run cannot satisfy final acceptance.

### Measured geometry footprint

The final building meshes preserve the baseline world identity `3b33c3a798c7decd`: 2,677 buildings and ground-floor doors, 38,602 windows, 74,337 collision segments, 14,378 interior fixtures and 8,592 loot fixtures. Render-derived planning admits 2,315 balcony buildings (3,142 stacks), 231 upper service-stair assemblies, 2,425 roof sheds and 1,582 roof-access volumes. These counts describe generated features, not surveyed addresses or newly traversable floors.

| Planned resident geometry | Baseline | Wave 1 | Change |
| --- | ---: | ---: | ---: |
| Spawn attribute/index payload | 116,807,580 bytes | 166,343,220 bytes | +42.41% |
| Existing `04a` street payload | 118,572,072 bytes | 166,357,464 bytes | +40.30% |
| Spawn vertices | 2,064,115 | 2,940,423 | +42.45% |
| Existing `04a` street vertices | 2,098,077 | 2,941,560 | +40.20% |
| Spawn submitted material meshes | 2,446 | 3,349 | +36.92% |
| Existing `04a` street material meshes | 2,939 | 4,315 | +46.82% |

The early implementation required 226,898,820 and 221,475,384 payload bytes at these positions. Visible-shell construction, near-only roof fittings and shared-material batching remove 60,555,600 and 55,117,920 bytes from that version while retaining the planned features. The additional geometry still has a real cost against the baseline. The CI audit limits vertices, triangles and payload to 145% of baseline and material meshes to 150% at both fixed positions; it fails on world-identity drift or a budget overrun. Exact feature and resident totals are emitted in `native-qa/exterior-workload.json` on every run.

The final CI run and capture evidence are recorded in the change's pull request. Android emulator checks exercise the actual APK, audio, performance preferences/thermal policy, touch/rotation, Continue and save-preserving updates. The offscreen native QA uses fixed simulation steps and a software renderer where necessary; physical-device timing remains a separate measurement.
