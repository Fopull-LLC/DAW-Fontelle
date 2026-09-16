//! Dropping a **recording** into Flopsynth and playing it across the keyboard.
//!
//! > *"i think those have features for dragging an audio file in and using
//! > the waveform of that as your oscilator or something ... we could
//! > actually sample a real piano sound and then do effects and modulating
//! > and layering with other oscilators and stuff etc."* — Ty, 2026-09-15
//!
//! `tests/flopsynth_wavetable.rs` is a sound cut into cycles;
//! `fontelle-core`'s `tests/user_sample.rs` is what a patch carrying a whole
//! one *is*. This is the studio's half: a file on disk becomes a recording
//! on the oscillator it was dropped on, pitched by what its name or its
//! sound says it is, as one undoable edit — and a **folder** of notes
//! becomes a multi-sample, which is what a sampled piano is.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_assets::fixtures::build_wav;
use fontelle_core::SILENT_DB;
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-smp-{name}-{}-{:?}",
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

/// A recording of a sine at `hz`, `seconds` long, at 48 kHz.
fn a_tone(dir: &Path, name: &str, hz: f32, seconds: f32) -> PathBuf {
    let samples: Vec<f32> = (0..(48_000.0 * seconds) as usize)
        .map(|i| (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin() * 0.8)
        .collect();
    let path = dir.join(format!("{name}.wav"));
    std::fs::write(&path, build_wav(48_000, 1, &samples)).expect("writable");
    path
}

fn patch_of(session: &Session) -> fontelle_core::Patch {
    session.selected_patch().expect("the channel has a patch")
}

fn source_of(patch: &fontelle_core::Patch, layer: usize) -> fontelle_dsp::SynthSource {
    match &patch.layers[layer].source {
        fontelle_core::Source::Synth(osc) => osc.source,
        _ => panic!("layer {layer} is not a synth layer"),
    }
}

fn render_note(patch: fontelle_core::Patch, key: u8, frames: usize) -> Vec<f32> {
    use fontelle_core::{NoteTrigger, PrepareContext, SampleStore, Sampler};
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR as f32,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, 100));
    let store = SampleStore::new();
    let mut out = Vec::new();
    while out.len() < frames {
        let mut left = vec![0.0f32; 512];
        let mut right = vec![0.0f32; 512];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
        out.extend_from_slice(&left);
    }
    out
}

/// Upward crossings of zero per second — over the part of the render that
/// is sounding, and with a little hysteresis, so the filter's residual after
/// a short recording ends (a billionth either side of zero) is not counted
/// as a thousand crossings.
fn zero_crossings_per_second(samples: &[f32]) -> f32 {
    let last = samples
        .iter()
        .rposition(|s| s.abs() > 1e-3)
        .map_or(samples.len(), |i| i + 1);
    let sounding = &samples[..last];
    let mut below = false;
    let mut crossings = 0usize;
    for sample in sounding {
        if *sample < -1e-4 {
            below = true;
        } else if *sample > 1e-4 && below {
            crossings += 1;
            below = false;
        }
    }
    crossings as f32 / (sounding.len().max(1) as f32 / SR as f32)
}

#[test]
fn a_dropped_sound_becomes_the_oscillators_recording() {
    let dir = scratch("drop");
    let mut session = a_session(&dir);
    let path = a_tone(&dir, "Rhodes A4", 440.0, 0.5);
    let said = session.load_sample(1, &path).expect("loads");
    assert!(
        said.contains("Rhodes A4"),
        "the status names the sound: {said}"
    );
    let patch = patch_of(&session);
    assert_eq!(patch.samples.len(), 1);
    assert_eq!(patch.samples[0].name, "Rhodes A4");
    assert_eq!(patch.samples[0].zones.len(), 1);
    assert_eq!(patch.samples[0].zones[0].sample_rate, 48_000);
    assert_eq!(patch.samples[0].zones[0].key_range, (0, 127));
    assert_eq!(source_of(&patch, 1), fontelle_dsp::SynthSource::Sample(0));
    // Switched on: every oscillator but the first is at the floor in the
    // Init patch, and a drop that loaded silently reads as one that did
    // nothing.
    assert!(patch.layers[1].gain_db > SILENT_DB);
    std::fs::remove_dir_all(&dir).ok();
}

/// The recording's pitch, from its **name** when the name says — the way
/// every sample library names its notes — and from the **sound** when it
/// does not.
#[test]
fn the_root_comes_from_the_name_or_failing_that_the_sound() {
    let dir = scratch("root");
    let mut session = a_session(&dir);
    for (name, key) in [
        ("piano_C#3", 49),
        ("A4v8", 69),
        ("Grand-Db5-soft", 73),
        ("kick c1", 24),
    ] {
        let path = a_tone(&dir, name, 200.0, 0.2);
        session.load_sample(0, &path).expect("loads");
        let patch = patch_of(&session);
        assert_eq!(
            patch.samples[0].zones[0].root_key, key,
            "{name} should be read as key {key}"
        );
        assert_eq!(patch.samples[0].zones[0].fine_cents, 0.0);
    }
    // No note in the name: the sound is a 440 Hz sine, which is A4.
    let path = a_tone(&dir, "thing", 440.0, 0.5);
    session.load_sample(0, &path).expect("loads");
    let zone = &patch_of(&session).samples[0].zones[0];
    assert_eq!(zone.root_key, 69, "a 440 Hz recording is A4");
    assert!(
        zone.fine_cents.abs() < 15.0,
        "and in tune: {} cents",
        zone.fine_cents
    );
    // A quarter-tone flat of B4, detected as such.
    let path = a_tone(&dir, "flat", 493.88 * 2f32.powf(-50.0 / 1200.0), 0.5);
    session.load_sample(0, &path).expect("loads");
    let zone = &patch_of(&session).samples[0].zones[0];
    assert!(
        (zone.root_key == 71 && (zone.fine_cents + 50.0).abs() < 15.0)
            || (zone.root_key == 70 && (zone.fine_cents - 50.0).abs() < 15.0),
        "a quarter-tone flat B4 should be found: key {} at {} cents",
        zone.root_key,
        zone.fine_cents
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A **folder** of notes is a multi-sample: one zone per file, each over
/// the keys nearest its root, so the whole keyboard is covered by the
/// nearest recording rather than one note stretched four octaves.
#[test]
fn a_folder_of_notes_becomes_a_multisample() {
    let dir = scratch("folder");
    let mut session = a_session(&dir);
    let notes = dir.join("Upright");
    std::fs::create_dir_all(&notes).expect("creatable");
    a_tone(&notes, "C5", 523.25, 0.8);
    a_tone(&notes, "C3", 130.81, 0.8);
    a_tone(&notes, "C4", 261.63, 0.8);
    // Something that is not a sound, which a folder of samples often has.
    std::fs::write(notes.join("readme.txt"), "three notes").expect("writable");
    let said = session.load_sample(0, &notes).expect("loads a folder");
    assert!(said.contains("Upright") && said.contains('3'), "{said}");
    let patch = patch_of(&session);
    assert_eq!(patch.samples.len(), 1);
    assert_eq!(patch.samples[0].name, "Upright");
    let zones = &patch.samples[0].zones;
    let roots: Vec<u8> = zones.iter().map(|z| z.root_key).collect();
    assert_eq!(
        roots,
        vec![48, 60, 72],
        "sorted by root, whatever order the files came in"
    );
    // The ranges tile the keyboard and split halfway between neighbours.
    assert_eq!(zones[0].key_range, (0, 54));
    assert_eq!(zones[1].key_range, (55, 66));
    assert_eq!(zones[2].key_range, (67, 127));
    // And each key plays its own recording at its own pitch: D4 from the
    // C4 zone, two semitones up.
    let out = render_note(patch, 62, 24_000);
    let measured = zero_crossings_per_second(&out[2_400..21_600]);
    assert!(
        (measured - 293.66).abs() < 4.0,
        "D4 should come out of the C4 recording at 293.66 Hz: {measured}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dropping_again_on_the_same_oscillator_replaces_its_recording() {
    let dir = scratch("replace");
    let mut session = a_session(&dir);
    session
        .load_sample(0, &a_tone(&dir, "One", 440.0, 0.1))
        .expect("loads");
    session
        .load_sample(0, &a_tone(&dir, "Two", 440.0, 0.1))
        .expect("loads");
    let patch = patch_of(&session);
    assert_eq!(patch.samples.len(), 1);
    assert_eq!(patch.samples[0].name, "Two");
    // A second oscillator gets a recording of its own.
    session
        .load_sample(2, &a_tone(&dir, "Three", 440.0, 0.1))
        .expect("loads");
    let patch = patch_of(&session);
    assert_eq!(patch.samples.len(), 2);
    assert_eq!(source_of(&patch, 0), fontelle_dsp::SynthSource::Sample(0));
    assert_eq!(source_of(&patch, 2), fontelle_dsp::SynthSource::Sample(1));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dropped_recording_is_what_the_channel_plays() {
    let dir = scratch("plays");
    let mut session = a_session(&dir);
    session
        .load_sample(0, &a_tone(&dir, "A4", 440.0, 1.0))
        .expect("loads");
    // Through the patch the session holds, so this is what says the samples
    // survive the base64 round trip the document puts them through.
    let out = render_note(patch_of(&session), 69, 24_000);
    assert!(out.iter().any(|s| s.abs() > 0.01), "audible");
    let measured = zero_crossings_per_second(&out[2_400..]);
    assert!(
        (measured - 440.0).abs() < 5.0,
        "A4 plays the A4 recording as recorded: {measured} Hz"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dropped_recording_can_be_taken_back() {
    let dir = scratch("undo");
    let mut session = a_session(&dir);
    session
        .load_sample(0, &a_tone(&dir, "Pad", 440.0, 0.1))
        .expect("loads");
    assert_eq!(patch_of(&session).samples.len(), 1);
    session.undo();
    assert!(patch_of(&session).samples.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_that_is_not_a_sound_is_refused_with_a_reason() {
    let dir = scratch("refuse");
    let mut session = a_session(&dir);
    let path = dir.join("notes.txt");
    std::fs::write(&path, "not audio").expect("writable");
    let said = session.load_sample(0, &path).expect_err("refused");
    assert!(!said.is_empty());
    assert!(patch_of(&session).samples.is_empty());
    // The noise layer takes no recording either: it is the noise layer.
    let said = session
        .load_sample(4, &a_tone(&dir, "A4", 440.0, 0.1))
        .expect_err("refused");
    assert!(said.contains("noise"), "{said}");
    std::fs::remove_dir_all(&dir).ok();
}

/// A sound dropped on a card is a **recording** unless it is shaped like a
/// wavetable — a whole number of 2048-sample cycles, which is what a table
/// exported from a wavetable editor is and what nothing recorded ever is.
/// One drop, one rule, said on the chip before the button comes up.
#[test]
fn a_dropped_sound_is_a_recording_unless_it_is_shaped_like_a_table() {
    let dir = scratch("shape");
    let mut session = a_session(&dir);
    // Four exact cycles: a wavetable.
    let table: Vec<f32> = (0..8_192)
        .map(|i| (i % 2_048) as f32 / 1_024.0 - 1.0)
        .collect();
    let path = dir.join("Serum export.wav");
    std::fs::write(&path, build_wav(48_000, 1, &table)).expect("writable");
    session.load_sound(0, &path).expect("loads");
    let patch = patch_of(&session);
    assert_eq!(patch.wavetables.len(), 1);
    assert!(patch.samples.is_empty());
    assert_eq!(source_of(&patch, 0), fontelle_dsp::SynthSource::User(0));
    // Half a second of a note: a recording.
    session
        .load_sound(1, &a_tone(&dir, "Note", 440.0, 0.5))
        .expect("loads");
    let patch = patch_of(&session);
    assert_eq!(patch.samples.len(), 1);
    assert_eq!(source_of(&patch, 1), fontelle_dsp::SynthSource::Sample(0));
    std::fs::remove_dir_all(&dir).ok();
}

/// The browser's row, by index — what the in-window drag carries.
#[test]
fn a_row_of_the_import_tab_lands_on_an_oscillator() {
    let dir = scratch("browser");
    let mut session = a_session(&dir);
    let folder = dir.join("sounds");
    std::fs::create_dir_all(&folder).expect("creatable");
    a_tone(&folder, "Bell", 880.0, 0.2);
    session.set_import_folder(fontelle_types::FolderKind::Audio, Some(folder));
    session.set_import_kind(fontelle_types::FolderKind::Audio);
    session.set_browser_mode(fontelle_ui::canvas::BrowserMode::Import);
    let row = session
        .import_files()
        .iter()
        .position(|entry| entry.name == "Bell")
        .expect("the file is listed");
    let said = session.load_import_into_oscillator(2, row).expect("loads");
    assert!(said.contains("Bell"), "{said}");
    let patch = patch_of(&session);
    assert_eq!(patch.samples[0].name, "Bell");
    assert_eq!(source_of(&patch, 2), fontelle_dsp::SynthSource::Sample(0));
    std::fs::remove_dir_all(&dir).ok();
}

/// The sounds a card's own menu offers: the bank's own recordings first
/// (the sampled grand, so a real piano can be layered and modulated
/// without a file to find), then every audio file in the Import tab's
/// audio folder, by name, whichever tab the browser is on — and a pick
/// from it lands on the oscillator like a drop would.
///
/// A drag between two windows is the compositor's to deliver; this is the
/// path that works whatever it decides.
#[test]
fn a_cards_menu_lists_the_audio_folder_and_loads_from_it() {
    let dir = scratch("menu");
    let mut session = a_session(&dir);
    let factory = vec!["Grand (soft)".to_string(), "Grand (hard)".to_string()];
    assert_eq!(
        session.audio_sounds(),
        factory,
        "no audio folder: the bank's own recordings are still there"
    );
    let folder = dir.join("sounds");
    std::fs::create_dir_all(&folder).expect("creatable");
    a_tone(&folder, "Bell", 880.0, 0.2);
    a_tone(&folder, "Alto", 440.0, 0.2);
    std::fs::write(folder.join("notes.txt"), "not a sound").unwrap();
    session.set_import_folder(fontelle_types::FolderKind::Audio, Some(folder));
    // The browser is on MIDI; the menu still lists the audio folder.
    session.set_import_kind(fontelle_types::FolderKind::Midi);
    let names = session.audio_sounds();
    let mut expected = factory.clone();
    expected.extend(["Alto".to_string(), "Bell".to_string()]);
    assert_eq!(names, expected);
    let said = session
        .load_audio_sound_into_oscillator(1, 3)
        .expect("loads");
    assert!(said.contains("Bell"), "{said}");
    let patch = patch_of(&session);
    assert_eq!(patch.samples[0].name, "Bell");
    assert_eq!(source_of(&patch, 1), fontelle_dsp::SynthSource::Sample(0));
    assert!(session.load_audio_sound_into_oscillator(1, 9).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

/// A pick of the bank's own grand from a card's menu: the recording lands
/// on the oscillator as a *named* set — the patch carries no audio, so a
/// project with it is a page of JSON — and it plays.
#[test]
fn a_cards_menu_offers_the_banks_own_grand() {
    let dir = scratch("menu-grand");
    let mut session = a_session(&dir);
    let said = session
        .load_audio_sound_into_oscillator(2, 1)
        .expect("the hard grand loads");
    assert!(said.contains("Grand (hard)"), "{said}");
    let patch = patch_of(&session);
    assert_eq!(source_of(&patch, 2), fontelle_dsp::SynthSource::Sample(0));
    assert_eq!(
        patch.samples[0].factory,
        Some(fontelle_core::factory_samples::FactorySampleSet::GrandHard)
    );
    assert!(patch.samples[0].zones.len() > 20);
    assert!(
        patch.layers[2].gain_db > fontelle_core::SILENT_DB,
        "a card given a sound is on"
    );
    let text = serde_json::to_string(&patch.to_data(&Default::default()).unwrap()).unwrap();
    assert!(text.len() < 40_000, "{} bytes", text.len());
    // A second oscillator given the same set shares it rather than adding
    // a second copy to the patch.
    session
        .load_audio_sound_into_oscillator(0, 1)
        .expect("loads again");
    let patch = patch_of(&session);
    assert_eq!(patch.samples.len(), 1);
    assert_eq!(source_of(&patch, 0), fontelle_dsp::SynthSource::Sample(0));
    std::fs::remove_dir_all(&dir).ok();
}
