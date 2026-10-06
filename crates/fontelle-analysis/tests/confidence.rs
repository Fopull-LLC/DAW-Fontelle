//! The extraction confidence (plan §2.6, §4 P1).

#![cfg(feature = "model")]

use fontelle_analysis::confidence::{Clarity, extraction_confidence, extraction_evidence};
use fontelle_analysis::testsignals;
use fontelle_analysis::transcribe::basic_pitch::{BasicPitch, SAMPLE_RATE};
use fontelle_analysis::transcribe::{NoteParams, notes_from_posteriorgrams, tidy_notes};
use std::sync::OnceLock;

fn model() -> &'static BasicPitch {
    static MODEL: OnceLock<BasicPitch> = OnceLock::new();
    MODEL.get_or_init(|| BasicPitch::load().expect("the model loads"))
}

fn confidence_of(audio: &[f32]) -> (f32, usize) {
    let post = model().posteriorgrams(audio).unwrap();
    let notes = tidy_notes(
        notes_from_posteriorgrams(&post, &NoteParams::default()),
        &post,
    );
    let evidence = extraction_evidence(audio, SAMPLE_RATE, &post, &notes);
    (extraction_confidence(&evidence), notes.len())
}

#[test]
fn clean_sine_melody_is_clear() {
    let (c, notes) = confidence_of(&testsignals::sine_melody(SAMPLE_RATE).samples);
    assert!(notes > 0);
    assert!(c >= 0.75, "{c}");
    assert_eq!(Clarity::of(c), Clarity::Clear);
}

#[test]
fn sung_lines_and_chords_are_clear_or_usable() {
    for fixture in testsignals::reference_fixtures() {
        let (c, _) = confidence_of(&fixture.samples);
        assert!(c >= 0.5, "{}: {c}", fixture.name);
    }
}

#[test]
fn drum_loop_is_a_rough_guess() {
    let (c, _) = confidence_of(&testsignals::drum_loop(SAMPLE_RATE));
    assert!(c < 0.4, "{c}");
    assert_eq!(Clarity::of(c), Clarity::RoughGuess);
}

#[test]
fn noise_is_a_rough_guess() {
    let (c, _) = confidence_of(&testsignals::noise(SAMPLE_RATE, 4.0, 0.3));
    assert!(c < 0.4, "{c}");
}

#[test]
fn a_melody_buried_in_noise_falls_from_clear() {
    let melody = testsignals::sine_melody(SAMPLE_RATE).samples;
    let hiss = testsignals::noise(SAMPLE_RATE, 4.6, 0.25);
    let buried: Vec<f32> = melody.iter().zip(&hiss).map(|(a, b)| a + b).collect();
    let (clean, _) = confidence_of(&melody);
    let (noisy, _) = confidence_of(&buried);
    assert!(noisy < clean - 0.15, "clean {clean}, noisy {noisy}");
}

#[test]
fn the_words() {
    assert_eq!(Clarity::of(0.91), Clarity::Clear);
    assert_eq!(Clarity::of(0.64), Clarity::Usable);
    assert_eq!(Clarity::of(0.18), Clarity::RoughGuess);
    assert_eq!(Clarity::Clear.label(), "Clear");
    assert_eq!(Clarity::Usable.label(), "Usable");
    assert_eq!(Clarity::RoughGuess.label(), "Rough guess");
}
