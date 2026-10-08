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
Install (`voicebanks::install`) takes a `.zip`, an `.oudep`, a `.7z`
(`sevenz-rust2`, pure Rust), a `.uar` (UTAU's installer archive, a zip with
an `install.txt` whose `folder=`/`contentsdir=` are honoured in any package)
or a folder — told apart by their first bytes, a RAR refused with what to do; zip
names are read in Shift-JIS, GBK or UTF-8 (chosen once per archive by which
reading looks like a language), the bank's root is found under any wrapping
folders (`dsconfig.yaml` → DiffSinger; `character.txt`/`character.yaml` over
an `oto.ini`, or the folder all the `oto.ini`s share → UTAU; `vocoder.yaml`
or `oudep.yaml` → a dependency), names that would climb out are refused, and
nothing half-installed is left.

### 2.2 Banks in the studio, lyrics in every script, takes kept (2026-10-07)

Branch `feature/vocal-banks`.

- **Portraits** (`fontelle_ui::portrait`): `character.txt`'s `image=` (BMP
  most often — 1/4/8/16/24/32-bit, bitfields, either row order, read here),
  `character.yaml`'s `portrait:`/`image:` (PNG, JPEG, or WebP whatever the
  extension says, `image-webp`). Box-filtered to 256 px at most and kept by
  path; drawn cropped to a rounded square on the picker's cards, the
  Voicebanks page's rows and the roll's singer chip.
- **The picker is a card grid** (`canvas::singer_picker`): portrait, name,
  kind badge (UTAU / DiffSinger / Engine), a chip per language (which
  chooses it), terms to read or agreed, research-only; the voicebanks'
  group above the engines'. A search field narrows by name, kind, language
  tag or name, engine; arrows move between cards as laid out, Tab walks the
  focused card's languages, Enter chooses, Esc closes. No singer, Install a
  voicebank…, Voicebanks & Find singers… below. Unread terms are still asked
  before a bank first sings.
- **The Voicebanks page opens from anywhere**: the instrument menus (New
  instrument, Change instrument) have a row for it, and the key
  `Ctrl+Shift+Y` (`Action::Voicebanks`, rebindable); it no longer needs a
  vocal track in the roll. An installed row has its languages, ▶ Sample (the
  sample its author named, through the Import tab's preview voice), Folder,
  its terms and Uninstall.
- **Lyrics in Chinese characters and hangul** (`fontelle-lang`): a typed line
  spreads a character or a syllable block a note (a bracketed reading stays
  with its character). Chinese reads toneless pinyin — each character's most
  common reading (Unihan `kMandarin` via `pinyin-data`, MIT/Unicode-3.0,
  `licenses/pinyin-data/`), common words that read a character otherwise
  matched across notes (`银` `行` → `yin hang`), a few readings songs prefer
  (`长` → `chang`), and `行[hang]` to say any other. Hangul is taken apart by
  Unicode's arithmetic into onset/vowel/coda, romanised, with a coda carried
  into a following vowel and stops nasalised before ㄴ/ㅁ. DiffSinger banks
  look each syllable up in their dictionary (`ni`, `han`), else sing its
  parts as phonemes (`zh/n zh/i`, `ko/h ko/a ko/n`); UTAU banks get two
  phonemizers appended to the setting: **Chinese CV/CVVC** (pinyin aliases,
  `- ni`, `i h`, `ao R`) and **Korean CV/CVVC** (hangul aliases: whole
  blocks when recorded, else the open syllable and a coda unit `a k`,
  vowel-to-consonant `a r`, `ae R`), each detected from a bank's aliases.
- **Takes kept on disk** (`fontelle_vocal::TakeStore`): final takes are
  written by the render thread that sang them to the song's own
  `cache/vocal/` (an unsaved song: Fontelle's cache folder's `vocal/`, by the
  analysis cache's rule), one file per render key — 16-bit samples
  (scaled when louder than full scale), delta-coded, high and low byte
  planes, deflated; the pitch line and phonemes beside them; a key and an
  xxHash checked on read. 512 MB a folder, least recently used out first.
  A phrase whose final is on disk is not previewed: a reopened song plays
  at once. A song saved for the first time takes its sung finals with it.
- **Packages**: `.7z` (`sevenz-rust2`, Apache-2.0) and `.uar` (a zip with
  UTAU's `install.txt`, whose `folder=`/`contentsdir=` are honoured in any
  package), told apart by their bytes; names checked before anything is
  written, as for zips.

### 2.3 Each singer an instrument of its own (2026-10-07)

> *"i thought we agreed to split generic vocals and [the paid engine] into
> separate addable instruments, that can be changed between and it will
> still sing correctly, but actually using them is separate instruments"*

- **One part, several instruments.** A vocal track is still
  `InstrumentKind::Vocal` with a `VocalVoice`; **which instrument it is, is
  who its voice's backend is**: `VocalVoice::engine_instrument()` is `None`
  for the Fontelle Singer (keys starting `fontelle-`,
  `fontelle_model::is_builtin_engine`; a track with no voice is one too)
  and the engine's name otherwise. Nothing new is stored, so old songs open
  as whichever backend their voice names, and changing instrument keeps the
  notes, lyrics, expression and lanes.
- **The instruments** (`SingerRegistry::instruments`, the host's
  `StudioHost::singer_instruments`): the Fontelle Singer, always (its banks
  can be installed later), then each engine loaded, named by
  `fontelle_vocal::instrument_name` — the engine's display name before any
  bracketed note (`"Some Engine (research only)"` → `Some Engine`). No
  engine is named in this tree.
- **The menus**: New instrument and Change instrument list *Fontelle
  Singer* (`InstrumentKind::Vocal`'s label now) and, right after it, a row
  per engine (`InstrumentRow::Engine`, `canvas::singer_instrument_menu_rows`);
  on a vocal track its own instrument is marked and greyed.
- **Adding** (`StudioHost::add_singer_channel`): a channel named for the
  instrument ("Fontelle Singer 4", "Some Engine 5") with one of **its own**
  singers (`SingerRegistry::default_voice_for`: the part's language first,
  an ordinary singer before a research-only one; the Fontelle Singer never
  borrows an engine's singer). An engine with nobody chosen is
  `VocalVoice { engine, singer: "" }` — still its instrument, silent with
  "Choose a singer". `add_channel_of(Vocal)` (song-file import, older
  callers) adds whichever instrument sings the first singer there is.
- **Changing** (`StudioHost::set_singer_instrument`): one `Compound` (one
  undo): the kind and starter patch when the channel was not vocal, the new
  instrument's singer in the part's language, and the channel renamed when
  it still carries the old instrument's default name. Already that
  instrument is no step.
- **Each picker its own singers** (`canvas::picker_filter_for`): the
  Fontelle Singer's shows voicebanks (with Install and Voicebanks & Find
  singers), an engine's only its singers; both have *Change instrument…*.
  "No singer" on an engine's track keeps the engine.
- **Told apart**: the arrangement's caption is "Fontelle Singer · 重音テト ·
  la la" / "Some Engine · Tenor · la la"; the roll's engine chip says
  "Open <instrument>" at the singer chip's size.
- **Bigger picker**: cards at least 300 × 192 logical px with 128 px
  portraits (twice what they were), names 17 px semibold, text 13 px (the
  app's own size), title 20 px; columns from the window's logical width, so
  a larger UI scale gets fewer, still-big cards; the footer wraps. The
  Voicebanks page's rows match (128 px portraits, the same styles).
- **One face for a bank**: `voicebanks::list` describes a bank from the
  folder that holds its files (`Installed::root` — Canary's and Teto's
  packages put them a folder down, which is why the Installed page showed
  initials where the picker showed pictures), and `voicebanks::bank_for_voice`
  finds a built-in runtime's voice among the installed banks (by its
  picture's path, its id as the folder, or the runtime's folder name —
  `utau/<folder>`, `utau/<folder> (<parent>)`). The picker's card takes the
  bank's name, picture and folder from there, so card and row agree: the
  name `character.txt`/`character.yaml` gives, the folder under it.

### 2.4 English that sounds like English, and a library to try (2026-10-07)

Branch `feature/vocal-singers`. Ty: "the english wording sounds off when i
try to force it to speak english" (Kasane Teto's English bank).

- **What was wrong** (fontelle-utau, X-SAMPA CVVC): the final cluster was
  dropped ("world" ended on `3 l`, "hand" on `{ n`, because only `ld-` and
  `d-` were tried and the bank names them `l d-`). None of the bank's
  cluster samples were used (`- str`, `st`, `tw`, `bju`, `r g`, `s kt-`).
  Two consonants between vowels lost the first one's release
  (`I t` → `s@`). Every unstressed "uh" was the stressed `V`, because
  CMUdict's stress had been stripped. Joule EN CVVC, a Salem-style X-SAMPA
  bank, was taken for ARPAsing (its `b d` and `- b` read as ARPAbet), and
  half its notes went unsung.
- **Now:**
  - The dictionary keeps an unstressed AH as `ax` (`data/make_cmudict.py`).
    X-SAMPA sings it `@`. ARPAsing banks without `ax` fall back to `ah` on
    both sides of a diphone, and VCCV sings it `u`.
  - Per vowel, the X-SAMPA planner picks the CV with the longest onset the
    bank joins to it, or VCV where the bank has it (`@ hA`). Between that
    and the VC it uses the bank's joined onsets, then `C C` pairs, then a
    released consonant on its own (an obstruent only). It ends over the
    whole final cluster (`l d-`, `s kt-`, else `l d` + `d -`). It gives a
    start to CVs that have none (`- b` + `bA`).
  - A coda consonant is joined only as the single consonant before the
    next vowel ("world I": `daI`, as OpenUtau does).
  - A consonant ending one word and starting the next is sung once.
  - Detection counts an alias as ARPAsing only when it has an ARPAbet
    vowel.
- **Also found by singing the library** (fontelle-diffsinger unless said):
  - English words a DiffSinger bank's dictionary lacks are read from
    CMUdict, when the bank has every sound. SPORE's dictionary has no
    words; Nishiren's has no "world".
  - A dictionary prefixed `en/w` serves a model whose phonemes are `w`
    (SPORE).
  - Vocoders get a per-sample noise pool (Canary DS's RefineGAN draws
    samples × 9), and duration and linguistic models get one per phoneme
    (Yoko).
  - (fontelle-utau) A folder with `dsconfig.yaml` is never a UTAU bank:
    Ritsu's DiffSinger bank ships an oto.ini.
  - (fontelle-utau) A Chinese or Korean syllable with no initial joins
    from the vowel before (`ao a`), else uses its start (`- ai`).
- **The library**, on this machine only:
  `/mnt/disks/6tb/FontelleTestData/voicebanks/LIBRARY.md` covers 21
  singers, with terms and quality notes. The verified ones with stable
  pages are in `docs/singers-index.json` (Get it, no rehosting).
- **Not done:**
  - X-SAMPA r-coloured vowels (`Ar`, `Er`, Salem's list) are not formed
    from `aa r`.
  - Teto's `a`/`e`/`o` vowel variants and dark-L (`5`, `l0`) are unused.
  - English VCCV keeps its old planner.
  - Yamine Renri's English needs its own dictionary format and stays "la".
  - "Teto" itself is not in CMUdict.

## 3. What the Fontelle Singer must do (parity with OpenUtau and Vocaloid)

Producers coming from OpenUtau, UTAU, Vocaloid or Synthesizer V should find
nothing missing in daily work.

**Voicebanks**
- Install by dropping a `.zip`, `.7z`*, `.oudep` or folder anywhere on the
  window, or from **Find singers** (§4); `.uar` too. File names in Shift-JIS, GBK or
  UTF-8 zips are decoded; `oto.ini`/`character.txt` in Shift-JIS or UTF-8
  are read; `character.yaml`, `readme.txt`, portraits and sample audio are
  shown.
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
  Japanese CV/VCV/CVVC, English ARPAsing/VCCV/X-SAMPA, Chinese and Korean
  CV/CVVC, DiffSinger dictionary; Chinese characters and hangul read (§2.2).
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

### 3.1 Song files, freezing and rendering (as built, 2026-10-07)

**Reading and writing** (`fontelle-vocal-files`): every reader makes a
`VocalSong` — tempo, bar, and one `SongPart` per sung line (notes with
lyric, `Note::voice`, velocity as consonant speed; `VocalPart` lanes), all
in project ticks from the song's 0. What a file carries that has no place
is named in `VocalSong::unmapped` and said in the toast.

| file | maps | does not map (named) |
|---|---|---|
| `.ust` (UTAU, Shift-JIS/UTF-8) | notes, lyrics (VCV context dropped), tempo (song and per note), Velocity → consonant speed, Intensity → dynamics, Mode2 PBS/PBW/PBY/PBM and Mode1 PitchBend → pitch-bend lane over the notes (the curved note's own glide set to none), VBR → vibrato, flags g → gender, B → breathiness | other resampler flags, Modulation, envelopes, per-note oto overrides |
| `.ustx` (OpenUtau) | tracks' voice parts → a part per track, tempos, first time signature, lyric (`+` → `-`), tone, vibrato, `vel`, pitch points (a flat snapped pair → portamento, OpenUtau's default → the backend's own, anything else → pitch-bend lane), phoneme overrides naming every phoneme → hand phonemes, curves pitd/dyn/bre(c)/voic/ten(c)/gen(c) → lanes, any other curve kept as `extra["openutau"]` and written back | override timing deltas, `clr` and other expressions, later time signatures |
| `.vsqx` (Vocaloid 3/4) | pre-measure removed, notes, lyrics, velocity, **locked** phonemes, vibLen/vibType/vibDep/vibRate → vibrato, tempo, first time signature, PIT×PBS → pitch bend, DYN → dynamics, BRE → breathiness, BRI → tension, GEN → gender (held values) | CLE, OPE, POR; unlocked phonemes (Vocaloid's own guess) |
| `.mid` with lyric events (or karaoke text) | the tracks with words → parts, lyrics on the note at their tick, tempo, first time signature, pitch bend at the RPN 0 range → pitch-bend lane | velocity (loudness, not consonant speed), tracks without words (left to the MIDI import) |
| `.musicxml`/`.xml`/`.mxl` | every part with words (else the first), its first sung voice, pitch, rests, ties joined, syllabic begin/middle → `"Hap-"`, a wordless note under a slur or after `<extend/>` → `-`, `<sound tempo>` or metronome, first time signature (6/8 → 3 quarters a bar) | other voices, chord tones after the first, grace notes, time-wise scores |

Written: **MIDI with lyrics** (a conductor track; per part a lyric event
before each note, the pitch-bend lane as bends at a 12-semitone range, 960
a quarter) and **USTX** (a track and voice part per part; glides as
OpenUtau pitch points, vibrato, `vel`, phoneme overrides, every standard
lane as its curve, kept OpenUtau curves). A vocal track reads out as a song
by `song_of_channel`: every clip on it (prefab instances, every pass of a
loop), notes and lanes on the song's ticks.

**Bringing one in** (`ImportVocal`, one command and undo step, wire form
appended): dropped anywhere, each part is a **vocal track of its own**
(named after it, the first free voicebank singing, rows where the window
is looking); dropped on a vocal track's row, or asked for from that track's
menu, the first part goes **into that track**. The clip starts at the bar
of its first note, and the file is placed **by beats** from the bar it was
let go in. Into an **empty song** the file's tempo map and bar come too (in
the same command); otherwise the song's are kept, and the toast says which.
A `.mid` is a vocal song only when it has words; `.xml` only when it is
MusicXML. Entry points: a drop on the window or a row; the rack menu of a
vocal track (*Import vocal file…*); the new instrument menu (*Vocal track
from a song file…*).

**Freezing** (`FreezeVocal` / `UnfreezeVocal`, one step each way):
the track's final takes, waited for with the export's progress card, played
through a `VocalNode` of its own with the channel's level and pan — what the
track hands its mixer strip — from half a second before its first phrase to
a second after its last, written to `renders/<track> (frozen).wav`, and put
on a row of its own under the track's last row as an audio clip **routed to
the track's own strip** (inserts, fader and sends go on applying, so the
song sounds the same). `Project::frozen` records it (format 3, left out of
the file when empty); a frozen track's part is kept whole and not sung
(`phrases_with`). Unfreezing takes the clip (and its row, when nothing else
is on it) away; the file stays in `renders/`.

**Render selection to clip**: the roll's selected notes (all the open
clip's when none is), the phrases they are in, rendered the same way from
0.15 s before the first to half a second after the last, onto a new row
under the clip's (`<clip> (rendered)`, `AddAudioClip`, one undo), routed to
the track's strip. A row or clip *Render to audio* now sings only the vocal
tracks on that row or clip (`VocalRenderPrep::retain_channels`).

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
3. Imports (UST/USTX/VSQX/MIDI/MusicXML), exports (MIDI, USTX), freeze and
   render to clip — **built** (§3.1). Phoneme row retiming.
