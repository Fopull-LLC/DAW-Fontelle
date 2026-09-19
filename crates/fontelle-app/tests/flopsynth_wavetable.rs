//! Dragging a sound into Flopsynth and playing it as a waveform.
//!
//! > *"i want to like with omnisphere or serum ... be able to drag audio files
//! > into it to use those waveforms in the synthesis as im pretty sure thats
//! > somethign you could do in them which would be a cool feature."*
//!
//! `fontelle-core`'s `tests/user_wavetable.rs` holds what a patch carrying its
//! own table *is*. This is the studio's half: a file on disk becomes one, on
//! the oscillator it was dropped on, as one undoable edit — and the sound
//! that comes out is the file's.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_assets::fixtures::build_wav;
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::DocumentHost;

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-wt-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

/// A session whose selected channel plays Flopsynth.
fn a_session(dir: &Path) -> Session {
    let library = SampleLibrary::new();
    let patch = fontelle_core::flopsynth::flopsynth_init();
    let project = common::demo_with(&patch, &library);
    let clip = Session::first_clip(&project).expect("the demo project has a clip");
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised = fontelle_app::realise(&project, &library, options).expect("realises");
    let (graphs, _source) = graph_channel(realised.graph);
    Session::new(
        project,
        library,
        realised.channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_settings_path(dir.join("settings.json"))
}

/// A sound file: four cycles of a saw, which is nothing like the sine the
/// Init patch's oscillator reads.
fn a_sound(dir: &Path, name: &str) -> PathBuf {
    let samples: Vec<f32> = (0..8_192)
        .map(|i| (i % 2_048) as f32 / 1_024.0 - 1.0)
        .collect();
    let path = dir.join(format!("{name}.wav"));
    std::fs::write(&path, build_wav(48_000, 1, &samples)).expect("writable");
    path
}

fn patch_of(session: &Session) -> fontelle_core::Patch {
    session.selected_patch().expect("the channel has a patch")
}

#[test]
fn a_dropped_sound_becomes_a_table_on_the_oscillator_it_was_dropped_on() {
    let dir = scratch("drop");
    let mut session = a_session(&dir);
    let path = a_sound(&dir, "Pad");

    let said = session
        .load_wavetable(1, &path)
        .expect("the sound loads onto oscillator B");
    assert!(
        said.contains("Pad"),
        "the studio should say what arrived: {said}"
    );

    let patch = patch_of(&session);
    assert_eq!(patch.wavetables.len(), 1);
    assert_eq!(patch.wavetables[0].name, "Pad");
    // Four cycles of 2048, so four frames for the position knob to walk.
    assert_eq!(patch.wavetables[0].frames, 4);
    match &patch.layers[1].source {
        fontelle_core::Source::Synth(osc) => {
            assert_eq!(osc.source, fontelle_dsp::SynthSource::User(0));
        }
        other => panic!("layer 1 is {other:?}"),
    }
    // And nothing else moved: the other oscillators read what they read.
    match &patch.layers[0].source {
        fontelle_core::Source::Synth(osc) => {
            assert!(matches!(osc.source, fontelle_dsp::SynthSource::Table(_)));
        }
        other => panic!("layer 0 is {other:?}"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_oscillator_it_lands_on_is_switched_on() {
    // Every oscillator but the first is at the silence floor in the Init
    // patch, so a sound dropped on one that is off would load and be
    // inaudible — which reads as the drop having done nothing.
    let dir = scratch("audible");
    let mut session = a_session(&dir);
    let path = a_sound(&dir, "Pad");
    let before = patch_of(&session).layers[1].gain_db;
    assert!(before <= fontelle_core::SILENT_DB);

    session.load_wavetable(1, &path).expect("loads");
    let after = patch_of(&session).layers[1].gain_db;
    assert!(
        after > fontelle_core::SILENT_DB,
        "the oscillator is still off at {after} dB"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_second_sound_on_another_oscillator_is_a_second_table() {
    let dir = scratch("two");
    let mut session = a_session(&dir);
    session
        .load_wavetable(0, &a_sound(&dir, "One"))
        .expect("loads");
    session
        .load_wavetable(1, &a_sound(&dir, "Two"))
        .expect("loads");
    let patch = patch_of(&session);
    assert_eq!(patch.wavetables.len(), 2);
    assert_eq!(patch.wavetables[0].name, "One");
    assert_eq!(patch.wavetables[1].name, "Two");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dropping_a_second_sound_on_the_same_oscillator_replaces_its_table() {
    // Rather than growing the patch by a table nothing reads any more: a
    // preset carries its samples, so a discarded one is dead weight in every
    // copy of it from then on.
    let dir = scratch("replace");
    let mut session = a_session(&dir);
    session
        .load_wavetable(0, &a_sound(&dir, "One"))
        .expect("loads");
    session
        .load_wavetable(0, &a_sound(&dir, "Two"))
        .expect("loads");
    let patch = patch_of(&session);
    assert_eq!(patch.wavetables.len(), 1);
    assert_eq!(patch.wavetables[0].name, "Two");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_that_is_not_a_sound_is_refused_with_a_reason() {
    let dir = scratch("refuse");
    let mut session = a_session(&dir);
    let path = dir.join("notes.txt");
    std::fs::write(&path, b"not audio").expect("writable");
    let said = session.load_wavetable(0, &path).expect_err("refused");
    assert!(!said.is_empty(), "a refusal has to say why");
    assert!(patch_of(&session).wavetables.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

/// One note of a patch, rendered.
///
/// Through the patch the **session** holds, which is the half that matters
/// here: a patch is stored as `PatchData` and read back, so this is what says
/// the samples survive the base64 round trip the document puts them through.
fn render_note(patch: fontelle_core::Patch, key: u8) -> Vec<f32> {
    use fontelle_core::{NoteTrigger, PrepareContext, SampleStore, Sampler};
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR as f32,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, 100));
    let store = SampleStore::new();
    let mut out = Vec::new();
    for _ in 0..4 {
        let mut left = vec![0.0f32; 512];
        let mut right = vec![0.0f32; 512];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
        out.extend_from_slice(&left);
    }
    out
}

#[test]
fn a_dropped_sound_is_what_the_channel_plays() {
    // The lesson the audio-clip work paid for: a document test cannot hear.
    let dir = scratch("audio");
    let mut session = a_session(&dir);
    let before = render_note(patch_of(&session), 60);
    session
        .load_wavetable(0, &a_sound(&dir, "Saw"))
        .expect("loads");
    let after = render_note(patch_of(&session), 60);
    assert!(
        after.iter().any(|s| s.abs() > 0.01),
        "a dropped sound has to be audible"
    );
    let changed: f32 = before
        .iter()
        .zip(&after)
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / before.len() as f32;
    assert!(
        changed > 0.01,
        "the oscillator should sound different once a sound is dropped on it: {changed}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dropped_sound_can_be_taken_back() {
    // One edit, undone like any other: a drop is a change to the patch and
    // has to sit on the history with everything else.
    let dir = scratch("undo");
    let mut session = a_session(&dir);
    session
        .load_wavetable(0, &a_sound(&dir, "Pad"))
        .expect("loads");
    assert_eq!(patch_of(&session).wavetables.len(), 1);
    session.undo();
    assert!(patch_of(&session).wavetables.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------ which card is which ---

/// A window can only route a drop to an oscillator if it knows which card is
/// one. The **app** layer knows — it is the layer that sees both a `Patch`
/// and a `FlopsynthView` — so the card says so and the window does not guess.
#[test]
fn every_oscillator_card_says_which_layer_it_is() {
    use fontelle_ui::canvas::FlopsynthPage;

    let patch = fontelle_core::flopsynth::flopsynth_init();
    let view = fontelle_app::flopsynth::describe(
        "Flopsynth",
        &patch,
        0.0,
        0.0,
        FlopsynthPage::Synth,
        Default::default(),
        Vec::new(),
        Default::default(),
    );
    let named = |name: &str| {
        view.cards
            .iter()
            .find(|card| card.group.name == name)
            .unwrap_or_else(|| panic!("no {name} card"))
            .oscillator
    };
    assert_eq!(named("OSC A"), Some(0));
    assert_eq!(named("OSC B"), Some(1));
    assert_eq!(named("OSC C"), Some(2));
    assert_eq!(named("SUB"), Some(3));
    assert_eq!(named("NOISE"), Some(4));
    // And a card that is not an oscillator is not a place to drop a sound.
    assert_eq!(named("Filter 1"), None);
    assert_eq!(named("Voice"), None);
}
