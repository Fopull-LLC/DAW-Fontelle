//! basic-pitch in tract (plan §4 P0): the model loads, is the reviewed file,
//! gives what Python basic-pitch gives on the same samples, and turns into
//! the notes a musician would write down.

#![cfg(feature = "model")]

use fontelle_analysis::testsignals::{self, ExpectedNote, Fixture};
use fontelle_analysis::transcribe::basic_pitch::{
    BasicPitch, MODEL_BYTES, MODEL_SHA256, SAMPLE_RATE,
};
use fontelle_analysis::transcribe::notes::{N_CONTOUR_BINS, N_KEYS};
use fontelle_analysis::transcribe::{
    NoteEvent, NoteParams, Posteriorgrams, notes_from_posteriorgrams,
};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

fn model() -> &'static BasicPitch {
    static MODEL: OnceLock<BasicPitch> = OnceLock::new();
    MODEL.get_or_init(|| BasicPitch::load().expect("the model loads"))
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn the_model_loads() {
    let post = model()
        .posteriorgrams(&vec![0.0; SAMPLE_RATE as usize])
        .unwrap();
    assert!(
        post.frames > 80,
        "a second is about 86 frames, got {}",
        post.frames
    );
    assert_eq!(post.onset.len(), post.frames * N_KEYS);
    assert_eq!(post.note.len(), post.frames * N_KEYS);
    assert_eq!(post.contour.len(), post.frames * N_CONTOUR_BINS);
}

#[test]
fn model_file_hash_is_pinned() {
    // The hash is written twice on purpose: here, and in the crate beside the
    // bytes. A changed model has to change both, and licenses/MODELS.md.
    const REVIEWED: &str = "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec";
    let actual: String = Sha256::digest(MODEL_BYTES)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(actual, REVIEWED);
    assert_eq!(MODEL_SHA256, REVIEWED);
    assert_eq!(MODEL_BYTES.len(), 230_444);
}

/// The reference posteriorgrams, every `stride`-th frame (see
/// `tests/fixtures/make_reference.py` for the layout).
struct Reference {
    frames: usize,
    stride: usize,
    rows: Vec<Vec<f32>>,
}

fn read_reference(name: &str) -> Reference {
    let bytes = std::fs::read(fixture_path(&format!("{name}.post.bin"))).unwrap();
    let word = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    let (frames, stride) = (word(0), word(4));
    let width = 2 * N_KEYS + N_CONTOUR_BINS;
    let rows = bytes[8..]
        .chunks_exact(2 * width)
        .map(|row| {
            row.as_chunks::<2>()
                .0
                .iter()
                .map(|b| f32::from(u16::from_le_bytes(*b)) / 65535.0)
                .collect()
        })
        .collect::<Vec<Vec<f32>>>();
    assert_eq!(rows.len(), frames.div_ceil(stride));
    Reference {
        frames,
        stride,
        rows,
    }
}

#[test]
fn posteriorgrams_match_reference() {
    for fixture in testsignals::reference_fixtures() {
        let reference = read_reference(fixture.name);
        let post = model().posteriorgrams(&fixture.samples).unwrap();
        assert_eq!(
            post.frames, reference.frames,
            "{}: frame count",
            fixture.name
        );
        // A window of nothing but digital silence has no level to normalise
        // its log-CQT by, and the two runtimes settle that 0/0 differently:
        // onnxruntime's rounding noise is stretched to full scale (up to 0.32
        // here), tract's exact zeros give a flat 0.108 everywhere. Ours must
        // be the flat one, under every threshold. Everywhere there is sound
        // the two agree to the summation order.
        let silent_window = |w: usize| {
            let first = (w * 36_164).saturating_sub(3_840);
            let last = (w * 36_164 + 43_844)
                .saturating_sub(3_840)
                .min(fixture.samples.len());
            fixture
                .samples
                .get(first..last)
                .is_none_or(|s| s.iter().all(|&x| x == 0.0))
        };
        let mut worst = 0.0f32;
        let mut checked = 0;
        for (r, row) in reference.rows.iter().enumerate() {
            let t = r * reference.stride;
            let ours = post.onset[t * N_KEYS..(t + 1) * N_KEYS]
                .iter()
                .chain(&post.note[t * N_KEYS..(t + 1) * N_KEYS])
                .chain(&post.contour[t * N_CONTOUR_BINS..(t + 1) * N_CONTOUR_BINS]);
            if silent_window(t / 142) {
                for a in ours {
                    assert!(*a < 0.2, "{}: frame {t} in silence: {a}", fixture.name);
                }
                continue;
            }
            checked += 1;
            for (a, b) in ours.zip(row) {
                worst = worst.max((a - b).abs());
            }
        }
        assert!(
            checked * reference.stride * 2 > reference.frames,
            "{}: most frames checked",
            fixture.name
        );
        // The reference is quantised to 1/65535; onnxruntime and tract differ
        // in summation order only.
        assert!(worst < 1e-3, "{}: worst difference {worst}", fixture.name);
    }
}

#[test]
fn notes_match_reference() {
    for fixture in testsignals::reference_fixtures() {
        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(fixture_path(&format!("{}.notes.json", fixture.name))).unwrap(),
        )
        .unwrap();
        let mut expected: Vec<(usize, usize, u8)> = json["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| {
                (
                    n[0].as_u64().unwrap() as usize,
                    n[1].as_u64().unwrap() as usize,
                    n[2].as_u64().unwrap() as u8,
                )
            })
            .collect();
        expected.sort();
        let post = model().posteriorgrams(&fixture.samples).unwrap();
        let notes = notes_from_posteriorgrams(&post, &NoteParams::default());
        let mut ours: Vec<(usize, usize, u8)> = notes
            .iter()
            .map(|n| (n.start_frame, n.end_frame, n.midi))
            .collect();
        ours.sort();
        assert_eq!(ours, expected, "{}", fixture.name);
        for n in &json["notes"].as_array().unwrap().clone() {
            let key = (
                n[0].as_u64().unwrap() as usize,
                n[1].as_u64().unwrap() as usize,
                n[2].as_u64().unwrap() as u8,
            );
            let mine = notes
                .iter()
                .find(|m| (m.start_frame, m.end_frame, m.midi) == key)
                .unwrap();
            assert!((mine.amplitude as f64 - n[3].as_f64().unwrap()).abs() < 1e-3);
            let bends: Vec<i8> = n[4]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b.as_i64().unwrap() as i8)
                .collect();
            assert_eq!(
                mine.bends.as_deref(),
                Some(&bends[..]),
                "{}: bends of {key:?}",
                fixture.name
            );
        }
    }
}

/// Every expected note found once, its onset within `onset_tol` seconds and
/// its end within `end_tol`, and nothing else.
fn assert_notes(fixture: &Fixture, notes: &[NoteEvent], onset_tol: f64, end_tol: f64) {
    let describe = |ns: &[NoteEvent]| {
        ns.iter()
            .map(|n| format!("{:.3}-{:.3} {}", n.start, n.end, n.midi))
            .collect::<Vec<_>>()
            .join(", ")
    };
    assert_eq!(
        notes.len(),
        fixture.notes.len(),
        "{}: got [{}], expected {:?}",
        fixture.name,
        describe(notes),
        fixture.notes
    );
    for &ExpectedNote { start, end, midi } in &fixture.notes {
        let found = notes.iter().find(|n| {
            n.midi == midi && (n.start - start).abs() <= onset_tol && (n.end - end).abs() <= end_tol
        });
        assert!(
            found.is_some(),
            "{}: no {midi} at {start:.3}-{end:.3} in [{}]",
            fixture.name,
            describe(notes)
        );
    }
}

#[test]
fn a_c_major_triad_comes_out_as_three_notes() {
    let fixture = testsignals::c_major_triad(SAMPLE_RATE);
    let notes = model()
        .transcribe(&fixture.samples, SAMPLE_RATE, &NoteParams::default())
        .unwrap();
    assert_notes(&fixture, &notes, 0.030, 0.060);
}

#[test]
fn a_c_major_triad_at_44_1_khz_is_the_same_three_notes() {
    let fixture = testsignals::c_major_triad(44_100);
    let notes = model()
        .transcribe(&fixture.samples, 44_100, &NoteParams::default())
        .unwrap();
    assert_notes(&fixture, &notes, 0.030, 0.060);
}

#[test]
fn a_sung_melody_comes_out_note_for_note() {
    let fixture = testsignals::vibrato_melody(SAMPLE_RATE);
    let notes = model()
        .transcribe(&fixture.samples, SAMPLE_RATE, &NoteParams::default())
        .unwrap();
    // Onsets to 30 ms. Ends looser: the F's last vibrato swing down, in its
    // release, reads to the model as the E that follows, and basic-pitch
    // (Python too) ends the F 90 ms early there.
    assert_notes(&fixture, &notes, 0.030, 0.100);
}

#[test]
fn melody_over_chords_comes_out_layered() {
    let fixture = testsignals::melody_and_chords(SAMPLE_RATE);
    let notes = model()
        .transcribe(&fixture.samples, SAMPLE_RATE, &NoteParams::default())
        .unwrap();
    assert_notes(&fixture, &notes, 0.030, 0.060);
}

#[test]
fn silence_gives_no_notes() {
    let notes = model()
        .transcribe(&vec![0.0; 3 * 44_100], 44_100, &NoteParams::default())
        .unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let post = model()
        .posteriorgrams(&vec![0.0; 3 * SAMPLE_RATE as usize])
        .unwrap();
    assert!(notes_from_posteriorgrams(&post, &NoteParams::default()).is_empty());
}

#[test]
fn frames_sit_where_the_windows_put_them() {
    // Frame f of the unwrapped output is frame f % 142 past the overlap of
    // window f / 142, and windows hop 36 164 samples.
    assert_eq!(Posteriorgrams::frame_time(0), 0.0);
    assert!((Posteriorgrams::frame_time(141) - 141.0 * 256.0 / 22_050.0).abs() < 1e-12);
    assert!((Posteriorgrams::frame_time(142) - 36_164.0 / 22_050.0).abs() < 1e-12);
    assert!(
        (Posteriorgrams::frame_time(300) - (2.0 * 36_164.0 + 16.0 * 256.0) / 22_050.0).abs()
            < 1e-12
    );
}

#[test]
fn a_short_clip_still_gets_its_frames() {
    // Shorter than one window: basic-pitch pads it out and keeps
    // len / 36164 * 142 frames.
    let post = model().posteriorgrams(&vec![0.0; 10_000]).unwrap();
    assert_eq!(post.frames, (10_000.0f64 / 36_164.0 * 142.0) as usize);
}
