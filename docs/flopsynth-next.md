# Flopsynth II — the plan to stand beside Serum 2 and Omnisphere (2026-09-17)

> *"i still feel like our flopsynth doesnt stack up to professional high
> quality synths like omnisphere and serum both in terms of visual
> polish/quality/design, as well as features. please analyze how we could
> expand flopsynth thoroughly and put together a in depth plan for another
> agent to follow through on ... youre free to harshly critique the current
> work adversarially."* — Ty, 2026-09-17

This is that plan. It is written for the agent who builds it, not for Ty,
and it assumes you have read `CLAUDE.md`, `PROGRESS.md`'s top two entries,
`docs/handoff.md`, and `docs/flopsynth-plan.md` §0–2, §8, §12 and §14 (the
original design, its principles and Ty's decisions). Everything in those
that this plan does not explicitly revisit **still stands**, and the
INVARIANTs in `FONTELLE_TDD.md` are hard.

The critique in §1 is deliberately unkind. Every claim in it was checked on
2026-09-17 against the source (file:line given) or against the real window
on the nested `Xwayland :99` at v0.9.0 (`target/release/fontelle
--no-menu`, the Grand Piano's window opened from the rack row's edit icon).
Nothing in it is a guess; if you find one, fix the plan.

---

## 0. Ground rules for this work

1. **Tests first, confirmed failing**, then the implementation — every step
   below names its test file. `PROGRESS.md` says why at the top.
2. **Look at every visual change** on `:99` before believing it, and grab
   twice (`docs/handoff.md` §5, memory `seeing-fontelles-gui`). Half the
   defects in §1 have a green test suite behind them; none could have been
   found any other way.
3. **Never `cargo fmt --all`**; format only files you edited with
   `rustfmt --edition 2024 <file>`. `cargo clippy --workspace --all-targets
   -- -D warnings` is part of the bar.
4. **Never re-trim the existing bank** (memory `flopsynth-preset-bank`).
   When a change moves a preset's loudness, that is a change to the preset,
   and it goes through `examples/preset_probe.rs` for the rows it touched.
5. **Never touch the addresses** (INVARIANT 7). Every new control is a new
   address; an existing address keeps its meaning. A chooser that grows a
   position grows it at the *end* (`synth/sample/loop` already taught this).
6. **Keep the per-revision cost flat.** Anything added to `refresh_studio`'s
   lists has to be a fraction of a millisecond
   (`FONTELLE_TRACE_FRAME=1`'s `arrange edit` line is the check — the drag
   stutter of v0.9.0 was the knob marks).
7. **New `#[serde(default)]` patch fields must `skip_serializing_if`** their
   default, or the export tool rewrites five hundred preset files.
8. **Publishing is Ty's** (a release, an announcement). Prepare, stop.
9. **Ty's decisions of 2026-09-06** (plan §14) are not re-litigated by you.
   Where this plan thinks one should be revisited it says so in §9 and
   leaves the choice to him.

---

## 1. The verdict — what is actually wrong

The engine is more capable than the window lets anyone discover, and the
window is more decorated than it is designed. Against Serum 2 the gap is
mostly *interaction and legibility*; against Omnisphere it is *breadth of
sound and the browser*. Against both it is *finish*: the thousand small
things that make a professional instrument feel like one, and that no test
here measures.

### 1.1 The window does not read as an instrument

Evidence: the Synth page at 1180×840 (grabbed 2026-09-17, dark theme).

- **There is no visual hierarchy.** One typeface, one weight, two sizes
  (13 px and 11 px — `theme/mod.rs:257-265`, `text/mod.rs:205`). A card's
  name, a knob's caption, a knob's value and a tab label are all the same
  grey-blue on the same dark teal. Eighty-odd controls on the Synth page
  and nothing tells the eye where to start. Serum's oscillator section
  reads in one glance because *WT POS* is not styled like *OSC A*.
- **The knobs are 24 px in 52×54 cells** (`FLOP_CELL_W/H/FLOP_KNOB`,
  `render/mod.rs:51-54`). At that size the value arc is a hairline, the
  needle is two pixels, and a 0 % and a 12 % knob look the same from a
  metre away. Serum's main knobs are ~44 px; Vital's ~40. Ours are the
  size of Serum's *fine* knobs, and every one of ours is that size, so
  cutoff is as small as fine-tune.
- **The pictures are 48 px tall** (floor 26). The filter response is a
  line in a slot; the envelope is three straight segments (the a/d/r shape
  knobs *exist* but `env_curve_points` draws straight lines,
  `canvas/flopsynth.rs:1141-1179`, so the picture lies about the curve —
  the exact thing plan §8.1(5) forbade). The wavetable picture is one
  cycle of one frame; there is no frame stack, no 3D, no position strip.
  A user cannot see what the *position* knob does to the table until they
  turn it.
- **The canopy takes 150–270 px of every page** (`CANOPY_MIN/MAX` 84/240,
  `canvas/flopsynth.rs:213`) and gives nothing back. It is a low-resolution
  nebula shaded on the CPU, three clip-art planets and a straight line,
  and it reacts only when a note is sounding. On the Modulation, Effects
  and Presets pages it is 270 px tall — a third of the window — above
  content that is cut off (§1.4). The "spaceship bridge" was Ty's ask and
  a good instinct (Serum 2 and Omnisphere both have a signature look); the
  execution is a picture *over* the instrument, not *of* it. It should
  become the instrument's eyes: the oscilloscope, the spectrum, the table.
- **The light theme is broken on this window.** Grabbed with `--light`: a
  purple smear on grey, the *Synth* tab label and the voice read-out white
  on white, the pictures grey on grey, the canopy frame a white outline.
  `docs/flopsynth-bridge.md` §4 said "nobody has looked". Now somebody has.
- **Uniform density, no rhythm.** Every card is packed to its border; the
  right third of the bottom band (under Macros, 1000–1170 × 640–830) is
  empty while the OSC cards wrap captions onto three rows. The Effects page
  is one card in the top-left corner and 90 % hull.
- **Labels are cut and collide.** "Bypas" (route chooser), "Onc", "Clea",
  "24 d", "Byp", "mod fro", "non", "Hardne:macro 3" at the minimum size
  (980×620, grabbed); "release a shaped shaper shape" on ENV 3/4 on the
  Modulation page at the *design* size. `WIDE_CHOICE = 9` counts characters
  (`cell_span`, `canvas/flopsynth.rs:457-476`); it should measure text.
  Known and open since v0.7.0 (`docs/flopsynth-sampling.md` §5).
- **Captions say what the address is, not what the knob does.** "mod
  from", "amount", "a shape", "pos", "key trk". Serum says *WT POS*, *FM
  FROM B*, *ATK CURVE*. Words are part of the design.
- **The chooser menus are text lists.** Forty wavetables in two columns of
  names with no waveform beside them (grabbed). Serum draws every table's
  frame in its menu; Vital draws the LFO shapes. Nobody knows what
  "Bitwave" or "Grit" is from the word.
- **Nothing eases.** Hover tints a cell; the lamp brightens while held;
  that is the entire motion vocabulary outside the sky (`sky.rs:258-270`
  is the only easing in the window). A knob jumps, a page cuts, a menu
  pops. Vital's knobs animate their arc; Serum's mod rings breathe when
  their source moves. Ours draws a dot on the LFO picture.

### 1.2 The window does not behave like an instrument

Every one of these was tried on `:99` or grep-verified
(`fontelle-ui/src/app.rs`, `canvas/flopsynth.rs`):

- **No hover value bubble**, no tooltip, no help — `FlopsynthChrome` has no
  tip field (`render/mod.rs:7737-7769`); the studio's `tooltip` is the
  studio's. The value under the knob is always drawn instead, in 11 px.
- **No double-click reset**; `press_flopsynth` (`app.rs:8005-8100`) never
  reads `double_click`. No `default_value` for a parameter anywhere.
- **No typed values.** `TextEntry` is used for tempo, searches and renames
  (`app.rs:1273-1394`) and nothing else. The comment at `app.rs:8224` says
  a control is "dragged, arrow-nudged or typed into"; neither of the last
  two exists for a synth parameter.
- **The right-click menu on a knob is two lines:** the greyed name and
  *Create automation clip* (`app.rs:11289-11294`) — and it is drawn
  translucent over the neighbouring controls and over the knob's own value
  (grabbed). No reset, no *remove modulation*, no *assign to macro*, no
  MIDI learn, no copy/paste.
- **Shift is the only modifier** (6× fine, `app.rs:8577`); no Ctrl for
  finer, no Alt for reset, no wheel (deliberately — "the wheel only
  scrolls", `app.rs:4327`; that rule is right for the arrangement and
  wrong for a knob under the pointer with a modifier held).
- **Modulation can only be assigned from the Modulation page**
  (`badge_at`, `press_flopsynth` 8021). A user on the Synth page looking at
  the cutoff knob cannot put an LFO on it without changing pages, and the
  page they change to hides the knob.
- **One ring, one colour, newest route only.** The ring on a knob edits
  the newest route (`route_depth`, `app.rs:8287-8296`) and nothing says so.
  Serum stacks a ring per source in the source's colour; Vital too.
- **The LFO picture is not editable** (`app.rs:8355-8357`, explicitly a
  no-op) — the single most-used editor in Serum and Vital, a draw-your-own
  shape with grid and tension, is a ten-item chooser here.
- **The matrix is a read-only list.** Rows cannot be added, reordered, or
  have their source/destination/curve/via changed from the table
  (`matrix_rows`, `canvas/flopsynth.rs:985-1006`); `via`, `curve` and
  `invert` exist on `ModRoute` (`mod_matrix.rs`) and *no UI sets them*.
- **Effect slots cannot be reordered.** `FlopsynthHit::Header` is returned
  (`canvas/flopsynth.rs:1072`) and never matched in `app.rs` — a dead
  affordance since the first build (plan §8.5 still open).
- **Macros cannot be named** from the window; captions come from
  `patch.macros[i].name` and nothing sets it — the Grand Piano's third and
  fourth read "macro 3" and "macro 4" (grabbed).
- **No undo affordance in the window** and no "back to the loaded preset";
  no A/B; no init; no randomise; no audition. A preset loads on a single
  click with no way to hear it first.
- **The search box is a toy**: characters, Backspace, Space, Escape, Enter
  (`flop_search_key`, `app.rs:8184-8219`). No arrows, no paste, no caret.
- **No computer-keyboard piano**, no on-screen keys, no way to make a
  sound from the window itself. Every other synth on earth has one.
- **No UI scale.** The only response to a smaller window is the shrink
  cascade (`canvas/flopsynth.rs:702-728`) which produces §1.1's clipped
  captions; the only response to a larger one is air.

### 1.3 The engine is deep in places and shallow in the ones users notice

Inventory verified against `fontelle-dsp/src/synth_osc.rs`,
`synth_filter.rs`, `fontelle-core/src/voice.rs`, `mod_matrix.rs`,
`flopsynth/mod.rs` — see §2 for the full table.

Present and good: five sources (table, user table, recording with five
read modes and phase-locked grains, noise, stiff string); FM/RM between
layers; hard sync; eight-voice unison with blend/width; four filter models
(SVF with seven shapes, ladder with drive and self-oscillation, formant,
comb) and per-layer routing; DAHDSR with shape knobs; four LFOs with
sync/retrigger/one-shot/smooth; a twelve-source, twenty-destination matrix
with via/curve/invert; four macros; 388 presets held by measured gates.
None of that is what a Serum user reaches for first and finds missing:

- **Aliasing.** No oversampling anywhere in the synth path
  (`grep -rni oversampl` hits only `fontelle-fx/src/distortion.rs`). Tables
  are mip-mapped (`wavetable.rs:11-53`), which covers the linear case, but
  FM, hard sync, Quantise, Bend/Mirror and the ladder's `tanh` alias at
  the top of the keyboard. Plan §12 deferred this "on evidence"; the
  evidence is a bank with FM leads in it. Serum's whole reputation is
  "clean at the top".
- **Modulators are few.** 4 LFOs, 4 envelopes (amp + 3, `MAX_MOD_ENVELOPES`
  `voice.rs:169`), 4 macros. Serum 2 has 10 LFOs and 4 envelopes; Vital 8
  LFOs, 6 envelopes, 4 randoms; Omnisphere 8 LFOs, 12 envelopes, 48
  matrix slots. And ours run at **block rate** — 375 Hz at 128 frames
  (`voice.rs:1129-1139`) — so a fast LFO on pitch or a short envelope on a
  gain is stepped (the piano's hammer gate had to be lengthened to be heard
  at all: `docs/flopsynth-bridge.md` §1). Only the amp envelope is
  per-sample; cutoff is ramped and rebuilt every 8 samples.
- **LFOs cannot modulate LFOs** (`voice.rs:1153-1157` reads zero by
  design); envelopes cannot loop; there is no chaos/Lorenz source, no
  per-voice random *walk* (only one draw per note), no envelope follower,
  no step sequencer source.
- **Effects are eight** (`PATCH_FX_KINDS`, `flopsynth/mod.rs:106-115`:
  Chorus, Delay, Reverb, Filter, EQ, Distortion, Bitcrush, Compressor) in
  **four slots**, with no phaser, flanger, wavefolder, frequency shifter,
  hyper/dimension, or multiband anything, and **no effect parameter is
  modulatable** from the matrix (plan §12 deferred `ModDest::FxParam`).
  Serum 2 has three FX buses, mid/side and multiband; Omnisphere lists 93
  units.
- **Warp modes are seven** (`WarpMode`, `synth_osc.rs:91-124`). Serum has
  ~20 per oscillator plus a second warp; there is no phase distortion, no
  formant shift, no asymmetric bend, no flip/remap, no "FM from noise".
- **Unison is one shape**: voices/detune/blend/width. No stack modes
  (octave, fifth, chord), no per-voice tuning curve, no per-voice phase
  spread choice, and the noise source has no unison at all.
- **No spectral oscillator** (harmonic resynthesis of a recording) — the
  `String` source's 64-partial bank *is* an additive engine already
  (`MAX_PARTIALS`, `synth_osc.rs:402`); it only lacks a way to feed it an
  analysed spectrum instead of a stiffness formula. This is the cheapest
  large feature in the plan.
- **No multisample import** (SFZ). `UserSample` has zones with key ranges
  (`patch.rs:294-367`); it lacks velocity layers (`vel_range`, noted open
  in `flopsynth-bridge.md` §4) and a reader for the one format every
  library ships in.
- **No wavetable editor** — import by drop only (`Session::load_wavetable`,
  `session.rs:3836-3900`, 64 frames max). No draw, no formula, no FFT
  bins, no morph between imported frames, no export.
- **Voice stealing is one policy** wearing three names: `Quietest` and
  `LowestPriority` fall through to `Oldest` (`voice.rs:1830`).
- **Sample read interpolation is hard-coded `Normal`** (`synth_osc.rs:1018,
  1023`) and ignores the session's quality setting.
- **Velocity is the SF2 square law, fixed** (`velocity_to_gain`,
  `voice.rs:147`); no curve, no per-layer velocity window.
- **No MPE, no poly aftertouch** — `Performance.aftertouch` is channel
  pressure; `mod_x`/`mod_y` are captured at note-on and never move.
- **No arpeggiator or sequencer in the patch.** Plan §12 forbids one on
  purpose (the roll's arpeggiate command is *the* arpeggiator, TDD §16.5).
  Serum 2 shipped a clip sequencer and an arp; Omnisphere's arp is half of
  why people buy it. This is a decision (§9.3), not an oversight.

### 1.4 Defects found today, with a green suite behind them

Fix these first (§7, Phase 0). Each is a bug, not a feature.

1. **A new project opens on "— no preset —\*".** `blank_project`
   (`fontelle-app/src/lib.rs:311-345`) builds the Grand Piano's patch and
   names the channel after it but never records the preset choice, so the
   bar reads *no preset, edited* and the About column says "edited since
   it was loaded" on a project nobody has touched. The comment above
   `STARTING_PRESET` says the point was "a sound somebody chose". Write
   the test in `fontelle-app/tests/flopsynth_browser.rs`: a fresh project's
   Flopsynth channel reports the Grand Piano as its preset, unmodified.
2. **The Modulation page draws the matrix under the ENV 3/4 cards.** At
   1180×840 the matrix panel starts at y≈650 and the cards end at y≈712;
   the first two rows are hidden and the last is cut at the window's edge
   (grabbed and cropped). Fit is tested for the *Synth* page
   (`the_whole_synth_page_fits_the_window_it_opens_at`); write the same
   test for the Modulation page with the Grand Piano's eleven routes, and
   for Effects with four slots.
3. **The minimum size (980×620) clips captions and values** ("20.00 kH",
   "Hardne:macro 3"). The shrink cascade shrinks cells without measuring
   text. Either measure, or raise the minimum to the size the text fits.
4. **The light theme** (§1.1). Decide (§9.2) then fix; there is no test
   that opens this window in the light theme — add one to
   `render_headless.rs` that asserts contrast between the tab label and
   the canopy, and between the picture ink and its screen.
5. **`FlopsynthHit::Header` is dead** (§1.2).
6. **The right-click menu paints over the value read-out and neighbours**
   (grabbed). It should be opaque and offset like the studio's.
7. **The chooser clips single-cell options past ~5 characters** (open
   since v0.7.0). Measure the widest option with `Labels` and span the
   cell; delete `WIDE_CHOICE`.
8. **ENV 3/4 captions overlap at the design size** ("release a shaped
   shaper shape").
9. **`StealPolicy::Quietest`/`LowestPriority` are lies** (§1.3).
   Implement or remove from the chooser.
10. **Three `fontelle-app/tests/studio.rs` tests are red** since v0.8.0
    (`PROGRESS.md` top entry names them). Fix them before adding to the
    browser; they are the browser's own tests.

### 1.5 What is genuinely better than the competition, and must not be lost

- **A recording as an FM/RM modulator** (`modulator` on any layer, any
  source) — nothing else does this; the Growls shelf is built on it.
- **The phase-locked grain cloud** (`SynthState::land`): a frozen note is
  a note, without spray. Keep it when adding density.
- **The stiff string** as a real inharmonic source with a picture of its
  partials.
- **Presets are held by measured gates** (loudness, peak, pairwise
  distance on five axes, tone-route audit). No commercial bank is.
- **Everything is an address**; every knob is automatable and the same
  wire serves plugin and built-in. Do not fork this for the synth window.
- **The layout is pure and tested** (`canvas/flopsynth.rs`). Keep the
  geometry pure; put the new drawing in `render/`, the new gestures in
  `app.rs` as dispatch only (plan §13's thirty-line rule).

---

## 2. The gap table

Verified 2026-09-17. Serum 2 figures from Xfer's page and reviews; Omnisphere
2.8 from Spectrasonics' overview; Vital from its manual. "—" is *absent*.

| | Flopsynth v0.9.0 | Serum 2 | Omnisphere 2.8 | Vital 1.5 |
| --- | --- | --- | --- | --- |
| Oscillators | 3 + sub + noise | 3 + sub + noise | 4 layers, up to 20 osc with Harmonia | 3 + sample |
| Source kinds | table, user table, sample (5 read modes, grains), noise, string | wavetable, multisample (SFZ), sample (slice, tails), granular, spectral | DSP waveforms (638), morphing wavetables, samples, granular, Harmonia | wavetable, sample |
| Wavetable editor | — (import by drop, ≤64 frames) | draw / formula / FFT / import with window size | — | draw / FFT / text-to-wavetable |
| Warp modes per osc | 7 | ~20, dual warps | Waveshaper, FM, RM, unison, Harmonia | ~20, spectral morph |
| Unison | ≤8, detune/blend/width | ≤16, tuning modes (harmonic, ratio, step), spread | ≤8 per layer | ≤16, stack modes (octave, fifth, chord …) |
| Filters | 2 slots, 4 models, 7 SVF shapes, drive, key track | 2 filters, ~40 types, per-osc routing | dual, 69 algorithms | 2 + FX filter, ~30, per-osc routing |
| Envelopes | 4 (DAHDSR, shape knobs, no loop) | 4 | 12 | 6 |
| LFOs | 4, 6 shapes, chooser only | 10, drawn, chaos, editable curves | 8 | 8, drawn, grid, tension, stereo |
| Other sources | vel, key, AT, wheel, bend, random (note-on), counter, note X/Y, 4 macros | vel, key, AT, wheel, chaos, note-on random, 8 macros, MIDI CC | flex-mod: many | vel, key, lift, slide, 4 randoms, 4 macros |
| Mod rate | block (375 Hz); amp env per sample | per-sample / oversampled | per-sample | per-sample |
| Matrix UI | list, delete only | full table, reorder, bypass, aux, curves | 48 slots | full table, stereo, bipolar, power |
| Drag-to-assign | Modulation page only | from anywhere | via matrix | from anywhere |
| Effects in patch | 8 kinds, 4 slots, unmodulatable | ~16 kinds, 3 buses, multiband, M/S, modulatable | 93 units, 4 per layer + aux | 9 kinds, reorderable, modulatable |
| Arp / sequencer | — (the roll's) | arp + clip sequencer | 8 arps, groove lock | — |
| Oversampling | — | 2×/4×/8× per patch | yes | 1–8× |
| MPE / poly AT | — | MPE | MPE | MPE |
| Browser | shelves + search + ★ | tags, folders, favourites, preview | tags, moods, sound match, audition | folders, tags, favourites |
| Preset audition | — | yes | yes | — |
| A/B, init, randomise | — | A/B, init | — | randomise, init |
| Knob: hover value / dbl-click / type / wheel / right-click | — / — / — / — / 1 item | all | all | all |
| Tooltips | — | yes | yes | yes |
| UI scale / skins | OS resize; 4 PNGs | resize; skins | resize; 3 sizes | 0.5–2×; skins |
| On-screen / QWERTY keys | — | yes | yes | yes |
| Oscilloscope / spectrum | — (the sky) | osc + spectrum | — | osc + spectrum |
| Factory presets | 388, 26 categories, gated | 626, 288 tables | tens of thousands, 18 libraries | 400+ |

---

## 3. The window, redesigned

### 3.1 The principle

**The instrument is the picture.** Ty's bridge was the right idea — a
signature look, a place the eye rests — but a picture over a control panel
is a poster. The canopy becomes the instrument's *eyes*: what the sound is
doing, drawn large, in the theme's inks, with the sky behind it as the
backdrop the scope and the spectrum are drawn on. The consoles under it
become sparser, larger and typed.

Plan §8.1's ten principles stand. Four are added:

11. **Three sizes of everything.** Type: 15 px headings (card names, tab
    labels), 12 px captions, 12 px values in a *tabular* face; weights
    matter — headings medium, captions regular, values medium. Knobs:
    **Large 44 px** (the one knob per card a player reaches for first —
    position, cutoff, decay, macro), **Medium 32 px** (everything
    continuous), **Small 24 px** (fine, phase, pan of a sub). The cell
    grid becomes 60×72 with a knob size per control declared by
    `FlopsynthCard` beside its shape. Pictures: **72 px** tall on the
    Synth page, floor 48.
12. **Words are design.** Every caption is a word a player uses: *WT POS*,
    *FM AMT*, *FM FROM*, *ATK CURVE*, *KEY TRK*, *SUB LEVEL*. Captions go
    in a caption file (`fontelle-app/src/instrument.rs` builds them;
    `tests/flopsynth_ui.rs` asserts them) and are the only text drawn in
    capitals.
13. **Motion tells you what changed.** A knob's arc eases to its value over
    ~80 ms when set by anything but the pointer (preset load, automation,
    modulation ring); a mod ring pulses in step with its source; a page
    crossfades over ~120 ms; a hover raises a value bubble. The animator
    count already exists (TDD §16.3 — nothing animates while stopped and
    silent, except a gesture in progress).
14. **A control is reachable from where you are.** Sources are on every
    page (the *mod strip*, §3.4), values can be typed anywhere, and the
    menu on a knob does what Serum's does.

### 3.2 Chrome and the canopy

- **The header** keeps the preset bar (it is the system's) and gains: an
  **A/B** pair with copy, **Init**, an **undo/redo** pair scoped to the
  patch (wired to the document's undo, filtered to this channel), a
  **compare-to-loaded** toggle, and a **scale** chooser (75 / 100 / 125 /
  150 %) that sets a window-level `scale` the layout multiplies by instead
  of the shrink cascade. The cascade goes; the minimum size is whatever
  100 % of the design fits at, and the window refuses smaller.
- **The canopy** is 120 px at 100 % on every page (not 84–240, not 0 on
  Presets). It holds, left to right: the **oscilloscope** (a stretch of
  the output waveform, `DocumentHost::instrument_sound` already hands one
  over), the **spectrum** (the analyser bands, drawn as bars over a log
  axis with the filters' response curves overlaid in their inks), and the
  **voice lamps** (one dot per sounding voice, brightness by level — the
  "n voices" read-out becomes visible). The sky stays *behind* them, at a
  fixed nebula resolution that costs under 0.5 ms a frame on this machine
  (measure: the CPU shading is `sky.rs:231-340`; cap the quarter-size
  buffer at 240×60 and reuse it while the analyser is quiet). The planets
  go unless a skin brings them.
- **Tabs** are 15 px, on the canopy's glass, with a 2 px underline in the
  page's ink; the active page crossfades.
- **The light theme** — see §9.2. Whichever Ty picks, the test in §1.4(4)
  holds it.

### 3.3 Controls

One widget set in `fontelle-ui/src/render/` (`draw_flop_knob`, `_chip`,
`_switch` are the seams; keep their signatures and add a `KnobSize`):

- **Knob**: three sizes; 270° sweep; value arc in the card's ink with a
  soft glow at Large; **hover bubble** above the knob with the value in
  the unit and the caption, 12 px, opaque, never covering the knob; the
  static value under the knob stays for Large and Medium and is dropped
  for Small (the bubble carries it). **Gestures:** drag (150 px throw),
  Shift 6×, **Ctrl 20×**, **Alt-click resets to the preset's value**,
  **double-click opens a typed field** in the knob's unit (parse "2.4k",
  "-12", "1/8", "37%"; `TextEntry` is the model — `text-fields` memory),
  **wheel with Ctrl held** nudges one unit (the wheel alone still only
  scrolls — the rule stands), **arrow keys nudge the focused knob** (there
  is a focus spine; use it), **right-click** opens the menu below.
- **The knob menu**: *Reset to preset* · *Reset to default* · *Type value…*
  · ─ · *Modulate from ▸* (sources, with a tick on the ones already
  routed) · *Remove modulation ▸* (one per route) · *Assign to macro ▸* ·
  ─ · *Create automation clip* · *Copy value* · *Paste value*. Opaque,
  offset so it never covers the control.
- **Chooser**: measures its widest option and spans cells; a chooser whose
  options are *shapes* (wavetable, LFO wave, filter model, warp) draws a
  32×16 thumbnail of each option in the menu (`Wave` picture code at
  thumbnail size) and the current one on the chip. Menu in columns, as
  now.
- **Switch**: as now, with the caption *on* the pill (ON/OFF) rather than
  under it.
- **Mod ring**: one band per route, stacked outward, each in its
  **source's colour** (envelopes violet, LFOs teal, macros amber, note
  sources green, performance sources rose — five tokens added to the
  theme, each with a light-theme value). The band's arc is the route's
  range; a moving source draws a dot on its band at the live value (the
  LFO phase is already sent per frame, `Heard.lfo_phases`; add the
  envelope levels and macro values). Dragging a band edits *that* route;
  hovering names it.
- **Slider** (matrix depth, macro): bipolar from centre, same three sizes.
- **Tooltips**: `tip()` on `FlopsynthHit`, one sentence each, in the
  window's own `tooltip` field, drawn like the studio's. The tests
  (`fontelle-ui/tests/flopsynth.rs`) assert every hit kind has one.

### 3.4 Modulation, everywhere

- **The mod strip** is a 56 px band across the bottom of *every* page:
  source badges (ENV 1–6, LFO 1–8, M1–8, the note and performance
  sources) each with a live thumbnail (the envelope's curve with its
  playhead, the LFO's shape with its dot, the macro's value). Drag a badge
  onto any knob on any page to route; drop on a card's name to open the
  matrix filtered to that card. Click a badge to open its editor in the
  **inspector** (below).
- **The inspector** replaces the Modulation page's fixed cards: a
  full-width editor area under the canopy that shows *the thing last
  clicked* — an LFO's shape editor, an envelope with curvature handles
  and loop points, a macro's name field and its routes, an oscillator's
  table stack. Serum's bottom panel; Vital's centre. The Modulation page
  becomes **Matrix**: the inspector plus the full table.
- **The matrix table**: add row (+), source, destination, depth slider,
  **via**, **curve**, **invert**, bypass, drag to reorder, delete; every
  cell a chooser; sort by source or destination; rows with a moving source
  animate their depth slider's dot. Route count is unbounded — cap the
  view with a scrollbar (the *table* scrolls; the page still does not).
- **LFO editor**: points with tension (drag the segment's middle for
  power-curve), grid snap (1–64 divisions), draw mode, step mode, smooth,
  shapes menu with thumbnails, import/export the shape as part of the
  patch (`Lfo.shape: Option<LfoShape>` — a new field, points ≤64, serde
  skip when empty; `LfoWave` keeps its meaning for old patches). Stereo
  phase offset (Vital). Rate to 100 Hz.
- **Envelope editor**: curvature handles on each segment (drives the
  existing shape values, so the picture and the voice agree), a **loop**
  region (new `EnvelopeConfig.loop: Option<(Stage, Stage)>`), and *hold*
  drawn as a plateau.

### 3.5 The Synth page

Same signal-path layout, larger, sparser:

- **OSC A/B/C**: nameplate with kind chooser and a **mute/solo** pair;
  picture 72 px: for a table, the **frame stack** (eight frames drawn
  behind one another, current frame lit, position marker, drag = position;
  a *3D* toggle spins it — vello draws lines, this is cheap); for a
  recording, the waveform with loop/grain region and a **play head** while
  a voice reads it; for the string, the partials with the felt corner. One
  Large knob (WT POS / START / BRIGHT), Medium for level, warp amount,
  unison, detune; Small for pan, phase, semi, fine, width, blend. The
  route chooser draws the topology (→F1, →F2, →F1→F2, →out).
- **SUB** and **NOISE** stay aside; NOISE gains a *type* chooser (§4.4).
- **Filter 1/2**: picture 72 px with the spectrum of the bus it filters
  ghosted behind the response (the analyser taps are there; add a tap per
  bus — `SamplerNode::with_scope` is the seam); XY drag as now; Large
  cutoff, Medium res/drive, Small key track/character; a **serial/parallel**
  glyph between the two cards that shows how the layers route.
- **ENV 1/2** stay on the page; **ENV 3–6** and the LFOs live in the strip
  and the inspector.
- **Voice** gains the unison *mode* chooser, the velocity curve picture
  (§4.6), the glide curve, and a **keyboard** strip (two octaves of keys
  at the bottom of the card; click plays; QWERTY plays while the window
  has focus, Z/X shift octave — `NoteTrigger` through the same path the
  roll's key column uses).
- **Macros**: eight, each a Medium knob with an editable name (click the
  caption → `TextEntry`), the mod ring showing its routes.

### 3.6 The Effects page

A **rack**, not a grid: slots in a column on the left (drag to reorder —
the dead `Header` hit becomes live; the drag is the rack's own), each with
on/off, a wet/dry, and a level meter; the selected slot's controls fill the
right, with the effect's picture (the EQ's curve, the delay's taps, the
reverb's decay tail, the distortion's transfer curve — `effect.rs` draws
the EQ already). **Eight slots.** Effect parameters take mod rings and
appear in the matrix as destinations (`ModDest::FxParam(slot, index)`,
evaluated once per block from the loudest voice, exactly as plan §12
sketched). Effect presets per slot from the system's preset bank (the
bank already stores effect presets).

### 3.7 The Presets page

The browser Omnisphere users expect, on the bank we have:

- **Three columns** as now, plus a **filter bar**: category chips,
  **tag** chips (multi-select; a preset carries tags: `Preset.tags:
  Vec<String>` in `fontelle-types/src/preset.rs`, factory rows get them in
  `presets.rs` — instrument family, mood, character: *warm, bright,
  aggressive, evolving, plucked, sustained, wide, mono, sampled, fm,
  granular…*), **source** (factory / mine / pack), ★, **recent**.
- **The list** shows name, category, tags as faint chips, ★, and an
  **audition** button; **hover 400 ms auditions** a C3 note through the
  channel (a preview render; see §5.2) without loading; **single click
  selects and previews, double-click loads** (or Enter) — a loaded
  preset's edits are never lost to a stray click. Arrow keys walk the
  list; the search box is a real `TextEntry` (arrows, paste, caret,
  selection).
- **The About column** becomes the **inspector** for the selected preset:
  a rendered **thumbnail** of its sound (waveform + spectrum, from the
  preview render), its tags, its macros by name, its sources ("2 tables,
  a recording, 5 routes"), author/notes (`Preset.notes`), and
  **"Sounds like"** — the five nearest presets in the bank on the same
  five axes the pairwise gate already measures (t30, centroid, crest,
  attack flux, ten-band shape; `tests/flopsynth_presets.rs` computes them —
  move the measurement into `fontelle-core` and cache the vector per
  preset in the bank at build time). This is Omnisphere's Sound Match, for
  free, from the tests.
- **Actions**: Save, Save as, **Rename**, **Delete** (mine only),
  **Duplicate**, **Export pack** (a zip of `.json` files), **Import pack**,
  **Randomise** (within the current preset's family: ±20 % on continuous
  values, never a source change), **Mutate** (a milder randomise), and
  **Init**.

---

## 4. The engine, expanded

Each item names its home, its test and its gate. Cost budgets in §6.

### 4.1 Anti-aliasing (Phase 2)

- **Per-oscillator oversampling** `quality: Osc quality { Off, 2×, 4× }` on
  `SynthOsc` (new field, serde skip on Off), applied around the warp and
  sync stage and the ladder's nonlinearity: render the oscillator at N×
  into a small stack buffer, decimate with a half-band FIR (12 taps,
  polyphase; one `[f32; 24]` state per oscillator in `SynthState`, which
  stays `Copy`). The patch-wide `patch/quality` sets the *default*, the
  oscillator overrides.
- **Tests** (`fontelle-dsp/tests/synth_alias.rs`): the existing
  `a_saw_high_up_has_no_energy_below_its_fundamental` pattern, asked of FM
  at 2 cycles on C7, Sync 8× on C7, Quantise at 4 steps on C6, the ladder
  at drive 24 dB — each must be ≥20 dB cleaner at 4× than Off, and Off must
  measure *no worse* than today (so the change is opt-in). Bench:
  `benches/flopsynth.rs` gains `supersaw/4x`; §6 holds the number.
- **Sample interpolation follows quality** (`Interpolation::Normal` at
  `synth_osc.rs:1018,1023` → the session's). Test in `synth_sample.rs`: a
  transposed sine at +19 st reads ≥12 dB less alias energy at the high
  setting.

### 4.2 Modulation (Phase 3)

- **Counts**: `MAX_LFOS` 4→8, `MAX_MOD_ENVELOPES` 3→5 (six envelopes with
  the amp), `MACRO_COUNT` 4→8. All are arrays in `Voice`; the voice's
  memory stays under 24 KB (measure in `flopsynth_no_allocation.rs`).
- **Rate**: mod envelopes and LFOs advance **per 8 samples** with linear
  ramps between (the `FILTER_STEP` rhythm the cutoff already keeps), so a
  4 ms gate is heard and a 30 Hz LFO on pitch is a vibrato, not a stair.
  Layer gain, pitch, position and pan read the ramped value per sample.
  Test: `fontelle-core/tests/mod_rate.rs` — an LFO at 20 Hz on pitch
  renders a sideband spectrum, not a comb; a 5 ms envelope on a gain is
  heard (the `flopsynth-bridge.md` §1 hammer gate, shortened). Cost: this
  is the one item that can blow §6 — measure before and after; if a
  per-8-sample matrix walk costs more than 0.15 % per voice, walk only the
  routes whose source moved (an `active` bitmask per block).
- **LFO shapes**: `Lfo.shape: Option<LfoShape>` (≤64 points with tension,
  grid), read through a 256-entry per-voice-less table rebuilt when the
  shape changes (shared, `Arc`, like wavetables). Test in
  `fontelle-dsp/tests/lfo_shape.rs`: a two-point ramp equals `SawUp`, a
  tension of ±1 bends the ramp by the same law the envelope shapes use.
- **LFO → LFO** (`voice.rs:1153-1157`): evaluate LFOs in index order and
  let a lower-numbered LFO modulate a higher one's rate/depth/phase. Test:
  LFO 1 on LFO 2's rate reads as frequency modulation of LFO 2.
- **Envelope loop** (`loop: Option<(Stage, Stage)>`): while the key is
  held, reaching the loop's end returns to its start. Test: a looping
  decay reads as a periodic gain.
- **New sources**: `Chaos` (a Lorenz attractor per voice, rate knob —
  Serum 2's), `RandomWalk` (per-voice smoothed noise, rate + smooth),
  `EnvelopeFollower` (of the voice's own level), `StepSeq(u8)` (two
  16-step sequencers with per-step values and a division; the *modulation*
  half of what an arp does, and no time addressing outside the patch —
  the value at a step is state, not events). Each is a `ModSource`
  variant appended at the end. Test per source in
  `fontelle-core/tests/mod_sources.rs`.
- **`ModDest::FxParam(slot, index)`** as plan §12 sketched. Test in
  `fontelle-engine/tests/flopsynth_fx.rs`: a macro on the delay's feedback
  changes the tail.
- **Velocity curve** (`VoiceConfig.velocity_curve: {Linear, Square (the
  default today), Soft, Hard, Custom(4 points)}`) and per-layer
  **velocity window** (`SynthOsc.velocity: (u8, u8)`, default 0..127) with
  a crossfade width — the sampled grand's two layers become the ordinary
  case. Test in `fontelle-core/tests/velocity.rs`.
- **MPE** (Phase 5, §9.4): per-note *continuous* slide, pressure and
  timbre through `NoteTrigger` — a `NoteMod` message stream keyed by voice
  id; `mod_x`/`mod_y` become live. `Aftertouch` gains a `Poly` reading
  that prefers the note's own value when present.

### 4.3 Oscillators (Phase 4)

- **Spectral source** `SynthSource::Spectral(u8)`: an analysed recording
  (the same `Patch::samples` zone) as a frame sequence of 64 partial
  amplitudes and frequencies at ~10 ms hops, played through the `String`
  bank's phasors (`string_partials()` is the seam: a *spectral frame*
  replaces the stiffness formula). Position scans the frames; **warp**
  gains `Stretch` (spread partials), `Shift` (formant), `Freeze`; the
  Partials picture draws it. Analysis in `fontelle-assets` (STFT, peak
  picking, partial tracking — `fontelle-assets/src/` owns audio analysis
  already; `AudioPreview`'s peaks are there). Test in
  `fontelle-dsp/tests/synth_spectral.rs`: a resynthesised sine at 440 Hz
  is a sine at 440 Hz within a cent; a chord's partials are all present;
  scanning position reads the recording's own brightness envelope.
- **Multisample import (SFZ)**: a reader in `fontelle-assets` for the
  subset every library uses (`<region>`, `sample`, `lokey/hikey/pitch_
  keycenter`, `lovel/hivel`, `loop_mode/loop_start/loop_end`, `tune`,
  `volume`) → `UserSample` zones with `vel_range` (the missing field). The
  card's menu gets *Import SFZ…*; a dropped `.sfz` lands as a multisample.
  Test in `fontelle-core/tests/sfz.rs` on a hand-written 6-region file.
- **Wavetable editor** in the inspector: draw (free / line / sine
  segments), harmonic bars (64, with phases), formula (a small expression
  language: `sin(x*2)+0.3*saw(x)`), FFT of a dropped sample with a chosen
  window size, per-frame morph (linear / spectral), frame add/remove/copy,
  **export** as a 2048-frame `.wav` Serum can read. Tables stay in the
  patch (`UserWavetable`, ≤64 frames — raise to 256 with the 16-bit
  storage; 1 MB a table). Tests in `fontelle-ui/tests/wavetable_editor.rs`
  (pure geometry) and `fontelle-dsp/tests/wavetable_user.rs` (a drawn
  square equals `Square` within the mip levels).
- **Warp modes** appended after `Rm`: `PhaseDistortion` (Casio: bend the
  phase by amount), `Formant` (stretch the cycle, hold the pitch), `Flip`
  (invert half the cycle at the position), `Asym` (bend + and − halves
  apart), `FmNoise` (FM from the noise layer), `Remap` (a user curve on
  phase — reuse `LfoShape`). Each has a picture in the chip's thumbnail.
  Test per mode in `synth_osc.rs`: the spectrum moves the way the mode
  says (formant shift keeps f0 and moves the centroid; PD adds odd
  harmonics; flip adds even ones).
- **Unison modes** (`Unison.mode: {Classic, Octave, Fifth, Chord(u8),
  Wide}`; `MAX_UNISON` 8→16; per-voice tuning spread `{Linear, Power,
  Harmonic}`). Cost: 16 voices of a 32-frame table is the existing cost
  ×2; §6.
- **Noise types** (`SynthSource::Noise` gains `NoiseKind`: `White, Pink,
  Brown, Blue, Crackle, Vinyl, Sample(zone)`): the colour knob stays and
  is the tilt on top. `Sample(zone)` is the kits' hats and rides as noise —
  a recording read at the note's rate with no pitch.
- **Sub engine**: the SUB card gains `shape {Sine, Tri, Square, Saw}`,
  `octave {−2, −1, 0}` and a **direct-out** switch (bypass the filters and
  effects, the trap the bank keeps meeting) — sugar over the existing
  layer, no new source.

### 4.4 Filters (Phase 4)

Add models behind `FilterModel` (each with `character`): `Diode` (a
303-shaped ladder), `Sallen` (MS-20 style with the clipping in the loop),
`Phaser` (4–12 allpass stages as a filter), `Vowel` (the formant model with
a *vowel pair* morph and gender), `Ring` (a fixed-ratio ring-modulating
filter — Serum's), `Dual` (two SVFs at cutoff ± spread with a mix). Filter
**FM from an oscillator** (`FilterSlot.fm_from: Option<u8>`, amount) is the
one filter feature Serum users ask for by name. Per-model tests in
`synth_filter.rs` (response shape, self-oscillation pitch within 2 cents at
full resonance, no NaN at drive 24 dB). Third slot: no — two slots and
routing is the design; the *Dual* model is the answer to "I want more".

### 4.5 Effects in the patch (Phase 4)

`PATCH_FX_KINDS` gains `Phaser`, `Flanger`, `Fold` (wavefolder with
symmetry), `Shifter` (Bode frequency shifter, ±5 kHz, feedback), `Hyper`
(Serum's unison-as-effect: four detuned copies with spread),
`Multiband(Dist)`, `Width` (M/S width + bass mono). All zero-latency (the
rule stands; look-ahead kinds stay out). `MAX_PATCH_FX` 4→8. Each is a
`fontelle-fx` module with the catalogue's family and a picture. Tests in
`fontelle-fx/tests/` per kind and `flopsynth_fx.rs` for the chain.

### 4.6 Voice (Phase 3)

- Implement `Quietest` (lowest current amp-envelope level × layer gain
  sum) and `LowestPriority` (release-phase voices first, then quietest);
  test: with polyphony 2 and a held chord, the third note steals what the
  policy says.
- **Glide curves** (`{Linear, Exponential, Fast, Slow}`), **per-note glide
  time** as a mod destination.
- **Chord memory**: no — the roll's chord tool is the tool.
- **Portamento in Poly** (Serum's *always* mode): a `glide_mode {Off,
  Legato, Always}` replacing the bool `glide_legato_only` (keep the bool's
  address reading the same values).

---

## 5. The bank and the browser (Phase 6)

### 5.1 Tags, notes, macros

- `Preset.tags`, `Preset.notes`, `Preset.author` in
  `fontelle-types/src/preset.rs` (serde default + skip). Factory rows carry
  tags in `presets.rs` via a builder verb `.tags(&[..])`; the gate in
  `flopsynth_presets.rs` requires **≥3 tags per row from a controlled
  vocabulary** (`TAGS: &[&str]` in `flopsynth/mod.rs`; the test refuses a
  tag not in it) and **all macros named** (today: two named, two "macro
  n" — the gate says two; raise it to all eight once §4.2 lands, and the
  export tool rewrites the bank once).
- **Every preset gets a showcase phrase** in `notes` (one sentence: what
  it is for and which macro to reach for).

### 5.2 Audition and sound match

- **Preview render**: `fontelle-core::preset_preview(patch) -> Preview
  { peaks, bands, vector }` — 1.5 s of C3 at velocity 100 through
  `Sampler` (the probe's render, moved into the crate). The bank caches
  the factory previews at `build.rs` time as a compact binary beside the
  JSON (388 × ~2 KB); user presets render on first view on a worker
  thread (never the audio thread, never the frame). The Presets page's
  audition plays the cached peaks' *source* — the actual patch — through
  the channel's own node for 1.5 s on hover/click without touching the
  document (a `PreviewNote` on the engine that swaps the patch in a
  scratch voice pool; `flopsynth_live.rs` is where the wire's tests live).
- **Sounds like**: the five-axis vector is the one `tests/flopsynth_
  presets.rs` computes; the five nearest by normalised Euclidean distance,
  across the whole bank. Test: the Grand Piano's neighbours are keys, not
  growls.

### 5.3 Bank growth

The gates stand (≥10 per category, loudness, peak, pairwise apart, tone
route, shows-off). Grow by **shelves of a feature**, the way the sampled
shelves were: *Spectral* (the new source), *Drawn* (user LFO shapes and
drawn tables), *Chaos & Walk*, *SFZ Instruments* (only if a CC0
multisample ships — see the Salamander credit for the pattern), *Hyper &
Shifted* (the new effects), *MPE* (if §9.4 is yes). Target **520** before
the release that carries this plan; every new shelf audited with
`examples/preset_audit.rs` and *listened to* (`--play-flopsynth`).

---

## 6. Performance budget

Plan §10's numbers stand as the *ceiling per feature*; the total is what
matters and this plan adds cost in three places. Bench before and after
each phase (`cargo bench -p fontelle-core --bench flopsynth`) and write
the numbers in `PROGRESS.md`.

| what | today | budget after |
| --- | --- | --- |
| Init voice | ≤0.3 % (measured 0.4 % with cached filters) | ≤0.4 % |
| Supersaw voice, 2 filters | ≤1.2 % | ≤1.4 % at Off, ≤3.5 % at 4× |
| Grand Piano voice | 1.6 % | ≤1.8 % |
| 16 voices of Choir Ahh + chain | ≤8 % | ≤10 % with 8 LFOs at 8-sample rate |
| the window, sounding, at 30 fps | unmeasured | ≤3 % of a core, sky included |
| the window, silent | ~0 (11 ticks / 5 s, measured) | unchanged |
| a preset load | unmeasured | ≤16 ms (one frame) |
| a revision (`arrange edit` lists) | 0.29 s / 1500 motions | unchanged |

If a phase cannot meet its line, the feature ships **off by default** (the
oversampling, the per-8-sample matrix walk) and the number goes in
`PROGRESS.md` beside the reason — not the budget loosened.

---

## 7. Build order

Six phases, each shippable, each ending with the workspace suite green,
clippy clean, and a session on `:99` looking at every page in both themes.
Do not start a phase's engine work before its tests exist and fail.

### Phase 0 — the defects (a day)

§1.4 items 1–10. Tests named there. This phase changes no design and
touches no address. Ship it as a patch release (Ty's call).

### Phase 1 — the window's bones (the biggest phase; a week)

Order matters: geometry first, then drawing, then gestures, then motion.

1. **Scale and sizes**: `scale` on the window; `KnobSize` per control in
   `FlopsynthCard`; cells 60×72; pictures 72 px; the shrink cascade
   deleted; minimum size from the fit. Tests: `fontelle-ui/tests/flopsynth
   .rs` — `the_whole_synth_page_fits_at_every_scale`, and the Modulation
   and Effects fits from §1.4(2). Look at it at 75/100/150 %.
2. **Type scale and captions**: three sizes and weights through
   `FontTokens` (cosmic-text has the weights; `text/mod.rs:89-117`
   already keys on `font_weight`); the caption file; captions in caps.
   Tests: `flopsynth_ui.rs` asserts the caption of every control against
   the file; the headless dump for the look.
3. **Controls**: hover bubble, Alt reset, Ctrl fine, typed field,
   arrow nudge, Ctrl-wheel, the full right-click menu, measured choosers
   with thumbnails, tooltips. Tests: `fontelle-ui/tests/flopsynth_
   gestures.rs` (pure arithmetic per gesture), `flopsynth_ui.rs` (the
   menu's entries per hit kind), `keymap.rs` (the new `Action`s — every
   key is a keymap action, never a literal).
4. **Canopy = eyes**: scope, spectrum with response overlay, voice lamps,
   the sky behind at a capped cost. Tests: `tests/sky.rs` (cost bound: a
   frame of the nebula under N µs on the test machine, asserted loosely),
   `render_headless.rs` scene with a synthetic analyser frame.
5. **Mod rings per source**, stacked, coloured, live dots; theme tokens
   with light values. Tests: `flopsynth.rs` ring geometry for 1/2/3
   routes; `mod_marks.rs`'s per-revision budget still holds.
6. **The mod strip and the inspector**; drag-to-assign from every page;
   Modulation → Matrix page with the full table. Tests: strip layout on
   every page, table edits (add, reorder, via, curve, invert, bypass,
   delete) each an undoable command in `fontelle-model`.
7. **LFO editor and envelope curvature/loop** in the inspector (the model
   fields land here even though the DSP reads them in Phase 3 — the
   picture is held to the DSP by a test that renders both).
8. **Effects rack** with reorder, wet/dry, meter, picture, eight slots.
9. **Motion**: eased arcs, ring pulse, page crossfade, hover bubble
   fade — all through the animator count.
10. **Light theme** per §9.2.

### Phase 2 — clean at the top (two days)

§4.1. Oversampling per oscillator, sample interpolation from quality, the
alias test file, the bench line.

### Phase 3 — modulation and voice (three days)

§4.2 and §4.6. Counts, rate, shapes, LFO→LFO, loop, new sources,
`FxParam`, velocity curve and windows, stealing, glide. The bank's
`shows_off` test gains a line per new source once a preset uses it.

### Phase 4 — sources, filters, effects (a week)

§4.3–4.5. Spectral first (the biggest win per line), then SFZ, then the
wavetable editor, then warp modes, unison modes, noise types, the sub
sugar, the filter models, the effect kinds.

### Phase 5 — MPE (two days, if §9.4 is yes)

### Phase 6 — the bank and the browser (three days)

§5. Tags/notes/macros gate, preview render and cache, audition,
sounds-like, A/B, init, randomise, packs, the real search field, the
inspector column. New shelves last, probed and listened to.

---

## 8. Tests that hold the whole thing

Beyond the per-step tests, three suites that hold the *claims* this plan
makes, written before Phase 1 starts:

- `fontelle-ui/tests/flopsynth_legibility.rs`: for every page, every
  scale, both themes — no caption or value wider than its cell (measured
  with the real shaper), no two hit rectangles overlap, every text against
  its background at ≥4.5:1 contrast (WCAG AA; compute from the theme's
  tokens), every picture at least its floor. This is the test that would
  have caught every item in §1.1 except taste.
- `fontelle-app/tests/flopsynth_everything_reachable.rs`: every
  modulation destination the engine offers can be assigned from the Synth
  page; every `ModRoute` field has a table cell that sets it; every
  `PATCH_FX_KINDS` member has a picture; every hit kind has a tooltip;
  every control has a menu with *Reset*.
- `fontelle-core/tests/flopsynth_engine_claims.rs`: the gap table's
  Flopsynth column, as asserts on the enums' lengths and the constants —
  so the table in this document and the code cannot drift apart without
  a red test that names the line to update.

---

## 9. Decisions for Ty

Numbered so a reply can be "1 yes, 2 dark, 3 no, 4 later".

1. **The canopy's fate.** This plan turns it into the scope/spectrum with
   the sky behind (§3.2) and drops the planets. The alternative is keeping
   the bridge as it is and adding the eyes as a fifth card. Recommendation:
   §3.2 — the sky as backdrop keeps the look Ty asked for and gives the
   space a job.
2. **The light theme.** (a) Give the synth window its **own palette**,
   dark under both themes, with the studio's accent inks mixed in (Serum,
   Omnisphere and Vital are all dark-only and it is not a defect); or (b)
   design a real light version of the hull, glass and sky. Recommendation:
   (a), with the four skin PNGs as the way to change it.
3. **An arpeggiator in the patch.** Plan §12 and TDD §16.5 say the roll's
   arpeggiate command is the arpeggiator and a second time-addressing
   scheme is forbidden. Serum 2's clip sequencer and Omnisphere's arp are
   the features reviewers name. A middle path that keeps the invariant:
   the **step-sequencer mod sources** of §4.2 (state, not events) now, and
   a per-patch arp *only* as a `NoteTrigger` transform inside `Sampler`
   (pattern, rate, gate, octaves, latch, swing — its clock is the
   transport's, its addresses are the patch's, it emits no events into
   the project). Recommendation: the mod sequencers now; the arp is a
   separate decision after Phase 4, with a design note first.
4. **MPE.** Two days; a `NoteTrigger` change that touches the roll's
   per-note slides. Recommendation: after Phase 4, when there is a
   spectral source worth pressing on.
5. **Window size.** 1180×840 at 100 % with a scale chooser (§3.2). The
   design size can be 1240×860 if the sizes in §3.1 do not fit at 1180 —
   the fit test decides; Ty decides whether 1240 is acceptable beside the
   arrangement on his screen.
6. **Eight macros, eight LFOs, six envelopes.** Cost is in §6; the strip
   holds them. Recommendation: yes to all three.
7. **The preset gate for tags** (three per row) rewrites 388 rows once.
   Recommendation: yes, in one commit, with the export tool.

---

## 10. What not to do

- Do not add a second knob widget, a second menu, a second text field, or
  a second colour system. Every one of those exists; extend it.
- Do not put interaction arithmetic in `app.rs`; the canvas is pure and
  tested, the app dispatches (plan §13).
- Do not draw the sky in a shader; `draw_window` is a pure function of its
  inputs so the headless dump can render it. Cap its cost instead.
- Do not re-trim the bank, re-voice the Grand Piano, or "fix" a preset the
  pairwise gate accepts. Voicing reports are Ty's to make.
- Do not read a modulator's picture from anything but the numbers the voice
  reads — a picture that lies is believed (plan §8.1(5); the straight-line
  envelope is the example).
- Do not accept a feature that costs more than §6 by loosening §6.
- Do not release, tag, announce, or make the repository's state public.
  Prepare it; stop at needs-review; Ty publishes.
