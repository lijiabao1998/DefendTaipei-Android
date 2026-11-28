# Wave 6 — PLA Helicopters Parity (`v24.24`)

Ports `C:\RUST\defendtaipei\src\systems\heli.ts` (`HeliPatrols`, `makeHeli`, `Z-20` / `Z-10`) into native Rust/Bevy (`game/src/heli.rs`).

## Implemented Behaviour
- **Variants**:
  - `Z-20 通用直升機` (`HeliKind::Z20`, 100 HP): utility transport frame with door gun bursts (`8–12` dmg, attenuated by Wave 5 armored vehicles to 10%).
  - `直-10 攻擊直升機` (`HeliKind::Z10`, 90 HP): narrow tandem-cockpit gunship with stub wings, rocket pods, chin cannon, and periodic rocket salvos dispatched through `WarCall::Strike`.
- **Flight & Escalation**:
  - Escalates with `war::war_level(campaign.game_min, campaign.reclaimed.len())` (`0` below `WAR_START`, up to `3` active aircraft at peak war pressure) over wide Wanhua road routes (`width >= 9 m`) at rooftop clearance (`max(cruise/hunt, roof + 7 m)`).
  - Night searchlight uses unlit additive beam cone and ground pool meshes (`NotShadowCaster`), keeping `lighting_workload`'s 6-spotlight budget strictly untouched (`0` dynamic `SpotLight`/`PointLight` entities spawned).
- **Detection, Cover & Return Fire**:
  - Detects exposed players within `34 m` (day) / `12 m` (night) after `1.6 s` / `0.9 s` spot delay, triggering an Android haptic warning and HUD alert; breaks off after `6 s` hidden (`war::hidden_from_sky`, indoors, upstairs, or under arcades) or beyond `90 m`.
  - Damaged by player heavy/aimed fire (`Gunshot`) and mounted vehicle weapons; crashing choppers spiral down onto street/roof height, detonate, increment `HeliStats::downed`, and spawn a scavengeable `helidrop:<n>` supply crate.
- **Deterministic Tests & QA**:
  - `heli::tests` verify route selection, exposure/hide break-off, anti-air shootdown + `helidrop` creation, and zero `SpotLight`/`PointLight` entities.
  - QA fixture helper `heli::spawn_qa_heli` supports deterministic desktop visual QA verification.
