# DAW Fontelle — Technical Design Document

**Project codename:** DAW Fontelle
**Owner:** Fopull LLC
**License:** MIT OR Apache-2.0 (dual, at user's option)
**Primary platform:** Linux (x86_64, Wayland + X11). Windows and macOS supported, not prioritised.
**Language:** Rust (2024 edition, MSRV pinned at 1.88.0)
**Status:** Design. No code written yet.

---

## 0. How to read this document

This document is written to be handed to an implementing agent. It is prescriptive where a
decision has been made and explicitly marked **OPEN** where it has not. Sections marked
**INVARIANT** describe rules that must never be violated — these are the constraints that are
free to honour on day one and prohibitively expensive to retrofit later. If an invariant appears
to block a feature, stop and escalate rather than working around it.

Where this document specifies a crate, that crate has been chosen for a reason including its
licence. Do not substitute dependencies without re-checking licence compatibility against
§3.4.

---

## 1. Product definition

### 1.1 What Fontelle is

Fontelle is a lightweight, Linux-first digital audio workstation built around SoundFont-based
sample playback. Its thesis is that SF2 files are an enormously expressive and under-exploited
sound source, and that every existing SoundFont player treats the file's internal parameters as
authoritative rather than as a starting point. Fontelle inverts that: the SF2 file supplies
*defaults*, and every one of those defaults is overridable — loop points, start offset,
envelopes, filtering, mapping, tuning, everything.

The target user is a producer making sample-forward, character-driven music (the reference point
is Toby Fox-style composition) who wants to open a program and start writing immediately, without
fighting a plugin host, a licence manager, or a 4GB install.

### 1.2 Design principles

1. **Flexibility over guardrails.** Where a reasonable user might want to do something unusual,
   allow it. Creativity happens in the space a tool leaves open.
2. **No imposed workflow.** The DAW is not pattern-based, not linear-only, not track-locked. The
   user decides how their project is organised.
3. **Nothing silently on your behalf.** Where the app must make a choice for the user
   (copy vs. reference on import, sample-rate conversion, overwrite vs. version), it asks once,
   remembers the answer, and always provides a per-action escape hatch (§17.3).
4. **Lightweight by construction.** Fast cold start, near-zero idle CPU, small install, no
   background services, no telemetry, no account.
5. **Your disk is yours.** Nothing is written outside explicitly configured locations.
6. **Familiar where familiarity helps.** Piano roll interaction and keybinds track FL Studio
   closely, because that is muscle memory the target user already has.

### 1.3 Non-goals for v1

- Hosting third-party plugins (CLAP/VST3/LV2). The boundary is scaffolded (§8.4); no host is
  implemented.
- Video, notation, surround/ambisonics, network collaboration, cloud anything.
- Windows/macOS parity testing. Both must build and run; neither blocks a release.
- Comprehensive audio-clip editing. Audio is supported and useful (§15) but the sampler is the
  product.

### 1.4 What "done" means for v1

A user can install a single package, open it, point it at a folder of SF2 files, and write,
arrange, mix, and export a complete multi-instrument track without touching another application.

---

## 2. Architectural overview

### 2.1 The three-layer split

```
┌──────────────────────────────────────────────────────────────┐
│  UI LAYER            (main thread, 60fps, GPU)               │
│  fontelle-ui, fontelle-app                                   │
│  Renders document state. Emits Commands. Never mutates.      │
└────────────────────────┬─────────────────────────────────────┘
                         │ Commands (§10.6)
┌────────────────────────▼─────────────────────────────────────┐
│  MODEL LAYER         (main thread, allocation permitted)     │
│  fontelle-model                                              │
│  Owns the document. Applies Commands. Resolves prefabs.      │
│  Compiles the flat event timeline.                           │
└────────────────────────┬─────────────────────────────────────┘
                         │ atomic buffer swap (§11.3)
┌────────────────────────▼─────────────────────────────────────┐
│  AUDIO LAYER         (RT thread, NO allocation ever)         │
│  fontelle-engine, fontelle-core, fontelle-dsp                │
│  Reads flat event timeline. Renders audio. Owns nothing      │
│  that requires the heap.                                     │
└──────────────────────────────────────────────────────────────┘
```

**INVARIANT 1 — The RT thread never allocates, locks, blocks, or syscalls.**
No `Box`, `Vec::push`, `String`, `HashMap`, `Mutex`, `RwLock`, `println!`, file I/O, or channel
operation that can block. Enforced by a debug-build global allocator that panics if called from
a thread tagged as RT (§20.4). This is not a style preference; a single allocation under load is
an audible dropout.

**INVARIANT 2 — The UI never mutates the document directly.**
Every change is a `Command` (§10.6). This is what makes undo, prefab override tracking, and
project dirty-state work uniformly rather than as three parallel half-correct systems.

**INVARIANT 3 — The RT thread never sees the prefab graph.**
It sees only a flat, fully-resolved event timeline. Playback cost is therefore completely
independent of how deeply prefabs are nested (§11).

### 2.2 Thread inventory

| Thread | Priority | Allocates | Responsibility |
|---|---|---|---|
| Main / UI | Normal | Yes | Rendering, input, document model, command application |
| Audio (RT) | Realtime (RT-kit) | **No** | Audio callback, graph processing, voice rendering |
| Disk streaming | High | Yes | Sample prefetch into RT-visible ring buffers |
| Asset loader | Low | Yes | SF2 parsing, waveform peak generation, thumbnailing |
| Export/render | Low | Yes | Offline bounce (runs the graph faster than realtime) |

Communication is exclusively via lock-free structures: `rtrb` (SPSC ring buffers) for event and
command traffic, `triple_buffer` for whole-state handoff, and atomics for scalar parameters.

### 2.3 Why Rust and not JUCE + Tracktion Engine

Tracktion Engine is roughly 115,000 lines of mature, battle-tested DAW engine, free to use, and
would provide plugin hosting and timeline machinery immediately. It was seriously considered and
rejected for three reasons:

1. **Licensing.** Tracktion Engine is GPL-or-commercial, and JUCE requires a separate licence.
   Neither is compatible with shipping MIT/Apache-2.0 under Fopull LLC.
2. **The product is the sampler.** Tracktion's value is in the parts of a DAW that are not our
   differentiator. We would inherit a large C++ dependency to save work on the least novel 40% of
   the project.
3. **Plugin hosting is explicitly out of scope for v1**, which is the single largest thing
   Tracktion would have given us.

The cost of this decision is real and should be understood: we are writing a timeline, a mixer,
an automation system, and a full effects suite from scratch. §22 stages that work.

---

## 3. Technology stack

### 3.1 Core dependencies

| Concern | Crate | Licence | Notes |
|---|---|---|---|
| Audio I/O | `cpal` ≥0.18 | Apache-2.0 | Native PipeWire backend as of 0.18; auto-selects PipeWire > PulseAudio > ALSA on Linux. ASIO on Windows, CoreAudio on macOS. |
| SF2 parsing | `soundfont` | MIT | Pure-Rust sf2 reader. Parsing only — synthesis is ours. |
| Plugin export | `nice-plug` | ISC | Community successor to NIH-plug under the RustAudio org. Exports CLAP + VST3 from one macro. |
| Windowing (app) | `winit` | Apache-2.0 | |
| Windowing (plugin) | `baseview` | MIT/Apache-2.0 | Child-window embedding for plugin editors. |
| GPU | `wgpu` | MIT/Apache-2.0 | |
| Vector rendering | `vello` | MIT/Apache-2.0 | GPU-accelerated 2D. See §16.2 for the fallback plan. |
| Text | `cosmic-text` | MIT | Shaping, layout, font fallback. |
| Resampling | `rubato` | MIT | Offline/clip resampling. Voice-level interpolation is ours (§7.6). |
| FFT | `realfft` / `rustfft` | MIT/Apache-2.0 | EQ analyser, spectral tools. |
| Audio decode | `symphonia` | MPL-2.0 | wav/flac/mp3/ogg. MPL is file-level copyleft; linking is fine for MIT/Apache distribution. |
| MIDI I/O | `midir` | MIT | §14. |
| MIDI file parse | `midly` | MIT | Import/export of .mid. |
| Serialisation | `serde` + `serde_json` | MIT/Apache-2.0 | Document format (§17.2). |
| Content hashing | `twox-hash` | MIT | xxhash for `AssetRef::content_hash` (§17.4). Added 2026-08-28: §17.4 specifies xxhash and this table had no hashing crate. `std`'s `DefaultHasher` is documented as unstable across Rust releases, so it cannot back a value written to disk. |
| Lock-free queues | `rtrb` | MIT/Apache-2.0 | |
| RT priority | `audio_thread_priority` | MPL-2.0 | rtkit/D-Bus promotion on Linux. |
| IDs | `uuid` (v7) | MIT/Apache-2.0 | Time-ordered, sortable. |
| Arenas | `slotmap` | Zlib | Stable handles with generation counters (§10.2). |

### 3.2 Deliberately rejected

- **OxiSynth** — LGPL-2.1. Incompatible with our licence, and we are not using a black-box
  synth anyway.
- **RustySynth** — MIT and excellent, but it is a *conformant* SF2 player, which is exactly the
  constraint we are escaping. **Use it as a correctness reference** when implementing SF2
  generator semantics (§7.3); do not depend on it.
- **`plugin_host` (crates.io)** — advertises VST3/CLAP bridges that are, per its own
  documentation, method stubs awaiting implementation. Do not use.
- **`rack`** — production-ready for AudioUnit on macOS only. VST3/CLAP merely planned.
- **egui** — excellent for tools, but immediate-mode redraw of a 10,000-note piano roll is the
  wrong cost model, and text/layout polish falls short of DAW expectations.
- **Slint** — the royalty-free licence terms are a poor fit for a permissively-licensed
  company-backed project.
- **Tracktion Engine / JUCE** — see §2.3.

### 3.3 Deferred with a decision attached

- **`signalsmith-stretch`** (MIT) is the best available time-stretch/pitch-shift, but it is a C++
  library bound via `cxx`, which pulls a C++ toolchain into the build and complicates
  contribution and packaging. **Decision: v1 ships varispeed and high-quality resampling only
  (pure Rust). Formant-preserving time-stretch is a v2 feature and the C++ dependency is
  accepted at that point, behind a Cargo feature flag so a pure-Rust build remains possible.**

### 3.4 Licence policy

The project is MIT OR Apache-2.0. Any new dependency must be MIT, Apache-2.0, BSD, ISC, Zlib, or
MPL-2.0. **GPL and LGPL dependencies are prohibited.** CI runs `cargo-deny` with an explicit
allowlist; a disallowed licence fails the build.

Two trademark notes for Fopull LLC:

- "SoundFont" is a Creative/E-mu trademark. It must not appear in the product name, the plugin
  name, or the binary name. Descriptive use in documentation ("loads SoundFont-format files") is
  fine. Prefer "SF2" in UI strings.
- CLAP is MIT with no additional agreement. The VST3 SDK's licensing has historically required a
  signed Steinberg agreement to develop or host VST3, and recent tooling suggests this may have
  changed. **ACTION REQUIRED: verify VST3 SDK licensing terms before shipping a VST3 build.**
  CLAP is the canonical export format regardless; VST3 is a convenience build and is allowed to
  be blocked on this question.

---

## 4. Repository layout

A single Cargo workspace.

```
fontelle/
├── Cargo.toml                  # workspace
├── crates/
│   ├── fontelle-core/          # sampler DSP. RT-safe. No GUI, no I/O, no DAW knowledge.
│   ├── fontelle-dsp/           # shared DSP primitives: filters, envelopes, oscillators,
│   │                           #   interpolators, meters. RT-safe. No allocation.
│   ├── fontelle-fx/            # built-in effects. Depends on fontelle-dsp.
│   ├── fontelle-engine/        # audio graph, transport, voice management, mixer topology.
│   ├── fontelle-model/         # document model, prefabs, commands, undo, serialisation.
│   ├── fontelle-sequencer/     # timeline → flat event stream compiler.
│   ├── fontelle-midi/          # MIDI device abstraction, routing, mapping.
│   ├── fontelle-assets/        # SF2/sample loading, streaming, peak generation, library index.
│   ├── fontelle-ui/            # widget layer. Runs on winit AND baseview.
│   ├── fontelle-app/           # the DAW binary.
│   └── fontelle-plugin/        # nice-plug wrapper. Exports CLAP + VST3.
├── assets/                     # icons, default theme, factory presets
├── benches/                    # criterion benchmarks (§20.5)
└── xtask/                      # build/bundle/package automation
```

### 4.1 The dependency rule

**INVARIANT 4 — `fontelle-core` must not depend on anything above it.**
It knows nothing about the DAW, the document model, files, or the GUI. Its public API is:
construct from a patch description, receive events, render into a buffer, report parameters.
Nothing else.

This is what makes §8 possible. It is also the constraint most likely to be casually violated by
an implementer reaching for "just this one thing from the model layer." Do not.

Permitted dependency direction:

```
core ──> dsp
fx ──> dsp
engine ──> core, fx, dsp
sequencer ──> model
model ──> (nothing in this workspace except shared id types)
app ──> everything
plugin ──> core, dsp, ui
```

---

## 5. Audio engine

### 5.1 Graph model

The audio graph is a directed acyclic graph of nodes. It is **compiled on the model thread** into
a flat, topologically-sorted schedule and handed to the RT thread by atomic pointer swap. The RT
thread walks a `&[ScheduledNode]` — it never traverses a graph, never resolves connections, never
allocates a buffer.

```rust
pub trait AudioNode: Send {
    fn prepare(&mut self, ctx: &PrepareContext);   // called off-RT; allocation permitted
    fn process(&mut self, ctx: &mut ProcessContext); // RT. NO allocation.
    fn reset(&mut self);                             // silence tails, clear state
    fn latency_samples(&self) -> u32 { 0 }
    fn params(&self) -> &dyn ParamSet;
}
```

`ProcessContext` carries: input buffer slices, output buffer slices, the sample-accurate event
slice for this block, transport state, and the block's sample range. Nothing in it requires the
heap.

**Node types in v1:** `SamplerNode` (wraps `fontelle-core`), `EffectNode` (wraps a
`fontelle-fx` effect), `MixerTrackNode`, `SendNode`, `AudioClipNode`, `MasterNode`.

### 5.2 Buffer management

A pre-allocated buffer pool sized at `prepare()` time from the compiled schedule's peak
concurrent-buffer requirement. Buffers are `f32`, deinterleaved (planar), 32-byte aligned for
SIMD. The scheduler assigns buffer indices at compile time using linear-scan register allocation
— the RT thread just indexes into the pool.

Internal processing is **always 32-bit float, always deinterleaved.** Interleaving and format
conversion happen exactly once, at the device boundary.

### 5.3 Block size and sub-block splitting

The device gives us a block (typically 128–1024 frames). Events within that block have
sample-accurate timestamps. Nodes that can handle sample-accurate events internally (the sampler)
do so. Nodes that cannot (most effects) are driven by **sub-block splitting**: the scheduler
splits the block at event boundaries, capped at a minimum sub-block of 16 frames to bound
overhead. Parameter changes within a sub-block are handled by per-parameter smoothers rather
than stepping.

### 5.4 Multicore

Independent branches of the graph process in parallel via a fixed-size worker pool (N = physical
cores − 1) with work-stealing over the compiled schedule's dependency levels. Workers are RT
threads and are bound by INVARIANT 1. The pool is created once at engine start; no thread is ever
spawned during processing.

Parallelism is opt-out in settings — some systems behave better single-threaded, and debugging is
far easier with it off.

### 5.5 Latency compensation

Each node reports `latency_samples()`. The graph compiler computes per-path latency and inserts
fixed delay lines to align branches. Reported total latency is surfaced in the audio settings UI
so the user can see what their configuration actually costs.

---

## 6. Transport and timing

### 6.1 Time representation

**INVARIANT 5 — Musical time is stored in ticks; audio time is stored in samples. They are never
conflated, and floating-point beats are never stored on disk.**

- **Tick** = `i64`, at **960 PPQN**. All note positions, clip bounds, automation points, and loop
  markers are ticks. 960 divides evenly by 2, 3, 4, 5, 6, 8, and 16, which covers every practical
  tuplet without rounding drift.
- **Sample** = `i64` from song start. Audio clip positions and recording use samples.
- Conversion goes through the `TempoMap`, never by ad-hoc arithmetic.

### 6.2 Tempo map

Tempo is not a scalar. The `TempoMap` is a piecewise function of tick → BPM supporting constant
segments and interpolated ramps, plus a time-signature track. It exposes:

```rust
fn tick_to_sample(&self, tick: i64) -> i64;
fn sample_to_tick(&self, sample: i64) -> i64;
fn tempo_at(&self, tick: i64) -> f64;
```

Both directions are implemented by integrating over segments with a cached prefix-sum table
rebuilt on edit, so lookups are O(log n) rather than O(n).

Tempo is automatable (§12) and therefore the tempo map is itself a compiled artefact of the
automation system, not an independent data structure.

### 6.3 Transport

States: `Stopped`, `Playing`, `Recording`, `Rendering`. Loop points are ticks. Transport state
lives in an atomic struct read by the RT thread and written by the model thread.

**Idle behaviour:** when `Stopped`, the graph is not processed. The audio callback fills silence
and returns immediately. UI animation stops. This is what delivers the near-zero idle CPU target
(§19) and it must be designed in, not optimised in later.

---

## 7. Fontelle sampler engine (`fontelle-core`)

This is the product. Everything else in this document is infrastructure to make this usable.

### 7.1 Conceptual model

An SF2 file is a container of **samples** plus **zone metadata** describing how those samples map
to keys, velocities, envelopes, and filters. Conventional players treat that metadata as the
instrument definition. Fontelle treats it as **a set of defaults populating an instrument
definition that the user then owns.**

```
SF2 file ──parse──> ImportedZones ──seed──> Patch ──user edits──> Patch'
                                              │
                                              └──> Voice architecture (§7.4)
```

Once a `Patch` is created, it has no live link to the SF2's metadata — only to its sample data.
Changing the SF2 on disk does not silently change the user's instrument. Re-importing is an
explicit action.

### 7.2 Patch structure

```rust
pub struct Patch {
    pub layers: Vec<Layer>,          // up to 16; stacked or key/vel-split
    pub filters: [FilterSlot; 2],
    pub envelopes: Vec<Envelope>,    // >= 2 (amp, mod); user may add
    pub lfos: Vec<Lfo>,              // >= 2; user may add
    pub mod_matrix: ModMatrix,       // §7.5
    pub voice_config: VoiceConfig,   // polyphony, stealing, glide, unison
    pub params: ParamSet,            // flat, stable string IDs (§8.2)
}

pub struct Layer {
    pub source: Source,
    pub key_range: (u8, u8),
    pub vel_range: (u8, u8),
    pub root_key: u8,
    pub fine_tune_cents: f32,
    pub playback: PlaybackConfig,    // §7.3
    pub gain_db: f32,
    pub pan: f32,
}

pub enum Source {
    Sf2Zone { file: AssetId, zone: ZoneId },
    Sample  { file: AssetId },
    Oscillator(OscKind),   // sine, saw, square, triangle, noise
}
```

**Correction, 2026-08-28: `file` is an `AssetId`, not an `AssetRef`.** This section originally
said `AssetRef`, which carries a `PathBuf`. A `Layer` is read by the voice on the RT thread, and
an owned path inside it is a heap allocation in the type a patch edit clones — the wrong place
for a filename. `AssetId` is the key of the decoded sample in the `SampleStore`, which is what
rendering actually needs. The `AssetRef` lives in the *stored* form of the patch instead (§8.3),
which is where a filename belongs and where §17.4's relinking can reach it.

Note that `Source::Oscillator` exists from day one. The decision was that SF2 samples are *one*
source among several, and a bare oscillator layer is cheap to implement and immediately useful
for reinforcing a weak sample's sub-bass or adding noise to a percussive attack.

### 7.3 Playback configuration — the override layer

This is where the FL Studio limitation is actually solved. Every field here is seeded from the
SF2 zone and every field is user-editable.

```rust
pub struct PlaybackConfig {
    pub start_offset: f64,        // fractional samples
    pub end_offset: f64,
    pub loop_mode: LoopMode,      // Off | Forward | PingPong | Sustain | Release
    pub loop_start: f64,
    pub loop_end: f64,
    pub loop_crossfade_ms: f32,   // NOT in the SF2 spec. Ours. Critical — see §7.8.
    pub reverse: bool,
    pub interpolation: Interpolation,
}
```

`loop_crossfade_ms` is not an SF2 concept and is one of the highest-value additions in the
project: a very large fraction of "soundfonts sound clicky" is abrupt loop boundaries, and an
equal-power crossfade over the loop point fixes it non-destructively.

**Implementation note on SF2 semantics:** the SF2 2.04 generator model (offsets, coarse/fine
tuning, key/vel ranges, envelope units in timecents, filter cutoff in absolute cents, modulator
defaults) has many non-obvious rules. Read RustySynth's implementation as the reference for
correct default behaviour before writing the importer. Getting import defaults wrong makes every
soundfont sound subtly incorrect and is very hard to debug later.

Support `.sf2`, `.sf3` (Vorbis-compressed sample data), and `.sfz` (text-based, references
external samples). SFZ import maps onto the same `Patch` structure.

### 7.4 Voice architecture

**INVARIANT 6 — The voice is a fixed-topology graph, not a free patch graph.**

Fixed topology means: predictable per-voice CPU cost, zero allocation on note-on, no cycle
detection, no graph compilation in the audio thread. The mod matrix (§7.5) supplies the
flexibility that a free-form patch graph would otherwise provide, at a fraction of the
complexity. If free-form routing is ever genuinely needed, it is a v3 conversation.

```
        ┌──────────────────────────────────────────────┐
Note ──>│ Layers (1..16) ──> mix ──> Filter 1 ──> Filter 2 ──> Amp ──> Pan ──> out
        └──────────────────────────────────────────────┘
              ▲              ▲            ▲          ▲
              └──────────────┴────────────┴──────────┘
                         Mod Matrix (§7.5)
                    sources: Env[], LFO[], velocity, key,
                    aftertouch, mod wheel, pitch bend,
                    random, note-on counter
```

`VoiceConfig` controls polyphony (1–256), stealing policy (oldest / quietest / lowest-priority),
glide/portamento (time, legato-only toggle), unison (voices, detune, spread, phase randomisation),
and mono/legato retrigger behaviour.

Voices are drawn from a pre-allocated pool sized to max polyphony at `prepare()` time. Note-on
with no free voice triggers stealing with a short release ramp to avoid clicks — never a hard cut.

### 7.5 Mod matrix

```rust
pub struct ModRoute {
    pub source: ModSource,
    pub destination: ModDest,   // any continuous parameter, addressed by stable ID
    pub depth: f32,             // bipolar, -1..1
    pub curve: Curve,           // linear | exp | log | s-curve | quantised
    pub via: Option<ModSource>, // secondary modulator scaling this route's depth
}
```

`via` is worth the small extra cost — "LFO depth controlled by mod wheel" is one of the most
common things people want and is annoying to express without it.

Destinations must include, at minimum: layer pitch, layer gain, layer pan, sample start offset,
loop start, loop length, filter cutoff, filter resonance, all envelope stage times and levels,
all LFO rates and depths, and unison detune. Sample start offset as a mod destination (modulated
by velocity or randomness) is a strong character tool and costs nothing to expose.

### 7.6 Interpolation and transposition quality

**This determines whether the sampler sounds good.** Pitching a sample far from its root is both
the main quality risk (aliasing) and the main per-voice CPU cost.

Provide selectable quality:

| Mode | Algorithm | Use |
|---|---|---|
| Draft | Linear | Preview while dragging, very high polyphony |
| Normal | 4-point Hermite | Default for playback |
| High | 8-point windowed sinc | Default for render/export |
| Ultra | 16-point windowed sinc + 2× oversample | Extreme upward transposition |

The playback and render quality settings are independent, so the user can work at Normal and
bounce at High without thinking about it. Downward transposition needs no anti-aliasing; upward
transposition beyond roughly +7 semitones benefits substantially from sinc.

### 7.7 Sample memory and streaming

Sample data is reference-counted and **shared across all patches referencing the same file** — a
2GB orchestral SF2 loaded on twelve channels occupies memory once.

- Files under a configurable threshold (default 64 MB): fully resident, memory-mapped where
  possible.
- Files above it: first N milliseconds (default 500ms) resident, remainder streamed by the disk
  thread into per-voice ring buffers.

Streaming must degrade gracefully: an underrun holds the last sample and logs, it never produces
a discontinuity click.

### 7.8 The harshness problem — voice-side mitigations

Stock soundfonts frequently sound clicky and spitty. Roughly half of this is fixable in the voice
and should be, because fixing it downstream with EQ costs the user a plugin slot and their
attention:

1. **Loop crossfade** (§7.3) — eliminates loop-boundary discontinuities.
2. **Attack micro-ramp** — a 1–3ms configurable ramp on note-on, defaulting on, eliminates
   start-of-sample transients from non-zero-crossing sample starts.
3. **Release micro-ramp** — same at note-off and on voice stealing.
4. **DC offset removal** — a one-pole high-pass at ~5Hz applied at import detection, toggleable.
5. **Anti-aliasing interpolation** (§7.6).

The complementary *effect*-side tool is specified in §13.4.

---

## 8. Plugin export (`fontelle-plugin`)

### 8.1 Why this is a day-one requirement

The sampler ships as a standalone CLAP and VST3 plugin from the first release. This is both a
distribution strategy — the engine is useful to people who will never switch DAWs — and a design
forcing function: an engine that can be embedded in a foreign host is an engine with a clean
boundary.

Retrofitting this later would mean untangling the sampler from the DAW's model, parameter, and
GUI layers after they have grown together. It is therefore built in from commit one.

### 8.2 The parameter contract

**INVARIANT 7 — Every automatable parameter has a stable string ID that never changes across
versions.**

```
channel:<uuid>/patch/layer[2]/filter1.cutoff
mixer:<uuid>/insert[0]/param/threshold
transport/tempo
```

This single addressing scheme serves five systems: plugin parameter export, DAW automation
targets (§12), preset serialisation, MIDI learn (§14.4), and undo command targets. If an
implementer creates a second, parallel addressing scheme for any of these, that is a design
regression — escalate it.

Parameters carry: stable ID, display name, range, default, unit, value distribution (linear /
skewed / stepped), and a smoother. This is `nice-plug`'s `Params` shape, adopted directly.

### 8.3 Preset compatibility

**A patch built inside the DAW loads in the standalone plugin, and vice versa, byte-identically.**
The serialisation format lives in `fontelle-core` and is versioned with an explicit migration
path from v0. Presets embed asset references (§17.4), not sample data, with an optional
"embed samples" export mode for sharing.

**How a stored patch names its audio (added 2026-08-28, implementing this section).** An
`AssetRef` names a *file*, and one soundfont holds hundreds of samples — so a layer's reference
is an `AssetRef` plus the index of the sample header inside that file (`fontelle_types::
SampleRef`). The in-memory `AssetId` is never written: it is a `slotmap` key minted by whichever
`SampleStore` happened to decode the file, so INVARIANT 8 forbids it and it would in any case
mean nothing on the machine that opens the preset. Reading a patch therefore takes a resolver
from `SampleRef` to a live `AssetId`; a reference that resolves to nothing yields a silent layer
and is reported, per §17.4's requirement that a project with a broken link still opens.

The stored form is `PatchData { format_version, body }` with an untyped `body`, because a
migration has to read shapes this build's structs no longer describe. The version is read before
the body, so a file from a newer build is refused as "upgrade Fontelle" rather than as damage.

### 8.4 Third-party hosting scaffold (not implemented in v1)

The `AudioNode` trait (§5.1) and the parameter contract (§8.2) are the entire boundary a future
CLAP host would plug into. No hosting code is written in v1. When it is written, the intended
path is `clack-host` for CLAP first, `livi` for LV2 second, and an out-of-process bridge for
VST3 if it is ever justified.

Do not add speculative hosting abstractions now. The trait boundary is sufficient.

---

## 9. GUI, plugin editors, and windowing

Covered in §16. Referenced here only to state the constraint that affects `fontelle-core`: the
sampler's editor UI must render both as a docked panel inside the DAW (winit) and inside a
host-provided parent window (baseview). This is the primary reason for the GUI architecture
chosen in §16.2.

---

## 10. Document model (`fontelle-model`)

### 10.1 Top-level structure

```rust
pub struct Project {
    pub meta: ProjectMeta,             // name, created, app version, format version
    pub tempo_map: TempoMap,
    pub channels: SlotMap<ChannelId, Channel>,   // instrument channels
    pub mixer: Mixer,                            // §13
    pub lanes: Vec<Lane>,                        // visual only — see 10.3
    pub clips: SlotMap<ClipId, Clip>,            // all clips, flat
    pub prefabs: SlotMap<PrefabId, Prefab>,      // §10.5
    pub assets: AssetTable,                      // §17.4
    pub markers: Vec<Marker>,
    pub view_state: ViewState,                   // zoom, scroll, panel layout, selection
}
```

### 10.2 Identity

**INVARIANT 8 — Every addressable element carries a persistent, stable ID. Indices are never
used as identity, and never serialised as references.**

This includes individual **notes** and individual **automation points**. This looks like overkill
until §10.5: the prefab override system needs to say "in this instance, *that specific note* is
transposed," and that statement must survive the source being edited, notes being inserted before
it, and a save/load round trip. Indices cannot do this.

- In-memory: `slotmap` keys (index + generation), cheap and cache-friendly.
- On disk: UUIDv7, time-ordered so diffs are readable and merges are tractable.

### 10.3 Lanes are visual only

The user's decision was FL-style: **the clip carries its instrument.** Therefore:

```rust
pub struct Lane {
    pub id: LaneId,
    pub name: String,
    pub height: f32,
    pub color: Color,
    pub muted: bool,       // sequencer-level mute, NOT a mixer operation
    pub locked: bool,
}
```

A lane has no audio identity, no routing, no mixer relationship, and no instrument. It is a
horizontal organisational strip. Consequences the implementer must handle:

- **Lanes are cheap**, so users will create hundreds. The timeline renderer must be virtualised
  from the start (§16.4) — build geometry only for visible lanes within the visible time window.
- **"Bounce this lane" is meaningless**, because a lane may hold clips from a dozen channels.
  Bounce is a *selection*-based or *channel*-based operation. Do not add a lane-bounce command.
- **Lane mute is a sequencer mute** — it suppresses event emission during compilation (§11), it
  does not mute a mixer track.

### 10.4 Clips

```rust
pub struct Clip {
    pub id: ClipId,
    pub lane: LaneId,
    pub start: Tick,
    pub length: Tick,
    pub source: ClipSource,
    pub prefab_link: Option<PrefabLink>,   // §10.5
    pub color: Option<Color>,              // overrides source colour
    pub muted: bool,
}

pub enum ClipSource {
    Notes(NoteData),                       // owns its notes, or inherits via prefab_link
    Automation(AutomationData),            // §12
    Audio(AudioClipData),                  // §15
}

pub struct NoteData {
    pub channel: ChannelId,                // THE CLIP CARRIES THE INSTRUMENT
    pub notes: SlotMap<NoteId, Note>,
}

pub struct Note {
    pub start: Tick,          // relative to clip start
    pub length: Tick,
    pub key: u8,
    pub velocity: u8,
    pub pan: i8,
    pub fine_pitch: i16,      // cents
    pub release: u8,
    pub mod_x: u8,            // free per-note modulation, routable in the mod matrix
    pub mod_y: u8,
}
```

Per-note pan, fine pitch, release, and two free modulation values are included because they are
cheap to store, cheap to route through the mod matrix (§7.5), and are exactly the kind of
per-note character control that makes sample-based writing expressive.

### 10.5 Prefabs

The model is Unity's: a prefab is a reusable source; instances reference it and may override it;
prefab variants are prefabs whose base is another prefab.

```rust
pub struct Prefab {
    pub id: PrefabId,
    pub name: String,
    pub base: Option<PrefabId>,        // Some(_) makes this a VARIANT
    pub source: ClipSource,            // the canonical content
    pub overrides: OverrideMap,        // variant's deltas against base
}

pub struct PrefabLink {
    pub prefab: PrefabId,
    pub overrides: OverrideMap,        // this instance's deltas
}

pub struct OverrideMap {
    props: HashMap<(ElementId, PropKey), PropValue>,   // property overrides
    added: Vec<Element>,                                // structural: added elements
    removed: HashSet<ElementId>,                        // structural: removed elements
}
```

**Resolution order:** `base prefab → variant chain (outermost last) → instance overrides`.
Resolution is an explicit pass producing a fully-materialised `ClipSource`. It is memoised and
invalidated when any ancestor changes.

**Cycle prevention:** the variant `base` chain and any future prefab-nesting must be validated
acyclic on every mutation. Reject the command; do not panic.

#### 10.5.1 Staging — read this carefully

The full system is the target. The v1 *shipped feature set* is deliberately narrower, but the
*data structures above are implemented in full from commit one.*

| Capability | v1 | Later |
|---|---|---|
| Persistent element IDs | **Required** | — |
| `OverrideMap` structure and serialisation | **Required** | — |
| Resolution pass | **Required** | — |
| Mirror instances (edit source → all update) | **Ship** | — |
| Property overrides (transpose, velocity, length) | **Ship** | — |
| Structural overrides (add/remove inside instance) | Structure only | Ship |
| Prefab variants (`base: Some(_)`) | Structure only | Ship |
| Per-property apply/revert UI | — | Ship |

The reason for this split: variants and structural overrides are where Unity's prefab system
generates its hardest bugs, and shipping them in v1 is a schedule risk. But omitting the *ID and
override infrastructure* would mean migrating every project file in existence when they land.
Build the bones now, ship the feature later.

### 10.6 Commands and undo

**INVARIANT 9 — Every document mutation goes through a `Command`. There are no direct writes to
`Project` from anywhere, including the UI, including "just this one small thing."**

```rust
pub trait Command: Send {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError>;
    fn invert(&self) -> Box<dyn Command>;
    fn label(&self) -> &str;              // shown in the history list
    fn merge_with(&mut self, next: &dyn Command) -> bool;  // coalescing
    fn memory_cost(&self) -> usize;
}
```

Command-pattern with inverse operations, not snapshots. This is chosen because it is cheap in
memory with large note data, and because it gives the history list **meaningful entry names for
free** — "Draw 14 notes", "Delete clip", "Convert to prefab" — rather than "Snapshot 7".

- `Ctrl+Z` / `Ctrl+Y`.
- History depth **default 100**, configurable, with a total memory ceiling (default 256 MB) that
  evicts oldest entries first.
- The Edit menu shows Undo/Redo plus a **History** submenu revealing the full stack on hover;
  clicking any entry jumps to that point by applying inverses in sequence.
- `merge_with` coalesces continuous gestures — dragging a note produces one history entry, not
  four hundred.

**Exception for large binary operations.** Audio recording and destructive audio edits cannot be
cheaply inverted by storing data. These store *file references* to the pre-edit state in the
project's `backups/` directory, and their inverse restores the reference.

---

## 11. Sequencer — compiling the timeline (`fontelle-sequencer`)

### 11.1 What it does

The sequencer turns the entire document — clips, prefab instances with overrides, lane mutes,
automation, the tempo map — into a **flat, immutable, sample-timestamped event list** that the RT
thread reads linearly.

```rust
pub struct CompiledTimeline {
    events: Vec<TimedEvent>,       // sorted by sample position
    index: Vec<(i64, usize)>,      // sparse seek index, one entry per bar
}

pub struct TimedEvent {
    pub sample: i64,
    pub target: NodeId,
    pub payload: EventPayload,     // NoteOn/NoteOff/ParamValue/ClipStart/...
}
```

### 11.2 Why this exists

Recall INVARIANT 3: the RT thread never sees the prefab graph. This is the mechanism. Prefab
resolution, override merging, variant chains, tempo conversion, and mute evaluation all happen
here, on the model thread, ahead of time. Playback cost is therefore **completely independent of
how baroque the prefab nesting is**. This is what allows the design to be generous about prefab
complexity without paying for it in dropouts.

### 11.3 Incremental recompilation and handoff

Full recompilation of a large project is too slow to do on every keystroke. Compilation is
**segmented by bar**; a mutation dirties only the bars it touches (plus any bar containing a
prefab instance whose source changed). Dirty segments recompile on a background task.

Handoff to the RT thread is by `triple_buffer`: the model thread writes a new `CompiledTimeline`,
publishes it atomically, and the RT thread picks it up at the next block boundary. The old
timeline is dropped on the model thread — **never on the RT thread**, since dropping deallocates.

Edits made during playback take effect at the next block boundary, which at 128 frames/48kHz is
under 3ms. Perceptually instant.

### 11.4 Note collision policy

Two clips on different lanes referencing the same channel, overlapping, on the same key: one
instrument instance receives two note-ons and then a note-off that would kill both.

**Decision: per-clip voice contexts.** Each clip gets a distinct voice-context tag; note-offs
only match note-ons with the same tag. Costs one small integer per voice and eliminates an entire
class of "my notes are cutting each other off" confusion.

The cost is voice count — two overlapping clips playing the same note consume two voices. That is
correct behaviour and matches what the user visually expects.

---

## 12. Automation

### 12.1 Representation

Automation is a **clip type**, placed on the timeline like any other clip, carrying its own
target. This is consistent with §10.3's clip-carries-its-instrument model and it means automation
clips can be **prefabs** — build a filter sweep once, instance it across the track, edit the
source, every instance updates.

```rust
pub struct AutomationData {
    pub target: ParamAddress,       // the stable string ID from §8.2
    pub points: SlotMap<PointId, AutomationPoint>,
}

pub struct AutomationPoint {
    pub tick: Tick,                 // relative to clip start
    pub value: f64,                 // normalised 0..1
    pub curve: CurveShape,          // shape of the segment FOLLOWING this point
    pub tension: f32,               // -1..1
}

pub enum CurveShape { Linear, Exponential, Logarithmic, SCurve, Stepped, Hold }
```

Points are kept sorted; evaluation is binary search plus segment interpolation.

### 12.2 The two rules FL leaves implicit

Both of these must be explicit, documented, and visible in the UI:

1. **Overlapping clips on the same target: last one wins.** The later-starting clip takes
   precedence for the duration of the overlap. The overlap region is drawn with a warning stripe
   so the user can see it. Additive blending was considered and rejected as confusing.
2. **Outside a clip's bounds, the parameter holds its last automated value.** It does not snap
   back to the knob's static position. This matches FL and is what people expect. The knob in the
   UI displays the automated value with a distinct ring colour while under automation control.

### 12.3 What is automatable

Anything with a `ParamAddress`. That means: every sampler parameter, every effect parameter,
every mixer track volume/pan/send level, and **tempo**. The tempo map (§6.2) is generated by
evaluating the tempo automation, so tempo automation and the tempo map are the same system rather
than two systems that must be kept in sync.

### 12.4 Creating automation

- Right-click any knob or slider → "Create automation clip" places a clip on the current lane at
  the playhead, pre-targeted.
- MIDI learn (§14.4) and automation share the `ParamAddress` mechanism.
- Draw modes: pencil (freehand, decimated), line, curve, and shape stamps (LFO shapes at a
  chosen rate/division, which is the fastest route to rhythmic automation).

---

## 13. Mixer

### 13.1 Topology

```rust
pub struct MixerTrack {
    pub id: MixerTrackId,
    pub name: String,
    pub color: Color,
    pub gain_db: f32,
    pub pan: f32,
    pub pan_law: PanLaw,          // -3dB default
    pub mute: bool,
    pub solo: bool,
    pub phase_invert: bool,
    pub inserts: Vec<EffectSlot>,  // ordered chain, each bypassable
    pub sends: Vec<Send>,          // to any other track — see 13.2
    pub output: Option<MixerTrackId>,  // None = master
    pub input: Option<AudioInputId>,   // for recording/monitoring
}
```

Instrument channels route to mixer tracks by `ChannelId → MixerTrackId`. Multiple channels may
share a mixer track.

### 13.2 Routing and cycle safety

A track may output to any other track and may send to any other track. **The routing graph must
be validated acyclic on every mutation** — reject the command with a clear message, never allow a
feedback loop into the graph compiler. (A "feedback send" feature with an explicit one-block delay
is a plausible later addition; it is not v1.)

Sends are pre- or post-fader, with independent level and pan.

### 13.3 Metering

Per-track peak and RMS, with configurable peak hold and a clip indicator that latches until
clicked. Meter data goes UI-ward through a lock-free ring buffer of downsampled values — the RT
thread writes a small fixed-size struct, it never pushes to a queue that can grow.

Master track additionally offers LUFS-M/S/I and true-peak metering. This is cheap to add and
saves the user a plugin.

### 13.4 Built-in effects (v1)

All effects live in `fontelle-fx`, are `AudioNode` implementations, and are written against
`fontelle-dsp` primitives.

| Effect | Notes |
|---|---|
| **Parametric EQ** | 8 bands, types: bell, low/high shelf, low/high pass (12/24/48 dB/oct), notch, band-pass. TPT/SVF topology (zero-delay feedback) for stable modulation. Real-time FFT analyser overlay. Per-band solo/listen. Mid/side mode. |
| **Compressor** | Threshold, ratio, attack, release, knee, makeup, auto-makeup. Peak/RMS detection. Sidechain input from any mixer track. Gain-reduction meter and transfer-curve display. |
| **Reverb** | Feedback delay network (FDN), 8–16 lines with Householder mixing. Size, decay, damping, pre-delay, diffusion, modulation, width, freeze. Not convolution in v1 — algorithmic is more tweakable and has no IR licensing questions. |
| **Delay** | Tempo-synced or free. Ping-pong, filtered feedback, saturation in the loop, modulation. Tape mode (variable delay-line read with pitch artefacts on time change) — this is the one that produces character. |
| **Distortion** | Multiple curves: soft clip, hard clip, tube, fold, wave-shape. 2–8× oversampling to control aliasing. Pre/post filtering, drive, mix. |
| **Bitcrush** | Bit-depth reduction with selectable dither, sample-rate decimation with optional anti-alias filter (defaults **off** — the aliasing is the point), mix. |
| **Repitcher** | v1: varispeed (pitch and time locked together) plus high-quality resampling. v2: formant-preserving pitch shift behind the `stretch` feature flag (§3.3). |
| **Soften** | See §13.5. |
| Utility | Gain, pan, width, phase, mono-maker, DC filter, spectrum analyser, oscilloscope, tuner. |

### 13.5 "Soften" — the soundfont harshness tool

A dedicated effect targeting the specific, recognisable ways stock soundfonts sound bad. Half the
problem is solved in the voice (§7.8); this handles the rest:

1. **Dynamic high shelf** — a shelf whose gain reduction tracks high-frequency energy, so it only
   engages when the top end actually gets spitty rather than dulling everything uniformly.
2. **Adaptive resonance suppressor** — detects narrow, sustained spectral peaks in the high-mid
   (roughly 1–6 kHz, where sampled instruments honk) and applies narrow, momentary cuts.
3. **Transient softener** — attack-stage-only gain shaping, distinct from a compressor in that it
   is triggered by transient detection rather than level threshold.
4. **Air restore** — a gentle broad shelf above the suppression region, so the result is smoothed
   rather than merely darkened.

Presets: Gentle / Standard / Aggressive / Vintage ROMpler. This should be good enough that
"soundfonts just sound better in Fontelle than in FL out of the box" is a defensible claim, which
is a genuine reason for someone to try the software.

---

## 14. MIDI (`fontelle-midi`)

### 14.1 Staging decision

MIDI is implemented as **one complete end-to-end pass, not incrementally.** v1's skeleton phase
scaffolds only the minimum: the event pipeline shape and the RT-safe boundary. The full feature
set lands in a single dedicated milestone (§22, M5).

**Critical constraint on the scaffold:** the placeholder must already be a device-agnostic,
RT-safe event pipeline feeding the same sample-accurate event stream as notes and automation
(§11.1). If the placeholder shortcuts this — for example by polling MIDI on the UI thread and
injecting notes directly — then the "one complete pass" later becomes a rewrite of the transport,
which is exactly what this staging decision was meant to avoid.

### 14.2 Device handling

The principle is that creative people should plug hardware in and have it work.

- All input devices are opened and **merged automatically** into a single logical stream. No
  device selection step, no enable checkboxes, no "which controller is this" dialog on startup.
- Devices are identified by a stable key (name + port + USB identifiers where available) so that
  configuration survives replug and reboot.
- Hot-plug is handled live. Connecting a device mid-session just works; disconnecting does not
  produce errors or stuck notes (all notes for that device are released).
- Per-device config is *optional refinement*, never required setup.

### 14.3 Mapping

Per-device, saved to the user config (not the project, so it follows the user across projects):

- **Per-pad/key note remapping**: any incoming note → any outgoing note, with a learn mode. This
  is the drum-pad-to-drum-soundfont workflow and it must be two clicks: click the target key,
  hit the pad.
- Channel filtering, transpose, velocity curve, and velocity range per device.
- Optional per-device routing to a specific instrument channel (default: whatever is focused).

### 14.4 MIDI learn

Right-click any parameter → Learn → move a control. Stores `(device_key, cc) → ParamAddress`
using the same addressing scheme as automation (§8.2). Supports absolute, relative (encoder),
and toggle modes, with take-over behaviour (jump / pickup / scale) to avoid parameter jumps.

### 14.5 Clock and transport sync

External MIDI clock in and out, MMC transport, and Song Position Pointer. When slaved to external
clock, the tempo map is driven by the incoming clock with a PLL to smooth jitter, and tempo
automation is disabled with a clear UI indication of why.

### 14.6 MIDI file import/export

`.mid` import maps tracks to instrument channels and creates note clips. Export writes the
resolved timeline. Both use `midly`. Import must handle tempo and time-signature meta events into
the tempo map, and must not silently discard channel/CC data.

---

## 15. Audio clips and recording

Audio is fully supported and will be heavily used, but it is explicitly not the product. Scope
accordingly: everything below is real, none of it should delay the sampler.

### 15.1 Clip properties (non-destructive)

Every property is stored on the clip and applied at playback; the source file is never modified.

```rust
pub struct AudioClipData {
    pub asset: AssetRef,
    pub source_start: i64,        // samples into the file
    pub source_end: i64,
    pub gain_db: f32,
    pub pan: f32,
    pub pitch_semitones: f32,
    pub speed: f64,               // varispeed; couples pitch unless time_lock
    pub time_lock: bool,          // v2: engages the stretch engine (§3.3)
    pub reverse: bool,
    pub fade_in: Fade,            // length + curve
    pub fade_out: Fade,
    pub filter: ClipFilter,       // cutoff, resonance, type — per-clip, no plugin slot needed
    pub eq: Option<ClipEq>,       // 3-band, inline
    pub normalize: bool,
    pub loop_mode: ClipLoopMode,
}
```

Per-clip filter and EQ live in the clip properties rather than requiring a mixer insert, because
"make this one clip darker" should not cost a plugin slot or a dedicated mixer track.

### 15.2 Fade handles

Dragging the top-left or top-right corner of an audio clip creates a fade directly, with the
curve adjustable by dragging the fade's midpoint. Overlapping two clips on the same lane offers
an automatic crossfade. This is the FL 2025-era interaction and it is worth matching precisely —
it is one of the highest ratio-of-value-to-implementation-cost features in the audio side.

### 15.3 Waveform display

Peak files are generated on the asset loader thread at multiple zoom levels and cached in the
project's cache directory (not `assets/`, since they are regenerable). Display must remain
responsive while peaks are still generating — draw what exists, fill in progressively.

### 15.4 Recording

- Input selection per mixer track, with monitoring and an input-gain stage.
- Recording writes directly to the configured recordings directory (§17.1) as WAV, streaming from
  the RT thread through a lock-free ring to the disk thread. **The RT thread never touches the
  filesystem.**
- Punch in/out on the loop region. Take lanes and comping are a v2 feature; v1 records to a new
  clip per take.
- A recording in progress is crash-safe: the WAV header is finalised incrementally so a killed
  process leaves a playable file.

---

## 16. GUI architecture

### 16.1 Window model

**Default and normal behaviour: a single window with docked, resizable panels.** This is a stated
preference and it keeps the common case simple and tidy.

However, the windowing layer is **multi-window capable from the start**, because plugin editors
(when third-party hosting arrives) are OS child windows that cannot be docked into our panel
system. The rule:

- Our own panels dock. They may not need to detach in v1, but the panel system must not *assume*
  every panel is ours.
- Foreign content — future plugin editors — opens as a real OS window, floating, non-dockable.
  That is an acceptable and expected asymmetry.

Panels: Timeline, Piano Roll, Channel Rack, Mixer, Browser (soundfont library), Sampler Editor,
Settings. Layouts are saveable and ship with sensible presets.

### 16.2 Rendering stack

The dual-target requirement from §8 — the sampler editor must render both as a docked DAW panel
(winit) and inside a host's parent window (baseview) — combined with the piano roll's performance
demands, settles this:

**A custom widget layer over `wgpu`, with a windowing abstraction over both `winit` and
`baseview`.**

```
fontelle-ui
├── backend/          winit (app) | baseview (plugin) — same widget code above this line
├── render/           wgpu device, surface, pipelines; vello for vector content
├── text/             cosmic-text shaping and atlas
├── widget/           retained widget tree, dirty-region invalidation
├── canvas/           direct-draw surfaces for timeline / piano roll / mixer
└── theme/            tokens, colours, metrics
```

`nice-plug` ships a `byo_gui_wgpu` example demonstrating exactly this pattern under baseview,
which is the reference to follow.

**Risk acknowledgement:** this is the highest-risk component in the project. Rust GUI is where
ambitious audio projects historically stall. Two mitigations: (a) the widget layer only needs to
be good enough for chrome — menus, panels, knobs, lists — because the editors are custom canvas
anyway; (b) if `vello` proves troublesome, the fallback is direct `wgpu` pipelines with `lyon`
for path tessellation, which is more code but fewer unknowns. Do not let the widget layer grow
into a general-purpose framework.

### 16.3 Retained tree, dirty regions

Retained widget tree with explicit invalidation. Redraw only dirty regions. When the transport is
stopped and nothing is animating, **issue no frames at all** — this is a hard requirement for the
idle-CPU target (§19), and it is much easier to build in than to retrofit.

### 16.4 Timeline and piano roll rendering

Both are custom canvases, not widget trees.

- **Virtualisation is mandatory.** Build geometry only for the visible time window and visible
  lanes/keys. A project with 200 lanes and 100,000 notes must scroll at full framerate.
- Notes render as instanced quads in a single draw call, with a separate pass for selection and
  velocity overlays.
- Grid, playhead, and automation curves are separate layers with independent invalidation — the
  playhead moving must not redirty the note geometry.
- Zoom is continuous and independently controllable per axis.

### 16.5 Piano roll interaction

Keybinds and mouse behaviour track FL Studio closely, since that is existing muscle memory for
the target user. Full keymap is a separate document (`KEYMAP.md`) produced during M3, but the
core contract:

- Tools: Draw (P), Paint, Delete (D), Select (E), Slice (C), Mute, Slip. Right-click deletes in
  any tool.
- `Ctrl+A` select all, `Ctrl+B` duplicate, `Ctrl+C/V/X`, `Alt+drag` for free positioning
  (snap bypass), `Shift+drag` for constrained axis.
- Snap: bar / beat / step / 1-6 divisions / triplets / none, with a global snap toggle.
- Ghost notes from other clips on the same channel, toggleable.
- Note property lanes below the roll: velocity, pan, fine pitch, release, mod X/Y — mirroring
  the `Note` struct (§10.4).
- Chord and scale helpers, arpeggiator, strum, randomise, humanise, quantise (with strength and
  swing), flam, and chop — all implemented as **Commands** operating on selections, which means
  they are all undoable and all appear in the history with meaningful names.

**All keybinds are remappable.** Ship an FL-compatible default set and leave the map open.

### 16.6 Theming

Token-based theme (colours, metrics, radii, font) loaded from a file. Ship a dark default and a
light variant. User themes are just files in the config directory. Do not build a theme editor in
v1; a documented file format is enough.

---

## 17. Project storage and asset management

### 17.1 On-disk layout

Projects are **folder bundles**:

```
MyTrack.fontelle/
├── project.json          # the document (§17.2)
├── assets/               # copied-in samples and soundfonts
├── recordings/           # captured audio
├── renders/              # bounces and exports
├── backups/              # timestamped autosaves + destructive-edit snapshots
└── cache/                # waveform peaks, thumbnails — regenerable, safe to delete
```

Benefits this buys over a single file, all of which should be implemented:

- **Atomic saves.** Write to a temp file, `fsync`, rename. A crash mid-save can never corrupt a
  project.
- **Crash recovery.** Autosaves are files in `backups/`, not mutations of the live document.
- **Partial/progressive loading.** Open the document and show the UI immediately; stream sample
  data and peaks in behind it.

The cost is that sharing a project means zipping a folder. Provide a one-click **Export Bundle**
that collects all referenced assets and packs the result.

### 17.2 Document format

`project.json` — JSON, `serde`-derived, with a `format_version` field and an explicit migration
chain. JSON is chosen over a binary format because it is diffable, inspectable, greppable, and
recoverable by hand when something goes wrong. If large projects become slow to parse, the
mitigation is to move note data to a sidecar binary blob referenced from the JSON, not to
binarise the whole document.

**Never write floating-point beat positions.** Ticks only (§6.1).

### 17.3 Storage locations — nothing outside configured paths

**INVARIANT 10 — Fontelle writes nothing outside locations the user has explicitly configured,
except its own config directory.**

First-run setup asks for, and settings allow changing independently:

| Path | Default | Notes |
|---|---|---|
| Projects root | asked at first run | Never assume `~/Documents` |
| Renders root | `<project>/renders` | Independently overridable — renders are what actually fill disks |
| Recordings root | `<project>/recordings` | Independently overridable |
| Soundfont library dirs | asked at first run | Multiple; §17.5 |
| Config | XDG (`~/.config/fontelle`) | The one exception |
| Cache | XDG (`~/.cache/fontelle`) | Purgeable from settings, with size shown |

### 17.4 Asset references and the import prompt

```rust
pub struct AssetRef {
    pub id: AssetId,
    pub path: PathBuf,        // absolute, or project-relative if copied in
    pub content_hash: u64,    // xxhash of first 1MB + file size
    pub size: u64,
    pub kind: AssetKind,
}
```

**Import behaviour.** On dragging in a sample or soundfont, the app asks whether to reference it
in place or copy it into the project. The dialog carries a "don't ask again" checkbox that stores
the answer as the default. Thereafter imports use the default silently — but **holding Shift
during the drop forces the dialog to appear** for that one import, so a different choice is always
one modifier away without changing the preference.

This pattern — *remembered default, per-action escape hatch, discoverable modifier* — is a general
UX principle for this application, not a one-off. Apply it anywhere the app would otherwise make a
silent decision on the user's behalf: sample-rate conversion on import, replace-vs-layer when
dropping a soundfont onto an occupied channel, overwrite-vs-version on render.

**Broken links are a normal condition, not an error state.** Reference-by-default guarantees
missing files will happen. Required behaviour:

1. On load, unresolved references are searched for automatically by content hash and filename
   across the soundfont library directories and recently-used paths.
2. Anything still unresolved surfaces in **one** relink dialog listing all of them.
3. Pointing that dialog at a parent directory resolves every file found beneath it in one action.
4. The project loads and plays with placeholders for anything still missing. It does not refuse
   to open, and it does not lose the references on the next save.

A modal per missing file, or silent failure, are both unacceptable.

### 17.5 Soundfont library

The user configures one or more soundfont directories. These are scanned in the background into
an index:

- Watched for changes (`notify`), incrementally reindexed.
- Index stored in the cache directory: path, hash, size, preset list, instrument names, sample
  count, and user-added tags and favourites.
- The Browser panel offers instant fuzzy search across **file names and preset names inside
  files** — searching "marimba" must find the marimba preset inside `GeneralUser.sf2`, not just
  files called marimba. This is the feature that makes a large soundfont collection usable and it
  is a genuine differentiator.
- Audition on click without loading into a channel.
- Drag from browser to timeline, channel rack, or an existing channel.

---

## 18. Settings and first run

First run is a short wizard, not a wall of options:

1. Audio device, sample rate, buffer size, with a test tone and a measured round-trip latency
   readout.
2. Projects directory.
3. Soundfont directories (with a link to a curated list of good free soundfonts — this materially
   improves the first-hour experience for a new user).
4. Theme.

Everything else has a working default and lives in Settings. Audio settings must include: device
selection, sample rate, buffer size, exclusive/shared mode where applicable, backend selection
(auto / PipeWire / JACK / ALSA / PulseAudio on Linux), multithreading toggle, and a plain-language
explanation of the latency/CPU trade-off rather than raw numbers alone.

On Linux, if RT priority cannot be acquired, say so clearly with the specific fix (rtkit missing,
or `@audio - rtprio 95` absent from `limits.conf`) rather than failing silently or crashing.

---

## 19. Performance targets

These are the acceptance criteria. They are measured in CI (§20.5), not assessed by feel.

| Metric | Target |
|---|---|
| Cold start to interactive window | < 1000 ms |
| Project open (100 clips, 20 channels) to interactive | < 500 ms; sample data streams behind |
| Audio thread allocations | **Zero** — enforced by debug assertion |
| Audio thread locks / syscalls | **Zero** |
| Callback deadline misses at 128 frames / 48 kHz | Zero under nominal load |
| UI framerate, 10,000 notes visible | 60 fps sustained |
| Idle CPU, transport stopped | < 0.5% of one core |
| Idle memory, empty project | < 150 MB |
| Installed size | < 100 MB |
| Sampler polyphony, Normal interpolation, mid-range CPU | > 256 simultaneous voices |

The stated floor is "no slower than FL Studio under Wine on the developer's machine," but that is
a low bar because Wine adds real overhead. **The actual target is parity with natively-compiled
Reaper or Bitwig.** Fontelle does dramatically less than either, so it should comfortably win on
startup time, idle CPU, and memory.

---

## 20. Testing and correctness

### 20.1 Unit and property tests

- **Tempo map**: property test that `sample_to_tick(tick_to_sample(t)) == t` across random tempo
  maps including ramps.
- **Prefab resolution**: property test that resolution is deterministic, that override application
  is order-independent within a level, and that cycles are always rejected.
- **Commands**: property test that `apply` followed by `invert().apply` restores the document
  byte-identically, for every command type. This is the single highest-value test in the project.
- **Serialisation**: round-trip property test; plus a corpus of saved projects from every format
  version that must continue to load.

### 20.2 DSP tests

Golden-file tests: each effect and the sampler render known input to a reference output, compared
within tolerance. Regenerating goldens requires an explicit flag and a justification in the commit
message.

Additionally: null tests (bypass must be bit-identical), denormal handling under sustained decay,
and stability sweeps for filters under extreme modulation.

### 20.3 SF2 conformance

A corpus of soundfonts covering the awkward cases: sf3 compressed, 24-bit samples, ROM samples,
missing loop points, zero-length zones, malformed chunks, huge files, and files with unusual
modulator configurations. Every one must either import correctly or fail with a clear message.
Neither crashing nor silent misbehaviour is acceptable.

### 20.4 Realtime-safety enforcement

A custom global allocator that, in debug and test builds, panics if invoked from a thread tagged
as RT. Additionally, run the engine under a soak test with `assert_no_alloc`-style instrumentation
in CI. Any allocation on the audio thread is a **build failure**, not a warning.

### 20.5 Benchmarks in CI from day one

`criterion` benchmarks tracked over time, failing the build on regression beyond a threshold:

- Voices before first dropout at 128/48k.
- Callback jitter distribution (p50, p99, max) under load.
- Timeline recompilation time vs. project size.
- Prefab resolution time vs. nesting depth.
- Frame time vs. visible note count.

Catching performance regressions by number rather than by noticing sluggishness six months later
is the entire point.

---

## 21. Build, packaging, distribution

- **Linux (primary):** AppImage (works everywhere, zero install), Flatpak (Flathub reach), AUR
  package (CachyOS/Arch, the developer's own platform), and a plain tarball. Static-link
  everything feasible; the only expected system dependencies are ALSA and, optionally, PipeWire
  or JACK client libraries.
- **Windows:** portable zip plus an installer. ASIO support requires the ASIO SDK path at build
  time and is a separate build configuration.
- **macOS:** unsigned tarball initially. Notarisation is a Fopull LLC decision with a cost
  attached; not a v1 blocker.
- **Plugin builds:** CLAP and VST3 bundles for all three platforms, produced by `cargo xtask
  bundle`. CLAP is canonical; VST3 is gated on the licensing verification in §3.4.
- **CI:** build and test on Linux, Windows, macOS. `cargo-deny` for licences. `clippy -D warnings`.
  `rustfmt` check. Benchmarks on Linux only.
- No auto-updater, no telemetry, no crash reporting that phones home. A local crash log the user
  can attach to an issue is sufficient and is what the target audience expects from FOSS.

---

## 22. Milestones

### M0 — Skeleton, with a hard gate

Scaffold every crate in §4 with real module boundaries and stub implementations. **Features may be
stubbed; boundaries may not be.**

The phase does not end until this vertical slice runs and is measured:

> Audio callback → compiled graph → one sampler voice reading a real SF2 zone → mixer track →
> device out, triggered by a note from a clip on the timeline, at 128 frames / 48 kHz, with the
> zero-allocation assertion active and passing.

Everything else may return silence. But if this slice works and the allocation assertion holds,
the RT/model boundary is proven and every subsequent feature is additive. Skipping this gate means
discovering an architectural mistake at month five.

For the agent specifically: "done" for a skeleton module means **compiles, runs, and passes a
named test** — not "looks structurally correct."

### M1 — Sampler core
SF2/sf3/sfz import, patch model, voice architecture, mod matrix, all interpolation modes,
streaming, the §7.8 voice-side harshness mitigations. Sampler editor UI. Preset save/load.

### M2 — Plugin export
`fontelle-plugin` shipping CLAP and VST3. Validated against `clap-validator` and tested in at
least two foreign hosts (Bitwig and Reaper on Linux). Preset compatibility verified in both
directions against the DAW build.

*This lands early deliberately. It proves the §4.1 boundary is real before the DAW grows around
it, and it produces a shippable artefact long before the DAW is finished.*

### M3 — Timeline, piano roll, transport
Document model, commands and undo, clips, lanes, sequencer compilation, playback. Piano roll with
the full FL-compatible keymap and note property lanes. Mirror prefabs and property overrides
(with the full ID/override infrastructure per §10.5.1).

### M4 — Mixer and effects
Routing, sends, metering. EQ, compressor, reverb, delay, distortion, bitcrush, repitcher, Soften,
utilities. Automation clips end to end.

### M5 — MIDI
The complete pass per §14. Devices, merging, hot-plug, remapping, learn, clock sync, file
import/export.

### M6 — Audio clips and recording
Clip properties, fade handles, waveform display, recording, export/bounce.

### M7 — Polish and first release
Browser search, theming, settings, first-run wizard, packaging, documentation, keymap
customisation UI, performance pass against §19.

### Post-v1
Prefab variants and structural overrides · third-party plugin hosting (CLAP → LV2 → VST3) ·
time-stretch behind the feature flag · take lanes and comping · convolution reverb · detachable
panels · MPE.

---

## 23. Risks

| Risk | Severity | Mitigation |
|---|---|---|
| GUI layer stalls the project | **High** | Keep the widget layer minimal — chrome only. Editors are custom canvas regardless. `lyon` fallback if `vello` disappoints. Timebox and reassess at M3. |
| Prefab system generates subtle correctness bugs | High | Ship only mirror + property overrides in v1. Property-test resolution determinism. Full ID infrastructure from day one so later work is additive. |
| Sampler scope expands without limit | High | INVARIANT 6 (fixed topology) is the boundary. Free-form patching is explicitly out of scope. |
| Solo-developer burnout | High | This is the documented cause of death for the most serious prior Rust DAW attempt. M2 exists partly so that a shippable, independently valuable artefact exists early. |
| Agent-generated code that looks right and does nothing | Medium | M0's named-test definition of done. The zero-allocation assertion. CI benchmarks from day one. |
| VST3 licensing blocks the plugin build | Medium | CLAP is canonical and unencumbered. VST3 is a convenience build that may be dropped without affecting the product. |
| SF2 import defaults subtly wrong | Medium | RustySynth as reference implementation. Conformance corpus (§20.3). |
| Streaming underruns on large soundfonts | Medium | Graceful degradation (hold last sample, never click). Configurable resident threshold. |

---

## 24. Open questions

1. **VST3 SDK licensing** (§3.4) — must be resolved before M2 ships a VST3 build.
2. **Project format scaling** — at what note count does JSON parsing become the bottleneck, and is
   the sidecar-blob mitigation needed for v1? Measure at M3.
3. **Feedback sends** — is a one-block-delay feedback routing option wanted, or is acyclic-only
   correct permanently?
4. **Multi-out from a single channel** — should one instrument channel be able to route different
   layers or key ranges to different mixer tracks (drum-kit-style)? Cheap to add in the model,
   meaningful UI cost. Decide before M4.
5. **Micro-tuning / non-12-TET** — the voice architecture supports it trivially. Worth exposing in
   v1, or later?
6. **Default history depth** — 100 is proposed against a 256 MB ceiling; validate against real
   projects at M3.
