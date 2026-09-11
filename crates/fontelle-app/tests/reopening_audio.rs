//! What happens to an audio clip when the project is closed and opened again.
//!
//! > *"audio clips, after closing the project and re opening, often would just
//! > be blank after that point."*
//!
//! **A clip addresses its audio by id.** `AudioClipData::asset` carries an
//! `AssetId`, and both of the things that make a clip look like anything read
//! the library by that id: `SampleLibrary::audio_peaks` draws the waveform in
//! the block, and `AudioStore::get` is what the player reads. A reopened
//! project whose library has neither is a block with nothing in it that makes
//! no sound — which is exactly what "blank" is.
//!
//! `open_project` reloaded the samples every **channel's patch** named and
//! nothing else, so it walked `project.channels` and never looked at
//! `project.clips`. Every audio clip in every saved project came back empty,
//! and quietly: a clip whose file could not be found is reported (§17.4's
//! broken link), and one nobody tried to load is not.
//!
//! A patch gets away with a fresh id because it stores its layers'
//! *provenance* and resolves them by file on load (`SampleLibrary::resolve`,
//! and the note in `reload_sample`). A clip has no such indirection, so this
//! is the other rule: **a clip's audio comes back under the id the project
//! wrote down**, or the clip is pointing at nothing.

use std::path::{Path, PathBuf};

use fontelle_app::{SampleLibrary, open_project, save_project};
use fontelle_assets::fixtures::build_wav;
use fontelle_model::{Clip, ClipSource, Project};
use fontelle_types::{AssetRef, AudioClipData, PPQN};

const SR: u32 = 48_000;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-reopen-audio-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// Half a second of a loud 220 Hz tone, on disk where a project can name it.
fn a_take(dir: &Path, name: &str) -> PathBuf {
    let samples: Vec<f32> = (0..24_000)
        .map(|i| (i as f32 * 220.0 * std::f32::consts::TAU / SR as f32).sin() * 0.8)
        .collect();
    let path = dir.join(name);
    std::fs::write(&path, build_wav(SR, 1, &samples)).expect("writable");
    path
}

/// A project with one audio clip on it, imported the way a drop does.
fn a_project_with_a_take(dir: &Path, name: &str) -> (Project, SampleLibrary, AssetRef) {
    let mut project = fontelle_app::blank_project(8, 120.0, SR);
    let mut library = SampleLibrary::new();
    let imported = library
        .import_audio(&a_take(dir, name))
        .expect("a wav this test just wrote");
    let lane = project
        .lane_ids()
        .first()
        .copied()
        .expect("a blank project has lanes");
    let asset = imported.asset.clone();
    project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Audio(AudioClipData::whole(
            asset.clone(),
            imported.frames as i64,
            imported.sample_rate,
        )),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    (project, library, asset)
}

#[test]
fn a_reopened_project_still_has_the_audio_its_clips_name() {
    let dir = scratch("plays");
    let bundle = dir.join("Song.fontelle");
    let (project, library, asset) = a_project_with_a_take(&dir, "Take.wav");

    // Before the save it plays — that is `audio_import.rs`'s ground.
    assert!(
        library.audio_store().get(asset.id).is_some(),
        "the fixture itself is wrong"
    );

    save_project(&project, &bundle).expect("a bundle is writable");
    let opened = open_project(&bundle).expect("the project opens again");

    assert!(
        opened.library.audio_store().get(asset.id).is_some(),
        "the clip's audio did not come back: a reopened take is silent"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_reopened_clip_still_draws_its_waveform() {
    // The half of "blank" you can see. §15.3's peaks are keyed by the same id,
    // so a block with no peaks is a block with nothing drawn in it.
    let dir = scratch("draws");
    let bundle = dir.join("Song.fontelle");
    let (project, _library, asset) = a_project_with_a_take(&dir, "Loop.wav");

    save_project(&project, &bundle).expect("a bundle is writable");
    let opened = open_project(&bundle).expect("the project opens again");

    let peaks = opened
        .library
        .audio_peaks(asset.id)
        .expect("the reopened clip has no waveform to draw");
    assert!(peaks.frames > 0, "the summary is empty");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_audio_comes_back_under_the_id_the_project_wrote_down() {
    // Not *a* buffer — **that** one. A library that decoded the file under a
    // fresh id would hold the audio and still leave every clip pointing at
    // nothing, which is the same blank block with a fuller library behind it.
    let dir = scratch("same-id");
    let bundle = dir.join("Song.fontelle");
    let (project, _library, asset) = a_project_with_a_take(&dir, "Take.wav");
    save_project(&project, &bundle).expect("a bundle is writable");

    let opened = open_project(&bundle).expect("the project opens again");
    let store = opened.library.audio_store();
    let buffer = store
        .get(asset.id)
        .expect("nothing under the clip's own id");
    assert_eq!(buffer.sample_rate, SR);
    assert!(
        buffer.data.iter().any(|s| s.abs() > 0.5),
        "the buffer under that id is not the take"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn two_clips_on_one_file_share_the_one_decode() {
    // A loop dropped on two rows is two clips naming one asset. Loading it
    // twice would be two copies of the audio in memory for no reason, and the
    // import path has deduped by path since it was written.
    let dir = scratch("shared");
    let bundle = dir.join("Song.fontelle");
    let (mut project, _library, asset) = a_project_with_a_take(&dir, "Loop.wav");
    let lanes = project.lane_ids();
    let second = *lanes
        .get(1)
        .expect("a blank project has more than one lane");
    let clip = project
        .clips
        .values()
        .next()
        .expect("the take is on the project")
        .clone();
    project.clips.insert(Clip {
        lane: second,
        start: PPQN * 4,
        ..clip
    });

    save_project(&project, &bundle).expect("a bundle is writable");
    let opened = open_project(&bundle).expect("the project opens again");

    assert!(opened.library.audio_store().get(asset.id).is_some());
    assert_eq!(
        opened.library.audio_store().len(),
        1,
        "one file behind two clips was decoded twice"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_take_whose_file_has_gone_is_reported_rather_than_dropped() {
    // §17.4: a broken link is a normal condition, and the project opens with
    // the reference intact so the next save does not lose it. What must *not*
    // happen is the silence this bug had — nobody tried to load it, so nobody
    // could say it was missing.
    let dir = scratch("missing");
    let bundle = dir.join("Song.fontelle");
    let (project, _library, asset) = a_project_with_a_take(&dir, "Gone.wav");
    save_project(&project, &bundle).expect("a bundle is writable");
    std::fs::remove_file(&asset.path).expect("the take was there a moment ago");

    let opened = open_project(&bundle).expect("a missing take is not a failure to open");
    assert!(
        opened.missing.iter().any(|m| m.file.path == asset.path),
        "the take vanished and the project said nothing"
    );
    assert!(
        opened.project.clips.values().any(|clip| matches!(
            &clip.source,
            ClipSource::Audio(data) if data.asset.path == asset.path
        )),
        "the reference was dropped, so saving again would lose it"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_imported_after_reopening_does_not_take_a_reopened_clips_id() {
    // The trap behind the fix. Ids are minted by the library, and a fresh
    // library mints from the bottom — so a reopened clip holding id 0 and a
    // library that then hands id 0 to the next drop would put the *new* file's
    // audio under the *old* clip's id, and the take would turn into the loop.
    let dir = scratch("collision");
    let bundle = dir.join("Song.fontelle");
    let (project, _library, take) = a_project_with_a_take(&dir, "Take.wav");
    save_project(&project, &bundle).expect("a bundle is writable");
    let mut opened = open_project(&bundle).expect("the project opens again");

    // A different sound: silence, so the two are told apart by content.
    let quiet = dir.join("Quiet.wav");
    std::fs::write(&quiet, build_wav(SR, 1, &[0.0; 4_800])).expect("writable");
    let imported = opened
        .library
        .import_audio(&quiet)
        .expect("a wav this test just wrote");

    assert_ne!(
        imported.asset.id, take.id,
        "the new import was minted the take's id"
    );
    let store = opened.library.audio_store();
    let kept = store.get(take.id).expect("the take is gone");
    assert!(
        kept.data.iter().any(|s| s.abs() > 0.5),
        "the take's audio was replaced by the file imported after it"
    );
    std::fs::remove_dir_all(&dir).ok();
}
