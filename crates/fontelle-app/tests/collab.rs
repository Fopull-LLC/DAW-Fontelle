//! Working on one song together (`docs/collab-plan.md`).
//!
//! Phase 0 is here: what the session does with an edit from somebody else,
//! the project's identity as the session saves it, the edits that used to
//! skip a command, and finding a song on disk by its id. Phase 1 grows this
//! into two sessions in one process (§12.2).

mod common;

use std::path::{Path, PathBuf};

use fontelle_model::{Command, RenameLane};
use fontelle_types::ParamAddress;
use fontelle_ui::document::{DocumentHost, StudioHost};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-collab-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn a_session() -> fontelle_app::Session {
    common::a_session_for(common::a_clip_project(4))
}

/// F12. A joiner's copy has to ask to be saved when somebody else changes the
/// song, or it is closed believing it is on disk.
#[test]
fn a_foreign_edit_marks_the_document_dirty() {
    let mut session = a_session();
    assert!(!session.is_dirty());
    let revision = session.revision();
    let lane = session.project().lane_ids()[0];

    let mut elsewhere = session.project().clone();
    let mut theirs = RenameLane::new(lane, "Theirs");
    theirs.apply(&mut elsewhere).unwrap();
    session
        .apply_foreign(theirs.to_edit())
        .expect("their edit applies here");

    assert_eq!(session.project().lanes[lane].name, "Theirs");
    assert!(
        session.is_dirty(),
        "somebody else's edit is unsaved work too"
    );
    assert_ne!(session.revision(), revision, "and the window re-reads");
    assert_eq!(
        session.project().sync_hash(),
        elsewhere.sync_hash(),
        "the two copies are the same song"
    );
}

/// F12's other half: an edit that does not apply here changes nothing and
/// says so.
#[test]
fn a_foreign_edit_that_does_not_apply_is_refused_and_changes_nothing() {
    let mut session = a_session();
    let before = session.project().sync_hash();
    let mut elsewhere = session.project().clone();
    let mut spare = fontelle_model::AddLane::new("Spare");
    spare.apply(&mut elsewhere).unwrap();
    let mut theirs = fontelle_model::RemoveLane::new(spare.id().unwrap());
    theirs.apply(&mut elsewhere).unwrap();

    assert!(
        session.apply_foreign(theirs.to_edit()).is_err(),
        "that row was never here"
    );
    assert_eq!(session.project().sync_hash(), before);
    assert!(!session.is_dirty());
}

/// F5. Making automation made its row off the undo stack, so taking the
/// automation back left an empty row behind — and nobody sharing the song
/// ever heard of the row.
#[test]
fn making_automation_is_one_undo_and_takes_its_row_with_it() {
    let mut session = a_session();
    let rows = session.lanes().len();
    let address = ParamAddress::new("master/gain");
    session.create_automation(&address, "Master \u{2014} gain", 0);
    assert_eq!(session.lanes().len(), rows + 1, "a row of its own");

    session.undo();
    assert_eq!(
        session.lanes().len(),
        rows,
        "one undo takes the clip and the row it was made on"
    );
}

/// F7 and §15 decision 2. Saving names the project after its folder without
/// an undo entry; a Save As of a saved song is a new song that remembers its
/// parent; every save stamps the revision.
#[test]
fn saving_stamps_the_song_and_save_as_forks_it() {
    let dir = scratch("stamps");
    let projects = dir.join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut session = a_session();
    session.set_projects_dir(Some(projects.clone()));
    let born = session.project().meta.id;

    session.save_as("First").expect("saves");
    assert_eq!(session.name(), "First");
    assert_eq!(
        session.project().meta.id,
        born,
        "the first save of a new song is that song, not a fork of it"
    );
    assert_eq!(session.project().meta.saved_revision, 1);
    assert!(!session.project().meta.saved_by.is_empty());

    session.undo();
    assert_eq!(
        session.name(),
        "First",
        "the name follows the folder, not the undo stack"
    );

    session.save().expect("saves again");
    assert_eq!(session.project().meta.saved_revision, 2);
    let on_disk = fontelle_model::peek_meta(session.bundle_path().unwrap()).unwrap();
    assert_eq!(on_disk.saved_revision, 2, "the stamp is what is on disk");
    assert_eq!(on_disk.id, born);

    session.save_as("Second").expect("saves as");
    assert_ne!(session.project().meta.id, born, "a Save As is a new song");
    assert_eq!(session.project().meta.forked_from, Some(born));
    assert_eq!(
        fontelle_model::peek_meta(&projects.join("First.fontelle"))
            .unwrap()
            .id,
        born,
        "and the song it came from is still itself"
    );

    // The autosave is not a save.
    let revision = session.project().meta.saved_revision;
    session.set_tempo(133.0);
    session.autosave();
    assert_eq!(session.project().meta.saved_revision, revision);
    std::fs::remove_dir_all(&dir).ok();
}

fn a_bundle_at(path: &Path, id: Option<fontelle_types::PersistentId>) {
    let mut project = fontelle_model::Project::new("song");
    if let Some(id) = id {
        project.meta.id = id;
    }
    fontelle_model::save_project(&project, path).unwrap();
}

/// §4.2 (F19's lookup). A join finds the song by its id, in the projects
/// folder and in its *Shared* folder, whatever the bundles are called.
#[test]
fn a_song_is_found_by_its_id_wherever_it_is_and_whatever_it_is_called() {
    let dir = scratch("find");
    let id = fontelle_types::PersistentId::new();
    a_bundle_at(&dir.join("Mine.fontelle"), Some(id));
    a_bundle_at(&dir.join("Other.fontelle"), None);
    a_bundle_at(&dir.join("Shared").join("Theirs (2).fontelle"), Some(id));
    a_bundle_at(&dir.join("Shared").join("Unrelated.fontelle"), None);
    // Not a bundle: nothing to peek.
    std::fs::create_dir_all(dir.join("Not a song")).unwrap();
    std::fs::write(dir.join("notes.txt"), "hello").unwrap();

    let mut found = fontelle_app::find_by_id(&dir, id);
    found.sort();
    assert_eq!(
        found,
        vec![
            dir.join("Mine.fontelle"),
            dir.join("Shared").join("Theirs (2).fontelle")
        ]
    );
    assert!(fontelle_app::find_by_id(&dir, fontelle_types::PersistentId::new()).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ Phase 1
//
// Two studios in one process, over the engine's own in-memory hub (§12.2).
// Latency is in hub ticks; each tick pumps both studios once. Every session
// here runs strict: a hash on the wire that disagrees with the joiner's copy
// panics (F18).

use fontelle_app::Session;
use fontelle_app::collab::{CollabOptions, JoinAnswer, Relation};
use fontelle_net::MemoryHub;
use fontelle_types::{ClipId, NoteId, PPQN, PersistentId};
use fontelle_ui::canvas::{ArrangeEdit, RollEdit};

fn options(name: &str, install: PersistentId) -> CollabOptions {
    let mut options = CollabOptions::new(name, install);
    options.strict = true;
    options.idle_break = std::time::Duration::ZERO;
    options
}

/// Takes a test's folder away when the test is done with it.
struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

struct Pair {
    hub: MemoryHub,
    now: u64,
    latency: u64,
    host: Session,
    joiner: Session,
    alice: PersistentId,
    bob: PersistentId,
    dir: PathBuf,
    _cleanup: Cleanup,
}

impl Pair {
    /// Alice has saved "Song" and shares it; Bob, who has never seen it,
    /// joins. Nobody has answered the join's question yet.
    fn new(name: &str, latency: u64) -> Pair {
        let dir = scratch(name);
        let mut host = a_session();
        host.set_projects_dir(Some(dir.join("alice")));
        std::fs::create_dir_all(dir.join("alice")).unwrap();
        host.save_as("Song").unwrap();
        let mut joiner = a_session();
        std::fs::create_dir_all(dir.join("bob")).unwrap();
        joiner.set_projects_dir(Some(dir.join("bob")));
        let cleanup = Cleanup(dir.clone());
        Pair::between(host, joiner, dir, latency, cleanup)
    }

    fn between(
        mut host: Session,
        mut joiner: Session,
        dir: PathBuf,
        latency: u64,
        cleanup: Cleanup,
    ) -> Pair {
        let hub = MemoryHub::new();
        hub.set_conditions(latency, 0.0);
        let (alice, bob) = (PersistentId::derived("alice"), PersistentId::derived("bob"));
        host.share(Box::new(hub.server_endpoint()), options("Alice", alice))
            .expect("a saved song can be shared");
        joiner
            .join(Box::new(hub.connect()), options("Bob", bob))
            .expect("a studio with nothing unsaved can join");
        let mut pair = Pair {
            hub,
            now: 0,
            latency,
            host,
            joiner,
            alice,
            bob,
            dir,
            _cleanup: cleanup,
        };
        pair.settle();
        pair
    }

    /// [`Pair::new`], with Bob's copy made and open.
    fn joined(name: &str, latency: u64) -> Pair {
        let mut pair = Pair::new(name, latency);
        pair.answer(JoinAnswer::Copy);
        assert!(pair.joiner.collab_live(), "Bob has the song open");
        pair.same();
        pair
    }

    fn answer(&mut self, answer: JoinAnswer) {
        self.joiner
            .answer_join(answer)
            .expect("the answer is taken");
        self.settle();
    }

    fn tick(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.now += 1;
            self.hub.set_now(self.now);
            self.host.pump_collab();
            self.joiner.pump_collab();
        }
    }

    /// Long enough for anything sent to have gone there and back twice.
    fn settle(&mut self) {
        self.tick(self.latency * 6 + 12);
    }

    fn same(&self) {
        assert_eq!(
            self.host.project().sync_hash(),
            self.joiner.project().sync_hash(),
            "the two studios hold the same song"
        );
    }
}

fn a_note(start: i64, key: u8) -> fontelle_model::Note {
    fontelle_model::Note {
        start,
        length: PPQN,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

fn draw(session: &mut Session, start: i64, key: u8) -> NoteId {
    let ids = session.edit(RollEdit::Add {
        note: a_note(start, key),
    });
    session.end_gesture();
    ids[0]
}

fn notes_of(session: &Session, clip: ClipId) -> Vec<(NoteId, i64, u8)> {
    match session.project().clip_source(clip).as_deref() {
        Some(fontelle_model::ClipSource::Notes(data)) => data
            .notes
            .iter()
            .map(|(id, n)| (id, n.start, n.key))
            .collect(),
        _ => Vec::new(),
    }
}

fn open_clip(session: &Session) -> ClipId {
    Session::first_clip(session.project()).expect("the song has a clip")
}

/// F13. A joiner ends up with the host's song, and every edit either of them
/// makes after that lands on both, in one order.
#[test]
fn a_joiner_ends_up_with_the_hosts_document() {
    let mut pair = Pair::joined("same-document", 3);
    let clip = open_clip(&pair.host);
    for i in 0..5 {
        draw(&mut pair.host, i * PPQN, 60 + i as u8);
        pair.tick(1);
        draw(&mut pair.joiner, i * PPQN, 72 + i as u8);
        pair.tick(1);
    }
    pair.settle();
    pair.same();
    assert_eq!(notes_of(&pair.host, clip).len(), 10);
}

/// F14 and F3. A joiner's edit is on its own screen at once, and the host's
/// copy has it — under the same id — a round trip later.
#[test]
fn a_joiners_edit_is_instant_and_then_confirmed() {
    let mut pair = Pair::joined("instant", 10);
    let clip = open_clip(&pair.host);
    let drawn = draw(&mut pair.joiner, 0, 64);
    assert!(notes_of(&pair.joiner, clip).iter().any(|n| n.0 == drawn));
    pair.tick(3);
    assert!(
        !notes_of(&pair.host, clip).iter().any(|n| n.0 == drawn),
        "the host has not heard of it yet"
    );
    pair.settle();
    assert!(notes_of(&pair.host, clip).iter().any(|n| n.0 == drawn));
    pair.same();
}

/// F55. Draw a note and drag it before the host has heard of the first: the
/// drag names the id the drawing made, and it lands — while the host draws
/// in the same clip at the same moment.
#[test]
fn a_joiners_second_edit_can_name_what_its_first_made() {
    let mut pair = Pair::joined("draw-then-drag", 10);
    let clip = open_clip(&pair.host);
    draw(&mut pair.host, PPQN * 2, 40);
    let drawn = draw(&mut pair.joiner, 0, 80);
    pair.joiner.edit(RollEdit::Move {
        ids: vec![drawn],
        tick_delta: PPQN,
        key_delta: 0,
    });
    pair.joiner.end_gesture();
    pair.settle();
    pair.same();
    let host_notes = notes_of(&pair.host, clip);
    assert!(host_notes.contains(&(drawn, PPQN, 80)), "{host_notes:?}");
    assert!(host_notes.iter().any(|n| n.2 == 40));
    assert!(
        pair.joiner.take_collab_notices().is_empty(),
        "nothing was refused"
    );
}

/// F15. The host deletes the clip a joiner is moving notes in. The joiner's
/// move is refused, taken back off its screen, and said.
#[test]
fn a_conflicting_proposal_is_refused_and_rolled_back() {
    let mut pair = Pair::joined("conflict", 10);
    let clip = open_clip(&pair.host);
    let ids: Vec<NoteId> = (0..3)
        .map(|i| draw(&mut pair.joiner, i * PPQN, 60))
        .collect();
    pair.settle();

    pair.host.arrange(ArrangeEdit::Remove(vec![clip]));
    pair.host.end_gesture();
    pair.joiner.edit(RollEdit::Move {
        ids,
        tick_delta: PPQN,
        key_delta: 2,
    });
    pair.joiner.end_gesture();
    pair.settle();

    pair.same();
    assert!(pair.joiner.project().clips.get(clip).is_none());
    let notices = pair.joiner.take_collab_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("Move"), "{}", notices[0]);
}

/// F17. Two of the joiner's edits wait for the host while one of the host's
/// arrives between them; all three land, in the host's order, on both.
#[test]
fn pending_edits_are_rebased_over_a_foreign_one() {
    let mut pair = Pair::joined("rebase", 10);
    let clip = open_clip(&pair.host);
    let a = draw(&mut pair.joiner, 0, 60);
    let b = draw(&mut pair.joiner, PPQN, 62);
    pair.settle();

    pair.joiner.edit(RollEdit::Move {
        ids: vec![a],
        tick_delta: PPQN * 4,
        key_delta: 0,
    });
    pair.joiner.end_gesture();
    pair.tick(2);
    let c = draw(&mut pair.host, PPQN * 2, 50);
    pair.tick(2);
    pair.joiner.edit(RollEdit::Move {
        ids: vec![b],
        tick_delta: PPQN * 4,
        key_delta: 0,
    });
    pair.joiner.end_gesture();
    pair.settle();

    pair.same();
    let notes = notes_of(&pair.joiner, clip);
    assert!(notes.contains(&(a, PPQN * 4, 60)));
    assert!(notes.contains(&(b, PPQN * 5, 62)));
    assert!(notes.iter().any(|n| n.0 == c && n.2 == 50));
}

/// F17. A drag in the joiner's hand is not moved under it: what arrives from
/// the host while the button is down waits, and the drag lands whole.
#[test]
fn a_drag_in_progress_survives_a_foreign_edit() {
    let mut pair = Pair::joined("drag", 4);
    let clip = open_clip(&pair.host);
    let a = draw(&mut pair.joiner, 0, 60);
    pair.settle();

    // One pump per step of the drag, so the drag is never idle long enough
    // to be let go of on its own; the host's note arrives in the middle.
    for step in 0..12 {
        pair.joiner.edit(RollEdit::Move {
            ids: vec![a],
            tick_delta: 10,
            key_delta: 0,
        });
        if step == 1 {
            draw(&mut pair.host, PPQN * 3, 48);
        }
        pair.tick(1);
        if step == 8 {
            assert!(
                !notes_of(&pair.joiner, clip).iter().any(|n| n.2 == 48),
                "nothing is rebased under a drag in the hand"
            );
        }
    }
    pair.joiner.end_gesture();
    pair.settle();

    pair.same();
    let notes = notes_of(&pair.host, clip);
    assert!(notes.contains(&(a, 120, 60)), "{notes:?}");
    assert!(notes.iter().any(|n| n.2 == 48));
}

/// F16. A joiner's undo is an edit like any other: it is sent, and it takes
/// back the joiner's own edit on both screens. The host's undo has nothing of
/// the joiner's to take.
#[test]
fn undo_on_a_joiner_is_a_proposal() {
    let mut pair = Pair::joined("undo", 5);
    let clip = open_clip(&pair.host);
    let drawn = draw(&mut pair.joiner, 0, 64);
    pair.settle();
    assert!(notes_of(&pair.host, clip).iter().any(|n| n.0 == drawn));

    pair.joiner.undo();
    pair.settle();
    assert!(!notes_of(&pair.host, clip).iter().any(|n| n.0 == drawn));
    pair.same();

    let before = pair.host.project().sync_hash();
    pair.host.undo();
    pair.settle();
    assert_eq!(
        pair.host.project().sync_hash(),
        before,
        "not the host's to undo"
    );
    pair.same();
}

/// F16. Undoing something somebody else has since taken away is refused,
/// with a sentence, and changes nothing anywhere.
#[test]
fn undo_of_a_thing_someone_changed_is_refused_with_a_sentence() {
    let mut pair = Pair::joined("undo-refused", 5);
    let clip = open_clip(&pair.host);
    draw(&mut pair.joiner, 0, 64);
    pair.settle();
    pair.host.arrange(ArrangeEdit::Remove(vec![clip]));
    pair.host.end_gesture();
    pair.settle();

    let before = pair.joiner.project().sync_hash();
    pair.joiner.undo();
    pair.settle();
    assert_eq!(pair.joiner.project().sync_hash(), before);
    pair.same();
    let said = pair.joiner.take_message().unwrap_or_default();
    assert!(said.contains("changed"), "{said:?}");
}

// ------------------------------------------------------------- the join

fn count_files(dir: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                count += 1;
            }
        }
    }
    count
}

/// F19. A song Bob has never seen is copied into his *Shared* folder, under
/// its own id, after one question — and opened.
#[test]
fn the_join_copies_into_shared_when_the_id_is_unknown() {
    let mut pair = Pair::new("copy", 2);
    let question = pair
        .joiner
        .join_question()
        .expect("Bob is asked first")
        .clone();
    assert!(question.lines[0].contains("Song"), "{:?}", question.lines);
    assert!(question.lines[0].contains("Alice"), "{:?}", question.lines);
    assert_eq!(question.buttons[0].0, JoinAnswer::Copy);
    assert_eq!(question.buttons.last().unwrap().0, JoinAnswer::Cancel);
    assert!(
        !pair.dir.join("bob").join("Shared").exists(),
        "nothing is written before the answer"
    );

    pair.answer(JoinAnswer::Copy);
    let copy = pair.dir.join("bob").join("Shared").join("Song.fontelle");
    assert_eq!(pair.joiner.bundle_path(), Some(copy.as_path()));
    assert_eq!(
        fontelle_model::peek_meta(&copy).unwrap().id,
        pair.host.project().meta.id
    );
    pair.same();
}

/// §4.3. Cancelling writes nothing and ends the session.
#[test]
fn cancelling_a_join_writes_nothing() {
    let mut pair = Pair::new("cancel", 2);
    let before = count_files(&pair.dir.join("bob"));
    pair.answer(JoinAnswer::Cancel);
    assert!(!pair.joiner.collab_live());
    assert_eq!(count_files(&pair.dir.join("bob")), before);
    assert!(!pair.dir.join("bob").join("Shared").exists());
    assert!(
        pair.host.session_peers().is_empty(),
        "Bob is gone from Alice's list"
    );
}

/// §4.4 rule 3. Unsaved work is asked about before anything else: a studio
/// with changes nobody saved does not join.
#[test]
fn a_join_with_unsaved_work_is_refused_until_it_is_answered() {
    let hub = MemoryHub::new();
    let mut joiner = a_session();
    draw(&mut joiner, 0, 60);
    assert!(joiner.is_dirty());
    let refused = joiner.join(Box::new(hub.connect()), options("Bob", PersistentId::new()));
    assert!(refused.is_err());
    assert!(!joiner.collab_live());
}

/// Bob joins "Song", leaves, and joins it again later: the second time he
/// already has it.
fn a_second_join(name: &str, bob_edits: bool, alice_edits: bool) -> Pair {
    let mut pair = Pair::joined(name, 2);
    pair.joiner.leave_session();
    pair.settle();
    if bob_edits {
        draw(&mut pair.joiner, PPQN * 7, 90);
    }
    pair.joiner.save().unwrap();
    if alice_edits {
        draw(&mut pair.host, PPQN * 6, 30);
    }
    pair.host.save().unwrap();
    pair.host.leave_session();

    let Pair {
        host,
        joiner,
        dir,
        latency,
        _cleanup,
        ..
    } = pair;
    Pair::between(host, joiner, dir, latency, _cleanup)
}

/// F19. When Bob already has the song, he is asked, with three answers and
/// the safe one first.
#[test]
fn the_join_asks_when_the_id_is_known() {
    let pair = a_second_join("known", false, true);
    let question = pair.joiner.join_question().expect("asked").clone();
    let answers: Vec<&JoinAnswer> = question.buttons.iter().map(|(a, _)| a).collect();
    assert!(matches!(answers[0], JoinAnswer::Update(_)), "{answers:?}");
    assert!(matches!(answers[1], JoinAnswer::KeepBoth(_)));
    assert_eq!(answers[2], &JoinAnswer::Cancel);
    assert_eq!(question.relation, Some(Relation::YouAreBehind));
    assert_eq!(
        question.default, 0,
        "updating a copy that is behind is safe"
    );
}

/// F20. Updating keeps a backup of what was there and deletes nothing.
#[test]
fn update_keeps_a_backup_and_deletes_nothing() {
    let mut pair = a_second_join("update", false, true);
    let copy = pair.dir.join("bob").join("Shared").join("Song.fontelle");
    let before_files = count_files(&copy);
    let before_manifest = std::fs::read(copy.join("project.json")).unwrap();

    pair.answer(JoinAnswer::Update(copy.clone()));
    pair.same();
    assert!(count_files(&copy) > before_files, "a backup was added");
    let backups: Vec<PathBuf> = std::fs::read_dir(copy.join("backups"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("before-join-"))
        })
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        std::fs::read(backups[0].join("project.json")).unwrap(),
        before_manifest,
        "the backup is what was there"
    );
    assert_eq!(pair.joiner.bundle_path(), Some(copy.as_path()));
}

/// F20. Keeping both makes Bob's copy a song of its own and copies Alice's
/// in beside it.
#[test]
fn keep_both_forks_the_local_copy() {
    let mut pair = a_second_join("keep-both", true, true);
    let copy = pair.dir.join("bob").join("Shared").join("Song.fontelle");
    let shared = pair.host.project().meta.id;

    pair.answer(JoinAnswer::KeepBoth(copy.clone()));
    let mine = fontelle_model::peek_meta(&copy).unwrap();
    assert_ne!(mine.id, shared, "Bob's is its own song now");
    assert_eq!(mine.forked_from, Some(shared));
    let found = fontelle_app::find_by_id(&pair.dir.join("bob"), shared);
    assert_eq!(found.len(), 1);
    assert_ne!(found[0], copy);
    assert_eq!(pair.joiner.bundle_path(), Some(found[0].as_path()));
    pair.same();
}

/// F21. When both copies changed since they last agreed, the question says
/// so and the safe answer is to keep both.
#[test]
fn both_changed_is_said_and_keep_both_is_the_default() {
    let pair = a_second_join("both-changed", true, true);
    let question = pair.joiner.join_question().expect("asked").clone();
    assert_eq!(question.relation, Some(Relation::BothChanged));
    assert!(
        question
            .lines
            .iter()
            .any(|l| l.contains("both have changed")),
        "{:?}",
        question.lines
    );
    assert!(matches!(
        question.buttons[question.default].0,
        JoinAnswer::KeepBoth(_)
    ));
}

/// F21. Leaving writes what the two copies agreed on, on both sides.
#[test]
fn leaving_writes_shared_revision_on_both_sides() {
    let mut pair = Pair::joined("leaving", 2);
    draw(&mut pair.joiner, 0, 61);
    pair.settle();
    let agreed = pair.host.project().sync_hash();
    pair.joiner.leave_session();
    pair.settle();
    assert_eq!(
        pair.joiner.project().meta.shared_revision,
        Some((pair.alice, agreed))
    );
    assert_eq!(
        pair.host.project().meta.shared_revision,
        Some((pair.bob, agreed))
    );
    assert!(!pair.joiner.collab_live(), "Bob carries on alone");
    assert!(pair.host.session_peers().is_empty());
}

/// F22. Two different Fontelles do not share, and the sentence says who has
/// which and what to do.
#[test]
fn a_version_mismatch_is_refused_naming_both() {
    let dir = scratch("versions");
    let mut host = a_session();
    host.set_projects_dir(Some(dir.join("alice")));
    std::fs::create_dir_all(dir.join("alice")).unwrap();
    host.save_as("Song").unwrap();
    let mut joiner = a_session();
    joiner.set_projects_dir(Some(dir.join("bob")));
    std::fs::create_dir_all(dir.join("bob")).unwrap();

    let hub = MemoryHub::new();
    let mut theirs = options("Alice", PersistentId::new());
    theirs.fontelle = "0.16.0".into();
    let mut mine = options("Bob", PersistentId::new());
    mine.fontelle = "0.15.0".into();
    host.share(Box::new(hub.server_endpoint()), theirs).unwrap();
    joiner.join(Box::new(hub.connect()), mine).unwrap();
    for tick in 1..20 {
        hub.set_now(tick);
        host.pump_collab();
        joiner.pump_collab();
    }
    let why = joiner.collab_ended().expect("the join ended");
    assert!(why.contains("0.16.0") && why.contains("0.15.0"), "{why}");
    assert!(why.contains("Alice"), "{why}");
    assert!(why.contains("update"), "{why}");
    assert!(joiner.join_question().is_none());
    assert_eq!(count_files(&dir.join("bob")), 0, "nothing was written");
    std::fs::remove_dir_all(&dir).ok();
}

/// F21's edge: an edit made a moment before leaving — still in the hand,
/// even — goes out before the goodbye, so the two copies part agreeing.
#[test]
fn an_edit_made_just_before_leaving_still_arrives() {
    let mut pair = Pair::joined("last-edit", 3);
    let clip = open_clip(&pair.host);
    let a = draw(&mut pair.joiner, 0, 61);
    pair.settle();
    // A drag still in the hand when Leave is pressed.
    pair.joiner.edit(RollEdit::Move {
        ids: vec![a],
        tick_delta: PPQN,
        key_delta: 0,
    });
    pair.joiner.leave_session();
    pair.settle();
    assert!(notes_of(&pair.host, clip).contains(&(a, PPQN, 61)));
    pair.same();
    assert_eq!(
        pair.host.project().meta.shared_revision.map(|(_, h)| h),
        pair.joiner.project().meta.shared_revision.map(|(_, h)| h),
        "both remember the same song"
    );

    // And the host's last edit before it stops sharing reaches the joiner
    // that is still there.
    let mut pair = Pair::joined("last-edit-host", 3);
    draw(&mut pair.host, 0, 33);
    pair.host.leave_session();
    pair.settle();
    assert!(notes_of(&pair.joiner, clip).iter().any(|n| n.2 == 33));
    pair.same();
}
