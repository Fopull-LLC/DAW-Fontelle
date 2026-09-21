//! The wavetable editor's operations (`docs/flopsynth-next.md` §4.3):
//! everything the inspector does to a patch's own table, as pure edits on
//! `UserWavetable` — draw a segment, set a harmonic, apply a formula, add,
//! copy and remove frames, morph between two, export a file Serum reads.
//! The window's gestures (`fontelle-ui/tests/wavetable_editor.rs`) turn
//! into these; the sound of the result is `fontelle-dsp`'s
//! (`wavetable_user.rs`).

use fontelle_core::{UserWavetable, WavetableEdit};
use fontelle_dsp::WAVETABLE_LEN;

fn blank() -> UserWavetable {
    UserWavetable::blank("Drawn")
}

fn apply(table: &mut UserWavetable, edit: WavetableEdit) {
    table
        .apply(&edit)
        .unwrap_or_else(|e| panic!("{edit:?}: {e}"));
}

/// The harmonic amplitudes of a frame, from its own analysis.
fn amps(table: &UserWavetable, frame: usize) -> Vec<f32> {
    table.harmonics(frame).iter().map(|(a, _)| *a).collect()
}

#[test]
fn a_blank_table_is_one_silent_frame_of_the_tables_own_length() {
    let table = blank();
    assert_eq!(table.frames, 1);
    assert_eq!(table.samples.len(), WAVETABLE_LEN);
    assert!(table.samples.iter().all(|s| *s == 0.0));
    assert_eq!(table.name, "Drawn");
}

/// A table that arrived from a file at some other length is made
/// editable by being laid out at the table's own length per frame — the
/// same read the oscillator does — before the first edit touches it.
#[test]
fn a_dropped_sound_is_laid_out_frame_by_frame_before_it_is_edited() {
    let sound: Vec<f32> = (0..3_000)
        .map(|i| (std::f32::consts::TAU * i as f32 / 1_500.0).sin())
        .collect();
    let mut table = UserWavetable {
        name: "sound".into(),
        frames: 2,
        samples: sound,
    };
    apply(&mut table, WavetableEdit::Normalise { frame: 0 });
    assert_eq!(table.frames, 2);
    assert_eq!(table.samples.len(), 2 * WAVETABLE_LEN);
    // Each frame is still one cycle of the sine.
    for frame in 0..2 {
        let a = amps(&table, frame);
        assert!(
            a[0] > 0.9 && a[1..].iter().all(|h| *h < 0.05),
            "{:?}",
            &a[..4]
        );
    }
}

/// Drawing: a segment from one point to another sets the samples under
/// it to the line between, in the table's own units — x across the cycle
/// in 0..1, y in −1..1 — and nothing outside it.
#[test]
fn a_drawn_segment_is_a_line_and_touches_nothing_outside_it() {
    let mut table = blank();
    apply(
        &mut table,
        WavetableEdit::Draw {
            frame: 0,
            from: (0.25, -1.0),
            to: (0.75, 1.0),
        },
    );
    let frame = table.frame(0);
    let at = |x: f32| frame[(x * WAVETABLE_LEN as f32) as usize];
    assert!((at(0.25) + 1.0).abs() < 0.02, "{}", at(0.25));
    assert!(at(0.5).abs() < 0.02, "{}", at(0.5));
    assert!((at(0.749) - 1.0).abs() < 0.02, "{}", at(0.749));
    assert_eq!(at(0.1), 0.0);
    assert_eq!(at(0.9), 0.0);
    // Drawn the other way round is the same line.
    let mut back = blank();
    apply(
        &mut back,
        WavetableEdit::Draw {
            frame: 0,
            from: (0.75, 1.0),
            to: (0.25, -1.0),
        },
    );
    assert_eq!(back.frame(0), table.frame(0));
}

/// A square drawn as four segments is a square: the odd harmonics at 1/n,
/// no even ones.
#[test]
fn a_drawn_square_has_a_squares_harmonics() {
    let mut table = blank();
    for (from, to) in [((0.0, 1.0), (0.5, 1.0)), ((0.5, -1.0), (1.0, -1.0))] {
        apply(&mut table, WavetableEdit::Draw { frame: 0, from, to });
    }
    let a = amps(&table, 0);
    assert!(
        (a[2] / a[0] - 1.0 / 3.0).abs() < 0.03,
        "third: {}",
        a[2] / a[0]
    );
    assert!((a[4] / a[0] - 0.2).abs() < 0.03, "fifth: {}", a[4] / a[0]);
    assert!(a[1] < 0.02 && a[3] < 0.02, "no even: {:?}", &a[..5]);
}

/// The harmonic bars: setting one puts a partial there, and what the
/// analysis reads back is what was set.
#[test]
fn a_harmonic_set_is_a_harmonic_read_back() {
    let mut table = blank();
    apply(
        &mut table,
        WavetableEdit::Harmonic {
            frame: 0,
            index: 0,
            amplitude: 1.0,
        },
    );
    apply(
        &mut table,
        WavetableEdit::Harmonic {
            frame: 0,
            index: 2,
            amplitude: 0.5,
        },
    );
    let a = amps(&table, 0);
    assert!(
        (a[0] - 1.0).abs() < 0.02 && (a[2] - 0.5).abs() < 0.02,
        "{:?}",
        &a[..4]
    );
    assert!(a[1] < 0.01 && a[3..].iter().all(|h| *h < 0.01));
    // The sixty-fourth is the last bar; past it nothing.
    assert_eq!(table.harmonics(0).len(), 64);
    // A harmonic set on a drawn frame keeps the frame's other partials:
    // the bars are an edit on the analysis, not a replacement.
    let mut drawn = blank();
    for (from, to) in [((0.0, 1.0), (0.5, 1.0)), ((0.5, -1.0), (1.0, -1.0))] {
        apply(&mut drawn, WavetableEdit::Draw { frame: 0, from, to });
    }
    apply(
        &mut drawn,
        WavetableEdit::Harmonic {
            frame: 0,
            index: 2,
            amplitude: 0.0,
        },
    );
    let a = amps(&drawn, 0);
    assert!(a[2] < 0.02, "the third is gone: {}", a[2]);
    assert!(
        (a[4] / a[0] - 0.2).abs() < 0.03,
        "the fifth stays: {}",
        a[4] / a[0]
    );
}

/// The formula: `x` is the phase in 0..1, the waves take cycles, and the
/// usual arithmetic applies.
#[test]
fn a_formula_makes_the_frame_it_says() {
    let mut table = blank();
    apply(
        &mut table,
        WavetableEdit::Formula {
            frame: 0,
            text: "sin(x*2) + 0.3*saw(x)".into(),
        },
    );
    let a = amps(&table, 0);
    // A sine at the second harmonic, and a saw's 1/n on top of it — the
    // whole normalised to a peak of one.
    assert!(
        a[1] > a[0] * 2.0,
        "the second harmonic is the sine's: {:?}",
        &a[..4]
    );
    assert!(
        (a[0] / a[2] - 3.0).abs() < 0.3,
        "the saw's 1/n on the rest: {:?}",
        &a[..4]
    );
    // The frame is normalised to full scale.
    let peak = table.frame(0).iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!((peak - 1.0).abs() < 1e-3, "{peak}");
    // Every function the window offers parses.
    for text in [
        "sin(x)",
        "cos(x)",
        "tri(x)",
        "square(x)",
        "saw(x)",
        "abs(sin(x))",
        "tanh(3*sin(x))",
        "sin(x)^3",
        "(1-x)*sin(x*8)",
        "-sin(x)",
    ] {
        let mut table = blank();
        assert!(
            table
                .apply(&WavetableEdit::Formula {
                    frame: 0,
                    text: text.into(),
                })
                .is_ok(),
            "{text} did not parse"
        );
    }
    // And a formula that is not one says so rather than drawing nothing.
    let mut table = blank();
    let err = table
        .apply(&WavetableEdit::Formula {
            frame: 0,
            text: "sin(x".into(),
        })
        .unwrap_err();
    assert!(!err.is_empty());
    let err = table
        .apply(&WavetableEdit::Formula {
            frame: 0,
            text: "foo(x)".into(),
        })
        .unwrap_err();
    assert!(err.contains("foo"), "{err}");
}

#[test]
fn frames_are_added_copied_and_removed() {
    let mut table = blank();
    apply(
        &mut table,
        WavetableEdit::Formula {
            frame: 0,
            text: "sin(x)".into(),
        },
    );
    // Add after: a silent frame at 1.
    apply(&mut table, WavetableEdit::AddFrame { after: 0 });
    assert_eq!(table.frames, 2);
    assert!(table.frame(1).iter().all(|s| *s == 0.0));
    assert!(amps(&table, 0)[0] > 0.9, "the first is still the sine");
    // Copy: frame 0 again at 1, the silent one pushed to 2.
    apply(&mut table, WavetableEdit::CopyFrame { frame: 0 });
    assert_eq!(table.frames, 3);
    assert_eq!(table.frame(1), table.frame(0));
    assert!(table.frame(2).iter().all(|s| *s == 0.0));
    // Remove the middle.
    apply(&mut table, WavetableEdit::RemoveFrame { frame: 1 });
    assert_eq!(table.frames, 2);
    assert!(table.frame(1).iter().all(|s| *s == 0.0));
    // The last frame cannot be removed: a table is at least one.
    apply(&mut table, WavetableEdit::RemoveFrame { frame: 1 });
    assert!(
        table
            .apply(&WavetableEdit::RemoveFrame { frame: 0 })
            .is_err(),
        "one frame stays"
    );
    assert_eq!(table.frames, 1);
    // And no more than the table can hold.
    for _ in 0..300 {
        let _ = table.apply(&WavetableEdit::AddFrame { after: 0 });
    }
    assert_eq!(table.frames, fontelle_dsp::MAX_USER_FRAMES);
    const { assert!(fontelle_dsp::MAX_USER_FRAMES >= 256, "§4.3: 256 frames") };
}

/// Morph fills the frames between two with the way from one to the
/// other: linearly on the samples, or spectrally on the partials — a sine
/// to a sine an octave up passes through a sine-plus-octave rather than
/// through a beat.
#[test]
fn morph_fills_the_frames_between() {
    let mut table = blank();
    apply(
        &mut table,
        WavetableEdit::Formula {
            frame: 0,
            text: "sin(x)".into(),
        },
    );
    for _ in 0..4 {
        apply(&mut table, WavetableEdit::AddFrame { after: 0 });
    }
    apply(
        &mut table,
        WavetableEdit::Formula {
            frame: 4,
            text: "square(x)".into(),
        },
    );
    let mut linear = table.clone();
    apply(
        &mut linear,
        WavetableEdit::Morph {
            from: 0,
            to: 4,
            spectral: false,
        },
    );
    // Halfway is half of each, sample for sample.
    let (a, b, mid) = (linear.frame(0), linear.frame(4), linear.frame(2));
    for i in (0..WAVETABLE_LEN).step_by(97) {
        assert!(((a[i] + b[i]) * 0.5 - mid[i]).abs() < 1e-4, "at {i}");
    }
    // Spectral: halfway, the partials are halfway — a sine's first at the
    // mean of the two firsts, the square's third at half.
    let mut spectral = table;
    apply(
        &mut spectral,
        WavetableEdit::Morph {
            from: 0,
            to: 4,
            spectral: true,
        },
    );
    let (a0, a4, a2) = (amps(&spectral, 0), amps(&spectral, 4), amps(&spectral, 2));
    assert!(
        ((a0[0] + a4[0]) * 0.5 - a2[0]).abs() < 0.03,
        "{} {} {}",
        a0[0],
        a4[0],
        a2[0]
    );
    assert!((a4[2] * 0.5 - a2[2]).abs() < 0.03, "{} {}", a4[2], a2[2]);
    // The ends are untouched either way.
    assert_eq!(spectral.frame(0), linear.frame(0));
    assert_eq!(spectral.frame(4), linear.frame(4));
}

/// The export: a 16-bit mono WAV of `frames × 2048` samples — the layout
/// Serum reads a table from, and the one this program's own drop reads
/// back frame for frame.
#[test]
fn export_is_a_wav_serum_reads() {
    let mut table = blank();
    apply(
        &mut table,
        WavetableEdit::Formula {
            frame: 0,
            text: "sin(x)".into(),
        },
    );
    apply(&mut table, WavetableEdit::AddFrame { after: 0 });
    apply(
        &mut table,
        WavetableEdit::Formula {
            frame: 1,
            text: "saw(x)".into(),
        },
    );
    let bytes = table.export_wav();
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    // 16-bit, mono.
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
    let bits = u16::from_le_bytes([bytes[34], bytes[35]]);
    assert_eq!((channels, bits), (1, 16));
    let data_len = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as usize;
    assert_eq!(data_len, 2 * WAVETABLE_LEN * 2);
    // The samples themselves, as 16-bit: the first frame's quarter is a
    // sine's peak.
    let sample = |n: usize| i16::from_le_bytes([bytes[44 + 2 * n], bytes[45 + 2 * n]]);
    assert!(
        sample(WAVETABLE_LEN / 4) > 32_000,
        "{}",
        sample(WAVETABLE_LEN / 4)
    );
    assert!(sample(0).abs() < 100, "{}", sample(0));
    // The drop reads it back frame for frame:
    // `fontelle-assets/tests/wavetable_export.rs`.
}
