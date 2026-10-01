# Note paths — a note that slides where you draw it

Designed and built 2026-10-01, all six phases. **Not released**: Ty tests it
locally first. This replaces the slide note.

## As built, and where it differs from the plan below

- **Ty's calls**: "build this all out", so D1–D5 went with the recommended
  options, listed in §8 with what was done for each.
- **The format** is `PathPoint { at, offset: i8 }`, with no `cents`
  (D4: whole keys). A defaulted field can add cents later.
- **Resizing** changes only the length. A point past the end is kept, and
  the curve is cut where the note stops, so lengthening the note again
  brings the slide back. Dragging a point that sits on the note's end moves
  the end with it.
- **Esc to cancel a drawing** is not built. Backspace takes points back,
  and Ctrl+Z undoes the whole note.
- **The MPE switch** is in the rack's channel menu ("Slides as MPE"),
  offered only for a plugin that hears raw MIDI. Bridges get MIDI through
  an optional symbol, `fontelle_bridge_midi`, not a new ABI version, so
  every installed bridge still loads. `fontelle-vst2` exports it (its
  commit `bd370a6`); a bridge without it keeps the channel bend.
- **The arrangement preview** draws a slide as a staircase
  (`Note::preview_pieces`).
- **MIDI import** reads an MPE lower zone back as one part, each note's bend
  curve thinned into its path. The export also writes a bend where each
  slide begins, so a hold reads back as a hold.

Ty: *"when dragging out a note, you can press the s key to place a point there
so the note extends to that point, then past that point it becomes a slide
note, sliding to wherever you decide to place the second part of the note ...
drawn as a diagonal note between the points where its flat so it all looks like
one continuous note ... multiple notes sliding at the same time in a chord ...
sliding to different notes ... as many times as i want."*

## 1. What is wrong with the slide note we have

`Note::slide` is FL's: a separate note that starts nothing and bends
**everything sounding in its voice context** to its key
(`Sampler::slide`, `PluginNode`'s `NoteSlide` arm). That one choice causes
all three of Ty's complaints:

- **A chord can't spread.** A slide names only the key it goes *to*, not the
  note it moves, so it moves the whole chord to one key. Three notes can't
  slide to three different places.
- **It doesn't look like what it does.** You see two bars, and the bend
  between them is hidden in the second bar's flag.
- **It only works in a context.** Delete the note in front of it and it
  silently does nothing.

The engine underneath is fine: `Voice::glide_to` already bends a single voice
without changing its key, so the note-off still finds it. CLAP and VST 3
already get a per-note `Tuning` expression (`HostedProcessor::note_tuning`).
What is wrong is the **addressing** (all voices in the context, when it should
be one note) and the **model** (a second note, when it should be one note with
a shape).

## 2. The model: a note is a path

```rust
pub struct Note {
    pub start: Tick, pub key: u8, pub length: Tick, // unchanged: the first vertex and the end
    ...
    /// Where the note goes after it starts, in time order. Empty = a plain note.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<PathPoint>,
}
pub struct PathPoint { pub at: Tick /* from the note's start */, pub key: u8, pub cents: i16 }
```

- The vertices are `(start, key)`, then each `path` point, then `(start + length,
  last key)`. Each segment is a straight line in **semitones over ticks**. If
  both ends are on the same key, the segment is flat (a hold). If not, it is
  diagonal (a slide).
- Hold, slide, hold, slide... in any order and any number of times. A whole
  melody from one note is one `Note` with a long `path`.
- `cents` is there now so a path can end between keys later without another
  format change. It stays 0 until the free-pitch modifier exists (§8 D4).
- **Old projects keep playing.** `slide: bool` stays readable and compiles
  exactly as it does today. See D1 for whether files get converted.

Why on the note, rather than as a lane or a second note: it moves, copies,
transposes, quantizes and deletes as one thing, and the roll can draw exactly
what the engine plays. It is also what CLAP and VST 3 model natively: one note
id and a tuning expression over time.

## 3. Drawing it

The gesture Ty described, with the drawing rule made exact:

1. Press on the grid and drag. The note grows as it does today.
2. Press **S** while still dragging. The current pointer position becomes a
   vertex; the line so far is fixed.
3. Keep dragging. A **live segment** runs from the last vertex to the pointer.
   Moving straight along the row makes it flat; moving to another row makes it
   diagonal. Either way, the pointer is where that part ends.
4. Press S again to fix another vertex, as many times as you like.
   **Backspace** mid-drag removes the last vertex. **Esc** cancels the whole
   note.
5. Releasing the mouse makes the pointer the note's end.

So "flat to here, then slide to there" is: drag along, S, drag up and right,
release. "Slide, then hold the new note" is one more S before moving flat.

- Vertices snap to the grid in time and to whole keys in pitch, with the roll's
  snap setting. The roll's existing no-snap modifier frees the time.
- **S is the snap key** in the roll today (`Action::SnapOrStretch`). Mid-drag
  it means *place a point* instead: a new `Action::PathPoint`, bound to S in
  the keymap and active only while a note is being drawn. This follows the
  keymap rule: a new Action plus a catalogue line, never a key literal. Ty may
  prefer a different key (D2).
- **Chords:** draw several notes, select them, and drag a vertex of one. Each
  note keeps its own path. Sliding three notes to three places is three paths,
  not a special feature.

**Editing a path after it's drawn** (the parts that make this better than FL):

- Each vertex shows a small handle when its note is selected or hovered. Drag
  it to move it in time and pitch. Its neighbours' segments follow.
- Double-click a segment to add a vertex there. Double-click a vertex to remove
  it.
- Moving, copying, transposing, quantizing and the scale tool apply to every
  vertex: transposing shifts the whole shape, quantizing snaps vertices, and
  "fit to scale" fits each vertex key.
- **Ctrl+B (split at the marker)** cuts a path into two notes. The second
  starts at the pitch where the cut lands, rounded to a key.
- **Resizing the end** stretches the last segment only.

## 4. Drawing it on screen

One continuous ribbon: flat segments are the bar exactly as a note is drawn
today, and diagonal segments are a parallelogram of the same thickness joining
them. It reads as one note bending, in the note's colour, with the velocity
shading along its whole length. Vertex handles are only drawn on selected or
hovered notes, so a dense part stays readable. The label (the key name) stays
at the start.

Hit-testing: a click on any part of the ribbon is a click on that note.
`roll_notes`/`touch()` and the rack rule are unchanged, because it is still one
note on one channel.

## 5. Playing it: compiled events

The compiler turns one path note into:

- `NoteOn { key: start key, ... }` at the start, as today.
- For each **diagonal** segment, at its start: a new
  `NoteGlide { key, voice_context, to_semitones: f32, glide_samples }`. It is
  addressed to **one note**, the one started at `key` in `voice_context`, and
  the amount is in semitones from that note's own key. Flat segments compile
  to nothing; the note just stays where the last glide left it.
- `NoteOff { key: start key }` at the end, as today. The voice's key never
  changed, so it is found.

(start key, context) is the same address `NoteOff` already uses, so this adds
no new ambiguity. Two notes on the same key in the same clip at the same time
are already ambiguous for note-off, and that rule stays as it is.

Ordering: a glide at tick `t` goes after a note-on at `t` (the existing rank-3
slot `NoteSlide` uses).

## 6. The question Ty asked: will plugins understand it?

Short version: **yes for modern plugins, mostly for the rest, with a
documented fallback, and the built-in instruments get it fully.** This is how
Bitwig's note expressions and other per-note slides reach plugins. I don't know
how Septabee does it, but every route is one of these, because these are all
the routes the formats have.

| Instrument | How a per-note slide gets there | Limits |
|---|---|---|
| Built-in (Flopsynth, sampler, soundfonts, drums) | `Voice::glide_to` on the one voice. It's ours. | None. Any distance, any chord. |
| **CLAP** | `Tuning` note expression on that note. **Already wired.** | None. |
| **VST 3** | `kTuningTypeID` note expression on that note id. **Already wired.** | None. |
| LV2, VST 2, bridged; any plugin that says it does MPE | **MPE output (new):** each note goes out on its own member channel, so its own pitch bend moves only that note. | ±48 semitones by MPE's default range; most MPE synths (Vital, Surge XT, Pigments, Serum 2...) accept this. |
| The same, with a plugin that does not do MPE | Channel pitch bend, as today. | Works exactly for **one note at a time**. In a chord, every note follows the most recent slide, the same as FL Studio sliding into a VST 2 synth. Range is the plugin's bend range (we assume ±2 unless told; see D5). |

The router already *receives* MPE (`NoteMod`, flopsynth-next §4.2). The new
part is *sending* it: a member-channel allocator in `PluginNode` with a fixed
table and no allocation (INVARIANT 1), the MPE configuration message (RPN 6)
on activation, and pitch bend sent on the note's own channel. That is one
self-contained piece of work, and it improves live MPE playing into LV2
plugins too.

**Which way a plugin is driven** is a per-plugin setting in the plugin window:
*Slides: Note expression / MPE / Channel bend*. The default is picked from the
format (CLAP and VST 3 → note expression, everything else → channel bend). A
plugin with no notion of pitch at all, such as a drum machine plugin, just
plays its notes; the slide does nothing to it, and the roll shows the path so
nobody is lied to.

**MIDI export** gets better too. Today it drops slides (`midi_export.rs`). A
path note can be written as MPE (its own channel, a pitch-bend curve on it,
range set by RPN), which Bitwig, Ableton, Reaper and Logic all read. Plain MIDI
export with one bend curve per channel stays available.

## 7. Phases (each tests first, confirmed failing)

0. **Model + compile.** `Note::path`, serde default, `NoteGlide` event,
   compile + ordering. Tests: a three-vertex path compiles to on / glide / off
   at the right samples, and an old `slide: true` project compiles exactly as
   before.
1. **Built-in playback.** `Sampler::glide_note` addresses one voice. Tests:
   two notes of a chord slide to two different keys (measured over ~64 blocks
   as a ratio, and one block *after* the change because of the limiter's 2 ms
   lookahead; see the performance-events traps).
2. **CLAP / VST 3.** `PluginNode` routes `NoteGlide` to the one sounding
   entry. This is a small change, because `Sounding` already holds per-note
   semitones.
3. **The roll:** ribbon drawing, the S-gesture, vertex handles, edit commands
   (`SetNotePath`, undoable). Look at it in the headless dump and on `:99`
   before believing it.
4. **MPE out** for LV2/VST 2/bridges, plus the per-plugin *Slides* setting.
5. **MIDI export** as MPE, and import of MPE pitch curves into paths.
6. **Old slides:** retire the tool, and convert old slides (D1).

Phases 0–3 are the feature Ty described, playing perfectly on everything
built-in plus CLAP and VST 3. Phase 4 is what makes chords slide apart on LV2
and VST 2 synths.

## 8. Decisions that were Ty's (each was taken as recommended)

- **D1 — old slide notes.** *Taken: (a).* The chip is off the toolbar; `A`
  still marks one, and old songs play as before. (a) Keep them working and hide the tool
  *(recommended)*, or (b) convert them to paths when a project opens. A
  conversion is exact for a single-note line. Under a chord, the old slide
  moved every note to one key, so converting means one path per moved note
  that ends on that key. That is faithful, but it rewrites the file.
- **D2 — the key.** *Taken: S*, in its own keymap context ("Drawing"),
  rebindable on the shortcuts page. S mid-drag (as asked; steals the snap key only during a
  drag) or another key.
- **D3 — curve shape.** *Taken: straight.* Straight in semitones *(recommended: what you draw is
  what you hear)*, or a per-segment curve handle (ease-in/out), which can come
  later without a format change.
- **D4 — off-key points.** *Taken: whole keys.* Whole keys only for now *(recommended)*. A modifier
  for free pitch (microtonal stops) can come later; `cents` is already in the
  format.
- **D5 — a non-MPE plugin's bend range.** *Taken: ±2 and clamp*; the
  bend-range box is not built (MPE is the way past two semitones). Assume ±2 and clamp (today), or
  expose a *bend range* box per plugin so a ±12 or ±24 synth slides farther.
