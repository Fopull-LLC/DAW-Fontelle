//! Mixer tracks have an order a person chooses, and can be duplicated.
//!
//! > *"please make it so you can right click and duplicate mixer tracks and
//! > easily reorder them."*
//!
//! The strips used to stand in the arena's own order, which is insertion
//! order with deleted places reused — nothing a person could change, and not
//! even stable across a delete. A track carries an `order` now, the way a
//! lane does (`Lane::order`), and [`Mixer::ordered_tracks`] is the one list
//! everything that numbers strips reads.

use fontelle_model::{
    AddInsert, AddMixerTrack, AddSend, Command, DuplicateMixerTrack, MoveMixerTrack, Project,
    RemoveMixerTrack, load_project, save_project,
};
use fontelle_types::{EffectKind, MixerTrackId};

fn add(project: &mut Project, name: &str) -> MixerTrackId {
    let mut command = AddMixerTrack::new(name);
    command.apply(project).expect("it applies");
    command.track().expect("it made one")
}

fn names(project: &Project) -> Vec<String> {
    project
        .mixer
        .ordered_tracks()
        .into_iter()
        .map(|id| project.mixer.tracks[id].name.clone())
        .collect()
}

fn three() -> Project {
    let mut project = Project::new("order");
    for name in ["A", "B", "C"] {
        add(&mut project, name);
    }
    project
}

#[test]
fn tracks_stand_in_the_order_they_were_made_and_the_master_is_not_one_of_them() {
    let project = three();
    assert_eq!(names(&project), ["A", "B", "C"]);
    let master = project.mixer.master;
    assert!(
        !project
            .mixer
            .ordered_tracks()
            .iter()
            .any(|id| Some(*id) == master),
        "the master is pinned apart, never reordered"
    );
}

#[test]
fn a_track_moves_to_where_it_was_dropped_and_undo_puts_it_back() {
    let mut project = three();
    let mut command = MoveMixerTrack::new(0, 2);
    command.apply(&mut project).expect("it moves");
    assert_eq!(names(&project), ["B", "C", "A"]);

    command.invert().apply(&mut project).expect("it undoes");
    assert_eq!(names(&project), ["A", "B", "C"]);

    MoveMixerTrack::new(2, 0).apply(&mut project).expect("left");
    assert_eq!(names(&project), ["C", "A", "B"]);
}

#[test]
fn a_move_off_the_end_is_a_no_op_not_an_error() {
    let mut project = three();
    MoveMixerTrack::new(1, 9)
        .apply(&mut project)
        .expect("refusing a drop past the end would be a gesture somebody has to handle");
    assert_eq!(names(&project), ["A", "B", "C"]);
    MoveMixerTrack::new(9, 0)
        .apply(&mut project)
        .expect("no-op");
    assert_eq!(names(&project), ["A", "B", "C"]);
}

#[test]
fn a_new_track_lands_at_the_end_after_a_reorder() {
    let mut project = three();
    MoveMixerTrack::new(2, 0)
        .apply(&mut project)
        .expect("moves");
    add(&mut project, "D");
    assert_eq!(names(&project), ["C", "A", "B", "D"]);
}

#[test]
fn the_order_survives_a_save() {
    let mut project = three();
    MoveMixerTrack::new(0, 2)
        .apply(&mut project)
        .expect("moves");
    let dir = std::env::temp_dir().join(format!("fontelle-mixer-order-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("song.fontelle");
    save_project(&project, &path).expect("saves");
    let back = load_project(&path).expect("loads");
    assert_eq!(names(&back), ["B", "C", "A"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_song_saved_before_tracks_had_an_order_keeps_the_order_it_had() {
    // Every track in an old file carries the default; the sort is stable, so
    // ties keep the arena's order — the order those strips always stood in.
    let project = three();
    let mut json = serde_json::to_value(&project.mixer).unwrap();
    // The arena writes `[key, value]` pairs.
    for entry in json["tracks"].as_array_mut().expect("a list of tracks") {
        entry[1].as_object_mut().expect("a track").remove("order");
    }
    let mixer: fontelle_model::Mixer = serde_json::from_value(json).expect("an old mixer reads");
    let mut old = project.clone();
    old.mixer = mixer;
    assert_eq!(names(&old), ["A", "B", "C"]);
}

#[test]
fn duplicating_a_track_copies_its_mix_and_lands_beside_it() {
    let mut project = three();
    let b = project.mixer.ordered_tracks()[1];
    let c = project.mixer.ordered_tracks()[2];
    project.mixer.tracks[b].gain_db = -7.5;
    AddInsert::new(b, EffectKind::Reverb)
        .apply(&mut project)
        .expect("an insert");
    AddSend::new(b, c).apply(&mut project).expect("a send");

    let mut command = DuplicateMixerTrack::new(b);
    command.apply(&mut project).expect("it duplicates");
    let copy = command.track().expect("it made one");

    assert_eq!(names(&project), ["A", "B", "B copy", "C"]);
    let (original, made) = (&project.mixer.tracks[b], &project.mixer.tracks[copy]);
    assert_eq!(made.gain_db, -7.5, "the copy sounds the same");
    assert_eq!(made.inserts.len(), 1);
    assert_eq!(made.sends.len(), 1);
    assert_eq!(made.sends[0].target, c);
    assert_eq!(made.output, original.output);
    assert_eq!(made.color, original.color);
    assert_ne!(
        made.inserts[0].id, original.inserts[0].id,
        "an insert is addressed by its id; two answering to one is a knob \
         that turns both"
    );
    assert_ne!(made.sends[0].id, original.sends[0].id);
    assert!(!project.mixer.has_cycle());
}

#[test]
fn undoing_a_duplicate_takes_the_copy_away_and_redo_brings_back_the_same_one() {
    let mut project = three();
    let a = project.mixer.ordered_tracks()[0];
    AddInsert::new(a, EffectKind::Reverb)
        .apply(&mut project)
        .expect("an insert");
    let mut command = DuplicateMixerTrack::new(a);
    command.apply(&mut project).expect("it duplicates");
    let copy = command.track().unwrap();
    let insert = project.mixer.tracks[copy].inserts[0].id;

    command.invert().apply(&mut project).expect("undo");
    assert_eq!(names(&project), ["A", "B", "C"]);

    command.apply(&mut project).expect("redo");
    assert_eq!(names(&project), ["A", "A copy", "B", "C"]);
    assert_eq!(
        project.mixer.tracks[copy].inserts[0].id, insert,
        "a redo, and the far end of a shared song, make the same track"
    );
}

#[test]
fn the_master_is_not_duplicated_or_moved() {
    let mut project = three();
    let master = project.mixer.master.expect("a master");
    assert!(
        DuplicateMixerTrack::new(master)
            .apply(&mut project)
            .is_err()
    );
    assert_eq!(names(&project), ["A", "B", "C"]);
}

#[test]
fn deleting_a_track_keeps_the_others_in_their_order() {
    let mut project = three();
    MoveMixerTrack::new(2, 0)
        .apply(&mut project)
        .expect("moves");
    let a = project.mixer.ordered_tracks()[1];
    let mut remove = RemoveMixerTrack::new(a);
    remove.apply(&mut project).expect("removes");
    assert_eq!(names(&project), ["C", "B"]);
    remove.invert().apply(&mut project).expect("restores");
    assert_eq!(names(&project), ["C", "A", "B"]);
}
