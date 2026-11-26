# 上樓: upper floors and the call for help

References: [Web `src/map/upstairs.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/upstairs.ts), the stairwell plan of [Web `src/map/clutter.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/map/clutter.ts), and the upstairs state, `rescueTick`, `rescueFloor` and `rescueTalk` in [Web `src/core/main.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/core/main.ts) (v23.4, v24.1, v24.8). Native implementation: [`map/upstairs.rs`](../game/src/map/upstairs.rs), [`upstairs.rs`](../game/src/upstairs.rs) and [`rescue.rs`](../game/src/rescue.rs).

## Player-facing behavior

- Multi-storey buildings with a door get a stairwell at the back of the ground floor where one fits clear of the furniture and with a route to the door. A flight of steps marks it. Standing at the landing, Interact (**上樓**) climbs to the next floor; on each floor the landing goes down (**下樓**) and the flight beside it goes up, to the top storey (at most 14).
- Each floor is the building's footprint at its real height. The building's walls rise to the floor and stop in a low ring, the roof is cut away, and the floor slab, a parapet ring, the furniture, the flight up and the stairwell opening are drawn at that height. The camera follows.
- Floors have their own deterministic layouts and containers: the second floor is a living floor (sofa, kitchen, fridge, table, bed, wardrobe), middle floors are bedrooms and the top floor of a three-storey or taller building is the shrine hall and storeroom. Containers are `u<building>_<floor>_<slot>` and keep their leftovers in the save like every other container.
- Upstairs counts as indoors. Occupation soldiers cannot follow and lose track of the player; they may still be near the door when the player comes down.
- A save made upstairs records the ground-floor landing; Continue starts there.
- **樓上有人在求救:** at 09:00 each day a text message names the floor of a walk-up (three or more storeys, 80–320 m away, not the player's home) near the nearest named street, and marks a rough spot within 15 m on the survivor map. The family is on that floor of that building: climbing there finds the mother ("是誰？……拜託，是來救我們的嗎？"). Talking to her sends the family to the shelter after dark together with a survivor who had not arrived yet, raises karma and every survivor's mood by six, gives two cans and a bandage, and turns the map mark safe. If nobody comes, the next 09:00 the message "never answered again", morale drops by four and a new call arrives.

## Boundaries

The Web's v24.30 two-pass rendering of the real street from an upper floor, walkable rooftops, rooms divided by partitions, indoor light pools and shooting between floors are not ported. Upper floors use the existing furniture models.

## Checks

Unit tests cover the stairwells (count, ownership, clear of ground furniture), per-floor furniture and container ids, determinism, floor collision that keeps the player inside and out of furniture, the rescue pick (a real walk-up floor and street, distance, seeding, validation) and the floor wording. Native QA stage `04o-upstairs-portrait` climbs the nearest furnished walk-up by real Interact messages and checks floor, height, fixtures and that soldiers lose the player; `04p-rescue-portrait` climbs to the day's call, finds the family and answers it.
