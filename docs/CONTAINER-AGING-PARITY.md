# Opened container food aging

Reference: Web v24.42.0, commit [`b9b0a2736642b37dd45925a4719dfef08b8f36b4`](https://github.com/lijiabao1998/defendtaipei/tree/b9b0a2736642b37dd45925a4719dfef08b8f36b4), `src/core/main.ts::updateStory`, `PERISH` and the call site that passes game hours.

## Rules now connected to the native world

| Rule | Native behavior |
| --- | --- |
| Power | The periodic process runs only while the actual saved shelter/world power flag is off. A frame crossing the scheduled outage charges only its post-outage time. |
| Interval | The source's threshold is strictly **more than six game hours**, not six real minutes. The campaign clock determines elapsed game time, including the selected day length and fast-forward. |
| Containers | Already generated/opened stock whose original Chinese label contains `冷藏`, `櫃台` or `攤`. Native drinks refrigerators and checkout counters qualify. Ordinary cupboards, shelves, dropped soldier supplies and unopened stock do not gain an extra periodic process. |
| Eligible food | Exact Web `PERISH` list: `bento`, `rice`, `oden`, `soymilk`, `milktea`, `danbing`, `chicken`, `sausage`, `egg`. This is deliberately distinct from the catalog-wide `perish` flag used by the daily bag/shelter rule. |
| Chance and quantity | At each interval, draw once for each listed food stack currently containing at least one portion. A draw **below 0.15** converts exactly **one portion** to `rotten`. Empty stacks draw nothing. Cans, water and dry goods stay unchanged. |
| Conservation | The same saved `Stash` loses one fresh portion and gains one rotten portion. No inventory transfer, duplicate container record or loot reroll occurs. Empty stock remains empty. |
| Live presentation | The selective pickup panel reads the current stock, and the existing stock renderer reads those same saved leftovers. If the selected container changes, its notice reports the converted portions. |

The existing rules remain separately responsible for (1) fresh food first rolled after a full day without power and (2) the daily 06:00 spoilage of carried/shelter food. The new six-hour process does not replace either rule.

## Saved clock and compatibility

`ScavengeState.container_aging` persists:

- `minute`: the last processed game minute;
- `elapsed_hours`: the accumulated powered-off hours not yet processed;
- `seed`: an independent deterministic random stream.

All fields have serde defaults. An older native save with no aging state starts the timer at its current game minute, preserving existing stock instead of applying previously unimplemented historical losses. New saves resume the exact partial interval and RNG state, so repeatedly loading cannot reset the timer or reroll the next outcome. Normal pause/background handling freezes the campaign clock and this system; menu and ended-run states do not process stock.

Save validation rejects non-finite or negative times, clocks ahead of the campaign, and accumulated time greater than the entire run. The unrelated weather RNG is not consumed by container aging.

## Deliberate native differences

The Web resets its unsaved `spoilT` to zero after one check. Native retains fractional overshoot and saves it. A large time jump processes every due interval, at most 64 per frame; any pending hours remain saved for bounded catch-up. Ordinary play advances by small fractions of a game hour, so this bound does not defer normal aging.

Native stock is generated when a fixture is searched, using the port's existing deterministic catalog. The periodic process visits saved stock in its stable saved order. It reproduces the Web probability, food list, label selection and per-stack quantity rule, but does not claim to reproduce the Web renderer's global random sequence or unopened, streamed container lifetime.

## Regression coverage

`game/src/worldplay/aging.rs` verifies the actual strict six-hour boundary, outage crossing, overshoot, powered periods, exact probability threshold, one-unit conversions, unchanged dry stock, empty-stock RNG behavior, legacy defaults, mid-interval serialization, unchanged outcomes after reload, and bounded large-jump catch-up.

An ECS test runs the real world system against opened cold storage, an ordinary shelf, an unopened counter and a body cache. It checks saved quantities, the live notice, pause, power restoration and repeated processing at the saved minute. Save-module tests validate the new persisted fields independently.
