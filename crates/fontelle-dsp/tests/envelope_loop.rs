//! An envelope's loop (`docs/flopsynth-next.md` §3.4): two stages, and the
//! envelope runs from the end of the second back to the start of the first
//! while the note is held. The field lands with the editor; the voice reads
//! it in Phase 3. Absent from the file unless set, so every envelope ever
//! written reads as it did.

use fontelle_dsp::{EnvStage, EnvelopeConfig};

#[test]
fn a_loop_names_two_stages_and_is_absent_from_the_file_unless_set() {
    let plain = EnvelopeConfig::default();
    assert_eq!(plain.loop_stages, None);
    let text = serde_json::to_string(&plain).unwrap();
    assert!(!text.contains("loop"), "{text}");
    let looped = EnvelopeConfig {
        loop_stages: Some((EnvStage::Attack, EnvStage::Decay)),
        ..EnvelopeConfig::default()
    };
    let text = serde_json::to_string(&looped).unwrap();
    assert!(text.contains("loop_stages"), "{text}");
    let back: EnvelopeConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(back.loop_stages, Some((EnvStage::Attack, EnvStage::Decay)));
    // The stages in their order, so a loop's two ends can be compared.
    assert!(EnvStage::Attack < EnvStage::Decay && EnvStage::Decay < EnvStage::Release);
    assert_eq!(EnvStage::ALL.len(), 6);
}
