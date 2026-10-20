# Sounds shipped in Fontelle

Every soundfont and every set of recordings Fontelle carries is listed here,
with whose it is and the licence that lets us pass it on. The same list is
compiled into the program (`crates/fontelle-core/src/bundled_sounds.rs`),
which carries each licence's text, and a test
(`crates/fontelle-core/tests/bundled_sounds.rs`, run by CI) fails the build
when a `.sf2`/`.sf3`/`.sfz`/`.sfark`/`.dls` anywhere in the tree, or any
recording under `assets/`, has no entry here.

## The rule

A sound is shipped or re-hosted only under one of these, each of which lets
anyone copy, change and pass the work on, commercially, asking at most a
credit:

| Licence | Identifier |
|---|---|
| MIT | `MIT` |
| CC0 1.0 | `CC0-1.0` |
| Creative Commons Attribution 3.0 | `CC-BY-3.0` |
| Creative Commons Attribution 4.0 | `CC-BY-4.0` |
| The author's written public-domain dedication | `LicenseRef-Public-Domain` |
| Apache 2.0 | `Apache-2.0` |
| GeneralUser GS's own licence (S. Christian Collins), its text as the font ships it | `LicenseRef-GeneralUser-GS` |
| Made in Fontelle from no outside material: the workspace's own | `MIT OR Apache-2.0` |

Refused, and why:

- **Non-commercial (`NC`)** — Fontelle and its packs are sold.
- **No derivatives (`ND`)** — every shipped sound is trimmed, resampled or
  rebuilt, and a pack is a changed work.
- **Share-alike (`SA`) and GPL/LGPL** — they may be passed on, but they can
  bind a pack or the program built on them to the same licence. Not a
  default; adding one is Ty's decision, sound by sound.
- **"Free for personal use", "freeware", no stated terms** — grants no right
  to pass it on.

A font that cannot be shipped can still be offered: the in-app library
downloads it from its author's own address after showing its licence
(`docs/soundfont-suite.md` §4).

## The sounds

### `salamander-grand-piano`

| | |
|---|---|
| What | The factory Grand Piano's 90 stereo recordings (`assets/flopsynth/samples/grand-stereo/`), sets `grand-left` and `grand-right`; and the earlier 46 mono ones (`assets/flopsynth/samples/grand/`), sets `grand-soft` and `grand-hard`, which the remade pianos in Sampled Keys play |
| Work | Salamander Grand Piano (Yamaha C5): the stereo cut from the SFZ+FLAC edition (V3+20200602), the mono one from the SF2 build, both assembled by Roberto (zenvoid.org) for FreePats |
| Author | Alexander Holm |
| Source | <https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html> |
| Licence | Creative Commons Attribution 3.0 (`CC-BY-3.0`); text in [`sounds/salamander-grand-piano/LICENSE`](sounds/salamander-grand-piano/LICENSE) (the SPDX copy of the CC BY 3.0 Unported legal code) |
| Changes | Stereo: layers 3, 7 and 14 kept, each note trimmed to 1.6–6 s with a fade, each key's channels levelled along the keyboard, layers level-matched, encoded as Ogg Vorbis (`assets/flopsynth/samples/grand-stereo/README.md`). Mono: each note played through Fontelle's sampler, trimmed to 1.3–4 s with a fade at the end, resampled to 24 or 32 kHz, summed to mono, scaled once per layer (`assets/flopsynth/samples/grand/README.md`) |

The FreePats page states the licence as Creative Commons Attribution 3.0
(checked 2026-10-19), and so does the SFZ+FLAC package's own `readme.txt`,
read when the stereo cut was made: *"The original Salamander Grand Piano was
created by Alexander Holm. Published under the terms of the Creative Commons
Attribution 3.0 license"*. The package carries no licence text of its own,
so this text is the licence's standard one.

### `fontelle-kits`

| | |
|---|---|
| What | The factory Studio and 808 kits' 72 recordings (`assets/flopsynth/samples/kit/`), sets `kit-studio` and `kit-808` |
| Author | Fopull LLC — Fontelle's own drum machine played through its kit bus |
| Licence | `MIT OR Apache-2.0`, the workspace's own ([`LICENSE-MIT`](../LICENSE-MIT), [`LICENSE-APACHE`](../LICENSE-APACHE)); no outside material |
