# Fontelle as a SoundFont suite

Hub card 0430, written 2026-10-19. Ty wants Fontelle to be a full
SoundFont suite as well as a DAW, and to sell packs made with it. This is
the design for the four pieces the card names. **§3 is built**; §1, §2 and
§4 are proposals, and the decisions marked **Ty** are his. Nothing here is
built beyond §3 until he has chosen; each piece then gets its own card
(§5).

---

## 0. What a soundfont is inside Fontelle today

Worth saying first, because it decides the audio approach.

Fontelle does not run a SoundFont synthesiser. Opening a `.sf2` **turns one
preset into a Fontelle patch** (`fontelle-assets/src/sf2_import.rs`,
`import_sf2_preset`): every zone becomes a sample layer, and the volume
envelope becomes the patch's one amp envelope, `envelopes[0]`, read from the
**first** zone that has a sample (`sf2_import.rs:434-450`). The channel then
keeps its own copy of that patch (`Channel::patch_data`), and the voice plays
it with Fontelle's own envelope (`fontelle-dsp/src/envelope.rs`).

Three things follow:

- The "generator" is ours. "Apply the envelope after the generator" and
  "rewrite the preset" are both options on our own code, not on a black box.
- One envelope per patch: a font whose zones have different envelopes (a
  piano whose low notes ring longer) is flattened to the first zone's
  today. That is a known, documented cut (`sf2_import.rs:304-317`), and it
  matters for §1.
- `.sf3` cannot be opened yet (`fontelle-app/src/bank.rs:104-113`): the
  importer reads only uncompressed samples.

---

## 1. Envelope overrides: shorter as well as longer

### Why a note's release can only get longer today

A note carries `release: u8` (`fontelle-model/src/note.rs:18`), passed on the
note-on (`fontelle-sequencer/src/compile.rs:384`) to the voice, which turns
it into a multiplier:

```rust
// fontelle-core/src/voice.rs:1330-1335
// `0` is the patch's own release and 127 is four times it. Only ever
// longer, because `0` is what every note ever written carries and a
// property whose default rewrote existing projects is not one worth
// having — see `EventPayload::NoteOn`.
self.note_release_scale =
    1.0 + (release.min(127) as f32 / 127.0) * (MAX_NOTE_RELEASE - 1.0);
```

So the range is 1× to 4× (`MAX_NOTE_RELEASE`, `voice.rs:859`), and **0 means
"the font's own"**. That last part is the real reason it cannot shorten:
every note ever saved has `release: 0`, so re-centring the byte (say 64 =
own, below = shorter) would make every existing song's notes cut short the
day it shipped. The rule is written down at `fontelle-types/src/event.rs:29-36`.
The FL importer hits the same wall and throws away FL's "shorter" half
(`fontelle-assets/src/fsc_import.rs:431-442`).

The per-instrument side already exists in one form: the instrument panel's
**Amp envelope** knobs (`fontelle-app/src/instrument.rs:201-219`) edit the
channel's copy of the patch, so an instrument's attack, decay, sustain and
release *can* already be shortened, as absolute times.

### The audio approach: scale the voice's own envelope (recommended)

Two ways to make a note shorter than the font says:

| | Scale the voice's envelope | A second envelope after the voice |
|---|---|---|
| How | Each voice keeps its own copy of the envelope settings with the overrides applied — exactly what the release lengthening already does (`voice.rs:1813-1819`) | Multiply the voice's output by an extra envelope of our own |
| Shorten | Yes | Yes |
| Lengthen | Yes | **No** — it cannot bring back sound the font's envelope already faded |
| Clicks | None: a release starts from whatever level the note is at (`Envelope::note_off`, `envelope.rs:220-225`), and the envelope reads its settings every sample | Two curves multiplied; a shortened release under a long font release sounds like a gate |
| Cost | Nothing new on the audio thread | A second envelope per voice |
| Fits SF2's dB curves | Yes: the font's `Decibel` curve is kept, only its times change | The shape becomes ours, not the font's |

**Recommendation: scale the voice's envelope.** It is the path the release
already takes, it does both directions, and it costs nothing. Two guards:

- **A floor on every shortened stage**, about 5 ms for release and 1 ms for
  attack, so a note dragged to "shortest" fades instead of clicking.
- **One-shot samples cannot be lengthened past their end.** A drum hit with
  no loop stops when its recording does, however long the release; the
  panel should say so rather than offer a knob that does nothing.

A post-voice gain is still the answer in one place: **plugins**. A hosted
soundfont player is a black box, so for a plugin channel we could only ever
shorten, by gating its output per channel. Not proposed for v1.

### Per note: a new optional field, not a re-read byte

`Note` gets an optional set of trims, saved only when used, so every song
written so far reads and writes back unchanged (the pattern `Note::channel`
and `Note::path` already follow, `note.rs:53-68`):

```rust
/// How this one note's envelope differs from its instrument's.
/// Each time is a signed step on a doubling scale: 0 is the instrument's
/// own, -8 is half as long, +8 twice as long (eighth-octaves, so ±32 is
/// 16× either way). Sustain is in dB, ±.
pub struct NoteEnvelope { attack: i8, decay: i8, sustain_db: i8, release: i8 }
// on Note:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub envelope: Option<NoteEnvelope>,
```

- A doubling scale rather than a straight one because "half as long" and
  "twice as long" should be the same distance from the middle of a lane.
- The old `release` byte keeps meaning what it means and **multiplies with**
  the new release trim, so nothing old changes. The roll's existing Release
  lane becomes a centred lane that writes the new field; Ty may want the old
  lane kept as it is instead (**Ty**).
- The trims ride `EventPayload::NoteOn` and `NoteTrigger` (four more bytes on
  a `Copy` event), and the voice applies them where it applies
  `note_release_scale` today.
- The song's written form changes, so `wire::PROTOCOL` moves to 4
  (`fontelle-model/src/wire.rs:505`; `tests/song_form.rs` enforces it). Two
  studios on different versions already cannot share; nothing more breaks.

### Per instrument: relative trims on the channel

The panel's knobs already give absolute times. What they cannot say is
"this font, but 30 % shorter everywhere" — and once the importer keeps each
zone's own envelope (the §0 cut, worth closing in the same piece), there is
no single absolute time to turn. So:

- `Channel` gets the same four trims (`envelope_trim`, optional, saved only
  when set), applied to every zone's envelope, before the note's own.
- The panel shows them as a small **"Envelope, relative to the font"** row
  under the absolute knobs, each centred on "font's own", with a reset.
- Automatable through the existing parameter path, applied to new notes
  only (a held note keeps the envelope it started with; changing a held
  note's release mid-fade is allowed by the envelope but sounds like a bug).

### Ty's decisions for §1

1. Range: ±16× (recommended) or the 4× today's lane has.
2. Whether the roll keeps the old one-way Release lane or replaces it with
   the centred one.
3. Whether attack, decay and sustain are per-note lanes too, or release
   alone per note and all four per instrument (recommended: all four per
   instrument, release and attack per note — decay/sustain per note are
   rarely drawn by hand).

**Size:** the voice and model change, about two days with tests; the panel
row and lane, about two more. Closing the first-zone envelope cut, two to
three days on its own.

---

## 2. A SoundFont maker

### What already exists

More than it looks. A Flopsynth sampled oscillator is a `UserSample`
(`fontelle-core/src/patch.rs:331`) whose `SampleZone`s
(`patch.rs:406-436`) already carry everything an SF2 instrument zone needs
for v1: root key, fine tune, key range, velocity range, gain and loop
points. Recordings come in by drag-and-drop (the arrival row) and from the
browser; the factory grand was built from a soundfont by
`fontelle-app/examples/grand_samples.rs`. What is missing is **an editor
whose subject is the zones**, and **a writer** for `.sf2`.

### The shape

A **Maker** window, opened from a soundfont channel or from the start menu
("New soundfont…"). One preset per file in v1.

- **Samples**: drop audio files in; each is a row with its waveform, name,
  root key (detected by pitch where the recording is tonal, using the
  existing analysis, and always editable), and fine tune.
- **Map**: a keyboard-by-velocity grid. Each zone is a rectangle; drag its
  edges to set key and velocity ranges; "spread" fills the gaps between
  roots the way `key_ranges` already does for the factory grand.
- **Loop**: the zone's waveform with start/end handles, a zero-crossing
  snap, and an audition key that holds the note so the loop can be heard.
- **Envelope**: the amp envelope (one per preset in v1, §1's per-zone in v2).
- **Audition** through the normal voice while editing, on the selected
  channel.
- **Export** `.sf2` (16-bit, 44.1 or 48 kHz), and **Save as patch** to the
  user bank.

### v1 ships / v1 does not

| v1 ships | v1 does not |
|---|---|
| Import WAV/FLAC/OGG/MP3 (the decoders already in the tree) | Recording straight into the maker (record a clip, then drop it) |
| Root-key detection with manual override | Automatic multi-sample chopping from one long recording |
| Key and velocity zones, drawn on a grid | Layered presets (one preset of several instruments) |
| Loop start/end with zero-crossing snap and audition | Crossfaded loops, loop smoothing |
| One amp envelope per preset | Per-zone envelopes, filter and modulation envelopes, LFOs, modulators |
| Export `.sf2`, one preset per file | **`.sf3` export** (needs a Vorbis encoder — a new C library on three systems; **Ty**) |
| Re-open a Fontelle-made `.sf2` and edit it | Editing arbitrary third-party `.sf2` files in place (they open as a patch today; their full generator set is not round-tripped) |
| A licence field on export (§3's list) and the pack's credits file | Banks of many presets, SFZ export, DLS |

**Why `.sf2` first:** the writer is plain RIFF and roughly 600–900 lines
with tests (the `soundfont` crate we use reads only). `.sf3` is the same
file with Vorbis-compressed samples; reading it needs the Symphonia Vorbis
decoder already in `Cargo.lock`, but writing it needs an encoder we do not
have. SF3 *reading* is a small piece worth doing alongside (it unblocks the
bank's `.sf3` rows).

**Size:** the writer and round-trip tests, about a week; the maker window
(samples, map, loop, envelope, audition), about three weeks; root-key
detection, two to three days on the existing analysis.

### Packs

A pack is a folder of `.sf2` files plus a `CREDITS.md` and the licence. The
maker writes the licence into each file's `ICMT`/`ICOP` fields and into the
pack. **A pack made in Fontelle from original samples is entirely ours** and
is the Steam Supporter Pack content (card 0430's note); the exporter refuses
to put a sample in a pack when the sample's source is a downloaded font whose
licence §3's `SoundLicence::judge` refuses — the provenance is already on
every sample (`SampleRef`).

### Ty's decisions for §2

1. Whether v1 is the maker as above, or the writer alone first (export a
   Flopsynth sampled patch as `.sf2`, no new window) — a two-week shorter
   path to a first pack.
2. `.sf3` export in v1 or later.
3. What licence Fontelle's own packs are sold under (a commercial EULA for
   the Supporter Pack, or CC BY so they spread) — this changes what the
   exporter writes.

---

## 3. The licensing rule for bundled and re-hosted sounds — **built**

`fontelle_core::bundled_sounds`, with `licenses/SOUNDS.md` as its readable
side and `crates/fontelle-core/tests/bundled_sounds.rs` as the gate.

- **The permitted list is a Rust enum**, `SoundLicence`: MIT, CC0-1.0,
  CC-BY-3.0, CC-BY-4.0, an author's written public-domain dedication,
  Apache-2.0, GeneralUser GS's own licence (named as itself, its text as the
  font ships it — FluidR3 is MIT), and Fontelle's own (MIT OR Apache-2.0).
  There is no variant for anything else, so a manifest entry under a refused
  licence cannot be written.
- **Refused, with the reason** (`Refusal`): non-commercial (we sell),
  no-derivatives (every shipped sound is changed), share-alike and GPL
  (they can bind the pack or program to their licence — Ty's call per
  sound, not a default), "free for personal use" and unknown terms (no right
  to pass it on). `SoundLicence::judge` reads a licence as written and gives
  the answer; §2's exporter and §4's library ask it too.
- **The binary carries every text**: each entry `include_str!`s its licence
  file, so a missing file does not compile.
- **Every factory set must have an entry**: `FactorySampleSet::bundled` is
  an exhaustive match.
- **The tree is checked**: any `.sf2`/`.sf3`/`.sfz`/`.sfark`/`.dls` anywhere,
  and any recording under `assets/`, with no entry, fails the test, which CI
  runs on every push. Release archives copy `licenses/` (and the top-level
  `LICENSE-MIT`/`LICENSE-APACHE`); the test checks the workflow still does.

Today the tree bundles **no soundfont**. It embeds two sets of recordings:
the factory grand (Salamander Grand Piano, Alexander Holm, CC BY 3.0 — now
with the CC BY 3.0 text at `licenses/sounds/salamander-grand-piano/LICENSE`,
which no archive carried before) and the factory kits (ours).

Not covered by this rule, because they are not sounds: the UI fonts and
pictures (`crates/fontelle-ui/fonts/`, `assets/themes/`). The fonts have
their OFL texts beside them, but outside `licenses/`, so the release
archives do not carry those texts. They deserve the same treatment in a
card of their own.

---

## 4. Fonts we cannot ship: a library that downloads from the author

Most good free soundfonts are not ours to re-host (non-commercial terms, or
simply no stated permission). They can still be one click away.

### The shape

- **A catalogue**: a small JSON list, one entry per font — name, author,
  author's page, the **author's own download URL**, size, SHA-256 of the file
  as reviewed, the licence as the author states it (id, link, and the text
  as reviewed), and a note on what it is good for. Fontelle never hosts the
  file.
- **In the app**: a *Get more sounds* tab in the browser. Each row shows the
  font, its author, and a licence badge from `SoundLicence::judge`: **green**
  "use in anything, including packs you sell", **amber** "free for your
  music; not for packs" (NC and friends), **grey** "read the terms".
- **Before download**: a card with the **whole licence text** and the
  author's link, and *Download* / *Cancel*. Nothing downloads before that
  card has been seen; the acceptance is remembered per font in settings.
- **The download**: through `curl`, the way the updater does
  (`fontelle-app/src/updates.rs`, "why curl"), with the progress bar the
  update uses; the SHA-256 is checked and a mismatch is refused ("the
  author's file has changed since it was reviewed"). Zip archives are
  unpacked; `.sfArk` and installers are not supported and are not listed.
- **Where it goes**: the user's soundfont bank folder, with its licence
  written beside it (`<name>.LICENSE.txt`), so the bank's rows can show it.
- **Packs and provenance**: a sample from a downloaded font remembers which
  font it came from; §2's exporter refuses one whose licence `judge`
  refuses, and says which font and why.
- **A dead link** is shown as "no longer at the author's address", with the
  author's page — never a mirror.

### Ty's decisions for §4

1. Where the catalogue lives: compiled into each release (simple, updates
   only with releases) or fetched from the website/hub (live, but a new
   thing to host and keep honest) — recommended: compiled in for v1.
2. Which fonts are in it, and who reviews each licence before it is added.
3. Whether non-commercial fonts are listed at all (recommended: yes, with
   the amber badge — they are fine for a user's own songs).

**Size:** about a week (catalogue, tab, licence card, download and checks),
plus the reviewing of each font's terms.

---

## 5. Proposed follow-up cards (not created)

| Title | Acceptance |
|---|---|
| Per-note envelope trims | A note can shorten and lengthen attack and release relative to its instrument, click-free, with old songs sounding identical; `PROTOCOL` 4 |
| Per-instrument envelope trims | A soundfont channel has four relative trims centred on the font's own, applied to every zone, automatable |
| Keep each zone's own envelope on import | A multi-zone `.sf2` whose zones differ in envelope plays each zone with its own |
| Read `.sf3` | `.sf3` files list in the bank and open, via the Vorbis decoder already in the tree |
| `.sf2` writer | A Fontelle sampled patch exports as a `.sf2` that FluidSynth and Polyphone open and that re-imports identically |
| The Maker window (v1) | Import samples, map key/velocity zones, set loops and one envelope, audition, export `.sf2`, per §2's table |
| Pack export and credits | A pack folder with `.sf2`s, `CREDITS.md` and licence; refuses samples from fonts `judge` refuses |
| In-app soundfont library | Catalogue, licence card before download, author's URL only, SHA-256 checked, licence saved beside the font |
| Licences for UI fonts and pictures | The same manifest-and-test rule for `crates/fontelle-ui/fonts/` and `assets/themes/` |
