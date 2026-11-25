# Licenses

## Defend Taipei UI

`fonts/DefendTaipeiUI.otf` is a subset of **Noto Sans CJK TC** (Regular),
Copyright 2014-2021 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source Han Sans'.
Copyright 2014, 2015, 2018, 2019, 2020, 2021 Google LLC, with Reserved Font Name 'Noto Sans CJK'.

The subset keeps the characters drawn by the game, including story copy.
`tools/subset_ui_font.py` rebuilds it from the Rust string literals plus
°←↑→↓▶. `fonts/NotoSansTC-Subset.otf` is that same subset for the asset
server. The embedded file is named "Defend Taipei UI" so the reserved names
are not reused.

This Font Software is licensed under the SIL Open Font License, Version 1.1.
The license is reproduced below. This is not a sale of the font.

## SIL Open Font License 1.1

Copyright 2014-2021 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source Han Sans'.
Copyright 2014, 2015, 2018, 2019, 2020, 2021 Google LLC, with Reserved Font Name 'Noto Sans CJK'.

This Font Software is licensed under the SIL Open Font License, Version 1.1.
This license is copied below, and is also available with a FAQ at:
https://openfontlicense.org

SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font creation
efforts of academic and linguistic communities, and to provide a free and
open framework in which fonts may be shared and improved in partnership
with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded,
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply
to any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may
include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components as
distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to a
new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,
in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless explicit written permission is granted by the corresponding
Copyright Holder. This restriction only applies to the primary font name as
presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created
using the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.

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
