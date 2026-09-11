//! Saving a project and opening it again (TDD §17.1–17.4).
//!
//! The test that matters is the last clause of the gate sentence: "save the
//! project, quit, reopen it to an identical-sounding state". Identical here
//! means bit-identical, rendered and compared sample for sample, because
//! anything looser would not have caught the sample that came back under a
//! different id or the patch field that quietly took its default.

mod common;

use std::path::PathBuf;

use fontelle_app::{
    OpenError, SampleLibrary, open_project, project_duration_samples, realise, render_offline,
    save_project, set_channel_patch,
};
use fontelle_assets::fixtures::{
    GEN_KEY_RANGE, GEN_OVERRIDING_ROOT_KEY, GEN_PAN, GEN_SAMPLE_MODES, KIT, Sf2Fixture, ZoneSpec,
    build_drum_kit_sf2, build_sf2, gen_range, gen_val, write_fixture_to_temp_file,
};
use fontelle_model::{Command, NumberTarget, Project, SetNumber, StorageError};
use fontelle_types::PPQN;

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-bundle-{name}-{}.fontelle",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    path
}

/// A real soundfont on disk, because a project that reopens has to find its
/// audio in a *file* — a synthetic buffer has nowhere to come back from.
fn a_soundfont(name: &str) -> PathBuf {
    let fixture = Sf2Fixture {
        samples: (0..64).map(|i| (i * 500 - 16_000) as i16).collect(),
        sample_rate: 44_100,
        header_start: 0,
        header_end: 64,
        header_loop_start: 8,
        header_loop_end: 56,
        origpitch: 60,
        pitchadj: 0,
        zone: ZoneSpec {
            generators: vec![
                gen_range(GEN_KEY_RANGE, 0, 127),
                gen_val(GEN_OVERRIDING_ROOT_KEY, 60),
                gen_val(GEN_SAMPLE_MODES, 1),
                gen_val(GEN_PAN, -100),
            ],
        },
        extra_zones: Vec::new(),
    };
    write_fixture_to_temp_file(name, &build_sf2(&fixture))
}

/// The demo phrase, playing a preset out of `soundfont`.
fn a_project(soundfont: &std::path::Path) -> (Project, SampleLibrary) {
    let mut library = SampleLibrary::new();
    let patch = library
        .import_sf2(soundfont, 0)
        .expect("the fixture must import");
    let mut project = fontelle_app::demo_project(60, 120.0, SR);
    let channel = project.channels.keys().next().unwrap();
    set_channel_patch(&mut project, channel, &patch, &library).unwrap();
    let master = project.mixer.master.unwrap();
    SetNumber::new(NumberTarget::TrackGainDb(master), -2.5)
        .apply(&mut project)
        .unwrap();
    (project, library)
}

fn render(project: &Project, library: &SampleLibrary) -> Vec<f32> {
    let mut realised = realise(
        project,
        library,
        fontelle_app::RealiseOptions {
            sample_rate: SR,
            block_size: fontelle_engine::BLOCK_SIZE,
            quality: fontelle_app::RENDER_QUALITY,
        },
    )
    .expect("this project must realise");
    let timeline =
        fontelle_sequencer::compile(project, &realised.channel_nodes, &realised.param_nodes);
    render_offline(
        &timeline,
        &mut realised.graph,
        project_duration_samples(project, PPQN),
    )
}

#[test]
fn a_reopened_project_renders_bit_identically_to_the_one_that_was_saved() {
    let soundfont = a_soundfont("bundle-round-trip");
    let bundle = scratch("round-trip");

    let (project, library) = a_project(&soundfont);
    let before = render(&project, &library);
    assert!(before.iter().any(|s| *s != 0.0), "the fixture must sound");
    save_project(&project, &bundle).expect("save");
    drop((project, library));

    let opened = open_project(&bundle).expect("open");
    assert!(
        opened.missing.is_empty(),
        "the soundfont is still where it was: {:?}",
        opened.missing
    );
    let after = render(&opened.project, &opened.library);

    assert_eq!(
        before, after,
        "a reopened project must sound exactly like the one that was saved"
    );
    std::fs::remove_dir_all(&bundle).ok();
    std::fs::remove_file(&soundfont).ok();
}

#[test]
fn the_reopened_document_carries_the_mixer_settings_that_were_saved() {
    let soundfont = a_soundfont("bundle-mixer");
    let bundle = scratch("mixer");
    let (project, _library) = a_project(&soundfont);
    save_project(&project, &bundle).expect("save");

    let opened = open_project(&bundle).expect("open");
    let master = opened.project.mixer.master.unwrap();
    assert_eq!(opened.project.mixer.tracks[master].gain_db, -2.5);
    assert_eq!(opened.project.channels.len(), 1);
    assert_eq!(opened.project.clips.len(), 1);
    std::fs::remove_dir_all(&bundle).ok();
    std::fs::remove_file(&soundfont).ok();
}

#[test]
fn a_project_whose_soundfont_has_moved_still_opens_and_plays_silence() {
    // TDD §17.4: broken links are a normal condition. The project loads, the
    // affected layers are silent, and the reference is reported rather than
    // dropped — the next save still has it.
    let soundfont = a_soundfont("bundle-moved");
    let bundle = scratch("moved");
    let (project, _library) = a_project(&soundfont);
    save_project(&project, &bundle).expect("save");
    std::fs::remove_file(&soundfont).ok();

    let opened = open_project(&bundle).expect("a broken link is not a refusal");
    assert_eq!(opened.missing.len(), 1);
    assert_eq!(opened.missing[0].file.path, soundfont);
    assert_eq!(
        opened.missing[0].channels.len(),
        1,
        "the message can name the instrument, not just the file"
    );

    let audio = render(&opened.project, &opened.library);
    assert!(
        audio.iter().all(|s| *s == 0.0),
        "the layer with no audio must be silent"
    );

    // And saving it again keeps the reference, so putting the file back is
    // all it takes.
    save_project(&opened.project, &bundle).expect("re-save");
    let text = std::fs::read_to_string(bundle.join("project.json")).unwrap();
    assert!(text.contains(soundfont.file_name().unwrap().to_str().unwrap()));
    std::fs::remove_dir_all(&bundle).ok();
}

#[test]
fn a_corrupt_document_fails_with_a_message_naming_the_file() {
    let bundle = scratch("corrupt");
    let soundfont = a_soundfont("bundle-corrupt");
    let (project, _library) = a_project(&soundfont);
    save_project(&project, &bundle).expect("save");
    let path = bundle.join("project.json");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, &text[..text.len() * 2 / 3]).unwrap();

    match open_project(&bundle) {
        Err(OpenError::Storage(StorageError::Format(message))) => {
            assert!(message.contains("project.json"), "{message}");
        }
        other => panic!("expected a clear format error, got {:?}", other.err()),
    }
    std::fs::remove_dir_all(&bundle).ok();
    std::fs::remove_file(&soundfont).ok();
}

#[test]
fn a_saved_project_references_its_soundfont_rather_than_copying_it_in() {
    // §17.4's ask-once policy, with no dialog to ask from: referencing cannot
    // surprise anybody by silently duplicating a 325 MB soundfont into their
    // project folder. `assets/` exists regardless, because it is part of the
    // bundle's shape and the copy path lands in it.
    let soundfont = a_soundfont("bundle-reference");
    let bundle = scratch("reference");
    let (project, _library) = a_project(&soundfont);
    save_project(&project, &bundle).expect("save");

    assert!(bundle.join("assets").is_dir());
    assert_eq!(
        std::fs::read_dir(bundle.join("assets")).unwrap().count(),
        0,
        "nothing was copied in"
    );
    let text = std::fs::read_to_string(bundle.join("project.json")).unwrap();
    // As JSON spells it: a Windows path's backslashes are escaped in the file.
    let spelled = serde_json::to_string(&soundfont).unwrap();
    assert!(text.contains(spelled.trim_matches('"')), "{text}");
    std::fs::remove_dir_all(&bundle).ok();
    std::fs::remove_file(&soundfont).ok();
}

/// The piano roll labels a drum kit's keys from the sample names in the file
/// (see `fontelle-app`'s `keymap`). A saved patch names its audio by file plus
/// header index (TDD §8.3) and is reloaded by that index rather than by
/// re-importing the preset — so the names have to come back on *that* path, or
/// a kit is labelled until you reopen the project and anonymous afterwards.
#[test]
fn a_reopened_kit_still_knows_what_its_keys_are_called() {
    let soundfont = write_fixture_to_temp_file("bundle-kit", &build_drum_kit_sf2(KIT));
    let bundle = scratch("kit-names");

    let mut library = SampleLibrary::new();
    let patch = library
        .import_sf2(&soundfont, 0)
        .expect("the kit must import");
    let mut project = fontelle_app::demo_project(60, 120.0, SR);
    let channel = project.channels.keys().next().unwrap();
    set_channel_patch(&mut project, channel, &patch, &library).unwrap();

    let before = fontelle_app::key_map(&patch, &library);
    assert_eq!(
        before.name(KIT[1].1),
        Some(KIT[1].0),
        "labelled to begin with"
    );

    save_project(&project, &bundle).expect("save");
    drop((project, library));

    let opened = open_project(&bundle).expect("open");
    assert!(opened.missing.is_empty(), "the kit is still where it was");
    let reloaded = fontelle_core::Patch::from_data(
        opened
            .project
            .channels
            .values()
            .next()
            .unwrap()
            .patch_data
            .as_ref()
            .expect("the channel kept its instrument"),
        |file| opened.library.resolve(file),
    )
    .expect("the stored patch must read back")
    .patch;

    let after = fontelle_app::key_map(&reloaded, &opened.library);
    assert!(after.is_named(), "a reopened kit is still a kit");
    for (name, key) in KIT {
        assert_eq!(
            after.name(*key),
            Some(*name),
            "key {key} lost its name across the round trip"
        );
    }

    std::fs::remove_dir_all(&bundle).ok();
    std::fs::remove_file(&soundfont).ok();
}
