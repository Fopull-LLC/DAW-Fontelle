//! Analyze Musically's studies in the document (`docs/analyze-musically-plan.md`
//! §3.9, §4 P2): what a person did to a piece of audio — pitch edits by span
//! of samples, the render they made — kept with the song, through commands
//! like every other edit (INVARIANT 9), one undo each, a drag one entry.

use fontelle_model::{
    AddAudioClip, AddStudy, ClipSource, Command, History, Project, RemoveClip, RemoveStudy,
    SetAudioClip, SetStudyEdits, SetStudyRender,
};
use fontelle_types::{AssetKind, AssetRef, AudioClipData, PPQN, PitchEdit, Study, StudySource};

fn an_asset(name: &str) -> AssetRef {
    AssetRef {
        id: fontelle_types::AssetId::default(),
        path: name.into(),
        content_hash: 7,
        size: 1024,
        kind: AssetKind::Sample,
    }
}

/// A song with one audio clip, and a study of it.
fn a_song() -> (Project, fontelle_types::ClipId, fontelle_types::StudyId) {
    let mut project = Project::new("studies");
    let mut add = AddAudioClip::new(
        "Vox",
        AudioClipData::whole(an_asset("Vox.wav"), 96_000, 48_000),
        0,
        PPQN * 8,
    );
    add.apply(&mut project).expect("the clip lands");
    let clip = add.clip().expect("a clip");
    let mut study = AddStudy::new(Study::new(
        "Vox",
        StudySource::Clip(clip),
        an_asset("Vox.wav"),
    ));
    study.apply(&mut project).expect("the study lands");
    let id = study.id().expect("a study");
    (project, clip, id)
}

fn moved(span: (i64, i64), cents: f32) -> PitchEdit {
    PitchEdit {
        shift_cents: cents,
        ..PitchEdit::none(span)
    }
}

#[test]
fn a_study_is_added_and_undone() {
    let (mut project, clip, id) = a_song();
    let study = &project.studies[id];
    assert_eq!(study.source, StudySource::Clip(clip));
    assert!(study.pitch_edits.is_empty());
    assert!(study.rendered.is_none());
    let mut add = AddStudy::new(Study::new(
        "Again",
        StudySource::Standalone,
        an_asset("a.wav"),
    ));
    add.apply(&mut project).unwrap();
    let made = add.id().unwrap();
    add.invert().apply(&mut project).unwrap();
    assert!(!project.studies.contains_key(made));
    // And a removal is undone under the same id.
    let mut remove = RemoveStudy::new(id);
    remove.apply(&mut project).unwrap();
    assert!(!project.studies.contains_key(id));
    remove.invert().apply(&mut project).unwrap();
    assert_eq!(project.studies[id].name, "Vox");
}

#[test]
fn edits_apply_and_invert() {
    let (mut project, _, id) = a_song();
    let edits = vec![moved((4_800, 24_000), 30.0)];
    let mut set = SetStudyEdits::new(id, edits.clone());
    set.apply(&mut project).unwrap();
    assert_eq!(project.studies[id].pitch_edits, edits);
    set.invert().apply(&mut project).unwrap();
    assert!(project.studies[id].pitch_edits.is_empty());
}

/// A drag is a run of edits, and one entry: one undo puts the note back
/// where it was before the drag began, not one step of it.
#[test]
fn a_drag_is_one_undo() {
    let (mut project, _, id) = a_song();
    let mut history = History::new();
    for cents in [10.0, 40.0, 70.0, 100.0] {
        history
            .apply(
                Box::new(SetStudyEdits::new(id, vec![moved((0, 9_600), cents)])),
                &mut project,
            )
            .unwrap();
    }
    assert_eq!(history.depth(), 1, "four steps of one drag merged");
    assert_eq!(project.studies[id].pitch_edits[0].shift_cents, 100.0);
    history.undo(&mut project).unwrap().unwrap();
    assert!(project.studies[id].pitch_edits.is_empty());
    // Another study's edits do not merge into it.
    history.break_gesture();
    history
        .apply(
            Box::new(SetStudyEdits::new(id, vec![moved((0, 9_600), 5.0)])),
            &mut project,
        )
        .unwrap();
    let mut other = AddStudy::new(Study::new("B", StudySource::Standalone, an_asset("b.wav")));
    other.apply(&mut project).unwrap();
    let before = history.depth();
    history
        .apply(
            Box::new(SetStudyEdits::new(
                other.id().unwrap(),
                vec![moved((0, 10), 5.0)],
            )),
            &mut project,
        )
        .unwrap();
    assert_eq!(history.depth(), before + 1);
}

/// Render to clip (plan §3.7): the clip's audio swapped and the study
/// stamped, as one compound, so one undo puts the original back.
#[test]
fn a_render_swaps_the_clip_and_one_undo_puts_it_back() {
    let (mut project, clip, id) = a_song();
    let ClipSource::Audio(before) = project.clips[clip].source.clone() else {
        panic!("audio");
    };
    let rendered = an_asset("renders/Vox (edited 1).wav");
    let mut data = before.clone();
    data.asset = rendered.clone();
    let mut history = History::new();
    history
        .apply(
            Box::new(fontelle_model::Compound::new(
                "Render edits to Vox",
                vec![
                    Box::new(SetAudioClip::new(clip, data)),
                    Box::new(SetStudyRender::new(id, Some(rendered.clone()))),
                ],
            )),
            &mut project,
        )
        .unwrap();
    let ClipSource::Audio(now) = &project.clips[clip].source else {
        panic!("audio");
    };
    assert_eq!(now.asset, rendered);
    assert_eq!(project.studies[id].rendered, Some(rendered));
    // The original is kept, whatever the clip plays.
    assert_eq!(project.studies[id].original, before.asset);
    assert_eq!(history.depth(), 1);
    history.undo(&mut project).unwrap().unwrap();
    let ClipSource::Audio(back) = &project.clips[clip].source else {
        panic!("audio");
    };
    assert_eq!(back.asset, before.asset);
    assert!(project.studies[id].rendered.is_none());
}

/// A clip deleted leaves its study standalone — never a dangling id, never
/// lost — and the undo puts the link back.
#[test]
fn deleting_the_clip_leaves_its_study_standalone() {
    let (mut project, clip, id) = a_song();
    let mut remove = RemoveClip::new(clip);
    remove.apply(&mut project).unwrap();
    assert_eq!(project.studies[id].source, StudySource::Standalone);
    remove.invert().apply(&mut project).unwrap();
    assert_eq!(project.studies[id].source, StudySource::Clip(clip));
}

/// The study's files are the song's files: a save collects the original
/// and the render into the bundle, and another machine is sent them.
#[test]
fn a_studys_files_are_the_songs() {
    let (mut project, _, id) = a_song();
    SetStudyRender::new(id, Some(an_asset("renders/r.wav")))
        .apply(&mut project)
        .unwrap();
    let files: Vec<_> = project.files().into_iter().map(|f| f.path).collect();
    assert!(files.contains(&"renders/r.wav".into()), "{files:?}");
    let mut seen = 0;
    project.each_file_mut(|file| {
        if file.path == std::path::Path::new("renders/r.wav") {
            seen += 1;
        }
    });
    assert_eq!(seen, 1);
}

/// A song from before studies opens with none, and one with edits reopens
/// with them.
#[test]
fn studies_are_saved_with_the_song_and_older_songs_open() {
    let (mut project, _, id) = a_song();
    SetStudyEdits::new(id, vec![moved((100, 2_000), -20.0)])
        .apply(&mut project)
        .unwrap();
    let mut json = serde_json::to_value(&project).unwrap();
    let back: Project = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(
        back.studies[id].pitch_edits,
        project.studies[id].pitch_edits
    );
    json.as_object_mut().unwrap().remove("studies");
    let older: Project = serde_json::from_value(json).unwrap();
    assert!(older.studies.is_empty());
}

/// The first move of a clip's note starts its study; the rest of that drag
/// merges into it, and one undo takes the study and the move back.
#[test]
fn a_drag_that_starts_a_study_is_one_undo() {
    let (mut project, clip, _) = a_song();
    let mut history = History::new();
    let mut first = Study::new("Vox 2", StudySource::Clip(clip), an_asset("Vox.wav"));
    first.pitch_edits = vec![moved((0, 9_600), 10.0)];
    history
        .apply(Box::new(AddStudy::new(first)), &mut project)
        .unwrap();
    let id = history
        .last_applied()
        .and_then(|c| c.as_any().downcast_ref::<AddStudy>())
        .and_then(AddStudy::id)
        .unwrap();
    for cents in [50.0, 90.0] {
        history
            .apply(
                Box::new(SetStudyEdits::new(id, vec![moved((0, 9_600), cents)])),
                &mut project,
            )
            .unwrap();
    }
    assert_eq!(history.depth(), 1);
    history.undo(&mut project).unwrap().unwrap();
    assert!(!project.studies.contains_key(id));
    history.redo(&mut project).unwrap().unwrap();
    assert_eq!(project.studies[id].pitch_edits[0].shift_cents, 90.0);
}
