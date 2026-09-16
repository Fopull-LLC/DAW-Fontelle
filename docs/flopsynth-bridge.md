# The bridge, and the piano — an experiment (2026-09-16)

> *"the default grand piano flopsynth sound really does not sound like a
> real grand piano, still ... i think those have features for dragging an
> audio file in and using the waveform of that as your oscilator ... improve
> the spacey scifi visual design of the flopsynth instrument to make it
> resemble a futuristic spaceship interior looking out through the window
> at the stars ... this should not be pushed or released or commited yet
> but instead be saved as an experiment for now for me to test."* — Ty,
> 2026-09-15

**Shipped as `v0.6.0`** on 2026-09-16, after Ty tested it as an
uncommitted experiment through the day (*"sounds great go ahead and
release this update"*). Every piece was built tests-first and the suites
are green — the test files are the proof of each claim, and they are named
below.

---

## 1. Why the piano could not be tuned into a piano

Three rounds of voicing the Grand Piano against a sampled grand got its
partial *levels* and *decays* right and it still read as an electric piano.
`crates/fontelle-core/examples/piano_partials.rs` found why on 2026-09-09
and the memory note said not to accept a fourth round of tweaking: **a
wavetable is periodic, so its partials sit at exact multiples of the
fundamental**, while a real string is stiff and its nth partial sits at
`n·f0·sqrt(1 + B·n²)` — the twelfth partial of middle C is **48 cents
sharp** on the reference and 0 on the table. That stretch is why tuners
stretch-tune, and no level, filter or envelope can move a partial.

So the fix is two new kinds of oscillator, not a preset:

### `String` — a stiff string (`fontelle_dsp::SynthSource::String`)

A bank of up to 64 partials placed where a stiff string puts them, each a
rotating phasor decaying at its own rate. Four knobs on the card:

| knob | what it is |
| --- | --- |
| **stiff** | the inharmonicity `B`, at middle C; the model scales it up the keyboard the way a piano's strings do (tenfold from the middle to the top). The default is a grand's own 4e-4. |
| **damp** | how much faster the high partials die than the low — a string's losses grow with frequency, so its top goes first and the note darkens as it rings. |
| **strike** | where the hammer lands, as a fraction of the string. A partial with a node under the hammer is not excited: at 1/8 the eighth is missing. |
| **ring** | how long the fundamental rings at middle C; longer in the bass, shorter at the top. |

The **bright** knob (the oscillator's position — the same address, so the
same velocity route) is the strike's hardness: the spectral tilt and the
felt's low-pass corner, five octaves over the knob. Unison works on it —
three strings a cent apart *is* a piano's unison, and the beating is the
two-slope decay a piano has. The window draws its partials as bars over a
harmonic grid, so the stretch is visible.

Cost: 1.6 % of a core per voice (`benches/flopsynth.rs`, "grand piano/1
voice"), level with Supersaw — the loop is vectorised eight partials at a
time and partials the felt has silenced are not rung.

Tests: `fontelle-dsp/tests/synth_string.rs` (the physics),
`fontelle-core/tests/grand_piano.rs` (the piano, including the new
`the_partials_are_stretched_the_way_a_stiff_strings_are`).

### `Sample` — a recording (`fontelle_dsp::SynthSource::Sample`)

A whole recording played across the keyboard through the same oscillator:
the same unison stack, the same FM/RM warp, the same routes. **The samples
live in the patch** (`Patch::samples`, 16-bit base64 in the file like the
wavetables), so a preset made from a recording opens on any machine and
never needs relinking. A recording is several **zones** — one per file
when a folder is dropped — each with a root key and the keys it serves,
which is what a sampled piano is. The card's knobs: **start** (the
position), **loop** Once/Loop, **loop in**, **loop out**.

The root key comes from the file's **name** when it says (`A4v8.wav`,
`piano_C#3.wav`, `Db5-soft.wav` — how every library names its notes) and
from the **sound** when it does not (the YIN tracker, median over the first
second and a half); a recording that is neither lands on middle C.

Tests: `fontelle-dsp/tests/synth_sample.rs`, `fontelle-core/tests/
user_sample.rs`, `fontelle-app/tests/flopsynth_sample.rs`,
`fontelle-app/src/sampling.rs`'s unit tests.

### The Grand Piano — a recording, in the end

> *"the grand piano still sounds just a lot like a basic synth wave and
> not a actual grand piano ... maybe you could use the osc sampling
> feature to make the piano sound more realistic if you can find a grand
> piano one shot to use."* — Ty, 2026-09-16, having heard the string.

So the fourth round is the one Ty asked for. The **Grand Piano row plays
a sampled grand**: forty-six recordings of a Yamaha C5 — the
[Salamander Grand Piano](https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html)
by Alexander Holm, CC BY 3.0; `assets/flopsynth/samples/grand/README.md`
is the credit and the README carries it too — cut out of the FreePats
SF2 by `fontelle-app/examples/grand_samples.rs` through Fontelle's own
sampler. The bottom A and every four semitones from C1 to C8 (so middle
C is a recording, and nothing is more than two semitones transposed), at
velocity 40 and at 120, 1.3 s at the top to 4 s in the bass, 24 or 32 kHz
mono: 6.5 MB, compiled into the binary as `fontelle_core::factory_samples`.

- **Two layers, crossfaded by velocity.** A soft strike is not a quiet
  hard one — its spectrum is different — so the soft set is on OSC A and
  the hard on OSC B, and velocity slides between them. The two routes
  (soft −12 dB on the way up; hard from −24 to +6) are chosen against
  each other so the sum never dips mid-way, and the voice's own velocity
  curve — a soundfont's, the square — is leaned against from the bottom
  so pp to ff is thirty-odd decibels, not seventy.
- **Nothing synthesised beside them**, and no envelope of the row's own:
  the recording's decay is the decay, the release is the damper (a tenth
  of a second at the top, three in the bass). A note held past its
  recording fades where the recording was cut.
- **Named, not carried.** `UserSample::factory` says which set a patch
  plays; the file stores the name, and this build decodes the set once
  and shares it. A project with four pianos holds one; the shipped preset
  is a page of JSON like the others. A file from a later build naming a
  set this one has not got reads as a silent oscillator, not a broken
  patch.
- **The card's menu offers the sets.** Right-click any oscillator's
  picture: *Grand (soft)* and *Grand (hard)* come before the audio
  folder, so the piano can be layered under a string or a pad and
  modulated like any recording, with no file to find.

`tests/grand_piano.rs` is rewritten for a piano that *is* one: the
recordings are what sounds; every key lands on a recording near its own
pitch; a soft touch is darker than a hard one and the level rises with
velocity all the way; the bass rings long and the top short; a note that
outlives its recording fades rather than stops; a release is a damper,
not a cut; the partials are stretched (the C5's own numbers — a big
piano's long strings are less stiff than a baby grand's, so the twelfth
partial of middle C is twenty cents sharp, not fifty); a project names
the piano. `tests/factory_samples.rs` holds the sets themselves and the
naming.

The string piano did not go: it is the bank's **Modelled Piano** now,
with the knobs a recording has not got.

#### And then the mechanics

> *"much better however i feel like it could still be made more realistic.
> it sounds like a soundfont with reverb right now pretty much. i want
> you to use the features in the synth to take this sampled sound and
> then turn it into a really tactile realistic feeling piano instrument
> preset."* — Ty, 2026-09-16

A soundfont is a recording played back. What the row has around the
recordings now is what a body does, each held by a test that measures
the *difference the layer makes* (mute it, render again, compare):

- **The keyboard lies across the stereo field.** Every layer sits left
  of centre and slides right with the key (`Key → LayerPan`): the bass on
  the left, the treble on the right, as from the bench, middle C in the
  middle.
- **The hammer is felt on a hard strike.** The noise layer, dark through
  its own low-pass, gated by env 2 for the ten-to-forty milliseconds
  where the recording's own hammer lands, on a velocity route through
  the *squared* curve — so it falls away faster than the note does and a
  pianissimo is not all thud.
- **The damper is heard on letting go.** The same noise reads
  `(1 − env 3) × env 1`: env 3 drops the instant the key comes up, and
  env 1 — full while the key is down, leaving over 150 ms after — is
  what keeps the felt at nothing on the note's first block, where every
  envelope reads zero and `1 − 0` alone was a burst at every strike.
- **The strings sing on.** The stiff-string source at the note, fading
  in over env 1 so the strike is the recording's, some fourteen decibels
  under it and ringing on its own decay — its partials stretched by a
  different stiffness than the C5's, so the two beat slowly against each
  other the way a piano's three strings do. From a second in it is most
  of the ring, which is what keeps a held chord alive past the
  recordings' cut.
- **Under the lid, not in a hall.** The reverb is small, short and dark,
  six milliseconds off the strike so the hammer is dry. The release is a
  fifth of a second at the top and half in the bass.

Two things the matrix taught on the way: it reads an envelope **once a
block** (ten milliseconds at 512 samples), so a gate shorter than that is
not heard; and every envelope reads **zero on the first block** by
design, so an inverted envelope route needs a via (`Build::inverted_via`).

Two traps met on the way, for whoever is next: `flopsynth_init`'s
oscillators start at **position 0.5**, which on a recording is a note
started halfway through (the builder and both load paths zero it), and a
new `#[serde(default)]` field on the patch has to `skip_serializing_if`
its default too, or the export tool rewrites all five hundred preset
files the day it is added.

To hear it: `cargo run --release -- --play-flopsynth "Grand Piano"` (and
`"Modelled Piano"` for the string). `examples/piano_probe` still compares
either against the SGM grand on this disk.

---

## 2. Getting a sound onto an oscillator

Three ways, all landing on `Session::load_sound`:

1. **A file dragged from the desktop** onto an oscillator card (this
   worked before; it made a wavetable). Now it makes a **recording** unless
   the file is shaped like a wavetable — a whole number of 2048-sample
   cycles, which is what Serum exports and what nothing recorded ever is.
   A **folder** dropped makes a multi-sample.
2. **A row dragged out of the Import tab** onto a card — the thing that
   *"didnt do anything"*, and then *"gets stuck inside the main daw
   window"*. The in-window carry had no oscillator target
   (`canvas/carry.rs`'s `CarryTarget::Oscillator` now; the card lights and
   the chip says *As OSC A's sound*). The sticking is the desktop's
   doing: a press grabs the pointer for the window it happened in, and on
   Wayland the grab holds until the button comes up, so the synth window
   never hears the drag and the studio hears a release *outside its own
   bounds* — which meant "nowhere". Now (`canvas::carry_release`) a row
   let go outside the studio while the synth window is open is **held**:
   the chip stays on the pointer and follows it into the synth window,
   the card under it lights, and the next click puts it down — *Click
   where it goes · Esc lets go*. Verified on the nested X server with the
   two windows side by side, which has the same grab. The name field's
   preset drop rides the same path.
3. **The card's own menu**: right-click an oscillator's picture (or click
   an empty sample card) and pick from the Import tab's audio folder —
   or the bank's own sampled grand, listed first. This one needs no drag
   at all.

Dropping on an oscillator that already plays a recording replaces it;
dropping on one that *shares* a recording with another oscillator gives it
its own. Switching a card to **Sample** from its nameplate plays the
patch's first recording until one is dropped on it — layering one sound
twice with different settings is the point of three oscillators.

Every drop is one undo entry.

---

## 3. The bridge

`crates/fontelle-ui/src/render/bridge.rs` draws the window as a room:

- **The hull** — plated metal under everything: graded, brushed, seamed,
  riveted along the dash.
- **The canopy** — the window onto the sky, under the tabs and over the
  consoles, on every page. As tall as the consoles leave it, 84–240 px;
  the sky gives before the controls do, so at the minimum window size it
  is a slit. The page tabs float on it as a head-up display.
- **The consoles** — each card a module set into the hull: a bevelled
  recess, a nameplate with the card's **kind** chooser on it (what the
  module *is*; moving it there off the grid bought every oscillator back a
  row), a status lamp in the family's ink that brightens while a control
  in it is held, the picture on an inset screen with a raster, knobs with
  a scale of tick marks.

**The window opens at 1180×840 now**, not 740: at 740 the consoles had to
shrink to make room for the least canopy. Ty decided 740 for the first
build (`docs/flopsynth-plan.md` §14); 840 is the experiment's number and
his to keep or send back (`layout::FLOPSYNTH_SIZE`).

### The sky (`crates/fontelle-ui/src/sky.rs`)

Not a fragment shader: `draw_window` is a pure function of its inputs so a
frame can be rendered with no GPU and no window, and a shader pass under
the scene would break that for one window. The nebula is shaded **on the
CPU at a quarter of the canopy's size** — domain-warped noise, a spiral
galaxy laid over it, a few thousand pixels a frame — and scaled up
bilinear; the stars, three planets (one ringed), the shooting stars and
the aurora are vector sprites at full resolution.

It listens. The instrument's node has a scope (`SamplerNode::with_scope`,
the same ring the EQ's analyser reads); the window is handed the analyser's
bands and a stretch of waveform once a frame (`DocumentHost::
instrument_sound`) and eases the sky towards it:

- the **level** lights the clouds and speeds the clock;
- **bass** warms the palette and swells the galaxy's core, **air** cools it
  and lights the arms; the **centre** of the energy leans the two cloud
  inks (the theme's accent and modulation colours) apart;
- the **mid** band is the warp that folds the clouds;
- energy integrates into the galaxy's **spin**;
- a **transient** throws a shooting star, two for a big one; a held note
  throws none;
- the **aurora** across the lower third *is* the waveform, brought up to
  height whatever the preset's level.

Silence is a still sky with the stars twinkling. The window redraws at
thirty a second while the sky moves and sleeps four seconds after the last
sound. `tests/sky.rs` holds all of it, and that the same state draws the
same sky.

### Skins (`crates/fontelle-ui/src/skin.rs`)

`assets/flopsynth/skin/README.md` names four PNGs — `hull.png`,
`glass.png`, `knob.png`, `console.png` — read from that folder (or
`~/.local/share/fontelle/skin/`, or `FONTELLE_SKIN_DIR`) when the studio
starts, never compiled in. Each replaces the procedural surface under it;
none is required. The README says where CC0 sets are.

### Looking at it

The headless shot: `FONTELLE_UI_DUMP=<dir> cargo test -p fontelle-ui --test render_headless flopsynth`
(a still sky, two cards). The real thing: `cargo run --release`, open
the Grand Piano's window, play. On the nested X server the sky needs a
note actually sounding to react — hold a key in the roll's key column.

---

## 4. What I would take next, and what is Ty's

- **Listen.** The Grand Piano is a recording with a body around it now.
  The knobs to reach for if a part of the body is too much or too little:
  the hammer is env 2's route on the noise layer (0.62), the damper is
  the `inverted_via` route (0.9), the singing string is layer C's gain
  (−53) and ring (0.9 s), the width is the four `LayerPan` routes (1.15).
  If the velocity feel is off, the two `LayerGain` routes and the
  inverted `Amp` route are the whole of it. A third velocity layer or
  denser keys are a change to `grand_samples.rs`'s two tables and the
  megabytes. The sustain pedal is the MIDI router's (CC 64 holds the
  note-offs), so it already works on this row.
- **The window size** (840) and **the canopy** (84–240) — Ty's numbers to
  confirm.
- **The held row** (§2) — try it on KWin: drag out, let go over the
  synth window, click a card. If the chip does not appear in the synth
  window after the release, the synth window is not getting pointer
  motion after the grab ends, and that is the next thing to look at.
- **Sample-and-hold ideas not built:** a loop's crossfade is a fixed
  5 ms; there is no reverse, no per-zone velocity layers, no auto-loop
  detection. `UserSample` has the zones for velocity layers already
  (`vel_range` would be the one field to add).
- **The Presets page** keeps its full list and gives the canopy the
  least; the Modulation page's matrix sits under a canopy too.
- **The light theme** has not been opened on the bridge. Everything is
  palette-mixed, but nobody has looked.
- **Extra layers**: `SynthSource::String` and `Sample` on the SUB card work
  (the SUB's kind chooser is on its nameplate); the NOISE card stays noise.
