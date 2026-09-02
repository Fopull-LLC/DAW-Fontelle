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

**Picking this up cold?** `docs/handoff.md` says what state the tree is in,
what is still open, and the handful of things about this machine and this
codebase that cost real time to rediscover.

## 2026-09-02 (latest): four effects, a preset picker, and the external sidechain

The four P0s at the top of `docs/effects-catalogue.md` §4's build order, each
written to the standard §3 set, plus **both** of the items that gate more than
one row below them. Test-first throughout: the config, then a test file
confirmed failing against `todo!()`, then the DSP.

**Where the count went:** 1889 → 2017 across the workspace, 0 failing.
`cargo clippy --workspace --all-targets -- -D warnings` reports four errors,
all of them ones the previous pass recorded as pre-existing
(`fontelle-model/tests/inserts.rs` ×2, `fontelle-ui/src/app.rs`,
`fontelle-ui/src/render/mod.rs`) and none in anything touched here. A fifth
that pass recorded — `large_enum_variant` on `EditorWindowChrome` — stopped
firing, because the field the key row added to `InstrumentChrome` closed the
gap between its two largest variants. Two clippy did find in this work — a
manual swap in `RestoreInsertConfig` and a constant assertion in a test — are
fixed.

### Utility (`fontelle-fx/tests/utility.rs`, 18)

One insert rather than seven: gain, pan, width, a mono-maker, a channel swap,
two mutes, two polarity flips, a DC/rumble filter, mix. First in
`EffectKind::ALL`, because it is the commonest thing in mixing and it should
be one click. The stubs that used to sit in `fontelle-fx/src/utility.rs`
became the effect; the three metering ones that shared the file moved to
`meters.rs`, which is where the catalogue's §2.5 puts them.

Three things the tests are watching, each of which was a decision:

- **Pan is a balance**, not a pan law. A constant-power law would take 3 dB
  off a centred signal the moment the insert was added, which is exactly what
  rule 2 forbids on a gain-staging tool.
- **A fresh utility is a wire sample for sample**, not nearly one. The
  mid/side stage is skipped whole when the width is at 100 % and the
  mono-maker is off, because `(l+r)/2 + (l−r)/2` is only *approximately* `l`
  in floating point. `a_fresh_utility_is_a_wire_sample_for_sample` is an
  `assert_eq!` on the whole buffer, and it is what forced the short-circuit.
- **A test that proved nothing, caught before it was believed.** The first
  draft had `the_mono_maker_runs_after_the_width`, on the theory that
  widening after summing would undo the sum. It would not: both act on the one
  side signal and one of them is a scalar, so they commute exactly. The test
  was replaced with `a_widened_bass_is_still_summed`, which is the claim a
  person actually cares about and which *would* fail if the mono-maker were
  written on the middle or on the channels. The ordering that does have teeth
  is invert-before-width, and `a_flipped_side_summed_to_mono_is_a_null` is it.

### Gate / expander (`fontelle-fx/tests/gate.rs`, 15)

Threshold, hysteresis, key high-pass, look-ahead, attack, hold, release,
ratio, range. One effect for the gate and the expander because they are one
machine at two settings, and both of the knobs that say which one you have are
continuous, so the expander that only ducks spill by six decibels is reachable
rather than being a third menu entry.

**What the tests caught.** The first implementation compared `|x|` to the
threshold, and eight of the fifteen tests failed at once: a steady sine's
rectified value visits zero twice a cycle, so the gate was opening and closing
at the tone's own frequency. That is not a tuning problem, it is a missing
stage — a gate compares an *envelope* to its threshold. The detector is now a
peak follower with a 20 ms hold (one cycle of the lowest note anybody gates
on) and a 10 ms release, and the ratio, range and stereo-link measurements
came out exact rather than approximately right. The hold is deliberately
*under* the `hold` knob's range, and it is why the gate takes about 20 ms to
notice a note has stopped.

**Three test premises were wrong and were corrected before the code was.** A
dip of 30 ms could not test the hold knob, because the detector's own hold had
not expired inside it; look-ahead of two attack time-constants does not reach
"open", because a one-pole never arrives, so the attack was shortened until
five fit inside the knob's ten milliseconds; and a sine starting at zero closes
the gate for a fraction of a cycle, so the pure-delay test uses a cosine.

Also: the look-ahead line is written every block whatever the knob says, so
turning it up mid-song reads history rather than the silence a line nobody had
been filling would hold. At zero the read slot *is* the write slot, so there
is no branch and no special case.

### Chorus / ensemble (`fontelle-fx/tests/chorus.rs`, 16)

One to four voices, a chorus/ensemble chooser, spread, rate with sync and a
note division, depth, centre delay, signed feedback, tone, mix. It is
`is_time_based` and opens half wet, because a fully wet chorus is a detuned
copy with nothing to beat against — which is a vibrato, and not what somebody
who added a chorus asked for.

**Each voice has its own centre as well as its own LFO phase**, spread across
±15 % of the delay knob. Spreading *n* voices evenly around one LFO's cycle is
the obvious design and it has a hole: for a sine, at every zero crossing — and
for an even *n*, at both ends of the sweep — the waveform takes the same value
at two of those points, and two voices are reading the same place. Four taps
at four delays is what a hardware ensemble did, and it is what makes
`each_voice_is_a_copy_of_its_own` a count rather than a hope.

Two measurements worth keeping: `depth_bends_the_pitch_of_what_comes_out`
counts zero crossings on the outward and the returning sweep (923 Hz and 1076
Hz from a 1 kHz tone), and the pair
`the_rate_is_how_often_the_sweep_comes_round` /
`ensemble_voices_never_come_back_into_step` is the difference between the two
modes as a number: the chorus repeats after one LFO period to within 0.02 and
the ensemble does not.

**Two bugs the tests found in code that was not new.** The phase accumulator
had to become `f64`: in `f32`, a hundred thousand additions of a 2 Hz step
drift by a third of a sample of read position, which is a chorus that is not
quite in the same place a cycle later — measurable, and the periodicity test
measures it. And `read_at`, copied from `delay.rs`, could index one past the
end: `rem_euclid` of a position a hair under zero comes back a hair under
`length`, and a hair under `length` *rounds to* `length` in `f32` once the
line is a few thousand samples long. It panicked on the first ensemble test.
Fixed in both files — it was a latent panic on the audio thread in the delay
too, reachable only on the first block after a reset with a long line.

### Filter (`fontelle-fx/tests/filter.rs`, 15)

The synthesiser filter as an insert. Eight shapes (LP/HP/BP at 12 and 24 dB,
notch, peak), cutoff, resonance, drive, a **signed** envelope amount with its
own attack and release, an LFO with six waves and a tempo sync, an output trim
and a mix.

- **The envelope amount is signed**, which is the half of an auto-wah nobody
  ships: a filter that closes as the signal gets loud is a duck with a tone.
  Both amounts are in octaves and they add, four octaves each at full.
- **The drive is before the filter**, and `the_drive_is_before_the_filter` is
  the only measurement that can tell the two orders apart: a `tanh` on a 500 Hz
  sine makes a third harmonic, and a corner at 800 Hz takes it away. If the
  drive were after the filter the harmonic would survive any corner.
- **Coefficients are rebuilt per sample only while something is moving them.**
  With both amounts at zero the corner is built once a block and the effect
  costs what an EQ band costs, which is what makes it reasonable to put one on
  every bus.
- **The band-pass is normalised to unity at its centre**, unlike the SVF's own
  band-pass output, which peaks at Q. `eq.rs` calls the un-normalised one "the
  right one for a voice's resonant filter, where the resonance is meant to be
  loud", and this *is* that filter — but it is also an insert on a mix bus,
  where a resonance knob carrying 21 dB of gain is a knob nobody turns past a
  third. Same one-multiply fix the EQ uses.
- **No key tracking**, and that is not an omission: a filter that follows the
  note needs a note, and an insert on a bus does not have one.

`LfoWave` is in `fontelle-types` rather than in the filter, because the
tremolo, the phaser and the flanger all want the same six and `value()` is a
closed form the panel could draw from. `SampleHold` is the exception and
returns zero from it: its value is a memory rather than a formula, and
`sample_and_hold_jumps_rather_than_sweeps` asks the DSP instead.

### A preset picker, at last

Soften has had four presets and no way to choose one since it was written.
There is now a row of chips above the first heading on the generic effect
panel, built from `EffectConfig::presets()` exactly the way the knobs are
built from `specs()` — so an effect that ships presets gets a picker for free,
the same bargain §8.2 already made for automation.

Clicking one runs `fontelle_model::SetInsertPreset`, which is **one entry on
the undo stack**. That is the whole reason it is a command rather than a run
of `SetInsertParam`s: fourteen entries for one click is a history nobody can
walk, and merging them would be worse, because `SetInsertParam::merge_with` is
deliberately per-control. Its inverse is `RestoreInsertConfig`, which puts the
whole parameter set back and refuses a slot whose kind has changed under it.

**Rule 10 grew an exception with a test behind it.** "Every effect with more
than eight parameters ships presets" is now "…or says why not": the utility's
ten controls are ten separate jobs, and the gate's and the filter's are one
machine each with one obvious knob to reach for. What
`whether_an_effect_ships_presets_is_a_decision_taken_for_every_one` prevents
is the third case — a new effect landing in the "no presets" list because
nobody decided. It also records the debt: the EQ, the compressor, the chorus,
the delay and the reverb are *owed* presets and have not been given them.

### The external sidechain

The second gating item, and the older of the two: `Compressor::process` has
taken an `Option<&[f32]>` key since it was written, with a comment saying the
routing was the graph's job and had not been done. It is done —
`docs/effects-catalogue.md` §6 is the design; what is worth recording here is
what it cost and what nearly went wrong.

**A key is a routing edge, not a parameter.** It is a field on `EffectSlot`
rather than a knob in the config, and that is forced: a `ParamSpec` is a float
with a fixed range and a permanent id, and a track is neither. A "key" chooser
stepping through whatever tracks happened to exist would be an automation lane
that pointed somewhere else after a rename — §8.2's second addressing scheme,
which the TDD forbids by name.

**The direction was wrong the first time, and a test caught it.** A key means
the named track *feeds* the track the insert sits on, which is the opposite of
how the document stores it — an insert names what it listens *to*. The first
`has_cycle` iterated each track's own inserts' keys as outgoing edges, which is
backwards, and `a_key_that_would_close_a_loop_is_refused` failed by accepting
one. The reversal now lives in exactly one place, `Mixer::key_listeners`, and
both the cycle check and the compiler's scheduling order read it from there.
Getting it backwards is a cycle check that passes a loop *and* a schedule that
reads the key a block late; one function, one direction, one place to be wrong.

**Why a tap rather than a second input on the node.** `CompiledGraph::process_
block` is deliberately narrow — in place, or one bus routed to another of the
same width, at most two buffers a side — and it carries the argument for that
narrowness in its own comments, along with the `no_allocation_during_render`
test that guards it. A key is a third set, a bus the node reads and never
writes, and widening that dispatch for one feature was the wrong trade. So a
`KeyTapNode` on the source's bus copies the block into a `KeyTap` and the
insert reads it out. Both ends are the audio thread in one pass, in an order
the compiler guarantees, so the tap needs no ring and no synchronisation past
what the `Arc` implies — unlike the analyser's, whose reader is a window.

**And a window control**, because a key nobody can choose is not delivered: a
second row of chips under the presets, laid out by the same `chip_row`. It
appears only on the two effects `EffectKind::takes_key` names.

`AudioNode` gained a `debug_name`, and it is worth saying why: a schedule is a
flat list of boxed trait objects whose **order** is a correctness property —
post-fader means after the fader, and a key means before the insert — and
neither shows up in the sound as anything but a one-block error. The tests that
hold the compiler to those orders read the list, and that is how they name what
they find.

### Still open

- **The gate's look-ahead is uncompensated latency**, like the master
  limiter's — item 16 above. It is off by default for that reason, and
  `Gate::latency_samples` reports it for whenever item 16 lands.
- **A chorus or a gate with look-ahead under a mix below 100 %** combs against
  an undelayed dry, because `EffectNode` blends the signal as it arrived.
  True of any lookahead insert without delay compensation; also item 16.
- **A key tap is rebuilt with the graph**, unlike the analyser's ring. It
  holds one block and is refilled before it is read, so the most a rebuild
  costs is a block of silence on the key — under three milliseconds, during
  which a keyed compressor opens slightly. Worth revisiting if a rebuild ever
  lands somewhere audible.
- **The ducker and the vocoder** (§2.1, §2.6) are unblocked and unwritten.
- **Judgement calls to revisit.** The chorus's voices are summed and divided
  by their count, so four voices are quieter than one rather than louder —
  never a level jump, at the cost of some thinning. The filter's modulation
  reaches four octaves each way at full, which is a guess at what a wah wants.
  The gate's threshold opens at the bottom of its range rather than its ratio
  at 1:1, which is a different reading of rule 2 from the compressor's and is
  argued on `GateConfig::new`.

## 2026-09-02: the effects catalogue, and two effects rebuilt to go far

*"I'm finding it hard to get more than a basic distortion sound with our
distortion plugin... all of our effects need to follow these principles of
having lots of functionality and use cases directly out of the box... there
are lots of types of bitcrush."*

Three things this pass delivered, in the order they were written:

### `docs/effects-catalogue.md`

What ships in the "+ Add effect" menu, what each one has to be able to do
before it counts as done, and in what order the missing ones get built. It
extends TDD §13.4's one-line-per-effect table into a design — forty-odd
effects across dynamics, EQ and filtering, distortion and lo-fi, time and
modulation, stereo and metering, and pitch — with a status (built /
rebuilt / planned), a priority (P0 a mix cannot be finished without; P1 a
producer will go looking for; P2 quality of life or fun), and for each the
*family* it has to be rather than the sound. §1 states the twelve rules
every effect follows, each of which exists because its absence has already
produced a defect this file records. §4 is the build order; §5 the recipe.
Two things gate several rows and are called out: a **preset picker** in the
effect window (Soften, the distortion and the bitcrush now have seventeen
presets between them and no way to choose one), and an **external sidechain
into `EffectNode`**.

### Distortion, rebuilt (`fontelle-fx/tests/distortion.rs`, 28)

Five curves, a drive, a tone and an output was a pedal. What was missing
was everything a pedal's *circuit* does around the clipping stage, and any
way to make each curve a continuum rather than one point. It has fourteen
parameters now, in three sections:

- **Drive**: ten curves (the five that were, plus diode, triangle fold,
  rectify, crossover and wrap), a `shape` knob that is each curve's own
  continuum — hardness, knee, asymmetry, fold count, polynomial order,
  half-to-full wave, dead-zone width, wrap threshold — `bias` (a shifted
  operating point, which is even harmonics on any curve, with the offset
  taken back out), and `sag` (drive that falls as the input gets loud: the
  amplifier's power supply).
- **Voicing**: a high-pass and a mid bell **before** the curve, a
  `clean_low` split that sends the bottom *around* the curve on a
  fourth-order Linkwitz–Riley crossover, and the tone after it.
- **Output**: output, `auto_gain` (the curve's own level at this drive,
  measured on a reference sine per block rather than estimated, so it is
  right for a folder as well as a clipper), oversampling as a chooser
  (off / 2× / 4× / 8×, eighth-order filters), mix.

Seven presets as constructors. Every stage is tested by what it should
change **and** by what it should leave alone: the pre-filters are proved to
sit *before* the curve by the harmonics they prevent, not the frequencies
they remove; the clean band by a 30 Hz tone that comes out undistorted
under a split that distorts 2 kHz as hard as ever, and by the split summing
flat at no drive.

**What the test suite caught.** With the oversampling filters raised to
eighth order, `more_oversampling_is_less_alias` — the test that says the
chooser is a ladder — failed with 4× only 44 % better than 2×, and a
diagnostic showed the residual almost entirely at 13 kHz. The filter
measured exactly as designed (35 kHz at −44 dB). The cause was a safety
clamp to ±1 *after* the DC blocker, at the base rate: a band-limited square
overshoots its corners by a sixth, and clamping that at 48 kHz re-clipped
the ringing, which is exactly the aliasing the oversampling exists to
prevent. Replaced by a wide net at ±1.5 that nothing ordinary reaches. The
two tests that had asserted a hard clipper stops within 5 % of its ceiling
now allow the ringing, with the reason written on them, and the wrap test
runs un-oversampled as its preset does.

Two other measurements were wrong about physics rather than about the
code and were corrected before the implementation was touched: "a harder
soft clip keeps more of the seventh harmonic" measured at a drive where
every soft clip is a square wave (true, and not the claim — hardness is
where the series *ends*, so it is now the eleventh to nineteenth at a
moderate drive), and the bitcrush rails test assumed a four-bit grid has a
level at 1.0 (it has fifteen steps with one at zero, so its top is 14/15).

**Compatibility.** A project saved with `"oversample": true` reads as 2×,
and every new field defaults to rest; `auto_gain` defaults **off** for a
file that does not say and **on** for a fresh effect, so an old project
does not change level on opening. The one thing that does move: an
automation lane on the `curve` chooser stores a normalised position, and
five positions became ten, so a lane written before this pass lands on a
different curve. The saved *config* names its curve and is unaffected.

### Bitcrush, rebuilt (`fontelle-fx/tests/bitcrush.rs`, 21)

Bits, rate, dither and anti-alias was one bitcrusher. What makes the
*types* is how the amplitude is rounded, how the time is held, and what the
level is when it happens. Eleven parameters in three sections:

- **Depth**: `input` gain (a quantiser is a level-dependent effect), bits,
  a **quantiser** chooser — round, truncate (toward zero, so small signals
  fall to silence: the 8-bit sample player's gating), µ-law (a logarithmic
  grid, so quiet detail survives and loud material crunches) — and a
  **dither** chooser: off, rectangular, triangular, shaped (error feedback,
  which pushes the noise up out of the way; measured as the low end of the
  error spectrum falling by half against triangular).
- **Rate**: rate, a **decimation** chooser — hold, linear (a ramp between
  takes, a sampler with interpolation), drop (one sample then silence:
  sparse, comb-like) — `jitter` (an unstable clock, measured as the hold
  lengths becoming uneven), anti-alias.
- **Output**: a post low-pass (the old sampler's output stage, which tames
  aliasing after the fact — a different sound from preventing it), output
  gain, mix.

Six presets. `"dither": true` in an old file reads as triangular.

### Sections, and no other UI change

`EffectConfig::sections()` is a list of `(name, count)` runs over the
effect's own table, and `effect_view` draws one heading per run. The split
lives beside the table rather than in the window, for the reason the table
does: the place that knows which five knobs are the drive is the place that
lists them. Effects with one grid declare one section and draw as they did.
`every_effects_sections_cover_its_parameters_exactly` holds every count to
its table.

- `fontelle-types/tests/effect_families.rs` (19): the contract side — ids,
  units, positions, sections, presets distinct from the wire and from each
  other, and both old-file shapes loading.
- `fontelle-ui/tests/inserts.rs` gained two.

**Verified:** `cargo test --workspace` green at **1889 passing** (1840 before
this pass, 0 failed, counted from every `test result` line rather than a
wrapper's exit code). `cargo clippy --all-targets -- -D warnings` is clean on
`fontelle-fx`, `fontelle-types` and `fontelle-engine`. It is **not** clean on
the workspace, and was not before this pass either: the two `unused Result`s
in `fontelle-model/tests/inserts.rs` the handoff already lists, and three
pre-existing hits in `fontelle-ui` (`app.rs:2147`, `render/mod.rs:532` and
`:4037`) that the previous "clippy clean" claim did not cover. One lint is
allowed crate-wide in `fontelle-fx` with its reason on it:
`needless_range_loop`, because every effect indexes several parallel arrays
by frame and by channel and the iterator form is the less readable one.

## 2026-09-02: two more effects, and the third wrong thing in the handoff

The mixer offered **two** effects at the start of this run of work and offers
**seven** now. This pass added the last two that belong in an insert slot, and
corrected one more claim in `docs/handoff.md` that did not survive being
checked.

### Bitcrush

Two destructions that are usually shipped as one "crush" knob, kept apart
because that is the whole use of them: **bit depth** quantises amplitude,
**rate** quantises time, and a 12-bit sample at full rate is a different sound
from a 16-bit one held at 8 kHz.

**Anti-aliasing is off by default**, which is the interesting default. The
fold-down a sample-and-hold produces *is* the effect — a 6 kHz tone held at
8 kHz comes back as 2 kHz, and that ring-modulation is what a bitcrusher is
for. A bitcrusher that band-limits its decimation is a low-pass with extra
steps. The switch is there for the times the grit is wanted and the fold is
not.

**A test that was wrong about its own subject.** `dither_breaks_the_steps_up`
asserted that dither makes the output land on more distinct values. It does
not: dither adds no levels, the output still lands on the quantiser's grid.
What it changes is *which* level gets chosen — by making the choice depend on a
random offset as well as the signal, so the error stops being correlated with
the material. The test that replaced it measures what dither is actually for: a
tone quieter than half a quantisation step rounds away to silence without it
and survives with it, in the average. Writing the wrong test first is how that
got noticed.

- `fontelle-fx/tests/bitcrush.rs` (10).

### Soften — the one that exists because of what this program is

TDD §13.5, and the only effect here whose reason is Fontelle rather than audio
in general. A sampled instrument played at a pitch it was not recorded at,
through a filter an SF2 file specified decades ago, is harsh in ways a treble
control cannot separate — turning the top down fixes all of them by throwing
away the record, and the result is *dull* rather than *smooth*.

Four stages, each tested by what it should change **and** by something it
should leave alone, because a stage that cut everything would pass half of
those:

- A **dynamic high shelf** whose depth follows how much top end there actually
  is, so a quiet passage is not darkened along with a loud one.
  `the_shelf_cuts_a_loud_top_end_harder_than_a_quiet_one` is the word
  "dynamic" as a measurement.
- An **adaptive suppressor** over three bands across 1–6 kHz, each ducking by
  how far it stands out *relative to the whole signal* — a band that is merely
  present is not honking; one that is most of the signal is.
- A **transient softener**, driven by how far a fast envelope stands above a
  slow one, which is what an attack is and nothing else.
- An **air restore** shelf at 12 kHz, above the harsh region rather than inside
  it. This is the stage that makes the effect smoothing rather than dulling:
  without it the first three add up to a low-pass, and
  `the_shelf_and_the_air_restore_are_not_the_same_shelf` is what says they are
  not one control undoing itself.

**It opens at Gentle, not at zero**, which is a deliberate departure from the
rule the EQ and the compressor follow. Those open as an identity because you
add them in order to dial in your own settings; this is one macro with one job,
and somebody who has just added "Soften" has already said what they want.
Opening at zero would be the dead panel this document keeps recording. The
`Gentle` preset's numbers and the spec table's defaults are written next to
each other for the reason every other pair like that is.

Presets are a **constructor, not a parameter**: a preset sets the four knobs and
then has nothing further to say, and a "preset" knob would fight the four it
had just written. There is no preset picker in the window yet.

- `fontelle-fx/tests/soften.rs` (13).

### The third wrong thing in the handoff

That document's top item has now been wrong three times, and the pattern is
worth naming: **it described what the code looked like rather than what it
did.**

1. "Seven effects are written and unreachable" — six were `todo!()`.
2. Their reachability was "cheap plumbing" — that part was exactly right.
3. **"The limiter is unreachable."** It is not, and never was: its DSP is
   complete and it has been running on the master bus in `MasterNode` the whole
   time, deliberately not an insert, with a comment on the field saying so. It
   was read as unreachable because it has no `EffectKind` variant — and it has
   none because it is not that kind of thing.

A file with a config struct, a state struct, a `process` signature and a
doc-comment reads exactly like a finished effect from anywhere except inside
the function body; a limiter wired into a node reads like an unused one from
anywhere except the node. Both mistakes are the same mistake.

**Still `todo!()`: the repitcher** — and it is arguably in the wrong crate. It
is varispeed over a *clip*, not a bus effect, so it has no insert slot to live
in and giving it one would be inventing a use for it.

## 2026-09-01: rows that can be put in order, and a cursor where you are typing

The last two items on `docs/handoff.md`'s list that can be done on this
machine. Both are small; one of them turned up a live bug that had been
harmless only by accident.

### Rows in the order you want them

*"theres no way to actually edit arrangement rows"* was answered a while ago
for adding, renaming, muting and deleting. **Ordering** was not: the arrangement
stacked rows in the arena's own order, which is insertion order and cannot be
changed, so an arrangement whose rows were made in the wrong order stayed that
way.

`Lane` gained an `order`, `Project::lane_ids()` is now the one answer to "what
is row 3", and `MoveLane` is the command. Three things it was important to get
right:

- **The sort is stable and the field is defaulted**, so a project written
  before this keeps the order it always had — every row in it carries the same
  number and the tie falls back to the arena. A sort breaking that tie any
  other way would silently rearrange every saved arrangement.
- **It swaps order numbers, never contents.** A clip names a `LaneId`, so
  swapping the two lanes' fields would leave every clip pointing at the wrong
  row. `moving_a_row_takes_its_clips_with_it` is the test for that alone.
- **A row added after a reorder goes to the bottom**, where somebody adding one
  is looking for it — not wherever a default of zero sorts. `AddLane` and the
  three places that build a lane straight on the arena (an automation lane, a
  channel's own) all follow the same rule.

**It is the menu, not a drag.** The handoff phrased the gap as "one cannot be
dragged above another"; this is "Move up" and "Move down" on the right-click
menu that already carries add, rename, mute and delete, greyed at the ends.
That makes reordering possible and is where somebody looks for it; dragging a
lane header is still not a gesture the arrangement has.

**The bug this turned up.** `Session::lanes()` walked the arena while
`Session::clips()` indexed rows through `lane_ids()`. Before rows could move
those two agreed by accident and nothing noticed — and the moment one moved,
every clip would have been drawn against the **wrong row**. That is the
two-lists-that-must-agree defect this document keeps recording, it was live for
exactly as long as it took a test to move a row, and it is the reason the
host-level test asserts *which row each clip is on by the row's name* rather
than just that the names reordered.

- `fontelle-model/tests/arranging.rs`, `fontelle-app/tests/studio.rs`.

### A caret where you are typing

Right-click → Rename put every keystroke straight into the document and the row
updated live. That is the right mechanism — the document is the buffer — and it
gave no sign the keyboard had been captured: the row looked exactly like a row
nobody was typing into, and the only way to find out was to press a letter and
watch what happened.

The rename state never reached the renderer at all, so it does now:
`RackChrome` and `TimelineChrome` carry which row is being typed into, and a
one-pixel bar goes after the name. The same shape the **search box** has drawn
since it was written — one mark for "the keyboard is going here", wherever it
is — and it is on channel rows and arrangement lane headers alike.

`a_row_being_renamed_shows_a_caret_and_the_others_do_not` shoots the rack twice
and compares the two, because "a caret is drawn" is only worth anything as
"this row has one and that row does not, in the same picture".

- `fontelle-ui/tests/render_headless.rs`.

### A process note

The menu entries and the `move_lane` host method were written **before** their
tests, which is the one rule stated at the top of this document. The tests came
straight after and are what found the `lanes()` bug above, so the work is
covered — but the order was wrong, and recording that is cheaper than letting
the section imply otherwise.

### Still open

- **Bitcrush, soften and repitcher are still `todo!()`**; the limiter's DSP is
  complete and it is still unreachable, because it carries lookahead latency
  that nothing compensates for.
- **Raising an already-open editor window** (item 8) cannot be checked here:
  the test harness is a bare nested X server with no window manager, so there
  is nothing to raise against. It needs a real session.

## 2026-09-01: the tempo reaches the audio thread, and three knobs stop lying

Three items off `docs/handoff.md`'s list, and the first one is the reason the
other two fitted in the same pass: it is the only one that needed a change to
what the engine is handed every block.

### A delay in note values, and the wire that had to be run for it

The delay shipped in the pass above with the time in **milliseconds only**, and
that was recorded as a deliberate gap rather than a missing field:
`ProcessContext` carried a `TransportSnapshot` with a position and a state in
it and **no tempo at all**, so "a dotted eighth" was a question nothing on the
audio thread could answer.

The wire runs like this, and every link is tested on its own:

1. **The sequencer compiles it.** `CompiledTimeline` gains
   `tempo: Vec<(Sample, f32)>` — the tempo map, converted to the block
   contract's own units. Here rather than anywhere else for the reason
   `param_nodes` is: this pass already owns every tick-to-sample conversion in
   the project, because that is what compiling a timeline *is*. A table built
   elsewhere would be a second answer to a question that already has one.
2. **The engine reads it.** `fontelle-engine` cannot see a `TempoMap` —
   INVARIANT 4 runs the other way — but it sees a `CompiledTimeline` every
   block. `TransportSnapshot` gains `bpm`: the tempo **here**, at the position
   being rendered, read through `CompiledTimeline::bpm_at` (binary search, no
   allocation). Never zero, because what reads it divides by it.
3. **The document interprets it.** `NoteDivision` and
   `DelayConfig::effective_time_ms(bpm)` live in `fontelle-types`, because what
   a dotted eighth *is* is a document fact. The engine's job is to supply the
   tempo, not to interpret it, and the DSP's is to turn a duration into sound.

Because the tempo is read **every block** rather than at build time, a synced
delay follows a tempo *change* — and follows it through the same glide a
dragged time knob uses, so a tempo automation ramp bends the repeats like tape
instead of stepping.

Two things the tests caught that are worth keeping:

- `the_divisions_run_from_longest_to_shortest` failed on the obvious ordering.
  Grouping the list by family — whole, half, quarter... then the dotted ones,
  then the triplets — is **not** monotonic in length: a dotted quarter (1.5
  beats) is longer than a half-note triplet (1⅓), and an eighth triplet is
  shorter than a dotted sixteenth. `NoteDivision::ALL` interleaves them, so
  turning the knob shortens the delay all the way down rather than jumping
  about half way.
- `MAX_DELAY_MS` went from 2 s to **4 s**, so that the longest division the
  chooser offers just fits: a whole note at 60 bpm is exactly four seconds.
  Offering a setting the line then silently clamps is worse than not offering
  it. Safe to change now and not later — nothing has been saved with a delay in
  it yet, and the range is what a normalised automation value is scaled against.

- `fontelle-types/tests/tempo_sync.rs` (14),
  `fontelle-sequencer/tests/tempo_track.rs` (4), and the synced half of
  `fontelle-fx/tests/delay.rs`, ending at
  `a_synced_delay_takes_its_time_from_the_transports_tempo` in
  `fontelle-engine/tests/effects.rs`, which is the one that says the links are
  connected to each other.

### A ramp cut in half stopped stepping

`SplitClip` sent each automation point to the half it fell in and inserted none
at the cut. A note lying across the cut has been cut in two since the tool was
written; a **ramp** across it was not, so the left half ended at its last point
and held, the right half began at its first, and between them the value
stepped. Cutting a filter sweep to move one end of it left a click at the join
— which is the exact thing automation exists to avoid.

The cut now reads the curve's value *at* the cut before dealing the points out
— after that, neither half has both sides of the seam to interpolate between —
and puts a point there in both halves, carrying the shape of the segment the
cut fell inside so the bend either side is unchanged. A point already sitting
exactly on the cut is the seam, and does not get a second one.

`the_two_halves_read_the_same_as_the_clip_they_came_from` is the test with the
teeth: it samples the whole curve before and after, not just the join, because
inserting a point at the cut must not bend the segments around it.

- `fontelle-model/tests/arranging.rs`.

### The polyphony lane that drew and did nothing

`patch/voice/polyphony` was addressable, drawable, saveable and automatable, and
moving it through a lane changed nothing: the pool was sized once in
`Sampler::new` and never read again. The *knob* worked, because turning it
rebuilds the graph. That is the worst shape a defect can have here — the lane
draws, saves and plays, and the only thing missing is the sound.

**The fix is not to resize the pool.** That is a `Vec` allocation on the audio
thread, which is INVARIANT 1's whole subject. The pool keeps the size it was
built at and polyphony became a **limit inside it**, which is what the word
means anyway: a note that finds nothing free under the limit steals one,
exactly as it does when the pool is full. Voices already sounding above a
lowered limit are left to ring out rather than cut, because cutting them would
put a click exactly where somebody was reaching for a swell.

**The honest limit:** a lane can take polyphony down and bring it back up to
the size the pool was built at, and cannot raise it past that. The pool is
built from the patch, so the knob's own value is the ceiling, and turning the
knob rebuilds the graph and raises it. `the_pool_is_the_ceiling` is the test
that states it rather than leaving it to be discovered.

- `fontelle-core/tests/patch_params.rs`.

### Still open

- **Bitcrush, soften and repitcher are still `todo!()`**, and the limiter is
  still unreachable — its DSP is complete, but it carries lookahead latency
  that nothing compensates for yet.
- Items 6-8 of `docs/handoff.md`: no lane reordering, rename has no caret, and
  raising an already-open editor window is still unverified on a real
  compositor.

## 2026-09-01: three effects that were names in a menu, and a ring on the knobs a lane owns

### The mixer had two effects and a list of seven promises

`docs/handoff.md` ranked this first and described it as cheap: *"seven effects
are written and unreachable — `fontelle-fx` has working `process` functions for
delay, reverb, distortion, bitcrush, limiter, soften and repitcher"*, needing
only an `EffectKind` variant and a spec table each.

**That was wrong, and it is worth recording why rather than just fixing it.**
Six of the seven were `todo!()`:

```rust
impl Delay {
    pub fn process(&mut self, _left: &mut [f32], _right: &mut [f32], _config: &DelayConfig) {
        todo!("tempo-aware delay line with filtered/saturated feedback loop")
    }
}
```

A file with a config struct, a state struct, a `process` signature and a
doc-comment describing the algorithm reads exactly like a finished effect from
anywhere except inside the function body. Only the limiter (433 lines) was
real. So this was not enum plumbing; it was writing the DSP.

Three of them are now real, reachable, and heard: **delay**, **reverb** and
**distortion** — the three a mixer is most obviously missing. Each is
`fontelle-fx` DSP + a `fontelle-types` config and spec table + one
`EffectState` arm, and **nothing in the UI changed at all**: `EffectKind::ALL`
drives the "+ Add effect" menu and `EffectConfig::specs()` drives the generic
window, so each arrived with a menu row, a panel of knobs in the right units,
read-outs, and a right-click that makes an automation lane — none of which
anybody wrote for them. That is the §8.2 payoff working exactly as the previous
pass claimed it would.

- **Delay** (`fontelle-fx/src/delay.rs`) — a stereo line with the damping
  filter and the saturation **inside the feedback path**, which is where they
  belong: the first repeat is what you played, and each one after it has been
  round the filter once more. On the output they would be a tone control.
  Ping-pong crosses the *feedback*, not the input, so the dry image does not
  move. The read pointer **glides** to a new time rather than jumping — a
  pointer that jumped would splice two unrelated points of the signal together,
  which is a click at full scale every time somebody drags the knob;
  `moving_the_time_while_it_runs_does_not_click` is the test.
- **Reverb** (`fontelle-fx/src/reverb.rs`) — an 8-line FDN with a Householder
  reflection (`y = x - (2/N)·Σx`) between the lines, which is orthogonal, so
  the network neither gains nor loses energy of its own. Line lengths are
  **mutually prime**, because two lines sharing a factor stack their echoes at
  every common multiple and the tail comes out metallic. Decay is an **RT60**,
  not a feedback gain: each line's gain is `10^(-3L/(RT60·fs))`, derived from
  *its own length*, which is what keeps the size knob a room control instead of
  a second decay control.
- **Distortion** (`fontelle-fx/src/distortion.rs`) — five curves, and
  **oversampled**, which is the part that matters. Clipping a 7 kHz tone puts a
  seventh harmonic at 49 kHz; at 48 kHz that frequency does not exist, so it
  folds back as a **1 kHz tone nobody played**. `oversampling_keeps_the_aliases_out`
  measures that one bin with and without, and
  `oversampling_keeps_the_harmonics_that_belong` is its other half — a
  decimation filter set too low would pass the first test by removing the
  wanted third harmonic along with the alias.

**No tempo sync**, and that is a deliberate gap rather than a missing field:
`ProcessContext` carries a `TransportSnapshot` with a position and a state in
it and **no tempo at all**, so a delay division in beats is a change to the
engine's block contract, not to the effect. The time is in milliseconds and the
field that would name a division is not there to be half-wired.

### A reverb that made the track disappear

Adding these broke an assumption the tests had been holding uniformly, and
finding it is the reason this is written up rather than just fixed.

`a_fresh_effect_is_all_wet` asserted every effect opens at 100 % wet. That is
right for a processor — an EQ *replaces* the signal, and the point of a
compressor is the compressed track. But `Delay::process` writes **the repeats
and only the repeats**, and `FdnReverb::process` writes **the tail**, because
`EffectNode` owns the dry/wet blend for every effect and one that mixed its own
dry back in would be blended twice. So a fully wet reverb insert is a track
replaced by its own reverb tail, with the sound that caused it gone.

The rule is now sharper rather than looser: **an effect opens at the default
its own spec declares**, `EffectKind::is_time_based` says which ones sit under
the track, and `MIX_PARAM` became `mix_param(default)` so the spec table and
the constructor cannot drift. Delay opens at 35 %, reverb at 30 %, everything
else at 100 %. `every_effect_at_its_defaults_leaves_something_audible` in
`fontelle-engine/tests/effects.rs` is the test that would have caught the
original as a bug.

### The tests loop over `EffectKind::ALL`, on purpose

`fontelle-engine/tests/effects.rs` is new and names no effect. It asserts that
every kind the menu offers builds a node that runs, leaves something audible at
its defaults, is a wire when bypassed, and is a wire at a fully dry mix — so
**an effect added later is covered the day it is added** rather than the day
somebody remembers to write its test. That is the one that catches a missing
`EffectState` arm, which is otherwise silent: the `match` falls through to
`_ => {}` and the slot passes the signal along looking like it works.

Two of my own tests were measuring the wrong thing and were corrected before
the implementation went in, which is worth recording because both would have
passed against wrong DSP:

- The delay's damping test used a *continuous* tone, so every window contained
  the fresh input — the line always holds what is being played into it — and no
  amount of damping changes that. It uses a burst now.
- The reverb's damping test used an unfaded burst, which is a tone plus two
  clicks, and a click is broadband: it was measuring the clicks. The burst is
  windowed now.

One real bug in the FDN, found by `the_decay_time_is_roughly_what_it_says`
reading **-158 dB** where -60 was expected: the lines were sized individually
but share one write pointer, so the long lines' reads wrapped somewhere the
writer never reached and most of the network was reading stale silence. Every
line is the same length now, read at different offsets.

- `fontelle-fx/tests/{delay,reverb,distortion}.rs` (34 tests),
  `fontelle-engine/tests/effects.rs`, and the existing generic tables in
  `fontelle-types/tests/{parameters,effect_mix}.rs`, which now cover all five
  kinds — `every_stepped_parameter_names_its_positions_or_none_of_them` had
  been hard-coded to two.

### The ring that was answered and drawn by nothing

`docs/handoff.md`'s second item: *"`is_automated` is implemented, answered by
the session, covered by a test — and drawn by nothing."* TDD §12.2 asks for a
distinct ring colour on a control under automation, and a knob a lane had taken
over looked exactly like one nobody had touched.

The answer was never the missing piece; the **join** was. `InstrumentParam`
gained an `automated` flag and `InstrumentView::mark_automated` sets it — after
the view is built rather than while, because the caller knows which addresses
are automated as **one set** and asking the document per parameter would walk
every clip in the project once per knob, forty-nine times for an EQ.

- It is the knob's **groove** that changes colour, not the value arc: the value
  arc is the part you read the setting off, and it has to keep saying what the
  setting is.
- A switch and a chooser have no groove, so the ring goes round the chip and
  round the row of pips — the same statement in the shape those controls have.
- The wet/dry dial **on the mixer strip** wears it too (`InsertInfo::mix_automated`).
  The mix is automatable like every other parameter, and a ring that appeared
  on one of the two places it is drawn and not the other would be a worse
  answer than neither.
- `mark_automated` **clears** as well as sets. The panel is rebuilt whenever
  the revision moves, so it runs again after a lane is deleted, and a ring left
  on a knob nothing owns any more is worse than no ring: it is a ring that
  lies.

Theme format **v6** adds `param_automated`, with the migration arm a v5 file
needs. Amber, which is the one hue left that neither the accent, the playhead,
a note nor a clipping meter has already claimed — and deliberately not one of
the three ramps, for the reason `meter_peak` is not: it is a statement about
*who is holding the control*, and a teal ring on teal chrome says nothing.

`a_knob_under_automation_wears_a_ring_an_ordinary_knob_does_not` in
`render_headless.rs` renders the panel and compares the automated knob's cell
against an ordinary one's **in the same picture**, which is the only form of
the claim that means anything to somebody looking at the panel.

- `fontelle-ui/tests/{instrument,theme,render_headless}.rs`,
  `fontelle-app/tests/insert_chains.rs`.

### Still open

- **Bitcrush, soften and repitcher are still `todo!()`**, and so is the
  limiter's *reachability* — its DSP is real and complete, but it has no
  `EffectKind` variant, and it is the one with lookahead latency to compensate,
  which is why it was not folded in here.
- **Tempo-synced delay** needs the tempo in `ProcessContext`.
- Items 3-7 of `docs/handoff.md` are untouched: `patch/voice/polyphony`
  automation is inert, cutting an automation clip can step at the seam, no lane
  reordering, rename has no caret, and raising an open editor window is still
  unverified on a real compositor.

## 2026-09-01: every knob automatable, and a window for the effects that had none

The two things the pass below left open, closed.

### Every knob, not just the two

*"there's no way to actually turn a knob into an automation clip. i want to be
able to right click on a knob and select create automation clip."*

The channel's own volume and pan became addressable in the pass below; the knobs
**inside** the instrument — a cutoff, an envelope stage, an oscillator's level —
did not, and their menu entry said so. What stood in the way was where the
mapping lived: turning one of §8.2's addresses into a change to a `Patch` was in
`fontelle-app`, which the audio thread cannot see (INVARIANT 4 runs the other
way).

It lives in [`fontelle_core::patch_params`] now, beside the `Patch` it writes to,
and both callers use it — the panel to draw and set, `Sampler::set_patch_param`
to apply what a lane is holding. That is not tidying-up: it is what makes *"every
knob is automatable"* true **by construction** rather than by two lists agreeing.
`realise` registers the addressable parameters by asking the *panel* for its own
list (`instrument::patch_addresses`), so a knob added to the panel is a knob the
graph can reach, and `every_address_the_panel_offers_is_one_the_graph_can_reach`
is the test that says so.

- `ParamTarget::ChannelPatch { channel, param }` — `channel:<id>/patch/...`,
  exactly as §8.2 writes it. **Anything after `patch/` parses**: which controls a
  patch has is the patch's business, and INVARIANT 7 already says an unknown name
  changes nothing and is not an error, so a project from a later build opens and
  its lane survives being saved again.
- The lane starts at the value the knob is on, and a second right-click **opens
  the lane that is there** rather than stacking a second curve on the first.
- `fontelle-core/tests/patch_params.rs`, `fontelle-app/tests/patch_automation.rs`
  (which drives real blocks through a real graph — a sweep to the bottom of the
  cutoff has to be *heard*), `fontelle-app/tests/instrument_editor.rs`.

### A window for every effect

Clicking an insert in the track-options column did nothing at all unless it
happened to be an EQ, because the EQ was the only effect with a panel. A
compressor was a row you could add, bypass and delete and never open — the same
"the button does nothing" the add-instrument button was.

`canvas::effect_view` already turned an effect's own `specs()` into a panel and
had never been drawn anywhere: another thing that existed and could not be
reached. It is what the window shows now, so **every effect added later gets a
window, read-outs in the right units and a right-click that makes an automation
lane, without anybody writing any of the three**.

Opening it exposed three things the list had been carrying quietly:

- A ten-millisecond attack read **"10 s"**. The compressor's times are stored in
  milliseconds — the DSP takes `attack_ms` — and the spec said `Seconds`, which
  nothing read until a window printed it. `Unit::Milliseconds` now exists.
- A two-position chooser read **"0.00"**. `ParamSpec::positions` names them, so
  detection reads *Peak* and *RMS*, and a band's type and channel read the words
  the document uses. Written beside the specs because a `static` read on the
  audio thread cannot call a method on an enum, and
  `a_bands_chooser_reads_the_way_the_document_does` keeps the two in step.
- Right-clicking one nested its address inside a second address, so the lane
  reached nothing — visible only because the window it opened was titled
  `mixer:4294967296/insert[0]/param/threshold`.
  `a_knob_can_be_written_by_its_address_or_by_its_id` is the test.

## 2026-09-01: a spectrum behind the curve, a synth in every new channel, a blade on the arrangement, and two bugs you could hear

A long list from somebody using the studio, and it splits three ways: things
that were **not there** (an analyser, a cut tool on the arrangement, a right-click
menu anywhere), things that were there and **could not be reached** (the
transport from inside a plugin window, undo from inside one, a band's Delete),
and two that were **wrong** — one silent, one loud.

### The two bugs

**A note that sometimes did not play.** *"sometimes notes will not play if i
have a note extending before it all the way until where the new note starts, it
just has a chance not to play."* Two notes on one key, the first ending exactly
where the second begins, compile to a note-off and a note-on at the same sample
— and the compiler emitted them in whatever order the clip's arena happened to
hold the notes. When the off came second it found the voice the *on* had just
taken (the pool hands out the lowest free slot, and the first note's voice can
already be free) and released it: the note was there, and silent. That it
depended on arena order is exactly what made it *"a chance"*.

Events at one sample now have a defined order — parameter values, then
note-offs, then slides, then note-ons — in `compile::rank`, and
`sort_events` is public so any later splice uses the same rule.
`fontelle-sequencer/tests/event_order.rs`.

**A knob that turned the wrong instrument.** *"i changed the volume on my hold
choir channel instrument and it was changing the volume on my bright yamaha
piano grand."* Not a mix-up over which channel was selected: the instrument
panel's volume and pan were the **mixer track's**, and every channel goes to the
master until somebody routes it elsewhere (see `Channel::mixer_track`), so two
panels were two knobs on one fader — and that fader was the master's, so it took
the whole song with it.

A channel now has its own `gain_db` beside the `pan` it already had, applied at
the sampler ahead of the bus, which is also what keeps it independent of how
many channels share a track. Both are addressable
(`ParamTarget::ChannelGain`/`ChannelPan`), so both can be automated.
`fontelle-app/tests/instrument_editor.rs`.

### A blank instrument that plays something

*"when clicking new instrument, right now it doesnt do anything until i select
an instrument in the soundfonts tab... then it actually happening later when you
werent intending."* A channel with no instrument has no panel, no keys that
sound and no knob that does anything — so the button that made one looked broken
and then looked haunted, because the click that finally seemed to work was
somebody choosing a preset for a channel they already had.

`Patch::basic_synth` is what a new channel plays now: a saw, a square an octave
down and a sine two, with only the saw up and enough headroom for a chord. That
needed `Source::Oscillator` — named in the patch format since it was written and
skipped by the renderer — to actually render, so `PreparedLayer` is an enum now
and a voice carries a phase per slot. The panel grew an **Oscillators** section:
shape, level, octave, tune and pan per oscillator.
`fontelle-core/tests/oscillator_layers.rs`.

### The analyser

*"currently theres no eq monitor graph drawn to view the frequency spectrum and
make edits based off it and see in realtime."*

Four pieces, each testable on its own:

- **The transform.** `fontelle_dsp::SpectrumAnalyser` — a 2048-point radix-2
  FFT over a Hann window, written out rather than pulled in (it is forty lines
  and would have been this crate's second dependency). `fontelle-dsp/tests/spectrum.rs`.
- **The tap.** `fontelle_engine::SpectrumTap` — a lock-free ring the audio
  thread *copies* into, one relaxed store a frame. The transform runs on the
  window, once a frame, only while an EQ's window is open; an analyser on the
  audio thread would make every mix pay for a picture nobody is watching.
  `fontelle-engine/tests/spectrum.rs`.
- **The mapping.** The transform's bins are linear and the picture is
  logarithmic, so each of 96 bands takes the **loudest** bin it covers — an
  analyser is read for where the peaks are.
- **The picture.** Filled to the floor of the plot behind the curve, and in the
  panel's ink rather than the accent, because the accent is the curve.
  `fontelle-ui/tests/inserts.rs`, and `fontelle-app/tests/spectrum.rs` is the
  end-to-end one that would have caught a chain wired at every joint and
  connected to nothing at one end.

It is taken **before** the effect, on purpose: the curve is drawn over the
signal you are shaping, so a cut you have just made leaves a dip in the *curve*
against an unchanged spectrum rather than flattening the picture and leaving
nothing to aim at. A bypassed insert still feeds it.

### Right-click, everywhere it was missing

One `canvas::menu` — geometry and strings, like every other piece of layout here
— and three things hang off it:

- **A channel**: open, rename, duplicate, clear its instrument, delete.
  `DuplicateChannel` copies the instrument *and* the clips, onto a row of their
  own, as one history entry.
- **A lane**: add, rename, mute, delete. *"i made one i dont want but i cant
  right click and delete it."* `RemoveLane` takes the clips on it with it, and
  the last lane cannot go — greyed rather than hidden, because a menu that
  hides the entry teaches nothing about why it is not there.
- **A knob**: *"create automation clip"*, which is §12.4's rule on the panel
  that did not have it. The patch's own knobs are not addressable yet (§8.2),
  so their entry says so rather than doing nothing.

Renaming has no text buffer beside it: every keystroke goes through the rename
command, which coalesces, so the document *is* the buffer, the row redraws as
you type, and one Ctrl+Z takes back the whole name.

### The cut tool on the arrangement

*"should work like the same tool in fl studio and correctly split up looped
clips and everything taking into account all edge cases cleanly."* `C` picks it
in whichever canvas has the keyboard, and it draws a line like the roll's.

`SplitClip` is where the edge cases are written down: a cut on an edge is not a
cut, a note across the cut is cut with it, and a **looped clip stays two looped
clips** — with the right-hand half's content *rotated* to the phase the loop was
at, because a second half that restarted the pattern would be a cut you can
hear. `fontelle-model/tests/arranging.rs`, `fontelle-ui/tests/timeline.rs`.

### The windows that could not hear you

*"cannot use keybinds to like pause and play when i have one of the opened
windows like an eq plugin window selected"*, and *"undoing and redoing isnt
working in there either"*. An editor is a separate OS window, so the whole
keyboard stopped at its title bar. `global_key` is the set that means the same
thing everywhere — transport, history, save, export — and every window answers
it. What is deliberately *not* shared is the canvas keys: Delete means "the
selected band" in an EQ window and "the selected notes" in the roll.

Two more from the same list: **Delete removes the selected EQ band**, and the
band handles are 18 logical pixels rather than 11 (*"its kinda easy to miss
them"* — eleven is under the 16 every desktop guideline asks for, on a target
that is dragged in two axes and carries a number).

And **clicking an instrument or an insert that is already open raises its
window** instead of doing nothing. Two calls, because one desktop in three
ignores each: `focus_window` is what X11, Windows and macOS take, and under
Wayland a client may not take focus by asking — it has to be given it, through
an xdg-activation token, which is what `request_user_attention` asks for.

## 2026-09-01: the EQ you could not touch, wet/dry, even rows, and searching inside every soundfont

Another pass driven entirely by somebody using the window, and the
characteristic failure mode shows up twice more: a control that exists in the
document with nothing on screen able to reach it, and a control that *was*
reachable but whose picture never changed, which from the other side of the
screen is the same thing.

### Keys light up under your hands

*"when midi keys are pressed and triggered it should highlight the note of the
piano in the piano roll... so its easier to say play something on the midi
keyboard and see which notes you might wanna draw in."*

`fontelle_midi::LiveKeys` is a pair of atomics — 128 bits, one per key — that
every device callback ORs into and the window loads once a frame. It mirrors
what the router is **sounding**, not what arrived on the wire: transpose is
applied on the way in, so the key that lights is the key you would draw, a note
the sustain pedal is holding stays lit until the pedal lets it go, and a device
unplugged mid-chord takes its own lights out with its notes. While anything is
down the window animates at frame rate rather than at the 100 ms idle poll, so
the light goes out when the key does. `fontelle-midi/tests/live_keys.rs`.

### The EQ drew a curve nobody could change

*"the eq effect is uninteractable i just see a flat line, however it does SOUND
like it is making an audible change."*

Both halves of that sentence were **one bug**, and it is the pattern this file
keeps recording. `Session::set_eq_band` wrote the document and published the new
config to the running graph — so it was audible — and did not move the studio's
**revision**. `WindowApp::refresh_studio` re-reads its lists only when the
revision moves (asking every frame allocates), so the editor kept drawing the
config it had cached when the window opened: eight flat bands, for ever. Every
other dragged control in the session already bumps it on the line after it
publishes. `fontelle-app/tests/eq_editor.rs` is that one line, as a test.

With the picture live again, the editor became worth building:

- **Eight numbered band chips.** A fresh EQ has every band switched off and
  therefore no handles at all — which is what *"just a flat line"* looks like.
  Clicking a chip switches its band on at a home frequency spread across the
  spectrum (60 Hz to 15 kHz, so eight bands are not eight handles stacked at
  1 kHz) and selects it; Ctrl-click switches it back off.
- **A row of controls for the selected band**: type, frequency, gain, Q, which
  part of the stereo image it works on, solo, off — and the effect's wet/dry.
  The numbers drag (Shift for fine), the choosers step forward on a click and
  back on Ctrl-click, and the wheel works over any of them.
- **Right-click any of them for an automation lane** (§12.4 taken at its word).
  The EQ has had 48 addressable parameters since it landed; what was missing was
  a rectangle per parameter to right-click.
- **A grid that says what it is** — the decade frequencies labelled along the
  bottom, ±12 dB and 0 down the side, the summed curve filled back to the zero
  line, and the band in hand drawn faintly behind it.

`fontelle-ui/tests/inserts.rs` covers the geometry, the hit-testing, the
read-outs and the per-band curve.

**What it still had not got** was a spectrum analyser behind the curve — an FFT
and a tap off the audio thread, which is a feature of its own rather than a
control that was missing a rectangle. The pass above is that feature.

### Wet/dry on every effect

*"i should have a knob to adjust the sound of the dry sound (before the plugin)
and the wet sound (after the plugin processes the dry sound) blending like how
fl studio and other daws do it."*

`mix` lives **in each effect's config** rather than beside the bypass on the
slot, and that buys three things for one field: it is addressable (so an
automation lane can sweep it), it crosses to the audio thread on the live
channel the knobs already use, and it is saved and undone by machinery that
already exists. `EffectNode` keeps a dry copy in scratch sized in `prepare` —
no block allocates — and blends with two gains rather than an equal-power law,
because parallel processing is a *sum*: a fully dry insert has to be exactly the
wire it replaced. There is a **dial** on every insert row in the track-options
column, with its number beside it (right-click it to automate it) and the same
control inside the EQ's own window. A dial rather than the groove it was first
drawn as, and turned rather than slid — `knob_value`, the same arithmetic the
instrument editor's knobs use, so every knob in the window behaves alike. `fontelle-types/tests/effect_mix.rs`, `fontelle-engine/tests/inserts.rs`,
`fontelle-model/tests/inserts.rs`, `fontelle-app/tests/eq_editor.rs`.

### The roll's rows were not all the same height

*"there is inconsistant sizing on the notes in the piano roll... single white
keys wont be as tall as other white keys."*

Two causes, and both are fixed in `canvas::key_row`, which is now the one
rectangle the keyboard, the grid's rows and the notes are all drawn in.

1. **Fractional pixels.** Every vertical zoom multiplied `key_height` by 1.2, so
   two notches from 16 gave 23.04 and the rasteriser rounded the rows to
   23, 23, 24, 23, 24. `zoom_y` keeps it a whole number now, rounding away from
   where it started so a step at the bottom of the range still moves.
2. **The strip drew a real piano.** A natural was one row and an accidental was
   a short bar over the *left* of its own row, so the white left over beside C#
   read as part of C's key: C looked half again as tall as E, which has no
   accidental above it. Every key gets an even band now, with a hairline under
   all of them; the accidentals keep their short black bar inside a band the
   same height as every other.

### A list of names instead of a keyboard

*"there should also be view options to switch between a piano visual view or
just a plain list of names... useful for drums since like right now if a drum
sound if on a black key i cant even read it."*

A chip on the roll's toolbar switches the strip between `keys` and `list`. The
list drops the black-and-white for one even band per key carrying the name of
what is on it — the sample's own name where the instrument has one, the note
otherwise — and takes the wider strip, because a column of names on 56 pixels is
a column of first syllables. It is **per channel and saved with the song**
(`Channel::named_keys`): a kit wants the list and the piano beside it wants the
keyboard.

### The lane seam was five pixels

*"the velocity / pan etc. section at the bottom does have a knob to drag it but
i am unable to drag it right now."* Driven with XTEST against the real window,
the drag itself turned out to be correct — press on the seam, move, and the lane
follows to the pixel. What was wrong was the size of the thing you have to hit:
`LANE_GRIP` was 5.0, which is three physical rows on a scaled display, and a
target that thin is one you miss and then conclude is not a control. It is nine
now, and the extra pixels come out of the grid, never the lane: a grip over the
top of the bars would eat the clicks that set a velocity to its loudest.

### Searching inside every soundfont

*"we can search through soundfonts, and then sounds inside the soundfonts, but
we are not able to search for sounds from within ALL of our soundfonts! ...type
"tuba" and of course no soundfont would show up since i dont have any sf2 file
named that but within several of my sf2s are sounds called tuba."*

Typing in the browser now searches the presets of the **whole collection**, with
the hits grouped under the soundfont they came from and the open soundfont's own
hits first. Clicking one loads it and brings the browser with it.

The interesting half is that listing a soundfont's presets means reading and
parsing the whole file, and a collection is hundreds of megabytes: doing that on
the UI thread at the first keystroke would freeze the window for seconds, which
is a worse feature than no feature. So the scan runs on a thread of its own, the
results arrive in `Session::pump` (where the graph's leavings are already
freed), the list fills in as they land, and the status line says how far it has
got. It is built once, lazily — somebody who never types in the box never pays
for it. `fontelle-app/tests/browsing.rs`.

### Seen on screen

Every one of these was driven with XTEST against the real window on a nested X
server and checked by reading pixels back, which is how the harness's own
mistakes were caught (events sent before the window had mapped, and no window
manager to give it keyboard focus):

- Every key band in the strip measures **exactly 16 pixels**, and 32 after three
  vertical zooms — no alternation.
- The `keys`/`list` chip widens the strip to the name column and back.
- The lane seam drags: 78 pixels to 159, following the pointer.
- Adding an EQ, turning its wet/dry dial, opening its window, switching a band on
  from its chip, dragging the handle (the curve bulges), stepping the band type
  (the shape changes), and dragging the wet/dry — which moves the groove on the
  mixer row at the same time.
- Typing in the browser fills the preset list with hits from across the
  collection, under headings, on the real 61-soundfont bank.

## 2026-08-31: a mixer you can build in, and the wires nobody had run

Everything in this pass came out of one session of *using* the window, and the
shape of it repeats: the document could already do the thing, and nothing on
screen could reach it. That is now the eleventh time PROGRESS.md has recorded
this pattern, and it is worth naming as the project's characteristic failure
mode rather than as a run of coincidences.

### Three wires that were never connected

- **The metronome was silent.** `MetronomeNode` had eight passing tests and no
  *beat*: `Metronome::new` starts at zero samples per beat — the "no tempo yet"
  value, which clicks nothing rather than dividing by it — and the only caller
  of `set_beat` was `Session::publish_metronome`, which had no metronome to
  publish to, because `Session::new` never took one. `realise` publishes the
  beat now, being the one place that sees both the tempo map and the node built
  out of it. `Session::with_metronome` is the other half: without it the button
  went dead the first time anybody chose a soundfont, since `rebuild_graph`
  passed `None`, `realise` minted a fresh metronome and the transport bar went
  on holding the original `Arc`. That failure had *no* failing test and could
  not have had one until the session held the switch.

- **A MIDI keyboard did nothing.** The window never opened a port: the hub was
  built only under `--midi-in`, and the loop that opens devices ran only on the
  path with no window. A window listens by default now, polling for hot-plug on
  its own thread. It also plays **what you have selected** (§14.3, which was
  "the first channel in the song") through a shared `LiveTarget` the UI thread
  stores into and each device callback loads. The router *latches* it and
  releases what it holds on the old instrument before adopting the new, because
  a note-off has to reach the instrument its note-on went to.

- **The Projects tab drew its status line across its own buttons.** Both rows
  were measured from `buttons.y`. The footer takes one row at a time off what
  is left now — a shape two rows cannot share however many are added later.

### The mixer became a place you build

Reported as three complaints and it is one: the mixer was a read-out of tracks
made somewhere else.

Strips select (the name row *and* the body — a strip you must aim at a
22-pixel caption to choose is one nobody realises they can choose), a `+` past
the last strip adds another and moves along, and a **track-options column**
sits between the last strip and the master carrying everything a 76-pixel strip
has no room for: where the track goes, its insert chain as rows you can read,
reorder and delete, and its sends.

`SetTrackOutput` writes `MixerTrack::output`, which had been in the document
since the mixer was written with no command to set it.

### Sends (§13.2), finally

`MixerTrack::sends` has been in the document just as long, `Mixer::has_cycle`
has counted send edges from the day it was written, and `SendNode` was two
lines and a comment.

`SendNode` is `BusSumNode` plus a level and a pan — which is exactly what
`BusSumNode`'s own note said a send would be. Pre-fader taps go after the
inserts and before the fader; post-fader after it. "Pre-fader" is a claim about
the *fader*, not the chain, and both readings have a test.

**The ordering bug this caught:** `realise` schedules tracks deepest-first, and
depth was measured along `output` alone. A send crossing from a shallow track
into a deeper bus is a feeding edge too, and the old measure would have put the
tap after the bus it feeds had already been read — the block silently gone.
`depth_to_master` is a longest-path walk over both edge kinds now, and
`soloed_audible` follows both as well, so a solo neither strands a reverb nor
closes the bus its own send feeds.

### Automation you can see

`Session::clips` filtered to `ClipSource::Notes`, so an automation clip was in
the document, compiled, audible and invisible. A clip says what kind it is now
and carries its curve flattened onto the block; the arrangement draws the
shape, which is the whole content of the clip. They get a lane of their own,
reused per *parameter* rather than per gesture — §12.4's "the current lane" put
the curve on top of the notes.

### Hover tips

The window is nearly all icons, and an icon set is a private language until
something translates it. The placement is arithmetic in `crate::tooltip`; the
words are a `tip()` beside each hover enum's existing `label()` and `icon()`,
so a control added without an explanation is a hole in one match rather than a
silent miss in a table somewhere else.

### Four things only looking at it could find

Opening the window and clicking found four pieces that were built, tested, and
never connected to a rectangle: **"+ Add effect" always added an EQ** (the
`EffectMenu` existed, with six tests, and nothing built one — so the compressor
was unreachable from the window entirely); the **Automation tab** was laid out
and hit-tested and never *drawn*; a **tooltip covered the menu its own button
had opened**; and an **automation block's curve was invisible**, drawn in a
lane colour picked to be quiet when it was a fill and wrong when it is the
line.

None of these could have been caught by a test that did not render, and three
of them are exactly the kind of thing §2.5's "the pixels have been seen once by
a human" exists for.

### Verified against the real thing

The window opens the machine's actual MPK mini 3 on its own; the metronome
button lights and its node has a tempo; three clicks on the `+` build three
tracks and the `+` moves along each time; a right-click on a fader puts a curve
on the arrangement and opens it.

### Still not built

- **The compressor's sidechain.** The DSP takes a key signal and now there are
  sends to carry one — what is missing is an address for "that effect's
  detector input" as a send *target*, which is a routing destination that is
  not a track.
- **Tempo automation.** `transport/tempo` addresses and parses and nothing
  compiles it: §12.3 wants the tempo map *generated* from it, which means
  `TempoMap` stops being stored and starts being derived. A change to who owns
  the tempo, not a lane to add.
- **Renaming a mixer track from the panel.** `RenameMixerTrack` exists and is
  tested; the options column's title row selects the track instead, because
  there is no text field in this window yet.
- **Latency compensation** (plan item 16).

### Tests

`fontelle-app/tests/click.rs` (7) and `sends.rs` (11),
`fontelle-midi/tests/focus.rs` (7), `fontelle-engine/tests/sends.rs` (10),
`fontelle-ui/tests/track_options.rs` (20) and `tooltips.rs` (14), plus the
send, routing and selection tests added to `fontelle-model/tests/routing.rs`,
`fontelle-app/tests/mixer.rs` and `insert_chains.rs`. 1522 in the workspace.

## 2026-08-31 (last): the compressor, and automation on everything

> *"Continue closing the remaining open not-built gaps, and also one note:
> please ensure we keep everything automatable — any value in these mixer
> effects I should be able to turn into an automation track in my timeline."*

That note is TDD §8.2's INVARIANT 7, and §8.2 is blunt about it: **one**
addressing scheme, serving plugin export, automation targets, preset
serialisation, MIDI learn and undo command targets, and *"if an implementer
creates a second, parallel addressing scheme for any of these, that is a design
regression — escalate it."* So this pass built the contract first and hung
everything else off it.

### The parameter contract (INVARIANT 7)

`fontelle_types::ParamSpec` is §8.2's own list: stable id, display name, range,
default, unit, value distribution. `EffectConfig::specs()` returns one per
parameter; `get`/`set` read and write by id; `normalised`/`set_normalised` do
the same on the 0..1 scale §12.1 stores automation points in.

`ParamTarget` is the address, in §8.2's own format —
`mixer:<track>/insert[0]/param/band1.gain`, `mixer:<track>/gain`,
`transport/tempo` — with a parser, because the string is what is *stored* and
the enum is a view of it. Not a second scheme: the round trip is tested, and an
address this build does not recognise parses to `None` rather than to a guess.

The EQ declares **48** parameters, the compressor **8**, and
`every_effect_parameter_is_reachable_by_address` is the user's sentence written
as a test: a parameter `specs` lists and `get` cannot read would be a lane that
draws and does nothing.

Two design notes worth keeping:

- **Frequencies and times are logarithmic**, ratios too. A linear 20 Hz–20 kHz
  automation lane spends nine tenths of its travel above 2 kHz.
- **Stepped parameters land on steps.** Automating a band type produces band
  types, not numbers between two of them, and
  `a_stepped_parameter_reaches_every_one_of_its_steps` checks all eleven are
  reachable rather than ten and a rounding error.

### The compressor (§13.4)

Feed-forward: measure, decide a gain from the transfer curve, smooth it with
attack and release, apply it. Feed-forward rather than feed-back because the
decision is then a pure function of the input, which is what makes the curve
drawable and this file's 18 tests assertable.

Threshold, ratio, attack, release, soft knee, makeup, auto-makeup, peak/RMS
detection, and the sidechain input §13.4 asks for — the DSP takes a key signal
already, though nothing routes one to it yet (that is sends, below).
Stereo-linked like the limiter, for the same reason.

One test-side lesson: a peak detector watching a **sine** sees the
instantaneous level, so a short release lets the gain creep back between peaks
and the output measures about half a decibel above what the transfer curve
says. That ripple is real — it is why a fast release on a bass line distorts —
but it is the envelope talking, and the curve tests set a long release so they
are measuring the thing they claim to.

**And it is the first non-linear link in a chain**, which is what makes chain
order audible at all. `two_eqs_in_a_chain_commute_because_both_are_linear` said
this was coming.

### Automation, all the way through (§12)

`AutomationData` had been in the document since the format was written and
nothing had ever evaluated it. Now:

- **Evaluation** — `value_at`, every curve shape, and §12.2's two rules stated
  as tests: overlapping clips, **the later one wins** (not blended); after a
  clip ends the parameter **holds what it left** rather than snapping back to
  the knob. Before any clip starts it is `None`, because rule 2 is about
  *after* and a clip cannot reach back in time.
- **`Hold` got a meaning.** The TDD names `Stepped` and `Hold` without saying
  what separates them, and two flat shapes that both jump at the next point are
  one shape with two names. `Stepped` is a staircase; `Hold` is a full stop —
  this value for the rest of the clip, whatever is drawn after it, which is how
  a lane says "stop moving here" without deleting what follows.
- **Compilation** — automation clips become `ParamValue` events at the node
  that owns the target. The address is resolved **off the audio thread**, from
  a map the realisation step builds, because it is the only layer that sees
  both the document's addresses and the graph's node ids (the same reason
  `compile` already took `channel_nodes`). The control rate is a fixed 256
  samples: per-sample is a hundred thousand events a second for a smoothness
  nobody can hear, per-block is what the engine would like and what the
  sequencer cannot know. A flat curve costs one event, not one per interval.
- **Application** — `EffectNode` and `MixerTrackNode` apply what arrives, and
  **hold it across blocks**, which is §12.2's second rule from the audio side.
  Automation is read *after* the live control channel, so a lane outranks the
  knob while it has an opinion.
- **Editing** — `AddAutomationPoint`, `MoveAutomationPoints`,
  `RemoveAutomationPoints`, `SetPointCurve`, all through `History`.

The drag command stores **where the points were before the drag**, not a
running total of deltas. That is not cosmetic: a drag is sixty commands merged
into one, and summing sixty `f64` deltas then subtracting them leaves a point a
rounding error from where it started. This project's rule is that applying a
command and inverting it puts the document back *exactly*, and
`0.4000000000000001` does not.

### And it is reachable

**Right-click any control.** A mixer fader, a track's pan, an EQ band's handle
— §12.4's gesture, and it works from all of them through one code path because
they share the addressing scheme rather than because it was wired three times.
The clip lands on the current lane at the playhead, one bar long, **flat at the
value the control is already at**: a lane that jumped the parameter the moment
it was made is a lane nobody trusts.

It opens in an **automation editor** tab — the roll's shape with a value axis
instead of a keyboard. Click empty grid to make a point and keep dragging;
right-click a point to delete it.

The other two gaps this closes: the **effect menu** (`+ fx` now drops a list of
`EffectKind::ALL`, which a new effect joins by existing rather than by being
added in a second place), and a **generic parameter editor** — any effect that
declares `specs()` gets one, built as the `InstrumentView` this crate already
had, because "a list of addressed parameters" is what the instrument editor
already was.

### One bug worth recording

Widening `compile`'s signature meant patching 23 call sites, and the mechanical
pass gave `Session` an **empty** parameter map — so the running app compiled
automation that reached nothing. Every unit test still passed. It was caught by
`drawing_on_the_lane_is_heard`, which goes through `StudioHost` and asks the
compiled timeline what it holds. The lesson is the one the shakedown made:
tests that drive the trait the window calls find what per-layer tests cannot.

### Still not built

*(Everything but the last two was built in the pass above; kept as written so
the record of what was open when reads honestly.)*

- **Sends** (§13.2) — including the compressor's sidechain, which needs one
  track's audio routed to another's detector. The DSP is ready; the graph is
  not.
- **Drag-to-reorder inserts.** `MoveInsert` exists and is tested; no row drags
  yet.
- **Tempo automation.** `transport/tempo` addresses and parses, and nothing
  compiles it — §12.3 says the tempo map is *generated* by evaluating tempo
  automation, which means `TempoMap` stops being stored and starts being
  derived. That is a change to who owns the tempo, not a lane to add.
- **Automation clips are not drawn on the arrangement yet** — they exist, play,
  and open in the editor, but a lane full of them looks like a lane of empty
  clips.
- **Latency compensation** (plan item 16). No insert has latency yet, and there
  is a test asserting it, so the day one does is not the day a phase error
  appears.

### Tests

`fontelle-types/tests/parameters.rs` (20), `fontelle-fx/tests/compressor.rs`
(18), `fontelle-model/tests/automation.rs` (22) and `automation_edits.rs` (14),
`fontelle-sequencer/tests/automation.rs` (11), plus the automation and
addressing tests in `fontelle-app/tests/insert_chains.rs` and the menu and
generic-editor tests in `fontelle-ui/tests/inserts.rs`.

## 2026-08-31 (later): mixer track effects — the chain, and the first thing in it

> *"I want you to start working on the mixer track effects."*

`MixerTrack::inserts` has been in the document since the format was written and
has never held anything, because `EffectSlot` was `{ effect_id: ParamAddress,
bypassed: bool }` — a name for an effect with nowhere to put a single
parameter. `EffectNode` was an empty struct with a comment saying the real one
lands in M4. `realise` said, in its own doc comment, *"inserts and sends are
not compiled"*. And of the nine effects in `fontelle-fx`, exactly one — the
limiter — had a body; `ParametricEq::process` and `Compressor::process` were
`todo!()`.

This pass builds the **whole seam**, end to end, with the parametric EQ as its
first inhabitant: document → command → graph → audible → on screen → saved.
Everything after it is a new arm on a match.

### Where an effect's parameters live

In `fontelle-types::effect`, beside `PanLaw` and for exactly its reason. The
document has to name them and may not depend on `fontelle-fx` (§4.1); the
effect has to read them; the mixer panel has to draw them. One type, shared,
rather than three parallel definitions of the same eight bands with translation
layers between them for them to drift in.

So the rule for every effect that follows: **its config lives in
`fontelle-types`, its state lives in `fontelle-fx`.** That is the split the
limiter already had before there was a chain to put it in, and it is the split
that makes a knob movable without the audio thread rebuilding anything.

(This adds `fx → types` to §4.1's arrow list. It is not a dependency *upward* —
`fontelle-types` is the shared vocabulary crate with no workspace dependencies
of its own — and the alternative is the drift above.)

### The parametric EQ (§13.4)

Eight bands, SVF per section, every type in the table: bell, low/high shelf,
low/high pass at 12/24/48 dB per octave, notch, band-pass. 35 tests, and every
one of them measures a **frequency response** rather than looking at a
waveform: a bell wired to the wrong coefficient still produces plausible audio,
a shelf whose gain is applied twice sounds like an EQ until you measure it, and
a cascade whose sections share one filter's state has a slope that is nearly
right.

Three things worth writing down:

- **The steep slopes are Butterworth cascades, not repeated sections.** Four
  copies of a Q=0.707 low-pass is 12 dB down at its own corner with a droop
  starting an octave early. The pole Qs of a Butterworth of order 2n are what
  make it flat to the corner and then steep, and they are a table.
- **The band-pass is normalised.** The SVF's own band-pass output peaks at Q,
  which is right for a voice's resonant filter and wrong for an EQ band: at
  Q=4 it would be a +12 dB bell nobody asked for, and a soloed band would get
  louder the narrower it got.
- **Per-band solo is *listen*** — a band-pass at the band's own frequency and
  Q, whatever kind of band it is. A soloed low-pass auditioned as a low-pass
  would just be the mix again.

### Mid/side, and a design error I had to be shown

The first draft had one `mid_side` switch on the whole EQ, which is what §13.4
says. The test for it failed, and the design was what was wrong: **a filter is
linear**, so `F(M) + F(S) = F(L)`, and filtering mid and side alike is exactly
filtering left and right alike. An EQ-wide mid/side mode with the same eight
bands either side of the rotation is provably a no-op.

It is per band now — `EqBand::channel` is `Stereo`, `Mid` or `Side`, and the
EQ only rotates if some band asks. Left/right as separate targets is a
deliberate cut: an EQ with some bands in L/R and some in M/S has no one
rotation to run in, and the useful half of the feature is the M/S half.

### The chain in the document

`EffectSlot { config: EffectConfig, bypassed: bool }`, and five commands, all
of them `Command`s through `History` (INVARIANT 9). 18 tests, of which the
sharp ones are about undo: removing an insert and undoing it puts back **that
effect, tuned as it was, at the index it was at** — not a fresh one of the same
kind, and not on the end. `SetEqBand` merges with itself while one band of one
EQ is being dragged, so a knob is one undo entry rather than sixty.

### The chain in the graph

`EffectNode` wraps the DSP and processes **in place on the track's bus**, which
is what makes a chain a chain: three inserts are three nodes scheduled in a row
on one pair of buffers, and the scheduler does not need a spare buffer per
slot. `realise` schedules them **before the fader**, where every mixer's
inserts are — the fader is the last thing on a strip, so pulling it down turns
the effects' output down rather than starving them of input.

**And an insert has a live end**, for the reason a fader does. An EQ knob is
dragged; rebuilding the `CompiledGraph` to move one number deserialises every
channel's patch and reloads every soundfont. So an insert writes both — the
command, for undo and the file, and `EffectControls`, for the sound between now
and the next rebuild.

A fader is four scalars and fits in atomics. An `EqConfig` is eight bands,
which is neither atomic nor a mutex the audio thread may take (INVARIANT 1), so
the mechanism is the **triple buffer** the compiled timeline already crosses
on. Unlike a fader's `Arc`, a triple buffer's two ends cannot be re-paired, so
a rebuild mints fresh ones seeded from the document — which is the source of
truth either way, so the two cannot drift.

### On screen

A **rack on every mixer strip**: a row per insert in chain order, each with a
bypass dot at its left-hand end that can be flicked without opening anything,
and a `+ fx` row under them. The rack comes out of the fader's space and only
as much of it as leaves a fader worth dragging — a rack that grew into one
would take away the mixer's one essential gesture the moment somebody used its
newest feature.

Clicking a row opens the **EQ editor**, a fourth tab in the editor column. It
is a curve you drag, not eight rows of numbers: an EQ *is* a shape, the shape
is what a person is deciding about, and the sum of eight bands is not something
anybody reads off a table. A press on empty curve switches the next unused band
on where it landed and the drag continues from the handle it just made — the
same "draw it and size it in one gesture" handshake the piano roll has.

**The curve is computed from the same numbers the filter is built from**
(`EqBand::response_db`, in `fontelle-types` beside the config), and
`fontelle-fx`'s tests hold the drawn curve against the measured response at a
dozen frequencies per band type. That check is the point: a curve that lies
about the sound is worse than no curve, because it is believed. It caught the
shelf formula, which was drawing +11.4 dB for a +8 dB shelf.

### What is not built

- **Only the EQ.** `EffectKind` has one variant. The compressor is next and is
  the interesting one, because it is the first **non-linear** link — which is
  what makes chain order audible at all. There is a test in
  `fontelle-app/tests/insert_chains.rs` saying exactly that, named for the
  truth (`two_eqs_in_a_chain_commute_because_both_are_linear`) after the
  version asserting "order is the sound" turned out to be asserting a bug.
- **No reorder gesture.** `MoveInsert` exists and is tested; nothing drags a
  row yet.
- **No effect menu.** One kind ships, so `+ fx` adds it rather than dropping a
  menu with one row in it. The menu is what the second effect brings.
- **Sends are still uncompiled** (§13.2), and latency compensation is still
  outstanding (item 16) — no insert has latency yet, and there is a test
  asserting that so the day one does is not the day a phase error appears.

### Tests

`fontelle-fx/tests/eq.rs` (35), `fontelle-model/tests/inserts.rs` (18),
`fontelle-engine/tests/inserts.rs` (9), `fontelle-app/tests/insert_chains.rs`
(12), `fontelle-ui/tests/inserts.rs` (22).

## 2026-08-31: rendering, backups, and the four properties nobody could hear

Three things, and the third is the seventh instance of the same defect.

### A render you can hand to somebody (plan item 11)

`Session::export_wav` renders the project offline at
`RENDER_QUALITY` — not the live quality, because an export has no deadline —
and writes 16-bit stereo into `<bundle>/renders/`. It goes *inside the bundle*
because of INVARIANT 10: Fontelle writes nothing outside places the user chose,
and the bundle is a place the user chose. A project that has never been saved
gets a message saying so rather than a file somewhere surprising.

Two details that only showed up by using it:

- **The name comes off the bundle, not off `meta.name`.** Opening
  `Bassline.fontelle` whose metadata still said "Untitled" produced
  `Untitled.wav`, an "Untitled" title bar and an "Untitled" row in the
  projects list — three places disagreeing about what the project is called.
  Fixed at both ends: renders are named from the bundle's stem, and `adopt`
  sets `meta.name` from it on open, so all three agree.
- **A second export is `Bassline 2.wav`,** by the same `unique_name` the
  projects folder uses. Overwriting the last render because you rendered twice
  is not a thing software should do quietly.

The Export button sits beside New on the browser's Projects tab, and Ctrl+E
does it from the keyboard.

### Autosave

`Session::autosave` writes the whole project as a bundle to
`<bundle>/backups/autosave.fontelle` every minute while the document is dirty.
It is a *backup*, not a save: it does not clear the dirty flag, does not touch
the title bar, and does nothing at all when the document is clean, when there
is no bundle to put it in, or right after a real save. The timer is
`fontelle_ui::widget::autosave_due`, which is arithmetic over two `Duration`s
and is tested as such — the window owns the clock, and a clock is not a thing
to test through a window.

(That closes item 10's last piece. The dirty title bar the older note lists as
outstanding has existed for a while — `refresh_title` appends a `•` and
sixteen call sites use it.)

### The four properties nobody could hear

`Note` has carried all five of §16.5's per-note properties since the document
format was written. The roll's property lane drew them, the lane menu offered
all six by name, dragging edited them, undo undid them — and `compile.rs` read
`key`, `velocity` and `pan` and dropped `fine_pitch`, `release`, `mod_x` and
`mod_y` on the floor. Four curves you could draw that changed no sound.

That is the **seventh** instance of this defect (after `SetNumber(Tempo)`, the
mixer's gain and pan, `Tool::Slice`, `VoiceConfig::glide_time_s`,
`RetriggerMode`, `Settings::projects_dir` and the bank's folder structure), and
it was the loudest of them, because unlike a setting nobody could find, this
one was on screen inviting you to use it.

What each one now means:

- **Fine pitch** is cents, added to the note's interval alongside the glide —
  the key itself never moves, because the key is the note's identity and a
  note-off names it.
- **Release** is `0..=127` and only ever *lengthens*, `0` being the patch's
  own release and `127` four times it (`MAX_NOTE_RELEASE`). `0` is what every
  note ever written carries, so any other reading would have changed how
  existing projects sound the day the field started being read. That is a
  compatibility rule, and there is a test asserting it rather than a comment
  hoping for it.
- **Mod X and mod Y** are two new `ModSource` variants, `NoteModX` and
  `NoteModY`, unipolar `0..1`. *Free* means the patch decides: nothing routes
  from them by default, so a note with both wide open under a patch that
  routes neither is sample-for-sample a note without them. Added after the
  existing variants, so no saved patch can name them and none of the old ones
  changes meaning.

All four ride on `EventPayload::NoteOn` and arrive as fields on `NoteTrigger`
— the struct that exists precisely so this kind of growth is a field rather
than a rewrite of every call site, which is what its own doc comment predicted
back when pan was the only one there.

### And a range that made one of them useless

`NoteProperty::FinePitch.range()` was `-8192..=8191`, mirroring a MIDI pitch
bend. Read as the **cents** it is documented in, that is ±81 semitones — so a
lane fifty pixels tall moved a note by about a minor third per pixel, and the
one thing a control called *fine* pitch has to be able to do is move a note a
little. It is now ±1200, an octave each way. Nothing had ever read the field,
so no project can hold a value this narrows away that anybody chose.

Worth naming as a pattern of its own: making a dead control audible is not
finished when the value arrives: a range nobody can aim inside is a control
that exists and still cannot be used, which is the same defect one layer up.

### Tests

`fontelle-core/tests/note_properties.rs` (11) measures the *audible* thing in
each case — pitch by counting cycles, release by the length of the tail a
note-off leaves, the two mod values by routing them at a patch's gain — plus
the two failure modes this class of field invites: a voice out of the pool
carrying the last note's properties, and two notes on one channel that have to
differ. `fontelle-sequencer/tests/note_properties.rs` (4) is the seam from
score to wire, with all four set to different values so a compiler wiring one
field to another's source fails rather than looks right.
`fontelle-model/tests/note_properties.rs` (4) is the ranges.
`fontelle-app/tests/exporting.rs` (12) covers the render and the backup.

### The shakedown, the half a machine can do (plan item 12)

`fontelle-app/tests/shakedown.rs` makes a whole piece the way a person makes
one — through the same `DocumentHost`/`StudioHost` trait the window calls, in
the order a person works: pick a projects folder, make a project, set tempo and
metre, load two soundfonts (one from a subfolder), add channels, build a mixer
by hand and route into it, draw a clip, write notes, draw properties on them,
mark a slide, loop the clip, save, render, and open it again from the browser
to find the same piece.

Through the *trait*, deliberately. Every one of this gate's dead controls was a
thing the model could do and the window could not reach, so a test that drives
`Session`'s internals is testing the layer the bugs were never in. And one long
test rather than eight short ones, because a per-feature test proves a feature
works while a session proves the features work **in each other's presence**,
which is the claim item 12 actually makes.

Three more alongside it: undo-to-exhaustion and redo-to-exhaustion are each
other's inverse over a whole session (state-based, not counting calls — how
many commands "add a channel with an instrument" decomposes into is the
session's business); a piece with nothing written in it still saves, opens and
renders; and **the rendered file is audible** — the samples read back out of
the WAV, with the same project minus its notes as the control, because a file
of the right length full of zeroes is exactly what every layer above can pass
while producing.

Three failures on the first run, and all three were the test's:

- The mixer's strip indices run **tracks then master** (`route_names` and the
  mixer panel agree; the route *menu* draws Master first as a row of its own,
  which is a display order and not that one). The test renamed master to "Low"
  and then routed a channel to it, which the session correctly read as "route
  to master" — `None`.
- Counting one undo per call assumed each call is one command. Adding a
  channel with an instrument on it is legitimately more than one. Rewritten as
  the invariant that actually matters.
- Setting mod X on a note and then making it a **slide** note, then looking for
  the mod X on its note-on. A slide compiles to `NoteSlide` and starts no
  voice, so it has no note-on to carry anything — correct, and now asserted
  as such rather than tripped over.

So the scripted half found nothing wrong with the product. That is a real
result and not a very reassuring one: what it covers is everything below the
window's own event handling, and the window's event handling is where all
seven dead controls lived. **The listening half — the actual sentence in §3,
"on hardware" — is still outstanding and needs a person at the keyboard.**

## 2026-08-30 (last): the soundfont browser learns about folders

> *"Make sure the soundfonts browser is able to handle multiple files and
> folders so I can see other folders in there and click into them to see their
> contents. Right now I'm only seeing just soundfont names even though my
> soundfont directory has folders."*

Exactly what it did. `SoundfontBank::rescan` walked the whole tree — six levels
deep, deliberately, "for how people actually organise a collection" — and then
**flattened** every file it found into one alphabetical list of names. The
organisation was read and thrown away. On the reporter's own bank
(`.../Patches/Soundfonts`) that turned six folders and sixteen loose files into
twenty-two names in a heap.

### Browsing and searching are two questions

The flat index is not the mistake; it is the other half. §17.5 calls instant
fuzzy search *the* feature that makes a large collection usable, and a search
that only looked in the folder you happened to be standing in would not be it.

So the panel does both, and **the search box decides which**:

- **Empty — browse.** Folders first with a count of what is under each, then
  the soundfonts in *this* folder, and a row back up. The structure is the
  answer.
- **Anything typed — search.** The whole collection, flat, each hit labelled
  with the folder it came from — because two files called `Kit` in different
  folders is the commonest thing in a collection and a list of bare names
  cannot tell them apart.

The status line follows: where you are while browsing, and *"12 matches in the
whole collection"* while searching, which is the surprising half.

### The decisions worth writing down

- **The top of a one-folder bank is its contents**, not a single folder row you
  have to click through first. With several configured folders there genuinely
  *is* a level above them, and it is the list of folders the user chose.
- **There is no way up from a configured root.** Fontelle reads nothing outside
  the folders the user named (INVARIANT 10), and a browser that could walk to
  `/` would be offering exactly that.
- **An empty folder is still listed.** Hiding it makes "I put my kits in there"
  unanswerable: the folder you are looking for is simply not in the list.
- **A folder that vanishes under you** — it is on somebody's disk and Fontelle
  is not the only thing that can move it — walks the browser up to the nearest
  folder that still exists rather than listing nothing with no way back.
- **The open file is remembered by its path, not its row.** The list moves
  constantly: a search, a folder change, a rescan. An index would name whatever
  landed in that slot afterwards, and the highlight now follows the file
  wherever the list puts it — or disappears, which is true when the file is not
  in the list at all.
- **The heading counts the collection, not the rows.** A heading counting what
  is in front of you says "2" inside a folder of two and reads as the
  collection having shrunk — and counts folders as soundfonts besides.
- **One method for both kinds of row.** A panel has one click;
  `StudioHost::open_file` is "activate row N" whatever the row turns out to be,
  because only the host knows what it was. The panel gets a `LibraryKind` for
  choosing a glyph and no business acting on it.

### Verified

`cargo test --workspace` green, clippy clean, fmt applied. New:
`fontelle-app/tests/browsing.rs` — twenty cases, half against the bank and half
driven through the same `StudioHost` seam the window uses. Seen in the real
window against the reporter's own bank: `Famicom`, `GBFont`, `GXSCC_gm_033`,
`Square`, `The_Ultimate_Megadrive_Soundfont` and `thenew (40 sf2)` as folder
rows with folder glyphs, the loose files below them.

**Two existing tests changed meaning and were updated rather than loosened**,
both because the behaviour they pinned is what this work replaced: with two
configured folders the browser's top level is now the two folders rather than a
flat list of everything under them, and a search no longer forgets which file
is open.

**A fixture that was quietly lying**, found on the way: `tests/browsing.rs`
first wrote sixteen zero bytes per "soundfont". Every browsing test passed over
a collection of rubbish, and the one test that actually *opened* a row was the
only one that noticed. It builds real, spec-valid SF2 bytes now — the same
hand-built kind the rest of the workspace uses.

## 2026-08-30 (later): a cut tool, and a way to make a clip at all

Two reports, and the second is the more serious of the two.

### The cut tool (`C`)

> *"We need a cut tool in the piano roll (C) that works basically the same as
> the FL Studio cut tool in the piano roll."*

`Tool::Slice` has been in the tool list since the roll was written, was bound
to `5`, and did nothing — the same shape as every other control this project
has found sitting there unreachable.

It is a **line**, not a click, and that is what makes it a tool: one stroke
across a chord cuts every note it crosses, and a **diagonal** stroke cuts them
at *different* times, which is a musical gesture rather than an artefact.

The rule is one sentence — *a note is cut where the line crosses the middle of
its own row, and only if that lands strictly inside the note* — and three
behaviours fall out of it rather than being special cases:

- a line drawn **along** a row never crosses its middle, so a horizontal
  wiggle inside a long note cuts nothing rather than cutting it somewhere
  nobody aimed at;
- a cut landing on a note's own **edge** is not offered, because a zero-length
  note is a note-on and a note-off at the same sample — silence you can
  neither see nor select (`SliceNotes` refuses one too, and both is right: the
  canvas refusing is what stops the gesture *looking* like it did something);
- each note is cut **once** per stroke, so a line that wanders back over a row
  does not offer a second cut measured against a length the first has changed.

`SliceNotes` is one command for the whole stroke, so a line across a chord is
one undo entry rather than one per note, and the second half of every cut
keeps everything the note was — velocity, pan, fine pitch, release, both mod
values, and whether it was a slide. The stroke is drawn while it is being
made: a tool whose gesture leaves no mark is one you have to aim blind on rows
fourteen pixels apart.

### You could not make a clip

> *"It's also way too difficult to just make a new clip in the arrangement
> right now — I can't even figure out how. It's like I'm given one clip when I
> make a new channel and I have to roll with that."*

Exactly right, and it was not difficulty — it was **impossible**. The
arrangement could move, resize, duplicate, delete, copy, cut, paste, mute and
loop clips; a press on empty grid started a marquee, always. The one gesture
anybody tries first did nothing visible.

The fix is FL Studio's, and it is a **tool** rather than a double-click: the
arrangement gets **Draw** and **Select** chips on the toolbar it already has,
and **Draw is the default**. Two reasons for a tool: a double-click is
invisible — there is nothing on screen saying it exists, which is the whole
complaint — and the roll next door already works this way, so it is one idea
rather than two. The marquee is still a press away (pick Select, or hold
**Ctrl**), and `P`/`E` pick the tool for **whichever canvas has the keyboard**,
the same rule Delete and Ctrl+C already follow.

A drawn clip lands **on the grid** rather than where the pointer was, is one
bar long, opens in the roll (you made it to put notes in), and plays whatever
else is already on that lane — which is what a lane *means* to somebody
looking at it, and stops a second clip on the drum row playing the piano.

### The same bug, twice, and what was done about it

The arrangement's Draw chip was laid out, hit-tested, themed — and the
renderer never lit it, because an edit script aborted before writing that
hunk. That is the **second** time in two sessions: the record and metronome
buttons went the same way. Both times every view-model test passed, because
the geometry and the hit-testing were right and only the pixels knew.

`the_active_tool_chip_is_lit_on_both_toolbars` joins
`every_transport_button_actually_draws_its_glyph` as the pair of tests that
catch this class. Both were found by **sampling the screenshot**, not by
looking at it — which is now three times measuring has beaten eyeballing on
this project, and is why `seeing-fontelles-gui` says so.

### Verified

`cargo test --workspace` green (**1128 tests**, up from 1092), clippy clean,
fmt applied. New: `fontelle-model/tests/slicing.rs`,
`fontelle-ui/tests/{slicing,arrange_draw}.rs`, four cases in
`fontelle-app/tests/studio.rs`, and the lit-chip pixel test. Seen in the real
window: the scissors on the roll's toolbar, and the two tool chips on the
arrangement's with Draw lit.

**One existing test changed meaning and was updated rather than loosened**:
`a_marquee_over_the_arrangement_selects_what_it_covers` pressed on empty grid
and expected a marquee. That is the behaviour this work deliberately replaced,
so the test now says which tool it is in, and why.

## 2026-08-30: routing, looping, projects, icons, slides, and the click

Six reports in one, and they are written up together because four of them
turned out to share a shape: a **field the model already had that nothing
could reach**, and one that changes what a thing *is* rather than what value
it holds.

### Channels do not own mixer tracks any more

> *"Channels shouldn't have their own mixer track. You should be able to make
> as many mixer tracks as you want and then route any channel to any mixer
> track you want. By default it just goes straight to master."*

That is FL Studio's model and it is the right one: a mixer track is a
**destination you build** — a drum bus, a reverb return — not a thing that
appears every time a soundfont is loaded. Twenty channels meant twenty strips
nobody asked for, and the one question a mixer answers ("what is going
where") had no way to be asked.

- `Channel::mixer_track` is `Option<MixerTrackId>`; **`None` is the master**,
  and that is where a new channel goes. A project written before this carries a
  bare id, which deserialises as `Some`.
- Four commands: `AddMixerTrack`, `RemoveMixerTrack`, `RenameMixerTrack`,
  `SetChannelRoute`. Deleting a track sends everything on it — channels *and*
  the tracks feeding it — back to the master rather than nowhere:
  audible-and-wrong is a state a person can fix, silent-and-wrong is one they
  have to debug. The master cannot be deleted.
- Every rack row has a **route chip** saying the mixer's own number (master is
  0), which drops a menu: Master, every track, and **+ New track** — which
  makes one *and* sends the channel to it, in one gesture.

**The consequence worth naming**, because it is a behaviour change and not a
refactor: **the rack's mute and solo had to move onto the channel.** They were
the mixer track's; with every channel on master by default, that switch would
now mute the master and silence the song. They are `Channel::muted` /
`soloed` now, and they are **sequencer** mutes — the compiler drops a muted
channel's clips, the same reading a lane mute has. The mixer keeps its own
per-track mute and solo; the two answer different questions and a project may
want both. `studio.rs`'s own mute test says so.

### Looping is a feature, and it is not copying

> *"I want to make it easier to loop things vs just extend the clip. [...] I
> want clips that I loop to be genuinely looped so it's just repeating what
> was in the first clip length. Copying and pasting is its own separate thing
> but looping is its own feature too."*

The distinction is real and the two look identical on the arrangement:

- **Copying** makes new clips with notes of their own. Editing one leaves the
  others alone; that is the point of it.
- **Looping** is *one* clip whose content repeats. One set of notes, so
  editing bar 1 changes every pass — which is what "repeat this drum pattern"
  means and what a copy can never give you.

`Clip::loop_length` is the period. The compiler emits the repeats from the one
set of notes, and three rules fell out of writing the tests: a note written
**past** the period is not part of the loop (it is what the clip would play if
it were not looping, and playing it every pass would be a second invisible
loop); a repeat that **starts** past the clip's end does not sound; and a loop
does not **ring** past its own end, or its last pass is longer than every one
before it.

The gesture is **Shift on the clip's right-hand grip** — the same grip, one
modifier, nothing new to find. A clip that already loops keeps its period when
stretched further: dragging a one-bar loop out to eight bars must not make it
an eight-bar loop. There is a **Loop** button beside Repeat in the arrangement
toolbar too, so the two readings of "play this again" sit side by side, which
is what teaches the difference.

**The picture**: seams with notches between the repeats and the caption
re-drawn faintly per pass, so a loop reads as a strip of tiles rather than as
one long block. Below six pixels a repeat the seams stop being drawn — at one
pixel each they are a filled rectangle, which is the solid block they exist to
stop looking like.

### A projects folder

> *"I want to be able to select any folder on my disk as my projects folder
> [...] I should be able to make new projects from within the app, browse my
> projects within the app, set my projects folder if not already set, change
> it if it is."*

`Settings::projects_dir` has existed since the settings file did and nothing
read it. `ProjectLibrary` reads it: `.fontelle` bundles with a manifest in
them, name and age ("3 days ago"), sortable by name or by recency.

The browser panel grew a **Sounds / Projects** switch rather than the window
growing a second panel — you want a project at the start and the end of a
session and a soundfont all the way through, so they are never wanted at once,
and a window that changes shape is one you have to re-learn. In Projects mode
the one list takes the whole area (a project has no presets inside it),
**+ New project** makes and opens one with a name nothing else in the folder
has, a row opens it, and the two folder buttons point at the projects folder
instead of the bank.

**INVARIANT 10 shapes all of it**: there is no default projects folder and no
guess at `~/Documents`. `None` means *ask*, scanning a folder that is not
there **reports** rather than creating it, and with nothing configured the
panel says so and offers to pick.

### Icons, and cursors that say which tool is live

> *"Make the cursor icons actually represent the action better, and I want the
> app to have more icons instead of just text."*

The Floptle repo was checked first, as suggested — its art is game assets
(swords, health bars) and there are no SVGs, so nothing there fits.

The icons are **drawn as paths** (`fontelle-ui/src/icon.rs`), not loaded.
vello is a path renderer, so an icon that *is* a path costs nothing extra and
is crisp at any scale; it takes the theme's own ink, so the light theme needs
no second set; it ships no files and tracks no licence. Thirty of them, in a
deliberately tiny vocabulary — polylines, filled polygons, circles — which is
what makes the whole set checkable by arithmetic: nothing empty, nothing
outside its box, nothing too small to read.

**The division that decides where an icon goes**: a **verb** gets a picture,
because a picture of an action is quicker to find than its name; a **read-out**
keeps its text, because the useful half is the value and no picture says
`1/16`. So the tools, the clipboard, the zooms, mute, loop, repeat and the two
new transport buttons are glyphs; snap, the lane chip and the onion skin are
still words.

And the same shapes rasterise into the **mouse cursor** — a small software
rasteriser, because a cursor is a 32-pixel bitmap made once at start-up and
reaching for the GPU pipeline to make one would tie the pointer to the surface
being alive. White with a dark outline, always: a cursor is drawn over the
user's own colours and a single-ink one disappears against half of them. The
draw, paint, select and delete tools each have their own now; everything else
keeps the desktop's, because a resize arrow drawn by hand is a worse resize
arrow.

### Slide notes, and portamento

> *"Expand the functionality of the piano roll to encompass things like slide
> and portamento notes that function similarly to FL Studio."*

Two ends of one piece of machinery, and **both were configuration nothing
read**: `VoiceConfig::glide_time_s`, `glide_legato_only` and `RetriggerMode`
have been on the patch — and on the instrument editor's panel — since the
format was written.

- **`Voice` has a glide**: an offset in semitones with a target and a rate.
  The voice's own `key` never moves, which is what lets the note-off the score
  wrote for key 60 still end a voice currently sounding key 67. Without that,
  every slid note in a piece hangs.
- **Mono and legato are implemented.** One voice per context: a note arriving
  while one is sounding takes it over rather than stacking. Legato keeps the
  envelope and the sample position and moves only the pitch; mono retriggers
  and carries the pitch over. Portamento is deliberately **not** applied in a
  poly patch — it would mean every note of a chord sliding from whichever one
  happened to be last.
- **`Note::slide`** is the score's end. A slide note starts no voice and ends
  none; it bends whatever is sounding to its pitch over its own length, and a
  slide with nothing sounding does nothing. `EventPayload::NoteSlide` carries
  it, in samples, because that is the only clock the RT side has.
- In the roll: a **slide chip** on the toolbar (and `A`) marks the selection
  *and* sets what the next note drawn will be — otherwise "make these slides,
  now draw another" produces an ordinary note in the middle of a run. A slide
  is drawn as the **ramp it is**, a wedge rising to the pitch it lands on,
  because it is not a note and must not look like one.

**The glide is block-rate**, and deliberately: the pitch a layer plays at is
already worked out once per block — the mod matrix's `LayerPitch` route, which
is what vibrato rides on, is computed in the same place. A per-sample glide
would be the only pitch modulation in the voice that was, and making all of it
per-sample is a change to the render loop rather than to this feature.

### Record-arm and the metronome

The last two pieces of item 9, and they go together: playing a part in time to
one you have not written yet needs something to play in time *to*.

**Arming is not a transport state.** `TransportState` has three values and none
of them is "stopped, but the next play records"; arming is a decision made
*before* play, and a fourth state would mean every `is_processing()` check in
the engine had to learn about it. So the bar has an `armed` flag and `play` is
what turns it into `Recording`. Arming throws the last take away, so a new one
does not begin with the end of the one before it still in the ring; stopping
turns what was played into notes on the open clip through `AddNotes`, so a
take is one undo entry like anything else, and says how many — "nothing was
played" is the commonest thing that happens to a record button.

**The metronome is a node, not a clip.** A click is not document data: it is
not saved, it does not bounce, it belongs to no channel, and a project emailed
to somebody else must not arrive with a woodblock on every beat. `Metronome` is
three atomics and `MetronomeNode` reads the transport's **position** rather
than counting its own elapsed blocks — which is what makes a seek and a loop
land on the beat. It sits after the master fader (so the fader does not move
it) and before the limiter (so it cannot clip), and it *adds* into the master
pair like every other source. The downbeat is louder, because a metronome that
only says how fast is a pulse.

**Its one documented limitation**: the beat is a constant number of samples,
written by the model side from the tempo map. Exact for a song at one tempo —
every song this build can create — and it drifts across a tempo *change*,
where the right answer needs a map the RT thread may not read (INVARIANT 3).

### Verified

`cargo test --workspace` green (**1092 tests**, up from 979), `cargo clippy
--all-targets` clean, `cargo fmt` applied. New test files:
`fontelle-model/tests/{routing,looping}.rs`,
`fontelle-sequencer/tests/looping.rs`, `fontelle-core/tests/glide.rs`,
`fontelle-engine/tests/metronome.rs`, `fontelle-app/tests/projects.rs`,
`fontelle-ui/tests/{routing,looping,projects,icons,slide,recording}.rs`.

**A bug the pixels caught and the view-model could not**, worth writing down
because it is the second time this project has been saved by measuring rather
than looking: the record and metronome buttons were laid out, hit-tested and
themed — and left out of the one array `draw_transport_bar` loops over. Two
buttons that could be pressed and could not be seen, with every geometry test
passing. `every_transport_button_actually_draws_its_glyph` is the test that
would have caught it, and does now.

Seen in the real window (nested Xwayland, per the agent memory), not only in
the headless dump: five transport buttons, the route chip on the rack row, the
Sounds/Projects switch, glyphs on both toolbars and on the editor tabs.

## 2026-08-29: a tempo you can set and a mixer you can use

> *"Currently we're lacking controls like tempo and the mixer for example."*

Both were the same shape as the last three reports of this kind: the feature
existed, in the model, with nothing on screen to reach it by.
`SetNumber(NumberTarget::Tempo)` has been in `fontelle-model` since Phase 1
item 3 and no pixel addressed it. `Project::mixer` has carried a gain, a pan
and two switches per track since the scaffolding, `fontelle_app::realise` has
compiled all four into the graph since this morning, and the only way to move
any of them was to edit a JSON file.

This closes **the last thing outstanding under item 9** of
`docs/first-usable-plan.md` — the mixer strip — and with it the last clause of
the §3 gate sentence that had nothing behind it: *"balance parts with
per-channel gain/pan/mute"*. (Record-arm and the metronome are still open.)

### The problem a fader poses, and the answer

A control you **drag** has to move the sound while it is moving, and has to
leave **one** undo entry behind when you let go. Those pull in opposite
directions here:

- The sound comes from a `CompiledGraph`. Rebuilding one runs
  `Patch::from_data` over every channel in the project — a JSON tree per zone.
- The undo entry comes from a `Command` through `History`, against the
  document, which INVARIANT 9 says is the only way anything may change.

Sixty graph rebuilds a second is not a fader. So a fader writes **both**:

- **`fontelle_engine::TrackControls`** — gain, pan, effective mute and two
  peak slots, as atomics, shared with the RT thread exactly the way
  `MasterMeter` already is. `MixerTrackNode` reads them once per block when it
  has one and falls back to its own fields when it does not, so every offline
  render and every existing test is untouched.
- **the command**, for undo and for the file.

`realise` builds the controls and hands them back; `Session` writes to both on
every movement and `apply_mixer_controls` pushes the document's values at the
running graph after any change. The two cannot drift, because a rebuild
re-seeds the atomics from the document.

**The bug this arrangement caused, and how it was fixed properly:** a fader
moved, then undone, left the document at unity and the *sound* where the drag
had put it — `undo` rebuilds the graph, and the rebuild minted a fresh set of
controls while everything holding the old ones went on writing to a graph that
had been thrown away. `realise_reusing` is the fix: a track's control surface
now lives as long as the **track**, not as long as a graph. A meter also keeps
its reading across an instrument change instead of dropping to silence, and
nothing holding one can end up talking to a dead graph.

**A side effect worth having:** muting or soloing from the channel rack used to
call `rebuild_graph`, which reloaded every soundfont in the project on every
click of the switch. It is three atomic stores per track now.
`studio.rs`'s own mute test says so — it reads the live control and asserts
that *no* new graph was published.

### The mixer panel

A third tab in the editor column, beside the roll and the instrument. One strip
per mixer track: a colour cap, the name, a pan with a centre detent, a fader
with a meter beside it, mute, solo, and the level in decibels underneath.

- **The master is pinned to the right and never scrolls.** It is where
  everything arrives, not one of the things arriving; a master fader you have
  to go and find is one you cannot use to set the level of what you are
  listening to.
- **The fader's taper is a four-point table**, not a formula:
  `-60 / -30 / -10 / +6 dB` at `0 / 0.25 / 0.60 / 1.0` of the travel. Unity
  lands at 0.85, so the top four-tenths of the throw cover the range a balance
  decision actually lives in. Piecewise linear, because it has to be **exactly
  invertible** — `fader_y_of_db` draws the handle where `fader_db_at` will read
  it back, and a fader that jumps the moment it is grabbed is unusable.
- **Both controls have a detent**, in pixels rather than in decibels so they
  feel the same on a short panel and a tall one. Getting exactly 0.0 dB or
  dead centre back by hand on a curved control is otherwise impossible, and
  "nearly unity" is a mix that drifts every time it is touched.
- **Virtualised** like the roll and the rack (§16.4): four hundred tracks build
  a screenful of rectangles.
- Metering is per track, taken **after** the fader — what a meter is for is
  telling you what you sent on, not what arrived. Read once a frame, and only
  while the tab is showing.

### The tempo and the time signature

Two boxes on the transport bar, after the position read-out, because *"where am
I"* and *"how fast is it going"* are read together.

They are the only controls on that bar that write to the **document** rather
than to the engine, and the view-model says so out loud: `action` now returns
`Option<TransportAction>` and answers `None` for both. Inventing a
`TransportAction` that `apply` would then have to refuse would have been the
easy lie.

- Drag the tempo (a quarter of a BPM per pixel, a fortieth with Shift), or roll
  the wheel over it (one BPM, a tenth with Shift). Clamped to 20–999 and
  rounded to the two places the box shows, in one function, so the number on
  screen is always exactly the number in the document.
- **`Project::beats_per_bar` is a real document field now**, defaulted so
  projects written before it opened in the 4/4 they were made in. The grid, the
  bar numbers, the arrangement's snap and the read-out all count by it — the
  `BEATS_PER_BAR` constant `Session` and the window were both hard-coding is
  gone. The **denominator is not settable and the box does not pretend it is**:
  `PPQN` is ticks per *quarter*, so a denominator other than four is a change
  to what a tick means everywhere, not a control.

### Verified

`cargo test --workspace` green, `cargo clippy --all-targets` clean, `cargo fmt`
applied. New: `fontelle-ui/tests/{mixer,tempo}.rs`, `fontelle-app/tests/mixer.rs`,
six `TrackControls` cases inside `fontelle-engine/src/nodes.rs`, three mixer
cases in `pointer.rs`, three machine-checked pixel shots in
`render_headless.rs`, and the editor-tab tests extended to three tabs.

The panel was **looked at**, through the headless dump rather than a window
(`FONTELLE_UI_DUMP=<dir> cargo test -p fontelle-ui --test render_headless`
writes `mixer.png`), and one thing only looking at it found: the grooves and
the meter wells were drawn in the window colour, which is four steps from the
panel's on this theme — a control at rest was a track you could not see there
was anything to grab. They take the border ink now.

**Not yet done in a real window.** The interaction wiring in `app.rs` — the
press routing, the two drags, the wheel — has no test that can run without one,
which is the same gap every GUI item in this project has had; the scene itself
is checked by machine through the same vello pipeline the window uses.

## 2026-08-29 (before that): the sixteen-layer wall

> *"A lot of notes are showing as ones that should be playable but just aren't
> producing any sound at all. It's making a lot of kits just incomplete to
> use."*

A real defect in the sampler, older than the key map that exposed it. One line:

```rust
for (slot, layer) in self.layers.iter_mut().zip(patch.layers.iter()) {
```

`Voice::layers` is a fixed `[LayerPlayback; MAX_LAYERS]` — sixteen — and `zip`
stops at the shorter side. Slot `n` played patch layer `n`, so **every zone
past index 15 never got a slot, never became active, and never rendered.** No
error, no warning; just a kit that half works.

That made `MAX_LAYERS` a limit on how many zones a patch may *contain*, when
TDD §7.4 means it as a limit on how many may sound *at once* ("stacked, or
split by key/velocity"). A key split is not a stack. A drum kit is one zone per
hit and real ones run to forty-odd.

Measured against the development bank before the fix — playable keys that made
no sound at velocity 100:

| soundfont | zones | keys the roll called playable | silent |
| --- | --- | --- | --- |
| `Mystical_Ninja_Starring_Goemon.sf2` — MN64 Drums | 46 | 84 | **68** |
| `Nokia_30.sf2` — Drums | 47 | 53 | **37** |
| `Setzer's_SPC_Soundfont.sf2` — Standard | 46 | 46 | **30** |

Velocity was not a factor in any of them: zero of those keys were `vel_range`
gated. It was the layer cap alone.

**The fix**: a slot now records *which* zone it is playing (`LayerPlayback::
layer`), and `trigger_note` fills slots with the zones that cover **this
note** — one or two in a kit — instead of being indexed by zone. `MAX_LAYERS`
goes back to meaning what it says, `Patch::layers` is explicitly unbounded, and
`render_with_pan` walks slots while still addressing every `ModDest` by the
zone's own index, because that is what a saved mod route names. Zones past 255
get no per-layer modulation rather than being aliased onto layer 0's routes.

Re-measured after, rendering a full second per key across the same bank: **29
silent keys out of 34,056**, all in one preset (`Nokia_30`'s Reverse Cymbal) at
the top of its range, where four octaves of upward transposition runs a quiet
attack past in a few samples. That is the sampler doing arithmetic correctly,
not a cap, and it is left alone.

**Worth noting for whoever reads this next:** the short render length in the
first diagnostic reported 5,978 "silent" keys, most of them false — a note
transposed *down* four octaves advances eight samples in a 256-sample block and
reads the quiet head of its own waveform. Measure a second, not a block.

## 2026-08-29 (earlier): the arrangement grows controls, and the grid becomes readable

Four reports, and three of them turned out to be the same shape: the feature
existed, in the model, with nothing on screen to reach it by.

### Pasting notes landed wherever the pointer was

> *"When I paste notes in the piano roll they're off time instead of snapped."*

`WindowApp::paste` worked out *where* from the playhead — or, when the playhead
is elsewhere in the song, from the raw pointer x — and handed that tick to
`PianoRoll::paste` untouched. Neither of those is ever on a line.

The snap now happens in `PianoRoll::paste`, not at the call site, because the
roll is what owns the division and `Alt` (see `live_snap`). A rule enforced by
the caller is a rule the next caller forgets. `Timeline::paste` does the same
for clips.

### The snap control was there and did not look like one

> *"I also don't see snap controls right now, please ensure we have those."*

It was in the roll's toolbar the whole time, drawn as the bare word `1/16`
sitting between `Del` and `-`. Nothing about a word in a strip says it can be
pressed — which is the *same* report that once produced the caret on the lane
chip, arriving again about the chip next to it.

The three read-out chips — snap, lane, onion skin — now always carry their
frame instead of only lighting on hover. And the arrangement, which had no
toolbar at all, has one.

### The arrangement had no controls

> *"We need better arrangement controls like looping, duplicating clips,
> copying and pasting arrangement clips, cutting, etc. Right now I can only
> change the length and move them around — like I made this drum loop but I
> can't repeat it."*

`TimelineView` has carried a `snap` since it was written and `duplicate` since
the arrangement existed. Both were reachable only from a keystroke you had to
already know, with the arrangement focused — which is not a control, it is a
rumour. Cut, copy and paste did not exist at all: `Ctrl+C` on the arrangement
copied *notes*, because the window's clipboard keys went straight to the roll
without asking which canvas the keyboard belonged to.

- `TimelineControl` and `timeline_toolbar_layout`: snap, Repeat, Cut, Copy,
  Paste, Mute, and the two zooms, in the same order as the roll's toolbar so
  the panels read alike. A button whose action would do nothing right now is
  drawn muted rather than hidden — a control that comes and goes is harder to
  learn than one that is plainly unavailable.
- `copy`/`cut`/`paste` now route by `Focus`, the way `duplicate` always did.
- **The clip clipboard lives in `Session`, not in the canvas.** A `ClipInfo` is
  a flattened view for drawing — no notes, no channel — so a canvas holding
  copied clips would hold what INVARIANT 2 says it may not see. It emits
  `ArrangeEdit::Copy`/`Paste` and the session keeps whole `Clip` values. That
  is also what makes **cut** work: paste has to put something down after the
  clip it copied from is gone, which a clipboard of ids could never do.

**The bug worth writing down**, because it is the reason `StudioHost::arrange`
now returns `Vec<ClipId>`: a duplicate's offset is measured from the selection,
so leaving the selection on the *original* makes pressing the key twice put two
copies in the same place. The roll solved this long ago — `AddNotes` hands its
ids back and `PianoRoll::notes_inserted` adopts them — and `Timeline::
clips_inserted` is that same handshake. It is what turns "duplicate" into
"repeat".

### The grid could not be counted

> *"It's kind of hard to tell the time right now, so ensure between bars or
> measures there's more dividers so I can see the on and offbeats."*

Two faults, and only one of them was the ink.

- **Subdivisions and beats shared `grid_line`.** Three levels drawn in two
  colours is a comb, not a ruler. Theme format v5 adds `grid_line_sub` for the
  faintest level; `grid_line` keeps the meaning its doc comment already
  claimed (beats) and `grid_line_strong` keeps bars.
- **The finest level was the *snap*.** Setting the snap to bars — or off —
  removed every line between the bars and left a bar-wide empty box to place
  notes in by eye. `subdivision_unit` fixes the rule: the grid is the ruler you
  read the time off, the snap is where a note may land, and the grid never goes
  coarser than an **eighth**, which is the coarsest division that still shows
  an offbeat.

Verified in the running window by reading pixels rather than by eye (see
`seeing-fontelles-gui` in the agent memory — twice now a colour has been "seen"
that the pixels disagreed with). Along one grid row: `#0d1c22` every 30 px
(sixteenths), `#18333d` every 120 px (beats), `#1f404c` at the bar.

## 2026-08-29 (earlier, third): pan you can hear, an eraser, and a keyboard that says what it plays

Three reports from the window, and the third turned into the largest piece of
this pass because getting it *right* meant measuring real soundfonts rather
than picking a rule that sounded reasonable.

### The pan lane was drawing a picture of a change

> *"Piano roll panning isn't working right now."*

It was not a UI bug at all. `Note::pan` was stored, editable in the property
lane, serialised, and round-tripped through save/load — and then
`fontelle_sequencer::compile` built a `NoteOn` without it, because
`EventPayload::NoteOn` had nowhere to put it. Everything downstream of that
point was working correctly on a value it was never given.

The seam now runs all the way through, and it is one conversion, written once:

- `EventPayload::NoteOn` carries `pan: i8`, in the range `Note::pan` is stored
  in. It is on the note-on and not a node parameter because §16.5's pan is
  **per note** — two notes sounding together on one channel may sit in
  different places, which a node-wide control cannot express.
- `fontelle_types::pan_unit` is the only crossing from the document's byte to
  the `-1.0..=1.0` the pan law takes. `SamplerNode::process` is the one caller.
- `fontelle_core::NoteTrigger` carries it onto a voice. It is a struct rather
  than a fifth argument on `Voice::trigger_from` deliberately: pan is the
  *first* of `Note`'s per-note properties to reach the audio path and will not
  be the last, and a struct grows a field where a signature this deep grows a
  rewrite of every call site.
- `Voice::render_with_pan` now adds **three** pans and clamps — the zone's, the
  note's, and the channel's live one. Any other reading throws one away.
- The live path carries it too (`Auditions`, `StudioHost::audition_on`), from
  the roll's template, so clicking a note you panned hard left sounds hard
  left instead of being centred until the transport reaches it.

**Still dropped, and deliberately flagged rather than quietly fixed:** the
other four properties `Note` carries — `fine_pitch`, `release`, `mod_x`,
`mod_y` — are stored, drawn and editable in the property lane and still reach
nothing. Each is a field on `NoteTrigger` when it lands. The lane will keep
looking like it works until they do.

### Right-click erases while it is held

> *"When holding right click hovering over anything should delete it, notes,
> arrangement clips, etc."*

Both canvases already deleted on a right *press*, and that was the whole of it:
the button went down, one thing under it went away, and the gesture ended.
Sweeping a bar meant clicking every note. Worse, a right press on empty grid
left whatever was in `self.gesture` from before untouched, so the next pointer
move carried on with it.

`Gesture::Erasing` in both `canvas/piano_roll.rs` and `canvas/timeline.rs`, and
it deliberately **carries no state**. Every other gesture here remembers where
it started because it emits deltas from that origin; an erase asks the current
document what is under the pointer, so a note already gone is simply not found
again. That is what makes "held still, asks for nothing" true by construction
rather than by a `last` field somebody has to maintain — which is exactly the
trap this codebase has now fallen into four times (`tests/erase.rs` asserts it).

Two details worth keeping:

- It hit-tests the **unclamped** pointer. Clamping is right for a move — a drag
  that strays onto the keyboard still means the nearest cell — and destructive
  for a rub-out, where it would delete whatever sits at the edge you slid past
  on your way off the canvas.
- The right button no longer reaches the roll's or the arrangement's chrome at
  all. Sounding a key, moving the playhead or grabbing the lane seam in the
  middle of a sweep is the window answering a question nobody asked.

The Delete tool sweeps on a left drag by the same path, which it always should
have.

### The piano roll now says which keys the soundfont can actually play

> *"Often I will use drum soundfonts however the piano roll shows these the
> same as normal ones, making it so that you have no idea which notes actually
> play anything ... instead of trial and error trying to figure out which notes
> actually play something for these drum soundfonts that only have a few sounds
> every few notes."*

Both facts needed were already in the patch and neither had any way to reach a
canvas. `Layer::key_range` is not decoration — `Voice::trigger_note` marks
every layer that does not cover the key inactive, so a note outside all of them
starts a voice with nothing in it and is *genuinely silent*. And the importer
read each sample's name to find its audio and threw it away.

- `ImportedPatch::names` and `LoadedSample::name` keep it, on **both** paths —
  the import and the reopen — so a kit is labelled whether the project was just
  built or just opened (`tests/project_bundle.rs` pins the round trip).
  Deliberately not on `Patch`: the name is a fact about the file, not about the
  user's instrument, and INVARIANT 8 keeps file-derived identity off disk.
- `SampleLibrary` holds them beside the provenance it already holds.
- `fontelle-app`'s new `keymap` module turns a `Patch` into a
  `fontelle_ui::document::KeyMap` — one entry per MIDI key, playable or not,
  named or not. It lives there for the reason `instrument::describe` does: the
  UI may not see a `Patch` (INVARIANT 2).
- The roll greys dead rows and dead keys, draws a note on one in
  `note_silent`, and writes the name beside the key — widening its key strip
  from 56 to 132 px **only** when there is something to write, so a melodic
  session's roll is the roll it always was, to the pixel
  (`tests/keyboard.rs`). Theme format v4 adds `row_dead`, `key_dead` and
  `note_silent`, with the usual migration.

**An empty map means "not known", not "plays nothing".** A channel with no
instrument greys nothing; the two are very different statements.

#### The naming rule, and why it is three measured constants

This is the judgement call in the feature, and the first two attempts at it
were both wrong in ways only real files showed. The diagnostic was thrown away;
the numbers it produced are in the doc comments in `crates/fontelle-app/src/keymap.rs`
and are reproduced here because they are the whole argument.

Against the 617 presets in the bank on the development machine:

| rule | result |
| --- | --- |
| name a key when its own zone is ≤2 keys | F-Zero labelled **5 of 18** playable keys, Mega Man X **3 of 67** |
| ...and treat a patch with ≥2 such hits as a key map | 10 of 11 kits fully labelled; `Z3 Percussion` still 1 of 12 |
| ...with ≥1 hit, and no name spanning >32 keys | **all 11 kits label 100% of their playable keys** |

The three constants that fell out, each measured rather than chosen:

- `HIT_SPAN_KEYS = 2`. Melodic multisamples put their zones at 3–7 keys
  (`STR_Ensemble.sf2`: 3, 4, 5, 6, 7, 31, 37); kits put hits at 1. Three would
  start labelling string sections.
- `KEY_MAP_HITS = 1`. Of 606 melodic presets, **two** contain a one-key zone at
  all — and both contain five, so they are read as key maps either way. Of 11
  percussion presets all 11 contain at least one hit and only 10 contain two,
  so requiring two cost a real kit and bought nothing the corpus shows is
  needed. The safety is in `HIT_SPAN_KEYS` being narrow, not in counting.
- `MAX_NAMED_BAND_KEYS = 32`. Inside a key map the wide zones get named too —
  they are one percussion sample stretched across a band, which is why F-Zero
  went from 5 labels to 18 — but the widest band in any real percussion preset
  is 28 keys, while the only two melodic presets containing a hit carry zones
  of 48, 56 and 59. The cut falls cleanly between them.

Velocity is deliberately not consulted: a `vel_range` can make a key silent
softly and loud hard, and a keyboard greying itself in and out as the pen
pressure changes would be worse than one that tells the truth about the key.

**Where this would need revisiting:** the corpus is one person's retro-game
soundfont bank. A kit whose author gave every hit three keys would fall
straight through `HIT_SPAN_KEYS`, and the fix is that one constant with nothing
else changing. That is why it is a constant with the measurement written next
to it rather than a number inline.

## 2026-08-29 (earlier still): the window bends to the person using it

A third pass from the window, and the most useful kind of report — six
complaints, of which three were bugs with mechanisms and three were features
that existed and could not be found.

### The hung note, and why a note-off is not enough

> *"When placing a note sometimes it would play a different note on hold that
> wouldn't stop until I replayed again."*

`Sampler::note_off` releases **one** voice — the first active one matching the
key and the voice context. That is correct and deliberate; §11.4's per-clip
tagging depends on it. The window, though, only sent a note-off when the key
*changed*: sounding the same key twice sent two note-ons and, between them,
nothing. The second voice had no note-off coming and sustained until something
else in the engine happened to take it.

`crates/fontelle-app/tests/audition.rs` pins the engine half down — a doubled
note-on really does survive one note-off, through the real graph — so that the
reason the window is careful is written where somebody would find it if voice
allocation ever changes.

### The flicker, which was a number

> *"Clicking a note on the piano roll I'm still not hearing it cleanly, just a
> flicker."*

The minimum audition was a flat 180 ms for everything, so clicking a half note
sounded a fifth of a beat of it. The roll knew the length all along and was
throwing it away at the door: `take_audition` returned a bare `u8`. It now
returns an `Audition { key, ticks }`, `DocumentHost::seconds_per_tick` converts
through the song's own tempo map (defaulted to 120 BPM, so a host without one
still answers), and what you click sounds for as long as it *is* — floored at
180 ms so a thirty-second is still a note, capped at 2.5 s so a whole note is
not a drone.

Both of these now live in `crates/fontelle-ui/src/audition.rs` as a state
machine that **returns what to send** rather than sending it, which is the same
shape every canvas here has and the reason both bugs are regression tests
(`tests/audition_voice.rs`) instead of something to re-notice by ear. The
property worth holding, and the one the bug broke: *every note-on it emits is
matched by exactly one note-off.*

### The drag after drawing a note

> *"If I click to place a note, then my cursor moves to drag it, it shouldn't
> change the note length it should move the note."*

FL Studio's draw-and-size gesture — press on empty grid, the same drag sets the
length — was the default, and it is a real habit. It also has a sharp edge this
report is the sound of: a press is never perfectly still, so *every* drawn note
was a resize in progress and the smallest wobble of the hand changed a length
nobody meant to change. Placing a note now leaves you holding it: the drag
moves it in both axes, and the right edge resizes it like every other note.
`DrawDrag::Resize` keeps the old gesture for anyone who wants it, and
`tests/roll_interaction.rs` still tests it under that flag.

### The trap, a third time

`Timeline::drag` had the *identical* live-clamp bug the roll was fixed for two
sections ago — `earliest`, `highest` and the resize floor all recomputed from
the `clips` slice the window refreshes between events. Nobody had reported it
because nobody had dragged a clip to bar one yet. Limits are captured at the
press now, `drag`'s `_clips` parameter is deliberately unread and says so, and
`tests/arrange_gestures.rs` is the regression.

**Three occurrences is a pattern, so state it plainly:** a gesture emits deltas
relative to its own start, so any clamp it applies must be measured **once,
when the gesture starts**. The test shape that catches it applies the canvas's
own edits back into the document and asserts that a stationary pointer asks for
nothing new.

### Two features that existed and could not be found

> *"I'm still not seeing panning options, only the velocity in the piano roll."*

Pan was there. Every property `Note` carries was there — the lane has drawn any
of them since it was written and the chip cycled through them. The chip said
`vel`, which is a perfectly good *read-out* and a completely invisible control.
So the feature existed and did not exist, which is the worse of the two. There
is a menu now (`lane_menu_layout`, `tests/lane_menu.rs`), the chip carries a
caret so it reads as something you press, and `L` still cycles.

> *"The left section, I can't resize it. I want to be able to make the
> soundfonts bigger than the channels."*

The sidebar was always `metrics.sidebar_width` and the rack always took 42% of
it. Both are now `Docks` fields with `None` meaning "the old default", and both
seams are the margin that was *already* between the panels — so nothing moved
under anybody who was used to the old layout, and there is something to grab.
`tests/docks.rs` is mostly about the clamps: a seam that can be dragged until a
panel is a sliver is a seam that will be, once, by accident.

### Keys

> *"Ensure our keybinds are expansive."*

There were no arrow keys at all, which is fine for a phrase you are placing and
hopeless for one you are correcting. Now: arrows move the selection, `Ctrl` up
and down by an **octave** and left and right by a bar, `Shift` left and right
changes the length and `Shift` up and down the lane's value, `1`-`7` pick a
tool. The arrangement takes the same keys against its own selection. Every one
of them goes through the same clamped edit a drag does, so a chord with one note
near the top of the keyboard transposes as far as it can instead of refusing.

**Not done, and deliberately:** none of the panel sizes survive a restart.
`Settings` has the right shape for it (`serde`, a format version) and nothing
writes to it at runtime yet; the next person to touch this should add a
`workspace` section rather than another constant. The roll's toolbar is also
twelve chips wide and will clip on a narrow window — every chip has a keyboard
shortcut, but it wants a second row.

## 2026-08-29 (later still): note editing stops fighting you

A second pass from somebody using the window. One of these was a real bug with
a precise mechanism, and it is the most instructive thing in this file.

`cargo test --workspace`: 779 passing. Clippy `-D warnings` and `cargo fmt
--all --check` clean.

### The flicker: a clamp that chased the thing it was clamping

Reported as *"when placing notes while playing it jitters and glitches out and
changes the size of my notes — sometimes it just does that in general"*.

A drag emits deltas **relative to the previous step of the same drag**, because
that is what `MoveNotes` and `ResizeNotes` take and what lets a drag coalesce
into one undo entry. But both clamps — "a note cannot go before the start of
the clip" and "a note cannot be shortened past nothing" — were recomputed from
the **live document**, which the drag is itself changing:

1. Drag a note at bar 3 hard left. `earliest` is bar 3, so the clamp allows
   -3 bars, and the note lands on bar 1.
2. Next step, `earliest` is now bar 1, so the clamp allows **0**, and `wanted`
   becomes 0 against an `applied` of -3 bars — a delta of **+3 bars**. The note
   jumps back.
3. Next step it jumps forward again. For ever, at mouse-move rate.

The resize had the identical shape, which is the "changes the size of my notes"
half, and it was loudest right after drawing a note because the pending-add
handshake leaves a resize whose floor is the note that was *just* created.

A gesture's limits are now measured **once, when it starts** (`MoveLimits`, and
`Gesture::Resizing::shortest`). The move also clamps the key, which it never
did: `MoveNotes` is all-or-nothing, so one note of a chord that would pass key
127 stopped the whole shape from moving at all.

`tests/roll_gestures.rs` is the regression, and its shape is the point — it
**applies the roll's edits to the arena** the way the host does. A test that
reads the roll's output without ever applying it cannot see this bug, which is
exactly why the roll shipped with it. The invariant it pins down is worth
stating: *a pointer that is not moving cannot be asking for anything new.*

### The static: a click is thirty milliseconds

Reported as *"I can't click on notes in the piano roll to hear them, I just
hear a short flicker of static"*. Two things, and neither was the engine —
`fontelle-app/tests/audition.rs` now drives the *mouse* audition path through
the real graph and the real idle gate and holds a note for sixty-four blocks,
which is the coverage the live path had from MIDI and never had from the
window.

- **Clicking an existing note auditioned nothing at all.** Only a *drawn* note
  was sounded. The roll now hands the window a key to sound
  (`PianoRoll::take_audition`, the same shape as `Timeline::take_open`) when
  you draw a note, click one, or drag one to a new pitch — and deliberately not
  when you slide one along in time, which would machine-gun the same pitch.
- **An audition lasted exactly as long as the mouse button was down.** Thirty
  milliseconds of any sample is a click rather than a note; of a chiptune noise
  channel it is literally static. `transport::MIN_AUDITION` is a floor, not a
  length — holding a key still sounds it for as long as it is held.

### Onion skins

Asked for: *"seeing onion skins of the other notes that match up in the
timeline, so I can go back and forth between chord and melody on different
instruments, and change the filter of the skinning."* `GhostFilter` steps
off → all → one instrument at a time (the `skin` chip, or `G`), and
`StudioHost::ghost_notes` maps the other clips' notes into the **open clip's
own tick space** — a ghost that does not line up with what it is read against
is worse than no ghost. Drawn faint and outlined in the source lane's colour,
which `render_headless.rs` asserts in pixels: a ghost must never be mistakable
for a real note.

### The cursor

`pointer.rs` is a pure function from the geometry the window already has to a
cursor, so it is tested without a window (`tests/pointer.rs`). Notes and clips
offer a grab, their right-hand edges a horizontal resize, the seams a vertical
one, knobs a vertical one, empty grid the draw crosshair, rulers a grab, and
everything clickable a hand. **A drag in progress overrides whatever is under
the pointer** — dragging a note over the keyboard must not turn the cursor into
a hand half way through the gesture.

### Smaller things found on the way

- **A lost mouse-up left a drag armed.** An alt-tab mid-gesture meant the next
  pointer move continued the drag with the button already up — a note that
  follows the mouse around on its own. `WindowEvent::Focused(false)` now ends
  the gesture, releases the audition and breaks the history entry.
- The roll's toolbar is twelve chips wide now. It clips rather than overlaps on
  a narrow window, and every chip has a keyboard shortcut, but it is the first
  thing that will want a second row.

## 2026-08-29 (later): the studio becomes usable

A pass driven entirely by **somebody actually using the window** and writing
down what was wrong with it. Every item below is a reported complaint, and the
list is worth keeping because almost all of it was arithmetic that was wrong at
an edge rather than a feature that was missing.

`cargo test --workspace`: 744 passing. `cargo clippy --workspace --all-targets
-- -D warnings` and `cargo fmt --all --check` clean.

### The things that were broken

- **"Dragging notes near the start of the piano roll is glitchy and jittery."**
  One bug. Dragging a note towards bar 1 walks the pointer off the left of the
  grid and onto the keyboard, and dragging one upwards walks it onto the ruler;
  both are a few pixels away at any normal zoom. Converted unclamped, left of
  the grid gave a negative tick that `x_to_tick` flattened to zero and above the
  grid gave a negative row count that ran the key up to 127 — so a gesture that
  strayed teleported the note. `canvas::clamp_to_grid` is the fix and
  `canvas::edge_scroll` is its other half: a drag held past an edge scrolls the
  view towards it, so a note can be dragged to bar 1 from a screen away.

- **"The soundfonts section was glitchy — overlaying things squished
  together."** Also one bug with two faces. Both lists built one row *more* than
  fits and clipped that row's **rectangle** to the list, so the last row was a
  few pixels tall, its caption was centred inside those few pixels and drew on
  top of the row above it, and the same over-long rectangle reached across the
  boundary so a click at the top of the preset list landed on a soundfont. Rows
  are whole rows now, the lists are a whole number of rows tall so there is no
  dead band, hit-testing asks *which list* before *which row*, and
  `draw_text_clipped` clips vertically as well as horizontally, which it never
  did. `tests/panel_rows.rs`.

- **"It wasn't highlighting the selected instrument."** There was nothing to
  highlight with: `StudioHost` could say which *file* was open and not which
  preset was on the channel. `selected_preset` says, the browser draws it, and
  changing a channel's instrument now **renames the channel to the preset** —
  a channel that goes on saying what it used to be is the loudest "nothing
  happened" a rack can give somebody who just changed its sound. That is two
  document changes for one gesture, which is what `Compound` is for.

- **"The time bar is annoying to drag and I can never get it to the very
  start."** It could only be *tapped*: `CursorMoved` sent every drag to the
  piano roll and nothing else, so the transport ruler had no drag at all and
  getting the playhead to sample zero was a matter of hitting one pixel. `Drag`
  is now a value the press decides, and `sample_at` clamps, so a drag that
  leaves the ruler on the left lands exactly on zero.

### The FL Studio behaviours that were missing

- **The time marker.** The last place clicked on either ruler is the mark; play
  starts there, the space bar and the play button pause back to it, and the stop
  square goes to the front of the song **and takes the mark with it** — a stop
  that returns the playhead and then plays from bar 5 again is a stop nobody can
  use. `transport::TransportAction`, `tests/marker.rs`.

- **The last note is a stencil.** A drawn note is a copy of the last note drawn
  or clicked — length, velocity, pan, tuning, all of it. `RollEdit::Add` now
  carries a whole `Note` rather than four fields, which is what made this a
  one-line change at the point of use. `PianoRoll::template`.

- **Property lanes.** The lane under the grid shows velocity, pan, fine pitch,
  release or either mod value, cycled by a chip on the toolbar, and it can be
  **dragged taller** by the seam above it. `NoteProperty` and `SetNoteProperty`
  are the document half — five of the six properties were already on `Note` and
  none of them had a command, so none could be edited at all (INVARIANT 9 leaves
  no other way in).

### The two panels that did not exist

- **The arrangement** (`canvas/timeline.rs`, `tests/timeline.rs`). Clips as
  blocks on lanes across the top of the editor column, with a draggable divider:
  move, size, duplicate, mute, delete, marquee-select, and click one to open it
  in the roll. Virtualised like the roll (§16.4). `ResizeClip` was the one clip
  command the model was missing. Ctrl+T hides it.

- **The instrument editor** (`canvas/instrument.rs`, `fontelle-app`'s
  `instrument.rs`), reached from a switch on every channel row or the second tab
  in the editor column. **This is §7.2's whole point finally exercised**: the
  SF2 supplies defaults and the user owns every parameter afterwards, and until
  this existed an imported preset played exactly what the file said. Voice
  config, both filters, the amp envelope, per-layer gain and pan, interpolation
  quality, and the channel's own fader — every control addressed by
  `ParamAddress` (§8.2, INVARIANT 7), so the same table will serve automation,
  MIDI learn and the plugin export rather than growing a second scheme.
  `SetChannelPatch` now coalesces with itself, so turning a knob is one undo
  rather than one per pixel.

### Notes for whoever is next

- `Session::channel_presets` and `Session::patch_cache` are **live-session
  state, not document state**. A `PatchData` records the audio a patch points at
  (INVARIANT 8), not the browser row it was picked from, so a project reopened
  from disk shows no preset highlight until one is chosen again. The channel's
  *name* is the half of that answer which does survive, which is why it is set
  from the preset.
- The instrument editor rebuilds the whole `CompiledGraph` on every step of a
  knob drag. That is correct — the graph carries the instrument, and you have to
  hear what you are turning — and it is the obvious thing to make incremental if
  it ever shows up in a profile.
- Still outstanding from item 9: the mixer strip, and record-arm wiring Phase 1
  item 5 into the UI with a metronome. The instrument editor took the place the
  mixer would have had, and per-channel gain and pan are on it in the meantime.

## 2026-08-29: the studio opens itself

Phase 2 item 9, most of the piano roll's second pass, and the engine piece both
of them needed. **This is the first build that does not need the command line.**

```sh
cargo run --release -p fontelle-app
```

A window with a channel rack, a soundfont browser and a piano roll. Pick a
soundfont from the browser, pick a preset, and it goes on a channel — while the
audio device stays open and the transport keeps rolling. Draw, and you hear it.

### Item 9, and the engine change under it

`AudioDevice::start_output_stream` took a `CompiledGraph` **by value** and the
callback kept it for the stream's life. The graph is where the instruments
live, so choosing a soundfont meant tearing the device down and opening it
again — which is why item 9 was the last thing standing between this and
self-sufficiency, and why it is an engine change and not a UI one.

`fontelle_engine::graph_channel` is the other half of `timeline_channel`, and
deliberately not the same shape:

- **Not a `triple_buffer`.** That needs `T: Clone`, and a `CompiledGraph` is a
  bag of `Box<dyn AudioNode>` holding `Patch`es and `Arc<SampleStore>`s. There
  is no meaningful clone of one.
- **An SPSC queue forward and a return queue back.** Swapping the graph is
  trivial; *not freeing the old one on the audio thread* is the whole problem,
  and it is INVARIANT 1 exactly — dropping a `CompiledGraph` frees every node,
  every patch and every buffer in it. So the RT side moves the graph it stopped
  using into a return queue and `GraphPublisher::reclaim` frees it on the
  thread that built the replacement.
- **The room check comes first.** `GraphSource::take_update` will not take a
  new graph unless there is somewhere to put the old one. With the return queue
  full the honest answer is to keep playing what we have; the publisher empties
  it on its next visit and the swap goes through then. That state turns out to
  be unreachable through the public API — the queues are the same size and
  every `publish` drains the return queue before it fills the forward one — so
  it is tested as a unit test inside the module, where the queue can be filled
  by hand. `tests/graph_channel.rs` has the reachable half.

This also retires the documented `ManuallyDrop` leak for the graph. The leak
was acceptable when the process exited seconds after `stop()`; a DAW that runs
for hours and changes its instruments cannot leak one graph per change.

### The soundfont bank (TDD §17.5, §17.3)

`~/.local/share/fontelle/soundfonts` — created once, on first run, and said out
loud. Drop `.sf2` files in it and they are in the browser next launch;
`--soundfonts <dir>` adds another folder and is remembered in
`~/.config/fontelle/settings.json`, so it only has to be said once.

**INVARIANT 10 is decided here and the decision is a judgement, not a
derivation.** The invariant says Fontelle writes nothing outside locations the
user configured *except its own config directory*, and the default bank folder
is under the XDG **data** directory rather than the config one. The reading
taken: the data directory is as much Fontelle's own as the config directory is,
and "a folder you drop your soundfonts into" cannot exist unless something
creates it. Nothing defaults to `~/Documents`, `~/Music` or anywhere else that
belongs to the user. **Overrule this if it is the wrong reading — it is one
function and one `create_dir_all`.** `settings.rs` says the same thing at the
top of the file.

The search is §17.5's fuzzy one: a case-insensitive *subsequence*, so "gus"
finds `GeneralUser GS`, scored so runs of consecutive letters and matches at a
word boundary win. It runs over file names and over the preset names inside the
**open** file. Across every preset in the collection is the same function over
a cached index, and the index is not built yet.

### Two real bugs the tests found by causing them

- **The test suite wrote into the developer's real `~/.config/fontelle`.**
  `Session::open_bank` saves the folders it settled on, the studio tests called
  it, and a `/tmp` path ended up in a real config file. `Session` now takes a
  settings path and the tests give it a scratch one.
- **Concurrent saves corrupted that file.** `save_to` wrote a fixed
  `settings.json.tmp` and renamed it; eight test threads doing that at once
  produced a document with an extra brace on the end and, sometimes, no file at
  all (the second rename found it already moved). The temp name carries the pid
  and a counter now, and `tests/bank.rs` reproduces the old behaviour with
  eight threads and twenty rounds each.

### The piano roll, second pass

Two things made it feel wrong to use, and both are fixed:

**You could not draw a note to length.** In FL Studio — and in every roll built
after it — a click on empty grid makes a note and the *same drag* sizes it.
Here the click made a note and the drag did nothing, because the roll does not
own the document and so did not know the id of the note it had just asked for.
The fix is a handshake: the press leaves a pending add, `DocumentHost::edit`
now returns the ids the command minted (read off `History::last_applied`, which
is new and exists for this), and `PianoRoll::note_added` turns the gesture into
a resize of that note.

**Zoom was one axis, about the left edge.** §16.4 asks for continuous,
independently controllable per axis. `zoom_x` and `zoom_y` now zoom about the
pointer, in `f64`, so what was under it stays under it to within less than a
pixel at any zoom — asserted in pixels rather than in ticks, because ticks is a
weaker promise at low zoom and an impossible one at high (`scroll_tick` is a
whole tick). Ctrl+wheel zooms time, Ctrl+Shift+wheel (or Alt+wheel) zooms
pitch, and there are buttons for both on the new toolbar.

Also landed, all of it from §16.5's list: marquee select (the Select tool, or
Ctrl+drag in any tool), Ctrl+C/X/V and Ctrl+B duplicate, Alt for free
positioning, Shift to constrain a drag to one axis, a Paint tool that draws
across every cell it crosses without stacking notes, a velocity lane with its
own `SetNoteVelocity` command (per-note inverse, and it merges so one drag is
one undo), bar numbers in the ruler, key names on the keyboard, note audition
on the live path so a drawn note sounds whether or not the transport is
rolling, and a **visible toolbar** carrying the tools, the snap division and
the zooms — the answer to not being able to see what the roll can do.

### The bank folder is reachable from inside the window

Reported straight after the first build of the above, and both halves were fair:
the status line said "no .sf2 files yet — put them in /home/" and ran off the
end of a 248-pixel panel at exactly the word that mattered, and there was no way
to say where the soundfonts should be without knowing `--soundfonts` exists.

The browser now has a footer that cannot scroll away: the bank folder, elided
from the left (`…/fontelle/soundfonts` — the end of a path is the half that says
where you are), over **Open folder** and **Change…**. Open folder shows it in
the desktop's file manager and *creates it first if it is not there*, which is
the whole point of pressing it. Change picks a different one and remembers it;
`Ctrl`+click adds one alongside instead of replacing. The count moved into the
panel heading (`Soundfonts — 5`), which is a line saved and a question answered.

**No file-dialog crate.** `rfd` is the obvious answer and its default Linux
backend is GTK, which is LGPL and banned outright by `deny.toml` (TDD §3.4); its
portal backend needs an async runtime this workspace does not otherwise have,
for two dialogs. `desktop.rs` runs the desktop's own picker as a subprocess —
kdialog, then zenity, then AppleScript or PowerShell off Linux — which links
nothing and degrades to a message naming `--soundfonts` on a machine with none.
The command shapes, the answer parsing and the *subprocess plumbing* are all
tested; the last of those is driven with `/bin/echo` and `/bin/false` rather
than by clicking a dialog, so a cancel, an answer and a not-installed
fall-through are all checked. **What is not verified is a human clicking through
a real picker** — that one needs Ty.

### Three defects found by looking at the window, not by thinking about it

Exactly what §2.5 of the plan predicts. All three are arithmetic now, in
`tests/chrome_text.rs`:

- The ruler numbered no bars at all. The stride was right; it was applied as
  `number % stride == 1`, which for the common stride of one is `0 == 1`.
- Soundfont names were drawn straight through their file sizes, because a name
  was clipped to the whole row rather than to the column beside the size.
- The keyboard was never labelled, at any zoom anyone would use: a line box is
  taller than the ink in it, and the guard compared the two exactly. The
  default row height went from 12 to 16 at the same time, which also makes a
  note an easier target.

### Where the process rule was followed, and the one place it was not

Tests first, confirmed failing, for everything with a pure core:
`graph_channel`, the bank and the settings file, the window and panel layouts,
every new roll gesture, `SetNoteVelocity`, `History::last_applied`, and all
three of the defects found by looking. Each of those was a compile error or a
red assertion before it was an implementation.

**The exception, stated plainly:** `Session`'s `StudioHost` implementation —
the channel rack operations, the browser wiring, the audition path — was
written before `tests/studio.rs` was, and those fourteen tests passed on their
first run. That is the failure mode the rule exists to catch, so they are worth
reading with more suspicion than the rest. Two of them were then written
red-first against real gaps and did fail: choosing an instrument bypassed
`History` entirely (the one edit in the app Ctrl+Z could not reach), and the
roll's ruler converted ticks to samples by multiplying by a hard-coded 120 bpm
instead of asking the document's own `TempoMap` (INVARIANT 5). Both fixed.

### Verified

- **658 tests**, `cargo clippy --all-targets -- -D warnings` clean, `cargo fmt`
  clean.
- **Both reference bounces byte-identical** against a build of the previous
  commit, on real soundfonts: the demo phrase, and a 64 MB render of a
  multi-part MIDI arrangement. The audio path is provably untouched.
- **Seen, on this machine**, through a nested Xwayland: the window opens with
  no arguments, finds five real soundfonts in a folder with their sizes, draws
  the rack, the browser, the toolbar, the ruler's bar numbers, the keyboard's
  key names and the velocity lane with the demo's crescendo in it.
- **§16.3 still holds:** 2 frames drawn over a 30-second unattended run
  (`--run-for 30`). Idle CPU is 2.2% of one core — and the same 2.2% for a
  build of the previous commit doing the same thing, so it is the open audio
  device, not the window. The earlier "0.05%" figure was a window with **no**
  audio device behind it and is not comparable.

### What is deliberately not built

- **Loading a soundfont blocks the window.** `import_sf2` decodes every sample
  in the preset on the calling thread, so a 300 MB soundfont freezes the UI
  while it loads. The fix is a worker thread and a progress line; the shape is
  ready for it because the graph already arrives through a channel.
- **A graph swap cuts every sounding voice.** The new graph's samplers are new,
  so notes ringing through an instrument change stop. FL does something
  similar; a crossfade is not worth it yet.
- **Adding an instrument is three undo entries** — the channel, its clip, its
  patch — because there is no compound command. Each one undoes correctly.
- No timeline/arrangement panel, no mixer panel, no record button in the
  window, no interactive scrollbars, no ghost notes, no note property lanes
  beyond velocity, no chord/scale/quantise helpers. §17.5's background scan,
  on-disk index and cross-file preset search are not built.
- Editing away a note that is currently sounding still leaves it ringing until
  its next note-off.

## 2026-08-29: a piano roll you can write in

Phase 2 item 8, most of item 6's remaining colour work, and the engine piece
both of them needed. This is the first build that is a **tool** rather than a
demonstration:

```sh
cargo run --release -p fontelle-app -- --play-sf2 <file.sf2> --window --blank
```

Eight empty bars over a real instrument. Left mouse draws, right deletes, drag
moves, drag the right edge resizes, Space plays, Ctrl+Z/Y, Ctrl+S saves, B
cycles snap, P/E/D pick draw/select/delete. What you draw you hear, while it
plays.

### The palette is the owner's, not mine

Three ramps — a teal primary, a green and a blue secondary — mapped onto the
token set. The dark end of the primary carries the structure and is kept
near-neutral on purpose: a fully saturated teal UI reads as a skin, and the
brief was FL Studio's newer look, where surfaces are neutral and colour is
reserved for things that mean something. So the accent is the top of the
primary ramp, the playhead is the green (it never competes with the chrome it
travels over), selection and notes are the blue.

**One colour is not from the palette, deliberately.** Nothing in teal, green
and blue can say "too loud", so `meter_peak` is a red. Clipping is a signal,
not a brand; a meter that cannot say it is not a meter. Easy to overrule — it
is one token in a file.

Theme format went to **v2** for five new tokens (`note`, `note_selected`,
`key_white`, `key_black`, `row_accidental`). The migration chain now has two
arms and a test that drives a v0 document through both of them, plus one
checking the half that is easy to get wrong: a migration fills in what is
*missing* and never overwrites what the old file actually said.

### Edit while playing: the timeline is a channel now

`AudioDevice::start_output_stream` took a `CompiledTimeline` by value and the
callback kept it forever. It now takes a `TimelineSource` — the RT end of a
`triple_buffer` (TDD §11.3) — and every INVARIANT 1 property is a property of
the channel:

- **No lock.** The RT thread swaps an index; it never waits for the writer.
- **No deallocation on the RT thread.** `TimelineSource` hands out
  `&CompiledTimeline` and nothing else, so the callback can never come to own
  one and drop it. The old events are freed inside `publish`, on the thread
  that published over them.
- **No backlog.** Dragging a note publishes one of these per mouse-move. The
  RT thread takes the newest; the rest are simply overwritten.

The bug worth naming: the reader's event cursor is an *index into a `Vec` that
no longer exists*. Carried across a swap it either replays notes already played
or skips ones that have not been. `TransportReader::retarget` repositions it,
and the device calls it only when `has_update` says something actually changed
— repositioning every block is the per-block work a cursor exists to avoid.

### The roll is a view, and the type system says so

`fontelle-ui` now depends on `fontelle-model` — read-only — and on
`fontelle-engine` not at all. The roll reads notes and emits `RollEdit` values;
`fontelle-app`'s new `Session` turns each into a `Command`, puts it through
`History`, recompiles and publishes. **There is no `&mut Project` reachable
from the UI at all**, so INVARIANT 2 and INVARIANT 9 hold because there is
nothing to hold them wrong with.

`crates/fontelle-app/tests/session.rs` walks that whole chain with no window
and no sound card: draw a note, and it is in the document *and* at the RT
thread's end of the timeline channel; undo, and it is off both.

### What §16.4 asked for, and what it did not get yet

`visible_ticks` and `visible_keys` bound every loop in the renderer, so the
geometry built per frame depends on the viewport and the zoom and on nothing
else — the §16.4 promise, and a test says so directly. The note *scan* is still
linear in the clip's note count: filtering, not indexing. Honest for the sizes
this opens today, and the first thing to change when it is not.

The four layers (row shading, grid, notes, playhead) share a frame rather than
having independent invalidation. `WidgetTree`'s bounds are where that split
goes when the roll is big enough for it to pay.

### Two things found by looking at it

- **The keyboard was black keys on a black ground.** It painted the strip dark
  and the naturals light, which gives a ladder of pale bars with gaps — neither
  a keyboard nor a countable octave. Now the naturals run full width and the
  accidentals sit short and dark on top of them, the way a keyboard looks, with
  the accent down every C. That marker is the roll's only orientation until the
  ruler learns to write bar numbers.
- **The headless render tests ran out of GPU memory** at the sixth one. Each
  built its own `Headless` — a wgpu device plus vello's pipelines — and the
  harness runs them in parallel. One shared device behind a `Mutex`, and the
  file also got 8x faster.

### Verified

- 511 tests before this, **569 now**; clippy and fmt clean.
- Both reference bounces byte-identical. The audio path was not touched; the
  timeline simply arrives by a different route.
- On hardware: the demo phrase renders in the roll exactly where the document
  puts it, and `--blank` opens eight empty bars titled "Untitled".
- Pixels asserted, not eyeballed: a note is drawn where the document puts it,
  an empty row is not, a selected note differs from an unselected one, every C
  is marked, and accidental rows are shaded differently from natural ones.

### What is deliberately not built

- **`--window` still needs `--play-sf2`.** There is no file picker, so the
  window cannot yet open a soundfont by itself. That is item 9, and it is what
  makes `fontelle` with no arguments self-sufficient.
- **No marquee select, and no Ctrl+B/C/V/X.** Item 8's list includes them; what
  landed is draw, delete, select-by-click, Ctrl+A, move, resize, snap,
  right-click delete and Ctrl+Z/Y. The clipboard is a `Command` away.
- **No bar numbers in the roll's ruler**, because that needs text shaped by the
  caller and the plumbing for it is item 9's anyway.
- **A timeline swap does not reset sequenced voices.** Editing away a note that
  is *currently sounding* leaves it ringing until its next note-off. Resetting
  instead would cut every voice on every keystroke, which is worse; the real
  answer is to compare what was removed against what is sounding.
- **Adding a channel while the stream runs.** The graph is realised once at
  startup and lives in the callback; only the *timeline* is republishable. A
  graph channel is item 9's, and it is the same `triple_buffer` shape.
- **Every keybind is hard-coded.** §16.5 says all of them are remappable.

## 2026-08-29 (second): the transport bar, over the real engine

Phase 2 item 7. The window and the audio thread now coexist, which is the
whole point of doing this before the piano roll: the plan puts it here to
prove the threading shape on the simplest feature there is.

```sh
cargo run --release -p fontelle-app -- --play-sf2 <file.sf2> --window
cargo run --release -p fontelle-app -- --open <project.fontelle> --window
```

Play, stop, loop, a playhead you can click to seek, a bars/beats and clock
read-out, and the master meters — all driven by the atomics the RT side was
already publishing. Nothing in `fontelle-engine` changed.

### Commands down, atomics up — as one trait

TDD §2.2's shape is `fontelle_ui::TransportHost`, and both halves are literal:

- **Down.** A click becomes exactly one call, which becomes one relaxed store.
  `apply()` is the only place a hit turns into a write, so it is a function
  with tests rather than a habit spread over an event handler. It reads the
  view first, which is what makes the loop button a *toggle* — asking for the
  opposite of what it last saw — rather than one that only ever turns looping
  on.
- **Up.** `TransportView` is a snapshot taken once per frame. Once, because
  `MasterMeter::take_peaks` *resets* what it reads: asking twice in a frame
  would hand half the picture to one caller and half to the other.

The trait is also what keeps `fontelle-ui` off `fontelle-engine` entirely.
`fontelle-app` is the layer allowed to see both (`src/window.rs`), so the UI
crate's own tests run against a fake that records the commands, and
`crates/fontelle-app/tests/window_host.rs` checks the real implementation
against a real `Transport` and `MasterMeter` **with no audio device** — which
is possible precisely because the interface is nothing but atomics.

### The bug that only running it could find: a window asleep at the wheel

The window was drawing correctly and following nothing. Playback rolled, the
audio came out of the speakers, and the playhead sat at zero.

`ControlFlow::Wait` blocks until the OS has something to say, and the engine
starting is not something the OS says. The window had no way to *learn* that
anything had happened. In normal use it is masked — the click that starts
playback is itself the event that wakes the loop — which is exactly what makes
it the kind of bug that ships.

The fix is a value, `widget::sleep_budget`, and it has three answers:

| animating | an engine to watch | sleep |
|---|---|---|
| yes | either | one frame (16.7 ms) |
| no | yes | 100 ms |
| no | no | forever |

**A poll is not a frame.** The loop wakes, reads six atomics, finds nothing
changed, marks nothing dirty and goes back to sleep having drawn nothing.
§16.3's promise is about frames issued, and this issues none. Measured in
release: the window thread costs **0.05% of one core** whether or not it is
watching an engine — the same as item 6's genuinely-idle window.

This will matter beyond the defensive case as soon as live audition lights the
meters while the transport is stopped (item 9's record-arm).

### A finding that is not ours, and is worth writing down

While measuring the above, per-thread: with the transport **stopped** and an
output stream open, `cpal_alsa_out` costs ~1.0% of a core and ALSA/PipeWire's
own helper thread another ~0.95%, for ~2% total against §19's "idle CPU,
transport stopped: < 0.5%". §6.3 says a stopped callback "fills silence and
returns immediately, which is what delivers the near-zero idle-CPU target" —
and the callback does exactly that, so the cost is the device period rate
itself, not our work inside it. Out of scope here; added to the next-steps
list, because §19 is measured on the app and not on the window.

### Meter ballistics are a state machine, so they are tested like one

Instant attack, 20 dB/s release, 1.5 s peak hold — PPM-ish, and a state
machine rather than a formula because what it reads depends on what it read
last. The floor is -60 dBFS: the bar is small, and spending a third of it on
levels nobody can hear compresses the range that matters.

Fed from `take_peaks`, which is itself a highest-since-last-read, so nothing
between two frames is missed even when the frames are far apart.

Verified against the real thing rather than by eye: the demo bounces at peak
0.966 with the limiter taking 1.7 dB, so a meter pinned near full and red
during the demo is correct, and one that empties 1.8 s after the last note is
the 20 dB/s release doing its job. Both were read off the actual pixels.

### The theme format has a migration now, and it is exercised

The transport bar needed a `transport_bar_height` token, so the theme format
went to **v1**. Rather than reaching for `serde(default)` — which would give
up `deny_unknown_fields` everywhere to solve one problem in one place — v0
files go through a real `migrate` arm that inserts the token. A test loads a
v0 document with the field removed and checks both that the default lands and
that everything the old file *did* say survives.

The arm also stamps the current version onto the migrated document. That was
not in the first draft, and the test caught it: a migrated theme that still
claims v0 disagrees with what it now contains.

### Verified

- 473 tests before this, **511 now**; clippy and fmt clean.
- Both reference bounces byte-identical (the demo still renders 108 000 frames
  at peak 0.966). Nothing in the audio path was touched and the tests say so.
- On real hardware, in a nested X server, against `F-Zero.sf2`: the window
  opens stopped and cued at zero; playback moves the playhead, advances the
  read-out through `1.4.333 / 0:01.673` and lights both meters; the play glyph
  is the accent colour while rolling and the text colour when not — read out of
  the framebuffer as `srgb(79,143,208)` and `srgb(230,230,235)` rather than
  eyeballed; the meters empty after the song ends.
- Clicking works, and I have the accident to prove it: a stray pointer event in
  the nested X server landed on the stop button — hover highlight drawn, and the
  transport stopped exactly where the probe said it did.

### What is deliberately not built

- **No record button.** `--window --record` is refused with a message saying
  so. Turning a take into a clip has to happen before the process exits, and
  the window has no "and then what" yet. Item 9.
- **The transport does not stop at the end of the song.** It runs on with the
  playhead clamped, which is what the engine has always done; making the end a
  stop is a transport-behaviour decision, not a bar-drawing one.
- **The ruler has no ticks, bars, or loop-drag.** Clicking it seeks and the
  loop range is shaded when looping is on; setting that range from the bar is
  item 8's snap arithmetic, reused.
- **No keyboard.** Space does not play. The keymap is item 8 (§16.5), and one
  binding hard-coded here is one binding to find and move later.
- **Time signature is assumed 4/4** for the bars read-out — there is nowhere in
  the document to put one yet.

## 2026-08-29 (third): the window opens

Phase 2 item 6 of `docs/first-usable-plan.md`, and the first pixel this project
has ever drawn. `fontelle` with no arguments was a `todo!()`; it is now a
window.

```sh
cargo run -p fontelle-app                    # the dark default
cargo run -p fontelle-app -- --light
cargo run -p fontelle-app -- --theme mine.json
cargo run -p fontelle-app -- --run-for 20    # closes itself, reports frames drawn
```

The §16.2 stack is real and works: `winit` -> `wgpu` surface -> `vello` scene,
with `cosmic-text` shaping the chrome. **The `lyon` fallback was not needed and
the timebox was not spent** — vello came up on the first serious attempt, which
is worth recording because §16.2 calls this the highest-risk component in the
project and budgeted for it going the other way.

### The GUI answer to the test-first rule

§2.5 of the plan is the rule this crate is shaped by: *everything that can be a
pure function is one, and the tests live there.* So of the six modules, five
have no window in them and are tested like any other code —

| module | what it decides | tested by |
|---|---|---|
| `theme` | every colour, metric and font token; the file format | `tests/theme.rs` |
| `layout` | where the window's rectangles are | `tests/layout.rs` |
| `widget` | **whether a frame happens at all** | `tests/invalidation.rs` |
| `text` | where each glyph goes | `tests/text_layout.rs` |
| `render` | the scene, as a pure function of the three above | `tests/render_headless.rs` |
| `app` | the event loop, and nothing else | run it |

— and `app.rs` is left holding only the part that genuinely needs a window.
331 tests before this stretch, 431 after Phase 1, **473 now**.

### The pixels are checked by a machine too

`render::Headless` renders a scene through the real vello pipeline into memory
with no surface attached. It is to the window exactly what `render_offline` is
to the audio path — the same code, driven without hardware, so the output can
be asserted on rather than looked at. `tests/render_headless.rs` checks that the
window corner is the theme's window colour, that the panel body and header are
where `window_layout` put them, that the title actually has ink inside the
header band (which catches text drawn at the wrong baseline, i.e. off the top of
the window), and that the same scene renders identically twice.

The load-bearing one is `a_different_theme_produces_different_pixels`. Every
other assertion in that file would still pass if `draw_window` had the dark
palette hard-coded in it — the trap this project keeps finding, most recently
the pan test that passed with the feature absent.

Setting `FONTELLE_UI_DUMP=<dir>` writes each frame out as a PNG. §2.5 makes
"the pixels have been seen once by a human" half the done-criterion for every
GUI item, and on a machine whose compositor an X11 screen-grabber cannot see,
that is otherwise impossible to satisfy.

### Zero frames when idle, built in and measured

§16.3 requires that a stopped, unanimated window issue *no frames at all*, and
the plan is explicit that this is far easier to build in than to retrofit. Three
things make it true, and none of them is a heuristic:

1. The event loop runs on `ControlFlow::Wait`. With nothing to do the thread
   blocks in the compositor — not a timer, not a poll.
2. `Redraw::take_dirty` returns `Option<Rect>`, and `None` *is* "issue no
   frame". Nothing calls `request_redraw` except an actual change.
3. Dirtiness and animation are separate questions. An animator (item 7's
   playhead) keeps the loop awake but claims no pixels, so whatever moves still
   has to say *where* — which is what makes §16.4's "the playhead moving must
   not redirty the note geometry" true by construction rather than by care.

`WindowApp::frames_drawn` counts what reached the GPU, so the claim is
checkable. Measured on this machine, `--run-for 35` with nobody touching it:
**1 frame drawn**, and **1 tick of CPU (10 ms) over a 20-second idle window —
0.05% of one core**, against §19's target of under 0.5%.

`Redraw` also coalesces: three widgets changing between two vsyncs is one frame
over the union of their rectangles, not three. And `WidgetTree::set_bounds`
dirties both the rectangle a widget left and the one it arrived at, because
redrawing only the new one is the dirty-region bug everybody writes once.

### The theme is a file, and it was written against contrast ratios

§16.6 asked for a token set, a dark default, a light variant, and a documented
file format. All four exist. Colours serialise as `#rrggbb` (or `#rrggbbaa`
when they are not opaque) rather than as `[14, 14, 17, 255]`, because the whole
reason "user themes are just files" works is that a person edits them.

`format_version` is read before the body, so a theme from a newer build is
refused *by version* rather than by whichever field happened to change shape
first — the same rule, and the same `migrate` shape, as the project document.

The palette was chosen against WCAG rather than by eye, and the test enforces
it: text on its own panel is at least 4.5:1 in both variants and muted text at
least 3:1. A DAW's chrome is small, dense, and looked at for hours.

The token list covers the panels §16.1 names, not only the one panel item 6
draws. Adding a token later is a format revision; the timeline, piano roll and
mixer already know which colours they will ask for.

### A real bug the first run found, and a dependency that was wrong

Neither was visible from the tests, and both are the reason the plan makes
running it part of the item.

- **The surface view format.** Vello configures its surface with an empty
  `view_formats` and builds its blitter for the plain format, so asking the
  swapchain texture for an sRGB view of itself is a validation error, not a
  colour space. It aborted on the first frame. Fixed by taking the surface's
  own format.
- **Two entire GPU stacks.** `fontelle-ui` declared `wgpu = "30.0.1"`
  alongside `vello = "0.10.0"`, and vello 0.10 builds against wgpu **29**. Both
  compiled, and every type crossing between them was a different type with the
  same name. The direct dependency is gone; vello owns the wgpu version and the
  crate uses `vello::wgpu`. The workspace now resolves exactly one wgpu, which
  also clears a `cargo-deny` `multiple-versions` warning nobody had looked at.

### What is deliberately not built

- **One panel, and it is empty.** That is the item: a window, a surface, a
  themed panel. The transport bar is item 7, the piano roll item 8, the docked
  panel set item 9. There is no widget in the tree yet beyond the panel itself.
- **Dirty regions are computed and not yet used to clip.** `take_dirty` returns
  the union and the frame redraws the whole scene inside it. With one static
  panel that is the same picture either way; the region is already threaded
  through so the piano roll can clip to it when there is geometry worth
  skipping.
- **No `baseview` backend work.** `BaseviewBackend::request_redraw` is still a
  `todo!()`. It is M2's, and M2 is deferred until after this gate per §2.2 of
  the plan.
- **No input.** No mouse, no keyboard, no keymap. The window closes and
  resizes; that is all it responds to. Interaction arrives with the things to
  interact with.
- **No settings, no layout persistence.** Item 10.

## 2026-08-29 (fourth): play it, keep it — MIDI recording

Phase 1 item 5, and the one feature in this stretch that the TDD did not
contain at all. `Transport` has had a `Recording` state since it was written
and §15.4 covers *audio* recording, but nothing said how live MIDI becomes a
note clip — which for a soundfont instrument aimed at players is the whole
capture loop. It is now TDD §14.7.

```sh
cargo run -p fontelle-app -- --play-sf2 <file.sf2> --midi-in --record \
    --record-seconds 9 --save Take.fontelle
cargo run -p fontelle-app -- --open Take.fontelle --render-wav take.wav
```

### A take is a copy of the stream that made the sound

That is the design, and everything else follows from it. While the transport
is `Recording`, every live event the audio thread drains is mirrored into a
preallocated ring **on its way to the graph** — not read a second time from the
device, and not a parallel path that has to be kept in step. So what was
written down and what was heard cannot drift apart, and sustain, velocity
curves, stuck-note release on disconnect and the zero-velocity-note-on rule are
all already applied by the router upstream of it. There is nothing to
reimplement.

The mirroring lives in `LiveEventSource::drain`, not in the audio callback,
for the reason this project keeps landing on: code inside a cpal closure can
only be run by a sound card, so anything put there is testable only by ear.

Two things it has to get right:

- **A full ring drops and counts.** Same rule as the input side — an audio
  thread must never wait for a model thread. But a take with holes in it is
  something the user has to be *told* about, so the drop count is published and
  the CLI prints it. A recording that quietly lost notes is worse than one that
  failed.
- **A `ParamValue` event is left out rather than cloned.** It carries an owned
  `ParamAddress`, and cloning a `String` on the audio thread is an allocation —
  INVARIANT 1, and the guard says so. Nothing live produces one today (CC
  routing beyond sustain is deliberately not built), and when it does the
  address wants to be a `Copy` handle, which §8.2's stable-id table gives it
  anyway. `no_allocation_during_render` now arms a capture and drains with
  `recording` true, so the guard covers this path.

### What a recording *is*, decided in one pure function

`notes_from_capture` is on the model thread and takes no I/O, which is what
makes every rule below a test rather than a listening session:

- Samples to ticks through the `TempoMap` (INVARIANT 5), rounded to nearest.
  Truncating would drag every note in every take systematically early.
- **A key retriggered before its note-off ends the first note.** Two
  overlapping notes on one key is a shape the piano roll cannot draw and the
  sampler cannot voice sensibly.
- **A key still held when recording stops ends there.** Finishing on a held
  chord is a normal way to stop playing; hanging forever or dropping it are
  both worse.
- A note-off with no note-on in front of it is ignored — recording started with
  a key already down.
- **A note shorter than one tick gets one tick.** A zero-length note is a
  note-on and a note-off on the same sample: the sequencer emits both and the
  sampler sounds nothing between them, so the fastest possible stab would
  vanish from the take.
- **No quantisation beyond that rounding.** Quantise is a piano-roll command
  (§16.5) over a selection the user can see. Doing it at capture throws the
  performance away before anyone has looked at it, and leaves nothing to undo
  back to.

The take becomes a clip through one `AddClip` command, on a new lane, so it is
undoable the moment there is a UI to undo it from and nothing existing has to
be deleted to make room for it.

### Verified on real hardware

Through this machine's ALSA "Midi Through" port, with `aplaymidi` playing four
notes into it while Fontelle recorded:

```
  recording 9.0s — play now.
  + Midi Through:Midi Through Port-0 14:0
  stopped at 9.00s
  recorded 4 note(s)
  saved Take.fontelle with the take
```

The saved clip holds exactly what was sent — keys 60, 64, 67, 72 at 966 / 962 /
962 / 1915 ticks, against the 960 and 1920 the file specified — and reopening
the project renders the demo phrase followed by the take: peak 0.966 over the
first 2.5 s, then 0.425 from 3 s on, where the arrangement is silent and only
the recording is playing.

Mutations, each caught by exactly one test: a retrigger not ending the previous
note; a zero-length note kept at zero; a note still held at the stop dropped;
samples converted by arithmetic instead of through the tempo map.

One bug the hardware run found that no test would have: **stopping the
transport when the song ends freezes the playhead the record deadline is
watching**, so a take longer than the arrangement never ended at all. Recording
past the end of the arrangement is not an edge case — it is what recording onto
empty bars *is* — so the song-finished stop is skipped while a take is running.

### What is deliberately not built

- **No count-in and no metronome.** Both are Phase 2, with the transport bar.
- **No loop recording, take lanes, or punch in/out.**
- **Overdub is the only mode**: a take is a new clip on a new lane, so nothing
  has to be deleted to make room for one. Replace is a choice that needs a UI
  to offer it.
- **One record-armed channel.** Splitting a take across several means splitting
  by `TimedEvent::target`, which needs the arm UI first.
- **`--record` needs a deadline, not Ctrl-C.** Turning a take into a clip and
  writing the project both have to happen before the process exits, and a
  signal handler cannot do that without a dependency. `--record-seconds`, or
  the end of the song.

**Where things stand:** 431 tests, clippy and fmt clean, plus the hardware run
above.

## 2026-08-29 (later still): a project you can save and open again

Phase 1 item 4. `MyTrack.fontelle/` is a folder bundle with `project.json` and
the five directories §17.1 names, written atomically, and the CLI can now save
one and reopen it.

```sh
cargo run -p fontelle-app -- --play-sf2 <file.sf2> --save MyTrack.fontelle
cargo run -p fontelle-app -- --open MyTrack.fontelle [--render-wav out.wav]
```

### Reopening is by *sample*, not by preset

This is the decision the rest of it hangs on. A saved patch names its audio by
file plus the index of the sample header inside it, so opening a project reads
exactly those headers out of exactly those files, through the same decode the
importer uses.

Re-importing the preset the patch originally came from would look equivalent
and is not: a patch the user has edited to reach a second preset's sample would
not survive it — and that editing is the entire product thesis. `load_sf2_
samples(path, &[header indices], store)` is the whole mechanism, and because it
shares `decode_sample` with the importer, a reopened project's audio is
bit-identical rather than merely equivalent.

### The rest of it

- **Atomic save**, per §17.1: temp file in the same directory, `sync_all`,
  rename over the real one, then a best-effort sync of the directory because a
  rename is only durable once its directory entry is. What is *testable* is
  that a completed save leaves nothing behind; the crash-mid-write half is not
  testable without a crash, and the test says so rather than implying more.
- **The version is read before the body**, the same shape the patch format
  uses, so a project from a newer build reads as "upgrade Fontelle" rather than
  as a confusing field-level parse error. Its migration chain is empty and its
  first arm is written out in the doc comment. A project's format version and a
  patch's are separate numbers on purpose.
- **Assets are referenced, not copied.** §17.4's ask-once policy has no dialog
  to ask from here, and referencing is the answer that cannot surprise anybody
  by silently duplicating a 325 MB soundfont into their project folder.
  `assets/` is created anyway, because it is part of the bundle's shape.
- **A broken link opens.** §17.4 point 4, implemented: the layers that pointed
  at a moved file render silence, `OpenedProject::missing` names the file *and
  the channels it affects* so a message can name the instrument rather than a
  path nobody recognises, and a re-save keeps the reference so putting the file
  back is all it takes. Points 1-3 — the automatic search and the relink dialog
  — belong with the UI.
- **`project.json` is pretty-printed.** §17.2 chose JSON to be diffable,
  greppable and hand-recoverable, which one enormous line is not.
- **`ProjectMeta::created`** is filled in on the first save and never
  restamped: it is when the piece was started, not when it was last touched.
  ISO-8601 UTC, from a fifteen-line `civil_from_days` rather than a calendar
  dependency — my own test constants for it were off by one, and the
  implementation was right.

The SF2 fixture builder moved out of `fontelle-assets/tests` into
`fontelle_assets::fixtures`, because the round-trip test needs a real file on
disk and an integration test cannot enable a feature on the crate it is
testing.

### Verified

The gate's last clause — "save the project, quit, reopen it to an
identical-sounding state" — checked as bit-identical rather than as
"sounds the same":

```
--play-sf2 F-Zero.sf2 --save Demo.fontelle --render-wav a.wav
--open Demo.fontelle --render-wav b.wav          # cmp: identical
--play-sf2 SGM-v2.01 --play-midi "Deltarune..." --save/--open  # 2 136 621 frames, identical
```

Both are also byte-identical to the renders taken *before* any of this
stretch of work started, which is four rewrites ago now: the patch format, the
realisation step, the arena, and the command layer.

By hand at the CLI: a project whose soundfont has been deleted opens, names
the file and the channel, and renders 108 000 frames at peak 0.000; a truncated
`project.json` fails with `is not readable JSON: EOF while parsing a value at
line 26 column 2`.

Mutations, each caught by exactly one test in each crate: `open_project` never
reloading the audio (two tests, both about audio coming back); the load
skipping its version check; the save not creating the bundle directories.

### What is deliberately not built

- **No autosave, no `backups/` writing, no Export Bundle.** Autosave on a timer
  is Phase 2 item 10; the directory exists for it.
- **No relink search or dialog** (§17.4 points 1-3) — UI work. The data a
  dialog needs is what `OpenedProject::missing` carries.
- **`AssetTable` is still empty.** The patch carries its own asset references,
  which is what §8.3 requires of a portable preset; the document-level table is
  what a *shared* asset list and the copy-into-`assets/` path will need.
- **`AssetKind::Sample` cannot be reloaded** — there is no loose-sample
  importer yet. Reported as unreadable rather than silently skipped.

**Where things stand:** 409 tests, clippy and fmt clean.

## 2026-08-29 (later): every edit is a command, and it can be taken back

Phase 1 item 3. `Command` was a trait with no implementations and
`History::undo`/`redo` were `todo!()`. INVARIANT 9 — "every document mutation
goes through a `Command`, including just this one small thing" — was
unenforceable because there were no commands.

### The prerequisite: a `slotmap` cannot give an id back

This came out of writing the acceptance test the plan names, and it is the
part worth remembering.

Undo needs the inverse of "delete note A" to put back **A**. The command above
it in the history refers to it by that id, so a restore that mints a fresh key
breaks the next redo. Concretely: draw a note, drag it, Ctrl+Z twice, Ctrl+Y
twice — the second redo moves a note that no longer exists. It also makes the
plan's own test unpassable, because after apply-then-invert the document is
*not* where it started: the id changed.

`slotmap` has no way to insert at a chosen key. So `fontelle_model::Arena`
replaces it in every id-addressed collection: the same dense `Vec` indexed by
the key's index, the same free list, the same version per slot so a stale key
never reads a slot that has been reused — plus `insert_at`. Keys stay
`slotmap`'s own key types, so nothing about §10.2's on-disk story changes.

Two details:

- **Iteration is in index order, and that is load-bearing.** The sequencer
  numbers voice contexts by a clip's position in the collection and the
  realisation step numbers engine nodes by a channel's, so an unordered
  container would make two runs of one project render differently. The
  byte-identical demo bounce is the check that it did not.
- **`insert_at` refuses an occupied slot** rather than overwriting. Under a
  strictly last-in-first-out history it cannot happen — anything inserted after
  the removal has itself been undone by then — so a refusal means something
  mutated the document outside a command.

### The command set

`AddChannel`/`RemoveChannel`, `SetChannelPatch`, `AddNotes`/`RemoveNotes`/
`MoveNotes`/`ResizeNotes`, `AddClip`/`RemoveClip`/`MoveClip`/`DuplicateClip`,
`SetNumber`, `SetFlag`, `SetLoopRange`.

Rules that hold across all of them:

- **A command that cannot do its whole job does nothing.** Every note in a
  selection is checked before any note moves, so one stale id does not leave
  the rest half-dragged.
- **Nothing is clamped.** A drag that would push a note off the keyboard or
  behind the start of its clip is refused, because clamping is not invertible:
  undo would put the note where the clamp left it. Bounding the gesture belongs
  to the caller, which is the layer that knows what the pointer is doing.
- **Moves and resizes are deltas, not absolutes.** That is what makes
  coalescing "add them up" and inverting "negate", both exactly.
- **`AddChannel` creates the channel and a mixer track together**, as one
  entry. Choosing an instrument is one action; needing two presses of Ctrl+Z to
  take it back would be a bug report.
- **`RemoveChannel` takes its clips with it**, and its mixer track if no other
  channel is using it. A clip left pointing at a channel that is gone is an
  orphan: silent, and invisible to a user trying to work out why.
- **`SetNumber(Tempo)` changes the opening tempo and leaves later changes
  alone.** An imported file's tempo curve has to survive somebody nudging the
  BPM box.
- **`SetFlag` never coalesces.** A toggle is not a drag, and merging two
  presses swallows one.

`Project::loop_range` is new — the loop is document state, so a project reopens
to the section you were working on. The `Transport` still holds the sample form
for the RT thread, for the reason it always did.

### `History::break_gesture`, and why merging needs a boundary

§10.6 asks for one history entry per drag and gives `merge_with` as the
mechanism, but `merge_with` cannot tell a drag's four hundredth step from a
deliberate second nudge a minute later. Only the caller knows the mouse came
up. A time window is guesswork that either splits a slow drag or swallows an
edit somebody meant to keep, so the boundary is explicit. An undo or a redo
also ends the gesture: resuming a drag across one is not the same drag.

**Redo re-applies the command itself**, not the inverse of the inverse, which
is the other half of the id story: the command remembers what it created and
`Arena` lets it put it back under the same key.

The memory ceiling §10.6 gives a default for is now read as well as stored —
it always keeps one entry, or an edit larger than the ceiling would be
unundoable the moment it happened.

### Verified

The property test the plan asks for, in two directions and over 39 seeds: 25
random edits, then undo everything, and the serialised document has to equal
the one it started from; and the same sequence undone and redone has to land
back on the edited document. Serialised JSON rather than field-by-field,
because that is what a saved project is and it leaves nothing out — the ids
included, which is the half a slotmap could not have given back.

Mutations. Caught by exactly one test: `MoveNotes` clamping instead of
refusing; `RemoveChannel` leaving its clips behind; `SetNumber::merge_with`
taking the later `previous`. Caught by several, all of them legitimately about
the same thing: `AddNotes` re-minting ids on redo (3), `History` never
coalescing (2), undo pushing the inverse onto the redo stack (4).

Two findings from doing it:

- **One mutation was not caught, and it turned out to be equivalent.**
  Replacing `previous.get_or_insert(v)` with `previous = Some(v)` in
  `SetNumber` passes every test — and it should, because under `History`'s
  discipline the value before a re-apply is always the value the inverse just
  restored. The comment claiming otherwise was wrong and has been fixed. What
  the probe did reveal is that nothing covered a *coalesced* fader sweep
  undoing to before the gesture, so that test now exists — and it catches the
  real version of the bug (`merge_with` taking the later `previous`).
- **A property test that hung instead of failing.** `while
  history.undo(&mut doc).is_some() {}` spins forever when an undo fails,
  because a failed undo puts its entry back rather than losing it. It reads the
  result now and panics with the seed.

The demo bounce is still byte-identical, which is what says routing the CLI's
mutations through commands changed nothing about what comes out.

### What is deliberately not built

- **§8.2's `ParamAddress` is not the addressing scheme for undo targets yet.**
  That section names undo as one of the five systems one scheme should serve.
  It needs the `PersistentId` half of §10.2, which nothing in `Project`
  carries; the value commands take a typed target until then.
- **No copy/paste, no quantise, no split/join.** §16.5's piano-roll edits that
  are not draw/move/resize/delete belong with the piano roll.
- **Automation points have no commands** — automation is out of the gate.
- **Construction is not a mutation.** `Project::new`, the MIDI importer and the
  test fixtures build documents directly; there is no history to record into
  while a document is being built. Everything that changes a document that
  already exists goes through a command, including `--gain-db` and putting a
  patch on a channel.

**Where things stand:** 392 tests, clippy and fmt clean.

## 2026-08-29: the document becomes the source of truth

Two items of `docs/first-usable-plan.md`'s Phase 1. Both are the same shape as
the last round's: a type that existed, was complete, and was consulted by
nothing. `Project::mixer` was decorative, `Channel::patch_data` was an empty
`Vec<u8>` with a comment saying the format was undesigned, and `fontelle-app`
hand-built the graph from a private `Song` type that duplicated what the
document already said. Three of this file's open questions were the same
missing step.

### A patch can be written down (TDD §8.3, §17.2)

Serde derives are the easy half. The hard half is that **a layer cannot store
the key its samples live under.**

`Source::Sample` names audio by `AssetId`, which is the `slotmap` key whichever
`SampleStore` happened to decode the file minted. INVARIANT 8 forbids putting
an index on disk, and it would be useless there anyway — the same soundfont
imported into a different store gets different keys. So a stored layer names
its audio by `SampleRef`: the `AssetRef` of the file (§8.3's "presets embed
asset references, not sample data") plus the index of the sample header inside
it, because an `AssetRef` names a *file* and one soundfont holds hundreds of
samples. Reading takes a resolver from `SampleRef` to a live id.

The test that separates the two designs writes a patch against one store and
reads it into another where the same audio sits under a different key. Nothing
weaker can tell them apart, because in a single store the naive design works.

Four more decisions:

- **An unresolvable sample is reported, not raised.** §17.4 requires a project
  with a broken link to open and play with placeholders. The layer gets a null
  id — silent, because `SampleStore::get` has nothing under it — and the
  reference comes back in `LoadedPatch::unresolved` for a relink dialog. A
  layer written with *no* provenance at all is a distinct reported case rather
  than a lie the relink dialog then chases.
- **The body is untyped JSON behind a `format_version`.** A migration has to
  read shapes this build's structs no longer describe, and the version has to
  be legible without deserialising the patch so a file from a newer build reads
  as "upgrade Fontelle" rather than as damage. The migration entry point exists
  with an empty chain and its first arm written out in the doc comment; the
  first migration is the one most likely to be added under time pressure.
- **`Channel::patch_data` is `Option<PatchData>`.** `None` is a real state — a
  channel with no instrument yet — and the typed form is also what keeps
  `project.json` readable: `serde_json` writes a byte vector as an array of
  decimal numbers, and §17.2 chose JSON precisely to be diffable, greppable and
  hand-recoverable.
- **The importer decodes a sample header once, not once per zone.** A key split
  or a sustain layer regularly points several zones at one header and each was
  getting its own copy of the audio. It also makes the provenance map a
  bijection, which is what lets a reopened project get from a `SampleRef` back
  to exactly one id.

`import_sf2`/`import_sf2_preset` return `ImportedPatch` — the patch plus where
each sample came from — because the importer is the only thing that knows, and
without it there is nothing to write down.

### `Project` -> graph: the step nothing owned

`fontelle_app::realise` reads `Project::channels` and `Project::mixer` and
returns the `CompiledGraph`, the bus layout and the `ChannelId -> NodeId` map
`fontelle_sequencer::compile` takes. `Song`, `SongChannel`, `build_graph` and
`build_graph_with_gain` are gone; `demo_project` and `project_from_midi` return
plain `Project`s.

It lives in `fontelle-app` because §4.1 makes that the only layer allowed to see
both the model and the engine. `fontelle-sequencer` cannot name a `NodeId`'s
owner, which is precisely why `compile` has taken the mapping as a parameter
since it was written.

What it turns on:

- **Every channel gets a node id, instrument or not.** A compiled timeline's
  shape then depends only on the notes, so choosing or changing an instrument
  does not invalidate one — the events reach a node that is not in the schedule
  and are heard by nobody. Tying the id to the patch would make an instrument
  swap a timeline recompile, and it would make a channel briefly disappear from
  its own arrangement.
- **Tracks are scheduled deepest first.** A group's fader has to run after
  everything feeding it has been summed in, so the schedule is: every sampler
  (they only add, so their order among themselves is free), then each track by
  decreasing distance to master, then the master fader and the limiter.
  Reversing that sort is caught by two tests.
- **A routing cycle is refused before the sort, not survived by it.**
  `Mixer::has_cycle` was a `todo!()`; it is a three-colour iterative DFS over
  `output` *and* `sends`. Three colours rather than a visited set, because a
  track reachable by two paths is an ordinary two-into-one group and a plain
  "seen" set calls it a cycle. Iterative, because the routing graph is
  user-authored and a long chain must not overflow the stack on the way to
  reporting that it is fine.
- **Solo makes audible the soloed track, what feeds it, and what carries it.**
  The last is the half that is easy to miss: muting "everything not soloed"
  silences a soloed track routed into a group, because the group is not itself
  soloed.
- **A channel whose mixer track has been deleted lands on the master.** A part
  you can hear and fix beats a part that vanished.

**`Channel::pan` is new, and it is deliberately not the track's pan.** The plan
said the MIDI importer's CC7 and CC10 should both land on `Project::mixer`; CC7
does, and CC10 does not, because this document already recorded why. A track's
pan is a *balance* control over a bus the voice has already placed on the
constant-power taper, so putting CC10 there applies a pan law twice and throws
half the signal away at the extremes. The open question was that a part's pan
was "not visible anywhere in the document", and the answer this file already
suggested is the right one: a channel field, because §13.1 lets several channels
share one track and each needs its own place in the field. Closed, on the
channel rather than on the track.

**`--play-sf2` now round-trips every patch through the document on every run.**
The imported patch goes onto `Channel::patch_data` in its serialised form and
comes back out through `realise`. Save/load has no file yet and the round trip
is already exercised by every run and every rendering test.

### Verified

The proof the plan asks for is the byte-identical bounce, and it is exact.
Rendered before the change and after it, with `cmp`:

```sh
--play-sf2 F-Zero.sf2 --preset 0 --render-wav out.wav          # 108 000 frames
--play-sf2 SGM-v2.01... --play-midi "Deltarune - Don't Forget.mid"  # 2 136 621 frames
```

Both **byte-identical** across the whole rewrite, including the trip through
`PatchData`. `--gain-db -12` still peaks at 0.296, the same figure this file
recorded for it in August, and `--loop 0:1 --repeat 3` still renders three
passes.

Mutations, each caught by exactly one test unless noted: dropping
`voice_config` on write; removing the patch version guard; ignoring
`Channel::pan`; solo forgetting the group carrying it; skipping the cycle
check. Sorting tracks shallowest-first was caught by two, both of them about
ordering.

One test of mine was wrong rather than the code, and in the way this file keeps
warning about: the first pan test panned two *identical* sources apart and then
swapped them, which renders the same audio either way. It passed with the
feature absent. The two parts are at different levels now, so the mirror is
visible.

### What is deliberately not built

- **Inserts and sends are not compiled.** Effects are M4 and out of the gate; a
  send is a `BusSumNode` with a level and a pan, so the shape is there when it
  is wanted. `has_cycle` already counts send edges, because a loop through one
  is the same feedback and harder to see in the UI.
- **`AssetRef::id` is null in a stored `SampleRef`.** The field is a runtime
  handle into the document's `AssetTable`, and a reference embedded in a preset
  from another machine has no meaningful value for it. Relinking matches on
  `AssetRef::same_content`, never on the id. There are two `AssetId` spaces in
  play — a *file* in the asset table and a decoded *sample* in the store — and
  they share a type, which is worth cleaning up when the asset table is
  actually filled.
- **No `Command` yet**, so `realise`'s tests and the CLI still write to
  `Project` directly. That is Phase 1 item 3, and INVARIANT 9 starts being
  enforced when it lands.

TDD corrections written into the TDD, per its own rule: §3.1 had no hashing
crate though §17.4 specifies xxhash (`twox-hash` added; `std`'s
`DefaultHasher` is documented as unstable across releases and cannot back a
value written to disk); §7.2 said `Source` holds an `AssetRef` when it holds an
`AssetId`, because a `Layer` is read on the RT thread and an owned `PathBuf`
does not belong in it; §8.3 did not say how a preset names one sample inside a
soundfont; §10.1's `lanes` was still a `Vec`; §11.1 named nothing as the owner
of the document-to-graph step; §13.1 had neither `Channel::pan` nor solo
semantics.

**Where things stand:** 360 tests, clippy and fmt clean.

## 2026-08-28: the transport moves, and MIDI arrives from outside

Two gaps closed, both of the same kind: a type that existed, was complete, and
was wired to nothing.

### `Transport` is finally read by something

`Transport` had been written, tested and exported since the engine crate
existed, and `AudioDevice` never looked at it. The callback hard-coded
`TransportState::Playing` and counted samples from zero, so there was no play,
no stop, no seek and no loop — playback was a consequence of the stream
existing, and stopping meant tearing the stream down mid-note.

**The whole decision lives in `TransportReader`, not in the callback.** That is
the load-bearing choice here. Code inside a cpal closure can only be run by a
real sound card, so anything put there is testable only by ear; `next_step`
takes the transport, the timeline and a frame count and returns what to render,
and the callback is a loop around it. Every behaviour below has a test as a
result.

Four things the design turns on:

- **A seek is a request, not a write to the playhead.** The RT side publishes
  `position_sample` every block, so a `seek` that stored into the same field
  would be overwritten by the next callback before anything acted on it — a
  transport that ignores about half the clicks on it. `seek` writes a target
  plus a generation counter; the reader applies it and publishes the result.
  The counter, rather than comparing positions, is also what makes "re-cue to
  where I am stopped" a real seek.
- **The event cursor has to go backwards.** `events_for_block` only ever
  advanced, which is correct for playback and silent for a seek: after playing
  to the end, the cursor sits past every event in the piece, so seeking back to
  bar 1 plays nothing at all. `CompiledTimeline::cursor_at` is a binary search
  over the already-sorted events — allocation-free, so the callback can do it
  the moment it observes the seek.
- **Loop points are ticks *and* samples, published together.** INVARIANT 5 says
  the conversion goes through the `TempoMap`, and the RT thread cannot run a
  lookup against a map the model thread may be editing. `set_loop_range` takes
  both halves in one call, which is also what stops them drifting apart after a
  tempo edit. The wrap is lazy — evaluated at the top of the step that would
  have crossed the seam — so a wrap and a seek share one code path. A
  degenerate range (`end <= start`, what a half-finished drag produces) plays
  straight through, because honouring it literally yields zero-frame steps and
  spins the audio callback forever.
- **Playback starting *before* a loop runs into it** rather than jumping to the
  loop start. That is a lead-in, and it is how you get a running start into the
  section you are working on. Only a playhead *past* the loop end is pulled
  back, because that stretch is genuinely unreachable.

`render_offline` goes through the same reader, so the offline bounce and the
device path are one implementation rather than two that have to be kept in
step — and the existing offline tests still pass byte-for-byte through it.

Reachable from the command line: `--start-beat <n>`, `--loop <from>:<to>` in
beats, `--repeat <n>`. Playback now ends by watching the **playhead** rather
than sleeping for the song's duration: a blind sleep assumes the device
consumes audio at exactly the arithmetic rate, cannot notice a stream that
died, and with a loop running has nothing to count passes with. A wrap is
visible as the playhead moving backwards.

Verified by ear and by file: three passes of a one-beat loop render
**bit-identical** in the actual WAV, and a `--start-beat 1.5` render is exactly
36 000 frames shorter than the straight one.

### Live MIDI: bytes from a real port to a voice

`fontelle-midi` was a stub whose every entry point was a `todo!()`. It is now
the pipeline TDD §14.1 insists the scaffold must already be — and §14.1 is
worth quoting, because it predicts exactly the shortcut that was available
here: if the placeholder polls MIDI on the UI thread and injects notes
directly, "the one complete pass later becomes a rewrite of the transport."

The path is: `midir` callback thread -> `decode` -> `MidiRouter` -> a
single-producer queue -> the audio thread drains it once per block -> the same
`TimedEvent` stream the sequencer produces.

**A fixed array of SPSC queues, not one shared queue.** `rtrb` is
single-producer, and §14.2's "all input devices merged automatically" means
several device threads producing at once. One shared queue cannot take that,
and a mutex could take it only by letting a device thread block the audio
thread. So each port gets a queue and the merge happens at the drain. The array
is allocated up front because the alternative is the audio thread walking a
collection that hot-plug is mutating underneath it — connecting a device is
then a hand-off of an existing queue, and the consumer's structure never
changes.

**The queue lives in `fontelle-engine` and `fontelle-midi` may not depend on
it** (§4.1). They meet through `EventSink`, a trait in `fontelle-types`: the
queue is an RT structure and belongs to the engine, the things that fill it sit
outside it, and both sides can name the trait.

Details that each produce a plausible-sounding wrong result:

- **A note-on at velocity zero is a note-off.** The same rule the file importer
  needed. Read literally, every note played hangs forever.
- **Active sensing and clock are ignored rather than misread.** Most keyboards
  send active sensing several times a second for as long as they are plugged
  in, so this is the common path, not an edge case.
- **CC 120 and 123 are channel-mode messages, not controller values.** Passing
  them through as CCs is how a panic button ends up setting a parameter to zero.
- **Pitch bend is fourteen bits, low byte first, centred at 8192.** Swapped, a
  small bend reads as a large one and the wheel never returns to centre.
- **A truncated message returns `None` rather than indexing off the end.** This
  decodes on a thread the backend owns, where a panic kills a thread nobody is
  watching.

**The router is where stuck notes are prevented.** §14.2 requires that
unplugging a device release its notes, which is only possible if something
remembers what it started — a bitset per channel. That same record is what
makes the sustain pedal work (deferred note-offs, with a retriggered key
correctly removed from the pedal's set, or lifting the pedal cuts the note the
player is currently holding), and what makes a note-off for a note *this*
device never played get dropped rather than releasing the arrangement's voice
on the same key. `MidiHub` needs no lock to do the release: `midir`'s
`close()` hands back the callback's state, and the router is in it.

### The audition path, where §6.3 and §14 disagree

§6.3 says a stopped transport does not process the graph, and that is what
delivers the near-zero idle CPU target. Taken literally it also means a
keyboard makes no sound unless the song is rolling, which is not a DAW.

The reconciliation is that "stopped" should mean *idle*, and idle means nothing
is making sound. `IdleGate` wakes the graph on a live event and keeps it awake
for exactly as long as its output is non-silent — measured off the samples the
callback is already copying, so it costs a compare. A held pad stays up
indefinitely; a release tail rings out; idle CPU comes back down on its own.
A fixed timeout after the last event would cut a held note off mid-sustain, and
asking every node whether it is silent needs every node to answer honestly.

The audition step runs the graph **without advancing the playhead and with no
timeline events**: the song is stopped, so only what is being played live
should sound, and the event cursor is left alone so pressing play afterwards
still starts the song from the top.

### Stop is a statement about the sequencer, not about the player

Transport stop, seek and the loop seam called `CompiledGraph::reset`, which
cut **every** voice. With live input that is wrong, and audibly so: hold a
chord, press stop, and the notes die while your fingers are still on the keys.
They do not come back until you let go and press again — and the router still
has those keys marked down, so the note-off that eventually arrives matches a
voice that no longer exists. Every DAW keeps those notes: stop is a statement
about the sequencer.

The fix is that a voice records **where it came from**. `VoiceOrigin` is a type
in `fontelle-types` rather than a reserved `voice_context` value, because the
sequencer's contexts are clip indices counting from zero (§11.4) and a "live"
sentinel would be two unrelated numbering schemes held apart by nothing but the
unlikelihood of a collision. `ProcessContext::events_with_origin` carries the
tag — the node already knows which slice an event came from, and `events()`
was throwing that away — and `Voice::trigger_from` records it in the same call
that starts the voice, so no reset can land on a voice that is briefly active
with the wrong origin.

`AudioNode::reset_sequenced` is a defaulted trait method that falls back to a
full reset, which is right for every node with no notion of live input: an
effect's tail belongs to the audio that was flowing through it either way. Only
`SamplerNode` overrides it. `CompiledGraph::reset` is still there and still
takes everything — device teardown and the panic button are not transport stop,
because by then nobody is holding anything.

One consequence worth writing down: **`IdleGate` must not be cleared on a
reset any more.** It was, on the reasoning that a just-silenced graph cannot be
ringing. That is no longer true — a stop now deliberately spares a held note —
and clearing the gate there puts the graph to sleep underneath the very note
the reset just went out of its way to keep. Output measurement is the only
input to that decision now.

Verified: a held key survives both a stop and a seek for as long as it is held,
the song's own voices are still cut by both, and a playback-only bounce is
byte-identical to before the change.

### On tests that pass with the feature absent

This document has recorded that failure mode twice before, so this round each
new behaviour was checked by breaking the implementation on purpose and
confirming exactly one test noticed. One did not, and it was hiding the bug
above:

**"Stopping silences the output" passed with the reset removed entirely** — a
stopped transport runs no nodes, so the output is silent whether or not
anything was cut. The reset's only observable consequence is what happens on
the *next* play, and the test that pins it holds a note with a four-second
release, stops, and presses play again where the timeline has no note-on:
anything audible is a voice that survived.

Worse, the *first* attempt at a live-input test for this was named
`a_live_note_survives_the_song_stopping_underneath_it` and its body re-sent the
note-on to make the assertion pass, with a comment admitting the note did not
in fact survive. A test whose name states the requirement and whose body works
around it not being met is worse than no test: it reads, in a listing, as
evidence for exactly the thing that is broken. That test is now two — one for
the held note surviving, one for the sequenced notes still being cut — and both
fail if `reset_sequenced` falls back to a full reset.

Seven other mutations (the seek not rewinding the cursor, the loop not clamping
to its end, the idle gate never staying awake, `events()` ignoring the live
slice, `release_all` forgetting the sustained set, the stop transition not
resetting, `reset_sequenced` resetting everything) were each caught by exactly
one test.

Two of my own test *assertions* were wrong rather than the code: a power
velocity curve fixes 0 and 127, not 1, so `Soft(1) = 11` is the curve working;
and a 128-frame block always contains a whole 100-sample cycle, so its peak is
identical whether or not the voice retriggered — phase continuity is the real
discriminator.

### A real bug the new tests found

`DeviceMapping` derived `Default`, which gives `velocity_range: (0, 0)`. The
range is a window a note must fall inside, so the default mapping — the one
every unconfigured device gets, and §14.3 says per-device config is "optional
refinement, never required setup" — silently discarded every note from every
device. A range is the one field whose identity value is not its zero.

`fontelle-midi::import_midi_file` was also removed: a `todo!()` sharing an
obvious name with `fontelle_assets::import_midi`, which has worked for weeks.
Anyone reaching for it would have found the real one by panicking.

### Verified against real hardware

`crates/fontelle-midi/tests/hardware_loopback.rs` (ignored by default) sends
into this machine's ALSA "Midi Through" port and receives it back through
`MidiHub` — enumeration, `connect`, the backend's callback thread, the decode,
the router, the queue, none of it simulated. Both a played note and a
disconnect-while-holding arrive correctly. The two tests share one physical
port and had to be serialised: run in parallel, one test's note-on was received
by the other's hub, which showed up as three events received for the two sent.

`cargo run -p fontelle-app -- --play-sf2 <file> --midi-in` opens every input,
reports devices arriving and leaving, plays the song underneath, and keeps the
keyboard live after it ends.

### What is deliberately not built

- **No note chase on seek.** Seeking into the middle of a held note starts
  nothing: a note-on behind the playhead is not retriggered. Chasing needs the
  timeline to answer "what is sounding at sample X", which is a table the
  sequencer builds off-thread, not a scan the callback can afford.
- **The loop seam is a hard cut.** A note sounding across it is cut and
  restarted, because its note-off is on the far side. Measured, the seam's
  largest sample-to-sample step is about 5% of full scale against 3% inside a
  pass — a small step, not a bang, but it is a discontinuity. Crossfading it is
  the fix.
- **Live events are stamped at the block start**, not their true arrival time
  — at most one block late (2.7 ms), always late, never early. Sample accuracy
  needs the backend's timestamps to be comparable with the audio clock, and
  guessing at that conversion buys accuracy that is wrong by an unknown offset.
- **No CC is routed anywhere.** Sustain is handled inside the router; every
  other controller is dropped. Routing a CC to a parameter is the MIDI-learn
  table (§14.4) resolving it to a `ParamAddress`, and the nodes it would
  address expose no parameters yet — emitting `ParamValue` events nothing reads
  would look like a working feature.
- **`ClockSync::on_midi_clock_tick` is still a `todo!()`** (§14.5), and MIDI
  file *export* with it (§14.6). Clock bytes are decoded and discarded today.
- **Device keys are port names.** `midir` exposes no portable route to USB
  identifiers, and a key built from a port index would renumber whenever
  anything else is plugged in — configuration would silently follow the wrong
  device.

**Where things stand:** 331 tests, clippy and fmt clean, plus three
hardware-only tests run deliberately.

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
  block RMS, latching clip indicator) and `DcBlocker` (one-pole/one-zero,
  corner placed from `cutoff_hz`) are real and tested. The `High`
  windowed-sinc interpolation kernel is real too; only `Ultra` remains a
  `todo!()` (it needs a stateful resampler, not a point-interpolator).
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
- **fontelle-model** — mostly real as data, still stub as behaviour.
  `TempoMap` is piecewise and real. `Project::lanes` is a `SlotMap<LaneId,
  Lane>`. `Project::new` creates a master mixer track, `Channel` carries an
  `Option<PatchData>` and a `pan`, and `Mixer::has_cycle` is real (2026-08-29:
  three-colour iterative DFS over `output` *and* `sends`). `Arena` replaces
  `SlotMap` in every id-addressed collection, because undo needs to give an id
  back. `Command`, `History::undo`/`redo` and fifteen commands are real
  (2026-08-29). Still `todo!()`: `prefab::resolve`. Tests:
  `crates/fontelle-model/src/{project,mixer,arena}.rs` and
  `crates/fontelle-model/tests/{commands,storage}.rs`. `save_project`/
  `load_project` are the §17.1 folder bundle, written atomically.
- **fontelle-sequencer** — `compile()` is real, scoped to `ClipSource::Notes`
  clips with no prefab resolution (see the "2026-08-23 update" above for the
  full scope-cut list). `collision::voice_context_for_clip` was already real
  (trivial). `incremental::{DirtyBars::mark_range, recompile_dirty}` are still
  `todo!()` — M3 work, not needed until incremental (as opposed to whole-project)
  recompilation matters. Tests: `crates/fontelle-sequencer/src/compile.rs`.
- **fontelle-engine** — `CompiledGraph::process_block` handles real multi-node
  chains via the in-place convention (a consuming node declares the same
  buffers in and out; see the 2026-08-24 update), and since 2026-08-26 also
  nodes whose inputs are a different set from their outputs (`BusSumNode`,
  which is what a send needs too). `SamplerNode` is real (wraps `Sampler` + a shared `Arc<SampleStore>`;
  renders mono and fans out across its output channels). `MixerTrackNode` is
  real for the fader stage — gain/pan/mute/phase, stereo or mono.
  `AudioDevice` is real — opens a real `cpal` stream, promotes the callback
  thread via `audio_thread_priority`, tags it via
  `rt_guard::mark_current_thread_rt`, chunks the callback into `BLOCK_SIZE`
  (128-frame) pieces regardless of what the backend delivers, walks the
  `CompiledTimeline` by sample range, and interleaves bus *N* into device
  channel *N*. `MasterNode` is real: a brickwall limiter, peak/RMS metering per
  channel, and a `MasterMeter` handle publishing peaks and gain reduction as
  atomics for anything off the RT thread. `EffectNode`/`SendNode`/
  `AudioClipNode` are still empty placeholder structs — M4/M6 work.
  `Transport` is read for real now (2026-08-28): `TransportReader::next_step`
  drives both the device callback and `render_offline` — play, stop, seek,
  loop, plus the `IdleGate` audition path and the live-event SPSC queues the
  MIDI hub feeds.
- **fontelle-assets** — `import_sf2` is real, see "SF2 import scope" below;
  `load_sf2_samples` reloads the specific sample headers a saved patch names
  (2026-08-29). `import_midi` is real (notes, piecewise tempo, per-channel
  program/CC, a mixer track and a channel pan per part). `fixtures` holds the
  hand-built SF2 byte streams both this crate's and `fontelle-app`'s tests use.
  `import_sfz`, `SoundfontLibrary`, peak generation are pure stub.
- **fontelle-midi** — real (2026-08-28): device enumeration and hot-plug via
  `midir`, decode, `MidiRouter` (sustain, stuck-note release on disconnect),
  `MidiHub`. Still stub: `ClockSync` (§14.5) and MIDI file *export* (§14.6).
  Recording a take is `fontelle-engine`'s capture ring plus
  `fontelle_model::notes_from_capture` (2026-08-29, TDD §14.7) — nothing here
  needed to change, because a take is a copy of the stream this crate already
  produces.
- **fontelle-app** — the DAW binary, plus a `lib.rs` holding what a
  `[[bin]]` cannot export. `realise` (2026-08-29) is the document -> graph
  step: it reads `Project::channels` and `Project::mixer` and returns the
  `CompiledGraph`, the bus layout and the `ChannelId -> NodeId` map. It lives
  here because §4.1 makes this the only layer allowed to see both the model and
  the engine. `SampleLibrary` holds the decoded audio and the two-way mapping
  between store ids and the file references a saved patch names them by.
  `render_offline`, `write_wav16`, `demo_project` and `project_from_midi` are
  here too, and `open_project`/`save_project` (2026-08-29) are the folder
  bundle plus the sample reloading a reopened project needs. `EngineHost`
  (`src/window.rs`, 2026-08-29) implements `fontelle_ui::TransportHost` over
  `Arc<Transport>` and `Arc<MasterMeter>`, and `Session` (`src/session.rs`,
  2026-08-29) implements `fontelle_ui::DocumentHost` — command, history,
  recompile, publish. This crate is the one layer allowed to see model, engine
  and UI at once, which is what keeps `fontelle-ui` off `fontelle-engine`.
  `blank_project` is the empty starting point `--blank` opens.
- **fontelle-ui** — real, as far as items 6-8 go, plus item 9's panels
  (2026-08-29). `theme` is a
  full token set with a dark default, a light variant and a versioned JSON
  format (**v2**, with working migrations from v0 and v1); `layout` is the
  window geometry; `widget` holds the §16.3 invalidation core — `Redraw`, and
  `sleep_budget`, which decides whether a frame happens at all and how long the
  loop may sleep; `text` turns `cosmic-text` shaping into vello glyph runs;
  `transport` is the transport bar's view-model behind the `TransportHost`
  trait; `canvas::piano_roll` is the roll's — geometry, virtualisation, snap,
  hit-testing and the drag state machine — emitting `RollEdit` values through
  the `DocumentHost` trait; `render` holds `draw_window` and `draw_piano_roll`
  (pure functions of theme + layout + chrome) and `Headless`, which renders a
  scene into memory with no surface; `app` is the winit/wgpu/vello event loop
  and the input routing, and nothing else. It depends on `fontelle-model`
  read-only and on `fontelle-engine` **not at all** — INVARIANT 2 and 9 hold
  because no `&mut Project` is reachable from here. `canvas::timeline` (the
  arrangement) and `canvas::mixer` (a fader, a pan, mute/solo and a meter per
  track, with the master pinned) are real too, and `transport` carries the
  tempo and time-signature boxes. Nothing in this crate is `todo!()` any more;
  what it does not have is widgets in the tree beyond the panels, the bar, the
  roll and the strips. Tests: `crates/fontelle-ui/tests/`.
- **fontelle-plugin** — pure stub, unchanged since scaffolding.
  `BaseviewBackend::request_redraw` is a `todo!()`; M2 is deferred until after
  the first-usable gate (§2.2 of `docs/first-usable-plan.md`).
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
- ~~**`fontelle-model::Channel` stores a serialised `Vec<u8>` patch.**~~
  **Settled 2026-08-29:** it holds `Option<fontelle_types::PatchData>`, the
  typed form the scaffolding note suggested. The format itself lives in
  `fontelle-core` (§8.3) and the model still depends on nothing but
  `fontelle-types`.
- **`TempoMap` holds a runtime `sample_rate_hz` field** that's `#[serde(skip)]`
  — never saved with the project, since it's the audio device's rate, not
  document data. Not stated anywhere in the TDD (§6.2's method signatures take
  only a tick or sample, implying the sample rate is known some other way);
  this is the scope-cut, single-segment `TempoMap`'s way of knowing it. Revisit
  once real ramp segments land — TDD §6.2's design doesn't visibly address
  where the sample rate comes from either.
- ~~**The mixer is built in `fontelle-app`, not compiled from
  `Project::mixer`.**~~ **Settled 2026-08-29:** `fontelle_app::realise` builds
  the graph, the bus layout and the channel->node map from `Project::channels`
  and `Project::mixer`. A MIDI file's CC7 lands on a document mixer track.
- ~~**MIDI CC10 is applied at the sampler, CC7 at the mixer track**, so a
  part's pan is not visible anywhere in the document.~~ **Settled 2026-08-29:**
  `Channel` has a `pan`, applied at the sampler by the realisation step. The
  distinction it was drawing is real and stays — a track's pan is a balance
  control over an already-placed bus — and a channel field is what §13.1's
  "several channels may share a mixer track" requires.
- **`fontelle_sequencer::compile` takes a `channel_nodes: &HashMap<ChannelId,
  NodeId>` parameter** not implied by the TDD's prose (§11.1 gives no Rust
  signature for `compile`). Needed because the crate can't depend on
  `fontelle-engine` to look up a channel's compiled `SamplerNode` identity
  itself. **The owner now exists** (2026-08-29): `fontelle_app::realise` hands
  out the map, and §11.1 has been corrected to say so. The parameter stays,
  because §4.1 still forbids the sequencer from discovering it.

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
11. ~~Transport.~~ **Done** — see the 2026-08-28 section. Play, stop, seek,
    loop, all decided in `TransportReader` rather than the callback. Note
    chase on seek and a crossfaded loop seam are the documented cuts.
12. ~~Live MIDI input (TDD §14).~~ **Done** — see the 2026-08-28 section.
    Devices, hot-plug, decode, router, SPSC queues into the audio thread,
    verified against real hardware. Clock sync and MIDI file export remain.
13. ~~Phase 1 of `docs/first-usable-plan.md`~~ **Done, 2026-08-29** — patch
    serialisation, the `Project` -> graph realisation step, commands and undo,
    the project bundle, and MIDI recording.
14. ~~**Phase 2 of `docs/first-usable-plan.md`, the walking GUI skeleton.**~~
    Items 6 (window + surface + one panel), 7 (the transport bar over the real
    engine), 8 (the piano roll) and 9 (channel rack + soundfont browser +
    arrangement) are **done, 2026-08-29** — see the sections at the top. The
    vello stack came up without needing the lyon fallback; zero frames at idle
    is built in and measured; and the window drives a live audio thread through
    nothing but atomics. The arrangement canvas and the instrument editor
    landed in the "the studio becomes usable" pass, along with the FL Studio
    behaviours a person using the window found missing.

    ~~**Still outstanding under item 9**~~ — **item 9 is closed** (2026-08-30):
    the mixer strip, the tempo and time-signature boxes, record-arm and the
    metronome are all in. ~~**Item 10 is most of the way done too**~~ —
    **item 10 is closed** (2026-08-31): the browser panel's Projects tab
    makes, lists and opens projects out of a configurable folder, the title
    bar has carried a `•` since `refresh_title` landed, and the autosave timer
    is in. The first-run settings it also mentions are covered by the two
    folder pickers.

    **Item 11 (export) is done** (2026-08-31) — an offline render at render
    quality into `<bundle>/renders/`, from a button and from Ctrl+E. What is
    left of Phase 3 is **item 12: the real-project shakedown** — making an
    actual multi-part piece in the window, on hardware, end to end, and fixing
    what that finds. Every gate before it has been closed by somebody using
    the thing rather than by reading it.
15. Then the rest of M1: streaming (TDD §7.7 — we currently hold whole
    soundfonts in memory) and effects. `ParametricEq::process` and
    `Compressor::process` are the two `fontelle-dsp` could already support.
16. **Latency compensation.** The master limiter is the first node in the tree
    with real latency (`AudioNode::latency_samples` reports it and nothing
    reads it). With one bus that is a uniform delay nobody can hear; with a
    send path or a track that bypasses it, it is a phase error.
17. **Idle CPU with the device open (§19).** Measured 2026-08-29 in release,
    per thread, with the transport **stopped** and an output stream open:
    `cpal_alsa_out` ~1.0% of a core and ALSA/PipeWire's helper thread ~0.95%,
    against §19's "< 0.5%". The window itself is 0.05%, and the callback does
    fill silence and return as §6.3 says — so the cost is the device period
    rate, not our work inside it. Worth a look at buffer size and at whether a
    stopped transport should let the stream go idle; §19 is measured on the
    app, not on the window.

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
