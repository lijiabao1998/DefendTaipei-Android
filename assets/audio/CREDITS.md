# Audio sources and transformations

Reference: [Defend Taipei Web v24.42.0](https://github.com/lijiabao1998/defendtaipei/tree/b9b0a2736642b37dd45925a4719dfef08b8f36b4), pinned commit b9b0a2736642b37dd45925a4719dfef08b8f36b4. See its CREDITS.md, src/audio/audio.ts and src/story/broadcasts/broadcast.ts.

## Recorded gunshots

The Free Firearm Sound Library, recorded by **Ben Jaszczak, Brian Nelson, Kevin Heras and Matthew Nanney**.

- Source: https://opengameart.org/content/the-free-firearm-sound-library
- License: **CC0 1.0**, https://creativecommons.org/publicdomain/zero/1.0/
- Imported from the existing pinned Web public/sfx/gun/*.mp3, not downloaded from an unidentified mirror.
- Source page author/license listing independently checked on 2026-11-26.

| Web files | Original recording named by Web credits |
| --- | --- |
| pistol1, pistol2 | Walther PPQ / X_39P.wav, first and second shot |
| rifle1, rifle2 | AR-15 / D_32P.wav, first and second shot |
| rifle3 | AK-47 / C_28P.wav, first shot |
| sniper1, sniper2 | Tikka / W_29P.wav, first and second shot |
| mg1, mg2 | Mosin Nagant / M_21P.wav, first and second shot; the Web uses these as machine-gun cues |
| shotgun1, shotgun2 | Nova / O_21P.wav, first and second shot |
| far1 | AR-15 / D_24P.wav, distant report |

## Recorded blasts

**rubberduck**, *25 CC0 bang / firework SFX*.

- Source: https://opengameart.org/content/25-cc0-bang-firework-sfx
- License: **CC0 1.0**, https://creativecommons.org/publicdomain/zero/1.0/
- Pinned Web files public/sfx/boom/boom1.mp3, boom2.mp3, boom3.mp3 derive from cannon_01.ogg, cannon_02.ogg, bang_06.ogg, respectively.
- These are processed firework/cannon recordings used as game blasts, not recordings of military shell explosions.
- Source page author/license listing independently checked on 2026-11-26.

## Native conversion and synthetic cues

tools/generate_audio.py converts each of the 15 existing Web MP3 recordings into three mono 22,050 Hz Vorbis variants: dry, filtered short room reflections, and filtered distant street reflections. These 45 files have a rec_ prefix. Reflection delays are rendered offline; this is not WebAudio's live convolution. The generator measures the actual decoded peak and re-encodes when needed to keep every clip below 0.86.

The remaining 59 clips are synthesized by the generator from noise, oscillators and the Web motifs, with additional native door, construction, item and interface Foley. They use the project's **CC0-1.0** procedural-audio dedication. No voice actor, cloned speaker, speech recording or external musical recording is included. The garbage-truck phrase is synthesized from the note sequence in the pinned Web source; that source identifies the melody as Beethoven's *Für Elise*.

manifest.json records every output's SHA-256, decoded peak/RMS, duration, channels and sample rate. Each recorded derivative also records the exact source path and source-file SHA-256. The generator can be rerun with:

~~~sh
python3 tools/generate_audio.py --web-root /path/to/pinned/defendtaipei-web
~~~

The recorded samples are already processed derivatives from the Web repository. This port does not claim restoration of their original raw recording fidelity.
