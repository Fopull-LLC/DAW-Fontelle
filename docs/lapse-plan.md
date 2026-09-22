# Lapse: the time and volume machine (2026-09-22)

> *"a gross beat like plugin … a section for time manipulation and a section
> for volume and you can draw in patterns for these and make curves and stuff
> in the editor easily and it affects the playback correctly like gross beat.
> should have extensive presets for general purpose and some more creative
> applications. visual design should be similar to flopsynth. feel free to add
> onto or expand on this where you see opportunities to make it possibly even
> better than the original."* — Ty, 2026-09-22

This is that plan. It is written for the agent who builds it, not for Ty, and
it assumes you have read `CLAUDE.md`, `PROGRESS.md`'s top two sections,
`docs/handoff.md`, `docs/effects-catalogue.md` §1 and §5, and
`docs/flopsynth-next.md` §3 (the window vocabulary this one borrows).
`FONTELLE_TDD.md`'s INVARIANTs are hard.

Every file:line in this document was checked against the tree at `b567d17`
(v0.11.0). If you find one that is wrong, fix the plan.

---

## 0. Ground rules for this work

1. **Tests first, confirmed failing**, then the implementation. Every step
   below names its test file.
2. **Look at every visual change** on the nested `Xwayland :99` before
   believing it, and grab twice (memory `seeing-fontelles-gui`). The headless
   dump (`FONTELLE_UI_DUMP=<dir> cargo test -p fontelle-ui --test
   render_headless`) is the test that can see.
3. **Never `cargo fmt --all`**; `rustfmt --edition 2024 <file>` on what you
   edited. `cargo clippy --workspace --all-targets -- -D warnings` is part of
   the bar.
4. **Never touch an existing address** (INVARIANT 7). Every control here is a
   new address; a chooser that grows a position grows it at the end.
5. **INVARIANT 1 on the audio thread**: no allocation, no lock, no syscall.
   Everything this effect reads per block is either a `Copy` config over a
   triple buffer or a pre-sized buffer from `prepare`.
6. **INVARIANT 5**: musical time is ticks, audio time is samples, and they are
   never confused. §3.4 is where this plan spends that rule.
7. **A new effect window is five lists, not one.** §7.7. Grep `self.notepad`
   and answer every hit; the pad taught this the hard way
   (`docs/handoff.md` §1).
8. **Publishing is Ty's.** Prepare a release, stop at needs-review.

---

## 1. What it is, and what "done" means

**Lapse is a bar of memory with a curve drawn over it.** The insert keeps the
last two bars of everything that passed through it. A **time curve** says,
for each point in the bar, how far back in that memory to read; a **volume
curve** says how loud to play it. Both are drawn, in the window, over a grid
of the bar. Everything the original is famous for — the stutter, the hold, the
scratch, the tape stop, the half-time — is one of those two curves at a
particular shape, and nothing else.

That is the whole machine. Its power is not in features; it is in the fact
that *one* drawn line is a freeze, a roll, a rewind and a groove template
depending on its slope. The rest of this document is about making that line
honest, drawable and fast.

**Done** means all of:

1. A fresh Lapse is **a wire, sample for sample** — the time curve flat at
   zero, the volume curve flat at unity (`fontelle-engine/tests/effects.rs`
   already measures the passthrough sample for sample for the Notepad; the
   same test shape holds here).
2. A drawn **hold** freezes the sound exactly: during it, the output is the
   input from the frozen instant, sample for sample, and when it releases the
   sound is where the song is, not where the buffer left off.
3. The pattern is **locked to the song**. Start playback at bar 1 or at bar
   37 and the same beat gets the same part of the curve. Change the tempo and
   the pattern follows it.
4. The **slope is the pitch**. A segment whose offset falls at half a beat per
   beat plays at half speed, an octave down, measurably.
5. A jump in the curve **does not click**, and the knob that controls that is
   a knob you can hear working.
6. **Sixty-four factory presets** in nine categories plus six twelve-scene
   kits, every one of them reachable from the preset bar and the browser, and
   every one of them audibly not the wire.
7. The window is **a Flopsynth window** — the same inks, the same knobs, the
   same preset bar — and its editor is something a person draws a tape stop in
   within ten seconds of opening it.
8. `cargo clippy --workspace --all-targets -- -D warnings` clean and the
   workspace suite green.

---

## 2. The name

Working name **Lapse** throughout this document: one word, in the family of
*Soften*, *Width*, *Hyper*, *Fold*, *Tune*; about time; and free of the other
program's trademark, which matters the day this repository goes public.
`EffectKind::Lapse`, `label()` → `"Lapse"`, slug `fx-lapse`.

**This is Ty's to overrule** (§14.1), and it is cheap to change before phase 2
and expensive after: the slug is a folder name and therefore INVARIANT 7's —
renaming it after a release moves everybody's presets. Alternatives that were
considered: *Flopbeat* (matches Flopsynth, says exactly what it is, and is the
funniest of them), *Rewind*, *Grab*, *Loophole*.

---

## 3. Where it sits: the five decisions

### 3.1 It is an insert, `EffectKind::Lapse`

For the reason every other one is: it is saved, undone, automated,
copied with a track chain, keyed to a preset and found in the same menu, for
no new machinery. It goes in the catalogue's §2.4 (*Time and modulation*),
appended to `EffectKind::ALL` after `Width` and before `Notepad`, so the pad
stays last and on its own.

It is **not** `is_time_based` (`crates/fontelle-types/src/effect.rs:101`). A
delay's output belongs *under* the track; Lapse's output *replaces* it. It
opens fully wet, like every other processor. (A hold at 50 % mix is the hold
flamming against the live signal. That is a real sound and somebody will use
it; it is not a default.)

It **takes notes** (`EffectKind::takes_notes`, the predicate Tune added) and
does **not** take a key: it has no detector.

**Two catalogue rows are absorbed by it.** §2.4's *Stutter / glitch* is a time
curve and §2.4's *Trance gate* is a volume curve — both are this effect at one
setting, and shipping them separately would be three effects that are one
machine. Mark both rows *absorbed* in the catalogue when this lands, with a
pointer here. That is a saving of two P2 effects, not a loss of two.

### 3.2 The curves live beside the config, not in it

`EffectConfig` is `Copy`, fixed-size and handed to the audio thread every
block (`crates/fontelle-engine/src/effect_channel.rs`). Twelve scenes of four
lanes of sixty-four points is 50 KB. It is none of those things, so the split
is the one the notepad and a hosted plugin already use
(`crates/fontelle-model/src/mixer.rs:108` and `:43`):

- **`LapseConfig`** is the knobs — depths, smoothing, quality, lookahead,
  which scene is playing, mix. `Copy`, small, automatable, and on the existing
  triple buffer with every other insert's config.
- **`EffectSlot::lapse: Option<Box<LapseBank>>`** is the drawn material.
  Boxed, so every other insert pays one null pointer for it; `Some` on every
  Lapse from `EffectSlot::new` onwards, `None` everywhere else, and
  `skip_serializing_if` so every project written before this opens and is
  written back unchanged.

A second field on the slot beside `notepad` rather than one `SlotState` enum
holding both: the enum is tidier and it is not worth the project-format churn
(`"notepad": {…}` would become `"state": {"notepad": {…}}`, and a v0.11.0
project would lose its lyrics). Say so in the doc comment so the next person
does not re-litigate it.

### 3.3 The curves cross to the audio thread on their own channel

This is where Lapse differs from the notepad, and it is the one genuinely new
piece of engine plumbing. A pad's words never cross; a curve must, and it must
cross *while somebody is dragging it*, because a curve editor whose sound
arrives on the next graph rebuild is a curve editor nobody can use. A graph
rebuild costs a patch deserialisation per channel (`effect_channel.rs`'s own
module comment) and is not available sixty times a second.

So: **`crates/fontelle-engine/src/lapse_channel.rs`**, the sibling of
`effect_channel.rs`, written to the same shape and for the same reason.

```rust
pub fn lapse_channel(grid: LapseGrid) -> (LapseControls, LapseSource);
impl LapseControls { pub fn publish(&mut self, grid: LapseGrid); }
impl LapseSource   { /** RT. */ pub fn current(&mut self) -> &LapseGrid; }
```

Two differences from `effect_channel`, both deliberate:

- **`current` returns a reference.** `EffectSource::current` returns
  `(EffectConfig, bool)` by value because a config is tens of bytes; a
  `LapseGrid` is 50 KB and copying it per block per insert would be 19 MB/s of
  memcpy for nothing. `triple_buffer::Output::read` already hands back a
  reference; take it.
- **`LapseGrid` is a realised form, not the document's.** The document stores
  `Vec<LapsePoint>` because that is what an editor and an undo stack want; the
  grid is a fixed-size POD (`[[RtLane; 4]; 12]`, sixty-four points a lane,
  `at`/`value`/`curve`/`tension` at 16 bytes a point) built by
  `fontelle-app`'s realise layer, which clamps to the cap. The document is the
  truth and the RT gets a realised copy — the same split `realise.rs` already
  is.

Attached to the node with `EffectNode::with_lapse(source)`, beside
`with_controls` (`nodes.rs:1147`) and `with_tune_tap` (`:1195`).

**Why not rasterise the curve into a table off-thread.** It was the first
design and it is worse: a table has a resolution, and the resolution of the
*time* curve is a pitch quantisation (the read rate is the curve's derivative,
so a stair in the table is a stair in the pitch). Evaluating the segment on
the RT thread is a cursor that only moves forward, a compare and one eased
interpolation per sample — cheaper than the table read it replaces, exact, and
it leaves no staleness question when a drag lands mid-block.

### 3.4 The song's position has to reach the audio thread, and today it does not

**This is the prerequisite, and it is not optional.** Every synced effect in
this program is phase-free: rule 5 of the catalogue gives a node
`TransportSnapshot::bpm` (`crates/fontelle-engine/src/transport.rs:211`), and
a tempo is a *period* — it says how long a 1/8 is, not *where in the bar you
are*. That is invisible on a delay, whose repeats are relative to whatever
went in, and fatal here, where the whole point is that beat 4 of every bar
does the same thing.

Deriving it inside the effect from `position_sample × bpm` is wrong the moment
there is a tempo change (it integrates the *current* tempo over the *whole*
song), and silently wrong, which is worse.

The honest answer is the one the tempo table's own doc comment already argues
(`crates/fontelle-types/src/event.rs:143-157`): **the sequencer owns every
tick-to-sample conversion in the project, so it compiles this too.**

- `CompiledTimeline::tempo` becomes a table of segments rather than pairs:
  `TempoSegment { start: Sample, bpm: f32, tick: Tick, ticks_per_sample: f64 }`
  — the tick at the segment's first sample, and the rate inside it. Built at
  `crates/fontelle-sequencer/src/compile.rs:438` from the same `TempoMap` walk
  that builds the pairs today; `bpm_at` keeps working and loses nothing.
- `CompiledTimeline::tick_at(sample) -> Tick` beside `bpm_at`
  (`event.rs:221`): binary search, no allocation, RT-safe, and `0` for a
  timeline nobody compiled.
- `CompiledTimeline::beats_per_bar: u32`, compiled from
  `Project::beats_per_bar` (`crates/fontelle-model/src/project.rs:249`).
  Defaulted to 4 so an uncompiled timeline answers something musical.
- `TransportSnapshot` gains three fields: `position_tick: Tick`,
  `ticks_per_sample: f64`, `beats_per_bar: u32`. Filled in
  `transport.rs`'s snapshot builder from the timeline it already reads
  `bpm` out of.

Ticks and not beats, because INVARIANT 5 says musical time is stored in ticks
and `PPQN` is 960 (`crates/fontelle-types/src/time.rs:8`). The effect divides
by `PPQN` when it wants a beat; nothing else in the program has to learn a new
unit.

`ticks_per_sample` rather than making the node divide: the tempo inside a
segment is constant, so advancing the phase across a block is one multiply per
sample and exact. A tempo change mid-block moves to the next block's snapshot,
which is what every other tempo-reading effect in the tree already does.

**What this unblocks besides Lapse**: the catalogue's tremolo, vibrato and
every future synced LFO can start *in phase with the bar* instead of wherever
the graph was last reset. That is a long-standing quiet wrongness in this
program and this is the fix for it.

### 3.5 The curve vocabulary is the automation lane's, and this is what finally gives `tension` a meaning

`CurveShape` — Linear, Exponential, Logarithmic, SCurve, Stepped, Hold — and a
per-point `tension` already exist, in `crates/fontelle-model/src/automation.rs:6`,
and the comment at `:46` says:

> *"`tension` is not read yet. It is in the document (§12.1) and the shapes
> below are its zero position; a curve editor that can bend one is what gives
> it a value to have."*

**Lapse is that editor.** So:

- `CurveShape`, `eased`, `holds` and `freezes` **move to
  `fontelle-types/src/curve.rs`** and `fontelle-model` re-exports the name so
  nothing above it changes. Forced rather than chosen: the audio thread reads
  these shapes and `fontelle-engine` may not see `fontelle-model`
  (INVARIANT 4).
- `eased` grows a `tension: f32` argument, bipolar in −1..1, zero being
  exactly today's shape. Use the rational bend
  `t' = t / (t + k·(1 − t))` with `k = ((1 − |τ|) / (1 + |τ|))^(±1)` — two
  multiplies and a divide, monotone, and it still returns 0 at 0 and 1 at 1,
  which is the property the doc comment says a shape may never break.
- The **automation lane inherits the bend for free** the day this lands: same
  function, same field, already stored. Hold that with a test in
  `fontelle-model/tests` — a tension of zero must give bit-identical results to
  today's `eased`, or every automation clip in every project changes shape.

---

## 4. The machine

`crates/fontelle-fx/src/lapse.rs`. One struct, `Lapse`, with `new`,
`prepare` (the only place that allocates), `reset`, and

```rust
pub fn process(
    &mut self,
    outputs: &mut [&mut [f32]],
    notes: NoteInput,          // the same shape Tune takes
    grid: &LapseGrid,
    config: &LapseConfig,
    music: MusicalTime,        // §3.4, built in nodes.rs from ctx.transport
);
```

### 4.1 The clock

Per block, from `music`: the tick at the block's first sample, and the ticks
per sample. Per lane, a **phase**

```
φ = ((tick − origin) / lane.length_ticks).rem_euclid(1.0)
```

advanced per sample (or per step, §4.6) by `ticks_per_sample / length_ticks`.
`origin` is 0 for the default clock mode, which is what makes bar 37 behave
like bar 1.

Three clock modes (`LapseSync`):

- **Song** (default). `origin = 0`; the pattern is nailed to the arrangement.
- **Retrigger**. `origin` is set to the tick of the last note-on arriving on
  the slot's `notes` channel (§4.7). The pattern becomes something you *play*.
- **Free**. `origin` is set on transport start; the pattern runs from wherever
  playback began, which is what somebody jamming over a loop actually wants
  when their loop does not start on bar 1.

**When the transport is stopped**, the tick does not advance — and an insert
being auditioned with the transport stopped must still stutter, or every
person who adds one before pressing play concludes it is broken. So a stopped
(or a non-rolling) transport advances an internal phase at `bpm`, seamlessly,
and the first rolling block re-locks to the song's own tick. Test it.

**Swing** (`swing`, 0–100 %): warp φ inside each 1/8 so the second half is
late by up to a third of it. One `if` and a lerp, applied to the phase before
the lanes read it, so swing bends the drawn curve rather than a grid on top of
it.

### 4.2 The memory

A ring of `LAPSE_MEMORY_SECONDS = 12.0` seconds per channel, sized in
`prepare` from the sample rate — 4.6 MB stereo at 48 kHz, 9.2 MB at 96 kHz.
Twelve seconds is two bars of 4/4 at 40 bpm plus a bar of lookahead; below
that tempo the offset **clamps** to what is in the buffer rather than reading
silence, and the window says so in its read-out. A constant rather than a
tempo-derived size because `prepare` is the only place that may allocate and
the tempo changes afterwards.

Written every block, unconditionally, **including while the effect is doing
nothing** — a memory with a hole in it is the bug where a stutter thrown in
after four bars of silence plays four bars of silence.

**The first bar problem.** After a reset (a stop, a seek, a loop wrap, an
un-bypass) there is no history, and a hold drawn over bar 1 would play
silence. Clamp the read position to the oldest sample actually written: the
pattern then plays the live signal until the memory has caught up, which is a
mild wrongness for one bar instead of a silence somebody files as a bug. It is
a rule, not an accident, and `fontelle-fx/tests/lapse.rs` holds it.

### 4.3 The read head, and why the slope is the pitch

Let `o(φ)` be the time lane's value in **beats**, negative for the past, and
`s` the samples per beat at the current tempo. Then

```
delay(t) = −o(φ(t)) · s        read(t) = write(t) − delay(t)
rate(t)  = d(read)/dt = 1 − d(delay)/dt
```

Everything the effect does falls out of that one line and nothing else has to
be written:

| What is drawn | What is heard |
|---|---|
| flat at 0 | a wire |
| flat at −1 beat | the same thing, a beat late |
| falling 1 beat per beat (a 45° line down, at ×1 zoom) | **a freeze** — `rate = 0` |
| falling half a beat per beat | half speed, an octave down |
| rising half a beat per beat | one and a half times speed, seven semitones up |
| falling 2 beats per beat | **reverse**, at normal speed |
| a vertical drop | a jump backwards — a stutter's repeat |
| stepped | a roll: N repeats of the same slice |

The editor draws a **45° guide** across the grid for exactly this reason, and
has a *hold* tool that lays down that line for you (§7.4). It is the single
piece of knowledge that separates somebody who can use this from somebody who
cannot, and it should be visible on the surface rather than learned from a
forum.

**Interpolation.** `fontelle_dsp::interpolate` already has the kernels
(`crates/fontelle-dsp/src/interpolation.rs`): 4-point Hermite at `Normal`,
8-point windowed sinc at `High`. It takes a `&[f32]` and a position; the ring
wraps, so add a wrapping read beside `lines::read_at`
(`crates/fontelle-fx/src/lines.rs:12`, which is linear and `pub(crate)` in
this crate already) rather than copying the kernel: `read_at_quality(line,
position, Interpolation)`. The chorus keeps its linear read — its loss is part
of its sound and the comment there says so.

At `rate > 1` the read is a decimation and aliases. `High` (8-tap sinc) is the
answer up to about 2×; beyond that the honest thing is to say so in the
window's read-out rather than to pretend. Measure it
(`fontelle-fx/tests/lapse.rs`'s alias case, the shape `synth_alias.rs`
already uses: energy off the note's grid against energy on it).

### 4.4 The crossfade, and the click that is not one

A discontinuity in `o` — a stepped point, a vertical segment, a lane wrap, a
scene change — moves the read head instantly and that is a click.

Two read heads and an equal-power crossfade over `smoothing` (0–50 ms,
default 12 ms). Detect the jump by comparing the read position this sample
against last sample plus the expected rate; a difference over half a
millisecond starts a fade. Hyper already runs two crossfaded heads over a
window (`crates/fontelle-fx/src/hyper.rs`) — read it before writing this, and
reuse `lines.rs` where it fits.

**Smoothing at zero must still be usable**: a hard cut at a zero crossing is a
sound people want (the "buzz" presets). At `smoothing = 0` snap, and let the
test assert the step is there rather than pretending a knob at zero does
something.

### 4.5 Lookahead: the thing the original cannot do

The original reads only the past, because it has no way to tell its host it
needs time. This program has had delay compensation since TDD §5.5, and
`EffectNode` already carries a **dry delay line** for a lookahead insert
(`crates/fontelle-engine/src/nodes.rs:723-744`, written for the gate, with the
comment explaining exactly why the dry must be delayed too).

So `lookahead` is a chooser — **Off / 1 beat / 1 bar** — and it does one
thing: it shifts the whole read head later by that amount, and reports it from
`EffectNode::insert_latency_samples`. The time lane's axis then extends
*above* zero, and a curve can read audio that has not been played yet: a
stutter that previews the next beat, a gate that opens before the transient,
a roll built out of what is coming. Off by default, because the cost is real
and rule 6's reasoning ("as a chooser rather than a switch, because the cost
is real and the person mixing chooses where to spend it") applies exactly.

A beat and a bar are *tempo-dependent*, and latency may not change per block —
so the latency is computed from the tempo at `prepare` time, held, and
re-prepared on a structural rebuild, the way Tune's range-dependent latency
already is (`docs/tune-plan.md` §2.4, §3.8). Hold that with a test: the
reported latency must not change while the graph is running.

### 4.6 The other two lanes

Four lanes, one machine. **Time** and **Volume** are the ones asked for and
are on by default (flat). Two more are off by default, cost nothing when off,
and are what turn this from a rhythm tool into an instrument:

- **Tone.** Bipolar around centre: below it a low-pass sweeping down to
  200 Hz, above it a high-pass sweeping up to 2 kHz, over a `tone range` of
  1–6 octaves, with a `tone mode` chooser (LP/HP pair · band-pass · tilt).
  The SVF is `fontelle_dsp::filter`'s, already in the tree. A stutter that
  darkens as it repeats is the difference between a gimmick and a build.
- **Pan.** Bipolar, ±100 %, through the existing equal-power law
  (`fontelle-types/src/pan.rs`). Per-channel, so it is the one lane that is
  not a mono control signal.

**Control rate.** The time lane is evaluated per sample — it is the read
position and a stair in it is audible. Volume, tone and pan are evaluated at
**step rate and ramped across the step**, exactly as the synth's modulation is
(`voice::MOD_STEP`, eight samples; memory `flopsynth-phase1-bones` and
`docs/flopsynth-next.md` §4.2). Eight samples is 6 kHz of control bandwidth,
which is past anything a drawn curve contains.

**Per-lane length.** Each lane carries its own length in ticks (1/4, 1/2, 1,
2, 3, 4, 6, 8 beats, or 1, 2, 4 bars). The original locks both lanes to one
grid; letting a 3-beat volume lane run under a 4-beat time lane gives a
twelve-beat pattern out of two simple curves, and costs one field. The config
also carries a global **`rate`** multiplier (×¼ ×½ ×1 ×2 ×4) that scales every
lane at once — which is "half-time" and "double-time" as one automatable knob,
and is the control people reach for live.

### 4.7 Twelve scenes, and playing them

The bank holds **twelve scenes**, each a full set of four lanes. Which one is
playing is `scene`, a stepped parameter in the config — which means it is
**automatable, MIDI-learnable and recordable** like every other parameter
(INVARIANT 7), and that is the workflow the original is actually used with:
people automate the slot, not the curve.

Twelve because it is an octave. The slot's `notes` edge already exists —
`EffectSlot::notes`, built for Tune (`crates/fontelle-model/src/mixer.rs:68`,
`docs/tune-plan.md` §5) — so pointing it at a channel makes **C to B select
scenes 1 to 12, live, from a keyboard**, with the scene held until the next
note. A `notes mode` chooser: *off* · *select* (note picks the scene) ·
*retrigger* (note also resets the phase, §4.1). Octave-agnostic: the pitch
class picks the scene, so it works on any keyboard.

A scene change mid-sound crosses the same fade §4.4 owns. Nothing else about
a scene change is special: the lanes are read from a different row of the same
array.

---

## 5. The parameters

All of them `ParamSpec`s through `EffectConfig::specs`, readable, writable,
normalisable, automatable, saved (INVARIANT 7 and catalogue rule 3), with a
`sections()` table (rule 11). Nineteen controls, six sections. `MAX_EFFECT_PARAMS`
is 64 (`nodes.rs:847`) — there is room.

**Pattern**

| Address | Kind | Range / positions | Default |
|---|---|---|---|
| `scene` | stepped | 1–12 | 1 |
| `rate` | chooser | ×¼ ×½ ×1 ×2 ×4 | ×1 |
| `sync` | chooser | song · retrigger · free | song |
| `swing` | continuous | 0–100 % | 0 |
| `notes` | chooser | off · select · retrigger | select |

**Time**

| `time` | continuous | 0–100 % (depth: scales every offset) | 100 |
| `smooth` | continuous | 0–50 ms | 12 |
| `quality` | chooser | normal · high | normal |
| `look` | chooser | off · 1 beat · 1 bar | off |

**Volume**

| `volume` | continuous | 0–100 % (depth) | 100 |

**Tone**

| `tone` | continuous | 0–100 % (depth; 0 bypasses the filter entirely) | 0 |
| `tone/mode` | chooser | lowpass/highpass · band · tilt | lp/hp |
| `tone/range` | continuous | 1–6 octaves | 3 |

**Pan**

| `pan` | continuous | 0–100 % (depth) | 0 |

**Output**

| `out` | continuous | −24..+24 dB | 0 |
| `mix` | continuous | 0–100 % | 100 |

A fresh Lapse is a wire (rule 2): the depths are at full but scene 1's lanes
are flat — time at zero, volume at unity, tone and pan off — so it passes the
signal through bit for bit until somebody draws something. That is the right
way round: the depth knobs are there to dial *back* a preset, and a preset is
what somebody will load ten seconds after adding it.

---

## 6. The document, and the edits

`crates/fontelle-types/src/lapse.rs`:

```rust
pub struct LapseBank  { pub scenes: Vec<LapseScene> }          // twelve
pub struct LapseScene { pub name: String, pub lanes: [LapseLane; 4] }
pub struct LapseLane  { pub points: Vec<LapsePoint>, pub length: Tick, pub on: bool }
pub struct LapsePoint { pub at: f64, pub value: f64, pub curve: CurveShape, pub tension: f32 }
pub enum   LapseLaneKind { Time, Volume, Tone, Pan }
```

`at` is 0..1 across the lane's own length and `value` is 0..1 in the lane's
own range (the time lane's range is set by the editor's zoom, §7.4; its value
maps to −2..+1 bars). Normalised because that is what a preset wants: a curve
drawn over one bar is the same curve over two, and a lane whose length changes
should keep its shape.

**The edits are an algebra.** `LapseEdit::apply` does one thing and returns
**the inverse of what it just did**, so `EditLapse` in `fontelle-model` has
nothing to work out and no second copy of the rules — the shape
`WavetableEdit` (`crates/fontelle-types/src/wavetable_edit.rs`) and
`NotepadEdit` both take, for the same reason.

```rust
pub enum LapseEdit {
    AddPoint   { scene: usize, lane: usize, point: LapsePoint },
    MovePoint  { scene: usize, lane: usize, index: usize, to: (f64, f64) },
    RemovePoint{ scene: usize, lane: usize, index: usize },
    SetCurve   { scene: usize, lane: usize, index: usize, curve: CurveShape },
    SetTension { scene: usize, lane: usize, index: usize, tension: f32 },
    Draw       { scene: usize, lane: usize, from: (f64, f64), to: (f64, f64) },  // free-hand: replaces the points under the stroke
    SetLength  { scene: usize, lane: usize, length: Tick },
    SetLaneOn  { scene: usize, lane: usize, on: bool },
    ClearLane  { scene: usize, lane: usize },
    FillLane   { scene: usize, lane: usize, shape: LapseFill },   // §7.4's shape menu
    CopyScene  { from: usize, to: usize },
    RenameScene{ scene: usize, name: String },
}
```

Rules, each of which exists because its absence is a defect:

- **An edit that changes nothing is refused** rather than recorded, so Ctrl+Z
  never walks back through edits that never happened.
- **A drag coalesces into one undo entry** until the window breaks the gesture
  — on pointer-up, on a tool change, on a lane change, on a scene change.
- **Points are kept sorted by `at`** and two points may not share one `at`
  (the second wins, the first is removed): a curve with a vertical segment is
  expressed by a `Stepped` point, not by two points at the same place, and the
  RT evaluator's forward-only cursor depends on it.
- **A lane always has at least one point.** Clearing gives the lane's neutral
  value, not an empty list — an empty lane would have to mean something and
  the two candidates (silence, and the wire) are both wrong half the time.
- **Sixty-four points a lane**, enforced here rather than in the window, and
  the sixty-fifth is refused with a message. The RT grid is fixed-size and a
  silent truncation would be a curve that plays differently from the one on
  screen.

`Session` publishes the realised `LapseGrid` on the channel after every
applied edit (one `publish` per command, not per point), and on a structural
rebuild the channel is opened fresh from the document, exactly as
`EffectControls` is — so the two cannot drift.

---

## 7. The window

**A Flopsynth window**, as asked: `fontelle-ui/src/canvas/lapse.rs` +
`draw_lapse*` in `fontelle-ui/src/render/`, reusing `draw_flop_knob`,
`draw_flop_switch`, `draw_flop_chip` and the theme's inks. Read
`docs/flopsynth-next.md` §3.1–3.3 first and follow it: three knob sizes,
captions in capitals from a caption file, the hover bubble, the knob menu, the
120 px canopy, 15/12/12 px type. **1180×760**, minimum the same (nothing here
shrinks), beside `FLOPSYNTH_SIZE`, `TUNE_SIZE` and `NOTEPAD_SIZE` in
`crates/fontelle-ui/src/layout.rs`.

### 7.1 The shape

```
┌──────────────────────────────────────────────────────────────────────┐
│  LAPSE            ‹ preset bar ›            A/B  INIT  ⟲ ⟳    75 %▾  │  header
├──────────────────────────────────────────────────────────────────────┤
│  the memory: 2 bars of waveform, the read head on it, the playhead    │  canopy 96 px
├──────────────────────────────────────────────────────────────────────┤
│ ┌── TIME ─────────────────────────────────────────────┐ ┌─ scenes ─┐ │
│ │        the grid, the curve, the 45° guides          │ │ 1 ● 2 3 4│ │
│ │                                          −7.0 st    │ │ 5 6 7 8  │ │  editor
│ └─────────────────────────────────────────────────────┘ │ 9 10 11 12│ │
│ ┌── VOLUME ───────────────────────────────────────────┐ └──────────┘ │
│ │                                                     │ ┌─ tools ──┐ │
│ └─────────────────────────────────────────────────────┘ │ ✎ ⟋ ⌐ ⌒ ▦│ │
│ [ TONE ]  [ PAN ]   ← collapsed lanes, one row each     └──────────┘ │
├──────────────────────────────────────────────────────────────────────┤
│  PATTERN        TIME           VOLUME    TONE      PAN      OUTPUT   │  console
│  scene rate     time smooth    volume    tone      pan      out mix  │
│  sync  swing    quality look             mode rng                    │
└──────────────────────────────────────────────────────────────────────┘
```

### 7.2 The canopy is the memory

The one idea in this window that the original does not have, and the reason to
build the tap: **draw the two bars of memory, as a waveform, with the read
head moving over it.** You can see what you are about to grab. A hold's read
head stops dead on the sample it froze; a reverse runs backwards over the
picture; a stutter jumps back three times. Nobody has to be told what the
curve means after watching that once.

`LapseTap` / `LapseFrame` in `crates/fontelle-engine/src/lapse_tap.rs`, the
sibling of `tune_tap.rs`, published per block and read per frame:

- a **peak envelope** of the memory — 512 buckets of min/max, maintained
  incrementally as the ring is written (O(1) a sample; the two-tone
  peak+RMS drawing is memory `clip-waveform-and-fade-polish`'s and
  `audio_clip.rs` already draws one),
- the current **phase**, **read position** and **rate**,
- whether the read is **clamped** to the available history (§4.2), so the
  window can say *"memory: 1.6 of 2 bars"* instead of sounding wrong quietly.

Nothing is computed on the RT thread that is not already being written.

### 7.3 The lane editors

Each lane is a grid: φ across, value up. The **time** lane draws

- the bar/beat grid, at the lane's own length, with the snap division shaded;
- **45° guides** — the freeze slope — faint, at every beat, so a hold is a
  line you trace rather than a number you compute. At the default ×1 zoom the
  freeze is exactly 45°, which is why the vertical range defaults to the
  lane's length (zoom chooser: ×1 ×2 ×4);
- the curve, its points as handles, and the segment's shape drawn honestly
  (a `Stepped` segment as a step, not a ramp);
- a live **rate read-out in semitones** under the cursor and at the playhead:
  `rate = 1 + slope` → `12·log2(rate)`. "−7.0 st" is a thing a musician can
  aim at; "slope −0.33" is not.

The **volume** lane draws a dB scale on the left and the unity line marked.
**Tone** and **pan** are collapsed to a one-row strip until switched on, and
expand to a full lane when they are — an off lane costs one row of chrome and
no attention.

### 7.4 Gestures

One table, the whole interaction:

| Gesture | What it does |
|---|---|
| click on empty grid | add a point there |
| drag a point | move it (snapped; hold Alt for free) |
| drag a **segment** | bend it — this is `tension`, and it is the gesture that gives the field a value (§3.5) |
| right-click a point | the shape menu: linear · exp · log · S · step · hold, and *delete* |
| double-click a point | reset its shape and tension |
| drag with the **pencil** | free-hand, one `Draw` edit per pointer step, one undo entry per stroke |
| drag with the **line** tool | a straight segment from press to release |
| drag with the **hold** tool | lays the 45° freeze from press to release — the one-gesture tape stop |
| drag with the **step** tool | paints steps at the snap division, trance-gate style |
| wheel | **scrolls only** (the rule stands, memory `ux-polish-navigation-never-edits`); Ctrl+wheel zooms the vertical range |
| shift-drag | constrains to horizontal or vertical |
| click a scene chip | switch scenes; drag one onto another copies it; double-click renames |
| the shape menu (per lane) | fill the lane with a named shape: flat, ramp, saw, triangle, N steps, N repeats, the freeze |

Snap division is a chip row: 1/4 · 1/8 · 1/16 · 1/32 · triplets · off.

### 7.5 What the window must not do

- **No wheel edits.** Only scroll.
- **No text shaping in the window layer** (INVARIANT 2). Every number drawn on
  the grid goes through `Labels` like everything else.
- **No per-frame re-ask of anything that costs a revision.** The drag stutter
  of v0.9.0 was the knob marks re-asked per pointer motion (memory
  `clip-waveform-and-fade-polish`); a curve drag will produce one edit per
  motion event and the same trap is waiting. Measure with
  `FONTELLE_TRACE_FRAME=1` and a 1 kHz XTEST stream before believing the drag
  is smooth.

### 7.6 Tests for the window (`fontelle-ui/tests/lapse.rs`)

- `the_whole_lapse_window_fits_the_window_it_opens_at` (1180×760, real view,
  nothing overlapping, nothing below the body).
- `every_control_is_where_the_layout_says` (hit-test every cell centre).
- `a_point_on_the_grid_round_trips_through_the_hit_test` (pointer → (lane, φ,
  value) → pixel, within half a pixel, at three zooms).
- `the_freeze_guide_is_forty_five_degrees_at_unit_zoom`.
- `the_rate_readout_says_minus_twelve_semitones_on_a_half_speed_segment`.
- `a_collapsed_lane_takes_one_row_and_still_hit_tests`.
- `the_memory_draws_nothing_without_a_tap` (an empty tap is not a crash and
  not a flat line at zero).
- a `render_headless` case that dumps the window under `FONTELLE_UI_DUMP`; the
  PR carries the PNG. **It is the test that can see.**

### 7.7 The five lists (do this first, in phase 3)

`app.rs` keeps one `Option<…View>` per effect window and every list that
touches them has to name all of them. With Lapse there are **five**, and the
notepad's three-list omission (`docs/handoff.md` §1) is a whole class of bug
waiting to happen again. Answer every one of these:

| Where | What it does | `app.rs` |
|---|---|---|
| `refresh_studio`'s view fetch | fills the five views from the document | `:5805` |
| `refresh_studio`'s "slot is gone" | clears `open_insert` — **all five** | `:5845` |
| `refresh_studio`'s "subject is gone" | closes the window — **all five** | `:5876` |
| `create_editor` | picks the window's size from what is in the slot | `:4334` |
| every `EditorKind::Effect` dispatch arm | key, press, wheel, drag, scene | `:4878`, `:5042`, `:4768`, `:11604`, `:5409` |

**Recommended, and Ty's call (§14.3): collapse the five `Option`s into one
`enum EffectWindow { Generic(InsertView), Eq(EqConfig), Tune(TuneView),
Notepad(NotepadView), Lapse(LapseView) }` first.** It makes the class of bug
unrepresentable — a `match` the compiler checks instead of five `is_none()`
calls somebody has to remember — and it is about forty mechanical sites in
`app.rs`. Do it as its own commit, with the suite green before and after, or
not at all.

---

## 8. The bank

Rule 10: presets are files in the DAW-wide bank, constructors in the tree
exported by `cargo xtask export-factory-presets`
(`xtask/src/presets.rs:76`), a slug in `preset.rs`, and
`fontelle-app/tests/preset_bank.rs` and `effect_editor.rs` check **the files,
not the recipes**.

**One piece of plumbing first.** `PresetPayload::Effect(EffectConfig)`
(`crates/fontelle-types/src/preset.rs:155`) carries a config and nothing else,
and a Lapse preset with no curve in it is not a preset. Add a **new variant**
— `PresetPayload::Lapse { config: LapseConfig, bank: LapseBank }` — rather
than adding a field to the existing one: a newtype variant that grew a field
would change the JSON shape of every effect preset file in the factory tree
*and* in every user's bank. A new variant leaves all of them untouched and
costs one `match` arm in each of the handful of places that read a payload.
`PRESET_FORMAT_VERSION` does **not** move; nothing old becomes unreadable.

**Sixty-four presets in nine categories**, plus six kits. Categories are a
free string on `Preset` and the browser groups by them; effects have all used
`"Factory"` until now because their presets are points on one control surface.
Lapse's are not — a scratch and a sidechain pump have nothing to do with each
other — so it is the first effect with real categories, and that is an
argument for it rather than against.

| Category | n | The rows |
|---|---|---|
| **Stutter** | 8 | 1/4 Roll · 1/8 Roll · 1/16 Roll · Triplet Roll · Accelerando · Ratchet 3 · Ratchet 5 · Buzz |
| **Hold** | 8 | Beat 1 · Half Bar · Last 1/8 · Freeze & Release · Stutter Hold · Hold Fade · Suspend · Glitch Hold |
| **Scratch** | 8 | Baby · Forward · Chirp · Transformer · Tear · Rub · Stab · Wikki |
| **Tape** | 6 | Tape Stop · Tape Start · Slow Dive · Speed Up · Half Speed · Double Speed |
| **Reverse** | 6 | Reverse Beat 4 · Reverse 1/8 · Reverse Bar · Rewind · Backspin · Reverse Tail |
| **Gate** | 8 | 1/8 Gate · 1/16 Gate · Trance 16 · Offbeat · Triplet Gate · Sidechain Pump · Long Pump · Breathe |
| **Groove** | 8 | Swing 16 · Shuffle · Push · Drag · Half-time · Double-time · Laid Back · Rushed |
| **Fill** | 6 | Bar Fill · Riser Gate · Drop Out · Build Stutter · Last Beat Roll · Silence & Return |
| **Creative** | 6 | Ghost Notes · Broken Tape · Hiccup · Time Melt · Drunk · Quantum |
| **Kits** | 6 | Scratch Kit · DJ Kit · Roll Kit · Stop Kit · Gate Kit · Groove Kit — **twelve scenes each**, laid out C to B so the whole kit is playable from an octave of a keyboard |

**Groove is the general-purpose category and the one to get right.** Eight
curves of ±30 ms of push and drag, applied to *audio* — a groove template for
a bounced loop, which is a thing no plugin in this class offers because none
of them thought of the time lane as micro-timing. It is four points and a
tiny depth, and it is the one people will use on every project.

Writing sixty-four curves as Rust literals wants a constructor DSL or it will
be unreadable: a `lane!` macro taking `(at, value, shape)` triples, in
`fontelle-types/src/lapse_presets.rs` beside `effect_presets.rs`. Every preset
carries a one-sentence `notes` string ("*what it is for*"), which the browser
already shows.

**The gates on the bank** (`fontelle-app/tests/preset_bank.rs`):

- every preset's file exists after the export, and round-trips;
- every preset is **audibly not the wire** — render a bar of drums through it
  and require the output to differ from the input by more than a threshold.
  The one that fails this is the bug, every time;
- no two presets in a category are the same curve (the pairwise gate the synth
  bank already uses, on the points rather than on the sound);
- every kit fills all twelve scenes, and no kit has two identical ones.

---

## 9. The plumbing, by crate

In dependency order, which is also the build order — each crate's tests can be
green before the one above it exists.

**`fontelle-types`** — `curve.rs` (`CurveShape` moved here, `eased` with
tension, §3.5). `lapse.rs`: the bank types, `LapseGrid` + `From<&LapseBank>`,
`LapseEdit::apply`, `LapseConfig` with `new()`, `get`/`set` by id,
`LAPSE_PARAMS`, `LAPSE_SECTIONS`, the five enums with `label`/`ALL`/position
tables, `LapsePreset::ALL` + `from_preset`. `effect.rs`: the `EffectKind`
variant, `label`, `ALL`, `takes_notes`, the `EffectConfig` arm.
`preset.rs`: the `fx-lapse` slug and the `PresetPayload::Lapse` variant.
`event.rs`: `TempoSegment`, `tick_at`, `beats_per_bar` (§3.4).

**`fontelle-dsp`** — nothing new but a wrapping, quality-aware ring read if
it belongs here rather than in `fontelle-fx/src/lines.rs`. Prefer `lines.rs`:
it is where the other delay-line reads live.

**`fontelle-fx`** — `lapse.rs`: `Lapse::new`, `prepare`, `reset`, `process`,
`latency_samples`, `tap_frame`. Owns the ring, the two read heads, the four
lane cursors, the SVF pair and the envelope buckets. `lib.rs` exports it.

**`fontelle-sequencer`** — `compile.rs:438`: the tempo table becomes
segments carrying their tick and rate; `beats_per_bar` onto the timeline.

**`fontelle-engine`** — `transport.rs`: the three snapshot fields.
`lapse_channel.rs` and `lapse_tap.rs` (§3.3, §7.2). `nodes.rs`: the
`EffectState::Lapse` arm (new/prepare/process/reset), `MusicalTime` built from
`ctx.transport`, `with_lapse`, `with_lapse_tap`, the second arm of
`insert_latency_samples` and the `max_insert_latency_samples` the dry line is
sized from. `lib.rs` exports.

**`fontelle-model`** — `mixer.rs`: `EffectSlot.lapse`. `commands.rs`:
`EditLapse` (the algebra's inverse, coalescing), `SetInsertScene` if the scene
chips do not go through the ordinary parameter road (they should — it is a
`ParamSpec`). `automation.rs`: `CurveShape` re-exported from types.

**`fontelle-app`** — `lapse.rs`: `describe(config, bank, tap) -> LapseView`
and `shape_of`, the layer allowed to see both halves (INVARIANT 4), sibling of
`tune.rs`. `realise.rs`: the channel, the tap, `with_lapse`. `session.rs`: the
host methods, the `lapse_taps` map, the publish after every edit, the
re-prepare on a lookahead change.

**`fontelle-ui`** — §7.

**`xtask`** — `presets.rs`: the `LapsePreset` loop and the kits.

---

## 10. Tests, written first

Each file is written, run and **seen to fail** before its implementation. The
names are the claims. A test that would pass against the effect with the
feature stripped out is not a test of the feature (catalogue rule 12).

### 10.1 `fontelle-sequencer/tests/tempo_table.rs`

- `the_tick_at_a_sample_is_the_tick_the_tempo_map_says` (against
  `TempoMap::sample_to_tick`, at three tempos).
- `the_tick_is_right_across_a_tempo_change` — the case a
  `position_sample × bpm` derivation gets wrong, and the reason §3.4 exists.
- `an_uncompiled_timeline_answers_tick_zero_and_four_beats_a_bar`.

### 10.2 `fontelle-fx/tests/lapse.rs`

The measures are `tests/common/mod.rs`'s (sines, impulses, a windowed DFT,
RMS × √2 for a level).

- `a_flat_curve_is_a_wire` — bit for bit, at every quality and every
  smoothing.
- `a_freeze_repeats_the_frozen_window_sample_for_sample` — a ramp in, the held
  slice out, and the release lands on the song's own sample, not on the
  buffer's.
- `a_half_slope_segment_is_an_octave_down` — a 440 Hz sine reads 220 in the
  DFT during the segment. **Two traps** (memory `performance-events`): measure
  inside the segment and not across its edges, and window long enough to
  resolve the shift.
- `a_negative_slope_plays_it_backwards` — an asymmetric transient comes out
  mirrored.
- `a_jump_with_smoothing_has_no_step_above_the_threshold`, and
  `a_jump_without_smoothing_does` — the knob is a knob you can hear.
- `the_volume_lane_silences_within_its_smoothing_and_unity_is_untouched`.
- `the_tone_lane_at_zero_depth_is_bit_identical` (an off filter is off, not a
  filter at a neutral setting).
- `the_read_clamps_to_the_history_it_has` — §4.2's first-bar rule.
- `lookahead_reports_its_latency_and_does_not_change_it_while_running`.
- `a_positive_offset_under_lookahead_plays_the_impulse_early`.
- `the_lanes_run_at_their_own_lengths` — a 3-beat volume lane under a 4-beat
  time lane repeats on 12.
- `swing_moves_the_offbeat_and_leaves_the_downbeat`.
- `high_quality_aliases_less_than_normal_at_double_rate` — the measure is
  `synth_alias.rs`'s: energy off the grid against energy on it.
- `a_stopped_transport_still_runs_the_pattern`.
- `every_curve_shape_does_something_and_they_are_not_all_the_same_thing`
  (rule 1's test, by name).

### 10.3 `fontelle-types/tests/lapse.rs` and `parameters.rs`

The generic parameter tests cover the config the moment the kind is in `ALL`
(addressability, normalisation, round-trip). Named tests only for the rules
that are this effect's:

- `an_edit_undoes_itself` — `apply` then `apply` the returned inverse, for
  every variant, and the bank is equal.
- `an_edit_that_changes_nothing_is_refused`.
- `points_stay_sorted_and_no_two_share_a_position`.
- `a_lane_always_has_a_point`.
- `the_sixty_fifth_point_is_refused`.
- `the_grid_is_the_bank_clamped_to_the_cap`.
- `tension_zero_is_exactly_todays_eased` — the automation lane may not change
  shape.
- `the_slug_is_fx_lapse` (the frozen-strings table).

### 10.4 `fontelle-engine/tests/effects.rs` (additions) and `lapse_node.rs`

- The generic sweep covers the kind the moment it is in `ALL` — including the
  440 Hz-tone-through-every-effect case, which a flat Lapse passes by
  definition.
- `the_pattern_is_where_the_song_is` — render one block at bar 1 and one at
  bar 37 with the same curve; the output is the same. **This is the test §3.4
  exists for.**
- `a_tempo_change_moves_the_pattern_with_it`.
- `the_curve_crosses_on_its_own_channel_without_a_rebuild`.
- `the_tap_says_what_the_read_head_is_doing`.
- `the_dry_is_delayed_by_the_lookahead` — the gate's own case, for the second
  insert that has one.

### 10.5 `fontelle-model/tests`

- `a_drag_is_one_undo_entry_and_a_pointer_up_breaks_it`.
- `a_lapse_slot_opens_with_twelve_flat_scenes_and_a_notepad_slot_does_not`.
- `a_project_written_before_lapse_opens_and_writes_back_unchanged`.

### 10.6 `fontelle-app/tests/lapse_editor.rs`, `preset_bank.rs`

§8's four gates, plus the preset bar's dirty `*` and the undo entry per preset
click that every device already has.

### 10.7 `fontelle-ui/tests/lapse.rs`

§7.6.

---

## 11. Performance budget

Per sample, per channel, worst case: one 8-tap sinc read plus a second one
during a fade (16 multiply-adds), a volume multiply, an SVF (about 10 flops),
a pan pair. Call it 40 flops — 3.8 MFLOP/s stereo at 48 kHz, which is nothing.
The curve evaluation is one compare and one `eased` per sample for the time
lane and one per eight samples for the other three.

The costs that are real and want watching:

- **Memory**: 4.6 MB stereo at 48 kHz per insert (§4.2). Ten Lapses is 46 MB.
  Acceptable, and worth a line in the window's read-out so it is not a
  surprise.
- **The envelope buckets**: O(1) a sample, but it is a scatter write into a
  512-entry array — keep it in the same loop as the ring write, not a second
  pass.
- **The `LapseGrid` publish**: 50 KB per edit. At a drag's sixty edits a
  second that is 3 MB/s on the **UI** thread, which is fine, and it must never
  be done per frame — only per applied edit.
- **The frame cost**: the editor draws up to four lanes of up to sixty-four
  segments plus a 512-bucket waveform. `FONTELLE_TRACE_FRAME=1`'s `arrange
  edit` line is the check, and the bar is a fraction of a millisecond
  (`docs/flopsynth-next.md` §0.6).

---

## 12. Phases

Each is a commit or a small run of them, tests first, suite green at the end
of each.

**Phase 0 — the clock (half a day).** §3.4 alone: the tempo table's segments,
`tick_at`, `beats_per_bar`, the three snapshot fields, §10.1. No effect yet.
It lands on its own because it is the piece most likely to reveal something
unpleasant, and because it is useful without the rest.

**Phase 1 — the machine (two days).** §4 in `fontelle-fx` against a
hand-built `LapseGrid`, with §10.2 written first. Everything measured here is
measured without a window, a document or a preset. Do not move on until the
freeze is sample-exact and the octave is an octave.

**Phase 2 — the document and the graph (a day and a half).** §3.2, §3.3, §5,
§6: the types, the slot field, the channel, the tap, the node arm, the edit
algebra, the command, the realise layer. §10.3–10.5. At the end of this phase
a Lapse can be added to a strip, automated and undone — with no window.

**Phase 3 — the window (three days, the biggest).** §7, beginning with §7.7's
five lists (and the `EffectWindow` enum if Ty says yes). The lane editor, the
tools, the scene chips, the canopy. **Open it on `:99` before believing any
of it**, and dump a PNG from `render_headless`.

**Phase 4 — the bank (a day and a half).** §8: the payload variant, the `lane!`
macro, sixty-four presets and six kits, the export, the four gates. Expect two
rounds: the first pass of any bank in this project has always had rows that
measure alike (memory `flopsynth-preset-bank`).

**Phase 5 — use it, then fix what using it found.** Play a loop through every
preset, on `:99`, with a drum bounce and with a vocal. Every defect from that
session is a test first. This phase is not optional; every chunk in
`PROGRESS.md` that skipped it produced a report a day later.

Then: `PROGRESS.md`'s top section, `docs/handoff.md`, the catalogue's §2.4 row
and the two rows it absorbs, and a version bump — the tag and the release are
Ty's (§0.8).

---

## 13. Deliberately not built

- **Anything above two bars of memory.** Four bars doubles the RAM for a
  feature that is a loop, and a loop is a clip.
- **A per-voice or per-patch Lapse.** Not in `PATCH_FX_KINDS`: an instrument's
  own chain has no bar clock plumbed to it, and the memory cost is per
  channel. The insert is where this belongs.
- **Spectral time-stretch.** The rate *is* the pitch here, deliberately — that
  is the sound of the thing. Somebody who wants tempo without pitch has the
  clip's own stretch (TDD §15) and, eventually, `PsolaShifter`.
- **A sidechain key.** It has no detector. A volume curve is not a
  compressor, and the effect that ducks off another track is the ducker
  (catalogue §2.1).
- **Curve automation from a lane.** An automation lane drawing *this* effect's
  curve would be two curve editors for one curve, and an insert reaching for
  another insert's shape is the second addressing scheme §8.2 forbids. The
  scene index is the automatable handle, and twelve scenes is the answer.
- **MIDI-learned per-point editing.** No.
- **More than twelve scenes.** Twelve is an octave and an octave is the
  keyboard mapping. Thirty-six would need three.

---

## 14. Decisions for Ty, before phase 0

1. **The name.** *Lapse*, or *Flopbeat*, or something else. It is a folder
   name and therefore permanent after the first release (§2).
2. **The two extra lanes** (Tone and Pan, §4.6). They are off by default and
   cost nothing when off; they are also two lanes the original does not have,
   and a window with four lanes is a busier window than one with two.
3. **The `EffectWindow` refactor** (§7.7). Recommended, and it touches about
   forty sites in `app.rs` before any of this is built.
4. **Categories in the preset browser for one effect** (§8). Every other
   effect uses `"Factory"`; Lapse would be the first with nine categories, and
   that changes how the Presets page looks for it.
5. **Lookahead** (§4.5). It is the best idea in this plan and it is also the
   one that makes an insert non-zero-latency, which is a thing people notice
   when they wonder why their track moved. Default is Off either way.
6. **Sixty-four presets plus six kits** is a day and a half of voicing. If
   that is too many for the first release, the nine categories cut cleanly to
   about thirty-five, and Groove is the one to keep.
