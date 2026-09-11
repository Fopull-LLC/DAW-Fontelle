# Tune: the built-in pitch corrector

**Status, 2026-09-09: built, to the end of this plan** — `PROGRESS.md`'s
2026-09-09 entries ("the autotune is built, and it opens on a console" and
"the autotune got a colour, a bank and a keyboard that is a keyboard") say
what came of it; the plan below is kept as it was written, because it is why
the family is shaped the way it is. As written on 2026-09-08 it began:
*planned, nothing built.* This is the design for the autotune insert, written to be followed the way `docs/flopsynth-plan.md` and
`docs/effects-catalogue.md` §3 were followed: tests first and confirmed
failing, one crate at a time, in the order §12 gives. Read
`docs/effects-catalogue.md` §1 (the twelve rules) and §5 (the recipe) before
this; every decision below is one of those rules applied to a pitch corrector,
or a reason written down for departing from one.

**The brief** (Ty, 2026-09-08): *"a built in autotune plugin ... stack up to
other professional autotunes or be able to sound like a cheap autotune ...
very flexible at getting different types of autotune sounds. a lot of autotune
plugins kind of lock you into the style of how that plugin sounds ... very
flexible, very configurable, high quality, easy to control, ability to control
with midi or by selecting the notes in your scale on the keyboard that should
be shown in it. there should be presets for the different sounding autotune
presets ... visually the plugin should be robotic and futuristic like the
interior of a scifi spaceship."*

Read that as four requirements, each of which is a section here:

1. **One family, many sounds** (§3, §4). The reason plugins "lock you in" is
   that the *character* of an autotune — the thing that makes one sound
   expensive and another sound like a toy — lives in decisions the plugin
   made for you: how it finds pitch marks, how wide its grains are, whether
   formants move with the pitch, how the correction glides in at a new note.
   Every one of those is a control here, with a chooser for the *kind* and a
   knob for the continuum (catalogue rule 1).
2. **Two ways to say what the notes are** (§4.2, §5): a scale on a keyboard
   drawn in the window, and MIDI from any channel in the rack — held keys as
   the melody to force, or held keys as the scale.
3. **Presets for the sounds people mean** when they say "autotune" (§6):
   transparent, pop, hard-tuned, robot, cheap plastic, chipmunk, MIDI melody.
4. **A window that looks like a ship's console** (§7), built with the same
   pieces Flopsynth's window is built from — cards on a ground, glowing arcs,
   a picture computed from the numbers the sound reads — with the ground and
   the inks changed and one big instrument in the middle: the pitch trace.

---

## 0. Ground rules for this work

- **Tests first, confirmed failing, then the implementation.** The project's
  standing rule (`PROGRESS.md` top, `docs/handoff.md` §2). Write the test
  against the API in this document, see it fail (a compile error against a
  missing function counts), then build. Never both in one edit.
- **The invariants are hard.** INVARIANT 1: nothing in `process` allocates —
  every ring, grain buffer and held-key table is sized in `prepare` or is a
  fixed array. INVARIANT 4: `fontelle-dsp` and `fontelle-fx` depend on
  nothing above them. INVARIANT 7: every parameter id and the preset folder
  slug are permanent from the first commit that carries them.
- **Loop on what you touched** (`fast-workflow-here` in the memory,
  `docs/handoff.md`): `cargo test -p fontelle-dsp --test pitch`, `-p
  fontelle-fx --test tune`, `-p fontelle-ui --test tune`. The workspace suite
  runs once, in the background, at the end. Never two cargo runs at once.
- **Look at the window** before believing a layout test: `FONTELLE_UI_DUMP=
  <dir> cargo test -p fontelle-ui --test render_headless` for the scene, and
  the real binary on `Xwayland :99` (the memory's `seeing-fontelles-gui`).
  The Flopsynth window's first build passed every layout test and opened
  with the noise card wrapped off the bottom; the screenshot found it.
- **Listen once.** Nothing in §9 can hear whether the hard-tuned preset
  sounds like a record. `cargo run --release -p fontelle-app -- --render-wav`
  through a project with a vocal clip and this insert is the fourth column.
- **`cargo clippy --workspace --all-targets -- -D warnings` stays clean.**

---

## 1. What "done" means

Every one of these is a test in §9 or a screenshot in §7.7, and the effect is
not done until all of them hold:

1. A 30-cent-flat sung A on a fresh insert comes out at A within 3 cents,
   and a fresh insert on speech leaves the speech alone.
2. Retune speed at 0 ms snaps within two analysis hops; at 200 ms the
   correction is 63 % of the way in 200 ms ± 10 %. The two are audibly the
   hard-tune sound and the transparent one.
3. A synthetic vowel shifted a fifth keeps its formant peaks within 5 % on
   the Smooth engine with formant follow at 0 %, and moves them by the ratio
   at 100 %. Same signal, two different sounds, one knob.
4. The Grain engine puts a measurable modulation at the grain rate into a
   steady tone that the Smooth engine does not. The "cheap" sound is real,
   not a preset name.
5. In C major a 470 Hz tone goes to B; with only C and G enabled it goes to
   G; with D as the root F# is in and F is out; and every one of those is
   reachable by clicking keys on the window's keyboard, one undo entry per
   click.
6. With E4 held on a channel the insert listens to, a sung C4 comes out as
   E4; letting go returns to the scale; the last key held wins. From the
   timeline *and* from a keyboard being played live.
7. The insert reports the latency it costs, the graph compensates it (an
   impulse through a track with the insert lands with its siblings), and the
   mix control at 50 % on a wire null does not comb.
8. Sixteen factory presets, every one away from the wire and from each
   other, in the bank and on the preset bar.
9. The window fits at 960×600 with every control hit-testable, the trace
   draws the sung and the corrected pitch against the scale's rails, and
   it looks like the brief — a screenshot is in the PR.
10. `process` allocates nothing (the generic engine test covers it once the
    kind is in `EffectKind::ALL`), and a stereo insert in Studio mode costs
    under 1.5 % of one core at 48 kHz (§10).

---

## 2. Where it sits: the four decisions

### 2.1 It is an insert, `EffectKind::Tune`

An ordinary row in the "+ Add effect" menu, one `EffectConfig` variant, one
`EffectState` arm, one `fontelle_fx` module — the catalogue's §5 recipe. It
runs on a mixer track's bus, so it corrects a recorded vocal clip, a live
input being monitored through that track (`MonitorNode` → chain), a sampled
instrument, or a plugin's output alike. Nothing about it is a special case of
the graph.

**Label** on the strip: `"Tune"`. **Slug** for the preset folder: `fx-tune`
(`DeviceKind::slug`, frozen by `the_table_of_device_slugs_is_frozen`). The
window's heading says "TUNE · pitch correction". The name is a decision Ty can
change **before phase 0 lands** and not after: the kind serialises by name in
every `project.json` and the slug is a folder everybody's presets live in
(INVARIANT 7). "Autotune" itself is a trademark and is not used in code.

### 2.2 The primitives go in `fontelle-dsp`, the corrector in `fontelle-fx`

The pitch tracker and the pitch-synchronous shifter are *primitives*: the
catalogue's tuner meter (§2.5), the harmoniser and the vocoder (§2.6) and a
future shimmer on the reverb all want them. They go in
`fontelle-dsp/src/pitch.rs` and `fontelle-dsp/src/psola.rs` beside the SVF and
the wavetables, with no knowledge of scales, MIDI or knobs. The corrector —
what to do with a pitch once it is known — is `fontelle-fx/src/tune.rs`, which
reads a `TuneConfig` every block and owns only state.

### 2.3 Notes reach the insert the way a sidechain key does

`EffectSlot.key: Option<MixerTrackId>` is a routing edge on the slot rather
than a parameter, because a track is not a float with a fixed range
(catalogue §6). A channel is not either. So the source of notes is
**`EffectSlot.notes: Option<ChannelId>`**, set by a `SetInsertNotes` command
that refuses it on any effect whose new `EffectKind::takes_notes` is false,
and cleared when the channel is removed. The engine side is one field on
`EffectNode` — the `NodeId` of that channel — and the node reads that node's
note events out of `ProcessContext::all_events` and `live_events`. No
compiler change, no duplicated events, and the on-screen keyboard and a MIDI
controller reach it for free because they already reach the selected
channel's node. §5 in full.

### 2.4 It is the first insert with a latency that depends on its settings

The gate's look-ahead is a knob; this effect's latency is a *function of the
range and the mode* (§3.8). Everything TDD §5.5 built handles it —
`insert_latency_samples` for the document's answer, `EffectNode::
latency_samples` for the node's, `DelayNode` padding for the siblings, the
dry line for the mix — but `EffectNode::prepare` sizes its dry line only for
the gate today. That match becomes a `max_insert_latency_samples(config,
sample_rate)` helper beside `insert_latency_samples`, and this is the second
arm of both.

---

## 3. The sound architecture

### 3.1 The path

```text
                 ┌───────────────── analysis, once per hop (32/64 samples) ─────────────────┐
in ──► mono sum ─► decimate ÷4 ─► YIN coarse ─► refine at full rate ─► median(3) ─► voiced? ─┐
  │                                                                                          │
  │        MIDI notes (§5) ─┐                                                                ▼
  │                         ▼                                              slow / fast split (§3.3)
  │   scale mask ─► target chooser ─► correction goal ─► retune glide ─► strength ─► out cents
  │                                          ▲                                      │
  │                        humanize, flex, amount                                   ▼
  │                                                       ratio per sample = 2^((out−in)/1200)
  ▼                                                                                 │
delay line (latency L) ──► PSOLA / Grain / Hard shifter, per channel, shared marks ◄┘
                                     │
                                     ▼
                            formant resample ─► output gain ─► (EffectNode blends the dry)
```

Detection runs on the mono sum. Shifting runs per channel with **one** mark
schedule, so a stereo source keeps its image (§3.7). Everything above the
delay line is control-rate; everything below is per sample.

### 3.2 Pitch detection (`fontelle-dsp/src/pitch.rs`)

**`PitchTracker`**, YIN with the cumulative-mean-normalised difference
(de Cheveigné & Kawahara 2002), in two passes because the plain difference
function is `O(W·τ)` and at a 64-sample hop that is a tenth of a core:

- **Coarse pass at fs/4.** A 4th-order Butterworth low-pass at fs/10 (two
  cascaded SVF low-passes at Q 0.54 and 1.31) then every fourth sample into a
  ring. Window `W₄ = 2·P_max/4`, lags `τ ∈ [P_min/4, P_max/4]`. Difference
  `d(τ) = Σ (x[n] − x[n+τ])²`, normalised `d'(τ) = d(τ) · τ / Σ_{j≤τ} d(j)`.
  The candidate is the **first** local minimum of `d'` under the threshold,
  or the global minimum if none is (YIN's step 4 — the rule that stops
  octave-up errors).
- **Refine at full rate.** `d'` over `τ ∈ [4τ₄ − 6, 4τ₄ + 6]` with `W = 2·4τ₄`
  (clamped to `2·P_max`), then parabolic interpolation on the three lags
  round the minimum for a fractional period. Confidence `c = 1 − d'(τ*)`.
- **Voiced** when `c > threshold` **and** the window's RMS is over the gate.
  `threshold` is the `tracking` knob (§4.1) mapped 0.30 relaxed → 0.08 strict
  onto the normalised difference.
- **Median of three** hops on the period, and **continuity**: if the new
  candidate is within 30 cents of double or half the previous voiced pitch
  and the previous confidence was over 0.8, and the previous period's own
  `d'` at this hop is also under the threshold, keep the previous. Two hops
  of disagreement override it. This is what stops a breathy consonant
  flipping the tracker an octave for one hop.
- **Hop** `H`: 64 samples in Studio mode, 32 in Live (§3.8). The tracker
  only ever looks *backwards*, so it adds no latency of its own — only
  reaction time, which is one hop plus the median's one-hop lag.
- **Output per hop**: `Option<PitchFrame { hz, cents, confidence, rms }>` —
  `None` is unvoiced. `cents` is MIDI cents: `6900 + 1200·log₂(hz/440)`.

Sample rates 44.1 k, 48 k and 96 k are all first-class: the ranges below are
in hertz and everything in samples is derived in `prepare`.

### 3.3 The pitch track: from a detected pitch to a target

Once per hop, in this order. Every step is a claim in
`fontelle-fx/tests/tune.rs`.

1. **Split slow from fast.** `slow` is a one-pole low-pass of `cents` with
   τ = 70 ms (≈ 2.3 Hz), `fast = cents − slow`. `slow` is *the note being
   sung*; `fast` is the vibrato and the scoop. Both are frozen (not updated)
   on an unvoiced hop.
2. **Onset.** A new note is an unvoiced→voiced edge, or `slow` jumping more
   than 80 cents between hops. An onset resets the retune glide (step 5),
   the humanize settle (step 6) and the added vibrato's onset clock (§3.5).
3. **Choose the target** from `slow`, by `control`:
   - *Scale*: the nearest **enabled pitch class** (§4.2), with 15 cents of
     hysteresis — the current target is kept until another enabled note is
     nearer by more than that, so a note sung on the boundary does not
     flip-flop.
   - *MIDI melody*: the last key held on the source channel (§5), plus the
     channel's pitch bend × `midi_bend`. No key held: fall back to *Scale*.
   - *MIDI scale*: the held keys' pitch classes are the mask while any is
     held; the scale otherwise.
   The target is in cents, at the octave nearest `slow` (for a pitch class)
   or exactly the key (for a melody).
4. **Strength.** `s = amount · flex_s · humanize_s`, with
   - `flex_s = 1 − smoothstep(edge, 50, |target − slow|)` where
     `edge = 50·(1 − flex)`. At `flex` 0 the edge is 50 cents and every
     pitch is fully corrected; at 100 a note 45 cents off is nearly left
     alone — the expressive slide survives. (Melody control ignores flex: a
     forced note is forced.)
   - `humanize_s` from step 6.
5. **Retune glide.** The goal is `g = (target − slow) · s`. The applied
   correction `corr` is a one-pole toward `g` with time constant
   `retune_ms`, run **per sample** in the shifter's ratio smoother; at 0 ms
   it is the goal. At an onset `corr` restarts from **zero** — the glide is
   from the note as sung to the note as wanted, which is what "retune speed"
   means on every record that has one, and the *yodel* between notes on a
   hard-tuned vocal is exactly this restart at 0 ms.
6. **Humanize.** `settled` ramps 0→1 over 300 ms while `|Δslow|` per hop is
   under 3 cents, and is reset at an onset. `humanize_s = 1 − humanize ·
   settled`. A sustained note drifts back toward where it was sung by up to
   the knob; a moving line is corrected in full.
7. **Output pitch** `out = slow + corr + fast · natural_vibrato + vib(t) +
   100 · transpose + detune`. `natural_vibrato` at 100 % passes the singer's
   own vibrato through untouched; at 0 it is flattened, which with retune at
   0 is the robot.
8. **The ratio** handed to the shifter is `2^((out − cents)/1200)` — against
   the *instantaneous* detected cents, so the shifter is exact even while
   `slow` lags. Between hops the ratio is interpolated per sample; over an
   unvoiced stretch it relaxes to 1 over 5 ms.

### 3.4 The shifter (`fontelle-dsp/src/psola.rs`) and its three engines

**`PsolaShifter`** is time-domain pitch-synchronous overlap-add — the
algorithm behind every low-latency vocal tuner, chosen over a phase vocoder
because it has a latency of a period rather than a frame, no phasiness, and
keeps the spectral envelope by construction.

- **Marks.** An *analysis* mark advances through the delayed input by the
  detected period `P_in` (fractional, accumulated in `f64`); a *synthesis*
  mark advances through the output by `P_out = P_in / ratio`. Each synthesis
  mark takes the analysis mark nearest it in time and lays that mark's grain
  — the input windowed over `2·P_in` centred on the mark — at the synthesis
  position. Ratio over 1 repeats grains; under 1 skips them. Marks are
  *synthetic* (spaced by the period from a running phase) rather than
  glottal-closure estimates: a real-time tuner has no look-ahead to find
  closures with, and the Hann window's width makes the phase of the mark
  within the period inaudible.
- **Unvoiced is the same algorithm at ratio 1.** With `P_in` held at the
  last voiced period (or 5 ms if there has been none) and `P_out = P_in`,
  Hann grains two periods wide at one-period spacing are a constant-overlap-
  add identity: the output *is* the delayed input. No crossfade, no mode
  switch, no click at a consonant.
- **Normalisation.** A second ring accumulates the window sum; the output is
  divided by it (floored at 0.25). Ratio ≠ 1 breaks exact COLA and this is
  what keeps the level flat through a glide.
- **Rings.** One input line per channel, capacity `4·P_max + max_block`,
  flat like `fontelle_fx::Gate`'s; one output OLA ring and one window-sum
  ring, same capacity. All sized in `prepare`.
- **Latency** `L` is fixed per range and mode (§3.8), and the synthesis
  clock runs `L` behind the input so a grain that reaches one period past
  its mark is always available.
- **Interpolation.** Grains are read at fractional positions through
  `fontelle_dsp::interpolate` at `Interpolation::Normal` (Hermite). `Ultra`
  is not needed: the source is already band-limited and the formant read
  (§3.5) is the only upward resample, bounded at ×2.

**Three engines**, a chooser (`engine`) each with its own reading of the
`texture` knob — the catalogue's curve-and-shape shape:

| Engine | What it is | `texture` 0 → 100 |
|---|---|---|
| **Smooth** | PSOLA as above: Hann, `2·P_in`, pitch-synchronous. The transparent one and the modern hard-tune. | Grain width `2.0·P` → `1.2·P`. Narrower grains harden the edges of a fast retune without breaking synchronism. |
| **Hard** | Pitch-synchronous, but a Tukey window (flat top) over `1.5·P`. Not quite COLA: a faint buzz at the period rate rides the sustain — the *metallic* edge people mean by "that autotune sound". | Taper 50 % → 5 %. At 5 the grains are nearly rectangular and the buzz is a rasp. |
| **Grain** | **Not** pitch-synchronous. Fixed grains of `grain` ms at half-grain spacing, Hann, read from the input at the ratio — a plain granular varispeed. Phasy, smeared, modulated at the grain rate: the cheap plug-in, and a sound in its own right. | Random jitter on grain positions, 0 → ±50 % of a grain. At 0 the modulation is a clean comb at the grain rate; at 100 it is a cloud. |

Every engine reads the same `ratio` and the same formant factor, so switching
engines changes *only* the character, which is the point.

### 3.5 Formants

The spectral envelope of a grain is moved by reading the grain at a stride
`F` before windowing — a grain read at `F = 1.5` has its formants a fifth
higher while the marks, and therefore the pitch, are unchanged. So:

- `F = 2^(formant/12) · ratio^(formant_follow)`.
- `formant_follow` at 0 % keeps formants where the singer put them (the
  expensive result); at 100 % they move with the pitch, which is exactly a
  resampler's chipmunk, on **any** engine. It is a knob rather than a switch
  because half-following is a sound of its own — a small voice that is still
  recognisably the singer.
- `formant` (±12 semitones) is the throat-length control: gender, size,
  cartoon. It applies with the correction, so a "deep voice" preset is one
  preset and not a chain.
- The read is bounded to `F ∈ [0.5, 2.0]`; a grain read faster than ×2 is
  an aliasing question this build does not answer, and the knob's range
  says so.

**Added vibrato** `vib(t)` is a sine or triangle (`vibrato_shape`) at
`vibrato_rate` Hz — or a `NoteDivision` when `vibrato_sync` is on, read off
`TransportSnapshot::bpm` like the chorus's LFO (rule 5) — with `vibrato_depth`
cents, starting `vibrato_onset` ms after each note onset and fading in over
100 ms. It is added in cents *after* the correction, so a robot can have a
perfectly regular vibrato and a natural singer can have their own kept plus a
little more.

### 3.6 Voiced, unvoiced, gate and tracking

- `gate` (dB): below it the tracker reports unvoiced, and by §3.4 the
  effect is the delayed input. Room noise does not get tuned.
- `tracking` (%): the confidence threshold. Relaxed follows a breathy or
  distorted source (and mis-tracks noise into notes now and then); strict
  corrects only what it is sure of. The default is the middle.
- Transitions cost nothing: there is no voiced/unvoiced crossfade because
  there is no mode switch (§3.4).

### 3.7 Stereo

Detection is on `(L + R)/2`. The mark schedule is computed once and both
channels are shifted through it, with their own rings; an inter-channel
phase offset in the source is preserved by construction, since the same
grain positions are read from both. A mono bus is the same code with one
channel.

### 3.8 Latency: two modes, five ranges

| `range` | f_min – f_max | P_max @ 48 k | Live latency (`P_max + 32`) | Studio (`2·P_max + 64`) |
|---|---|---|---|---|
| Soprano | 160 – 1400 Hz | 300 | 332 smp · 6.9 ms | 664 · 13.8 ms |
| Alto / Tenor | 100 – 1000 Hz | 480 | 512 · 10.7 ms | 1024 · 21.3 ms |
| Baritone / Bass | 60 – 600 Hz | 800 | 832 · 17.3 ms | 1664 · 34.7 ms |
| Instrument | 40 – 2000 Hz | 1200 | 1232 · 25.7 ms | 2464 · 51.3 ms |
| Low | 25 – 400 Hz | 1920 | 1952 · 40.7 ms | 3904 · 81.3 ms |

- **Live** places grains from marks one period behind the input; the hop is
  32. It is the mode for a singer monitoring through the insert.
- **Studio** centres the analysis window on the grain (one more period of
  look-ahead) and hops at 64: tracking on onsets is steadier and octave
  errors rarer. It is the mode for a recorded clip, where the graph pays the
  latency and nobody feels it.
- The number is **fixed for a (range, mode) pair** and does not move with the
  note, so the graph's compensation is stable. `insert_latency_samples`
  computes it from the config; the window prints it under the `mode`
  chooser ("21.3 ms") because a person choosing a mode is choosing a cost.
- A change of range or mode is a latency change and therefore a **graph
  rebuild** (the session already rebuilds on structural edits); the other
  thirty-odd knobs go down the live wire (`EffectControls::publish`) like
  every effect's.

---

## 4. The parameters

Ids are permanent (INVARIANT 7). The table is `TUNE_PARAMS`, built through
`with_mix` like every other, in this order; `TUNE_SECTIONS` groups it; the
counts are held by `every_effects_sections_cover_its_parameters_exactly`.
Thirty-nine parameters plus `mix` — under `MAX_EFFECT_PARAMS` (64).

### 4.1 Input

| id | name | range | default | unit / taper |
|---|---|---|---|---|
| `range` | Range | Soprano · Alto/Tenor · Baritone/Bass · Instrument · Low | Alto/Tenor | Stepped(5), positions named |
| `mode` | Mode | Live · Studio | Studio | Stepped(2) |
| `tracking` | Tracking | 0 – 100 % | 50 | Percent, Linear |
| `gate` | Gate | −80 – −20 dB | −50 | Decibels, Linear |

### 4.2 Scale

| id | name | range | default | unit / taper |
|---|---|---|---|---|
| `root` | Root | C · C# · D · … · B | C | Stepped(12), positions named |
| `scale` | Scale | see below | Chromatic | Stepped(15), positions named |
| `note_c` … `note_b` | C … B (twelve) | off / on | all on | Switch |
| `control` | Control | Scale · MIDI melody · MIDI scale | Scale | Stepped(3) |
| `midi_bend` | MIDI bend | off / on | on | Switch |

**Scales** (`TuneScale`, with `label()`, `ALL`, and `mask(root) -> u16`):
Chromatic, Major, Natural minor, Harmonic minor, Melodic minor, Dorian,
Phrygian, Lydian, Mixolydian, Locrian, Major pentatonic, Minor pentatonic,
Blues, Whole tone, **Custom**. The DSP's active set is
`if scale == Custom { the twelve switches } else { scale.mask(root) }`.
Choosing a named scale does not touch the switches; clicking a key on the
window's keyboard **copies the derived mask into the switches, flips the
switch clicked, and sets `scale` to Custom** — one `ApplyPreset`-style
whole-config write, one undo entry (§7.5). The switches are twelve
`ParamSpec`s so a lane can automate the scale by section, which nothing on
the market lets you do from the automation system.

### 4.3 Correction

| id | name | range | default | unit / taper |
|---|---|---|---|---|
| `retune` | Retune speed | 0.5 – 400 ms | 20 | Milliseconds, Logarithmic. The bottom of the knob reads "instant" and the DSP treats anything under 1 ms as no glide; a log taper cannot hold a zero, and half a millisecond is one hop. |
| `amount` | Amount | 0 – 100 % | 100 | Percent |
| `humanize` | Humanize | 0 – 100 % | 0 | Percent |
| `flex` | Flex | 0 – 100 % | 0 | Percent |
| `natural_vibrato` | Natural vibrato | 0 – 100 % | 100 | Percent |

### 4.4 Vibrato

| id | name | range | default | unit / taper |
|---|---|---|---|---|
| `vibrato_depth` | Depth | 0 – 100 cents | 0 | None (cents), Linear |
| `vibrato_rate` | Rate | 0.1 – 12 Hz | 5.5 | Hertz, Logarithmic |
| `vibrato_sync` | Sync | off / on | off | Switch |
| `vibrato_division` | Division | `NoteDivision::ALL` | 1/8 | Stepped, positions named |
| `vibrato_onset` | Onset | 0 – 1000 ms | 200 | Milliseconds, Linear |
| `vibrato_shape` | Shape | Sine · Triangle | Sine | Stepped(2) |

### 4.5 Voice

| id | name | range | default | unit / taper |
|---|---|---|---|---|
| `engine` | Engine | Smooth · Hard · Grain | Smooth | Stepped(3) |
| `texture` | Texture | 0 – 100 % | 0 | Percent |
| `grain` | Grain | 5 – 60 ms | 25 | Milliseconds, Logarithmic (Grain engine only; the window greys it otherwise) |
| `formant` | Formant | −12 – +12 st | 0 | None (semitones), Linear |
| `formant_follow` | Formant follow | 0 – 100 % | 0 | Percent |
| `transpose` | Transpose | −12 – +12 st | 0 | None, Stepped(25) |
| `detune` | Detune | −100 – +100 cents | 0 | None, Linear |

### 4.6 Output

| id | name | range | default |
|---|---|---|---|
| `output` | Output | −24 – +12 dB | 0 |
| `mix` | Mix | 0 – 100 % | 100 (`ALL_WET`; not `is_time_based`) |

### 4.8 Character (added 2026-09-09)

| id | name | range | default | unit / taper |
|---|---|---|---|---|
| `drive` | Drive | 0 – 100 % | 0 | Percent, Linear |
| `crush` | Crush | 0 – 100 % | 0 | Percent, Linear |
| `air` | Air | −100 – +100 % | 0 | Percent, Linear |
| `width` | Width | 0 – 200 % | 100 | Percent, Linear |

**Why this section exists.** The brief asks for a corrector that is not
locked into one sound, and §3/§4 gave it every axis of the *correction* —
how the pitch is found, how it is pulled, how the grains are laid. It gave it
none of the *colour*. But the thing that makes a cheap autotune sound cheap is
not only its grain schedule: it is saturation, band-limiting and brightness on
top of a hard correction. With `engine` and `texture` as the only character
controls, sixteen presets could differ in how they corrected and barely at all
in what they sounded like made of.

So: four knobs after the shifter and before the output gain (§3.1's path
gains one box), all off or unity in a fresh corrector, in the order
saturate → decimate → tilt → spread. Drive before crush, because a saturator
after a sample-and-hold smooths away the steps the crush was for; the tilt
after both because it shapes what they made; width last because it is the
only one about two channels rather than one.

**`drive` is auto-gained, and this is the whole of the design.** Two obvious
normalisations are both wrong and the preset bank found each in turn.
`tanh(k·x)/tanh(k)` fixes the curve at full scale, a level real signals never
reach, so a vocal at −20 dBFS comes out *eight to seventeen decibels louder* —
an upward compressor wearing a saturator's name. `tanh(k·x)/k` fixes the slope
at the origin, which puts the ceiling at 1/k: at the top of the knob that is
−21 dBFS and the same vocal comes out eighteen decibels *quieter*. A fixed
curve cannot match both ends, because compressing the loud part while leaving
the quiet part alone is what saturation is. So the gain is **measured** —
block RMS before and after, the ratio applied back, smoothed a quarter per
block. That is what a real saturator's auto-gain does and it is the only
version that leaves a bank at one volume, which is the property that decides
whether anybody browses it.

Two gates hold the bank to that, both in `fontelle-fx/tests/tune.rs`:
`every_tune_preset_makes_a_sound_within_three_decibels_of_the_wire` (which
predates this section) and `no_factory_preset_clips_a_normal_vocal` (which
does not, and which caught seven presets at once).

### 4.7 A fresh Tune is a working tuner

Rule 2 says a fresh effect is nearly a wire, and the gate records the
exception this effect also takes: the knob a person reaches for on a tuner is
the retune speed, and an insert that did nothing until `amount` was found
would look broken. So a fresh Tune is **chromatic, retune 20 ms, amount
100 %, natural vibrato kept, Smooth, formants preserved** — a transparent
tuner that corrects the next note it hears. `a_fresh_tune_is_a_tuner_and_not_
a_wire` holds it. The two ways to make it a wire are `amount` at 0 and
`mix` at 0, and the test that an effect whose mix is fully dry is a wire
covers the second already.

Every enum above gets `label()`, `ALL` and a `static` position table
(catalogue §5 step 1): `TuneRange`, `TuneMode`, `TuneControl`, `TuneScale`,
`TuneEngine`, `VibratoShape`. `NoteDivision` and `Switch` are reused.

---

## 5. MIDI into an insert

The one piece of plumbing this effect adds to the DAW, and the smallest
version of it that gives both halves of the brief.

### 5.1 The document

`EffectSlot` gains `notes: Option<ChannelId>`, `#[serde(default,
skip_serializing_if = "Option::is_none")]` like `key`, so every existing
project reads unchanged. `EffectKind::takes_notes(self) -> bool` is true for
`Tune` alone, beside `takes_key`, for the reason `takes_key` is a predicate:
the command validates on it, the builder wires on it, the window offers on
it, and three copies of the list is two to forget.

**`SetInsertNotes { track, index, notes }`**, mirroring `SetInsertKey`:
refuses on an effect that does not take notes, refuses a channel that does
not exist, records the previous value for undo. **`RemoveChannel`** clears
every slot that named it, the way removing a mixer track clears the keys that
named it — check what that command does for `key` and do the same thing in
the same place.

A channel with **no instrument** (`Channel::instrument == None`, a real
state) is a perfectly good source: its notes compile (`compile.rs` targets
`channel_nodes[channel]` for every channel) and it makes no sound. "Add a
channel, leave it empty, write the melody in its roll, point the tuner at it"
is the workflow, and it needs nothing new.

### 5.2 The engine

`EffectNode` gains `notes_from: Option<NodeId>` and a builder
`with_notes_from`. In `process`, before the DSP:

```rust
if let Some(source) = self.notes_from {
    for event in ctx.all_events.iter().chain(ctx.live_events.iter())
        .filter(|e| e.target == source)
    {
        match event.payload {
            EventPayload::NoteOn { key, .. } => self.held.press(key),
            EventPayload::NoteOff { key, .. } => self.held.release(key),
            EventPayload::PitchBend { .. } => self.bend = …,
            _ => {}
        }
    }
}
```

`held` is a fixed `HeldKeys { keys: [u8; 16], len: usize }` — last-note
priority, a 16-deep stack, no `Vec` (INVARIANT 1). The frame's held set and
the bend are handed to `fontelle_fx::Tune::process` as a `NoteInput { last:
Option<u8>, mask: u16, bend_cents: f32 }`, so the DSP crate never sees an
event. `reset` and `reset_sequenced` clear the stack.

**Why read the source node's events rather than receive copies.**
`ProcessContext` warns that a node reading `all_events` "will play other
instruments' parts" — and this node is *configured* to listen to one other
node's part, which is the intent. The alternative — teaching the compiler to
emit a second copy of every note for every listener, and teaching
`LiveEventSource` to fan live events out — touches the sequencer, the live
path and the router for the same result. Both nodes see the same block's
slices whatever order they run in, so there is no ordering hazard, which is
the hazard the key tap has and this does not.

**Wiring**: `realise` sets `with_notes_from(channel_nodes[channel])` when the
slot names a channel that still exists. A rebuild mints new node ids and a
rebuild rewires this along with everything else.

### 5.3 The window and the session

A drop-down on the Scale card (§7.2): "no MIDI", then every channel by name
with its colour swatch, the chosen one marked. `DocumentHost::set_insert_notes
(strip, slot, Option<usize>)` beside `set_insert_key`; the session maps the
index onto a `ChannelId` and runs the command. Playing the on-screen keyboard
or a controller while that channel is selected reaches the insert at once,
because the live target is the selected channel's node.

---

## 6. The preset bank

Sixteen, category **Factory** (xtask's convention for effects: one category,
because these are points on one control surface). Recipes are constructors
on `TuneConfig` (`TunePreset::ALL`, `from_preset`), exported by `cargo xtask
export-factory-presets` into `assets/presets/fx-tune/Factory/`, embedded by
`build.rs`. Anything not listed is the fresh state (§4.7).

| Preset | The sound | Settings |
|---|---|---|
| **Transparent** | Nobody can tell | retune 120, humanize 60, flex 40, natural vibrato 100, Studio |
| **Pop Polish** | The modern radio vocal | retune 35, humanize 30, natural vibrato 80, flex 15 |
| **Hard Tune** | The Cher / T-Pain snap | retune 0, humanize 0, flex 0, natural vibrato 0, Smooth, texture 40 |
| **Trap Robot** | Harder, colder | retune 0, natural vibrato 0, Hard, texture 70, gate −40 |
| **Cheap Plastic** | The free plug-in | Grain, grain 25, texture 20, formant follow 100, retune 5, tracking 25 |
| **Chipmunk** | Up an octave, small | transpose +12, formant follow 100, retune 10, Smooth |
| **Deep Voice** | Down a fourth, bigger | transpose −5, formant −3, retune 60, Baritone/Bass |
| **Gender Up** | Same notes, smaller throat | formant +4, retune 80, humanize 40 |
| **Gender Down** | Same notes, longer throat | formant −4, retune 80, humanize 40 |
| **MIDI Melody** | Play the vocal from a keyboard | control MIDI melody, retune 10, natural vibrato 30 |
| **MIDI Scale** | Held chord is the scale | control MIDI scale, retune 40, humanize 20 |
| **Synth Vibrato** | Robot with a regular wobble | retune 0, natural vibrato 0, depth 40, rate 5.5, onset 250 |
| **Flat Line** | No vibrato, no drift, no glide | retune 0, natural vibrato 0, humanize 0, tracking 70, gate −40 |
| **Instrument** | Guitar, sax, whistling | range Instrument, Studio, retune 25, flex 30 |
| **Octave Under** | A doubler beneath the dry | transpose −12, mix 50, retune 30 |
| **Fifth Above** | A harmony beside the dry | transpose +7, mix 45, retune 30, formant follow 20 |

`every_tune_preset_is_somewhere_other_than_the_wire_and_than_each_other`
holds them apart on the config, as the distortion's does; §9.3's
`the_three_archetypes_measure_differently` holds three of them apart on the
*sound*, because a table of numbers is not a family until the numbers are
heard to differ.

---

## 7. The window

### 7.1 The look: a ship's console, made concrete

The brief is "robotic and futuristic like the interior of a sci-fi spaceship".
A builder cannot draw that from the adjective, so here is what it is in
primitives the renderer already has (`fill_glow`, `glow_polyline`,
`fill_rect_vertical`, `stroke_polyline`, the Flopsynth card and knob drawers):

- **The ground** (`draw_tune_ground`, the sibling of `draw_flop_ground`): a
  near-black vertical gradient, `mix(window, accent, 0.06)` at the top to
  `window` at the bottom, with a **lattice** — a hexagonal grid of 28 px
  cells stroked at `text.with_alpha(0x10)` — and one horizon: a 1 px line at
  60 % height in `accent.with_alpha(0x50)` with a `fill_glow` below it. No
  stars; that is Flopsynth's sky and the two windows should be tellable
  apart at a glance.
- **Consoles** (`draw_tune_card`): the card's body is `panel` at 0xd8 alpha,
  and its corners are **chamfered**, not rounded — a 6 px 45° cut at the
  top-left and bottom-right only, drawn as a path. A 1 px edge light in the
  card's ink along the top edge and down the left, with a 4 px `fill_glow`
  behind it; the name in uppercase, `Labels::ensure_small`, in the ink. A
  faint **scanline** over any display area: every third row a 1 px line at
  `window.with_alpha(0x30)`.
- **Knobs**: `draw_flop_knob` as it is — the arc, the glow, the caption —
  in the card's ink. Choosers are the generic `ContextMenu` drop-downs and
  switches are `draw_flop_switch`, both already in the sci-fi register.
- **Inks** (`tune_ink`, the sibling of `card_ink`): Input and Output cards in
  `text_muted`; Scale in `accent`; Correction in `accent`; Vibrato and
  Voice in `modulation` (they *move* something); MIDI in `playhead`; the
  reticle in `meter_peak`. **No new palette tokens**: Flopsynth added one
  and paid a theme-format bump for it; this window is drawn from the seven
  inks the palette already has, so a person's own theme recolours it.
- **The viewport** (§7.3) is the centrepiece and gets the console treatment
  hardest: a 2 px inset frame in the accent at 0x80, a `fill_glow` at each
  corner, the lattice showing through at half strength, the scanlines, and
  the trace itself in `glow_polyline`.
- **Read-outs** under choosers in the theme's monospace where the theme has
  one (`FontTokens::family` is the theme's; the window does not ship a
  font). Numbers are what make a console look like one; every knob has its
  read-out (`display`), and the `mode` chooser's is the latency.

### 7.2 The layout

**Size** 960×600, minimum 820×520, **no scrolling** and no pages: a tuner is
one page or it is not easy to control. Cells are Flopsynth's `FLOP_CELL_W ×
FLOP_CELL_H` (52×54) and shrink together to `CELL_FLOOR` (0.8) before the
viewport gives up height, exactly as Flopsynth's pictures do.

```
┌ header: "TUNE · pitch correction"      [preset bar ...........................] ┐
│ ┌──────────────────────────── VIEWPORT (full width, 168 px) ──────────────────┐ │
│ │  scale rails ─────  sung pitch (dim)  corrected (bright)  MIDI bars  reticle│ │
│ └──────────────────────────────────────────────────────────────────────────────┘ │
│ ┌──────────────────────────── KEYBOARD (full width, 64 px) ───────────────────┐ │
│ │ two octaves, enabled keys lit, root ringed, MIDI keys pulsing, target bright │ │
│ └──────────────────────────────────────────────────────────────────────────────┘ │
│ ┌ INPUT (4) ───┐ ┌ CORRECTION (5) ─────┐ ┌ VOICE (7) ───────────────────────┐   │
│ │ range mode   │ │ retune amount human │ │ engine texture grain formant     │   │
│ │ track  gate  │ │ flex  natvib        │ │ follow transpose detune          │   │
│ └──────────────┘ └─────────────────────┘ └──────────────────────────────────┘   │
│ ┌ SCALE (4) ───────┐ ┌ VIBRATO (6) ───────────────┐ ┌ OUTPUT (2) ┐ ┌ MIDI (2) ┐ │
│ │ root scale ctrl  │ │ depth rate sync div onset  │ │ out  mix   │ │ src bend │ │
│ │ (keys above)     │ │ shape                      │ │            │ │          │ │
│ └──────────────────┘ └────────────────────────────┘ └────────────┘ └──────────┘ │
└──────────────────────────────────────────────────────────────────────────────────┘
```

Declared shapes, as Flopsynth's `shape_of` declares them
(`fontelle-app/src/tune.rs::shape_of`): the viewport is band 0 and full
width; the keyboard band 1 and full width; Input, Correction and Voice are
band 2 at 4, 5 and 7 cells; Scale, Vibrato, Output and MIDI are band 3 at 4,
6, 2 and 2. The twelve note switches are **not** cells — the keyboard is
their control, and `TuneView` marks them as owned by the picture the way
Flopsynth's `picture_control` marks a wave position. A chooser with a name
over `WIDE_CHOICE` (9) characters spans two cells; "Baritone/Bass" does, so
the Input card is really five cells wide and the table above says four
because the builder will find that out the way Flopsynth's did.

`the_whole_tune_window_fits_the_window_it_opens_at` builds the real view —
every card, with the fresh config — at 960×600 and at 820×520 and asserts no
card's bottom is below the body and no cell overlaps another, the test that
would have caught Flopsynth's first build.

### 7.3 The viewport and the `TuneTap`

The picture is *the pitch track*, four seconds of it, scrolling left:

- **Rails**: one horizontal line per enabled pitch class across the visible
  octaves, `accent.with_alpha(0x40)`; the root's rail brighter; a disabled
  class draws nothing. The y axis is cents, `range`'s f_min to f_max, log.
- **Sung**: the detected pitch as a dim polyline (`text_muted`), gaps where
  unvoiced.
- **Corrected**: the output pitch as a bright `glow_polyline` in the accent.
  Where the two coincide the eye sees one line; where they part is the
  correction, which is the whole story of the effect drawn in one picture.
- **MIDI**: held keys as bars in `playhead` for their duration.
- **Reticle**: at the right edge, on the current corrected pitch, a bracket
  pair `[ ]` in `meter_peak` that closes to `[]` when `|target − slow| <
  5 cents` — the lock. Beside it the cents read-out ("−23 ¢ → A3").
- **Latency, engine, mode** as small captions in the frame's top-right.

**`TuneTap`** (`fontelle-engine/src/tune_tap.rs`) is the analyser tap's
sibling: a ring of `TuneFrame { sung_cents: f32, out_cents: f32, target:
f32, flags: u32 }`, one per hop, written by the node with aligned 32-bit
stores and read by the window once a frame — the same honesty the spectrum
tap states: a stalled reader draws a seam nobody can see, and the audio
thread never waits. 4 s at a 64-sample hop at 48 kHz is 3000 frames; size it
for 96 kHz at hop 32 (12 000) and never resize. `EffectNode::with_tune_tap`,
`Session::tune_taps` keyed like `spectrum_taps`, and `DocumentHost::
tune_trace(strip, slot) -> Vec<TuneFrame>` (allocating on the UI thread, once
a frame, which is what `spectrum` already does). The tap is attached only
while the window is open, like the analyser's, so a mix pays nothing for a
picture nobody is looking at.

`viewport_points(area, frames, range, mask)` in `canvas/tune.rs` is pure and
tested: hz→y at the rails, gaps at unvoiced, and an empty trace draws
nothing rather than a line along the floor.

### 7.4 The keyboard

Two octaves, C to B twice, drawn by `draw_tune_keyboard` — its own function,
not `draw_keyboard`, which is welded to `RollLayout`. Twenty-four key rects
from `tune_keyboard_layout(area) -> [Rect; 24]` (naturals full height,
accidentals two-thirds, the piano-roll's proportions). Ink per key:

- **enabled** (in the active mask): filled `accent` at 0x60, edge light on;
- **disabled**: `panel` at 0x80, no light;
- **root**: a ring inside the key in the accent;
- **MIDI held**: `playhead` fill, both octaves;
- **current target**: `meter_peak` edge, the one key lit brightest;
- **sung**: a small dot at the sung pitch class, `text_muted`.

Both octaves show the same mask (a scale is octave-periodic); the reason
there are two is that the target and the sung dot are drawn at their real
octave within the range when it fits, so the keyboard doubles as a display.

**Click** a key: toggles that pitch class (§4.2: derive mask, flip, set
Custom — one entry). **Right-click** a key: sets `root` to it (and leaves the
scale alone, which on a named scale re-derives the mask). **Shift-click**:
solo — only that class enabled. The scale drop-down beside it names the
result: Custom, or the named scale if the mask happens to equal one at the
current root (recognised, not remembered — the same rule the preset bar's
`*` follows).

### 7.5 Gestures, in one table

| Gesture | On | Does | History |
|---|---|---|---|
| Drag up/down | knob | `set_insert_param` by id, `knob_value` throw, shift fine | one entry per drag (coalesced as `SetInsertParam` already coalesces) |
| Wheel | knob | step | entry |
| Double-click | knob | reset to default | entry |
| Click | chooser | `ContextMenu` of its positions | entry on pick |
| Click | switch | flip | entry |
| Click | keyboard key | toggle class → Custom | one whole-config entry (`ApplyPreset`'s shape, not two `SetInsertParam`s) |
| Right-click | keyboard key | root | entry |
| Shift-click | keyboard key | solo class | one whole-config entry |
| Click | MIDI source drop-down | `set_insert_notes` | entry |
| Click | preset bar | as everywhere | one entry |
| Right-click | any knob | "Create automation lane", as the generic panel offers | — |
| Hover | viewport | tooltip with the frame under the pointer: sung, corrected, target | — |

### 7.6 Plumbing the window through the traits

Flopsynth's window is the template and the file list is the same shape:

- `fontelle-ui/src/canvas/tune.rs`: `TuneView` (title, the config's values
  as `InstrumentParam`s grouped into the seven cards, the active mask, held
  keys, target, the trace, the latency caption, the MIDI source list and
  choice), `TuneCard`/`tune_layout`/`TuneLayout`/`tune_hit`/`TuneHit`, the
  keyboard layout and hit, `viewport_points`. Pure; no `Scene`.
- `fontelle-ui/src/render/mod.rs`: `TuneChrome`, `draw_tune`,
  `draw_tune_ground`, `draw_tune_card`, `draw_tune_keyboard`,
  `draw_tune_viewport`. Reuses `draw_flop_knob`, `draw_flop_switch`,
  `draw_flop_chip`.
- `fontelle-ui/src/app.rs`: a third branch beside `eq` and `insert_view` —
  `tune: Option<TuneView>` filled in `open_insert` and `refresh_studio` from
  `doc.tune_view(strip, slot)`; `press_tune_editor`, `wheel_tune`, the drag
  arm, and `EditorKind::Effect` dispatching on which of the three is `Some`.
  The effect window's size for a Tune is 960×600 (the EQ's and the generic
  panel's stay what they are).
- `fontelle-ui/src/document.rs`: `DocumentHost::tune_view`, `tune_trace`,
  `set_insert_notes`, with no-op defaults like `eq_config`'s.
- `fontelle-app/src/tune.rs`: `describe(config, held, trace, channels) ->
  TuneView` and `shape_of`, the layer allowed to see both halves
  (INVARIANT 4), the sibling of `fontelle-app/src/flopsynth.rs`.
- `fontelle-app/src/session.rs`: the three `DocumentHost` methods, the
  `tune_taps` map, the rebuild on a range/mode change.

### 7.7 Tests for the window (`fontelle-ui/tests/tune.rs`)

- `the_whole_tune_window_fits_the_window_it_opens_at` (960×600 and
  820×520, real seven-card view, no overlap, nothing below the body).
- `every_control_is_where_the_layout_says` (hit-test every cell centre and
  get that control back; every keyboard key likewise).
- `a_wide_chooser_takes_two_cells`.
- `the_keyboard_marks_the_mask_the_root_the_held_keys_and_the_target`.
- `clicking_a_key_asks_for_a_custom_scale_with_that_class_flipped` (the
  view's answer, pure; the document half is in `fontelle-app`).
- `viewport_points_put_a_pitch_on_its_rail_and_leave_gaps_where_unvoiced`.
- `an_empty_trace_draws_nothing`.
- A `render_headless` case that draws the window and dumps it under
  `FONTELLE_UI_DUMP`; the PR carries the PNG. It is the test that can see.

---

## 8. The plumbing, by crate

In dependency order — which is also the build order, because each crate's
tests can be green before the one above it exists.

**`fontelle-types`** — `effect.rs`: `EffectKind::Tune` (+ `label`, `ALL`,
`takes_notes`), `EffectConfig::Tune(TuneConfig)`, `TuneConfig` with
`new()` (§4.7), `get`/`set` by id, `TUNE_PARAMS`, `TUNE_SECTIONS`, the six
enums with labels/ALL/position tables, `TunePreset::ALL` + `from_preset`.
`preset.rs`: the `fx-tune` slug. New constants: `TUNE_MAX_LATENCY_MS` is not
one number — it is `TuneRange::p_max_hz()` and the mode's multiplier, exposed
as `TuneConfig::latency_samples(sample_rate)` **here**, so the document, the
node and the window all ask one function.

**`fontelle-dsp`** — `pitch.rs`: `PitchTracker::new(f_min, f_max, hop)`,
`prepare(sample_rate)`, `reset`, `push(&[f32]) -> Option<PitchFrame>` per hop,
`threshold(f32)`, `gate_db(f32)`. `psola.rs`: `PsolaShifter::new(channels)`,
`prepare(sample_rate, p_max, max_block)`, `reset`, `set_period(f32)`,
`set_ratio(f32)`, `set_formant(f32)`, `set_engine(Engine, texture, grain)`,
`process(&mut [&mut [f32]])` in place with the fixed latency. Both `no_std`-
shaped: no allocation after `prepare`, `#![allow(clippy::needless_range_
loop)]` where the crate already allows it.

**`fontelle-fx`** — `tune.rs`: `Tune::new()`, `prepare(sample_rate)`,
`reset`, `process(outputs, notes: NoteInput, &TuneConfig, bpm)`, plus
`trace_frame() -> TuneFrame` for the tap. Owns the tracker, the shifter,
the pitch-track state (§3.3), the vibrato phase, and the mono sum scratch.
`lib.rs` exports it; `meters.rs`'s `Tuner` stub is **deleted** — the
catalogue's tuner row is now "a window on `PitchTracker`" and the stub was a
shape with nothing behind it.

**`fontelle-engine`** — `nodes.rs`: the `EffectState::Tune` arm (new,
prepare, process, reset), `notes_from` + `HeldKeys` + `with_notes_from`,
`with_tune_tap`, the second arm of `insert_latency_samples` and the new
`max_insert_latency_samples` that `prepare` sizes the dry line from.
`tune_tap.rs`: `TuneTap`, `TuneFrame`. `lib.rs` exports.

**`fontelle-model`** — `mixer.rs`: `EffectSlot.notes`. `commands.rs`:
`SetInsertNotes`; `RemoveChannel` clears listeners. `lib.rs` exports.

**`fontelle-sequencer`** — nothing. §5.2 is why.

**`fontelle-app`** — `realise.rs`: `with_notes_from` and the tap; the latency
walk already reads `insert_latency_samples`. `session.rs`: the three host
methods, `tune_taps`, `set_insert_notes` → command, structural-rebuild on
range/mode. `tune.rs`: `describe`, `shape_of`. `build.rs` embeds the new
folder without change.

**`fontelle-ui`** — §7.6.

**`xtask`** — `presets.rs::effect_presets` gains the `TunePreset` loop.

---

## 9. Tests, written first

Each file below is written, run and seen to fail before its implementation.
The names are the claims; a test that would pass against the effect with the
feature stripped out is not a test of the feature (catalogue rule 12).

### 9.1 `fontelle-dsp/tests/pitch.rs`

- `a_sine_is_found_within_a_cent` (220 Hz at 44.1/48/96 k).
- `a_sawtooth_is_found_at_its_fundamental_not_its_second_harmonic` (110 Hz;
  the octave-up guard).
- `a_synthetic_vowel_is_found_within_two_cents` (f0 150 Hz, harmonics
  shaped by formants at 700/1200/2600 Hz).
- `noise_is_unvoiced` and `silence_under_the_gate_is_unvoiced`.
- `a_glide_is_followed_within_ten_cents_and_three_hops` (200→300 Hz over
  1 s).
- `a_step_lands_within_three_hops` (the median's lag plus one).
- `the_lowest_note_of_each_range_is_found_and_the_one_below_is_not`.
- `a_breath_between_notes_does_not_flip_the_octave` (a voiced tone, 20 ms
  of noise at −20 dB, the tone again: the pitch before and after agree and
  no hop in between reports an octave off).
- `the_tracker_adds_no_latency_only_reaction_time` (an onset is reported
  within one hop + the median of the sample it happened at).

### 9.2 `fontelle-dsp/tests/psola.rs`

- `ratio_one_is_the_input_delayed_by_the_reported_latency` (null under
  −60 dB on a vowel, on speech-like noise bursts, and on silence).
- `a_fifth_up_comes_out_a_fifth_up` (220 → 329.6 Hz within 2 cents; measure
  with the tracker over ≥ 64 blocks — the zero-crossing trap in the
  `performance-events` memory).
- `an_octave_down_comes_out_an_octave_down`.
- `formants_stay_where_they_were_at_follow_zero` (vowel's spectral envelope
  peaks within 5 % after a fifth; envelope by a 30-bin cepstral lifter on the
  magnitude spectrum, not by centroid — the `flopsynth-preset-bank` memory
  says why a centroid cannot see this).
- `formants_move_with_the_pitch_at_follow_one` (peaks at ×1.5).
- `a_formant_shift_alone_leaves_the_pitch_alone`.
- `the_level_is_flat_through_a_glide` (RMS within ±1 dB across a 2-octave
  sweep of ratio; the window-sum normalisation).
- `no_click_at_a_grain_boundary` (max first difference of the output ≤ 2 ×
  the input's on a steady tone).
- `unvoiced_passes_through_unchanged` (correlation > 0.99 against the
  delayed input; ratio held at 1 for unvoiced).
- `both_channels_share_one_mark_schedule` (a stereo tone with a 90° offset
  keeps its inter-channel correlation).
- `the_grain_engine_modulates_at_the_grain_rate_and_smooth_does_not`
  (spectral line at 1/(grain/2) in the envelope of a steady tone: > −30 dB
  for Grain, < −50 dB for Smooth).
- `the_hard_engine_puts_a_buzz_at_the_period_rate_that_smooth_does_not`.
- `texture_does_something_different_on_every_engine`.
- `latency_is_fixed_for_a_range_and_a_mode` (the reported number equals the
  measured null delay for all five ranges × two modes).

### 9.3 `fontelle-fx/tests/tune.rs`

- `a_fresh_tune_is_a_tuner_and_not_a_wire` (30-cent-flat A → 440 within
  3 cents after 200 ms).
- `speech_is_left_alone` (an unvoiced-heavy synthetic: output null against
  delayed input under −40 dB over the unvoiced parts).
- `retune_zero_snaps_within_two_hops` and
  `retune_two_hundred_takes_two_hundred_milliseconds_to_get_most_of_the_way`
  (63 % ± 10 %).
- `amount_scales_the_correction` (50 % → half the cents).
- `in_c_major_four_seventy_hertz_goes_to_b`; `with_only_c_and_g_it_goes_to_g`;
  `with_d_as_the_root_f_sharp_is_in_and_f_is_out`;
  `the_chromatic_scale_sends_it_to_b_flat`.
- `a_custom_mask_is_read_only_when_the_scale_says_custom`.
- `a_note_on_the_boundary_does_not_flip_between_two_targets` (hysteresis).
- `flex_leaves_a_far_off_note_mostly_alone` (45 cents off, flex 100: < 20 %
  corrected; flex 0: > 95 %).
- `humanize_lets_a_held_note_drift_back_and_corrects_a_moving_one_in_full`.
- `the_singers_vibrato_survives_at_one_hundred_and_is_flat_at_zero` (6 Hz
  ±40 cents; measure the 6 Hz line in the output pitch track).
- `added_vibrato_has_its_depth_its_rate_and_its_onset` and
  `a_synced_vibrato_takes_its_rate_from_the_tempo` (1/8 at 120 = 4 Hz).
- `midi_melody_forces_the_held_key`, `letting_go_returns_to_the_scale`,
  `the_last_key_held_wins`, `midi_scale_uses_the_held_classes`,
  `the_bend_moves_the_target_when_asked_and_not_otherwise`.
- `an_onset_restarts_the_glide_from_the_sung_pitch` (the yodel is real:
  with retune 100 ms, the correction at the hop after a jump is ~0).
- `transpose_and_detune_are_added_after_the_correction`.
- `below_the_gate_nothing_is_corrected`;
  `strict_tracking_leaves_a_noisy_tone_alone_and_relaxed_corrects_it`.
- `the_three_archetypes_measure_differently` (Transparent, Hard Tune,
  Cheap Plastic on one glided vowel: settling time, formant motion, grain
  modulation — three axes, three different corners).
- `every_tune_preset_makes_a_sound_within_three_decibels_of_the_wire`.

### 9.4 `fontelle-types/tests/effect_families.rs` (additions)

- `the_tune_is_input_scale_correction_vibrato_voice_then_output` (section
  names and counts).
- `the_tunes_choosers_name_every_position` (five ranges, fifteen scales,
  three engines, three controls, twelve roots).
- `the_tunes_knobs_have_the_units_and_ranges_this_plan_gives`.
- `the_twelve_note_switches_are_switches_and_are_reachable_by_address`.
- `a_fresh_tune_is_what_section_four_says`.
- `every_scale_mask_has_the_right_notes_at_every_root` (a table: Major at C
  is `0b1010_1011_0101`, and so on — write the table by hand, not by calling
  the function).
- `every_tune_preset_is_somewhere_other_than_the_wire_and_than_each_other`.
- `a_tune_round_trips_through_json`.
- `the_latency_a_config_reports_matches_the_table_in_the_plan` (§3.8's
  samples at 48 k, all ten cells).
- `preset.rs`: the slug table gains `fx-tune`.

### 9.5 `fontelle-engine/tests`

- `effects.rs` covers build/run/bypass/dry-is-a-wire generically once the
  kind is in `ALL`; `no_allocation_during_render.rs` likewise.
- New `tune_notes.rs`: `an_insert_hears_the_notes_of_the_channel_it_names`
  (timeline slice), `and_the_live_ones` (live slice),
  `and_not_anybody_elses`, `a_reset_lets_go_of_every_held_key`,
  `sixteen_keys_held_is_the_most_it_remembers_and_the_seventeenth_does_not_
  allocate`.
- `latency.rs`: `a_tune_in_studio_mode_is_padded_like_a_gate_with_look_ahead`
  (impulse alignment against a sibling track), and the dry-line null at 50 %
  mix for every range (the comb test the gate has).

### 9.6 `fontelle-model/tests`

- `set_insert_notes_refuses_an_effect_that_takes_none`,
  `…_refuses_a_channel_that_is_not_there`, `…_is_undoable`,
  `removing_a_channel_clears_the_inserts_that_listened_to_it`,
  `a_project_with_notes_on_a_slot_round_trips_and_one_without_writes_no_field`.

### 9.7 `fontelle-app/tests/tune_editor.rs`

- `a_tune_has_a_view_of_its_own_and_not_the_generic_panel`.
- `clicking_a_key_writes_the_mask_and_flips_the_scale_to_custom_in_one_entry`.
- `right_clicking_a_key_sets_the_root`.
- `a_named_scale_is_recognised_from_a_custom_mask_that_equals_it`.
- `the_midi_source_row_lists_every_channel_and_writes_the_slot`.
- `a_preset_from_the_bank_writes_every_knob_and_is_one_entry`.
- `changing_the_range_rebuilds_and_the_reported_latency_moves`.
- `the_trace_reads_back_what_the_node_wrote` (render through a session with
  a tone, read `tune_trace`).
- `playing_the_selected_channel_live_reaches_the_tuner`.

---

## 10. Performance budget

- **Bench**: `crates/fontelle-fx/benches/tune.rs` with `criterion`, a
  stereo block of 128 at 48 kHz, each engine, each mode, Alto/Tenor.
- **Targets** (release, this machine): Smooth/Studio stereo ≤ 1.5 % of one
  core; Live ≤ 2 % (the hop is halved); Grain ≤ 1 %. Memory per insert
  under 400 KB at 96 kHz / Low range (the rings are the whole of it).
- **Where the time goes**: the coarse YIN at fs/4 is `W₄ · τ_range ≈ 240 ×
  120 = 29 k` MACs per hop for Alto/Tenor; the refine is `13 × 960 ≈ 12 k`;
  the shifter is two Hermite reads and two window multiplies per sample per
  channel. At 750 hops/s that is ~30 M MAC/s for detection and ~1 M for
  shifting — the detector is the cost, and the decimation is why it is
  affordable. Low range at Live mode is the worst case (`W₄ = 960`, `τ` up to
  480) and is the one the bench holds.
- If the coarse pass is over budget, the next step is the FFT autocorrelation
  (`fontelle_dsp::fft_in_place` exists) rather than a bigger hop; a bigger hop
  is a slower tuner.

---

## 11. Storage

- `TuneConfig` is a `Copy` struct with serde defaults on every field, so a
  project saved by a build with fewer knobs reads at this build's defaults
  (the rule every effect follows). No format bump.
- `EffectSlot.notes` is `default` + `skip_serializing_if`: a project without
  it writes no field and reads as `None`.
- Presets are the ordinary `PresetPayload::Effect(EffectConfig::Tune(_))`;
  `PRESET_FORMAT_VERSION` is unchanged.
- `THEME_FORMAT_VERSION` is unchanged (§7.1, no new tokens).

---

## 12. Phases

Each phase ends green on its own crate's tests and clippy, with a section
in `PROGRESS.md` saying what was measured and what was cut. Nothing in a
later phase is started before the earlier one's gate.

- **0 — Types.** `fontelle-types` as §8: the kind, the config, the table,
  the sections, the enums, the presets as constructors, `takes_notes`, the
  slug, `latency_samples`. Tests §9.4. The name is frozen here (§2.1).
  *Gate:* `cargo test -p fontelle-types` green; `EffectKind::ALL` has twelve
  entries and `fontelle-engine/tests/effects.rs` therefore fails to compile
  until phase 3, which is the RED for the arm.
- **1 — Primitives.** `PitchTracker` then `PsolaShifter`, tests §9.1 and
  §9.2 first. Build the tracker on sines and saws before vowels; build the
  shifter at ratio 1 (the null) before any other ratio; build Smooth before
  Hard and Grain. *Gate:* every §9.1/§9.2 test green; a throwaway example
  that shifts a WAV up a fifth and writes it out, **listened to**.
- **2 — The corrector.** `fontelle_fx::Tune`, tests §9.3 first, in the order
  §3.3 lists the steps. Presets last, with `the_three_archetypes_measure_
  differently` written before the recipes are tuned, so the recipes are
  tuned *to* the measurement. *Gate:* §9.3 green; the bench under §10.
- **3 — The engine.** The `EffectState` arm, latency, the generalised dry
  line, `notes_from`, the tap. Tests §9.5 first. *Gate:* the generic engine
  suite green with twelve kinds; `latency.rs` green.
- **4 — The document and the app.** `EffectSlot.notes`, `SetInsertNotes`,
  `realise` wiring, the session's three host methods, `xtask` export, the
  preset files committed. Tests §9.6 and the non-window half of §9.7. The
  insert is now usable through the **generic panel** — every knob, the
  twelve switches as switches — and this is the point to render a vocal
  through every preset and listen, before a single pixel of the console is
  drawn. *Gate:* the sixteen presets in the bank, an offline render per
  preset in the PR's notes.
- **5 — The window.** §7 in full, tests §7.7 first, the headless dump looked
  at, then the real binary on `:99` with a project that has the insert on a
  track, the trace drawing while a clip plays. *Gate:* the screenshot
  matches the brief to Ty's eye; every §7.7 test green.
- **6 — Close.** `cargo test --workspace` once in the background; clippy;
  the catalogue's §2.6 row moved from "planned" to "built" with a pointer
  here; `PROGRESS.md`'s entry; the `Tuner` stub gone.

---

## 13. Deliberately not built

- **Polyphonic correction.** A chord has no single period and PSOLA has no
  answer for it; that is a spectral method and its own plan.
- **Graphical (offline, note-by-note) mode** — Melodyne's model. It is a
  clip operation, not an insert, and belongs with the clip's other
  operations when they exist (catalogue §2.6, the repitcher's reasoning).
- **Automatic key detection.** A knob that guesses the song's key from what
  it hears is a chooser that changes under your hand. The scale is stated,
  by keyboard or by MIDI.
- **A "classic" chooser of other products' voicings.** The engines and the
  formant knobs *are* the voicings; naming them after somebody else's plug-in
  would be a preset pretending to be a control.
- **Harmonies as voices inside this insert.** `Fifth Above` at 45 % mix is
  a harmony; a real harmoniser with its own pan per voice is the catalogue's
  §2.6 row and reuses `PsolaShifter` when it comes.
- **A per-note pitch lane** (drawing the correction curve by hand). The
  automation system owns lanes; automate `transpose`, `amount` or a note
  switch instead.

---

## 14. Decisions taken here, for Ty to overrule before phase 0

1. **The name is `Tune`** (`EffectKind::Tune`, slug `fx-tune`, strip label
   "Tune"). Permanent once committed.
2. **A fresh insert corrects** (§4.7) rather than being a wire.
3. **Notes come from a channel, not from a MIDI port** (§5): the rack is
   the router, and a channel with no instrument is the "MIDI track".
4. **Latency is fixed per range and mode** (§3.8) and a range change is a
   rebuild.
5. **No new theme tokens** (§7.1); the console is drawn from the palette's
   existing inks, so themes keep working.
6. **One page, 960×600, no scrolling** (§7.2).
7. **Sixteen presets in one Factory category** (§6). *Superseded 2026-09-09:
   forty, still one category. §4.8's four knobs opened an axis the original
   sixteen could not reach, and a family with new axes and no new points on
   them has a range nobody finds.*
8. **The twelve note switches are parameters**, automatable, and the
   keyboard is their control.
