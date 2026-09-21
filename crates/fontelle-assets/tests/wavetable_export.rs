//! A table the editor exports (`docs/flopsynth-next.md` §4.3) comes back
//! through the drop frame for frame: the file is the one Serum reads, and
//! the one this program's own `load_wavetable` reads.

use fontelle_core::{UserWavetable, WavetableEdit};
use fontelle_dsp::WAVETABLE_LEN;

#[test]
fn an_exported_table_drops_back_in_frame_for_frame() {
    let mut table = UserWavetable::blank("Drawn");
    table
        .apply(&WavetableEdit::Formula {
            frame: 0,
            text: "sin(x)".into(),
        })
        .unwrap();
    table.apply(&WavetableEdit::AddFrame { after: 0 }).unwrap();
    table
        .apply(&WavetableEdit::Formula {
            frame: 1,
            text: "saw(x)".into(),
        })
        .unwrap();
    let bytes = table.export_wav();
    let decoded = fontelle_assets::read_audio(&bytes, "drawn.wav").expect("a wav");
    assert_eq!(decoded.channels, 1);
    assert_eq!(decoded.frames, 2 * WAVETABLE_LEN);
    let back = UserWavetable {
        name: "back".into(),
        frames: decoded.frames / WAVETABLE_LEN,
        samples: decoded.samples,
    };
    assert_eq!(back.frames, 2);
    let amps =
        |frame: usize| -> Vec<f32> { back.harmonics(frame).iter().map(|(a, _)| *a).collect() };
    let (a, b) = (amps(0), amps(1));
    assert!(
        a[0] > 0.9 && a[1] < 0.02,
        "the sine came back: {:?}",
        &a[..3]
    );
    assert!(
        (b[0] / b[1] - 2.0).abs() < 0.1,
        "the saw came back: {:?}",
        &b[..3]
    );
    // Sample for sample, to 16 bits.
    for (x, y) in table.samples.iter().zip(&back.samples).step_by(37) {
        assert!((x - y).abs() < 1.0 / 16_000.0, "{x} vs {y}");
    }
}
