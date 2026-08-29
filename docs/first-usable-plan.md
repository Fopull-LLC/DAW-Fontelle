# The road to the first usable Fontelle

Written 2026-08-28, after a full audit of `FONTELLE_TDD.md`, `PROGRESS.md`, and
the code (331 tests passing, clippy/fmt clean, verified before writing this).
This is the execution plan for the next stretch of work: everything between
today's state — a musically capable engine driven by a CLI — and the first
version of Fontelle a person can open, make a piece of music in, and save.
That version is what lets iteration switch from "what the TDD predicts users
want" to "what users actually do."

Read `PROGRESS.md` first if you're picking this up cold. The process rule there
(tests before implementation, confirmed to fail first) applies to every item
below, with one adaptation for GUI work noted in Phase 2.

---

## 1. Where things actually stand

The engine is far ahead of where the milestone list implies. In TDD terms:

- **M0 closed** (2026-08-24, verified on hardware).
- **M1 mostly closed on the engine side:** SF2 import (filters, envelopes,
  LFOs, mod matrix, both default modulators), stereo voice architecture,
  velocity/pan/interpolation quality, multi-timbral playback with per-part
  mixer tracks. Missing from M1: sfz import, streaming (§7.7), the §7.8
  harshness mitigations, the sampler editor UI, preset save/load.
- **Most of M5's hard half is done early:** live MIDI devices, hot-plug,
  merging, sustain, stuck-note safety, all verified against real ALSA
  hardware. Missing: mapping UI, MIDI learn, clock sync, file export.
- **Pieces of M3/M4 landed early too:** piecewise tempo map, real transport
  (play/stop/seek/loop through `TransportReader`), master bus with a
  brickwall limiter and metering, offline bounce.
- **Nothing of the UI exists.** `fontelle-ui` is 272 lines of module skeleton.
- **Nothing of persistence exists.** No save, no load, no undo
  (`Command`/`History` are stubs), and the document model is not actually the
  source of truth — `fontelle-app` hand-builds the graph from its own `Song`
  type, `Project::mixer` is read by nothing, `Channel.patch_data` is left
  empty.

The quality of what exists is genuinely high — the test-first culture is real,
the RT discipline is mechanically enforced, and PROGRESS.md's claims checked
out against the code everywhere I verified them. Nothing below is a rewrite;
this plan is purely additive, which is exactly what the M0 gate was supposed
to buy and did.

## 2. Audit findings

Flaws or gaps in the plan itself, in order of consequence. Each shapes the
phases in §4.

### 2.1 MIDI recording is missing from the TDD entirely

`Transport` has a `Recording` state, and §15.4 covers *audio* recording (M6).
No section anywhere says how live MIDI input becomes a note clip. For this
product — a soundfont instrument aimed at players — "play it, keep it, edit
it" is the core capture loop of making something, and the live-input pipeline
that makes it cheap to add just landed. This is the highest-value single
feature absent from both the TDD and the code, and it is added to the
first-usable gate below (Phase 1, item 5).

### 2.2 The milestone order buries first user value

M2 (plugin export) precedes M3 (UI). The TDD's rationale — prove the §4.1
boundary early, ship an independently valuable artifact against solo-dev
burnout — is sound reasoning, but both halves have weakened since it was
written: the boundary has now been proven by other means (the engine runs
under a foreign host's constraints already — cpal's callback, the
zero-allocation guard, the in-place buffer convention), and a plugin without
the sampler editor UI is only usable through host-generated generic sliders,
which does not exercise the product thesis (the SF2-overrides workflow) at
all. The sampler editor needs `fontelle-ui` regardless of which target ships
first.

**Recommendation, and what this plan assumes: defer M2 until after the first
usable DAW version.** The UI stack built in Phase 2 is dual-target by design
(§16.2, winit + baseview), so nothing is foreclosed — M2 becomes cheaper, not
harder, by waiting for the UI layer to exist. This is a deliberate deviation
from the TDD's ordering; if the owner wants the early-shippable-artifact
insurance instead, the alternative is a headless CLAP checkpoint after
Phase 1, and everything else in this plan still holds.

### 2.3 The model→engine link is the load-bearing missing piece

Three of PROGRESS.md's open questions are the same question: who builds the
`CompiledGraph` from the `Project`? Today `fontelle-app` hand-assigns node
IDs and buses from its own `Song` type, `Project::mixer` is decorative, and
`fontelle-sequencer::compile` has a `channel_nodes` map threaded in from
outside because no component owns the mapping. Save/load, the mixer UI, undo,
and choosing instruments in the UI all land on this one step. The TDD never
names it as a component. It is Phase 1, item 2, and almost everything else
sequences after it.

### 2.4 The patch serialisation format is on the critical path and undesigned

§17.2 (project.json) and §8.3 (preset compatibility) both need a versioned
serialised form of `fontelle_core::Patch`, and `Channel.patch_data:
Vec<u8>` was left as an opaque blob "until the format is designed." Save/load
forces the design now. It is deliberately small: serde on `Patch` with a
`format_version`, owned by `fontelle-core` per §8.3. Phase 1, item 1.

### 2.5 The test-first rule has no GUI answer

§20 covers unit, DSP, RT-safety, and benchmarks; nothing says how
`fontelle-ui` gets tested, and this project's culture will otherwise hit a
wall at exactly its riskiest component. The answer this plan mandates:
**everything that can be a pure function is one, and the tests live there.**
Visible-range math, hit-testing, geometry building, snap arithmetic,
keybind→command resolution — all pure, all testable without a window. The
widget/backend layer stays a thin shell that calls them. A GUI item in this
plan is "done" when its view-model functions are tested and the pixels have
been seen once by a human, mirroring the engine's "tests plus a hardware
listen" convention.

### 2.6 Smaller findings, all deliberately deferred

- **Streaming (§7.7):** a 325 MB soundfont is fully resident today. Fine for
  first-usable; memory cost should be *visible* (the channel list already
  prints sizes on import — keep that in the UI). Post-first-usable.
- **Latency compensation:** `latency_samples` is reported and unread. With
  the limiter as the only latent node on the one master bus it is a uniform
  delay nobody can hear. Must land before sends (M4), not before this gate.
- **Prefabs (§10.5):** the TDD's own risk table calls them a top correctness
  risk. Nothing about making a first track needs them; the ID/override
  infrastructure they need is already in the model, so deferring stays
  additive. Explicitly out of the first-usable gate despite being listed
  under M3.
- **Automation (§12):** out of the gate; first thing to revisit after it.
- **Effects (§13.4):** out of the gate — the master limiter exists and per
  track gain/pan/mute is enough to balance a piece. The `fontelle-fx` stubs
  (EQ and compressor first, per PROGRESS) are ready when wanted.
- **Voice stealing** `Quietest`/`LowestPriority` alias `Oldest`; `Ultra`
  interpolation needs a stateful resampler; tempo ramps have no creator; CC
  routing beyond sustain is dropped by design until parameters exist. All
  correctly deferred already; none block the gate.

## 3. The gate: what "first usable" means

Modelled on the M0 gate — one sentence a human can verify:

> Launch `fontelle` with no arguments. In the window that opens: create a
> channel from an SF2 file, draw notes in the piano roll, record more from a
> MIDI keyboard, arrange clips on the timeline, loop and edit while playing,
> balance parts with per-channel gain/pan/mute, save the project, quit,
> reopen it to an identical-sounding state, and export a WAV — with the
> zero-allocation assertion active throughout and near-zero CPU while
> stopped and idle.

Every noun in that sentence is either done (the engine underneath) or in a
phase below. Nothing else is in the gate.

## 4. The plan

Three phases. Phase 1 has no UI and stays fully inside the project's proven
competence; it removes every architectural unknown from Phase 2 except the
one that belongs there (the rendering stack). Items within a phase are
ordered by dependency.

### Phase 1 — the document becomes the source of truth (CLI-verifiable)

1. **Patch serialisation in `fontelle-core`.** Serde on `Patch` and its
   children with an explicit `format_version` and a migration entry point
   (even if v0→v0 is the only arm). Replace `Channel.patch_data: Vec<u8>`
   with a typed form in `fontelle-types` (per the scaffolding note's own
   suggestion). Test: import an SF2 preset → serialise → deserialise →
   byte-identical render against the un-round-tripped patch.

2. **`Project` → graph realisation.** One function, living in `fontelle-app`'s
   lib (the one layer allowed to see both model and engine), that reads
   `Project::channels` + `Project::mixer` and produces the `CompiledGraph`,
   bus layout, and the `ChannelId → NodeId` map that `sequencer::compile`
   takes — replacing the hand-built `Song` path. It owns the mapping that
   PROGRESS's open question says needs an owner. The existing CLI must come
   out the other side working identically (the byte-identical-bounce tests
   are the proof), with `Song` deleted or reduced to a CLI convenience.
   MIDI-import CC7/CC10 land on `Project::mixer` tracks now, closing that
   open question too.

3. **Commands and undo.** Implement `History::undo/redo` and the command set
   the first UI needs: add/remove channel (with patch), add/move/resize/
   delete notes, create/move/duplicate/delete clip, set tempo, set track
   gain/pan/mute, set loop range. `merge_with` coalescing for the drag-shaped
   ones. INVARIANT 9 starts being enforced here: the CLI and tests mutate
   `Project` through commands from this point on. Test: property-test that
   apply→invert→apply is an identity on the document.

4. **Project save/load (§17.1–17.2).** Folder bundle, atomic write
   (temp + fsync + rename), `format_version`, assets copied or referenced
   per §17.4's ask-once policy (headless default: reference). CLI grows
   `--open <project> [--render-wav]`. Test: build a project in code → save →
   load → render → byte-identical with the pre-save render; corrupt/truncated
   JSON fails with a clear message (§20.3's standard applied to our own
   format).

5. **MIDI recording (the §2.1 gap).** While the transport is `Recording`,
   live events already flowing through the SPSC queues are also captured
   (timestamped, off the RT thread — mirror them from the drain into a
   capture buffer the model thread empties). On stop, one command turns the
   capture into a note clip: samples→ticks through the `TempoMap`, zero-
   velocity note-ons as note-offs (rule already exists), sustain resolved by
   the router as it already is for live play. No quantisation beyond
   tick-rounding — quantise is a piano-roll command later, per §16.5. CLI
   proof before any UI exists: `--midi-in --record` writes a `.fontelle`
   project you can `--open` and hear back.

### Phase 2 — the walking GUI skeleton (the risk burn-down)

The §16.2 stack (wgpu + vello + cosmic-text over winit) is the highest-risk
component in the project, per the TDD itself. The mitigation is to make it
walk before it runs, and to timebox: **if a window with themed panels
rendering at 60 fps dirty-region-only is not real within the timebox, take
§16.2's own fallback (direct wgpu + lyon) rather than fighting vello.** Do
not relitigate the toolkit choice beyond that documented fallback.

6. ~~**Window + surface + one panel.**~~ **Done, 2026-08-29.** winit event
   loop, wgpu surface, vello scene, cosmic-text rendering the theme's token
   set (dark default *and* the light variant, plus the versioned file format
   §16.6 asks for). Redraw only on invalidation; zero frames issued when idle,
   measured at 1 frame and 0.05% of one core over a 35-second unattended run.
   **The vello stack came up without needing the lyon fallback** — the
   timebox was not spent, and §16.2's fallback stays unused. The §2.5 answer
   is written into the TDD as §20.6. See PROGRESS.md's top section.

7. **Transport bar over the real engine.** Play/stop/seek/loop controls and a
   playhead driven by the `position_sample` the RT side already publishes;
   master meters from the `MasterMeter` atomics. This is the first moment
   the window and the audio thread coexist — prove the threading shape
   (commands down, atomics/triple-buffer up, per §2.2) on the simplest
   feature, not on the piano roll.

8. **Piano roll MVP.** Virtualised canvas (§16.4: visible-window geometry
   only, instanced note quads, playhead on its own layer), and the core of
   §16.5: draw/delete/select/move/resize, snap with the standard divisions,
   right-click delete, Ctrl+Z/Y through the real `History`, Ctrl+A/B/C/V/X.
   Every edit is a command from item 3 — the roll is a view, never a mutator
   (INVARIANT 2). The full FL keymap document is gate-adjacent polish; the
   subset above is the gate.

9. **Timeline + channels + mixer strip.** Arrangement canvas (clips as
   blocks: move/duplicate/delete/mute), a channel list that creates channels
   from an SF2 file picker with the preset list the CLI already prints, and
   per-channel gain/pan/mute bound to `Project::mixer` through commands.
   Record-arm per channel wiring item 5 into the UI, with a metronome
   (small engine node; count-in optional).

10. **Save/open/new in the UI.** File dialogs over item 4, dirty-state in
    the title bar, autosave to `backups/` on a timer. First-run settings can
    be minimal (§18's full wizard is M7): audio device selection and the
    projects/soundfonts paths, honouring INVARIANT 10.

### Phase 3 — closing the gate

11. **Export dialog.** `render_offline` already exists and renders at `High`
    quality; wire it to a dialog writing into `<project>/renders/` with the
    clipped-sample count surfaced.

12. **A real-project shakedown.** Make an actual multi-part piece in the app,
    start to finish, on hardware — the equivalent of M0's "heard on real
    hardware" clause, applied to the whole gate sentence in §3. Fix what the
    shakedown finds before calling the gate closed; PROGRESS.md's history
    says this step always finds something.

### After the gate (explicitly out, in likely order)

Automation → EQ/compressor and the first insert effects (needs latency
compensation at the send/insert boundary) → sampler editor panel → **M2
plugin export** (now with a real UI stack to build the editor on) → streaming
→ prefabs → sfz → MIDI learn/clock → audio clips/recording (M6) → M7 polish.

## 5. Ground rules carried forward

- Tests first, confirmed failing, per PROGRESS.md's process rule — with §2.5's
  view-model shape as the GUI adaptation.
- INVARIANTS 1, 2, 9 and 10 are the ones this stretch of work can newly
  violate; the allocator guard covers 1, and the command discipline in
  Phase 1 item 3 is what makes 2 and 9 checkable at all.
- Update PROGRESS.md as sections land; it is the project's memory and it was
  accurate when audited, which is worth preserving.
- Anything in this plan that turns out to need a §-level TDD change
  (as §2.1's MIDI recording does) gets written into the TDD, not just done.
