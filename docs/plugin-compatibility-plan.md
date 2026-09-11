# Widening what Fontelle can host

**Status, 2026-09-11: the in-tree items are done** — plugin editors, LV2
state, the bridge seam and delay compensation are built (`PROGRESS.md`,
2026-09-05 and 2026-09-06). The VST bridge itself lives out of tree and is
not shipped; see `PROGRESS.md` for what that means for a release.

Written 2026-09-05, after the pass that made CLAP and LV2 plugins audible and
gave them their own editors. This is the ordered plan for the four things that
still limit what a person can actually use, and it is meant to be worked
top-down: each item is smaller and higher-value than the one below it.

**Read `PROGRESS.md`'s 2026-09-05 entries first.** They say what was just built
and why, and items 1 and 2 below are both repairs to seams that work names.

**Status, later on 2026-09-05:** items 1, 2 and 3 are done, and the first half
of item 4 (the ABI v2 editor entry points). The decisions each item named are
recorded in `PROGRESS.md`'s "the compatibility plan, top down" entry and in
TDD §8.4. What remains is the VST3 bridge itself, and the follow-ups listed
under §8.4's "Still not done".

**Status, 2026-09-06:** every follow-up this tree can hold is closed — the
offline bounce hosts plugins, LV2 sidechains are read off `lv2:isSideChain`,
slides reach a hosted instrument, the wheels reach a built-in voice (§7.4's
half of item 3), and the bridge ABI is v3 with the rest of a performance on
it. See `PROGRESS.md`'s 2026-09-06 entry. **What is left is item 4's second
half — the VST3 bridge itself — which is a separate repository and cannot be
written here.**

---

## Ground rules for this work

- **Tests first, always.** This project's rule: the test is written against the
  intended API and confirmed failing (a compile error against a missing method
  counts) *before* the implementation. Every item below names the fixture that
  makes its test honest.
- **The fixtures are real plugins.** `fontelle-testplug` is a CLAP bundle and
  `fontelle-testlv2` an LV2 bundle, both built in-tree, both loaded through the
  real entry point across the real ABI. Do not mock a plugin, and do not test
  against whatever is installed on the machine — that tests the machine.
- **After editing a fixture, rebuild it**: `cargo build -p fontelle-testlv2`
  (or `-p fontelle-testplug`). A staleness guard in `tests/common/mod.rs` fails
  every test in the suite with "is newer than the built plugin" otherwise, and
  it looks like a catastrophe rather than a stale `.so`.
- **`lv2_raw` is pinned to `=0.2.0` on purpose.** It must be the version `livi`
  uses, because the `urid:map` feature handed to an LV2 UI is the plugin's own,
  taken straight out of `livi::Features`. Two versions of `LV2Feature` are two
  types that cannot meet. Do not bump it.
- **Seeing it work**: `docs/handoff.md` and the memory note on screenshotting
  cover driving the real window. For a plugin's own editor there is
  `cargo run -p fontelle-host --example plugin_editor -- <bundle> [id] [secs]`,
  with `PROBE_DUMP=out.png` to write out what the plugin actually drew.
- **Do not write into the user's projects folder** (`/mnt/disks/6tb/FontelleProjects`)
  while testing. Use a scratch directory.

---

## 1. LV2 plugin state — the sampler's file has to survive a save

**Why first.** It is the smallest of the four and it repairs the one just
built: an LV2 sampler's editor now opens and can be handed a file, and the file
is gone the next time the project opens. `HostedPlugin::keeps_state` is
hardcoded `false` for LV2 (`plugin.rs`, in `open_lv2`) and the state extension
is never read, so an LV2 plugin is saved as its control ports and nothing else.
Parameters come back; a loaded sample, a drawn curve, a chosen preset do not.

**Where.** `crates/fontelle-host/src/lv2.rs` and the `save_state` / `load_state`
/ `keeps_state` arms in `crates/fontelle-host/src/plugin.rs`.

**The hard part, and it is the whole job.** LV2's `state:interface` is
`extension_data` on the **instance**, and for LV2 this program keeps the whole
instance inside `Lv2Processor` — the audio half, which is out in a graph while
anything is playing (see the `lv2` module note on why there is one object and
not two). `HostedPlugin::snapshot` is a main-thread call. So the first design
decision is how the main thread reaches the instance:

- **Recommended**: go through `ProcessorBay`. `PluginRack` already reclaims a
  processor on the main thread to retire a plugin (`Live::retire`); saving can
  do the same — reclaim, save or restore, park it back. The cost is a handful
  of silent blocks at the moment somebody presses Ctrl+S, against a correctness
  problem that loses work. Prove the silence is bounded.
- The alternative — keeping a second main-thread handle — is not available:
  lilv gives one instance, and calling `save` concurrently with `run` is what
  the specification forbids.

**Paths are the second half.** A sampler saves *a reference to a file*. LV2's
answer is the `state:mapPath` / `state:freePath` features: the host is asked to
turn an absolute path into an abstract one for storage and back again on load.
Without them a project moved to another machine points at a file that is not
there. Implement `mapPath` as identity first and get the round trip working;
then decide whether Fontelle copies the sample into the bundle, which is
**INVARIANT 8 and §17.4 territory** — a project names what a thing *is* and the
machine resolves where — and is a real design decision, not a detail.

**Fixture.** Give `fontelle-testlv2`'s gain (or a fourth plugin) a
`state:interface` that stores something no control port exposes — a string, a
counter. Then the test is: set it, snapshot, open a second instance, restore,
and read it back. `keeps_state()` must become `true` for that plugin and stay
`false` for one without the interface, which the `plain` plugin already covers.

**Done when.** In the real window: put a sample into an LSP Multi-Sampler,
save, quit, reopen the project, and hear the sample.

---

## 2. Every audio port, and sidechains

**Why second.** Small, mechanical, and it removes a class of "this plugin
behaves oddly" that is otherwise impossible for a user to diagnose.

**What is wrong.** `plugin.rs::read_audio_ports` takes `.next()` — the *first*
port's channel count — and `processor.rs::ClapProcessor::run` builds a
one-element array for each direction. So a CLAP plugin declaring two output
ports is given one. CLAP says the host passes as many ports as the plugin
declared, and a plugin is entitled to check; some refuse to render. It also
means multi-output instruments (LSP's "DirectOut" sampler variants, drum
plugins with a bus per pad) can only ever produce their main output, and that
a plugin with a sidechain input never receives one.

**LV2 is already fine** — `livi` connects every declared port — so this is
CLAP-only work in `processor.rs` and `plugin.rs`.

**Shape.** Keep a `Vec` of channel counts per port rather than one number;
allocate one buffer set per port at activation (still nothing per block); build
`AudioPorts::with_capacity(total_channels, port_count)` and hand over every
port. The **main** port — the one flagged `IS_MAIN`, falling back to index 0 —
is the one the mixer bus maps to. Decide explicitly what happens to the extra
outputs and write it down: dropping them is defensible for now, summing them
into the main bus is not (it would make a drum plugin louder than it is).

**Sidechains** ride on machinery that already exists: `EffectSlot::key` names
another track and `realise::schedule_inserts` already wires a `KeyTap` for the
built-in effects that have a detector. Feed that same tap into the plugin's
sidechain input port.

**Fixture.** `fontelle-testplug`'s gain grows a second output port and a
sidechain input; assert the host presents both port counts, that the main
output still reaches the bus, and that a signal on the sidechain changes what
the plugin does. Note this will move the port-count assertions in
`tests/hosting.rs` — that is the test doing its job.

---

## 3. Controllers: pitch bend, mod wheel, aftertouch

**Why third.** Bigger than the two above because it changes the document model,
and it is the one that most changes how the program *feels* to play.

**What is wrong.** `fontelle_types::EventPayload` has four variants — `NoteOn`,
`NoteOff`, `NoteSlide`, `ParamValue` — and no way to say "the mod wheel moved".
`fontelle-midi` decodes control changes and pitch bend perfectly well
(`message.rs`) and then `router.rs` drops every one of them except the sustain
pedal, which it turns into note bookkeeping. So a keyboard's wheels reach
nothing: not plugins, not the built-in instruments.

**Order within this item.** Do the plugin half first; it is self-contained and
proves the whole path before anything touches `fontelle-core`.

1. **The event.** Add the payloads — a controller with a number and a value,
   a pitch bend, a channel pressure. Keep the model's habit: these are
   *performance* events, distinct from `ParamValue`, which is automation with a
   §8.2 address. Say so in the doc comment, because the two will look alike to
   whoever reads it next.
2. **Out of the router.** `router.rs`'s catch-all arm at the bottom becomes
   real forwarding. Keep the sustain-pedal special case exactly as it is.
3. **Into a plugin.** LV2 is nearly free: `Lv2Processor::push` already sends
   three-byte MIDI, so a controller is one more call. CLAP is the decision —
   the honest mapping is a `MidiEvent` on the note port when the plugin's note
   port declares the MIDI dialect (`read_note_ports` already reads the port;
   extend it to report the dialect), and CLAP note expressions otherwise. Do
   not invent a mapping onto parameters.
4. **Into the built-ins** is a separate decision and can be a separate pass:
   what a `fontelle-core` `Sampler` does with a pitch bend or a mod wheel is a
   §7.4 question about the voice, not a hosting question.

**Fixture.** `fontelle-testlv2`'s sine already reads MIDI from its atom port —
make it respond to controller 1 by changing its level, and the test asserts a
sound changing. `fontelle-testplug`'s sine gets the CLAP equivalent.

**While you are here.** `PluginNode::take_notes` drops `NoteSlide` with a
comment explaining why. Once note expressions exist for item 3's CLAP half, a
slide becomes expressible; that is the moment to revisit it.

---

## 4. The VST3 bridge — the only thing that changes the format answer

**Why last, and why it will not fit in one session.** It is a separate
repository, it needs Steinberg's SDK, and §3.4 is the reason it lives outside
this tree: Fontelle never links a bridge and a bridge never links Fontelle.

**What it unlocks.** VST3 is what most paid Linux plugins ship, and `yabridge`
exposes Windows VST2 and VST3 plugins as native VST3 — so one bridge covers
both native Linux VST3 *and* the Windows catalogue. Nothing currently installed
on this machine needs it: there are zero `.vst3` bundles here today.

**The tree is already shaped for it.** `fontelle-bridge-abi` is a versioned
`#[repr(C)]` vtable at `ABI_VERSION = 1`; `fontelle_host::bridge` loads bridges
out of `$XDG_DATA_HOME/fontelle/bridges`; `fontelle-testbridge` is a working
bridge with no SDK in it and is what the tests load. A bridged plugin is
already the third arm of `Inner` and `HostedProcessor`.

**Two pieces of work, in order.**

1. **ABI v2: editors.** The bridge vtable has no GUI entry points, which is why
   bridged plugins keep the generated panel. The shape is now known rather than
   guessed, because both hosted formats needed exactly the same four things:
   `has_editor(instance) -> u8`, `open_editor(instance, x11_window_id) -> i32`,
   `close_editor(instance)`, and a per-frame `tick_editor(instance)`, plus a
   way to report a wanted size. They hang off the `PluginWindow` that already
   exists in `fontelle_host::gui`; the host side of this is an afternoon.
   `fontelle-testbridge` implements them trivially (reporting no editor) so the
   "this plugin has no face" path stays honest.
2. **The bridge itself**, out of tree, against the VST3 SDK.

*(1) is done, and so is the performance half above it; the standing
instruction below still governs anything further — per-note pitch over the
ABI waits for a bridge that can carry it.*

**Do not do (1) speculatively before somebody is writing (2).** §8.4's standing
instruction is not to add hosting abstractions before there is a host for them,
and that instruction has been right twice already.

---

## What is deliberately not on this list

- **suil.** The usual way to host an LV2 UI of a foreign toolkit. Of the
  seventeen bundles installed here that ship a UI, seventeen ship `ui:X11UI`,
  which `fontelle_host::lv2_ui` hosts directly. suil would add Gtk and Qt UIs
  and nothing on this machine wants one.
- **Calf's editors.** Calf ships no UI of any kind in its Turtle. There is
  nothing to host; the generated panel is the only face it has.
- **Latency compensation.** Real, and worth doing, but it is a §5.3 engine
  question rather than a hosting one: `AudioNode::latency_samples` exists and
  nothing reads it. Its own pass.
