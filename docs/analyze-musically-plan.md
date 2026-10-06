# Analyze Musically: design plan

*Written 2026-10-06 from a read-only design pass over the tree at v0.25.2. **Status (2026-10-06):** P0 done — tract runs `nmp.onnx` whole, matching Python basic-pitch; the analysis half of P1 (pYIN, segmentation, key, chords, extraction confidence, cache) is built in `fontelle-analysis`; the P1 UI (the window, the job, copy notes and scale, notes under the audio) is built on `feature/analyze-musically`; P2 (studies in the song, offline PSOLA, the preview player and transport, the Move tool and edit keys, Render to clip and Revert) is built there too. `PROGRESS.md` has the numbers.*

**What Ty asked for, condensed.** An Edison-like tool, but much easier to use. It opens from an audio clip (the clip's name menu → *Analyze Musically*) or as its own instance. It analyses the audio straight away and draws the notes (chords and melodies) over the waveform. You drag notes to repitch them and the result has to sound natural. You can render the edits back to the clip. You can copy the notes into any piano roll as MIDI. It estimates the key/scale with a confidence %, and gives a separate confidence for how well notes could be pulled out at all. Clicking the scale copies it, for pasting into Tune or the roll's scale, and you can view the notes in the scale. The deeper tools are tucked away: noise capture and removal, trim, markers, slicing into a sampler, and recording takes. The look follows Flopsynth. It should be friendly for beginners, have depth for pros, be readable at a glance and be fast from the keyboard.

---

## 0. Ground rules (same as every plan in `docs/`)

- **Tests first, confirmed failing, then the implementation.** Never both in one edit (`CLAUDE.md`, `PROGRESS.md` top).
- **INVARIANTs are hard.** The ones that matter most here:
  - **1:** nothing on the RT thread allocates. Analysis and rendering always run off the RT thread. The preview player is preallocated.
  - **2 / 9:** the window emits edits and `fontelle-app` turns them into `Command`s.
  - **4:** the new crate depends only downward.
  - **5:** samples for audio time, ticks for musical time. Conversion goes through `TempoMap` only, when notes are copied.
  - **7 / 8:** new ids are stable and new action ids are permanent.
  - **10:** caches go in `<bundle>/cache/` or XDG cache. Renders go in `<bundle>/renders/`, takes in `<bundle>/recordings/`.
- **Licences:** only MIT, Apache-2.0, BSD or ISC, enforced by `deny.toml`. **Model weights are held to the same rule.** `cargo-deny` cannot see an `.onnx` file, so each shipped weight file gets a `licenses/MODELS.md` entry and a test that the file's hash matches the reviewed one.
- **Look at it before believing it:** check the headless dump (`FONTELLE_UI_DUMP=… cargo test -p fontelle-ui --test render_headless`) and the real binary on Xwayland `:99`.

---

## 1. What exists to build on

| Need | What is in tree | Path(s) | Reuse verdict |
|---|---|---|---|
| Audio clip data | `AudioClipData` is non-destructive: `asset: AssetRef`, `source_start/end` (frames), `sample_rate`, `gain_db`, `pitch_semitones` (grain-shifted), `speed`, `reverse`, fades, `filter`, `normalize`, `loop_mode`, `stretch`, `file_frames`, `loop_phase` | `crates/fontelle-types/src/audio_clip.rs` | Render-back swaps `asset` and keeps every other field. Edits never change duration, so `source_start/end` stay valid. |
| Clip commands | `AddAudioClip`, `SetAudioClip` (captures `before`), `RemoveAudioClip`, `SetKey`, `Compound` | `crates/fontelle-model/src/commands.rs` (≈4275, 4371, 4691) | Render-back = `Compound[SetAudioClip(asset swap), SetStudy(render stamp)]`, so one undo. |
| Command / undo / wire | `Command` trait (`apply/invert/merge_with/to_edit`). `History` has outbox and gesture merging. The `wire::Edit` enum's variant names are permanent | `crates/fontelle-model/src/command.rs`, `wire.rs`, `tests/wire.rs` (`wire_variant_order_is_pinned`) | New study commands follow the `SetAudioClip` shape. New `Edit` variants are appended. |
| Project document | `Project { key: Option<KeyScale>, clips, markers: Arena<MarkerId,_>, loop_range, … }` | `crates/fontelle-model/src/project.rs` | Add `#[serde(default)] studies: Arena<StudyId, Study>`. |
| Clip editor window | `EditorKind::{Instrument, Effect, AudioClip}`. These are separate OS windows (`App.editors`, `open_editor`, `redraw_editor`) | `crates/fontelle-ui/src/layout/mod.rs:583`, `app.rs` (≈5157–6550) | Add `EditorKind::Analyze`. |
| Clip name menu | **Does not exist.** `ClipPart` is `Body/RightEdge/LeftEdge/Fade*/Point/Curve`. Right-click on a clip erases (FL style). `MenuTarget` has no clip variant | `crates/fontelle-ui/src/canvas/timeline.rs:1491`, `app.rs:539` | New `ClipPart::Title` (the block's name strip) and `MenuTarget::Clip(ClipId)`. See §3.2. |
| Waveform peaks | `PeakData`, `generate_peaks`, multi-level. The clip block preview comes from `Session::…preview` | `crates/fontelle-assets/src/peaks.rs`, `session.rs` ≈1240 | Draw the waveform background from it. |
| Decode / import | `import_audio`, `read_audio` (symphonia: wav/flac/mp3/vorbis), `AudioStore` (stereo) vs `SampleStore` (mono, sampler) | `crates/fontelle-assets/src/audio_import.rs`, `crates/fontelle-core/src/streaming.rs` | Analysis reads through `import_audio`. |
| Content hash | SHA-256 low 64 bits plus hex, cached by path/mtime | `crates/fontelle-assets/src/content_hash.rs` | Analysis cache key. |
| WAV writing | `WavWriter::create/write/finish` | `crates/fontelle-assets/src/wav_writer.rs` | Renders and takes. |
| Render / bounce | `render_lane`, `start_bounce`, `poll_job` (worker thread, `JobPoll::{Idle,Running,Finished}`, `JobProgress{label,fraction}`), one bounce at a time | `crates/fontelle-app/src/session.rs` ≈3600–4060, `fontelle-ui/src/document.rs:960` | Follow the pattern for the analysis job and the study render. Keep these in a **separate** job slot so they never block a bounce. |
| Recording | `start_input_stream(Some(name), writer)`, `input_capture_channel(96_000*2)`, `keep_audio_take` (drain, WAV into `recordings/Take N.wav`, `import_audio_at`), `take_path` (makes an unsaved project real), `armed_track` | `session.rs` ≈990–1225, `crates/fontelle-engine/src/{audio_input,input_monitor,device,pipewire}.rs` | Reuse the capture ring. See R4 for the "one device opened twice" problem. |
| Sampler | `Patch { layers: Vec<Layer> }`. `Layer { source: Source::Sample{file}, key_range, vel_range, root_key, fine_tune_cents, playback: PlaybackConfig{start_offset,end_offset,loop_*,…} }`. `sampler_patch()` builds a one-shot. `add_sampler_from(path)` | `crates/fontelle-core/src/{patch.rs,playback.rs,sampler.rs}`, `session.rs:5772–5900` | Slice-to-sampler = N layers over one rendered file with `start/end_offset` (frames, check the units in `voice.rs:1429`). |
| Drum Machine | `InstrumentKind::DrumMachine` is **synthesised** (`Source::Drum(DrumVoice)`, GM map). It cannot hold samples | `crates/fontelle-core/src/drum_kit.rs`, `fontelle-types/src/instrument.rs` | Do **not** target it. "Send to sampler" makes an `InstrumentKind::Sampler` channel, with an optional GM-drum key layout (§3.8). |
| Pitch tracking | `PitchTracker`: two-pass YIN (CMNDF, decimated coarse pass plus refine), octave guard, gate. `hz_to_cents/cents_to_hz` | `crates/fontelle-dsp/src/pitch.rs` | Pull the difference/CMNDF function out into a pure `yin_cmndf(frame, τ range)` and build offline **pYIN** on it. Keep the realtime tracker. |
| Pitch shifting | `PsolaShifter` (TD-PSOLA, synthetic marks, `set_ratio`, `set_formant`, `GrainEngine::{Smooth,Hard,Grain}`), RT-safe | `crates/fontelle-dsp/src/psola.rs` | Good enough for P2's "Standard" engine once it is driven by an offline, per-sample ratio curve and real pitch marks (§2.3). |
| Autotune | `fontelle-fx/src/tune.rs`. `TuneConfig { root, scale: TuneScale, notes: u16 mask, … }`. `TuneScale` is a **fixed enum** (15 entries, stored by index), and `Custom` reads the `notes` mask | `crates/fontelle-fx/src/tune.rs`, `fontelle-types/src/effect.rs:4520,4808`, `docs/tune-plan.md` | Pasting a scale into Tune = a named `TuneScale` if the pitch set matches, else `Custom` plus mask. The trace drawing in `canvas/tune.rs` (rails, sung vs corrected) is the visual model for the pitch curve. |
| Scales / song key | `SCALES` (ids are permanent, families, aliases), `Scale::mask(root)`, `KeyScale{root, scale:id}`, `fit_to_scale`. Roll side: `RollScale`, `row_shade`, `scale_fit`, `MenuTarget::{KeyRoot,KeyScale}` | `crates/fontelle-types/src/scale.rs`, `crates/fontelle-ui/src/canvas/roll_scale.rs` | Key detection outputs `KeyScale`. "View notes in scale" reuses `row_shade`. "Set as song key" is the existing `SetKey`. |
| Note model | `Note { start, length: Tick, key, velocity, fine_pitch: i16, path: Vec<PathPoint{at: Tick, offset: i8 semitones}>, slide, channel }` | `crates/fontelle-model/src/note.rs` | Copied notes use `key` + `fine_pitch`. Bends of a semitone or more become `path` points. Paths are integer semitones, so finer bends are lost (they are approximated by `fine_pitch`). |
| Piano roll clipboard | `PianoRoll.clipboard: Vec<Note>` (private to the canvas). `copy/cut/paste(at)` snap; `selected_phrase` puts notes at 0 | `crates/fontelle-ui/src/canvas/piano_roll.rs:2015, 3301–3400` | Lift it into one `NoteClipboard` owned by `App` that both windows share (§3.6). |
| Text clipboard | `TextClipboard`, `SystemClipboard` (wl-copy/xclip/xsel, PowerShell), `StudioHost::system_clipboard` | `canvas/text_entry.rs`, `fontelle-app/src/desktop.rs:108–220`, `document.rs:1446` | The scale chip also puts "A natural minor" on the desktop clipboard. |
| Keymap | `Action` enum (stable ids), `Context::{Global,Studio,Editor,Drawing}`, rebindable `Chord`s | `crates/fontelle-ui/src/canvas/keymap.rs`, `keybinds.rs` | New actions in `Context::Editor`. |
| Flopsynth look | Cards on a grid (`FLOP_GRID`, `CARD_HEADER 18`, `CARD_PAD 6`, `CARD_GAP 8`, knobs 40/32/20, `PICTURE_HEIGHT 60`, `CANOPY_HEIGHT 120`, `SCALES [0.75,1,1.25,1.5]`). `fit_cards` (shared with Tune via `TUNE_GRID`). Drawing in `render/bridge.rs`: `draw_hull`, `draw_canopy`, `draw_console`, `draw_screen`, `draw_hud_tab`. `render/mod.rs`: `draw_flop_card/knob/chip/switch/button`, `card_ink` | `crates/fontelle-ui/src/canvas/flopsynth.rs`, `canvas/tune.rs`, `render/bridge.rs`, `render/mod.rs` ≈10742–12950 | The Analyze window is built from these same parts (§3.1). |
| Theme | `Palette` tokens: `accent`, `playhead`, `note`, `note_selected`, `row_out_of_scale`, `row_scale_root`, `modulation`, `mod_envelope/lfo/macro/note/performance`, `meter`, `meter_peak`. The type ramp is 15/12/12 tabular (`flopsynth-next.md` §3.1 principle 11) | `crates/fontelle-ui/src/theme/mod.rs`, `docs/themes.md` | Prefer **no new tokens**. If confidence colours are needed, reuse `meter` (good), `automation`/`param_automated` (fair), `meter_peak` (poor). |
| FFT | `realfft 3.5` (already in `fontelle-fx`), `fontelle-dsp::spectrum::{fft_in_place, SpectrumAnalyser, analyse_spectral}` | `crates/fontelle-dsp/src/spectrum.rs`, `fontelle-fx/Cargo.toml` | STFT/CQT for the spectrogram, chroma and denoise. |
| Resampling | `rubato 5` (engine) | `crates/fontelle-engine/Cargo.toml` | Resample to 22 050 Hz for basic-pitch, back to file rate for renders. |
| Preview playback | Preview voice = one-shot mono sampler (`preview_import`, `end_preview`) | `session.rs:10883–10935` | Too limited (mono, no seek or loop, no playhead). Add a small `StudyPlayerNode` (§3.10). |

**Repo facts this plan adapts to:**
- There is no clip context menu yet.
- The roll clipboard is canvas-private.
- `TuneScale` is a closed enum.
- `PathPoint` offsets are whole semitones.
- Drum Machine is synth-only.

---

## 2. Technology choices and licences

### 2.1 Where the code lives

**New crate `crates/fontelle-analysis`.**
- Pure, with no UI or engine dependency.
- Depends on `fontelle-dsp`, `fontelle-types`, `realfft`, `rubato`, and `tract-onnx` behind the default feature `model`.
- `fontelle-app` depends on it. `fontelle-ui` does not; it gets plain view structs through `StudioHost`, the same way `NotePreview` and `AudioPreview` work today.

Modules:
- `transcribe/` (basic-pitch and the classic fallback)
- `mono.rs` (pYIN plus note segmentation)
- `key.rs`, `chords.rs`, `confidence.rs`
- `denoise.rs`, `onsets.rs`, `slice.rs`
- `resynth/` (behind the `Resynth` trait)
- `render.rs` (the study chain → buffer)
- `cache.rs` (serde of `Analysis`)

### 2.2 Polyphonic transcription

**Choice: Spotify basic-pitch (ICASSP 2022 "NMP" model), run in pure Rust with `tract-onnx`. A classic CQT-salience engine is the fallback.**

- **Licence.** The code is Apache-2.0. `nmp.onnx` ships *in* the repo at `basic_pitch/saved_models/icassp_2022/nmp.onnx` alongside `.tflite`, `.mlpackage` and the SavedModel. **No separate weights licence was found**, so the repo-wide Apache-2.0 LICENSE covers it. That fits our allow-list. We must ship the Apache LICENSE/NOTICE text in `licenses/` and the About page. **To verify before P1 lands:** ask Spotify via an issue, or confirm there is no `NOTICE` exception. Pin the file hash.
- **Runtime.** Choose `tract-onnx` (sonos/tract, MIT OR Apache-2.0, pure Rust, x86/ARM, Linux/macOS/Windows) over `ort` (pyke, MIT/Apache).
  - `ort` downloads or links Microsoft's prebuilt onnxruntime binaries per target. That complicates the four-archive release, the `[sources]` rules in `deny.toml`, and the macOS cross-clippy preflight.
  - tract is one more pure-Rust dependency and adds no native libraries.
  - **Risk:** operator coverage for basic-pitch's in-graph harmonic-stacking CQT. P0 is a spike that proves it.
- **What it gives.** Input is audio resampled to 22 050 Hz, in roughly 2 s windows with overlap. Output is three posteriorgrams at about 86 frames/s:
  - *onset* (88 keys)
  - *note* (88 keys)
  - *contour* (264 bins = 3 per semitone)

  Port `note_creation.py` (Apache-2.0) to Rust for the post-processing: onset peak-picking, note tracking with frame/onset thresholds, minimum length, and **pitch bends taken from the contour bins** inside each note's ±1 semitone. Notes come out as `(start_s, end_s, midi, amplitude)` plus a bend curve in about 33-cent steps, interpolated more finely by parabolic fit across the contour bins.
- **Realistic quality.**
  - Good on solo voice, guitar, piano, bass, and simple chords plus melody.
  - Usable on sparse mixes.
  - Weak on dense full mixes: false notes from harmonics, and drums produce junk notes.
  - Not instrument-aware: everything lands on one lane.
  - That is why the extraction-confidence meter (§2.6) matters.
- **Cost.** The model is tiny (well under 1 MB, reported as about 17k parameters; **verify the file size**) and fast on CPU. Target: **a 3-minute song analysed in about 5 s on one core.** Measure it in `benches/analysis.rs`. Memory: posteriorgrams for 3 minutes ≈ 15.5k frames × 440 f32 ≈ 27 MB transient. Keep only the notes plus the contour downsampled to u8, which is the spectrogram-like background image (about 4 MB).
- **Alternatives considered.**
  - **MT3** (Apache-2.0, multi-instrument): a T5 transformer with hundreds of MB, needs JAX/seq decoding. Rejected for v1.
  - **Classic engine** (CQT 36 bins/octave → harmonic-sum salience → peak tracking → NMF with harmonic templates for 2–4-voice material): no weights and fully ours. Ship it as `Engine::Classic`, used when the `model` feature is off or the model fails to load. It also provides the CQT chroma for key detection and the background spectrogram. Lower quality on chords, fine on monophonic material.

### 2.3 Monophonic pitch tracking (vocals, solo instruments)

- **In tree:** YIN (`fontelle-dsp/src/pitch.rs`). **Build offline pYIN** (Mauch & Dixon 2014; an algorithm, so no licence issue):
  - multiple CMNDF thresholds give pitch candidates with probabilities
  - an HMM over 20-cent pitch states plus an unvoiced state
  - Viterbi decoding
  - Output: an F0 track at 5.8 ms hops (256 @ 44.1k) with per-frame voicing probability.
  - Offline gives look-ahead and smoothing that the realtime tracker cannot have.
- **Segmenting into notes (Melodyne-style "blobs"):**
  - split on unvoiced gaps over 40 ms, on energy onsets (spectral flux peaks from `onsets.rs`), and on pitch jumps (more than 70 cents sustained for over 30 ms)
  - each note's **pitch centre** = duration-weighted median of its stable middle 60 %
  - **drift** = low-pass (< 3 Hz) of contour minus centre
  - **vibrato** = residual band 3–9 Hz
- **Mono vs poly decision** is automatic:
  - mono if basic-pitch reports at most 1 active note in at least 90 % of active frames, **and** the pYIN voiced frames have mean voicing probability of at least 0.6
  - the user can override with a chip: `Melody | Chords`
- **Neural F0 options, later and optional, behind the same tract seam:**
  - CREPE (MIT code; "tiny" model weights are a few MB, full is about 80 MB; **weights licence to verify**)
  - FCPE / torchfcpe (MIT; **weights licence to verify**)
  - Not needed for v1. pYIN is excellent on clean vocals.

### 2.4 Natural pitch editing: the engines

Two rules make it sound natural, whatever the engine:

1. **Only edited spans are resynthesised.** Everything else is bit-identical source. Each edited note is rendered with about 30 ms of padding and spliced back with an equal-power crossfade of about 10 ms, placed at a low-energy, pitch-synchronous point. That is the "fix one note in a sung take" requirement.
2. **Edit the F0 *contour*, not a constant ratio.** `f0_out(t) = f0_in(t) · 2^((shift(t) − flatten·drift(t) + (vib_scale−1)·vibrato(t))/1200)`. `shift(t)` eases in and out over the note's transition times (default 40 ms, clamped to the gap to neighbours). The singer's vibrato and scoops survive a move, which is what makes a moved note sound sung rather than tuned.

All engines sit behind one trait in `fontelle-analysis/src/resynth/mod.rs`:

```rust
pub trait Resynth: Send + Sync {
    fn id(&self) -> &'static str;                 // permanent, stored in the study
    fn render_span(&self, input: &[f32], channels: usize, sr: u32,
                   f0_in: &F0Track, f0_out: &F0Track, formant_cents: &Curve,
                   span: Range<usize>, out: &mut Vec<f32>) -> Result<(), ResynthError>;
}
```

| Engine | What | Licence | Use |
|---|---|---|---|
| **`psola`** (P2, default) | Offline TD-PSOLA with **real pitch marks**: epoch placement at the waveform peak per period, guided by pYIN. The ratio curve is applied per period. Formant is kept by construction. Built on `fontelle-dsp/src/psola.rs`'s grain code, refactored to take a mark list and a per-mark ratio | ours | Voice and monophonic instruments. Excellent for ±3 semitones; fine to ±7 |
| **`world`** (P2b, optional "High quality") | WORLD vocoder (Morise): DIO/Harvest F0, CheapTrick envelope, D4C aperiodicity, synthesis with the edited F0. Its strength is drift flattening and large shifts with formant control | WORLD is **modified BSD**. Rust wrapper `rsworld` / `rsworld-sys` (**licence of the wrapper reported MIT; verify**) compiles the C++ with `cc` | Optional feature `world`. Needs a C++ toolchain on all four CI targets. Its synthesis can sound buzzy on breathy voices, so it is not the default |
| **`stretch`** (P6, experimental polyphonic) | signalsmith-stretch (phase-vocoder-family, multi-channel, `setFormantFactor`) | **MIT**. Rust wrapper crate `signalsmith-stretch` 0.1.3 (MIT; builds C++; **check build.rs needs**: cc only, or bindgen/libclang?) | Pitch-shifting polyphonic material and masked regions |

**A future seam, mentioned only:** a higher-quality resynthesis engine could later plug in as another `Resynth` implementation, selected by `id`. Nothing in this plan depends on one.

### 2.5 Polyphonic editing: realistic scope

Separating one note out of a chord (Melodyne DNA) is research-grade work. Proposed scope:

- **v1 (P1):** polyphonic notes can be **viewed, selected, copied and auditioned (as MIDI through the selected channel)**. They are not movable. The note's hover says "Chord notes can't be moved yet. Copy them to a piano roll." The UI is honest about this.
- **Monophonic material (P2):** fully editable.
- **P6, "experimental" switch:** move one polyphonic note by:
  1. building a harmonic mask from its contour (partials k·f0 ± ½ bin, k ≤ 30, soft Gaussian across frequency, onset/offset ramps)
  2. extracting the masked STFT, pitch-shifting that component with `stretch` (or a phase-locked vocoder), and resynthesising
  3. writing it back into the residual (original × (1 − mask))

  Overlapping partials (octaves, fifths) will smear. The window stamps the edit "experimental" with an amber badge, and A/B (`B` key) is one keypress away.

### 2.6 Key, scale, chords and the two confidences

**Chroma, two sources:**
- (a) **note chroma**: Σ duration × amplitude × confidence per pitch class, from the detected notes, with bass notes weighted ×1.5
- (b) **CQT chroma**: tuning-corrected (the global tuning offset is estimated from the contour bins' distribution mod 100 cents and reported: "Tuning: +14 ct")
- Blended at 0.6 / 0.4 when extraction confidence is above 0.5, otherwise CQT-weighted.

**Key:**
- correlate against 24 rotations of the **Krumhansl–Kessler** and **Temperley (Kostka–Payne)** profiles, averaged (published numbers, not licensed artefacts)
- then for the winner's pitch set, test the modes and common scales in `SCALES` with a pitch-set fit: coverage of note mass, and penalised out-of-set mass
- Report the main reading (e.g. **A natural minor**) plus up to two alternatives (C major, the relative; A dorian if F♯ shows).

**Key confidence**, calibrated rather than raw r:
- `p_key = σ(a·(r₁ − r₂) + b·r₁ + c·log(note_mass) + d)`
- `r₂` is the best key that **is not the relative** (relative pairs share a pitch set). Their ambiguity is shown as a separate "tonic" read-out: "A minor 78 % · or C major".
- Coefficients are fitted offline on a labelled set, kept in a test fixture, and pinned by tests. The calibration corpus is **not shipped**, so its licence only has to allow research use (to verify).
- Synthetic sets made from our own MIDI fixtures rendered through Flopsynth are always available as a floor.

**Chords** (cheap and very beginner-friendly): per-beat segments, using the project tempo, or 0.5 s when standalone. Template-match note chroma against triads, sevenths, sus and power chords. Print the labels in a thin chord lane (`Am  F  C  G`). They are copyable as notes too.

**Extraction confidence**, "how well notes could be pulled out at all", from 0 to 1:
- `c_post`: mean peak *note* posterior inside detected notes
- `c_cov`: fraction of frames with energy above the noise floor that are explained by at least one confident note (energy without notes is bad)
- `c_snr`: SNR estimate, from the 95th to 10th percentile of frame energy
- `c_harm`: harmonicity, from the YIN aperiodicity median
- `c_perc`: percussiveness penalty, from spectral flatness and flux

Combine as `logistic(w·[…])`, calibrated the same way. Show it as a word plus a % (**Clear 91 %**, **Usable 64 %**, **Rough guess 18 %**). A drum loop or noise still gets notes and a key; they are simply labelled a rough guess. That is the "garbage samples still get a guess" requirement.

### 2.7 Noise, trim, markers, slices

- **Denoise** (`denoise.rs`):
  - STFT 2048 / hop 512, Hann, `realfft`
  - capture the noise profile from a selection: per-bin 50th–80th percentile magnitude, plus the 1/3-octave smoothed shape
  - **decision-directed Wiener / spectral subtraction** (Ephraim–Malah style a-priori SNR, over-subtraction α 1–4 from an "Amount" knob, spectral floor β 0.02–0.2 against musical noise, 2-frame temporal smoothing)
  - "Reduce by" dB knob, "Sensitivity" knob, and a "Listen to what's removed" switch (outputs the difference)
  - Optional **voice denoiser:** `nnnoiseless`, a pure-Rust RNNoise port. RNNoise is **BSD-3-Clause**. **Verify that the nnnoiseless crate's own licence is BSD-3 or MIT/Apache** before adding it. It runs at 48 kHz mono only and needs no profile.
- **Trim / fades / gain** are study operations. Trim maps onto the clip's `source_start/end` at render, so the file is not cut.
- **Markers / regions:** sample-positioned, inside the study. They are not song markers.
- **Onsets** (`onsets.rs`): super-flux on a log-mel spectrogram, adaptive threshold. They feed "Slice at transients", "Slice at notes" and segmentation.

### 2.8 Licence summary

| Component | Licence | Status |
|---|---|---|
| basic-pitch code | Apache-2.0 | verified (repo) |
| basic-pitch `nmp.onnx` weights | in-repo, covered by repo Apache-2.0; no separate weights licence (checked 2026-10-06 at `fa5997a`, `licenses/MODELS.md`) | shipped with LICENSE/NOTICE in `licenses/basic-pitch/`; an upstream word on the training data before release stays advisable |
| tract / tract-onnx | MIT OR Apache-2.0 | verified |
| ort (not chosen) | MIT/Apache (wraps MIT onnxruntime binaries) | not chosen |
| WORLD | modified BSD | verified (widely documented) |
| rsworld / rsworld-sys | reported MIT | **verify** |
| signalsmith-stretch (C++) and crate `signalsmith-stretch` 0.1.3 | MIT | verified (repo and crates.io); **check build deps** |
| RNNoise | BSD-3-Clause | verified |
| nnnoiseless | reported BSD-3 | **verify** |
| CREPE / torchfcpe code | MIT | weights **verify** (optional, later) |
| realfft, rustfft, rubato, symphonia | already in tree | already pass `deny.toml` |

Sources:
- [basic-pitch repo](https://github.com/spotify/basic-pitch)
- [basic-pitch model dir](https://github.com/spotify/basic-pitch/tree/main/basic_pitch/saved_models/icassp_2022)
- [tract](https://github.com/sonos/tract)
- [signalsmith-stretch](https://github.com/Signalsmith-Audio/signalsmith-stretch)
- [signalsmith-stretch crate](https://crates.io/crates/signalsmith-stretch)
- [rsworld](https://lib.rs/crates/rsworld)
- [world-sys](https://github.com/echelon/world-sys)
- [RNNoise COPYING](https://gitlab.xiph.org/xiph/rnnoise/-/blob/main/COPYING)
- [nnnoiseless](https://docs.rs/crate/nnnoiseless/latest)
- [ort search result](https://docs.rs/crate/ort/latest/source/)
- [CREPE](https://paperswithcode.com/paper/crepe-a-convolutional-representation-fo2)
- [FCPE](https://arxiv.org/html/2509.15140v1)

---

## 3. UX and visual design

### 3.1 The window

New `EditorKind::Analyze`. It is an OS window like Tune's console and is built from Flopsynth's parts:
- hull ground (`bridge::draw_hull`)
- **the canopy becomes the instrument**: one large `draw_screen` (the note lane), in the role Tune's viewport plays
- HUD tabs on the canopy glass (`draw_hud_tab`)
- consoles (`draw_console`) for the cards at the bottom
- chips and switches from `draw_flop_chip/switch`
- the same scale chooser (75–150 %)

Title: "Analyze Musically · <clip name>". Default size 1180 × 740 (Flopsynth's), minimum 900 × 600.

```
┌─ Analyze Musically · Vox take 3 ───────────────────────────────────────────────┐
│ ◉ Clear 91%   ♪ A natural minor 82% ▾  ⧉   Tuning +6 ct   ♩ 92 BPM   [Melody|Chords] │  ← header strip
│ ─────────────────────────────────────────────────────────────────────────────── │
│  NOTES   CLEAN   SLICE   RECORD                            ▸ 0:12.4 / 1:03.0    │  ← HUD tabs (pages)
│ ┌─────────────────────────────────────────────────────────────────────────────┐ │
│ │ Am        │ F          │ C          │ G           ← chord lane (toggle C)   │ │
│ │C5 ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄ │ │
│ │B4 ▒▒▒▒(out of scale rows dimmed)▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒ │ │
│ │A4 ═══════════[███████~~~]════════[████]═══  ← note blobs, pitch curve inside │ │
│ │G4 ┄┄┄┄┄┄┄┄[██████]┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄[██████▲ 23ct]┄┄┄┄┄┄┄┄┄┄┄┄┄┄ │ │
│ │   ░░▓▓██▓▓░░░▓▓███▓░░░  waveform / spectrogram background (Tab toggles)   │ │
│ │ ▼M1            ▼M2                [==== selection ====]          ▼M3       │ │  ← markers, region
│ └─────────────────────────────────────────────────────────────────────────────┘ │
│  ⬚ Select  ✥ Move  ✎ Draw  ✂ Trim  ⚑ Marker  ⌇ Noise     ⟲ A/B   ▶ Space       │  ← tool strip
│ ┌ NOTE ──────────┐ ┌ PITCH ─────────────────┐ ┌ OUTPUT ──────────────────────┐ │
│ │ A4  +23 ct     │ │ (●)CENTRE (●)DRIFT     │ │ [ Copy notes ⧉ ] [ Copy scale ]│ │  ← consoles (cards)
│ │ 0:04.21–0:05.8 │ │ (●)VIBRATO (●)GLIDE    │ │ [ Render to clip  ⏎ ]          │ │
│ │ conf 94%       │ │ (●)FORMANT  [Snap ⌁Q]  │ │ [ Send to sampler… ] More ▾   │ │
│ └────────────────┘ └────────────────────────┘ └──────────────────────────────┘ │
│  ▓▓▓▓▓▓▓▓▓▓▓░░░░ Analysing… 64%  (notes appear as they're found)               │  ← job strip (only while busy)
└────────────────────────────────────────────────────────────────────────────────┘
```

**Pages**
- **Notes**: the default and all a beginner needs.
- **Clean**: noise profile card (capture, amount, floor, listen-removed), trim/fade/gain card, voice-denoise switch.
- **Slice**: markers list, auto-slice (transients / notes / grid / equal N), layout (Chop C3↑ / By pitch / GM drums), "Send to sampler".
- **Record**: input chooser, monitor, count-in, punch/loop, takes list, comp lane.

Pages 2–4 are always present, so pros can find them. The Notes page never shows their controls.

**Styling rules taken from Flopsynth**
- Card inks via `card_ink`-style mapping:
  - NOTE = `p.note`
  - PITCH = `p.modulation`
  - OUTPUT = `p.accent`
  - CLEAN cards = `p.mod_lfo` (teal)
  - SLICE = `p.mod_macro` (amber)
  - RECORD = `p.meter_peak` (red lamp while armed)
- Captions in capitals only on knobs (§3.1 principle 12). Type is 15/12/12 tabular.
- Knob gestures are Flopsynth's: drag, Shift fine, Ctrl finer, Alt-click reset, double-click to type, right-click menu.

### 3.2 Opening it

1. **Clip name menu (new).** Add `ClipPart::Title`: the block's caption strip, about 14 px at the top of every block, excluding the fade handles, which already win at the corners (`fade_anatomy`).
   - Left-click on the title opens `MenuTarget::Clip(ClipId)` (FL's clip menu): *Analyze Musically…* · *Edit audio…* (the existing AudioClip editor) · *Rename* · *Render to audio* (lanes only) · ─ · *Make unique* (later).
   - For note and automation clips the Analyze row is omitted.
   - Right-click elsewhere on the body stays as erase.
   - Test: `fontelle-ui/tests/clip_menu.rs`.
2. **Ctrl+Shift+A** with an audio clip selected (rebindable `Action::AnalyzeClip`).
3. **Standalone instance**:
   - the browser's Import tab, right-click a file → *Analyze Musically*
   - the transport's record menu (`MenuTarget::RecordMode`) gains *Record into Analyze Musically…*
   - dropping a file onto an open Analyze window
   - A standalone study lives in the project (§3.9). It appears in a tiny list under the window's title menu ("Studies ▾") so it can be reopened.

### 3.3 The note lane

- **Axes:** time (zoom and scroll like the roll: wheel scrolls, Ctrl+wheel zooms, consistent with the studio). Pitch rows like the piano roll, with a mini key column on the left that auditions the row on click.
- **Background:** the waveform (peaks, from the clip's `PeakData`) by default. **Tab** switches to the spectrogram, which is the contour/CQT image at u8 intensity with the theme's ramp.
- **Notes as blobs** (Melodyne-like), rounded rects in `p.note`:
  - height scaled by amplitude
  - opacity = note confidence (below 0.4 drawn hatched)
  - the **pitch curve** drawn through them as a 1.5 px line (monophonic: the pYIN contour; polyphonic: the basic-pitch bend)
  - an edited note shows the original curve ghosted, and the new one in `note_selected`
  - a small cents tag (`▲ 23ct`) appears when the centre is off by more than 15 cents. That gives a glanceable "which notes are off" view.
- **Scale rails:** out-of-scale rows dimmed via `roll_scale::row_shade` (the same look as the roll). Root rows tinted.
- **Polyphonic notes** have a thin outline. In mono mode they are filled.

### 3.4 Header: confidence badges and the scale chip

- **Extraction badge** `◉ Clear 91%`: the lamp colour goes from good to fair to poor. Hover gives a one-line reason ("Lots of noise and drums: treat these notes as a starting point").
- **Scale chip** `♪ A natural minor 82% ▾ ⧉`:
  - **Click the name:** copies the scale (internal `ScaleClipboard` + desktop text "A natural minor"). Toast: "Scale copied, paste it into Tune or the piano roll's scale".
  - **Hover:** a popover with a one-octave keyboard lighting the scale, note names with degrees (A B C D E F G · 1 2 ♭3 4 5 ♭6 ♭7), and "or C major (relative) · A dorian 41%".
  - **▾:** menu: alternatives (each copyable) · *Show scale on lane* (toggle) · *Set as song key* (`SetKey`, one undo) · *Fit selected notes to scale* (mono: snap centres; poly: MIDI copy only).
  - **⧉** = copy (redundant on purpose for discoverability).
- **Paste targets**, all reading `App.scale_clipboard: Option<KeyScale>`:
  - piano roll's KeyRoot/KeyScale chip menus gain a first row *Paste scale (A natural minor)*, and Ctrl+V works while either chip is hovered
  - Tune's keyboard card right-click → *Paste scale* (named `TuneScale` if the pitch set matches, else `Custom` + `notes` mask)
  - Flopsynth's scale chooser (`MenuTarget::FlopScale`) gets the same row

### 3.5 Tools and keyboard

The tool strip (chips, `draw_flop_chip`) and the shortcuts. Every one is a new `Action` in `Context::Editor`, rebindable, with ids frozen.

| Key | Action |
|---|---|
| `1`..`6` / `S` `M` `D` `T` `K` `N` | Select / Move / Draw / Trim / Marker / Noise-capture tools |
| `Space` | Play/stop from the cursor (loops the region if one is set) |
| `Enter` | Play the selection |
| `B` | A/B: hear the original vs the edits |
| `↑` `↓` | Selected notes ± 1 semitone (`Shift` octave, `Alt` ± 10 ct) |
| `Q` | Snap pitch centre to nearest note (scale-aware when scale is shown); repeat = 50 % → 100 % |
| `F` | Flatten drift (toggle 0 → 70 %) |
| `V` | Vibrato: cycle 100 / 50 / 0 % |
| `Del` | Reset selected notes' edits |
| `Ctrl+A`, `Ctrl+Z / Ctrl+Y` | All notes; document undo/redo (the window routes to the studio's history) |
| `Ctrl+C` | Copy notes as MIDI (selection, or all) |
| `Ctrl+Shift+C` | Copy scale |
| `Ctrl+Enter` | Render to clip |
| `Ctrl+B` | Send to sampler… |
| `Tab` | Waveform ↔ spectrogram |
| `C` | Chord lane |
| `Z` | Zoom to selection; `Shift+Z` zoom all |
| `R` | Record (Record page) |
| `?` / `F1` | Shortcut overlay (Flopsynth's overlay pattern) |

Drags on the lane in Move mode:
- vertical drag moves the centre, snapping to semitones; `Alt` frees it to cents
- dragging the blob's ends moves its transitions (glide in/out)
- `Ctrl`+drag on a note draws a replacement contour, as a pro feature (P2b)

### 3.6 Copy notes → paste into any piano roll

- **Shared clipboard:** move `PianoRoll.clipboard: Vec<Note>` into `App.note_clipboard: NoteClipboard { notes: Vec<Note>, origin: Option<(ClipId, Tick)> }` (`fontelle-ui/src/canvas/note_clipboard.rs`). The roll's `copy/cut/paste` take `&mut NoteClipboard`. Paste semantics stay the same (earliest note at 0, snapped landing). Existing tests in `piano_roll` / `arrange_clipboard` keep passing, and that is the regression check.
- **Seconds → ticks:**
  - For a clip study, the host converts with the project `TempoMap` at the clip's song position (`AudioPlacement`), so `origin` = the clip's start tick. The roll's paste menu then offers *Paste at original position* (lines up under the audio), as well as the normal paste at the playhead.
  - For a standalone study, use the project tempo (or the detected BPM if "Use detected tempo" is ticked).
- **Pitch:**
  - `key` = rounded centre
  - `fine_pitch` = the remaining cents. This is opt-in, default **off**, because most users want clean MIDI.
  - Bends ≥ 1 semitone become `path` points (whole-semitone `PathPoint.offset`) when "Keep slides" is on.
  - Velocity comes from amplitude (log-mapped 30–120).
- **One more button:** *Make a note clip under this audio* = `Compound[AddLane(below), AddNoteClip(channel = selected)]`. One undo, and the notes sit aligned beneath the clip.
- **Later (P7):** drag notes out of the window as a `.mid` via `midi_export.rs`.

### 3.7 Render to clip

1. Press `Ctrl+Enter` or **Render to clip**.
2. The study chain renders off-thread through the job strip:
   - trim does not apply here (it is mapped onto the clip)
   - denoise
   - pitch edits
   - study gain
3. The output goes to `<bundle>/renders/<clip> (edited N).wav` at the source rate and channel count, **whole-file length**, so the clip's `source_start/end`, fades and `loop_phase` stay valid.
4. One command: `Compound("Render edits to Vox take 3", [SetAudioClip{asset: new}, SetStudy{rendered: Some(new_asset_ref), …}])`.
   - Undo puts the original asset back.
   - The study keeps `original: AssetRef`, so edits always re-render from the original and never compound artefacts.
5. Afterwards the window says "Clip uses your edits · Revert to original".
6. An option in the Output card's ▾ menu: *Render as a new clip below* (keeps the original clip, adds a lane), for people who want both.

### 3.8 Send to sampler

On the Slice page:

1. **Choose slice points:** markers, or *Auto: transients / detected notes / every beat / N equal*. The sensitivity knob previews the markers live.
2. **Choose a layout:**
   - **Chop:** slice *i* → key 48 + i, `root_key` = same key, plays at its own pitch (FL Slicex style)
   - **By pitch:** each slice's detected centre becomes `root_key`, and key ranges are split halfway between neighbouring roots, giving a playable melodic multisample. `fine_tune_cents` = −cents offset, so it plays in tune.
   - **Drum map:** slices are classified kick/snare/hat/other by spectral centroid and low-band energy and placed on GM keys 36/38/42/…, so drum patterns written for Drum Machine play the chops.
3. Options: *also make a note clip that replays the slices in order* (Slicex's killer feature), *one-shot / hold*, *2 ms de-click fades*.
4. **Commit:**
   - render the processed audio once to `<bundle>/renders/<name> slices.wav`
   - build one `Patch` with N `Layer{ source: Sample{file}, playback.start_offset/end_offset }` (offsets in frames; verify units in `voice.rs:1429`)
   - `Compound[AddChannel(InstrumentKind::Sampler, patch), optionally AddNoteClip]`. One undo.
   - Session side: a new `Session::add_sampler_slices(path, slices, layout)` next to `sampler_patch()` at `session.rs:5870`.

### 3.9 Data model, undo, collaboration

- **New id:** `StudyId` (`fontelle-types/src/id.rs`, INVARIANT 8).
- **Types** in `fontelle-types/src/study.rs` (serde, shared by model and app):

```rust
pub struct Study {
    pub name: String,
    pub source: StudySource,              // Clip(ClipId) | Standalone
    pub original: AssetRef,               // never modified
    pub rendered: Option<AssetRef>,
    pub engine: StudyEngines,             // transcriber id + resynth id (permanent strings)
    pub mode: Option<StudyMode>,          // user override Melody/Chords
    pub pitch_edits: Vec<PitchEdit>,      // self-describing, see below
    pub markers: Vec<StudyMarker>,        // { id: u32 (study-local, monotonic), at: Sample, name }
    pub region: Option<(Sample, Sample)>,
    pub trim: Option<(Sample, Sample)>, pub fades: (Fade, Fade), pub gain_db: f32,
    pub denoise: Option<Denoise>,         // profile: Vec<f32> 1025 bins (small), amount, floor, voice: bool
    pub takes: Vec<Take>, pub comp: Vec<CompSpan>,   // P5
}
pub struct PitchEdit {          // independent of analysis ids, so re-analysis never orphans an edit
    pub span: (Sample, Sample), // the note's extent at edit time
    pub shift_cents: f32, pub flatten: f32, pub vibrato: f32,
    pub glide_in_ms: f32, pub glide_out_ms: f32, pub formant_cents: f32, pub gain_db: f32,
    pub experimental_poly: Option<f32>, // P6: the note's centre it was masked at
}
```

  The analysis result is **not** in the document. It is derived and cached (§3.10). An edit names a sample span, not a detected-note index, so a newer model or a different machine's float rounding can never orphan an edit.
- **Project:** `#[serde(default)] pub studies: Arena<StudyId, Study>`. Follow the project's format/migration rule: a defaulted field may not need a version bump. Confirm against how `key` and `lane_routing` were added.
- **Commands** (`fontelle-model/src/commands.rs`, each with inverse and `to_edit`, `wire::Edit` variants appended, `tests/wire.rs` updated):
  - `AddStudy`, `RemoveStudy`
  - `SetStudyEdits { study, edits }`, with `merge_with` for continuous drags (one history entry per gesture, like note drags)
  - `SetStudyMarkers`, `SetStudyClean` (trim/fades/gain/denoise), `SetStudyMode`, `SetStudyRender`
  - `SetStudyTakes` (P5)
  - Clip removal: `RemoveAudioClip` leaves the study orphaned → it becomes `Standalone` (no dangling id). Do this inside `RemoveAudioClip`'s apply with its inverse, or via a `LaneUpkeep`-style post-step.
- **Non-destructive until render:** the arrangement plays the original until Render. The window previews edits live (§3.10). A later, optional "Keep clip in sync" would auto-render after edits, debounced.
- **Collaboration:** study commands travel like every edit, as small JSON. The noise profile is about 1 k floats; quantise it to `u16` dB if size matters. Analyses are **recomputed on each machine** (deterministic within an engine version; edits don't depend on them). Rendered WAVs and takes are assets and ship through the existing `collab/files.rs` relink path like recordings. A remote peer's render becomes a remote `SetAudioClip`, and the file follows. Recording is local; takes ship only once committed to the study.

### 3.10 Performance and threading

- **Analysis job:** `Session::start_analysis(study)` runs on its own worker (`fontelle-analyze`, below-normal priority), separate from `job` (bounce), and reports through a new `StudioHost::poll_analysis() -> AnalysisPoll`. It is **progressive**: chunks of 10 s with 1 s overlap, and notes are published as each chunk finishes, so the lane fills left to right. It can be cancelled when the window closes.
- **Cache:** `<bundle>/cache/analysis/<sha256-hex>-<engine-version>.json`, or `$XDG_CACHE_HOME/fontelle/analysis/` before the project has a bundle (INVARIANT 10: both are allowed). The spectrogram image is stored beside it as `.bin`. A clip reopened later shows at once.
- **Preview player:** new `fontelle-engine/src/study_player.rs` (`StudyPlayerNode`).
  - It plays an `Arc<[f32]>` handed over at graph rebuild (preallocated), routed to master like the preview voice, with its own atomic playhead for the window's cursor, loop region and A/B as two buffers.
  - The edited buffer = original with re-rendered spans spliced in.
  - Edits re-render **only their spans** in the background, debounced 120 ms. Typical cost is a few ms per note with PSOLA.
- **UI:** drawing reads view structs only (`StudyView { notes, contour points per visible column, peaks, chords, markers, job }`) built per frame from cached data, never from the audio.
- **Budgets** (written as tests or benches):
  - 3-minute mono vocal fully analysed in under 3 s; with basic-pitch, under 6 s
  - one-note PSOLA re-render under 20 ms
  - window frame under 4 ms at 1180 × 740 with 2 000 notes

---

## 4. Phases

Each phase ships something usable. Write the tests first and confirm they fail.

### P0: spike (no UI; decides the transcriber)

- Load `nmp.onnx` in `tract-onnx` and run the 22 050 Hz windowing.
- Compare against reference outputs exported once from Python basic-pitch for 3 fixtures. Keep the npy/JSON references in `crates/fontelle-analysis/tests/fixtures/`, generated offline; the fixtures themselves are our own renders.
- Tests: `transcribe_model.rs::{the_model_loads, posteriorgrams_match_reference_within_1e-3, a_c_major_triad_comes_out_as_three_notes}`. Bench: `benches/analysis.rs`.
- **Go/no-go:** if tract can't run the graph in a day or two of work, fall back to `ort` with the `load-dynamic` feature (Ty decides, R1), or ship Classic only in P1.

### P1: see it, copy it (window, waveform, transcription, key, confidences, copy notes, copy scale)

- `fontelle-analysis`: classic engine, basic-pitch post-processing, pYIN, segmentation, key, chords, confidence, cache.
- `EditorKind::Analyze`, `ClipPart::Title` + `MenuTarget::Clip`, the Notes page (read-only), header badges, scale chip with popover, shared `NoteClipboard` and `ScaleClipboard`, paste rows in the roll, Tune and Flopsynth, *Set as song key*, *Make a note clip under this audio*, the progressive job strip.
- Tests first:
  - `fontelle-dsp/tests/pitch.rs`: `yin_cmndf_is_the_trackers_own` (refactor guard)
  - `fontelle-analysis/tests/mono.rs`: `pyin_tracks_a_sung_glissando_within_10_cents`, `a_sung_melody_segments_into_its_notes`, `drift_and_vibrato_are_separated`
  - `transcribe.rs`: `triads_and_a_melody_come_out_layered`, `silence_gives_no_notes`, `noise_gives_a_low_confidence_guess_not_nothing`
  - `key.rs`: `c_major_scale_is_c_major`, `a_minor_reports_its_relative`, `confidence_falls_as_the_material_gets_chromatic`, `calibration_is_monotone`
  - `chords.rs`: `i_vi_iv_v_reads_back`
  - `confidence.rs`: `clean_sine_melody_is_clear`, `drum_loop_is_a_rough_guess`
  - `cache.rs`: `same_bytes_same_analysis_from_cache`
  - `fontelle-ui/tests/analyze_window.rs`: layout fits at every scale; every hit has a tooltip; the scale chip click yields `CopyScale`
  - `clip_menu.rs`: a press on the title strip opens the clip menu, fade handles still win at the corners, right-click on the body still erases
  - `note_clipboard.rs`: the roll's existing copy/paste unchanged; notes from Analyze paste snapped; *paste at original position* lands under the clip
  - `fontelle-app/tests/analyze_session.rs`: opening on a clip starts a job; the job finishes; `SetKey` from the chip is one undo; Tune paste maps to a named scale or `Custom`
  - `render_headless.rs`: an `analyze-notes` scene. **Look at it.**

### P2: fix a note (monophonic pitch edit, preview, render back)

- Move tool, pitch card (centre / drift / vibrato / glide / formant), `Q`/`F`/`V`, the `StudyPlayerNode` preview and A/B, offline PSOLA with real marks, span-only resynthesis with spliced crossfades, Render to clip with undo and revert, the study commands with wire forms.
- Tests first:
  - `fontelle-analysis/tests/resynth_psola.rs`: `a_30_cent_flat_note_moved_lands_within_3_cents`, `unedited_samples_are_bit_identical`, `the_splice_has_no_click` (peak of the HF residual at the seams below −60 dBFS), `vibrato_survives_a_move`, `formant_peaks_stay_within_5_percent`
  - `render.rs`: `a_render_is_the_same_length_as_the_source`
  - `fontelle-model/tests/study_commands.rs`: apply, invert, merge of a drag, removal of a clip orphans its study to standalone
  - `tests/wire.rs`: new variants pinned
  - `fontelle-app/tests/analyze_render.rs`: render swaps the asset, undo restores it, a re-render starts from the original
  - `fontelle-engine/tests/study_player.rs`: no allocation in process (the RT-guard allocator), loop and seek
  - **Listen once** with `--render-wav`, on a real sung take.
- **P2b (optional):** `world` engine behind a feature, plus contour drawing (`Ctrl`+drag). Test: `resynth_world.rs` (same three assertions).

### P3: clean it (select/trim/markers/noise)

- Clean page: noise capture tool, denoise card with "listen to removed", trim/fades/gain mapped to the clip at render, markers and region on the lane, optional `nnnoiseless`.
- Tests first:
  - `denoise.rs`: `white_noise_under_a_tone_drops_by_the_set_amount`, `a_clean_tone_passes_within_0_5_db`, `no_musical_noise_above_threshold` (spectral-flatness check)
  - `onsets.rs`: `clicks_are_found_within_5ms`
  - `analyze_clean.rs`: trim becomes `source_start/end` on render, and is one undo
  - UI: the marker tool adds and moves; `Del` removes

### P4: chop it (slice to sampler)

- Slice page, auto-slice, the three layouts, the "replay in order" note clip, `Session::add_sampler_slices`.
- Tests first:
  - `slice.rs`: `n_markers_make_n_plus_1_slices`, `by_pitch_ranges_tile_the_keyboard_without_gaps`, `drum_map_puts_a_kick_on_36`
  - `fontelle-app/tests/analyze_sampler.rs`: one undo removes the channel and the clip; each key plays its slice (render a note and correlate against the slice); the replay clip reproduces the source within −30 dB residual

### P5: record it (takes, comp, cleanup, send to arrangement)

- Record page: input chooser (reuses `TrackInput` lists), monitor, count-in, free or song-synced takes into `recordings/`, a takes list (keep / discard / star), a comp lane (swipe a span on a take to choose it; 10 ms crossfades), then clean/tune → **Send to arrangement** as a new rendered clip at the take's song position, or at the playhead when free.
- Tests first:
  - `fontelle-app/tests/analyze_record.rs`: a take lands in the study, not the arrangement; discard deletes its file only if no command references it; send-to-arrangement is one undo; recording works with the transport stopped
  - `comp.rs`: `a_comp_of_two_takes_crossfades_without_a_click`
  - Engine: the capture ring is shared with an armed track that has the same device open (R4)

### P6: experimental polyphonic editing

- Harmonic-mask extraction plus the `stretch` engine (signalsmith), an "Experimental" switch, the amber badge.
- Tests first:
  - `resynth_poly.rs`: in a C-E-G triad, moving E to F makes the F partials rise and leaves the C and G partials within 3 dB; untouched spans are bit-identical

### P7: polish and pro extras (pick per demand)

- tempo/beat detection, so copied notes land on the grid
- drag `.mid` out
- audio → Drum Machine pattern (onset classes → GM keys)
- vocal "Correct all" (snap to scale at X %, Tune-style but offline)
- per-note gain
- time-moving notes (needs stretch)
- a CREPE/FCPE tracker option
- export the study's notes and lyrics placeholder to the notepad

---

## 5. Risks and open questions for Ty

**Risks**

- **R1: running the model.** tract may not run basic-pitch's in-graph CQT. P0 settles it. The fallback is `ort` (adds Microsoft's runtime binary to all four release archives) or Classic-only for v1.
- **R2: weights licence.** `nmp.onnx` has no weights-specific licence. We rely on the repo's Apache-2.0. If Ty wants certainty, open an upstream issue before release.
- **R3: polyphonic quality.** Full mixes give messy notes. The confidence meter and the "rough guess" wording are the mitigation. Do not market it as stem-level separation.
- **R4: two capture streams.** One audio interface usually can't be opened for capture twice. The Record page must tap the mixer's existing input ring when the same device is already armed (`input_capture_channel` gets a second reader), otherwise it opens its own.
- **R5: C++ toolchains.** WORLD and signalsmith both add C++ builds on the Windows and macOS runners. Keep them behind features so the default build stays pure Rust.

**Open questions**

1. **Name and permanence.** Window title "Analyze Musically". Action ids `analyze-*` and `StudyId` are permanent once shipped. OK as named?
2. **Render default.** *Replace the clip's audio* (undoable, original kept in the study) or *new clip on a new lane below*?
3. **Shipping the model.** Is about 1 MB of basic-pitch weights in the binary acceptable (via `include_bytes!`, hash-pinned)? And should the WORLD "High quality" engine (C++ build) ship in v1 or wait?
4. **Standalone instances.** Is a "Studies ▾" list inside the window enough, or should studies appear in the browser or the rack?
5. **Copied notes' pitch.** Should pasted notes default to clean semitones, or keep cents and slides (`fine_pitch` / `path`)?

---

### Critical files for implementation

- `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-ui/src/app.rs` (MenuTarget, editors, clipboards, keymap dispatch)
- `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-ui/src/render/bridge.rs` and `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-ui/src/canvas/flopsynth.rs` (the look: hull, canopy, consoles, cards, `fit_cards`)
- `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-app/src/session.rs` (jobs ≈3960–4060, takes ≈1081–1225, `sampler_patch` ≈5851–5900, preview ≈10883–10935)
- `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-model/src/commands.rs` and `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-model/src/wire.rs` (Study commands, `SetAudioClip` swap, wire variants)
- `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-dsp/src/pitch.rs` and `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-dsp/src/psola.rs` (YIN refactor for pYIN, offline PSOLA)
- Also: `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-ui/src/canvas/piano_roll.rs` (clipboard lift, line ≈2015/3301), `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-ui/src/canvas/timeline.rs` (`ClipPart::Title`, line 1491), `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-types/src/scale.rs`, `/mnt/disks/6tb/Repos/DAW-Fontelle/crates/fontelle-types/src/effect.rs` (`TuneScale`/`TuneConfig`, ≈4520/4808), `/mnt/disks/6tb/Repos/DAW-Fontelle/deny.toml`

---

## 6. Ty's answers (2026-10-06)

1. **Name:** "Analyze Musically", as named; action ids `analyze-*` and `StudyId` are permanent once shipped.
2. **Render default:** *Replace the clip's audio* (undoable, the original kept in the study). *Render as a new clip below* is offered beside it.
3. **Shipping:** basic-pitch's weights (Spotify's, Apache-2.0) are embedded, hash-pinned. The standard engine is offline PSOLA; the WORLD "High quality" engine waits for a later release. Ty's general rule for what crosses from closed work into this public tree: generic capability that shows what Fontelle can do is welcome, but never enough pieces that someone could rebuild a closed product by stitching them together; the `Resynth` trait stays a plain seam.
4. **Standalone instances** (an Analyze Musically opened on a file or a recording rather than an arrangement clip): they are listed where the user already looks for things they made, in the browser under the project, as well as in the window's own menu, so one is never "lost" after its window closes.
5. **Copied notes**, chosen the way a working musician would want MIDI from audio (as Melodyne's and Ableton's audio-to-MIDI default): **clean semitones**, **timing as played** (not snapped; the roll's quantize is one key away), **velocity from loudness**. Pitch bends and cents are an opt-in "Keep slides and bends" switch on the Output card, off by default, because a bend baked into MIDI is rarely what one wants when replaying the part on another instrument.

### 6.1 Analyze Musically as a mixer insert (Ty, 2026-10-06)

> *"you should be able to add it to a mixer track as a plugin like you can
> with edison in fl to record into it like that and then send something into
> the playlist ... just want it to be flexible enough to allow for however
> the user is wanting to interact with it it just kind of works how they
> expect."*

So there are three ways in, and all three open the same window on a study:

| Way in | Audio comes from | Where it is found again |
|---|---|---|
| A clip's name menu (§3.2) | the clip's audio | the clip |
| The browser, or *Record into Analyze Musically…* | a file, or the input device | the browser, under the project (§6 answer 4) |
| **An insert on a mixer track (new)** | **whatever plays through that track** | **the track's effect slot**, like any effect |

**The insert.** A new built-in effect kind, "Analyze Musically", added from
the strip's effect list like the others. Audio passes through it untouched
(zero latency, no colouring). Its slot opens the window, whose Record page
gains a *Source* chooser: **This track** (the default when it is an insert)
or an input device. Record arms, as Edison's do:

- **On play:** records while the transport plays (punch to the loop range if
  one is set);
- **On input:** starts when the signal crosses a threshold, stops after a
  set silence (catch the next take without touching anything);
- **Now:** records from the press until the next.

Each take lands in the study's takes list, exactly like a device take (P5).
From there the usual tools apply (notes, clean, slice), and **Send to
arrangement** puts the result in the playlist as a new audio clip, at the
song position it was recorded from (or at the playhead for a free take).
Dragging the take or the selection out of the window onto a lane does the
same.

**Engineering.**

- The insert's process writes into a preallocated lock-free ring and nothing
  else (INVARIANT 1). A writer thread drains it to `<bundle>/recordings/`,
  the same path device takes use (`keep_audio_take`'s writer). Recording
  never blocks audio; a full ring drops and counts frames and says so.
- The effect's state (its study id, arm mode, threshold) is the insert's
  saved state like any effect's; the study itself lives in
  `Project::studies` (§3.9) with `StudySource::Insert { track, slot }`.
  Removing the effect leaves the study standalone (listed in the browser),
  never lost.
- Recording at the track's point in the graph (post-inserts before it,
  pre-fader) is what Edison does; a *Pre / Post fader* switch on the Record
  card covers the other wish.
- Collaboration: the effect travels like any effect; takes are local until
  kept, then ship as assets (§3.9).
- Phase: the insert's pass-through and capture ring join **P5**, with tests
  first: `analyze_insert.rs::{audio_passes_through_bit_identical,
  on_play_records_exactly_the_played_span, on_input_starts_at_threshold,
  a_full_ring_counts_dropped_frames_and_never_blocks,
  send_to_arrangement_lands_at_the_recorded_song_position}` and the engine's
  no-allocation guard over the insert's process.
