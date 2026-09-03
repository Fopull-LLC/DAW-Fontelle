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

`cargo test --workspace` is green at **2265 passing** and
`cargo clippy --workspace --all-targets -- -D warnings` is **clean**. Keep it
that way: clippy at `-D warnings` is part of the bar now, and every hit the
older handoffs recorded as "pre-existing" has been fixed.

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

## 3. What is still open

Ranked by what I would take first.

1. **Drag-and-drop has not been watched working.** The import pass wired
   `WindowEvent::DroppedFile` and `Session::drop_file` is tested for every kind
   (`.mid`, `.fsc`, `.sf2`, a file of the wrong sort, a file that is not
   there), but synthesising an XDND drop against the nested X server was not
   attempted, so the winit half is unproven. Everything *else* about importing
   was driven in the real window — see `PROGRESS.md`'s top section, which also
   lists the three bugs that found. Cheapest check: drag a `.mid` onto the
   window by hand.
2. **The effects catalogue's build order, from item 5.** `docs/effects-catalogue.md`
   §4 is the list and §5 is the recipe; items 1–4 (utility, gate, chorus,
   filter) are done, and both of the things that gated several rows — the
   preset picker and the external sidechain — are done too. Next is the
   **limiter as an insert** (the DSP exists; an afternoon), then the
   compressor's character/lookahead/link extensions, then delay and reverb.
   **Presets are owed** on the EQ, the compressor, the chorus, the delay and
   the reverb, and rule 10 has a test that stops a new effect landing in the
   "no presets" list by accident.
3. **Automation: what the block cannot do yet.** Points are placed and dragged
   one at a time — there is no marquee over several inside a block, and no
   pencil/line/shape-stamp draw modes (§12.4 names all three). The gestures
   that exist are in `fontelle-ui/tests/automation_blocks.rs`.
4. **A tempo lane is a staircase**, not a ramp: `effective_tempo_map` samples
   it every sixteenth note into constant segments, because `TempoMap` holds
   only constant segments (§6.2's scope cut). Interpolated segments are a
   bounded addition and the formula is written down in `project.rs`.
5. **`effective_tempo_map` is built twice per republish** and calls
   `automation_at` once per step, each of which sorts a copy of the clip's
   points. A project with no tempo lane pays nothing, so this only bites once
   somebody automates the tempo.
6. **Unverified: the last move of a right-drag on a ruler.** Driving the
   window with synthetic input, a right-drag that selects a time range
   sometimes commits one grid step short of where the button came up — the
   final `MotionNotify` before the release does not always reach the window.
   `canvas::time_selection` is tested directly and is right; whether this is
   the XTEST harness (which this file already records as unreliable near a
   button release) or the window's own event handling is **not settled**.
   Check it with a real mouse before spending time on it.
7. **Unverified: raising an already-open editor window.** The code calls
   `focus_window()` *and* `request_user_attention()` (the Wayland
   xdg-activation path). It could not be verified here — the test harness is a
   bare nested X server with no window manager, so there is nothing to raise
   against. Check it on a real KDE session before believing it.
8. **The gate's look-ahead is uncompensated latency**, like the master
   limiter's, and any lookahead insert under a mix below 100 % combs against
   an undelayed dry. Both wait on delay compensation.
9. **The ducker, the vocoder and the repitcher** are unwritten; the repitcher
   is varispeed over a *clip* and is in the wrong crate.

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
- **Do not clamp after the oversampling filters at the base rate.** A
  band-limited square overshoots by a sixth; clamping that at 48 kHz is the
  aliasing the oversampling exists to prevent. It cost a diagnostic to find
  and is written on the code in `fontelle-fx/src/distortion.rs`.

## 5. Seeing the GUI, and the traps in it

The recipe is in the agent's memory file `seeing-fontelles-gui.md`. What that
file does not yet say, learned the hard way this session:

- **Synthetic input needs a warm-up move.** The first `ButtonPress` after the
  pointer enters a window is often swallowed. Send a `MotionNotify` somewhere
  inside the window, sleep ~0.3 s, *then* move to the target and click. Two
  "bugs" this session were this and nothing else.
- **Screenshots can be a frame stale.** A menu that had definitely opened —
  confirmed later by tracing — did not appear in a grab taken 1.5 s after the
  click. When something looks missing, grab again before believing it.
- **Keyboard input needs explicit focus.** There is no window manager, so
  nothing sets input focus. Use `d.set_input_focus(window, ...)` on the window
  you mean by matching its `WM_NAME`; without it every key is dropped.
- **`--run-for <seconds>` expires.** Windows vanishing mid-experiment is usually
  this, not a crash.
- **`pkill -x fontelle`**, never `pkill -f`, which matches the agent's own shell.

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
