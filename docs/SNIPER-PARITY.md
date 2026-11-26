# 狙擊手: rooftop snipers over the main junctions

References: [Web `src/systems/sniper.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/systems/sniper.ts) and `setupSnipers`, `snpEye` and `roofDoor` in [Web `src/core/main.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/core/main.ts) (v22.5). Native implementation: [`sniper.rs`](../game/src/sniper.rs).

## Player-facing behavior

- Up to five posts: every crossing of two named main roads (西園路, 和平西路, 康定路, 中華路, 環河南路, 桂林路, not their lanes), at least 60 m apart and sorted the Web way; each post is the roof corner nearest the junction, 0.6 m inside the parapet, of the tallest building 9 m or taller 12–50 m from it. The sniper holds a QBU-88.
- By day (06:00–18:30), a player within 60 m of the junction (and 85 m of him) whom he can see is locked: a glint flashes and a red laser holds on them for 1.6 s, then he fires (38–47 damage, half the time bleeding) and works the bolt for 3.5 s. Indoors and under arcades, upstairs, within 1.8 m of an abandoned car or bus, or with a wall between him and the player (his eye is 1.4 m out past the parapet), the lock breaks. The first lock marks his junction on the survivor map; the lock warning ends accelerated time.
- Aimed gunfire within 10° of him and in range (160 m with a scoped rifle, 70 m otherwise) with a clear line hits him — 80% with a scoped rifle, which kills outright, 30% for 40 damage otherwise. At his building's door, Interact (**爬頂樓**) climbs up and overpowers him.
- A cleared post is safe by day, counts as a kill, turns his map mark safe and leaves 5 rounds of 7.62 and a bandage at the door. Cleared posts are saved.

## Boundaries

The Web's separate first-person scope view, bullet drop, the morning moral choice in the scope and window posts on stair buildings (v24.31) are not ported; the climb is resolved at the door rather than by walking the roof.

## Checks

Unit tests cover the post rule (tall buildings near real junctions, eye outside the roof, determinism) and the daytime window. Native QA stage `04r-sniper-landscape` stands at a junction a sniper can see and records the lock.
