# Trimming a note clip from the front — plan, not built

Written 2026-09-29, after v0.17.0 made clip edges easier to grab and left this
open: an **audio** clip's left edge trims its front in and back out
(`TrimClipStart`), a **note** clip's does not — its left edge is its body.
`TrimClipStart`'s own comment says why: *"a note clip has nowhere to keep the
notes an edge dragged in would uncover."*

Nothing here is built. Two decisions below are Ty's.

## The shape

A **`Clip`-level content offset**, `offset: Tick`, `#[serde(default,
skip_serializing_if = "is_zero")]` (so old files, saved bytes and
`Project::sync_hash` are unchanged while it is zero). A note at content tick
`t` sounds at song tick `clip.start + t - offset`, when `offset <= t` and
`t - offset < clip.length`.

On `Clip`, **not** `NoteData`: a prefab's instances share one `NoteData`
(`Project::clip_source`, `MakePrefabFromClip`), so an offset there would trim
every instance at once. On `Clip` it is per instance — and it fixes an
existing fault on the way: `SplitClip` on a prefab instance today restarts the
prefab from its beginning in the right half; with an offset the right half is
`offset + cut`.

The alternative — shift the notes' starts and allow negative starts as
hidden — is worse: `MoveNotes` refuses negatives, `notes_from_capture` drops
them, the compiler has **no lower bound** and would play them before the
clip, and it cannot work for prefabs at all.

The roll stays in **content ticks** (its clamps, `MoveLimits`, copy
normalisation and prefab editing are then unchanged); the host's song↔clip
conversions add the offset, and the roll shades `[0, offset)` the way
`roll_past_end` shades past the end.

## Decisions that are Ty's

1. **A looped note clip, trimmed at the front: rotate or cut?** Audio rotates
   (`AudioClipData::loop_phase`), `split_notes` rotates, and `loop_marks`
   already draws a phase — so the offset should probably be a **phase mod the
   period** when looping: the pattern is kept and starts later in its cycle.
   The other reading (the window `[offset, offset + period)`) drops the
   pattern's front notes and reveals ones past the period — a different loop.
   Recommendation: rotate, matching audio.
2. **A note straddling the new edge** (starts before it, still sounding past
   it): drop it, or play it truncated from the clip's start? `SplitClip` today
   keeps the tail, so "trim to X" and "cut at X and delete the left" would
   sound different unless the compiler emits a truncated note-on at the
   window's start. Recommendation: truncate, matching the split.

## Every place that must honour it (about 35 edits)

**Sequencer** — `fontelle-sequencer/src/compile.rs` `compile_with`: the
period filter (~274), the window's lower and upper bound (~290–311; there is
no lower bound today), and the pass end of the cut (~327). Centralise the rule
in `Clip::repeats` / `Clip::repeat_start` (`fontelle-model/src/clip.rs:61`)
or a new helper, because **three hand copies of it exist**: compile,
`fontelle-assets/src/midi_export.rs` `placed_notes`, and the canvas's
`clip_notes` — plus `flatten_loop` and `song_end_tick`. They must not drift.

**Model** — `TrimClipStart` (the note branch: `offset += moved`; its inverse
is already a whole-clip replace), `trim::clamp_front` (a note case: not before
`clip.start - offset` unlooped), `SplitClip` + `split_notes` (set the right
half's offset rather than rewrite notes — this changes what
`tests/slicing.rs` expects), `flatten_loop`, `SetClipLoop`'s unloop fold,
`notes_from_capture`'s lower bound. `ImportParts`, `AddPrefabInstance`, and
every constructor set 0; ~13 `Clip { .. }` literals in `src` and ~60 in tests
need the field unless `Clip` gets a constructor first.

**Session** — `playhead_tick`, `clip_length` (+ a "hidden before" value),
`sample_of_clip_tick`, `song_tick_of_clip_tick` / `clip_tick_of_song_tick`
(time selection, score import), `recording_notes` and `keep_take` (pass
`clip.start - offset` or a take lands shifted), `ghost_notes` (both clips'
offsets; it also ignores prefabs and loops today), `clips()` → `ClipInfo`
(carry the offset), `ArrangeEdit::TrimStart`'s audio-only filter,
`song_end_tick`.

**UI** — `clip_notes`, `loop_marks` (phase from the offset, not only
`audio.loop_offset`), `LeftEdge` for note clips in `timeline_hit` /
`timeline_grab`, `Gesture::TrimmingStart`'s audio filter and magnet, the
drag's clamp (per clip, `-offset` — needs the offset in `ClipInfo`, or the
canvas overshoots and the model clamps silently), and the roll's before-start
shade.

**Wire** — nothing new: `Clip` travels whole inside the edits that already
carry it, and `TrimClipStart` is already on the wire.

Tests to extend: `fontelle-sequencer/tests/{clip_bounds,looping,prefabs,
clip_mode}.rs`, `fontelle-model/tests/{slicing,arranging,looping,prefabs,
recording}.rs`, `fontelle-ui/tests/clip_left_edge.rs`.
