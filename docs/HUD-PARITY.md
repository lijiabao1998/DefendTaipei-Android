# Survival HUD, item cards and received reports

The reference is Web **v24.42.0**, pinned to
[`b9b0a2736642b37dd45925a4719dfef08b8f36b4`](https://github.com/lijiabao1998/defendtaipei/tree/b9b0a2736642b37dd45925a4719dfef08b8f36b4).
This wave follows the Web game's dark instrument cards, warm headings, four survival
bars, condition labels, category-specific inventory metadata and readable broadcast
history. It does not add fake objectives, invented item effects or placeholder menu
options.

## Source map

| Web reference | Native implementation |
| --- | --- |
| [`drawHud` and `bar` in core/main.ts](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/core/main.ts) | `game/src/hud.rs`: health, fullness, hydration and energy; live condition labels; clock, weapon/ammunition, load and utility status |
| [`#stats`, `.moodles`, `#prompt` and `#phone` in style.css](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/ui/style.css) | Shared responsive HUD/radio geometry, contextual action hint and report-card input capture |
| Inventory cards and `radioLogHtml` in the same core source | `game/src/ui/shelter.rs`: differentiated item cards, selected tabs, received timeline reports and daily shelter records |
| [`ui/menu.ts`](https://github.com/lijiabao1998/defendtaipei/blob/b9b0a2736642b37dd45925a4719dfef08b8f36b4/src/ui/menu.ts) | `game/src/ui/pause.rs`: run summary, explicit paused state, Resume and Save and return to menu |

## Actual state, not decorative status

The four meters read `GameState.hp`, hunger, thirst and **100 − fatigue**. They use the
Web palette: red, ochre, blue and violet. A warning line appears only for active
bleeding, severe injury, hunger, thirst, exhaustion, sleep, wet ground or the active
sneak/cover stance. The original survival thresholds remain intact. Native wetness describes the ground;
it does not claim that the player is soaked, cold or suffering an unimplemented effect. The clock distinguishes
18:00 nightfall from the announced 20:00 curfew. Equipped weapon and ammunition use
the same inventory/weapon resources that combat consumes.

The contextual prompt and Interact toolbar label read `worldplay::InteractionHint`.
That resource is calculated by the same target selector that executes Interact, so
the UI does not independently guess whether a door, cabinet or water fixture is the
next action. Hint text stays clear of the movement controls and map button. HUD and
radio hit regions are removed while a panel, map, pause screen or non-playing flow
is active.

## Item cards and panels

Items display the actual carried count, category, total stack weight and catalog
description. An equipped weapon has a highlighted card and an Unequip action.
Category badges are native text and colour, not a new image download or per-frame
texture render. Item use, construction, deposit, prayer and barter still call the
existing gameplay functions. Disabled actions remain non-interactive. Selected tabs
are visually distinct from unavailable actions.

The five-button toolbar and existing shelter action coordinates are preserved. The
shelter and inventory screens continue to paginate based on the viewport and actual
text extent estimates. Their clocks keep running, while the separate pause screen
stops time. The pause card shows the current run day, clock and HP; its two main
buttons retain their previous touch positions.

## Reports and persistence

Tapping a visible radio/phone card opens the Journal. It lists **only received timed
reports**, newest first, followed by the shelter's existing daily log. A report is
eligible only when its ID is in the saved `Campaign.fired` set. Advancing the display
clock alone does not reveal unreached reports. The displayed report time is the
original scheduled time, not an invented exact reception timestamp.

History therefore survives Continue using the existing atomic save; no parallel
history file is added. The original Chinese report text remains unchanged in either
interface language. Manual debug story events and prologue phone messages are not
included in this derived timeline history. This is a native subset of the Web radio
log, not the complete free-form Web broadcast archive.

## Verification

Focused checks cover energy inversion and clamping, warning thresholds, HUD/radio
bounds, hiding both instruments and their input regions behind modal/pause screens,
item action fidelity, and received-report history after campaign serialization. The
existing touch tests still cover same-frame pause and disabled actions. Visual QA
must inspect the actual portrait and short landscape screens and the received-report
panel; CI outcome for the concrete revision is the final validation record.
