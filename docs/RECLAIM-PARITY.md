# Taking back the Wanhua core

References: `SECTORS`, `sectorOf`, `checkSectors`, `reclaimParty` and `reclaimDawn` in [Web `src/core/main.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/core/main.ts), and `reclaimTitle` in [Web `src/systems/reclaim.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/systems/reclaim.ts). Native implementation: [`sectors.rs`](../game/src/sectors.rs), [`survival.rs`](../game/src/survival.rs) and [`hud.rs`](../game/src/hud.rs).

## Player-facing behavior

- Wanhua is divided into the Web's fifteen named areas. A point belongs to the area whose anchor is nearest. The HUD's location line names the area the player stands in and marks it **（佔領）** while occupied, **★** once taken back.
- Each of the 64 street patrols belongs to the area of its starting point. Night patrols count too, exactly as `checkSectors` counts them, so an area with night patrols can only be finished after dark. Areas with no soldiers are never "taken".
- When the last soldier assigned to an area dies — by the player's hand, the ROC remnants or JSDF support — that area is reclaimed:
  - a centred title **★ <area> 奪回來了** with one of the four Web lines fades in for 1.6 s, holds and fades out by about 8 s, with the Web's responsive font sizes;
  - firecrackers and the temple bell play;
  - the neighbours press one of the four Web gifts into the bag (bento/water/bandage, hot noodles/boiled eggs/can, mantou/dried tofu/water, braised pork/cans/bandages);
  - karma rises by one and every shelter survivor's mood by eight (capped at 100).
- Every later 06:00, before the day's meals are counted, the shelter receives one rice per two reclaimed areas and one water per three. The journal records the delivery.
- Reclaimed areas persist in the existing campaign save. A Continue or new run is silent: an area already taken in the save is not celebrated again.

## Boundaries

The six areas whose anchors lie in the hand-built core (西門町, 中華路, 康定路, 龍山寺, 貴陽街, 華西街) can be taken here. The nine outer areas are garrisoned outposts on streamed district tiles in the Web build. Those tiles are not ported, so the native campaign keeps the Web's fifteen-area ending rule but cannot complete it yet. Street dressing on reclaimed districts (banners while occupied, flags and neighbours afterwards) is a later phase.

## Checks

Unit tests cover the campaign order of the names, the six core anchors, nearest-anchor assignment against real street positions, "every soldier dead" liberation (including empty areas), the dawn ratios and stash delivery, the gift items and the title fade. Native QA stage `04m-reclaim-title-landscape` kills every soldier of the least-defended core area through real `EnemyDied` messages and checks the reclaimed list, the title and the exact gift added to the bag.
