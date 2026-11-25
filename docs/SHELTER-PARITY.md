# Native parity: shelter simulation

Reference: web v24.42.0, commit [b9b0a27](https://github.com/lijiabao1998/defendtaipei/tree/b9b0a2736642b37dd45925a4719dfef08b8f36b4).

- Rules: [src/systems/survival.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/systems/survival.ts).
- Dawn scheduling: src/core/main.ts uses floor((gameMin - 360) / 1440).
- Native: game/src/survival.rs; shared by desktop and Android. The plugin initializes a fresh shelter at the prologue and ticks during Playing.
- Ported: roster and bilingual labels, stash insertion order, daily meals/water/illness/healing/morale, deaths, seeded events, raids and arrivals, planters/rain collectors, upgrade cost table, prayer and barter valuation.
- Tests cover the day-two/day-three sequence, clock boundary, prayer, stash ordering, upgrade costs, and utility/log behavior.
- Not yet ported in this PR: home interaction and inventory transfer, upgrade purchase UI, barter stalls, moral decisions, ending paragraph, persisted shelter state, weather/refrigeration/utility integration. ShelterEnvironment is an adapter resource, not the world utility system. The temporary accelerated campaign clock is unchanged.
- No visual parity claim. The remaining UI should follow the original shelter panel, not invent a different mechanic. Compare the original and native state at equal game minutes, with equal inventory and seeded weather input.

Verification: cargo test -p defend_taipei --lib; the existing desktop/Android CI runs on the PR. Physical Android device playtest remains a separate gate.
