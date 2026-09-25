# Working on a song together: the collaboration plan

**Status, 2026-09-25: planned, nothing built.** This is the design for
working on one project with somebody else over the internet, at the same
time, written to be followed the way `docs/tune-plan.md` and
`docs/disgusting-beat-plan.md` were followed: tests first and confirmed
failing, one crate at a time, in the order §13 gives. It was written after a
read of the relay and the cloud that Floptle already runs (the hub repo's
`contracts/` and its task threads `0005`, `0054`, `0188`, `0191`, `0217`,
`0235`, `0253`; the engine's `crates/floptle-net` and `crates/floptle-relay`)
and of every place in this tree a sync layer would have to touch. Where a
number below is quoted, it is quoted from those, with the file named.

**The brief** (Ty, 2026-09-25): *"a way for people to be able to over the
internet work on a song together at the same time ... i have a cloud hosting
service for my game engine that maybe we could route stuff through the relay
(oracle gives us free relay) to do it so that nobody has to port forward and
its just easy ... every project made gets its own unique identifier that will
like never be repeated. then when in a project you can share it and open it
to the internet, that will give you a code as the relay does ... your friend
would just join from there using that and that would see that identifier and
how it doesnt match any projects you had, so it would copy the friend's
project to your shared projects matching that unique identifier so if you
ever want to make your own changes on your friends song for example you could
then share it back to them and then they could join that to get it to update
to your version of the files ... we need confirmation prompts to make sure
people dont accidentally lose files that would be very bad."*

**§18 is the ledger**: every problem the two surveys found, numbered, with
the section that answers it, the phase that closes it and the test that
proves it. A phase is done when its rows are; nothing found while planning
is allowed to live only in a chat log.

Read that as five requirements, each of which is a section here:

1. **Nobody port-forwards** (§3, §8). The Floptle relay already solves this
   for games, and it turns out to forward opaque bytes in a star around the
   host, which is exactly the shape a shared song wants.
2. **Every project has an identity that is never repeated** (§4). A UUIDv7
   in the bundle, minted at creation, carried by every copy a join makes.
3. **Join copies, join updates** (§4). A code that names a project you do not
   have copies it into your *Shared* projects; one you already have updates
   your copy — after a prompt, and never by deleting anything.
4. **At the same time** (§5). The host's document is the one truth; every
   edit anyone makes is a command already, and a command replayed on another
   machine is exactly what redo does on this one.
5. **Nobody loses a file** (§4.4). Three rules, each of which is a test.

---

## 0. Ground rules for this work

- **Tests first, confirmed failing, then the implementation.** The project's
  standing rule (`PROGRESS.md` top, `docs/handoff.md`). Write the test
  against the API in this document, see it fail, then build. Never both in
  one edit.
- **The invariants are hard**, and three of them are the reason this plan is
  shaped as it is. INVARIANT 2: the window never mutates the document — so a
  remote edit arrives in the session, not in the window. INVARIANT 8:
  persistent ids, never indices — the places that still use indices (§5.2)
  are the places a remote edit can land on the wrong thing. INVARIANT 9:
  every mutation goes through a `Command` — the mutations that do not (§5.2)
  are the mutations the other side never sees. INVARIANT 10: the join copy
  lands in the projects folder the user configured, nowhere else.
- **Loop on what you touched**: `cargo test -p fontelle-model --test wire`,
  `-p fontelle-app --test collab`, `-p fontelle-net --test relay`. The
  workspace suite runs once, in the background, at the end. Never two cargo
  runs at once.
- **Look at the window**: two studios on `Xwayland :99`, one hosting and one
  joining, are the test that can see. `docs/handoff.md` and the memory's
  `seeing-fontelles-gui` say how; §12.5 says what to look at.
- **No attribution trailers** on any commit (PROTOCOL §3a; the hook refuses
  them).
- **Anything public is Ty's** (PROTOCOL §5): the Cloud key for Fontelle, the
  release that carries this, the product page's wording. Prepare, stop at
  needs-review.
- **`cargo clippy --workspace --all-targets -- -D warnings` stays clean**,
  on the Windows target too — this feature will be the first thing in the
  tree that opens a socket, and the Windows build runs under Wine on this
  machine (`docs/handoff.md`, 2026-09-25 (a)).

---

## 1. What "done" means

Every one of these is a test in §12 or a scene in §12.5, and the feature is
not done until all of them hold:

1. Two studios on two machines, each behind a home router, edit one song at
   the same time through `relay.fopull.com`, and neither person configured
   anything but a six-letter code.
2. A joiner who has never seen the song ends up with a complete, playable
   copy under *Shared* in their projects folder, with every sample the song
   uses, and it opens on its own after the session ends.
3. A joiner who already has the song is asked before their copy is replaced,
   is told which copy is newer, can choose *keep both*, and finds a backup of
   what they had afterwards. No file is ever deleted by a join.
4. An edit by either person is on the other's screen within a second, on a
   round trip through Ashburn, and both documents are byte-identical after
   every edit (checked by a hash on the wire, §5.6).
5. Each person's undo undoes their own edits and nobody else's.
6. A sample recorded or dropped by either person plays on the other's machine
   once it has arrived, and is drawn as *fetching* until then — never silent
   with no explanation.
7. Two different Fontelle versions refuse to share, in a sentence that says
   who should update.
8. Closing the host's window ends the session cleanly on every joiner, and a
   joiner can carry on editing their own copy alone.
9. Nothing in the studio behaves differently when no session is open. No
   socket opens until *Share* or *Join* is pressed; the product page's line
   about network use stays true with one clause added (§15).

---

## 2. The shape, in one paragraph

The person who presses **Share** is the host. Their studio registers a lobby
on the Floptle relay with Fontelle's own Cloud key and gets a six-letter
code. A friend presses **Join** on the start menu and types the code. The
relay forwards bytes between them and understands none of it. The host's
document is the one truth: every edit anybody makes is a `Command` already,
so the host's `History` becomes a numbered stream of commands, the joiner
applies each one the way redo applies a command (same ids, `Arena::insert_at`),
and a joiner's own edits are applied locally at once, sent to the host as a
proposal, and rebased if the host ordered something else first. Samples
travel by content hash, only when missing, chunked under the relay's frame
cap. The project itself lives on the machines, never in the cloud: a join
copies the bundle into the joiner's *Shared* folder under the project's id,
and a later join of the same id offers to update that copy, with a backup
and a prompt. When the host leaves, everyone keeps a full copy.

---

## 3. The options, and why this shape

Three decisions were open. Each is laid out with what was considered, so the
choice can be overruled with the reasons in view.

### 3.1 Transport: how two studios reach each other

| Option | For | Against |
|---|---|---|
| **A. The Floptle relay** (`relay.fopull.com:7788`, QUIC, Oracle Ashburn) | Exists, runs at $0 on an Always-Free VM, nobody port-forwards, encrypted to the relay (TLS 1.3, `*.fopull.com` verified against webpki roots), opaque byte forwarding in a star round the host — "a session over a relay is the same bytes as a direct one" (`floptle-net/src/relay.rs:18`). Six-letter codes people already know from Floptle. Joiners need no account and no key. The client is three files E wrote to be lifted (§9.1). | Brings quinn, rustls, tokio and postcard into a tree that has deliberately had no TLS crate (the updater shells out to `curl` for that reason, `updates.rs:19-26`). Per-connection budget of 512 KiB/s and 4000 messages/s, and **the host pays once per joiner** (0235: "the relay does not multiply — the HOST does"). Reliable frames capped at 128 KB host→peer and 64 KB peer→host. The host must be online for anyone to work. Hosting on the managed relay needs a Cloud game key (§9.3) — Ty's account, Ty's action. |
| B. A new collaboration service on Floptle Cloud (W builds a WebSocket server that holds the document) | Works when the host is offline; a server can keep the op log and merge later; joiners could pull the project any time. | Does not exist; W would build and run it; storage costs and a database of other people's songs; still needs a TLS client in the tree; accounts become necessary; the product page's "no account, no cloud" becomes untrue. A much bigger commitment than the ask. |
| C. Direct peer-to-peer with hole punching (STUN/ICE), relay as fallback | No relay bandwidth in the common case. | The relay is still needed for the fallback and for rendezvous, so it is A plus a second transport and a NAT-traversal library; symmetric NATs fail; weeks of work for bandwidth the relay already has. |
| D. A third-party peer-to-peer library (iroh: QUIC, hole punching, public relays) | Direct connections when possible, relay otherwise, all in one crate. | Somebody else's relays unless self-hosted; a large dependency tree that moves fast; Ty already runs a relay he controls. |
| E. Manual port-forwarding / a direct QUIC address | Simplest code. | Exactly what the brief rules out. |

**Recommendation: A.** It is the only option that exists today, costs
nothing, needs no account from a joiner, and its protocol was designed to
carry bytes it does not understand. The dependency cost is real and it is
paid once: quinn + rustls + tokio is roughly forty crates and a few MB of
binary; the updater's "thirty crates for two requests" argument was about
proportion, and a feature that keeps a socket open for an hour is
proportionate. The budgets are the design constraint to respect (§8.4), not
a reason to pick something else. B is the natural *later* — a place a shared
song can live when its host is asleep — and nothing here forecloses it: the
op stream and the content-hashed assets are what such a service would store.

### 3.2 Consistency: who decides the order of edits

| Option | For | Against |
|---|---|---|
| **A. Host-authoritative stream, joiners predict and rebase** | One truth, no merge algorithm; the pieces exist (`Command::invert`, `Arena::insert_at`, `History::break_gesture`); this is the Floptle netcode's own model (server-authoritative, client prediction). A joiner's own drag is instant. | A joiner's edit can be refused after the fact (someone deleted the clip first); the rebase must invert and re-apply pending edits. |
| B. Host-authoritative, joiners wait for the echo | Simplest possible client. | 100–200 ms on every pointer motion of every drag. Not usable. |
| C. CRDT over the document | No host, works offline and merges. | A DAW document is arenas of typed records with cross-references and invariants (a clip's lane must exist, a note's channel must exist, a place's prefab must exist); a CRDT that keeps those is a research project, and the document would be rebuilt around it. Wrong size for the ask. |
| D. Op log with a merge on rejoin (git-like) | Offline edits by two people could be reconciled. | The merge is the hard part and the brief does not ask for it: the brief's flow is *copy, then update*, one direction at a time. Kept as a later possibility the op stream makes possible (§14). |

**Recommendation: A.** It is what the document already nearly is: an edit is
a value with an inverse, and redo already re-applies a stored command with
the ids it minted the first time. A remote edit is a redo from somebody
else's history.

### 3.3 Where the project lives

| Option | For | Against |
|---|---|---|
| **A. On the machines: the host's bundle is the truth, a join copies it** | No storage, no accounts, no service; the brief's flow exactly; INVARIANT 10 stays simple (the copy goes in the configured projects folder, under *Shared*). | The host must be online for the song to be reachable; two people who both edited offline cannot merge (one side is replaced, with a backup and a prompt). |
| B. In the cloud (object storage keyed by project id) | Reachable when the host is offline; a natural home for the op log. | Does not exist: Cloud saves are 256 KB JSON slots (0054), build storage accepts only server bundles (`contracts/cloud-hosting.md` §4). A new W service, costs, accounts. |

**Recommendation: A**, with the bundle made self-contained first (§7.1),
because a copy is only a copy if it carries its samples.

---

## 4. The identity of a project, and the join

### 4.1 The id

`ProjectMeta` gains two fields, and `PROJECT_FORMAT_VERSION` goes from 0 to 1
with the first real entry in `storage.rs`'s `migrate()`:

```rust
pub struct ProjectMeta {
    pub name: String,
    pub created: String,
    pub app_version: String,
    pub format_version: u32,
    /// Never repeated. Minted when a project is created; carried by every
    /// copy a join makes; a Save As mints a new one and records where it
    /// came from (§15, decision 2).
    pub id: PersistentId,
    pub forked_from: Option<PersistentId>,
    /// Bumped on every save. What the join prompt compares (§4.3).
    pub saved_revision: u64,
    pub saved_at: String,
    pub saved_by: String,
}
```

`PersistentId` is `fontelle-types/src/id.rs:36-51`, a UUIDv7, already in the
tree and used by nothing in `Project` yet. **The migration for a format-0
project derives the id from `created` and `name`** (a UUIDv5-style hash of
the two, not `now_v7()`): two zipped copies of an old project on two
machines then agree about who they are, which `now_v7()` on load would
break. From format 1 on, ids are minted, never derived.

`saved_by` is the name the person gave in the settings page's new *Your
name* row (§10.4), or the OS user name until they do.

### 4.2 Where a copy goes, and how it is found again

A join copy lands at `<projects_dir>/Shared/<name>.fontelle`, `<name> (2)`
if taken; **the folder name is a label, the id is the identity**.
`projects.rs` gains `find_by_id(projects_dir, id) -> Vec<PathBuf>`: a walk of
the projects folder (top level and `Shared/`) that peeks each bundle's
`project.json` for `meta.id` without reading the body (`storage::peek_meta`,
which reads the same leading bytes `load_project` already reads for the
version). Two hundred bundles is two hundred small reads at join time;
cache nothing until it is slow. The projects browser shows *Shared* as a
section under the person's own projects.

### 4.3 The join, step by step

1. Joiner presses **Join** (start menu, §10.2), types the code. A studio
   with an open, unsaved project first goes through the existing
   `Leave`/`SaveAnswer` prompt — the same one quitting uses.
2. `Hello` / `Welcome` (§8.2). The host refuses a version mismatch here.
3. `Welcome` carries the project's id, name, `saved_revision`, `saved_at`,
   `saved_by`, and the asset manifest. The joiner looks the id up (§4.2).
4. **No match** → one prompt: *"Copy 'Song' from Alice into your Shared
   projects? 38 MB, 14 samples."* Confirm → snapshot and assets stream in,
   the bundle is written, the studio opens it live. Cancel → nothing was
   written.
5. **One match** → the prompt below. **Two or more** (a copy and its
   *keep both* twin from an earlier day, before that twin was forked) → the
   list, pick one, then the prompt.
6. **The prompt when you already have it**, three buttons, the safe one
   first:

   > You already have **Song**.
   > Yours: saved 2 days ago by you (revision 41).
   > Alice's: saved 10 minutes ago by Alice (revision 58).
   >
   > **[Update mine to Alice's]** a backup of yours is kept
   > **[Keep both]** yours becomes its own project
   > **[Cancel]**

   *Update* backs up (§4.4), rewrites `project.json` from the snapshot, adds
   any asset it lacks, opens live. *Keep both* mints a new id for the local
   copy (`forked_from` = the shared id) and then proceeds as *no match*. The
   sentence about which is newer is written from `saved_at` and
   `saved_revision`; when the revisions have diverged (yours is 41, theirs
   is 58, but yours has edits since the common ancestor — see §4.5) it says
   *"both have changed since you last shared"* and the default button is
   *Keep both*.
7. The host sees *"Bob joined"* on the status line and in the Share panel.

The host's side of a share is simpler: **Share** needs a saved bundle with
its assets collected (§7.1), so pressing it on an unsaved project runs the
save prompt first and the collect step second, and only then registers the
lobby.

### 4.4 The three rules that keep a file safe

Each is a test in §12.2, and the code is shaped so breaking one is hard:

1. **A join never deletes.** The sync adds files to `assets/` and rewrites
   `project.json`; it removes nothing, ever. A sample the new version does
   not reference stays on disk. (A *tidy unused files* action is a separate,
   later, explicit thing — §14.)
2. **A replace is a backup first.** Before `project.json` is rewritten,
   the old one is copied to `backups/before-join-<date>_<time>/project.json`
   in the same bundle — `backups/` is where `autosave.fontelle` already
   lives (`session.rs:2623-2645`). The prompt names the backup. Because of
   rule 1 the backup needs only the manifest: every file it references is
   still there.
3. **Unsaved work is asked about before anything else.** The existing
   `Leave` flow, reused, not reimplemented.

Two more, weaker, still worth writing down: the joiner's copy is written
with `save_project`'s temp-fsync-rename, so a dropped connection mid-copy
leaves either the old bundle or the new, never half; and a copy that did not
complete (assets still streaming when the host left) opens with those clips
in the *missing* state the bundle loader already has (`bundle.rs:16-43`),
not silently empty.

### 4.5 Knowing whether both sides changed

`saved_revision` alone cannot tell *diverged* from *behind*. Add one thing:
each bundle keeps `shared_revision: Option<(PersistentId /*peer*/, u64)>`
in `ProjectMeta` — the `saved_revision` the two copies last agreed on, written
by both sides when a session ends. At join time: yours == shared → *you are
behind, Update is safe*; theirs == shared → *they are behind, updating would
lose their nothing but your edits — say so*; neither → *diverged*. Small, and
it is the whole difference between a prompt that informs and one that
guesses.

---

## 5. The document as a stream of edits

### 5.1 What exists, and what is missing

What exists (`fontelle-model/src/command.rs`, `commands.rs`,
`arena.rs`):

- Every edit is a `Command` with `apply`, `invert`, `merge_with`,
  `as_any`. 102 implementations, ~92 public, the rest `Restore*` inverses.
- Ids are `Arena` keys; a command mints them on first `apply` and **redo
  re-applies the same command, which puts the same ids back with
  `Arena::insert_at`** (`arena.rs:100-127`). So a command that has been
  applied once is a complete description of its effect, ids included.
- `History::break_gesture` marks where a drag ends; `merge_with` folds a
  drag's four hundred commands into one entry.

What is missing:

- **Commands do not serialise.** `Box<dyn Command>`, private fields, no
  serde. This is the one structural change (§5.3).
- **Not every mutation is a command** (§5.2).
- **Some targets are positions** (§5.2).
- **Two peers minting ids would collide** — solved by never letting them:
  only the host mints in a session (§5.4), and a joiner's optimistic mint is
  thrown away and replaced by the host's (§5.5).

### 5.2 Mutations to fix before any of this

Found by the survey; each becomes a command or is declared *local* and
excluded from the wire. Do this first (Phase 0), as its own commit, because
every one is an INVARIANT 9 debt on its own.

Become commands:

- `automation_lane` inserts a lane directly (`session.rs:1941-1960`; its own
  comment admits lane creation is off the undo stack).
- Row render renames a lane via `lanes.get_mut` (`session.rs:2464`).
- `meta.name` written directly at `session.rs:991`, `:1721`, `:1773` and in
  `adopt` → `RenameProject`.

Declared local (applied on every peer independently, deterministic given
the same document and the same sample lengths, never sent):

- `heal_audio_clips` (`session.rs:2774-2787`), which rewrites every audio
  clip's trim on every `republish` from the library's knowledge of the
  file. On a joiner the file is the same bytes, so the result is the same.
  The wire hash (§5.6) is what proves it; if it ever disagrees, this is the
  first suspect.
- `capture_plugin_states` (`session.rs:3095-3120`) writes plugin blobs at
  save. Blobs are the plugin's, per machine; a joiner without the plugin
  keeps the blob it received and never overwrites it (§7.4).
- `view_state` — zoom and scroll live inside the document today. Excluded
  from the snapshot hash and never carried by an edit; verify no command
  touches it (a test in §12.1).

**Positions that must become ids.** `MixerTrack.inserts` and `sends` are
addressed by `slot: usize` (`PluginTarget::Insert{track, slot}`,
`PresetTarget::Insert{track, index}`, `ParamTarget::Insert{track, slot,
param}`), and `markers: Vec<Marker>` has no ids. Under the host's ordering a
position is unambiguous at the moment the host applies it; it is a joiner's
*optimistic* apply that can land on the wrong slot (their slot 2 is the
host's slot 3 if an insert arrived first), and while the rebase in §5.5
would eventually correct the document, the person would see their EQ land
on the wrong insert for a round trip. So this is fixed in Phase 0, not
carried:

- `EffectSlot` and `Send` gain `id: PersistentId` (`#[serde(default)]`
  minting on load, so format 1's migration gives every existing slot one).
  Every command that names a slot carries `SlotRef { index: usize, id:
  PersistentId }` and **refuses** at apply when the slot at `index` is not
  `id` (INVARIANT 9's "refuse rather than clamp"). `ParamAddress` strings
  (`insert:{track}/{slot}/{param}`) are **not** changed: they are stable per
  document and identical on every peer, and rewriting them would touch every
  automation clip ever saved.
- `markers: Vec<Marker>` becomes `Arena<MarkerId, Marker>` — `MarkerId`
  already exists in `fontelle-types/src/id.rs`, unused. The marker commands
  address by id.
- The `StudioHost` methods that take a row `index: usize` (about 300, e.g.
  `toggle_mute(index)`) stay: they are the window's vocabulary and the
  session resolves an index to an id *before* building the command, on the
  machine whose rows they are.

**Things the survey found that must be settled by a test, not a belief:**

- `prefab.rs:123` says a `NoteId` "is minted fresh every session"; the
  `Arena` serde keeps `(idx, version)` keys (`arena.rs:225-262`). The test
  `note_ids_survive_save_and_load` decides, and the comment is corrected
  either way. (If keys really were reminted on load, no join could ever
  agree about a note.)
- `project.assets: AssetTable` is written and read by nothing. Phase 2's
  `collect_assets` makes it the bundle's asset table (id, relative path,
  hash, size, kind) and the manifest in `Welcome` is read from it — or, if
  that proves the wrong shape, it is removed, with the format bump that is
  already happening. Not left as a field nothing uses.
- `Session.dirty` is set in ~50 places by local edits; a foreign edit must
  set it too, or a joiner's copy never asks to be saved (`apply_foreign`
  sets it).
- `Session.revision` counts view changes as well as edits and is right for
  what it does; the wire has its own `seq` and never reads it.

### 5.3 The wire form of a command

A new module `fontelle_model::wire` with one enum:

```rust
/// One edit, as it crosses a wire. Append-only: variants are numbered by
/// declaration order (postcard), a new one goes at the end, and the test
/// `wire_variant_order_is_pinned` holds the numbers.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Edit {
    AddNotes(AddNotes),
    MoveNotes(MoveNotes),
    /* ... one per Command, including every Restore* inverse ... */
    Compound { label: String, parts: Vec<Edit> },
}

impl Edit {
    pub fn into_command(self) -> Box<dyn Command>;
}

pub trait Command: Send {
    /* existing */
    /// This command as a value, ids and all, so another machine can apply
    /// it the way redo does. `None` is refused by the wire tests: every
    /// command has one.
    fn to_edit(&self) -> Edit;
}
```

Mechanics: derive `Serialize, Deserialize` on every command struct and let a
macro write the 102 `Edit` arms and the `to_edit`/`into_command` pairs. A
command's serialised form **after apply** carries its minted ids, so
`Edit → Command → apply` on another machine takes the redo path
(`insert_at`) and produces the same document. `Compound` nests. Commands that
hold a `Box<dyn Command>` inside (the compound, any others the survey
missed) hold an `Edit` on the wire.

**Why not `typetag`?** It would make `Box<dyn Command>` serialise with less
code, but the wire needs the append-only numbering discipline the relay's
own `RelayMsg` uses, and an explicit enum is the thing a test can pin.

### 5.4 The choke point: every applied command is offered to the wire

`History` grows an outbox:

```rust
impl History {
    /// Every command that lands in the undo stack, in order, as it landed:
    /// merged gestures appear once, when they break. Drained by the session.
    pub fn take_outbox(&mut self) -> Vec<Edit>;
    /// Applies a command that came from another peer: it does not enter the
    /// undo stack (undo is yours, §5.7) and it does not go to the outbox.
    pub fn apply_foreign(&mut self, edit: Edit, doc: &mut Project) -> Result<(), CommandError>;
}
```

Putting the outbox in `History::apply` rather than in `Session::run` is the
point: `run` has 82 callers, `apply_for` 17, and there are five direct
`history.apply` calls — one hook under all of them. **Gesture granularity**:
the outbox is fed when a gesture breaks, with the merged command, so a drag
is one message when the button goes up, not four hundred. (Live ghosts of a
drag in progress are §14.)

The host applies its own edits as today and its outbox becomes the stream.
A joiner's outbox becomes proposals (§5.5). In both cases the session drains
it once per tick, next to `poll_job`.

**Republishing a foreign edit.** Which of `republish`, `rebuild_graph`,
`publish_mixer` an edit needs is decided at each of 47 call sites today, by
hand. For a foreign edit v1 does what undo does (`session.rs:5109-5135`):
`rebuild_graph()` then `republish()`. It is heavier than necessary and it is
what undo already costs. An `Edit::needs_graph() -> bool` classification is
the later optimisation, and the test that would justify it is a glitch
count, not an opinion.

### 5.5 The host orders, the joiner predicts

Per peer, in `fontelle-app/src/collab/`:

- **Host**: on `Propose{local_seq, edit}` from peer P, `apply` it (fresh
  command from the edit with ids stripped — the host mints); on success
  broadcast `Applied{seq, author: P, author_seq: local_seq, edit: applied
  form with ids}` to every peer including P; on failure send P
  `Refused{local_seq, reason}`. The host's own edits go out as `Applied`
  with `author: 0`.
- **Joiner**: apply locally at once through the normal `History` (so undo
  works and the screen is instant), push the edit onto `pending` with its
  `local_seq`, send `Propose`. On any inbound `Applied`:
  1. invert every pending edit, newest first (`Command::invert`, applied
     directly to the document, not through history);
  2. apply the inbound edit with `apply_foreign` — unless its `author` is me
     and `author_seq` is the head of `pending`, in which case pop it and
     apply the host's form of it (ids now the host's);
  3. re-apply the remaining pending edits in order; one that now fails is
     dropped with a toast (*"Your move of 3 notes was undone — Alice deleted
     the clip"*) and its undo entry removed.
  On `Refused`: the same, with the refused edit dropped.

A joiner's locally minted ids differ from the host's only if their arenas
differ, which they do only while `pending` is non-empty; step 2's replace
makes that window invisible. The invariant to test: **after every inbound
message, with an empty `pending`, the joiner's document hash equals the
host's** (§5.6).

A drag in progress is a command being merged into the history head that has
not reached the outbox yet. If a foreign edit arrives mid-drag, it is
inverted and re-applied like anything pending; the clamp the gesture
measured at its start (`MoveLimits`, the memory's `fontelle-self-contained-studio`)
may now be stale, and the re-apply may refuse — the toast above covers it.
Rare, and honest.

### 5.6 The hash that proves it

`Project::sync_hash(&self) -> u64`: xxhash64 (`twox-hash`, already in
`fontelle-assets`; move the dependency to `fontelle-model` or hash in the
app) over the postcard encoding of the project with `view_state` zeroed and
every `AssetRef.path` replaced by its content hash (paths differ per machine
by design, §7.1). Every `Applied` carries the host's hash after applying it.
A joiner that disagrees when `pending` is empty logs it, asks for a fresh
snapshot (`ResyncRequest`), and carries on; the session log line is the bug
report. Under `cfg(test)` a disagreement panics.

### 5.7 Undo is yours

A joiner's `History` holds only its own commands; foreign edits bypass the
stack. Undo inverts the head as today, and the inverse is an edit like any
other: it goes to the outbox, is proposed, and is ordered by the host. If
somebody has changed the thing since, the inverse refuses (INVARIANT 9's
"refuse rather than clamp") and the toast says so. Redo the same. This is
the per-user undo every collaborative editor settles on, and it costs
nothing new. The host is the same with `pending` always empty.

---

## 6. Who is in the room

Presence is small in v1 and worth having from the first session because it
is the only feedback a person has that the thing is working:

- `Hello` carries a display name (§10.4) and the host assigns each peer a
  colour from the theme's clip palette.
- The Share panel (§10.1) lists names; the status line says *"Bob joined"*
  / *"Bob left"*.
- Each `Applied` already names its author, so the toast for a foreign edit
  worth mentioning (a lane added, a clip deleted) can say who — v1 shows
  nothing per edit; the arrangement just changes. Cursors and selections are
  §14.

---

## 7. Samples, soundfonts and plugins

### 7.1 A shared bundle is self-contained

Today a bundle references imported audio by absolute path and copies nothing
(`bundle.rs:75-82`, `library.rs:186-225`); only recordings live inside it.
TDD §17.1 already plans *Export Bundle*, "collects referenced assets". Build
that, as the first step of **Share**:

- `Session::collect_assets()`: for every `AssetRef` (audio clips, sampler
  layers, soundfont files) not already under the bundle, copy the file to
  `assets/<sha256-hex>.<ext>` and rewrite the reference to a **bundle-relative
  path**. `sha2` is already a dependency of `fontelle-app`. `AssetRef.
  content_hash` (u64, stored as 0 today, "nothing reads it yet") becomes the
  low 64 bits of that digest, so the manifest and the wire have one name for
  a file.
- Relative paths need `AssetRef` resolution to know the bundle root: one
  function, `bundle.rs::resolve(bundle, &AssetRef) -> PathBuf`, and every
  reader goes through it (the same rule shape as `Project::clip_source`).
- On the joiner, received files land at the same relative path, so the two
  documents are identical apart from nothing.

Soundfonts are the big case: a `.sf2` in the bank can be hundreds of MB
(325 MB is cited in the tree). Collecting one into every bundle that uses it
is wasteful on the host's disk and slow on the wire (§8.4). **v1 rule:**
a soundfont is referenced by content hash and *looked for* before it is
transferred — first in the joiner's bundle, then in the joiner's bank
(`soundfonts/`, hashing each file once and remembering it in a small
`hashes.json` beside them) — and only fetched if absent, into the bank, not
the bundle, after a prompt that names the size (*"This song uses 'Salamander
Grand' (325 MB), which you don't have. Fetch it? About 10 minutes."*). The
same lookup serves ordinary samples: a file already in the bundle is never
sent twice.

### 7.2 Transfer by hash, pulled, chunked

Assets are pulled, never pushed: a peer that finds a hash it lacks sends
`AssetRequest{hash}`; the owner answers `AssetChunk{hash, index, of,
bytes}` in 48 KB pieces (under the 64 KB peer→host cap, so one chunk size
both ways) followed by `AssetDone{hash}`. Clients only speak to the host, so
a joiner's recording travels joiner → host on the edit that introduced it
(the host requests it at once) and host → other joiners on their request.
One request in flight per peer, in the order asked, so a 300 MB soundfont
does not starve a 2 MB drum hit: **the manifest is fetched in ascending
size order** and the person sees clips fill in smallest first.

### 7.3 A clip whose sample has not arrived

The bundle loader already opens a project with missing files and renders
those clips silent (`OpenedProject.missing`, `bundle.rs:16-43`). Extend the
state, not the mechanism: an `AssetRef` whose file is absent but whose hash
is in flight is drawn hatched with *"fetching 43 %"* in the clip's caption
and is silent until `AssetDone` swaps the buffer in through
`SampleLibrary::adopt_audio(id, path)` — the reload-under-a-stored-id path
`reload_audio` already takes (`library.rs:320-372`). The `AssetId` is the
host's (it is in the edit), so the joiner's library must accept a key it did
not mint: that is the one change to the library.

### 7.4 Plugins somebody does not have

A `PluginState` names its plugin by `format:id` and carries a blob. A joiner
without `clap:com.u-he.diva` keeps the slot, keeps the blob and the params,
and the slot is silent with a message — `plugins.rs:545-549` already does
exactly this for a plugin that vanished between sessions ("§17.4's rule for
a missing file, applied to a missing plugin"). Two things to add: the slot
is *drawn* as missing in the mixer (today only the message says so), and
**a blob is never written back from a machine that could not open the
plugin**: `capture_plugin_states` skips a slot whose plugin is not live, so
the owner's state survives a round trip. Test: `a_missing_plugins_state_
survives_a_round_trip_through_a_machine_without_it`. Built-in effects and
Flopsynth are always present; soundfonts are §7.1.

---

## 8. The wire

### 8.1 Layers

```
relay (QUIC, RelayMsg, opaque bytes)          — floptle-net, lifted (§9.1)
  └─ Fontelle session (postcard `Msg`, reliable channel only)
       ├─ handshake, snapshot, edits, presence
       └─ asset transfer
```

Everything goes on the relay's **reliable** channel (ordered stream). The
unreliable and sequenced channels exist for games and are not used;
`Presence` could use sequenced later.

### 8.2 Messages

One postcard enum, append-only, variant order pinned by a test, same
discipline as `RelayMsg` (`relay.rs:1793`):

```rust
pub enum Msg {
    // handshake
    Hello { protocol: u32, fontelle: String, name: String },
    Welcome { peer: u16, protocol: u32, fontelle: String, project: ProjectHead, manifest: Vec<AssetEntry> },
    Refuse { reason: String },
    // the document
    SnapshotChunk { index: u32, of: u32, bytes: Vec<u8> },   // postcard(Project), 48 KB pieces
    SnapshotDone { hash: u64, seq: u64 },                    // the edit seq this snapshot is at
    Propose { local_seq: u64, edit: Edit },                  // joiner → host
    Applied { seq: u64, author: u16, author_seq: u64, edit: Edit, hash: u64 },
    Refused { local_seq: u64, reason: String },
    ResyncRequest,
    // files
    AssetRequest { hash: u64 },
    AssetChunk { hash: u64, index: u32, of: u32, bytes: Vec<u8> },
    AssetDone { hash: u64 },
    AssetMissing { hash: u64 },
    // people
    Joined { peer: u16, name: String, colour: u8 },
    Left { peer: u16 },
    Bye,
}
pub struct ProjectHead { id: PersistentId, name: String, saved_revision: u64, saved_at: String, saved_by: String, shared_revision: Option<(PersistentId, u64)> }
pub struct AssetEntry { hash: u64, size: u64, file_name: String, kind: AssetKind }
```

The snapshot is postcard on the wire (several times smaller than the
pretty-printed JSON on disk) and is written to disk as `project.json`
through `save_project`, so the storage tests' guarantees hold for a copy.

### 8.3 Versions

`PROTOCOL: u32 = 1` in `Hello`/`Welcome`. **v1 additionally requires the
same Fontelle version string** (`CARGO_PKG_VERSION`): the document format,
the command set and the wire all move with a release, and an exact match is
the only rule that cannot be wrong. The refusal names both: *"Alice has
Fontelle 0.16.0 and you have 0.15.0 — the newer one should host, or update
from the start menu."* Relaxing this to a protocol number is a decision for
a later release, once a release has shipped without changing any of the
three.

### 8.4 Budgets, and what they mean for the design

From `floptle-net/src/relay.rs:263-319` and `docs/multiplayer.md:777-789`,
which apply on the managed relay too:

| Limit | Value | Consequence here |
|---|---|---|
| Reliable frame, host → peer | 128 KB | chunks of 48 KB everywhere |
| Reliable frame, peer → host | 64 KB | same chunk size both ways |
| Bytes per connection | 512 KiB/s | a 38 MB song copies in ~75 s to one joiner |
| Host fan-out | host pays N× | three joiners copying at once: ~4 min each |
| Messages per connection | 4000/s | irrelevant at gesture granularity |
| Over budget | 3 s in a row closes the connection | the sender must pace itself, not fire and hope |
| Clients per lobby | 64 | plenty |
| Host gone | lobby held 20 s, then ends | a host's brief drop reconnects (the lifted client already backs off 1–30 s) |
| Lobby idle | 30 min with no clients | a host alone that long re-registers on the next Share |
| CCU, free plan | 100, pooled across Ty's account | a session of two is two |

The pacing rule is the important one: `fontelle-net`'s sender meters
outbound bytes to 480 KiB/s per connection *below* the relay's budget, so
the relay never has to. Asset chunks fill what edits leave. A card to W/E
(§16) asks whether a per-key budget is a thing the control plane can grant;
until it is, the numbers above are the product.

### 8.5 Security, stated plainly

The code is the secret: anyone with it can edit the song for as long as the
host shares. Six characters from a 32-symbol alphabet with a region letter is
about a billion codes; the relay's join rate limit (30 a minute per address)
makes guessing impractical. The relay sees the bytes in the clear between
its two TLS legs — it is Ty's machine. End-to-end encryption under the relay
is §14; view-only guests are §14.

---

## 9. The transport crate: `fontelle-net`

### 9.1 Lifted from `floptle-net`

E's relay client is three files that depend only on each other:
`quic.rs` (1406 lines), `relay.rs` (3046), `transport.rs` (408);
`floptle_core` appears only in their tests. MIT OR Apache-2.0, same company.
**Copy them** into `crates/fontelle-net/src/`, keeping the postcard variant
order and the test that pins it, and note in the crate's `lib.rs` which
engine commit they came from. Because `relay.rs` carries `RelayServer` too,
the tests get an in-process open relay on `127.0.0.1:0` and never touch the
internet.

Dependencies (from `floptle-net/Cargo.toml`, crates.io only — `deny.toml`
bans git and unknown registries): `quinn 0.11` (runtime-tokio, rustls-ring),
`rustls 0.23`, `webpki-roots 0.26`, `rcgen 0.13` (tests), `ring 0.17`,
`tokio 1` (rt-multi-thread, sync, time, net), `socket2 0.6`, `postcard`,
`serde`, `bytes`. **`fontelle-net` depends on `fontelle-types` and nothing
else in the tree** (INVARIANT 4's spirit): the `Msg` enum and `Edit` live in
`fontelle-model::wire`, and the app joins them.

A hub card to E (§16) asks for the client to be published as a standalone
crate later, so the copy can become a dependency. Until then a relay
protocol change on E's side must be mirrored here — the pinned variant test
is what notices.

### 9.2 Threads and the window's sleep

quinn runs its tokio runtime on its own threads; the session talks to it
through channels, as the engine does. `Collab::poll()` drains inbound
messages once per tick, next to `poll_job` (`app.rs:19042`). The window's
loop is winit in `ControlFlow::Wait` / `WaitUntil` (`app.rs:977`, `:19209`)
and sleeps `Forever` when idle, `ENGINE_POLL` (100 ms) while the engine is
watched (`widget/*.rs:208`). Two steps, both in this plan:

- **Phase 3**: while a session is open the window wakes at `ENGINE_POLL`,
  the existing precedent — a remote edit is on screen within 100 ms plus
  the round trip, with no new event-loop code.
- **Phase 4**: the network thread holds an `EventLoopProxy` (winit's
  `EventLoop::create_proxy`, which the tree does not use yet) and sends a
  user event when a message lands, so a remote edit wakes the window at
  once and an idle session sleeps `Forever` again. `sleep_budget` learns
  nothing: the proxy is a wake, not a policy. If the proxy proves awkward
  on one platform, the poll stays as the fallback and the handoff says so.

**Reconnects.** The lifted client already reconnects a host to a lost
relay with 1–30 s backoff, and the relay holds the lobby for `HOST_GRACE
= 20 s`; the host uses `host_keyed_reclaiming(code)` so **the code does not
change** across a host's brief drop and nobody has to be told a new one. A
joiner that drops rejoins with the same code and takes a fresh snapshot
(§8.2's `ResyncRequest` path) — no op log is kept for catch-up, because a
snapshot at 480 KiB/s is seconds and a log is a second source of truth.
QUIC keep-alive is 500 ms and the idle timeout 8 s (`quic.rs:43-46`), so a
laptop lid closed for a minute is a rejoin, and the status line says so.

### 9.3 Hosting on the managed relay needs a key

`relay.fopull.com` has been managed since 2026-09-07. A keyless `Host` is
refused with *"This relay is Floptle Cloud. Connect your project to a game
at fopull.com/cloud, or self-host floptle-relay."* (`floptle-relay/src/
policy.rs:605-612`). Hosts send `HostKeyed{key, build}`; the key is public
by design (`fk_live_…`, it ships inside game builds); **joiners need
nothing**. So Fontelle is registered as a game under Ty's Cloud account
(**done 2026-09-25**, slug `fontelle`) and its key is compiled into
`fontelle-net` as `FONTELLE_CLOUD_KEY`, sent with `build:
Some(CARGO_PKG_VERSION)`:

```
fk_live_U3JJ95XPGFZ69XZJM73SARMMHF3GZEMC
```

It is public by design, like every game's, so it lives in the source and
in this document; revoking and reissuing it is a website action of Ty's. Codes come back as six characters with the region
letter first (`U` = us-east); joining resolves the letter through a
compiled-in table `{U: "us-east.relay.fopull.com:7788"}` rather than fetching
the regions API — one HTTP client the tree still does not need. A new
region is a release.

`Settings` gains `relay: Option<String>` for a self-hosted open relay
(`host:port`, keyless `Host`, five-letter codes) — the developer's and the
tests' path, and anyone's who wants to run their own.

---

## 10. The window

### 10.1 The Share panel

A small panel from the transport bar's right end, next to where the master
meter is: **Share this song**. Pressing it saves if needed, collects assets
(§7.1, with the progress bar `poll_job` already draws), registers the lobby,
and the panel shows the code in the studio's largest type with **Copy** and
**Stop sharing**, the list of who is here (§6), and a one-line status
(*"Bob is fetching 3 of 14 samples"*). Each person's row on the host's
panel has two small controls: **view only** (the host answers that peer's
every `Propose` with `Refused{"view only"}` and the joiner's panel says so
before they try — their own edits are still applied locally and rolled
back, so the switch is cheap and honest) and **remove** (a `Bye` to that
peer, then the transport's disconnect; the code is unchanged, so *remove*
is not *lock out* — §14.6 has the rest). While sharing, the project
caption (`canvas::project_caption`) carries a dot in the host's colour.

### 10.2 Join

On the start menu (`welcome.rs`, a new `WelcomeHit::Join`): **Join a shared
song**, which opens the existing name prompt (`NameFor::JoinCode`) for the
code, then the prompts of §4.3 through the existing `SaveAnswer`-shaped
question mechanism. From inside the studio the same lives in the Share panel
(*Join instead…*), which goes through the `Leave` flow first.

### 10.3 In a session

- Every foreign edit arrives through `apply_foreign` and `reread_studio`
  (the revision-forgetting re-read `docs/handoff.md` describes), so the
  window re-reads its lists; nothing else in the window knows it is in a
  session.
- A fetching clip is hatched with its percentage (§7.3).
- A refused or dropped edit is a toast (§5.5).
- The status line and the panel say who came and went.
- Leaving: **Stop sharing** on the host sends `Bye` to everyone; on a
  joiner **Leave session** keeps the copy open, now alone. Quitting mid-
  session is the same as leaving. Both sides write `shared_revision` (§4.5)
  on the way out.

### 10.4 Settings

Two rows on the settings page (`SETTING_ROWS`, `settings.rs:399`): **Your
name**, shown to people you share with (a text row — the first one; the
page has headings, buttons, sliders, choices and one switch today, so
`SettingControlKind::Text` backed by `TextEntry` is new and small), and
**Relay** (blank = Floptle Cloud) for §9.3's self-hosted case. `SETTINGS_
FORMAT_VERSION` 6 → 7.

---

## 11. The plumbing, by crate

| Crate | Change |
|---|---|
| `fontelle-types` | `ProjectMeta` fields (§4.1) live in `fontelle-model`, but `AssetRef.content_hash` gets meaning (§7.1) and `AssetKind` is on the wire. |
| `fontelle-model` | `wire.rs`: `Edit`, `Msg`, `ProjectHead`, `AssetEntry`; `Command::to_edit`; serde on every command; `History::{take_outbox, apply_foreign}`; `Project::sync_hash`; `ProjectMeta` fields + format 1 migration; `RenameProject`, the lane-creation and lane-rename commands (§5.2); `storage::peek_meta`. |
| `fontelle-net` (new) | The lifted relay client and server (§9.1), `Msg` framing over the reliable channel with 48 KB chunking and 480 KiB/s pacing, the `Collab` handle (`host(key or open addr) -> code`, `join(code)`, `send`, `poll`), the region table, the key. Tests run an in-process relay. |
| `fontelle-app` | `collab/`: host and joiner state machines (§5.5), asset pull (§7.2), snapshot in/out, the join flow (§4.3) over `projects::find_by_id`, backups (§4.4); `Session::collect_assets`; `bundle::resolve` and relative asset paths; `SampleLibrary::adopt_audio`; `Settings` rows; the `StudioHost` doors (`share`, `stop_sharing`, `join`, `answer_join`, `session_peers`, `session_status`). |
| `fontelle-ui` | Share panel, `WelcomeHit::Join`, `NameFor::JoinCode`, the three-button prompt, the hatched fetching clip, the caption dot, the text settings row. |
| `xtask` | Nothing — but the release checklist gains "the key is in the binary" once decision 1 lands. |

---

## 12. Tests, written first

### 12.1 `fontelle-model/tests/wire.rs`

- `every_command_has_a_wire_form`: for each public command constructor used
  in `commands.rs`'s own tests, apply on doc A, `to_edit`, `into_command`,
  apply on doc B (a clone of A before the edit) → A and B are byte-identical
  JSON (the `snapshot()` helper `tests/commands.rs` already has). Includes
  every `Restore*` by inverting first.
- `wire_variant_order_is_pinned`: the postcard tag of a sample of variants,
  as `relay.rs:1793` does.
- `a_foreign_edit_does_not_enter_the_undo_stack`; `undo_after_a_foreign_edit_
  undoes_only_mine`.
- `the_outbox_sees_a_drag_once`: four hundred merged moves, one `break_
  gesture`, one edit in the outbox, and it is the merged one.
- `sync_hash_ignores_view_state_and_paths`.
- `no_command_touches_view_state`.
- `a_format_0_project_gets_an_id_derived_from_its_birth`: two loads of the
  same bytes agree; a fresh project's id differs from every other's.
- `a_slot_command_refuses_when_the_slot_moved`: a `SlotRef` whose index no
  longer holds its id is refused, not applied to the neighbour.
- `note_ids_survive_save_and_load` (F10) and
  `a_foreign_edit_marks_the_document_dirty` (F12).

### 12.2 `fontelle-app/tests/collab.rs` — two sessions, one process

A `Loopback` transport (`fontelle_net::Transport` implemented over channels,
no socket) drives two `Session`s built with `Session::new` (`session.rs:1243`):

- `a_joiner_ends_up_with_the_hosts_document`: snapshot then five edits from
  each side; hashes equal after every message.
- `a_joiners_edit_is_instant_and_then_confirmed`: the joiner's document has
  the note before the host has heard of it, and the ids match after.
- `a_conflicting_proposal_is_refused_and_rolled_back`: the host deletes the
  clip the joiner is moving notes in; the joiner's pending move is dropped;
  hashes equal; the message is the toast text.
- `pending_edits_are_rebased_over_a_foreign_one`: two pending, one foreign
  in between, all three in the host's order on both sides.
- `undo_on_a_joiner_is_a_proposal`; `undo_of_a_thing_someone_changed_is_
  refused_with_a_sentence`.
- `a_recording_on_the_joiner_reaches_the_host_and_plays`: the edit lands
  first, the clip is `fetching`, then the bytes, then it renders.
- `the_join_copies_into_shared_when_the_id_is_unknown`,
  `the_join_asks_when_the_id_is_known`, `update_keeps_a_backup_and_deletes_
  nothing` (count files before and after; the manifest's old bytes are in
  `backups/`), `keep_both_forks_the_local_copy`, `both_changed_is_said_and_
  keep_both_is_the_default` (§4.5).
- `a_version_mismatch_is_refused_naming_both`.
- `sharing_an_unsaved_project_saves_first_and_collects_assets`: after
  Share, every `AssetRef` is bundle-relative and the files are under
  `assets/` by hash.
- `a_soundfont_already_in_the_bank_is_not_transferred`.
- `leaving_writes_shared_revision_on_both_sides`.
- `a_drag_in_progress_survives_a_foreign_edit` (F17); `an_assets_hash_is_
  its_bytes` and `the_manifest_is_the_asset_table` (F24, F25); `a_large_
  fetch_asks_first` (F29); `a_missing_plugins_state_survives_a_round_trip_
  through_a_machine_without_it` (F30); `a_view_only_peers_proposal_is_
  refused` and `a_removed_peer_is_gone` (F49).

### 12.3 `fontelle-net/tests/relay.rs`

- The lifted tests, unchanged.
- `host_join_and_echo_through_an_in_process_relay`.
- `a_frame_over_48kb_is_chunked_and_reassembled`.
- `the_sender_never_exceeds_the_budget`: 10 MB in, timestamps out, no
  second over 480 KiB.
- `a_lost_host_reconnects_inside_the_grace`.
- `a_dropped_joiner_rejoins_with_a_snapshot` (F39); `an_idle_lobby_lapse_
  is_reported_not_hidden` (F40); `a_code_names_its_relay` (F41).

### 12.4 `fontelle-ui/tests/`

- `welcome.rs`: the Join hit exists and lands in the code prompt.
- `share_panel.rs`: layout at the narrow and wide widths; the code is the
  largest text on the panel; three buttons in the prompt with the safe one
  first.
- `render_headless.rs`: a scene with a hatched fetching clip and the
  caption dot; a scene of the prompt.
- `app` level: `a_session_keeps_the_window_awake` (F42, the `sleep_budget`
  policy with a session open) and `a_message_wakes_the_window` (F48, the
  proxy delivers a user event).

### 12.5 On `:99`, with eyes

Two studios, two projects folders, one machine, the real relay
(`relay.fopull.com` once the key exists; the in-process open relay from a
tiny `xtask relay` before then). Share on one, join on the other, and look
at: the code readable at a glance; the copy appearing under *Shared*; a
note dragged on one screen arriving on the other; a take recorded on the
joiner playing on the host; the prompt on a second join, and the backup
folder afterwards; a version mismatch's sentence (run one studio with a
patched `CARGO_PKG_VERSION`). The memory's grab-lag rule applies: nudge
over a chip, grab twice.

---

## 13. Phases

Each phase is a commit or a few, green on its own, useful without the next.
Every phase names the ledger rows (§18) it closes; a phase is not finished
while one of its rows is open.

- **Phase 0 — Identity and the wire form (no network).** §4.1's fields and
  the format-1 migration; §5.2's stray mutations become commands, slots and
  markers get ids, the note-id question is settled by its test; §5.3's
  `Edit` and serde on every command; §5.4's outbox and `apply_foreign`
  (which sets `dirty`); §5.6's hash; `peek_meta` and `find_by_id`. Tests
  §12.1. *Nothing a user can see, and the tree is more honest for it.*
  Ledger: F1–F12.
- **Phase 1 — Two sessions, one process.** `collab/` host and joiner
  machines over a loopback transport; prediction and rebase; per-user undo;
  the join flow, its prompts and its three rules on a temp projects folder;
  `shared_revision`; the version refusal. Tests §12.2 minus the asset ones.
  *The whole feature works, with no socket.* Ledger: F13–F22.
- **Phase 2 — Files.** `collect_assets` and `project.assets` as the table,
  relative paths and `resolve`, `content_hash`, the library deduping by
  hash, the pull protocol over loopback, `adopt_audio`, the fetching state,
  the soundfont lookup and its prompt, the missing-plugin slot drawn and its
  blob protected. Tests: the rest of §12.2. Ledger: F23–F31.
- **Phase 3 — The relay.** `fontelle-net`: lift, chunking, pacing, the
  `Collab` handle, reconnect and reclaim, the region table, the key (or the
  open-relay setting until it exists), the `ENGINE_POLL` wake, the Windows
  build under Wine. Tests §12.3. Phase 1's machines run over it unchanged.
  Ledger: F32–F43. Cards §16 filed at the start of this phase.
- **Phase 4 — The window.** §10 including view-only and remove, the
  `EventLoopProxy` wake, the settings rows, tests §12.4, then §12.5 with two
  studios. `PROGRESS.md`, `docs/handoff.md`, the TDD's non-goals line and
  the product page's sentence updated (F44–F47). Release as `v0.16.0` when
  Ty gives the go. Ledger: F44–F50.
- **Phase 5 onward — §14**, each item designed there so it is a chunk to
  pick up, not a wish. Ledger: L1–L10.

Phases 0–2 need nothing from anyone. Phase 3 needs the key (decision 1) to
test against the managed relay, but not to build: the in-process open relay
is the test bed and a self-hosted open relay on any VPS is a valid product
path too.

---

## 14. After v1: designed now so nothing here is forgotten

Each of these was found or raised while planning v1 and is deliberately not
in it. Each has enough design here to be started cold, and a ledger row
(§18, L-rows) that stays open until it ships or Ty closes it.

### 14.1 Live gestures and cursors (L1)

Today an edit crosses the wire when the gesture breaks (§5.4). To show a
drag in flight: the history head's merged command, while its gesture is
open, is sent every 50 ms as `Ghost{peer, edit}` on the relay's **sequenced**
channel (stale ones dropped end to end, `relay.rs:13-16`); a receiver draws
the ghost by applying the edit to a *scratch clone of the affected clip's
notes* for drawing only — never to the document (INVARIANT 2 and §5.5 both
stay true). `Cursor{peer, view, tick, key}` on the same channel, 10 Hz,
draws a named marker in the peer's colour. Both are presence, both are
lossy, neither touches `History`.

### 14.2 Play together (L2)

A `Transport{peer, playing, tick, at: Instant}` message from the host, 4 Hz
and on every change, and a **follow host** switch on the joiner's panel that
sets the local transport from it, compensating by half the measured round
trip. Each machine renders its own copy; nobody's audio crosses the wire.
Decision 5 in §15 asks whether to build it in v1 after all.

### 14.3 Merging two copies that both changed (L3)

The join replaces or forks (§4.3); `shared_revision` (§4.5) knows when
both sides changed. A merge is possible because both sides' edits since the
common point are `Edit` values: keep each bundle's outbox since
`shared_revision` in `backups/edits-since-shared.postcard`, and on a
diverged join offer **Merge**: replay the joiner's edits on top of the
host's snapshot through the host as proposals, dropping the ones that
refuse, and list what was dropped. That is a rebase, not a three-way merge,
and it is honest about what it lost. Build it when a person has actually
hit the diverged prompt more than once.

### 14.4 A song reachable while its host is asleep (L4)

Option 3.3-B. The snapshot and the content-hashed assets are exactly what an
object store would hold, keyed by project id; W would build a small
service (presigned PUT/GET like the build store, `contracts/cloud-hosting.md`
§4, but for any bytes); it needs an account and it costs storage. When Ty
wants it, the card to W is: *"a bucket per Fontelle project id; PUT the
snapshot and missing assets on Stop sharing; a join of an id with no host
online GETs them."* The identity contract's device flow
(`contracts/identity-auth.md`) and `crates/floptle-account` are the login
to reuse.

### 14.5 Verified names (L5)

v1 names are whatever the person typed (§10.4). Floptle's per-server join
tokens (`0184`, `POST /oauth/join-token`, audience `cloud://<CODE>`) are the
path to *"this really is Alice"* — the joiner presents the token in
`Hello`, the host verifies it against the JWKS. Needs accounts; wait for
14.4.

### 14.6 Locking a code (L6)

*Remove* (§10.1) does not stop a removed person rejoining with the same
code. The fix is a host-side deny list by relay peer address for the
session, and **Regenerate code** on the panel (stop sharing, share again —
`WantCode` cannot ask for a *different* code, but a fresh `HostKeyed` gets
one). A passphrase in `Hello` is the next step if it is ever needed.

### 14.7 End-to-end encryption under the relay (L7)

The relay sees plaintext between its two TLS legs. If a session should be
private from the relay: the code becomes `UABCDE-k7x9…` where the suffix is
a 128-bit key, `Msg` bytes are sealed with ChaCha20-Poly1305 (`chacha20poly1305`,
one small crate) under it, and the relay forwards ciphertext. The code stays
a thing you paste. Not now, because the relay is Ty's.

### 14.8 Tidying a bundle (L8)

A join never deletes (§4.4), so a bundle's `assets/` can hold files no clip
references any more. An explicit **Tidy unused files** on the projects
browser lists them with sizes and moves them to `backups/tidied-<date>/`,
never to the bin. Separate, explicit, reversible.

### 14.9 Tolerating version skew (L9)

§8.3 requires identical versions. Once a release ships that changes neither
the document format, the command set nor `Msg`, a `PROTOCOL` match is
enough; the rule becomes *same protocol, same format version*, and the
refusal sentence names whichever differs.

### 14.10 The relay client as a dependency (L10)

§9.1 copies three files. The card to E in §16 asks for a crate; when it
exists, `fontelle-net` depends on it and deletes the copy. Until then the
pinned variant-order test is the tripwire.

---

## 15. Decisions for Ty, before phase 3 (phases 0–2 need none)

1. ~~Register Fontelle as a Floptle Cloud game and issue its key.~~
   **Done 2026-09-25**: registered as `fontelle`, key in §9.3. The W card
   in §16 is now only the budget question.
2. **What Save As does to the id.** Plan: a Save As mints a new id and
   records `forked_from`; only a join copy carries the original. So "Song"
   and "Song v2" are two songs, and sharing one never offers to replace the
   other. The alternative — Save As keeps the id — means a share of "Song"
   would find both on the friend's disk and ask which. *Recommended: mint.*
3. **Exact-version rule** (§8.3). Strict; every mismatch is a sentence
   naming who updates. *Recommended: yes for v1.*
4. **Soundfont fetching** (§7.1): fetch into the bank after a prompt naming
   the size, or refuse over some size. *Recommended: prompt, no cap; the
   prompt says the minutes.*
5. **Play together**: none in v1 (each transport independent), or a *follow
   host* switch. *Recommended: none in v1; add it when someone asks.*
6. **Where joins land**: `<projects_dir>/Shared/`. Or the top level, mixed
   in. *Recommended: Shared, so the browser can say which songs came from
   someone.*
7. **The name people see** (§10.4): a settings row, defaulting to the OS user
   name. Or ask on first Share. *Recommended: the row, and a first-Share
   prompt if it is still the OS name.*
8. **The product page** (0234) says "no account, no cloud" and "the only
   network request it ever makes is the optional update check". After phase
   4 the true sentence is *"…and, only while you share or join a song, a
   connection to Fopull's relay"*. Ty's wording, W's page.

---

## 16. Cards for the hub (file at the start of phase 3, not before)

All `project: fontelle`, `from: D`, next free number, per PROTOCOL §2. Draft
acceptance criteria so they are tasks, not questions.

- **To W — "Fontelle's relay budget."** (The registry entry and key
  already exist, 2026-09-25.) Goal: a song's samples move at more than
  512 KiB/s. Asks whether the control plane can grant the `fontelle` key a
  per-key byte budget and a larger reliable frame, and if not whether that
  is E's (`RelayLimits`). Acceptance: an answer in the thread, and if yes
  the numbers the key gets. Not blocking; §8.4's pacing is the product
  until then.
- **To E — "Publish the relay client as a crate, or bless the copy."** Goal:
  `fontelle-net` carries copies of `quic.rs`, `relay.rs`, `transport.rs`
  from commit X; a crate on crates.io (`floptle-relay-client`, no
  `floptle-core`) would let it become a dependency. Acceptance: either the
  crate exists at a version speaking the deployed relay's protocol, or a
  thread note that the copy is fine and a promise to post here when
  `RelayMsg` gains a variant. Not blocking.
- **To W, on 0234 (append to its thread, phase 4)** — the sentence in §15's
  decision 8, once Ty has chosen it.

---

## 17. Numbers to keep in view

| Thing | Figure | Where from |
|---|---|---|
| Relay | Oracle Always-Free E2.1.Micro, 1 OCPU, 954 MB, 480 Mbps, $0; relay uses 780 KB resident | 0188, 0191 |
| Per-connection budget | 512 KiB/s, 4000 msg/s, 3 s over → closed | `relay.rs:263-303` |
| Reliable frame caps | 128 KB down, 64 KB up | `relay.rs:305-319` |
| A 3-minute stereo 48 kHz WAV | ~33 MB → ~65 s to one joiner | arithmetic |
| A 325 MB soundfont | ~11 min to one joiner | arithmetic; hence §7.1's lookup and prompt |
| A drag | 1 message, at release | §5.4 |
| Round trip to Ashburn from the US | ~50–100 ms; from Europe ~150 ms; plus the 100 ms poll | §9.2 |
| Code space | 32⁵ ≈ 33 M per region; 30 joins/min/address | `relay.rs:479-500`, limits |
| Free plan | 100 CCU pooled per account | 0253 |
| New crates | quinn, rustls, ring, tokio, webpki-roots, socket2, postcard, bytes + transitive (~40) | `floptle-net/Cargo.toml` |

---

## 18. The findings ledger

Everything the two surveys turned up, in one place, so nothing is left to be
remembered. **F** rows are v1; a phase (§13) is finished when its rows are.
**L** rows are §14's later work and stay open until they ship or Ty closes
them. *Proof* is a test name from §12, a scene from §12.5, or a document
change; "—" means the row is a design decision that the phase's tests
exercise as a whole. Tick a row in this file when it is closed, with the
commit.

### Phase 0 — identity and the wire form

| # | Finding (where) | Answer | Proof |
|---|---|---|---|
| F1 ✓ `d25d828` | No project id: `ProjectMeta` is name, created, app_version, format_version (`project.rs:13-18`) | §4.1: `id`, `forked_from`, `saved_revision`, `saved_at`, `saved_by`, `shared_revision`; format 0→1; a legacy id derived from `created`+`name` | `a_format_0_project_gets_an_id_derived_from_its_birth` |
| F2 ✓ `d25d828` | Commands do not serialise: `Box<dyn Command>`, private fields, 102 impls, `Compound` nests (`command.rs:19-33`, `commands.rs`) | §5.3: `wire::Edit`, serde on every command, `Command::to_edit`, `Edit::into_command`, append-only numbering | `every_command_has_a_wire_form`, `wire_variant_order_is_pinned` |
| F3 ✓ `d25d828` | Ids are `Arena` keys minted locally at first `apply`; two peers would mint the same key for different things (`arena.rs:78-97`, `commands.rs:2094-2140`) | §5.4–5.5: only the host mints; a joiner's optimistic ids are replaced by the host's form of its own edit; redo's `insert_at` path is the replay | `a_joiners_edit_is_instant_and_then_confirmed` |
| F4 ✓ `d25d828` | No single choke point: `Session::run` (82 callers), `apply_for` (17), five direct `history.apply` (`session.rs:3215, :5679, :3729, :3977, :4145, :4904, :6578`) | §5.4: the outbox lives in `History::apply`, under all of them | `the_outbox_sees_a_drag_once` |
| F5 ✓ `d25d828` | `automation_lane` inserts a lane directly, off the undo stack (`session.rs:1941-1960`) | §5.2: becomes a command | the command's apply/invert round trip in `tests/commands.rs` |
| F6 ✓ `d25d828` | Row render renames a lane via `lanes.get_mut` (`session.rs:2464`) | §5.2: becomes a command | same |
| F7 ✓ `d25d828` | `meta.name` written directly (`session.rs:991, :1721, :1773`, `adopt`) | §5.2: `RenameProject` | same |
| F8 ✓ `d25d828` | Insert slots and sends addressed by `usize` (`PluginTarget::Insert`, `PresetTarget::Insert`, `ParamTarget::Insert`; `commands.rs:7346, :7501`, `param.rs:249`) | §5.2: `EffectSlot`/`Send` get `id: PersistentId`; commands carry `SlotRef{index, id}` and refuse on mismatch; `ParamAddress` strings untouched | `a_slot_command_refuses_when_the_slot_moved` |
| F9 ✓ `d25d828` | `markers: Vec<Marker>` has no ids (`project.rs`) | §5.2: `Arena<MarkerId, Marker>`; `MarkerId` already in `id.rs` | marker commands' round trip |
| F10 ✓ `d25d828` | `prefab.rs:123` says a `NoteId` is "minted fresh every session"; `Arena` serde keeps keys (`arena.rs:225-262`) — a contradiction | §5.2: the test decides, the comment is corrected | `note_ids_survive_save_and_load` |
| F11 ✓ `d25d828` | `view_state` (zoom, scroll) lives inside the document; `Session.revision` counts view changes too | §5.2, §5.6: excluded from the hash, never on the wire; the wire has its own `seq` | `sync_hash_ignores_view_state_and_paths`, `no_command_touches_view_state` |
| F12 ✓ `d25d828` | `Session.dirty` is set by ~50 local paths only | §5.2: `apply_foreign` sets it, so a joiner's copy asks to be saved | `a_foreign_edit_marks_the_document_dirty` |
| F51 ✓ `d25d828` | *Found building Phase 0.* Undoing a track preset lost every plugin insert, sidechain key, note source and page of notepad words the preset had replaced: `ApplyTrackChain`'s inverse was the same command pointed at the old rack *as a chain*, and a chain is a sound, not a rack | `RestoreTrackChain` puts the exact slots back | `every_command_has_a_wire_form` (the inverse has to put the studio back) |
| F52 ✓ `d25d828` | *Found building Phase 0.* The document cannot travel in postcard: `PatchData.body` is a `serde_json::Value`, `effect.rs` has two `#[serde(untagged)]` enums, and seventeen fields are `skip_serializing_if` — a format that does not describe itself can read none of them back | An `Edit` (and, Phase 1, the snapshot) travels as JSON, which every document type already survives because the project saves as JSON; JSON names variants, so the pinned test pins **names** (never rename a variant or a field). `Msg`'s envelope may still be postcard around JSON bytes — decided in Phase 1 | `wire_variant_order_is_pinned`, `every_command_has_a_wire_form` |
| F53 ✓ `d25d828` | *Found building Phase 0.* `OverrideMap` was a `HashMap` keyed by a tuple — not writable as JSON at all once it held anything, so the first prefab override ever made would fail the save — and a `HashSet`, which writes in iteration order and would give two copies of one song two hashes | Sorted maps, written as a list of pairs; old files' empty `{}` still read | `an_override_map_writes_one_way_whatever_order_it_was_filled_in` |
| F54 ✓ `d25d828` | *Found building Phase 0.* Four inverses (`RestoreInsertConfig`, `RestoreDeviceState`, `RestoreChannelAbOther`, and `RestoreTrackChain` as first written) swapped their own fields with the document, so their applied form carried the *old* state and replayed backwards on another machine. (`RestoreInsertConfig` is built by nothing since `SetInsertPreset` went; it is exported, so it is fixed rather than left.) | Write a copy, remember what was replaced — the shape of every other command | `every_command_has_a_wire_form` |

### Phase 1 — two sessions, one process

| # | Finding | Answer | Proof |
|---|---|---|---|
| F13 ✓ `24ab14a` | Two people editing at once need one order | §3.2, §5.5: host-authoritative stream; `Applied{seq}` | `a_joiner_ends_up_with_the_hosts_document` |
| F14 ✓ `24ab14a` | A round trip through Ashburn is 50–200 ms; a drag per pointer motion cannot wait for it | §5.5: joiner applies at once, proposes, rebases | `a_joiners_edit_is_instant_and_then_confirmed` |
| F15 ✓ `24ab14a` | A proposal can be refused after the joiner already sees it | §5.5: `Refused`, invert, toast naming what and why | `a_conflicting_proposal_is_refused_and_rolled_back` |
| F16 ✓ `24ab14a` | Undo across two people's edits | §5.7: per-user undo; a foreign edit never enters the stack; an inverse is a proposal | `a_foreign_edit_does_not_enter_the_undo_stack`, `undo_after_a_foreign_edit_undoes_only_mine`, `undo_on_a_joiner_is_a_proposal`, `undo_of_a_thing_someone_changed_is_refused_with_a_sentence` |
| F17 ✓ `24ab14a` | A foreign edit mid-drag; `MoveLimits` measured at gesture start goes stale | §5.5 last paragraph: the open gesture is inverted and re-applied like any pending edit; a refusal is the toast | `pending_edits_are_rebased_over_a_foreign_one`, `a_drag_in_progress_survives_a_foreign_edit` |
| F18 ✓ `24ab14a` | Two documents could drift with nothing noticing (`heal_audio_clips`, an unsent mutation) | §5.6: `sync_hash` on every `Applied`; resync on mismatch; panic under `cfg(test)` | every §12.2 test asserts equal hashes after every message |
| F19 ✓ `24ab14a` | Nothing can find a bundle by id; `projects.rs` knows names | §4.2: `storage::peek_meta`, `projects::find_by_id`, the `Shared/` folder | `the_join_copies_into_shared_when_the_id_is_unknown`, `the_join_asks_when_the_id_is_known` |
| F20 ✓ `24ab14a` | Replacing a local copy could lose a file — "that would be very bad" | §4.4: never delete; backup `project.json` first; unsaved work asked first; atomic write | `update_keeps_a_backup_and_deletes_nothing`, `keep_both_forks_the_local_copy` |
| F21 ✓ `24ab14a` | `saved_revision` cannot tell *behind* from *diverged* | §4.5: `shared_revision` written by both sides at session end | `both_changed_is_said_and_keep_both_is_the_default`, `leaving_writes_shared_revision_on_both_sides` |
| F22 ✓ `24ab14a` | Format, command set and `Msg` all move with a release | §8.3: exact version match, a sentence naming who updates | `a_version_mismatch_is_refused_naming_both` |
| F55 ✓ `24ab14a` | *Found building Phase 0.* §5.5 has the host strip a proposal's ids and mint its own. Then a joiner's **second** edit that names what its first one made — draw a note, then drag it, inside one round trip — names an id that exists on neither side after the rebase: the host refuses it and the joiner's rebase drops it, every time. Remapping ids inside an edit is not possible from its JSON either: a `NoteId` and a `ClipId` are the same `{idx, version}` there | Each joiner mints in **its own id range** (a per-peer space in `Arena`), so the host applies a proposal with the ids it came with and they cannot collide with the host's or another joiner's; the order stays the host's and a real conflict (the clip was deleted) is still a refusal. The plan's "only the host mints" becomes "nobody mints where anybody else can" | `a_joiners_second_edit_can_name_what_its_first_made` |

### Phase 2 — files

| # | Finding | Answer | Proof |
|---|---|---|---|
| F23 ✓ `e41089d` | Imported audio is referenced by absolute path and never copied (`bundle.rs:75-82`, `library.rs:186-225`); TDD §17.1's Export Bundle is unbuilt | §7.1: `collect_assets` into `assets/<sha256>.<ext>`, bundle-relative refs, `bundle::resolve` as the one reader | `sharing_an_unsaved_project_saves_first_and_collects_assets` |
| F24 ✓ `e41089d` | `AssetRef.content_hash` is stored as 0, "nothing reads it yet" (`library.rs:214-217`); only SF2 hashes (xxhash64 of 1 MB) | §7.1: the sha256's low 64 bits, on every kind; the manifest's name for a file | `an_assets_hash_is_its_bytes` |
| F25 ✓ `e41089d` | `project.assets: AssetTable` is read and written by nothing | §5.2: becomes the bundle's asset table and the `Welcome` manifest, or is removed with the format bump | `the_manifest_is_the_asset_table` |
| F26 ✓ `e41089d` | `AssetId` is a runtime `SampleLibrary` key that is persisted; a joiner's library would mint different ones (`library.rs:320-372`) | §7.3: `SampleLibrary::adopt_audio(id, path)` accepts the host's id, on the `reload_audio` path | `a_recording_on_the_joiner_reaches_the_host_and_plays` |
| F27 ✓ `e41089d` | The library dedupes by path (`audio_by_path`) | §7.1: also by hash, so a file already present is never fetched | `a_soundfont_already_in_the_bank_is_not_transferred` |
| F28 ✓ `e41089d` | A missing file renders silent with no reason shown beyond a message (`bundle.rs:16-43`) | §7.3: the *fetching N %* state, hatched, swapped in on `AssetDone` | the same test, plus the §12.4 scene |
| F29 ✓ `e41089d` | Soundfonts are hundreds of MB; 325 MB is ~11 min at the relay's budget | §7.1: looked for by hash in bundle then bank (`hashes.json`); fetched only after a prompt naming size and minutes; smallest first | `a_soundfont_already_in_the_bank_is_not_transferred`, `a_large_fetch_asks_first` |
| F30 ✓ `e41089d` | A plugin the joiner lacks: the rack already leaves the slot silent with a message (`plugins.rs:545-549`); `capture_plugin_states` writes blobs outside any command at save (`session.rs:3095-3120`) | §7.4: draw the slot as missing; never write a blob from a machine that could not open the plugin | `a_missing_plugins_state_survives_a_round_trip_through_a_machine_without_it` |
| F31 ✓ `e41089d` | `heal_audio_clips` rewrites every audio clip's trim on every `republish`, outside any command (`session.rs:2774-2787`) | §5.2: declared local and deterministic given the same bytes; the hash (F18) is what proves it, and this is the first suspect if it ever fails | hashes equal after a recording lands on both sides |
| F56 ✓ `e41089d` | *Found building Phase 1.* An audio clip's `AssetId` is minted by each studio's `SampleLibrary` (its own `Arena`), outside the history — so a joiner's import and the host's at the same moment can claim one id for two different files, and the clip would play the wrong audio on one side | A library import runs in the history's mint space, like everything the history applies (F55); a received file is loaded under the id the edit names (`reload_audio`) | `two_imports_at_once_are_two_files` |
| F57 ✓ `e41089d` | *Found building Phase 2.* `bundle::open_project` reloads a channel's patch samples and the audio clips' files, and nothing else: a **prefab's** audio source and a channel's **A/B slot** patch name files that are never read back, so a reopened song — and every joiner's copy, which is a reopened song — plays those silent | The open reloads every file the song names, from the same walk the manifest is made from | `a_prefabs_audio_and_an_ab_slots_samples_open_with_the_song` |

### Phase 3 — the relay

| # | Finding | Answer | Proof |
|---|---|---|---|
| F32 ✓ `43e00dd` | The tree has no network, TLS or async crate on purpose (`updates.rs:19-26`); `deny.toml` bans git and unknown registries | §3.1, §9.1: `fontelle-net` with quinn/rustls/tokio/postcard from crates.io; the proportion argument answered | `cargo deny` and clippy on both targets stay clean |
| F33 ✓ `43e00dd` | The relay client is three files inside `floptle-net` with `floptle_core` only in tests; postcard variant order must be kept (`relay.rs:1793`); no `contracts/relay.md` exists — the wire is defined only in code | §9.1: copy `quic.rs`, `relay.rs`, `transport.rs`, note the engine commit, keep the pinned test; card to E (§16) | the lifted tests, `host_join_and_echo_through_an_in_process_relay` |
| F34 ✓ `43e00dd` | The managed relay refuses a keyless host ("This relay is Floptle Cloud…", `policy.rs:605-612`); joiners need nothing; if the authorize endpoint is unreachable the free tier falls back to 20 CCU | §9.3: `HostKeyed` with Fontelle's compiled-in key (registered 2026-09-25, key in §9.3); `Settings::relay` for an open relay | manual: `Hosted{code}` from `relay.fopull.com` with the key |
| F35 ✓ `43e00dd` | Reliable frames capped at 128 KB down and 64 KB up (`relay.rs:305-319`); the decoder refuses >1 MiB | §7.2, §8.4: 48 KB chunks both ways for snapshots and assets | `a_frame_over_48kb_is_chunked_and_reassembled` |
| F36 ✓ `43e00dd` | 512 KiB/s and 4000 msg/s per connection; three seconds over closes it; per address 10 opens and 30 joins a minute (`relay.rs:263-303`) | §8.4: the sender paces at 480 KiB/s; the `:99` test does not restart studios in a tight loop | `the_sender_never_exceeds_the_budget` |
| F37 ✓ `43e00dd` | The host pays its budget once per joiner (0235); Oracle egress assumed $0 (plan §7–8) | §7.2, §17: manifest in ascending size, one request in flight per peer, the numbers written down; card to W/E asks about a per-key budget | — |
| F38 ✓ `43e00dd` | A host that drops is held 20 s (`HOST_GRACE`); the client backs off 1–30 s; `WantCode`/`host_keyed_reclaiming` exist | §9.2: reclaim the same code on reconnect so nobody is told a new one | `a_lost_host_reconnects_inside_the_grace` |
| F39 ✓ `43e00dd` | QUIC keep-alive 500 ms, idle timeout 8 s (`quic.rs:43-46`) | §9.2: a joiner rejoins and takes a fresh snapshot; no op log for catch-up | `a_dropped_joiner_rejoins_with_a_snapshot` |
| F40 ✓ `43e00dd` | A lobby with no clients ends after 30 min (`relay.rs:1232-1253`) | §8.4: the host re-registers on the next Share; the panel says the code lapsed | `an_idle_lobby_lapse_is_reported_not_hidden` |
| F41 ✓ `43e00dd` | Codes carry a region letter (`U` = us-east); the engine resolves it from a cached regions API | §9.3: a compiled-in letter table; a new region is a release | `a_code_names_its_relay` |
| F42 ✓ `43e00dd` | The window sleeps `Forever` when idle; there is no wake path for a network thread (`widget/*.rs:208`) | §9.2: `ENGINE_POLL` while a session is open (this phase); the proxy is F48 | `a_session_keeps_the_window_awake` |
| F43 | CI builds three targets; the Windows build runs under Wine here; sockets and rustls on `x86_64-pc-windows-gnu` are untested in this tree | §0: Phase 3 ends with the Windows build hosting and joining under Wine on `:99`, and clippy on the msvc target | scene in §12.5, on Wine |
| F58 ✓ `43e00dd` | *Found preparing Phase 3.* F38's answer does not hold on today's relay: a returning host reclaims its lobby inside `HOST_GRACE` only when the relay's policy says the key owns the code (`claim_code`), and the managed policy grants that only for codes the control plane has *reserved* for a deployment (`floptle-relay/src/policy.rs:630-636`). An ordinary Fontelle host that drops comes back under a new code, and its joiners — held in the old lobby for twenty seconds — are stranded; on an open relay (no policy) reclaim never happens | Fontelle re-hosts with `WantCode` (its own code) on a drop, which is right the day the relay agrees; a card to E asks the managed policy to let a host reclaim the code it was given, inside the grace window. Until then a host's drop ends the session on the joiners with a sentence saying so, and the host's panel shows the new code | `a_lost_host_reconnects_inside_the_grace` (against an in-process relay whose policy grants it), the card's thread |
| F59 | *Found building Phase 3.* **No QUIC socket opens under Wine.** quinn-udp 0.5 asks `getsockopt(IPPROTO_IPV6, IPV6_V6ONLY)` of every socket, an IPv4 one included, and treats a failure as fatal; Wine 11.16 answers `STATUS_NOT_SUPPORTED` (seen with `WINEDEBUG=+winsock`: `server_getsockopt status 0xc00000bb`, then *"bind 0.0.0.0:0: OS Error 10045"*). Real Windows answers it, which is why quinn works there. So F43's "hosting and joining under Wine" cannot be done on this machine | The Windows build's sockets are proved where they run: CI's `windows-latest` job runs `cargo test --workspace`, `fontelle-net/tests/relay.rs` included (host, join, frames, reclaim, idle lapse over a real relay). Under Wine, everything but the network is still checked. A live two-studio session on real Windows is the person with a Windows machine's | CI's Windows run of `tests/relay.rs` on the next push (Ty's) |

### Phase 4 — the window

| # | Finding | Answer | Proof |
|---|---|---|---|
| F44 | The product page (0234) says "no account, no cloud" and "the only network request it ever makes is the optional update check" (lines 52, 75, 82-84, 145) | §15 decision 8: one clause added, Ty's wording; appended to 0234's thread | the thread entry |
| F45 | `FONTELLE_TDD.md:64` lists "network collaboration, cloud anything" as a v1 non-goal | The TDD is Ty's: propose the amended line in `PROGRESS.md`'s entry and leave the edit to him | the `PROGRESS.md` entry |
| F46 | No Share or Join anywhere in the window | §10.1–10.3 | `welcome.rs`, `share_panel.rs`, the scenes |
| F47 | No display name; the settings page has no text row (`SettingControlKind`) | §10.4: *Your name* and *Relay* rows, `SettingControlKind::Text` over `TextEntry`, settings format 6→7 | `settings_tab.rs` |
| F48 | Remote edits should not wait for a 100 ms poll | §9.2: `EventLoopProxy` wake from the network thread; the poll stays as the fallback | `a_message_wakes_the_window` |
| F49 | Anyone with the code can edit; the host cannot remove anyone | §10.1: view-only and remove per row | `a_view_only_peers_proposal_is_refused`, `a_removed_peer_is_gone` |
| F50 | Everything above must be seen, not believed | §12.5 with two studios and the real relay; `PROGRESS.md` and `docs/handoff.md` updated | the scenes, the entries |
| F60 | *Found on `:99`.* The panel's one status line clipped its sentence — *"Alice removed you from the session — your copy is…"* | The status is two lines, broken at the sentence's dash (`canvas::status_lines`) | `a_status_sentence_gets_two_lines_broken_at_its_dash` |
| F61 | *Found on `:99`.* A joiner's panel said *"Nobody has joined yet"* while working with the host: the host is not one of the peers the session hands out | The joiner's panel lists the host first, in the host's colour | `the_window_doors_share_and_join_over_a_relay` |
| F62 | *Found on `:99`.* The join's answers were sentences (*"Update mine to Alice's — a backup of yours is kept"*) and ran out of their buttons | *Update mine*, *Keep both*, *Cancel*; the two lines above them say what each does | `the_joins_answers_fit_on_their_buttons_and_the_lines_say_the_rest` |
| F63 | *Found on `:99`.* Both copies said *"saved by fopull"*: a save was signed with the login, not the name the others know | A save is signed with *Your name* (`Settings::your_name`) | `a_save_is_signed_with_the_name_the_others_see` |
| F64 | *Found on `:99`.* The Share button's tip was drawn over the panel it had just opened, and the view-only toast was clipped at *"…for yo"* | No tip under the panel or a session's question; the toast says *"— it is view only."* | seen on `:99`; `a_view_only_peers_proposal_is_refused` |

### Later — §14

| # | Item | Design |
|---|---|---|
| L1 | Live drag ghosts and cursors | §14.1 |
| L2 | Play together | §14.2, decision 5 |
| L3 | Merging two copies that both changed | §14.3 |
| L4 | A song reachable while its host is asleep (cloud storage; Cloud saves are 256 KB JSON slots and build storage takes only server bundles, so this is a new W service) | §14.4 |
| L5 | Verified names via join tokens (0184) | §14.5 |
| L6 | Locking a code after a removal; regenerate | §14.6 |
| L7 | End-to-end encryption under the relay | §14.7 |
| L8 | Tidying unused files from a bundle | §14.8 |
| L9 | Tolerating version skew | §14.9 |
| L10 | The relay client as a crate dependency | §14.10, card to E |

---

## 19. Where the build departs from the plan

Written as the phases land, the way `docs/disgusting-beat-plan.md` §15 was:
each is a place the tree answers a question differently from the text above,
with the reason. The text above is left as it was planned.

**Phase 0.**

- **Edits are JSON, not postcard** (§5.3, §8.2). F52 says why. The numbering
  discipline becomes a naming one: `Edit::TAGS` and the pinned literals.
- **`SlotRef` is a field the command learns** (§5.2). Every command that names
  an insert or a send by its place carries `slot: Option<PersistentId>`,
  filled on the first apply — on the machine whose rows the place was counted
  against — and checked on every apply after, including the other end of a
  wire. The wire only ever carries applied commands, so the effect is the
  plan's `SlotRef { index, id }`, and not one of the window's ~300 index-taking
  `StudioHost` calls had to change. Commands that *make* a slot remember the id
  they gave it (`made`), or a redo would make a different insert.
- **The outbox is shut until a session opens it** (§5.4). `History::open_outbox`
  / `close_outbox`; closed, `take_outbox` is empty and nothing is copied — §1's
  ninth point. An undo sends the gesture it undoes first if that gesture had
  not gone yet, so the other side never inverts a thing it was never given.
- **The project's name is not the song** (§5.2, F7). `RenameProject` exists and
  every write of the name goes through it, but the session applies it outside
  the history: the name follows the folder (`Session::adopt`), a joiner's copy
  may be "Song (2)", and an undo that put the old name back would title the
  window with a name no folder has. `ProjectMeta` as a whole — name, id, save
  stamps — is left out of `sync_hash`.
- **F12's test lives in `fontelle-app/tests/collab.rs`**, not `wire.rs`:
  `dirty` is the session's, and so is `apply_foreign` at that level
  (`History::apply_foreign` is the model's half).
- **A Save As forks only a song that already has a file.** The first save of a
  new, never-saved song is that song; forking it would record a parent nobody
  has on disk.
- **A format-0 project's inserts and sends are given derived ids too**, not
  minted on load (`#[serde(default)]` minting stays, for a hand-edited file).
  Two loads of one old file then agree about every id in it, not just the
  song's.


**Phase 1.**

- **The joiner rebuilds rather than unwinds** (§5.5 steps 1–3). It keeps the
  host's song exactly (`confirmed`: every `Applied` in order, nothing of its
  own) beside the one on screen, and on anything from outside makes the
  screen's again as *confirmed + pending*. Unwinding pending edits through
  their inverses was the plan; a command's remembered "previous" goes stale
  the moment somebody else changes the same thing, and the second unwind
  through a stale one puts back a state that never existed. The cost is a
  second copy of the song on a joiner, and a clone per foreign edit only
  while something of its own is pending.
- **Nobody mints where anybody else can** (F55). A joiner's history mints in
  its own space of every arena (`arena::minting_in`, from `peer × 2²⁴` up, in
  a sparse region beside the dense one); the host takes a proposal with the
  ids it came with and never strips them. Welcome's `peer` is that space.
- **Nothing from outside lands under a gesture in the hand**, on either side
  (§5.5's last paragraph). The host holds proposals and joins while it has a
  drag down — a snapshot of a drag's middle would be a song nobody else could
  reach — and a joiner holds the host's stream. An edit left in the hand with
  nothing more coming is let go of after `CollabOptions::idle_break` (400
  ms): a key press has no mouse-up to end it. While sharing, a drag held
  perfectly still that long becomes two undo entries.
- **`shared_revision`'s number is the song's hash at parting**, not a save
  count (§4.5). Each side counts its own saves, so a count cannot say whether
  the other side changed; `sync_hash` is what both can compute without
  talking. The join's copy is written with the record already in it, so a
  copy never saved after the join is still recognised as *behind*.
- **The snapshot is the song's JSON**, not postcard (F52).
- **`Hello` carries `install` and `Welcome` carries `host` and
  `host_install`** — the record of what two copies agreed on names a studio,
  and the question names the host. **`Applied.hash` is an `Option`**: when the
  host sends several of its own edits at once, only the last is on its own in
  the document to be hashed.
- **A join copy is named by the tree's own rule** ("Song 2", `unique_name`),
  not "Song (2)".
- **`Session::share` saves unsaved changes itself**; the window's name prompt
  is for a song that has never been saved.
- **The transport seam came over in Phase 1**: `transport.rs` is the loopback
  §12.2 asks for (`MemoryHub`, with latency by the tick). `quic.rs` and
  `relay.rs` follow in Phase 3.
- **Leaving lets go of an edit in the hand and sends what is waiting before
  the goodbye**, on both sides, or the last thing done never reaches the
  other and the two copies part disagreeing about the song.
- **A refused edit's toast names the edit, not the reason**: *"“Move 3
  notes” was taken back — Alice had changed it first."* The reason the
  command gave names ids; it goes to the log.

**Phase 2.**

- **The asset table is gone** (F25's second answer). What a song needs is
  what it names — `Project::files()`, one walk over clips, prefabs and every
  patch body including the A/B slot's — and the manifest, the open and
  collecting all read that walk. A table every command had to keep in step
  with the song would be a second list to drift.
- **A collected file is `assets/<name>.<16 hex digits>.<ext>`**, not
  `assets/<sha256>.<ext>`. The sixteen digits are the `content_hash` itself,
  so the file is still named by what is in it; the name is so an audio clip
  still says "Take 3" rather than sixty-four hex digits
  (`library::sound_name` drops the digits for a caption).
- **`content_hash` is the digest's last eight bytes, big-endian**, on every
  kind — soundfonts too, which used to be an xxhash of the first megabyte.
  Hashes are cached by path, size and modification time in `hashes.json` in
  Fontelle's **data** folder rather than beside the soundfonts: the bank
  folder is the person's.
- **Whether a studio has a file is decided by its hash, in its own bundle
  and its own soundfont folders**, never by the path a reference carries: a
  path in somebody else's edit names a place on *their* disk. (Two studios on
  one computer, which is what every test is, found each other's files by
  path until this was so.) `bundle::resolve` — the one reader of a path —
  falls back to the same lookup.
- **A file somebody's edit names that is already here is loaded at once**;
  one that is not is fetched from whoever made the edit.
- **A sound brought in while sharing is collected straight away**, as
  sharing collected everything before it.
- **Every soundfont is asked about before it is fetched** (it goes into the
  bank, outside the song), and anything over `CollabOptions::ask_above` (64
  MB). Decision 4's recommendation: the question gives the size and the
  minutes, and there is no cap.
- **F27's "the library dedupes by hash too"** is where it matters — the
  fetch never asks for a file that is here (`Here::has`). The library still
  dedupes imports by path: one sound imported from two places is two assets
  until it is collected, and then one file.
- **Sixteen pieces a turn** per studio being sent to; the relay's byte
  budget is Phase 3's pacing.
- **F57 was found here**: a reopened song never read back a prefab's audio or
  an A/B slot's samples.
- **A missing plugin's slot draws its name in the alarm colour**; the slot,
  its settings and its state are kept, and the state is never written from a
  machine without the plugin (`PluginRack::snapshot` already answered `None`
  for one that is not live).

**Phase 3.**

- **Every message is framed, not only snapshots and files**: `Framed` splits
  any reliable message over 56 KB (not 48) and joins it again, so an edit that
  deletes five thousand notes crosses as surely as a sample. 56 KB is under
  the relay's 64 KB peer-to-host cap with room for its envelope, and makes a
  48 KB piece of a file one frame. The session's own 48 KB pieces stay.
- **Pacing is per connection, on an injectable clock** (`Paced::with_clock`):
  a host's one leg carries what it sends every joiner, which is §8.4's
  arithmetic, and the budget test runs ten simulated megabytes in a moment.
- **A host re-hosts itself when its leg drops**, asking for its own code
  (`host_keyed_reclaiming`, on a thread), rather than letting the lifted client
  back off and quietly mint a new code; the "lost the connection to the relay"
  it raises for each joiner is kept from the session, which would forget them.
  **`reclaim` is tested; the automatic trigger is not** — the lifted code
  offers no way to cut a live QUIC leg in-process — and **today's relay would
  answer with a new code anyway** (F58, hub card `tasks/fontelle/0266`), which
  a notice says. Also found: after a reclaim the lifted client's handshake
  loop swallows the relay's re-announcement of who is still in the lobby; the
  session keeps its roster across the reconnect, so it does not need it.
- **A lapsed lobby ends the share with a sentence** (F40) — *"Your share ended
  — nobody joined it for half an hour. Press Share for a new code."* — and its
  code is never offered as live again.
- **A dropped joiner writes down what it last agreed with the host** (the
  host's song as it last had it), so joining again finds its copy *behind*
  and offers the safe update — F39's fresh snapshot.
- **The window's wake is `watching(engine, session)`** feeding `sleep_budget`,
  and the session is pumped once a pass through `StudioHost::pump_session`
  (F42). The `EventLoopProxy` wake is Phase 4's F48.
- **`deny.toml` allows `CDLA-Permissive-2.0`**: `webpki-roots` is Mozilla's
  root certificates as data under that licence. Everything else the relay
  client brings was already allowed.
- **Three of the lifted tests stay in the engine**: they drive its game
  session (`NetSession`, `floptle_core::World`); each is marked where it was.
- **Hosting on Floptle Cloud was seen** (F34): `fontelle_hosts_on_floptle_cloud`,
  run by hand on 2026-09-25, hosted as `UL22A6` and then `UQVAXH` and joined
  it, bytes both ways.
- **F43 is not closed** — see F59: no QUIC socket opens under Wine, and the
  Windows proof is CI's run of `tests/relay.rs` on real Windows.

**Phase 4.**

- **Two messages appended** to `Msg` (F49): `ViewOnly { view_only }` (tag 16)
  so the joiner's panel says so before anybody tries, and `Removed { by }`
  (tag 17) so a removal is not mistaken for the host stopping. The plan had a
  `Bye` for both.
- **The people's colours are Fontelle's own eight** (`canvas::PEER_COLOURS`):
  the plan's "theme's clip palette" does not exist — a clip is its channel's
  colour.
- **The dot is on the Share button, not in the caption**: the caption is only
  the OS title bar, which cannot carry a colour. The title says *(shared)* in
  words.
- **A text settings row is typed into the prompt a project is named in**
  (`SettingPress::Type`), not a new `TextEntry` field on the page: the prompt
  already is one, with its caret, its keys and its Enter.
- **The first Share or Join asks the name** when *Your name* was never typed
  (decision 7); a blank answer keeps the computer's user name, so it is asked
  once.
- **Copy uses the desktop's own clipboard program** (`wl-copy`, `xclip`,
  `xsel`, `clip`, `pbcopy`), the text on its standard input — the same
  no-new-crate rule the file dialogs follow; with none, the toast says the
  code instead.
- **The wake is one event per burst** (`widget::WindowWake`): the network's
  thread calls it on every message, the loop gets one user event until it has
  looked. The hook is Fontelle's own addition to the lifted transport
  (`Transport::set_wake`, marked in `transport.rs` and `quic.rs`); hub card
  `0265` is told.
- **§12.5, seen on 2026-09-25**, two studios through `relay.fopull.com`
  (codes `UMS6ZY`, `U3SW8X`): Share, the code, Copy onto the real clipboard,
  Join by the code typed in lower case, the copy under *Shared*, a clip and
  notes both ways, view only (a refused note taken back with its toast),
  remove, a second join asking with the unsaved work saved first, *Update
  mine* writing `backups/before-join-…`, *Keep both* making `Duet 2`, and the
  joiner's panel naming the host. F60–F64 were found doing it. **Not done by
  hand**: a recorded take on the joiner, and the version-mismatch sentence —
  both are tests (`a_recording_on_the_joiner_reaches_the_host_and_plays`,
  `a_version_mismatch_is_refused_naming_both`), neither was looked at.

