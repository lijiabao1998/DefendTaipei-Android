# 國軍殘部: ROC remnant positions and PLA checkpoints

References: [Web `src/npc/remnant.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/npc/remnant.ts), `remnantSites`, `setupRemnants` and `sandbags` in [Web `src/core/main.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/core/main.ts), the checkpoints of [Web `src/map/decay.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/decay.ts), `flagPole` in [Web `src/npc/roc.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/npc/roc.ts) and [Web `src/npc/firearms.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/npc/firearms.ts). Native implementation: [`remnant.rs`](../game/src/remnant.rs), [`map/fortify.rs`](../game/src/map/fortify.rs), [`dialogue.rs`](../game/src/dialogue.rs) and [`human/rig.rs`](../game/src/human/rig.rs).

## Player-facing behavior

- Four sandbagged street corners hold out 110–420 m from the start, one in each quarter of the compass and more than 160 m apart, on the open carriageway of a mid-size road — the Web placement rule. Each has a five-bag front wall, a three-bag flank wall, a 3.2 m pole with the ROC flag (painted procedurally like the Web canvas) and four soldiers in ROC greens with T91, T91, T75 SAW and T65K2.
- The sandbags are ordinary 1 m low cover in the shared world collision, for movement, sight and shots.
- The squads fire on 解放軍 soldiers within 42 m of their position when they can see them over their own sandbags. Each man has his own cadence; a hit does 40 damage with the Web's distance-scaled chance. Their gunfire is heard and draws patrols. Soldiers fighting or investigating with sight of the position wound a random defender (30 damage) at the Web rate. Kills are the squad's, not the player's.
- Men fall one by one; the rest do not withdraw. When the last one falls the radio card carries the unit's last words and a crate of their gear (T91, 60 rounds of 5.56, rations, a first-aid kit, two grenades) is left by the sandbags under the name **國軍殘部遺留裝備**.
- Within 3.2 m of the leader, Interact opens his conversation card with his own rotating lines: supplies once a day (20 rounds of 5.56, a ration, a bandage), treating the most wounded man with a first-aid kit (+70) or bandage (+40), and intelligence that marks the PLA checkpoints on the survivor map and reveals the JSDF liaison. A fallen position offers its memorial text instead.
- Each morning a squad still holding reports on the radio.
- Two 解放軍 checkpoints stand across wide streets 35–112 m from the start (the Web's `R × 0.8` with R = 140), 50 m apart, each with two sandbag emplacements. The first four patrols man them as sentries, two at each, and do not answer the radio — as in the Web boot.
- Every firearm model of the Web `firearms.ts` is now on the native rigs: pistols, AR-pattern rifles with flat-top or carry handle, QBZ-191 and the QBU-191 marksman rifle, bullpups, the T93 sniper rifle, belt- and box-fed machine guns, the QCW-05, shotguns, rocket tubes and the JSDF Type 20, Type 89, MINIMI, 9 mm pistol and LMAT.

## Persistence

Each position's four health values, fallen flag, last supply day, first meeting and spoken-line counter are a defaulted `remnants` section of the native save, validated on load. Older saves start with all four squads at full strength. The fallen positions' crates use the existing world drops and keep their leftovers.

## Boundaries

The Web's v24.39–v24.41 links to computer teammates (rescues, credited kills between allies) are part of the teammate system, which is not ported. Map marks use the existing survivor-map annotation types (danger for checkpoints) rather than separate intelligence layers.

## Checks

Unit tests cover the four positions and their spacing, sandbag cover in the world, the two checkpoints and their posts, the roster, saved-state validation and round trip, the one-time gear crate and the flag image. Native QA stages `04k-remnant-position-landscape` (four rigged soldiers visible, Interact reaches the leader) and `04l-remnant-talk-portrait` (the leader's card with supplies and intelligence) record the result.
