//! Delay times in beats rather than milliseconds (TDD §13.4), and the tempo
//! the audio thread reads them against.
//!
//! A delay that cannot be set in note values is half a delay: the useful
//! settings are all *musical* — a dotted eighth behind the snare, a quarter
//! under an arpeggio — and finding those by dragging a millisecond knob means
//! recomputing them by hand every time the tempo moves.
//!
//! # Why the tempo is on the compiled timeline
//!
//! `fontelle-engine` cannot see a `TempoMap`: that is `fontelle-model`'s, and
//! INVARIANT 4 runs the other way. What the engine *does* see every block is a
//! [`CompiledTimeline`], which the sequencer builds — and the sequencer
//! already owns every tick-to-sample conversion in the project, because that
//! is what compiling a timeline *is*.
//!
//! So the tempo rides along on the timeline as a table the audio thread can
//! binary-search, in the same units the rest of the block contract is in
//! (samples), and `TransportSnapshot` carries the one number a node wants:
//! the tempo **here**, at the position being rendered.

use fontelle_types::{CompiledTimeline, DEFAULT_BPM, DelayConfig, MAX_DELAY_MS, NoteDivision};

// ------------------------------------------------------------- the divisions

#[test]
fn a_division_is_worth_the_beats_its_name_says() {
    // In quarter notes, which is what a beat is here and what the tempo is in.
    for (division, beats) in [
        (NoteDivision::Whole, 4.0),
        (NoteDivision::Half, 2.0),
        (NoteDivision::Quarter, 1.0),
        (NoteDivision::Eighth, 0.5),
        (NoteDivision::Sixteenth, 0.25),
        (NoteDivision::ThirtySecond, 0.125),
    ] {
        assert_eq!(division.beats(), beats, "{}", division.label());
    }
}

#[test]
fn a_dot_adds_half_again_and_a_triplet_takes_a_third_off() {
    // The two modifiers, and the only two that matter: a dotted eighth is the
    // delay setting, and a triplet is the other one.
    for (plain, dotted, triplet) in [
        (
            NoteDivision::Half,
            NoteDivision::HalfDotted,
            NoteDivision::HalfTriplet,
        ),
        (
            NoteDivision::Quarter,
            NoteDivision::QuarterDotted,
            NoteDivision::QuarterTriplet,
        ),
        (
            NoteDivision::Eighth,
            NoteDivision::EighthDotted,
            NoteDivision::EighthTriplet,
        ),
    ] {
        assert!(
            (dotted.beats() - plain.beats() * 1.5).abs() < 1e-6,
            "{} should be half again as long as {}",
            dotted.label(),
            plain.label()
        );
        assert!(
            (triplet.beats() - plain.beats() * 2.0 / 3.0).abs() < 1e-6,
            "{} should be two thirds of {}",
            triplet.label(),
            plain.label()
        );
    }
}

#[test]
fn every_division_is_named_and_they_are_all_different() {
    let mut names: Vec<&str> = NoteDivision::ALL.iter().map(|d| d.label()).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), before, "two divisions share a name");
    assert!(NoteDivision::ALL.iter().all(|d| d.beats() > 0.0));
}

#[test]
fn the_divisions_run_from_longest_to_shortest() {
    // Which is the order a chooser steps through them, and the order an
    // automation lane sweeps: turning the knob up should shorten the delay
    // monotonically rather than jumping about.
    for pair in NoteDivision::ALL.windows(2) {
        assert!(
            pair[0].beats() > pair[1].beats(),
            "{} is not longer than {}",
            pair[0].label(),
            pair[1].label()
        );
    }
}

// ------------------------------------------------- what the delay asks for

fn synced(division: NoteDivision) -> DelayConfig {
    DelayConfig {
        sync: true,
        division,
        ..DelayConfig::new()
    }
}

#[test]
fn a_synced_delay_asks_for_the_note_value_at_the_tempo_it_is_given() {
    // A quarter note at 120 bpm is half a second, and that is not a
    // convention — it is what 120 beats per minute means.
    let quarter = synced(NoteDivision::Quarter);
    assert!((quarter.effective_time_ms(120.0) - 500.0).abs() < 1e-3);
    assert!((quarter.effective_time_ms(60.0) - 1_000.0).abs() < 1e-3);
    assert!((quarter.effective_time_ms(140.0) - 60_000.0 / 140.0).abs() < 1e-3);

    // And the setting everybody actually uses.
    let dotted_eighth = synced(NoteDivision::EighthDotted);
    assert!((dotted_eighth.effective_time_ms(120.0) - 375.0).abs() < 1e-3);
}

#[test]
fn an_unsynced_delay_ignores_the_tempo_entirely() {
    let mut config = DelayConfig::new();
    config.sync = false;
    config.time_ms = 230.0;
    for bpm in [40.0, 120.0, 300.0] {
        assert!(
            (config.effective_time_ms(bpm) - 230.0).abs() < 1e-3,
            "a free delay moved when the tempo did"
        );
    }
}

#[test]
fn a_synced_delay_ignores_its_millisecond_knob() {
    // Both values are kept — switching sync off has to give back the time that
    // was set — but only one of them is in force at a time.
    let mut config = synced(NoteDivision::Quarter);
    config.time_ms = 17.0;
    assert!((config.effective_time_ms(120.0) - 500.0).abs() < 1e-3);
    config.sync = false;
    assert!(
        (config.effective_time_ms(120.0) - 17.0).abs() < 1e-3,
        "switching sync off should give back the time the knob was on"
    );
}

#[test]
fn a_note_longer_than_the_line_is_clamped_rather_than_wrapping() {
    // A whole note at 30 bpm is eight seconds and the line holds four. Clamped
    // is the wrong *time*, and it is the only answer that is not a read past
    // the end of the buffer — the DSP clamps again for the same reason.
    let whole = synced(NoteDivision::Whole);
    assert!(whole.effective_time_ms(30.0) <= MAX_DELAY_MS);
    assert!(whole.effective_time_ms(30.0) >= MAX_DELAY_MS - 1e-3);
    // At 60 bpm a whole note is exactly the longest line, which is where the
    // range was set to so that the longest useful division just fits.
    assert!((whole.effective_time_ms(60.0) - 4_000.0).abs() < 1e-3);
}

#[test]
fn a_tempo_that_is_not_a_tempo_does_not_produce_an_infinite_delay() {
    // A malformed project, or a snapshot taken before anything set one. Every
    // number downstream of this is an index into a buffer.
    let quarter = synced(NoteDivision::Quarter);
    for bpm in [0.0, -20.0, f32::NAN, f32::INFINITY] {
        let time = quarter.effective_time_ms(bpm);
        assert!(
            time.is_finite() && (1.0..=MAX_DELAY_MS).contains(&time),
            "a tempo of {bpm} gave a delay time of {time}"
        );
    }
}

// ------------------------------------------------- the tempo on the timeline

#[test]
fn a_timeline_with_no_tempo_reads_the_default_rather_than_zero() {
    // The whole point of a default: a node asking the tempo of an empty
    // timeline must get a tempo, not a division by zero. `CompiledTimeline`
    // is `Default` in several places that never compile a project.
    let timeline = CompiledTimeline::empty();
    assert_eq!(timeline.bpm_at(0), DEFAULT_BPM);
    assert_eq!(timeline.bpm_at(48_000 * 600), DEFAULT_BPM);
}

#[test]
fn a_timeline_reads_back_the_tempo_in_force_at_a_sample() {
    let mut timeline = CompiledTimeline::empty();
    timeline.tempo = vec![(0, 120.0), (48_000, 90.0), (96_000, 140.0)];

    assert_eq!(timeline.bpm_at(0), 120.0);
    assert_eq!(timeline.bpm_at(47_999), 120.0, "up to but not including it");
    assert_eq!(timeline.bpm_at(48_000), 90.0, "and from it");
    assert_eq!(timeline.bpm_at(95_999), 90.0);
    assert_eq!(timeline.bpm_at(96_000), 140.0);
    assert_eq!(timeline.bpm_at(i64::MAX), 140.0, "the last one runs on");
}

#[test]
fn a_sample_before_the_first_segment_reads_the_first_segment() {
    // Negative positions are reachable: the transport clamps at zero, but a
    // node doing its own arithmetic on `position_sample` need not.
    let mut timeline = CompiledTimeline::empty();
    timeline.tempo = vec![(0, 90.0)];
    assert_eq!(timeline.bpm_at(-1_000), 90.0);
}

/// And the chooser says what the enum says.
///
/// The positions are written out beside the specs rather than built from
/// `NoteDivision::ALL` — a `ParamSpec` is a `static` read on the audio thread
/// and a `const fn` cannot call a method on an enum — so this is what stops
/// the two drifting, exactly as `a_bands_chooser_reads_the_way_the_document_does`
/// does for the EQ.
#[test]
fn the_delays_chooser_reads_the_way_the_document_does() {
    use fontelle_types::{EffectConfig, EffectKind};

    let config = EffectConfig::new(EffectKind::Delay);
    let positions = config
        .specs()
        .iter()
        .find(|spec| spec.id == "division")
        .expect("a synced delay has one")
        .positions;
    let labels: Vec<&str> = NoteDivision::ALL.iter().map(|d| d.label()).collect();
    assert_eq!(positions, labels.as_slice());
}

/// A fresh delay opens on a value somebody would pick, and the spec agrees.
#[test]
fn a_fresh_delay_is_an_eighth_and_not_synced() {
    let config = DelayConfig::new();
    assert!(!config.sync, "a delay opens on its millisecond knob");
    assert_eq!(config.division, NoteDivision::Eighth);
    // Which is where the spec's default index has to point, or the chooser
    // jumps the first time anybody touches it.
    let index = NoteDivision::ALL
        .iter()
        .position(|d| *d == NoteDivision::Eighth)
        .unwrap();
    let spec = fontelle_types::EffectConfig::new(fontelle_types::EffectKind::Delay)
        .specs()
        .iter()
        .find(|spec| spec.id == "division")
        .copied()
        .expect("a synced delay has one");
    assert_eq!(spec.default, index as f32);
}
