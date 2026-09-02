//! An automation lane on one of the instrument's own knobs, all the way to the
//! sound.
//!
//! *"i want to be able to right click on a knob and select create automation
//! clip with value and then it appears in my timeline and im able to draw
//! it."* The lane appearing is `instrument_editor.rs`; this is the half that
//! makes it *do* something — the address has to resolve to the channel's node
//! (`realise`), the compiler has to emit values for it, and the node has to
//! apply them to the patch it is playing.
//!
//! Each of those is a place the chain can be complete on both sides and joined
//! at neither, which is why the test drives real blocks through a real graph.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, Session, blank_project};
use fontelle_engine::{
    BLOCK_SIZE, Transport, TransportReader, graph_channel, timeline_channel,
};
use fontelle_model::{AddNotes, Command, Note};
use fontelle_types::{CompiledTimeline, PPQN, ParamAddress};
use fontelle_ui::document::StudioHost;

use common::SR;

fn a_note(start: i64, length: i64, key: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity: 127,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
    }
}

/// A studio holding one long note on the built-in synth, and the RT ends of
/// its two channels.
fn studio() -> (
    Session,
    fontelle_engine::GraphSource,
    fontelle_engine::TimelineSource,
) {
    let mut project = blank_project(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    AddNotes::new(clip, vec![a_note(0, PPQN * 16, 60)])
        .apply(&mut project)
        .expect("the clip takes a note");

    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, timelines) = timeline_channel(CompiledTimeline::empty());
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let library = SampleLibrary::new();
    let realised =
        fontelle_app::realise(&project, &library, options).expect("a blank project must realise");
    let (graphs, source) = graph_channel(realised.graph);
    let session = Session::new(project, library, channel_nodes, publisher, options, clip, None)
        .with_graphs(graphs, realised.track_controls)
        .with_param_nodes(realised.param_nodes)
        .with_spectrum_taps(realised.spectrum_taps);
    (session, source, timelines)
}

/// Renders `blocks` blocks from the start of the song and returns the RMS of
/// the master's left channel.
///
/// The timeline is passed in rather than read off the session's channel: what
/// is being measured is what a *given* compiled song sounds like, and taking
/// it as an argument is what lets the same graph be played before and after an
/// automation lane is drawn.
fn play(
    graphs: &mut fontelle_engine::GraphSource,
    timeline: &CompiledTimeline,
    blocks: usize,
) -> f32 {
    graphs.take_update();
    let transport = Transport::new();
    transport.play();
    let mut reader = TransportReader::new();
    let mut sum = 0.0f64;
    let mut count = 0usize;
    for _ in 0..blocks {
        let step = reader.next_step(&transport, timeline, BLOCK_SIZE, BLOCK_SIZE, false);
        if !step.process {
            continue;
        }
        let graph = graphs.current();
        graph.process_block(step.events, step.snapshot, step.range.clone());
        for sample in &graph.buffer_pool.buffer_mut(0)[..step.frames] {
            sum += f64::from(*sample) * f64::from(*sample);
            count += 1;
        }
    }
    (sum / count.max(1) as f64).sqrt() as f32
}

/// A lane that shuts the filter is **heard** — which is the whole of the ask,
/// and the part that was missing.
#[test]
fn a_lane_on_the_filters_cutoff_is_audible() {
    let cutoff = ParamAddress::new("patch/filter[0]/cutoff");

    let open = {
        let (session, mut graphs, _timelines) = studio();
        let timeline = session.compiled();
        play(&mut graphs, &timeline, 20)
    };

    let (mut session, mut graphs, _timelines) = studio();
    session.automate_instrument_param(&cutoff, 0);
    // Draw the lane down to nothing: two points, both at the bottom.
    let data = session.automation_data().expect("the lane is open");
    let ids: Vec<_> = data.points.iter().map(|(id, _)| id).collect();
    for id in ids {
        session.edit_automation(fontelle_ui::canvas::AutomationEdit::Move {
            ids: vec![id],
            tick_delta: 0,
            value_delta: -1.0,
        });
    }
    let closed = play(&mut graphs, &session.compiled(), 20);

    assert!(open > 0.001, "the note has to be audible to begin with");
    assert!(
        closed < open * 0.5,
        "a lane sweeping the cutoff shut has to be heard: {open} open against {closed} closed"
    );
}

/// The address the panel gives a knob is the one the graph knows it by. If
/// these two ever disagree, every lane in the window points at nothing — and
/// silently, because an unresolved target simply emits no events.
#[test]
fn every_address_the_panel_offers_is_one_the_graph_can_reach() {
    let (session, _graphs, _timelines) = studio();
    let view = session.instrument().expect("the panel is there");
    let reachable = session.automatable_addresses();
    for group in &view.groups {
        for param in &group.params {
            assert!(
                reachable.contains(&param.address),
                "{} is on the panel and the graph cannot reach it",
                param.address
            );
        }
    }
}
