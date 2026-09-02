# Handoff

Written at the end of a session that closed a long list of reports from using
the studio. Read this **after** `PROGRESS.md`'s top two sections and before you
touch anything.

`PROGRESS.md` says what the project *is* and what was built. This says what
state the tree is in, what is still open, and the handful of things that cost
this session real time to discover.

---

## 1. Where the tree stands

Branch `main`. Everything described in `PROGRESS.md` up to and including the
2026-09-02 effects pass is **committed** — the uncommitted stretch that ran
from `ee06e6b` through ten sessions' worth of work was landed on 2026-09-02
as two commits (the code, then the docs). Read `git log` for the sequence;
read `PROGRESS.md`'s sections for what each pass built and why.

`cargo test --workspace` is green at **2017 passing** and
`cargo clippy --workspace --all-targets -- -D warnings` is **clean** — the
handful of pre-existing hits the last session recorded (two unused `Result`s
in `fontelle-model/tests/inserts.rs`, a `clone` on a `Copy` type, a
draw function with eight arguments, and four more in test files) were fixed
before the commit. Keep it that way: clippy at `-D warnings` is part of the
bar now.

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

Ranked by what I would take first. None of these are from the user's original
list — that list is closed.

1. ~~**Seven effects are written and unreachable.**~~ **Corrected, and then
   partly done.** This item was wrong in a way that cost the next session time,
   so read the correction before trusting anything else in this list.

   Six of those seven were **`todo!()` stubs**, not working DSP — a config
   struct, a state struct, a `process` signature and a doc-comment describing
   the algorithm, with `todo!("...")` as the body. Only the **limiter** was
   real. So it was never "cheap plumbing"; it was writing the DSP.

   **Delay, reverb and distortion are now written and reachable** (see
   `PROGRESS.md`'s newest section). The plumbing claim itself held up
   perfectly: each needed exactly an `EffectKind` variant, a spec table in
   `fontelle-types/src/effect.rs` and an `EffectState` arm in
   `fontelle-engine/src/nodes.rs`, and **no UI change at all**.

   **Bitcrush and soften are done too**, so the mixer offers seven effects.

   And the third correction: **the limiter was never unreachable.** Its DSP is
   complete and it has been running on the master bus in `MasterNode` all
   along — deliberately not an insert, with a comment on the field saying so.
   It has no `EffectKind` variant because it is not that kind of thing.

   Still `todo!()`: **the repitcher**, which is varispeed over a *clip* rather
   than a bus effect. It has no insert slot to live in and giving it one would
   be inventing a use for it.

   **2026-09-02, later:** the distortion and the bitcrush were rebuilt to
   `docs/effects-catalogue.md`'s designs — read that document before adding
   or extending any effect; it is the authority on how far each one goes and
   in what order the missing ones are built. The catalogue's §4 names two
   items that gate several others: a **preset picker** in the effect window
   (seventeen presets exist across three effects and none can be chosen from
   the window) and an **external sidechain into `EffectNode`**.
2. ~~**A knob under automation looks like any other.**~~ **Done.** Theme format
   v6 adds `param_automated`; `InstrumentView::mark_automated` sets the flag
   and the knob's groove, a switch's chip and the mixer strip's wet/dry dial
   all wear it.
3. ~~**A delay cannot be synced to the tempo.**~~ **Done.** The tempo now
   reaches the audio thread: the sequencer compiles the tempo map onto
   `CompiledTimeline::tempo` in samples, `TransportSnapshot` carries the `bpm`
   at the block being rendered, and `DelayConfig::effective_time_ms` turns a
   note value into a duration. **Anything else that wants the tempo can now
   have it** — an LFO is the obvious next one — and it costs a field read.
4. ~~**Automating `patch/voice/polyphony` is inert.**~~ **Done, with a stated
   ceiling.** Polyphony is a live limit inside the pool rather than the pool's
   size, since growing a `Vec` on the audio thread is INVARIANT 1's subject. A
   lane can lower it and raise it back to the size the pool was built at, and
   **not past that** — the pool is built from the patch, so the knob is the
   ceiling and turning it rebuilds the graph.
5. ~~**Cutting an automation clip can step at the seam.**~~ **Done.** The cut
   reads the curve's value at the seam and puts a point there in both halves,
   carrying the shape of the segment it fell inside.
6. ~~**No lane reordering.**~~ **Done as a menu, not a drag.** `Lane::order`,
   `Project::lane_ids()` and the `MoveLane` command, driven by "Move up" and
   "Move down" on the row's right-click menu. Dragging a lane header is still
   not a gesture the arrangement has. This turned up a live bug: `lanes()` and
   `clips()` disagreed about row order and would have drawn every clip against
   the wrong row.
7. ~~**Rename has no caret.**~~ **Done.** `RackChrome` and `TimelineChrome`
   carry which row is being typed into, and the same one-pixel bar the search
   box uses goes after the name.
8. **Unverified: raising an already-open editor window.** The code calls
   `focus_window()` (X11/Windows/macOS) *and* `request_user_attention()` (the
   Wayland xdg-activation path, since a Wayland client may not take focus by
   asking). It could not be verified here — the test harness is a bare nested X
   server with no window manager, so there is nothing to raise against. Check it
   on a real KDE session before believing it.

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
