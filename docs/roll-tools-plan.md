# The roll's tools: the plan to make them feel complete (2026-10-20)

> *"im wanting to now spend some time focusing on the piano roll tools which
> weve largely ignored for a bit we just finally added a quantization tool and
> improved the arpeggiator but theres still a LOT of improvement to be had and
> small ux issues present and theres still need for more tools that other daws
> have that were just missing like strum and chop (lke repeated chops
> basically) and more."* — Ty, 2026-10-20, after 0.30.0

> **Status, 2026-10-20 (evening): built, for 0.31.0-beta.1.** Ty: *"great
> plan now follow through release on beta"* — §7 was taken as recommended
> (B, yes, yes with the vocal exception, yes, Phase 3 + Slip deleted, keep
> under a heading, frame first). Built: Phase 0, Phase 1 except typing a
> value, all of Phase 2, all of Phase 3. **Not built:** typing a value into
> a dialog row (§3.3 — the arrows, the scrub and the click cover it; the
> values are ladders anyway), a drawn tension curve (§8), FL's
> click-to-mute pencil (mute is a menu row and Ctrl+Shift+M in the roll),
> the docked drawer (A). Where things live: PROGRESS.md's top entry.

Two readers: **Ty**, who decides §7 before anything contested is built
(memory `collaborative-review-workflow`), and **the agent who builds it**,
who has read `CLAUDE.md`, `PROGRESS.md`'s top two entries and
`docs/handoff.md`. Everything in §1 was checked on 2026-10-20 against the
source (file:line given) or against the release binary on a nested
`Xwayland :99`, with the tour song's Keys clip open. §2's comparisons come
from the vendors' manuals (sources in §9); anything not confirmed there is
marked.

---

## 0. Ground rules

1. **Tests first, confirmed failing**, then the implementation. Every step in
   §6 names its test file.
2. **A preview asks the same function the Apply runs.** This is the
   `slice_marks` rule (PROGRESS, 2026-09-09: *"a preview computed from a
   second copy of the snapping rule is a preview that is right until somebody
   edits one of them"*). A tool is one pure function in `fontelle-model`;
   the ghosts, *Hear it* and Apply all call it.
3. **One tool run is one undo, and every run is a `Compound` of commands that
   already exist** (§3.1). No new wire tags for collaboration unless §7 says
   so.
4. **The wheel never changes a value** (Ty's standing rule, memory
   `ux-polish-navigation-never-edits`). Dialog controls are clicked, dragged
   or typed.
5. **A new shortcut is a new `Action` plus a catalogue line** in
   `canvas/keybinds.rs`, never a key literal (memory `keybinds-page-and-master-meter`).
6. **Look at it on `:99`**, grab twice and nudge over a chip
   (memory `seeing-fontelles-gui`). Kill the nested server by pid when done
   (memory `gpu-shared-with-floptle`).
7. Publishing is Ty's: local preview build, then beta, then stable.

---

## 1. Where the tools stand

### 1.1 What there is

The menu is `TOOL_MENU` (`canvas/tools.rs:309`). The dialogs are
`ToolKind` (`:175`). The maths is in `fontelle-model/src/tools.rs`.

| Tool | Controls | Preview | Hear | Scope | Key |
|---|---|---|---|---|---|
| Transpose | semitones ±24 | ghost | – | selection | – |
| Adjust | property, amount, Add / Take off | – | – | selection | – |
| Randomize | strength, "sense" (Around/Anywhere), hidden seed | – | – | selection | – |
| Arpeggiate | step, direction, range, gate, repeat, swing, ramp | ghost | yes | selection, grouped into chords | – |
| Quantize | grid, strength, ends | ghost | yes | selection (Ctrl+Q: else the clip) | Ctrl+Q |
| Legato | none | – | – | selection | Ctrl+L |
| Import MIDI / FL score, Export MIDI | file actions, not edits | | | | Ctrl+Shift+E |

These live elsewhere:
- **Smart chords:** H, `canvas/chord_strip.rs`.
- **Scale and Fit to key:** `canvas/roll_scale.rs`. Acts on the selection, or the whole clip.
- **Scale lock:** K.
- **Edit tools:** Draw, Paint, Select, Delete, Slice. These are on the toolbar.
- **Property lane:** six properties.
- **Clipboard, Duplicate, arrows.**
- **Overlap hatching.**

What each tool does is written to a high standard. The arpeggiator's
chord grouping, swing and ramp, quantize's id-keeping `Retime` and legato's
three rules are each better reasoned than their FL counterparts. Section 1.2
is about the frame around those tools. The tools themselves are sound.

### 1.2 What is wrong today, with the evidence

**Defects** (behaviour that contradicts its own code or comments):

- **D1. Arpeggiate takes two undos, and the first one empties the clip.**
  The code is `vec![RollEdit::Remove(selection), RollEdit::Insert(made)]`
  (`canvas/tools.rs:697`). Its comment says the two are "one gesture", but
  `RemoveNotes::merge_with` returns false (`commands.rs:3146`). I saw the
  result on `:99`: I applied an arpeggio and pressed Ctrl+Z once, and the
  roll was blank, with neither the run nor the chords. The remove also fires
  the delete-particle burst (`app.rs`, `apply_roll_edits`), so the chords
  "explode" as the arpeggio appears.
- **D2. The row tips are written and never shown.** `Tools::tip()`
  (`tools.rs:526`) has no caller, because the hover chain does not cover
  dialog rows.
- **D3. Stale words.**
  - The chip's tooltip still reads *"Transpose, adjust, randomize — and import a file"*.
  - `tools.rs:26` says "three tools", and so does `tools.rs:301`.
  - The doc on `RollControl::Tools` (`piano_roll.rs:1536`) describes the old bench.
- **D4. `Tool::Mute` and `Tool::Slip` are declared** (`piano_roll.rs:1457`)
  with tips, but they have no icon, no key and no behaviour. `Note` has no
  mute flag.

**UX faults** (seen on `:99`):

- **U1. Nothing shows that a value can be changed, or how.** A click steps a
  value forward and Ctrl+click steps it back (`press_tools_panel`,
  `app.rs:23347`). There is no arrow, no −/+, no drag and no typing, so going
  from gate 50 % to 80 % takes six clicks, and a new user cannot discover
  Ctrl+click.
- **U2. The dialog covers what it previews.** It opens under the chip, on top
  of the roll's right half (`tools_dialog_layout`, `tools.rs:879`, 188 px
  wide). On the tour song it hid the second bar's chords, the very notes
  being previewed. It cannot be moved.
- **U3. The preview is hard to read.** Ghosts are drawn *over* the original
  notes, which stay at full brightness, so an arpeggio's ghost reads as
  little squares sitting on top of the chord. Before and after cannot be told
  apart at a glance.
- **U4. Previews that land off-screen are invisible.** Transpose +12 ghosts
  above the view, and only one sliver showed on the tour song.
- **U5. Half the dialogs preview nothing.** Adjust and Randomize return no
  ghosts (`Tools::preview` gives empty), and their changes are to velocity
  and similar properties, which the ghosts do not show anyway. Transpose has
  no *Hear it*.
- **U6. The scope rule differs from tool to tool.** Every dialog refuses with
  no selection ("Select some notes first", `app.rs:23404`). Ctrl+Q and *Fit
  notes to key* take the whole clip. FL's rule, and Fontelle's own for those
  two, is: the selection, or else everything.
- **U7. The menu mixes edits and files.** Import MIDI, Import FL score and
  Export MIDI sit under the same chip as Transpose.
- **U8. Words.**
  - "Sense: Around / Anywhere" is not a term anyone uses.
  - "+12 st", and "vel" for velocity.
  - "Amount 10" has no unit.
  - Randomize's seed exists but is never shown, so a roll you liked cannot be gone back to.
- **U9. No speed.**
  - Only Quantize and Legato have keys. Others: none.
  - There is no way to run the last tool again, and no Reset.
  - A tool's settings live on the `Tools` struct in the window's state, and I did not confirm they survive a restart.

---

## 2. What the others have

Full survey in §9; the condensed result:

**Every DAW has these, and so do we:** quantize, transpose, legato,
arpeggiate.

**The commonly expected tools we lack**, roughly in how often they appear:

1. **Humanize timing.** Live, Logic, Bitwig, Cubase and Reaper all have it.
   FL spreads it across Randomize and Articulate. Ours is deliberately
   property-only (PROGRESS: *"the randomizer does not randomize pitch or
   timing"*). This is §7 Q4.
2. **Velocity shaping:** scale, compress, limit, fixed and ramp/crescendo.
   FL Scale levels, Cubase Velocity, Logic Crescendo, Live Velocity Shaper.
   Our Adjust only adds and subtracts.
3. **Chop.** FL (pattern, and Ctrl+U by snap), Live 12 (parts, gaps,
   emphasis), Bitwig (slice in place).
4. **Glue / Join.** FL Ctrl+G, Live Ctrl+J, Reaper, Cubase.
5. **Flip in time / reverse.** Every DAW. FL also has a "keep start grid"
   option, and Bitwig separates Reverse from Reverse Pattern.
6. **Flip in pitch / invert.** FL, Live (scale-aware), Logic (pivot), Cubase.
7. **Strum.**
   - FL: time with a sign for direction, tension, velocity change, keep ends, trigger ahead, alternate direction.
   - Live 12: strum low/high and tension.
   - Logic: Q-Flam.
8. **Lengths / articulate:** staccato %, fixed length, gap, min/max. FL
   Articulate, Live Span, Logic, Cubase.
9. **Overlap cleanup:** delete doubles, and mono/poly overlap trimming.
   Cubase and Logic; FL does it with Select overlapping.
10. **Flam / grace notes.** FL Flam, Live Ornament.
11. **Selection tools:** by key, random, off-beat, overlapping, invert. FL's
    Select menu, Live's Find and Select.
12. **Speed ×2 / ÷2.** Logic, Live, Bitwig.

**The signature ideas worth taking:**
- FL Claw machine: thin out every Nth slice.
- Live Recombine: shuffle the pitches but keep the rhythm.
- Logic Q-Range: quantize only the badly-off notes.
- Live Time Warp: accelerando.
- Bitwig's "Quantize" (no ellipsis), which repeats the last settings.
- FL's quick-plus-dialog pairs: Ctrl+U / Alt+U, Ctrl+Q / Alt+Q.

**What nobody has found a better UX for:**
- A live preview while the dialog is open. FL redraws the roll; Live has
  Auto Apply.
- Reset.
- Selection-else-everything.
- A visible seed with ◀ ▶.

Fontelle already has the first, and with *Hear it* it goes a step further
than either of them.

---

## 3. The foundation (Phase 1)

New tools on today's frame would inherit U1–U9 nine times. The frame comes
first, and it is most of the round's lasting value.

### 3.1 One shape for every tool's result

```rust
// fontelle-model
pub struct ToolResult {
    pub rewrite: Vec<(NoteId, Note)>, // same id, any field changed
    pub remove: Vec<NoteId>,
    pub insert: Vec<Note>,
}
```

A tool is `fn(notes: &[(NoteId, Note)], spec, ctx) -> ToolResult`. Here
`ctx` holds the grid, the key/scale, the clip's end and the seed.

`ToolResult` becomes one `RollEdit::Apply { label, result }`.
`Session::edit` builds a single `Compound` from it, the way `Retime` already
does (`session.rs:8575`). The Compound contains only the changes it needs:
- `MoveNotes` per moved note.
- `SetNoteLengths`.
- `SetNoteKeys`.
- `SetNotePropertyEach` per property.
- `RemoveNotes`.
- `AddNotes`.

What this buys:
- **D1 fixed by construction.** Remove and insert are one Compound, so one
  undo.
- **No new wire tags.** Collaboration replays a Compound already
  (`wire.rs:61`).
- **Ids kept** for tools that only rewrite, so the selection and anything
  keyed by id survive.
- **The ghosts are drawn from the same `ToolResult`:** rewritten notes at
  their new place, inserts as ghosts, removes hidden.
- **The result is selected after Apply**, so tools chain: arpeggiate, then
  strum the run. This needs a test, because today I could not confirm that
  inserted notes come back selected.
- The delete burst fires only for a real delete, never for a tool's
  `remove`.

Port Transpose, Adjust, Randomize, Arpeggiate, Quantize and Legato onto it.
Their maths does not change; only their output type does.

### 3.2 The scope rule

**The selection; with nothing selected, every note of the open clip on the
selected channel** (`Session::roll_notes`). This is already the rule for
Ctrl+Q and Fit to key; it becomes the rule for all tools. The dialog's title
row says which, e.g. *Strum — 12 selected* or *Strum — whole clip (48)*, so
nobody is surprised.

### 3.3 Controls that show themselves

Each value row becomes `‹ value ›`:
- The arrows step the value; Ctrl+click on the value still steps it back.
- Dragging horizontally on the value scrubs it.
- A double-click types it, through the shared `TextEntry`. Add the field to
  `WindowApp::active_field`, or a click ends it (memory
  `feedback-round-2026-10-20`).
- No wheel.

Further changes to the rows:
- Toggles become a check, and choices (direction, mode) become a small
  segmented row where they fit.
- Every row's tip is shown on hover. That is D2's fix, through the existing
  hover chain.
- Every dialog gains **Reset**.
- Seeded tools show **Seed ◀ n ▶**.

### 3.4 Where the dialog sits

Today it covers the notes (U2). There are three options; the choice is §7 Q1:

- **A. A tools drawer docked at the roll's right edge**, full roll height,
  the grid narrowing while it is open.
  - Never covers notes, and has room for tall dialogs and a drawn curve (strum tension, velocity ramp).
  - Costs ~220 px of roll width while open, and it is a new panel to build.
- **B. A floating dialog that opens beside the selection, not over it**
  (the side with more room), and can be dragged by its title. It remembers
  where it was put.
  - Cheapest, and keeps today's look.
  - Can still land on notes in a dense clip.
- **C. Today's placement**, with the fixes elsewhere.

**Recommendation: B now, built so that A is only a different `frame`.**
`tools_dialog_layout` already returns a frame, and rows are laid out inside
it.

### 3.5 A preview you can read

- **While a preview is showing, the notes it changes are drawn dimmed**, as
  outlines only. The ghosts take the full accent colour. Before and after
  then read at a glance (U3). The headless dump checks this in pixels.
- **A preview that leaves the view gets an edge marker:** a small chevron on
  the grid's edge on the side it went to, with a count (U4).
- **Property changes preview in the property lane**, as ghost stems beside
  the real ones. That covers Adjust, Randomize and the new Levels (U5).
- ***Hear it* on every dialog that changes notes**, including Transpose.
- **Enter applies, Esc closes.** Apply leaves the dialog open, as today.
  Ctrl+Z after an Apply undoes and leaves the dialog open, so you can try
  again.

### 3.6 Memory and speed

- **Each tool's settings are saved in the user's settings file** (a
  `roll_tools` map, serde default, skipped when equal to default), with
  Reset beside them.
- **Run the last tool again** with its last settings and current scope:
  `Action::RepeatTool`, default **Ctrl+Shift+T** (free; Ctrl+T is the
  timeline). Nobody in §2 has a generic repeat-last; Bitwig's no-ellipsis
  Quantize is the nearest.
- **Quick-plus-dialog pairs, FL's muscle memory.** See §5 for keys.

### 3.7 The menu, reorganised

The menu is grouped, with a small heading per group. Notes:
- **Timing:** Quantize, Strum, Humanize*, Flip in time.
- **Pitch:** Transpose, Flip in pitch, Fit to key (also stays in the scale menu).
- **Levels:** Levels, Randomize.
- **Length:** Legato, Lengths.
- **Shape:** Chop, Glue, Flam*, Thin out*, Shuffle*.
- **Generate:** Arpeggiate, then later items.
- **Select:** §4.4.

*Phase 3; see §6.*

The three file actions move to a **Files** group at the bottom, titled as
such (§7 Q6 if Ty would rather they left the menu). Each row shows its key.

---

## 4. The new tools

Each tool is one pure function in `fontelle-model/src/tools/` (a module
per tool; `tools.rs` is 550 lines and will not hold twelve). Each gets one
test file, `fontelle-model/tests/<tool>.rs`, and its dialog in
`canvas/tools/<tool>.rs`. Parameters are given with defaults.

### 4.1 Phase 2: what users expect to find

**Strum** (FL Strum, Live Strum). Spreads each chord's start times.
- Chords are notes sharing a start, within a small catch window.
- Controls:
  - **Time**, as a grid fraction or ms: ¼ step.
  - **Direction:** up / down / alternate (per chord) / random.
  - **Tension:** −100…100, where 0 is even and positive accelerates.
  - **Velocity:** −20…+20 per note across the strum.
  - **Keep ends:** on, so lengths shrink and the chord releases together.
  - **Before the beat:** off; when on, the strum straddles the beat.
- Single notes are untouched.
- Output: rewrite.

**Chop** (FL Chop, Live 12 Chop). Cuts each note into repeats; Ty's
*"repeated chops"*.
- Controls:
  - **Step**, a grid fraction or "snap", or **Parts** N.
  - **Gate:** 100 %.
  - **Pattern:** a row of up to 16 step toggles drawn in the dialog, where an
    off step leaves a gap, e.g. `x x . x`. Default all on.
  - **Velocity:** flat / ramp down / ramp up / accent first.
  - **Pitch:** none, or a semitone step per chop, for a stair (FL can only do
    this with a pattern file).
  - **Grid-locked vs per-note:** chops fall on the song's grid, or start at
    each note's own start.
- Quick form, **Ctrl+U**: chop the selection by the snap.
- Output: remove + insert. One undo, by §3.1.

**Glue** (FL Ctrl+G, Live Ctrl+J). Joins same-key notes that touch or
overlap (within the catch window) into one. It is Chop's inverse.
- There is no dialog; the key is **Ctrl+G**.
- The velocity and other properties are the first note's; the lyric is
  joined.
- Output: rewrite of the first + remove of the rest.

**Flip** (FL Flip, Live Reverse/Invert, Bitwig Reverse/Reverse Pattern).
- **In time:** mirrors the selection's span.
  - *Keep the grid*: starts stay on their original grid slots, and only the
    order reverses. This is FL's "preserve start times" and Bitwig's Reverse
    Pattern.
  - Paths (note slides) reverse with their note.
- **In pitch:** mirrors around the selection's middle key, or around a
  chosen key.
  - **In key** when the clip has a scale, by scale degree (Live's way). Alt
    for chromatic, matching the scale lock.
- Output: rewrite.

**Levels** replaces Adjust, which it contains (FL Scale levels, Cubase
Velocity, Logic Crescendo, Live Velocity ramp). It works on any of the six
properties. Mode:
- **Add:** today's Adjust.
- **Set** to a value.
- **Scale %.**
- **Compress / expand** toward a centre.
- **Limit** to a low…high range.
- **Ramp** from A to B across the selection's time, with a curve.

The preview is in the lane (§3.5).

**Humanize.** Only if §7 Q4 says yes. It is Randomize grown to cover time:
- **Timing** ± a grid fraction, never crossing the next event.
- **Length** ± %.
- The six properties, as today.
- Seed ◀ ▶.

Pitch stays out, as PROGRESS decided.

**Lengths**, the Articulate tool (FL Articulate, Live Span).
- Mode:
  - **Legato** (today's Ctrl+L, now with a gap/overlap value).
  - **Staccato %.**
  - **Fixed** length (a grid value).
  - **Min / max** length.
  - **Ensure a small gap.**
- Ctrl+L stays the quick legato.

**Transpose**, upgraded:
- **Octave −/+** buttons.
- **In key** (scale degrees) when the clip has a scale.
- **Keep in range** low…high, wrapping by octave (Cubase, FL Limit).
- *Hear it*.

**Clean up overlaps** (Cubase Delete Overlaps and Delete Doubles; the hatching
from 0.30.0 already shows where):
- **Remove doubles:** same key, same start.
- **Trim overlaps:** same key; the earlier note ends where the later one
  starts.
- **Mono:** across keys, for basslines and leads.

There is no dialog: three rows in the menu's Length group.

### 4.2 Phase 3: the ones that make it feel deliberate

- **Flam / grace notes** (FL Flam, Live Ornament). Adds a short note before
  each note.
  - Controls: time; before / on the beat; velocity %; grace pitch (same /
    above / below, in key).
  - Output: insert.
- **Thin out** (FL Claw machine). Cuts the span into slices of a period,
  removes every Nth, and can *stretch to fill*. It turns a steady roll into
  a stutter or a snare build.
- **Shuffle** (Live Recombine).
  - Keep the rhythm and shuffle, rotate or mirror the **pitches**; or keep
    the pitches and do it to the **velocities** or **lengths**.
  - Seeded.
  - This is the closest thing to a riff generator that does not invent
    notes.
- **Speed** ×2 / ÷2 / a factor, anchored at the selection's start (Logic,
  Live, Bitwig). An accelerando curve (Live Time Warp) is noted, not
  planned.
- **Quantize, round two.** Add these to the existing tool:
  - **Swing** (Logic Q-Swing, Cubase).
  - **Only notes further than X** from the grid (Logic Q-Range), which keeps
    a near-grid feel.

### 4.3 Mute (D4). Decision, not default

A note mute (FL's Mute tool and Edit ▸ Mute) needs these:
- A `Note::muted` field, `#[serde(default, skip_serializing_if)]`.
- A command `SetNoteMuted`, which is a new wire tag (§7 Q5).
- The compiler skipping muted notes, which needs a check against the TDD's
  INVARIANTs.
- A look for muted notes: hollow, with the hatching.

Slip has no defined meaning anywhere in the tree. **Recommendation:** build
Mute in Phase 3 if Ty wants it; delete `Tool::Slip`.

### 4.4 Select (Phase 2, small)

A **Select** group in the menu (FL's Select menu, Live's Find and Select).
It changes only the selection, so it is not an edit:
- Same key as the clicked note.
- Overlapping.
- Off-beat.
- Random % (seeded).
- Invert.
- Shorter / longer than the snap.
- Velocity above / below.

Because tools act on the selection, this multiplies every other tool:
"select off-beats, then Levels −20" is a groove.

### 4.5 Not in this round, and why

- **Riff machine and a chord-progression generator.** Each one is a product
  of its own. Smart chords already covers chord entry; a generator deserves
  its own plan.
- **LFO tool** (FL): Levels' ramp covers the common case.
- **Groove templates from a clip** (FL `.fsc` patterns, Logic grooves):
  wanted eventually; they need a groove file format.
- **Non-destructive per-clip quantize** (Logic's region Q-parameters): a
  change to what a clip *is*, not a tool.
- **Playback-time chance and repeats** (Bitwig Operators, Live Chance): an
  engine feature, not a roll tool.

---

## 5. Keys

All are new `Action`s with catalogue lines, in the roll's context. FL's keys
are used where they are free. Collisions checked against
`keymap.rs` `defaults()` on 2026-10-20:

| Action | Key | Note |
|---|---|---|
| Quick quantize (exists) | Ctrl+Q | |
| Quick quantize, starts only | Shift+Q | FL |
| Quick chop by snap | Ctrl+U | FL |
| Glue | Ctrl+G | FL |
| Legato (exists) | Ctrl+L | |
| Repeat last tool | Ctrl+Shift+T | new |
| Open a dialog | Alt+letter, FL's (Alt+S strum, Alt+U chop, Alt+A arp, Alt+Q quantize, Alt+R randomize, Alt+L lengths, Alt+Y flip) | **Alt+S/Q/R already mean Smooth/Snap/Reset pitch on vocal clips.** Keep those on vocal clips, use FL's everywhere else (the keymap already has contexts). That is §7 Q3. |
| Duplicate (exists) | Ctrl+D | FL uses Ctrl+B, which is split-at-marker here, and stays so |

---

## 6. Build order

**Phase 0: the defects (half a day).**
- D1: one undo for the arpeggio, and no burst. Test:
  `fontelle-app/tests/quantize_window.rs`'s pattern, one Ctrl+Z restores
  the chords.
- D2: tips shown. Test: `tools_panel.rs`.
- D3: words.

This can ship on its own as a patch.

**Phase 1: the frame (2–3 days).** In this order:
1. §3.1 `ToolResult` and `RollEdit::Apply`.
2. Port the six tools.
3. §3.2 scope.
4. §3.5 dimmed originals and lane ghosts.
5. §3.3 controls.
6. §3.4 placement.
7. §3.6 memory and repeat.
8. §3.7 menu.

Tests:
- `fontelle-model/tests/tool_result.rs`: Apply then undo returns identical
  `NoteData`, ids kept on rewrite, one history entry.
- `tools_panel.rs`.
- A new `fontelle-app/tests/roll_tools_window.rs` covering scope, the
  result being selected, Enter/Esc, Reset, repeat-last and settings round
  trip.
- The headless dump for dimming and the edge marker.

**Phase 2: the expected set (4–5 days).** Strum, Chop (+Ctrl+U), Glue,
Flip, Levels, Lengths, the Transpose upgrade, Clean up overlaps, Select, and
Humanize if yes.
- One model test file each.
- Each property stated as a test: e.g. "chop then glue gives back the
  original notes", "flip in time twice is the identity", "strum never moves
  a single note".

**Phase 3: the signature set (3 days).** Flam, Thin out, Shuffle, Speed,
Quantize swing and range, and Mute if yes.

**Each phase ends** with the full suite once in the background, clippy,
preflight, a `:99` pass over every dialog it touched, PROGRESS updated and
`main` pushed. Ty gets a local preview build after Phases 1 and 2.

---

## 7. Decisions for Ty

1. **Where the dialog lives:** a drawer docked at the roll's right edge (A),
   a floating dialog beside the selection that you can drag (B), or as now
   (C). *Recommended: B, built so A is easy later.*
2. **Scope with nothing selected:** every tool acts on the whole clip, as
   Ctrl+Q and Fit to key already do. *Recommended: yes.*
3. **FL's Alt+letter keys** for the dialogs. On vocal clips, Alt+S/Q/R keep
   meaning pitch smoothing, snapping and reset. *Recommended: yes, with that
   exception.*
4. **Humanize timing.** In September we decided the randomizer would not
   touch timing. Every other DAW lets you nudge timing by a small random
   amount. *Recommended: add timing and length to Randomize (renamed
   Humanize). Pitch stays out.*
5. **Note mute** (a new saved field, and a new kind of edit that
   collaboration has to carry). *Recommended: Phase 3. And delete the
   never-built Slip tool.*
6. **Import/Export MIDI**: keep them in the Tools menu under a "Files"
   heading, or move them out. *Recommended: keep them, under the heading.
   People look for them there.*
7. **Which tools first.** Phase 2 as listed, or reorder, e.g. Strum and Chop
   before the frame so you can play with them sooner. *Recommended: the frame
   first. Every tool built on today's frame would need its controls redone.*

---

## 8. Risks and honest caveats

- **`app.rs` is 27,408 lines**, and every dialog today adds window code to
  it. Phase 1 should move the tools' window side into
  `canvas/tools/`. Otherwise it grows by another thousand lines.
- **The 188-px dialog cannot hold Chop's step row or a drawn tension
  curve.** B's dialog grows per tool; that is fine, but a drawn curve is
  Phase 3 at the earliest. Until then, tension is a number.
- **A strum or flam in ms vs grid.** Grid fractions follow tempo; ms is what
  a guitarist means. The plan offers both, and the default is the grid (FL
  and Live default to the grid). Worth checking by ear on the store song.
- **Chord detection** (what counts as "the same start") needs a catch
  window. The arpeggiator groups by overlap instead. Strum's rule must be
  stated as a test and kept the same everywhere it is used.
- **Ghosts for removed notes.** Chop and Glue rewrite many notes at once,
  and hiding the originals while drawing the result could flicker on every
  setting change. Watch the frame rule from memory `shaped-label-cache`.
- **Collab.** Compounds of existing commands replay today, but nobody has
  replayed one this large (a chopped clip can be hundreds of `AddNotes`).
  Test one over two studios on `:99`/`:98` in Phase 2.
- **Unverified here:** whether tool settings survive a restart today, and
  whether inserted notes come back selected (§3.1). Both become tests, not
  assumptions.

---

## 9. Sources

FL Studio manual (Image-Line), `…/fl-studio-online-manual/html/`:
- `pianoroll_menu.htm`.
- The tool pages `pianoroll_strum.htm`, `_chp`, `_arpeggiate`, `_flam`, `_claw`, `_qnt`, `_limit`, `_flip`, `_random`, `_scale`, `_lfo`, `_articulate` and `_riff`.

Ableton Live 12 manual: `ableton.com/en/live-manual/12/midi-tools/` and
`/editing-midi/`.

Logic Pro: MIDI Transform presets (`help.apple.com/logicpro/mac/10.1/en.lproj/lgcp215831be.html`)
and region Q-parameters.

Cubase/Nuendo help (`steinberg.help`): MIDI functions, Velocity, Legato,
Transpose Setup and the Quantize panel.

Bitwig user guide: event functions and operators. Reaper wiki: MIDI Editor
menus.

Unconfirmed in those sources:
- An official FL "Humanize".
- Logic's Mac-only Force Legato variants.
- Cubase's Mirror and Restrict Polyphony parameters.
