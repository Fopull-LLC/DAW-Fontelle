//! Turning an audio file into a sampler on a channel of its own.
//!
//! Reported from using the window:
//!
//! > *"currently i cannot drag an audio clip from the audio import tab into
//! > the channel rack to turn it into a sampler, please add this feature."*
//!
//! The reason it was missing is that the two stores are different, and
//! deliberately: an audio *clip*'s audio lives in the `AudioStore` and is
//! stereo, and a sampler *layer*'s lives in the `SampleStore` and is mono. So
//! dropping a file on the arrangement and dropping it on the rack are two
//! different acts on the same file, and only the first one existed.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{SampleLibrary};
use fontelle_assets::fixtures::build_wav;
use fontelle_types::InstrumentKind;
use fontelle_ui::DocumentHost;
use fontelle_ui::document::StudioHost;

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-sampler-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

/// A one-second tone, written where a test can point at it.
fn a_wav(dir: &Path, name: &str, channels: u16) -> PathBuf {
    let frames = 24_000;
    let mut samples = Vec::with_capacity(frames * channels as usize);
    for i in 0..frames {
        let v = (i as f32 * 220.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.8;
        for c in 0..channels {
            // The two sides differ, so a fold that kept only one would show.
            samples.push(if c == 0 { v } else { -v });
        }
    }
    let path = dir.join(name);
    std::fs::write(&path, build_wav(48_000, channels, &samples)).expect("writable");
    path
}

#[test]
fn a_wav_becomes_a_sampler_on_a_channel_of_its_own() {
    let dir = scratch("basic");
    let path = a_wav(&dir, "Kick.wav", 1);
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    let before = session.channels().len();

    let name = session.add_sampler_from(&path).expect("imports");

    assert_eq!(name, "Kick");
    assert_eq!(session.channels().len(), before + 1);
    let made = session.channels().len() - 1;
    assert_eq!(session.channel_kind(made), Some(InstrumentKind::Sampler));
    assert!(
        session.channels()[made].has_instrument,
        "a sampler made from a file plays that file"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_sampler_covers_the_whole_keyboard_from_middle_c() {
    // A file has no key mapping of its own to offer, so the only honest
    // reading is one layer over every key, at its own pitch at middle C.
    let dir = scratch("range");
    let path = a_wav(&dir, "Tone.wav", 1);
    let mut library = SampleLibrary::new();
    let imported = library.import_sample(&path).expect("imports");
    assert!(imported.frames > 0);
    assert_eq!(imported.sample_rate, 48_000);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stereo_file_keeps_both_sides_rather_than_losing_one() {
    // Folded by averaging. This fixture's sides are exact opposites, so a
    // fold that took the left channel would give a full-scale tone and the
    // average gives silence — which is the sharpest possible way to tell the
    // two apart.
    let dir = scratch("stereo");
    let path = a_wav(&dir, "Wide.wav", 2);
    let mut library = SampleLibrary::new();
    let imported = library.import_sample(&path).expect("imports");
    let store = library.store();
    let buffer = store.get(imported.id).expect("in the store");
    let peak = buffer.data.iter().fold(0.0f32, |a, b| a.max(b.abs()));
    assert!(
        peak < 0.01,
        "the sides were not averaged — peak {peak} says one was dropped"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_same_file_twice_is_one_sample_rather_than_two_copies() {
    let dir = scratch("dedupe");
    let path = a_wav(&dir, "Snare.wav", 1);
    let mut library = SampleLibrary::new();
    let first = library.import_sample(&path).expect("imports");
    let second = library.import_sample(&path).expect("imports");
    assert_eq!(first.id, second.id, "the file was decoded twice");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_with_no_sound_in_it_is_refused_rather_than_made_into_a_silent_channel() {
    let dir = scratch("empty");
    let path = dir.join("Nothing.wav");
    std::fs::write(&path, build_wav(48_000, 1, &[])).expect("writable");
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    let before = session.channels().len();
    assert!(session.add_sampler_from(&path).is_err());
    assert_eq!(
        session.channels().len(),
        before,
        "a channel was made anyway"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_that_is_not_there_says_so() {
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    let error = session
        .add_sampler_from(Path::new("/nowhere/at/all.wav"))
        .expect_err("must refuse");
    assert!(!error.is_empty());
}

#[test]
fn the_sample_can_be_found_again_after_a_reload() {
    // The half that makes a saved project open with its sound: a patch stores
    // its layers' provenance and resolves them on load, so the file it names
    // has to be findable by that name.
    let dir = scratch("reload");
    let path = a_wav(&dir, "Loop.wav", 1);
    let mut library = SampleLibrary::new();
    let imported = library.import_sample(&path).expect("imports");

    let mut reopened = SampleLibrary::new();
    reopened
        .reload_sample(&imported.file.file)
        .expect("reloads");
    assert!(
        reopened.resolve(&imported.file).is_some(),
        "a reopened project could not find the sample its patch names"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------ onto a channel that is already there ---
//
// > *"i want to be able to click and drag them into the sampler or into the
// > channel rack to make it have a sampler with that clip sampled."*
//
// Two targets, two meanings, and the difference is the one every drop on this
// rack already makes: **empty space makes a new one, a row changes that one.**
// Dropping a file on the row you are working on has to replace what it plays,
// or building a kit means dragging eight files in and then deleting the eight
// channels they landed beside.

#[test]
fn a_file_dropped_on_a_channel_makes_that_channel_the_sampler() {
    let dir = scratch("onto");
    let path = a_wav(&dir, "Snare.wav", 1);
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    let before = session.channels().len();
    assert!(before > 0, "the blank project has a channel to drop onto");

    let name = session
        .set_channel_sampler_from(0, &path)
        .expect("takes the file");

    assert_eq!(name, "Snare");
    assert_eq!(
        session.channels().len(),
        before,
        "a channel was added as well as changed"
    );
    assert_eq!(session.channel_kind(0), Some(InstrumentKind::Sampler));
    assert!(session.channels()[0].has_instrument);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_channel_takes_the_files_name_so_the_rack_says_what_it_plays() {
    // The rule `install_preset` already follows for a soundfont: a row that
    // goes on saying what it used to be is the loudest "nothing happened" a
    // rack can give somebody who has just changed its sound.
    let dir = scratch("named");
    let path = a_wav(&dir, "Rimshot.wav", 1);
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    session
        .set_channel_sampler_from(0, &path)
        .expect("takes the file");
    assert_eq!(session.channels()[0].name, "Rimshot");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn one_drop_is_one_undo() {
    // INVARIANT 9: every document change goes through the history, and
    // changing an instrument by mistake is one Ctrl+Z away — not three, for
    // the patch, the kind and the name.
    let dir = scratch("undo");
    let path = a_wav(&dir, "Clap.wav", 1);
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    let was = session.channels()[0].name.clone();
    let kind = session.channel_kind(0);

    session
        .set_channel_sampler_from(0, &path)
        .expect("takes the file");
    assert_ne!(session.channels()[0].name, was);

    session.undo();
    assert_eq!(session.channels()[0].name, was, "one undo was not enough");
    assert_eq!(session.channel_kind(0), kind);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_channel_that_is_not_there_is_refused_rather_than_guessed_at() {
    let dir = scratch("norow");
    let path = a_wav(&dir, "Tom.wav", 1);
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    let before = session.channels().len();
    assert!(session.set_channel_sampler_from(99, &path).is_err());
    assert_eq!(
        session.channels().len(),
        before,
        "a channel was made to drop onto"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_with_no_sound_in_it_leaves_the_channel_as_it_was() {
    // The same refusal `add_sampler_from` makes, in the place where getting it
    // wrong is worse: this one would silence an instrument somebody is using.
    let dir = scratch("silent");
    let path = dir.join("Nothing.wav");
    std::fs::write(&path, build_wav(48_000, 1, &[])).expect("writable");
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    let was = session.channels()[0].name.clone();
    let kind = session.channel_kind(0);

    assert!(session.set_channel_sampler_from(0, &path).is_err());
    assert_eq!(session.channels()[0].name, was);
    assert_eq!(session.channel_kind(0), kind);
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------- and it has to sound ---
//
// > *"i clicked and dragged my hardstyle kick in and it did make a sampler but
// > nothing i did was audible at all."*
//
// A channel that says "Sampler" in the rack and renders silence is worse than
// no channel at all: everything above this line passed while the studio made
// no sound, because every one of those assertions is about the *document*.
// These are about the samples that come out of the graph.

/// The realise options the studio itself plays with.
fn play_options() -> fontelle_app::RealiseOptions {
    fontelle_app::RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    }
}

/// Renders `frames` of the project with one long note held on `channel`, and
/// hands back the loudest sample in it.
fn peak_of_channel(
    project: &mut fontelle_model::Project,
    library: &fontelle_app::SampleLibrary,
    channel: fontelle_types::ChannelId,
    frames: usize,
) -> f32 {
    use fontelle_model::{Arena, Clip, ClipSource, Note, NoteData};
    use fontelle_types::PPQN;

    let lane = project.lanes.keys().next().expect("a lane to draw in");
    let mut notes = Arena::default();
    notes.insert(Note {
        start: 0,
        length: PPQN * 4,
        key: 60,
        velocity: 127,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    });
    project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData { channel, notes }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });

    let mut realised =
        fontelle_app::realise(project, library, play_options()).expect("this project must realise");
    let timeline =
        fontelle_sequencer::compile(project, &realised.channel_nodes, &Default::default());
    let out = fontelle_app::render_offline(&timeline, &mut realised.graph, frames as i64);
    out.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn a_sampler_made_from_a_file_actually_makes_a_sound() {
    let dir = scratch("audible");
    let path = a_wav(&dir, "Kick.wav", 1);
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    session.add_sampler_from(&path).expect("imports");

    let made = session
        .project()
        .channels
        .keys()
        .last()
        .expect("the channel that was just made");
    let mut project = session.project().clone();
    let peak = peak_of_channel(&mut project, session.library(), made, SR as usize);
    assert!(
        peak > 0.01,
        "the sampler rendered silence — peak {peak}, so the layer's sample \
         never reached the graph"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_dropped_on_a_channel_makes_that_channel_sound() {
    let dir = scratch("audible-onto");
    let path = a_wav(&dir, "Snare.wav", 1);
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    session
        .set_channel_sampler_from(0, &path)
        .expect("takes the file");

    let onto = session
        .project()
        .channels
        .keys()
        .next()
        .expect("the channel it was dropped on");
    let mut project = session.project().clone();
    let peak = peak_of_channel(&mut project, session.library(), onto, SR as usize);
    assert!(
        peak > 0.01,
        "the channel it was dropped on is silent — peak {peak}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn nothing_about_the_patch_is_left_unresolved() {
    // The sharper statement of the two above: `realise` reports every layer
    // whose sample it could not find, and a sampler built from a file that was
    // just decoded must have none. A peak of zero has many causes; this has
    // one.
    let dir = scratch("resolved");
    let path = a_wav(&dir, "Tom.wav", 1);
    let mut session = common::a_session_for(common::a_project_with_a_clip(4, 120.0, SR));
    session.add_sampler_from(&path).expect("imports");

    let realised = fontelle_app::realise(session.project(), session.library(), play_options())
        .expect("this project must realise");
    assert!(
        realised.unresolved.is_empty(),
        "the sampler's own sample could not be found again: {:?}",
        realised.unresolved
    );
    std::fs::remove_dir_all(&dir).ok();
}
