# Android performance controls and verification

The native port now has persistent performance profiles, separate scene/UI resolution, Android capability and thermal observations, an application frame cap, and a local performance report. These controls make a phone's behavior observable and adjustable while preserving the Wanhua world, street detail and gameplay rules.

This document describes the implementation and its verification procedure. It does not certify a phone model, establish a hardware FPS improvement, or report a completed physical-device endurance test. Use the [CI run for the revision being installed](https://github.com/lijiabao1998/DefendTaipei-Android/actions/workflows/ci.yml) for automated results. The distributed phone artifact remains an optimized Rust library inside a development-signed debug APK, requiring Android 12 / API 31+, ARM64 and Vulkan. See [TEST-SIGNING.md](TEST-SIGNING.md).

## Settings and effective behavior

Open **設定 / Settings → 效能 / Performance** from Home or Pause. **畫面設定 / Display** contains the profile, requested frame cap and 3D resolution. **診斷與記錄 / Diagnostics** shows observed frame intervals, dimensions, PSS, thermal status, the effective policy reason, and capture/export controls. Changes apply immediately and save independently of the campaign.

### Profiles

| Profile | Scene scale when resolution is Auto | Frame-cap behavior | Automatic adjustment |
| --- | --- | --- | --- |
| **自動 / Auto** | Android starts at 85%; desktop starts at 100%; low-RAM devices or less than 4 GiB total RAM start at 70% | Uses the supported requested cap; the same memory constraint limits it to 30 FPS | Sustained slow active-gameplay windows can reduce scale, then cap; sustained recovery restores one step at a time |
| **省電 / Battery** | 70% | No more than 30 FPS | Fixed profile; system thermal, battery-saver and memory-pressure protections remain active |
| **均衡 / Balanced** | 85% | Uses the supported requested cap | Fixed profile; the same system protections remain active |
| **畫質 / Quality** | 100% | Uses the supported requested cap | Fixed profile; the same system protections remain active |

The default preference is **Auto, requested 60 FPS, automatic resolution, overlay off**. RAM is used as a conservative memory constraint; it is not treated as a GPU score or a phone-model ranking. Unknown models use reported capabilities and the fallback policy. No manufacturer-specific performance claim is inferred from a model name.

Selecting **70%, 85% or 100%** overrides the profile's ordinary scene scale. In Auto it also holds that resolution against frame-history-driven changes; cap adaptation and system protections remain active. Selecting a fixed profile stops frame-history-driven quality changes; selecting Auto re-enables them. Changing profile, requested cap or scene scale resets the adaptive history. Toggling the overlay preserves the learned adaptive step.

### Frame caps and display modes

The selectable caps are **30, 60, 90 and 120 FPS**. They are requests. The Rust policy chooses a cap at or below that request that evenly divides an Android-reported display mode at the current physical resolution where possible. If no modes are reported, the policy does not assume support above 60 FPS. Unusual display rates can produce a lower compatible divisor outside the four menu choices.

For example, a reported 120 Hz mode supports the policy's 120, 60 and 30 FPS choices; it does not by itself establish a 90 Hz mode. The Diagnostics page shows the resulting target. The Android bridge then requests a supported refresh preference on the UI thread, favoring the lowest suitable multiple of the desired cadence. It releases that preference on focus loss/background and does not request a resolution change.

The bridge uses [`Display.getSupportedModes()`](https://developer.android.com/reference/android/view/Display#getSupportedModes()) and [`WindowManager.LayoutParams.preferredRefreshRate`](https://developer.android.com/reference/android/view/WindowManager.LayoutParams#preferredRefreshRate). Android retains control of actual display behavior. The report therefore records the requested frame rate, applied refresh hint and observed refresh rate separately.

The app cap uses a monotonic deadline at the next main-loop boundary: it sleeps only for the remaining interval when the preceding work finished early. Non-playing screens and Pause are capped at no more than 30 app updates per second. In-game modal panels retain the gameplay cap while their time is excluded from sampling. A slow frame is not given an additional full-frame sleep.

This is an **application-side cap**, not AGDK Swappy integration, a GPU timestamp, or proof of display presentation cadence. Android's [Frame Pacing library](https://developer.android.com/games/sdk/frame-pacing) additionally coordinates presentation timestamps and fences with the display pipeline. That integration is outside this change; high-refresh requests still need physical-device verification for smoothness, latency and sustained power behavior.

### Scene resolution keeps the interface readable

The main 3D camera renders into a reusable texture. A separate presentation camera draws that texture and the UI at the window's full physical resolution. Resizing or rotating updates the texture dimensions while preserving the camera's framing and full-window touch coordinates.

| Selected scale | Scene width and height | Approximate scene pixel count relative to 100% |
| --- | --- | --- |
| 100% | Native window dimensions | 100% |
| 85% | Each dimension × 0.85, rounded to pixels | 72.25% |
| 70% | Each dimension × 0.70, rounded to pixels | 49% |

These percentages describe scene pixels, not measured GPU savings. The full-resolution interface, composition pass, geometry work and gameplay still have costs. A device limited by CPU work may gain little from reducing scene resolution. Text, buttons and touch targets keep their original window coordinates.

## Adaptive policy and system protections

Auto evaluates active-gameplay intervals in windows of at least two seconds. Three consecutive poor windows, with P95 above 125% of the policy's frame budget, lower one adaptive step. That budget uses the lower of the effective cap and a fresh observed display refresh rate; a display remaining at 60 Hz after a 120 FPS request is not by itself treated as rendering overload. Without a usable current refresh observation, the budget uses the effective cap.

The first degradation lowers a 100% starting scale to 85%, or an 85% starting scale to 70%. The second step limits automatic resolution to 70%; the third also limits the cap to 30 FPS. A step can leave visible resolution unchanged when it already meets that ceiling. Recovery requires thirty accumulated seconds of healthy windows before restoring one step. Both the current interval P95 must fit 110% of its budget and the pre-sleep work/wait P95 must fit 80% of the prospective recovered budget. Stable capped 30 FPS alone therefore does not establish capacity for 60 FPS. Intermediate or poor windows interrupt recovery. The pre-sleep observation includes application work, scheduling and rendering waits; it is not a measurement of CPU utilization or GPU execution time.

The policy also reads Android's current observations:

| Observed condition | Effective ceiling |
| --- | --- |
| System battery saver | 30 FPS and 70% scene scale |
| System low-memory signal | 30 FPS and 70% scene scale |
| Thermal status Moderate, or recent headroom ≥ 0.85 | 60 FPS and 85% scene scale |
| Thermal status Severe or higher, or recent headroom ≥ 1.0 | 30 FPS and 70% scene scale |

These are ceilings: a lower user-selected cap or scale remains lower. Manual Quality does not bypass them. The policy reason identifies the active constraint; the saved preference itself is not rewritten by a temporary thermal event.

Thermal recovery is delayed to avoid immediately restoring load when a threshold clears. Severe protection requires fifteen consecutive seconds of fresh, cooler foreground observations before moving to the 60 FPS/85% caution ceiling; caution requires another fifteen seconds before releasing its ceiling. Fresh thermal status is required, and recent headroom, when available, must be below 0.90 to leave severe protection and below 0.75 to leave caution. Missing or stale thermal status, focus/background interruptions and update gaps longer than five seconds reset the cooling window. Slower continuous updates contribute at most one second each. An established thermal ceiling stays in place while readings are unavailable. With uninterrupted qualifying observations, severe-to-normal recovery takes at least thirty seconds; independent battery-saver or low-memory constraints may still apply.

Android thermal status is a platform severity level. Headroom is a dimensionless estimate; 1.0 marks the platform's severe-throttling threshold. Neither value is a CPU, GPU or battery temperature in degrees. The bridge uses `getThermalHeadroom(0)`, so it records the current estimate rather than a future-time forecast. Missing or unsupported values remain unknown. See the official [Thermal API guidance](https://developer.android.com/games/optimize/adpf/thermal) and [`PowerManager` reference](https://developer.android.com/reference/android/os/PowerManager#getThermalHeadroom(int)).

Device snapshots older than ten seconds are marked stale and do not establish a new dynamic thermal, battery-saver or low-memory condition. An existing thermal recovery latch remains subject to the rules above. Headroom additionally needs a sample age no greater than twenty seconds. The diagnostics worker normally updates every two seconds while the activity is resumed; PSS and headroom are sampled no more frequently than every ten seconds. Thermal callbacks and lifecycle updates do not bypass that heavier-sampling limit. Rust reads the validated snapshot without blocking the frame thread on the worker.

## World-work changes and their boundaries

The rendering work budget reduces repeated CPU-side discovery and simultaneous uploads:

- Reuse the immutable world chunk-key allocation. Refresh its nearest-first ordering after four metres of player movement, while checking the requested LOD against the current position on every update.
- Refresh near-detail candidates after one metre of movement, with an extra metre of discovery margin so short movements across the original admission boundary remain visible to the queue.
- Continue draining pending detail work while standing still. Admit at most one detailed building per update at an effective cap of 30 FPS or lower, otherwise two.
- Apply at most one completed chunk upload per Android update, or two on desktop. Keep the existing limit of two asynchronous chunk builds in flight.
- Expose resident/pending chunks, detail buildings, per-update uploads and spatial-query counters for local diagnostics. These are workload counts, not GPU-time or memory estimates.

The existing LOD distances and near-detail admission/removal distances remain in use. The world chunk geometry is retained; this change does not introduce a new finite residency radius, change enemy counts, simplify collision, remove doors, or reduce the final density of street/facade details. Details can take more updates to finish loading under the smaller work budget. Reduced startup or traversal stalls must be measured on the target phone before attaching a numerical benefit to these changes.

## Make and read a performance report

1. Enter or Continue a game. Open **Pause → Settings → Performance → Diagnostics**. Capture is unavailable on the Title screen because it records gameplay.
2. Select **記錄 60 秒 / Record 60 s**. Close Settings, then select **Continue/Resume** on Pause. Starting capture does not silently resume the game.
3. Play the section you want to inspect. The recorder discards at least five seconds of active gameplay as warmup, then records at least sixty seconds of active gameplay. The last complete interval can take either total slightly beyond its nominal boundary.
4. Menus, Pause, modal panels, focus loss, background time and the transition back into active play are excluded. Opening a panel extends the wall-clock time needed to complete the capture. Returning to Title cancels an incomplete capture.
5. After the report is saved, return to Diagnostics and choose **匯出報告 / Export report**. Android opens its normal chooser. Select a destination only when you want to share the file. Cancelling the chooser leaves the local report intact.

Cancelling a capture preserves the previous complete report. A completed report atomically replaces app-private `perf-report.json`, beside the independent settings file. Export copies that report into a dedicated cache subdirectory and shares only that immutable copy through a non-exported FileProvider with temporary read access. The game does not upload telemetry automatically. No serial number, advertising identifier, account information or game-save contents are included. Android's [secure file-sharing guidance](https://developer.android.com/training/secure-file-sharing/share-file) explains the content-URI/read-grant mechanism.

### Report fields and interpretation

| Field | Meaning |
| --- | --- |
| `schema_version`, `build_revision`, `app_version` | Report format and source/build identity |
| `measurement` | Always `app_frame_interval_not_gpu_or_present_time` |
| `source` | `live_session`, or explicitly `fixed_timestep_qa` for the native QA harness |
| `duration_seconds`, `warmup_seconds` | Actual accumulated recorded and discarded-warmup interval durations |
| `frames` | Each observed interval in milliseconds, with its effective target FPS and scene scale |
| `summary` | Sample count, mean interval, reciprocal FPS, P50/P95/P99/max, over-budget count and ratio |
| `device_start`, `device_end` | Android snapshots at the first measured frame after warmup and at capture completion, including sample ages |
| `gpu` | Available render-adapter name, backend, driver and vendor/device metadata; no GPU utilization or timing claim |
| `render_dimensions_start/end` | Physical UI and scene dimensions and scene-scale percentage at the first measured frame after warmup and at completion |
| `preferences_start`, `effective_start/end`, `events` | Requested configuration and effective policy at the first measured frame after warmup, final effective policy, and bounded policy-change history during sampling |
| `interruptions`, `notes` | Count of transitions out of eligible sampling, and accompanying measurement limitations |

The timer uses monotonic `Instant` observations of the application loop. Its intervals include frame-cap sleep, scheduling delays and rendering waits. The reported FPS is `1000 / mean_ms`, not a count of confirmed screen presentations. P95 and P99 use nearest-rank percentiles. “Over budget” means an interval greater than **125% of that sample's effective cap budget**; at 60 FPS the threshold is about 20.83 ms, and at 30 FPS about 41.67 ms. It is an application-interval indicator, not Android's official jank classification.

The Diagnostics overlay shows a bounded recent active-gameplay history. When sampling is paused, its last gameplay statistics can remain visible with the paused label. A sixty-second report is a live observation of what the player actually did. The recorder does not standardize the route, combat, weather or starting temperature, so unrelated captures cannot establish a controlled speedup.

System memory totals/availability are bytes and are not a guaranteed app allocation budget. `process_pss_kb` records process proportional set size in KiB, with its sample age; the UI converts it to MiB. See [`Debug.getPss()`](https://developer.android.com/reference/android/os/Debug#getPss()). PSS and the overall system's available RAM answer different questions.

## Existing settings and save compatibility

The existing version-1 `settings.json` gains an additive `performance` object. Its new portion is shown below; the existing version, audio and game fields remain alongside it:

```json
{
  "performance": {
    "profile": "auto",
    "frame_cap": 60,
    "render_scale": 0,
    "overlay": false
  }
}
```

An older valid version-1 settings document without that object loads the new defaults and retains its audio, language and next-run day length. Reading it alone does not rewrite the file. A subsequent real preference change writes the complete current document. Invalid cap/scale numbers normalize to supported defaults; malformed documents and unsupported file versions retain the existing visible-error behavior.

Performance preferences remain separate from the campaign. They do not start a new run, change saved day length, or delete the game slot. **Reset this page** on Performance restores only performance defaults. Ordinary same-signer APK updates preserve the app's existing preferences and save. The older cross-signer migration helper guarantees the game-save backup/restore; it does not migrate the separate preferences file. See [ANDROID-TEST-UPDATES.md](ANDROID-TEST-UPDATES.md).

## Automated verification

The implementation lives in [`game/src/performance/`](../game/src/performance/), [`device.rs`](../game/src/device.rs), [`perf.rs`](../game/src/perf.rs), [`map/mod.rs`](../game/src/map/mod.rs) and the [Android activity bridge](../android/app/src/main/java/tw/lijiabao/defendtaipei/MainActivity.java). The renderer uses the locally pinned Bevy 0.20.1 APIs for [`RenderTarget::Image`](https://docs.rs/bevy/latest/bevy/camera/enum.RenderTarget.html) and [`IsDefaultUiCamera`](https://docs.rs/bevy/latest/bevy/ui/struct.IsDefaultUiCamera.html); the UI and game cameras have separate destinations. The entire scene target is redrawn, so resizing does not copy or retain the previous texture contents.

Run the repository gates from its root:

```sh
cargo fmt --all -- --check
CARGO_PROFILE_DEV_DEBUG=0 cargo clippy --workspace --all-targets -- -D warnings
CARGO_PROFILE_DEV_DEBUG=0 cargo test -p defend_taipei --lib
python3 -m unittest discover -s scripts/tests -p 'test_*.py' -v
cargo run -p defend_taipei -- --qa
```

Native QA checks the real render pipeline, portrait/landscape performance controls, persisted settings, full-resolution UI and scaled-scene geometry. Settings captures require a visible, correctly sized UI root on the presentation camera and readback pixels matching the opaque settings panel and selected control border; an open-state flag alone cannot pass. It uses the fixed-timestep harness; automatic frame-history adaptation and app-cap sleeping are disabled there. Its render evidence is not a physical-phone benchmark, even though a harness report can contain observed wall-clock intervals.

After the existing disposable-emulator gameplay and audio stages, CI runs:

```sh
timeout 600s python3 scripts/emulator-performance-smoke.py \
  --out emulator-performance-qa
```

The script requires the existing save/preferences produced by those stages and refuses a non-emulator device. It checks actual short touches, cold-restart application of the complete preference document, unchanged saved-game bytes during Title settings, paused settings in both orientations, scene/UI dimensions, an actual live-session report, and the Android share chooser without selecting a recipient. Where the emulator exposes `thermalservice override-status`, it injects Severe, checks the effective policy, resets the override and checks recovery. Injected status is a policy test, not evidence of physical heating. The original audio and update gates remain in the workflow.

The `emulator-smoke` artifact includes the `emulator-performance-qa/` directory with the report, PNGs, runtime logs, private-save/settings evidence and chooser diagnostics. Review `performance-smoke-report.json` and the run conclusion; this document does not substitute a planned check for a successful run. The report validator requires coherent actual samples, not a minimum emulator FPS. Software Vulkan timing on a CI host does not predict ARM64 phone performance.

## Physical-phone verification: repeatable 20-minute sessions

The following is a proposed test protocol, not a completed measurement. Run one twenty-minute session per configuration on the **same phone and APK revision**. Use the same starting campaign checkpoint, fixed screen brightness, orientation, zoom, audio route, system power mode, charge/charging state and ambient conditions. Let the phone return to a comparable idle thermal state between sessions. Keep the game's time speed at 1× and note weather, crowd/combat differences or interruptions.

| Session time | Procedure | Evidence to retain |
| --- | --- | --- |
| Before starting | Record device/OS/build, display modes and selected/effective settings. Choose a repeatable shelter → street → shop → shelter route. | Setup notes; initial Diagnostics screenshot |
| 0–2 minutes | Traverse the route once to allow normal resource preparation. | Loading issues, controls and any unexpected policy changes |
| 2–5 minutes | Capture the same route for at least sixty active seconds; export the first report. | Initial report and route notes |
| 5–15 minutes | Repeat the route, including the same door, inventory, sound and indoor/outdoor actions. Include one rotation and one background/resume cycle at a recorded point. | Visual/input/audio defects; whether the run and settings restore correctly |
| 15–18 minutes | Repeat the capture route with the same zoom and orientation; export a second report. | Sustained-load report, thermal status/headroom and PSS sample ages |
| 18–20 minutes | Save, fully close and relaunch, then Continue and inspect the stored configuration. | Save/Continue result, final Diagnostics screenshot and any failure log |

For an initial baseline, use **Quality, 100%, requested 60 FPS**. Compare with **Balanced, 85%, requested 60 FPS** in a separate session with comparable starting conditions. Auto is useful for a third session to observe policy transitions; Battery provides the fixed 30 FPS/70% alternative. A higher cap is a later comparison only when the device actually reports a suitable mode. These are test configurations, not recommendations based on a measured phone result.

Compare P95/P99, over-budget ratio, the early-versus-late change, effective-policy transitions and visible/touch behavior. If the actual caps differ, state that difference instead of attributing an FPS change solely to rendering optimization. A decrease in scene scale's pixel count alone does not prove a CPU/GPU bottleneck. Use a platform trace or GPU tool when distinguishing main-thread scheduling, GPU work and presentation is necessary.

Two short reports per session can expose a large deterioration, but do not establish all-device compatibility, a memory-leak-free session, battery life, touch-to-photon latency or speaker/Bluetooth latency. Those require their own measurements. Keep the reports and observations attached to the exact source revision, phone, configuration and route before making a device-specific performance claim.
