# The next update — strategy for review

Written 2026-10-18, after 0.28.0. **Nothing here is built.** This is the
plan for Ty to read, push back on, and decide; the parts marked
**Decision** are his. The release path for this round is: a local preview
build for Ty → beta (`v0.29.0-beta.N`) when he says → stable later.

Every claim about the code below was traced in the source (file:line where
it matters). Where a cause is a guess rather than a finding, it says so.

---

## 0. Order of work — recommendation

Fix what makes collaboration feel broken *before* adding collaboration
features on top of it; presence cursors on a session that silently cuts
your audio would be polish on a cracked floor.

| Phase | What | Why first |
|---|---|---|
| **1. Shared-song bugs** | §1.1–1.5 | One of them can lose work. All are in the code you are about to extend. |
| **2. Session controls** | §2.1–2.4: session ending properly, presence (playheads + cursors), chat, kick/permissions | Builds on phase 1's fixes. |
| **3. Smart chords** | §3 | The headline feature; needs Ty's design calls first, so it is discussed while phases 1–2 are built. |
| **4. Small features** | §4: lane multi-select, Analyze button, note sound preference, delete particles, monitor button, wire unplug, preset preview | Each is a day or less; good preview-build filler. |
| **5. Theme editor** | §5 | The largest by far; recommend splitting it, see there. |
| **6. Discord status** | §6 | Small, independent, can land anywhere. |

---

## 1. Shared-song bugs (phase 1)

### 1.1 Opening another song does not end the session — **data-loss risk**

Found while tracing "it looks like my friend is still there".

**What happens.** Only the Share panel's buttons call `leave_song`
(`app.rs:24742-24770`). Opening a project from the browser, from the start
menu's recent list, or making a new one all go through `adopt_at`
(`session.rs:3700+`), which swaps the song and the history but **keeps the
live session object** (`self.collab`). So:

1. The host still looks hosting to everyone; the joiner never hears `Bye`.
2. The host's edits to the *new* song are not sent (the new history has no
   outbox) — fine — **but joiners' edits keep being applied to the new,
   unrelated song**, the hashes disagree, the joiner asks for a resync, and
   the host sends it a snapshot of the unrelated song. The joiner's copy
   keeps its file identity, so **a save would write the wrong song over the
   joiner's Shared copy.**

**Fix.** Opening, creating or closing a song while in a session ends the
session first, through the same path as Stop sharing (save prompt, `Bye`,
`shared_revision`). Plus: after any end, actually drop the transport, so
the relay closes the lobby rather than waiting on a `Bye` that might be
lost (today a host who pressed Stop sharing stays connected forever).

**Decision:** when you open another song while hosting, should Fontelle
(a) ask "End the session for everyone?" first, or (b) end it and say so?
I recommend **(a)** — it is the one moment a person can lose a friend's
work by accident.

### 1.2 The other person's edits cut your notes short

**Cause (firm).** Every edit that arrives from someone else rebuilds the
whole audio graph (`collab_effects`, `session.rs:2840-2850` →
`rebuild_graph`). A rebuild makes brand-new instrument nodes, and the swap
on the audio thread drops every voice the old nodes were playing, and every
reverb and delay tail. Your own edits don't do this — a local note edit
only republishes the timeline (`Session::run`, `session.rs:5854`). So a
busy collaborator cuts your playback every few hundred milliseconds.

**Options.**

- **(a) Sort edits by what they touch** — recommended. Notes/clips/tempo →
  republish the timeline only (what a local edit does). Fader/pan/send
  values → set the values in place (that path exists:
  `apply_mixer_controls`). Only structure (add a channel, an insert, a
  track, a route) → rebuild. One function, `fn needs_rebuild(&Edit)`, with a
  test per edit kind; the `graph_builds` counter is already there to assert
  on.
- (b) Carry voices across graph swaps. Fixes structural edits too, but it
  is deep engine work with real-time hazards; not this round.

**Caveat, honestly:** with (a), a *structural* remote edit (they add an
insert) will still cut sound once. That matches what happens when you add
an insert yourself today, so it is consistent, but it is not zero.

### 1.3 A soundfont routed to a new mixer track is silent

**Not pinned down in code — the likeliest cause is in your screenshot.**
Track 2's headphones are lit: it is **soloed**. Solo is part of the shared
song, so it applies on both machines, and a brand-new track is always
outside the solo set — silent — while routing to the soloed track "works".
I traced the new-track path (create → route → rebuild) on both sides and
found no step that leaves the track without a bus.

**Plan.** Reproduce on two studios on `:99`/`:98` first. If it is solo:
the fix is visibility, not engine — the soloed state should be impossible
to miss (a "SOLO" banner on the mixer, and a toast when someone else solos).
**Decision:** should solo be *shared* at all? Many collaborative tools make
solo/mute-for-listening **personal** (each person hears what they choose)
while mute-as-arrangement stays shared. I lean towards **personal solo**,
but it is a model change and worth your call.

### 1.4 Soundfonts copied from a shared song

Three separate faults:

1. **It doesn't show up in your folder** — it *is* on disk, but nothing
   rescans the soundfont list after a file arrives (`load_arrived`,
   `session.rs:2749`, never calls `bank.rescan()`). Easy fix.
2. **It lands in "the first folder"** (`banks.first()`), which may be a
   read-only system folder (Fedora `/usr/share/soundfonts`); the copy then
   fails and is marked "damaged". Fix: land in the first *writable* folder,
   and say where it went.
3. **One "Not now" or failure is permanent for the whole session**
   (`Files::knows`, `files.rs:161`): it is never asked about again, even if
   a later edit needs it. Fix: a declined file can be asked about again
   (a "Get missing files" button in the Share panel), and a host's "Not
   now" tells waiting joiners instead of leaving them at 0 %.

### 1.5 The same soundfont, not recognised

The hashes themselves agree everywhere (SHA-256 of the whole file — the
comment in `asset.rs:26` saying "xxhash of the first 1 MB" is stale and
will be corrected). The misses:

1. **Symlinks are skipped** (`find_by_hash` uses `DirEntry::metadata`,
   which doesn't follow links). Fix: follow them.
2. **Only your configured soundfont folders are searched**, not, say,
   Downloads. Correct in principle (paths in a song name someone else's
   disk), but it should say *"you have a file with this name in X but it is
   a different version"* when names match and bytes don't — today it just
   looks unrecognised.
3. **Songs saved before the hash change** carry old hashes no machine can
   match. Fix: re-hash on open, and on Share.
4. A separate hazard found on the way: on arrival, a file is looked up by
   the host's *path* first, trusting it without a hash check
   (`bundle.rs:107`). Two machines with different files at the same path
   (same distro path, different version) load the wrong one. Fix: check
   the hash.

---

## 2. Session controls (phase 2)

### 2.1 Sessions that end cleanly

After §1.1: every way a session ends — host stops, host quits, host's
connection drops, host opens another song, a joiner is removed — leaves
each person with **a clear card, not a toast**: *"Alice ended the session.
Your copy is open and yours now — Save it / Keep working."* The joiner's
copy should be marked unsaved at that moment (today `shared_revision` is
written in memory only and may never reach disk).

The relay already has the message for this (`LobbyEnd::HostLeft`, never
used); a host leaving cleanly should send it, so joiners hear within a
second rather than after a 20 s grace period.

### 2.2 Presence: everyone's time marker and cursor

**The position problem you described is the right model.** Each person
sends *where in the song* they are pointing, never *where on their screen*:

- **Arrangement:** `(tick, lane id, fraction down the lane)`.
- **Piano roll:** `(clip id, tick, key + fraction)` — and only drawn for
  someone viewing the same clip.
- **Mixer:** `(track id)` — highlight the strip rather than draw a cursor.

Each receiver converts that point through *its own* view (its zoom, scroll,
lane heights) and draws a cursor glyph of fixed size there, with the name
tag beside it. A point off-screen is drawn as an edge arrow with the name,
so you can see "Bob is working at bar 40" and click to go there.

**Engineering:**
- A new message `Presence { tick, view: Arrangement|Roll(clip)|Mixer, …,
  playhead, playing }`, appended to `Msg` (the list is append-only), sent
  on the **unreliable, sequenced** channel the transport already supports
  but the session never uses — a lost cursor update should be replaced by
  the next one, never retried. 15 Hz, only when it changed.
- Joiners can only talk to the host, so the host re-broadcasts each
  joiner's presence.
- **Two traps found in the code:** the session pump returns early while a
  gesture is in hand (`mod.rs:843`, `1386`), so presence would freeze during
  every drag; and the pacing queue is one FIFO, so cursors would queue
  behind file transfers. Both need handling, and both get a test.
- Playheads: each person's time marker as a thin line in their colour on
  the ruler and arrangement, with their name at the top.

**Decision:** "follow" mode (your view tracks another person's) — this
round or later? I'd say later; cursors and playheads first.

### 2.3 Session chat

A small panel that slides out from the Share button, closed by default; an
unread dot on the Share button when a message arrives; messages also flash
briefly by the sender's cursor (so you notice without opening anything).
Reliable channel; not saved in the song; the last ~200 kept for the
session. **Caveat:** chat text is shown to everyone in the session as
typed — it needs the same text rendering the rest of the UI has, including
non-Latin scripts and emoji fallback, which is worth testing on Windows.

### 2.4 Kick and permissions

What exists: Remove (kicks; the code stays usable) and View only. Proposed:

- **Remove** also locks the code against that person for the session
  (ledger item L6 in `docs/collab-plan.md` — already designed).
- **Permissions per person**, shown as a small menu on their row in the
  Share panel: *Edit / View only*, *May add plugins the host doesn't
  have*, *May add or change soundfonts and samples* (these are the ones
  that create missing-file problems for others), *May change the mixer*.
  Enforced by the host, which already refuses proposals (`mod.rs:1027`), so
  the check is one place: "is this edit allowed for this person?".

**Decision:** which permissions matter to you? I'd start with the three
above plus View only, and not go finer until people ask.

---

## 3. Smart chords (phase 3)

### 3.1 The design problem

Beginners need *"show me chords that sound good here"*; theory people need
*"let me build exactly the chord in my head, fast"* — and neither should
feel the tool is in the way. The trap on both sides: a list of 40 options
overwhelms a beginner, and a list of 7 triads bores an expert.

### 3.2 Proposed design — layered, so each person stops where they want

**Layer 1 — the strip (beginners land here).** With a scale set and Smart
chords on (a chip next to *scale ▾*, with a keybind), placing a note shows a
**strip of chord chips** just above it: the diatonic chords of that degree
in the scale, simplest first — in C major, a note on A offers
`Am  F  Dm7  Am7  Fmaj7 …`. Each chip shows its name and its Roman numeral
in small type (`vi`), so beginners see names and theory people see function.
**Hovering a chip shows the chord as ghost notes** in the roll (the
onion-skin style already exists) *and plays it*; clicking writes it. One
undo removes it. Moving the pointer away or pressing Esc leaves the single
note.

The interesting choice: *is the placed note the chord's root, or any tone of
the chord?* Offering chords **that contain** the note (it could be the
root, third or fifth) gives far more musical options and is how people
actually harmonise a melody. **Decision** — I recommend: the first chips
are the chords where it is the root; a small "contains" toggle widens to
all chords containing it.

**Layer 2 — shaping (one click deeper).** On the strip, small controls that
change *every* chip at once: **voicing** (close, open, drop-2, spread),
**inversion** (root / 1st / 2nd — or "nearest to the previous chord", which
is voice leading done for you), **extensions** (triad / 7 / 9 / sus /
add), and **length** (one cell, or to the next chord). These are what
make an expert feel the tool keeps up.

**Layer 3 — type it.** A text field on the strip: type `Cmaj9/E` or `ii7`
and it is written. Experts will mostly use this; it costs little because a
chord-name parser is a small, testable piece.

**Off-key on purpose.** A **Scale lock** chip with its own keybind
suspends the scale without clearing it — the rows stay shaded so you still
see the key, but snapping turns off. Today holding **Alt** already does
this momentarily (`PianoRoll::scaled_key`); the chip is the latched
version. With the lock off, the strip offers *borrowed* chords too
(secondary dominants, the iv in a major key, Neapolitan…) marked with a
small "out of key" dot, so going out of key is a choice you can see.

**Without a scale set,** the strip still works — it just offers common
chord qualities on the note as a root (maj, min, 7, maj7, m7, dim, sus).

### 3.3 Engineering, briefly

- **The chord math goes in `fontelle-types`** next to `scale.rs`: stacking
  thirds on a scale's steps for diatonic chords, a quality table, voicing
  and inversion as pure functions, and a name parser. The analysis crate
  already has a `Chord`/`Quality` type for Analyze Musically, but the UI
  can't depend on that crate; the clean move is to bring that type down into
  `fontelle-types` and have analysis use it, so there is one chord model.
  (Name clash to avoid: the keymap already has a `Chord` meaning a key
  combination.)
- **Writing** uses the existing multi-note insert (`RollEdit::Insert`) —
  one undo.
- **New work in the UI:** the roll gets no plain-hover events today (only
  presses and drags); the strip needs a hover path. And the audition voice
  plays **one** note at a time; previewing a chord needs a small polyphonic
  version that still guarantees every note-on gets its note-off.
- **Keybinds** go through the keymap (two new actions: Smart chords, Scale
  lock), never key literals.

### 3.4 Honest caveats

- Diatonic chords for non-heptatonic scales (pentatonic, blues, the exotic
  ones in the catalogue) are not well defined by stacking thirds. Proposal:
  for scales without 7 notes, fall back to "chords whose notes are all in
  the scale" — still useful, just not numbered.
- Drum kits get no strip (they have no scale today either).
- The strip will cover a few rows above the note; it needs a rule for where
  it goes near the top of the view.

---

## 4. Small features (phase 4)

| Item | Finding | Plan |
|---|---|---|
| **Multi-select lanes** (shift-click) | Selection is a single lane everywhere (`selected_lane: Option`); shift-click does a plain click. Delete is one lane per command. | A lane selection set; shift-click ranges, ctrl-click toggles; delete/mute/solo/move act on all, as one undo (a `Compound` of `RemoveLane`s that always leaves one lane). New clips still land on the *last-clicked* lane. |
| **Analyze Musically in the audio clip window** | No button control kind on that panel today. | A button in the window's header beside the clip's name (cleaner than a fake row), calling the existing `analyze_musically`. |
| **Notes sound when placed/moved** (preference) | Placing and dragging are silent **on purpose** — an earlier report of yours asked for exactly that (quoted at `piano_roll.rs:2070`). | A setting, **off by default** so nobody's current habit changes: on = a note sounds when placed and each time a drag crosses to a new key (never on every pixel). |
| **Delete particles** | No particle system exists; the cable physics shows the pattern (pure, stepped by time). | A tiny pure `Particles` module: a removed note bursts into a few fading flecks in its colour, under 0.4 s. Respects Settings → Effects (Still/Off disables it). **Caveat:** a 200-note delete must not be 200 bursts — cap it, and burst from the selection's outline instead. |
| **Monitor button** | Input monitoring already exists and is **always on** whenever a track has an input. | A Monitor toggle by the track's "In:" row (on by default, so nothing changes for existing songs); off = still records, not heard. |
| **"Still wired after I disconnect"** | Dragging a wire off and letting go in empty space **does nothing** — it springs back (`cable_drop_target` → None). Also: a track with its output off still feeds its **sends**. | Letting go of a wire over nothing unplugs it (output off / send removed) with a little slack-cable animation, and the strip shows "sends still on" when output is off. |
| **Preset preview** ("stuck sounding the same" / needs a clip) | Stuck: a Presets-tab click applies the preset to the channel but leaves the window *previewing* the last soundfont/flop row, so the old sound keeps playing (`install_bank_preset` never clears `previewing`). The needs-a-clip half I could not confirm by reading — the template note's velocity in a clip-less project is the first suspect. | Fix the stuck case in code; reproduce the other on the real binary before changing anything. |

---

## 5. Theme editor (phase 5) — recommend splitting

What exists in-app: choosing a theme, rounding, see-through, picture
per section. No colour editing, no shader or gradient editing, no colour
picker widget. Themes are JSON files with ~41 colour tokens, metrics,
fonts and a layer stack per section.

**Floptle's shader graph, honestly assessed:** the graph model, IR and
WGSL transpiler (`floptle-shader`, ~7.9k lines) depend only on `naga` — the
same version Fontelle uses — so they port. **The editor (~4.4k lines) is
egui and does not**: Fontelle draws with its own canvas on vello, so a node
editor would be rewritten from scratch. It also has no "backdrop" stage yet.
That is weeks, not days, and it is the riskiest single item on the list.

**Proposal:**

- **5a (this round):** an Appearance editor — a real colour picker (wheel
  or square + hue strip + hex field + eyedropper from the screen), every
  palette token grouped by what it colours with a live preview, and each
  section's layer stack editable: add/remove/reorder layers, pick a
  built-in shader and set its colours and parameters with knobs. That
  covers what most people mean by "make my own theme" with no text editor.
- **5b (a later round):** the shader graph, built on Floptle's IR and
  transpiler with a Fontelle-native node editor. **Decision:** is 5b worth
  it, or would "edit the WGSL in a built-in code panel with live preview"
  (a far smaller job, using the Notepad's editor) serve your users? I'd ask
  a few theme makers.

---

## 6. Discord status

Nothing exists; no crate needed. Discord's local protocol is tiny (an
8-byte header + JSON over a named pipe on Windows / a Unix socket
elsewhere, with known extra paths for Flatpak and Snap Discord). ~200 lines
on a worker thread, reconnecting quietly; everything needed is already a
dependency.

- **Off by default**, a switch in Settings (privacy: a song title is
  something people may not want broadcast). Shows "Making music in
  Fontelle", optionally the song name (a second switch), elapsed time.
- **Needs a Discord application ID** from the Discord developer portal,
  registered by Fopull — **that is Ty's to create** (it carries the name and
  icon people see).

---

## 6b. Added 2026-10-18 — time signature changes, and the wheel over a ruler

### Time signature changes

> *"right clicking the time signature thing up top and a context menu says
> change time signature point ... it lets you place a point in your
> arrangement where the time signature changes and clicking on that
> highlights the time signature button to let you change it ... the project
> time signature i guess would be considered the default."*

**Where the code stands.** A song has one signature, and only its top
number: `Project::beats_per_bar: u32`; the bottom number is always 4 (the
comment at `project.rs:296` names a "time-signature track" as the known
gap). About 150 places read that one number — the grid, bar numbers, snap,
the ruler, the metronome, the arrangement, the roll, DisgustingBeat, the
transport hosted plugins are told, MIDI export.

**Design.**
- A **meter map** beside the tempo map: the song's signature is the first
  entry (the "default" Ty describes), and each change is `(bar, top,
  bottom)`. Changes sit **on bar lines** — that is how notation and every
  DAW do it, and it is what keeps bar numbers meaningful.
- **Bottom numbers other than 4 are possible** (6/8, 7/8, 5/16): a beat is
  then a 1/8 or 1/16 note, `PPQN × 4 / bottom` ticks, which is whole for
  the denominators people use (2, 4, 8, 16). The old comment's worry was
  about redefining the tick; this does not.
- **One API replaces the number**: `bar_at(tick)`, `tick_of_bar(bar)`,
  `beats_in_bar(bar)`, and an iterator of bar/beat lines over a range. Every
  caller asks *at a tick* instead of reading one global. This is the bulk
  of the work: mechanical, but wide.
- **The gesture, as Ty described it:** right-click the signature box →
  *Add signature change here* (at the marker's bar). A change shows as a
  small flag on the arrangement's ruler; clicking a flag selects it and the
  signature box then edits *that change* (it lights to say so); clicking
  empty ruler goes back to the song's own signature. Drag a flag to another
  bar; right-click it to delete.
- New commands (`SetMeterChange`, `RemoveMeterChange`) with wire forms;
  songs without changes save exactly as today.

**Caveats.** Hosted plugins are told "bar start" and the signature; a
change mid-song has to reach them per block. MIDI export gains time
signature events (easy). Old songs open unchanged.

### The wheel over a ruler zooms

> *"if i scroll my mousewheel while my mouse cursor is hovering over the time
> bars in the arrangement or piano roll or very top, it should squish and
> stretch"* — FL Studio's behaviour.

Over the arrangement's ruler → zoom the arrangement in time about the
pointer; over the roll's ruler → zoom the roll; over the transport bar's
song strip at the very top → zoom the arrangement about that point of the
song. The zoom functions already exist (`timeline_zoom_x`, `zoom_x`, both
anchored to the pointer); this is three hit-tests in the wheel handler.
Zooming is navigation, so it stays within the "the wheel never edits" rule.

## 7. Decisions — Ty's answers (2026-10-18)

Ty agreed with the recommendations above. His calls:

1. **Opening another song while hosting asks first** ("End the session for
   everyone?").
2. **Solo is personal, mute is shared.** *"muting should be shared since
   thats like an arrangement tool kinda but soloing should just be a local
   thing not a synced state."* Solo leaves the document (and the wire) and
   becomes window/session state per studio; a song's saved solo is read on
   open for compatibility and then held locally.
3. **Follow mode this round.** *"one person could teach some one how to do
   something and they can show them."* Built on presence (§2.2): following
   someone adopts their view (tab, clip, scroll, zoom) as it moves, their
   cursor drawn large; any input of your own, or Esc, stops following.
4. **Permissions:** the ones in §2.4, plus whatever else is recommended
   when it is built (proposed then, not invented silently).
5. **Rooted / containing** is a quick toggle on the chord strip itself (and
   a key while the strip is open), so switching is one press.
6. **Keybinds:** to be chosen with Ty — see the reply of 2026-10-18; free
   single keys in the studio are A H I J K Q U V W X Y Z.
7. **Theme shaders: a built-in WGSL code editor with live preview** this
   round, designed so a shader-graph mode can be added beside it later.
8. **Discord:** Ty creates the Discord application later, with a
   step-by-step walkthrough from D when he has time.

## 8. What I'd like you to review in code as we go

Rather than every diff, the few places where the design lives:

- `needs_rebuild(&Edit)` (§1.2) — the rule deciding what a remote edit
  costs; the whole fix hangs on it being right per edit kind.
- The `Presence` message and its coordinate conversion (§2.2) — one pure
  function per view, the heart of "everyone sees the same point".
- The chord model in `fontelle-types` (§3.3) — the public types everything
  else builds on; worth getting the names right before the UI is built.
- The session-end state machine (§1.1/§2.1) — the one part where a mistake
  loses someone's work.
