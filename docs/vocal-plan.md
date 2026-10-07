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

### 2.1 How the layers are built (2026-10-07)

**The part** (`fontelle-model`, project format **3**, written only by songs
that use it; the step from 2 is empty):

- `Note::voice: Option<Box<NoteVoice>>` — phonemes placed by hand (offsets
  in **seconds** from the note's start: a consonant does not lengthen with
  the tempo), vibrato, the glide in, and per-note options **by backend**
  (`options[backend][id]`). A vocal note's **velocity is consonant speed**
  (OpenUtau's VEL on the note's own velocity: 100 is the voice's own, ±27
  doubles/halves; `consonant_speed`, `velocity_for_speed`), so the roll's
  velocity lane edits it.
- `NoteData::vocal: Option<Box<VocalPart>>` — on the clip's (or prefab's)
  notes, so moving, copying, looping or making a prefab carries the curves:
  the standard lanes, each backend's own lanes (`extra[backend][id]`),
  settings (`settings[backend][id]`) and state blob (`state[backend]`, base64
  in the file). **Lanes are points in ticks from the clip's start**
  (`LaneCurve`: straight lines between points, the ends held; a stroke puts
  the old values back beside itself and simplifies straight runs), so a
  tempo change keeps a swell under its note. Nothing is pruned when the
  singer changes.
- Commands, each one undo step per gesture (merged while the button is
  held), wire forms appended: `SetNoteVoices`, `SetVocalLane`,
  `SetVocalSettings`, `SetVocalState`. A clip cut in two shifts the second
  half's lanes.

**The phrase** (`fontelle-vocal::phrases_with`): each note's expression (only
the singing backend's options), the part's lanes sampled every 5 ms from
0.25 s before the first note to 0.25 s after the last from whichever clip
pass is playing (a lane neutral all the way through is left out), the
backend's own lanes, settings and state from the first note's clip, and the
phrase's identity (`start`, `note_ids`) — **which the render key leaves out**:
the key hashes the whole phrase in its own time, numbers rounded to what
can be heard, so a moved phrase is the same take.

**Who sings** (`fontelle-vocal::SingerRegistry`): the built-in runtimes, then
every external engine. A song's `VocalVoice::engine` is the backend's key:
`fontelle-utau`, `fontelle-diffsinger` (keys starting `fontelle-` are
reserved), or an engine's name. **Wiring a runtime** is one entry in
`fontelle_vocal::builtin_backends()`:

```rust
BuiltinBackend {
    key: "fontelle-utau",
    kind: SingerKind::Utau,
    make: |voicebanks: &Path| Arc::new(fontelle_utau::UtauBackend::new(voicebanks)),
},
```

`make` is called again after an install or uninstall, so a backend may read
the folder once when made. A new vocal track gets the first free voicebank,
else the first engine singer that is not research-only.

**The voicebanks folder**: `<data>/fontelle/voicebanks/` by the app's folder
rule (`$XDG_DATA_HOME` or `~/.local/share`, macOS included; Windows
`%LOCALAPPDATA%`). One folder per bank exactly as its author laid it out
(OpenUtau's `Singers` layout); dependencies (vocoders) in `Dependencies/<name>/`
(OpenUtau's `Dependencies`). Fontelle's own files there start with a dot:
`.installing-*` (an install in progress), `.downloads/`, `.agreed-terms.json`
(singers whose terms were agreed to, by backend and voice, with a hash of the
terms), and a bank's `.fontelle-terms` (its readme's terms, acknowledged).
Install (`voicebanks::install`) takes a `.zip`, an `.oudep` or a folder; zip
names are read in Shift-JIS, GBK or UTF-8 (chosen once per archive by which
reading looks like a language), the bank's root is found under any wrapping
folders (`dsconfig.yaml` → DiffSinger; `character.txt`/`character.yaml` over
an `oto.ini`, or the folder all the `oto.ini`s share → UTAU; `vocoder.yaml`
or `oudep.yaml` → a dependency), names that would climb out are refused, and
nothing half-installed is left. `.7z` is not read yet.

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
  *(Built 2026-10-07 on `feature/vocal-roll`: Shift+P pitch pen, Alt+S /
  Alt+Q / Alt+R, Alt+Enter the note menu — `canvas::vocal_pitch`.)*
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

### 4.1 The index format (format 1)

A JSON file; unknown keys are ignored everywhere, so the format grows by
adding keys. A seed is `docs/singers-index.json`, shipped inside the app as
the fallback; `<data>/fontelle/singers-index.json` replaces it when present
(until the catalogue repository serves one).

```jsonc
{
  "format": 1,
  "updated": "2026-10-07",
  "singers": [
    {
      "id": "kasane-teto.utau",            // stable, unique in the index
      "name": "Kasane Teto (UTAU)",
      "kind": "utau",                      // "utau" | "diffsinger" | "dependency"
      "languages": ["ja", "en"],           // BCP 47
      "author": "TWINDRILL",               // optional
      "portrait": "https://…/teto.png",    // optional, an image URL
      "sample": "https://…/teto.ogg",      // optional, audio to listen to
      "terms": {
        "summary": "One or two sentences: what the author allows.",
        "url": "https://…/terms",          // the author's full terms
        "commercial": "yes" | "no" | "ask" | "unknown",
        "ai_training": "yes" | "no" | "ask" | "unknown",
        "credit": "yes" | "no" | "ask" | "unknown"   // credit required?
      },
      "download": {                        // optional: a direct, rehostable-by-author link
        "url": "https://…/bank.zip",       // https only
        "sha256": "64 hex digits",
        "size": 123456789                  // bytes
      },
      "page": "https://…"                  // the author's page for it
    }
  ]
}
```

An entry with a valid `download` is **Install**: fetched with the app's
`curl` (as the updater does), checked against `size` and `sha256`, installed.
Otherwise it is **Get it**: `page` opens in the browser and the Downloads
folder (the desktop's `XDG_DOWNLOAD_DIR`, else `~/Downloads`; Windows
`%USERPROFILE%\Downloads`) is watched for a new `.zip`/`.oudep`/`.7z`/`.uar`
that has stopped growing — and, when the entry has a `sha256`, only that
file — which is then installed. An entry with neither, or without terms, is
left out and named (`Catalogue::skipped`). Only entries whose terms were
read at their source go in; nothing is rehosted.

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

### 5.1 What the host does with ABI 2 (as built)

- **Loading**: an engine built for 1 or 2 loads; for 2, each singer's
  `capabilities` and `has_editor` are asked once on the loading thread and
  kept. A version-1 engine honours nothing beyond notes and words (every
  lane greyed); hand-placed phonemes reach it as a bracketed lyric.
- **Rendering**: through `fontelle_sing::Session::sing_flagged`, the
  scheduler's own cancel flag handed straight to `render_phrase`. Each
  worker thread keeps one instance per engine. The seed is derived from the
  phrase's content less the quality (documented on `PhraseRequest`).
  The phonemes report may carry `"offsets"` (seconds from each note's
  start), which the roll's phoneme row places.
- **The window** (`fontelle_host::SingerEditor`): an instance of its own,
  opened into a `PluginWindow` (the X11 window plugin editors embed into;
  parent 0 when headless — engines must accept it), ticked once a frame,
  resized with the window, closed with its close button. `get_part` answers
  the part (`PartView`: the open clip's notes on the track, its lanes in
  song seconds every 5 ms, the backend's own lanes, settings and state, the
  roll's selection, the playhead) as last shown; `editor_part_changed` is
  called whenever that changes (an edit, a take landing, the selection, the
  playhead). `apply_edits` checks the batch whole (every note named in the
  part, every number a number) and queues it; after the tick the app makes
  each batch **one command, one undo step** (`Compound`): lyrics, phonemes,
  velocity (as consonant speed), vibrato, glides and options merged per
  note, curves converted to clip-tick lanes, `Settings` **merged** (keys
  given set, others kept), `State` replaced; `Select` selects in the roll.
  `transport` plays, stops and seeks the song. `take_at` answers the best
  take of the phrase under a song second (samples, pitch, and its phonemes
  report with offsets), valid until the engine next returns.
- **The track**: an engine with a window shows **Open <engine>** beside the
  singer chip in the roll; its declared lanes appear as tabs in the lane
  area, and its per-note options in the note menu.

## 6. Boundary

Everything in §§2–5 is open and generic. No closed engine's models,
weights, training code or glue enters this tree; the DiffSinger runtime here
is written from the published format, not from any closed engine's code.

## 7. Order of work

1. `fontelle-sing` (the contract) and ABI 2 — done with this plan.
2. In parallel: `fontelle-diffsinger`, `fontelle-utau`, and the integration
   (part model for expression and lanes, the roll's lanes and handles, the
   Fontelle Singer instrument, voicebank install and the picker, ABI 2 host
   side, Find singers). **The integration is built** (§2.1, §4.1, §5.1);
   the two runtimes are wired by one `builtin_backends()` entry each.
3. Imports (UST/USTX/VSQX/MIDI/MusicXML), freeze, phoneme row retiming.
