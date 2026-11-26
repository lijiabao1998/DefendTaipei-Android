# Wanhua exterior wave 2 — commercial street fronts

## Scope and starting point

This wave starts from merged Wave 1 main `6d0bbd55964d802e2ddaae886baf9ca8dc17dcb0`
(PR #22, source tree `cb6945b8846454dd73a02d17ea5f64471e568516`). The first wave
established the townhouse, walkup and shophouse architecture. The second wave
develops the shop fronts beneath it: recognisable shop identity, constructed signs,
shutter/window bays and exposed shop canopies. Road objects and a citywide lighting
redesign remain separate follow-ups.

The existing map already has real entrance positions, arcades, floor tiles, lamps,
shutters and sign text. The commercial work derives a render-only plan from those
records. It does not reclassify a building's gameplay use to match its decorative
business sign. In particular, a home-kind lot can have an existing restaurant,
barber, pawnshop or hardware sign; the sign's atlas index is the visual identity.

## Compatibility and visual contracts

- The original building, door, collision, window, road, prop and interior-fixture
  records remain the source of truth. No save migration or new upper-floor route
  is introduced.
- New lower storefront treatments occupy existing solid frontage beside the live
  door. They do not create an extra entrance or a searchable display fixture.
- Fascia and projecting signs use the original shop's text. A second unrelated
  random business name must not be attached to the same shop.
- Raised signs, canopies and tall storefront panels follow the building cutaway.
  Only low owner trim remains in the always-drawn base, so a new opaque frontage
  cannot re-block the player after the original wall has faded. Shared material
  batches are retained.
- Canopies and projecting signs require an admitted envelope, not only a clear
  centre point. Ground entrances, swing space, the arcade walking route, nearby
  lots and the earlier architectural projections must remain clear.
- The existing saved time and power state continue to control sign emission.
  Commercial night/blackout checks do not establish a new city-lighting system.

## Storefront construction

The planner groups the existing sign atlas into food shops, traditional businesses,
convenience stores, pharmacies and police stations. Each frontage retains its
original business text and uses a coordinated palette. Closed glazed, shuttered
or timber panels are fitted to the actual solid door flanks. A dark display panel
is decorative surface treatment, not a transparent window into a new room.

Exposed canopies have sloping tops, side returns and a framed lightbox at their
front edge, keeping the shop name readable from the game's ordinary elevated
camera. Admitted projecting signs are short, double-sided horizontal lightboxes
using the same atlas cell. No texture atlas, dependency or per-shop material is
added. Panels, canopies and signs do not receive separate entities; their geometry
joins the building's material batches.

Arcade panels are clipped to their real solid wall intervals beside the door,
which also admits compact shops whose theoretical ownership bay straddles an
entrance. A matching secondary board hangs from the street-side soffit, above
the covered walking route. Its hangers reach the existing underside while
remaining below the Wave 1 facade band and balcony geometry. Recessed panels
remain subject to normal physical occlusion from the arcade and upper building.

Near and Mid LOD keep the primary storefront silhouettes, with the finer framing
and surface treatment reduced at Mid. Far LOD retains its existing simplified
building representation. Ownership and material metadata identify the actual
uploaded batches for QA without splitting them into extra draw submissions.

## Geometry accounting

`docs/commercial-wave1-baseline.json` contains the exact geometry counts, world
identity and architectural features from the completed Wave 1 CI #75 artifact.
It records the source commit, source tree, run and artifact identifiers; timing
fields are omitted because the report is a reproducible geometry reference.

The existing `exterior_workload` executable retains both original fixed positions:
spawn and the established `04a` street view. Its Wave 2 check requires:

1. The same complete world identity as both Wave 1 and the pre-architecture map.
2. The same Wave 1 architectural feature counts and facade vertex counts.
3. At most 106% of Wave 1's resident vertices, triangles and attribute/index bytes,
   and at most 105% of its submitted material meshes, at both positions.

The report also retains cumulative changes against the original pre-architecture
baseline. Moving to the Wave 1 reference for the bounded second-wave increment
does not erase the earlier cost. These counts are planned CPU-generated resident
meshes at the actual streaming thresholds, not measured GPU memory, visible draw
calls, physical-phone frame rate or thermal behavior.

### Measured coverage and cost

All 603 originally signed buildings receive a commercial plan: 87 food shops,
182 traditional businesses, 178 convenience stores, 112 pharmacies and 44 police
stations. This includes 67 signed arcade frontages. The full-world plan admits
1,173 closed panels (686 glazed, 341 shuttered, 146 timber), 358 exposed canopies
(134 striped) and 384 matching projecting signs. Police stations keep their
institutional treatment without a commercial canopy or projecting shop sign.

| Resident metric | Wave 1 spawn | Wave 2 spawn | Change | Wave 1 `04a` | Wave 2 `04a` | Change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Vertices | 2,940,423 | 3,008,121 | +2.302% | 2,941,560 | 3,010,296 | +2.337% |
| Triangles | 2,100,243 | 2,133,487 | +1.583% | 2,096,882 | 2,130,190 | +1.588% |
| Material meshes | 3,349 | 3,344 | -5 | 4,315 | 4,301 | -14 |
| Attribute/index payload bytes | 166,343,220 | 169,991,652 | +2.193% | 166,357,464 | 170,056,488 | +2.224% |

Both original positions pass the 6% geometry and 5% material-mesh increment
limits. Cumulative payload growth from the pre-architecture map is 45.531% at
spawn and 43.420% at `04a`; the earlier architectural cost remains visible. The
world fingerprint remains `3b33c3a798c7decd`, and all Wave 1 feature and facade
vertex counts match. The largest admitted shop assembly uses 804 Near vertices
and 428 Mid vertices, below its 1,400/600 per-building caps.

## Verification

The implementation is checked against complete-world geometry/clearance contracts
and actual GPU captures. The existing building, entrance, interior, save and audio
flows remain required. The complete CI result and Android artifact for a revision
are authoritative; a development-only commercial subset is visual iteration and
cannot satisfy full acceptance.

```sh
cargo fmt --all -- --check
CARGO_PROFILE_DEV_DEBUG=0 cargo clippy --workspace --all-targets -- -D warnings
CARGO_PROFILE_DEV_DEBUG=0 cargo test -p defend_taipei --lib
cargo run -p defend_taipei --example exterior_workload -- --check
cargo run -p defend_taipei -- --qa
```

The full native run retains its previous 53 captures and adds these five:

| Capture | Evidence |
| --- | --- |
| `04d18-commercial-arcade-landscape.png` | Compact, real arcade; admitted panels and a street-side hanging identity board. |
| `04d19-commercial-food-landscape.png` | Food-shop panels, sloping canopy, front lightbox and matching projecting sign. |
| `04d20-commercial-essentials-landscape.png` | A real pharmacy or convenience storefront, using its original sign identity. |
| `04d21-commercial-night-landscape.png` | The same food shop and camera at powered night; sign emission and active lights. |
| `04d22-commercial-blackout-landscape.png` | Identical shop/camera at night with power off; sign emission and active lights at zero. |

QA records the actual uploaded base/shell material batches, their vertices and
bounds, main-camera membership and projected feature bounds. Those measurements
establish integration and framing; they do not constitute a per-pixel occlusion
test. The real screenshots are also reviewed for readability and physical
placement. The arcade fixture is deliberately compact under the normal camera;
its recessed wall can still be hidden naturally by the arcade above it.

`DEFEND_TAIPEI_QA_COMMERCIAL_ONLY=1` selects a ten-capture development run (five
startup captures plus these five scenes). It reports `commercial_subset` and
cannot satisfy CI, which requires `scope=full`, all 58 captures and successful
Save → Menu → Continue. The required desktop, ARM64 APK and x86_64 emulator jobs
also retain the native PCM, Android audio/settings, gameplay, performance-control
and same-signer update checks. The pull request and its retained artifacts record
the exact revision's completed results.
