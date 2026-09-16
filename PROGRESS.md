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

## Where things stand (maintained; the entries below are history)

**As of 2026-09-15 — `v0.5.1`, the keymap's edge cases.** A pass over
*"when a tooltip is shown it shows ur actual configured keybind"* and the
odd interactions around it: every control a key also drives now names its
`Action` (the editor tabs, the mixer's M/S, the rack tabs, the browser's
search box and its Export button, on top of the transport and toolbar
buttons), so every tip reads the map. A chord is read from the key
**without** its modifiers (`key_without_modifiers`), so Shift+1 is
`Shift+1` on every layout rather than `!` here and `1` there — and since
that would have broken `+` (Shift+= on a US keyboard), the symbol a press
*typed* is tried as a fallback only when the shifted non-letter chord is
bound to nothing (`Keymap::action_of_press`). The arrows are no longer
chords (they are a fixed family answered before the map). On the sheet the
rebindable row under the pointer lights up and the cursor is a hand;
tooltips from controls under the sheet are suppressed; an editor window
routes its keys to the sheet while it is up, so F1 from the EQ window
works and a row can listen to a chord typed there.

**As of 2026-09-15 — `v0.5.0`, remappable shortcuts.** TDD §16.5's *"all
keybinds are remappable"* is built. `canvas/keymap.rs` is the map: a `Chord`
(modifiers + one key, with a text form), an `Action` for every command a key
can mean (38, each with a stable id and a context — global, studio, editor),
and a `Keymap` from chords to actions with FL's defaults. The window's `key`,
`global_key` and `editor_own_key` now ask the map what a chord means and
dispatch on the action (`studio_action`); the only keys still literal are
Esc, the arrows, Enter in lists and text-field keys, which the page lists
without a chip. On the shortcuts page a rebindable chip has an accent edge;
click its row and it listens (`Rebind`): the chord is read on the **release**
of the key with the modifiers held at the press or the release, so Ctrl let
go before Q is still Ctrl+Q. A chord taken from another action unbinds it
there and the page says so; *Reset to defaults* puts everything back.
Tooltips and the Tools menu read their keys off the map (`action()` on
`TransportHit`, `RollControl`, `TimelineControl`, `ToolMenuItem::label_in`).
Only the differences from the defaults are kept, in `settings.json` under
`keybinds` (`StudioHost::keymap_overrides` / `set_keymap_overrides`, written
on every change). Verified in the running studio: rebind, conflict, the key
working afterwards, the file, and reset.

**As of 2026-09-15 — `v0.4.0`, the second UX pass.** Seven things from one
report, each tested first and each looked at in the running studio on the
nested X server (`docs/handoff.md` §5):

- **A keyboard shortcuts page** (`canvas/keybinds.rs`): every binding the
  window answers, in eleven sections, on a scrolling two-column sheet. Opened
  by a `?` on the start menu, a small `?` after the transport bar's mode chip,
  and **F1**; Esc, F1, its × or a press off the card shuts it. The catalogue
  is data, and `tests/keybinds.rs` holds it to the keys `key`/`global_key`
  actually match on, so a binding added without a line here fails a test.
- **The tempo box is typed into.** A click (press and release without
  travel) opens it as a field seeded with the tempo, all selected; Enter
  writes what `transport::parse_tempo` accepts (clamped, rounded to the two
  places the box shows), Esc puts it back, a press elsewhere keeps a valid
  entry. Drag still slides it.
- **The signature is a drop-down** (`MenuTarget::Signature`,
  `signature_menu_entries`): 1/4 … 16/4 with the current one greyed. The
  click-to-step `cycle_beats_per_bar` is gone.
- **F** flips the rack between Instruments and Prefabs (`RackTab::other`).
- **Tab** swaps which of the arrangement and the editor is the tall one
  (`layout::toggled_timeline_height`, two thirds / one third). Stateless:
  whichever has more than half gives it up, so a seam dragged by hand needs
  nothing undone before the next press.
- **The transport bar's meter was broken two ways**, both fixed. (1) Every
  graph rebuild minted a fresh `MasterMeter` while `EngineHost` kept the
  first graph's `Arc` — so the bar read silence from the first added
  channel/insert onward ("shows sometimes but not always"). The same wire
  the metronome switch was once dead on: `KeptTaps.master`,
  `MasterNode::with_meter`, `Session::with_master_meter`
  (`fontelle-app/tests/master_meter.rs`). (2) `MasterNode` published
  `PeakRmsMeter::peak`, which is *held* since the node's last reset, so the
  bar re-read the session's loudest moment every frame until a rebuild
  ("leaves it hanging"). It publishes each block's own peak now, like the
  track meters, and the bar's ballistics are 120 dB/s with a 0.6 s hold so
  it agrees with the master strip about when the song went quiet.
- `Icon::Help`, and the tooltips on the new controls.

Full workspace suite (4036 tests) and `clippy --workspace --all-targets
-D warnings` green; released as `v0.4.0` on Ty's go-ahead for this update.

**As of 2026-09-15 — `v0.3.0`, the UX-polish release.** A pass over the two
places ordinary navigation caused unintended edits, reported from real use.
Four changes, each independently tested and the settings panel pixel-verified
(`render_headless::shoot_settings_controls`):

- **The wheel only ever scrolls, never edits.** It used to step a value under
  the pointer in the settings list, the transport tempo/signature, and every
  editor window; scrolling "to look around" nudged velocity, a fade, a knob.
  Every one of those value-stepping wheel paths is gone (`app.rs`); values
  change by drag, arrow keys and menus, never by the wheel.
- **Settings rows are real controls, not click-to-step.** A number is a drag
  slider (`Drag::SettingSlider`, absolute), a choice drops down
  (`MenuTarget::SettingChoice`), the update switch flips, folders/extensions
  stay buttons. The host answers one new query, `setting_controls()`, returning
  a per-row `SettingControl` descriptor — the window still knows nothing about
  what a row *means* (`SettingRow::control_kind`/`fraction`/`choices` own that
  in `fontelle-app`). Arrow keys nudge the focused row for precision.
- **Destructive settings actions are reversible or ask first.** Removing a
  plugin folder acts and offers an **Undo** toast; uninstalling an extension —
  which a click cannot take back — asks with a confirm modal first
  (`canvas/overlays.rs`).
- **The clip info panel reads like FL Studio.** The compact continuous
  parameters are a grid of labelled **knobs**, the fades stay sliders, choices
  are drop-downs, the waveform on top (`canvas/audio_clip.rs`).

Full workspace suite green (4009 tests) and `clippy --workspace --all-targets
-D warnings` clean. Ty approved the release.

**As of 2026-09-15.** The repository is public; `v0.2.0` is tagged and
released (VST 3 hosting, the two top entries' worth of polish, and MIDI
export). Ty triggered the tag; its `release.yml` built the four archives and
`SHA256SUMS`, and the `fontelle-vst2` extension released `v0.1.0` alongside. That
extension is now at **`v0.1.1`**: v0.1.0 was Linux-only and, it turned out,
broken for *every* real plugin — its `VST_MAGIC` was `0x5665_7350` ('VesP'),
a transposition of the ABI's `0x5673_7450` ('VstP'), so the loader rejected
every third-party plugin and only the fixture (which shared the typo) passed.
Found by loading a real plugin through the bridge, which the fixture test by
construction could not do. v0.1.1 fixes the constant, adds a real-plugin probe
(`tests/real.rs`) and CI, and builds all three platforms; verified end to end
against amsynth/ZynAddSubFX/ZynChorus/Wolf Spectrum on Linux and a real
`Synsonic BD-909.dll` under Wine on Windows. The shipped 0.2.0 install logic
needed no change. Two cross-platform test bugs the release CI surfaced
were fixed after the tag (test-only, shipped binaries unaffected): the
extension-install test hard-coded the Linux `.so` name where the install path
picks the per-OS library name (`77c68ee`), and the Wine-prefix folder tests
asserted POSIX paths that can't hold on native Windows and are now `cfg(unix)`
(`31b10ce`). CI is green on Linux, macOS and Windows. The
workspace is one version, `cargo fmt --all -- --check` passes,
`cargo clippy --workspace --all-targets -- -D warnings` is clean on Linux
and cross-checked for Windows, and `cargo test --workspace` is green at
4,000-odd tests. What is built is the README's *Project status* section,
which is kept true; what is designed and not built is the TDD's later
milestones. **Plugin export is now built** (MIDI, top entry); VST hosting is
VST 3 in the tree plus VST 2 through the `fontelle-vst2` extension (a separate
repo, built, tested and released — Ty's option-B decision to ship the
clean-room bridge and own the residual risk; see the 2026-09-14 entries). Every entry below this heading is the account of how each part came
to be, newest first, and later work sometimes reversed an earlier entry's
decision — where it did, the later entry says so. Read the code and its tests
for what is true now; read the entries for why.

**Next after the plugin update:** the real-project shakedown — somebody making an actual multi-part
piece in the window, on hardware, end to end, and fixing what that finds
(item 12 of the M0 plan). Two things the release surfaced that are still
open: the Windows and macOS builds come off the release workflow but are
not exercised by anybody yet (plugin editors are not shown there at all —
`fontelle-host`'s `gui.rs` says why), and the report that an input set in
a saved project does not capture until it is re-chosen has a fix that is a
best guess (top entry below) rather than a reproduction.

The one open engineering question is **Flopsynth's cost per voice**, which is
over the budget its plan set. The numbers and where the time goes are at the
end of the section below; the plan's own instruction is that this is a design
conversation rather than a target to loosen.

## 2026-09-15 (latest): the keymap — every shortcut a binding, and the page that changes them

> *"could you make these keybinds in the ? tab completely configurable so
> users can cleanly click on one they don't like, then input the new binding
> they want ... it waits for the release to see your combination basically.
> you please figure out the cleanest solution to accomplish this."*

The summary at the top has the build; what follows is the shape and why.

**The cleanest solution was to stop matching on keys at all.** The handlers
used to say `"z" if ctrl => undo()` in three places. Now they turn the event
into a `Chord`, ask `Keymap::action(chord, context)`, and `match` on the
`Action`. That one change is what makes everything else small: the page is
a list of actions whose chips read `keymap.label(action)`; a rebind is
`keymap.rebind(action, chord)`; the file is `keymap.overrides()`; the
tooltips ask the same map. The old catalogue of strings became a list of
`KeybindEntry::Action(..)` and `Fixed { .. }`, and the test that used to
check the strings now checks that every `Action` is on the page exactly
once.

**Contexts, not one flat map.** `D` is the delete tool in the studio and
`Delete` removes an EQ band in an editor window; the two never listen at
once, so they may share a key and a rebind of one need not cost the other.
Global actions (transport, history, file) are heard everywhere and take
from everywhere. `Context::overlaps` is the whole rule and the defaults
are tested against it.

**The listener commits on release.** `Rebind` remembers the key that went
down and the modifiers down with it, and returns the chord when *that* key
comes up, with the modifiers held at either end OR'd together — so
Ctrl-then-Q with Ctrl lifted first is Ctrl+Q, a modifier alone is never a
chord, and a second key pressed before the first lifts is the one listened
for. The window forwards key *releases* only while the sheet is up; nothing
else in it has ever needed one.

**Shift means nothing on a non-letter.** `+` is Shift+= here and its own
key elsewhere, and the window always took both. A chord normalises the
shift away for non-alphabetic characters at construction, so a binding made
on one keyboard reads the same on another.

## 2026-09-15: the shortcuts page, a typed tempo, a signature list, F and Tab, and the meter that lied twice

> *"could you make it so the f key toggles the tab between instrument and
> prefab ... a dedicated keybinds page that shows every single keybind ...
> i cant click and type in the field like an input field to input my tempo
> ... it should be a dropdown instead of just iterating through pre set
> list of options ... pressing tab should toggle between the piano/mixer
> section being larger or smaller than the arrangement ... the top right
> theres an audio monitor but it doesnt work correctly."*

The summary at the top of this file has the build. Two things worth
keeping from the day.

**The meter had two faults that presented as one.** The first was the
metronome's old bug on the other `Arc` — a rebuild minting a new
`MasterMeter` while the bar held the first — and was found by reading
`realise.rs`. The second was only found by *looking*: with the first fixed,
the bar tracked the master strip while a preview played and then sat at a
constant level for seconds after the strip had gone dark. That was
`MasterNode` publishing the DSP meter's **held** peak, so "peak since last
read" on the window's side was reading "peak since the node was reset" on
the engine's. A block's own peak is what a meter wants published; the hold
and the release belong to the reader. `nodes.rs` has the unit test, and
the limiter's look-ahead is why that test reads twice.

**Grabs of the nested X server fall seconds behind while the window
animates.** `docs/handoff.md` already says a grab is a frame behind; with
the transport rolling it was several, and three "Tab did nothing" and
"the keys were dropped" readings were nothing but that. Judge steady
states, stop the transport before judging a layout, and grab again
before believing a change is missing.

## 2026-09-14: MIDI export, a limiter that ducks, Flopsynth's search stops eating keys, and clicking a sound plays it

> *"cannot save piano roll to midi files ... im always typing in the search bar
> even when ive never clicked that input field ... side chain duck by routing a
> kick to another track ... clicking on an audio file in the import tab
> instantly imports it instead of ... preview on select."*

Five things, on top of the VST 3 update below it.

**MIDI export** (`fontelle_assets::export_project_to_midi`, the inverse of the
importer). The stub in `fontelle-midi` was a `todo!()` with no caller;
`export_midi_file` now lives beside `import_midi` in `fontelle-assets`, because
that is the crate that can see a `Project`. It writes a Format 1 SMF on the
project's own PPQN, so no timing is rescaled either way — the song, not the
open pattern: clip starts offset the notes, loops are unrolled the way the
compiler unrolls them, and each note is cut at the nearer of its pass end and
the clip's end. Slides are left out (they sound no voice of their own; writing
one would double the note it bends), and so are per-note pan/pitch/mod, which
MIDI has no field for. Reached from the roll's Tools menu (*Export MIDI
file…*) and **Ctrl+Shift+E**, through a new `desktop::choose_save_file`. The
roundtrip test is the strongest of the seven: export, import, same notes.

**A limiter as an insert, with a Ducking preset** (`EffectKind::Limiter`). The
brickwall DSP already existed for the master bus; this makes it the tenth
built-in effect and gives it a **sidechain**, which is what turns it into a
ducker: keyed from another track, its gain computer measures the *key*, so a
kick keyed into a low ceiling pushes the track down under it. Six presets, and
*ducking* is the one the report asked for — put it on the bass, key it from the
kick. The look-ahead is fixed (1.5 ms, compensated like the gate's) rather than
a knob, because the brickwall guarantee ties the window to the delay line and a
knob would mean resizing buffers on the audio thread. The existing sidechain
work — the compressor's key picker, the `SidechainPump` preset, adjustable
sends to "off" — already did most of a duck; this is the effect the report
named on top of it.

**Flopsynth's Presets search is click-to-focus now.** It used to take *every*
key while the Presets page was open — *"i cant use any daw keybinds while its
open"* — so Space never reached the transport and a typed letter went into the
box. It has the keyboard only after a click into it now (`WindowApp::flop_searching`),
exactly the way the browser's search box works: a press elsewhere or Escape
hands the keyboard back, and until then the global keybinds work in the window
like they do everywhere else. The box draws its lit outline and a caret when it
has focus so the state is visible.

**Clicking an Import file previews it; double-click imports it.** A click used
to import instantly; now it plays the file through the preview voice — the same
voice a soundfont preview uses, loaded with the file as a one-shot sampler
patch — a *listen*, not an import, nothing written to the document. Double-click
imports, and the stop button (or clicking another file) stops the preview.

**Dropping an Import file lands where the pointer is.** A drag onto the
arrangement used to make a new row at the foot every time; now it lands on the
row under the pointer, snapped to the current grid, and only a drop into the
empty space past the last row makes a new one (`AddAudioClip::on_lane`, and
`CarryTarget::Clip` carrying the target lane). The carry chip says which.

**`fontelle-vst2` is built** — the VST 2 extension, in its own repository
(`/mnt/disks/3tb/GithubRepositories/fontelle-vst2`, Ty's blank repo filled in).
A clean-room VST 2.4 host bridge implementing `fontelle-bridge-abi` v3: scan,
open, params, audio, notes, a performance, chunk state, and the plugin's own
editor embedded in an X11 window. No Steinberg code — the interface is a
clean-room transliteration with the provenance rule written into
`CONTRIBUTING.md`. A fixture plugin (an independent transliteration of the same
ABI) and the bridge load and run each other in the loader test. **It ships** —
told the interoperability risk is small but real and untested for a commercial
clean-room host, Ty chose to release it free, separate and opt-in and to own
that residual risk rather than wait on a formal legal opinion (`docs/vst-plan.md`
§3.3, option B). The product describes the *format* — "loads plugins in the
VST 2.4 format", no VST logo, the ® attribution. The Extensions page installs
it from its own release the way the updater installs Fontelle's.

## 2026-09-14: VST 3 hosted in the tree, plugin folders you can see, VST 2 as an extension, a download that shows its progress

> *"support the maximum amount of plugins and instruments as possible. people
> should be able to use their vst3s or download the vst2 extension ... also in
> this update make it so theres a progress bar when installing an update."*

**The research changed the answer, so the code did.** `docs/vst-plan.md` (last
session) verified the VST 3 SDK went MIT on 31 October 2025, which took away
every reason VST 3 was behind a bridge. A half-day spike opened Surge XT's
`.vst3` through the `vst3` crate (coupler-rs, MIT/Apache — not `vst3-sys`,
GPLv3), pushed a note through it and read audio out, which answered the one
unknown: the crate's host-side COM support is complete enough, no C++ shim.

**VST 3 is the third arm of `HostedPlugin`/`HostedProcessor**, in
`crates/fontelle-host/src/vst3.rs`, mapped interface-by-interface onto what
the CLAP arm already does. The host implements `IHostApplication`,
`IComponentHandler`, `IEventList`, `IParameterChanges`, `IBStream`,
`IPlugFrame` and `IRunLoop` as COM objects from Rust. Three things are
different about the format and the code says so: parameters are **normalised
on the wire** (a hosted VST 3 parameter's range is 0..1, or 0..steps, and the
RT thread converts by arithmetic); the wheels are **not events** but the
parameters `IMidiMapping` maps them to, resolved at open and driven as
parameter changes; a slide is a `kTuningTypeID` note expression, so per-note
pitch reaches VST 3 the way it reaches CLAP; and state is **two blobs**, the
component's and the controller's. Every bus is activated and handed a buffer
(the 2026-09-05 CLAP lesson), and every plugin is handed a real output
parameter queue too — a DPF-built plugin asserts on a null one. The editor is
an `IPlugView` in the same X11 window `gui.rs` makes, its run loop pumped on
the same `GuiPump` CLAP's timers are.

`crates/fontelle-testvst3` is the fixture — a real VST 3 module in Rust with
the same crate, a gain (sidechain, two state halves, an editor that drives the
host's run loop), a sine (mapped wheels, a sub bus, a slide) and a single-
component plugin — so every test in `hosting.rs` has a twin in `vst3.rs` (37
of them). **Heard on real plugins**: an ignored test opens every `.vst3` on
this machine — 205 of them (Surge XT, Dexed, the Dragonfly reverbs, the whole
LSP suite) — activates each, runs a block and plays every instrument. All 205
opened; the two instruments sounded. One trap that cost real time and is
written into the code: seeding Surge XT's 2,855 normalised parameters back to
it at activation turned its default patch down to a whisper (peak 0.01 against
0.31), because its values do not all round-trip through its own
`setParamNormalized` — so the VST 3 arm sends the component **only what the
document changed since open**, not the whole wire.

**Plugin folders you can see and manage** (`docs/vst-plan.md` §5): the list is
one row per folder under the add button, each named by its end, each a button
that removes it — a second folder was invisible except as a count before. The
VST 3 standard folders (`~/.vst3`, `/usr/lib/vst3`, `/usr/local/lib/vst3`,
`$VST3_PATH`) are searched by default. And *"sync it to their FL"*: one row,
*Use FL Studio's folders*, reads FL's extra search folders — off the Windows
registry through `reg query`, or off a Wine prefix's `user.reg` on Linux,
mapping a Windows path back through the prefix's drives (`daw_folders.rs`).

**Extensions** (`docs/vst-plan.md` §4): a catalogue compiled into the binary
(`extensions.rs`), one entry today — `vst2`. A row per entry under an
Extensions heading in Options installs it with the updater's own downloader
(the repo's `releases/latest`, the asset verified against `SHA256SUMS`,
renamed into the bridges folder) and removes it. A bridge whose ABI is not
this build's is listed as *needs a newer Fontelle* and never fetched.
Installing or removing is refused while a plugin is open. **`fontelle-vst2`
itself is a separate repository, gated on counsel** (§3.3) — the catalogue
points at it and the release of that download stays needs-review; nothing in
this tree links a VST 2 header. The test bridge fixture moved onto the `vst2`
tag to prove the path end to end.

**A progress bar when an update downloads.** `UpdateStatus::Downloading` now
carries `done`/`total`; the updater streams the archive through `curl` to a
pipe it reads in chunks (`fetch_with_progress`), reporting bytes as they
arrive, with a `HEAD` for the size first. The start menu draws a bar in the
offer's slot — filled in the accent to the fraction, marching indeterminate
when the server gave no size — and the line says *"6.0 of 8.0 MB"*. Looked at
in the headless dump.

`PluginFormat` gained `Vst2` (named, not hosted — a bridged-only format);
`hosted()` now returns true for `Vst3`. `FONTELLE_TDD.md` §1.3 and §3.4 are
brought up to date; `docs/plugin-compatibility-plan.md` §4 is marked
superseded. The three public gates are unchanged and remain Ty's: tagging a
release, and the VST 2 extension's own release.

## 2026-09-13: the organs had no fundamental, the blade marks its notes, a clip picks its instrument

> *"the rock organ sounds good but the rest of the flopsynth organ presets
> sound very noisy. help me make preset sounds are not too noisy and actually
> sound like what theyre trying to imitate."* — and two more in the same
> breath: the cut tool's marks in the roll, and a clip selecting its
> instrument.

**The organs, measured.** `examples/preset_noise` read the shelf's
harmonic-to-noise over the sustain: eleven of the twelve between −5 and
+8 dB, the Rock Organ at 17. Muting the noise layer put the same eleven at
16–50, so it was the noise layer — and it was two things, not one. The
`organ` archetype's key click was a noise layer **at −30 dB for as long as
the key was down**, with an envelope adding a burst on top (the memory note
from 2026-09-07 had already said so and nobody had acted on it). And the
Init patch's oscillators are on the *serial* filter route, which goes
through F2 — the click's 2 kHz high-pass — so **every organ that did not
route its oscillators to a filter of its own had its tone high-passed at
2 kHz**: no fundamental, no 16′ bar, no pedals. The Rock Organ routes A and
B to F1 by hand, and that, more than its drive, is why it was the one that
sounded like an organ. Both fixed in the archetype: the noise parked a
decibel over `SILENT_DB` (rendered, so the click's first block is not
lost; inaudible) and lifted 60 dB by a fifteen-millisecond envelope,
velocity scaling the click rather than a bed (`route_via`); A, B and C on
`Bypass`. Then the rows: transistor combos and pipe organs have no
contacts, so `no_click()` for the Vox, the Farfisa, the harmonium and the
theatre organ, and a `chiff()` — a band-passed breath at the front of the
note — for the Church and the Pipe Flute; the three rows that had set
their own continuous noise bed lose it; "Drawbar 888" is *888000000*
(0.23 of the knob) rather than all nine bars; the harmonium is a saw
through a nasal low-pass rather than an odd-only table that read 0.15
from the Pipe Flute once both had fundamentals; the Jazz gets the C3's
scanner vibrato; the pedals get the archetype's attack back. Eight trims
rewritten from `preset_probe` — Theatre had been peaking at 4.3 with its
tone high-passed away. The shelf reads 30–49 dB now; `tests/organ_click.rs`
holds a 25 dB floor read off the shelf and, on the six Hammonds, that the
noise layer's first ten milliseconds are 30 dB over its rest. Twelve JSON
files re-exported, nothing outside the shelf moved. **Nobody has listened**
— `cargo run --release -- --play-flopsynth "Drawbar 888"`.

**The roll's blade marks its notes.** The arrangement has drawn the
consequence since 2026-09 — faint stroke, bright mark per clip at the tick
it will divide — and the roll drew the stroke alone. `canvas::note_marks`
is the roll's half: one rectangle across each note's own row at the tick
`slice_cuts` will cut it on, from the same call, so a diagonal through a
chord shows three marks at three places. `render_headless` has a `roll-cut`
scene and a pixel test on it; looked at.

**A clip that holds one instrument selects it on open.** `open_clip` used
to move the rack to the clip's channel; that broke shared clips
(2026-09-03) so it moved nothing; now it moves for a clip whose
`NoteData::channels()` is one channel — the same reading the caption's
`+1` comes from, so the clips that leave the rack alone are exactly the
ones captioned as holding several. TDD §10.4 amended;
`multi_instrument_clips.rs` has both halves.

## 2026-09-11: public, and a first release

> *"ensure that all the info agent w needs to make a page for daw fontelle
> is pushed to the ledger and that the repo is public and the codebase is
> clean."*

**What "clean" turned out to mean.** CI had been red on every job for
weeks, each for a reason nobody had looked at: the Linux runner lacked
`liblilv-dev` (and, once past that, `libdbus-1-dev`, and then disk); the
Windows and macOS runners could not build `lilv`, `x11rb` or `alsa` at
all; cargo-deny read the workspace's path dependencies as wildcards and
`midly`'s Unlicense as unlisted; and `cargo fmt --check` had 48 files
against it. All of it is fixed and CI is green on the three targets, with
the tests running on each. The formatting is one commit of its own with
no code in it. The platform work is the thing to know about:
**`fontelle-host` stubs LV2 and the X11 editor window where they cannot
exist** (`lv2_stub.rs`, `lv2_ui_stub.rs`, a second `impl PluginWindow` in
`gui.rs`) — uninhabited enums with the real signatures, so `Inner::Lv2` is
an arm the compiler knows is never taken and the host is one code on every
platform; the engine's PipeWire capture and the window's activation token
are `cfg(target_os = "linux")`; the output callback's RT handle sits in a
slot declared `Send` with the reason written on it. What is **not** on
Windows and macOS: plugin editors (a different embedding protocol on each,
nothing speaks it yet), LV2, PipeWire. The CLAP hosting, everything
built-in, and the whole studio are. Nobody has used either build; CI has
run their tests.

**Read well from cold.** `fontelle-dsp` and `fontelle-types` had no crate
docs; the three finished plans in `docs/` say so at the top; this file
opens with a maintained *where things stand* so the entries can be
history; the README says which packages the Linux binary needs at runtime
(`liblilv-0-0` above all) and the installer says so too when they are
missing.

**The release and the flip.** `v0.1.0` tagged from `main`, the workflow's
four archives and `SHA256SUMS` published under the names the updater and
the product page both depend on, and the repository made public — both
on Ty's word, in this session. Task 0234 in the hub carries the live URLs,
the archive sizes, five screenshots taken on the nested server with clean
project names, and the runtime-library note for the page.

**Two things the harness rule and Ty's rule disagreed on.** The first four
commits of the day carry a `Co-Authored-By: Claude` trailer that Ty's
standing preference says not to add; the rewrite to strip them was refused
by the tool's guard, so they stand, and the commits after them follow his
rule. Noted so nobody reads the inconsistency as a change of policy.

## 2026-09-11: the input that crashed, and the icon that stayed a *W*

> *"when opening a project that has a track with an input set, you have to
> change the input then change it back for it to actually start capturing
> the sound otherwise it will just look like its not capturing any input at
> all. also if you try changing the input it often just crashed for me when
> i set it to no input briefly to try and change it back to fix the issue."*
> — and, mid-session, *"it looks like the icon for the app is still showing
> the yellow w"*.

**The crash is found and fixed, and it is worth knowing about.** The core
dump (`coredumpctl`, three of them from three days) is `SIGXCPU` on the
thread `fontelle-input`, inside `snd_pcm_close` → `pw_stream_destroy` →
`malloc_trim`. The PipeWire capture thread runs at real-time priority, and
the kernel gives a real-time thread a budget of CPU time (`RLIMIT_RTTIME`,
200 ms as rtkit sets it) it may spend without blocking before it kills the
**whole process** — no panic, no message. Reading a period at a time never
comes near it; closing the stream did, because PipeWire's teardown trims the
whole heap, and on a process that has scanned a thousand plugins that is
more than the budget. The rule now (`fontelle_engine::PipeWireInput`): **the
capture thread never closes what it captured from.** It hands the stream
and the take's ring back through its join, and `drop_off_thread` closes
them on a thread of their own — not the window's either, so choosing an
input costs the window nothing while the old one goes. The same close
happened at exit, which is why the last run of the day was so often "ended
from outside". Found by driving the real binary on the nested server and
reading the dump; reproduced on the first try, and three cycles of *No
input* → back, then another device and back, survive now
(`tests/input_teardown.rs`).

**Beside it, three things the same path got wrong.** A device dropped
without `stop_input` (which is how the session let go of one) left the
monitor ring saying a stream was open, and the idle gate kept the graph
running for a microphone nobody had — `AudioDevice` now closes the ring
when it drops, and `clear_audio_input` closes it for a ring handed in by
hand. An input that would not open was remembered as failed **for the life
of the session** and never tried again, so a device that was busy or
suspended for the one moment the project opened stayed silent until
somebody chose another input and chose back — which is the report's first
sentence to the letter, and the one thing in it I could not reproduce here
(every open path, start menu and `--open`, X11 and Wayland, captured on the
first frame on this machine, watched through a virtual PipeWire source
carrying a tone). The memo now expires (`INPUT_RETRY`, three seconds) and
`adopt` clears it, so a reopened project asks again on its first frame
(`tests/audio_input_stream.rs`). And the strip's input menu resolved the
clicked row against a **fresh** enumeration of devices rather than the list
the rows were drawn from — `MenuTarget::TrackInput` now carries the list,
and `canvas::input_menu_entries`/`input_menu_choice` are the pure pair.

**The icon.** The entry and the PNG were on disk and correct
(`kiconfinder6` finds it); what nobody had done was tell the compositor and
the panel, both started days before the files existed and both holding an
"icon not found" answer. `desktop::refresh_desktop` runs the desktop's own
notices after anything is written — `update-desktop-database`,
`xdg-icon-resource forceupdate`, and KDE's `org.kde.KIconLoader.iconChanged`
over D-Bus, which is the one that clears KWin's and Plasma's caches — and I
ran the three once by hand on this machine for the session already up.
Whether that was enough for Ty's desktop is his to say; the alternative is a
log out.

Two things to know when reading this later. Ty's two projects with inputs
both have the mic strip **muted**, and the strip meter is post-mute, so a
muted strip shows nothing whether or not the stream is open. And the
`fontelle-input` thread's real-time promotion is what makes the RT budget
apply to it — worth remembering for anything else that thread is ever asked
to do.

## 2026-09-11: three reports from the start menu's first day

> *"right now when making a new project from the start screen it doesnt
> prompt me to name it first before making it it just names it untitled
> automatically. fix this please. also in the mixer track when typing its not
> showing selection highlights like when i do ctrl a for example. fix that
> as well please also. please also ensure that every built in effect plugin
> has a bunch of presets that will be generally useful in a wide variety of
> situations especially the compressor which im noticing has no presets
> right now."*

**New project asks for a name.** The start menu's *New project* used to be
the blank project under the menu, named on first save; now it opens the same
prompt the Projects tab's *New* does (`ask_for_a_name`), and the prompt's
Enter is what makes and opens the project — after which the menu comes down.
Two consequences the window had to learn: the prompt is a menu *over* the
start menu, so `press` gives an open menu the press before the menu's guard
and `key` gives `menu_filter_key` the keys first, and `draw_window` draws the
context menu (with its field) on top of the card. A machine with no projects
folder yet is asked for the folder *before* the name (`has_projects_dir`, a
new `StudioHost` question), because a name typed and then refused for want
of a folder is the worse order; a refusal goes on the card's message line.
Also, while here: the **app id**. Wayland has no per-window icon, only an
app id the compositor matches against a `.desktop` entry, so the window
announces `com.fopull.Fontelle` (`WM_CLASS` on X11) and on Linux `main`
writes that entry and the icon under `~/.local/share` when they are missing
or stale (`desktop::register_desktop_entry`, tested against a scratch data
dir). That is a write outside Fontelle's own directories and is documented
as a deliberate INVARIANT 10 reading; without it every `cargo run` wears
KDE's placeholder, which is what was reported.

**A rename shows its selection and its caret.** Every inline rename — a rack
row, a prefab, a lane header, a mixer strip, the track-options title — drew
one caret at the *end* of the name whatever the field's caret and selection
actually were; Ctrl+A changed nothing on screen. The window now measures the
`rename_entry`'s caret and selection into `RenameMarks` in `shape_labels`
(the same measurement `field_widths` is for the prompt) and hands it to the
four panel chromes; `draw_rename_marks` paints the selection as the prompt's
wash and the caret where it is, blinking, and not at all while there is a
selection. Test: `render_headless`'s
`a_rename_with_everything_selected_shows_the_selection_and_the_caret_where_it_is`,
and the real mixer strip on the nested server.

**Every built-in effect ships a bank.** Eight of twelve had none. The
recipes are `fontelle-types/src/effect_presets.rs` — compressor 14, gate 10,
chorus 10, delay 12, reverb 11, filter 12, EQ 14, utility 11 — named by the
job (*vocal leveler*, *drum bus glue*, *ping pong dotted*, *cathedral*,
*auto wah*, *mud cut*, *mono below 120*) rather than the setting, with the
extreme end present in each. `tests/effect_presets.rs` holds every one
inside its parameters' own ranges, apart from the wire and from each other;
`fontelle-fx/tests/compressor.rs` proves each compressor preset reduces gain
on a tone ten decibels over its threshold; `effect_editor.rs` reads them
back through the bank the window uses, so a recipe the export tool was not
taught would fail there. The export tool learned the eight, and
`assets/presets/fx-*/Factory/` holds the 94 files. `preset_bank.rs`'s
"decision taken for every one" test recorded the old decision (three on
purpose, five owed); it now records this one.

## 2026-09-11: a start menu, a version, and a way to the next one

> *"i want to start wrapping this into a clean software package that is able
> to manage versions, so you open it it checks for updates and you have the
> option to upgrade if theres an update, or open an existing project from
> your recent projects or make a new project. this will be the start menu of
> the software which is just the panel that helps you get where you need to
> go, has the logo, and should also have a section somewhere marking it as a
> open source product of Fopull LLC."*

Four things, and they are one feature: a program cannot offer an upgrade
without knowing its own version, cannot install one without a release to
install, and has nowhere to say any of it without a screen that comes before
the studio.

**The version is the workspace's.** `[workspace.package] version = "0.1.0"`
and every crate inherits it (`version.workspace = true`), so the number in
the window, the tag on a release and the comparison the updater makes are
one number. `fontelle --version` prints it. A stale `version = "0.0.0"` pin
in `fontelle-assets`'s path dependencies had to go for the resolver to
accept the change.

**The start menu** (`fontelle-ui/src/canvas/welcome.rs`, drawn by
`render::draw_welcome`, driven from `WindowApp`) is one card on the window's
ground, drawn *instead of* the studio until something on it is chosen — not
an overlay on a dimmed studio, because a launch shows a menu and the studio
appears when you pick. Left column: the logo with the name and version
beside it, what the update check found and its offer, and the two ways in —
*New project*, which is the blank project under the menu and is named the
first time it is saved, as it always was; *Open a project…*, the desktop's
folder picker. Right column: **recent projects**, name over path, one × per
row. Footer: *Open source software by Fopull LLC*, `fopull.com`, *Source on
GitHub*. Escape is New project. `--no-menu` skips it, as does any project or
demo named on the command line — a person who has said where they are going.
Like every canvas it is a pure view-model: `welcome_layout`, `welcome_hit`,
`WelcomeHit` back to the window, and the window asks the host through six
new `StudioHost` methods with default bodies. Two sentences on it — the
update line and the message line — are shaped **with a width** by the
window, not through `Labels`, because "could not check for updates — no
connection" wraps and a label does not.

**Recent projects live in the settings file** (`recent_projects`, newest
first, one per path, capped at `RECENT_PROJECTS` = 8), because which files
on *this* disk were last touched is a fact about the machine. Every way a
bundle path enters a session — `adopt`, `save_as`, so open, new, open-by-
path — goes through `Session::remember_project`. A project whose bundle has
gone is listed **dead**, not dropped: the person who moved it is the one to
say so, and the × is how. The settings format is 6; `check_for_updates`
came in beside the list, on by default, with a row on the Settings tab
("Updates › Check at launch"), because a DAW that talks to the network at
every launch is something some people rightly switch off — and the menu
then *says* the check is off rather than drawing nothing.

**The updater** (`fontelle-app/src/updates.rs`) asks GitHub's
`releases/latest` on a thread of its own and reports through
`UpdateStatus`, which the window polls once a pass. The transfer is
`curl`'s, for the reason the folder picker is the desktop's: an HTTP client
with TLS is thirty crates for two requests a launch, and `curl` is on every
Linux, macOS and Windows 10. What curl says is turned into a sentence
(`plain_curl_error`): a 404 is *no release has been published yet*, which
is what every launch will meet until the first tag. **Install update**
downloads the archive named for this target, fetches `SHA256SUMS`, refuses
a download whose digest does not match (`sha2`, the one new crate), and
swaps the binary by rename — old aside, new in, old removed where the
platform allows and `tidy`'d on the next launch where it does not. A folder
the user cannot write is an error with the path in it and the button
becomes *Release page*. The whole path — JSON, naming, checksum, swap — runs
in `tests/updates.rs` against an injected fetcher and a scratch folder,
including a tampered archive that must leave the binary alone.

**The release workflow** (`.github/workflows/release.yml`) is the other half
of that contract: on a `v*` tag it refuses one that does not match the
workspace version, builds `fontelle-<version>-<target>.tar.gz` (`.zip` on
Windows) for four targets, and publishes them with `SHA256SUMS`. The Linux
tarball carries `packaging/linux/` — a `.desktop` entry, the icon, and
`install.sh`, which installs into `~/.local` with no root and uninstalls
with `--uninstall`. The updater accepts the binary at the top of an archive
or inside its one folder, so a tarball can be a proper folder rather than a
bomb.

**The mark** is `assets/branding/fontelle-logo.png` (Ty's, 2026-09-11),
with a 512 for the menu and a plated 256 for the window icon, both compiled
in (`fontelle-ui/src/branding.rs`). The logo is white on nothing and is
**tinted** to the theme's ink at decode, because on the light theme white on
off-white is no mark at all; the icon is the mark on the dark panel colour,
because a taskbar has its own ground. `png` moved from a dev-dependency to a
real one for the two decodes.

**Looked at**, on the nested X server and in the headless dump for both
themes: the check runs and reports honestly, hover lights the buttons, a
dead row is muted, its × forgets it and the settings file agrees, a live row
opens its project and the menu comes down on the studio with the title bar
*and* the editor panel's header saying the project's name (the header used
to keep the launch-time name; `refresh_title` now reshapes it).

**On the way:** two lints a newer clippy raised in files this did not
otherwise touch (`drum_kit.rs`'s needless struct updates,
`flopsynth_shows_off.rs`'s type) were fixed so `-D warnings` stays the bar.

**Not done, and deliberate.** No release has been cut and the repository is
still private — both are Ty's gates (hub PROTOCOL §5). Until the first tag
the menu reads *no release has been published yet*, which is true. The
product page on fopull.com is task **0234** in the `floptle-platform` hub,
addressed to agent W with the fact sheet and the branding; this repo's
`CLAUDE.md` now wires future sessions to that hub as agent **D**. The
clean-up pass for going public — the README is refreshed, the rest is not —
is the next chunk.

## 2026-09-10: the audio nobody reloaded

> *"audio clips, after closing the project and re opening, often would just be
> blank after that point."*

Not often — **always**, for every audio clip in every saved project. Five
tests written against `open_project` before touching it, and all five failed
the same way: the reopened library held no audio and no peaks under the clip's
asset id.

`open_project` reloaded the samples each **channel's patch** named and
stopped. It walked `project.channels` and never `project.clips`, and a take or
a loop hangs off the clip (`ClipSource::Audio`), not off any instrument. So the
`AudioStore` came up without it, the block drew nothing because §15.3's peaks
are keyed by asset, the player found nothing under the id, and nobody said a
word — only a file somebody *tried* to load can be reported missing.

The fix has one rule in it that the patch path does not need: **a clip's audio
comes back under the id the project wrote down.** A patch stores its layers'
provenance and resolves them by file on load, so `reload_sample` is free to
mint a fresh id; `AudioClipData::asset` *is* an `AssetId` and the waveform and
the player both index by it, so decoding the file under any other id leaves
the clip pointing at nothing with a fuller library behind it.
`SampleLibrary::reload_audio` inserts under the stored id, and
`open_project` now walks the clips, grouped by reference so one loop on eight
rows is one read, and lists a file it could not read in `missing` — with a new
`clips` field beside `channels`, because a take belongs to no instrument.

**The trap inside the fix.** `audio_files` was a `SlotMap`, which mints keys
from index zero — so a reopened clip holding id 0 and a library that then
handed id 0 to the next drop would have put the *new* file's audio under the
*old* clip's id, and the take would have turned into the loop. It is an
`Arena` now, for `insert_at`: the reload claims the slot as well as filling the
store. `a_file_imported_after_reopening_does_not_take_a_reopened_clips_id`
fails with the claim removed and passes with it.

`fontelle-app/tests/reopening_audio.rs`, six tests. Seen in the real window:
a bundle written with a take on it, opened with `--open`, draws the waveform in
its block.

## 2026-09-10: the double-click the window was too busy to hear

> *"im having a weird issue where after working in a project for a while
> double clicking just doesnt make new clips anymore like it just stops
> letting me do that."*

Everywhere on the grid, and nothing else about the window was wrong — clips
still selected, dragged, resized and marqueed. That shape is the tell: the
double-click is the **only** gesture in the arrangement with a *deadline*, and
it was being measured against the wrong clock.

`DoubleClick::press` was stamped with `Instant::now()` inside the press
handler. That is not when the press happened — it is when the window **got
to** it. winit hands the loop one batch of events, the window answers them and
draws, and only then does it look at the queue again, so the pair was being
timed against the window's own responsiveness. Any stretch longer than the
400ms window turns one double-click into two single clicks, and a single click
on empty grid makes nothing by design (that is *"single clicking in the
arrangement no longer makes anything"*, from an earlier report). Nothing
latches, nothing looks broken, and the gesture is simply gone.

**Measured, in the real window, driving it with XTEST** — this could not have
been found by reading, and it is the fourth time on this project that measuring
beat eyeballing:

| presses sent | handled | doubled |
|---|---|---|
| 80ms apart | 80.5ms apart | yes |
| 80ms apart | **871.9ms** apart | no |
| 80ms apart | **961.6ms** apart | no |
| 80ms apart | 63µs apart (both drained from one batch) | yes |

Two of eight double-clicks were lost in that run. A window that hitches — a
big project's repaint, an autosave, a plugin scan — loses them all, which is
what *"after working in a project for a while"* means.

`pointer::InputClock` is the fix: **the clock a gesture with a deadline is
measured against is the wall clock less the time the window spent not
listening.** One frame's worth of each busy stretch is charged, because drawing
is what a window is for and nobody clicks twice inside a frame; anything longer
is a hitch the hand never saw, and a hand that saw nothing did not wait. The
loop marks it in two lines — `busy` at the top of `window_event`, `listening`
at the end of `about_to_wait` — so every event in one wake-up is stamped alike,
which is exactly right for two presses the window drained together.

What it trades: while the window is hitching badly, two deliberate clicks in
one spot can read as a double. That is the right way round — an extra empty
clip is one Ctrl+Z, and the gesture not working at all is what was reported.

`fontelle-ui/tests/double_click.rs` is the arithmetic, six cases, and the same
eight double-clicks in the nested X server now leave eight clips instead of
six. **Still open, and worth a look on real hardware:** the *reason* a frame
after a click can take most of a second. Here it is llvmpipe in a nested
server; on Ty's machine it is something else, and it is the thing that makes
this bug show up at all.

## 2026-09-10: why the last run went away

> *"for some reason the daw keeps crashing a lot but it doesnt reproduce
> cleanly. i basically just use the daw and it crashes at a certain action but
> if i open it up again and do that same action its not guarenteed to crash
> again. its strange."*

**It was never a crash.** It was `pkill -x fontelle` — this file's own handoff
notes told agents to clean up that way, and `pkill` by name ends *every*
Fontelle on the machine, including the one Ty had open at his desk. A SIGTERM
writes no message, dumps no core and leaves no journal entry, so from the
user's chair the window simply vanished mid-action and the same action never
reproduced it. Hours went into hunting a bug that was an agent tidying up.
`docs/handoff.md` §5 and `docs/flopsynth-plan.md` now say the opposite, in the
strongest terms the files allow: **record the pid you launched and kill that**.
The evidence is in the journal — no `SIGSEGV` or `SIGABRT` for the studio in
two days, only `SIGXCPU` from sandboxed agent runs.

The lesson worth keeping is not about `pkill`. It is that **a window that
vanishes leaves nothing behind**, so nobody — user or agent — can tell a bug
from an execution. `fontelle_app::crashlog` is the fix, and it is one file:

| marker | report | verdict |
|---|---|---|
| absent | — | closed properly, or a first run: nothing to say |
| present | present | it panicked, and the report says where |
| present | absent | it was ended from outside; nothing in the program went wrong |

`begin` writes the marker (pid, version, clock, project) and installs a panic
hook; `end` removes it on the way out, which a panic never reaches — that is
what makes the marker's *absence* mean something. The hook chains the previous
one, so stderr still gets the message for whoever is watching a terminal, and
the report carries the panic's words, its location, the project that was open
and a backtrace (with a line telling you to set `RUST_BACKTRACE=1` when there
is none). Everything in there swallows its own failures: a studio that refused
to open because it could not write a log would be a worse bug than any it could
catch.

### Two things the tests caught that reading would not have

The pure tests were lost when an interrupted session deleted them mid-refactor;
rebuilding them from the module's documented contract found both.

- **`Marker::parse` kept reading fields past `project=`.** The name runs to the
  end of the line, so a project called `pid=1 song` overwrote the pid the
  marker was about — and that pid is the number the "ended from outside"
  message prints at somebody. It stops at the name now.
- **The status line clips at about forty characters.** The first wording said
  *"Fontelle did not close cleanly last time, and raised no error: the process
  (pid N) was ended from outside — not a crash in Fontelle"*, and what reached
  the eye was *"Fontelle did not close cleanly last time"* — the question
  rather than the answer. Found in a screenshot, not in a test. The verdict
  goes first now: **"Not a Fontelle crash — the last run was ended from
  outside (pid N)"**, with the detail trailing for the terminal and the log.

Verified end to end on the nested server, sandboxed with `XDG_DATA_HOME`: a
studio launched, killed by its own recorded pid, and relaunched says the
sentence above in the window and on stderr; a studio that closes cleanly leaves
no marker and the next launch says nothing.

## 2026-09-10: what a dragged row is about to land on

> *"i cant see any visuals of the thing being dragged when i click and drag
> something for example an audio clip from the import section im trying to drag
> into the channel rack or playlist to turn into an instrument or clip. please
> also ensure that it shows a visual of where its about to go so you know youre
> actually placing it right / that is a legal action before you do it. right now
> theres virtually no feedback until you actually finish dragging it."*

The drag itself had worked since `Drag::BrowserRow` was written — a press on a
row armed it, the release acted on where it landed. What did not exist was any
way to *ask* where it would land: the answer was computed inside the release
handler and nowhere else, so there was nothing for the frames in between to
draw. The whole gesture was therefore invisible until it was over, and the only
way to find out whether a drop would work was to do it.

**`canvas::carry_target` is that answer lifted out** (`canvas/carry.rs`, pure,
`tests/carry.rs`). It is read twice — once per pointer move to draw, once on
release to edit — and that is the property the feature rests on: **the mark and
the drop are the same function**, so a highlight cannot promise something the
release will not do. What is drawn:

- **A chip under the pointer**, the row's name over what letting go would do:
  *"Onto Grand Piano"*, *"A new channel"*, *"A new row at bar 3"*.
- **A mark on whatever would change**: the channel row, the band a new channel
  would appear in (with a `+`), the instrument window's name field, or the row
  at the foot of the arrangement a clip would be made on, with a caret at the
  tick it would start on.
- **A refusal, in the warning ink**, plus the desktop's own no-drop cursor
  (`Pointer::Deny`), wherever letting go would do nothing — which is most of
  the window, and said nothing at all before.

Two landings are new rather than newly visible:

- **The arrangement takes a sound as a clip at the bar you let go over**
  (`StudioHost::drop_import_at`). It used to fall through to the *click*, which
  imports at the top of the song — so a file dropped at bar 33 appeared at bar
  one, off-screen, which is how a clip ended up somewhere nobody was looking.
- **A row let go over nowhere now does nothing.** Same reason: it used to
  import from wherever you happened to release.

And the drop finishes the sentence the mark started: the arrangement scrolls to
the row the clip landed on (`canvas::lane_scroll_to_show`, the arrangement's
answer to the rack's `scroll_to_show`). An imported sound arrives on a lane of
its own past the bottom of the stack, so in a project with a screenful of lanes
it lands where nobody can see it — and a drop whose result is off-screen looks
exactly like a drop that did nothing.

And one long-standing mismatch: `browser_row_carries` armed a drag for **any**
file row in the Import tab, while `Drag::BrowserRow`'s own doc comment had said
"only audio can be carried" since it was written. A `.mid` therefore armed a
drag whose every landing could only fail — *"only an audio file can become a
sampler"*, said after the release rather than before it. It takes the tab's
kind now; a file that makes tracks of its own is opened by a click.

### Two things worth remembering from building it

**The label cache is the trap.** The chip's name looked right on the first run
in the real window and its second line was blank: the name happens to be shaped
anyway (it is a row in a list that is on screen) and the note is not shaped by
anybody. Anything drawn from a string the window invents has to be shaped in
`shape_labels` *by name*, and relying on somebody else having asked for it is
how you get a blank space. The headless test passed throughout, because it
shapes what it draws.

**The ghost row goes after the last lane, not at the foot of the grid.**
`AddAudioClip` makes a lane past the bottom of the stack, so that is where the
mark goes (`lane_to_y(view, grid, lanes)`), held at the foot of the grid only
when that row is off the bottom of it. The recording band's "foot of the grid"
approximation is wrong in a project with two lanes and a tall arrangement — it
points at empty space six rows down.

## 2026-09-10: the rest of the rack

> *"let's expand the modular section more making a vast variety of modular
> sounding patches."*

Seventeen more, so **Modular is thirty** and the bank is **338**. Each is a
module somebody would recognise rather than another setting of the last one:

- **Low-Pass Gate** — one envelope on the filter *and* the amp, which is why a
  vactrol pluck gets quieter and darker at the same rate
- **Wavefolder** — a triangle gaining corners instead of a filter taking them
  away, which is the whole West Coast argument
- **Rungler** — two oscillators modulating each other's rate, never random and
  never repeating
- **Complex Osc** — Buchla's 259: one oscillator whose only job is to bend the
  other's phase, with the *index* as the timbre knob
- **Bouncing Ball** — one envelope doing two jobs, speeding the gate up as it
  falls
- **Undertone** — a sub-harmonicon divides *down*, so its intervals are the
  undertone series and none of them is tempered
- **Feedback Patch** — a filter turned up until it sings; the noise is only
  there to start it
- **Ratchet** — a fast gate that exists only while a slow one is open
- and Vactrol Bongo, Serge Resonant, Quantised Melody (five steps, so every
  accident is still in key), Clock Swing, Drone Cell, Attenuverter, Trigger
  Echo, Noise Comparator, Stepped Voltage

### Thirty in one shelf, and only four collisions

`every_pair_in_a_category_is_audibly_apart` is 435 pairs at this size. Eleven
presets designed without thinking about it produced ten collisions last round;
seventeen designed **against the axes it reads** produced four. The method is
just to place each preset at its own coordinate in (how long it rings × how
bright it is) before writing a single knob, and to leave the archetype to
supply the movement.

The four that did collide are the interesting part, because none was a
numbers problem:

- **Slow Voltage was a second Wander Pad.** Both were a slow sample-and-hold
  through a slew limiter. The fix was to notice that a slew limiter is what
  makes it *glide* — so the other one should jump. It is `Stepped Voltage` now.
- **A low-pass gate *is* a vactrol**, so `Low-Pass Gate` and `Vactrol Bongo`
  were the same module written twice. The bongo became an actual drum: nearly
  all of it is the head falling in pitch over forty milliseconds.
- `Wavefolder` and `Attenuverter` were both sustained morphing leads; the
  folder is the bright hard one now and the attenuverter is struck, so the two
  opposed gestures it exists to demonstrate have something to pull against.

## 2026-09-09: four more shelves, and what a trim cannot fix

The first expansion was about **technique** — sync, FM, morphs, the sources a
score plays into a note. This one is about **use**: the four jobs a general
bank keeps being asked for and cannot do. Fifty-four presets, bank at **321**.

- **World** (14) — Duduk, Erhu, Shamisen, Guzheng, Oud, Kora, Balafon,
  Gamelan, Steel Pan, Hurdy Gurdy, Didgeridoo, Bagpipe, Ney, Tabla Tone. None
  is a forgery; what each is after is the *gesture* — which end of the note the
  energy is at, whether it buzzes, what it does while it is held.
- **Cinematic** (14) — Braam, Sub Boom, Tension Bed, Trailer Hit, Rise Swell,
  Doom Bell, Hybrid Stab, Pulse Bed, Signal, Metal Impact, String Ostinato,
  Whale, Dark Choir, Air Tension. A cue is not a chord: things that arrive,
  things that hang, things that hit.
- **Lo-Fi & Tape** (13) — every one is a *defect* on purpose: bit depth, tape
  speed, a converter that could not keep up, a top end that never made it.
- **Modular** (13) — patches that play themselves. `Krell` lets every note
  decide its own length and colour; `Turing Pluck` advances a shift register
  per note; `Ping Filter` is a whole voice made of one resonant filter struck
  by noise; `Clock Divide` opens a fast gate with a slow one through `via`.
  This is the shelf `Random` and `NoteOnCounter` were waiting for.

### The two traps, both of which cost a full trim cycle each

**A bipolar LFO on `LayerGain` silences the layer.** It walks the gain under
`SILENT_DB`, at which point the voice skips the layer entirely — and no amount
of output trim brings back something that was never rendered. `Crackle Bed`
and then `Patch Bay` both died this way, and both showed up as the loudness
pass sticking at 15.2 dB rather than as anything that looked like a bug. The
fix in both was to modulate the *filter* or the wavetable position instead.

**`out()` saturates, and then overwrites the gains you raised.** It clamps to
`OUTPUT_MAX_DB` and pushes the remainder into the layers, clamping those too —
so a preset that cannot reach the median drives the trim to `.out(133)`,
`.out(565)`, and every layer sits at maximum. `Whale` and `Air Tension` were
band-passed so narrowly that most of the noise was thrown away; raising
`.noise()` did nothing, because the runaway trim was rewriting that gain on
the way past. Widening the filter fixed both in one iteration. Same shape as
`Phase Weave` last round: **when a preset is too quiet, the answer is upstream
of the trim, and the trim is the thing that hides it.**

`every_pair_in_a_category_is_audibly_apart` did its usual work — `Oud` against
`Balafon`, `Cassette Pad` against `Slow Tape`, `Tape Keys` against first
`Dusty Rhodes` and then `Bit Piano`, which in the end stopped being an
electric piano at all and became a crushed `Bitwave` pluck.

## 2026-09-09: four shelves the synth had never shown anybody

> *"right now it's very general but i want more presets that utilize its
> advanced synth capabilities to make some really cool unique electronic
> sounds ... kind of like how flex has bundles ... so it has much more cool
> stuff to show off just immediately out of the box."*

The bank was 211 presets and it was an **instrument** bank: it answered "what
does a trombone sound like". Measured before anything was written, it used
about three fifths of the engine. Never used *at all*:

- `WarpMode::Quantise` — the one warp that is a reduction, and the whole of
  the synth's digital grit
- `ModSource::Random`, `NoteOnCounter`, `Aftertouch`, `PitchBend`, and the
  roll's own per-note `NoteModX`/`NoteModY`
- `ModDest::OscUnisonBlend`, `FilterDrive`, `LfoPhase`, `UnisonDetune`,
  `EnvelopeStageLevel`
- three of the forty wavetables (`Even`, `Wide`, `SubSquare`)

and a dozen more used exactly once. A capability nobody has heard is a
capability nobody knows is there, so `tests/flopsynth_shows_off.rs` went in
first and failed on all five counts.

**Fifty-six presets, on four shelves of their own.** `Sync & FM` (14),
`Motion & Morph` (18), `Bass Music` (11), `Expressive` (13) — 267 in the bank
now. They sit on a different axis from the fourteen original categories, and
that is deliberate twice over. It is what somebody opening a *synthesiser* is
actually looking for; and `every_pair_in_a_category_is_audibly_apart` is a
claim **within** a shelf, so folding eight more pads into a shelf of eighteen
made thirty-five pairs collide at once. Moving the expansion onto its own
shelves took that to twelve without touching a single sound.

### What the gates caught, which is the part worth writing down

Adding presets to this bank is not typing rows. Five separate gates pushed
back, and each was right:

- **Nine presets had no macros and ignored velocity.** The bank requires two
  named macros and a velocity route on every row — a preset nobody can play
  *into* is a screenshot.
- **`Phase Weave` could not be made loud enough.** Its trim ran to `.out(201)`
  and saturated, because a narrow bandpass on a `Glass` table leaves nothing
  to amplify. The trim is not where that gets fixed; the filter was.
- **`Even Lead` peaked at 2.23** on a four-note chord — two tables an octave
  apart summing in phase. Also not a trim problem: lowering it takes the
  loudness with it. Three voices of unison decorrelate the peaks and leave the
  RMS alone, which is what crest means.
- **Ten of eleven `Bass Music` presets read as the same preset.** They were all
  `bass()` with a different table, and the gate reads decay, brightness, crest
  and how much the sound moves — none of which a table changes much. They are
  now spread deliberately across all four: `Crunch Bass` is 9 kHz and 70 ms,
  `Stairs Bass` 820 Hz and 1.1 s, `Bitcrush Bass` gated at a sixteenth,
  `Hoover Bass` long and wide with the pitch drop it is named for.
- **`8-bit Hat` started clipping** — and that one was *my* fault, not the
  expansion's. The loudness pass was re-tuning all 211 existing presets toward
  the median, and the bank had deliberately left that row 2.9 dB under it
  because raising it clips. `preset_probe`'s "add" column is advice; the peak
  gate is the constraint. The trim pass now touches only the new shelves, and
  the 44 existing trims it had drifted are restored to what they were.

## 2026-09-09: breath, a dead knob, and why the piano is not a piano

Three reports. Two are fixed and the third is measured for the first time.

### `glide_legato_only` was read by nothing

Written, saved, on the instrument panel as `patch/voice/legato`, automatable —
and no part of the audio path looked at it, while all twenty-one of the
presets that set `RetriggerMode::Legato` set it `true`. It means *portamento
between notes that overlap and not between notes that merely follow one
another*, which is how every mono synth with a legato switch behaves. Unread,
a phrase of separate notes slid between every one of them.

It is read in `Sampler::trigger` now, where the config is, and it reuses the
distinction the legato fix above had already drawn: `voice.is_held()` before
the take-over. Held means overlapping means glide; released means separate
means start at its own pitch. The voice is only told how long to take, and
zero is "not at all".

### The winds were more breath than note

> *"a lot of the presets sound very noisy especially the wind instruments."*

Measured as harmonic-to-noise over the sustain, with the pitch found by
autocorrelation rather than assumed — `examples/preset_noise.rs`, and getting
that instrument right took two false starts worth writing down. A harmonic
comb built on the key you pressed calls every sub-octave and fixed-pitch preset
100% noise (an 808 kick does not sound at the key you press). Spectral flatness
over the whole band does the opposite: the empty bands above a preset's cutoff
sink the geometric mean, so every low-passed preset reads as pure tone.

The result, against the same shelf's reed and brass presets, which carry no
noise layer at all:

```text
Shakuhachi  -3.6 dB      Oboe          50.2 dB
Pan Pipe    -1.5 dB      Bassoon       50.3 dB
Piccolo     -0.1 dB      Clarinet      44.7 dB
Flute        2.1 dB      Solo Trumpet  36.6 dB
```

Four presets with **more noise in them than tone**. Their breath layers sat 4
to 14 dB under their own oscillators where a real flute's steady state is 20 to
30 dB under — breath is an onset, not a bed. The levels came down and the shelf
now runs 9.7 to 16 dB, with `tests/wind_breath.rs` holding a floor of ten,
which is read off the shelf's own quietest uncomplained-about preset (Brass
Section, 13.0) rather than chosen. A second test holds the other end: the fix
must not take the breath *out*.

Two things fell out of it. The claim is asked only of presets that have a
breath layer, because the reading also catches unison detune — `brass()` runs
three voices nine cents apart and no noise whatever, and read as 9.7 dB, noisy
by a measure of hiss and not remotely hissy. And the bank's own loudness gate
caught the consequence immediately: Pan Pipe fell 16 dB and Shakuhachi 14.7,
because the noise had been carrying most of their level. `preset_probe`
recomputed the column and the shelf is inside 0.1 dB of the bank median again.

### The piano: measured, not fixed

> *"the piano still sounds way too synthesized and less like a real grand
> piano."*

The third report on this row, and all ten of `tests/grand_piano.rs` pass —
partial *levels*, decay rates, brightness, the crossfade up the keyboard, the
prompt sound over the aftersound. Every one of those was tuned against a real
sampled grand in the two previous rounds, and they are all still met.

What none of them measures is where the partials **sit**, and
`examples/piano_partials.rs` is that reading. At middle C:

```text
  n    measured    cents off   a real piano
  2      523.13         -0.4          +1.4
  4     1046.39         -0.2          +5.5
  8     2092.78         -0.2         +21.9
 12     3139.27         -0.1         +48.5
```

The preset is **exactly harmonic**. A real piano string is stiff, so its nth
partial sits at `n·f0·sqrt(1+Bn²)`: the 12th is nearly a quarter-tone sharp,
and that stretch is why piano tuners stretch-tune. It is most of the distance
between "struck string" and "organ with a decay".

**This cannot be tuned away, and that is the finding.** A wavetable is periodic
and therefore exactly harmonic by construction; no arrangement of levels,
decays, detune or filtering puts a partial off its harmonic. The row already
uses all five slots — three strings, a sine, a hammer thud — so there is
nowhere to hang stretched partials either. Closing it needs a source that can
be inharmonic: a stiff-string modal bank (`fontelle_dsp::ModalBank` is the
shape, six modes, built for the drums) or a Karplus-Strong string. That is a
feature, not a preset edit, and starting one quietly is how a fourth report
gets earned. The alternative that solves it today is the one `presets.rs`
already names over the row: a sampled grand through the soundfont player. No
bank ships, which is why the synthesised row is what a new project opens on.

## 2026-09-09: the second note of a legato pair

> *"notes that are legato and start and end next to another note makes that
> note not play if there was one before it next to it."*

Exact, and reproducible in four lines. In `RetriggerMode::Legato` a new note
takes over the voice already sounding in its context, and `Voice::legato_to`
deliberately leaves the envelopes alone — *not* restarting them is the whole
difference between legato and a retrigger.

But the voice it takes over may already have been **let go of**, and
`legato_to` said so in its own comment ("it may take over a voice that was
already let go of — so this voice is held again") while doing nothing about it.
Two notes that touch put a note-off and a note-on on the same sample, and
`fontelle_sequencer::sort_events` orders the off first *on purpose* — that
ordering is itself the fix for an older version of this bug, and its `rank`
comment describes the failure in the same words. So by the time the second note
arrives the envelope is in its release stage, and a take-over that inherits it
gives a voice that is held, in tune, and on its way to zero. **The note is
there, and silent.**

Measured, on the fixture: 0.011 against the first note's 0.620, 35 dB down. On
a run of five touching notes the levels alternate —
`[0.620, 0.011, 0.620, 0.011, 0.620]` — because a dead voice goes inactive and
the note after it finds nothing to take over and allocates a fresh one, which
is exactly the *"if there was one before it next to it"* in the report. On the
real bank, the preset "Acid" produced a second note of **exactly zero**.

`RetriggerMode::Mono` never had this: it goes through `trigger_note`, which
starts the envelopes again. The fix is Legato being given the same answer for
the same case and only that case — `legato_to` reads `held` before it
overwrites it, and re-gates the amp and mod envelopes only when the voice it
took over had already been released. A note arriving over a key that is still
down still carries the envelope it found, which
`a_legato_take_over_of_a_held_note_still_does_not_restart_the_envelope` holds.

Twenty-one of Flopsynth's factory presets go through the builder's `mono`,
which is this mode, so `crates/fontelle-core/tests/legato_notes.rs` makes the
claim against the bank itself as well as against a fixture: every legato preset
has to play the second note of a touching pair. All four tests were confirmed
failing before the change.

**Still open, found next door and not fixed:** `VoiceConfig::glide_legato_only`
is written, saved, exposed on the instrument panel as `patch/voice/legato` and
automatable — and **read by nothing in the audio path**. All twenty-one of
those presets set it `true`. On the face of it, it should mean a touching note
does not glide while an overlapping one does, which is a decision about how
those presets sound rather than a defect to quietly patch.

## 2026-09-09: two tests that were not doing their jobs

Both found while doing something else, both fixed rather than written down.

**`FONTELLE_UI_DUMP` crashed the one shot that needed it.** `render_headless`
had a `dump(pixels, name)` that filled in `W`/`H` for you and a `dump_sized`
that took them. Every shot in the file that is not the standard size called
`dump_sized` — except the rack's, which is deliberately 640x480 so two rows
fit, and which called `dump`. So it handed a 640x480 buffer to a 640x360
encoder and panicked with `ImageBufferSize { expected: 921600, actual:
1228800 }`. Invisible unless the variable was set, which is exactly when you
need it: this is the route `docs/handoff.md` names for seeing pixels on a
machine whose compositor will not give a screenshot up.

`dump` is gone rather than fixed. It had one caller, that caller was the bug,
and its entire body was a guess about which size the caller wanted. A
convenience whose whole content is an assumption about its caller is worth less
than the line it saves.

**`every_pair_of_kits_is_audibly_apart` was not that claim.** Its fold is
`f32::max` over nine readings, so it passes a pair that differs on one and is
identical on the other eight — and under that name it stayed green through two
rounds of "the kits all sound the same" and let both ship. It is renamed to
`every_pair_of_kits_differs_on_something_measurable`, which is what the body
supports.

It was **not** strengthened, and the measurement is why: across the 231 pairs
the number of readings that clear the threshold is distributed
`[0,0,5,18,36,46,70,40,15,1]`, and the pairs down at two include Boom Bap
against Lo-Fi — 7.7 dB apart on a real listening measure. The instrument reads
three numbers off three *raw voices* with no bus in front of them; any fold
strict enough to be a separation gate would fail kits that are genuinely
different. Whether two kits sound different is asked where it can be answered,
in `fontelle-engine/tests/drum_kit_bus.rs`.

What core keeps instead is the cheap threshold-free half, and that had a hole
of its own: `every_style_is_actually_a_different_kit` walked `ALL` against
itself offset by one, so it compared each kit with its **neighbour in the menu
order** and nothing else, on three of the thirty-six hits. A duplicate two rows
apart passed. It is now `no_two_kits_are_the_same_numbers` — every pair, every
hit, exact equality — and it was confirmed to fail by making Latin a copy of
Studio, which is a pair the old one could never have looked at.

## 2026-09-09: a strip says how much, not what

> *"the effects trail is showing both on each track and in the section where
> you have it selected, however this makes showing it on the track redundant
> and is making it so that the volume bar is squished the more effects you
> add."*

Both halves true, and the second is the serious one. A mixer strip drew **a row
per insert** — name, bypass switch, the lot — and took the height for them off
the fader. So the one control a mixer exists for shrank every time somebody
used the newest feature, and it shrank in aid of a list the track-options
column was already drawing beside it, in full and legibly, at twenty pixels a
row instead of twelve.

`MixerStripLayout::inserts`/`add` are now one `chain: Rect`: a **fixed** row of
dots, one per insert, in the track's colour and dimmed where an insert is
switched out. Fixed is the whole point — a track with sixteen effects costs its
fader exactly what a track with one does — and it is *reserved* whether or not
there are any effects, for the reason `STRIP_WIDTH` gives about width: a mixer
is a thing you learn the shape of, and a fader that moves when the first effect
lands is a fader somewhere new every time you look. It still gives way entirely
before the fader does, which is what `MIN_FADER_HEIGHT` is now for.

What survives is the part the options column genuinely cannot show: that column
points at **one** track, and "which of these sixteen strips has anything on it,
and is anything switched out" is a question about all of them.

The row is deliberately not a target of its own — it falls through to the strip,
which selects the track and points the options column at it. Opening an effect,
bypassing one and adding one all already existed on `OptionsHit`, so
`MixerHit::Insert`, `BypassInsert` and `AddInsert` went with the rack rather
than being left as three unreachable variants with handlers behind them.
Splitting a nine-pixel row into six targets is six targets nobody can hit.

`shoot_mixer` in `render_headless` now carries six effects with one bypassed,
because a shot that cannot show the complaint cannot show the fix either.

## 2026-09-09: a kit is a bus, and the gate that had been lying

> *"we did some work on improving the drum machine built in plugin however all
> of the presets still sound nearly the same"*

The third report of this, after `metal`/`crush` fixed the noise
and the modal bank fixed the body. Both of those worked. Both of them worked on
**the hit**, and the thing that was wrong this time was not a hit.

### The gate had been lying, which is why it shipped twice

`every_pair_of_kits_is_audibly_apart` reads three numbers off three raw voices
and folds the nine differences with **`f32::max`**. Two kits pass it by
differing on *one* reading out of nine and being identical on the other eight.
It is a floor on the best case, and it was green the entire time the complaint
was true.

So the first thing built was an honest reading, and it lives in
`fontelle-engine` (`tests/drum_kit_bus.rs`) because INVARIANT 4 forbids core
from seeing `fontelle-fx` — and core therefore cannot render the thing that
makes a kit a kit. A kit becomes a **fingerprint**: six audition hits, each as
twenty log bands over twenty overlapping 43 ms frames, in dB, floored 60 dB
under its own peak and with its loudness removed. Two kits are as far apart as
the RMS decibel difference between their fingerprints.

Measured on the kits as they stood: the closest pair was 4.4 dB, the median
10.4, and **only Chiptune stood clear of the huddle** — the one kit whose
*source* is different. Studio/Rock 6.4, Studio/Techno 6.9, 909/Techno 7.3.

Two traps in building that reading, both of which would have sent the whole
round tuning the wrong knobs:

- **A Hann window is zero at its start.** With frames laid end to end the first
  one begins exactly at the onset, so the attack — the most recognisable 5 ms
  of any drum — was windowed away to nothing. The reading said a hit with its
  click at full and the same hit with no click were *identical*. Frames overlap
  by half now, with the signal padded by half a frame, so the onset lands where
  the window is one.
- **Flooring at -120 dB measures silence.** Most of a drum's last frames are
  noise floor, and a band at -118 against one at -104 is fourteen decibels of
  "difference" nobody can hear. Sixty down from the hit's own peak is the range
  it actually occupies.

### The cause: every kit was thirty-six dry one-shots

All twenty-two shipped with `fx: []` and the same two filters left wide open.
Nothing had ever touched the **bus** — and a rock kit is a room, a gated snare
is a room cut off, an 808 is dry on purpose, and a hall is what makes a
cinematic kit cinematic. None of that lives in an oscillator.

`KitSpace` is that bus: tone, glue, character, room, in the order a drum bus is
actually built, emitted as an ordinary `Patch::fx` chain — the same four slots
Flopsynth presets already carry, run by the same code in `SamplerNode`,
editable and automatable afterwards. A stage set to a wire is not written at
all, so Chiptune has one slot and Trap has two.

### The bus was necessary and not sufficient, and the measurement said so

With the buses in, the huddle barely moved — Studio/Funk went 4.42 → 4.89.
Broken down: the bus moves a kit a great deal *on its own* (Ambient 11.2, Rock
8.4), but for a pair whose **voices** are 4-5 apart it adds nothing, and Techno
and Metal got *closer* because both were given hard-clip drive and a scoop.

So the voices had to move too, and a per-knob authority measurement said which
ones could move them. `drive`, `tune`, `decay` and — on metalwork — `crush`
carry real weight; `tail`, `bend`, `rattle` and `snap` are worth under a
decibel each. Three axes were missing outright:

- **`cymbal_decay`.** `Family::Cymbal` took a family decay of one, so every kit
  in the program had the same 1.2-second ride. Two of the six audition hits
  were contributing almost nothing.
- **`cymbal_metal`.** `metal` at one is the 808's six squares and reads as
  *that machine*, so every acoustic kit left it at zero — which left every
  acoustic ride as a band of white noise, and that is the same band of white
  noise in every kit. A ride is a struck plate whether or not the hats are a
  circuit.
- **`hat_crush`.** The highest-authority knob the metalwork has: 0.15 of it
  moves a hat further than doubling its length. A machine did not sample
  everything at the same depth — memory was expensive and the cymbals are the
  longest sounds in the box — and it has to stay at zero on anything acoustic,
  which is exactly why it could not be the global `crush`.

`perc_decay` came along with them for the same reason.

### The floor is read off a pair, not chosen

`APART_DB` is **7.5**, and it is Boom Bap against Lo-Fi. Those two are the
catalogue's deliberate cousins — both a sampler with the top eaten off — and
they measure 7.6. What the gate forbids is *duplicates*, not neighbours: a
catalogue of twenty-two kits legitimately has families in it. Nine was tried
first (the 808/909 gap) and it is the wrong number — it sits near the far edge
of the space, and requiring every pair to be as unlike as those two is
requiring a catalogue with no families at all. Fitting the threshold to
whatever the kits managed would have been the same mistake as the `max` fold.

Where it landed, against where it started:

| | closest pair | p25 | median |
|---|---|---|---|
| before | 4.35 | 8.30 | 10.38 |
| after | **7.60** | **10.07** | **11.82** |

Every kit's nearest neighbour is now between 7.6 and 10.8, where before it was
one kit at 10.2 and twenty-one in a huddle.

### Two defects the bus exposed on its way in

- **Ten kits clipped through their own chain.** A bus is a gain stage;
  `no_kit_leaves_full_scale_through_its_own_bus` is the same promise core makes
  about the voices, made again through everything.
- **The kits were 29 dB apart in level** — Drum & Bass at 5.43 of full scale,
  Ambient at 0.19 — which the fingerprint could not see because it removes
  loudness on purpose. `KitSpace::trim_db` is each kit's own headroom on the
  layer, converged against a real pattern.

And one thing the trim taught: **auto make-up turns a compressor into a level
regulator, and a regulator downstream of a fader ignores the fader.** Drum &
Bass sat at 1.31 and no amount of layer trim moved it, because every decibel
taken off its input came straight back as make-up. The make-up is explicit per
kit now (`KitSpace::makeup_db`), inside the chain where the level was lost —
restoring it on the layer instead makes the *dry* voices loud, which is a kit
that clips the moment somebody switches its bus off. Pattern loudness spans
13.5 dB now rather than 20.3; the remainder is the anti-clip ceiling binding on
the transient-heavy kits, which is a real constraint rather than a setting.

`cargo run --release -p fontelle-engine --example kit_wavs -- /tmp/kits`
renders every kit **through its bus** — two bars and then the six audition hits
— which is what `drum_probe` cannot do from core. `cargo run --release -p
fontelle-core --example kit_diff -- Studio House` prints what two kits still
share, hit by hit, which is how the remaining collisions were found (808 and
Techno had the same 37.44 Hz kick).

## 2026-09-09: fields you can type in, and knowing where you are

> *"the input entering field has really bad ux and visuals right now it doesn't
> look like a input field it's just text on a background making it look like
> it's a label and not somewhere you can type you can't even see your cursor
> where you're typing and i can't ctrl a to select all my text."*

**Three complaints, one cause: there was no model of being edited.** Every
place you could type held a bare `String` and drew it with a block character
stuck on the end — so there was no caret to move, no selection to make, and
nothing for Ctrl+A to select. A field that cannot say where its caret is
cannot draw one, and a field that cannot draw one looks like a label.

**`canvas::TextEntry`** is the missing half: a string, a caret, an anchor, and
the keyboard a field has — Ctrl+A, shift-arrows, Ctrl+arrows by word,
Home/End, Ctrl+C/X/V. Byte indices always on a character boundary, so a caret
cannot land inside an accent and `String::insert` cannot panic.

**Where the split is**, and it is forced rather than chosen: the model is in
the canvas where it can be tested without a window; the *geometry* is the
renderer's, because placing a caret means measuring the text in front of it
and shaping is not something that crate may do (INVARIANT 2). So the window
measures and hands over points.

**One implementation, three surfaces.** `canvas::text_key` is shared by the
name prompts, the browser's search and inline renames. They were three
handlers with backspace-and-typing each and nothing else, and the one that went
stale was always the one you were not looking at.

**And it looks like a field**: a recess darker than the panel it sits on, an
accent border lit while it has the keyboard, a selection drawn as a wash
(rather than an inversion, so the characters keep their colour), and a caret
that **blinks** at half a second — a caret that does not blink is easy to read
as a character. `render_headless` draws both states and the PNGs are the proof.

Renames now open with **the whole name selected**, which is what every program
does and why: the commonest thing to do to a name you just opened is replace
it.

> *"we need to make it possible to tell which window is currently focused ...
> subtle but easy to tell at a glance."*

A **two-point accent spine** down the inside of the focused pane's left edge.
The roll had no way to report focus at all, so half the answer was missing and
the guarantee only held in one direction. Two louder readings were rejected for
a reason each: dimming the unfocused pane's *contents* would change the colour
of the clips and notes you judge edits by, and a picture that shifts with focus
is one you cannot trust; tinting a header sits outside the working area, where
the eye is not. It is a spine rather than the full outline the arrangement used
to draw, because an outline reads as a *selected object* and a pane is not
something you selected — it is where you are.

**`S` toggles Stretch** on the arrangement, and still cycles the snap in the
roll — the same "which canvas has the keyboard" rule `copy`, `cut` and `paste`
follow. Nothing was lost: `cycle_snap` only ever stepped the *roll's* grid, so
`S` on the arrangement had been changing a setting on a panel you were not
looking at.

**Still in-row rather than boxed:** inline renames got the whole keyboard but
draw where they always did. A recessed box on a fourteen-point row would fight
the row, and that is a decision worth making on purpose rather than by
extending a pattern into a place it does not fit.

## 2026-09-09: the blade shows the cut, and the roll got an arpeggiator

**The cut tool was drawing the question, not the answer.**

> *"it is displaying the actual visuals of the tool from the exact pixel of
> where I'm clicking and dragging my mouse instead of actually displaying it
> rounded to the grid that it's going to cut the actual clip at."*

Exactly right, and the two were never the same thing: `clip_cuts` snaps every
crossing to the arrangement's grid, per clip, and the preview was a line
between the two pointer positions. A stroke aimed a third of a beat late
*looked* like a cut a third of a beat late and landed on the beat.

`canvas::slice_marks` is the answer instead: it asks **the same function the
release will ask** and turns each cut into a mark across that clip's own row,
so the two cannot drift — a preview computed from a second copy of the
snapping rule is a preview that is right until somebody edits one of them. The
stroke is still drawn, faint, because it is feedback that the drag is
happening; the bright marks are what will actually be cut. A diagonal across
three lanes marks three different places, which is what it will do.

**An arpeggiator in the Tools menu**, as its own dialog like the other three.
FL's five controls — step (1/4 to 1/32, straight and triplet), direction (up,
down, up-down, down-up, as played, random), range in octaves, gate, repeat —
and two FL does not have:

- **Swing**, because an arp on a perfectly straight grid is the most obviously
  machine-made thing anybody puts in a project. The off-beats land late and
  the down-beats never move, so the bar stays where it is while the feel
  changes.
- **Ramp**, a velocity slope across the run, because a run in which every note
  is struck identically reads as a preset rather than as playing.

It works on **chords** — overlapping notes — so two chords in sequence become
two runs rather than one across the gap between them, and each run fills its
own chord's span and never overshoots it. Up-down does not repeat the end
notes (C E G E, not C E G G E C), which is the difference between a turn and a
stutter; random never plays the same pitch twice running. The maths is
`fontelle_model::arpeggiated`, pure and tested on its own.

**And the waveform fault, found and fixed.** The reproductions did not catch
it because they drove the *edits*; the fault was in the **gesture**.

A diagnostic through the reported sequence says it in one line:

```
stretch off   length 1920  speed 4.000  natural 1920  columns 900
grown back    length 9600  speed 4.000  natural 1920  columns 180
```

An edge drag sent `SetStretch { Off }` as its first step whenever the toolbar
switch was off. Turning a stretched clip off **freezes** the rate it was being
played at into the clip's own `speed` — `with_stretch` does that on purpose, so
the sound does not jump, and that was itself the fix for an earlier report. But
it means a clip stretched down to a quarter and then dragged became a clip
genuinely playing four times too fast, whose take really *was* a quarter as
long. Blank, unrecoverable, and the picture was arithmetically right the whole
time.

Both reports about this corner are the same fault seen from two sides:

> *"its stretching the clip back to how it was before before letting you extend
> the length of the ending"* — and — *"making the audio show completely blank
> after that even though it actually does have content"*.

**The rule now: a trim is never lossy.** That is what FL gives you and it is
the invariant that makes editing against a waveform trustworthy. A drag only
ever turns stretching *on*; it never turns it off. Turning it off is the
deliberate act it reads as — the switch does it, to the selection, and doing it
deliberately still freezes the sound where it is. So the switch's two
directions are now two different kinds of thing, which is written down where it
is read: **on** is a setting for the next drag, **off** is an action on what is
selected.

**And a block that is longer than its take says so** (`canvas::content_end`): a
rule where the file stops and a dimmed band with a centre line past it. Blank
used to be indistinguishable from broken, which is most of why this was
reported as a drawing fault. `None` for a block its file fills, and `None` for
a looping block — whose gaps are a rhythm rather than an ending.

**Still not reproduced:** the waveform appearing *offset* after a cut. Both
halves keep their own peaks and their own `natural_length`, and two suspected
mechanisms were checked and cleared — `clip_rect` is not clamped to the visible
grid, so scrolling cannot shift the picture, and the peak buckets already go
through `AudioClipData::source_position`, which accounts for trim, speed and
reverse. `fontelle-app/tests/audio_waveform.rs` keeps the reproductions.

**Superseded, for the record:

** `with_stretch_off_a_drag_on_a_stretched_clip_turns_its_stretch_off_so_the_
drag_cuts` read the switch as *"what the drag does, not a filter on which clips
it does it to"*. Coherent, and it is what made trims lossy. The test is now
`..._leaves_its_stretch_alone`, and it carries the reasoning so the rule is not
quietly reverted by somebody reading only the old comment.

## 2026-09-09: the drums are struck things now, not oscillators

> *"the sounds in it still sound way too synthesized and not realistic enough
> and not diverse enough ... ultimately still just sounding like tweaked
> versions of the same synthesized sounding sounds."*

The second report of this, and the first fix was only half of it. On
2026-09-04 the answer was that the **noise** source was the same everywhere,
and `metal` and `crush` gave the kits an axis apart. That was true and it was
not enough, because the other half of every pitched hit — the **body** — was
still one oscillator. A kick, a tom and a conga were the same sine with three
envelopes on it, and no value of tune, bend or decay could make any of them
ring like a drum, because what a drum does was not in the model.

**What a drum does.** Strike a membrane and it rings at a set of *inharmonic*
modes decided by its shape — 1.000, 1.593, 2.135, 2.295, 2.917 of its
fundamental for an ideal circular head — each with its own decay, the high
ones dying first. Those ratios are not multiples of the lowest, so the ear
never fuses them into a pitch, and that is exactly the difference between "a
drum" and "a beep at 120 Hz".

**`fontelle-dsp::ModalBank`** is that: six two-pole resonators, struck by the
hit's own transient, two multiplies and two adds a sample each, `Copy` so a
voice can still be handed back to a pool by being overwritten. Every input is
clamped, because a pole outside the unit circle does not fade — it grows,
through the layer, the track and the master — and
`every_mode_is_stable_however_it_is_asked_for` is the test that says it
cannot.

**Three knobs on `DrumVoice`**, all reading with serde defaults so every
project made until today opens sounding exactly as it did:

- **`modes`** — how much of the pitched half is a struck membrane rather than
  an oscillator. The ratios come from `DrumModel`, because the shape is a fact
  about the *object*: a kick is a head over a deep shell whatever an 808 did
  to it. Membranes get Bessel ratios; a tom gets the air-loaded ones (1.50,
  1.75, 2.00 — nearly harmonic, which is *why* a tuned tom sounds like a note
  and a conga does not); a rim and a cowbell get **bar** ratios (1 : 2.76 :
  5.40), which is why a woodblock reads as wood.
- **`tail`** — a slower decay under the fast one. A shell still moving after
  the head has stopped, which one exponential cannot be.
- **`rattle`** — the noise rung through a resonance a fifth over the body,
  where a snare's wires buzz against its shell.

**And the kits got an axis they were missing.** `modes` is the
acoustic-to-machine control, and it is not a quality knob: the 808 is at 0.06
and the chiptune kit at 0.0 **on purpose** — an 808 is a sine, and a circuit
has no membrane. Studio, Rock, Funk, Jazz and Latin are at 0.8 to 1.0. The
pairwise gate caught Lo-Fi and Industrial at 0.30 apart when the new axes went
in, which is what it is for; they are now a sampled acoustic kit through a bad
converter (short, crushed, modal) and struck steel that will not stop ringing
(long tail, no membrane).

**The kit stopped being a point source.** Every layer had a `pan` and every
kit was leaving it at zero — which is the single most "drum machine" thing
about a drum machine. Kick and snare down the middle, hats one side, ride the
other, and the toms sweeping left to right as they get bigger, because the key
*is* the position. Modest: ±0.35 at the widest, so a mono fold-down loses
nothing.

`cargo run --release -p fontelle-core --example drum_probe` renders every kit
to a WAV and prints what each hit measures, which is the thing to listen to
before touching a recipe.

## 2026-09-09: a mixer track is a preset now, and sixteen vocal chains ship

> *"i want mixer track presets ... right click a track and there's a presets
> option ... or i could save a new preset of my current version of that track."*

**A track's whole chain is a preset.** `DeviceKind::Track` and
`PresetPayload::Track(TrackChain)` — a fourth payload beside the patch, the
effect and the plugin — and `ApplyTrackChain`, one command so a chain lands as
**one** undo entry rather than a fader move and six `AddInsert`s.

Everything behind it is the preset machinery that already existed: a folder in
the bank, categories, stars, Save-as, the same `PresetDevice` the device bars
use. That was the whole reason to make a track a `PresetDevice` rather than
build a second mechanism beside it — a second mechanism would have been a
second set of bugs.

**What a chain carries**: the level, the placement, the polarity and every
built-in insert with its whole configuration and its bypass. **What it
deliberately does not**, each with its reason on `TrackChain`:

- the **name** — a preset names the sound, a track names the part;
- the **sends** — a send points at another track by id, and an id from the
  project it was saved in means nothing here, or worse means something wrong;
- the **routing and the input** — this session's wiring, not a vocal sound;
- **hosted plugins** — a chain naming a plugin this machine has not got can
  only fail at load, so saving skips them and the message says how many.

**The gesture.** Right-click a strip: the track's name, *Track presets…*,
*Save track preset…*, *Rename*. The presets row opens the bank grouped by
shelf; the save row asks a name and then a shelf, in the same place, so the
two prompts read as one gesture.

It is a **second menu rather than a hover-out submenu**, which is a departure
from what was asked for and is written down here rather than left to be
noticed. Nothing in this program has a submenu and the menu code has no notion
of one; every other two-step choice here — the plugin picker, the preset
drop-down — opens a second menu on a press, and that path already works with
the keyboard, with type-to-filter and with Escape. Real submenus are an
interaction system rather than a feature, and worth doing on purpose if they
are wanted.

**Sixteen vocal chains**, on three shelves: *tuned* (Rap Lead, Trap Lead, R&B
Smooth, Pop Lead, Drill Lead, Hyperpop Lead, Clean Correct), *natural* (Ballad
Lead, Rock Lead, Spoken Word, Backing Stack, Doubler Wide) and *effects*
(Telephone, Lo-Fi Tape, Robot Vocal, Dream Wash).

They are **named for the sound and not for a singer**. `docs/tune-plan.md` §13
already refuses a chooser of other products' voicings, on the grounds that
naming a control after somebody else's plug-in is "a preset pretending to be a
control"; naming one after a person promises more than six inserts can deliver
and is not ours to promise. Each chain is built in one order — clean up (gate,
high pass), control (correction, then compression), colour (drive, crush,
tone), place (delay, then reverb) — with the settings varying rather than the
idea. Correction sits before compression on purpose: a corrector tracks pitch
and a compressor changes level, and the tracker should hear what was sung.

## 2026-09-09: the autotune got a colour, a bank and a keyboard that is a keyboard

A second pass over the corrector, asked for as "expansive feature sets to
match other professional autotunes, and vast presets".

**§4.8, Character.** Four knobs after the shifter — `drive`, `crush`, `air`,
`width` — off or unity in a fresh corrector. The family had every axis of the
*correction* and none of the *colour*, and colour is most of what separates
an expensive-sounding autotune from a cheap one. The whole design is in the
plan; the part worth repeating is that **`drive` measures its own make-up
gain** rather than assuming it. Both obvious normalisations are wrong and the
bank found each in turn: fixing the curve at full scale made a −20 dBFS vocal
come out eight to seventeen decibels *louder*, and fixing the slope at the
origin made it eighteen decibels *quieter*. Block RMS in, block RMS out, the
ratio applied back, smoothed. Two gates hold it —
`every_tune_preset_makes_a_sound_within_three_decibels_of_the_wire` and the
new `no_factory_preset_clips_a_normal_vocal`, which caught seven presets at
once and is why the bank is now level across all forty.

**Forty presets, up from sixteen**, in four groups: the hard and robotic
(Classic R&B, Hyperpop, Drill, Robot Choir, Vocoder Lite, Glitch Tune,
Talkbox, Automaton), the natural (Gentle Correct, Live Vocal, Backing Vocal,
Rap Tighten, Country Slide, Jazz Loose, Opera, Podcast), the coloured (Warm
Tape, Bright Pop, Lo-Fi Cassette, Radio Voice, Telephone) and the shifts
(Monster, Alien, Whisper Twin). `cargo run --release -p fontelle-app --example
tune_presets` renders a vocal through every one and prints what each does to
the pitch, the level and the peak; the whole bank now sits inside ±3 dB with
nothing clipping.

**Three faults the pictures found, which no test had.**

- **The keyboard's black keys were on the wrong notes.** `ACCIDENTALS` was
  written with its bits in the wrong direction, so C# and D# were laid out as
  white keys and D and E as black ones. Every assertion still passed —
  fourteen white keys, ten black, tiling the band with no gaps — because the
  test counted them and never named them. It names them now.
- **Nothing shaped the console's labels.** `draw_window` is pure and draws
  only what is in `Labels`; `shape_labels` had a block for Flopsynth and none
  for the corrector, so in the running app every caption, read-out and card
  name on that window was **blank**. The headless shot could not see it
  either, because a test shapes its own — so the shot now shapes from the same
  two functions the app does.
- **The layout stranded a card.** Eight cards on two bands wrapped the last
  one onto a row of its own with a window's width of empty ground beside it,
  and the viewport left its spare height unused rather than growing into it.
  Two bands of three and five, a window at 1120×660, and the trace takes the
  slack up to half again its design height.

**And the rest of §7 that was still missing**: the MIDI bars under the trace,
the "−23 ¢ → A3" read-out beside the reticle, the sung dot and the bright
target key on the keyboard, and note names on the naturals — which is what
turns a row of boxes into something you can pick a scale off without counting
from the left.

## 2026-09-09: the autotune is built, and it opens on a console

`docs/tune-plan.md` followed to its end. **`EffectKind::Tune`** is a real
insert: a YIN tracker and a PSOLA shifter in `fontelle-dsp`, three engines
(Smooth / Hard / Grain) over a `texture` continuum, formant shift and
formant-follow, retune speed with humanize and flex, natural and added
vibrato, fifteen scales, notes from any channel in the rack, sixteen factory
presets under `assets/presets/fx-tune/Factory/`, and its own window drawn as
a ship's console — the pitch trace scrolling under a two-octave keyboard that
*is* the scale control, over seven cards.

**What the last stretch of it actually was.** The DSP, the corrector, the
engine arm, the presets and the window's own geometry were already standing
and green. What was missing was the wiring that makes any of it reachable:

- **`Session::tune_view` did not exist.** `fontelle_app::tune::describe` had
  no caller, so `DocumentHost::tune_view` fell through to the trait's `None`
  default and the console could never open — the whole window was dead code
  behind a default method. It is implemented now, and
  `tests/tune_editor.rs` reaches it the way the window does.
- **The MIDI source drop-down was never built.** `SetInsertNotes`,
  `EffectSlot.notes`, `Session::set_insert_notes` and their tests were all
  there, with nothing in the UI that could call them: §5's second half — the
  half that makes this a *MIDI* autotune — had no control. The MIDI card is
  the seventh card now, and its source chooser carries `canvas::TUNE_SOURCE`
  rather than a parameter address, because a source is a routing edge and not
  a value in the config. It is drawn, measured, hit-tested and opened by the
  same code as every other chooser; only the write is different, and that
  branch sits in `write_insert_param`, which is the one place every gesture
  arrives.
- **The window opened at the EQ's 720×420.** The console's layout was
  measured at 960×600 and there was no `TUNE_SIZE` for `create_editor` to
  open it at, so every layout test passed and the real window would have
  wrapped its cards off the bottom — Flopsynth's first build's fault exactly.
  `layout::TUNE_SIZE`/`TUNE_MINIMUM` exist, and `tests/tune.rs` now reads its
  two sizes off them rather than spelling them again.
- **Captions clipped.** "Natural vibrato" read "Natural vi" and "Formant
  follow" read "Formant f": `cell_span` widened a chooser on its *options*
  and the plan's §7.2 rule is about its **name**. It counts the caption too
  now, at `>= WIDE_CHOICE` — a caption has one character less room than an
  option, and "MIDI bend" at exactly nine is what proves it.

**Looked at, not just tested.** `tests/render_headless.rs` gained the case
§7.7 asks for, and the PNG is what found the last two faults above; the
keyboard is shot on A major rather than the chromatic wall a fresh config
draws, so the picture shows a scale.

**Measured (bench, this machine, stereo block of 128 at 48 kHz).** The
budget is §10's and two of its three targets are missed, narrowly:

| case | per block | of one core | §10 target |
|---|---|---|---|
| Smooth / Studio / Alto | 33.1 µs | 1.24 % | ≤ 1.5 % ✓ |
| Hard / Studio / Alto | 31.2 µs | 1.17 % | — |
| Grain / Studio / Alto | 32.8 µs | 1.23 % | ≤ 1 % **✗** |
| Smooth / Live / Alto | 56.8 µs | 2.13 % | ≤ 2 % **✗** |
| Smooth / Live / Low | 121.9 µs | 4.57 % | the stated worst case |

Both misses are small and both are the **detector**, which §10 predicted
would be the cost. The plan's own next step if this happened is written
down and is not a bigger hop: the FFT autocorrelation, on the
`fontelle_dsp::fft_in_place` that already exists. Left as it is, deliberately
and on the record, rather than loosened — the same instruction Flopsynth's
cost-per-voice carries.

**Not done, and named.** §13's list is untouched and still right. The
`Tuner` stub in `fontelle-fx/src/meters.rs` is gone as phase 6 asks, with a
note where it was: the catalogue still wants that read-out insert, and it
should wrap `PitchTracker` rather than grow a second pitch detector. `held`
on the keyboard is read off the newest trace frame, so it marks the note
being *forced* rather than every key somebody is leaning on —
`TuneFrame` is four aligned words on purpose and a held mask does not go in
it; the reasoning is on `session::held_classes`.

## 2026-09-08: the autotune is designed, not built

Ty asked for a built-in autotune that can be a professional one or a cheap
one and everything between, controlled by MIDI or by a scale on a keyboard in
its own window, with presets, drawn like a ship's console. The design is
**`docs/tune-plan.md`** — the parameters and their ids, the two DSP
primitives (a YIN tracker and a PSOLA shifter, both for `fontelle-dsp`), the
three engines, how notes reach an insert (`EffectSlot.notes`, a routing edge
like `key`), the sixteen presets with their settings, the window's layout and
look in the renderer's own primitives, every test to write first, the budget
and the phase order. No code has been written; phase 0 freezes the name
(`EffectKind::Tune`, slug `fx-tune`) and Ty's overrides go in before that.

## 2026-09-07: the text stopped flickering, and the piano stopped being a clavinet

Two reports from using it, and both were real faults with a measurable cause.

> *"the text constantly keeps flickering while trying to navigate the app"*

**`Labels` was emptying itself in the middle of the frame it was filling.**
The shaped-label cache is bounded at 512 strings, and over the cap it was
cleared *wholesale* — but `ensure` is called from `shape_labels`, which is the
frame's own shaping pass, so every string shaped before the clear was gone by
the time the (pure) renderer looked it up, and `Labels::get` returning `None`
draws as nothing. One frame of missing text, and then the next frame refilled
toward the cap and did it again. Navigating is what fills it: a browser page
of soundfont names and details, an editor window's captions and read-outs,
the roll's key names, all strings that come from data rather than the theme.

Every string now carries the frame that last asked for it (`Labels::
begin_frame`, called at the top of `shape_labels`), and a full cache drops
only what the *current* frame has not asked for. A frame that wants more than
the cap keeps all of it, which is the right trade: one frame of extra memory
against text that vanishes. `tests/text_layout.rs` holds both halves — a
frame of 700 strings keeps every one, and a cache that fills lets the
previous frame's go. Confirmed on the nested X server before and after.

> *"the grand piano sound doesnt sound realistic at all"* … and, after the
> first pass, *"sounds more like a clav or something"*

The second report was the useful one, because a clavinet is a *specific*
wrong answer and it named two measurable faults. Both are now tests in
`crates/fontelle-core/tests/grand_piano.rs`, which is the file to read before
touching this row.

**The spectrum was hollow.** The `Struck` table was built as `sin(πhβ)/h` —
the *velocity* distribution of a struck string — which puts the second
partial level with the first; with the old octave layer on top of it, the
second partial measured **3.8 dB over the fundamental**. That is a nasal,
hollow spectrum and it is a clavinet's. What is heard is the *displacement*,
`sin(πhβ)/h²`, and the hammer's hardness tilts that exponent (2.6 soft to 1.3
hard) rather than only moving a felt corner — which is also where velocity's
brightness now comes from, the string's own spectrum rather than a filter
sweep. It stays above `h^0.9` at every position, which is the exponent at
which the comb would make it hollow again.

**There was one slope where a piano has two.** A piano loses most of its
energy in the first half-second and then rings on quietly for tens of
seconds; this row drew a straight −14 dB/s line and was thirty decibels down
in 1.8 seconds, which is an electric piano. The prompt sound is now its own
layer — the same string struck harder, brighter, on a fast envelope of its
own — sitting over a long aftersound. Middle C now falls 30 dB in 5.7
seconds, C7 in 1.4, C2 in 13.7.

**Three traps found on the way, all worth knowing:**

- **A gain route reads an envelope's level and multiplies it into decibels**,
  so a `Decibel`-curve envelope on `LayerGain` is exponential twice over. The
  prompt layer was fifty decibels down fifty milliseconds in — a flash, not a
  prompt sound — and the strike came out no brighter than the ring it was
  meant to be shining over. Modulation envelopes on a gain want `Linear`; the
  *amp* envelope is the opposite case and wants `Decibel`, because its level
  is the gain rather than a number of decibels.
- **A layer at exactly `SILENT_DB` is skipped by the voice** until a route
  lifts it, and a route is read at the start of a block — which for a knock
  twelve milliseconds long was most of the knock. Anything a fast envelope
  brings in sits just *above* the floor.
- **A spectral centroid cannot see brightness on a fundamental-dominated
  spectrum.** It is a magnitude-weighted mean, so once the fundamental is
  properly the strongest partial it is pinned there: a strike an ear hears as
  far brighter than its own ring read a fifth of an octave apart. The tests
  measure a *tilt* instead — partials 4–16 against 1–3, in decibels — which
  is what "bright" means for a harmonic tone.

**`ModDest::EnvelopeStageTime` was never implemented.** It has been in the
matrix since the matrix existed and is offered in Flopsynth's own address
table as "Env N decay time", and the voice never read it — a route to it was
a knob that moved nothing. It is applied now, in **octaves of time** (a
full-depth route spans eight, like cutoff and pitch), for every envelope, and
it is what makes the piano's decay follow the key: the stored time is the
*treble's* and an inverted key route stretches it towards the bottom.
`crates/fontelle-core/tests/envelope_times.rs` covers it on its own.

The bank is still 211, every gate passes, and the trim was re-measured with
`preset_probe` after each change. Nobody has *listened* yet — `cargo run
--release -p fontelle-app -- --play-flopsynth "Grand Piano"` is the fourth
column and no analyser can stand in for it.

## 2026-09-07: a piano to open on, instruments untied from tracks, and prefabs

Three asks in one message, and the third is the one that changes the model.

> *"instead of starting with a 3osc it starts you with a flopsynth instrument
> on a grand piano preset and instead of there being a clip in the arrangement
> already make there be no clip yet."*

**`blank_project` opens on Flopsynth playing a new `Grand Piano` row** of the
bank (`STARTING_PRESET`), channel named after it, and puts **no clip** on the
arrangement — ten rows, nothing on any of them. `tests/starting_project.rs`
holds all three claims. The bank declined a grand piano on principle ("a
sampled instrument"); what was authored is the *shape* of one: three strings
five cents apart, a hammer gated silent at rest, upper partials on their own
envelope, a sine at the fundamental for weight, no sustain, a 100 ms damper,
and **no sub octave** — a `SubSine` at −21 dB measured as four fifths of the
spectrum. Two traps in the tuning, both now in the row's comments: a layer
arrives as `FilterRoute::Serial` (F1 *then* F2), so the strings went through
the hammer's high-pass until routed to F1; and the octave layer beat the
fundamental in the analyser's bins until the sine went in. The bank is 211,
every gate passes, and every other preset is byte-identical to before —
verified against the JSON embedded in a pre-change `libfontelle_app` rlib,
which is how a global `str.replace` that had silently retuned seven other
rows was caught and reverted.

Tests that wanted *a clip to work on* now take one from
`common::a_project_with_a_clip`; the shakedown draws its own blocks, which is
what "the way a person makes one" meant anyway. **And `main.rs` had gated the
whole studio session on `first_clip`** — the window opened with no rack, no
browser and no arrangement the first time it was launched, and no test saw
it. The gate is gone (`Session::adopt` always opened clip-less projects this
way); the fault was found by looking, on the nested X server.

> *"make it so duplicating an instrument literally just makes a new instrument
> that has the exact same params so it sounds the same but it shouldnt have
> the same notes ... shouldnt affect the arrangement at all."*

**`DuplicateChannel` copies the channel and nothing else** — no clips, no
lane. It had copied the clips onto a row of their own, which was coherent
while a clip was one instrument's track and stopped being when a clip could
hold several (`Note::channel`). Three tests in `tests/arranging.rs`, including
that undoing a duplicate takes nothing of the original's with it.

> *"the prefab system for having clips that you can basically draw into your
> arrangement that making a change in that prefab clip affects all the clips
> in the arrangement that are referencing that prefab."*

**Prefabs, TDD §10.5, mirror instances shipped.** A `Prefab` is named content
off the arrangement; a **place** is a `Clip` whose `prefab_link` names it and
whose own `source` is *empty*. The model's one rule, and the one every reader
now follows: **read a clip through `Project::clip_source`, never
`clip.source`** — borrowed for a mirror, owned only when there is something to
resolve. Note commands take `impl Into<NoteHome>` (a `ClipId` still reads as
one), `Project::note_home(clip)` says which of the two an edit to a clip
means, and `Session::note_target()` adds the second way in Ty asked for: pick
the prefab in the list, pick the instrument in the rack, edit the roll.
`AddPrefab`, `AddPrefabInstance`, `RenamePrefab`, `DetachPrefab`,
`RemovePrefab` and `MakePrefabFromClip` (in place — same `ClipId`); the last
three **bake** the resolved content into every place first, so deleting a
prefab never empties eight bars of somebody's song. The compiler resolves
places on the model thread (INVARIANT 3), places cut rather than grow, loop,
and mute like any clip. Fourteen model tests, nine sequencer tests, fourteen
session tests.

**The panel.** The rack has a tab strip — *Instruments | Prefabs* —
shared by both lists (`canvas::tab_strip`) so it cannot move when pressed;
the prefab list is the rack's shape (whole rows, pinned `+ Make prefab`,
virtualised, its own scroll), each row carrying a use count (an em dash for
"nowhere"), right-click to rename or delete, click to open in the roll, click
again to put the roll back on the arrangement. With a prefab picked, the draw
tool puts a **place** down, captioned with the prefab's name rather than the
instrument's. Fifteen geometry tests in `fontelle-ui/tests/prefab_panel.rs`;
the workflow itself was driven by XTEST and looked at.

**Deliberately not built, and why:** `OverrideMap` is carried in the format
and **not applied** — a note has no `PersistentId` yet, so an `ElementId`
cannot address one; `prefab::apply` says so rather than shipping a no-op under
a working function's name. Variants are structure-only. And there is **no
gesture yet for "make a prefab from this clip"**: the host method exists and
is tested, but the arrangement has no clip context menu (the right button
erases), and whether to give it one is Ty's call.

**On the process:** this session ran the full suite six times and each run
told it what a targeted run would have in a minute, while the one regression
that mattered — the empty window — no test could see. Ty's rule, now in
`docs/handoff.md` §2: the workspace suite runs **once, at the end**; the loop
runs the binaries you touched, and anything visual gets looked at.

## 2026-09-07: five reports from playing it, and the window-raising one

A session of reports from using the studio. Four were small; the fifth had
been reported twice before and was wrong both times, so it is written up in
`docs/handoff.md` §4 under *"Raising a window on the user's desktop"* rather
than only here.

**Plugins are scanned while the studio opens.** > *"can you make it load
plugins when the program starts instead of loading them when you go to add a
plugin."* `Session::scan_plugins` walks the folders and answers
`(found, would not load)`; `main` calls it beside `open_bank` and prints the
count, and the settings tab's *Rescan* goes through the same function. The
lazy `PluginRack::scan_once` stays, so a headless render and every test rig
still pay nothing — but in a window the list is there before any menu opens,
which is what a menu that hung for three seconds on 1047 plugins was.

**A menu takes the wheel in an editor window, and its thumb can be dragged.**
Only the studio handed the wheel to an open menu, so the preset drop-down in
Flopsynth's own window — 210 presets — could never be scrolled past its first
section (`WindowApp::wheel_menu`, called from both windows now). The scrollbar
was drawn and had no hit test at all; `ContextMenu::scrollbar_track`,
`thumb_grab` and `drag_thumb` are the geometry, `Drag::MenuScroll` is the
gesture, and a press *beside* the thumb takes hold of it by its middle,
because a four-pixel target you have to aim at is worse than the wheel.

**The stuck note.** > *"after playing notes it seems to want to often just
hold a note forever if i spam lower notes."* `Sampler::note_off` asked the
pool for the first *active* voice on the key — and a voice in its release tail
is still active. Press a key, let go, press it again before the first has
finished ringing, and the note-off went to the voice that had already had one;
the voice actually being held was left with nothing that could ever address
it, sustaining forever on any patch whose envelope sustains. Low notes reached
it first because their tails are longest, and each press handed the orphan on
to a new voice, which is why it kept happening once it started. A `Voice` now
knows whether its key is **down** as well as whether it is **sounding**, and
`VoicePool::find_active_mut` considers only held voices, newest first — so a
lost note-off strands an old voice the steal path reclaims rather than the one
being played. `sampler::tests::a_key_pressed_again_while_it_rings_out_still_gets_its_note_off`
reproduces the report exactly and is the regression.

**An instrument or effect that is added or changed opens its window**
(`open_chosen_instrument`, `open_newest_insert`), from every path that chooses
one. Browsing presets deliberately does not.

**And the window-raising one, third time lucky.** > *"it does flash in my
taskbar like its trying to focus that window but its not actually bringing the
window to the front."* Under Wayland `focus_window` and `set_window_level` are
empty functions in winit — the two previous attempts were built on calls the
compositor never sees. The mechanism is xdg-activation, and KWin honours a
token only if it carries the serial of the interaction that asked for it
(`Workspace::mayActivate`); a token without one is refused, and the refusal
path is `demandAttention()` — the flash Ty saw. winit exposes no input serial,
so `fontelle-ui`'s new `activation` module opens a second event queue on
winit's own `wl_display`, binds `xdg_activation_v1` and the seat, keeps the
largest button/key serial its own pointer and keyboard hear, and asks for a
token on the studio's surface with that serial on it. Confirmed working on
Ty's Plasma 6.7.4 desktop. **The sandbox cannot test this** — the nested X
server has no compositor — so `docs/handoff.md` has the full derivation,
including the two dead ends, so that nobody walks the ladder a fourth time.

## 2026-09-07: the bank, widened to two hundred and ten

> *"expand the roster of built in presets for flopsynth even further.
> variety of high quality instrument sounds."* — Ty

**Eighty-two new presets**, taking Flopsynth's bank from 128 to **210** in
the same fourteen categories, weighted towards the instruments the brief
asks for: eight guitars and a sitar and a koto under Pluck, eight more
winds (both saxes, trombone, tuba, bassoon, piccolo, a muted trumpet, a
shakuhachi), an accordion and a melodica and two more electric pianos, six
more strings from a viola to a mute, six organ registrations, six bells,
and six acoustic-leaning basses. `cargo test --workspace` is **3463
passing, 0 failing** and `cargo clippy --workspace --all-targets -D
warnings` is clean.

**The gate moved first, as it should have.**
`there_are_at_least_a_hundred_and_twenty_and_every_category_has_at_least_six`
is now `…_two_hundred_and_every_category_has_at_least_ten` — written,
watched to fail at 128, and only then filled. Ten to a category rather than
six because a shelf you read in one glance is a list, not somewhere to look.

**`cargo run -p fontelle-core --example preset_probe --release` is new, and
is the reason this was one pass rather than thirty.** The module's docs had
claimed it existed since the bank was written; it did not. It renders the
whole bank once and prints what the tests measure — each preset's level, the
**number to add to its `.out(…)`**, its peak on a loud chord, and its nearest
neighbour inside its category on the five axes. With a category named it also
prints the axes themselves, which is what a collision actually needs: a pair
that reads "too close" is close on *one* axis, and knowing which one is the
whole of the fix. The eighty-two loudness trims are its output, not a guess.

**Three things the measurement found that listening would have taken longer
to, and that the next person adding rows should know:**

- **The sub goes *around* the filter** (`bass`, and rightly — §7.3). So on a
  patch whose filter is a narrow window, the sub *is* the output, and four
  basses built on different ideas measured as one preset because all anyone
  could hear was the same sine. Metallic and Bowed now have no sub at all and
  Fretless has it 8 dB down; the plan's Bass note says so.
- **The organ archetype's key click is a continuous −30 dB of high-passed
  noise** with an envelope on top, not a gated burst. On any registration
  whose own spectrum is thin, that hiss is the whole profile — which is why
  three drawbar registrations read as the same preset. Gospel and Theatre turn
  it down to −44 dB and give velocity somewhere audible to go instead.
- **An effect cannot make two presets different**, because the pairwise test
  renders through `Sampler` and the chain runs in `SamplerNode` (§2.2). Rock
  Organ was a Drawbar Jazz with a distortion on it; the saturation had to move
  into the ladder's own drive before it was a second preset.

One thing outside this work: `canvas/effect.rs` still imported `Taper` and
`Unit` into a function that no longer uses them, left over from the previous
session's `effect_params` extraction. It fails `clippy -D warnings` on a cold
build, so it is fixed here.

**Still not done, and still worth doing:** nobody has *listened* to the new
eighty-two. Three numbers are necessary and not sufficient — the bank is
provably distinct and provably level, which is not the same as good.
`cargo run --release -- --play-flopsynth "<name>"` plays one and `list`
prints all 210.

## 2026-09-07: Flopsynth's window, looked at

> *"right now all of the presets cannot cleanly display and fit on screen.
> also the synth settings seem to be going off screen particularly the noise
> section. the design also feels pretty basic for the synth, we should make it
> look spacey and futuristic."* — Ty

The section below this one built the window and never opened it on the
Init patch at the size it opens at. Doing that found five things no geometry
test with four cards in it could see, and this session is the fixes, the
tests that now hold them with **the whole page**, and the look.

`cargo test --workspace` is **3463 passing, 0 failing**;
`cargo clippy --workspace --all-targets -- -D warnings` is clean. Every piece
below was written test-first and seen to fail first — the layout tests
against the new `FlopsynthCard` fields, the app tests against host methods
that did not exist.

### What was wrong

1. **The cards were placed in list order, not band order.** The host lists
   the channel and the voice first because that is where the parameter list
   begins; the layout honoured the band only when it *changed*, so the two
   last-band cards sat above the oscillators.
2. **The Synth page was a band and a half taller than the window.** Five
   sources in one row wrapped the noise under the oscillators, and the three
   filters, both envelopes and the macros were never drawn at all — the
   window does not scroll (§8.1 rule 7), so they were simply gone.
3. **The Presets page was blank.** `page_of` sent no card there and nothing
   else was drawn.
4. **The preset drop-down was one column three thousand pixels tall**, of
   which the window showed thirty rows and hinted at nothing — no thumb, no
   typing.
5. **Escape on an open drop-down closed the whole editor**, because the
   editor's key path gave Escape to the window before a menu could take it.

### The page that fits (`canvas/flopsynth.rs`)

- **A card declares its shape.** `FlopsynthCard` carries `aside` and
  `columns` beside `row`, set by `fontelle-app/src/flopsynth.rs::shape_of`
  — the layer that knows what each card *is*. The three oscillators are six
  cells across; the sub and the noise are **set aside** in a column down the
  right-hand edge, three across, beside the oscillators *and* the filters;
  the channel's two knobs end the filters' row, where the sound goes out;
  the voice and the macros stand beside the two envelopes. Three bands, and
  the bands are sorted before they are placed.
- **A chooser with long names takes two cells** (`cell_span`, threshold
  `WIDE_CHOICE`): "NES Pulse 12.5" in a fifty-pixel cell was "NES Pu".
- **Cells are 52 × 54 with a 24-pixel knob**, captioned at the small label
  size (`text::SMALL_LABEL`, eleven pixels; `Labels::ensure_small`). A
  hundred and thirty controls do not fit at thirteen.
- **When the page still would not fit, everything gives in order**: air,
  then the pictures to a soft floor, then every cell together down to
  `CELL_FLOOR` (0.8), then the pictures to their hard floor. At the minimum
  window size the whole page is there, smaller, rather than with its bottom
  band missing. `flop_knob_rect` is read off the cell, so the knob shrinks
  with it and the modulation ring still clears the caption.
- The Modulation page's matrix **takes the room under the cards** instead of
  sitting two rows tall at the bottom with a dead band above it.
- Held by `the_whole_synth_page_fits_the_window_it_opens_at` and its
  neighbours, which build the real thirteen-card page and check it at
  `FLOPSYNTH_SIZE` and `FLOPSYNTH_MINIMUM`; and by
  `the_synth_page_declares_each_cards_shape` in `fontelle-app`, which does
  the same with the view the session actually builds.

### The bank you can see

- **The Presets page is §8.6's**: shelves down the left (★ Favourites when
  there are any, All, every category, Mine when there are any), the presets
  under a search box in the middle, and the loaded preset described on the
  right. A row is `ApplyPreset` and a star is the favourite the bar's star
  is; it invents no mechanism. Typing goes to the search whenever the page
  is showing, Escape clears it before it closes anything, and the wheel
  scrolls the list. On the mixed shelves a row wears its category.
- **A menu too tall for the window lays out in columns** when the columns
  fit (`ContextMenu::columns`), the way every big menu in every DAW does:
  the 128-preset drop-down is five columns with every heading visible at
  once. A list too long even for that — the 357-plugin picker — scrolls as
  before, and **shows a thumb** now (`ContextMenu::scrollbar`).
- **The drop-down filters as you type**, the plugin picker's rule
  (`preset_menu(choices, query)`), with a first row saying what has been
  typed and the empty sections dropped.

### The chain you can edit (§8.5)

The Init patch's Effects page was an empty sky with no word on it, and no
way to put anything there. It has a **`+ effect`** button now, whose list is
`fontelle_core::flopsynth::PATCH_FX_KINDS` — the eight zero-latency kinds in
§3.9's order; the gate looks ahead and is not offered — and every effect
card has a ✕ in its header. `add_patch_effect` / `remove_patch_effect` are
structural edits through `store_patch`, so they undo. An effect card is
built by `canvas::effect_params`, the same function the effect window uses,
so a chorus's mode is a chooser that says its names rather than a knob
reading "0.00".

### The look

The one place this program draws a gradient. The ground is the theme's
`window` graded towards the accent with a nebula in the accent at one corner
and in the modulation violet at the other, and ninety stars from a fixed
scatter. Cards are glass — a graded translucent panel with a highlight along
its top edge and its family's rule under its name: the three oscillators in
the theme's three ramps (§8.1 rule 2), the filters in the accent, everything
that *moves* something in the violet. Knobs are domed and their value arc
glows; choosers are chips carrying their value with a wedge; switches are
pills with a dot. Pictures sit in a scope with a faint grid, the curve lit
under a wide faint stroke, the response and envelope filled beneath. The tab
strip is one track with the page you are on lit in it. Everything is the
palette's own colours mixed, so the light theme gets a pale version of the
same sky.

### A trap worth recording

**A grab off the nested X server is one presented frame behind.** Every
"the click did nothing" this session — the row that did not load, the
search that did not filter, the drop-down that did not open — was the
program having done it and the screenshot showing the frame before. Nudge
the pointer and grab again before believing a screenshot; `docs/handoff.md`
§5 has the rule.

### Left alone, on purpose

- Reordering an effect slot by dragging its header (§8.5) and the About
  column's macro and wheel lines (§8.6) are not built; the header carries
  `FlopsynthHit::Header` for the first, and `preset_about` is where the
  second goes.
- The cost per voice is where the section below left it.

## 2026-09-06: a preset system for every device, Flopsynth's window, and what a voice costs

The rest of `docs/flopsynth-plan.md`: **§P (the preset system), phases 4–6 (the
window), §P.8 (the browser tab), §P.9 (the removals) and §10 (the bench)**.
The section below this one is the synthesiser itself, which was already
sounding; this is everything around it.

`cargo test --workspace` is **3437 passing, 0 failing**;
`cargo clippy --workspace --all-targets -- -D warnings` is clean. Every piece
below was written test-first and seen to fail first.

### A preset is a file, for every device

> *"a preset system kind of like FL Studio's baked into the DAW itself that
> works for every instrument and effect so we don't have to hardcode presets in
> every plugin ... when you have a `*` for unsaved edits you're able to save it
> either to the same preset or save as to a new preset in your bank."*

- **The bank** (`fontelle-app/src/preset_bank.rs`). Factory presets are files
  in `assets/presets/<device>/<category>/<name>.json`, embedded at build time
  by a forty-line `build.rs`; user presets are the same files in a folder the
  user owns (`Settings::preset_dir`, defaulting under the XDG data directory).
  **167 factory presets ship**: Flopsynth's 128, the drum machine's 22, and the
  distortion's, bitcrush's and Soften's 17.
- **`cargo xtask export-factory-presets`** is where they come from. The recipes
  that used to be constructor code — `DistortionConfig::from_preset`,
  `drum_kit(style)`, Flopsynth's `FACTORY` — are now the *authoring tool* the
  export runs, in the position `DrumKitStyle` always had. Running it twice
  writes nothing the second time, which is a test.
- **One bar in every editor window** (`canvas/preset_bar.rs`): `◀ ▶`, the name
  with its `*`, a drop-down grouped by category with favourites first, a star,
  Save and Save as…. The same bar over a synthesiser, a drum machine, a reverb
  and a hosted plugin, because what a device contributes is nothing but *what
  its state is*.
- **The `*` rule is Ty's** (§P.6): the name is remembered and the cleanliness
  is *recognised*. A device carries the `PresetRef` it was loaded from through
  every edit and never stores whether it is dirty — that is computed against
  the bank's copy of the file, which is why one undo makes the star go out with
  nothing to remember.
- **A fifth browser tab** lists every preset for every device, with a search
  across the whole bank. An instrument preset clicked there lands on the
  selected channel, switching its kind if it has to; an effect preset lands in
  the insert whose window is open, and says so when none is.
- **Two commands** carry it in the document: `ApplyPreset` writes the state and
  the name in one undo entry, `SetPresetRef` writes only the name — which is
  what a save does after the file is on disk, so undoing a save puts the old
  name back and leaves the file alone.
- **The chip rows are gone** (§P.9). `EffectConfig::{presets, apply_preset,
  matching_preset}`, `SetInsertPreset`, `set_instrument_preset` and the panel's
  preset row went with them: one preset mechanism, not two.

**A latent defect found on the way:** `serde_json`'s default float parser can
land a ULP away from the number in the file, so a patch written and read back
was not the patch that was written — a preset read as *edited* the moment it
was loaded, and every float in every project shifted on every open. The
`float_roundtrip` feature is on across the workspace now.

### Flopsynth's window

- **Four pages** — Synth, Modulation, Effects, Presets — as a tab strip along
  the top of the body. The cards are filtered to the page by what they *are*
  (`fontelle-app/src/flopsynth.rs::page_of`), so the window is still the signal
  path rather than a list.
- **The gestures of §8.7.** A wave picture is dragged sideways for position; a
  filter's response is dragged in both directions for corner and resonance; an
  envelope's four corners are dragged, times sideways and the sustain up. Each
  moves a control the panel already draws, found by the tail of its address —
  so a picture and a knob can never disagree about what they are.
- **Drag-to-assign.** A source badge on the Modulation page is dragged onto a
  knob: every control that can take a route lights a violet ring while it is in
  flight, and releasing on one makes the route at half depth. The ring itself
  is a control — a band four pixels outside the groove — so a modulated knob
  can still be *turned*, and dragging the ring is the depth of the newest route
  to it.
- **The matrix** is rows rather than a card of knobs: source, destination, a
  bipolar slider and a remove button, with the depth on the ordinary live wire
  like every other parameter.
- **A fifth palette token, `modulation`** (theme format v7): violet, from
  outside the three ramps, because at three pixels an arc has to be tellable
  from the accent, the playhead, a note *and* the automation amber.
- **A voice meter and an LFO dot.** How many voices are sounding is read off
  the audio thread's own state through a new `VoiceMeter` — the first read-out
  in this program that is not a fact the document has — and the newest voice's
  LFO phases come back the same way, so the dot on each shape says what the LFO
  is *doing* rather than what it is set to.
- **`--help`**, which this program did not have.

Two things the headless shot caught that no geometry test could: the `◀ ▶`
chevrons were drawn pointing the wrong way, and the modulation arc struck
through the caption of the knob it belonged to. The second is why the control
cell is 74 px tall rather than §8.8's 60 — a bipolar arc grows from straight
up, which is the topmost point of the circle, so a ring outside the groove
needs room above the knob that a cell sized before the ring existed did not
have.

### What a voice costs, and what was done about it

`crates/fontelle-core/benches/flopsynth.rs` is the first bench in this tree
(TDD §20.5 asked for them from day one and `benches/` has been empty since).
One iteration is **one second of audio at 48 kHz**, so the wall time *is* the
share of one core.

| case | budget (§10) | before | after |
|---|---|---|---|
| one voice of Init | 0.3 % | 1.65 % | **0.54 %** |
| one voice of Supersaw | 1.2 % | 3.29 % | **1.31 %** |
| sixteen voices of Choir Ahh | 8 % | 49 % | **26 %** |
| the wavetable bank, all 39 tables | — | — | 54 ms, once |

Three optimisations, each test-guarded:

1. **A silent layer nobody reads is not rendered.** The Init patch is five
   layers with one of them up; the other four were costing a table read, a
   unison stack and a filter feed per sample for silence. The exception is
   stated and tested: a layer at the floor that another layer *modulates with*
   is still rendered, because a modulator's level is how much of it you hear
   and not whether it modulates.
2. **A filter's coefficients are built when they move.** `SvfFilter::coeffs`
   pre-warps the corner with a `tan` and was being called **per sample** —
   twice at 24 dB, three times on a formant. The settings only move every
   `FILTER_STEP` samples. This was two thirds of an Init voice.
3. **A unison stack's per-voice constants likewise.** A detune ratio (a
   `powf`), a phase step and a pair of pan gains, per voice per sample, for
   numbers that only change per block. "Supersaw" is three oscillators of seven
   voices, so that was twenty-one `powf`s a sample for constants. Supersaw went
   from 2.4 % to 1.31 % on this one.

**Where the remaining cost is**, measured rather than guessed
(`flopsynth/where-it-goes`): of an Init voice's 5.4 ms, the filters are about
2.0 and the mod matrix about 0.6; the other 2.8 is one oscillator, the amp
envelope and the per-sample bus routing. Meeting 0.3 % means the whole voice
costing what one bare oscillator costs today. §10 names two more levers — a
two-frame read when the position sits on a frame (landed, and it does not fire
for the presets measured, because their positions sit between frames) and
`f32::tanh` → a rational approximation. Past those it is a design conversation
about the fixed topology, which is what §10 says to have.

## 2026-09-06: Flopsynth — the synthesiser, its bank, and everything under them

> *"a new built in synthesizer plugin. this will be our main synth for the daw
> kind of like how fl studio has flex ... should be an advanced synthesizer
> inspired by the likes of omnisphere and Serum ... should have lots of built
> in presets in a bank for tons of instruments organized by type."*

`docs/flopsynth-plan.md`, phases 0 through 3, plus the panel that makes it
editable and the browser row that makes its bank reachable. **3,300-odd tests
green, clippy clean.** Every piece below was written test-first and seen to
fail first.

### What it is

An ordinary `fontelle_core::Patch` whose layers carry a new `Source::Synth`
— the drum machine's lesson taken again, so save, load, automation, the key
map, the mixer and undo never had to be told it exists. `InstrumentKind` is
six now; a Flopsynth channel arrives playing and needs no files, because
every wavetable it reads is **generated from a spectrum recipe at first use**.

### The sound (`fontelle-dsp`, `fontelle-core`)

- **Thirty-nine wavetables**, in nine families, each a frames × mip pyramid
  built lazily behind a `OnceLock` and resolved in `prepare` (INVARIANT 1).
  Ten mip levels rather than seven, because seven stops at sixteen harmonics
  and sixteen harmonics of A8 is 112 kHz.
- **The oscillator**: eight-voice unison with a bias so a wide stack sounds
  wide, seven warp modes each a continuum that is a wire at zero, through-zero
  FM and RM reading a *later* layer, hard sync whose slave restarts on the
  master's overshoot, and tilted noise.
- **Four filter models** behind one slot — Clean (with a 24 dB cascade),
  Ladder, Formant and Comb — plus drive, key tracking and a character knob
  whose caption is the model's.
- **Envelope shapes**, a per-voice LFO with tempo sync, free-run, one-shot,
  fade and smoothing, macros, `Random` and `NoteOnCounter` implemented, eight
  new modulation destinations, and a per-layer filter route through four buses.
- The format went to **version 1** with a migration (`Lfo::shape` became
  `Lfo::wave`), and every patch the tree could write before still round-trips.

### Three bugs the tests found, which would all have shipped

- **The ladder self-oscillated at half resonance when bright.** Its feedback
  was taken from the previous sample, and a sample of delay is a phase lag
  proportional to frequency — so near Nyquist the loop hit −180° with the
  stages barely attenuating. Any preset sweeping a ladder upward screeched.
  The loop is solved algebraically now, the way the SVF's is.
- **Phase 0 of every table was a cosine, not a sine**, so a note-on started at
  full amplitude — a click on every note.
- **An FM modulator turned down to silence stopped modulating**, because the
  modulator's sample was read *after* its level knob. A dedicated FM operator
  is exactly the thing nobody wants to hear.

### The bank

**A hundred and twenty-eight presets in fourteen categories**, written as a
table of rows over a dozen archetypes (`flopsynth/presets.rs`) rather than as
JSON, because a row reads as a sentence and two hundred fields do not. Held by
`fontelle-core/tests/flopsynth_presets.rs`: every preset sounds, every one
stays inside full scale on a four-note chord at velocity 127, every one sits
within 3 dB of the bank's median, and **every pair inside a category is
measurably apart** — on five axes, because the four the plan named cannot tell
two vowels apart and the fifth (a ten-band spectral profile) can.

Loudness was matched by measuring the whole bank and writing the column, not
by ear (§13's third risk). `--play-flopsynth <name>` plays one, `list` prints
them all — which is the listening half of the gate, because three numbers are
necessary and not sufficient.

### Reaching it

- **`Session::set_instrument_param` no longer rebuilds the graph** for a patch
  parameter (§2.3): the document is written quietly and the value goes on the
  live wire, so a cutoff can be swept under a held chord without every mouse
  move cutting every sounding note. A layer's *table* is the one exception,
  because resolving one locks the wavetable bank.
- **The instrument's own effects chain** runs in `SamplerNode` after the voice
  sum, so a preset's chorus and reverb are part of the preset.
- **The panel draws every control** — nineteen cards, a hundred and fifty
  knobs, all automatable, all on the wire. It is the general grid and not yet
  §8's bespoke canvas; the addresses are the ones the canvas will use.
- **Flopsynth's bank is a row in the Sounds tab**, opening to its presets
  grouped by category, with the search and the click-to-install the soundfont
  list already had.

### Still to do

Nothing in `docs/flopsynth-plan.md` — see the section above this one, which is
the rest of it. What is left is the cost per voice, which the plan budgets and
this build does not meet.

## 2026-09-06: the plan's leftovers, a clip's end, and delay compensation

Two instructions, in order: *"follow through on everything still remaining
open"*, and then *"clip endings don't actually cut the clip short audibly
right now it keeps playing"*. The report is under "The clip's end" below;
what follows here is the first half — the four things the previous session
named
(`--render-wav` has no rack, LV2 `isSideChain`, slides into hosted
instruments, the VST3 bridge) and the rest of TDD §8.4's "Still not done".
**All of it is closed but the VST3 bridge**, which is a separate private
repository built against Steinberg's SDK and cannot be written in this tree
(§3.4 is the reason it lives outside it). Every piece below was written
test-first and seen to fail first.

`cargo test --workspace` is **3218 passing, 0 failing**;
`cargo clippy --workspace --all-targets -- -D warnings` is clean.

### The offline bounce hosts plugins

`--render-wav` realised its graph with no rack at all, so a channel playing a
plugin bounced as **silence** — the one path in the program where the
document's plugins were not hosted, and a mistake nobody would notice until
they listened to the file. `fontelle_app::bounce` is the three steps the
studio's `rebuild_graph` takes (open what the document names, build the graph
around it, play) plus the one a headless run needs and a window never does:
when the graph is dropped every `PluginNode` parks its processor, so
`PluginRack::close_all` can retire every plugin rather than leak it at exit.
A plugin the project names that is not installed leaves its channel silent
and **says so** — §17.4's rule for a missing file, applied to a missing
plugin — and so does a settings file that would not read, because a rack
looking in the wrong folders reports "not installed" and that is the wrong
diagnosis. Three tests in `fontelle-app/tests/plugin_hosting.rs`; the
headless *play* path got the same rack while there.

**Heard, in the real binary.** A project whose one channel plays the fixture
sine, `--open ... --render-wav` against an isolated `XDG_CONFIG_HOME`:
120 000 frames at a 0.354 peak, where the same command wrote a silent file
before.

### LV2 sidechains

LV2 declares a sidechain with a **port property**, `lv2:isSideChain`, on an
audio input rather than with a separate port the way CLAP does. `lv2::open`
reads it off the plugin's own Turtle and folds those inputs into the same
second port a CLAP sidechain presents, so `HostedPlugin::takes_key` is one
question with one answer whatever the format and everything upstream — the
key chips on the plugin panel, `EffectSlot::effective_key`, the scheduling
edge — was already right. The processor keeps its input buffers **main-first**
and connects them in *port* order, which is livi's contract; the key is
written every block, silence included, so one handed over once does not go on
ducking. The fixture gain grew a `lv2:isSideChain` port at index 4 and
**ducks** by it sample for sample: a key that is heard rather than detected,
so a host that put the bus on the key port would be heard silencing itself —
which is exactly what four older tests said the moment the port existed and
the host had not been taught about it yet.

### Slides into a hosted instrument

A slide note names only the key it goes *to*, and a plugin will not say what
it is playing, so `PluginNode` now keeps the score's own answer: a fixed
table (no `Vec`, INVARIANT 1) of every note it has started and not ended,
with where that note's pitch is. A slide bends every note in its voice
context — a slide under a chord moves the chord, as
`fontelle_core::Sampler::slide` has always done — gliding at **block rate**,
which is the rate `Voice::advance_glide` moves at and for the reason it
gives. The pitch reaches a CLAP plugin as a `Tuning` note expression on the
key, in semitones and unbounded, so a slide of an octave is an octave; an
LV2 or bridged plugin gets a **channel bend** clamped to two semitones,
because MIDI has no per-note pitch and MPE is not spoken here. A note that
ends puts its own bend back and any note still bent is told its pitch again,
so a channel-wide plugin does not start the next note bent. Five tests
across `fontelle-host` and `fontelle-engine/tests/plugin_nodes.rs`.

### The wheels, into a built-in instrument (§7.4's half of item 3)

The hosting pass carried a controller into whatever language a plugin speaks
and left this half open, so a soundfont played from a keyboard heard the
notes and nothing of the hand playing them: `ModSource::ModWheel`,
`PitchBend` and `Aftertouch` had been in the matrix since it was written and
read as a flat zero.

`fontelle_core::Performance` is the channel-wide, **live** half of playing —
read at render rather than captured at note-on, beside the channel's own pan
and for the same reason: a wheel has to move what is already sounding. Two of
the three are matrix sources and nothing else, because where a wheel goes is
the patch's decision and inventing one would be a mapping nobody asked for.
The **bend** is the exception: it is applied to the note's own pitch over
`VoiceConfig::bend_range_semitones` (two by default, which is what SF2's
always-present pitch-wheel modulator amounts to) *and* readable as a source,
because every keyboard bends pitch and a patch should not have to wire a
route for it. A reset lets go of all three — a transport stop that left a
bend on would start the next note bent.

`SamplerNode` translates the three payloads, mapping **CC 1** and dropping
every other controller: the same rule the host follows for a CLAP-only
plugin, and for the same reason. And imported soundfonts get SF2 2.04
§8.4.2's default modulators **2 and 6** — the mod wheel and channel pressure
each scaling the vibrato LFO's pitch by 50 cents, through `ModRoute::via`,
which is the case §7.5 says `via` exists for. That is why a wheel adds
vibrato on any soundfont in any player, and Fontelle's did nothing.

Seven tests in `fontelle-core/tests/performance.rs`, three in
`fontelle-assets`, and two end to end in `fontelle-app/tests/live_midi.rs` —
raw MIDI bytes into a `MidiRouter`, out as a bent note.

**Two test bugs worth writing down**, both caught by the tests-first rule
doing its job rather than by luck. The first draft of the bend test measured
eight blocks, where a whole tone is worth *one* crossing more than nothing —
and it passed before the feature existed. Sixty-four blocks make two
semitones twenty crossings, and the test then failed honestly. The second:
the master limiter looks ahead **two milliseconds**, which is 96 samples of a
128-sample block, so the block a change lands in still carries most of a
block of what came before it and its peak is that. A level read directly
says a wheel took a block longer than it did; `Callback::settled` renders one
block and measures the next.

### Bridge ABI 3: the rest of a performance

The table carried notes and nothing else of a performance, so a bridged
instrument was the one kind that could not be played with a wheel — and a
VST3 bridge is the whole point of the seam. `controller`, `pitch_bend` and
`channel_pressure` are on the end of the table now, channel-wide and in time
order with the notes, and **performance rather than automation**: a knob the
document moves still arrives through `set_param` by its own id. A slide
converts to the bend that reaches it, as for LV2; per-note pitch is what an
ABI 4 would add, once there is a bridge that wants it, which is §8.4's
standing instruction about not adding abstractions before there is a host for
them. `fontelle-testbridge`'s sine scales its level by the wheel, ducks under
pressure and bends two semitones — the same shape both in-tree fixtures have,
so a bridge that swapped two of them is told apart from one that got it
right. Three tests in `fontelle-host/tests/bridges.rs`.

### The clip's end, and what it means

> *"clip endings don't actually cut the clip short audibly right now it
> keeps playing"*

Dragging a clip's right edge in made the block shorter and changed nothing
about what came out of the speakers: a note written past the new end still
sounded, and a note crossing it still rang to its own length. Only **looped**
clips clamped, because a loop whose last pass runs longer than the others is
obviously wrong — but the rule was never about looping. An audio clip's
placement has always been `clip.start .. clip.start + clip.length`; this is
the note half of the same rule, and there is one rule now: a note that starts
at or after the end does not sound, and a note that runs past it is **cut**
there. Cut, not silenced — the note-off lands at the clip's end and the
instrument's release rings out from it, because stopping the sound dead on a
boundary is a click. Eight tests in
`fontelle-sequencer/tests/clip_bounds.rs`.

**And it loops cleanly from there**, which was the second half of the
instruction. Inside a loop the cut happens at the end of every *pass*, not
only at the clip's end: a note written longer than the period used to ring on
through the passes after it, so the second pass played over the first one's
tail and the third over both. A loop that gets thicker as it goes is not a
loop. Four more tests in the same file.

**A clip does not grow to contain what is put in it.** The first attempt at
this made drawing, dragging or stretching a note past the end grow the clip —
so that a note drawn out there would still sound — and that was corrected:

> *"the clip should not grow to contain what you put in it it should just cut
> off wherever you put the ending to be and then cleanly loop from that
> point"*

So the growth is gone and the end is the end. What the correction costs is
visibility: a note written past the end is silent, and nothing said so. The
roll **shades the grid past the clip's end** now
(`canvas::roll_past_end`, five tests on the geometry, `Session::clip_length`
for where the number comes from) — the same "nothing here sounds" ink the
dead rows of a drum kit use, and two more tests over the headless renderer's
pixels. **Looked at**, not only measured: `FONTELLE_UI_DUMP` writes the frame
out, and the striped grid stops where the clip does.

That pixel test was wrong first, and it is worth recording why: both of its
samples landed **exactly on grid lines**, which are drawn over the shade, so
it read `grid_line_sub` on either side of the boundary and would have passed
with no shade at all. Sampling five pixels off a snap boundary is what makes
it a test of the fill.

**Heard, in the real binary.** The same project rendered twice through
`--render-wav`, one clip two beats long and one eight: the sound stops at
1.00 s and at 4.00 s. Before, both played for four seconds.

Two session tests drew a note at beat eight of a three-and-a-half-beat demo
clip and had been passing by playing something the arrangement does not show.
They draw inside the clip now, which is what they were always claiming to
test.

### Delay compensation (TDD §5.5)

`AudioNode::latency_samples` had existed since the graph did and **nothing
read it**. Two things were wrong because of that, and the first is the one a
person would notice.

**An insert that looks ahead combed against its own dry.** A gate with
look-ahead delays what it outputs; the dry the mix control blends back in was
the block as it *arrived*, so the two summed into a comb filter. Not half the
effect — a different effect, and on a gate doing nothing at all it should be
inaudible. `EffectNode` delays the dry by the same look-ahead now, and a
fully open gate at any mix is the wire it claims to be, sample for sample.

**And a look-ahead track was late against every other track.** The
compensator is `DelayNode`: a fixed number of samples of nothing, no feedback
and no mix, which is what makes it not `fontelle_fx::Delay`. `realise` works
out from the document — before a node is built — what arrives at each track's
bus and what leaves it, then holds back every track quicker than its
siblings, and holds the **sources** back once at the one point where a bus
carries them and nothing else. `Realised::latency_samples` is measured off
the *built* graph rather than added up from the document, so the number the
user is told includes nodes the builder did not know about (the master
limiter's two milliseconds among them).

**A plugin's own number now reaches it.** `HostedPlugin::latency_samples`
reads CLAP's `latency` extension when the plugin declares one; the rack
carries it in `PluginWiring`, `PluginNode` reports it, and the chain sum uses
it — so a mastering limiter or a linear-phase EQ on one track no longer drags
that track behind the mix. LV2 answers through an output control port this
build does not read, and a bridge's table has no entry for it; both report
zero, which is what a host that cannot ask has to assume.

**Two things are deliberately not compensated, and say so where they
happen.** A plugin *instrument* that reports latency is late against the
other channels on its track, because every channel adds into one shared bus
and holding one back would hold back everything already in it — the fix is
per-source buffers. And the **live bypass** switch moves a track by its
insert's latency until the next rebuild, because a bypass that still delayed
would not be a bypass.

Five tests in `fontelle-engine/tests/latency.rs`, three in
`fontelle-engine/tests/inserts.rs`, three in
`fontelle-app/tests/latency_compensation.rs` (which measure where a click
lands, not what a number says), and one each in `fontelle-host` and
`fontelle-app` for the plugin's declared number.

### What is still open after this

- **The VST3 bridge itself.** A separate private repository against
  Steinberg's SDK; this tree holds the ABI (v3), the loader, and a
  SDK-free bridge that proves both. Nothing else in `docs/plugin-compatibility-plan.md`
  remains.
- **Per-source delay compensation.** A plugin instrument that reports latency
  is still late against the other channels on its track; every channel adds
  into one shared bus, so the fix is giving sources buffers of their own.
- **LV2 latency**, which is an output control port designated `lv2:latency`
  that this build does not read, and latency over the bridge ABI.
- **Per-note pitch over the bridge ABI**, waiting on a bridge that can carry it.
- **Copying or relativising a sample an LV2 plugin loaded** (§17.4's import
  prompt, for plugin-loaded files) — a real design decision, deliberately
  not made here.
- The three items in `docs/handoff.md` §3 that are about the *window* rather
  than about plugins: input monitoring has never been heard through a
  speaker, the arrangement's newer gestures have not been driven by hand,
  and fade handles show only on the selected block.

## 2026-09-05: the compatibility plan, top down — and two more crashes

> *"its crashing the daw a lot for several plugins when opening their
> instrument window and also when adding a plugin instrument i cannot hear
> my other instruments anymore at the same time"*

`docs/plugin-compatibility-plan.md`'s items 1–3 landed whole, the first
half of item 4 with them, and the two faults in the report first — both
were the host's, both had a core dump, and neither had a test until now.
Every piece below was written test-first against the two fixture bundles,
which grew a plugin each and learnt four things between them.

### The two faults

**"I cannot hear my other instruments."** `PluginNode` wrote the plugin's
block *over* its bus. The graph clears every bus once a block and each
source adds into it — that is what lets several channels share a track —
so a plugin instrument silenced every channel scheduled before it on the
same bus, which on a project whose channels all go to the master was every
other instrument. It renders into scratch of its own now (sized in
`prepare`, INVARIANT 1), takes the channel's level and pan there, and
**adds**. Two tests in `fontelle-engine/tests/plugin_nodes.rs`, the second
of which checks the channel's gain no longer scales what the others wrote.

**"Crashing when opening their instrument window."** Three of today's
core dumps were the same address — zero — reached from
`HostedPlugin::tick_editor` with **JuceOPL.lv2** loaded. LV2 says a UI's
`port_event` *"may be NULL if the UI is not interested in any port
events"*, and JuceOPL's is; `lv2_raw` types the field as a plain function
pointer, which in Rust cannot be null, so reading a NULL through it is
undefined behaviour and the release optimiser folded the later null check
away. The studio called address zero the first time a knob moved.
`lv2_ui` reads the descriptor through its own `#[repr(C)]` struct with
`Option` wherever C may be NULL — `instantiate`, `cleanup`, `port_event`,
`extension_data`, and the idle interface's `idle` — and the fixture bundle
gained a fourth plugin, **`deaf`**, whose editor has neither `port_event`
nor `extension_data`: `an_editor_with_no_port_event_is_not_told_about_a_knob`.
The gdb route, for next time: `coredumpctl dump <pid> -o core`, then
`gdb -batch -ex bt` on it — systemd's own unwinder stops at a null PC, gdb
does not — and `x/14i $pc-48` in the caller shows which call it was.

The fourth dump was SpectMorph's **LV2** editor dying inside its own
constructor at `open_editor`. It could not be reproduced: both SpectMorph
rows (CLAP and LV2) open and draw in the studio now, alive, no new dump.
The likeliest reading is below under the idle gate — an editor handed a
plugin that had never run — and it is recorded as a reading, not a fix.
While there: the picker lists the two SpectMorphs as two identical rows,
which is a fact about the row and worth a format tag next time.

### 1. LV2 state — the sampler's file survives a save

`state:interface` is on the **instance**, the instance rides in the
processor, and LV2 forbids calling `save` while `run` executes. The design
decision the plan named was made the way it recommended: **through the
bay**. `ProcessorBay` carries a request now — `recall(timeout)` raises it,
the node sees it at the top of its next block and parks the processor
(`try_park`, never waiting), the main thread takes it, reads the state
with the processor in hand (`HostedPlugin::snapshot_with`), and parks it
back for the node to pick up. The silence is bounded and measured: one
block to hand over, one to take back, and however long the read takes —
`an_lv2_plugins_own_state_is_saved_while_the_graph_is_playing_it` renders
on a thread while the main thread snapshots, and asserts the counter it
reads came off the running instance, that the snapshot took under half a
second, and that the processor went back and kept running. A recall
nobody answers — a stopped audio thread — gives up after 250 ms and
withdraws its request, and the snapshot then carries what the plugin was
last *given*, with a message. Before a plugin has run there is no
instance to ask, so a restored state is **kept** and applied the moment
the instance exists (`Lv2Plugin::activate`, before its first block —
the one moment LV2 lets a host restore any plugin without asking whether
it is thread-safe), and read off the instance again on `deactivate`.

The interface itself is hand-rolled in `fontelle_host::lv2_state` rather
than borrowed from lilv's state API, which serialises to Turtle and owns
the path mapping against a directory: this program keeps one blob in one
JSON file and decides for itself what a path means. `Lv2State` is the
list of properties a plugin stored (key and type as **URIs**, since a
URID is a number one process agreed on), encoded as its own small
length-prefixed form, base64 in `project.json` exactly where a CLAP blob
goes. It is public and decodable, because *"the sampler forgot its file"*
and *"the sampler never stored one"* look the same from outside.

**Paths — the second decision.** `state:mapPath` and `state:freePath` are
offered, because a sampler that finds them missing may refuse to store
its file at all, and `abstract_path` answers **identity**: the abstract
path is the absolute one. That is the rule an audio clip referenced in
place already follows (§17.4's headless default: *reference, never copy*)
— a project names the file by where it is. Making the abstract form
project-relative, or copying the sample into the project, is the import
prompt's decision (§17.4's remembered-default-with-an-escape-hatch) and
belongs to the pass that builds one for plugin-loaded files; `makePath`
is not offered because nothing here has a folder to hand a plugin to
write into. The fixture gain keeps two properties no port exposes — a run
counter (`atom:Int`) and its own bundle folder as an `atom:Path` that
goes through `mapPath` both ways — and **refuses a restore whose path
did not map back**, so the round trip is asserted by the plugin itself.
Seven tests in `fontelle-host/tests/lv2.rs`, three in `tests/bay.rs`, two
in `fontelle-app/tests/plugin_hosting.rs`.

### 2. Every port, and sidechains

The "every port" half of this item had already landed with the crash fix
above it; what was left was the key. CLAP has no sidechain flag — a
sidechain is any input port that is not the main one — so `PortLayout`
now names it (`key()`), `HostedPlugin::takes_key` says whether there is
one, and `HostedProcessor::process_insert_keyed` puts a mono key on every
channel of that port (and silence there on every other block, so a key
handed over once does not go on ducking after its tap is gone). The
extra **outputs** stay dropped, written down as such: summing a drum
plugin's individual outs into its main pair would make it twice as loud
as it is. The fixture gain grew a mono sidechain input beside its main
pair and a mono aux output that carries the key back out — so a host
that summed the aux would be heard putting the key on the bus — and
**ducks** by the key sample for sample, a key that is *heard* rather than
detected so a test can say exactly what it expects.

Upstream it rides what the compressor already had: `EffectSlot::key`,
`key_listeners`, the `KeyTap` the source track's node fills, the
scheduling edge. Two rules moved. `EffectSlot::effective_key` says a key
on a **plugin** slot is always an edge — whether the plugin has a port
for it is the host's knowledge, and an edge that feeds nothing only
orders the graph — and `SetInsertKey` lets a plugin slot be keyed
(`fontelle-model/tests/plugins.rs`, where the test that said the opposite
now says this). `realise` hands the tap to `PluginNode::with_key`, which
copies it into a buffer sized in `prepare`. The plugin's panel offers the
same key chips a compressor's does when the rack says the plugin takes
one (`insert_view`), so the routing is one click away rather than a
field only a test can set. LV2's `lv2:isSideChain` port property is the
next thing here; an LV2 insert given a key ignores it, and a test says so.

### 3. Pitch bend, mod wheel, aftertouch

`EventPayload` gained `Controller { controller, value }`, `PitchBend {
value }` and `ChannelPressure { value }`, documented as what they are —
**performance events, not automation**: a `ParamValue` names one of this
program's controls by its §8.2 address and comes out of a lane or the
learn table, while these are the raw fact that a hand moved a wheel,
carried whole for the instrument to interpret. The router forwards every
controller, bend and pressure it decodes on the current target; the
sustain pedal stays its own (holding notes is the router's bookkeeping,
not the instrument's) and a program change still goes nowhere. Six tests
in `fontelle-midi/tests/router.rs`.

Into a plugin, in the language its note port speaks —
`fontelle_host::NoteDialect`, read off the port's declared dialects with
MIDI winning when it is *supported*, whatever the plugin prefers, because
the preference is about notes and MIDI is the honest carrier for a wheel.
LV2 is three more bytes in the atom sequence. CLAP with MIDI is a
`MidiEvent` on the note port. CLAP without is the nearest **note
expression** on every note: the wheel as vibrato, aftertouch as
pressure, the bend as two semitones of tuning; 7, 10, 11 and 74 as
volume, pan, expression and brightness; anything else dropped rather
than invented onto a parameter. `fontelle-testplug`'s sine ships
**twice** for this — `SinePlugin<true>` speaking MIDI beside CLAP and
`SINE_CLAP_ONLY` speaking CLAP alone — and both sines (and the LV2 one)
scale their level by the wheel, bend two semitones, and *duck* under
pressure, the opposite of the wheel so a host that sent one as the other
is told apart from one that got it right. Twelve host tests across
`hosting.rs` and `lv2.rs`, two on the node. Bridged plugins get none of
this yet: the ABI carries notes and nothing else of a performance.

Not done, and said where: a slide into a hosted instrument is now
*expressible* as a tuning expression, but a `NoteSlide` names only the
key it goes to and the node keeps no table of what is sounding to bend
from; and what a `fontelle-core` voice does with a wheel is §7.4's
question, for its own pass.

### 4. Bridge ABI 2 — the editor half

Items 1–3 were finished, tested and clippy-clean, so the first half of
item 4 followed as instructed. `fontelle-bridge-abi` is **version 2**:
five entry points at the end of the table — `has_editor`, `open_editor`
into an X11 window id (writing the size it wants), `close_editor`, a
per-frame `tick_editor`, and `resize_editor` — hanging off the same
`PluginWindow` the two hosted formats use. The host reads every
parameter back off the bridge after each tick, which is how a knob moved
in a bridged editor reaches the document, since the ABI has no parameter
events. `fontelle-testbridge`'s gain has no face and its sine has one
that draws nothing and writes a level on its first tick — the same trick
the LV2 fixture editor plays — so both answers are fixtures. Three tests
in `fontelle-host/tests/bridges.rs`. The bridge itself is still the
separate repository and is not written.

### Counts that moved, on purpose

The CLAP fixture bundle holds four plugins now and the LV2 one four, so
the scan counts in `scanning.rs`, `lv2.rs`, `plugin_hosting.rs` and
`plugin_ui.rs` moved with them — the tests doing their job.

### Seen in the real window — and two more things it found

Driven on the nested `Xwayland :99` with an isolated `XDG_CONFIG_HOME` so
nothing touched the real settings or projects folder: *+ Add instrument*
→ *Plugin…* → `x12` → *LSP Multi-Sampler x12 Stereo — LSP LV2*, *Open
instrument*, LSP's own editor at 1100×675, its file dialog steered to
`/usr/share/sounds/alsa/Front_Center.wav`, OPEN. **Nothing loaded.** The
same clicks in the `plugin_editor` probe loaded and drew the waveform, and
a new `PROBE_THREADED=1` mode (the processor on a thread of its own, as
the studio runs it) loaded too — so the difference was the studio. A
trace switch, `FONTELLE_ATOM_TRACE`, said it in one line: *editor wrote
88 bytes to port 75; plugin ran 854 blocks; to plugin 4/0 taken*. The
editor was talking; the plugin had stopped listening 2.5 seconds after
it was activated, and stayed stopped.

**The idle gate.** §6.3 says a stopped transport does not process the
graph, and `IdleGate` is what makes that true: with nothing held on a
keyboard, nothing ringing and no input monitored, the audio thread runs
no nodes at all. An LV2 editor talks to its plugin *only* through `run` —
the file it was handed rides an atom the plugin reads at the top of a
block, and *"I loaded it"* comes out of one — so a gate that slept under
an open editor was a sampler that could never be given a sample while the
song was stopped, which is exactly when one is. A fifth reason to be
awake: **attended**. The window writes `Transport::set_attended` from
`tick_plugin_editors`, the callback reads it every block, and
`an_attended_plugin_keeps_the_graph_awake` pins it. With that the load
went through — *plugin ran 13059 blocks; to plugin 3/3 taken; to editor
4/4* — and the waveform drew. This is also the likeliest reading of the
SpectMorph LV2 dump: its editor reads a plan its plugin builds in `run`,
and that plugin had never run.

**Saved, and read back.** Ctrl+S; `project.json` carries a 570 KB blob of
4948 properties, among them `sf_0_0` as an `atom:Path` holding
`/usr/share/sounds/alsa/Front_Center.wav` — identity-mapped, as decided.
`[bay] recalled after 3.2ms` and `5.3ms` in the same log are the studio's
own autosave fetching the running instance's state through the bay: the
silence item 1 was told to bound, measured in the window.

**Reopened cold, and silent.** `--open <project> --window` came up with
the sampler channel silent and *Open instrument* showing an empty panel:
the window's first graph is built in `main.rs` before the session exists
and **without a rack**, and nothing rebuilt it. A project that names
plugins is now hosted on the session's first `pump` — after every builder
has run and the settings' plugin folders are known —
`a_project_that_names_a_plugin_hosts_it_before_any_edit`. After that the
editor opened showing the file with nothing sent from the editor
(`to_plugin=0`: it came from the state), and a note on MIDI 57 was heard:
`parec --monitor-stream` on Fontelle's own PipeWire stream, play pressed
in the window, a 0.26-peak burst from 2.9 s to 4.2 s — the 1.4-second
sample. Item 1's *"done when"*, end to end.

**Not done, and named:** the CLI's `--render-wav` bounce has no rack and
renders plugins as silence; that is its own pass.

## 2026-09-05: three plugin faults, from one report

> *"a lot of drum synth plugin keeps making the daw crash and like
> spectremorph for example i tried it seemed to try loading a custom ui and
> then everything crashed and lots of them dont seem to have custom ui and
> the ones that seem to be trying to open a custom ui most often are just
> closing their window as soon as it opens"*

Reproduced with `fontelle-host`'s `plugin_editor` example against the
installed plugins rather than in the window, which is what made each of the
three separable. All three are fixed, tests first, and every one of them
was the host's fault.

### 1. The crash: a CLAP plugin is handed **every** port it declares

`ClapProcessor` passed one input port and one output port — the main pair —
whatever the plugin had declared. The OneTrick drum synths declare eight to
eleven output ports (individual drum outs), and nih-plug, which they are
built on, reads its auxiliary ports straight off the end of whatever array
the host gave it (its bounds check is `>` where it should be `>=`). Handed
one descriptor, SIMIAN2 and URCHIN read a second off the heap and cleared
memory at address `0x13` in their **first block**, before a note was played
— so the DAW died the moment one landed on a channel, editor or no editor.
Surge XT declares three output ports and had only been surviving by luck.

`read_audio_ports` now reads the whole layout (`PortLayout`: channel count
per port, and which is main), `ClapProcessor` keeps one buffer set per port
and hands them all over every block; the bus is still copied to and from
the main port alone, the rest are silent in and dropped out. The sine
fixture declares a mono *sub* port beside its main pair and refuses to
render when handed fewer ports than it declared —
`every_port_a_plugin_declares_is_handed_over` in
`fontelle-host/tests/hosting.rs`. Before: memset to `0x13`; after: forty
blocks and a drum hit at -6 dB from each of SIMIAN2, URCHIN and B-BOI.

### 2. The window that closed itself: a `false` from `show` is advisory

clap-helpers' default `guiShow` returns **false** unless a plugin overrides
it, and plugins that put their window up in `set_parent` — SpectMorph, the
OneTrick series — never do. `open_editor` read that false as a refusal,
destroyed the editor it had just embedded, and the window went with it:
*"closing their window as soon as it opens"*, exactly. Once `set_parent`
has succeeded the editor exists; what `show` says after that is ignored.
`fontelle-testplug` gained a third plugin, `FacePlugin` (a note effect, so
it is on neither of the menus' lists), whose `show` answers the way those
do — `a_plugin_whose_show_says_no_still_has_its_editor`. The fixture bundle
holds three plugins now; the scan tests that counted two say three.

### 3. The editors that never appeared: `instance-access` for LV2 UIs

drumsynth's editor printed *"Host does not support instance-access, cannot
use UI"* and refused to instantiate. That feature — the plugin's own
`LV2_Handle`, handed to its UI — is discouraged by the specification and
required by every DPF-built editor there is: Cardinal, Dexed, drumsynth,
drumgizmo, geonkick and some two dozen more of the bundles on this machine.
`Lv2Plugin` now publishes the running instance's handle (`activate` writes
it, the `Lv2Processor`'s drop clears it, compare-and-swap so a newer
instance's handle is never clobbered), and `Lv2Ui::open` passes it as the
feature when there is one. **The rule that makes it safe:** the pointer a
UI took at instantiate cannot be revoked, so an LV2 editor is closed before
the instance it was handed can go — `HostedPlugin::activate` closes it
first, and `PluginRack::retire` already closed editors before dropping a
plugin. The fixture gain's UI checks the handle really is a `Gain` by a
magic word and greets with `HELLO_FROM_THE_INSTANCE` instead of
`HELLO_FROM_THE_EDITOR`; three tests in `fontelle-host/tests/lv2.rs`.
drumsynth's editor opens and draws now (682x320, its own size).

**Still true, and not a bug here:** an LV2 plugin whose UI is Gtk or Qt
(drumkv1, synthv1, Calf's none) keeps the generated panel — see the
`lv2_ui` module note on why this is not suil.

**The probe route, for next time:** `cargo run -p fontelle-host --example
plugin_editor -- <bundle>` on `DISPLAY=:1`, and when it dies, `coredumpctl
-1 info` for the frames and `gdb -batch` with a breakpoint on
`clack-host`'s `process.rs:504` to dump the `clap_process` it was handed.
That is how the `0x13` was found, and it took a fraction of the time
driving the window would have.

## 2026-09-05: stars — favourites in every picker

> *"make it so that i can favorite (star) plugins, instruments, effects,
> etc. so that the favorites are always the most visible (highlighted
> appearance) and there should be a favorites section at the top of every
> dropdown basically that includes all of your applicable favorites if there
> are any."*

Every row that names a thing you can add — a built-in effect on the mixer's
*+ fx* menu, a kind on the rack's *New instrument* / *Change instrument*
menus, and every plugin in the picker — now has a **star at its right end**.
Pressing the star toggles it; the menu stays open, rebuilt where it was and
scrolled where it was, so starring three effects is three presses and not
three trips. A starred row is drawn **lit** (an accent wash under it, a
filled star) wherever it appears, and whenever anything in a menu is starred
a **Favorites** section goes first, under the heading and above a rule,
holding every applicable favourite. The full list follows unchanged.

**A starred plugin is one press away, not two.** A favourite plugin effect
sits in the *+ fx* menu's favourites section itself, and a favourite plugin
instrument in *New instrument*'s — the row behind *Plugin…* was the reason
to star it. A favourite that names a plugin not installed on this machine is
left out rather than offered as a row that would do nothing. Those two menus
scan for plugins once, on opening, **only if** some favourite is a plugin:
somebody who never starred one never waits for a scan they did not ask for.

**Where it is kept.** `fontelle_types::Favorite` — `Effect(EffectKind)`,
`Instrument(InstrumentKind)`, `Plugin(PluginKey)` — in the **settings file**
(`Settings::favorites`, format version **4**), not the project: a favourite is
a fact about the person, and the same reverb is a favourite in every song.
Written as `{"effect":"Reverb"}` / `{"plugin":"clap:com.u-he.diva"}` so the
file stays readable; saved at once, like a folder choice; and not an edit
(the project does not go dirty for it). A plugin is named by its `PluginKey`,
so it stays starred after being reinstalled somewhere else.

**How it is built** — `crates/fontelle-ui/src/canvas/favorites.rs`. Each of
the three menus is one pure function returning **the rows and what each row
means** as a pair (`effect_menu_rows` → `EffectRow`, `instrument_menu_rows` →
`InstrumentRow`, `plugin_picker_rows` → `PickerRow`); the window builds the
menu from one half and answers a press from the other, so they cannot
disagree about which row was the reverb. This retired the mixer's bespoke
`EffectMenu` — *+ fx* is a `ContextMenu` like every other menu now
(`MenuTarget::AddEffect`), which is what let it scroll, star and carry a
section without a second implementation of all three. `MenuEntry` grew
`star: Option<bool>`; `ContextMenu::star_rect` and `context_menu_star_hit`
divide a row between its caption and its star, and `context_menu_hit` no
longer answers for the star's part. The star is `Icon::Star` /
`Icon::StarFilled`, drawn as paths like every other icon. The host seam is
two methods on `StudioHost`: `favorites()` and `toggle_favorite()`, and
`PluginListing` now carries its `key` so the window can say which rows are
starred (it still *chooses* by position — INVARIANT 2).

Tests, written first: `fontelle-types/tests/favorites.rs` (the written
form), `fontelle-app/tests/favorites.rs` (the toggle, the file, the session,
not-dirty), `fontelle-ui/tests/favorites.rs` (the three menus' rows with and
without favourites, the star's geometry, a greyed row's star still answering,
a star scrolled out of sight not answering, a starred menu being wider).

**Not done, on purpose:** the soundfont browser's presets. They are a panel
with search headings, not a dropdown, and starring one is a browser feature
of its own (a *Favorites* heading in the bank) rather than a menu row — asked
for next, it would go through the same `Favorite` type with a fourth variant.

## 2026-09-05: the LV2 half of a plugin's own face

> *"close the gaps that still stand"*

The gaps were: LV2 plugins got the generated panel, so an LV2 sampler could
never be handed a file; and the autotune "created no audible or visual
difference". Both are the same gap, and it is closed.

### `fontelle_host::lv2_ui` — an LV2 editor, hosted

Found through lilv off the plugin's own Turtle, `dlopen`ed, matched **by URI**
— LSP ships a single `lsp-plugins-lv2ui.so` answering for three hundred
plugins, so a host that takes the first descriptor opens the wrong editor, and
the walk to find the right one has to be longer than 64 — then instantiated
with `ui:parent` pointing at the very same `PluginWindow` a CLAP plugin gets.
`idle` runs off the same per-frame tick; `ui:resize` is how it asks for its
window; a control port moved in the studio reaches it through `port_event` and
what it writes comes back on the parameter wire.

**The one rule that makes the loop stable** is at the bottom of `Lv2Ui::tick`:
what the editor wrote is never sent back to it. Without that, a knob under the
mouse fights the host for its own position.

**Not suil.** Of the seventeen bundles installed here that ship a UI,
seventeen ship X11 — so suil would only add Gtk and Qt, and a wrapper for
toolkits nothing here uses is a dependency for nobody. And the Calf half of
what I said last time was wrong: Calf ships **no UI at all**, not a Gtk one. It
keeps the generated panel because it has nothing else, which is a fact about
Calf rather than a gap in here.

### `fontelle_host::atom` — because a float cannot say "load this file"

An LSP sampler's whole state is a file it was handed, and it is handed one as a
`patch:Set` **atom** written by its editor. A host that carries floats and
drops atoms opens a sampler's editor onto a sampler that can never be given a
sample — which is exactly the shape the gap had.

Two fixed-capacity rings, allocated once, `try_lock`ed rather than waited on
(the `ProcessorBay` trade), carrying whole atoms between the editor on the main
thread and the plugin on the audio thread. The subtle part is *when*: the
editor's messages go into the atom sequence **as the block that just ran is
emptied**, so they sit at frame zero ahead of the next block's notes. Appending
them at the top of the next `run` would put an atom at frame 0 after a note at
frame 100, and a plugin is entitled to stop reading a sequence at the first
event out of order. The cost is one block of latency on a command: 2.7 ms.

### What it was tested against

`fontelle-testlv2` grew **two editors and a third plugin**, so the host's half
is exercised with no display at all:

- the gain's editor writes a parameter on its first idle *only if it was given
  a window*, and mirrors whatever it is told about one port onto another — so
  one assertion covers found, loaded, parented, driven, told, and heard back;
- the sine's editor speaks only in **atoms**: it sends a MIDI note-on through
  `atom:eventTransfer`, and the sine now has an atom output port to answer on,
  so the test asserts a *sound* and then asserts the reply arrived;
- and a third plugin that ships no editor at all, because "the host must
  survive a plugin with no face" is Calf's whole suite and deserved a fixture
  rather than an assumption.

31 tests in `fontelle-host/tests/lv2.rs`, 5 more for the ring itself.

### And on the real ones

- **x42-Autotune** (the one from the report): editor opens, asks for 615×108
  through `ui:resize`, and draws its keyboard, Mode, Tuning, Bias, Filter,
  Corr. and Offset.
- **LSP Multi-Sampler x24 Stereo**: editor opens at 1100×675 with *"Click or
  drag to load"* — driven in the studio window, from *Open instrument* on its
  channel. `AtomPipe::carried` says 1 atom to the plugin and 1001 back in eight
  seconds, so the wire the file loading rides on is live and measured.

**Bridged editors stay open**, and deliberately: the ABI has no GUI entry
points, and adding them now would be a speculative abstraction against zero
consumers — the VST3 bridge is not written. The shape when it is, is recorded
in the TDD.

## 2026-09-05: plugins that make a sound, and show their own face

> *"i added plugin instruments but cant hear them and they arent really
> displaying cleanly in the instrument menu. shouldnt these also be showing the
> custom plugins own display in their windows not a auto made one from the
> parameters."*

Five things, and the first one is why the rest looked worse than they were.

### 1. Every plugin parameter was being set to its minimum

Fontelle seeded its parameter wire from CLAP's `param_info.default_value` and
then `activate` marked the lot to be sent, so the first block a plugin ever
rendered carried "set every parameter to its default" as the host understood
it. **Surge XT reports `default_value` as zero for all seven hundred and
seventy-five of its parameters.** Its `get_value`, on the freshly instantiated
plugin, returns the real setting — Global Volume at -2.03 dB, Polyphony Limit
at 16. So the host turned the global volume to -48 dB before a note was played,
and the synth was silent. The screenshot in the report says it outright:
*Global Volume -48.00 dB*, *Polyphony Limit 2*, every send at *-inf*.

`read_params` now asks the plugin what each parameter **is**, and falls back to
what its description **claims** only when it will not say. `fontelle-testplug`'s
sine grew an `Output` parameter that tells the same lie on purpose and that the
sine is multiplied by, so five tests fail without the fix and pass with it.

Measured on this machine: Surge XT went from peak 0.0 to peak 0.21 for a held
note through `realise_hosting` and `render_offline` — the whole app chain, not
just the host. Calf Organ (LV2) renders at 0.13, unchanged.

While in there: an instrument that declares audio **inputs** is now handed them,
zero-filled, instead of an empty port array. CLAP says the host passes as many
ports as the plugin declared and several plugins check.

### 2. A plugin instrument could not be replaced with another plugin

*"currently cant replace a plugin instrument with another plugin instrument."*

The *Change instrument* menu greyed out the kind you already were — right for
the four built-ins, where choosing again would throw away what you had edited,
and wrong for **Plugin**, which is not a kind you *are* but a promise to name
one. On a channel already playing a plugin, the one row that leads to the
plugin picker was the one row you could not press. It is the same mistake the
record button's mode menu made (*"i was locked out of the audio option"*) and
it has the same answer: mark it, do not disable it. `Plugin…` now carries an
ellipsis in both instrument menus, which are one list
(`canvas::instrument_menu_entries`) rather than two that can drift.

### 3. The plugin panel read badly

CLAP's `module` is a path and plugins write it with separators, so Surge's
groups were headed `/Macros/` and `/Global & FX/` — drawn raw, slashes and all.
They are turned back into words now. A caption too long for its ninety-two-pixel
cell is elided with a mark rather than clipped mid-word ("Polyphony Limi" reads
as a misspelling; "Polyphony Limi…" reads as a name that did not fit). A
parameter the plugin says is read-only is no longer drawn as a knob you can
turn — it is still automatable and still saved, since both go by the plugin's
own id.

### 4. A plugin's own editor, in its own window

This is the real answer to *"shouldnt these also be showing the custom plugins
own display"*, and it is new: `fontelle_host::gui`.

- **An X11 window, made with `x11rb`.** Surge XT's CLAP build supports *x11,
  embedded* and refuses floating, Wayland and floating-Wayland — the ordinary
  answer from anything built on JUCE. One process has one `winit` backend, so
  matching that with a `winit` window would mean moving the whole studio onto
  XWayland. Making just this window directly leaves the studio alone.
- **Three host extensions**, because a CLAP editor has no thread of its own: it
  repaints when the host fires the timer it registered (`clap_host_timer_support`)
  and sees a click when the host says its connection is readable
  (`clap_host_posix_fd_support`), and it asks its window to resize through
  `clap_host_gui`. `HostedPlugin::tick_editor` pays the first two once a frame,
  out of `about_to_wait`; `arm_deadline` holds the loop awake at 60 Hz while an
  editor is open, because otherwise the studio sleeps and the plugin freezes.
  **A window that opens grey and stays grey is those two calls missing.**
- **`PluginRack` owns the window**, beside the plugin, so `retire` closes the
  editor before the plugin it belongs to can go.
- A plugin with no editor — which is every LV2 and bridged one in this build —
  answers `false` and gets the generated panel, which is what the panel is for.

Driven in the real window to confirm it: added a Surge XT channel, right-clicked
it, chose *Open instrument*, and Surge's own editor came up at 1141×711 beside
the studio, drawn and live. `cargo run -p fontelle-host --example plugin_editor`
is the tool that does the same thing without the studio, and `PROBE_DUMP` writes
out what the plugin drew — taken off the window rather than off the screen, so
it works on this desktop, which has no screenshot tool that will co-operate.

### 5. Naming a project, and saving one that has no file

> *"when i make a new project i need to be prompted to name it and also when im
> not in a project yet, i currently cant save that blank no project into a new
> project ... if i try to save and theirs no project directory it can just make
> a new one ... giving you the option to name it and stuff."*

Ctrl+S on a studio with no file used to answer *"this project has no file yet —
open it with `--save <path>`"*, which is a sentence about the command line to
somebody who is looking at a window. Both gestures now put a **name prompt** up:
a menu whose heading carries what you have typed with a caret, Enter presses it,
Escape cancels. It is a menu rather than a new modal because a menu already lays
out, draws, hit-tests and takes the keyboard — the plugin picker has typed into
one since yesterday.

Underneath: `Session::save_as` and `Session::new_project_named`, and a name is
made **safe** before it is made unique — INVARIANT 10, since a name box is the
one place somebody can type `../../etc/passwd` and a project must not land
there. Eight tests in `tests/naming_projects.rs`.

## 2026-09-05: the plugin menu that was never drawn

> *"its still not seeing my plugins right now but i updated i installed them
> and restarted"*

It could see them. The scan was perfect — 370 plugins, 13 instruments, 357
effects, no failures, Surge XT among them — and the Settings tab said "370
plugins" at the bottom of the window while the report was being written.

**Clicking *"Plugin…"* opened nothing at all**, and a menu that refuses to
exist is indistinguishable, from the outside, from a program that cannot see
the plugins.

### One line, and it was a reasonable line

`canvas::context_menu_layout` refused to lay out any menu taller than the
room it was given:

```rust
if entries.is_empty() || bounds.is_empty() || width <= 0.0 || height > bounds.height {
    return ContextMenu::default();     // "better than a sliver listing two of six things"
}
```

That is right for the six-line menus the window had when it was written, and
it was even tested (`a_menu_that_will_not_fit_is_not_drawn`). Then 357 LV2
effects arrived: 359 entries at 22 pixels is a menu **7904 pixels tall**, the
guard fired, `open_menu` saw an empty menu and returned, and the row did
nothing. Plugin *instruments* were fine the whole time — 15 entries is 336
pixels, and it fit.

The rule is now **a menu longer than its room scrolls; only a menu with no
room for a single whole row is not drawn.** The old test passes unchanged: 4
entries in 10 pixels still has room for none of them.

### What the menu grew

- **Scrolling.** `ContextMenu` keeps its scroll, its row height and its
  content height, so `scroll_by` is a function of the menu alone rather than a
  re-layout needing the caller to have kept the anchor. An entry scrolled out
  of sight gets an **empty rectangle**, which is the one rule for what is
  showing: the renderer already skipped empty rows and `context_menu_hit`
  already could not land on one, so what is drawn and what can be clicked
  cannot disagree. The wheel over an open menu is the menu's before it is
  anything else's, three rows a notch.
- **Type to filter.** Sixteen screens of scrolling is not a picker, so the
  plugin menu filters as you type: a plain case-insensitive substring over the
  name *and* the vendor, re-laid-out in place on each keystroke. The heading
  carries the query ("Plugin effects — verb"), because a filtered menu that
  does not show its filter is a menu that has lost your plugins; an empty
  result says *"nothing matches"* rather than *"none found"*. Backspace and
  Escape do what they should. Only the plugin picker filters — six entries
  need no search box.

### Driven in the real window, which is the only place this was visible

None of it had a failing test and none of it could have: the geometry was
correct for every menu that existed. Verified through the nested X server and
XTEST (`seeing-fontelles-gui`): the mixer's *"+ Add effect"*, then
*"Plugin…"*, and the picker opens with 357 effects in a frame capped to the
window; the wheel reaches the last row (`Rescan plugin folders`); typing
`verb` narrows it to six reverbs from Calf, Dragonfly and LSP.

Two notes for whoever drives that harness next. `pkill -f` and `pgrep -f`
**match the driving shell's own command line** — three sessions were killed
mid-run before that was spotted; use a pid file. And the frame really is
stale: two screenshots here showed a menu that had already closed, and both
times a second grab a second later was right.

### Installing the plugins broke four tests, and they were right to break

The moment 370 real plugins existed on this machine, four tests that say *"the
rack lists what is in the folders it is given"* started counting them:
`PluginRack::folders` always walked the folders the formats nominate **as well
as** the ones it was handed, so `set_folders` never meant "only these". It had
simply never mattered, because the standard folders were empty.

`PluginRack::search_standard_folders(bool)` is the switch — on for the studio,
which has to find what an installer put where it was told to, off for a
fixture — and `Session::with_plugin_folders` turns it off with them, since a
test naming its folders means exactly those. `PluginRack` also lost its
`#[derive(Default)]`: `standard: false` is the one field a derive gets wrong,
and it would be a studio that finds no plugins at all.

This is `fontelle-testplug`'s argument arriving from the other end. A fixture
keeps a test off the developer's machine; this keeps the *machine* out of the
test.

**9 tests** (`fontelle-ui/tests/context_menu.rs`, 13 in the file now; plus the
rack's isolation switch), and `cargo test --workspace` is green at **3018**
with clippy clean at `-D warnings`.

`fontelle-ui/src/canvas/menu.rs` · `WindowApp::{relayout_menu,
menu_filter_key}` · `MENU_WHEEL_ROWS`.

### Still open

**A tooltip is drawn over an open menu** — the control underneath keeps its
tip while a menu covers it. Seen in two of the screenshots. Pre-existing and
cosmetic, and the same shape as a bug this window has had before.

## 2026-09-04: kits you can tell apart, a chip that says which, LV2, and a door for the formats that cannot come in

> *"the drumkits in the drum machine kind of all sound very similar and also not
> noticing much feedback for when i actually change a selection of kit
> visually like not much user feedback. also i agree with implementing LV2 for
> sure, and if vst3 and vst2 are legally murky we could always try and add it
> in a way that keeps it completely separate to the open source stuff and never
> gets included with it ... that way i can locally use vsts lv2s or clap
> plugins"*

Three things, in the order they were asked for.

### The kits sounded alike, and a test now listens

The report was measured before anything was changed: every kit's closed hat
had its spectral centre between 10 and 13 kHz and lasted the same 25 ms, every
kick sat between 80 and 110 Hz, and LinnDrum against Rock differed by fifteen
percent on one number. The old test (`every_style_is_actually_a_different_kit`)
could not see it — two tables that differ in the third decimal are "two kits"
to `!=` and one kit to an ear.

The cause was the number of **axes**. Every hit had one noise source (white
noise through one filter) and every kit was a handful of multipliers on decay,
tone and drive — so the styles could only be shorter or longer, brighter or
darker versions of one drum. What tells an 808 from a 909 from a LinnDrum is
the *source*, and two knobs were added to `DrumVoice` for it:

- **`metal`** — blends the noise from white noise to the 808's six square
  oscillators, at the ratios the 808 uses (205, 304, 370, 523, 540, 800 Hz
  over 205), tuned from the hit's `tune_hz` so a 606's hats sit above an
  808's. Through a filter of its own so the two sources are balanced *after*
  the corner where the ear hears them: measured, a bank of squares carries a
  tenth of white noise's power above a hat's corner, and `METAL_LEVEL` is that
  make-up. The six start at staggered phases, or a hat opened with a click six
  times the size of any one of them.
- **`crush`** — one converter doing both a sample-rate reduction (a held
  sample, down to a thirteenth of the host rate) and a bit-depth reduction
  (down to four bits). After the drive, which is the order the hardware had.

`KitCharacter` grew the axes a genre actually differs on — `bend`/`bend_time`
(the kick's punch: an 808 has almost none, a 909 is mostly sweep), `hat_tune`,
`hat_tone`, `metal`, `crush`, `snare_tune`, and a `kick_body` separate from
the rest — and all twenty-two recipes were rewritten around them.

**The test that holds it** is `every_pair_of_kits_is_audibly_apart`
(`fontelle-core/tests/drum_kit.rs`): render each kit's kick, snare and closed
hat, read three things an ear reads (seconds to fall thirty decibels, spectral
centroid, peak over RMS) as logs, and demand that every one of the 231 pairs
differs by at least `APART = 0.35` (about forty percent) on at least one of
them. It failed on eight pairs after the first rewrite and the recipes were
pushed until it passed; that is what "different kits" means now, and a recipe
edit that makes two kits the same will be told so.

Both knobs are read from a project with a default of zero, so a kit written
before they existed sounds exactly as it did.

### The chip lights

A preset writes the knobs and then has nothing further to say (rule 10), so
nothing *remembered* which kit was chosen — and the preset row was drawn with
no chosen chip, on every effect as well as on the drum machine. The panel now
**looks** instead of remembering: `DrumKitStyle::matching(&patch)` answers
"which kit are these thirty-six layers exactly" and
`EffectConfig::matching_preset()` the same for an effect, and
`InstrumentView::preset` is drawn filled. The moment one hit or one knob is
touched it is nobody's preset and no chip lights, which is the truth. A new
drum machine says "Studio" from its first frame; undo moves the mark back.

### LV2, through lilv

`fontelle-host` hosts LV2 now, and it is what §8.4 said the second format
would be: an arm in a `match` and not a second host. `HostedPlugin` and
`HostedProcessor` are the same two types with a format enum inside, and
nothing in `fontelle-engine` or `fontelle-app` changed to get an LV2 synth on
a channel or an LV2 effect in a chain.

What is different about LV2, and where it is absorbed (`fontelle-host/src/lv2.rs`):

- **A bundle is a folder** — the library beside Turtle files — and everything
  a menu shows is read from the Turtle by lilv. A scan `dlopen`s nothing.
- **A parameter is a control port**; its id is the port index, which LV2 makes
  stable. There are no parameter events: the wire is written into the ports at
  the top of each block.
- **Notes are MIDI in an atom sequence**, stamped with their frame, and the
  fixture honours the frame so the placement test works.
- **One object, not two.** An LV2 instance is one handle, so the whole
  instance rides in the processor and is made at `activate`, freed at
  `deactivate`.
- **One lilv world per bundle**, loading only that bundle, because the folders
  Fontelle searches are the user's and not `LV2_PATH` — and an environment
  variable is process-wide, which a test suite on sixteen threads cannot set.
  Classes and port types are read off the plugin's own data instead of the
  specification's class tree, which is enough for a scanner and a host.
- **One feature set per host** (`livi` starts a worker thread per feature
  set), shared by every LV2 plugin the host opens.
- **No state extension yet.** An LV2 plugin is saved as its control ports;
  a sampler keeping a file in `state:interface` comes back empty. Next.

**The bug the tests found:** `PluginRack` drops its host before its parked
processors, a lilv instance holds no reference to the world that loaded its
library, and the world `dlclose`s that library when freed — so the first
end-to-end render segfaulted on the way out. `Lv2Processor::_world` is the
plugin handle kept only to pin the world.

`fontelle-testlv2` is a real LV2 bundle built here — gain and sine again,
written against the raw C structs so nothing sits between the test and the
ABI — and its two Turtle files are constants a test helper writes beside the
library. **`cargo build -p fontelle-testlv2`** builds the `.so`; depending on
the crate does not (the same trap `fontelle-testplug` documents).

### A door for the formats that cannot come in

§3.4's problem is that the VST3 SDK is GPLv3-or-proprietary and the VST2 SDK
is withdrawn, so neither can be linked from this tree. The answer built is
the one §8.4 anticipated: a **bridge**.

- **`fontelle-bridge-abi`** (MIT/Apache, ~150 lines) is the whole shared
  surface: a `#[repr(C)]` table of function pointers — scan, open, params,
  activate, process, notes, state — with a version number and a thread
  contract that is the one every plugin API draws (process/notes/reset on the
  audio thread, the rest on main, the bridge synchronises the two).
- **`fontelle_host::Bridges`** loads every library in Fontelle's own folder
  (`$XDG_DATA_HOME/fontelle/bridges`, or `FONTELLE_BRIDGES`), checks the ABI
  version, and refuses one that claims a natively hosted format. A bridged
  plugin is the third arm of the same two enums. `PluginFormat::hosted` still
  says what *this build* loads; `PluginHost::can_host` adds what is installed.
- **Fontelle never links a bridge and a bridge never links Fontelle.** What a
  bridge links — the SDK, `yabridge`'s output — is built from a repository of
  its own under its own licence, and never enters this one.
- **`fontelle-testbridge`** is a bridge with no SDK in it (its "bundles" are
  folders with a `plugins.txt`), and is what the tests load. What they prove
  is the loader and the seam.

**The VST3 bridge itself is not written.** It is a separate, private
repository's worth of COM-style FFI against Steinberg's SDK, and it is the
next thing for anyone who wants a Windows plugin collection in here (through
`yabridge`, which turns a Windows VST3 into a Linux VST3 — so one bridge
covers both). The seam it plugs into is done and tested.

### Where it is, and the count

`fontelle-dsp/src/drum.rs` · `fontelle-core/src/drum_kit.rs` ·
`fontelle-types::EffectConfig::matching_preset` · `InstrumentView::preset` ·
`fontelle-host/src/{lv2,bridge}.rs` · `fontelle-bridge-abi` ·
`fontelle-testlv2` · `fontelle-testbridge` · `PluginRack::set_bridge_folders`.

`cargo test --workspace` is green at **3010 (2944 before this)** and clippy at
`-D warnings` is clean.

## 2026-09-04: plugins somebody else wrote, actually running

> *"close the remaining gaps to make our instruments and plugins completely
> modular to allow for third party effect plugins and instrument vsts etc."*

**Fontelle hosts CLAP plugins.** Not a scaffold and not a mock: it walks the
folders CLAP nominates, `dlopen`s what it finds, instantiates a plugin, reads
its parameters, feeds it notes and audio on the RT thread, saves its state into
`project.json`, and puts it back the way it was when the project is reopened.
An instrument plugin plays from the piano roll; an effect plugin sits in an
insert chain.

### The prediction in §8.4 was right

The TDD said: *"the `AudioNode` trait (§5.1) and the parameter contract (§8.2)
are the entire boundary a future CLAP host would plug into … do not add
speculative hosting abstractions now."* That is exactly what it cost:

- **One new node type.** `fontelle_engine::PluginNode`, ~150 lines, which does
  what `EffectNode` and `SamplerNode` do except that it hands the block to
  somebody else's code.
- **No new addressing scheme.** An insert's plugin parameter is
  `mixer:<track>/insert[0]/param/<clap id>` — the address inserts already had.
  An instrument's is `channel:<id>/patch/plugin/param/<clap id>`, which parses
  because `ParamTarget::ChannelPatch` was written to take anything after
  `patch/`. Both end `/param/<id>`, and that is the only rule the node reads.
  Automation, undo, right-click-to-automate and preset storage all worked
  without being touched.
- **No new panel.** `describe_plugin` builds the same `InstrumentView` the
  soundfont editor and the effect windows are drawn from. A plugin's parameters
  differ from a built-in effect's in exactly one way — their names arrive at run
  time instead of living in the binary — which is why `HostedParam` has `String`
  where `ParamSpec` has `&'static str`, and why that is the only difference.

### Where a plugin lives, and why it cannot live in the graph

This is the one genuinely hard part, and it is worth reading before touching any
of it.

A CLAP plugin may be **activated once**. Its main-thread handle and its audio
processor are two objects, on two threads, by specification — `clack` encodes
that in the types, and a `PluginInstance` dropped while its processor is still
out is deliberately *leaked* rather than freed on the wrong thread. Meanwhile
this engine rebuilds the graph **whole** on every structural edit, and builds
the new one *while the old one is still playing*. So at the moment a new plugin
node is constructed, the processor it needs is inside the node it replaces.

So a plugin does not belong to a graph. It belongs to `fontelle_app::PluginRack`,
for as long as the document has a slot for it, and the graph gets a share of two
things:

- a **`ProcessorBay`**, where the processor waits between graphs. A retired node
  parks it on the way out — in `GraphPublisher::reclaim`, which was already *the*
  one place a live graph dies, on the main thread. The new node takes it the
  first time it renders, with a `try_lock` that never waits: a block that cannot
  have it renders **pass-through** (not silence — a hole in the mix is worse
  than a few unprocessed blocks), and tries again next block.
- the **`ParamValues`** a knob writes on: one atomic per parameter plus a moved
  flag, drained into CLAP events at the top of each block. The same shape
  `TrackControls` uses for a fader, for the same reason — the document is still
  the source of truth, this is how a drag is *heard* before the next rebuild.

A plugin whose slot the document stops asking for is **retired**, not dropped,
and swept once its processor has come home.

### The document

- `PluginKey` — `clap:com.u-he.diva`. The **plugin's own id, never a path**:
  INVARIANT 8's rule for audio applied to plugins. A project names what a thing
  is; the machine resolves where it is.
- `EffectSlot::plugin` and `Channel::plugin`, both `#[serde(default,
  skip_serializing_if)]`, so every project written before this opens unchanged
  and is written back unchanged. `EffectSlot::kind()` now returns
  `Option<EffectKind>` — `None` means "this slot is somebody else's plugin and
  every question about `EffectConfig` is the wrong question", and the compiler
  found all sixteen places that needed to ask.
- Values are stored **plain**, in the plugin's units, not normalised. CLAP's own
  advice, and the argument is the same as for a taper: a plugin that widens a
  range in an update should keep sounding the same, and only the plain number
  can promise that.
- State is the plugin's opaque blob (base64 — `fontelle-types/src/base64.rs`,
  forty lines rather than a dependency that reads project files) **plus** every
  parameter. Both: the blob carries what no parameter can, and the parameters
  are what this program can automate. A plugin with no state extension is
  restored from the parameters alone, which is the case `fontelle-testplug`'s
  sine exists to keep honest.

### Testing a foreign ABI

`crates/fontelle-testplug` is **a real CLAP bundle built in this repository** —
a gain effect and a sine instrument, ~500 lines. The host's tests load it through
the real entry point across the real ABI.

The alternative was worse in both directions. A mock tests the mock; a test
against whatever the developer has installed tests that machine. The fixture
also earns its keep by being *deliberately awkward*: two plugins in one bundle
(the ordinary case, and a scanner that had only ever seen bundles of one would
be wrong in a way nobody noticed), one plugin with the state extension and one
without, a stepped parameter, and an instrument that honours event **timestamps**
— which is how the host's sample-accurate note placement is checked at all.

**One trap, and there is a guard for it now.** Depending on `fontelle-testplug`
builds its *rlib*; the `.clap` a test loads is the *cdylib*, which only a build
of that package itself produces. So `cargo test -p fontelle-host` alone can run
new tests against an old plugin and fail for reasons that are nowhere in the
diff — it cost half an hour. The test helpers now refuse to run if the built
plugin is older than its source and say what to type.

### What is not in it

- **No plugin editor windows.** A plugin is edited on Fontelle's generic panel.
  This is the largest remaining gap and it is a real one: a synth whose sound is
  drawn rather than dialled is much less useful without its own UI. It needs
  `clap_plugin_gui` and a child window, which §16 says the windowing layer was
  built multi-window for.
- **No latency compensation** for a plugin that reports latency.
  `PluginNode::latency_samples` returns zero, which is the honest answer to what
  this build does; a wrong number would misalign every other track.
- **No note expressions or slides** into a hosted instrument, and no CLAP
  sidechain input ports (`EffectSlot::effective_key` returns `None` for a plugin
  — an edge that fed nothing would still order the graph).
- **No VST3 or LV2.** `PluginFormat` names all three and `hosted()` says which
  can actually be loaded, so a project from a machine that had one says *"a VST3
  named X is missing"* rather than failing to parse. §3.4 is why VST3 is last.

### Where it is

`fontelle-host` (scan, load, run) · `fontelle-testplug` (the fixture) ·
`fontelle_engine::PluginNode` · `fontelle_app::PluginRack` ·
`fontelle_app::realise_hosting` · `fontelle_types::{PluginKey, PluginState}` ·
`fontelle_model::{AddPluginInsert, SetChannelPlugin, SetPluginParam}` ·
`Settings::plugin_dirs`.

**110 tests**, and `cargo test --workspace` is green at **2944** (2834 before
this):

| where | tests | what they hold |
| --- | ---: | --- |
| `fontelle-types/tests/plugin.rs` | 9 | keys, formats, what a slot stores |
| `fontelle-types/tests/base64.rs` | 5 | RFC 4648's own vectors, and every byte |
| `fontelle-host/tests/scanning.rs` | 9 | walking folders, and what will not load |
| `fontelle-host/tests/hosting.rs` | 19 | opening, reading, setting, hearing, saving |
| `fontelle-host/tests/bay.rs` | 3 | the handover between graphs |
| `fontelle-model/tests/plugins.rs` | 14 | the document, its commands, its undo |
| `fontelle-engine/tests/plugin_nodes.rs` | 12 | the node in a block |
| `fontelle-engine/tests/plugin_no_allocation.rs` | 3 | INVARIANT 1, under a guarding allocator |
| `fontelle-app/tests/plugin_hosting.rs` | 15 | end to end: chosen, realised, heard |
| `fontelle-app/tests/plugin_ui.rs` | 14 | the browser, the panels, the knobs |
| `fontelle-app/tests/plugin_folders.rs` | 7 | where it looks, and what the tab says |

The allocation ones are worth singling out. `no_allocation_during_render.rs`
exists because a per-block `Vec` in `process_block` was found on real hardware
and nowhere else; the plugin file is the same guard over the hosting path, and
it **caught the same bug again** — draining the parameter wire into a scratch
`Vec` is the obvious spelling and allocates once a block. What it can promise is
only Fontelle's half: a real plugin that allocates is not a bug in this code and
this test would not see it.

## 2026-09-04: a drum machine, and it is not a special case

> *"i want you to create a new instrument, a built in general purpose drum
> machine that can just make a variety of drum styles and sounds and you can
> play them all in the piano roll all labeled and stuff should have lots of
> presets for different styles and genres of kits. should be encorperated like
> any other vst would be."*

**A kit is an ordinary `Patch`.** That is the whole design, and it is what
*"like any other vst"* turned out to mean in this codebase: thirty-six layers,
one per key, each carrying `Source::Drum(DrumVoice)`. Nothing else had to be
told it exists — it is saved by `to_data`, read by `from_data`, **labelled by
the key map already** (a kit's layers are one-key zones, so `keymap.rs` reads it
as a key map and only had to be told where the names live), filtered by the
patch's two filters, routed by the channel's mixer track, and addressed by §8.2
like any other patch. Below the panel the new code is one `Source` variant and
about twenty lines in `Voice`.

### The hits are synthesised, not sampled

`fontelle-dsp/src/drum.rs`. A sampled kit is a folder of files, and this
program's whole position is that a file supplies *defaults you then own* — so a
synthesised kit is that position taken to the drums. Every hit is ten numbers,
so every hit is tunable, twenty-two kits cost twenty-two rows instead of sixty
megabytes, and **the drum machine is the only instrument here that works on a
fresh install with no bank configured**.

Ten models — kick, snare, tom, two hats, clap, cymbal, rim, cowbell, perc — and
each is the same three parts in different proportions: a pitched **body** whose
pitch falls, a **noise** half through a filter, and a **snap** at the front. The
model decides what is wired to what; `DrumVoice` decides how it sounds.
Everything expensive is computed in `trigger` rather than per sample, and
`DrumSynth` is `Copy` and fixed-size so a voice slot can hold one (INVARIANT 1).
`fontelle-dsp/tests/drum.rs` (14) — what a test can honestly say about a drum is
that it sounds, that it *stops*, that it stops when it was told to, that it stays
inside full scale, and that two settings are two sounds.

### General MIDI, so a drum file lands right

`GM_DRUM_MAP` is keys 35–70 — kick on 36, snare on 38, closed hat on 42. Chosen
over a run of keys from zero because it is what every drum MIDI file, every pad
controller and every other drum plugin agrees on, so a part written elsewhere
plays here and vice versa.

### Twenty-two kits from a table, not seven thousand numbers

Studio, 808, 909, 707, 606, LinnDrum, Trap, Boom Bap, Lo-Fi, House, Techno,
Drum & Bass, Garage, Rock, Metal, Funk, Jazz Brushes, Latin, Cinematic,
Chiptune, Industrial, Ambient.

Written out longhand that is thirty-six hits times ten numbers times
twenty-two, and the twentieth would be a copy of the fourth with two edits. So
there is **one** kit — the GM slots, each with its plain studio settings — and a
`KitCharacter` per style: eleven numbers saying how this genre differs. An 808's
kick rings and its hats are short; metal is a clicky triggered kick and a gated
snare; chiptune is square waves. Each row reads as a recipe. What lands on the
channel afterwards is thirty-six independent voices — a preset writes the knobs
and then has nothing further to say (rule 10), so nothing remembers which kit it
came from and every hit stays editable.

They are the **preset chips** on the instrument panel, which had no preset row
until there was an instrument with something to put in one
(`StudioHost::set_instrument_preset`).

### A drum part is a chord

The one thing the tests found rather than confirmed: seven hits landing on one
tick summed to **2.5** — well past full scale. `KIT_HEADROOM_DB` is ten decibels
under, on the layer rather than on the hit so a hit's own `gain_db` still reads
as "the crash is ten under the kick". Same argument `basic_synth` makes about
its saw, and the same reason: the master limiter *would* catch it, and a kit
that lives in the limiter is a kit that sounds squashed with nothing to point
at.

### A hit ends itself

The drum machine's one departure from every other instrument here. The kit's amp
envelope is held **open** — attack and decay at zero, sustain at full — so the
length of a sound is the hit's own `decay_s` rather than the patch envelope's. A
kick and a hat sharing one decay would not be a kit, and a drum whose length is
set in two places is a drum whose knob appears not to work. The slot marks
itself inactive when the hit finishes, so a note held for a bar costs nothing
after the drum has gone.

`fontelle-core/tests/drum_kit.rs` (20), `fontelle-app/tests/drum_machine.rs` (9).

### What is not in it

**Per-hit editing on the panel.** A kit is thirty-six hits of six knobs and that
is two hundred and sixteen controls; the panel would need a notion of a
*selected hit* to draw one at a time, which is a real piece of UI and not a
line. The kits, the two patch filters and the amp envelope are the shaping that
is there today, and all three work on a kit because none of them was told it was
one. Per-hit addresses would hang off the `patch/layer[n]/` scheme that already
exists.

## 2026-09-04: Ctrl+L joins a phrase up

> *"if i press ctrl l with a note selection in the piano roll it makes all the
> notes lengths not have gaps like how it does in fl studio with that same
> keybind. just makes all the notes cleanly connect to eachother basically in
> length."*

FL's Quick Legato. The arithmetic is `fontelle_model::legato_lengths` and it is
pure, so the three rules that are easy to get wrong are answered once and
tested rather than being buried in a window:

- **A start, not a note, is what a note reaches.** Notes sharing a tick are one
  musical event: a chord's notes all reach the *next* event, and none of them is
  "the next note" for the other two. Grouping by note would collapse every voice
  of a chord but the top one to nothing.
- **It shortens as well as lengthens.** A note running under the one after it is
  pulled back to it. "At least touch" would mean a phrase run through the tool
  twice kept growing, with no way back.
- **The last event keeps the length it had.** There is nothing after it to
  touch, and picking a length for it would be the tool inventing something
  nobody asked for.

It acts on the **selection and nothing else** — a note left out is neither
resized nor used as the thing the note before it should reach — and it is one
edit: `SetNoteLengths` writes a length per note (absolute, unlike `ResizeNotes`'
single delta) and inverts by restoring the ones it found, so one Ctrl+Z takes
the whole phrase back. A phrase already joined up asks for nothing, so pressing
it twice costs one undo rather than two.

**The key does two things and the selection decides which.** `Ctrl+L` was the
play-mode toggle, which is still what it does with no notes in hand — the chip
on the transport bar is the other way and a narrow window has no room for it
(`MIN_RULER_WIDTH`), so that mode has to stay reachable. The two can never both
apply: legato needs a note selection in the roll, and the play mode is not about
notes at all.

There is a **Legato** row on the Tools chip's menu too, with the shortcut
written on it — a key nobody is told about is a key nobody presses. Both ways in
go through `canvas::legato_edits`, so the menu row and the keystroke cannot
drift into meaning different things. `fontelle-model/tests/note_tools.rs` (10),
`fontelle-ui/tests/roll_keys.rs` (7), `fontelle-ui/tests/tools_panel.rs` (2).

## 2026-09-04: controls that are controls, and a lane you draw on

> *"make ctrl + m toggle the metronome, make pressing M while having a mixer
> track selected toggles its mute and pressing N solos ... make the piano rolls
> velocity controls less like a slider you drag up and down and more like fls
> where youre kind of drawing it ... currently its hard to actually edit
> multiple notes velocities at once or in a long string or if notes are
> overlapping eachother or start at the same time ... a lot of options that
> could be knobs or sliders or dropdowns for some reason are instead shown as
> buttons you click to toggle through a list of options in order iteratively
> ... the pitch changing should be a knob ... when stretch is off on an audio
> clip, when i change the pitch it still is visually stretching the clip in the
> arrangement ... extending a loop on a clip is affecting the fade lengths."*

### The property lane is a canvas now

It was one little fader per note: the press caught a note and the drag moved
that one note, however far sideways it went. Three complaints in one, and the
fix is one idea — **the lane is a thing you draw on**.

A stroke takes every bar it *crosses*, including the ones it skipped between
two mouse reports (a mouse reports about a hundred times a second and a hand
crosses four bars in less than that), and each of them is set to the height the
pointer was **over it** rather than to where the pointer ended up — so a ramp
drawn quickly is a ramp rather than a flat row at the final value.

A *column* is now a question about note **starts**, because that is where the
lane draws its bars. Asking it the other way — which note covers this tick —
is what made *"notes overlapping eachother or start at the same time"*
unreachable: three of a chord's four bars are behind the topmost, and a held
pad answered for every column it lay under. There is one fallback, deliberately:
with no bar anywhere near the brush, the note covering that tick is taken, so a
lone held note is still grabbable anywhere along it.

The one gesture that does *not* follow the pointer is the old one — press on a
bar that is already **selected** and the whole selection takes the value and
keeps taking it, which is what flattens a chord in one movement and is the only
way to aim a stroke at chosen notes rather than at everything under the brush.
`fontelle-ui/tests/roll_interaction.rs` (8 new).

### Controls that are the shape of what they set

*"a lot of options that could be knobs or sliders or dropdowns ... are instead
shown as buttons you click to toggle through a list of options in order
iteratively. this is really annoying."*

The audio clip editor was the named example and had one gesture for eighteen
different kinds of value. Every row now says what it **is**
(`canvas::AudioControl`) and gets the control that shape deserves: a **slider**
for anything continuous, a **switch** for anything on or off, a **drop-down**
for anything that is one of a list. Stepping survives as the **wheel**, which
is what a wheel over a control should do anyway and is how a value is nudged by
exactly one of its own units.

A slider rather than a knob because the panel is a list of rows: a knob in a
22-pixel row is a smudge with no readable travel. The tracks are detented at
the values a mix is built out of — unity gain, dead centre, normal speed — and
pitch quantises to whole semitones, because four octaves each way over a couple
of hundred points is about two points a semitone and a track that offered cents
is a track that cannot reliably land on a note. Speed and cutoff run
logarithmically, exactly as their stepping already did. The fill grows from
each row's **neutral**, so a cut and a boost read as opposite things.
`fontelle-ui/tests/audio_editor.rs` (14 new).

The same complaint, in the two other places it was true:

- A `ParamKind::Choice` on the instrument panel or on an effect's own drops its
  list instead of stepping. The row of pips stays — it is how the control reads
  at a glance — and a press now opens the list, so the sixth option is one
  press away rather than five sounds you did not ask for.
- The **snap chips**, on both toolbars. `SNAP_DIVISIONS` is the cycle written
  down as a list; the chip drops it and `S` still steps it, and a test holds the
  two walks together. Both chips wear a caret now, the rule the lane chip
  already followed: a control that opens something has to look like one.

`open_menu` learned that the press which *shut* a menu is not the press that
reopens it (`dismissed`), so every drop-down that hangs off a chip is a toggle
rather than a flicker.

### Three keys

- **Ctrl+M** is the metronome, and it is in `global_key` beside Space — wanting
  the click on while you play a part in is not a statement about which panel you
  were last looking at, and the editor windows answer that function too. Muting
  the selected clips moved to **Ctrl+Shift+M**.
- **M** and **N** mute and solo the **selected mixer strip**, bare, while the
  mixer is the open tab. Which strip is the one the track-options column is
  already pointed at, so the answer is on screen before the key is pressed.
  `canvas::mixer_key`, `fontelle-ui/tests/mixer.rs` (2 new).

### Two bugs behind the pictures

**Repitching drew a stretch.** *"when stretch is off ... it still is visually
stretching the clip in the arrangement (tested on a looping audio clip)."* The
block already gets shorter — `natural_length` divides by the rate, because
varispeed is what pitch *is* until the stretch engine lands (§3.3) — and the
waveform inside it was then scaled by the rate a **second** time: the buckets
were addressed in file frames and handed to `source_position`, which multiplies
by the rate. An octave up drew half the file across the whole strip and smeared
its last bucket over the rest. `Resample` is the one mode where that scaling is
right, because there the block is the constant and the player's own ratio folds
the pass length in — so the two modes ask for two different spans, and now do.
`fontelle-app/tests/stretch_toggle.rs` (2 new).

**Extending a loop lengthened the fades.** A fade is frames of the **file** —
that is what `fade_anatomy` places the handles against and what the host stores
— and `fade_curve` was measuring against the *block*. Dragging a one-bar loop
out to four bars drew a fade four times as long over a clip whose fade had not
moved, and the curve and its own handle stopped agreeing.
`fontelle-ui/tests/clip_fades.rs` (2 new).

**Still true, and not a bug:** with stretch off, moving the pitch still changes
how long the clip *plays* for, because resampling is the only repitch there is
until the stretch engine (§3.3, v2). What changed is that the picture no longer
lies about it on top.

## 2026-09-04: a drag that stays where you put it, and room to work in

> *"i cannot loop clips ... when i drag things they often go wayyyy off into
> infinity for me like with the slightest mouse movement ... i cant figure out
> how to turn [a loop] back into just a normal clip ... adjust the sizing
> between the soundfonts top section and bottom section ... add a lane above or
> a lane below the lane i right clicked ... instead of only starting with 1
> lane make it like 10 ... decrease the default size of the mixer/piano roll
> area vs the arrangement ... if i click the eq effect in the mixer track
> effect rack to focus its window, it doesnt seemingly do anything ... go
> through the selected instruments with arrow keys ... press enter while its
> selected."*

### The drag that ran away, and why it was the mouse

*"they often go wayyyy off into infinity ... with the slightest mouse
movement"*, and *"pretty much universally all places i can drag something that
moves the view"* — which is the clue. Every one of those places is edge
scrolling, and `edge_scroll` answered **how far to move the view because of
this pointer event**. The window applied it once per `CursorMoved`.

So the speed of the scroll was the *mouse's report rate*. Nobody chose that and
it differs by an order of magnitude between one mouse and the next: at the
arrangement's default zoom a single event one pixel outside the grid moved 40
ticks, so a 1000 Hz mouse moved 40 000 ticks a second — ten bars — for a
pointer that was barely outside, and a couple of hundred bars a second for one
thrown at the edge. Its own test bounded the travel *per event*, which is
exactly the wrong invariant: a thousand bounded events a second is still a
thousand of them.

It is a **rate** now — `edge_scroll_rate`, in pixels per second — and the
window integrates it against a real clock. `EdgeScroll` keeps the part of a
tick that has not added up yet, because the opposite failure is just as easy:
at a fine zoom one millisecond of travel is a fraction of a tick, and
truncating each step on its own would scroll nothing at all on a fast mouse.
`dt` is clamped, since wall-clock has holes in it and thirty seconds of
catch-up is the same runaway arriving by another door.
`fontelle-ui/tests/edge_scroll.rs` (8), and `roll_polish.rs`'s old per-event
bound is now a per-second one.

### A loop you can undo

*"i cant figure out (if there even is a way) how to turn it back into just a
normal clip i can extend the length of."* There was not one. Now the gesture
that made it undoes it: the same edge grip, **without** Shift, dragged back to
the period. A block no longer than one pass has nothing to repeat, so calling
it a loop was a state you could be in and could not see. Not while Shift is
held — that drag is *asking* for a loop, and one that unlooped on its way past
its own period could never make a short one. `fontelle-ui/tests/looping.rs`
(6 new).

### Room to work in

Ten rows in a new project rather than one (*"its too barren"*), and the
arrangement opens at 300 px rather than 200.

**Getting that number right took two wrong answers, and both are worth writing
down.** Measured at the window the studio actually opens at, 1280x720:

| height | rows shown | keys of roll |
|--------|-----------:|-------------:|
| 200 (before) | 3 | 21 |
| 300 (now)    | 6 | 14 |
| 340          | 7 | 11 |
| 430          | 8 | 8  |

430 was the first try and 340 the second; both left the piano roll around
**eleven keys of grid** — under an octave. `MIN_EDITOR_HEIGHT` caught neither,
because it bounds the panel **frame** and what had gone was the room *inside*
it: at 340 the frame was 316 px, comfortably above its 220 floor, with 152 px
of grid in it.

Two things came out of that. The invariant now held is on the **grid**
(`the_editor_keeps_an_octave`), not the frame. And the tests measure
**1280x720** rather than the 1400x820 they first used — a window the app never
opens, which is how a default can pass its own test and still be wrong.
*"Decrease the mixer/piano roll area"* is not "shrink it to nothing".
`fontelle-app/tests/lanes.rs`, `fontelle-ui/tests/arrangement_room.rs`.

Ten rows had one ripple worth knowing about: `importing.rs`'s
`answering_all_brings_every_part_in_under_its_own_name` counted lanes as
`3` — "a row each, plus the one that was there" — which quietly encoded the old
starting count. It measures the **delta** now, because "one row per imported
part" is the invariant and the number of rows a project opens with is a default
that has already moved once.

A right-clicked row offers **Add lane above** and **Add lane below** instead of
one "Add lane" that went to the end. `AddLane::at` renumbers the stack rather
than touching lane ids, because a clip names a lane id and renumbering the
arena would move somebody's music to another row — the same rule `MoveLane`
already followed.

### A seam in the soundfont panel

The bank and its presets were split by a constant, which is right until one
list is forty rows and the other is two. There is a strip between them now that
drags, the same shape as the sidebar's own seam and with the same floors:
neither list can be pushed away to nothing, because the seam goes with it and
then there is nothing left to grab. `fontelle-ui/tests/browser_split.rs` (8).

### Arrow keys in the bank

Clicking a preset row gives the list the keyboard: up and down walk it, Enter
chooses, Escape hands the arrows back to the notes and clips. Headings are
**stepped over** rather than landed on — a search across the collection puts one
over every run of hits, and a focus that stops on them makes the down arrow
appear to do nothing every few presses — and the ends hold rather than wrap.
The focused row is outlined where the playing one is washed, so "where the
keyboard is" and "what is on the channel" stay two different marks.
`fontelle-ui/tests/browser_keys.rs` (8).

### The window that would not come forward

*"if its an effect for example like an eq and i click the eq effect in the mixer
track effect rack to focus its window, it doesnt seemingly do anything ... the
window already existing means that it will not focus to the top."* `raise_editor`
called `focus_window` and `request_user_attention`, and under Wayland the first
is a documented no-op — a client may not take the focus by asking, it has to be
given it. So the window stayed where it was.

It now un-minimises and un-hides first (a window in the taskbar is not behind
the studio, it is nowhere, and every other call is a no-op on one that is not
mapped), then asks for focus, then **raises itself above the others for one
frame** and drops back to an ordinary window on its next draw. A restack is a
thing the compositor will do on request where a focus change is not, which is
the part that makes this work on KDE Plasma; held for a frame rather than
dropped in the same breath because a compositor that batched the two calls
would see no change at all.

### The five that were open, now closed

**An instrument is a *kind* now, and the choice is stored.** There was no such
notion: a `Patch` is layers, and which of the three you had was read off their
`Source`. That works for a patch with something in it and not at all for an
empty one — a sampler with no sample and a soundfont player with no soundfont
are the same empty patch, and both are states you sit in while deciding what to
load. So `Channel::instrument` keeps the **choice** and the patch follows from
it. `+ Add instrument` asks which; a channel's menu has **Change instrument**
with the kind you are already on greyed, because choosing it would throw away
whatever you had edited. A project written before the field derives it from
what is loaded — which is exactly the derivation the field replaces, so an old
project opens saying what it always was. `fontelle-app/tests/instrument_kinds.rs`.

**A preview voice, so hearing an instrument is not choosing one.** The reason
clicking a soundfont worked the weird way round is structural: the only
instruments that existed were the ones on channels, so the only way to hear one
was to *put it on a channel*. `Realised::preview_node` is a sampler on the
master bus that is in **no** `channel_nodes` map — the sequencer never names it,
so it is silent unless the window sends it a live note. On the master
deliberately: a preview must not pick up whatever inserts a track is carrying,
or *"how does this soundfont sound"* is answered through somebody's sidechained
compressor. Click plays middle C, Ctrl an octave down, Shift up; double-click
or Enter assigns; playing the keyboard or a note aims the live path back at
your own instrument. `fontelle-app/tests/preview_voice.rs`.

**Rendering a row to audio.** `CompileScope::Lane` compiles one row and nothing
else — a scope that let the rest of the song through would put the whole mix in
every "track" render. Right-click offers **Render to audio**, prompting only
when there is a time selection to choose between.

One decision the tests forced: a **range** render bounces exactly the range,
and only a whole-row render keeps the two-bar release tail. The first version
kept the tail either way, which made a two-bar selection come back four bars
long — a render that does not line up with the selection it came from.
`fontelle-sequencer/tests/lane_scope.rs`, `fontelle-app/tests/render_lane.rs`.

**A sampler from a file, and a name field to drop one on.** This needed
something that did not exist. The two stores are different **on purpose**: an
audio clip's audio is stereo in the `AudioStore`, a sampler layer's is mono in
the `SampleStore` (`fontelle_core::SampleBuffer`). So dropping a file on the
arrangement and dropping it on the rack are two different acts on one file, and
only the first existed. `SampleLibrary::import_sample` folds to mono **by
averaging** rather than taking the left channel — half of a stereo drum loop is
a thinner drum loop rather than an obviously wrong one — and registers real
provenance; `bundle.rs` gained an `AssetKind::Sample` reload arm, without which
a sampler built this way would open silent.

A row dragged out of the browser means different things by where it lands: the
rack makes a new instrument of it, and the instrument window's **name field**
assigns to the one already there. The name is drawn as a field rather than a
bar because a field is a thing you put something *in*, which is what it is for.
`fontelle-app/tests/sampler_from_file.rs`.

### Still open from the same report

*(All five below were built in the section above; kept because the reasoning
about **why** each was awkward is the part worth not re-deriving.)*


- **A soundfont clicked in the bank should sound at C**, an octave down with
  Ctrl and up with Shift. This needs a preview voice: `audition_on` puts its
  note on the *selected channel's* node, so hearing a preset that is not loaded
  anywhere means a sampler in the graph that is not in the document, threaded
  through `realise` and rebuilt when the preset changes. Until it exists,
  clicking a preset still **assigns** it (as it always has) rather than doing
  nothing, and Enter now does the same from the keyboard.
- **An instrument selection menu** — SoundFont player, 3OSC, Sampler — and
  replacing one kind with another. There is no instrument *kind* in the model
  today: a `Patch` is layers, and which of the three it is has to be read off
  their `Source`. Doing this properly is a field on `Channel`, a command to set
  it, and the rack and the editor switching on it.
- **Dragging an audio file from the Import tab into the rack** to make a
  sampler, and **dropping a soundfont onto the instrument window's name field**.
  Both need a drag *out of* the browser, which the panel has no notion of yet.
- **Rendering a track to an audio clip** — right-click, prompt for the time
  selection when there is one, and a new lane named `<name> (rendered)`
  underneath. The offline render path exists (`export_wav`); what is missing is
  rendering *one* track's scope into the audio store and putting a clip on the
  arrangement.

## 2026-09-04: a switch that says whether a drag stretches or cuts

> *"i cannot loop clips, whenever i drag them it is ALWAYS stretching them. we
> should make it so theres a stretch on/off toggle control with the arrangement
> controls and that defines whether it cuts the clip or stretches it and then
> also resolve the issue of it trying to stretch while looping and whatnot so
> it all works together cleanly."*

**Where the count went:** 2628 -> 2662 across the workspace, 0 failing, clippy
clean at `-D warnings`. Written test-first; the test files are named below.

### The picture was the half that lied

Dragging an audio clip's edge never did stretch the sound. `ClipStretch::Off`
is the default and it means *the block is a window onto the file* — the player
reads at the file's own rate and goes quiet when the file runs out. What
stretched was the **waveform**: `clip_waveform` mapped its columns across the
whole block, so a clip dragged to twice its length drew its take spread over
twice the space. The report is what that looks like from the outside, and it is
a fair reading of it: the picture said "stretched" on every drag.

So the fix is two things that had to arrive together — a switch that says which
of the two a drag means, and a picture that draws whichever one is happening.

### The switch

`TimelineControl::Stretch` sits beside the snap chip, because it is the same
kind of control: not an action but what the *next* drag means. It draws its
word rather than a glyph, for the reason the snap chip does — the state is the
useful half. **Off by default**, since a take has to sound like the take.

The mode is decided **at the press** and kept for the whole drag, the same rule
`looping` already followed: a switch flipped mid-drag must not change what the
drag has been doing. On the first step it emits one `ArrangeEdit::SetStretch`
for exactly the clips it would change — note clips are never named, and a clip
already in that mode is not told again — and the host writes it to the clip's
own `ClipStretch`, which is what the player reads. The mode goes **before** the
resize in the same list, so the block grows on a clip that already knows what a
longer block means. `fontelle-ui/tests/stretch_toggle.rs` (22),
`fontelle-app/tests/stretch_toggle.rs` (5).

Shift on the same grip still loops, and now the two compose in a fixed order:
mode, then period, then size. With the switch off, Shift-dragging a stretched
clip un-stretches it, loops it and grows it — every pass the file at its own
rate, however far the block is pulled.

### The picture, per pass and per mode

`canvas::content_ticks` and `content_fraction` are the one answer to *where in
the file is this column*, and the waveform, the fade handles, the fade drag and
the crossfade curves all ask them, so the four cannot disagree:

- Not stretched, the file takes the ticks it takes (`AudioPreview::natural_length`,
  which the host measures through the tempo map, so a tempo change moves it,
  and divides by `AudioClipData::rate` so a clip an octave up is over in half
  the ticks). Past its end **no column is drawn** — the take is not quiet
  there, it is over.
- Stretched, the file fills its pass whatever the pass is.
- A pass is the **period** when the arrangement repeats the clip, so a loop
  draws its file again at every seam instead of once across the whole block.
- A clip whose rate is not known yet fills its block: §15.3's "draw what
  exists", never a blank.

A fade moved with it, and had to: a fade is frames of the **file**, so on an
unstretched block longer than its file the out handle now sits where the sound
stops rather than at a corner nothing is playing under.

### The blade, which was broken for both

`SplitClip` found its seam at the file's own rate, counted from the block's
start. That is right only for a clip that neither stretches nor repeats, and
for the other two it lands past the end of the file, clamps, and hands one half
the whole take and the other half nothing. Now:

- **Not looping**, the seam is found the way the player finds it, through
  `AudioClipData::read_ratio` — so a stretched clip is cut where it sounds.
- **Looping**, the cut divides the *arrangement* and not the file: both halves
  keep the whole take and go on repeating it, and each half's own block
  truncates its last pass. Trimming them to the blade is the tempting answer
  and the wrong one — a trimmed half still repeats every period, so every pass
  after the first would play the shortened range and then sit silent for the
  rest of the period. One right frame at the blade bought with a hole in every
  bar.

This also reversed a rule inherited from note clips: an audio clip's front half
**keeps** its loop. A note clip's stops because `split_notes` writes its repeats
out and it plays them anyway; there is nothing to write out for a take, so a
front half that stopped looping would play the file once and then sit silent
for the passes it used to play. `fontelle-model/tests/audio_clips.rs` (3 new),
`fontelle-engine/tests/audio_clips.rs` (2 new, the arrangement's repeat against
each mode).

### Seen, not assumed

The four rows and the switch were shot through the real vello renderer
(`render_headless.rs`, `a_take_is_drawn_where_it_sounds_and_the_switch_says_which_way_it_is_set`,
which dumps `timeline-stretch-{on,off}.png` and reads the chip's own pixels to
prove it looks different on and off). The chip was then pressed in the real
window over a nested X server and it lit; all 13 controls still fit the bar.

### Known limit

A looped audio clip cut **mid-pass** restarts its loop at the blade, because a
clip stores where in the *file* it begins and not where in the *pass* — the
phase a mid-pass half would need has nowhere to live. Exact when the cut lands
on a seam, which is what the snapped grid gives you. Storing a loop phase on
`AudioClipData` would close it and is a real model change, not a tidy-up.

## 2026-09-03: one rule for the rack, fades you can grab, and a microphone that stays found

> *"for some reason its not recognizing my logitech camera mic input ... when i
> record it was working at first until i pressed stop to finish the recording
> and the clip didint get made ... i was locked out of the audio option ...
> whenever i click in the arrangement its making a new clip ... i want it to be
> a double click ... a single click should instead place a exact copy of
> whatever your last selection is ... swap between piano roll and mixer by
> pressing 1 and 2 ... its guessing what instrument i want based on the lane
> which is super weird ... clips can have multiple instruments, we just base
> our interactions on what your currently selected instrument in the channel
> rack is ... the edges of clips are always visible ... a kind of diagonal
> stripe pattern on the overlapping part ... blend together like a transition
> ... drag in from the start or end of an audio clip to create a clip fade ...
> bend the control node to bend the curve like fl studios too."*

**Where the count went:** 2489 → 2628 across the workspace, 0 failing,
clippy clean at `-D warnings`. Every item below was written test-first; the
test files are named where they are the proof.

### The microphone, and why it kept changing its name

The input list was ALSA's PCM names, filtered by asking each whether it would
open. On this desktop **PipeWire holds the hardware**: when another program
was listening to the camera through PipeWire (it was — `fuser` on the capture
device said so), every direct ALSA open failed and the camera vanished; when
nothing was, whichever of ALSA's aliases for the card opened first named it —
*"Logi Webcam C920e, USB Audio"* one day, *"Logi Webcam C920e"* the next.

So on a PipeWire machine the sources are asked of PipeWire (`pw-dump`, parsed
with `serde_json` — a bindgen dependency on `libpipewire` for a question asked
when a menu opens was not worth it), named by `node.description` and opened
through PipeWire's own ALSA plugin as `pipewire:NODE=<name>`, which **shares**
the device with whoever else has it. The output has always gone through
PipeWire (`default` *is* the plugin here), so this puts one sound server on
both ends. `crate::pipewire` in `fontelle-engine`; a machine without PipeWire
gets the ALSA list it always had. A name a project saved under the old scheme
still finds its source — `find_pipewire_source` reads the card name in front
of the comma. Checked on real hardware: `pipewire_sources.rs`'s ignored test
opened this machine's first source and saw 14 336 frames in 300 ms, with the
monitor seeing 128-frame blocks.

### The take that was not kept, twice

Two bugs behind one report. **The clip did not get made** because the studio
had been started with no arguments and had no bundle, and `keep_audio_take`
refused — and the window reported every refusal as *"nothing arrived on the
input"*, so the person went looking at cables. The take now **makes the
project real**: saved into the projects folder under its own name, as the
Projects tab's *New* would, and the take goes into it (INVARIANT 10: the
projects folder is a place the user named). Only with no projects folder
either does it refuse, and then it says exactly that; `keep_audio_take`
returns `Result<usize, String>` so the window cannot mistake a refusal for
silence again.

**Locked out of the audio option** was the record menu greying out the mode
that was on — meant as "this is the one", read as "you cannot have this" —
and choosing is how the button arms. `record_menu_entries_for` marks the
current mode with a tick and keeps it choosable.

### One rule for the rack

> *"it should instead just be based on whatever instrument you have selected
> in the channel rack."*

`Note::channel: Option<ChannelId>` — a note may play a channel other than
its clip's — and one rule everywhere: **the rack's selection is the
instrument every interaction means.** A drawn clip is on the selected
channel (it used to guess from the lane). A note drawn, pasted or recorded
goes on the selected channel in whichever clip is open, `None` when that is
the clip's own so an unmixed project is saved as it always was. The roll
shows the open clip's notes *on the selected channel* — `Session::roll_notes`,
a view rebuilt by `touch()`, which is now the one way the revision moves —
and ghosts the rest. Opening a clip does not move the rack; selecting a
channel does not move the roll off the clip in hand; and a new channel no
longer brings a lane and a clip with it, because a clip is a place and a
channel is an instrument. The compiler resolves mute, solo and the node per
note. The caption says `Drums +1` when a block holds more than its own
instrument. TDD §10.4 records the change. `multi_instrument_clips.rs`,
`note_channels.rs` (model and sequencer).

### A click stamps, a double-click draws

FL's playlist: the thing in hand is what a click puts down. Here it is **the
last clip chosen**, of any kind, and a press on empty grid asks for
`ArrangeEdit::Stamp` — an exact copy, notes and settings and loop, at that
row and bar. A double-click takes the copy its first press made back and
draws a blank clip in its place, so a double-click leaves exactly one clip
behind (`Timeline::double_press`). Nothing ever chosen still draws.
`arrange_stamp.rs` in both crates.

`1` and `2` show the roll and the mixer (`layout::editor_tab_for_key`). The
number row used to pick tools, a second binding for keys that had FL's
letters, and went unused for exactly that reason.

### Edges, stripes, and the crossfade

Every block gets a dark edge line **after** all the bodies are painted, so
two blocks of one colour end to end are two blocks and a block painted over
another still shows where the one under it ends. Where two clips on a row
lie over each other, `canvas::clip_overlaps` gives the shared rectangle and
the renderer hatches it. `clip_overlaps.rs`, and a headless render test that
reads the pixels.

The overlap also **draws the crossfade it is playing**: two curves across
the striped section, the later clip rising and the earlier falling, crossing
at three decibels down apiece — FL's picture, over the stripes rather than
instead of them. *"i do want it to also show the graph line drawn to show
the fade on the overlap ... just with them crossing through eachother."*
They are the player's envelope and not a decoration, so they follow the
compiler's rules exactly: equal power, times each clip's own fade where it
falls on it, and only a clip whose **end** the overlap reaches draws a
falling curve — which is why a clip dropped wholly inside another shows one
curve and not two. `canvas::clip_overlaps` answers where the overlap is and
what it sounds like in one pass, so the stripes and the curves cannot
disagree.

Two **audio** clips overlapping also crossfade, over exactly the overlap —
*"the timing based on how long the overlap section is."* `AudioPlacement`
carries `crossfade_in`/`crossfade_out` in song samples, worked out by the
compiler (which can see both clips and owns the conversions), and
`auto_gain` applies an **equal-power** curve — two different recordings
blended linearly dip three decibels in the middle, and a dip is not a
transition. It is a fact about the *placement*, not the clip: a clip's own
fades go with it wherever it is put, the crossfade is about the clip beside
it, and both apply. `crossfade.rs` in types, sequencer and engine.

### Fade handles, and the node that bends them

FL's anatomy, kept: a handle at each top corner of an audio block; drag it
along the block and the clip fades over the distance dragged; with a fade in
place the handle sits where the fade ends. A **node** at the midpoint of the
curve bends it — `Fade::tension`, −1..1, a power curve over the fade's shape
whose ends never move, with `tension_for_midpoint` as the inverse so the
node lands under the pointer. The canvas speaks in fractions of the block
(`SetFade`, `SetFadeTension`); the session turns a fraction into frames of
the clip's own audio, the same conversion the block's preview makes the
other way. `Fade::at` is the one reading of a fade: the player, the block
and the editor's waveform all go through it. `clip_fades.rs` in both crates,
`fade_bend.rs` in types.

### Left over from the previous session, found by running the suite

- `shoot_sized` in `render_headless.rs` wrote a wide frame through `dump`,
  which assumes the standard size — so the encoder panicked, and only ever
  when `FONTELLE_UI_DUMP` was set, which is exactly when somebody is trying
  to look at the window. It writes through `dump_sized` now.

- A test in `realise.rs` wrote two seconds of tone into the monitor ring in
  one call, and the monitor now sizes its slack from the largest block it
  has seen — so it waited for ever. Real inputs deliver blocks; the test now
  does too.
- `render_headless.rs` was missing the `recording` field the previous session
  added to `TimelineChrome`, and `tempo_showing` was written and never wired:
  the transport's tempo box now reads it.

### What is not done

- **Fade handles are drawn only on the selected block.** FL shows them on
  hover; this window has no per-block hover. The curve and its shading are
  always drawn.
- **The automatic crossfade cannot be switched off**, and it is one curve.
  TDD §15.2 says the overlap *offers* a crossfade; this always gives one.
- **A clip inside another** fades in over all of itself and the outer clip
  does not fade out, because its end is not in the overlap. That is a
  decision, recorded in `crossfade.rs`; it may want revisiting once heard.
  The block draws the same one-sided picture, on purpose.
- **A clip on a muted row still draws its crossfade**, because `ClipInfo`
  carries no lane mute — the same blind spot the waveform and the fade
  handles already have.
- **Monitoring through the speakers is still unheard.** Capture through
  PipeWire was checked; the loop out to a speaker was not.
- **The PipeWire list is read by running `pw-dump`** on every menu open and
  every device open. Twenty milliseconds here; a bindgen dependency if it is
  ever not.

## 2026-09-03: you can hear yourself, and a strip has a name you can type

> *"currently i cannot rename mixer tracks i want to be able to click on their
> name to type in that field ... please also make it so i can monitor my inputs
> so it should work like fl, tracks are already automatically routed to master
> so i should be able to hear routed input playing even when song isnt playing
> or im not recording."*

**Where the count went:** 2452 → 2489 across the workspace, 0 failing, clippy
clean at `-D warnings`.

Three things, and the second and third are the same thing seen from either end
of the signal path.

### Renaming a strip

There is one rule and it lives in `canvas::name_press`, not in an event
handler: **the first click on a name selects the strip, and a click on the name
of the strip already selected renames it.** Two clicks from anywhere in the
mixer, one from the track you are already working on, and a rename that can
never happen on the way to choosing a different strip. The track-options
column's title is a single click, because that column is already about that
track and there is nothing for a press on it to choose.

Nothing new was needed underneath: `RenameMixerTrack` has always existed and
coalesced, and the caret is the one the rack's rows and the arrangement's lanes
already draw. What was missing was the gesture, and the comment where the
gesture should have been said so — *"a rename needs a text field, and the panel
has none yet."*

### Monitoring, and the second ring

`InputWriter`'s ring goes from the input callback to the **disk** thread; that
is what a take is. Monitoring goes from the input callback to the **output**
callback. They cannot be one ring — two consumers draining at different rates
on different threads would each be stealing the other's samples, and the one
that lost would be the take — so `InputMonitor` is a second, written by the
same callback in the same breath, and `MonitorNode` reads it at the head of the
monitored track's chain. Through the track, not beside it: the fader, the
inserts and the routing are what the report means by *"i should be able to hear
it because of it routing my input track to master"*.

Three things about it are worth writing down.

**The device opens because a track names an input, not because record was
pressed.** That is the whole of *"even when song isnt playing or im not
recording"*, and it is why `armed_track` is now the track that **names** an
input rather than whichever strip happens to be selected — selection is where
you are looking, an input is a decision you made. `sync_audio_input` is called
once a frame and is two comparisons when nothing has changed.

**The stream being open is not the take being kept.** A microphone left plugged
in would otherwise grow a take for as long as the window stayed open, so the
ring is always drained and only kept while `capturing`. Arming discards
whatever monitoring had left in it, or every take would start with however long
ago you chose the input.

**Two clocks, and they drift both ways.** An input device and an output device
are two crystals and nothing keeps them in step. Towards empty the node runs
dry, goes silent and re-primes — a hole is honest, and the last block played
again is a stutter that sounds like the microphone rather than like the
software. Towards full the ring would reach its end and drop *every* block from
then on, so the reader catches up once and carries on. The slack it holds
before the first sample is latency and `latency_samples` reports it. A
transport stop does **not** interrupt it: that cuts what the song started, and a
microphone is not the song.

The idle gate grew its fourth reason to be awake, and it is the only one that
is neither an event nor a measurement — the honest state of a microphone in a
quiet room is silence, and a gate that measured its way to sleep would swallow
the first word spoken into it.

### A track that goes nowhere

> *"but if i chose to not route it to master, i wont be hearing my own input but
> it will still be recording the audio clip."*

`output: None` has always meant *the master*, so there was no way to say
**nowhere**. `MixerTrack::output_on` is that switch, and it is deliberately not
a third mute: a track's **sends still carry**, which is how a track feeding only
a reverb is built. The bus sum *is* the routing edge, so switching it off is
simply not scheduling one — the track still runs and its inserts still run, and
nothing takes the result anywhere. The destination is kept, so switching it back
on puts the track where it was rather than at the master.

And the last clause of the report, which is the reason the switch is worth
having at all:

> *"for ease of use make it so that if the input track has no output send it
> automatically will just route it to master for the clip you record ... that
> way your recording will actually be audible after playing it even if you
> werent using monitoring."*

`Mixer::reaches_master` is one walk in the document, and both halves read it:
whether monitoring is audible, and — when it is not — where the take goes. It
is a **walk** rather than one hop, because a mic feeding a group whose own
output is off is exactly as inaudible as one switched off itself, and a test on
the track alone would have missed it.

### What is not done

- **An un-routed track says so in the track-options column and in its output
  menu, and not on the strip itself.** "Why is this one silent" is exactly the
  kind of invisible state this file keeps recording, and a strip-level mark is
  the obvious next thing.
- **No input monitoring has been heard.** The path is tested end to end through
  the real node and the real graph with a synthetic ring, and the device layer
  is real, but no microphone was opened and no sound was made on a speaker.
  Arm a real input and listen; it is a one-minute check.
- **Monitoring is one input at a time.** `AudioDevice` holds one capture
  stream, so the armed track is the one that is heard. Two microphones on two
  strips is a second stream and a second ring.
- **Latency is reported and not compensated**, like the gate's and the master
  limiter's. `MonitorNode::latency_samples` is a block.

## 2026-09-03: audio arrives — import, playback, an editor, and recording

> *"right now we can basically only do things with soundfonts but i want to also
> be able to record my voice into the daw or import different sounds and loops
> and whatnot to make songs with."*

**Where the count went:** 2265 → 2452 across the workspace, 0 failing, clippy
clean at `-D warnings`. Six commits, each one usable on its own.

### Four things the window got wrong, fixed first

All four reported from using it, and all four the same shape as the ones before:
the state was right, the geometry was right, and the last step was wrong.

- **A selected automation clip hid its own graph.** The block was filled in the
  selection colour and the curve was then stroked in that same colour — a line
  painted onto its own background, so the one moment you most need the shape was
  the one moment it was gone. An automation block keeps its dark ground now
  whatever else is true of it and says it is selected with an edge. Asserted in
  pixels through the real vello pipeline.
- **Editing a note played it.** There is one rule now: a bare click on an
  existing note sounds it, and everything else is silent — drawing, painting,
  moving, resizing, and the arrow keys that stand in for a drag. Which means the
  audition cannot be decided at press time, because at press time nobody knows
  yet whether this is a click or the first pixel of a drag: a press *offers* one,
  three points of slop absorb a hand's jitter, and the release takes it.
- **The cut tool's line was invisible** — *"often totally invisible for me? still
  works though."* Both halves were one bug: a drag dirties the panel when it
  produces an edit and a marquee was special-cased on top of that, but a slice
  does neither until the button comes up. `draws_overlay` asks the question once
  rather than listing gestures at each call site.
- **The Tools panel was one bench.** Every tool's settings and every tool's
  button at once, which is how a transpose amount came to sit three rows above a
  button belonging to a different tool — and why the settings tab's *keyboard*
  transpose read as its missing half. The chip opens a **menu** now and each tool
  is its own dialog with its own Apply; the settings row says whose transpose it
  is.

### Audio, in five layers

Each layer is testable without a device, and all of it is tested that way.

**Decode and peaks** (`fontelle-assets`). Symphonia was already a dependency and
had never been called. The decoder deliberately does *not* resample — a file
records the rate it was written at and the player reads it at whatever ratio the
device asks for, because resampling on import throws the original away.
`generate_peaks` was a `todo!()`; it is min/max per bucket at every
power-of-two resolution, folded upward from the finest, which makes *"zooming
out never loses the peak"* true by construction.

**What a clip is** (`fontelle-types::AudioClipData`, §15.1). A reference plus a
list of numbers: trim, boost, pan, pitch, speed, reverse, two fades, a filter, a
loop mode, and the file's own rate. It lives in `fontelle-types` because both
ends need it and the engine may not depend on the document. The filter *is*
`FilterConfig`, so the cutoff and resonance that were asked for arrive with a
whole synthesiser filter behind them and no new DSP.

**Playback** (`AudioClipNode`). A clip is not an event and that is the whole
design: a note compiles to moments the RT thread walks a cursor through, and a
stream is a range to be inside of. So it compiles to an `AudioPlacement` beside
the events rather than among them. The node renders the same samples in blocks
of 1, 7, 85, 128 and 512 — this project has already shipped that bug once,
audible as bitcrushing — and reads a 44.1 kHz loop on a 48 kHz device at the
ratio between them.

**The window.** A take arrives on a row of its own with its waveform in it,
drawn against the clip's own trimmed range in play order, so a reversed clip
draws backwards and the fades shape the picture exactly as they shape the sound.
Double-clicking one opens an editor of nineteen rows under four headings.
`FolderKind` grew an `Audio` variant, which was meant to be the whole change —
except the Import tab had two *named* chip fields for a two-valued enum, so three
kinds drew two buttons. Same class as the fourth browser tab that shipped with no
words on it, fixed the same way.

**Recording.** The record button opens a menu — *"prompts me what i would like to
record: notes, audio from mic, automation"* — and choosing is what arms. The
mixer strip grew an input row above its output row. The count-in is not a delay:
the transport rolls a bar early over the click and the tape starts at the marker.
The take goes through §15.4's ring (push and return, never allocating, counting
what it drops) to a WAV whose header is rewritten after every block, into the
project's own `recordings/`, and then back in through **the same import path a
dropped file takes**.

### What the window found that no test could

- **Thirty-two microphones.** Opening the input menu on this machine listed every
  ALSA PCM: the same Scarlett four times, and most of the rest plumbing —
  *"Rate Converter Plugin Using Libav/FFmpeg Library"*, *"Plugin for channel
  upmix (4,6,8)"*. The list is filtered by **asking each device whether it will
  open**, which is a real question rather than a guess at what a name means, and
  deduplicated: thirty-two rows became seven. The same probe caught a second
  thing — ALSA's own `default` PCM calls itself *"Default Audio Device"* and then
  refuses to open for capture — so `default_input_name` goes through the same
  filter and the window cannot offer a default that cannot record.
- **A waveform that did not follow its own numbers.** The editor cached the
  preview beside the properties; the first fade stepped redrew the block on the
  timeline and left the strip in the window alone. It is read off the
  arrangement's own list at draw time now, so the two are literally one picture.
- **A menu opened off the right-hand edge**, which is how the thirty-two-row list
  was noticed at all.
- **The blade's stroke stayed on screen after the button came up.** The same
  fault as the one that made it invisible during the drag, pointed the other
  way: a marquee and a cut are drawn by the canvas and known to nothing in the
  document, so the frame that *clears* them has to be asked for on their own
  account too. Watching a take get cut into three was what showed it — the
  screenshots kept disagreeing with the arrangement.

### What is not done

- **No input monitoring.** The take is captured and plays back; you do not hear
  yourself through the track while recording. That needs the capture ring routed
  into the graph — a node and a second ring — and it is where latency and
  feedback live.
- **`ClipEq` and `time_lock`** from §15.1 are deliberately absent. Time-lock
  needs the stretch engine (§3.3, v2); a third tone control on a clip that
  already carries a whole multimode filter is a panel nobody can read.
- **Fade handles on the block** (§15.2). The fades are edited in the dialog and
  drawn on the block; dragging a block's corner does not yet make one, and the
  automatic crossfade on overlap is not built.
- **Long files are held whole.** §7.7's streaming threshold is about soundfonts;
  an audio clip is a take or a loop and holding it is the simple thing that
  works. A twenty-minute import is twenty minutes of `f32` in memory.
- **Drag-and-drop is still wired but not watched.** Unchanged from the pass
  before: `.wav` now joins `.mid`, `.fsc` and `.sf2` in `drop_file`, and all of
  them are tested, but no XDND drop has been synthesised against the nested
  server.

## 2026-09-03: files come in, and the roll grows a bench

> *"i want you to help implement a midi and fsc file import feature. should be
> able to drag the files in or in the piano roll there should be a tools tab
> which has various tools such as a randomizer (which should work like fl
> studios randomizer basically), a transposer (and it should also let you
> transpose all of your selections velocity or pan or whatever all at once
> adding or subtracting a value ...), and importantly a import/midi and
> import/fsc option which should if i dont have a folder selected yet, take me
> to the settings menu where i can select my preffered folder ... and should
> work cleanly with subdirectories."*

**Where the count went:** 2103 → 2265 across the workspace, 0 failing, clippy
clean at `-D warnings`.

### The `.fsc` format, read off FL's own library rather than guessed at

Image-Line publishes no specification for a piano-roll score file, so the
format in `fontelle-assets/src/fsc_import.rs` was read off **FL Studio's own
factory score library**: 609 files holding 5523 notes, written by every version
of FL from 3.0.0 to 20.9.0. Every rule the importer follows holds across all of
them with no exceptions, and `a_whole_library_of_real_scores_reads_cleanly`
(ignored by default, pointed at a real copy with `FONTELLE_FSC_CORPUS`) is that
claim — it reads all 609 on this machine.

Two things about it are worth writing down, because neither is guessable:

- **A note record is 20 bytes before FL 8 and 24 from FL 8 on**, and the
  version string is the only thing in the file that says which. The block
  length cannot settle it: 120 bytes is six narrow notes *or* five wide ones,
  and **240 of the 609 files are ambiguous that way**. A reader that guessed
  would drop a note and read every field of the others out of the wrong byte —
  silently, because every byte of a real note is a plausible value of some
  other field. A file with no version string is refused rather than guessed at.
- **Every field is centred where FL's knob is, not where Fontelle's is.** Pan
  is 0..128 about 64, fine pitch 0..240 about 120, release 0..128 about 64,
  velocity 0..128 where this document holds 1..127. Each is converted, and the
  two that cannot be converted cleanly are documented rather than fudged: FL's
  release *shortens* below its centre and this document's field only lengthens
  (so the bottom half collapses onto "the patch's own"), and FL's velocity 0 —
  which it really does write, on slide notes — becomes 1, because a note-on at
  velocity 0 is a note-off and dropping the note would lose the slide.

Fine pitch has a version rule of its own: before FL 3.3 the byte is present and
always zero, which read as a value is a full semitone flat. That is what every
note of the two oldest files in FL's own library would have imported as.

### MIDI: a survey before an import, and parts with names on them

A `.mid` may hold one part or sixteen, and which it is decides what importing
should even mean. `survey_midi` answers that **without building a document** —
so the question can be asked before anything is opened, and a "no" costs a file
read rather than a project.

A part is named from the best source it has: the track's own name, then the
General MIDI program it selects, then "Drums" for channel 10, then its channel
number. The track name is used **only when the track carries one channel** — a
format-0 file is one track holding every channel and its name is the *song's*,
so handing it to all sixteen would call every instrument in the piece the same
thing, which is worse than a number.

### Bringing them in

`ImportParts` is one command, so importing eight tracks is one entry in the
history and one press of Ctrl+Z takes all of it back. It could not be a
`Compound` of `AddChannel`/`AddLane`/`AddClip`: the clip has to name the channel
id that the `AddChannel` beside it is about to mint, and a `Compound` holds
commands built before any of them ran.

The two formats land in different places, and that is the difference between
them rather than an inconsistency:

- A **`.mid` is a song** — it goes in as instruments, arrangement rows and
  clips, each named, each on its own mixer strip at the level its CC7 asked
  for. More than one part and it asks first.
- An **`.fsc` is a phrase** — no instrument, no tempo, no arrangement — so it
  goes into the clip that is open, which is what FL's own *import score* does.

**The tempo is left alone** when the project already has clips in it. A file's
tempo is right for the file and wrong for the piece you are working on; the
status line says what the file's was instead.

### The Tools panel

A chip on the roll's toolbar (`T`) drops a panel of rows: a transposer, a
property offset that adds or subtracts across the whole selection, a
randomizer, and the two importers. Its shape is the settings tab's, because
that shape is already in this window: **a name, a value, and a click that steps
it — Ctrl+click steps back.** What the settings tab does not have is *action*
rows, and `Tools::action` is what tells the two apart so a press on "Randomize"
can never quietly change the amount above it.

The randomizer is a pure function over a **seed** rather than something that
reaches for entropy, which is what makes it re-rollable ("give me another one")
*and* testable across every property and a few thousand seeds. Two modes:
`Around` wobbles each note by up to *n*% of the property's range, keeping the
shape of a phrase you shaped by hand; `Anywhere` mixes in a fresh value by *n*%,
so the dial slides continuously from "leave it alone" to "forget what was
there". Every answer is inside what the property may hold — a randomizer that
could produce a velocity of 0 could silently delete a note.

### The Import tab, and the folders behind it

A fourth browser tab rather than a list inside the Tools panel, because
everything a browser of files needs is already there: folders you walk into, a
`..` row back out, a search across the whole tree, and a virtualised list.
`SoundfontBank` became `FileBank` with a `BankFilter`, so the sf2 bank and the
two import folders are one implementation rather than two that can disagree
about what a folder walk is.

Neither folder is guessed at (INVARIANT 10): with none set the Tools panel's
importer **sends you to the Settings tab** and says so, which is what was asked
for. The score folder is usually inside an FL Studio installation, which is
exactly why it cannot be guessed — on this machine it is under a Wine prefix on
a second disk.

### Three bugs the window found and no test could

Driven with synthetic input on a nested X server, against FL's real score
library and a three-part `.mid`:

- **A tab drawn with no words on it.** The label-shaping pass listed three
  modes by hand; the fourth drew as an empty box. `BrowserMode::ALL` is the
  fix, and the renderer's tab loop reads the same list.
- **A menu that was never drawn.** The main window drew menus for four named
  targets and the editor windows for two others; the import question matched
  neither, so it was *open* — holding the next click, swallowing Escape — and
  invisible. `MenuTarget::editor_window` asks that question once now, phrased
  as "which editor window", so a new main-window menu is nothing to remember.
- **A folder read only when something changed.** The import bank was rescanned
  when the *kind* changed, so opening the tab on the kind it already had —
  which is what "Import MIDI…" does on a fresh launch — browsed a bank that had
  never been read and reported "no sf2 files" over a folder full of MIDI. It is
  a lazy `ensure_import_bank` now: "is the bank the one the settings name?"
  cannot go stale the way "did something just change?" can. That one has a test
  (`a_folder_already_in_the_settings_file_is_read_without_anything_being_changed`),
  confirmed to fail without the fix.

The first two are the class this project has hit before and the reason
`docs/handoff.md` says to look at the window: the state was right, the geometry
was right, and only the draw filter was wrong.

### What is not done

- **Drag-and-drop is wired but not seen.** `WindowEvent::DroppedFile` is
  handled and `Session::drop_file` is tested for every kind — `.mid`, `.fsc`,
  `.sf2`, a file of the wrong sort, and a file that is not there — but
  synthesising an XDND drop against the nested server was not attempted, so the
  winit half has not been watched working. It is a few lines and they are the
  ordinary ones.
- **The randomizer does not randomize pitch or timing.** FL's Riff Machine
  does; this randomizes note *properties*, which is what humanising means and
  what the property lane already draws. Pitch randomisation mangles music and
  the transposer covers the deliberate case.
- **A score holding several instruments is flattened onto one clip.** Every
  file in FL's own library uses rack slot 0 only, so this has no test data
  behind it; `FscScore::notes_on` keeps them apart if a caller ever wants to.

## 2026-09-02: clips show what is in them, and a cut through a loop

Two reports from using the window, and one bug found only by looking at it.

> *"make it so the midi clips in the arrangement arent just blank rectangles
> but instead actually show a preview of the notes drawn out inside of it like
> how other daws do. ensure it actually displays cleanly so the sections
> actually line up with what youre editing."*
>
> *"i dont like that right now when i cut something that loops it seems to
> change the start and ending of the clip and that is weird. instead we should
> do more of what garage band does where you split the cut section from the
> rest of the loops so the rest remains a looped clip and the first part is
> just a cut clip of what you made... without making any edits the user didnt
> intend to make themselves essentially."*

**Where the count went:** 2080 → 2103 across the workspace, 0 failing, clippy
clean at `-D warnings`.

### The notes are in the block

`ClipInfo` carries the clip's own notes now, and `canvas::clip_notes` turns
them into rectangles. Three decisions, each with a test:

- **The pattern, not the passes.** A two-hundred-bar clip looping one bar
  would otherwise be two hundred copies of the same list, held per clip and
  rebuilt on every revision. The canvas tiles them the way it already tiles
  the seams (`loop_marks`), which is also what makes the notes and the seams
  one picture rather than two that can disagree — and
  `a_looped_clip_draws_every_pass_where_its_seams_say_they_are` is that claim.
- **It lines up.** A note is placed against the block's **whole** rectangle,
  which is the axis the ruler above it is drawn against — not against the part
  of the block on screen, which would slide as the arrangement scrolled. Bar 3
  of the clip is bar 3 of the song.
- **The compiler's rules, not a second set.** A note at or past the loop's
  period is content the loop does not contain, and a pass ringing past the
  clip's end is cut there. Both are what `fontelle_sequencer::compile` does, so
  the picture is of the song rather than of the document.

The pitch axis is scaled to the notes present with an octave floor, so a clip
of one note is a note rather than a slab, and a bass part sits low in its block
while a lead sits high — readable at a glance with no numbers on it. A block
too short to draw a note in draws none: a row of one-pixel smudges is less
readable than the plain block the arrangement used to have.

A note clip now has the **caption band** an automation block has, and the name
is written in it rather than across the middle: the middle is where the content
is drawn now. `canvas::clip_bands` is that split, one function for both kinds,
because a caption over the content of one and beside the content of the other
is two pictures where there should be one.

### A cut through a loop

A cut is no longer two loops. The right-hand half is **the loop, carrying on**;
the left-hand half is a **plain clip holding the notes that were sounding**,
its passes written out. Which is what the report asks for, and — more
importantly — the only version of it that does not change the song: left as a
one-pass pattern the piece you cut off would go silent after its first bar,
which is an edit nobody asked for.

`cutting_a_loop_changes_nothing_about_what_plays` is the sharp version of the
rule, measured over five cut points against every note the project sounds. It
passed *before* this change too, which is how the tail's existing rotation
was confirmed sound and kept: what the cut does to the head is the part that
was wrong.

`flatten_loop` follows the compiler's rules for the same reason the preview
does. Two answers to "what does this loop play" is a place for them to
disagree, and the disagreement is a cut that changes the song.

### A revision that never moved

Found by drawing notes in the real window and looking at the block: the roll
filled up and the arrangement went on showing an empty clip.

The window re-reads its lists only when `Session::revision` moves, and drawing
a note never moved it. That was harmless while a block carried nothing that
changed with its notes; the moment the block started showing them it was a
preview that never updated. **No unit test could have caught it** — every one
of them asks `Session::clips()` directly, and that has always answered
correctly.

The cause is worth recording because it is a shape: there were **three** ways
into the history — `run`, `apply_for`, and `Session::insert` calling
`History::apply` itself — and the third was the one that forgot. There are two
now, both bump the revision, and `insert` goes through `apply_for` like every
other command that hands its ids back.

### Verified in the window, not only in tests

The arrangement was driven with synthetic input over a real frame: notes drawn
in the roll appear in the block on the next frame, a right-drag on the ruler
draws the loop band on the ruler and down the grid, and the mode chip is on
the bar. Two headless render tests hold the parts that only a real frame can
answer — that a note's ink can be told from the block it is on, and that the
mode chip looks different in the two modes.

One thing that could **not** be settled that way: the last pointer move before
a button release is not always delivered by the synthetic-input harness, so a
right-drag sometimes commits a selection one grid step short of where it was
released. The gesture itself is `canvas::time_selection`, which is tested
directly; whether this is the harness or the window is open (see
`docs/handoff.md`).

## 2026-09-02: automation clips are edited where they sit

Four reports, and they are one feature between them:

> *"right now when i right click a knob and click create automation clip it
> opens the automation clip as a new separate window. i currently cant even do
> anything in this window no matter what i click... i want the automation graph
> to be a literal graph drawn inside the clip for that automation working like
> how fl studio automation clips work in the arrangement."*
>
> *"even the tempo section for example which i currently cannot turn into an
> automation clip."*
>
> *"we should have time looping selections like how fl studio works where you
> right click and drag on the time bar to loop a time section you are editing
> either in the piano roll or arrangement."*
>
> *"there should also be a way to swap between clip and song mode currently its
> always on song so you cant ONLY focus one instrument."*

Test-first throughout, and the tests earned their keep — three of the things
below are bugs no amount of reading would have found.

**Where the count went:** 2017 → 2080 across the workspace, 0 failing.
`cargo clippy --workspace --all-targets -- -D warnings` is clean, which it was
not at the start of the session: the four hits the last handoff recorded as
pre-existing, plus four more in test files, were fixed before the first commit.

### The window is gone; the block is the editor

`EditorKind::Automation` no longer exists. An automation clip is edited inside
its own block on the arrangement, which is what the report asked for and what
makes the feature reachable at all — the window it used to open was drawn
against a layout nothing kept up to date, so clicking in it did nothing
visible.

A block now has an **anatomy** (`canvas::automation_block`): a caption band
across the top, which is the clip as every other clip is — grab it to move,
select, resize or erase — and under it the **curve area**, where a click on
bare curve makes a point, a drag carries one, and a right-click on one opens a
menu of the six shapes plus Delete. `ClipPart` gained `Point(PointId)` and
`Curve { tick, value }`; `ArrangeEdit` gained `AddPoint`, `MovePoints`,
`RemovePoints` and `SetPointCurve`, so a point edit is an arrangement edit and
goes through the same history as everything else.

**Two bugs the tests caught, both invisible by inspection.**

- **The last point of every automation clip could not be grabbed.** A clip is
  created flat with a point at each end, the far one sits on the block's
  right-hand edge, and `timeline_hit` checks the resize grip *first* — so
  every press aimed at that point was a press on the grip, and dragging it
  resized the clip instead. The curve area now stops half a handle short of
  where the grip begins (`canvas::clip_grip`, one answer to "where does the
  grip start", read by both). Sizing still works; the point can be taken hold
  of.
- **The caption band ate the curve on a short block.** A fixed four-pixel
  inset out of a fourteen-pixel lane left less curve than caption, which is
  the wrong way round on a block whose whole content is its shape. The
  vertical inset scales with the block now.

**The curve in the block is evaluated by the same function the audio thread's
values come from.** `fontelle_model::curve_value` takes points already in time
order; `AutomationData::value_at` is that function over a sorted copy of the
arena, and the canvas calls it directly on the flattened `Vec<CurvePoint>` the
host hands over. Two evaluators would be a place for them to disagree, and the
disagreement people hit is a shape you chose that draws as a straight line.
`ClipInfo::curve` is points with **ids** now rather than `(tick, value)` pairs,
because the block is edited through them.

Two headless render tests hold what only a real frame can answer: that the
curve's ink is on screen against a ground you can see it against (a curve
whose colour matched its own background is a bug this project has actually
shipped), and that a rising curve is drawn rising.

### The tempo is a parameter like any other

§12.3 has always said so — *"every mixer track volume/pan/send level, and
**tempo**"* — and the tempo box was the one control with no right-click menu.
It has one now, and the whole of what made it hard is that a tempo lane is not
a `ParamValue` on the wire: **the tempo map is generated by evaluating the
tempo automation**, which §12.3 also says, and which nothing implemented.

`fontelle_model::effective_tempo_map` is where the two become one: the
document's own map, with any clip aimed at `transport/tempo` sampled every
sixteenth note into constant segments, a run of equal steps kept as one so a
flat lane costs one segment. The compiler converts every tick through it, so a
ritardando moves the notes after it; `CompiledTimeline::tempo` carries the
ramp to the audio thread; and the window's clock reads the same map, which is
what keeps the playhead in the bar the notes are in.

`fontelle_types` publishes the one mapping between a normalised lane value and
a BPM (`TEMPO_MIN_BPM`..`TEMPO_MAX_BPM`, 20 to 300). It is narrower than the
box's own range on purpose: a lane to a thousand puts every ordinary song in
the bottom tenth of the block, where a curve is a flat line nobody can edit.

### A clip that spans something worth drawing in

*"it creates a new automation clip in my arrangement just flat on the value
that its currently at basically with the clip extending the current length of
the song or time selection."* All three halves of that sentence changed:

- **Flat, at the value the control is at now** — that part was already true
  and is kept.
- **Over the time selection, or the whole song.** It used to be one bar at the
  playhead, which is a clip you have to stretch before you can draw anything.
- **A second gesture on the same control hands back the clip it already has**,
  rather than a second clip on the same lane. That was defensible while a clip
  was one bar; now that a clip spans the song, a second would sit on top of
  the first, and §12.2's rule for two clips over one target is "the later one
  wins" — which here means a curve silently replacing the one you drew.

Nothing opens. The lane is on the arrangement, selected, and that is where it
is drawn in.

### A time selection, on either ruler

`Project::loop_range` has been document state since 2026-08-29 and there was
no way to set it from the window. Right-drag either ruler — the arrangement's
or the roll's — and the stretch you drew is the loop: `canvas::time_selection`
is the arithmetic, one function for both because the gesture is the same, and
a drag whose ends land on the same grid line is a click, which **clears** the
selection. That is what makes one button both make and unmake a loop.

The roll's ruler works in the clip's ticks and the document stores the song's,
so `DocumentHost` gained the two conversions. One loop, drawn on both rulers
and as a band down both grids.

The range reaches the transport in **both** units together (§6.3's reason: the
RT thread cannot run a `TempoMap` lookup against a map this thread may be
editing), which is why `Session` now holds the `Transport`. A tempo change
moves the loop's samples and leaves its bars alone, and there is a test that
says so.

### Song and clip

A chip on the transport bar, and `Ctrl+L`. In clip mode the timeline carries
**one clip** — `fontelle_sequencer::CompileScope::Clip` — and the transport
loops that clip's own bars; leaving it puts the time selection back.

Two decisions worth recording. A clip scope leaves **other lanes' automation
out**, tempo included: a lane on another row is not part of the part being
soloed. And the window's cached tempo map follows the same scope, which is a
bug the test for it caught — the playhead was being drawn through the
automated map while the notes had been compiled without it, so in clip mode
with a tempo lane the playhead was in the wrong bar.

### The transport bar learned to give something up

Adding the mode chip took the ruler to **nothing** at 640 logical pixels — a
width the window opens at — and the playhead then drew at the same pixel
wherever the song was. The bar's four read-outs are now dropped from the right
when there is not room for them and an 80-pixel ruler: the mode chip goes
first, the position read-out last, because "where am I in the song" is the
other half of what the bar is for. The same rule the roll's toolbar has always
followed. `Ctrl+L` exists so the mode is reachable at a width the chip is not.

### Still open

- **A tempo lane costs a map rebuild per republish**, and the rebuild calls
  `automation_at` once per sixteenth note, each of which sorts a copy of the
  clip's points. A song with no tempo lane pays nothing (the function returns
  the document's own map), so this only bites once somebody automates the
  tempo. Worth a sorted cache on `AutomationData` if it ever shows.
- **`effective_tempo_map` is built twice per republish** — once for the
  session's cache and once inside `compile_scoped`. Same note applies.
- **Only constant segments.** A tempo ramp is a staircase at a sixteenth note,
  not a true interpolation; `TempoMap` still has the scope cut §6.2 records.
- **Points cannot be marquee-selected** inside a block, and a drag moves the
  ones that are selected. Several at once works; selecting several does not
  yet.

## 2026-09-02: four effects, a preset picker, and the external sidechain

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
