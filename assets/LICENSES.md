# Asset licenses

Every file under `assets/audio/` is an original procedural rendering of the cues the web build synthesizes in `src/audio/audio.ts` and `src/story/broadcasts/broadcast.ts`. No third-party recording is included. The optional CC0 gunshot library used by the web build is not shipped; these files are the same synthesizer's fallback.

Each file is dedicated to the public domain under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). Generator: `tools/generate_audio.py`. Author: DefendTaipei project. There is no upstream sample URL.

| File | Bytes | What it stands in for | License |
| --- | ---: | --- | --- |
| `audio/gun_pistol.ogg` | 5,805 | `gunshot('pistol')` fallback | CC0-1.0 |
| `audio/gun_rifle.ogg` | 6,682 | `gunshot('rifle')` fallback | CC0-1.0 |
| `audio/gun_sniper.ogg` | 9,537 | `gunshot('sniper')` fallback | CC0-1.0 |
| `audio/gun_mg.ogg` | 6,294 | `gunshot('mg')` fallback | CC0-1.0 |
| `audio/gun_shotgun.ogg` | 7,873 | `gunshot('shotgun')` fallback | CC0-1.0 |
| `audio/gun_at.ogg` | 14,782 | `gunshot('at')` fallback | CC0-1.0 |
| `audio/step_tile.ogg` | 4,368 | `footstep('tile')` | CC0-1.0 |
| `audio/step_asphalt.ogg` | 4,394 | `footstep('asphalt')` | CC0-1.0 |
| `audio/step_wood.ogg` | 4,309 | `footstep('wood')` | CC0-1.0 |
| `audio/step_metro.ogg` | 4,411 | `footstep('metro')` | CC0-1.0 |
| `audio/hurt.ogg` | 4,969 | `hurt()` | CC0-1.0 |
| `audio/death.ogg` | 5,938 | `death()` | CC0-1.0 |
| `audio/swish.ogg` | 4,695 | `swish()` | CC0-1.0 |
| `audio/melee.ogg` | 4,471 | `meleeHit()` | CC0-1.0 |
| `audio/radio.ogg` | 6,542 | `radio()` squelch and beeps | CC0-1.0 |
| `audio/clink.ogg` | 3,967 | `clink()` | CC0-1.0 |
| `audio/rummage.ogg` | 7,618 | `rummage()` | CC0-1.0 |
| `audio/alert_test.ogg` | 5,802 | `alertTest()` three-note chime | CC0-1.0 |
| `audio/siren.ogg` | 10,413 | 萬安 rotor loop used by `airRaid('alert')` | CC0-1.0 |
| `audio/all_clear.ogg` | 12,365 | `airRaid('clear')` tone | CC0-1.0 |
| `audio/whistle.ogg` | 5,303 | `whistle()` | CC0-1.0 |
| `audio/boom.ogg` | 15,422 | `shellBoom()` fallback | CC0-1.0 |
| `audio/amb_explosion.ogg` | 15,846 | distant `explosion()` in `ambience()` | CC0-1.0 |
| `audio/amb_dog.ogg` | 4,948 | `dog()` | CC0-1.0 |
| `audio/amb_scooter.ogg` | 8,038 | `scooter()` | CC0-1.0 |
| `audio/amb_chime.ogg` | 5,436 | loudspeaker `broadcast()` chime | CC0-1.0 |
| `audio/rain.ogg` | 17,323 | rain bed | CC0-1.0 |
| `audio/room.ogg` | 19,496 | shelter room tone | CC0-1.0 |
| `audio/radio_bed.ogg` | 20,466 | AM hiss bed behind a broadcast | CC0-1.0 |

Media total: 247,513 bytes (about 242 KiB), mono OGG Vorbis.
