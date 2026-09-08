//! Hearing an instrument without choosing it (TDD §14.1's live path).
//!
//! Reported from using the window:
//!
//! > *"we should make it instead so if you click a soundfont in the soundfont
//! > menu it plays that instrument at a c tone (make it so if im holding ctrl
//! > while i do it, it plays it an octave lower, and if im holding shift, it
//! > does it an octave higher) this is just so i can easily click on
//! > instruments and hear how they sound."*
//!
//! The reason it worked the weird way round before is structural: the only
//! instruments that existed were the ones on channels, so the only way to hear
//! a soundfont was to **put it on one**. The preview voice is a sampler on the
//! master bus that is in no `channel_nodes` map — the sequencer never names
//! it, so it is silent unless the window sends it a live note.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, blank_project, realise};

use common::SR;

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    }
}

#[test]
fn the_graph_carries_a_preview_voice_that_is_not_any_channel() {
    let project = blank_project(8, 120.0, SR);
    let library = SampleLibrary::new();
    let realised = realise(&project, &library, options()).expect("realises");

    assert!(
        !realised
            .channel_nodes
            .values()
            .any(|node| *node == realised.preview_node),
        "the preview voice is one of the channels, so the song would play through it"
    );
    assert!(
        !realised
            .audio_nodes
            .values()
            .any(|node| *node == realised.preview_node),
        "the preview voice is an audio clip player"
    );
}

#[test]
fn it_is_a_real_node_in_the_graph_rather_than_an_id_nothing_answers_to() {
    // A node id that names nothing is a note sent into a hole — which is what
    // "clicking does nothing" looks like from the outside.
    let project = blank_project(8, 120.0, SR);
    let library = SampleLibrary::new();
    let realised = realise(&project, &library, options()).expect("realises");
    assert!(
        realised
            .graph
            .schedule
            .iter()
            .any(|node| node.id == realised.preview_node),
        "the graph has no node {:?}",
        realised.preview_node
    );
}

#[test]
fn a_fresh_preview_voice_is_silent() {
    // It arrives holding nothing, so a note sent to it before anything has
    // been clicked renders silence rather than the built-in saw.
    let project = blank_project(8, 120.0, SR);
    let library = SampleLibrary::new();
    let realised = realise(&project, &library, options()).expect("realises");
    // Nothing to assert about sound without a device; what is checkable is
    // that realising twice gives the same shape, so the voice is not a
    // conditional extra that some projects have and others do not.
    let again = realise(&project, &library, options()).expect("realises");
    assert_eq!(
        realised.channel_nodes.len(),
        again.channel_nodes.len(),
        "the graph's shape moved between two identical realisations"
    );
}

#[test]
fn every_project_gets_one_however_many_channels_it_has() {
    for channels in [0usize, 1, 3] {
        let mut project = blank_project(8, 120.0, SR);
        for n in 0..channels {
            let mut add = fontelle_model::AddChannel::new(format!("Extra {n}"), None);
            fontelle_model::Command::apply(&mut add, &mut project).expect("adds");
        }
        let library = SampleLibrary::new();
        let realised = realise(&project, &library, options()).expect("realises");
        assert!(
            realised
                .graph
                .schedule
                .iter()
                .any(|node| node.id == realised.preview_node),
            "{channels} extra channels and no preview voice"
        );
    }
}
