# Progress

Living document. Read this before `FONTELLE_TDD.md` if you're picking this project
up cold — the TDD is the destination, this is where things actually stand and how
to get back into flow. Update it whenever you finish a chunk of work; don't let it
drift from reality.

**Process rule this project follows, and you should too:** tests are written
*before* the implementation they check, against the intended API, and confirmed to
fail first (a compile error against a stub, or a `todo!()` panic both count). Only
then does the implementation get written, iterating against the compiler and the
test suite as ground truth. Every section below that claims something is "real"
was built this way — check the corresponding test file if you want the proof
rather than the claim.

## 2026-08-26 (later): tempo that changes, and a master bus

### `TempoMap` is piecewise

It held one constant BPM and the importer kept only a file's first tempo event.
A piece that changes tempo played at its opening tempo throughout, and the
rhythm drifted further out the longer it ran — a failure that says nothing
about where it came from, it just sounds wrong from the change on.

Now a sorted segment list with a prefix-sum table, so both conversion
directions are a binary search plus one multiply (TDD §6.2). Three decisions:

- **The prefix is in seconds, not samples.** The sample rate belongs to the
  audio device, so a rate-independent cache means `set_sample_rate` rescales
  every answer without rebuilding anything.
- **The cache is never serialised.** `#[serde(from/into)]` reconstructs it from
  the segments, because a cache that can disagree with its source is a bug
  waiting to happen.
- **A tempo of zero is clamped.** A malformed file can carry one, and dividing
  by it gives an infinite sample position that poisons every conversion after
  it — including notes *before* the bad segment, once a prefix sum picks it up.

`from_segments` sorts (a MIDI file may put tempo events in any track, so they
arrive in file order), resolves same-tick ties by taking the last, and reaches
the first segment back to tick 0 if the file has none there — a count-in then
runs backwards at the opening tempo rather than collapsing onto the downbeat.

`MidiImport::bpm` stays the *opening* tempo, for reporting, and
`tempo_changes` says how many segments there are: a piece that changes tempo is
not summarised by any single number. `Song::from_midi` calls `set_sample_rate`
rather than building a fresh map — that is exactly the line that would have
thrown the curve away again.

**Ramps are still not built**, and deliberately: nothing can create one. MIDI
carries only constant tempo events and tempo automation (§12) does not exist.
The doc comment carries the closed form (`(1/m) * ln(b1/b0)` and its inverse)
so it stays a bounded addition rather than a redesign.

Verified: eight quarter notes, four at 120 bpm and four at 30, render to
exactly 12.0 s. At the old constant tempo it was 4.5 s.

### A master bus, so the fader is a fader

`DEMO_TRACK_GAIN_DB` was -12 dB of headroom picked by hand, and this document
said twice that a limiter was the real answer. It is. The master fader is at
unity now.

**Why the limiter is a brickwall and not a hope.** Let `t[n]` be the gain that
puts input frame *n* exactly on the ceiling, `m[n]` the minimum of `t` over the
last `W` frames, and `g[n]` the average of `m` over the last `W`. Every `m` in
that average covers a window containing frame `n - W + 1`, so
`g[n] <= t[n - W + 1]`. Delaying the audio by exactly `W - 1` frames therefore
guarantees the output never exceeds the ceiling — no clipping stage, no
dependence on how fast the gain "can" move. The averaging is what keeps it
clean: a sliding minimum alone steps the gain down the instant a peak enters
the window, and a step in gain on quiet material is a click.

The sliding minimum is a monotonic deque over a preallocated ring; scanning the
window per sample is a couple of hundred operations per sample per channel,
which an RT thread cannot spend on a safety net. Stereo-linked, because
limiting one channel by its own peak pulls the image toward the other every
time it fires. The ceiling is -0.3 dBFS: an inter-sample peak can exceed the
samples either side of it, and a converter reconstructing the waveform
overshoots a signal limited to exactly 1.0.

`PeakRmsMeter` was a `todo!()` too. Peak is *held* until reset — a meter
reporting only the last block's peak flickers past anything shorter than a UI
refresh, which is every transient worth seeing. `MasterNode` owns both and
publishes peaks and gain reduction through plain atomics rather than §13.3's
downsampled ring, because these are three scalars rather than a waveform. The
handle has to be taken before the graph goes to the audio callback, since after
that nothing owns the node.

Metering happens **after** the limiter: what a master meter is for is showing
what left the machine.

The RT-allocation test grew a third case covering the full chain — `BusSumNode`
and `MasterNode` were both newer than anything it checked.

Verified on a 15-part arrangement: unity master, peak 0.755, no limiting, where
the old -12 dB fader put it at 0.190. Pushed to +6 and +18 dB the peak sits at
exactly the 0.966 ceiling with zero clipped samples, and the render reports
3.9 dB and 15.9 dB of reduction at its hardest.

**Where things stand:** 234 tests, clippy and fmt clean.

## 2026-08-26: the mix becomes a mix, and patches learn to move

Four pieces, in the order they were built. Each closes a gap where a whole
feature existed as data with nothing reading it.

### `Layer::pan` finally does something

The importer has read every SF2 zone's `pan` generator since it was written
and **nothing applied it**: `Voice::render` wrote one mono buffer and
`SamplerNode` copied it across the bus. Every instrument was dead centre, and
a stereo SF2 sample — which the format stores as two mono zones panned hard
apart — folded to the middle.

`Voice::render` takes planar output now (`[left, right]`, or `[mono]`) and pans
each layer through the constant-power taper *before* the mix. Pan is per
**layer**, not per voice, because that is where the format puts it; panning
after the mix is exactly what collapses the stereo-pair case.

Three consequences worth naming:

- **Two filters per slot, one per channel.** Once layers are panned apart the
  channels carry different signals, and one shared filter state lets each
  side's history bleed into the other.
- **A mono render ignores pan** rather than folding it down. The centre
  pan-law gain on a signal with nowhere to pan is 3 dB of attenuation the
  caller never asked for — the same call `MixerTrackNode` already makes for a
  mono track.
- **The demo's mixer track became a balance control** (`PanLaw::Linear`). What
  reaches it is genuinely stereo, and a pan law is for placing a *mono* source.
  Applying one on top of the voice's own placement pulled a second 3 dB out of
  every centred track.

### Every part gets its own mixer track

A song had one fader. Whatever balance an arrangement asked for was discarded,
and muting anything muted the song.

**The graph now supports a node whose inputs are a different set from its
outputs** — the one shape `process_block` refused, and the reason a mixer could
not exist. `BufferPool::buffers_mut` takes them all in one disjoint borrow and
the input half is demoted to `&[f32]` afterwards; asking for them one at a
time borrows the pool twice. Both sides must be the same width, because a
width change is a downmix and that is a node's decision rather than the
graph's.

`BusSumNode` is the compiled form of `MixerTrack::output` (TDD §13.1): it adds
one bus into another and leaves its source alone, because a bus may be routed
*and* tapped by a send. A send (§13.2) is this plus a level and a pan.
`build_graph` lays the buses out as `[0, 1]` master and `[2 + 2i, 3 + 2i]` per
part.

**Where each control is applied is the part worth recording.** MIDI CC10 goes
to `Sampler::set_pan`, not to the part's mixer track. A track fader is a
*balance* control over a bus the voice has already placed on the
constant-power taper; a pan law is for putting an essentially-mono source
somewhere. Applying the track's law on top of the voice's placement pulls a
second 3 dB out of every centred part, and panning a track hard over throws
away half the signal instead of moving it. Channel pan is read per block rather
than captured at note-on, so moving a part moves the notes already sounding.

CC7 goes through the GM2/DLS curve `40 * log10(v / 127)`, and a channel that
sends none takes MIDI's own reset value of 100 — about -4 dB, not unity.
Reading the absent case as unity would put every silent channel above the ones
that spelled the default out. CC7 of 0 lands on a -96 dB floor rather than
`-inf`, which no fader can hold. First value per channel wins, exactly as for a
program change; a swept controller is an automation curve and there is nowhere
to put one yet. The channel listing prints both, so a part that arrives silent
because its file said so is visible rather than a mystery.

Verified end to end: two parts hard-panned apart render fully decorrelated
(rms(L-R) 0.081 against 0.057/0.055 a side, where it was exactly 0 before), and
a 15-part arrangement reports each part's pan and level and peaks at 0.190.

### Envelopes and LFOs become modulation sources

The matrix had two live sources — velocity and key — both fixed for the length
of a note. Nothing in a patch could move while it sounded, which rules out
every filter envelope, every vibrato and every tremolo.

**`Oscillator::next_sample` was a `todo!()`** and would have panicked on
anything that reached it. It is PolyBLEP-corrected for `Saw` and `Square`,
naive for `Sine` (no harmonics to alias), `Triangle` (harmonics fall off as
1/n² and are at the noise floor by Nyquist) and `Noise` (broadband on purpose).
Every shape is phase-aligned with the sine, so changing an LFO's shape moves
the waveform without jumping its position.

The aliasing test measures **energy below the fundamental**, where a correct
oscillator has none at all — a naive saw at 5 kHz folds its 9th, 10th and 19th
harmonics down there, and that is precisely the metallic ringing a naive
digital oscillator is recognisable by. Hann-windowed, or the fundamental's
leakage drowns what is being measured.

**Modulation runs at block rate** — 375 Hz at the engine's 128-frame blocks,
the same order as every hardware sampler ever shipped. The amp envelope stays
per-sample, because it is a gain rather than a control value and a stepped one
buzzes on fast attacks. Only the sources some route actually names are
advanced; `via` counts as much as `source`.

Newly reachable, since a moving source is what makes them worth having:
`LayerPitch` (cents, so a route is the same interval wherever the note sits),
`LayerGain` (decibels, so a tremolo is symmetric in loudness) and
`FilterResonance`. `Envelope(0)` reads the amp envelope as well as driving the
amp stage.

A note on testing: the first amp-envelope test **passed with the feature
absent**. On a single-frequency tone a lowpass changes amplitude and not shape,
so every proxy for brightness is really a proxy for level, and the envelope
moves the level either way. It now compares against the same patch *without*
the route, which is the only form that isolates what the route did.

### And the importer reads SF2's modulation half

Both LFOs, the modulation envelope, and the six amounts connecting them to
pitch, cutoff and volume. Three unit conversions each produce a
plausible-sounding wrong result:

- **`sustainModEnv` is a decrease in 0.1% units, not centibels of
  attenuation** — `sustainVolEnv` is the latter, one generator away in the same
  table. Read as centibels, a half-sustained filter envelope collapses to
  nothing.
- **`decayModEnv` is the time for a 100% change**, the same rule the volume
  envelope's stage times follow. Read as a stage duration it stretches every
  filter envelope's decay by 1/(1 - sustain).
- **`modLfoToVolume` is in centibels** while every other amount here is in
  cents.

**One route per layer** for the per-layer destinations: an SF2 modulation
generator applies to the whole voice, while §7.5's pitch/gain/pan destinations
are addressed by layer index, so a key-split instrument routed only at layer 0
plays its upper half unmodulated.

`Lfo` gained `delay_s` and the voice honours it — `delayVibLFO` exists because
vibrato from the note's first sample is the most recognisable way a sampled
string section gives itself away. The oscillator keeps turning through the
delay; one that only started afterwards would always emerge at the same point
in its cycle as one with no delay at all.

`inspect_sf2` prints the LFOs and the matrix. Checked against the five
soundfonts on this machine: **none of them uses a single modulation
generator**, which is worth knowing — plain sample-playback fonts are the
common case, and this feature is for the ones that aren't.

**Where things stand:** 208 tests, clippy and fmt clean.

## 2026-08-27: multi-timbral playback, and three graph faults it exposed

A MIDI file now plays with **one instrument per part**, chosen from the file's
own program changes. On a five-part arrangement:

```
  channel 1     279 notes program=0  -> preset 279 "Grand Piano"
  channel 3     132 notes program=60 -> preset 54  "French Horns"
  channel 5     118 notes program=10 -> preset 7   "Music Box"
```

Preset lookup matches **bank and program together**. General MIDI puts melodic
programs in bank 0 and drum kits in bank 128, and matching on program alone
would hand a drum channel whichever melodic instrument shared its number. A
part whose program the soundfont doesn't contain falls back to `--preset`: a
soundfont is under no obligation to be a complete GM set, and playing the part
on something beats dropping it silently.

### Three faults in the graph, all invisible with one instrument

Going multi-timbral turned up three things that had been wrong all along and
could not show while the graph held a single source node.

1. **Events went to every node.** `TimedEvent::target` has existed since the
   type was written and the sequencer has always filled it in, but
   `process_block` handed every event to every node. With one sampler that
   changes nothing; with two it means every instrument plays every part. The
   filter now lives in `ProcessContext::events()` rather than in each node, so
   the routing rule has one home, and it returns an iterator rather than a
   slice because a node's events are not contiguous — the timeline is ordered
   by time, not by target. The raw slice is still reachable as `all_events`,
   renamed so that reaching for it is deliberate.

2. **Source nodes overwrote their output.** Two instruments on one bus meant
   whichever ran last was the only one anybody heard. Buses are now cleared
   once per block by the graph and sources add into them. `SamplerNode` renders
   into scratch and adds rather than `Sampler::render` becoming additive:
   clearing what it is given is the contract a plugin host expects of
   `fontelle-core`'s boundary (TDD §8.1), and that boundary should not bend to
   suit the DAW that happens to be its first host.

3. **`AudioNode::prepare` was never called by anything.** The trait declared
   it, every node implemented it, and the only call site in the tree was inside
   a test. The one source node was built from an already-prepared `Sampler`, so
   the omission stayed invisible until a node needed internal storage of its
   own — at which point it produced silence rather than an error.
   `CompiledGraph::prepare` now walks the schedule, and the device and offline
   paths both call it.

A note on the test for (1): it passed while the bug was present. Both fixture
patches were at the same level, so a node that wrongly played an event was
indistinguishable from one that correctly ignored it. Equal fixtures are worth
suspecting whenever a test is about *which* thing acted.

### The track fader is a fader now

`DEMO_TRACK_GAIN_DB` was chosen so the demo phrase's three coincident voices
would not clip. A whole arrangement through the same -12 dB lands about 20 dB
down: the five-part piece above peaked at 0.082. It is now a default rather
than a constant, `--gain-db` overrides it, and the render reports its peak so
the choice can be made on evidence — the same piece at unity peaks 0.328 with
no clipped samples. The real answer is a master limiter, which is M4 work.

**Where things stand:** 166 tests, clippy and fmt clean.

## 2026-08-26: Fontelle plays music

The headline: `--play-midi` imports a `.mid` file and plays it through the same
document -> sequencer -> timeline -> engine path the built-in phrase uses. That
is the first time the project has produced music rather than a test phrase, and
it is what makes the rest of it judgeable by ear.

```sh
cargo run -p fontelle-app -- --play-sf2 "<path.sf2>" --preset <n> \
    --play-midi "<file.mid>" [--midi-channel <1-16> | --midi-all] \
    [--render-wav <out.wav>]
```

Verified on real files: a 83-note piece at 105 bpm read from the file's own
tempo event, and a 5780-event one played live through the device with the
zero-allocation guard active, exit 0, no violation. A 7830-note file bounces to
124 s at peak 0.138 with zero clipped samples.

### What MIDI import does and does not do

TDD §14.6 asks for tracks mapped to instrument channels, tempo and time
signature into the tempo map, and no silent discarding of channel/CC data. What
landed is notes, their timing converted to the project's resolution, and the
file's initial tempo. Stated plainly, because each of these is a missing
feature rather than a wrong result:

- Every selected MIDI channel lands on **one** document channel, so playback is
  monotimbral. Multi-timbral needs a patch per channel, which needs the user to
  have chosen an instrument per channel — UI work, not import work.
- Only the **first** tempo event is read. Tempo changes need the piecewise
  `TempoMap` that lands with M3; a single constant is what the map can hold
  today, and averaging would be worse than being clear.
- Time signature, program changes and CC are read far enough to *report* but do
  not affect the document yet. The channel listing prints each channel's note
  count and program so nothing vanishes silently.

Three details that would each have produced a plausible-sounding wrong result:

- **A note-on with zero velocity is a note-off.** Almost every real file uses
  it in place of an explicit one. Read literally it starts a silent note that
  never ends, and the piece plays as one endless chord.
- **Channel 10 is percussion** — note numbers select drums, not pitches.
  Excluded by default (`MidiChannels::Melodic`), because arriving by accident
  through a melodic patch is noise; reachable with `--midi-all` or
  `--midi-channel 10`, because it is real content in the file.
- **Tick conversion rounds to nearest.** Truncating drags every note early and
  rounding up drags every note late; either bias is systematic, so at a source
  resolution that does not divide 960 the result is a rhythm consistently
  wrong rather than randomly jittered.

SMPTE-timecode files are refused with a message explaining why, rather than
imported with wrong rhythm: their timing is in real seconds, and placing a note
on a musical grid from that needs the tempo map inverted.

Channels are 1-based on the command line and 0-based in the file, because that
is how every DAW and every piece of MIDI documentation numbers them.

### The mod matrix (next-steps item 5 — now closed)

`ModMatrix::evaluate` was a `todo!()`, so TDD §7.5's "flexibility that a
free-form patch graph would otherwise provide" provided none, and the filter
had nothing able to move it.

`evaluate` sums the routes aimed at one destination, each shaped by its curve
and scaled by depth and optional `via`. The result is deliberately unclamped:
what a sum of contributions means belongs to the destination — cents for pitch
and cutoff, decibels for gain — and clamping here would cap combinations the
destination represents perfectly well. `ModDest::full_scale` is where each
destination declares its unit range, since depth is normalised.

Every curve is an identity at 0 and ±1, so a full-depth route still reaches
full depth whatever curve it carries, and every curve preserves sign so a
bipolar source is shaped symmetrically rather than folded to one side.
`Curve::Quantised` carries its own step count: there is no single count right
for both a two-position switch and a 24-note arpeggio.

**`ModRoute` gained an `invert` flag that TDD §7.5's field list does not have.**
The format the importer must represent does: an SF2 modulator carries a
direction bit, and both of its always-present defaults use the negative
direction. "Velocity to filter cutoff, -2400 cents" means full cutoff at full
velocity falling two octaves toward silence — an offset from full scale, which
a plain product of source and depth cannot express at any depth.

Cutoff modulation is applied in cents so it scales the corner rather than
shifting it: an octave down means the same thing at 200 Hz as at 8 kHz. Only
the note-on sources are live — LFOs are not built, envelopes are not yet
exposed as sources — and the rest read as at-rest rather than as
plausible-looking numbers.

Measured on SGM-v2.01's "Halo Pad" (a 579 Hz lowpass at Q 2.72), the demo's
40/80/120 crescendo now opens the tone as well as the level: a high-frequency
energy proxy reads 0.0346 / 0.0429 / 0.0514 across the three notes.

**Where things stand:** 156 tests, clippy and fmt clean.

## 2026-08-25: the sampler starts behaving like an instrument

Two fixes, both from the "Next steps" list, both audible, both test-first.

### Envelope curve shape (next-steps item 2 — now closed)

`EnvelopeGenerator` decayed and released as a straight line in **amplitude**.
SF2 defines the volume envelope as a straight line in **decibels**, and it also
does not mean what you'd assume by "decay time": SF2 2.04 defines
`decayVolEnv`/`releaseVolEnv` as *the time for a 100% change in the envelope
value*, with full attenuation being 1000 centibels. So a stage time is the time
to travel **100 dB**, not the duration of the stage — a decay down to a -6 dB
sustain takes 6% of `decay_s`, not all of it.

Reading those numbers as stage durations stretches a decay by more than an
order of magnitude. Measured on `F-Zero.sf2` preset 0 ("FZ Electric Piano 2",
`decay_s` 5.518 s, sustain at full attenuation), rendering the demo phrase with
each curve and taking the RMS envelope of the held chord in 50 ms windows:

| t (s) | old, linear in amplitude | new, linear in dB |
|---|---|---|
| 0.75 | -16.01 dBFS | -16.43 dBFS |
| 1.00 | -16.02 | -20.53 |
| 1.25 | -16.54 | -25.14 |
| 1.50 | -16.77 | -29.43 |
| 1.70 | -17.62 | -33.61 |

The old curve loses **1.6 dB in a full second** — an electric piano that
doesn't decay. The new one loses 17 dB, in a straight line, at 18.7 dB/s
against the 18.1 dB/s the file's 5.518 s decay specifies. (The small excess is
the sample's own decay adding to the envelope's.)

**How it's built.** `EnvelopeConfig` gained a `curve: EnvelopeCurve` field —
`Linear` or `Decibel`. This is a flag rather than a rewrite because the
distinction is real: SF2 puts only the *volume* envelope in the dB domain. Its
modulation envelope is linear, and so is any envelope used as a general control
source, so `Linear` stays the default and `sf2_import` sets `Decibel` on the
volume envelope only. On the decibel curve the attack also stays a linear
amplitude ramp — that is what SF2 specifies, and an exponential attack would
make every note's onset audibly soft. `EnvelopeGenerator::stage_duration` is
where the "100 dB span" rule lives; the level within a stage is still computed
from elapsed time rather than accumulated sample-to-sample, which costs an
`exp2` per sample but is the only form that stays correct when the mod matrix
moves a stage time *during* the stage.

Tests: four new ones in `fontelle-dsp/src/envelope.rs` (constant dB rate,
the 100 dB stage-time rule, release to the floor, attack stays linear) plus
curve assertions in `fontelle-assets/tests/sf2_import.rs`.

### Velocity → loudness (next-steps item 3 — now closed)

Velocity 1 and velocity 127 used to sound identical. They don't now.

The previous entry framed this as a choice between a cheap `Voice::trigger`
shortcut and the "proper" SF2 §8.4 default modulator. That framing was wrong,
and the reason is worth recording: the SF2 default
velocity → initial-attenuation modulator (concave curve, negative direction,
amount 960 cB) has a closed form. Running that 96 dB concave span at 960 cB
works out to **amplitude proportional to the square of normalised velocity** —
velocity 64 is ~12 dB down, not 6. So the cheap path and the spec-correct
answer are the same code. `fontelle_core::velocity_to_gain` computes it
directly and `Voice::trigger` folds it into each layer's gain once per note, so
it costs nothing per sample.

What is *not* done: `ModMatrix::evaluate` is still `todo!()`, so a file that
**overrides** the default modulator is not honoured. The overwhelming majority
don't. When the mod matrix lands, this becomes the seeded default route rather
than a hardcoded one, and nothing about the current shape blocks that.

Verified on the render, isolating onset peaks of the demo's three run notes
(one layer, one sample, so velocity is the only variable):

| velocity | onset peak | ratio to vel 40 | (v/127)² prediction |
|---|---|---|---|
| 40 | 0.0170 | 1.000 | 1.000 |
| 80 | 0.0682 | 4.011 | 4.000 |
| 120 | 0.1525 | 8.973 | 9.000 |

### The demo phrase now demonstrates it

The opening run was three notes at a flat velocity of 100. It's now a
crescendo — 40 / 80 / 120, a 19 dB swell — because otherwise the only way to
hear the feature is to edit the source. The held chord stays at 100.
`DEMO_TRACK_GAIN_DB` is unchanged at -12 dB; on `F-Zero.sf2` preset 0 the demo
now peaks at 0.296 rather than 0.477, which is quieter but has honest headroom.

**Where the demo stands:** 132 tests, clippy and fmt clean, played on real
hardware from both a 97 KB soundfont and a 325 MB one.

```sh
cargo run -p fontelle-app -- --play-sf2 "<path.sf2>" --preset <n>
cargo run -p fontelle-app -- --play-sf2 "<path.sf2>" --preset <n> --render-wav /tmp/out.wav
cargo run -p fontelle-assets --example inspect_sf2 -- "<path.sf2>" <n>
```

### Interpolation: the windowed-sinc kernel, and an export-quality path

**A correction to the previous entry's next-steps list, which claimed
"everything currently runs on `Draft` (linear)".** That was wrong. Both
`PlaybackConfig::default()` and the SF2 importer already set
`Interpolation::Normal`, so playback has been on 4-point Hermite all along;
every `Draft` in the tree is in a test fixture. The claim was written from a
call-site grep without checking the defaults it was asserting about.

The real gap in TDD §7.6 was that `High` and `Ultra` were `todo!()` — they
panicked if selected — and that nothing implemented the "playback and render
quality are independent settings" half of the section.

`High` is now an 8-point Blackman-windowed sinc. RMS error against an analytic
sine, sampled across fractional positions:

| content (cycles/sample) | Draft | Normal | High |
|---|---|---|---|
| 0.05 | 0.006372 | 0.000272 | 0.000194 |
| 0.10 | 0.025422 | 0.002595 | 0.000463 |
| 0.20 | 0.098284 | 0.028565 | 0.002011 |
| 0.30 | 0.212150 | 0.114248 | 0.038362 |
| 0.40 | 0.351865 | 0.276654 | 0.195752 |

At 0.2 cycles per sample — ordinary upper-mid content — it is 14x more accurate
than Hermite. The advantage narrows near Nyquist (0.4) because an 8-tap
Blackman window has real passband droop up there; content that high is already
compromised by then, and widening the kernel is what `Ultra` is for.

Two implementation notes worth keeping:

- The kernel is evaluated directly rather than from a precomputed phase table.
  A table would be faster, but it has to be built somewhere, and building it
  lazily would put an allocation on the audio thread. If `High` ever needs to
  run at playback rates the table belongs in `prepare()`, not behind a
  `LazyLock`.
- The taps are normalised to sum to unity. They don't naturally at an arbitrary
  phase, and the residue is a periodic amplitude ripple on steady material —
  audible as a whine. There's a test for it.

`Ultra` stays unimplemented, and deliberately: "16-point sinc + 2x oversample"
is not expressible in a point-interpolator, because oversampling is a property
of a stream of output samples. It needs a stateful resampler. The `todo!()`
now says so rather than implying the kernel was merely unwritten.

For the quality split, `Sampler::set_render_quality` overrides every layer's
mode for a render pass. It is engine-side state, not document data — an export
must not mutate the patch — and `None` means the patch decides. `fontelle-app`
sets it to `High` for `--render-wav`, which now reports the mode it rendered
at. On the F-Zero electric piano the bounce differs from a `Normal` render by
1.19% of signal level; that sample is transposed *downward*, where
interpolation error is small, so this is the quiet end of the effect.

### The voice filter is real, and SF2 files that use it are now honoured

`SvfFilter::coeffs` and `process` were both `todo!()`, `DcBlocker::process` too,
and `Voice` never ran a filter at all — TDD §7.4's fixed topology
(`layers -> mix -> Filter1 -> Filter2 -> Amp -> Pan`) was missing its middle.
The importer also disabled both slots unconditionally, so a file's
`initialFilterFc` and `initialFilterQ` were discarded on the way in. A patch
authored as a dark resonant pad played back wide open, which sounds like a bad
soundfont rather than like a missing feature.

**The filter.** A Cytomic-form TPT state-variable filter with zero-delay
feedback. Two decisions worth recording:

- The mode lives in output-mix coefficients (`m0`/`m1`/`m2`) rather than in
  `process`, so the per-sample path has no branch on mode and a mod route that
  moves cutoff only has to rebuild the coefficient struct. All seven modes are
  implemented, including bell and both shelves, which fold their gain into the
  damping and the corner respectively.
- `FilterSlot::resonance` is **Q**, not a normalised dial. At the Butterworth
  value of `1/sqrt(2)` a lowpass is exactly -3 dB at its cutoff, and above that
  the magnitude at the corner *is* Q — so the parameter is checkable, and it
  matches both `EqBand::q` and SF2, whose `initialFilterQ` is defined as peak
  height above DC gain.

Coefficients are pre-warped with `tan()`. Without that the actual corner drifts
from the requested one, increasingly so toward Nyquist; there's a test at 100 Hz,
1 kHz, and 10 kHz for exactly that.

**In the voice.** Filter state is per voice, not per patch — two notes sounding
at once each need their own memory, and sharing one would make a voice's output
depend on which other voices happened to render first. The filters reset on
`trigger`, because a voice comes back out of the pool carrying the last note's
memory and would otherwise discharge it into the new note as a click. Both are
covered by tests, and both are the same class of bug as the shared-buffer
envelope fault fixed earlier.

**In the importer.** `initialFilterFc` is absolute cents against SF2's 8.176 Hz
anchor; `initialFilterQ` is centibels of peak height, with 3.01 dB taken off
first because the spec defines 0 as *no* resonance and no resonance is
Butterworth rather than unity Q. Skipping that offset would put a 3 dB bump at
the corner of every unresonant zone in every file. A cutoff at or above the
spec's wide-open 13500 cents imports disabled rather than as a filter that costs
every voice work to do nothing.

Verified against real files. `F-Zero.sf2` uses no filtering anywhere — every
preset sits at the default and imports disabled, correctly. `SGM-v2.01` does:
"Synth Vox" is a 599 Hz lowpass, and "Halo Pad" a 579 Hz lowpass at Q 2.72.
Rendering Halo Pad with and without the filter, the ratio of 2-8 kHz to
100-600 Hz energy goes from 0.031 to effectively zero — that content was never
meant to be there.

`inspect_sf2` now prints both filter slots, so whether a file uses this is
visible without reading the code.

### Two smaller things closed alongside it

- **Interpolation precedence** (the open question from the previous entry).
  `PlaybackConfig::interpolation` is now `Option<Interpolation>`: `None` — the
  default, and what SF2 import produces, since the format has no interpolation
  generator — follows the session's quality, and `Some` pins the layer.
  `Sampler::set_quality` sets the session value, defaulting to `Normal`, and
  `fontelle-app` uses `PLAYBACK_QUALITY`/`RENDER_QUALITY`. A pinned layer is
  honoured in playback and export alike and is never silently upgraded for a
  bounce: `Draft`'s aliasing is a legitimate character choice in a sampler, so
  treating the modes as a ladder the export may climb would quietly change how
  a deliberately lo-fi patch sounds in the mix it ships in.
- **`DcBlocker`** is implemented (one-pole/one-zero, corner placed from
  `cutoff_hz`), so TDD §7.8.4's import-time DC removal has its primitive.

### Still wrong, found while doing the above

- **Release times are the SF2 default on most files** (-12000 timecents,
  ~1 ms), so a note-off is effectively a hard stop. That's faithful to the
  file and is what other players do, but it means the demo's tail is silent
  and note-offs are on the edge of clicking. Nothing to fix in our code; worth
  knowing before blaming the sampler.
- **`import_sf2` reads the entire file into memory** — the 325 MB SGM
  soundfont is fully resident to play three notes. TDD §7.7's streaming is the
  answer and it is not built.

## 2026-08-23 update: the first real-hardware bug report

The manual-verification commands from the previous update were run on real
hardware. The synthetic
tone (`manual_audio_output`) worked. `fontelle-app -- --play-sf2` on a real SF2
file (`Square.sf2`) crashed the process with `SIGABRT`.

**Two real bugs, both fixed, both covered by a test now:**

1. `AudioDevice::start_output_stream` called `mark_current_thread_rt()` *before*
   `audio_thread_priority::promote_current_thread_to_real_time()`. That
   promotion goes through `rtkit` over D-Bus on Linux, which legitimately
   allocates internally for the one-time handshake — so the very first audio
   callback tripped INVARIANT 1 on an allocation that was never a violation.
   **Fix:** the whole first callback is now treated as warm-up — do the
   promotion, output one silent block (~2.7ms, inaudible), and only start
   tagging the thread RT and processing real audio from the *second* callback
   on. This also covers any backend-internal first-use lazy setup (format
   conversion buffers etc.), not just the specific rtkit call.
2. **Independent of (1), and worth fixing regardless:** `RtGuardAllocator`'s own
   violation-report `panic!(...)` formats a message, which allocates, which
   re-enters `alloc()` while the thread is still tagged RT, which panics again
   mid-unwind — Rust aborts on a double panic. So *any* real INVARIANT 1
   violation, past or future, would show up as an unhelpful `SIGABRT` with no
   message rather than a clean diagnostic. **Fix:** `assert_not_rt` clears the
   RT flag before calling `panic!`, so the panic machinery's own allocation is
   allowed through.

Bug (2) is reproduced and regression-tested without any audio hardware:
`crates/fontelle-engine/tests/rt_guard_panic_safety.rs` is its own binary (every
`tests/*.rs` file is) with its own `#[global_allocator] = RtGuardAllocator`,
confirmed to reproduce the exact `SIGABRT` before the fix and pass cleanly
after. Bug (1) doesn't have an automated test — it's specifically about real
`cpal`/`rtkit` interaction — but the diagnosis was done by inspecting the real
`Square.sf2` file's imported `Patch` (`cargo run -p fontelle-assets --example
inspect_sf2 -- <path>`, kept as a permanent dev tool) to rule out an
SF2-import-side cause first, silently and without touching audio.

**Update after further hardware runs:** the "rtkit allocates" theory (bug 1)
was wrong, or at least incomplete. `--play-sf2` was run repeatedly against a
real device to chase this down. Findings,
in the order they happened:

3. Same crash, byte-for-byte identical (`dealloc … size=16, align=4`), even
   with the `audio_thread_priority::promote_current_thread_to_real_time` call
   temporarily disabled entirely — ruling out rtkit as the cause of *this*
   specific violation. And a new `no_allocation_during_render.rs` test run for
   800 blocks (~2.1s, matching the real playback duration) with the same
   **synthetic** single-layer patch passed clean — so it isn't just "steady
   state for long enough" either. A throwaway diagnostic
   (`FONTELLE_TEST_SF2=<real path> cargo test … zz_scratch_diag`, since
   deleted per the tests-first-strict rule: learn the shape, then delete) that
   imports the **real** Square.sf2 file (7 layers) and drives `process_block`
   in a direct loop for 800 blocks — no `cpal`, no real device — *also* passed
   clean. So neither "real file" nor "real duration" alone reproduces it
   off-hardware; only the real `cpal`/ALSA callback path does.
4. **The bigger, structural finding:** on Linux, ALSA calls the audio callback
   through a C function pointer, and a Rust panic unwinding across that
   boundary is undefined behaviour — the runtime detects it and hard-aborts
   the whole process, independent of *why* the panic happened or how clean its
   message is. This explains why `RUST_BACKTRACE=1` never printed a backtrace
   on real hardware even after fix (2) stopped the double-panic: the abort was
   happening for an unrelated reason (the FFI-unwind guard), not because
   backtrace capture itself panicked.
5. **Fix:** the entire callback body now runs inside `catch_unwind`
   (unconditionally, not just in debug builds — this is standard practice for
   audio callbacks generally, not an INVARIANT-1-specific aid). On a caught
   panic: report once via `eprintln!`, then output silence for the rest of the
   stream's life instead of letting anything reach ALSA's C boundary.
   **Confirmed working:** `cargo run -p fontelle-app -- --play-sf2 <real
   file>` now runs its full ~2.16s and exits 0 — no more `SIGABRT`, timed with
   `time` to be sure it wasn't exiting early. (Curiosity, not yet chased
   further: adding `RUST_BACKTRACE=1` back *does* still crash the process —
   backtrace capture itself appears to misbehave in this FFI-adjacent
   context, a separate, lower-priority issue a normal run without that env
   var doesn't hit.)

**The root allocation itself (bug 3) is still not found** — *at the time this
section was written.* **It has since been root-caused, and the theory below is
wrong.** It is not inside `cpal`'s ALSA backend as a per-block allocation; it
was our own RT tag outliving the callback, so cpal's worker-thread *teardown*
tripped the guard. Kept here unedited as a record of how the wrong hypothesis
looked from inside — see "2026-08-24, later" below for what it actually was.

> ~~It's real — the panic message and size/align are exactly reproducible —
> but every attempt to reproduce it through direct, off-hardware simulation
> failed to trigger it; only the genuine `cpal`→ALSA callback path does. That
> strongly suggests it originates inside `cpal`'s ALSA backend itself, not in
> Fontelle's own code. Its impact is fully contained by fix 5 regardless: one
> bad/silent block, not a crash.~~

## 2026-08-24, latest: the distortion was a real bug — short device blocks

**Symptom:** heavily distorted, bitcrushed-sounding output. Persisted after
switching from the accidental whale preset to a real piano.

**Cause:** `CompiledGraph::process_block` rendered each node's *entire buffer*
(`BLOCK_SIZE` = 128 frames) regardless of how many frames `sample_range`
actually asked for. **The device does not deliver `BLOCK_SIZE` frames.** On
this machine ALSA delivers **85**. So every single callback advanced every
voice 128 samples while emitting only 85 — **34% of the audio discarded, with
a phase jump, ~560 times a second.** That is precisely what a bitcrusher does,
which is why it sounded like one.

**Why it took a while:** the offline WAV render (built for
exactly this purpose) was *clean* — no clipping, no discontinuities, peak
0.37, max sample-to-sample delta 0.03. That looked like it exonerated the
engine, but it was the clue: `render_offline` always uses full 128-frame
blocks, so it never exercised the short-block path. The divergence between the
two paths was the bug.

**Diagnostic sequence that got there,** all of it useful to keep:
1. Rendered to WAV and analysed the waveform (peak / RMS / zero-crossing rate
   / first-difference spikes) — ruled out clipping and clicks.
2. Extracted the raw sample from the SF2 with an independent Python script and
   compared against our decoded PCM: **byte-identical** (same length, peak,
   sum, abs-sum, and mid-buffer window). Ruled out the importer entirely.
3. Dumped the instrument's real generators — confirmed `OverridingRootKey=75`
   is genuinely in the file, so the transposition was faithful too.
4. That left the device path. Recording the callback's real frame count (via
   an atomic — an `eprintln!` there allocates, and the RT guard correctly
   rejected it) showed **85 frames, remainder 85**.

**Fix:** `process_block` derives `frames` from `sample_range` and hands each
node `&mut buffer[..frames]`. Covered by
`graph::tests::a_short_block_renders_the_same_audio_as_a_full_one`, which
renders the same material in 128-frame and 85-frame chunks over a *ramp*
sample (a constant would have masked the position error) and requires them to
match sample-for-sample. It failed at exactly frame 85 before the fix —
`0.332` (correct position 85) against `0.5` (position 128).

**Confirmed by ear on hardware: "that sounded much better."**

### Also added this round

- **Offline bounce.** `render_offline` + `write_wav16`, exposed as
  `--render-wav <out>`. Drives the identical signal path the device does, so
  the WAV is what you'd have heard — which makes the audio inspectable,
  diffable, and testable without a sound card. This is the tool that made the
  diagnosis above tractable, and it's the foundation of TDD §22's M6 export.
  `write_wav16` returns the count of clipped samples rather than silently
  wrapping them, since silent wrapping turns clipping into noise that looks
  like a synthesis bug.
- **`inspect_sf2` prints a PCM fingerprint** (length, peak, sum, abs-sum, and
  a mid-buffer window) so our decode can be checked against an independent
  extraction of the same file. That check is what ruled the importer out here.

### Known-wrong, not yet fixed: envelope curve shape

`EnvelopeGenerator`'s decay and release are **linear in amplitude**. SF2's
volume envelope is defined as linear in *centibels* — i.e. exponential in
amplitude. A 13.8 s piano decay therefore holds level far too long and then
falls off a cliff, instead of the natural exponential taper. Not what caused
the distortion above, and not audible in a 2.25 s demo, but it *is* wrong
against TDD §7.3's instruction to follow RustySynth's semantics, and it will
be obvious on any sustained instrument. Fix belongs with M1's sampler work.

## 2026-08-24, later: hardware run — the M0 gate is CLOSED, and the phantom
## allocation is finally root-caused

This round was run
and heard on real hardware rather than reasoned about.

### The `dealloc size=16, align=4` mystery: solved, and it was ours

Three prior sessions failed to reproduce this off-hardware and concluded it
was "very likely inside `cpal`'s ALSA backend, not Fontelle's own code." **That
conclusion was half right and the impact assessment was wrong.** The
allocation really is cpal's, but the *bug* was ours.

Method that finally cracked it: `RUST_BACKTRACE=1` still killed the process
(as documented), so instead of panicking, a temporary diagnostic captured the
violation into a static and symbolised it later on the main thread. The
backtrace was immediate and unambiguous:

```
6: <alloc::boxed::Box<[libc::unix::pollfd]> as Drop>::drop
8: core::ptr::drop_glue::<cpal::host::alsa::StreamWorkerContext>
9: cpal::host::alsa::output_stream_worker  (cpal-0.18.2 alsa/mod.rs:1023)
```

`pollfd` is 8 bytes at align 4; a boxed slice of two is exactly
`size=16, align=4`. Line 1023 is the **closing brace** of
`output_stream_worker` — this is cpal's ALSA worker thread freeing its own
poll descriptors *as the thread exits*, on the thread we had tagged RT and
never untagged.

**So it was never a per-block allocation at all — it was stream teardown.**
Every previous attempt to reproduce it by rendering blocks in a loop was
looking in the wrong place by construction. The "one bad/silent block early in
playback" characterisation in the earlier sections of this file is wrong;
the timing in the failing run says so plainly (the panic printed *after* the
duration line, at 2.288s of a 2.25s run). Playback was almost certainly fine
the whole time.

**Fix:** `rt_guard::with_rt_thread(f)` tags the thread, runs `f`, and clears
the tag on every exit path including unwind. `AudioDevice`'s callback now
wraps only its own processing in it. INVARIANT 1 is about *our* code not
allocating; the backend owns that thread between callbacks and its own
allocations there are legitimate and none of our business. Covered by
`fontelle-engine/tests/rt_tag_is_scoped.rs`.

**Result:** `--play-sf2` now runs clean — no violation, full duration, exit 0,
stable across repeated runs.

### It sounded distorted, and mostly it wasn't a bug

First listening test: heavily distorted and bitcrushed. Two causes, one of them
embarrassing:

1. **We were playing a whale.** `import_sf2` only ever imported preset 0, and
   SF2 files store presets in arbitrary order. Preset 0 of `Secret_of_Mana.sf2`
   is `SOM Orca` — a sound effect sampled at **1824 Hz**. The actual piano is
   at index 27. The documented "only the first preset is imported" scope cut
   was quietly ruinous in practice, because "first in file" has nothing to do
   with "the instrument anyone wants."
   **Fix:** `list_presets()` and `import_sf2_preset(path, index, store)`;
   `import_sf2` is now the index-0 case of the latter, unchanged. `--play-sf2`
   prints the full preset list with the selected one marked, and takes
   `--preset <n>`. `inspect_sf2` takes an optional preset index too. Tested
   against a new **multi-preset** hand-built fixture (the old fixture builder
   could only make single-preset files, so preset *selection* had been
   literally untestable).
2. **Real clipping.** The demo's three-voice chord sums to roughly 3.0 with no
   headroom anywhere, which hard-clips at the device. Velocity would normally
   handle this, but velocity still doesn't affect loudness. **Fix:** the
   demo's mixer track now sits at -12 dB (`DEMO_TRACK_GAIN_DB`) — the fader
   doing exactly the job a fader exists for. Not a general answer; per-voice
   velocity response and a master limiter are the real ones, both later work.

Verified after both fixes: `SOM Piano` (preset 27) imports as a genuine
instrument — `sample_rate=28960`, forward loop, a 13.8 s decay to near-silent
sustain — and plays clean.

### Also fixed: the unquoted-path trap, for good

An unquoted path with spaces had now cost two sessions (`fish` splits
`/…/FL 2026 Linux/…` into three arguments, and Fontelle reported "no such
file: /mnt/disks/3tb/Apps/FL" — a path nobody typed). Documenting it in this
file was not a fix. `resolve_sf2_path` now detects the case — non-flag
arguments following an unresolvable path, rejoined with spaces, that *do*
resolve — and prints the correctly-quoted command to copy. Bad paths exit(1)
with a clean message instead of a panic. Covered by
`fontelle-app/tests/cli_path.rs`.

### M0 gate status: **closed**

Every clause of TDD §22's gate is implemented, tested, and now heard on real
hardware at 128 frames / 48 kHz with the zero-allocation assertion active and
passing. 88 tests across the workspace.

## 2026-08-24 update: code review, two real bugs, and the M0 gate closes

Before more feature work, the existing scaffolding got a full review — every
non-stub source file read directly, rather than trusting the docs' claims
about them.

### Verdict on the existing code

**Good, and better than "scaffolding" implies.** Specifically worth keeping:
`fontelle-dsp`'s `EnvelopeGenerator` (the zero-duration-stage cascade and the
half-sample drift tolerance are both correct and non-obvious) and
`interpolate` (Hermite verified against a linear ramp); `fontelle-assets`'
hand-built SF2 byte-stream fixtures, which assert exact values by
round-tripping the timecent/centibel math through its own inverse rather than
hardcoding magic numbers; `rt_guard`'s un-flag-before-panic fix. The stub
crates are honest — real type shapes with `todo!()` bodies and doc comments
tying each back to a TDD section, not empty files pretending to be done.

**Two real bugs found, both now fixed test-first.**

1. **Polyphony was broken — every voice re-enveloped every voice mixed before
   it.** `Voice::render` added its layers across the whole shared output
   buffer, then multiplied *the entire buffer* by its own amp envelope. Since
   `Sampler::render` mixes all active voices into one buffer, voice 2's
   envelope also scaled voice 1's already-mixed output, voice 3's scaled both,
   and so on. Audibly: **hold a chord, play another note, and the held notes
   duck to near-silence for the length of the new note's attack.** Measured in
   the reproducing test before the fix: a sustained voice at 0.799 rms dropped
   to **0.0027** when a second note started. Every pre-existing test missed it
   because they all used `sustain_level: 1.0`, where multiplying by the
   envelope is a no-op.
   **Fix:** `Voice::render` is now sample-major rather than layer-major —
   per-layer constants resolve once into a fixed-size stack array, then a
   single pass advances the envelope once per output sample and scales only
   *this* voice's own mixed sample before adding it. Same allocation profile,
   correct additive semantics. Covered by
   `sampler::tests::{each_voice_applies_its_envelope_only_to_its_own_contribution,
   a_new_notes_attack_does_not_duck_already_sounding_voices}`.
   The same rewrite also fixed a latent loop-wrap bug: the wrap was `if
   position >= loop_end`, a single subtraction, which is not enough when the
   playback step exceeds the loop length (extreme upward transposition of a
   short loop). It's a `while` now.
2. **`import_sf2` panicked out of bounds on a truncated or corrupt SF2.**
   `build_layer` indexed the `smpl` chunk using the sample header's
   `start`/`end` on trust; nothing in the parser cross-checks the two, so a
   header claiming a range past the real PCM data took the process down with
   `index out of bounds`. TDD §20.3 requires malformed files to "fail with a
   clear message" and names crashing as unacceptable — and the existing
   `catch_unwind` only wrapped `SoundFont2::load`, not the decode that follows
   it. **Fix:** the range is validated up front and returns a descriptive
   `ImportError`. Covered by
   `a_sample_header_pointing_past_the_end_of_the_pcm_data_fails_cleanly`.

**Smaller things noted, not fixed** (none are defects, but they're worth
knowing): `PlaybackConfig::default()` has `end_offset: 0.0`, so a layer built
from the default config and never assigned one renders silence — every real
caller sets it, but it's a footgun of a default. And velocity still has no
effect on loudness (`trigger` takes it, uses it only for vel-range testing);
that's the documented "modulators are ignored entirely" import cut plus the
unimplemented mod matrix, not a regression, but it does mean a demo can't yet
show dynamics.

### The M0 gate closes

3. **`MixerTrackNode` is real** — gain, pan (with a real pan law), mute,
   phase invert. Stereo when given two buffers, mono when given one; in mono
   `pan` is deliberately ignored rather than applying the centre pan-law gain,
   which would leave every mono track quietly 3dB down for no visible reason.
   Inserts, sends, metering, and solo are still M4.
4. **`PanLaw` moved to `fontelle-types`** so the engine node and the document
   model share one type (the engine can't depend on the model, TDD §4.1), and
   it grew a real `gains()` implementation. **An open question against the TDD:**
   the TDD names four laws but defines none, and on the usual reading "-6 dB"
   and "linear" are the same taper. They're kept distinct as `Minus6Db` = the
   linear taper (centre 0.5) and `Linear` = a balance-style control (centre at
   unity, only the far side attenuated). Tested by asserting each variant's
   centre attenuation matches the dB figure in its own name, and that the
   -3dB law really does hold `l² + r² == 1` across the sweep.
5. **`CompiledGraph::process_block` supports real multi-node chains.** Nodes
   process **in place**: a node consuming another's output declares the same
   buffer indices in `input_buffers` and `output_buffers`, and its buffers
   arrive already carrying the upstream signal. That's the default contract in
   every plugin API, and it sidesteps both a per-node copy and the aliasing
   problem of handing out `&[f32]` and `&mut [f32]` to one pool slot. Still
   capped at two buffers per node (mono or stereo); genuinely *distinct*
   input and output sets — a mixer send tapping one bus into another, a
   sidechain — need real disjoint multi-set borrowing and stay M4.
6. **The signal path is stereo end to end.** `SamplerNode` renders mono (the
   sampler still mono-sums; `Layer::pan` is unimplemented) and copies across
   its remaining output channels, which is what feeding a mono source into a
   stereo bus means. `AudioDevice` now interleaves bus *N* into device channel
   *N* instead of duplicating bus 0 everywhere.
7. **`fontelle-app` gained a `lib.rs`.** `demo_song` and `build_graph` live
   there so integration tests can import them — a `[[bin]]` can't be. `main.rs`
   is now thin. `--play-sf2` also takes an optional `--key <note>`.
8. **The demo is actual music now**, not one held note: a root/third/fifth run
   in eighth notes, then the triad held as a chord. The chord is the point —
   simultaneous voices are exactly what bug (1) broke, so the demo exercises
   the fix by ear. `manual_audio_output` plays the equivalent phrase and its
   header doc says what to listen for.

**New tests:** 74 total across the workspace (was 43). Notably
`fontelle-app/tests/vertical_slice.rs::the_full_m0_chain_renders_the_demo_song_through_a_mixer_track`,
which drives the *same* library functions `--play-sf2` uses — not a parallel
re-implementation that could drift — and
`no_allocation_during_render.rs::the_stereo_sampler_into_mixer_chain_does_not_allocate_per_block`,
which runs the real two-node stereo shape with a real `CompiledTimeline` and
event cursor under `RtGuardAllocator`. That second one was written as a
regression guard *after* the code it covers, unlike everything else here;
it passed first run.

**Still not verified on hardware.** Same caveat as the previous update, now
covering more: stereo interleaving, the mixer track, and the polyphony fix have
never been heard. Next steps item 1.

## 2026-08-23 update: the M0 gate's timeline-wiring piece closes

Picked up from "Next steps" item 1 below. Tests written and confirmed red
(`todo!()` panics or a signature mismatch that didn't compile against the
intended API) before each implementation, per the project's process rule.

1. **`fontelle_model::TempoMap`** is no longer a stub. **Scoped deliberately:**
   only a single constant-tempo segment — `bpm` never changes across a
   project. The full piecewise segment list + prefix-sum cache (ramps, a
   time-signature track, TDD §6.2's O(log n) requirement) is real work that
   still belongs to M3; this is enough for tick↔sample conversion to be
   honest (goes through `TempoMap`, never ad-hoc arithmetic, per TDD §6.1)
   for the M0 slice. Tested: an exact known conversion (a quarter note at
   120bpm/48kHz is exactly 24,000 samples), and an *exact* (not
   tolerance-based) round trip across 2,700+ tick values — exact because at
   that particular (bpm, sample-rate) pair, samples-per-tick is an integer,
   so there's no rounding error to paper over. `crates/fontelle-model/src/project.rs`.
2. **`fontelle_model::Project::lanes`** changed from a `Vec<Lane>` +
   parallel `Vec<LaneId>` (`Lane` had no way to recover its own ID) to a
   `SlotMap<LaneId, Lane>`, matching every other ID-addressed collection on
   `Project`. Found while implementing lane-mute checking in `compile()` —
   the parallel-array shape made an honest lookup by ID awkward. Nothing
   else referenced the old fields.
3. **`fontelle_sequencer::compile`** is no longer a stub. Turns a `Project`'s
   `ClipSource::Notes` clips into sample-timestamped `NoteOn`/`NoteOff`
   events, honouring clip mute and lane mute (TDD §11's "lane mute is a
   sequencer mute, not a mixer operation"), sorted by sample. **Signature
   changed from the original stub** to add a `channel_nodes: &HashMap<ChannelId,
   NodeId>` parameter — the crate has no dependency on `fontelle-engine`
   (TDD §4.1: `sequencer ──> types, model` only), so it has no way to
   discover which engine-side `NodeId` a document `ChannelId`'s compiled
   `SamplerNode` was assigned; whoever builds the graph passes the mapping
   in. **Scope cuts, both documented in the function's own doc comment:**
   `ClipSource::Automation`/`ClipSource::Audio` clips are silently skipped
   (M4/M6 respectively); `clip.prefab_link` is ignored, so prefab resolution
   (TDD §10.5) isn't wired in yet. `CompiledTimeline.index` (the sparse
   per-bar seek index) is left empty — not required for correctness, only
   for an O(1)-seek optimisation that needs the time-signature track TempoMap
   doesn't have yet. Tested: one-note compile produces the right two events
   at the right sample positions targeting the right node; sort order; clip
   mute, lane mute, and an unmapped channel all correctly produce zero
   events. `crates/fontelle-sequencer/src/compile.rs`.
4. **`fontelle_types::CompiledTimeline::events_for_block`** is new: given a
   monotonically-advancing `cursor` and a block's sample range, returns the
   contiguous slice of the (already sorted) `events` Vec inside that range —
   no allocation, no binary search needed given sequential calls, so it's
   safe to call from the RT thread. This is the piece that lets
   `AudioDevice`'s real callback feed a `CompiledGraph` real events instead
   of a hardcoded `&[]`. Tested directly (empty timeline, sequential-block
   partitioning, half-open boundary behaviour at a block edge, multiple
   events at one sample) without needing any engine or device machinery.
   `crates/fontelle-types/src/event.rs`.
5. **`fontelle_engine::AudioDevice::start_output_stream`** now takes a
   `CompiledTimeline` parameter and drives every block's `process_block`
   call with the real slice `events_for_block` returns, via a cursor kept
   across callback invocations. `timeline` gets the same `ManuallyDrop`
   treatment as `graph` and for the same reason (documented in the function's
   doc comment already): cpal tears the callback down on the RT thread
   itself, and dropping a `Vec` there would violate INVARIANT 1 exactly like
   dropping `graph` would. The manual hardware test
   (`fontelle-engine/tests/manual_audio_output.rs`) still triggers its note
   directly via `sampler.note_on()` before building the graph — unaffected,
   it just now passes `CompiledTimeline::empty()` for the new parameter.
6. **`fontelle-app --play-sf2`** no longer calls `sampler.note_on()` by
   hand. It builds the smallest `Project` that exercises the real path (one
   channel/lane/clip/note — not loaded from disk or built by a UI, neither
   exists yet), compiles it, and hands the result to `start_output_stream`.
   `Channel.patch_data` is left empty (documented in `play_sf2`'s doc
   comment): the real `Patch` is still built straight from the SF2 import,
   not round-tripped through the model's serialised form — that link is real
   work for whenever project save/load exists, not required to close this
   gate.
7. **New automated test:** `crates/fontelle-app/tests/vertical_slice.rs`
   assembles the exact same sequence `play_sf2` drives against a real
   device — `Project` → `compile` → `CompiledTimeline::events_for_block` →
   `CompiledGraph::process_block` — against a synthetic patch instead of a
   real device, so it runs in `cargo test --workspace` / CI. It's the
   closest thing to an automated M0-gate acceptance test that doesn't need
   speakers: confirms the compiled timeline has the right two events, that
   real sampler audio comes out while the note is active, and that both
   events get consumed by the end of playback.

**What this does and doesn't close:** TDD §22's M0 gate wording is "...one
sampler voice reading a real SF2 zone → **mixer track** → device out,
triggered by a note from a clip on the timeline...". Everything up through
"triggered by a note from a clip on the timeline" is now real and tested,
including on the real device path (mechanically — not re-verified on
hardware at that point, see below). The mixer track is still the one
literal piece of the gate's own wording that isn't built (`MixerTrackNode`
is still an empty struct) — that's next steps item 1 now, previously item 2.

**Not yet done, flagging explicitly:** this update hasn't been run against
real hardware (`cargo run -p fontelle-app -- --play-sf2 <path>`) since the
timeline-wiring change — the logic is covered by the new unit and
integration tests above, but "the compiled graph now reads events from a
`CompiledTimeline` instead of `&[]`" is exactly the kind of change that's
worth a real listen before calling it done-done, the same way the
raw-hardware bugs earlier in this file were only found that way.

## Where things stand — 2026-08-23

**Repo:** `github.com/Fopull-LLC/DAW-Fontelle`, private. Workspace builds clean
(`cargo check --workspace --all-targets`), clippy clean (`-D warnings`), full test
suite green (`cargo test --workspace`).

### The M0 gate (TDD §22), piece by piece

> "Audio callback → compiled graph → one sampler voice reading a real SF2 zone →
> mixer track → device out, triggered by a note from a clip on the timeline, at
> 128 frames / 48 kHz, with the zero-allocation assertion active and passing."

| Piece | Status |
|---|---|
| Zero-allocation debug assertion | **Done, real.** `fontelle-engine::RtGuardAllocator`, installed as `fontelle-app`'s `#[global_allocator]`. Panics if the RT-tagged thread allocates/reallocates/deallocates in debug builds. |
| Real SF2 zone → `Patch` | **Done, real,** with documented scope cuts (below). `fontelle_assets::import_sf2`. |
| One sampler voice rendering it | **Done, real.** `fontelle_core::{Sampler, Voice, VoicePool}` — envelope, pitch, looping, gain, key/vel range, voice stealing. |
| Compiled graph carrying a note to output | **Done, real,** scoped to source nodes only (see below). `fontelle_engine::{CompiledGraph, SamplerNode}`. |
| Device out | **Real, hardware-verified.** `fontelle_engine::AudioDevice::start_output_stream` opens a real `cpal` stream. Confirmed repeatedly on real hardware: the demo phrase plays audibly through a real SF2 preset, full duration, exit 0, with zero INVARIANT 1 violations. The long-standing `dealloc size=16, align=4` caveat is **resolved** — it was our RT tag outliving the callback, not a per-block allocation; see the "2026-08-24, later" section. |
| Mixer track | **Done, real, scoped to the fader.** `fontelle_engine::MixerTrackNode` — gain, pan (real pan laws), mute, phase invert, over a stereo bus pair, in the actual signal path of both `--play-sf2` and the manual test. Inserts, sends, metering, and solo are M4. |
| Triggered by a clip on the timeline | **Done, real, scoped** (see "2026-08-23 update: the M0 gate's timeline-wiring piece closes" above). `fontelle_sequencer::compile` turns a `Project`'s notes into a `CompiledTimeline`; `fontelle-app --play-sf2` drives real playback from it instead of a hardcoded `note_on`. Prefab resolution, automation clips, and audio clips are documented scope cuts, not this pass. |

**So: every piece of the gate's wording is now implemented and tested** —
audio callback, compiled graph, sampler voice on a real SF2 zone, mixer
track, device out, triggered from a clip on a timeline, at 128 frames /
48 kHz, with the zero-allocation assertion active and passing (including on
the two-node stereo shape playback actually uses). TDD §23 names "SF2 import
defaults subtly wrong" and "agent-generated code that looks right and does
nothing" as risks; both are why this took the tests-first route, and the
2026-08-24 review found two real bugs that vindicate it.

**The gate is closed.** Run and heard on real hardware on 2026-08-24: the
demo phrase plays audibly through a real SF2 preset, full duration, exit 0,
zero INVARIANT 1 violations, stable across repeated runs. See the
"2026-08-24, later" section at the top for what that run turned up (a
long-standing phantom allocation root-caused, and two reasons the first
listen sounded wrong).

### Verify by ear (needs a human at the keyboard)

```sh
cargo test -p fontelle-engine --test manual_audio_output -- --ignored --nocapture
# or, to hear a real SF2 file instead of the built-in synthetic test tone:
FONTELLE_TEST_SF2=/path/to/file.sf2 cargo test -p fontelle-engine --test manual_audio_output -- --ignored --nocapture

# or run the app binary directly (this is the one that goes through the real
# document -> sequencer -> timeline path, so it's the better demo):
cargo run -p fontelle-app -- --play-sf2 /path/to/file.sf2
cargo run -p fontelle-app -- --play-sf2 /path/to/file.sf2 --key 48
```

Both open the real default output device and play ~3.5 seconds: a
root/third/fifth run in eighth notes, then the triad held as a chord. Neither
runs in `cargo test --workspace` or CI (the `--ignored` test is skipped by
default; the app binary's default `main()` still `todo!()`s into the unbuilt
windowed DAW unless you pass `--play-sf2`).

**What to listen for, in priority order:**

1. **The chord.** Three simultaneous voices is what the 2026-08-24 polyphony
   bug broke. If the run's notes hold steady as the chord arrives — rather
   than ducking or dropping out — that fix is good on real hardware too.
2. **Stereo.** Both speakers should carry equal level (the mixer track is
   centred at unity). Silence on one side means the new bus-to-channel
   interleaving is wrong.
3. **No crash, and the full duration.** Time it rather than eyeballing it —
   that's how the earlier `SIGABRT` rounds were caught exiting early.

**Quote the path** if it has spaces (`--play-sf2 "/path/with spaces/file.sf2"`)
— the flag only consumes one argument, and an unquoted space-containing path
gets split by the shell before Fontelle ever sees it (that's what happened on
the first crash report, not a Fontelle bug: `fish` split `.../FL 2026 Linux/...`
into three separate arguments).

To inspect what a given SF2 file actually imports as, without any audio:
`cargo run -p fontelle-assets --example inspect_sf2 -- /path/to/file.sf2`.

## What's real vs. stub, per crate

- **fontelle-types** — real. Shared IDs, `ParamAddress`, `AssetRef`,
  `CompiledTimeline`/`TimedEvent`. Not part of the TDD's crate list — see
  `docs/scaffolding-notes.md` for why it exists.
- **fontelle-dsp** — `interpolate()` (Draft/Normal — Hermite verified to
  reproduce a linear ramp exactly, and to pass through control points),
  `EnvelopeGenerator` (full delay/attack/hold/decay/sustain/release state
  machine, early-release-from-any-stage, float-accumulation-robust stage
  timing), `SvfFilter`, `Oscillator` (PolyBLEP saw/square, plus
  `advance_block` for control-rate LFO use) and `PeakRmsMeter` (held peak,
  block RMS, latching clip indicator) are real and tested. `DcBlocker` is
  still `todo!()`.
- **fontelle-core** — real: `SampleStore` (insert/get, `AssetId`-keyed, only the
  fully-resident case — no disk streaming yet, see below), `Voice::render` (pitch
  from root-key+fine-tune, per-sample interpolated playback, forward looping,
  per-layer gain, patch-wide amp envelope, auto-deactivation), `VoicePool`
  (free-voice search then age-based stealing — `Quietest`/`LowestPriority`
  currently alias `Oldest`, no level/priority tracking exists yet), `Sampler`
  (`note_on`/`note_off` via voice-context matching, `render`, `set_pan`).
  Output is **stereo**: every layer is placed by `Layer::pan` plus the
  channel's own pan on the constant-power taper, with one filter per channel
  per slot. The matrix's live sources are velocity, key, the amp envelope, up
  to three modulation envelopes and up to four LFOs, sampled once per block;
  its live destinations are layer pitch/gain/pan and filter
  cutoff/resonance. Only `Source::Sample` layers render — `Source::Sf2Zone` is
  effectively unused (import always produces `Sample` layers) and
  `Source::Oscillator` is silently skipped, not wired to
  `fontelle_dsp::Oscillator` yet. Tests: `crates/fontelle-core/src/{streaming,voice,sampler}.rs`.
- **fontelle-fx** — `Limiter` is real (look-ahead brickwall, monotonic-deque
  sliding minimum, stereo-linked; see the 2026-08-26 section for why it cannot
  overshoot). Everything else is still the scaffolded shape —
  `ParametricEq::process` and `Compressor::process` are the two that
  `fontelle-dsp` could already support.
- **fontelle-model** — mostly still stub, but no longer *pure* stub.
  `TempoMap` is real for constant tempo (see the "2026-08-23 update" above for
  the scope cut). `Project::lanes` is a `SlotMap<LaneId, Lane>` now (was an
  awkward `Vec<Lane>` + parallel `Vec<LaneId>`). Everything else —
  `Command`/`History::undo`/`redo`, `Mixer::has_cycle`, `prefab::resolve` —
  is still `todo!()`; none of it is on the M0 path. Tests:
  `crates/fontelle-model/src/project.rs`.
- **fontelle-sequencer** — `compile()` is real, scoped to `ClipSource::Notes`
  clips with no prefab resolution (see the "2026-08-23 update" above for the
  full scope-cut list). `collision::voice_context_for_clip` was already real
  (trivial). `incremental::{DirtyBars::mark_range, recompile_dirty}` are still
  `todo!()` — M3 work, not needed until incremental (as opposed to whole-project)
  recompilation matters. Tests: `crates/fontelle-sequencer/src/compile.rs`.
- **fontelle-engine** — `CompiledGraph::process_block` handles real multi-node
  chains via the in-place convention (a consuming node declares the same
  buffers in and out; see the 2026-08-24 update). Capped at two buffers per
  node — distinct input/output sets, which sends and sidechains need, remain
  M4 and panic with a clear message rather than silently processing the wrong
  buffer. `SamplerNode` is real (wraps `Sampler` + a shared `Arc<SampleStore>`;
  renders mono and fans out across its output channels). `MixerTrackNode` is
  real for the fader stage — gain/pan/mute/phase, stereo or mono.
  `AudioDevice` is real — opens a real `cpal` stream, promotes the callback
  thread via `audio_thread_priority`, tags it via
  `rt_guard::mark_current_thread_rt`, chunks the callback into `BLOCK_SIZE`
  (128-frame) pieces regardless of what the backend delivers, walks the
  `CompiledTimeline` by sample range, and interleaves bus *N* into device
  channel *N*. `EffectNode`/`SendNode`/`AudioClipNode`/`MasterNode` are still
  empty placeholder structs — M4/M6 work. `SamplerNode::reset` is still
  `todo!()` (nothing calls it yet; transport stop/seek will).
- **fontelle-assets** — `import_sf2` is real, see "SF2 import scope" below.
  `import_sfz`, `SoundfontLibrary`, peak generation are pure stub.
- **fontelle-ui**, **fontelle-plugin**, **fontelle-midi** — pure stub, unchanged
  since scaffolding. Not on the M0 path.
- **xtask** — pure stub.

## SF2 import: what's real, what's deliberately cut

`fontelle_assets::import_sf2` (`crates/fontelle-assets/src/sf2_import.rs`) parses
via the `soundfont` crate and reads real generator values off the chosen
instrument's zones. Tested with **hand-built, spec-valid SF2 byte streams**
(`crates/fontelle-assets/tests/sf2_import.rs`) rather than a downloaded fixture —
deterministic, license-free, and lets the tests assert *exact* expected values
(including round-tripping the timecent/centibel math through its own inverse
formula, so a sign error or unit mistake fails the test, not just a missing
field).

**Implemented and tested:** KeyRange, VelRange, OverridingRootKey (with the
sample header's `origpitch` as fallback, and the spec's `-1` "no override"
sentinel honoured), CoarseTune/FineTune (+ the sample header's own `pitchadj`),
InitialAttenuation → gain, Pan, SampleModes → loop mode, the volume envelope
(Delay/Attack/Hold/Decay/Sustain/Release), and all four start/end/loop
fine+coarse offset generator pairs.

**Deliberately not implemented** (each is a real, scoped decision, not an
oversight — extend `build_layer` in `sf2_import.rs` when one of these matters):
- Only the **first preset** in the file is imported.
- **Preset-level zone generators are not layered over instrument-level ones** —
  only each instrument zone's own generators are read. Real SF2 files do use
  preset-level generators; a file that relies on them will import with wrong
  values for whatever they'd have overridden.
- **Modulators are ignored entirely** — no velocity-to-attenuation curve, no
  MIDI-CC-to-parameter default modulators (SF2 §8.4's defaults), nothing.
- **Only the amp envelope is populated** (`Patch.envelopes[0]`, taken from the
  *first* sample-bearing zone found) — Fontelle's fixed voice topology (§7.4)
  gives one shared amp envelope per patch, not one per layer, so a multi-zone
  instrument with per-zone envelope generators loses everything past the first
  zone's. This is an architecture constraint (INVARIANT 6), not a shortcut.
- ~~**No filter/LFO/mod-matrix generators are read.**~~ Now read: the filter
  (2026-08-25), and both LFOs, the modulation envelope and all six `*LfoTo*` /
  `ModEnvTo*` amounts (2026-08-26). Still not read: `keynumToModEnvHold` and
  `keynumToModEnvDecay`, and SF2's *modulator* records (as opposed to its
  generators) — only the two always-present defaults are seeded, by hand.
- **Instrument global zones aren't merged.** A zone with no `sample()` id inside
  an instrument (its global zone, carrying defaults for the other zones) is
  silently skipped rather than merged into the local zones that follow it.

**Also found and worked around:** `soundfont::Zone::key_range()`/`vel_range()`
(the crate's own convenience methods) call `.as_i16().unwrap()` on a generator
that's actually always parsed as the `Range` variant — they'd panic on *any* real
KeyRange/VelRange generator. `import_sf2` reads `zone.gen_list` directly instead
and never calls those two methods. Separately, `soundfont::SoundFont2::load` uses
a bare `assert_eq!` on the RIFF header instead of returning `Err`, so it panics on
a non-SF2 file; `import_sf2` wraps the call in `catch_unwind` to convert that into
a normal `ImportError` (TDD §20.3 requires malformed files to fail cleanly, not
crash the process).

## Open questions against the TDD

- **`fontelle-types` crate** (not in the TDD's crate list) — `docs/scaffolding-notes.md`.
- **`fontelle-assets` depends on `fontelle-core` and `fontelle-dsp`** (not stated
  in TDD §4.1, which doesn't list `fontelle-assets`'s dependencies at all) —
  needed so the SF2 importer can seed a real `Patch` directly, matching §7.3's
  "SF2 file --parse--> ImportedZones --seed--> Patch" pipeline. `ImportedZone`/
  `ImportedZones` still exist as unused placeholder types matching that diagram's
  two-step shape, in case splitting parse-from-seed turns out to matter later.
- **Sample residency: fully-resident only.** `fontelle_core::SampleStore` doesn't
  implement TDD §7.7's streamed case (files over a threshold, first N ms resident,
  remainder streamed by a disk thread into per-voice ring buffers) — that needs
  `fontelle-engine` to own a disk thread first, which doesn't exist yet.
- **`fontelle-model::Channel` stores a serialised `Vec<u8>` patch**, not a live
  `fontelle_core::Patch` — the model crate can't depend on `fontelle-core`
  (INVARIANT 4's model-side counterpart: model depends on nothing but
  `fontelle-types`). Revisit once the project serialisation format (§17.2) is
  actually designed; a dedicated intermediate type might be cleaner than an
  opaque blob. `fontelle-app --play-sf2` currently leaves `patch_data` empty
  for exactly this reason — see the "2026-08-23 update" above.
- **`TempoMap` holds a runtime `sample_rate_hz` field** that's `#[serde(skip)]`
  — never saved with the project, since it's the audio device's rate, not
  document data. Not stated anywhere in the TDD (§6.2's method signatures take
  only a tick or sample, implying the sample rate is known some other way);
  this is the scope-cut, single-segment `TempoMap`'s way of knowing it. Revisit
  once real ramp segments land — TDD §6.2's design doesn't visibly address
  where the sample rate comes from either.
- **The mixer is built in `fontelle-app`, not compiled from `Project::mixer`.**
  The model has a full `Mixer`/`MixerTrack` (TDD §13.1) and nothing reads it:
  `build_graph` assigns buses and faders from `Song::channels`, and a MIDI
  file's CC7 lands there rather than on a document mixer track. The fix is the
  same "build the graph from the project" step that would own `channel_nodes`
  below, and it is what has to exist before a UI can show a mixer at all.
- **MIDI CC10 is applied at the sampler, CC7 at the mixer track**, which is a
  real distinction (constant-power placement of a mono-ish source versus
  balance over a stereo bus) but means a part's pan is not visible anywhere in
  the document. `fontelle_model::Channel` has no `pan`; TDD §13.1 notes that
  several channels may share a mixer track, which is the case that would
  require one.
- **`fontelle_sequencer::compile` takes a `channel_nodes: &HashMap<ChannelId,
  NodeId>` parameter** not implied by the TDD's prose (§11.1 gives no Rust
  signature for `compile`). Needed because the crate can't depend on
  `fontelle-engine` to look up a channel's compiled `SamplerNode` identity
  itself. See the "2026-08-23 update" above for the full reasoning — worth
  revisiting once there's a real "build the graph from the project" step
  somewhere (currently `fontelle-app` hand-assigns `NodeId`s), since that step
  would be the natural owner of this mapping instead of a parameter threaded
  in from outside.

## Next steps, in priority order

1. ~~Run `--play-sf2` on real hardware and listen.~~ **Done** — see the
   "hardware run" section at the top. M0 is closed.
2. ~~Envelope curve shape.~~ **Done** — see the 2026-08-25 section.
3. ~~Velocity → loudness.~~ **Done** — see the 2026-08-25 section. The
   remaining piece is the mod matrix honouring a file's *override* of the
   default modulator, which is part of item 4.
4. ~~Interpolation quality (TDD §7.6).~~ **Mostly done** — see the 2026-08-25
   section. `Ultra` is still unimplemented and needs a stateful resampler
   rather than a point-interpolator; the per-layer/global quality precedence
   is an open question, below.
5. ~~`ModMatrix::evaluate`, then sources to route.~~ **Done** — the matrix
   in the 2026-08-26 (first) section, envelopes and LFOs in the 2026-08-26
   (second) one, and the SF2 generators that drive them alongside.
6. ~~Multi-timbral playback.~~ **Done** — see the 2026-08-27 section.
7. ~~Per-voice panning, and a fader per part.~~ **Done** — see the 2026-08-26
   section.
8. ~~Root-cause the `dealloc size=16, align=4` allocation.~~ **Done** — it
   was our RT tag outliving the callback, not a per-block allocation. See the
   "hardware run" section at the top.
9. ~~Tempo changes.~~ **Done** — see the 2026-08-26 (later) section. Ramps
   remain, and cannot be created by anything yet.
10. ~~A master limiter.~~ **Done** — see the 2026-08-26 (later) section.
11. **Transport.** `SamplerNode::reset` is still `todo!()` — nothing calls it,
    and the first thing that will is transport stop or seek. Play/stop/seek is
    also the smallest thing that makes the CLI feel like a DAW rather than a
    one-shot renderer, and `MasterNode::reset` is already written against the
    assumption that something eventually will.
12. **Live MIDI input** (TDD §14). `fontelle-midi` is a pure stub, and the
    staging note in `device.rs` is the constraint that matters: it has to feed
    the same RT-safe sample-accurate `TimedEvent` pipeline as notes and
    automation, not poll on the UI thread. Playing a soundfont from a keyboard
    is the first thing that would make this feel like an instrument.
13. Then the rest of M1: streaming (TDD §7.7 — we currently hold whole
    soundfonts in memory) and effects. `ParametricEq::process` and
    `Compressor::process` are the two `fontelle-dsp` could already support.
14. **Latency compensation.** The master limiter is the first node in the tree
    with real latency (`AudioNode::latency_samples` reports it and nothing
    reads it). With one bus that is a uniform delay nobody can hear; with a
    send path or a track that bypasses it, it is a phase error.

## Open questions against the TDD (2026-08-25 additions)

- **~~Per-layer vs global interpolation quality.~~ Settled**: the layer's value
  became `Option<Interpolation>`, `None` follows the session. See the
  2026-08-25 section for the reasoning.
- **SF2's per-zone filter against our per-voice one.** SF2 puts
  `initialFilterFc`/`initialFilterQ` on every zone; TDD §7.4's fixed topology
  puts the filter after the layer mix, so a multi-zone preset whose zones
  disagree cannot be represented exactly. The importer takes the first zone's,
  matching what it already does for the amp envelope. Averaging would produce a
  setting no zone asked for; per-layer filters would be a real change to §7.4's
  topology and its per-voice cost.

## Open questions against the TDD (2026-08-24 additions)

- **`PanLaw::Linear` vs `PanLaw::Minus6Db`** — the TDD names four pan laws and
  defines none, and on the usual reading those two are the same taper. They're
  implemented as distinct (`Minus6Db` = linear taper, centre 0.5; `Linear` =
  balance-style, centre at unity). Revisit if `Linear` was meant to be
  something else. `crates/fontelle-types/src/pan.rs`.
- **The graph's in-place node convention** — a node consuming another's output
  declares identical `input_buffers` and `output_buffers`. Not stated in the
  TDD (§5.1's `ProcessContext` implies separate input and output slices), but
  it's what every plugin API does by default and it avoids both a per-node
  copy and an aliasing problem on the RT thread. Distinct in/out sets stay
  possible later; nothing here forecloses them.
- **`fontelle-app` now has a `lib.rs`** as well as its `[[bin]]`, so the demo
  song and graph construction are testable. Not a TDD change, just structure.
