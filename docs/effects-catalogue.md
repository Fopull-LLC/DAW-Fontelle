# The built-in effects: a catalogue

What ships in the mixer's "+ Add effect" menu, what each one has to be able
to do before it counts as done, and in what order the missing ones get built.

This extends TDD §13.4's one-line-per-effect table into a design. §13.4 stays
the authority on *which* effects exist in v1; this is the authority on how far
each one has to go. Where the two disagree, this is the newer document and
the disagreement is recorded here.

**The brief this answers** (Ty, 2026-09-02): the built-ins have to carry a
producer on their own. Not "a distortion", but *the* distortion — the one
that means you do not go looking for another. Every effect is a family of
sounds behind one name, with the necessities first and the fun ones after.
The test of a built-in is whether a person can get somewhere they have not
been before by turning its knobs, not whether it has a knob.

---

## 1. Principles every effect follows

These are the rules the existing seven were built to and the rules the rest
are held to. Each one exists because its absence has already produced a
defect this project recorded.

1. **A family, not a sound.** One "bitcrush" that makes one kind of crunch is
   a preset pretending to be an effect. The chooser on an effect names
   *kinds* — curves, quantisers, decimators — and a `shape` knob turns each
   kind into a continuum. Two positions of a chooser that sound the same are
   one position, and `every_curve_does_something_and_they_are_not_all_the_
   same_thing` is the test that says so.
2. **A fresh effect is very nearly a wire**, unless it is a macro with one job
   (Soften opens at Gentle for a reason written on its constructor). Drive at
   zero, bits at sixteen, ratio at 1:1. Somebody who has just added an effect
   has not yet said what they want.
3. **Every parameter is a `ParamSpec`**, reached through `EffectConfig::specs`,
   readable, writable, normalisable, and therefore automatable, MIDI-learnable
   and saved (INVARIANT 7). There is no such thing as a knob that a lane
   cannot own. A chooser names its positions.
4. **Gain staging is part of the effect.** Anything nonlinear has an input
   gain and an output gain, because the sound of a quantiser or a shaper *is*
   the level going into it. An "auto gain" that keeps the loudness where it
   was is what turns a drive knob from a volume into a tone.
5. **Time follows the song.** Anything measured in time — delay, LFO rate,
   gate hold, stutter length — has a `sync` switch and a note-value chooser
   beside its millisecond knob. The tempo reaches the audio thread
   (`TransportSnapshot::bpm`); reading it costs a field.
6. **Nonlinear means oversampled**, as a chooser (off/2×/4×/8×) rather than a
   switch, because the cost is real and the person mixing chooses where to
   spend it. `fontelle-fx/tests/distortion.rs` measures what it buys.
7. **The dry signal is `EffectNode`'s**, not the effect's. An effect writes
   only what it made; the node blends it under the track. This is what makes
   parallel anything one knob.
8. **Filters inside a loop are one-pole**; filters on a signal are the SVF.
   A resonant filter in a feedback path rings the loop.
9. **Stereo-linked detection** for anything dynamic, so the image does not
   move with the material.
10. **Presets are constructors, not parameters.** A preset writes the knobs
    and then has nothing further to say; a preset *knob* would fight them.
    Every effect with more than eight parameters ships presets, **or says why
    not**: the utility's ten controls are ten separate jobs and the gate's and
    the filter's are one machine with one obvious knob, so a preset that wrote
    all of them would be a preset for nobody. The picker landed 2026-09-02 —
    a row of chips over the generic panel, one undo entry per click. Which
    effects ship them is a decision taken per effect and held by a test.
    **Amended 2026-09-06** (`docs/flopsynth-plan.md` §P): presets become
    *files* in a DAW-wide bank rather than constructors per effect, the
    chip row becomes one preset bar on every window, and a preset's name
    now persists on the device with a `*` for unsaved edits — "a preset is
    not a parameter" stands; "nothing remembers which one" does not.
11. **Sections.** An effect with more than about eight controls declares
    `sections()`, and the generic panel draws a heading per section rather
    than one grid. Drive / Voicing / Output is a layout a person can read;
    fourteen knobs in a row is not.
12. **Tests measure the claim**, not the mechanism: a harmonic, a held sample,
    a gain that moved. A test that would pass against the effect stripped of
    its feature is not a test of the feature.

---

## 2. The catalogue

Status key: **built** — in the menu today; **rebuilt** — in the menu today,
extended to this document's design on 2026-09-02; **new** — written to this
document's design on 2026-09-02, in the pass after the two rebuilt ones;
**planned** — not yet written, with a priority.

Priority key: **P0** — a mix cannot be finished without it; **P1** — a mix
can be finished but a producer will go looking for it; **P2** — quality of
life or fun, worth having because it is cheap or because it is the kind of
thing that makes somebody choose the software.

### 2.1 Dynamics

| Effect | Status | Priority | The family it has to be |
|---|---|---|---|
| **Compressor** | built | P0 | Threshold, ratio, attack, release, knee, makeup, auto-makeup, peak/RMS, mix. **To add:** sidechain high-pass (so the kick does not pump the bass), lookahead (0–10 ms), stereo link amount (100 % linked → independent), program-dependent release (auto), external key from any track (the graph's job, §13.2 — the DSP already takes one), and a **character** chooser: *clean* (what it is now), *opto* (release slows as it recovers), *FET* (fast, with a little grit on the attack), *VCA-bus* (gentle knee, slow, the glue setting). Gain-reduction meter and transfer curve in the window. |
| **Limiter** | built, master only | P0 | The master's limiter is real and has been running since 2026-08-26. **To add:** an insert version (`EffectKind::Limiter`) with ceiling, lookahead, release, *true-peak* switch (4× oversampled detection), a *style* chooser (transparent / loud / punchy — which is release shape and attack pre-shaping), and gain-reduction metering. Same DSP, one more `EffectState` arm. |
| **Gate / Expander** | **new** | P0 | See §3.3. Threshold, hysteresis, key high-pass, look-ahead, attack, hold, release, ratio, range. One effect for the gate and the expander because they are one machine at two settings. Detection is a peak **envelope** with a 20 ms hold, not a rectifier — see §3.3. **Still to come:** the external key, which waits on sends reaching `EffectNode`; the DSP takes one already. |
| **Transient shaper** | planned | P1 | Attack (−100..+100 %), sustain (−100..+100 %), detection speed (fast/medium/slow), per-band split optional (low/high) so a kick's click and its body can be shaped apart. Triggered by envelope *difference* rather than level, which Soften's transient stage already does — it is the same detector with an output gain instead of a duck. |
| **Multiband compressor** | planned | P1 | Three or four bands on Linkwitz–Riley crossovers, each a full compressor (threshold, ratio, attack, release, makeup, solo, bypass), plus global mix. This is the mastering necessity and the one that "makes the low end sit". Built as N copies of `Compressor` behind a crossover; nothing new in the detector. |
| **De-esser** | planned | P1 | Frequency (2–12 kHz), threshold, range, mode (*wideband* ducks everything, *split* ducks only the band), listen switch. A band-limited compressor; ships as its own kind because the workflow is different and the defaults are not a compressor's. |
| **Clipper** | planned | P1 | Ceiling, softness (hard → soft knee), oversampling, output. The mastering clipper is the distortion's hard-clip curve with a ceiling below full scale and a meter of how much it is clipping; it ships as its own kind because "a clipper" is what people look for and its defaults are a clipper's. Shares `fontelle_fx::shape`. |
| **Ducker** | planned | P2 | A compressor whose key is another track and whose knobs are *amount*, *attack*, *hold*, *release*. Sidechain compression with the compressor's controls hidden, because that is what nine of ten sidechain uses want. Lands the day external keys reach `EffectNode`. |

### 2.2 Equalisation and filtering

| Effect | Status | Priority | The family it has to be |
|---|---|---|---|
| **Parametric EQ** | built | P0 | Eight bands, eleven types, mid/side per band, analyser, mix. **To add:** per-band *dynamic* mode (threshold and range, so a band cuts only when its region is loud — the surgical version of Soften's shelf), band solo/listen, a spectrum *output* tap beside the input one, and a *linear-phase* switch for mastering (v2 — needs an FFT convolver; the latency compensation it also needs is built, TDD §5.5). |
| **Filter** | **new** | P0 | See §3.6. Eight shapes (LP/HP/BP at 12 and 24 dB, notch, peak), cutoff, resonance, drive before the filter, a signed envelope amount with its own attack and release, an LFO with six waves and a sync, output trim, mix. No key tracking: a filter that follows the note needs a note, and an insert on a bus does not have one. |
| **Tilt EQ** | planned | P2 | One knob: pivot frequency and tilt in dB. A shelving pair that lifts one end as it lowers the other. Cheap, and the fastest "brighter/darker" there is. |
| **Graphic EQ** | planned | P2 | Ten bands at ISO centres, ±12 dB. Not better than the parametric; different. Some people think in sliders. Built as ten bells with fixed Q. |
| **Formant filter** | planned | P2 | Vowel chooser (A E I O U), morph between two vowels, resonance, LFO on the morph. Three band-passes at the vowel's formants. The talk-box sound without a tube in your mouth. |
| **DC / rumble filter** | planned | P2 | Part of Utility — see §2.5. |

### 2.3 Distortion and lo-fi

| Effect | Status | Priority | The family it has to be |
|---|---|---|---|
| **Distortion** | **rebuilt** | P0 | See §3.1. Ten curves each with a `shape` continuum, bias, sag, pre-voicing (high-pass and a mid bell before the drive), clean low band, tone, output, auto-gain, oversampling off/2×/4×/8×, mix. Presets: *Overdrive*, *Fuzz*, *Amp*, *Fold*, *Bass grit*, *Octave*, *Digital*. |
| **Bitcrush** | **rebuilt** | P0 | See §3.2. Input gain, bits, three quantisers, four dithers, rate, three decimators, jitter, anti-alias, post filter, output, mix. Presets: *12-bit sampler*, *8-bit console*, *Telephone*, *Broken clock*, *Sparse*, *Vinyl-adjacent*. |
| **Tape** | planned | P1 | Saturation with hysteresis-shaped curve (soft, level-dependent, with a little HF loss under drive), bias (asymmetry), wow (slow pitch drift: depth, rate), flutter (fast: depth, rate), high-frequency loss (a shelf that deepens with drive), hiss (level, colour), and a *speed* chooser (7.5 / 15 / 30 ips, which sets the defaults for all of the above). The warmth tool; distinct from the distortion because its whole point is subtlety. |
| **Ring modulator** | planned | P1 | Carrier frequency (Hz, or a note offset when *track* is on and the insert sits on an instrument channel), carrier wave (sine/tri/saw/square), fine-tune, LFO on the frequency, mix. Metallic bells and robot voices; cheap. |
| **Frequency shifter** | planned | P2 | Shift in Hz (±5 kHz, log around zero), up/down/both, feedback (the barber-pole), mix. Needs a Hilbert pair (an allpass cascade), which is the only new DSP. The one that sounds like nothing an EQ can do. |
| **Lo-fi** | planned | P2 | Vinyl crackle, hum, noise floor, band-limit (a telephone or a cassette), wow, dropouts. Overlaps with tape and bitcrush by design; it is the *preset* half of both, aimed at somebody who wants "old" without knowing which kind. Built out of their parts. |

### 2.4 Time and modulation

| Effect | Status | Priority | The family it has to be |
|---|---|---|---|
| **Delay** | built | P0 | Time, sync, division, feedback, damping, drive, ping-pong, mix. **To add:** a second time for the right channel (with a link switch, so stereo offsets and dual-tap rhythms exist), a high-pass in the loop beside the low-pass, *ducking* (the repeats drop while the source plays — an envelope on the input, gain on the wet), *tape* mode (wow and flutter on the read head, with the pitch smear the glide already gives), *diffusion* (a short allpass pair so repeats blur), *reverse* (a buffer played backwards per repeat), *freeze* (feedback at unity, input off — a switch), and an *era* chooser (digital / tape / bucket-brigade, which sets the loop filter and the drive). |
| **Reverb** | built | P0 | Size, decay, damping, pre-delay, width, mix. **To add:** diffusion (the allpasses in front of the network, which is what stops a small room sounding like a flutter), modulation (depth, rate — a slow wobble on the line lengths, which is what makes a long tail lush instead of metallic), low cut and high cut on the wet, early reflections level, *freeze*, *ducking*, and an *algorithm* chooser: room / hall / plate / chamber / ambience, which are line-length sets and diffusion defaults. *Shimmer* waits on the repitcher. Convolution is v2 (IR licensing, §13.4). |
| **Chorus / ensemble** | **new** | P0 | See §3.5. Voices 1–4, a chorus/ensemble chooser, spread, rate with sync and a division, depth, centre delay, **signed** feedback, tone, mix. Each voice has its own centre as well as its own LFO phase, which is what stops two of them being one. |
| **Flanger** | planned | P1 | Rate (sync), depth, delay (0.1–10 ms), feedback (±, negative for the hollow one), stereo phase, *through-zero* switch (a dry delay equal to the centre, so the sweep passes through nothing), manual (the centre as a knob for automation). Shares the chorus's modulated line. |
| **Phaser** | planned | P1 | Stages (2–12, even), rate (sync), depth, centre frequency, feedback, spread, stereo phase, mix. A cascade of first-order allpasses; a different sound from a flanger and people know which they want. |
| **Tremolo / auto-pan** | planned | P1 | Rate (sync), depth, wave (sine / triangle / square / saw up / saw down / sample-and-hold), stereo phase (0° is tremolo, 180° is auto-pan), smoothing (for the square), mix. One effect because they are one LFO on two gains. |
| **Vibrato** | planned | P2 | Rate (sync), depth (cents), wave, stereo phase. A modulated delay line with no dry — pitch wobble on anything. Shares the chorus's line. |
| **Stutter / glitch** | planned | P2 | Length (sync), repeats, decay, reverse chance, pitch step per repeat, trigger (manual/threshold/every beat), mix. A buffer that repeats what just happened. Fun, and tempo-native from day one because rule 5 says so. |
| **Trance gate** | planned | P2 | Sixteen-step pattern (each step a level), rate (sync), attack, release, swing, stereo alternate. A gate driven by a pattern rather than a threshold. Built on the gate's envelope. |

### 2.5 Stereo, utility and metering

| Effect | Status | Priority | The family it has to be |
|---|---|---|---|
| **Utility** | **new** | P0 | See §3.4. Gain, pan (a *balance*), width 0–200 %, mono-maker, swap, two mutes, two polarity flips, a DC/rumble filter, mix. One insert rather than seven, and first in the "+ Add effect" menu because it is the commonest thing in mixing. The metering stubs that shared its file moved to `fontelle-fx/src/meters.rs`. |
| **Stereo imager** | planned | P1 | Three bands (crossovers), width per band (0–200 %), pan per band, solo, a correlation meter in the window. Multiband width is how a mix gets wide without the bass smearing. |
| **Mid/side** | planned | P2 | Encode / decode, mid gain, side gain. Two switches and two knobs; lets any *other* effect be put on the side channel by bracketing it. |
| **Haas widener** | planned | P2 | Delay (0–30 ms) on one side, gain match, mono-safe switch (a high-pass on the delayed side). A cheap width that the imager cannot do. |
| **Spectrum analyser** | built as a tap | P1 | The EQ window draws one. **To add:** a standalone insert that only draws — spectrum, with peak hold, average, slope (3 dB/oct tilt so pink reads flat), and a second input from another track for overlay. |
| **Oscilloscope** | planned | P2 | Time window, trigger level, stereo overlay. The tap exists; this is a window. |
| **Loudness meter** | planned | P1 | Integrated / short-term / momentary LUFS, true-peak, range. The mastering necessity that is a *meter*, so it belongs on the master by default like the limiter. ITU-R BS.1770 K-weighting is two filters. |
| **Tuner** | planned | P2 | Detected pitch, cents, a needle. For checking a soundfont's root against its name, which is a thing this program's users do. The pitch is already found: wrap `fontelle_dsp::PitchTracker` (§2.6's Tune row built it) rather than growing a second detector — what is missing is a read-out insert around it and the needle. |
| **Correlation / goniometer** | planned | P2 | Part of the imager's window. |

### 2.6 Pitch (mostly v2)

| Effect | Status | Priority | The family it has to be |
|---|---|---|---|
| **Repitcher** | `todo!()` | P1 | Varispeed over a *clip*, not a bus (§13.4). It lives in the clip's operations, not in this menu, and the stub in `fontelle-fx` is in the wrong crate. |
| **Pitch shifter** | v2 | P2 | Formant-preserving shift as an insert, behind the `stretch` feature flag (§3.3). Until then, a granular shifter (two crossfaded windowed reads) would give a usable octave-down and a *shimmer* for the reverb — worth building as *Pitch (granular)* if the flag stays closed. |
| **Harmoniser** | v2 | P2 | Two shifted voices at intervals with pan. On the pitch shifter. |
| **Tune** (pitch corrector) | **built** | P1 | The autotune: a YIN tracker and a PSOLA shifter in `fontelle-dsp`, three engines (Smooth / Hard / Grain) with a `texture` continuum, formant shift and formant-follow, retune speed, humanize, flex, natural and added vibrato, fifteen scales on a keyboard in the window, notes from any channel in the rack, a Character section (drive, crush, air, width — §4.8, with a measured auto-gain on the drive), **forty** presets, and a window drawn as a ship's console. **The whole design is `docs/tune-plan.md`**; read it before touching anything pitch-shaped. Its `PitchTracker` is what the Tuner row above becomes. |
| **Vocoder** | planned | P2 | Sixteen bands, carrier from another track (or an internal saw/noise), formant shift, attack/release per band. Needs the external key; the band filters are the SVF. Fun, and the kind of thing that sells a demo. |

### 2.7 Deliberately not in the catalogue

- **Third-party plugin hosting.** M2 (§8.4). Nothing here depends on it.
- **Convolution reverb.** v2, for the IR-licensing reason §13.4 gives.
- **Amp/cabinet simulation with impulse responses.** Same reason. The
  distortion's *Amp* preset and its voicing stages are the substitute.
- **Spectral effects** (freeze, morph, denoise). v2; need an STFT framework
  that nothing else here needs.
- **An LFO / modulator effect that drives other effects' parameters.** That
  is the automation system's job (§12), or a mod matrix's, not an insert's.
  An insert that reaches across the graph to move another insert's knob is
  the second addressing scheme §8.2 forbids. **The mod matrix is where that
  job now lives**: Flopsynth's four LFOs and four envelopes reach every
  per-voice destination in its own patch (`docs/flopsynth-plan.md` §3.6), and
  its own effects chain is part of the patch — so "an LFO on a chorus" is a
  route inside one instrument rather than one insert reaching for another.

---

## 3. The reference designs, in full

The first two are the effects rebuilt on 2026-09-02 and are what "goes far"
means; the four after them are the ones written to that standard in the pass
that followed, in the build order §4 gives.

### 3.1 Distortion

The report: *"I'm finding it hard to get more than a basic distortion sound
with our distortion plugin."* Five curves, a drive, a tone and an output is
a pedal. What was missing is everything a pedal's *circuit* does around the
clipping stage — the voicing before it, the sag under it, the bias across
it — and any way to make each curve a continuum rather than one point.

**Sections and parameters** (ids are permanent, INVARIANT 7):

*Drive*
- `curve` — chooser, ten positions: **soft clip** (variable-hardness
  saturator, `x / (1+|x|^p)^(1/p)`), **hard clip** (a ceiling, with a knee
  from `shape`), **tube** (asymmetric soft clip), **diode** (exponential knee,
  the germanium fuzz), **fold** (sine fold), **triangle fold** (linear wrap
  fold — sharper, more synthesiser), **wave shape** (cubic → quintic
  polynomial), **rectify** (half → full wave; the octave-up fuzz),
  **crossover** (a dead zone at zero — the spitting, gated fuzz),
  **wrap** (integer-overflow wraparound — the digital one).
- `shape` — 0–100 %, what each curve's continuum is: hardness, knee,
  asymmetry, fold count, polynomial order, half-to-full, dead-zone width,
  wrap threshold. Documented per curve on `DistortionCurve`.
- `drive` — 0–48 dB into the curve.
- `bias` — −100..+100 %, a DC offset into the curve, taken back out after.
  Asymmetry on any curve, which is even harmonics on any curve.
- `sag` — 0–100 %, drive falls as the input gets loud (an envelope on the
  input pulling the drive down by up to 12 dB). The amp's power supply.

*Voicing*
- `pre_hp` — 20 Hz–2 kHz, a high-pass **before** the curve. Tight versus
  flabby: the difference between a bass fuzz and a mess.
- `pre_mid_hz`, `pre_mid_db` — a bell before the curve (200 Hz–5 kHz,
  ±18 dB). The Tube Screamer's mid hump, and every other pedal's voicing.
- `clean_low` — 20 Hz–500 Hz, or off at 20: below this the signal goes
  *around* the curve and is added back clean. Bass distortion that keeps its
  bottom. A second-order crossover so the sum is flat.
- `tone` — 200 Hz–20 kHz low-pass after the curve. Unchanged.

*Output*
- `output` — −24..+12 dB.
- `auto_gain` — switch. Measures the curve's own level at this drive on a
  reference sine and compensates, so the drive knob is a tone control and
  not a volume.
- `oversample` — chooser off / 2× / 4× / 8×. Was a switch; a saved `true`
  reads as 2×.
- `mix`.

**What it is not.** Not multiband (three of these behind a crossover would
be a multiband distortion, and the `clean_low` band is the case that
matters most). Not an amp simulator (no cabinet). Not a gate (put one
after it).

**Presets** (constructors on `DistortionConfig`): Overdrive (soft, shape 20,
pre-mid +6 dB at 800 Hz, tone 6 kHz), Fuzz (diode, shape 70, bias 20, sag
40, pre-hp 120 Hz), Amp (tube, sag 50, pre-mid +4 at 1.2 kHz, tone 4.5 kHz,
2×), Fold (fold, shape 40, 4×), Bass grit (soft, clean-low 120 Hz, pre-hp
30 Hz), Octave (rectify, shape 100, tone 3 kHz), Digital (wrap, shape 50,
oversample off).

### 3.2 Bitcrush

The report: *"there are lots of types of bitcrush... configurable to get
lots of different sounds of bitcrush within that one crush plugin."* Bits,
rate, dither and anti-alias is one bitcrusher. What makes the *types* is
how the amplitude is rounded, how the time is held, and what the level is
when it happens.

**Sections and parameters:**

*Depth*
- `input` — −24..+24 dB into the quantiser. A quantiser is a level-dependent
  effect: a quiet signal at 4 bits is a gated crackle and a loud one is a
  square wave, and this knob is the difference.
- `bits` — 1–16, fractional. Unchanged.
- `quantiser` — chooser: **round** (to nearest, what it was), **truncate**
  (toward zero — small signals fall to silence, the 8-bit sample player's
  gating), **µ-law** (a logarithmic grid, so quiet detail survives and loud
  material crunches — telephony, and the old samplers that used companding).
- `dither` — chooser: **off**, **rectangular**, **triangular** (what the
  switch did), **shaped** (triangular with first-order error feedback,
  pushing the noise upward — the hi-fi one).

*Rate*
- `rate` — 200 Hz–48 kHz, log. Unchanged.
- `decimation` — chooser: **hold** (sample-and-hold, what it was),
  **linear** (a ramp between held values — a sampler with interpolation,
  smoother and darker), **drop** (the held value plays for one sample and
  silence for the rest — sparse, comb-like, very different).
- `jitter` — 0–100 %, random variation of the hold period. An unstable
  clock; at small amounts a tape-like smear, at large ones a broken machine.
- `antialias` — switch, off by default. Unchanged, for the reason written
  on it.

*Output*
- `post_lp` — 200 Hz–20 kHz, a low-pass after everything. The
  reconstruction filter of an old sampler's output stage; tames the
  aliasing after the fact, which is a different sound from preventing it.
- `output` — −24..+24 dB.
- `mix`.

**Presets**: 12-bit sampler (12 bits, round, 26 kHz, linear, post 14 kHz),
8-bit console (8 bits, truncate, 16 kHz, hold, input +6), Telephone (8 bits,
µ-law, 8 kHz, hold, anti-alias on, post 3.4 kHz), Broken clock (10 bits,
round, 12 kHz, hold, jitter 60), Sparse (16 bits, 6 kHz, drop, post 8 kHz),
Crunch (4 bits, truncate, input +12).

### 3.3 Gate / expander

The drum-cleanup tool, and the substrate the trance gate will be built on.
One effect for the gate and the expander because they are one machine at two
settings: a gate is an expander whose ratio is steep enough and whose range is
deep enough that "quieter" becomes "gone". Both of those are knobs, so the
useful settings between them — the expander that ducks spill by six decibels
and leaves the room in — are reachable rather than being a third entry in the
menu.

**Sections and parameters:**

*Detection*
- `threshold` — −80..0 dB. At the bottom of its range nothing audible is ever
  under it, which is this effect's "off".
- `hysteresis` — 0..24 dB. How much *quieter* than the opening threshold the
  signal has to get before the gate closes. Without it a signal sitting at the
  threshold crosses it dozens of times a second and the gate stutters.
- `key_hp` — 20 Hz–2 kHz, a high-pass on what the **detector** hears and not
  on the audio. The reason a kick in the overheads does not open the hi-hat's
  gate. 20 Hz is off.
- `lookahead` — 0–10 ms. The audio is delayed and the decision is not, so the
  gate is already open when the transient that opened it arrives. It is
  latency, and it is **compensated** since 2026-09-06: the graph holds every
  other track back to meet it, and the insert delays its own dry path so a mix
  below 100 % does not comb against it (TDD §5.5).

*Envelope*
- `attack`, `hold`, `release` — 0.05–100 ms, 0–500 ms, 5–5000 ms. Hold is what
  stops a decay being cut in half by its own first dip.

*Amount*
- `ratio` — 1:1 to 100:1, logarithmic. One is a wire at any threshold.
- `range` — −80..0 dB. The floor; −80 is a gate and −6 is an expander.
- `mix`.

**The detector is an envelope, not a rectifier.** A steady sine's rectified
value visits zero twice a cycle, so a gate written on `|x|` opens and closes
at the tone's own frequency and every measurement of its threshold comes out
wrong. The peak is held for 20 ms — one cycle of the lowest note anybody gates
on — and only then let go. That hold is under the `hold` knob rather than over
it, and it is why the gate takes about that long to notice a note has stopped.

**A fresh gate is a gate with its threshold at "never"**, which is a different
reading of rule 2 from the compressor's 1:1 and is deliberate: the knob a
person reaches for on a gate is the threshold, and a gate whose threshold does
nothing until a second knob has been found looks broken.

**What is not there yet:** the external key. The DSP takes a sidechain slice
already; what is missing is a way for an insert to name a track, which is §4's
second open item.

### 3.4 Utility

The plumbing, in one insert rather than seven. Every control here is a *fix* —
the take that came in six decibels hot, the side that was wired backwards, the
bass that will not stay in the middle — and a fix is something a person
reaches for once, applies, and stops thinking about. Seven entries in the menu
would make the commonest thing in mixing the fiddliest. It is first in
`EffectKind::ALL` for the same reason.

**Sections and parameters:** *Level* — `gain` (±24 dB), `pan`. *Stereo* —
`width` (0–200 %), `mono` (20–500 Hz, 20 is off), `swap`. *Channels* —
`mute_l`, `mute_r`, `invert_l`, `invert_r`. *Output* — `dc` (5–500 Hz, 5 is
off), `mix`.

Three decisions worth writing down:

- **Pan is a balance**, not a pan law. What arrives at an insert is already
  stereo, and a constant-power law would attenuate a centred signal by 3 dB
  the moment the effect was added — which is exactly what rule 2 forbids.
- **The order is the order a repair happens in**: mute, invert, swap, width,
  mono-maker, DC, pan, gain. The mute and polarity switches name the channel
  that *arrives*, and the swap happens after them. Width and the mono-maker
  turn out to commute (both act on the one side signal and one of them is a
  scalar), so the ordering that has teeth is invert-before-width — which is
  what makes width at 0 with one side flipped a null test.
- **A fresh utility is a wire sample for sample.** The mid/side stage is
  skipped entirely when the width is at 100 % and the mono-maker is off,
  because `(l+r)/2 + (l−r)/2` is only *nearly* `l` in floating point and a
  gain-staging tool has to be exact.

**No presets**, and that is a decision rather than an omission: its ten
controls are ten separate jobs, and a preset that wrote all of them would be a
preset for nobody. Recorded on `EffectConfig::presets` and held by
`whether_an_effect_ships_presets_is_a_decision_taken_for_every_one`.

### 3.5 Chorus / ensemble

The modulation necessity, and the cheapest big-sounding thing a sampled string
patch can have. Its modulated line is what the flanger and the vibrato will be
built from.

**Sections and parameters:** *Voices* — `voices` (1–4), `mode`
(chorus/ensemble), `spread`. *Modulation* — `rate`, `sync`, `division`,
`depth`, `delay` (5–30 ms). *Output* — `feedback` (**−90..+90 %**), `tone`,
`mix`.

- **Chorus** puts every voice on one LFO, spread evenly around its cycle;
  **ensemble** gives each its own at a rate that never comes back into step,
  which is what a 1970s string machine was. Two machines, not two presets.
- **Each voice also has its own centre**, spread across ±15 % of the delay
  knob. Spreading *n* voices around one LFO's cycle alone has a hole in it: at
  every instant the waveform takes the same value at two of those points — for
  a sine, at every zero crossing, and for an even *n*, at both ends of the
  sweep — two voices are reading the same place and one of them is doing
  nothing. Four taps at four delays is what a hardware ensemble did.
- **Depth is a proportion of the room there is**, not a number of
  milliseconds, so the two knobs together cannot produce a read pointer ahead
  of the write pointer.
- **Feedback is signed.** Positive is the resonant comb a flanger has;
  negative is the hollow one, with the teeth in the gaps. A different sound,
  and the reason it is not a percentage.
- Voices are summed and divided by their count, so more voices is never
  louder. It can be quieter — copies at different delays cancel in places, and
  that cancellation *is* the comb.
- It opens **half wet** and is `is_time_based`, because a fully wet chorus is
  a detuned copy with nothing to beat against, which is a vibrato.

### 3.6 Filter

The synthesiser filter as an insert, because a sampler has one per voice and a
mix wants one per bus. What makes it more than an EQ band is that its corner
**moves**, and the two things that move it are the two a synthesiser has.

**Sections and parameters:** *Filter* — `shape` (eight: LP/HP/BP at 12 and 24
dB, notch, peak), `cutoff` (20 Hz–20 kHz, opening at the top so a fresh one is
a wire), `resonance`, `drive`. *Envelope* — `env` (**−100..+100 %**),
`env_attack`, `env_release`. *LFO* — `lfo`, `lfo_rate`, `lfo_sync`,
`lfo_division`, `lfo_wave` (sine, triangle, saw up, saw down, square, sample &
hold). *Output* — `output`, `mix`.

- **The envelope amount is signed**, which is the half of an auto-wah nobody
  ships: a filter that *closes* as the signal gets loud is a duck with a tone.
  Both amounts are in octaves and they add, so the LFO sweeps around wherever
  the envelope has left the corner. Four octaves each at full.
- **The drive is before the filter**, which is where a synthesiser puts it and
  the only place it sounds like one: the drive makes harmonics and the filter
  sweeps through what it made. After the filter it would be a distortion with
  a tone control in front of it, which this program already has.
  `the_drive_is_before_the_filter` is the measurement that can tell the two
  apart.
- **Coefficients are rebuilt per sample only while something is moving them.**
  The SVF is zero-delay-feedback, which is exactly what per-sample modulation
  needs; with both amounts at zero the corner is built once a block and the
  effect costs what an EQ band costs. That is what makes it reasonable to
  reach for one on every bus.
- **The band-pass is normalised to unity at its centre.** The SVF's plain
  band-pass peaks at Q, which is right for a voice's filter and wrong for an
  insert on a mix bus, where a resonance knob carrying twenty-one decibels of
  gain is a knob nobody can turn past a third. The resonance still narrows the
  band, which is the wah.
- **Peak** is the one shape whose resonance knob is a *lift* rather than a Q,
  up to +24 dB — a bell that moves with the envelope and the LFO.
- **No key tracking.** A filter that follows the note needs a note; an insert
  on a bus does not have one, and the sampler's own filter does.

---

---

## 4. Build order

What gets written next, in order, and why that order. Each is one
`EffectKind`, one config with a spec table and sections, one `EffectState`
arm, one `fontelle-fx` module, and a test file that measures the claim.

1. ~~**Utility**~~ — done 2026-09-02 (§3.4). The metering stubs that shared
   its file moved to `fontelle-fx/src/meters.rs`.
2. ~~**Gate / expander**~~ — done 2026-09-02 (§3.3), less the external key.
3. ~~**Chorus / ensemble**~~ — done 2026-09-02 (§3.5). Its modulated line is
   now the flanger's and the vibrato's to reuse.
4. ~~**Filter**~~ — done 2026-09-02 (§3.6).
5. **Limiter as an insert** — P0, an afternoon: the DSP exists.
6. **Compressor extensions** — P0: sidechain high-pass, lookahead, link,
   character. External key waits on sends reaching `EffectNode`.
7. **Delay and reverb extensions** — P0: dual time, loop high-pass,
   ducking, tape mode; diffusion, modulation, wet EQ, algorithms.
8. **Multiband compressor**, **de-esser**, **clipper**, **transient
   shaper** — P1, in that order.
9. **Flanger**, **phaser**, **tremolo/auto-pan** — P1, cheap once the
   chorus exists.
10. **Tape**, **ring modulator**, **stereo imager**, **loudness meter** —
    P1.
11. Everything P2, as wanted.

Two things gate more than one row above and are worth doing early:

- ~~**A preset picker in the effect window.**~~ Done 2026-09-02. The
  generic panel draws a row of chips above the first heading, built from
  `EffectConfig::presets()` the same way the knobs are built from `specs()`;
  clicking one runs `fontelle_model::SetInsertPreset`, which is **one** entry
  on the history because choosing a preset is one thing a person did. Soften's
  four, the distortion's seven and the bitcrush's six are all reachable now.
  Which effects ship presets is a decision recorded per effect on
  `EffectConfig::presets` and held by a test, so a new effect cannot land in
  the "none" list by accident. Still owed presets: the EQ, the compressor,
  the chorus, the delay, the reverb.
- ~~**External sidechain into `EffectNode`.**~~ Done 2026-09-02. See §6.

---

## 5. How to add one

The recipe the last five followed, so the next one takes an afternoon of
plumbing and the rest of the day on the sound:

1. **Config in `fontelle-types/src/effect.rs`**: the struct, `new()` as a
   wire, `get`/`set` by id, the `static` spec table (through `with_mix`),
   `sections()` if there are more than eight controls, a variant on
   `EffectKind` and `EffectConfig`, a row in `EffectKind::ALL`. Presets as
   constructors. Every enum the config carries gets a `label()`, an `ALL`,
   and a `static` position table.
2. **Tests first, in `fontelle-types/tests/parameters.rs`** the generic
   ones already cover it; add a named test only for a rule specific to the
   effect (a chooser's order, a unit).
3. **Tests first, in `fontelle-fx/tests/<effect>.rs`**: one per claim the
   effect makes, measured. Confirm they fail against `todo!()`.
4. **DSP in `fontelle-fx/src/<effect>.rs`**: `new`, `prepare` (the only
   place that allocates), `reset`, `process(&mut [&mut [f32]], &Config[,
   bpm])`. Reads the config every block; keeps only state.
5. **One arm in `fontelle-engine/src/nodes.rs`** for `new`, `prepare`,
   `process`, `reset`. `fontelle-engine/tests/effects.rs` covers the rest
   generically the moment the kind is in `ALL`.
6. **No UI change.** `effect_view` draws the spec table; `sections()` puts
   headings in it. The storage test round-trips it. The menu lists it.
7. **A section in `PROGRESS.md`** saying what was measured and what was cut.

---

## 6. The external sidechain

An insert's detector, listening to another track's bus
(TDD §13.4's "sidechain input from any mixer track"). Built 2026-09-02, and
what unblocks the ducker, the compressor's character work and the vocoder.

**The document.** `EffectSlot.key: Option<MixerTrackId>` — a field on the
slot, not a parameter in the config, and that is forced rather than chosen: a
`ParamSpec` is a float with a fixed range and a permanent id (INVARIANT 7),
and a track is neither. A "key" knob stepping through whatever tracks happened
to exist would be an automation lane pointing at a different track after a
rename, which is the second addressing scheme §8.2 forbids. `None` on every
effect with no detector, and on every project written before the field.

**It is a routing edge**, and every rule a send's target follows applies to it.
`Mixer::has_cycle` counts it, so a key that closed a loop is refused before it
reaches the compiler; `SetInsertKey` also refuses a key on an effect with no
detector and a track keying an insert on itself, which is what "no key"
already means. The edge points the way the *signal* flows — the named track
feeds the track the insert sits on — and `Mixer::key_listeners` is the one
place that reversal is written, because getting it backwards is a cycle check
that passes a loop and a schedule that reads the key one block late.

**The graph.** `depth_to_master` counts key edges, so a keyed track is
scheduled after its source. A `KeyTapNode` on the source's bus, alongside the
post-fader sends and for the same reason — what a sidechain should hear is
what the track sends to the speakers — copies the block into a `KeyTap`, and
the insert's `EffectNode` reads it out.

**Why a tap rather than a second input on the node.** The graph's dispatch is
deliberately narrow: a node processes its bus in place or routes one bus to
another of the same width, at most two buffers a side. A key is a third set —
a bus this node reads and does not write — and widening that dispatch would
put a new shape into the hottest loop in the engine for one feature. Both ends
are the audio thread in one pass, in a guaranteed order, so the tap needs no
ring and no synchronisation beyond what an `Arc` implies.

**The window.** A row of chips under the presets, on the generic panel: "no
key" and then every strip, the chosen one filled. Same shape as the preset
row and the same `chip_row` laying both out. Only on an effect whose
`EffectKind::takes_key` says it has a detector, which is the compressor and
the gate.

**What is still open.** The ducker (§2.1) — a compressor with its knobs
relabelled and its key exposed — and the vocoder's carrier are now unblocked
and unwritten. A key on a track whose insert chain has latency now
arrives compensated like everything else (TDD §5.5); what a *key tap* reads is
still the source track's bus at the point the tap sits, which is what a
sidechain should hear.
