# Native audio parity

Reference: Web v24.42.0, b9b0a2736642b37dd45925a4719dfef08b8f36b4; src/audio/audio.ts, src/core/main.ts's taipeiSounds, and src/story/broadcasts/broadcast.ts.

## Asset pack

The native pack contains **104 OGG files**, approximately **1.07 MB**: 45 variants derived from the Web's 12 recorded gunshots and three recorded blasts, plus 59 synthetic clips. The exact checked inventory is assets/audio/manifest.json; attribution and source links are in assets/audio/CREDITS.md.

All files are mono 22,050 Hz Vorbis, decoded by Bevy's existing Vorbis support. The generator validates finite, nonempty decoded PCM, RMS and decoded peaks. Lossy compression can overshoot an input peak, so it re-encodes from the original PCM until the decoded peak is at most 0.86. The mixer also limits the sum of effective stream gains to 0.85. This is conservative gain budgeting, not the Web's compressor/limiter DSP.

## Playback behavior

- Pistol, rifle, sniper, machine-gun and shotgun requests randomly choose available recorded variants. Anti-armour retains procedural synthesis. Missing/unloaded recorded samples use the existing procedural fallback if its asset is ready.
- Indoor shots use offline filtered room reflections; distant shots use filtered street reflections. Non-pistol shots beyond 70 m use the distant rifle report, following the Web rule. Distance attenuation and the 260 m gun/600 m blast limits remain; a distant blast is delayed by up to 1.2 seconds.
- Footsteps use actual resolved player and nearby active soldier displacement. Holding movement against a wall, standing still, loading a save or teleporting does not fabricate steps. Homes use wood, other interiors and sidewalks use tile, and the road carriageway uses asphalt. The metro sound remains available for the future metro world. Run/heavy variations and small pitch variations avoid a fixed repeated cadence.
- Existing damage, death, melee, radio, grenade, shell, warning and broadcast messages have distinct cues. Door opening/closing, boarding/removal, pickup, equipment/ammo handling, food/drink, healing, cooking, water, construction, interface confirmation and explicit denial are separate native Foley cues.
- Rain crossfades between outdoor and low-pass indoor clips. Home hum respects the actual power cutoff. Shelter cups, clock ticks, chairs and pot lids follow the Web's 4–11 second life interval.
- Street blasts, loudspeakers, dogs and scooters use a real independent random stream and 6–18 second intervals. Night suppresses daytime loudspeakers/scooters. These distant atmospheric sounds do not invent hostile units or change AI state.
- The garbage-truck phrase follows the saved clock's **19:00–19:12** window and stops permanently at the actual power cutoff. Distance changes across that window. Temple cues use mapped temple names and the existing Longshan courtyard landmark within 70 m; thunder is scheduled during rain above 0.6. The rotor sounds near the actual open native helicopter evacuation site.
- The siren keeps the original three cycles of 15/5/5 seconds with gaps. All-clear stops the alert loop and plays its separate tone. Broadcasts contain radio texture, not spoken Chinese.

Success cues come from successful existing gameplay actions. Searching opens a Rummage cue; actual transfers emit Pickup. Item use distinguishes food, water, treatment and equipment. The Reload timbre currently accompanies ammunition unloading/handling; there is no new magazine or manual reload gameplay. Shelter transfers, prayer, barter and construction emit cues only on successful mutations. Denied/no-op/paused actions never play a success cue. Player melee swishes at swing timing and impacts require actual contact; indoor gun/grenade treatment considers the source or listener roof.

## Controls and lifecycle

audio::AudioSettings exposes master, effects, ambience and radio gains plus mute. The separate settings module persists these preferences independently from the run save. Values are finite/clamped; absent fields default safely. Effects mute does not implicitly mute ambience or radio.

Every player starts at zero gain until the current frame's mix is applied. Menu/pause/background transitions cancel world one-shots and delayed blasts, pause beds, and prevent late-loaded sound from leaking into a different screen. Interface previews can play on title/pause while the app is foreground; no preview is allowed in the background. Muting a bus silences existing sinks and cancels its one-shots. A paused siren keeps its logical position; leaving the run cancels it.

At most **24 one-shot/siren voices**, **32 delayed requests** and **three bed entities** exist. Voices have explicit lifetimes even if no audio sink/device is available; the siren checks tagged entities, not only decoded sinks, so device loss cannot repeatedly spawn loops. Diagnostics use a finite cue vocabulary.

## Verification boundaries

AudioDiagnostics counts requests, dispatched voices, suppressions, dropped requests, recorded/fallback dispatches, source-byte readiness and live sinks. These counters alone are **not proof of audible output**. AudioVoice tags allow the separate audio QA harness to correlate real Bevy dispatch with captured device PCM.

The isolated PCM harness inserts AudioScheduleControl with automatic=false. This suppresses only automatic street, shelter, evening, temple, weather and evacuation scheduling so a random cue cannot make a broken rain test pass. Normal gameplay defaults to automatic=true. The harness still uses the actual asset loader, decoder, AudioPlayer, AudioSink, volume routing, rain beds and lifecycle gates; explicit fixture sound requests are identified as such.

Recording starts only after at least half a second of actual PCM and a later advancing buffer are observed; a monitor that never streams fails a bounded startup wait. The native output fixtures record real scene-start, transition and finish timestamps. Live mute, pause and window-focus loss require audible PCM before the transition and silence afterward; resume requires the inverse. Analysis windows remain within the actual scene across the bounded monitor-clock uncertainty, independent of application teardown. These windows verify routing and lifecycle output, not hardware latency.

The Android emulator uses an explicit authenticated local PulseAudio server and dedicated sink for both QEMU playback and capture. A host preflight checks the actual emulator process's playback stream and sink routing after launch; a missing or muted route fails before the longer gameplay checks. Nine later PCM captures still independently require the expected audible or silent output after real settings touches, including cold restart. Host connection metadata cannot satisfy those audio assertions.

Linux CI starts the full emulator under Xvfb and deliberately omits `-no-window` and `-no-qt`. The upstream [launcher](https://android.googlesource.com/platform/external/qemu/+/ae9d18d2b6261179fbd57fffec720a04f7bfb053/android/emulator/main-emulator.cpp) selects a separate headless binary for those flags; that binary links [PulseAudio stubs](https://android.googlesource.com/platform/external/qemu/+/ae9d18d2b6261179fbd57fffec720a04f7bfb053/audio/paaudio-headless-impl.c) instead of the real host playback implementation. A working host Pulse server alone therefore cannot make the headless binary audible.

Pure/ECS regressions cover settings sanitization, distance limits, actual-motion steps, siren windows, lifecycle admission, blackout/evening boundaries, rain routing, sample/fallback selection, sinkless lifetime cleanup and duplicate-siren prevention. The release should cite the concrete PCM/Android results for its exact commit; this document does not predeclare those outcomes.

## Remaining differences

The Web performs runtime biquad filtering, convolution and compression; the native port uses bounded, pre-rendered variants and gain budgeting. It does not reproduce those graphs sample-for-sample, and there is no HRTF spatialisation. Footsteps classify native surface geometry rather than every Web district's material.

Firecrackers and jet/air-support cues are available to native producers, but unported festival, air-support and metro systems must not be described as implemented just because their sound file exists. There is no speech synthesis or background music soundtrack. No physical-phone loudness calibration, headphone spatial assessment or hardware audio-latency benchmark has been performed.
