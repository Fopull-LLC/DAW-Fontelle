# Vocal tracks and the Fontelle Singer

Status: decided 2026-10-07 (Ty); being built on `feature/vocal-track`.

## 1. What Ty asked for

> *"make the free generic singers path as part of the public vocal track
> release and make it easy for people to find and install and use all the
> free open singers out there that would be usable for it ... i want the
> generic singer to genuinely stack up to existing workflows for utau and
> vocaloid producers so it doesn't feel lacking ... the advanced plugin is
> where we go beyond expectations."*

Decisions:

1. **The Fontelle Singer** is a built-in, free instrument for vocal tracks.
   It sings **classic UTAU voicebanks** (samples + `oto.ini`) and
   **DiffSinger voicebanks in the OpenUtau layout** (ONNX), so the free
   singers people already use work in Fontelle. Its runtimes are written
   for this tree; they share no code with any closed engine.
2. **External singer engines** (`fontelle-singer-abi`) are instruments of
   their own: from ABI 2 an engine may draw **its own window, with its own
   design**, declare its own lanes, settings and per-note options, and keep
   state in the part. Paid engines live there, outside this tree.
3. **Notes and lyrics belong to the vocal track, not to the singer.** A part
   moves between the Fontelle Singer and any engine without losing work;
   lanes and options a singer does not use are kept, not deleted.

## 2. The layers

```
 Vocal part (fontelle-model)        notes · lyrics · phoneme edits · velocity ·
   one per vocal clip               vibrato · portamento · standard lanes ·
                                    each singer's own lanes, settings, state
        │  phrases_of + render keys (fontelle-vocal)
        ▼
 fontelle-sing::Phrase              the one contract every singer is driven by
        │
        ├── Fontelle Singer (built in, free)
        │     ├── fontelle-utau        classic UTAU banks: oto.ini, CV/VCV/CVVC,
        │     │                        appends, a WORLD-class resampler
        │     └── fontelle-diffsinger  OpenUtau DiffSinger banks: dsconfig.yaml,
        │                              ONNX variance/acoustic/vocoder, dsdict
        └── External engines (fontelle-singer-abi, dlopen'd, own licence)
              v1: notes + lyrics → PCM
              v2: the whole Phrase, capabilities, its own window (HostCallbacks)
        │
        ▼
 render scheduler + cache → VocalNode (plays takes in time, RT-safe)
```

`fontelle_sing::Backend` / `Session` is the in-process face of a singer; the
host adapts external engines to it, so the scheduler, cache and node never
know which kind sang.

## 3. What the Fontelle Singer must do (parity with OpenUtau and Vocaloid)

Producers coming from OpenUtau, UTAU, Vocaloid or Synthesizer V should find
nothing missing in daily work.

**Voicebanks**
- Install by dropping a `.zip`, `.7z`*, `.oudep` or folder anywhere on the
  window, or from **Find singers** (§4). File names in Shift-JIS, GBK or
  UTF-8 zips are decoded; `oto.ini`/`character.txt` in Shift-JIS or UTF-8
  are read; `character.yaml`, `readme.txt`, portraits and sample audio are
  shown. *(`.7z` if a pure-Rust decoder fits.)*
- The **terms** each author wrote are shown at install and on the picker;
  the user acknowledges them. Fontelle records what is known: commercial use,
  AI use, credit required.
- Classic UTAU: CV, VCV, CVVC, VCCV and ARPAsing banks; multi-pitch
  (`prefix.map`) and appends/voice colours (suffixes) as a per-note option.
- DiffSinger (OpenUtau layout): acoustic + variance/pitch/duration models,
  speakers (and speaker mixing over time as a lane), languages, `dsdict`
  dictionaries, the bank's own vocoder dependency.

**Writing**
- Lyrics as now (Tab through notes, a line spread over notes, `-` slurs,
  hyphenated syllables, `[phonemes]`), with the phonemizer chosen per bank:
  Japanese CV/VCV/CVVC, English ARPAsing/VCCV/X-SAMPA, DiffSinger dictionary.
- **Phoneme row**: every note's phonemes, editable; drag a phoneme's start to
  retime it (banks that honour timing).
- **Per-note expression**: velocity (consonant speed), vibrato (depth, rate,
  delay, fade in/out — drawn on the note, with handles), portamento in
  (length, position, shape — handles at the note's head), voice colour.
- **Lanes** under the roll: pitch bend (draw over the computed pitch line),
  dynamics, breathiness, voicing, tension, gender; plus each DiffSinger
  bank's or engine's own. A lane the voice ignores is greyed with why.
- **Pitch drawing**: pencil the pitch line directly; reset to the notes;
  smooth; snap the drawn line to the scale.
- Import **UST**, **USTX**, **VSQX**, **MIDI with lyrics** and **MusicXML**;
  export MIDI with lyrics and USTX.

**Hearing**
- Preview renders first, final in the background, cached by content; a
  part is never silent while something better renders.
- Freeze a vocal track to audio; render a selection to a clip.

## 4. Find singers

A **catalogue of free singers** inside Fontelle, like the extension
catalogue: name, portrait, languages, kind (UTAU / DiffSinger), a sample to
listen to, the author's terms summarised with a link to the full text, and
the download.

- The index is a file in the package catalogue repository (git), edited by
  pull request; each entry records what the author allows. Nothing is
  rehosted without permission.
- Where the author gives a direct download link, **Install** fetches,
  verifies (size, sha256 recorded in the index) and installs. Where the
  author's page must be visited (most UTAU banks), **Get it** opens the page
  and Fontelle installs the file when it lands in the Downloads folder.
- Installed singers show in the vocal track's picker immediately.

## 5. External engines (`fontelle-singer-abi`)

ABI 1: notes and lyrics in, PCM out. ABI 2 (additive; the host loads both):

- `capabilities(singer)` → `fontelle_sing::Capabilities` (JSON): which
  standard lanes and expression it honours, its own lanes, settings and
  per-note options.
- `render_phrase(Phrase JSON, quality, seed, cancel)`.
- Its own editor window, embedded like a plugin bridge's: `editor_open` into
  a host window, `editor_tick` each frame, `editor_part_changed`. The window
  reads the part (`PartView`) and edits it through `HostCallbacks.apply_edits`
  (`PartEdit` batches, each one undo step), moves the transport, and reads
  takes to draw them.
- The vocal track shows an engine's button **Open <engine>** beside the
  singer; its declared lanes appear in the roll under its name.

An engine's window is the place for everything this tree does not do; the
part stays the shared record of the song.

## 6. Boundary

Everything in §§2–5 is open and generic. No closed engine's models,
weights, training code or glue enters this tree; the DiffSinger runtime here
is written from the published format, not from any closed engine's code.

## 7. Order of work

1. `fontelle-sing` (the contract) and ABI 2 — done with this plan.
2. In parallel: `fontelle-diffsinger`, `fontelle-utau`, and the integration
   (part model for expression and lanes, the roll's lanes and handles, the
   Fontelle Singer instrument, voicebank install and the picker, ABI 2 host
   side, Find singers).
3. Imports (UST/USTX/VSQX/MIDI/MusicXML), freeze, phoneme row retiming.
