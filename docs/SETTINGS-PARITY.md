# Audio and game preferences

Reference: Web v24.42.0 `src/ui/settings.ts` and the new-game day-duration choices in `src/core/main.ts` (`b9b0a2736642b37dd45925a4719dfef08b8f36b4`).

## Playable controls

Home has a **設定 / Settings** button between Controls and About. Pause has the same entry beside its heading. Both open the same responsive overlay, with Audio and Game & language tabs. Closing settings from Pause keeps the world paused and returns to the pause screen. Opening or closing this overlay never starts a new run or replaces the game slot.

Audio offers four live sliders, plus five-percentage-point decrement/increment buttons:

| Preference | Default | Effective gain |
| --- | ---: | --- |
| Master | 80% | Shared multiplier |
| Effects | 100% | Master × effects |
| Music / ambience | 70% | Master × ambience |
| Radio effects | 80% | Master × radio |

Mute temporarily sets every effective gain to zero while retaining the four slider values. The Effects, Ambience and Radio audition buttons use the corresponding real audio category and honour both gain and mute. They are allowed from Home and Pause; app-background suppression still applies. Radio here means radio effects and the broadcast bed, not generated speech. The native port has no speech-synthesis voice toggle.

The Game & language tab switches Traditional Chinese/English in place and selects **1, 2 or 4 real hours per in-game day at 1×**. Day duration is a preference for the **next new run**. Depart copies it into `Campaign.day_hours`; Continue retains the value in the existing save. The departure briefing displays the selected duration. During a run, the settings panel states both the current run's duration and that the new choice applies on the next departure.

**Reset this page** resets only the active tab. Resetting Audio restores the four Web-aligned gains and unmutes. Resetting Game & language selects Traditional Chinese and a one-hour day. Neither reset touches the game slot.

## Persistence

`settings.json` is a versioned preferences file separate from the permadeath save. Android stores it in the app-private internal data directory. Desktop uses the per-user configuration directory, or `DEFEND_TAIPEI_SETTINGS_DIR`; `DEFEND_TAIPEI_SAVE_DIR` also isolates preferences for QA. The `--qa` run defaults to its disposable output/save directory.

Changes are applied immediately and written after 0.3 seconds without further edits. Done/back flushes pending changes. Losing focus, `WillSuspend` / `Suspended`, and application exit also flush pending changes. Writes use a unique temporary file, write and fsync, decoded read-back verification, atomic rename and (on Unix) directory fsync. A failed write leaves a visible settings status and retries; it never claims success before the write completes.

Missing preferences use the defaults above. Partial version-1 data uses field defaults. Gain values saturate to 0–1, invalid day durations become one hour, and unknown language codes become Traditional Chinese. Malformed, oversized and unknown-version files are reported rather than silently accepted; merely starting the app does not overwrite them. The independently saved run is never modified by preferences I/O.

Ordinary same-signer Android replacement preserves the app's private files. The separate cross-signer update helper's documented guarantee concerns the game save; preferences are not part of that older helper's backup format unless explicitly added to its migration scope.

## Input and layout

Both the Home/Pause entry buttons and the overlay use ordered raw mouse/touch messages so DOWN and UP in one low-frame-rate frame are not lost. Two short taps reusing Android pointer 0 are handled separately. Slider drags are cancelled on pointer cancellation, viewport rotation, focus loss or app suspension; a stale move or release cannot jump the slider afterward. The overlay keeps the closing press captured so the same press cannot operate a button underneath it.

Portrait uses four stacked gain rows. Wide landscape uses a two-by-two layout. The UI provides dedicated hit rectangles for each slider, +/- button, audition, language, day-duration and close/reset action. `DTSETTINGS layout=...` records the actual coordinates on opening/resize for emulator QA; tests and native visual QA use the same layout/actions. Existing Home New game/Continue and Pause Resume/Save-and-menu targets keep their previous positions.

## Scope

This wave does not expose unsupported Web options. Graphics presets, vibration, pointer sensitivity, touch-pad sizing and custom keyboard bindings are not implemented. Settings do not alter story dialogue language or add speech synthesis. The category mix and measured audio-output coverage are documented in `AUDIO-PARITY.md` and the audio QA artifacts.
