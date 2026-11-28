# 日本支援: the JSDF liaison and field radio

References: [Web `src/systems/japan.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/systems/japan.ts) and `liaisonSpot`, `setupJapan`, `talkLiaison`, `useJpRadio`, `jpCall`, `jpLanded` and `blast` in [Web `src/core/main.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/core/main.ts). Native implementation: [`japan.rs`](../game/src/japan.rs) with [`dialogue.rs`](../game/src/dialogue.rs).

## Player-facing behavior

- Captain Takahashi (fictional JGSDF liaison officer, Type 20 rifle, type III greens) waits 1.6 m inside the door of the lowest-id core building 140–280 m from the start. He turns to face the player who comes in.
- The ROC remnants' intelligence marks him on the survivor map; the Japanese expat survivor knows from the start.
- The first conversation hands over the **自衛隊野戰無線電** and two batteries. Later he has his own four lines, and gives one battery a day when the player has none.
- **Use** on the radio in the bag opens its card: the battery count and three calls with their remaining cooldowns. Each successful call spends one battery, ends accelerated time and puts the radio line on the radio card:
  - **C-2 airdrop** (6 min): a 44 m transport passes along the fixed track at 70 m, drops a crate on a parachute 22 m ahead of the player on the nearest open ground; it descends at 7 m/s, lands under ten red smoke puffs that last 90 s, is marked on the map and holds a Type 20 or Type 89, 60 rounds of 5.56, three JSDF rations, a first-aid kit, two waters and a battery, with the Web chances of a 9 mm pistol and rounds, a MINIMI or an LMAT. Patrols within 50 m see the parachute.
  - **F-2 strike** (5 min): a pair of F-2s passes at 42 m over the densest 解放軍 group 18–150 m away (most soldiers within 12 m of one). Three bombs fall along the track; each does 220 to soldiers within 6 m and up to 45 to a player within 6 m. No target in range means no sortie and no battery spent.
  - **AH-64DJ cover** (4 min): the gunship flies in over seven seconds, orbits the player at 22 m and 18 m altitude for about 45 s, firing its 30 mm gun every 0.9 s at the nearest soldier within 55 m (60% hits, 45 damage), then climbs away.
- Calls are heard by patrols within 60 m (the airdrop excepted). Jet, rotor, gun and blast sounds use the existing audio buses. All aircraft carry procedural 日の丸 roundels.

## Persistence

Whether the liaison is known or met, the conversation counter, the day of the last spare battery, the three cooldowns and the drop counter are a defaulted `japan` section of the native save. Landed crates are ordinary world drops named **日本空投物資箱**. Aircraft in flight and smoke are not saved, as in the Web.

## Checks

Unit tests cover the liaison building rule, the strike target (danger-close and out-of-range soldiers ignored), the airdrop contents and item ids, and cooldown validation.
