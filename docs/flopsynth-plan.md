# Flopsynth: the built-in synthesiser

**Status, 2026-09-06: every phase is built and green — P and 0 through 6.**
What sounds, what is reachable and what the numbers are is written up in
`PROGRESS.md` under "Flopsynth — the synthesiser, its bank, and everything
under them" and the section after it. Five things below were changed during
the build; each is recorded where it happened as well as here.

1. **Three filter slots, not two.** A stateful filter cannot be in two places
   at once, so the serial route needs its own (`Voice::filters`).
2. **The bank's "audibly apart" test has a fifth axis.** A spectral centroid is
   a mean, and two vowels have almost the same one.
3. **§8.6's Presets page is the preset bar's drop-down and the browser's
   Presets tab**, rather than a fourth page inside the synth window. The tab
   (§P.8) is the same rows over the same bank with a search across every
   device, and a third place to list them would be a third thing to keep in
   step. The Synth, Modulation and Effects pages are as drawn.
4. **§P.9's deletions stop at the panel.** `EffectConfig::{presets,
   apply_preset, matching_preset}`, the chip rows, `SetInsertPreset` and
   `set_instrument_preset` are gone, which is what §P.9 is *for* — one preset
   mechanism, not two. The **recipes** stay, in the position `DrumKitStyle`
   keeps: `DistortionPreset` and its two neighbours are what `cargo xtask
   export-factory-presets` runs, and deleting them would have made the
   committed files unregenerable and thrown away the tests that prove they are
   distinct.
5. **The bench is two files, not one.** §10's sixteen-voice case goes "through
   `SamplerNode`", which is `fontelle-engine`'s and may not be seen from
   `fontelle-core` (INVARIANT 4). The voices and the wavetable bank are
   `fontelle-core/benches/flopsynth.rs`; the chain is measured beside it.

**The performance budget is not met, and §10 says that is a design
conversation.** Three optimisations landed against it — a silent layer is not
rendered, a filter's coefficients are built when they move rather than per
sample, and a unison stack's per-voice constants likewise — which together
took one Init voice from 1.65 % of a core to 0.54 % and a Supersaw voice from
3.29 % to 1.31 % against a 1.2 % budget. Init's own budget is 0.3 % and
sixteen Choir Ahh voices are at 26 % against 8 %. The numbers, and where the
remaining cost goes, are in `PROGRESS.md`.

Written 2026-09-06 against the working tree at `7ce3a23` plus the uncommitted
2026-09-04..06 sessions (3218 tests green, clippy clean). This is the design
and the build order for Fontelle's main synthesiser, written for the agent who
builds it. It is meant to be worked top-down and phase by phase; every phase
names its tests, its files and its gate.

**The brief** (Ty, 2026-09-06): *"a new built in synthesizer plugin. this will
be our main synth for the daw kind of like how fl studio has flex ... we should
call Flopsynth. It should be an advanced synthesizer inspired by the likes of
omnisphere and Serum. should just be a highly customizable, highly flexible
synthesizer capable of producing all types of sounds across different genres
and instruments. should have lots of built in presets in a bank for tons of
instruments organized by type. make sure to include a wide variety between
synthesis and synths that sound like other instruments like choir ahhs or
strings etc. we need to ensure that this has a really polished, clean, and
pretty user interface. it should be organized, have actual design, flow cleanly
for the users eyes while also having tons of complexity to allow for massive
configuration."*

**Finalised 2026-09-06 after Ty's answers** to the questions the first draft
asked (now §14, "Decisions"): three oscillators; the window at the size this
plan recommends; a preset's name **persists** after edits with a `*` for
unsaved changes; the live parameter wire of §2.3 is approved; and one addition
that reshapes the order of work — **a DAW-wide preset system, like FL
Studio's, for every instrument and effect, built first** (§P), so that
Flopsynth's bank, the effects' presets and a user's own tweaks all go through
one mechanism and nothing is hard-coded per plugin again.

Read, in this order, before starting: `PROGRESS.md`'s top two sections,
`docs/handoff.md` §2 and §4, `docs/effects-catalogue.md` §1 (the twelve
principles apply to an instrument as much as to an effect), TDD §7 (the
sampler engine Flopsynth lives inside) and §8.2 (addresses), and the
`drum_kit.rs` module docs in `fontelle-core`, which is the precedent this
design follows.

---

## 0. Ground rules for this work

- **Tests first, confirmed failing, then the implementation.** The project's
  rule, without exception. Every phase below lists the test files and the
  claims they measure. A test that would pass against the feature stripped out
  is not a test of the feature (catalogue rule 12).
- **The invariants are hard.** INVARIANT 1 (no allocation on the RT thread —
  wavetables are resolved in `prepare`, never in `render`), INVARIANT 4
  (`fontelle-core` depends on `fontelle-dsp` and `fontelle-types` and nothing
  else), INVARIANT 6 (the voice is a fixed topology; every new thing in it is a
  fixed-size array), INVARIANT 7 (every address below is permanent from the day
  it ships), INVARIANT 2 (the window emits edits and never mutates the
  document).
- **Not a special case.** The drum machine's lesson: a kit is an ordinary
  `Patch`, so save, load, automation, the key map, the mixer and undo never had
  to be told it exists. Flopsynth is the same: an ordinary `Patch` whose layers
  carry a new `Source` variant. Where this plan generalises the voice (more
  filter models, envelope shapes, tempo-synced LFOs, macros, an effects chain),
  every built-in instrument gets the generalisation, because it is the *patch*
  that grew.
- **`cargo clippy --workspace --all-targets -- -D warnings` stays clean** and
  `PROGRESS.md` gets a section per phase saying what was measured and what was
  cut.
- **Build in the background** (handoff §6) and check `pgrep -f 'cargo (test|build)'`
  before starting another. Kill **only the studio you launched**, by the pid you
  recorded — `pkill` by name ends the user's own session too, and looks to them
  exactly like a random crash (handoff §5).
- **Seeing it** is half of every GUI gate (`docs/first-usable-plan.md` §2.5):
  `FONTELLE_UI_DUMP=<dir> cargo test -p fontelle-ui --test render_headless` for
  the pixels, and the nested `Xwayland :99` plus the XTEST script in the
  `seeing-fontelles-gui` memory note for real clicks.

---

## 1. What "done" means

Flopsynth is done when all of the following hold, and each is held by a named
test or a recorded human check:

0. **The preset system is in** (§P) before any of the rest: every instrument
   and effect window carries the same preset bar, a preset is a file, factory
   presets are embedded files, user presets are files in the bank folder, the
   browser has a Presets tab, and a device whose knobs have moved since its
   preset was loaded says so with a `*` and can be saved to the same preset
   or as a new one (`fontelle-app/tests/preset_bar.rs`, `preset_bank.rs`).
1. **It is on the New Instrument menu** as "Flopsynth", arrives playing its
   Init patch, and a note in the roll sounds through it with nothing configured
   (`fontelle-app/tests/flopsynth.rs`).
2. **Every control on its window is a stable address** that automation, MIDI
   learn and undo reach (`every_control_on_the_window_can_be_automated`,
   the Flopsynth twin of `instrument_editor.rs`'s test).
3. **A knob is heard while it turns**, on a note already sounding, without the
   graph being rebuilt (`a_knob_drag_does_not_cut_a_held_note`).
4. **The bank ships at least 200 presets in at least 14 categories, ten to a
   category, as
   factory preset files** (§P.3), every one of them sounds, stays inside full
   scale on a four-note chord at full velocity, sits within ±3 dB loudness of
   every other, and is audibly apart from every other preset in its category
   (`fontelle-core/tests/flopsynth_presets.rs`, reading the files the
   recipes generated).
5. **The window is the design in §8**: every section, every gesture, every
   page, drawn through the real pipeline and looked at by a person; the pure
   geometry has tests (`fontelle-ui/tests/flopsynth.rs`).
6. **The RT thread does not allocate** with Flopsynth on a channel, under the
   guarding allocator (`fontelle-engine/tests/flopsynth_no_allocation.rs`).
7. **Performance:** sixteen voices of the heaviest shipped preset render in
   under 8 % of one core at 48 kHz in release (§10), measured by a bench that
   is checked in.
8. **A patch written today opens tomorrow**: the format version is bumped, the
   migration is written, and every patch the tree could write before this
   work still loads byte-for-byte equivalent (`fontelle-core/tests/patch_format.rs`).

---

## 2. Where it sits: the three decisions

### 2.1 Flopsynth is an ordinary `Patch`

`fontelle_core::Patch` already is a synthesiser: layers, two filters, a list
of envelopes, a list of LFOs, a mod matrix with `via`, a voice config with
glide, mono/legato and unison fields. What it lacks is a *source* worth
building a synth on (`Source::Oscillator` is five polyBLEP shapes with no
unison, no wavetable and no cross-modulation), and the generalisations listed
in §3. So:

- **`Source::Synth(SynthOsc)`** is the new variant, beside `Drum(DrumVoice)`
  and for the same reason: a `Copy`, fixed-size description of one oscillator
  that a voice slot can hold, naming no file, so it never needs relinking and
  works on a fresh install.
- A **Flopsynth patch** is five layers in a fixed order — **A, B, C, Sub,
  Noise** — each `Source::Synth`, over every key and velocity. The order is a
  convention the window relies on (`flopsynth::layer_role(index)`), not a rule
  the voice enforces: a patch with a sampled layer appended is still a valid
  patch and still plays, which is how Omnisphere-style hybrids arrive later
  (§12).
- `InstrumentKind::Flopsynth` is the sixth kind. `Session::kind_of` answers it
  when any layer is `Source::Synth`, checked **before** the `Oscillator`
  fallback and after `Drum`. `plays_on_arrival` is true; `wants` is `None`.
- **3OSC stays.** It is the blank instrument `blank_project` starts on and
  forty-odd tests assume it; retiring it is a separate decision for Ty once
  Flopsynth's Init patch has proven as light. Do not fold them.

### 2.2 The effects live in the patch and run in the node

A Serum preset without its chorus and reverb is not that preset. The
dependency rule forbids `fontelle-core` from seeing `fontelle-fx`, but
`EffectConfig` lives in `fontelle-types`, which core already depends on — so
the **document** can carry the chain and the **engine** (which depends on
both) can run it:

- `Patch::fx: Vec<PatchFx>` with `PatchFx { config: EffectConfig, enabled:
  bool }`, at most `MAX_PATCH_FX = 4`, `#[serde(default)]` so every existing
  patch reads as "no effects".
- `SamplerNode` (engine) owns one `EffectState` per slot, built in `prepare`
  from the patch's configs, and runs them after `Sampler::render` on the
  node's scratch pair, blending dry/wet per slot the way `EffectNode` does.
  The configs it reads each block are **the sampler's patch's**, so an
  automation lane or a knob writing `patch/fx[1]/mix` through
  `patch_params::set` is heard the same block.
- Only zero-latency kinds are offered (§3.9): the compressor with no
  lookahead, the filter, the EQ, the distortion, the bitcrush, the chorus, the
  delay, the reverb. The gate and the insert limiter are excluded because a
  plugin *instrument* with latency is the one case §5.5 deliberately does not
  compensate (handoff §3 item 12), and Flopsynth is an instrument.
- **A tail keeps the graph awake on its own.** The idle gate (TDD §6.3,
  `fontelle_engine::live::IdleGate`) stays awake "for exactly as long as the
  output is non-silent" — it *measures* the graph's output rather than asking
  nodes — so a reverb tail after the last note-off is heard without the node
  reporting anything. The test is a rendered one: a delay slot's repeats are
  audible for the slot's tail after the last note-off with the transport
  stopped, through the live path (`the_chain_rings_after_the_last_note_off`).

### 2.3 A knob is heard through the wire, not through a rebuild

**Approved by Ty 2026-09-06** ("if that's the recommended fix let's go for
it"). Today `Session::set_instrument_param` on a patch parameter calls `store_patch`,
which calls `rebuild_graph` — a new graph, a new `Sampler`, a fresh voice pool,
on **every mouse move**. That is tolerable for a soundfont's release knob and
unacceptable for a wavetable position swept by hand under a held chord.

The fix already exists as a path: `SamplerNode::process` applies
`EventPayload::ParamValue { target: ChannelPatch { .. } }` to its sampler
through `Sampler::set_patch_param`, and the live input (`fontelle-engine/src/live.rs`,
the path the on-screen keyboard and MIDI use) reaches the node every block. So:

- A parameter edit does three things: `patch_params::set` on the cached patch,
  `store_patch_quiet` (history, coalesced; cache; dirty; **no rebuild**), and a
  `ParamValue` onto the live wire aimed at the channel's node.
- A **structural** edit rebuilds as today: a layer's table changes (the node's
  wavetable set is resolved in `prepare`), a route is added or removed, an
  effect slot changes kind, the voice count changes, a preset is chosen. The
  rule that decides: *does `patch_params::set` know the address?* If it does,
  it is a parameter and goes on the wire; if it does not, it is structure and
  rebuilds. `Session::apply_flopsynth_edit` is the one place that asks.
- **Undo and redo rebuild**, because the document moved by something other
  than a knob and the node has to be brought back to it.
- This is applied to **every** built-in instrument's panel, not only
  Flopsynth's, because it is the patch that changed and not the synth.

Test: `a_knob_drag_does_not_cut_a_held_note` renders a note, moves a cutoff
through the session's own `set_instrument_param` every block for sixty-four
blocks, and asserts the note's level never falls to the floor and the cutoff's
effect is heard (spectral centroid moves).

---

## P. The preset system — built first, for every device

Lettered rather than numbered because it was added after the rest was
written and is read before it: **Phase P precedes Phase 0** (§11). It stands
on its own — nothing in it knows Flopsynth exists — and it is what makes
"lots of presets organised by type" a property of the DAW rather than of one
plugin.

**The brief** (Ty, 2026-09-06): *"a preset system kind of like FL Studio's
baked into the DAW itself that works for every instrument and effect so we
don't have to hardcode presets in every plugin ... when you have a `*` for
unsaved edits you're able to save it either to the same preset or save as to
a new preset in your bank."*

### P.1 What it is, in one paragraph

A **preset is a file**: a name, a category, the device it is for, and that
device's own saved state. **Factory** presets are such files embedded in the
binary at build time; **user** presets are such files in a bank folder the
user owns. Every device — a built-in instrument, a built-in effect, a hosted
plugin — gets the same **preset bar** in its window (previous, next, the name
with its `*`, a drop-down, a star, Save, Save as…), the same rows in the
browser's new **Presets** tab, the same favourites, the same commands and the
same undo. What a device contributes is nothing but *what its state is*; the
system does the rest. The constructor-presets that exist today (the
distortion's seven, the bitcrush's six, Soften's four, the twenty-two drum
kits) become factory files and the code that made them becomes the tool that
*generated* those files (§P.9).

### P.2 The types (`fontelle-types/src/preset.rs`)

```rust
pub const PRESET_FORMAT_VERSION: u32 = 1;

/// Which device a preset is for. Its `slug()` is a folder name and is
/// INVARIANT 7's: "flopsynth", "drum-machine", "soundfont", "sampler",
/// "3osc", "fx-distortion", "fx-eq", "plugin-clap-com.u-he.diva".
pub enum DeviceKind { Instrument(InstrumentKind), Effect(EffectKind), Plugin(PluginKey) }

/// The device's own saved state — the three shapes the document already stores.
pub enum PresetPayload { Patch(PatchData), Effect(EffectConfig), Plugin(PluginState) }

pub struct Preset {
    pub format_version: u32,
    pub device: DeviceKind,
    pub name: String,
    pub category: String,
    pub payload: PresetPayload,
}

/// What a device remembers about the preset it was loaded from.
pub struct PresetRef { pub name: String, pub category: String, pub origin: PresetOrigin }
pub enum PresetOrigin { Factory, User }
```

- `Preset::is_consistent()` refuses a payload that does not match its device
  (an `Effect` payload for an instrument); the bank lists such a file as
  unreadable rather than loading it.
- No new payload type is invented: `PatchData` is what a channel stores,
  `EffectConfig` what an insert stores, `PluginState` what either stores for
  a plugin. **That is the whole reason presets are free for every device**:
  a device's state is already a serialisable value the document holds, so
  saving one is writing that value to a file with a name on it.
- Serde by name for every enum, so a file is readable in a text editor
  (TDD §17.2's reason for JSON) and a device kind renamed in code would be a
  migration, not a broken bank.

### P.3 Where files live

- **Factory:** `assets/presets/<device slug>/<category>/<name>.json` in the
  repository — the folder TDD §4 already names for "factory presets" and
  which is empty today. Embedded by a **`build.rs` in `fontelle-app`** of
  about forty lines that walks the folder at compile time and writes a
  generated `factory.rs` of `(path, include_str!(..))` pairs. No crate for
  it, for the reason `base64.rs` is forty lines rather than a dependency. A
  factory preset is therefore present on a fresh install, which is the drum
  machine's position kept.
- **User:** `Settings::preset_dir`, default `$XDG_DATA_HOME/fontelle/presets`,
  the same layout. The data directory is Fontelle's own (INVARIANT 10, as
  `settings.rs` reads it for the soundfont bank); the folder is changeable
  in the Settings tab like every other.
- **A category is a folder**, so a user's categories are free-form and
  "Save as…" can make one. The name is the file's stem. Names are unique
  within a device *and origin*: a user preset named like a factory one is a
  second row, tagged, not a shadow — the two are different files and hiding
  one would be a preset that vanished.
- **Soundfont and sampler patches are presets too.** A tuned soundfont
  patch is the product thesis (TDD §7.1), and "keep this the way I tuned it"
  is exactly what a preset is. Its `PatchData` carries `SampleRef`s and
  loads through the same relink path a project does (§17.4); a preset whose
  file is gone loads silent and says so, like a project does.

### P.4 The bank (`fontelle-app/src/preset_bank.rs`)

Modelled on `bank.rs` (`FileBank`), not copied from it — the folder walk,
the unreadable list, the settle-after-delete and `fuzzy_score` are reused.

```rust
pub struct PresetBank { .. }
impl PresetBank {
    pub fn new(user_dir: Option<PathBuf>) -> Self;      // factory always present
    pub fn rescan(&mut self);
    pub fn for_device(&self, device: &DeviceKind) -> Vec<&PresetEntry>; // factory, then user; category, then name
    pub fn categories(&self, device: &DeviceKind) -> Vec<String>;
    pub fn find(&self, device: &DeviceKind, r: &PresetRef) -> Option<&PresetEntry>;
    pub fn load(&self, entry: &PresetEntry) -> Result<Preset, String>;
    pub fn save(&mut self, preset: &Preset) -> Result<PresetRef, String>;   // user only
    pub fn delete(&mut self, entry: &PresetEntry) -> Result<(), String>;    // user only
    pub fn search(&self, query: &str) -> Vec<&PresetEntry>;                 // every device
    pub fn unreadable(&self) -> &[(PathBuf, String)];
}
```

Factory entries are parsed once at construction (they are in memory
already); user entries are parsed on `load` and cached by modification time.
`save` writes atomically (temp file, rename) and refuses a name that is
empty, that contains a path separator, or that is already a user preset of
that device unless `overwrite` is asked for — which is what "Save" (as
opposed to "Save as…") passes.

### P.5 The document: a device remembers its preset's name

`Channel::preset: Option<PresetRef>` and `EffectSlot::preset:
Option<PresetRef>`, both `#[serde(default, skip_serializing_if)]` so every
project written before reads and writes unchanged.

Two commands in `fontelle-model`:

- `ApplyPreset { target, preset: Preset }` where `target` is a channel or an
  insert slot. For a channel it writes `instrument`, `patch_data` **or**
  `plugin`, and `preset` — one entry, one undo — and if the preset's device
  is a different `InstrumentKind` from the channel's it **switches the
  channel's kind** (FL's behaviour: choosing a Flopsynth preset on a
  soundfont channel makes it a Flopsynth channel). For an insert it writes
  `config` or `plugin` and `preset`; a preset for a different `EffectKind`
  than the slot holds is refused by the command (a slot's kind is chosen from
  the "+ effect" menu, not by a preset).
- `SetPresetRef { target, preset: Option<PresetRef> }` — what a save writes
  after the file is on disk, so an undo of the save puts the old name back
  (the file stays; undo does not delete files).

`SetInsertPreset` (index into a constructor list) and
`StudioHost::set_instrument_preset` **go**; `ApplyPreset` replaces both.

### P.6 The `*` rule

Ty's decision replaces the second half of catalogue rule 10 ("nothing
remembers which preset it came from"). The new rule, for every device:

> **The name is remembered; the cleanliness is recognised.** A device
> carries the `PresetRef` it was loaded from and keeps it through every
> edit. Whether it is *dirty* is never stored: `Session::preset_state`
> compares the device's current payload with the bank's payload for that
> ref, and the bar draws `Name*` when they differ, when the ref names a
> preset the bank no longer has, or when the device has no ref at all and
> its state is not the device's own `new()`/`init()`.

The comparison is one `PartialEq` on a value the bank already holds (factory
in memory, user cached), per device window, per revision — cheap. `undo`
after an edit makes the `*` go out because the payload matches again, with
nothing to remember.

### P.7 The preset bar (`fontelle-ui/src/canvas/preset_bar.rs`)

One widget, drawn into the header of **every** editor window — the generic
instrument grid, the Flopsynth window, the effect grid, the EQ curve, a
hosted plugin's parameter panel — and nowhere else:

```
│ ◀ ▶ │ Choir Ahh*  ▾ │ Choir & Vocal │ ★ │ Save │ Save as… │
```

- `◀ ▶` load the previous/next entry of `for_device`, wrapping, one undo
  entry each. On an instrument with the transport stopped the new preset is
  auditioned through the preview voice (`preview_preset`'s path, aimed at
  the channel's own patch), so walking the bank is heard.
- The name is a drop-down (`ContextMenu`, with headings): **Favourites**
  first (the rule every menu here follows, `favorites.rs`), then each
  category as a heading with its rows, factory and user rows tagged by a
  small mark in the row's right end. Choosing a row is `ApplyPreset`.
- `★` toggles `Favorite::Preset` (§P.10).
- **Save** is enabled only when the bar shows `*` *and* the ref is a user
  preset; it overwrites that file and clears the `*`. On a factory preset it
  is drawn disabled with the tip "factory presets are read-only — Save as…".
- **Save as…** opens the existing name prompt (`name_prompt_entries`),
  prefilled with the current name, followed by a category chooser listing
  the device's categories plus "New category…"; it writes a user file, then
  `SetPresetRef`, and the bar shows the new name clean.
- A device with no ref shows "— no preset —" and Save disabled; Save as…
  always works.
- Pure geometry (`preset_bar_layout`, `preset_bar_hit`) and a view struct
  `PresetBarView { name, category, origin, dirty, favourite, can_save }`
  the host builds. Right-click on the name: "Reveal in folder" (user), "Copy
  name".

### P.8 The browser's Presets tab

A fifth `BrowserMode::Presets`, beside Sounds / Projects / Import / Settings,
because a preset is something you go looking for the way you go looking for a
soundfont:

- Rows are a folder walk like the Import tab's: device → category → preset,
  with `..` rows back up, a search across every device's presets
  (`PresetBank::search`), and a star on every preset row.
- **Click** on an instrument preset applies it to the **selected channel**
  (switching its kind if needed, §P.5) and auditions it; on an effect preset,
  applies it to the insert whose window is open, and is drawn disabled with
  the tip "open an effect's window first" when none is; on a plugin preset,
  likewise by whichever the device is.
- Right-click: delete (user only, after the projects tab's confirmation),
  reveal folder, rename (save as + delete, one gesture).
- The status line says where the user bank is and counts both origins.

### P.9 What moves, and what goes

- `DistortionConfig::from_preset`, `BitcrushConfig::from_preset`,
  `SoftenConfig::from_preset`, `DrumKitStyle::ALL → drum_kit(style)` and
  every other constructor-preset become **generators**: an xtask,
  `cargo xtask export-factory-presets`, writes each as a file under
  `assets/presets/` once, the files are committed, and the constructor code
  is deleted along with `EffectConfig::{presets, apply_preset,
  matching_preset}`, `DrumKitStyle::matching`, `InstrumentView::{presets,
  preset}`, the chip row and its layout (`chip_row` stays for the key row).
  The catalogue's reference designs (§3.1, §3.2) remain the documentation of
  what those files contain.
- `DrumKitStyle` and `KitCharacter` **stay** as the drum machine's authoring
  tool and its tests' fixture; they simply no longer sit behind the panel.
  Flopsynth's recipes (§7.1) are built the same way from the start.
- The test `whether_an_effect_ships_presets_is_a_decision_taken_for_every_one`
  becomes: every effect with more than eight parameters has at least one
  factory file under its slug, or is on the no-presets list with its reason
  (utility, gate, filter — unchanged).
- Rule 10's first half stands: a preset is a constructor, not a parameter;
  there is no "preset" knob and no lane can sweep one.

### P.10 Favourites

`Favorite::Preset { device: DeviceKind, name: String, origin: PresetOrigin }`
— the fourth variant the `favorites-and-stars` note anticipated.
`SETTINGS_FORMAT_VERSION` 4 → 5; a file without the variant reads as before.
The favourites section is first in the bar's drop-down and in the tab.

### P.11 Tests (written first, every one)

| File | Holds |
|---|---|
| `fontelle-types/tests/preset.rs` | round-trip of every payload kind; `is_consistent` refuses a mismatch; every slug is a valid folder name and the table of slugs is frozen (a test with the literal strings, so a rename is a conscious break); a v2 file is refused as from the future |
| `fontelle-app/tests/preset_bank.rs` | the factory list is non-empty and every factory file loads and is consistent; a scratch user folder scans, saves atomically, loads back equal, deletes, lists an unreadable file by path; `for_device` order is factory-then-user, category-then-name; `find` by ref; a name with a separator is refused; `overwrite` semantics |
| `fontelle-model/tests/presets.rs` | `ApplyPreset` writes kind + payload + ref in one entry and inverts to all three; a kind switch on a channel; an effect preset of the wrong kind is refused; `SetPresetRef` undoes |
| `fontelle-app/tests/preset_bar.rs` | a fresh device shows no preset; after loading one, its name; after a knob, `Name*`; after Save (user) clean; Save disabled on factory; Save as… writes a file and shows the new name clean; prev/next wrap and cost one undo each; undo after an edit clears the `*` by recognition; loading an instrument preset onto a channel of another kind switches it; an effect preset lands in the open insert; a plugin payload restores through the rack (against `fontelle-testplug`) |
| `fontelle-ui/tests/preset_bar.rs` | layout at three widths; hits for all seven controls; the drop-down's rows have favourites first then category headings; disabled Save is not hit |
| `fontelle-ui/tests/browser_presets.rs` | the tab lays out; folder rows walk device → category → preset; search filters across devices; the disabled effect row when no insert window is open |
| `xtask` | `export-factory-presets` is idempotent and every generated file round-trips through the bank |

### P.12 Phases

- **P.1 — types and format.** `preset.rs`, the settings field, the
  favourite variant, the two commands. Gate: types/model suites green, every
  existing project and settings file still opens.
- **P.2 — the bank and the factory.** `preset_bank.rs`, `build.rs`, the
  xtask export, the constructor presets exported and deleted, the chip row
  removed, the drum machine's kits as files. Gate: every test that named a
  constructor preset now names a file; `cargo test --workspace` green; the
  drum machine's kit test (`every_pair_of_kits_is_audibly_apart`) reads the
  files.
- **P.3 — the bar.** In every editor window, with the `*` rule, Save and
  Save as…, prev/next with audition. Gate: `preset_bar.rs` suites green;
  driven in the real window on an effect and on the drum machine.
- **P.4 — the tab.** Browser mode, search, stars, click-to-apply with the
  kind switch, delete/rename. Gate: suites green; seen in the window.

Then Phase 0 of Flopsynth begins, with a preset system to land its bank in.

---

## 3. The sound architecture

```
                 ┌─────────── per voice (fixed topology, INVARIANT 6) ───────────┐
  note ─────────►│ OSC A ──┐                                                      │
                 │ OSC B ──┼─► (FM/RM/sync between A←B←C) ─┐                      │
                 │ OSC C ──┘                               │  route per layer:    │
                 │ SUB ────────────────────────────────────┼─► F1 │ F2 │ F1→F2 │ – │
                 │ NOISE ──────────────────────────────────┘        │             │
                 │                                          F1 ─► F2 ─► AMP(env1)─► pan ─┐
                 │       ENV 1..4   LFO 1..4   velocity key AT wheel bend           │
                 │       macros 1..4  random  counter  note X/Y                     │
                 │              └──────── mod matrix (≤32 routes, via, curve) ──────┘
                 └────────────────────────────────────────────────────────────────┘
  Σ voices ─► output trim ─► FX 1 ─► FX 2 ─► FX 3 ─► FX 4 ─► channel gain/pan ─► bus
```

Everything above the "Σ voices" line is `fontelle-core`'s `Voice`, extended;
everything below is `SamplerNode`. The sections that follow specify each box.
Where a parameter is named, its **address** is in §4 and its **range and
taper** in §4's table; this section is about behaviour.

### 3.1 Oscillators (`SynthOsc`, in `fontelle-dsp/src/synth_osc.rs`)

One description covers A, B, C, the Sub and the Noise; the window shows fewer
controls for the last two.

```rust
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SynthOsc {
    pub source: SynthSource,          // Table(WavetableId) | Noise
    pub position: f32,                // 0..1 across the table's frames
    pub warp: WarpMode,               // Off, Bend, Sync, Mirror, Quantise, Fm, Rm
    pub warp_amount: f32,             // 0..1 — the `shape` continuum of the warp
    pub modulator: Option<u8>,        // which later layer feeds Fm/Rm/Sync
    pub unison: Unison,               // voices 1..8, detune cents, blend, width
    pub phase: f32,                   // 0..1 start phase
    pub random_phase: bool,
    pub semitones: i8,                // −36..+36
    pub key_track: bool,              // false = fixed pitch (drones, noise)
    pub filter_route: FilterRoute,    // F1, F2, Serial, Bypass
    pub noise_colour: f32,            // Noise only: 0 white, 0.5 pink-ish, 1 brown
}
```

- **Wavetables** are the source. `WavetableId` names a built-in table
  (§3.2); `position` picks a frame with linear interpolation between adjacent
  frames, and the frame is read with linear interpolation between samples at
  the mip level chosen for the pitch. Four table reads per unison voice.
- **Unison** is per oscillator (Serum's model, not `VoiceConfig::unison`,
  which stays as it is: declared, unimplemented, and not this plan's
  business). Voices are spread symmetrically in cents around the centre
  (`detune` is the *outermost* voice's offset; inner voices at
  `±detune · k/(n−1)` with a slight exponential bias so eight voices do not
  stack), `blend` is the level of every voice but the centre one, and `width`
  pans the side voices out from the centre in alternating pairs. A single
  voice is exactly the centre voice: unison at 1 costs nothing and changes
  nothing (`one_unison_voice_is_the_plain_oscillator`).
- **Phase.** `random_phase` gives each unison voice and each note a fresh
  start from an xorshift seeded by the voice's age counter, so tests are
  deterministic; `phase` is the start phase when it is off. The centre voice
  of a random-phase stack still starts at `phase` so a bass note with one
  voice does not lose its click-free start.
- **Warp modes**, each a continuum under `warp_amount` (catalogue rule 1):
  *Bend* skews the read phase (`t^(2^(±k))`, sign from the amount's halves);
  *Sync* is hard sync of the table read at 1..8× the note's pitch, band-limited
  by reading the mip level for the *synced* frequency; *Mirror* folds the phase
  back on itself (a PWM-like asymmetry); *Quantise* holds the read to
  `2..64` steps per cycle — the digital grit; *Fm* is through-zero phase
  modulation by the modulator's sample (index in radians up to 4π); *Rm*
  multiplies by the modulator. Fm/Rm/Sync name a **later** layer as the
  modulator, so the sample loop evaluates Flopsynth layers **last to first**
  and the modulator's sample for this frame already exists. The panel refuses
  a modulator that is not a later layer.
- **Noise** is the existing xorshift white noise through a one-pole tilt
  whose corner `noise_colour` sets (white → pink-ish → brown), read at audio
  rate, ignoring `position`, `warp` and unison.
- **The Sub** is a Synth layer whose table is one of the *Sub* family
  (§3.2) and whose `semitones` is −12 or −24; nothing else about it is special.
- Every oscillator costs what its unison count says and nothing when its level
  is at the floor: a layer at `SILENT_DB` is skipped in `trigger_note`, which is
  what makes an Init patch with B, C, Sub and Noise "off" cost one oscillator.

**Tests, in `fontelle-dsp/tests/synth_osc.rs`** (written first, against
`todo!()`): a table plays the note's frequency (period measured by
zero-crossings over 64 cycles); a saw table at 5 kHz has less energy below its
fundamental than a naive saw (the existing `energy_below` measurement, moved
to a shared test helper); unison at 1 equals the plain oscillator sample for
sample; unison at 7 has seven distinct spectral lines around the fundamental;
`blend` at 0 is the centre voice alone; every warp mode at amount 0 is the
unwarped table and at amount 1 is measurably different from it and from every
other mode (the drum kit's "apart" test, on spectral centroid and crest);
Fm from a modulator produces sidebands at `f_c ± f_m`; sync at 3× produces a
spectrum whose lines are at multiples of the note, not of the synced
frequency; noise colour 0 is flat and 1 falls at ~6 dB/oct; `random_phase`
yields two different first samples for two triggers and identical ones when
off.

### 3.2 Wavetables (`fontelle-dsp/src/wavetable.rs`)

No files: every table is **generated** from a spectrum recipe, so Flopsynth
works on a fresh install and a preset never breaks a link. That is the drum
machine's position taken to the oscillators.

- A `Wavetable` is `frames × mip levels`; frame length 2048 at level 0,
  halving per level (1024, 512, … 32); level *k* holds harmonics ≤ `1024 >> k`.
  A frame's mip pyramid is 4096 samples (16 KB); a 32-frame table is 512 KB;
  the whole bank of ~36 tables is ~16 MB if every one is built, and they are
  built **lazily**: `WavetableBank::get(id)` builds on first use behind a
  `Mutex<HashMap<WavetableId, Arc<Wavetable>>>` in a process-wide `OnceLock`,
  called only from `Sampler::prepare` (off-RT; locking and allocating are
  fine there and forbidden in `render`).
- Generation: for each frame, fill a 1024-bin spectrum from the recipe
  (`amplitude(harmonic, frame_t)` and a phase — zero for most, alternating for
  the Analog family so a square has its correct symmetry), inverse-transform
  with `fft_in_place` (conjugate trick), normalise the peak of **level 0** to
  0.9 and use the same gain for every level so mip switching does not change
  loudness. Level selection at render: the smallest *k* with
  `f0 · (1024 >> k) ≤ 0.45 · sample_rate`.
- `WavetableId` (`Copy`, serde by name, `label()`, `family()`, `ALL`) — the
  first-ship bank, grouped as the drop-down groups them:

  | Family | Tables | What the frames do |
  |---|---|---|
  | **Basic** | Sine, Triangle, Saw, Square, Pulse | Pulse: width 50 % → 5 % across frames. The others are one frame. |
  | **Analog** | Analog Morph, PWM Sweep, Sync Sweep, Sub Saw | Analog Morph: sine → tri → saw → square. Sync Sweep: a saw hard-synced 1× → 6× baked into frames, so a *static* sync tone costs a table read. |
  | **Harmonic** | Drawbar, Odd, Even, Sawstack, Bright Stack, Hollow | Drawbar: the nine Hammond drawbars faded in one at a time across frames. Odd/Even: only those partials, rolling off faster with frame. |
  | **Vocal** | Vowel, Choir, Formant Sweep | Vowel: A → E → I → O → U as a formant envelope over a saw's harmonics (F1–F3 from the standard vowel table). Choir: the same with the partials slightly detuned per frame and a breath band. |
  | **Bell** | FM Bell, Glass, Tine, Gong | Two-operator FM at inharmonic ratios (1:1.41, 1:3.5) baked per frame at rising index; Tine is the DX e-piano ratio 1:14 at low index. |
  | **Digital** | Grit, Stairs, Crunch, Bitwave | Quantised and folded shapes; harmonics decaying slower than 1/n. |
  | **Modern** | Growl, Reese, Hoover, Wide | Reese: two detuned saws' beating captured across a cycle of frames. Hoover: a saw with a PWM'd square and octave-up, chorused. |
  | **Sub** | Sub Sine, Sub Tri, Sub Square | One frame each, a family so the Sub's drop-down is short. |
  | **Chip** | NES Pulse 12.5/25/50, NES Triangle, Game Boy, C64 | The 4-bit triangle *is* 15 steps; the pulses are the exact duty cycles. Toby Fox's territory (`fopull-llc-and-ty`), so these are first-class and not a novelty. |

  Thirty-six. Adding one later is a recipe function and a variant; a table's
  name is INVARIANT 7's the moment a patch is saved naming it.

**Tests, in `fontelle-dsp/tests/wavetable.rs`:** every table builds and every
level's peak is ≤ 1.0; a mip level *k* has no energy above harmonic
`1024 >> k` (a windowed DFT — the drum-kit memory's lesson about sparse
probes applies); frame interpolation at position 0.5 between two frames is
their mean; the Vowel table's frame 0 has its energy peaks at A's F1/F2 (730
and 1090 Hz, within a bin); the NES Triangle has exactly 15 distinct levels;
building the whole bank stays under 24 MB (a counting allocator in the test)
and under 400 ms in debug; a second `get` returns the same `Arc`.

### 3.3 Filters (`FilterSlot`, extended)

`FilterSlot` gains, all `#[serde(default)]` so every existing patch reads as it
did:

```rust
pub slope: FilterSlope,      // Db12 (today's SVF) | Db24 (two cascaded)
pub model: FilterModel,      // Clean | Ladder | Formant | Comb
pub drive: f32,              // 0..1 → 0..+24 dB into a tanh before the filter
pub key_track: f32,          // 0..1: cutoff follows the key from middle C
pub character: f32,          // per model, see below
```

- **Clean** is the SVF as it is; `Db24` cascades two with the Q split
  (0.54 / 1.31 for Butterworth at resonance 0, scaled by the resonance knob
  above that). The seven `SvfMode`s stay available; the window offers LP, HP,
  BP, Notch, Peak (the shelves and bell are EQ shapes and stay reachable by
  address, not by the drop-down).
- **Ladder** is four cascaded one-pole TPT stages with `tanh` in the feedback
  loop and resonance 0..4 (self-oscillation at the top). `character` is the
  saturation amount in the loop. Not oversampled in v1; the drive's `tanh` is
  before the filter as in the Filter insert (catalogue §3.6, the argument is
  the same and the test `the_drive_is_before_the_filter` is copied).
- **Formant** is three SVF band-passes in parallel at a vowel's F1–F3 with
  the vowel table's per-formant gains. `cutoff` scales all three formants
  (an octave up is a child's voice), `resonance` is their bandwidth,
  `character` morphs A → E → I → O → U with linear interpolation of the
  formant frequencies. This is the choir and the talk-box, and it is why
  "Choir Ahh" can be built without a sample.
- **Comb** is a feed-forward/feedback comb with delay `1/cutoff`;
  `character` is the feedback amount (signed around 0.5, so the hollow one is
  reachable); `resonance` is the damping in the loop. The delay line is
  **1024 samples per channel per slot**, allocated with the voice, which puts
  the lowest cutoff at 47 Hz at 48 kHz and adds 8 KB to a voice.
- **Drive** is `tanh(x · gain)` before the filter, with the make-up so a
  drive at 0 is a wire (`a_fresh_filter_is_a_wire`).
- **Modulated cutoff is smoothed within the block.** Today the coefficients
  are built once per block. With an LFO on cutoff at 375 Hz block rate that is
  audible zipper. Cutoff (in cents) is ramped linearly from the previous
  block's value to this one's and the coefficients rebuilt every
  `FILTER_STEP = 8` samples along the ramp; the SVF's zero-delay topology is
  what makes this stable. A patch with no route to cutoff pays the old cost.
- **Routing** is per layer (`SynthOsc::filter_route`), and the voice mixes
  four buses before the filters: to F1, to F2, to F1→F2, and around both.
  Serial is F1 then F2 as today; "F1" alone goes to the amp without F2. The
  arithmetic is four stereo pairs per voice instead of one.

**Tests, in `fontelle-dsp/tests/filters.rs`** (new; the SVF's own tests stay
in the module): a 24 dB low-pass is 24 dB down an octave above cutoff where
the 12 dB is 12; the ladder self-oscillates at resonance 4 with the input
silent and does not blow up (peak < 2.0 over a second); the formant filter at
vowel A puts its two loudest peaks at A's formants and at vowel I at I's;
the comb at 100 Hz puts nulls at odd multiples of 50 Hz with negative
feedback; key tracking at 1.0 moves the corner one octave for twelve keys;
drive at 0 is a wire sample for sample; a cutoff swept by a route shows no
step larger than 3 dB between adjacent samples at 8 kHz
(`modulated_cutoff_does_not_zipper`).

### 3.4 Envelopes

`EnvelopeConfig` gains `attack_shape`, `decay_shape`, `release_shape` (−1..1,
`#[serde(default)]` 0 = today's straight line), applied as a power curve to
the stage's progress: `t^(2^(2·shape))`. Flopsynth's four envelopes all use
`EnvelopeCurve::Linear` with shapes; the amp envelope's default decay and
release shapes are −0.6, which sounds like the exponential fall an ear
expects while keeping stage *times* meaning the time the stage takes (the
`Decibel` curve's "time to travel 100 dB" reading is right for SF2 and
confusing under a shape knob — see `EnvelopeCurve` docs).

Envelope 1 is the amp envelope, as everywhere; 2 is routed to filter 1's
cutoff in the Init patch at depth 0, so the first thing somebody turns up does
what they expect; 3 and 4 are free. All four are mod sources; `Envelope(0)` is
readable as today.

**Tests, in `fontelle-dsp/tests/envelope_shapes.rs`:** shape 0 is bit-identical
to today's envelope over a full AHDSR; shape −1 on decay reaches half level
before a quarter of the stage; shape +1 after three quarters; the stage's
*duration* is unchanged by its shape; a mod route on `EnvelopeStageTime`
still moves it mid-stage (the existing property, re-asserted against the new
code).

### 3.5 LFOs

`Lfo` becomes:

```rust
pub struct Lfo {
    pub wave: fontelle_types::LfoWave,   // was `shape: OscKind` — migration, §5
    pub rate_hz: f32,
    pub sync: bool,
    pub division: fontelle_types::NoteDivision,
    pub depth: f32,
    pub delay_s: f32,
    pub fade_s: f32,                      // depth ramps in over this after the delay
    pub phase: f32,                       // start phase, 0..1
    pub mode: LfoMode,                    // Retrigger | Free | OneShot
    pub smooth: f32,                      // one-pole on the output, for S&H and square
}
```

- **`LfoWave` is `fontelle-types`'s**, the same six the Filter insert uses,
  so the chooser names its positions once (catalogue rule 3) and the shape the
  window draws is the shape the voice plays (`LfoWave::value`). Sample & hold
  needs memory, so the voice's `lfos: [Oscillator; 4]` becomes
  `[LfoState; 4]` — phase, held value, rng, fade level — `Copy`, fixed.
- **Sync** needs the tempo on the voice. `Sampler::render` gains a
  `RenderClock { bpm: f32, position_sample: u64 }` argument, filled by
  `SamplerNode` from `ProcessContext::transport` (which already carries both).
  A synced rate is `bpm / 60 / division.beats()`.
- **Free** mode derives the phase from the clock —
  `(position_sample / sample_rate · rate + phase) mod 1` — so every voice
  reads the same LFO, it is the same on every play from the same bar, and it
  costs no shared state. Retrigger is today's behaviour; OneShot stops at the
  end of the first cycle and holds.
- `delay_s` stays the SF2 delay; `fade_s` ramps the depth from 0 after it.

**Tests, in `fontelle-core/tests/lfo.rs`:** a synced 1/4 at 120 bpm completes
a cycle in exactly 24 000 samples; two voices in Free mode read identical
values in the same block; OneShot holds its last value; fade reaches full
depth after `fade_s`; sample & hold changes value only at cycle boundaries
and `smooth` rounds the step; a v0 patch's `shape: "Saw"` reads as `SawUp`.

### 3.6 The mod matrix

`ModSource` gains `Macro(u8)`; `Random` and `NoteOnCounter` are **implemented**
(they read as zero today): Random is a per-note value from the voice's xorshift
seeded by its age counter, NoteOnCounter cycles `0, 1/7, …, 1` per note-on so
an eight-step alternation is a route with `Curve::Quantised { steps: 7 }`.

`ModDest` gains, each indexed by layer or slot as the existing ones are:
`OscPosition(u8)`, `OscWarp(u8)`, `OscUnisonDetune(u8)`, `OscUnisonBlend(u8)`,
`FilterDrive(u8)`, `FilterCharacter(u8)`, `LfoPhase(u8)` and one voice-wide
`Amp` (post-envelope gain in dB, so a tremolo is one route rather than three).
`full_scale` is extended: position, warp, blend and drive are 1.0 (the whole
knob); detune is 100 cents; `Amp` is 24 dB.

`ModRoute` is unchanged in shape. `MAX_ROUTES = 32` is a window limit, not a
type limit — `evaluate` is O(routes) per destination per block, and thirty-two
routes over twenty destinations is 640 comparisons per voice per block, which
is nothing. `patch/mod[n]/depth` is the one addressable field of a route (§4);
source, destination, curve and via are structure.

**The window's own destinations list** is built by `flopsynth::destinations
(&patch)`, which returns every `ModDest` a Flopsynth patch has with its label
("Osc A position", "Filter 1 cutoff"), and the panel offers nothing else — a
route to `SampleStartOffset` is possible by file and meaningless here.

**Tests, in `fontelle-core/tests/mod_matrix.rs`** (new file; the module's own
unit tests stay): every new destination moves the thing it names (rendered,
measured: position by spectral centroid, warp by the same, detune by line
count, drive by harmonic content, Amp by RMS); a macro at 0.5 reads 0.5; two
notes read two different Randoms and the same note-on sequence reads the same
Randoms twice; NoteOnCounter cycles.

### 3.7 Macros

`Patch::macros: [Macro; 4]`, `Macro { name: String, value: f32 }`,
`#[serde(default)]` ("Macro 1".."Macro 4", 0). A macro is a source and nothing
else; its whole meaning is the routes that read it. `patch/macro[n]` is
automatable, which is how a preset's "Brightness" becomes a lane. The `String`
is touched off-RT only; the RT thread reads `value`.

### 3.8 Voice

`VoiceConfig::retrigger` (Poly/Mono/Legato) and glide exist; Phase 0 verifies
with a test that Mono and Legato actually behave (there is a `glide.rs` suite;
extend it rather than assume). Flopsynth adds nothing to `VoiceConfig` except
that `patch/voice/mode` and `patch/voice/bend_range` become addresses (§4).

`Patch::output_db` (`#[serde(default)]` 0) is a trim applied in
`Sampler::render` after the voice sum and before the channel's own gain. It
exists so presets can be **loudness-matched** (§7.4) without touching their
layers' balance.

### 3.9 The effects chain

Per §2.2. The kinds offered, in the "+ effect" drop-down's order: Chorus,
Delay, Reverb, Filter, EQ, Distortion, Bitcrush, Compressor. Each slot draws
its `EffectConfig::specs` grouped by `sections()` exactly as the effect window
does (the same `instrument_layout` inside a card), with its presets as a row
of chips, an enable switch and a mix knob — nothing new to draw, which is the
point of storing an `EffectConfig`. A slot can be dragged above or below its
neighbour (order is audible), which is a structural edit.

**Tests, in `fontelle-engine/tests/flopsynth_fx.rs`:** a patch with a delay
slot is audible after the last note-off for the delay's tail through the
live path with the transport stopped (`the_chain_rings_after_the_last_note_off`
— the idle gate measures, §2.2); a patch with no fx renders bit-
identical to the sampler alone; `patch/fx[0]/mix` written through the live
wire changes the next block; two slots run in order (a distortion before a
filter is not a filter before a distortion — measured as the catalogue does);
bypassing a slot is a wire; the chain allocates nothing in `process` (folded
into the no-allocation test).

---

## 4. The address table (INVARIANT 7)

Every row is permanent from the commit it lands in. All values normalised
0..1 on the wire; the taper is the parameter's. `n` is a layer index for
oscillators (A=0, B=1, C=2, Sub=3, Noise=4), a slot index otherwise.
Existing addresses are marked *(exists)* and keep their meaning; `set` for
`shape`/`octave`/`tune` on a `Source::Synth` layer is **extended** to accept
them (today it refuses non-`Oscillator` layers).

| Address | Kind | Range / positions | Taper |
|---|---|---|---|
| `patch/voice/polyphony` *(exists)* | knob | 1..256 | linear |
| `patch/voice/glide` *(exists)* | knob | 0..2 s | linear |
| `patch/voice/legato` *(exists)* | switch | | |
| `patch/voice/mode` | choice | Poly, Mono, Legato | stepped |
| `patch/voice/bend_range` | knob | 0..24 st | linear |
| `patch/output` | knob | −24..+12 dB | linear |
| `patch/quality` *(exists)* | choice | | |
| `patch/layer[n]/gain` *(exists)* | knob | −60..+12 dB, −60 = off | linear |
| `patch/layer[n]/pan` *(exists)* | knob, bipolar | −1..1 | linear |
| `patch/layer[n]/octave` *(exists)* | choice | −2..+2 | stepped |
| `patch/layer[n]/tune` *(exists)* | knob, bipolar | ±100 c | linear |
| `patch/layer[n]/synth/table` | choice | `WavetableId::ALL` | stepped |
| `patch/layer[n]/synth/position` | knob | 0..1 | linear |
| `patch/layer[n]/synth/warp_mode` | choice | Off, Bend, Sync, Mirror, Quantise, FM, RM | stepped |
| `patch/layer[n]/synth/warp` | knob | 0..1 | linear |
| `patch/layer[n]/synth/modulator` | choice | none, B, C (only later layers) | stepped |
| `patch/layer[n]/synth/semitones` | knob, bipolar | −36..+36 | linear, integer |
| `patch/layer[n]/synth/phase` | knob | 0..1 | linear |
| `patch/layer[n]/synth/random_phase` | switch | | |
| `patch/layer[n]/synth/key_track` | switch | | |
| `patch/layer[n]/synth/route` | choice | F1, F2, F1→F2, Bypass | stepped |
| `patch/layer[n]/synth/unison/voices` | knob | 1..8 | linear, integer |
| `patch/layer[n]/synth/unison/detune` | knob | 0..100 c | linear |
| `patch/layer[n]/synth/unison/blend` | knob | 0..1 | linear |
| `patch/layer[n]/synth/unison/width` | knob | 0..1 | linear |
| `patch/layer[n]/synth/noise_colour` | knob | 0..1 | linear |
| `patch/filter[n]/enabled` *(exists)* | switch | | |
| `patch/filter[n]/mode` *(exists)* | choice | LP, HP, BP, Notch, Peak (window); all seven by address | stepped |
| `patch/filter[n]/cutoff` *(exists)* | knob | 20 Hz..20 kHz | log |
| `patch/filter[n]/resonance` *(exists)* | knob | 0..1 (Q for Clean, 0..4 for Ladder) | linear |
| `patch/filter[n]/slope` | choice | 12, 24 dB | stepped |
| `patch/filter[n]/model` | choice | Clean, Ladder, Formant, Comb | stepped |
| `patch/filter[n]/drive` | knob | 0..1 | linear |
| `patch/filter[n]/key_track` | knob | 0..1 | linear |
| `patch/filter[n]/character` | knob | 0..1 | linear |
| `patch/env[n]/delay,attack,hold,decay,sustain,release` *(exist)* | knob | | cubic (`lerp_stage`) |
| `patch/env[n]/attack_shape`, `decay_shape`, `release_shape` | knob, bipolar | −1..1 | linear |
| `patch/lfo[n]/wave` | choice | `LfoWave::ALL` | stepped |
| `patch/lfo[n]/rate` | knob | 0.01..40 Hz | log |
| `patch/lfo[n]/sync` | switch | | |
| `patch/lfo[n]/division` | choice | `NoteDivision::ALL` | stepped |
| `patch/lfo[n]/depth` | knob | 0..1 | linear |
| `patch/lfo[n]/delay`, `fade` | knob | 0..10 s | cubic |
| `patch/lfo[n]/phase` | knob | 0..1 | linear |
| `patch/lfo[n]/mode` | choice | Retrigger, Free, One-shot | stepped |
| `patch/lfo[n]/smooth` | knob | 0..1 | linear |
| `patch/macro[n]` | knob | 0..1 | linear |
| `patch/mod[n]/depth` | knob, bipolar | −1..1 | linear |
| `patch/fx[n]/enabled` | switch | | |
| `patch/fx[n]/<effect param id>` | as the effect's `ParamSpec` | | the spec's |

Rules that hold the table:

- **`patch_params::set` and `value` are the whole of it**, extended with
  `patch/lfo[`, `patch/macro[`, `patch/mod[`, `patch/fx[` arms and a
  `synth/` sub-match inside `set_layer`. No allocation: `split_once` on
  `&str`, as today. A `patch/fx[n]/<id>` goes to
  `EffectConfig::set_normalised(id, t)` on the slot's config, which is already
  RT-safe.
- **`fontelle-core/tests/patch_params.rs`'s round-trip test covers every new
  row** by construction, because it iterates the address list the panel
  offers; `flopsynth::addresses(&patch)` is that list for a Flopsynth patch
  and `instrument::patch_addresses` dispatches to it, so `realise`'s
  `param_nodes` map and the window's controls are still one list
  (handoff §4, "`param_nodes` is what makes automation reach anything").
- A **choice** is `Taper::Stepped` in `ParamSpec` terms and `choice_value`/
  `choice_index` in `patch_params` terms; the endpoints rule (first and last
  option at 0 and 1) is the existing one.

---

## 5. Storage: the format bump and the migration

`PATCH_FORMAT_VERSION` goes **0 → 1**. Two things force it, by the rule on
the constant's docs ("changing what an existing field *means* always does"):
`Lfo::shape: OscKind` becomes `Lfo::wave: LfoWave`, and `Source` gains a
variant an older build would report as `Malformed` rather than
`FromTheFuture`, which is the wrong diagnosis ("your project is damaged"
against "upgrade Fontelle").

`migrate(body, 0)` → 1: for every `lfos[i]`, rename `shape` to `wave` and
map `Sine→Sine, Triangle→Triangle, Saw→SawUp, Square→Square, Noise→SampleHold`.
Everything else added in this plan is `#[serde(default)]` and needs no
migration; the v1 reader with defaults is the v0 reader.

`StoredSource` gains `Synth(SynthOsc)` — stored whole, like `Drum`, because it
names no file.

**Tests, in `fontelle-core/tests/patch_format.rs`** (a new integration
file; the module's own unit tests in `src/patch_format.rs` stay where they
are): a v0 body written by *this test* from a hand-built JSON (not from
`to_data`, which would write v1) reads and its LFO wave is `SawUp`; a v1 body
with none of the new fields reads as the same patch as the v0 body did; every
preset in the bank round-trips `to_data` → `from_data` equal; a v2 body is
refused as `FromTheFuture`; `basic_synth` and every drum kit round-trip
unchanged (the "old patches still load byte-equivalent" gate).

---

## 6. The Init patch

`Patch::flopsynth_init()` in `fontelle-core/src/flopsynth/mod.rs`, held to the
same reasoning `basic_synth` documents:

- A: Basic/Saw, position 0, unison 1, level −12 dB, route F1→F2.
  B: Analog/Analog Morph, off (`SILENT_DB`), semitones 0. C: Harmonic/Sawstack,
  off. Sub: Sub Sine, −12 st, off. Noise: white, off. Every one set up so
  turning its level up is the only step to hearing it.
- F1: Clean LP 24 dB, cutoff 20 kHz (open, so the knob only ever closes),
  resonance 0.2 (a little life at the corner), drive 0, key track 0. F2: HP 12,
  20 Hz, disabled.
- ENV 1: A 5 ms, H 0, D 0, S 1.0, R 150 ms, shapes 0 / −0.6 / −0.6.
  ENV 2: A 2 ms, D 300 ms, S 0, R 200 ms, routed to F1 cutoff at depth 0.
  ENV 3, 4: the same shape, unrouted.
- LFO 1: sine 5 Hz, retrigger, depth 1, unrouted. LFO 2: sine, synced 1/4,
  Free. LFO 3, 4: sine 1 Hz.
- Macros 1..4 unnamed, at 0. No fx. Polyphony 32 (not 64: sixteen voices of
  three eight-voice unison stacks is already a lot of oscillators, and the
  window says the number). Bend range 2. Output 0 dB.
- **Headroom:** a four-note chord at velocity 127 through the Init patch peaks
  under 0.8 (`the_init_patch_leaves_headroom`), the same argument
  `basic_synth` and `KIT_HEADROOM_DB` make.

---

## 7. The preset bank

### 7.1 Shape of the bank

**What ships is files** (§P.3): `assets/presets/flopsynth/<category>/<name>.json`,
one per preset, embedded into the binary, loaded through the same bank every
other device uses, with the name remembered on the channel and the `*` by
recognition (§P.6). Nothing in Flopsynth's own code lists a preset.

**What writes those files is code**, because two hundred and ten JSON files
of two hundred fields each are not reviewable and the drum kits' recipes
were. `fontelle-core/src/flopsynth/presets.rs` holds the **recipes over
archetypes**: a dozen `fn archetype_x(Recipe) -> Patch` builders (supersaw,
pluck, fm keys, formant voice, string section, brass, organ, bell, bass,
chip, atmos, sequence), each taking a small struct of the numbers that differ
between presets in that family, and a table `FACTORY: &[(Category, &str,
Recipe)]` of one row per preset. `cargo xtask export-factory-presets`
(§P.9) writes the table out as files, and a test holds that every committed
file equals what its row builds — so the recipe stays the reviewable truth
and the file stays the one that ships. Editing a preset by ear in the window
and pressing Save as… writes a user file; promoting it to factory means
transcribing it back into a row, which is the review.

Every preset is one row; a row reads as a sentence.

`FlopsynthCategory` (order is the browser's order): Bass, Lead, Pad, Keys,
Pluck, Strings, Brass & Winds, Choir & Vocal, Organ, Bells & Mallets, Chip &
Retro, Sequence & Arp, Atmos & FX, Synth Drums. Fourteen.

### 7.2 The catalogue

Notation: `A/B/C` oscillators as *table@position ×unison* ; `F1`/`F2` as
*model shape cutoff res* ; `E2→cut` is the filter envelope's depth ; `fx` in
chain order. Everything not named is the Init value. The implementing agent
tunes the numbers by ear against the tests in §7.4; the rows are the design.

**Bass (18)** — every one has its output normalised so that switching between
them does not jump, and every one has the Sub on **except** the three whose
filter is a narrow window (Metallic, Bowed, and Fretless at −25 dB): the Sub
goes *around* the filter, so on those it would be most of what came out and the
body the row was built for would never be heard.

| Name | Recipe |
|---|---|
| Init Bass | A Saw, Sub sine −12, F1 ladder LP 1.2 kHz res 0.3, E2→cut 0.4, ENV1 R 80 ms |
| Sub Sine | A Sub Sine, Noise 0.05 white 10 ms burst (ENV3 on noise level), no filter |
| Reese | A Modern/Reese@0.4 ×2 det 12, B Saw −12 ×2 det 8, F1 LP24 900 Hz, fx Chorus (ensemble, 2 voices) |
| Acid | A Saw, F1 ladder LP 600 Hz res 2.8 char 0.6, E2→cut 0.7 D 180 ms, glide 60 ms mono legato, fx Distortion Overdrive mix 0.4 |
| Moog Stack | A Saw, B Square −12, C Tri −12 det +7c, F1 ladder LP 2 kHz res 1.0, mono, glide 30 ms |
| FM Growl | A Sine, B Sine +19 st as FM modulator (A warp FM 0.6, LFO1→A warp 0.3 at 1/8 sync), Sub, F1 LP 3 kHz |
| Pluck Bass | A Analog Morph@0.7, Sub, ENV1 D 350 ms S 0, E2→cut 0.8 D 120 ms, F1 LP24 400 Hz |
| Wobble | A Reese@0.2 ×3, F1 ladder LP 300 Hz res 1.5, LFO1 (sync 1/4, S&H off, sine) → cut 0.8 via Macro 1 "Wobble rate" → LFO1 rate |
| Chip Bass | A Chip/NES Pulse 25 %, Sub NES Triangle, no filter, ENV1 R 20 ms, fx Bitcrush 12-bit sampler mix 0.3 |
| Distorted | A Saw ×3 det 15, Sub, F1 LP 1.5 kHz, fx Distortion Fuzz mix 0.5, EQ low shelf +3 dB at 80 Hz |
| Warm Round | A Tri, B Sine −12, F1 LP12 800 Hz key track 0.5, ENV1 A 15 ms R 250 ms |
| Neuro | A Growl@0.5 ×2, B Grit ×2 +12 (RM into A 0.5), F1 comb 220 Hz char 0.7 → F2 LP 2 kHz, LFO2 free 1/16 → F1 cut 0.5, fx Distortion, Chorus |
| Upright | A Tri −12 ×1, B Saw −12 −26 dB, F1 comb 170 Hz char 0.55 key track 1.0 → F2 LP 1.6 kHz, Noise 0.5 −28 dB gated by E3 30 ms, ENV1 D 1.1 s S 0, mono glide 50 ms |
| Slap | A Square, F1 ladder LP 620 Hz res 0.78 char 0.5, E2→cut 0.9 D 55 ms, ENV1 D 500 ms S 0.12, Noise −24 dB → F2 HP 4 kHz gated by E3 12 ms, fx Distortion Soft clip |
| Rubber | A Square warp Mirror 0.45, F1 ladder LP 460 Hz res 0.55 char 0.25, E2→cut 0.55 D 200 ms, ENV1 D 600 ms S 0.35, Macro 3 "Hollow" → A warp, fx Chorus |
| Fretless | A Tri, Sub −25 dB, B Sine +19 −20 dB, F1 LP 9 kHz key track 0.6 → F2 comb 300 Hz char 0.85, E2→cut 0.5, ENV1 S 1.0 R 800 ms, mono glide 140 ms, LFO1 vibrato on the wheel |
| Metallic | A Saw warp **RM** 1.0 by B Sine +11 st (silent), **no Sub**, F1 LP 6 kHz, ENV1 D 1.4 s S 0.22, Macro 3 "Metal" → A warp |
| Bowed | A Sawstack@0.35 ×2, **no Sub**, F1 formant char 0.6 900 Hz → F2 LP 3 kHz, ENV1 A 300 ms R 700 ms, LFO1 late vibrato, fx Ensemble |

**Lead (18)**

| Name | Recipe |
|---|---|
| Init Lead | A Saw ×3 det 10, F1 LP24 4 kHz, mono legato glide 40 ms, LFO1 sine 5.5 Hz → pitch 8 c via wheel, delay 1/8 mix 0.2 |
| Supersaw | A Saw ×7 det 22 blend 0.8 width 0.9, B Saw ×7 +12 st level −10, F1 LP 8 kHz, fx Chorus, Reverb size 0.4 mix 0.2 |
| Sync Lead | A Sync Sweep@0.3 warp Sync 0.5, E2→A warp 0.6 D 250 ms, F1 LP 6 kHz res 0.4, mono glide 20 ms |
| Square Lead | A Pulse@0.3, LFO1 tri 0.3 Hz → A position 0.2 (slow PWM), F1 LP 5 kHz, vibrato LFO2 delay 300 ms fade 400 ms |
| Hoover | A Modern/Hoover@0.5 ×4 det 30, B Saw −12 ×2, F1 LP 7 kHz, fx Chorus ensemble, Delay ping-pong 1/8 mix 0.25 |
| Soft Sine | A Sine, B Tri −12 level −18, F1 LP 3 kHz, ENV1 A 40 ms R 400 ms, LFO1 → pitch 6 c delay 250 ms, Reverb mix 0.3 |
| Bright Pulse | A NES Pulse 12.5 %, ENV1 R 30 ms, fx Delay 1/8 dotted mix 0.3 |
| Screamer | A Saw ×2 det 12, F1 ladder LP 3 kHz res 2.5 char 0.8, fx Distortion Amp mix 0.6, Delay |
| Whistle | A Sine +12, F1 BP 4 kHz res 0.8, LFO1 5 Hz → pitch 10 c, Noise 0.15 through F1, ENV1 A 60 ms |
| Talk Lead | A Saw ×2, F1 Formant char 0.2 res 0.5, LFO1 tri 0.4 Hz → F1 character 0.5, Macro 1 "Vowel" → F1 character |
| Portamento | A Saw, B Square −12 −8 dB, F1 LP 4 kHz, mono legato glide 180 ms, ENV1 R 300 ms |
| Retro Lead | A C64@0.5, F1 LP 6 kHz, LFO1 square 6 Hz → pitch 40 c via wheel (the arcade trill), fx Bitcrush 8-bit console mix 0.2 |
| Fifths | A Saw ×2, B Saw +7, C Saw +12, ENV1 R 150 ms, fx Chorus |
| Bell Lead | A Sine warp FM 0.35 by B Sine +19 (silent), ENV1 D 1.2 s S 0.25, E3 D 350 ms → index, Macro 3 "Bite", fx Delay 1/8· |
| Ring Lead | A Square warp **RM** 0.8 by B Sine +14 (silent), LFO2 0.25 Hz → warp, Macro 3 "Drift", fx Ping-pong 1/8 |
| Reso Sweep | A Saw ×2, F1 ladder LP 700 Hz res 0.85 char 0.55, E2→cut 0.8 A 10 ms D 500 ms S 0.15, fx Delay 1/4 |
| Growl Lead | A Modern/Growl@0.6 ×2 det 14, F1 ladder LP 3 kHz char 0.6, LFO2 0.18 Hz → position, Macro 3 "Growl", fx Distortion Tube |
| Sub Lead | A Sub Tri −12 ×1, Sub Sine −24 st around the filter, F1 LP 1.4 kHz, fx Distortion Soft clip |

**Pad (18)**

| Name | Recipe |
|---|---|
| Init Pad | A Saw ×5 det 14, B Saw −12 ×3 −6 dB, F1 LP 2.5 kHz, ENV1 A 600 ms R 1.2 s, LFO1 0.2 Hz → cut 0.2, fx Chorus, Reverb size 0.7 mix 0.35 |
| Warm Analog | A Analog Morph@0.55 ×4, B Tri −12, F1 ladder LP 1.8 kHz char 0.3, ENV1 A 800 ms R 1.5 s |
| Glass | A Bell/Glass@0.3 ×3 det 6, B Sine +12 −14 dB, F1 LP 6 kHz, ENV1 A 1.2 s R 2 s, LFO1 0.1 Hz → A position 0.4, Reverb mix 0.45 |
| Choir Pad | *Choir & Vocal archetype*, F1 Formant char 0.35, ENV1 A 700 ms — see §7.3 |
| String Pad | *Strings archetype* with A 900 ms R 1.8 s |
| Dark Drone | A Hollow@0.6 ×2, B Sub Saw −12, F1 LP 700 Hz res 0.5, key track off on B, LFO1 free 0.05 Hz → cut 0.3, Reverb size 0.9 mix 0.5 |
| Shimmer | A Sine ×3 det 4, B Sine +19 −12 dB, C Sine +24 −18 dB, ENV1 A 1.5 s, LFO1..3 at 0.13/0.17/0.19 Hz → B/C level 0.3, Reverb size 0.85 mix 0.5, Delay 1/4 mix 0.2 |
| Evolving | A Formant Sweep@0.2 ×3, LFO1 free 0.07 Hz → A position 0.5, LFO2 0.11 Hz → F1 cut 0.3, F1 LP 3 kHz, Chorus, Reverb |
| Wide Digital | A Stairs@0.4 ×6 det 18 width 1.0, F1 LP 5 kHz, ENV1 A 400 ms R 1 s, Chorus ensemble 4 |
| Vox Air | A Choir@0.5 ×4 det 9, Noise 0.1 brown-ish through F2 BP 3 kHz, F1 Formant char 0.6, ENV1 A 900 ms, Reverb size 0.8 mix 0.4 |
| Filtered Saw | A Saw ×4 det 12, F1 LP24 1 kHz res 0.7, E2→cut 0.5 A 1.2 s D 2 s S 0.4 (the slow-open pad), Reverb |
| Dream | A Sine ×2 det 5, B Tri +12 −10 dB, F1 LP 4 kHz, ENV1 A 300 ms R 2.5 s, LFO1 0.3 Hz → pan 0.4, Delay 1/8 dotted mix 0.3, Reverb mix 0.4 |
| Halo | A Sine ×3 warp FM 0.18 by C Sine +26 (silent), B Sine +12, E2 A 2 s → index, Macro 3 "Bell", fx Reverb 0.9 |
| Bowed Glass | A Hollow@0.35 ×3, F1 comb 320 Hz char 0.7 key track 1.0 → F2 LP 6 kHz, ENV1 A 1.4 s R 2.6 s |
| Ice Field | A Bright Stack@0.5 ×4, B off, Noise 0.95 −26 dB → F2 HP 5 kHz, LFO2 0.09 Hz free → noise level, Macro 3 "Air", fx Reverb 0.95 |
| Reso Swell | A Saw ×4, F1 ladder **band-pass** 400 Hz res 0.9, E2→cut 0.75 A 2.5 s, ENV1 A 1.8 s R 3 s |
| Brass Pad | Brass on Bright Stack@0.5 ×4, C Saw +12, ENV1 A 500 ms R 1.8 s, E2→cut 0.65 A 500 ms, fx Ensemble + Reverb |
| Metal Pad | A Gong@0.4 ×2, B Gong@0.7 +7, LFO2 0.06 Hz free → B position, Macro 3 "Metal", fx Reverb 0.9 |

**Keys (17)** — the FM e-pianos are here because two-operator FM is what a
DX7 e-piano *is*; a grand piano is not attempted, and the reason is stated on
the category: it is a sampled instrument and Fontelle's soundfont player is the
right tool for one (§12).

| Name | Recipe |
|---|---|
| EP Tine | A Sine, B Sine +43 st (ratio 14) as FM mod, A warp FM 0.25, E3→A warp 0.5 D 400 ms (the bell in the attack), ENV1 D 2 s S 0.3 R 300 ms, velocity → A warp 0.3, Chorus mix 0.2 |
| EP Soft | as Tine with FM 0.12, F1 LP 3 kHz, velocity → cut 0.4 |
| EP Dirty | Tine + fx Distortion Overdrive mix 0.35, Chorus |
| Clav | A Pulse@0.15, F1 HP 200 Hz → F2 LP 5 kHz res 0.6, ENV1 D 900 ms S 0.1 R 60 ms, E2→cut 0.5 D 80 ms, velocity → cut 0.5 |
| Wurly | A Tri, B Sine +12 −12 dB, warp Bend 0.3 via velocity, ENV1 D 1.5 s S 0.4, fx Distortion soft mix 0.15, Chorus |
| Synth Piano | A Saw ×2 det 4, B Sine +12 −10 dB, F1 LP 3 kHz key track 0.7, E2→cut 0.6 D 600 ms, ENV1 D 3 s S 0.2 R 250 ms, velocity → cut 0.6 |
| Harpsi | A Sawstack@0.3, B Saw +12 −6 dB, F1 HP 300 Hz, ENV1 A 1 ms D 1.2 s S 0 R 40 ms, E2 → A position 0.3 D 20 ms |
| Toy Piano | A Bell/Tine@0.6, B Sine +24 −8 dB, ENV1 D 700 ms S 0 R 200 ms, Reverb small |
| Music Box | A Glass@0.1, B Sine +36 −14 dB, ENV1 A 1 ms D 1.5 s S 0, Delay 1/8 mix 0.15, Reverb |
| Digital Keys | A Bitwave@0.4, F1 LP 6 kHz, ENV1 D 800 ms S 0.5 R 200 ms, E2→cut 0.4, Chorus |
| Accordion | A Harmonic/Odd ×2 det 14, B Pulse@0.35 +9 c, F1 LP12 band-pass 1.4 kHz, ENV1 R 80 ms, Macro 1 "Musette" → unison detune, fx Reverb 0.4 |
| Melodica | A Square, Noise 0.35 → F2 BP 2.2 kHz gated by E3 50 ms, F1 LP 3.2 kHz key track 0.5, ENV1 D 250 ms S 0.7 |
| Clav Wah | A Pulse@0.7, F1 ladder **band-pass** 900 Hz res 0.8, LFO1 sync 1/4 → cut 0.7, ENV1 D 900 ms S 0.1, fx Distortion Soft clip |
| Electric Grand | FM keys at index 0.12, modulator +24 st, ×2 det 4, F1 LP 5 kHz key track 0.6, ENV1 D 3.5 s S 0.15, fx Chorus + Reverb |
| Rhodes Bell | FM keys at index 0.45, modulator +31 st, ENV1 D 2.2 s S 0.12, E3 D 700 ms → index, fx Delay 1/8 + Reverb |
| Prepared | A Tri warp **RM** 0.75 by B Sine +13 (silent), Noise −30 dB gated by E3 20 ms, F1 LP 6 kHz, ENV1 D 1.4 s S 0 |
| Poly Keys | A Saw ×2, B Pulse@0.4, F1 ladder LP 2.4 kHz char 0.3, E2→cut 0.45, ENV1 D 1.6 s S 0.35, LFO2 0.35 Hz → B position, fx Chorus |

**Pluck (18)**

| Name | Recipe |
|---|---|
| Init Pluck | A Saw ×2 det 8, F1 LP24 400 Hz, E2→cut 0.85 D 200 ms, ENV1 D 500 ms S 0 R 150 ms |
| Kalimba | A Sine, B Sine +31 −10 dB (FM 0.2 decaying by E3), ENV1 D 900 ms S 0, F1 LP 5 kHz |
| Guitar-ish | A Sawstack@0.5 ×2, F1 comb (cut = key, key track 1.0) char 0.65 → F2 LP 4 kHz, E2→F2 cut 0.7 D 150 ms, ENV1 D 1.2 s S 0 |
| Pizzicato | A Saw ×3 det 6, F1 LP 1.8 kHz, E2→cut 0.6 D 90 ms, ENV1 D 300 ms S 0 R 80 ms, Reverb small |
| Harp | A Tri, B Saw +12 −8 dB, F1 LP 3 kHz key track 0.6, ENV1 D 1.8 s S 0, Delay 1/16 mix 0.1 |
| Marimba-ish | A Sine, B Sine +24 (4:1) −8 dB D 80 ms via E3, ENV1 D 600 ms S 0, F1 LP 4 kHz |
| Chip Pluck | A NES Pulse 50 %, ENV1 D 200 ms S 0 R 10 ms, E2 → A position (12.5 → 50) D 60 ms |
| Steel | A Grit@0.3, F1 BP 2 kHz res 0.5 → F2 LP 6 kHz, ENV1 D 700 ms S 0, E2→F1 cut 0.5 |
| Water Drop | A Sine, E2 → pitch −24 st D 40 ms (the drop), ENV1 D 350 ms S 0, Delay 1/8 mix 0.25, Reverb |
| Dulcimer | A Sawstack@0.2 ×3 det 3, F1 LP 5 kHz, ENV1 D 2 s S 0, Delay 1/16 dotted mix 0.15 |
| Nylon | A Tri ×1, B Saw −24 dB, F1 comb 220 Hz char 0.5 key track 1.0 → F2 LP 2.6 kHz, ENV1 D 1.4 s |
| Jazz Guitar | A Tri ×1, B Saw −22 dB, F1 comb 190 Hz char 0.85 → F2 LP 1.1 kHz, Noise pick gated by E3 12 ms, ENV1 D 2.6 s R 700 ms |
| Banjo | A Digital/Grit@0.6, F1 HP 500 Hz (E2→cut −0.3) → F2 comb 240 Hz char 0.7, Noise pick gated by E3 10 ms, ENV1 D 500 ms |
| Koto | A Sawstack@0.15, F1 comb 300 Hz char 0.85 → F2 LP 4.5 kHz, E3 D 120 ms → pitch (the bend), Macro 3 "Bend" |
| Sitar | A Grit@0.35 ×2, B Sawstack +12 → F2 BP 3 kHz (the sympathetic strings), F1 comb 280 Hz char 0.9, ENV1 D 1.6 s, fx Reverb |
| Ukulele | A Tri +12 ×1, B Saw +12, F1 comb 420 Hz char 0.45 → F2 HP 350 Hz, ENV1 D 550 ms |
| Mandolin | A Sawstack@0.4 +12 ×2 det 11, F1 BP 2.4 kHz → F2 LP 7 kHz, LFO1 sync 1/32 → amp, Macro 3 "Tremolo" |
| Muted | A Saw ×1, Sub −20 dB around the filter, F1 ladder LP 1 kHz, E2→cut 0.35 D 50 ms, ENV1 D 200 ms, Noise thump gated by E3 15 ms |

**Strings (14)** — the archetype: two saw stacks an octave apart with slow
attacks, a low-pass that opens with velocity, a vibrato LFO that arrives late
and fades in (the `Lfo::delay_s` docs say why), and the **ensemble** chorus,
which is what a string machine was.

| Name | Recipe |
|---|---|
| Ensemble | A Saw ×5 det 12, B Saw −12 ×3 −6 dB, F1 LP 3.5 kHz, ENV1 A 350 ms R 900 ms, LFO1 5.2 Hz delay 350 ms fade 400 ms → pitch 6 c, Chorus ensemble 4 voices mix 0.5, Reverb size 0.6 mix 0.3 |
| Solo Violin | A Saw ×1, B Sawstack +12 −12 dB, F1 Formant char 0.55 res 0.3 → F2 LP 5 kHz, ENV1 A 120 ms, LFO1 6 Hz delay 200 ms fade 300 ms → pitch 12 c, mono legato glide 25 ms |
| Cello | as Solo Violin −12, F1 Formant char 0.75 cut 0.7×, F2 LP 2.5 kHz, glide 40 ms |
| Staccato | Ensemble with ENV1 A 15 ms D 250 ms S 0.3 R 120 ms |
| Tremolo Strings | Ensemble with LFO2 synced 1/16 square smooth 0.3 → Amp 0.6 |
| Synth Strings | A Analog Morph@0.8 ×6 det 16 width 1.0, F1 LP 4 kHz, ENV1 A 250 ms R 800 ms, Chorus ensemble |
| Pizz Section | Pizzicato ×3 layers wider, Reverb hall |
| Baroque | Harpsi-style A, B Saw −12, F1 LP 2.5 kHz, ENV1 A 60 ms R 400 ms, Reverb chamber |
| Viola | Solo: A Saw −7 ×1, B −19, F1 formant char 0.65 850 Hz → F2 LP 3.5 kHz, mono glide 30 ms |
| Double Bass | A Saw −24 ×1, B off, F1 formant char 0.85 450 Hz → F2 LP 1.4 kHz, mono glide 50 ms |
| Quartet | A Saw ×2 det 5, B Sawstack@0.4 −12 ×2, F1 HP12 180 Hz → F2 LP 6 kHz, ENV1 A 120 ms, fx Reverb 0.45 |
| Sordino | Ensemble with F1 LP 700 Hz res 0.3, ×3 det 8, ENV1 A 400 ms R 1.2 s |
| Marcato | ×4 det 11, ENV1 A 5 ms D 350 ms S 0.55, E2→cut 0.55 D 120 ms S 0.3, fx Ensemble |
| Slow Strings | ×6 det 16 width 1.0, B Sawstack@0.6 −12, F1 LP 5 kHz, ENV1 A 1.8 s R 3.4 s, fx Ensemble + Reverb |

**Brass & Winds (16)** — the archetype: saw through a low-pass whose envelope
attacks fast and *overshoots* (E2 decay to a lower sustain), a little detune,
and a pitch envelope of a few cents on the attack.

| Name | Recipe |
|---|---|
| Brass Section | A Saw ×3 det 9, B Saw −12 ×2 −8 dB, F1 LP24 1.2 kHz, E2→cut 0.7 A 40 ms D 250 ms S 0.55, E3 → pitch −30 c A 0 D 60 ms, ENV1 A 30 ms R 250 ms, Chorus mix 0.2, Reverb |
| Solo Trumpet | A Saw ×1, F1 Formant char 0.15 cut 1.3× → F2 LP 4 kHz, E2→F2 cut 0.6 A 30 ms, mono legato, LFO1 5.5 Hz delay 400 ms → pitch 8 c |
| French Horn | A Saw, B Tri −12 −6 dB, F1 LP 900 Hz, E2→cut 0.5 A 80 ms, ENV1 A 90 ms R 400 ms, Reverb hall mix 0.35 |
| Synth Brass | A Saw ×5 det 18, F1 ladder LP 1.5 kHz char 0.4, E2→cut 0.8 A 20 ms D 300 ms S 0.5, Chorus |
| Flute | A Sine, B Tri +12 −16 dB, Noise 0.2 through F2 BP 2.5 kHz res 0.6, ENV1 A 70 ms, LFO1 5 Hz delay 300 ms fade 300 ms → Amp 0.15 + pitch 5 c, Reverb |
| Clarinet | A Square (odd harmonics), F1 LP 2.5 kHz key track 0.8, ENV1 A 50 ms R 150 ms, E2→cut 0.3 A 50 ms, mono legato |
| Oboe | A Pulse@0.2, F1 Formant char 0.45 res 0.4, F2 LP 4 kHz, ENV1 A 40 ms, LFO1 5.5 Hz delay 250 ms → pitch 6 c |
| Pan Pipe | A Sine, Noise 0.35 through F2 BP 1.8 kHz, ENV1 A 35 ms D 800 ms S 0.6, E3 → Noise level 0.5 A 0 D 60 ms (the chiff), Delay 1/8 mix 0.2 |
| Trombone | Brass at −12 ×1, B Saw −24, F1 ladder LP 900 Hz char 0.45, ENV1 A 60 ms, mono glide 60 ms, fx Reverb |
| Tuba | Brass at −24 ×1, B off, Sub −24 around the filter, F1 ladder LP 420 Hz, ENV1 A 90 ms, mono glide 50 ms |
| Alto Sax | A Pulse@0.35, Noise 0.4 → F1 (the growl is *inside* the throat), F1 formant char 0.3 1.15 kHz → F2 ladder LP 3.2 kHz char 0.5, ENV1 A 35 ms, mono, LFO1 late vibrato, fx Reverb |
| Tenor Sax | A Modern/Growl@0.3 −12, Noise 0.5 → F1, F1 formant char 0.55 780 Hz → F2 LP 2.4 kHz, mono, fx Distortion Tube |
| Bassoon | A Pulse@0.15 −12, F1 LP 1.3 kHz key track 0.7, E2→cut 0.3, mono, LFO1 vibrato at rest on Macro 2 |
| Piccolo | A Sine +24, B Tri +36, Noise 0.3 −22 dB → F2 BP 6 kHz, ENV1 A 30 ms, LFO1 6 Hz late |
| Muted Trumpet | Brass ×1, B off, F1 HP 900 Hz (E2→cut −0.3) → F2 BP 2.8 kHz res 1.4, mono glide 20 ms |
| Shakuhachi | A Sine, B Tri +12, Noise 0.2 −16 dB → F2 BP 1.6 kHz, E3 D 120 ms → pitch −8 c and → noise, LFO1 4.2 Hz late 500 ms, fx Reverb |

**Choir & Vocal (12)** — the archetype: a detuned saw stack (or the Choir
table) into the **Formant** filter, a slow attack, breath noise at a whisper,
a wide ensemble chorus and a hall. This is the "choir ahhs" the brief names,
and the test that holds it is spectral: the rendered sustain of "Choir Ahh"
has its two loudest peaks within a bin of A's formants.

| Name | Recipe |
|---|---|
| Choir Ahh | A Choir@0.4 ×5 det 10, B Saw −12 ×3 −9 dB, Noise 0.06 brown through F2 BP 3 kHz, F1 Formant char 0.0 (A) res 0.35, ENV1 A 450 ms R 1.2 s, LFO1 4.5 Hz delay 500 ms fade 600 ms → pitch 5 c, Chorus ensemble 4 mix 0.4, Reverb hall size 0.8 mix 0.4 |
| Choir Ooh | Choir Ahh with F1 char 0.85 (between O and U) |
| Choir Mmm | Ooh with F1 cut 0.6× and F2 LP 1.5 kHz |
| Vowel Morph | Choir Ahh with LFO2 free 0.08 Hz → F1 character 0.5 and Macro 1 "Vowel" → F1 character 1.0 |
| Boys Choir | Choir Ahh +12 with F1 cut 1.25× (a smaller throat), ENV1 A 300 ms |
| Synth Vox | A Vowel@0.3 ×3, F1 LP 5 kHz, LFO1 0.2 Hz → A position 0.4, Chorus, Delay 1/4 mix 0.2 |
| Whisper | Noise 1.0 white through F1 Formant char 0.3 res 0.7, A Choir −20 dB, ENV1 A 200 ms, Reverb |
| Robot Voice | A Saw ×2, F1 Formant char via LFO1 S&H sync 1/8 depth 1.0 smooth 0.2, F2 comb 300 Hz char 0.6, fx Bitcrush Telephone mix 0.5 |
| Soprano | Choir at vowel 0.2, ×1 +12, B off, F1 formant 1.5 kHz res 0.75 char 0.2, mono, LFO1 5.5 Hz late → pitch 15 c |
| Baritone | Choir at vowel 0.75, ×1 −12, B off, F1 formant 620 Hz res 0.8 char 0.8, mono glide 40 ms, LFO1 late vibrato |
| Gregorian | Choir at vowel 0.55, ×4 det 7 at −12, C Choir@0.6 −24, F1 formant 700 Hz, ENV1 A 1.4 s R 2 s |
| Vocal Stab | Choir at vowel 0.15, ×3 det 9, F1 formant 1.1 kHz res 0.8, ENV1 A 10 ms D 350 ms S 0, E2→cut 0.3, fx Delay 1/8 |

**Organ (12)**

| Name | Recipe |
|---|---|
| Drawbar 888 | A Drawbar@0.9, no filter, ENV1 A 4 ms R 40 ms (with a 1 ms click: Noise 0.3 through F2 HP 2 kHz gated by E3 A 0 D 8 ms), LFO1 6.5 Hz → pan 0.5 + pitch 3 c (the Leslie), fx Distortion Amp mix 0.2, Reverb |
| Drawbar Jazz | A Drawbar@0.45, LFO1 as above at 0.8 Hz (slow Leslie), Macro 1 "Leslie" → LFO1 rate |
| Church | A Drawbar@0.7, B Sine −12, C Sine +19 −12 dB, ENV1 A 60 ms R 600 ms, Reverb hall size 0.95 mix 0.5 |
| Combo | A Square, B Square +12 −6 dB, C Square +24 −12 dB, F1 LP 4 kHz, ENV1 A 3 ms R 30 ms, LFO1 6 Hz → Amp 0.15 |
| Farfisa-ish | A Pulse@0.4 ×2 det 3, F1 LP 5 kHz, ENV1 A 2 ms R 20 ms, LFO1 → pitch 4 c |
| Percussive | Drawbar Jazz with E3 → B (Sine +19) level 0.8 A 0 D 250 ms (the "percussion" tab) |
| Rock Organ | Drawbar@0.8, B Sine +19, F1 **ladder** LP 3.5 kHz drive 0.85 char 0.85 — the saturation has to be in the filter, not only the effect — Leslie 7.2 Hz, fx Distortion Tube |
| Gospel | Drawbar@0.75 at −12 (the 16′), B Sine +7, C Sine +19, key click down to −44 dB, velocity → amp, fx Distortion Tube |
| Pipe Flute | A Sine, B Sine +19 −26 dB, Noise 0.4 −34 dB chiff, no Leslie, ENV1 A 80 ms R 250 ms, fx Reverb 0.95 |
| Reed Organ | A Harmonic/Odd, B Pulse@0.3 +7 c, F1 LP 3 kHz, ENV1 A 40 ms |
| Theatre | A Sine −8 dB, B Sine −12, C Tri +12, key click down to −42 dB, LFO2 6.5 Hz → amp 0.6 (the tremulant), Macro 3 "Tremulant", fx Reverb 0.8 |
| Bass Pedals | Drawbar@0.85 at −24, B Sine −12, F1 LP 900 Hz, key click −40 dB, velocity → amp |

**Bells & Mallets (14)**

| Name | Recipe |
|---|---|
| Tubular | A FM Bell@0.5, B Sine +19.4 st −10 dB (the strike partial), ENV1 D 4 s S 0 R 2 s, E3 → A position 0.4 D 300 ms, Reverb hall |
| Glockenspiel | A Sine, B Sine +31 −12 dB, C Sine +43 −18 dB, ENV1 D 1.5 s S 0, F1 LP 10 kHz, Reverb |
| Vibraphone | A Sine, B Sine +24 −10 dB, ENV1 D 2.5 s S 0, LFO1 5 Hz → Amp 0.5 (the motor), Reverb |
| Celesta | A Tine@0.2, B Sine +36 −14 dB, ENV1 D 1.2 s S 0 R 400 ms |
| Gong | A Gong@0.6 ×2 det 4, ENV1 A 20 ms D 6 s S 0, LFO1 0.4 Hz → A position 0.3, Reverb size 0.9 |
| Steel Drum | A Sine, B Sine +7 st −6 dB (the pan's fifth), C Sine +12 −12 dB, warp Bend 0.2 via velocity, ENV1 D 800 ms S 0 |
| Crystal | A Glass@0.8, B Sine +24 −14 dB, ENV1 A 10 ms D 3 s S 0, Delay 1/8 dotted mix 0.25, Reverb |
| Chime Tree | A Glass@0.5 ×4 det 20, random phase, ENV1 D 2 s S 0, LFO1..3 → A/B/C levels, Reverb |
| Xylophone | A Sine, B Sine +19, Noise mallet → F1 HP 3 kHz gated by E4 8 ms (A and B bypass it), ENV1 D 280 ms |
| Hand Bells | A FM Bell@0.25, B Sine +12, C Sine +26, ENV1 D 1.6 s, fx Reverb 0.55 |
| Temple Bell | A Gong@0.25 −12 warp **RM** 0.35 by B Sine +15 (silent), ENV1 D 8 s R 4 s, fx Reverb 0.95 |
| Carillon | A FM Bell@0.7 −12 ×2 det 6, B FM Bell@0.4 +15, ENV1 D 3.5 s, fx Reverb 0.9 |
| Crotales | A Sine +24, B Sine +36, ENV1 D 2.2 s, fx Reverb 0.7 |
| Glass Harp | A Glass@0.45 ×2, B Sine +19, Noise 0.9 −38 dB, ENV1 **A 900 ms** S 1.0 R 1.4 s — bowed, not struck — fx Reverb 0.85 |

**Chip & Retro (14)** — no filter unless named, `patch/quality` Draft is
*not* used for effect (the tables are exact), bitcrush is.

| Name | Recipe |
|---|---|
| NES Lead | A NES Pulse 50 %, ENV1 R 5 ms, LFO1 square 7 Hz → pitch 30 c via wheel |
| NES Pulse 25 | A NES Pulse 25 %, ENV1 D 120 ms S 0.7 R 5 ms |
| NES Bass | A NES Triangle, no filter, mono |
| Game Boy Wave | A Game Boy@0.3, LFO1 sync 1/16 saw down → A position 0.3 |
| C64 Arp | A C64@0.4, LFO1 sync 1/16 square → pitch 12 st (quantised 1 step) — the two-note arp; Macro 1 "Interval" → depth |
| SID Bass | A Pulse@0.25, F1 LP12 800 Hz res 0.6, E2→cut 0.6 D 100 ms, LFO1 → A position 0.2 |
| Chip Pad | A NES Pulse 12.5 ×3 det 8, B Game Boy −12, ENV1 A 200 ms R 400 ms, fx Bitcrush 8-bit mix 0.3, Reverb small |
| 8-bit Drum Kit Hat | Noise white 1.0, ENV1 D 40 ms S 0, fx Bitcrush 8-bit console |
| Arcade Zap | A Saw, E2 → pitch −36 st D 120 ms, ENV1 D 150 ms S 0, fx Bitcrush |
| Lo-fi Keys | A Bitwave@0.2, F1 LP 3 kHz, ENV1 D 1 s S 0.3, fx Bitcrush 12-bit sampler mix 0.6, Chorus |
| Duty Sweep | A Analog/PWM Sweep@0.2, LFO2 0.7 Hz → position 0.6, Macro 3 "Sweep" |
| Chip Organ | A NES Pulse 50 %, B +12, C +19, ENV1 R 20 ms |
| Amiga Lead | A Digital/Crunch@0.35 ×2 det 7, F1 LP 5 kHz, ENV1 D 400 ms S 0.6, fx Bitcrush 8-bit + Delay 1/16· |
| PC Speaker | A Square, F1 HP 1.2 kHz, no envelope at all, mono |

**Sequence & Arp (13)** — rhythm from synced LFOs on the amp or the filter, so
a held note *is* the sequence (there is no arpeggiator in the synth; the
roll's arpeggiate command is the arpeggiator, TDD §16.5).

| Name | Recipe |
|---|---|
| Gated Pad | Init Pad + LFO2 sync 1/16 square smooth 0.15 → Amp 1.0 |
| Trance Pluck | Init Pluck ×7 det 18, Delay 1/8 dotted mix 0.35, Reverb |
| Sidechain Feel | Warm Analog + LFO2 sync 1/4 saw down (inverted via curve) → Amp 0.7 |
| Filter Step | Saw ×3, F1 LP 600 Hz, LFO1 sync 1/8 S&H → cut 0.8 smooth 0.1 |
| Octave Bounce | Saw, LFO1 sync 1/8 square → pitch 12 st quantised 1 step, ENV1 D 200 ms S 0.4 |
| Pulse Train | Square, LFO1 sync 1/16 square → Amp 1.0, LFO2 sync 1/1 tri → F1 cut 0.5 |
| Random Bleeps | Sine ×1, LFO1 sync 1/16 S&H → pitch 24 st quantised 12 steps, LFO2 sync 1/16 square → Amp 1.0, Delay 1/8 mix 0.3 |
| Tremolo Keys | EP Tine + LFO2 sync 1/8 tri → Amp 0.6 + pan 0.4 |
| Bass Sequence | Bass archetype, LFO2 sync 1/16 square smooth 0.05 → amp, LFO1 sync 1/8 S&H → cut 0.6 |
| Arp Bells | Bell on Glass@0.4, LFO2 sync 1/16 → amp, LFO3 sync 1/8 S&H → pitch quantised to 7 steps, fx Ping-pong + Reverb |
| Sync Seq | A Sync Sweep warp Sync 0.3, LFO1 sync 1/16 S&H → warp 0.8, LFO2 sync 1/16 → amp, fx Delay 1/8 |
| Noise Rhythm | Noise 0.25 → F1 BP 2.5 kHz, LFO1 sync 1/16 → amp, LFO2 sync 1/8 S&H → cut 0.8, fx Ping-pong 1/16 |
| Motion Keys | A Saw ×2, B Pulse@0.35, F1 ladder LP 1.6 kHz, LFO1 sync 1/4 → cut, LFO2 sync 1/8 saw down → amp 0.45, fx Chorus |

**Atmos & FX (15)**

| Name | Recipe |
|---|---|
| Wind | Noise 1.0 brown, F1 BP 400 Hz res 0.8 with LFO1 free 0.08 Hz → cut 1.0, LFO2 0.13 Hz → res 0.4, ENV1 A 2 s R 3 s, Reverb size 0.9 |
| Rain | Noise white through F1 HP 4 kHz, LFO1 S&H 30 Hz → Amp 0.4, Reverb |
| Ocean | Wind with F1 LP and LFO1 0.06 Hz, Delay 1/2 mix 0.3 |
| Riser | A Saw ×6 det 25, E2 → pitch +24 st A 4 s (with sustain), E3 → cut 1.0 A 4 s, Noise 0.4 rising with E3, Reverb — a held note rises for four seconds |
| Downer | Riser with the envelopes inverted (depth −) |
| Space Drone | A Hollow@0.7 ×3 det 5, B Sine −24, F1 LP 800 Hz, LFO1..4 free at 0.03/0.05/0.07/0.11 Hz on position/cut/pan/B level, Reverb size 1.0 mix 0.6 |
| Sci-fi Sweep | A Sync Sweep, LFO1 sync 1/1 saw up → A position 1.0, F1 BP 2 kHz res 0.8 → LFO1, Delay ping-pong |
| Laser | A Sine, E2 → pitch −48 st D 250 ms, ENV1 D 300 ms S 0, Delay 1/16 mix 0.4 |
| Impact | A Sub Sine, E2 → pitch −24 D 80 ms, Noise 0.6 D 200 ms via E3, ENV1 D 1.5 s S 0, Reverb size 0.9 mix 0.5 |
| Glitch | A Bitwave@0.5, LFO1 S&H sync 1/32 → A position 1.0 + F1 cut 0.8 + Amp 0.5, fx Bitcrush Broken clock |
| Sonar | A Sine +12, E2 D 300 ms → pitch −0.02, ENV1 D 350 ms, fx Ping-pong 1/2 feedback 0.55 + Reverb 0.95 |
| Metal Scrape | A Gong@0.6 warp **RM** 0.8 by B Sine +11 (silent) → F1 BP 2.5 kHz on the atmos LFOs, ENV1 A 400 ms R 1.2 s |
| Sub Drop | A Sub Sine, E2 D 2.5 s → pitch 0.35 (down across the whole note), no filter |
| Reverse Swell | Noise 0.15 → F1 BP 1.2 kHz, ENV1 **A 2.4 s R 20 ms**, E2 A 2.4 s → cut 0.85 and → resonance, fx Reverb 0.85 |
| Static | Noise 0.35 → F1 BP 1.8 kHz res 1.4, LFO1 34 Hz S&H free → amp 0.4, LFO2 3 Hz S&H free → cut, fx Bitcrush |

**Synth Drums (11)** — the drum machine is the drum instrument; these exist
because a synth kick or a zap is a thing a person reaches for in a synth, and
they live on a key rather than a map.

| Name | Recipe |
|---|---|
| 808 Kick | A Sub Sine, E2 → pitch −30 st D 45 ms, ENV1 D 700 ms S 0, fx Distortion soft mix 0.2 |
| 909 Kick | A Sine, E2 → pitch −36 st D 60 ms, Noise 0.3 D 15 ms via E3, ENV1 D 350 ms S 0, fx Distortion Overdrive mix 0.3 |
| Snare Synth | A Sine +12 with E2 → pitch −12 D 30 ms, Noise 0.9 through F1 BP 3 kHz, ENV1 D 180 ms S 0 |
| Hat Synth | Noise 1.0 through F1 HP 7 kHz res 0.5, ENV1 D 60 ms S 0 |
| Tom Synth | A Sine, E2 → pitch −18 st D 120 ms, ENV1 D 400 ms S 0 |
| Zap | A Saw, E2 → pitch −48 D 90 ms, F1 LP 3 kHz, E3 → cut −1.0 D 90 ms, ENV1 D 120 ms S 0 |
| Rim Synth | Drum on Square, pitch drop 20 st in 8 ms, F1 BP 1.8 kHz res 1.4, ENV1 D 50 ms |
| Clap Synth | Noise 0.1 → F1 BP 1.4 kHz, LFO1 55 Hz square free → amp (the four hands), E2 D 35 ms → amp, ENV1 D 220 ms, fx Reverb 0.35 |
| Cowbell | A Square +7, B Square +18 +40 c, F1 BP 2.6 kHz, ENV1 D 280 ms |
| Conga Synth | Drum on Sine, pitch drop 8 st in 50 ms, F1 BP 400 Hz, ENV1 D 300 ms |
| Open Hat | Noise → F1 HP 6 kHz, ENV1 D 550 ms R 200 ms |

That is **130**. The count and the categories are held by tests, not by this
table.

### 7.3 Sound-design rules the archetypes follow

Written down because the drum kits taught that a bank of "different" presets
can be one preset at forty brightnesses (`drum-kit-axes`):

- **Instrument imitations differ on the source and the filter model**, not
  only on envelope times. Strings are a saw stack through a *clean* low-pass
  with ensemble; choirs are a stack through the *formant* filter; brass is a
  saw through a *ladder* with an overshooting envelope; bells and e-pianos are
  *FM* tables. Two presets built on the same source and filter model must
  differ on at least two of: unison count, envelope shape, a modulation route,
  an effect.
- **Vibrato arrives late and fades in** on every acoustic imitation
  (`Lfo::delay_s` and `fade_s`); a vibrato on the first sample is the tell.
- **Velocity goes somewhere** on every preset: cutoff, warp, unison blend or
  Amp. A preset that ignores velocity is a preset for a sequencer, and this
  program's user plays.
- **The mod wheel goes somewhere** on every preset in Lead, Pad, Strings,
  Brass, Choir: vibrato depth, filter, or a macro. Nothing invents a mapping
  the patch does not name (`performance-events`), so the preset names it.
- **Every preset names at least two macros** with the two things a person
  would reach for on it ("Brightness", "Motion", "Space", "Drive", "Vowel",
  "Wobble rate"). The macro's routes are how a preset becomes playable from
  one knob.
- **Effects are a sound's, not a mix's:** a preset's reverb is the size of the
  instrument's own room, never the mix's hall; mixes above 0.5 wet on any slot
  need a reason on the row.

### 7.4 The tests that hold the bank (`fontelle-core/tests/flopsynth_presets.rs`)

Every one written first against a bank with one entry, seen to fail, then
the bank filled in.

- `there_are_at_least_120_and_every_category_has_at_least_six`.
- `every_name_is_unique_and_every_preset_builds_a_flopsynth_patch`
  (`kind_of` would say Flopsynth; five Synth layers first).
- `every_preset_sounds`: a middle-C note for a second, RMS above −40 dBFS
  somewhere in it (a Riser sounds late; measure the whole second).
- `every_preset_stays_inside_full_scale`: a four-note chord at velocity 127
  for two seconds plus release, peak ≤ 0.98 after the chain (the chain is in
  the node, so this test builds a `SamplerNode` — it lives in
  `fontelle-engine/tests/flopsynth_fx.rs` and reads the bank from core).
- `every_preset_sits_within_three_db_of_loudness`: the same chord's RMS over
  its first second, all within ±3 dB of the bank's median. This is what
  `Patch::output_db` is for; the agent tunes it per row until the test
  passes.
- `every_pair_in_a_category_is_audibly_apart`: the drum kit's test —
  ln(t30), spectral centroid, ln(crest) plus a fourth axis, spectral flux
  over the first 300 ms (attack character) — every pair in a category apart by
  `APART = 0.35` on at least one.
- `choir_ahh_has_a_voice_in_it`: the sustain's two loudest spectral peaks
  within a bin of vowel A's F1 and F2, scaled by the preset's formant cutoff.
- `every_preset_round_trips_the_format` (also in §5's suite).
- `every_preset_names_two_macros_and_routes_velocity`.
- `every_committed_factory_file_equals_its_recipe` (the export is
  idempotent; a row edited without re-exporting fails here).
- In `fontelle-app/tests/flopsynth_ui.rs`: loading a preset through the bar
  shows its name clean; one knob shows `Name*`; undo shows it clean again.

---

## 8. The window

This is the half of the brief with the strongest words in it, and the one
this project has the least precedent for: the EQ's curve is the only bespoke
editor, everything else is the generic grid. The generic grid is the wrong
tool here — it draws a list, and a synthesiser is a **picture of a signal
path**. So Flopsynth has its own canvas (`fontelle-ui/src/canvas/flopsynth.rs`,
pure geometry, tested) and its own drawing (`draw_flopsynth` in `render`),
in the same OS window every instrument opens in (`EditorKind::Instrument`),
told apart the way the EQ and the knob grid are (`EditorWindowChrome`).

### 8.1 Design principles

Each one is either a rule this project already holds or a reading of the
brief; both are cited.

1. **The layout is the signal path, left to right and top to bottom.**
   Sources on the top row, filters and the amp envelope under them,
   modulators under those, output at the bottom. Somebody who has never seen
   it reads it in the order the sound is made. (*"flow cleanly for the users
   eyes"*.)
2. **Sections are cards.** Every section has a header strip with its name
   in `text_muted`, a 1 px `border`, a `panel` surface on the `window`
   ground, and 8 px of air. Nothing is drawn outside a card. The three
   oscillators carry a thin coloured rule under their header — A `accent`, B
   `playhead`, C `note` — the three ramps the theme already has, so a route's
   source badge and its oscillator share a colour without a new token.
3. **A control is the shape of what it sets** (PROGRESS 2026-09-04): a
   knob for anything continuous, a switch for on/off, a drop-down with a caret
   for a choice, a bipolar knob (arc from twelve o'clock) for anything signed.
   Nothing steps through a list on click. The wheel nudges by one unit.
4. **Every value is readable** under its control, in its own unit ("2.4 kHz",
   "+7 st", "1/8", "37 %"), and every control has a tooltip (`tip()` on the
   hit enum, the rule `tooltip.rs` states).
5. **Pictures where a number is not the thing being decided:** each
   oscillator draws its current frame; each filter draws its response; each
   envelope draws its curve with draggable nodes; each LFO draws its cycle.
   The pictures are computed from the same numbers the voice reads
   (`effect.rs`'s argument for the EQ curve: a picture that lies is believed).
6. **Modulation is visible on the knob.** A route to a control draws a
   second arc, in the new `modulation` colour, spanning the range the sum of
   its routes can move it. Automation keeps its orange groove (§12.2's ring);
   the two never share a colour. (Serum's one great idea, and *"tons of
   complexity"* made legible.)
7. **Four pages, one header.** The window is 1180 × 740 (min 980 × 620) and
   does not scroll; what does not fit on the *Synth* page is on the
   *Modulation*, *Effects* or *Presets* page, chosen by tabs in the header. A
   scrolling synth is a synth whose filter is under its oscillators.
8. **The preset bar is the system's, not Flopsynth's** (§P.7): the name
   persists through edits and the `*` is recognised (§P.6). The channel's
   name field (the drop target, `draw_instrument`'s "field") is also set to
   the preset's name when one is chosen, so the rack reads "Choir Ahh".
9. **Nothing animates while the transport is stopped and nothing sounds**
   (TDD §16.3). The envelope's playhead and the LFO's phase dot animate only
   while the channel has active voices, through the existing animator count.
10. **Pure geometry, tested; pixels seen.** `flopsynth_layout`,
    `flopsynth_hit`, every curve-points function and every gesture's
    arithmetic are pure and in `fontelle-ui/tests/flopsynth.rs`; the drawing
    is looked at through `render_headless` and the nested X server.

### 8.2 The header (all pages)

```
┌───────────────────────────────────────────────────────────────────────────────────────┐
│ FLOPSYNTH │ ◀ ▶ │ Choir Ahh*  ▾ │ Choir & Vocal │ ★ │ Save │ Save as… │ Synth Modulation Effects Presets │ ⌂ Init │
└───────────────────────────────────────────────────────────────────────────────────────┘
```

- Everything between the title and the page tabs is the **shared preset
  bar** (§P.7), drawn by `preset_bar.rs` — Flopsynth adds nothing to it and
  takes nothing away. Prev/next audition through the preview voice, the
  drop-down groups by category with favourites first, Save and Save as…
  behave as §P.7 says.
- The tabs are `PageTab` chips with the chosen one filled (`accent`).
- `Init` writes `flopsynth_init()` through `ApplyPreset` with no ref, so the
  bar shows "— no preset —" and one undo takes it back.

### 8.3 The Synth page

```
┌ OSC A ─────────────┐┌ OSC B ─────────────┐┌ OSC C ─────────────┐┌ SUB ───────────┐
│ [table ▾]  ~~~~~~~ ││                    ││                    ││ [Sub Sine ▾]   │
│ (wave picture,     ││                    ││                    ││ oct  level     │
│  drag = position)  ││                    ││                    ││ route ▾  ⏻     │
│ pos   warp▾  amt   ││                    ││                    │├ NOISE ─────────┤
│ uni   det    blend ││                    ││                    ││ colour level   │
│ wid   semi   fine  ││                    ││                    ││ route ▾  ⏻     │
│ level pan  route▾ ⏻││                    ││                    ││                │
└────────────────────┘└────────────────────┘└────────────────────┘└────────────────┘
┌ FILTER 1 ──────────────┐┌ FILTER 2 ──────────────┐┌ ENV 1 · AMP ───────┐┌ ENV 2 · FILTER ─────┐
│ [Clean ▾][LP ▾][24 ▾]⏻ ││                        ││   /‾‾‾‾\___        ││   /\                │
│ (response, drag = cut/res)│                      ││  (curve, nodes)    ││  (curve, nodes)     │
│ cutoff res drive       ││                        ││ A  H  D  S  R      ││ A  H  D  S  R  →cut │
│ keytrk char            ││                        ││ shapes (3 bipolar) ││                     │
└────────────────────────┘└────────────────────────┘└────────────────────┘└─────────────────────┘
┌ LFO 1 ───────────┐┌ LFO 2 ───────────┐┌ MACROS ─────────────────┐┌ VOICE ──────────────────┐
│ [sine ▾] ∿∿∿∿    ││                  ││ (1)   (2)   (3)   (4)   ││ mode▾ poly glide legato │
│ rate sync div▾   ││                  ││ Bright Motion Space Drv ││ bend  output   [●●●●○]  │
│ depth delay fade ││                  ││ (names are editable)    ││ (voice count meter)      │
└──────────────────┘└──────────────────┘└─────────────────────────┘└─────────────────────────┘
```

- **Oscillator card.** The table drop-down is grouped by family with
  headings (`ContextMenu` already draws headings for favourites). The wave
  picture is 128 points of the *current* frame (`flopsynth::wave_points`),
  dragged horizontally to set position — the picture changes under the hand.
  Off oscillators (level at floor) draw their picture and controls at
  `text_muted` with the `⏻` switch outlined, so a silent oscillator looks
  silent rather than absent. `route` is a four-way drop-down. The modulator
  drop-down appears only when `warp` is FM/RM/Sync and lists only later
  layers.
- **Filter card.** Three drop-downs across the top (model, shape, slope) and
  the switch. The response is 96 points over the EQ's log axis
  (`flopsynth::filter_response_db`, computed in `fontelle-app` from the
  actual coefficients — for Clean and Ladder a magnitude sweep, for Formant
  the three band-passes summed, for Comb the closed form); dragging on it
  moves cutoff on x and resonance on y, the EQ handle's gesture. The
  `character` knob's caption changes with the model ("vowel", "feedback",
  "saturation", "—" and hidden for Clean).
- **Envelope card.** The curve is drawn from the stage times on a **square-
  root time axis** so a 5 ms attack and a 2 s release are both visible, with
  four draggable nodes (attack peak, decay/sustain knee, sustain end, release
  end) — dragging a node writes the stage it owns (`flopsynth::env_node_drag`
  returns the address and value). The five stage knobs and three bipolar
  shape knobs are under it; the Synth page shows ENV 1 and ENV 2 (with its
  `→cut` depth knob, which is `patch/mod[k]/depth` for the route the Init
  patch wrote — if the route is gone the knob is hidden), the Modulation
  page shows all four.
- **LFO card.** The cycle is drawn from `LfoWave::value` (types, pure);
  S&H draws its held steps from a fixed seed so the picture is stable. A
  phase dot moves along it while voices sound. `div` appears when `sync` is
  on and `rate` reads "1/8" then.
- **Macro card.** Four knobs whose captions are the macro names; double-
  clicking a caption opens the name prompt. A macro with no routes draws its
  knob outlined (a knob that moves nothing is worth saying so).
- **Voice card.** `mode` drop-down, `poly` knob (integer), `glide`, `legato`
  switch, `bend` knob, `output` knob, and a voice-count meter of eight pips
  lit by `active_voices / polyphony` while sounding.

### 8.4 The Modulation page

```
┌ LFO 1 ──┐┌ LFO 2 ──┐┌ LFO 3 ──┐┌ LFO 4 ──┐   ┌ ENV 3 ─────┐┌ ENV 4 ─────┐
│ (as above, wider, phase + mode + smooth shown) │            ││            │
└─────────┘└─────────┘└─────────┘└─────────┘   └────────────┘└────────────┘
┌ SOURCES ───────────────────────────────────────────────────────────────────┐
│ [ENV 1][ENV 2][ENV 3][ENV 4] [LFO 1][LFO 2][LFO 3][LFO 4] [M1][M2][M3][M4]  │
│ [Velocity][Key][Aftertouch][Wheel][Bend][Random][Counter][Note X][Note Y]   │
└────────────────────────────────────────────────────────────────────────────┘
┌ MATRIX (12 of 32) ─────────────────────────────────────────────────────────┐
│ #  source ▾     destination ▾         depth ═══●═══   curve ▾   via ▾    ✕ │
│ 1  ENV 2        Filter 1 cutoff       +0.62           linear    —        ✕ │
│ 2  LFO 1        Osc A pitch           +0.05           linear    Wheel    ✕ │
│ …                                                       (virtualised list)  │
│ [+ route]                                                                   │
└────────────────────────────────────────────────────────────────────────────┘
```

- **Drag to assign.** Press a source badge and drag: every control that is a
  `ModDest` lights a dashed `modulation` ring (§8.1 rule 6) on *every* page —
  the tabs are hot during the drag so a source can be carried to the Effects
  page? No: effects are not per-voice destinations (§3.6), so only the Synth
  and Modulation pages' knobs light, and the tabs to those two pages are hot.
  Release on a lit knob adds a route at depth +0.5 (bipolar destinations at
  +0.5 too); release anywhere else adds nothing. The badge follows the
  pointer during the drag (drawn by the window from the drag state).
- **Ring drag.** Press on a knob's modulation arc (a band 4 px outside the
  groove, `flopsynth::ring_hit`) and drag vertically to change the depth of
  the **most recent** route to it; Shift is fine. With two or more routes the
  tooltip says "2 routes — edit in the matrix".
- **The matrix rows** are the same list, editable in place: source and
  destination drop-downs (grouped), a bipolar slider for depth (a slider
  because the row is 22 px — the audio editor's argument), curve and via
  drop-downs, and a remove button. A row's source badge colour matches the
  page's badges.
- **Right-click on any modulated knob** adds "Remove modulation (LFO 1)" rows
  to the existing knob menu, one per route, above "Create automation clip".

### 8.5 The Effects page

Four slot cards in a row (empty ones show a single `+ effect` drop-down),
each with: the kind's name, `⏻`, the preset chips the effect ships
(`EffectConfig::presets`, the same chips), the spec grid grouped by
`sections()` (the existing `instrument_layout` inside the card body), and a
`mix` knob. Grab handles on the card headers reorder by dragging (a structural
edit). The order is drawn as arrows between the cards.

### 8.6 The Presets page

```
┌ CATEGORIES ──┐┌ PRESETS · Pad · 12 ─────────────────────┐┌ ABOUT ───────────────────┐
│ ★ Favourites ││ 🔍 search                                ││ Choir Ahh                │
│ All          ││ Init Pad                                 ││ Choir & Vocal            │
│ Bass         ││ Warm Analog                              ││ Macros: Vowel, Space,    │
│ Lead         ││ Glass                                    ││   Air, Drive             │
│ Pad ◀        ││ Choir Pad                          ★     ││ Wheel: vibrato depth     │
│ Keys         ││ …                                        ││ Velocity: brightness     │
│ …            ││ (virtualised; click = load + audition)   ││ [Load]  [★]  [Delete]    │
│ User         ││                                          ││ (Delete: user only)      │
└──────────────┘└──────────────────────────────────────────┘└──────────────────────────┘
```

- This page is a **view over the preset bank filtered to this device**
  (`PresetBank::for_device`), the same rows the browser's Presets tab shows
  under "flopsynth", laid out with the browser's virtualised list
  (`row_under`/`scrolled` reused, not copied) and searched with
  `PresetBank::search`. It exists so a person does not have to leave the
  synth to browse; it invents no mechanism. A click is `ApplyPreset` plus
  audition. The About column is generated from the patch: which macros are
  named, where the wheel and velocity go (`flopsynth::describe_routes`), so
  it is never stale.
- **User** rows are the user origin's; delete and rename are the tab's own
  gestures (§P.8) reached from here too.

### 8.7 Gestures, in one table

| Gesture | On | Does | Pure function |
|---|---|---|---|
| drag ↕ | knob | value; Shift ÷6 | `knob_value` (exists) |
| double-click | knob | its default | `flopsynth::default_of(address)` |
| wheel | knob, choice, switch | one unit | `nudge` |
| press | choice | opens its list under the control | `ContextMenu` (exists) |
| press | switch | flips | `next_value` (exists) |
| right-click | knob | automate / remove modulation | menu rows |
| drag ↔ | wave picture | position | `wave_position_at(x)` |
| drag ↔↕ | filter response | cutoff, resonance | `filter_xy_at(x, y)` |
| drag | envelope node | its stage | `env_node_drag` |
| drag | source badge → knob | adds a route | `assign_target_at` |
| drag ↕ | modulation ring | depth of the latest route | `ring_depth` |
| press | preset row | `ApplyPreset` + audition | — |
| drag | fx card header | reorder | `fx_order_at` |
| key ←/→ (window focused) | preset bar | previous / next preset (§P.7) | — |

### 8.8 Sizes, tokens, chrome

- `EditorKind::default_size` becomes `default_size_for(&InstrumentSize)` —
  the host reports `instrument_editor_size()` (Flopsynth: 1180 × 740; the
  grid: 620 × 760) and the window resizes with `set_inner_size` when the
  selected channel changes kind. Minimum for Flopsynth 980 × 620; below the
  default the cards keep their control sizes and lose air first, then the
  pictures shrink to a 40 px floor.
- Knobs are 34 px in a 62 × 60 cell (caption above, value below); the
  generic grid's 92 × 76 cell is too coarse for a card of nine. Constants
  `FLOP_KNOB`, `FLOP_CELL_W`, `FLOP_CELL_H`, `CARD_HEADER`, `CARD_PAD`,
  `CARD_GAP` on the canvas, all named, none repeated in the renderer.
- **One new palette token: `modulation`** (dark: a violet `#9a6fd0`-ish
  from outside the three ramps, because the modulation arc has to be
  tellable from accent, playhead, note *and* the automation orange at 3 px;
  light: a darker violet). `THEME_FORMAT_VERSION` 6 → 7, both defaults
  carry it, the theme test asserts it, and a theme file without it is
  refused the way v6 files without their additions were.
- Page tabs, badges, rows and chips reuse the existing chip drawing; cards,
  pictures, the bipolar knob and the modulation arc are new drawing
  functions beside `draw_knob`.

### 8.9 Tests for the window (`fontelle-ui/tests/flopsynth.rs`)

Written first, against `todo!()` layout functions:

- Every control in the view gets a cell, every card a header, on every page,
  at the default size and at the minimum size, with no rectangle outside the
  body and no two cells overlapping (`nothing_overlaps_at_any_size`).
- `flopsynth_hit` names the control under every cell's centre, the tab under
  each tab, the badge under each badge, the node under each envelope node;
  the modulation ring band is hit outside the knob and not inside it.
- `wave_position_at` maps the picture's left edge to 0 and right edge to 1;
  `filter_xy_at` maps the EQ axis (same functions as `eq_freq_at`).
- `env_curve_points` starts at 0, peaks at 1 after the attack, holds the
  sustain, ends at 0; the square-root time axis puts 5 ms at a visible width.
- `assign_target_at` returns the destination for a lit knob and `None` for a
  knob that is not a destination (the output knob) and for empty air.
- The header carries the preset bar's layout at the Flopsynth width, and the
  bar's own tests (§P.11) cover the name and the `*`.
- The Presets page lists the category's presets, the search filters them,
  and the User category is present with zero rows without laying out any.
- `render_headless`: a `shoot_flopsynth` that renders the Synth page in both
  themes; pixel assertions that a card's header strip is `panel_header`, an
  off oscillator's caption is `text_muted`, a modulated knob's arc pixel is
  `modulation` and an automated one's groove is `param_automated`, sampled
  off grid lines (the PROGRESS 2026-09-06 lesson about sampling exactly on a
  line).

And in `fontelle-app/tests/flopsynth_ui.rs`, through `StudioHost`: the view
for a fresh Flopsynth channel has five oscillators, two filters, four
envelopes, four LFOs, four macros, one route (ENV 2 → F1 cutoff), no fx;
every control's address round-trips through `set_instrument_param` and reads
back; adding a route through the edit enum changes the view and the sound;
choosing a preset renames the channel and the header recognises it; a knob
move un-recognises it; undo re-recognises it.

---

## 9. The plumbing

### 9.1 `fontelle-core`

- `src/flopsynth/mod.rs`: `flopsynth_init()`, `layer_role`, `LayerRole`,
  `addresses(&Patch) -> Vec<String>`, `destinations(&Patch)`, `is_flopsynth`.
- `src/flopsynth/presets.rs`: `FlopsynthCategory`, the archetypes, the
  `Recipe` structs and the `FACTORY` row table the xtask exports (§7.1).
- `patch.rs`: `Source::Synth`, `Patch::{fx, macros, output_db}`,
  `FilterSlot` fields, `Lfo` fields; `patch_format.rs`: version 1, the
  migration, `StoredSource::Synth`; `patch_params.rs`: the §4 rows.
- `voice.rs`: `LayerPlayback::synth: SynthState`, `lfos: [LfoState; 4]`, the
  four filter buses, the reverse layer walk for modulators, the ramped
  cutoff, the Random/Counter sources, `Amp`; `sampler.rs`: `RenderClock`,
  `output_db`, `active_voices` already exists.
- `mod_matrix.rs`: the new sources and destinations with `full_scale`.

### 9.2 `fontelle-dsp`

`wavetable.rs`, `synth_osc.rs`, `ladder.rs`, `formant.rs`, `comb.rs`,
`lfo.rs`; `envelope.rs` shapes; `filter.rs` gains the 24 dB cascade helper.
Every type a voice holds is `Copy`; every buffer is a fixed array.

### 9.3 `fontelle-engine`

`SamplerNode`: `RenderClock` from the transport; the fx chain (`EffectState`
per slot, dry/wet, order); `latency_samples` stays 0 (the kinds are chosen
so). Nothing for the idle gate — it measures (§2.2). `flopsynth_no_allocation.rs` mirrors
`plugin_no_allocation.rs` with the heaviest preset.

### 9.4 `fontelle-app`

- `src/flopsynth.rs`: `describe_flopsynth(&Patch, ..) -> FlopsynthView`
  (wave points, response curves, mod ranges per control, matched preset,
  route descriptions), `apply_edit(&mut Patch, FlopsynthEdit) -> EditKind
  {Parameter, Structure}`, user preset files.
- `session.rs`: `kind_of`, `starter_patch`, `instrument()` returns the
  Flopsynth view kind, `set_instrument_param` goes on the wire (§2.3) for
  every patch parameter, `flopsynth_edit`, `instrument_editor_size`. Presets,
  saving, prev/next and favourites are the system's (§P) and need nothing
  here.
- `instrument.rs`: `patch_addresses` dispatches to `flopsynth::addresses`.
- `realise.rs`: nothing — `param_nodes` reads `patch_addresses`.
- `xtask`: the Flopsynth rows in `export-factory-presets` (§7.1).

### 9.5 Presets, saving, favourites

All of it is §P. Flopsynth's only contribution is the recipe table that
generates its factory files (§7.1) and the in-window Presets page, which is a
filtered view of the bank (§8.6).

### 9.6 `StudioHost`

Additions, all defaulted in `DocumentHost`'s test doubles:

```rust
fn instrument_view_kind(&self) -> InstrumentViewKind;      // Grid | Flopsynth
fn flopsynth(&self) -> Option<FlopsynthView>;
fn flopsynth_edit(&mut self, edit: FlopsynthEdit);         // routes, fx, macros' names, page
fn instrument_editor_size(&self) -> (u32, u32);
```

`set_instrument_param`, `automate_instrument_param`, `preview_preset` and the
preset system's seams (`preset_state`, `apply_preset`, `step_preset`,
`save_preset`, `presets_for_device`, `toggle_favorite`) are reused as they
are.

---

## 10. Performance budget

- **Bench:** `crates/fontelle-core/benches/flopsynth.rs` with `criterion`
  as a dev-dependency (TDD §20.5 asked for benches in CI from day one and the
  `benches/` folder is empty; this is the first). Cases: one voice of Init;
  one voice of "Supersaw" (3 × 7 unison, two filters); sixteen voices of
  "Choir Ahh" (formant filter, chorus, reverb — through `SamplerNode`); the
  wavetable bank's build.
- **Targets (release, 48 kHz, this machine):** Init voice ≤ 0.3 % of a core;
  Supersaw voice ≤ 1.2 %; sixteen Choir Ahh voices with their chain ≤ 8 %
  (gate 7 in §1). A number over budget is a design conversation, not a
  loosened target.
- **Memory:** a voice grows from today's ~2 KB by the synth state (five
  layers × 8 phases), the comb lines (8 KB per slot) and the LFO states —
  under 24 KB; 256 voices is 6 MB, allocated once at `Sampler::new`. The
  wavetable bank's ceiling is in §3.2.
- **Where the cost goes:** per-sample work is the table reads (4 per unison
  voice), the filters (2–8 one-pole/SVF stages per channel) and the `tanh`s
  (drive, ladder). Everything else — envelope shapes, LFOs, the matrix,
  coefficient rebuilds — is per block or per `FILTER_STEP`. If the Supersaw
  voice misses its budget, the first thing to try is a two-frame read (skip
  frame interpolation when position sits on a frame) and the second is
  `f32::tanh` → a rational approximation in `fontelle-dsp`.

---

## 11. Build order

Eight phases. Each lists what to test first, what to build, and the gate that
closes it. Do not start a phase's implementation before its tests are written
and seen failing; do not start the next phase before the gate.

### Phase P — the preset system, for every device

§P.12 has the four sub-phases and their gates. It comes first by Ty's
instruction and by dependency: Phase 3 lands its bank in it and Phase 5's
window carries its bar. When it closes, the drum machine's kits, the
distortion's, the bitcrush's and Soften's presets are files, every editor
window has the bar, the browser has the tab, and `PROGRESS.md` says so.

### Phase 0 — the patch grows, and nothing that exists changes (types, format, addresses)

*Tests first:* `fontelle-core/tests/patch_format.rs` (§5),
`fontelle-core/tests/patch_params.rs` extended with every §4 row (the
round-trip iterates `flopsynth::addresses`), `fontelle-core/tests/flopsynth.rs`
(`init` is a Flopsynth patch, has five Synth layers in role order, leaves
headroom, `kind_of` says Flopsynth, `addresses` has no duplicates and every
one is accepted by `set`), `fontelle-app/tests/instrument_kinds.rs` extended
to six kinds, `fontelle-app/tests/flopsynth.rs` (on the menu, arrives playing,
the roll plays every key, saves and reopens).

*Build:* `SynthOsc` and its enums (dsp, with `todo!()` render), `Source::Synth`,
`Patch` fields, `FilterSlot`/`Lfo`/`EnvelopeConfig` fields, the format bump
and migration, `patch_params` rows, `flopsynth::{init, addresses, layer_role}`,
`InstrumentKind::Flopsynth`, `kind_of`, `starter_patch`, `patch_addresses`
dispatch. The voice renders a `Source::Synth` layer as silence for now.

*Gate:* `cargo test --workspace` green, clippy clean, and every patch the
tree could write before this phase round-trips unchanged.

### Phase 1 — the sound (dsp and voice)

*Tests first:* `fontelle-dsp/tests/{wavetable,synth_osc,filters,envelope_shapes}.rs`,
`fontelle-core/tests/{lfo,mod_matrix}.rs`, plus in
`fontelle-core/tests/flopsynth.rs`: a note through Init sounds at its pitch,
filter routing per layer is heard (a layer routed Bypass ignores a closed
F1), the reverse layer walk makes FM audible, `Random` differs per note.

*Build:* §3.1–3.8 in the order wavetable → oscillator → voice integration →
filters → envelopes → LFOs (with `RenderClock`) → matrix → macros →
`output_db`.

*Gate:* the dsp and core suites green; `no_allocation_during_render.rs`
extended with a Flopsynth patch and green; a **listen** — `--play-sf2`'s
sibling, a `--play-flopsynth <preset>` flag on the binary (the Init patch
until Phase 3), heard through the speakers and the outcome written in
`PROGRESS.md`.

### Phase 2 — the wire and the chain (engine and session)

*Tests first:* `fontelle-engine/tests/flopsynth_fx.rs` (§3.9),
`fontelle-engine/tests/flopsynth_no_allocation.rs`,
`fontelle-app/tests/flopsynth_live.rs` (`a_knob_drag_does_not_cut_a_held_note`,
a structural edit rebuilds and a parameter edit does not — observable through
`Session::revision` moving and the graph publisher's swap count, which needs
a small `graphs_published()` counter on the session for tests), the existing
`instrument_editor.rs` suite still green with the soundfont panel on the wire.

*Build:* `RenderClock` into `SamplerNode`, the fx chain,
`store_patch_quiet` and the live `ParamValue` path in `set_instrument_param`
for **every** patch parameter, `EditKind` dispatch.

*Gate:* suites green; the guarding allocator passes with the chain; the
studio, in the real window, sweeps a 3OSC cutoff under a held chord without a
gap (a human listens, and says so in `PROGRESS.md`).

### Phase 3 — the bank

*Tests first:* `fontelle-core/tests/flopsynth_presets.rs` (§7.4) against a
one-entry bank; the `stays_inside_full_scale` and loudness tests in the
engine suite.

*Build:* the categories, the archetypes, the 130 rows, the xtask export of
them into `assets/presets/flopsynth/`, `--play-flopsynth <name>`.

*Gate:* every §7.4 test green at ≥ 200 presets; the files are committed and
equal their rows; Ty has heard a walk through the bank (the binary can play
them in sequence: `--play-flopsynth all`, and the bar's `▶` does the same in
the window), because "audibly apart" by three numbers is necessary and not
sufficient (`drum-kit-axes`).

### Phase 4 — the window: geometry and drawing

*Tests first:* `fontelle-ui/tests/flopsynth.rs` (§8.9, every layout and hit
claim), the headless shots.

*Build:* `FlopsynthView` (ui) and `describe_flopsynth` (app),
`canvas/flopsynth.rs` layout for all four pages, `draw_flopsynth`, the
`modulation` token, the bipolar knob and the arc, the pictures, the editor
size seam, `EditorWindowChrome::Flopsynth`. No gestures yet beyond the knob
drag and the drop-downs the grid already has.

*Gate:* geometry suite green; both themes' Synth page dumped through
`FONTELLE_UI_DUMP` and looked at; opened in the real window on a Flopsynth
channel and on a soundfont channel (which must still get the grid).

### Phase 5 — the window: gestures, modulation and presets

*Tests first:* the gesture rows of §8.7 in `fontelle-ui/tests/flopsynth.rs`;
`fontelle-app/tests/flopsynth_ui.rs` (§8.9's second list, plus the bar's
name/`*`/undo claims for this device).

*Build:* drag-to-assign and the lit rings, ring drag, envelope nodes, wave
and filter picture drags, the matrix rows, the Effects page, the Presets page
as a view over the bank, `Init`, the knob menu's remove-modulation rows,
tooltips for every hit. The bar, its keys and its saving are already there
from Phase P.

*Gate:* suites green; every gesture in §8.7 driven with the XTEST script
against the nested server and seen to do what the row says (the memory note's
three traps apply: warm-up move, explicit focus, re-grab before believing).

### Phase 6 — polish and the leftovers

- The voice-count meter and the LFO phase dot (animators only while sounding).
- `--play-flopsynth` documented in `--help`.
- The bench (§10) checked in, numbers in `PROGRESS.md`.
- `docs/effects-catalogue.md` §2.7's last bullet ("an LFO that drives other
  effects' parameters is not an insert's job") gains a sentence pointing at
  the matrix as the place that job now lives.
- A `PROGRESS.md` section per phase, and this document's status line updated.

---

## 12. Deliberately not in v1

Each with the reason and the seam it would use, so nobody re-derives it:

- **User wavetable import** (a `.wav` of 2048-sample frames). Needs an
  `AssetRef`-backed `WavetableId::User(SampleRef)` and the relink prompt
  (§17.4). The bank's `get` is the seam; a user table is one more arm.
- **Sampled oscillators** (Omnisphere's core). A `Source::Sample` layer
  *already* plays inside a Flopsynth patch — the roles are a convention. What
  is missing is the window drawing a sixth card for it and a way to put a
  file there; the name field is already a drop target. This is the cheapest
  large feature after v1 and it is why the roles are not enforced.
- **A grand piano.** Not attempted by synthesis; the soundfont player is the
  tool, and saying so on the Keys category is more honest than a preset called
  "Piano" that is not one.
- **Per-voice oversampling** for FM/sync/ladder. The mip tables and polyBLEP
  handle the linear cases; the nonlinear ones alias at the top of the
  keyboard. A `quality` chooser per oscillator (off/2×) is the addition, and
  the test is the existing `energy_below` one at a high note.
- **An arpeggiator inside the synth.** The roll's arpeggiate command is the
  arpeggiator (TDD §16.5) and a second one in the instrument is the second
  addressing scheme §8.2 forbids for time.
- **MPE.** Per-note pitch arrives as slides and note properties already; a
  per-note *continuous* controller is a `NoteTrigger` change, not a Flopsynth
  one.
- **Modulating effect parameters from the matrix.** Effects are per
  instrument and the matrix is per voice; Serum sums voices' modulation for
  it. The addition is a `ModDest::FxParam(slot, index)` evaluated once per
  block from the *loudest* voice — small, and worth doing once someone asks.
- **Standalone plugin export of Flopsynth** (`fontelle-plugin`). The crate
  is 92 lines and M2 was deferred (`first-usable-plan` §2.2). Nothing here
  forecloses it: the patch, the addresses and the window are all
  host-agnostic by construction.
- **A theme editor, or per-preset colours.** §16.6.
- **A hosted plugin's own factory presets** (CLAP preset discovery, LV2
  `pset:Preset`). The preset system saves and loads a plugin's *state* as a
  user preset from day one (§P.2); reading the plugin's shipped presets into
  the bank is a `fontelle-host` scanner per format, and a second origin
  (`PresetOrigin::Plugin`) on the row.
- **Drag a preset row onto a rack row or a strip.** The tab applies to the
  selected channel or the open insert (§P.8); a drop target is the rack's
  and the mixer's existing drop plumbing plus one payload kind.

---

## 13. Risks

- **The window's interaction code lands in `app.rs`, which is 10 000 lines.**
  Keep every decision in `canvas/flopsynth.rs` as a pure function and the
  `app.rs` additions to dispatch; if a Flopsynth handler exceeds thirty lines
  it is doing arithmetic that belongs on the canvas.
- **Zipper and clicks.** Every new per-block value that reaches a sample
  (cutoff, position, unison detune, Amp) is ramped within the block or is a
  click waiting for a slow LFO. `modulated_cutoff_does_not_zipper` is the
  model; write the same test for position and Amp.
- **Loudness tuning is slow by hand.** The loudness test's failure message
  must print the per-preset RMS table so the agent can adjust `output_db`
  rows in one pass rather than one preset per run.
- **The format bump touches every stored patch.** The migration test with a
  hand-written v0 body is the guard; do not generate the v0 fixture with
  `to_data`.
- **Aliasing on the warp modes** at high notes. Measured, not hidden: the
  test asserts the level and `PROGRESS.md` records the number, so the
  oversampling decision in §12 is made on evidence.
- **The bank's "apart" test can be gamed** by making presets louder or
  brighter rather than different. The fourth axis (attack flux) and the
  loudness match are the counterweights; if a pair only passes on loudness,
  it fails by construction because loudness is normalised.

---

## 14. Decisions (Ty, 2026-09-06)

The first draft asked five questions; these are the answers, and the plan
above is written to them.

1. **Three oscillators.** A, B, C plus Sub and Noise (§2.1, §8.3).
2. **3OSC stays** for now (§2.1); revisit after the Init patch has been
   heard.
3. **Preset count:** the gate was 120 and the catalogue 130. Ty asked for the
   roster to be widened again on 2026-09-07, so the gate is now **200 with ten
   to a category** and the catalogue is 210 — still a row each, and still the
   §7.4 tests that decide whether a row earns its place.
4. **Window size: 1180 × 740, minimum 980 × 620** (§8.8) — Ty asked for the
   recommendation and this is it, sized for a 1920-wide screen beside the
   arrangement. If it proves large on his display the fold is written in
   §8.8's own terms: air first, then the pictures, down to 980 wide.
5. **The name persists, the `*` marks unsaved edits**, and saving goes to
   the same preset or to a new one — through the DAW-wide preset system
   (§P), which is built **first**. This replaces the "recognition over
   memory" half of catalogue rule 10 for every device; the first half (a
   preset is not a parameter) stands.
6. **The live parameter wire (§2.3) is approved** and applies to every
   built-in instrument's panel.
