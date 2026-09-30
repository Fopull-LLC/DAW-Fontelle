//! Every mixer track has a colour of its own.
//!
//! > *"it might be worth adding in a feature that highlights items that are
//! > linked to a mixer track so people can better see what is what."*
//!
//! Ty chose to answer that partly with colour: a channel's route chip is its
//! mixer track's colour, and a strip's sources glow in it
//! (`docs/ux-routing-and-learning-plan.md` §3–4). Every track used to be the
//! same grey, which would make a colour say nothing. So a new track takes
//! the next colour from a fixed palette, a person can change it, and a song
//! saved before any of this opens with its grey tracks coloured.

use fontelle_model::{
    AddMixerTrack, Command, LEGACY_TRACK_GREY, Project, RemoveMixerTrack, SetMixerTrackColor,
    TRACK_PALETTE, load_project, save_project,
};
use fontelle_types::MixerTrackId;

fn add(project: &mut Project, name: &str) -> MixerTrackId {
    let mut command = AddMixerTrack::new(name);
    command.apply(project).expect("it applies");
    command.track().expect("it made one")
}

fn color(project: &Project, id: MixerTrackId) -> [u8; 4] {
    project.mixer.tracks.get(id).expect("the track").color
}

#[test]
fn new_tracks_take_different_colours_from_the_palette() {
    let mut project = Project::new("colours");
    let ids: Vec<_> = (0..5)
        .map(|n| add(&mut project, &format!("T{n}")))
        .collect();
    let colors: Vec<_> = ids.iter().map(|id| color(&project, *id)).collect();
    for c in &colors {
        assert!(TRACK_PALETTE.contains(c), "{c:?} is from the palette");
        assert_ne!(*c, LEGACY_TRACK_GREY, "and not the old grey");
    }
    let mut distinct = colors.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), colors.len(), "no two alike: {colors:?}");
}

#[test]
fn a_colour_freed_by_a_deleted_track_is_the_next_one_used() {
    // The least-used colour, not the next in a counter: delete the second of
    // three and the fourth track fills the gap rather than bunching up.
    let mut project = Project::new("gaps");
    let a = add(&mut project, "A");
    let b = add(&mut project, "B");
    let c = add(&mut project, "C");
    let freed = color(&project, b);
    RemoveMixerTrack::new(b).apply(&mut project).unwrap();
    let d = add(&mut project, "D");
    assert_eq!(color(&project, d), freed);
    assert_ne!(color(&project, d), color(&project, a));
    assert_ne!(color(&project, d), color(&project, c));
}

#[test]
fn a_palette_used_up_starts_again_rather_than_running_out() {
    let mut project = Project::new("many");
    let ids: Vec<_> = (0..TRACK_PALETTE.len() + 3)
        .map(|n| add(&mut project, &format!("T{n}")))
        .collect();
    assert!(
        ids.iter()
            .all(|id| TRACK_PALETTE.contains(&color(&project, *id)))
    );
    assert_eq!(color(&project, ids[TRACK_PALETTE.len()]), TRACK_PALETTE[0]);
}

#[test]
fn the_colour_is_the_same_on_every_peer_and_after_an_undo_and_a_redo() {
    // An edit crosses the wire and is applied again on the other side, and a
    // redo applies it again here: the colour it chose travels with it.
    let mut here = Project::new("peers");
    let a = add(&mut here, "A");
    let mut there = here.clone();
    let mut command = AddMixerTrack::new("B");
    command.apply(&mut here).unwrap();
    let id = command.track().unwrap();
    let chosen = color(&here, id);

    // The other side has recoloured A to the colour B was given here, so a
    // choice remade there would land somewhere else.
    SetMixerTrackColor::new(a, chosen)
        .apply(&mut there)
        .unwrap();
    assert_ne!(there.mixer.next_track_color(), chosen);
    let wire = serde_json::to_string(&command.to_edit()).unwrap();
    let edit: fontelle_model::wire::Edit = serde_json::from_str(&wire).unwrap();
    edit.into_command().apply(&mut there).unwrap();
    assert_eq!(color(&there, id), chosen, "the other peer agrees");

    let undo = command.invert();
    let mut undo = undo;
    undo.apply(&mut here).unwrap();
    let mut redo = undo.invert();
    redo.apply(&mut here).unwrap();
    assert_eq!(color(&here, id), chosen, "and so does a redo");
}

#[test]
fn a_track_can_be_recoloured_and_the_undo_puts_it_back() {
    let mut project = Project::new("recolour");
    let id = add(&mut project, "Bus");
    let was = color(&project, id);
    let mut set = SetMixerTrackColor::new(id, TRACK_PALETTE[3]);
    set.apply(&mut project).unwrap();
    assert_eq!(color(&project, id), TRACK_PALETTE[3]);
    set.invert().apply(&mut project).unwrap();
    assert_eq!(color(&project, id), was);
}

#[test]
fn a_song_saved_with_grey_tracks_opens_with_them_coloured() {
    let mut project = Project::new("old song");
    let ids: Vec<_> = (0..3)
        .map(|n| add(&mut project, &format!("T{n}")))
        .collect();
    // As every track was before this existed.
    for id in &ids {
        project.mixer.tracks.get_mut(*id).unwrap().color = LEGACY_TRACK_GREY;
    }
    let chosen = TRACK_PALETTE[5];
    SetMixerTrackColor::new(ids[1], chosen)
        .apply(&mut project)
        .unwrap();
    let bundle = std::env::temp_dir().join(format!(
        "fontelle-track-colors-{}.fontelle",
        std::process::id()
    ));
    std::fs::remove_dir_all(&bundle).ok();
    save_project(&project, &bundle).unwrap();
    let opened = load_project(&bundle).unwrap();
    std::fs::remove_dir_all(&bundle).ok();

    assert_eq!(
        color(&opened, ids[1]),
        chosen,
        "a colour somebody chose stays"
    );
    let (a, c) = (color(&opened, ids[0]), color(&opened, ids[2]));
    assert!(TRACK_PALETTE.contains(&a) && TRACK_PALETTE.contains(&c));
    assert!(
        a != c && a != chosen && c != chosen,
        "{a:?} {c:?} {chosen:?}"
    );
    let master = opened.mixer.master.unwrap();
    assert_eq!(
        color(&opened, master),
        color(&project, master),
        "the master keeps its own"
    );
}
