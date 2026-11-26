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

Facade architecture has its own `BuildingMeshes.structure` group. Near chunks tag it with `BldDetail`; roofs use `BldRoof`. They hide when the player enters or passes behind that building and return with their original ceramic/metal materials. They are never assigned `BldWalls`, whose material replacement and height scaling are specific to the main walls. Mid chunks merge the same shared material batches.

The new facade layer is limited to three shared materials, at most 10,000 vertices per building at Near and 5,000 at Mid. Roof shell/fittings also have explicit whole-world mesh and material limits in their tests. These are workload limits, not a claim about physical-phone FPS.

`WorldData` planning, source footprints, doors, collision segments, building IDs, interior fixture IDs and seeds are unchanged. No save migration is introduced. Existing door, barricade, loot, Continue and APK-update gates remain part of CI.

## Verification

The standard workflow retains the original 47 GPU screenshots and adds six exterior captures (53 total). Original street/entrance views remain comparable with CI #72. The new captures select actual eligible lots and record the building ID, typology, player/camera position, zoom, projected bounds, admitted balcony/stair/roof features, uploaded mesh counts and streaming state.

The capture gate waits for the target's Near chunk and detailed facade/roof entities. It checks actual camera framing and cutaway visibility. Mesh counts and bounds are recorded before GPU upload, because the production renderer releases CPU vertex arrays after extraction.

The CPU regressions cover actual-world family coverage; deterministic layouts; full neighbor, road, concavity and tank clearance; geometry staying above the ground route; retained Near/Mid silhouettes; finite normals and valid indices; material/vertex budgets; window opening clearance; and hide/restore of mixed architectural materials. The same walkup is then shown from outside and from its real interior in GPU QA.

```sh
cargo fmt --all -- --check
CARGO_PROFILE_DEV_DEBUG=0 cargo clippy --workspace --all-targets -- -D warnings
CARGO_PROFILE_DEV_DEBUG=0 cargo test -p defend_taipei --lib
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts/tests -p 'test_*.py'
cargo run -p defend_taipei -- --qa
```

The final CI run and capture evidence are recorded in the change's pull request. Android emulator checks exercise the actual APK, audio, performance preferences/thermal policy, touch/rotation, Continue and save-preserving updates. The offscreen native QA uses fixed simulation steps and a software renderer where necessary; physical-device timing remains a separate measurement.
