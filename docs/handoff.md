# Handoff

Written at the end of a session that closed a long list of reports from using
the studio. Read this **after** `PROGRESS.md`'s top two sections and before you
touch anything.

`PROGRESS.md` says what the project *is* and what was built. This says what
state the tree is in, what is still open, and the handful of things that cost
this session real time to discover.

---

## 1. Where the tree stands

Branch `main`. Everything described in `PROGRESS.md` is **committed** — the
long uncommitted stretch that ran from `ee06e6b` through ten sessions was
landed on 2026-09-02, and the automation pass after it.

**Updated 2026-09-07 (third session).** `cargo test --workspace` is green at
**3471 passing** and `cargo clippy --workspace --all-targets -- -D warnings`
is clean. The eight new tests are the stuck-note regression in
`fontelle-core`'s sampler and the seven menu-scrollbar tests in
`fontelle-ui/tests/context_menu.rs`; see `PROGRESS.md`'s top entry.

**Updated 2026-09-07 (second session).** `cargo test --workspace` was green at
**3463 passing** and clippy is clean. Flopsynth's bank is now **210 presets**;
the window and the bank are the top two 2026-09-07 entries in `PROGRESS.md`.

**Updated 2026-09-06.** `cargo test --workspace` is green at **3218 passing**
and clippy is clean; the plugin follow-ups below that said "not done" are done
— read `PROGRESS.md`'s 2026-09-06 entry first, then this file for the parts of
it that are about the machine rather than the code.

`cargo test --workspace` was green at **3018 passing** and
`cargo clippy --workspace --all-targets -- -D warnings` is **clean**. Keep it
that way: clippy at `-D warnings` is part of the bar now, and every hit the
older handoffs recorded as "pre-existing" has been fixed.

Everything since `7ce3a23` — the drum machine, CLAP hosting, and the
2026-09-04 session (drum kit axes, the marked preset chip, LV2 hosting, the
bridge seam; see the top of `PROGRESS.md`) — is in the **working tree,
uncommitted**, because nobody asked for a commit. Read `git status` before
assuming anything about `HEAD`. Three new crates are in the workspace:
`fontelle-testlv2`, `fontelle-bridge-abi`, `fontelle-testbridge`.

## 2. Process rules that are not optional here

- **Tests first, confirmed failing.** Written against the intended API, run,
  seen to fail (a compile error against a missing function counts), *then* the
  implementation. `PROGRESS.md` states this at the top and every section in it
  claims to have been built this way. Do not break the chain.
- **The invariants in `FONTELLE_TDD.md` are hard.** The ones that bit this
  session: INVARIANT 1 (the RT thread never allocates — see `patch_params`,
  which runs on it), INVARIANT 2 (the UI never mutates the document; it emits
  edit values), INVARIANT 4 (`fontelle-core` depends on nothing above it),
  INVARIANT 7 (a parameter address never changes, and one this build does not
  recognise changes nothing and is *not* an error).
- **Update `PROGRESS.md` when you finish a chunk.** It is the project's own
  handoff mechanism and it is expected to stay honest.
- **Comments carry the reasoning**, not the mechanics. Match the surrounding
  density; quote the report a fix came from when there is one. The codebase is
  consistent about this and it is why the invariants survive.
- **The workspace suite runs once, at the end** (Ty, 2026-09-07: *"the full
  test suite takes far too long making small requests turn into several hour
  long change sessions"*). In the loop, run the binaries you touched —
  `cargo test -p <crate> --test <file>` — and `cargo test -p fontelle-ui` for
  anything in the window. Never run two workspace suites at once; the second
  dies with SIGXCPU and reads as a failure. And **look at anything visual**:
  the headless dump (§5) for a scene, the nested X server for the studio. The
  regression that mattered that day — the window opening with no panels — had
  no failing test and could not have.

## 3. What is still open

Ranked by what I would take first.

0. **Prefabs are built, mirror-only, and two things wait on decisions.**
   `PROGRESS.md`'s 2026-09-07 entry is the build. Open: (a) *no gesture* for
   "make a prefab from this clip" — `StudioHost::make_prefab_from` exists and
   is tested, but the arrangement's right button erases, so a clip context
   menu is a change to an existing gesture and Ty's call; (b) property
   overrides need notes to carry a `PersistentId` — `OverrideMap` is in the
   format, `prefab::apply` deliberately does nothing and says why; (c)
   variants (`base: Some`) are structure-only. The rule that keeps all of it
   honest: read a clip's content through `Project::clip_source`, never
   `clip.source` — a place's own source is empty.

0a. **The preset system and Flopsynth are built, and the window has been
   looked at.** `docs/flopsynth-plan.md` is the design; `PROGRESS.md`'s
   2026-09-06 entries are the build and its 2026-09-07 entry is the session
   that opened the window on the Init patch and fixed what that found (the
   page overflowed, the Presets page was blank, the drop-down was one column
   three thousand pixels tall). What is still open there: dragging an effect
   slot above its neighbour (§8.5), the About column's macro and wheel lines
   (§8.6), and the cost per voice, which is over §10's budget and is a design
   conversation rather than a target to loosen. The light theme has not been
   looked at on the new window — everything is palette-mixed, but nobody has
   opened it.

   **The bank was widened to 210 on 2026-09-07** (the entry above the window's).
   If you add rows, the tool to use is
   `cargo run -p fontelle-core --example preset_probe --release`, optionally
   with a category name: it renders the bank once and prints each preset's
   loudness trim, its peak, and its nearest neighbour on the five axes, so the
   `.out(…)` column is measured rather than guessed and a collision tells you
   which axis it is on. Two traps it exists to catch are written up in that
   entry — the sub bypasses the filter, and an effect cannot make two presets
   different, because the pairwise test renders without the chain.

   **Nobody has listened to the eighty-two new ones.** The tests prove they are
   distinct and level-matched, which is not the same as good;
   `cargo run --release -- --play-flopsynth "<name>"` is how you check, and
   `list` prints all 210.

1. **Nobody has heard input monitoring through a speaker.** Capture *is*
   proven on this machine now: `fontelle-engine/tests/pipewire_sources.rs`
   has an ignored test that opens the first PipeWire source and counts the
   frames (`cargo test -p fontelle-engine --test pipewire_sources -- --ignored`).
   The loop out to the speakers — `MonitorNode` through a realised graph in
   the real window — has still not been listened to. Arm a real input in the
   window and listen; it is a one-minute check.

   Two things about it to know before you look. The device is opened because a
   **mixer track names an input**, not because record was pressed
   (`Session::sync_audio_input`, called once a frame from `pump`), so
   monitoring starts the moment you choose one. And on a PipeWire desktop the
   input list is **PipeWire's** (`crate::pipewire`), opened through its ALSA
   plugin so another program holding the device is no longer a problem; the
   old ALSA probe list is the fallback for machines without it. The
   "exclusive capture" blind spot the previous handoff recorded is therefore
   gone here, and `audio_inputs` putting the open device back at the head of
   the list is belt-and-braces now.
2. **The arrangement's double-click has now been driven; the rest of its
   gestures have not.** Driving it found the bug at the top of `PROGRESS.md`
   — the double-click was timed against the moment the window *reached* the
   press, so a frame longer than 400ms ate the gesture — and none of the
   canvas-level tests could have: the fault was in the event loop, not in
   `double_press`. Stamping, the fade handles and the node are still
   unclicked, as are the `1`/`2` tab keys. Cheapest check: a nested X server
   and the XTEST script in the `seeing-fontelles-gui` memory note.

   Two things that turned up beside it and are **not** fixed. The arrangement
   scrolls **past its last lane** into a blank grid — the view's `top_lane`
   is not clamped — and a double-click down there still makes a clip, on the
   last lane, off screen where nobody sees it. And `Session::draw_prefab`
   does not clamp the row the way `ArrangeEdit::Add` does, so with a prefab
   picked, a double-click below the last lane silently draws nothing at all.
3. **Fade handles show only on the selected block**, and the automatic
   crossfade has no switch. Both are decisions made to ship, recorded in
   `PROGRESS.md`; the second is a departure from §15.2's "offers".
4. **An un-routed mixer track says so in the track-options column and in its
   output menu, and nowhere on the strip.** `MixerTrack::output_on` is new and
   a track with it off is silent by design — but "why is this one silent" with
   no mark on the strip is the exact shape of invisible state this project
   keeps rediscovering. A strip-level mark wants `MixerStrip` to carry the
   flag, which is a field four test files build by struct literal.
5. **Drag-and-drop has not been watched working.** The import pass wired
   `WindowEvent::DroppedFile` and `Session::drop_file` is tested for every kind
   (`.mid`, `.fsc`, `.sf2`, a file of the wrong sort, a file that is not
   there), but synthesising an XDND drop against the nested X server was not
   attempted, so the winit half is unproven. Everything *else* about importing
   was driven in the real window — see `PROGRESS.md`'s top section, which also
   lists the three bugs that found. Cheapest check: drag a `.mid` onto the
   window by hand.
6. **The effects catalogue's build order, from item 5.** `docs/effects-catalogue.md`
   §4 is the list and §5 is the recipe; items 1–4 (utility, gate, chorus,
   filter) are done, and both of the things that gated several rows — the
   preset picker and the external sidechain — are done too. Next is the
   **limiter as an insert** (the DSP exists; an afternoon), then the
   compressor's character/lookahead/link extensions, then delay and reverb.
   **Presets are owed** on the EQ, the compressor, the chorus, the delay and
   the reverb, and rule 10 has a test that stops a new effect landing in the
   "no presets" list by accident.
7. **Automation: what the block cannot do yet.** Points are placed and dragged
   one at a time — there is no marquee over several inside a block, and no
   pencil/line/shape-stamp draw modes (§12.4 names all three). The gestures
   that exist are in `fontelle-ui/tests/automation_blocks.rs`.
8. **A tempo lane is a staircase**, not a ramp: `effective_tempo_map` samples
   it every sixteenth note into constant segments, because `TempoMap` holds
   only constant segments (§6.2's scope cut). Interpolated segments are a
   bounded addition and the formula is written down in `project.rs`.
9. **`effective_tempo_map` is built twice per republish** and calls
   `automation_at` once per step, each of which sorts a copy of the clip's
   points. A project with no tempo lane pays nothing, so this only bites once
   somebody automates the tempo.
10. **Unverified: the last move of a right-drag on a ruler.** Driving the
   window with synthetic input, a right-drag that selects a time range
   sometimes commits one grid step short of where the button came up — the
   final `MotionNotify` before the release does not always reach the window.
   `canvas::time_selection` is tested directly and is right; whether this is
   the XTEST harness (which this file already records as unreliable near a
   button release) or the window's own event handling is **not settled**.
   Check it with a real mouse before spending time on it.
11. **Unverified: raising an already-open editor window.** The code calls
   `focus_window()` *and* `request_user_attention()` (the Wayland
   xdg-activation path). It could not be verified here — the test harness is a
   bare nested X server with no window manager, so there is nothing to raise
   against. Check it on a real KDE session before believing it.
12. ~~**The gate's look-ahead is uncompensated latency.**~~ **Done
   2026-09-06** (TDD §5.5): the graph holds every other track back to meet a
   track that looks ahead, an insert delays its own dry so a mix below 100 %
   no longer combs, and `Realised::latency_samples` says what the whole
   configuration costs. What is *not* compensated, and says so in the code: a
   plugin **instrument** that reports latency (every channel adds into one
   shared bus), and the live bypass switch, which moves a track by its
   insert's latency until the next rebuild.
13. **The ducker, the vocoder and the repitcher** are unwritten; the repitcher
   is varispeed over a *clip* and is in the wrong crate.
0. **Plugins are installed on this machine now** (2026-09-05): `surge-xt-clap`
   in `/usr/lib/clap`, and Calf, LSP, x42 and Dragonfly LV2 in `/usr/lib/lv2`
   — 370 in all, 13 instruments and 357 effects, scanned with no failures.
   `sudo pacman -Syu` first if the databases are stale; the mirrors 404 on the
   exact versions otherwise, which is what a six-week-old sync looks like.

14. **No plugin has been heard through a speaker.** CLAP and LV2 hosting are
   proven end to end in tests — `fontelle-app/tests/plugin_hosting.rs` renders
   hosted instruments and effects of both formats through a realised graph —
   but against `fontelle-testplug` and `fontelle-testlv2`, which this
   repository wrote. Nobody has installed a real plugin and played it in the
   window. Do that before trusting it. This machine has none installed
   (2026-09-04): `sudo pacman -S surge-xt` gives a CLAP synth, and
   `lsp-plugins-lv2 calf x42-plugins dragonfly-reverb` give LV2 effects, all
   found in `/usr/lib/lv2` and `/usr/lib/clap` without configuration.
   Building the fixtures is a separate step: `cargo build -p fontelle-testplug
   -p fontelle-testlv2 -p fontelle-testbridge` — a test that only *depends* on
   them builds their rlibs and the helpers say so.
14a. ~~**LV2 has no state extension yet.**~~ **Done 2026-09-05**: `lv2_state`
   is hand-rolled over raw `lilv-sys`, reached through `ProcessorBay::recall`
   while the graph plays, and an LSP sampler's loaded file survives a save and
   a cold reopen — verified in the window. Paths are identity-mapped
   (§17.4's "reference, never copy"); *copying* a plugin-loaded sample into
   the project is the import prompt's job and is still not built.
14b. **The VST3 bridge is not written** — and it is now the *only* plugin
   item left, because everything else in `docs/plugin-compatibility-plan.md`
   is closed. The seam is here (`fontelle-bridge-abi` at **ABI 3**, which
   carries notes, a performance and an editor; `fontelle_host::Bridges`;
   `fontelle-testbridge` proving the loader against all three). A bridge
   built against Steinberg's SDK in a *separate, private* repository is what
   makes a `.vst3` load, including the Windows ones `yabridge` produces. Drop
   the built `.so` in `~/.local/share/fontelle/bridges/`. A bridge that will
   not load is reported through the rack's message. **A bridge built against
   ABI 2 is refused**, by design: implement the three new entry points.

14d. **Clip ends cut what they play** as of 2026-09-06, for plain clips as
   well as looped ones, and every *pass* of a loop is cut at the loop point
   (TDD §11.4). A clip does **not** grow to contain a note drawn past its end
   — that was tried and corrected in the same session — so a note out there
   is silent by design, and the roll shades the grid past the end to say so.

14c. **The plugin work that landed 2026-09-06**, so nobody re-opens it: the
   offline bounce hosts plugins (`fontelle_app::bounce`, verified with the
   real binary), LV2 sidechains are read off `lv2:isSideChain`, slides bend a
   hosted instrument, the mod wheel / bend / aftertouch reach a **built-in**
   voice (and imported soundfonts carry SF2's own default wheel vibrato), and
   the bridge ABI carries a performance. What is *still* open beside the
   bridge: latency compensation (a §5.5 engine pass — `latency_samples`
   exists and nothing reads it), per-note pitch over the bridge ABI, and
   copying a plugin-loaded sample into the project.
15. **A hosted plugin has no editor window of its own.** It is edited on
   Fontelle's generic parameter panel, which is fine for a compressor and much
   less fine for a synth whose sound is drawn. `clap_plugin_gui` plus a child
   window is the work; §16 says the windowing layer was built multi-window for
   exactly this.
16. **A plugin that reports latency is not compensated for.**
   `PluginNode::latency_samples` returns zero, which is honest about what this
   build does and wrong about the plugin. It waits on delay compensation with
   the gate's lookahead and the master limiter's.

## 4. Architecture notes that cost time to learn

- **The window re-reads the studio only when `Session::revision` moves.** A
  mutation that changes the sound and forgets to bump it is *audible and
  invisible* — the EQ shipped that way once and it looked like a dead panel.
  Any new `StudioHost` write should bump it.
- **`param_nodes` is what makes automation reach anything.** It maps a
  `ParamAddress` to the engine node that owns it, built in `realise`. An
  address missing from that map emits no events at all, so the lane is made,
  drawn, saved — and silent. There is a test asserting the instrument panel's
  own list and this map are the same list; keep it that way.
- **`fontelle_core::patch_params` is the one table** for reading and writing a
  patch's parameters by address. The panel draws through it and the audio thread
  applies automation through it, which is what makes "every knob is automatable"
  true by construction. It runs on the RT thread: no allocation, `&str` splits
  only.
- **`insert_config` reads the document**, not the live control surfaces, so
  `insert_view` and `eq_config` show what is saved rather than what was last
  published.
- **Editor windows are separate OS windows.** They read `self.cursor` (not the
  event position), they need their own `ModifiersChanged`, and a keystroke aimed
  at one never reaches the studio's handler — `global_key` is the set that means
  the same thing everywhere (transport, history, save, export). Canvas keys
  deliberately stay per-window: Delete means "the selected band" in an EQ and
  "the selected notes" in the roll.
- **A clip block has two bands** (`canvas::clip_bands`): a caption across the
  top and the content under it. Both kinds of clip use it — an automation
  block's curve and a note block's preview sit in the same place, and the
  name is written in the band rather than across the content.
- **A note preview is the pattern, tiled** — `ClipInfo::notes` is one pass and
  `canvas::clip_notes` repeats it, using the same arithmetic `loop_marks`
  uses, so the notes and the seams cannot disagree. It follows the
  **compiler's** rules for what a loop plays (a note past the period is out, a
  pass past the clip's end is cut), because two answers to that question is a
  picture of a song the document does not play.
- **Cutting a looped clip makes a plain clip and a loop**, not two loops: the
  head's passes are written out (`flatten_loop`) and its `loop_length` is
  cleared, the tail carries on rotated to the phase the cut fell on. The rule
  that matters is that the song sounds the same either side of a cut, and
  `cutting_a_loop_changes_nothing_about_what_plays` measures it.
- **Every accepted command moves `Session::revision`**, and there are exactly
  two ways into the history (`run` and `apply_for`) so that it cannot be
  forgotten. There used to be a third, and it was the one that forgot — notes
  drawn in the roll never reached the block on the arrangement, and no unit
  test could see it because they all ask `Session::clips()` directly.
- **An automation clip is not a window.** It used to be, and the window could
  not be made to do anything. It is edited inside its block on the
  arrangement: `canvas::automation_block` is the anatomy (a caption band, then
  the curve area), `ClipPart::Point`/`Curve` are what the hit test returns,
  and `ArrangeEdit`'s four point variants are what the gestures emit. The
  block's curve is evaluated by `fontelle_model::curve_value` — **the same
  function the audio thread's values come from**, so what is drawn is what is
  heard.
- **The resize grip and the curve area must not overlap.** `canvas::clip_grip`
  is the one answer to "where does the grip start", and the curve area stops
  half a handle short of it. Before that, the last point of every automation
  clip sat under the grip and could never be grabbed — a clip is created with
  a point at each end, so this was every clip.
- **Everything that turns a tick into a sample goes through
  `Session::effective_tempo`**, never `project.tempo_map`. The latter is the
  tempo *box*; the former is that map bent by the tempo lane, and it is
  rebuilt against the same scope the timeline was compiled with. Reading the
  wrong one draws the playhead in a bar the notes are not in.
- **There are two effect windows behind one `EditorKind::Effect`**: the EQ's
  curve, and the grid of knobs every other effect gets. They are told apart by
  `self.eq.is_none()`, and exactly one of `eq` / `insert_view` is `Some`.
- **The generic effect panel is grouped by `EffectConfig::sections()`**, a
  list of `(name, count)` runs over the spec table. Add a parameter to a
  table and you must add one to a count, or the types test fails.
- **A live input takes a different ring from a take.** `InputWriter` goes to
  the disk thread; `InputMonitor` goes to the output callback. One ring with
  two consumers would have them stealing each other's samples, and the loser
  would be the recording. The input callback writes both in the same breath.
- **The device is open because a track names an input.** Not because record
  was pressed — that is what makes monitoring work with the transport stopped,
  and it is why `Session::armed_track` is "the track that names one" rather
  than "the selected strip". The consequence to remember: the stream being open
  is *not* the take being kept, so `pump` always drains the capture ring and
  only accumulates while `capturing`.
- **`Mixer::reaches_master` is the one answer to "can this be heard".** Both
  halves of the routing report read it — whether monitoring is audible, and
  where a take has to be put when it is not. It is a walk, not one hop.
- **`Session::touch()` is the one way the revision moves.** It also rebuilds
  `roll_notes`, the view the piano roll is handed (the open clip's notes on
  the selected channel). A bare `revision += 1` would leave the roll a
  revision behind the document, which is the bug that made this a rule.
- **The rack's selection is the instrument every interaction means.** A
  drawn clip, a drawn note, a pasted note, a recorded note: all on the
  selected channel, whatever clip they land in. A clip is a place; a channel
  is an instrument. `Note::channel_or(home)` is the one resolution, and the
  compiler, the roll and the caption all go through it.
- **An overlap is one function's answer twice over.**
  `canvas::clip_overlaps` gives both the rectangle the stripes go in and the
  two crossfade curves drawn over it, so the marking and the shape cannot
  disagree about where the blend is. The curves follow the *compiler's*
  rules (equal power, times each clip's own fade, and only a clip the
  overlap carries to its end falls), because what is drawn has to be what is
  heard.
- **A fade is three pictures and one function.** `Fade::at` is read by the
  player, the block on the arrangement and the editor's waveform; the block's
  handles are `canvas::fade_anatomy`'s rectangles, which are also what the
  pointer is tested against. The canvas speaks in fractions of the block and
  the session converts to frames of the clip's own audio — the same
  conversion `audio_preview` makes the other way.
- **The crossfade belongs to the placement, not the clip.** `crossfade_in` /
  `crossfade_out` are worked out by the compiler from the overlap; a clip's
  own fades go with it wherever it is put. Both apply in the player.
- **Clip edges are drawn after every body**, in a pass of their own, or a
  block painted later hides the end of the one before it.
- **`MixerTrack::output_on` is a routing switch, not a third mute.** The bus
  sum *is* the routing edge, so `realise` simply does not schedule one; the
  track's **sends still carry**, which is how a send-only track is built.
- **Do not clamp after the oversampling filters at the base rate.** A
  band-limited square overshoots by a sixth; clamping that at 48 kHz is the
  aliasing the oversampling exists to prevent. It cost a diagnostic to find
  and is written on the code in `fontelle-fx/src/distortion.rs`.

### Hosting plugins (added with `fontelle-host`)

- **A CLAP plugin is handed every port it declared, every block.** Not the
  main pair: nih-plug reads its auxiliary ports off the end of whatever
  array it was given, and the OneTrick drum synths (eight to eleven output
  ports) crashed in their first block when handed one. `PortLayout` in
  `plugin.rs`, one buffer set per port in `ClapProcessor`. The sine fixture
  declares a *sub* port and refuses to render without it, so this cannot
  quietly regress.
- **A `false` from a CLAP `show` is advisory.** clap-helpers' default
  returns it; SpectMorph and OneTrick never override it. Reading it as a
  refusal is *"the window closes as soon as it opens"*.
- **An LV2 editor may hold the running instance** (`instance-access`, which
  every DPF UI requires). Close the editor before the instance goes:
  `HostedPlugin::activate` and `PluginRack::retire` both do, and anything new
  that drops an `Lv2Processor` must too.
- **Probe a plugin outside the window first**: `cargo run -p fontelle-host
  --example plugin_editor -- <bundle>` on `DISPLAY=:1`, then `coredumpctl -1
  info` and `gdb -batch` when it dies. See PROGRESS.md 2026-09-05 (latest).
  When `coredumpctl info` shows one frame at address zero, dump the core
  (`coredumpctl dump <pid> -o core`) and `gdb -batch -ex bt` it: gdb unwinds
  a null call, systemd's walker does not.
- **Any C function pointer a plugin may leave NULL is read as an
  `Option<extern "C" fn>`**, never a plain fn type — LV2's `port_event`,
  `extension_data`, the idle interface. A NULL read through a non-nullable
  type is UB and the release optimiser drops the later check; JuceOPL's NULL
  `port_event` crashed the studio three times in a minute. `lv2_ui` has its
  own `#[repr(C)]` descriptor for exactly this.
- **A plugin instrument adds into its bus.** `PluginNode` renders into its
  own scratch (sized in `prepare`) and sums; writing over the bus silenced
  every other instrument on the same track.
- **An LV2 plugin's state is on the instance, in the processor.** Save it
  with the processor in hand: `HostedPlugin::snapshot_with`, fetched from a
  playing graph through `ProcessorBay::recall` (the node parks it at the top
  of its next block). Restore is applied at `Lv2Plugin::activate`, before the
  first block. `mapPath` is identity — the blob holds absolute paths.
- **A key on a plugin insert is always an edge**; whether the plugin has a
  sidechain port (`HostedPlugin::takes_key`) is the host's knowledge. The
  key goes to the first non-main CLAP input port; LV2 `isSideChain` is not
  read yet.
- **Controllers are `EventPayload::Controller` / `PitchBend` /
  `ChannelPressure`**, performance events distinct from `ParamValue`, sent
  to a plugin in its note port's dialect (`NoteDialect`): MIDI bytes when the
  port takes MIDI, note expressions otherwise.
- **The idle gate must stay awake under an open plugin editor.** §6.3's
  gate runs no nodes while the song is stopped and nothing is held or
  ringing; an LV2 editor reaches its plugin only through `run`, so LSP could
  never be handed a file. `Transport::set_attended` (from
  `tick_plugin_editors`) is the fifth reason. Symptom, if it regresses:
  `FONTELLE_ATOM_TRACE=1` prints *to plugin N/0 taken* and a block count
  that stops moving.
- **A project that names plugins is hosted on the session's first `pump`.**
  `main.rs` realises the window's first graph before the session exists
  and without a rack; without that hook a reopened project was silent
  until the first edit. `--render-wav` still has no rack at all.
- **Two probe switches:** `FONTELLE_ATOM_TRACE=1` (editor atoms sent vs
  taken, blocks the plugin ran, bay recalls — works in the studio too) and
  `PROBE_THREADED=1` on `plugin_editor` (the processor on its own thread,
  as the studio runs it).

- **A plugin cannot belong to a graph, and this is the reason.** CLAP says a
  plugin may be activated once, and its main-thread handle and its audio
  processor are separate objects on separate threads; `clack` enforces both in
  the type system, and it **leaks** a `PluginInstance` dropped while its
  processor is still out rather than freeing it on the wrong thread. This
  engine, meanwhile, builds the replacement graph *while the old one is still
  playing*. So the plugin lives in `fontelle_app::PluginRack` and the graph
  borrows a `ProcessorBay` (where the processor waits between graphs) and a
  `ParamValues` (the atomics a knob writes). A retired `PluginNode` parks the
  processor in its `Drop`, which runs in `GraphPublisher::reclaim` — already
  *the* one place a live graph dies, on the main thread. If you ever move where
  graphs are freed, this breaks quietly: the plugin just goes silent.
- **A plugin node passes signal through when it has no processor**, rather than
  writing silence. That is the handful of blocks between a graph swap and the
  next reclaim, and a hole in the mix would be worse than an unprocessed one.
- **`cargo test -p <crate>` can run against a stale test plugin.** Depending on
  `fontelle-testplug` builds its *rlib*; the `.clap` the host tests load is the
  *cdylib*, which only a build of that package produces. Half an hour went into
  a "reset does not work" that was a plugin from before `reset` was written.
  The helpers now refuse to run if the built plugin is older than its source —
  when they do, `cargo build -p fontelle-testplug`.
- **The test bundle is copied once per test binary, through a rename.** Once,
  because tests inside a binary run in parallel and two of them writing one
  staging file produced a "file too short" from `dlopen`; through a rename,
  because two binaries run in parallel too.
- **The document is the source of truth for a plugin's knobs, and something has
  to carry that back.** An undo changes the document and nothing else — the
  plugin is still set the way the gesture left it. `PluginRack::ensure` writes
  the document's values onto the plugin on every realise, and a parameter the
  document does not mention goes to the plugin's **default**, which is what
  makes undoing a first touch put a knob back where it started.
- **A hosted plugin's parameters are stored plain, not normalised**, unlike
  every built-in. CLAP's advice, and the panel converts through
  `HostedParam::normalise`/`plain` — which means the panel needs the plugin
  *open* to draw a value, and a channel whose plugin is missing draws no knobs.
- **`EffectSlot::kind()` returns `Option<EffectKind>` now.** `None` means the
  slot holds a plugin and every question about `EffectConfig` is the wrong
  question. If you add a place that reads `slot.config` directly, ask
  `slot.config()` instead.

### Raising a window on the user's desktop (Wayland / KDE Plasma)

**Read this before touching anything that opens or focuses an editor window.**
It was reported three times and re-derived from scratch twice; the third
attempt is the one that works, and it works because it was read out of KWin's
source rather than guessed at.

The symptom ladder, in the order it was walked:

1. *"clicking it does nothing"* — `Window::focus_window()`.
2. *"its still not bringing the windows to the front"* — the above plus
   `set_window_level(AlwaysOnTop)` held for one frame.
3. *"it does flash in my taskbar like its trying to focus that window but its
   not actually bringing the window to the front"* — an xdg-activation token
   with no serial on it.
4. Works.

**Steps 1 and 2 could never have worked.** In winit 0.30's Wayland backend
(`src/platform_impl/linux/wayland/window/mod.rs`) both calls are *literally
empty*:

```rust
pub fn focus_window(&self) {}
pub fn set_window_level(&self, _level: WindowLevel) {}
```

They are kept for X11, Windows and macOS, which do honour them. Under Wayland
a client may not take the focus or restack itself at all; the only mechanism
is **xdg-activation**, where the window that has the user's attention asks the
compositor for a token and spends it on the window that should have it next.

**What KWin checks** (read from `kwin/src/xdgactivationv1.cpp` and
`src/activation.cpp`, Plasma 6.7 — the version on Ty's box):

- `XdgActivationV1Integration::requestToken` **grants** a token when
  `workspace()->activeWindow()->surface() == surface` — so ask for it on the
  *studio's* surface, right after the click that was in the studio, not on the
  editor's.
- `Workspace::mayActivate` **honours** it only when
  `input()->lastInteractionSerial() <= m_activationTokenSerial`. A token
  committed without `set_serial` fails that test, and the failure path is
  `window->demandAttention()` — **which is the taskbar flash**. A flash means
  the token arrived and was refused; nothing at all means no token arrived.
- `activateSurface` on a window that is `!readyForPainting()` *stores* the
  token on that window and applies it when it paints. So a window being
  created needs **exactly one** token, in its attributes — asking for a second
  to spend on its first frame only replaces the first as `m_activationToken`
  and guarantees the survivor fails.

**The serial is the hard part, and it is why this needs its own module.**
winit never exposes an input serial. `crates/fontelle-ui/src/activation.rs`
therefore borrows winit's own `wl_display`
(`Backend::from_foreign_display` over the pointer from `RawDisplayHandle`),
opens a **second event queue** on it, and binds `xdg_activation_v1` *and the
seat*. Its own `wl_pointer` and `wl_keyboard` receive copies of every input
event the process is sent — that is ordinary libwayland behaviour, one queue
per object, all reading the same socket — so it can keep the largest
button/key serial it has seen. `WindowApp::about_to_wait` calls
`Activation::poll` once a pass to drain that queue, and a token request is
`set_serial(latest, seat)` + `set_surface(studio)` + `commit`, answered by a
round trip.

Smaller traps inside that:

- `GlobalList::bind` **panics** — not errors — if the top of the requested
  version range is above the version the crate's XML knows. Take it from
  `WlSeat::interface().version` rather than writing a literal.
- The activation crates (`wayland-client`, `wayland-backend` with
  `client_system`, `wayland-protocols` with `staging`) are pinned to the
  versions winit already resolved, so nothing extra is linked. If winit is
  upgraded and those move, they must move together or two libwayland client
  states end up in one process.
- On X11 `Activation::open` answers `None` and the plain winit calls are used,
  which is why the nested-X sandbox in §5 **cannot test any of this**. There
  is no compositor in there. The only test of record is Ty's desktop, and it
  passed on 2026-09-07 with Plasma 6.7.4.

Adjacent, and settled in the same session: **an instrument or effect that is
added or changed opens its window**, from every path — *New instrument*,
*Change instrument…*, the plugin picker for either, and *+ Add effect* for
built-ins and plugins alike (`open_chosen_instrument` and `open_newest_insert`
in `app.rs`). Browsing presets deliberately does not, or the browser would
throw a window per row.

## 5. Seeing the GUI, and the traps in it

The recipe is in the agent's memory file `seeing-fontelles-gui.md`. What that
file does not yet say, learned the hard way this session:

- **Synthetic input needs a warm-up move.** The first `ButtonPress` after the
  pointer enters a window is often swallowed. Send a `MotionNotify` somewhere
  inside the window, sleep ~0.3 s, *then* move to the target and click. Two
  "bugs" this session were this and nothing else.
- **Screenshots are one presented frame behind — always, not sometimes.**
  A menu that had definitely opened — confirmed by tracing — did not appear
  in a grab taken 1.5 s after the click, and the 2026-09-07 session lost an
  hour to the same thing four times over (a row that "did not load", a
  search that "did not filter"). The nested server hands back the frame
  *before* the one the program just presented. Nudge the pointer, sleep,
  nudge again, then grab — and when something looks missing, grab twice
  before believing it.
- **Keyboard input needs explicit focus.** There is no window manager, so
  nothing sets input focus. Use `d.set_input_focus(window, ...)` on the window
  you mean by matching its `WM_NAME`; without it every key is dropped.
- **`--run-for <seconds>` expires.** Windows vanishing mid-experiment is usually
  this, not a crash.
- **Never kill a studio you did not start.** `pkill -x fontelle` and
  `pkill -f target/release/fontelle` both kill **every** Fontelle on the
  machine — including the one the user is working in at their desk, three
  metres away. That is silent by nature: SIGTERM writes no message, dumps no
  core, and leaves no journal entry, so from the user's chair the window simply
  vanishes mid-action and the same action never reproduces it. It was reported
  as *"the daw keeps crashing a lot but it doesnt reproduce cleanly"* on
  2026-09-10, and by then this file had been telling agents to do it for a
  fortnight.

  Record the pid of the studio **you** launched and kill that:

  ```sh
  ./target/release/fontelle --run-for 120 & echo $! > /tmp/mine.pid
  ...
  kill "$(cat /tmp/mine.pid)"      # never pkill, never by name
  ```

  `--run-for <seconds>` is the belt to that braces: an agent-launched studio
  expires on its own, so a forgotten one cannot outlive the session. The same
  goes for `pkill -x cargo`, which ends the user's `cargo run --release`.

## 6. Build environment

This machine is shared with the user's own long-running work — a video
transcode during this session took the load average to 90+ on 16 cores with 34%
iowait, and a full workspace build went from ~3 minutes to over 30.

- Run builds and test sweeps **in the background** and collect them, rather than
  in the foreground where they hit the tool timeout and get orphaned.
- **Orphaned cargo jobs stack.** A timed-out build keeps running; three of them
  at once fight over the target directory lock and starve each other. Check with
  `pgrep -f 'cargo (test|build)'` and kill them before starting a fresh one.
- Check `uptime` before concluding that a build is hung.
- **A test binary killed under load looks like a test failure.** A workspace
  run at load average 80 ended with `error: test failed, to rerun pass -p
  fontelle-app --test audio_recording` and **no panic line anywhere above
  it** — the binary was killed rather than having failed an assertion, and
  the same target passed 17/17 on its own a minute later. No panic line means
  suspect the machine; re-run before believing it, and never chase a failure
  whose assertion you cannot read.
