# Hand grenades and the war getting worse

References: `GREN`, `playerThrow`, `grenadeTick`, `blastReach`, `WAR`, `weaponDmg`, `warLevel`, `warTick`, `warBarrage`, `warStrike`, `warImpact` and `updateShells` in [Web `src/core/main.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/core/main.ts) (v24.20, v24.21). Native implementation: [`war.rs`](../game/src/war.rs).

## v24.20 hand grenades

- **Use** on a service grenade or grandpa's old grenade throws it the way the player faces, up to 13 m; it sails over low cover and stops short of the first wall. It flies for a quarter second plus 55 ms per metre and goes off 3.5 s after the throw.
- Soldiers within 5 m take 240, to 15 m a falling share of 67; a wall between stops the blast (`blastReach` checks the line and two lines 0.8 m to each side). Kills are the player's. The player's own blast hurts them on the same falloff (95 to 42 → 0) and can start bleeding.
- Three in ten old grenades are duds ("……沒有炸。放了太多年，它啞了。"). Throwing is quiet (6 m); the blast is heard 45 m away. Grenades cannot be thrown from an upper floor.

## v24.21 the war gets worse

- War level = clamp((day − 0.25) / 6 + 0.015 × reclaimed areas, 0, 1): 0 at the landing, 1 about a week in, hotter with every area taken back.
- Bursts of distant rifle or machine-gun fire, 140–520 m away, every (40 − 32 × level) × (0.6–1.4) s, with 3 to 8 + 10 × level shots; muffled indoors.
- From level 0.1 (the first evening), every (260 − 190 × level) × (0.7–1.3) s a barrage of 2 to 4 + 3 × level mortar rounds falls 30–130 m away, each under a red warning ring, whistling in the last 1.6 s. Mortar: 6 m core (85), 18 m wound (50).
- Above level 0.3, every (900 − 600 × level) × (0.7–1.3) s an air-raid siren and warning precede a long-range rocket (16/45 m, 190/70), cruise missile (22/62 m, 230/75) or short-range ballistic missile (24/70 m, 240/80) under a big ring, landing 8–10 s later outside the wound ring until the war is at its worst; each leaves a crater burning for 45 s.
- Soldiers in the rings are hit too (300 in the core). Indoors (or upstairs) the player takes 15% of mortar fragments, and 60% (core) or 25% of a missile; cover halves it outdoors; more than 15 damage starts bleeding. Nothing falls while the player sleeps, and an air-raid warning ends accelerated time.

## Boundaries

The Web's screen kick, the fire-and-earth column of a big warhead and its particle smoke are simplified to the shared blast flash, a scorch disc and rising flames. Timers are not saved, as in the Web.

## Checks

Unit tests cover the war level curve and start, the damage falloff of every weapon and of the grenade. Native QA stage `04q-war-landscape` throws a real grenade from the bag and calls a barrage and a rocket, checking that the grenade is spent and the rounds fall under their rings.
