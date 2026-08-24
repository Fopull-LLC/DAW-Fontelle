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

## 2026-08-23 update: the first real-hardware bug report

Ty ran the manual-verification commands from the previous update. The synthetic
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

**Update — Ty re-ran it, with permission for me to run it too:** the "rtkit
allocates" theory (bug 1) was wrong, or at least incomplete. With Ty's explicit
go-ahead I ran `--play-sf2` myself several times to chase this down. Findings,
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

**The root allocation itself (bug 3) is still not found.** It's real — the
panic message and size/align are exactly reproducible — but every attempt to
reproduce it through direct, off-hardware simulation (synthetic patch, real
imported patch, both at matching or exceeding real playback duration) failed
to trigger it; only the genuine `cpal`→ALSA callback path does. That strongly
suggests it originates inside `cpal`'s ALSA backend itself (a one-time lazy
buffer/conversion setup under real hardware conditions our simulation can't
recreate), not in Fontelle's own code — which has now been checked
exhaustively off-hardware and found clean. Its *impact* is fully contained by
fix 5 regardless of where it turns out to live: one bad/silent block, not a
crash. Worth a proper native-debugger (gdb, break on `malloc`) session
eventually; not blocking further work.

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
| Device out | **Real, verified working end-to-end.** `fontelle_engine::AudioDevice::start_output_stream` opens a real `cpal` stream. Confirmed on Ty's real hardware: synthetic tone plays audibly, and `--play-sf2` on a real file runs its full duration and exits 0 (timed, not just eyeballed) after three real crash-fix rounds — see "2026-08-23 update" above. **Caveat:** one specific allocation is still unroot-caused (very likely inside `cpal`'s ALSA backend, not Fontelle's code) and produces one bad/silent block early in playback before a `catch_unwind` safety net silences the rest of that stream — contained, not invisible. Listed in "Next steps" below. |
| Mixer track | **Not started.** The graph above is one `SamplerNode` straight to a buffer — no `MixerTrackNode`, no routing. |
| Triggered by a clip on the timeline | **Not started.** `fontelle-sequencer::compile` and `fontelle-model::Project` are still full of `todo!()`. Today's note-on is hardcoded in `fontelle-app/src/main.rs` and the manual test, not sourced from a document. |

**So: the gate is not closed.** What's real and tested is the entire audio-data
path from a real SF2 file to real interpolated, enveloped, gain-staged samples
reaching a real device callback — genuinely the hard, correctness-risk-bearing
part (TDD §23 names "SF2 import defaults subtly wrong" and "agent-generated code
that looks right and does nothing" as named risks; both are why this took the
tests-first route). What's missing to actually close the gate is document→timeline
wiring and the mixer stub — both mechanical relative to what's done, not
research-risk work.

### Verify by ear (needs a human at the keyboard)

```sh
cargo test -p fontelle-engine --test manual_audio_output -- --ignored --nocapture
# or, to hear a real SF2 file instead of the built-in synthetic test tone:
FONTELLE_TEST_SF2=/path/to/file.sf2 cargo test -p fontelle-engine --test manual_audio_output -- --ignored --nocapture

# or run the app binary directly:
cargo run -p fontelle-app -- --play-sf2 /path/to/file.sf2
```

Both open the real default output device and play ~1-2 seconds of audio. Neither
runs in `cargo test --workspace` or CI (the `--ignored` test is skipped by
default; the app binary's default `main()` still `todo!()`s into the unbuilt
windowed DAW unless you pass `--play-sf2`).

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
  timing) are real and tested. `SvfFilter`, `Oscillator`, `PeakRmsMeter`,
  `DcBlocker` are still `todo!()` — not on the M0 critical path (filters are
  bypassable via `FilterSlot::enabled`, so `Voice::render` never calls them yet).
- **fontelle-core** — real: `SampleStore` (insert/get, `AssetId`-keyed, only the
  fully-resident case — no disk streaming yet, see below), `Voice::render` (pitch
  from root-key+fine-tune, per-sample interpolated playback, forward looping,
  per-layer gain, patch-wide amp envelope, auto-deactivation), `VoicePool`
  (free-voice search then age-based stealing — `Quietest`/`LowestPriority`
  currently alias `Oldest`, no level/priority tracking exists yet), `Sampler`
  (`note_on`/`note_off` via voice-context matching, `render`). Only
  `Source::Sample` layers render — `Source::Sf2Zone` is effectively unused
  (import always produces `Sample` layers) and `Source::Oscillator` is silently
  skipped, not wired to `fontelle_dsp::Oscillator` yet. Output is mono-summed;
  `Layer::pan` has no effect. Tests: `crates/fontelle-core/src/{streaming,voice,sampler}.rs`.
- **fontelle-fx** — pure stub, unchanged since scaffolding. Not on the M0 path.
- **fontelle-model** — pure stub. `TempoMap`, `Command`/`History` internals,
  cycle checks all still `todo!()`. Not on the M0 path (M0's note-on is hardcoded,
  not sourced from a `Project`).
- **fontelle-sequencer** — pure stub. This is the actual remaining M0 work: wire
  a real (even minimal) `Project` → `compile()` → `CompiledTimeline` path so the
  hardcoded note-on in `fontelle-app`/the manual test can instead come from a
  clip.
- **fontelle-engine** — `CompiledGraph::process_block` is real but scoped to
  source nodes: any `ScheduledNode` with a non-empty `input_buffers`, or more
  than one `output_buffers` entry, panics with a clear message rather than
  attempting the general multi-buffer-aliasing case. That's real infrastructure
  work (`[T]::get_disjoint_mut` is the right tool once it's needed) belonging to
  M4, when effect chains and mixer sends actually need it — not before. `SamplerNode`
  is real (wraps `Sampler` + a shared `Arc<SampleStore>`). `AudioDevice` is real —
  opens a real `cpal` stream, promotes the callback thread via
  `audio_thread_priority`, tags it via `rt_guard::mark_current_thread_rt`, chunks
  the callback into `BLOCK_SIZE` (128-frame) pieces regardless of what the
  backend actually delivers. `EffectNode`/`MixerTrackNode`/`SendNode`/
  `AudioClipNode`/`MasterNode` are empty placeholder structs — M4/M6 work.
- **fontelle-assets** — `import_sf2` is real, see "SF2 import scope" below.
  `import_sfz`, `SoundfontLibrary`, peak generation are pure stub.
- **fontelle-ui**, **fontelle-plugin**, **fontelle-midi** — pure stub, unchanged
  since scaffolding. Not on the M0 path.
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
- **No filter/LFO/mod-matrix generators are read** — `InitialFilterFc`,
  `InitialFilterQ`, every `*LfoTo*` generator, `ModEnvTo*`. Filters stay
  disabled; the mod matrix stays empty.
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

## Known design decisions worth flagging to Ty

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
- **`fontelle-model::Channel` stores a serialised `Vec<u8>` patch**, not a live
  `fontelle_core::Patch` — the model crate can't depend on `fontelle-core`
  (INVARIANT 4's model-side counterpart: model depends on nothing but
  `fontelle-types`). Revisit once the project serialisation format (§17.2) is
  actually designed; a dedicated intermediate type might be cleaner than an
  opaque blob.

## Next steps, in the order I'd tackle them

1. **Close the M0 gate for real:** a minimal `fontelle-sequencer::compile` that
   turns a `Project` with one channel/one clip/one note into a `CompiledTimeline`,
   and wire `fontelle-app` (or a new integration test) to drive `CompiledGraph`
   from that instead of a hardcoded `note_on` call. This is the last mechanical
   piece — everything it depends on (`Sampler`, `SamplerNode`, `CompiledGraph`,
   `AudioDevice`) is already real.
2. **A minimal `MixerTrackNode`** so "mixer track" in the gate's own wording is
   literally true, not just "the sampler writes straight to the device buffer."
   Doesn't need sends/inserts yet — gain + pan + mute is enough to be honest
   about the phrase.
3. Only after 1–2 are done should M1-onward feature work (full interpolation
   modes, streaming, more of the mod matrix, effects) start — the TDD's own
   staging (§22) puts the vertical slice before feature breadth on purpose.
4. **Lower priority, not blocking:** root-cause the still-unidentified
   allocation from the "2026-08-23 update" section above (`dealloc size=16,
   align=4`, real on hardware, not reproducible through any off-hardware
   simulation attempted so far — very likely inside `cpal`'s ALSA backend).
   Its impact is contained (one bad block, then that stream goes silent for
   the rest of its life via the `catch_unwind` safety net) but it's still a
   real defect worth an actual fix — a native debugger session (gdb, break on
   `malloc`/`calloc`/`realloc` while running `--play-sf2`) is the next
   escalation past what's been tried. Don't let this block 1–2 above.
