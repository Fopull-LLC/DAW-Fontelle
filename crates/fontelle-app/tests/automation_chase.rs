//! Playback that starts inside an automation clip plays the clip's value.
//!
//! > *"if i have an automation clip right now thats like setting something to
//! > 0% vs a 100 % wet then if i dont start the playhead before the start of
//! > the automation clip that sets it to 0 it will put it at 100 even though
//! > the value at that point where the playhead is at on the automation clip
//! > should be computing 0."*
//!
//! Automation compiles to a value **where the curve changes**, so a flat
//! stretch says nothing after its first moment, and every seek and every loop
//! wrap resets the graph — which lets the parameter fall back to its knob
//! until the curve next moves. Two things close that:
//!
//! - the **compiler** anchors every automated parameter at the loop's start,
//!   so a wrap is told where it is;
//! - the window **chases**: when playback starts or jumps it sends each
//!   automated parameter its value at the playhead
//!   (`StudioHost::chase_automation`) — or, before the first clip, tells it to
//!   go back to its knob.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{
    BLOCK_SIZE, GraphSource, LiveEventSource, Transport, TransportReader, graph_channel,
    live_event_channel, timeline_channel,
};
use fontelle_model::{AddNotes, Command, Note};
use fontelle_types::{CompiledTimeline, EventPayload, PPQN, ParamTarget, Tick};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

const BAR: Tick = PPQN * 4;

fn a_note(start: Tick, length: Tick) -> Note {
    Note {
        start,
        length,
        key: 60,
        velocity: 110,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

struct Rig {
    session: Session,
    live: LiveEventSource,
    graphs: GraphSource,
}

/// A studio whose one clip holds a long note from bar 3, with the RT ends of
/// its graph and its live port kept here so a test can play it.
fn rig() -> Rig {
    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    AddNotes::new(clip, vec![a_note(BAR * 2, BAR * 2)])
        .apply(&mut project)
        .expect("the clip takes a note");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timelines) = timeline_channel(CompiledTimeline::empty());
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let library = SampleLibrary::new();
    let realised = fontelle_app::realise(&project, &library, options).expect("realises");
    let (graphs, source) = graph_channel(realised.graph);
    let (live, mut ports) = live_event_channel(2, 256);
    let session = Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_audition(Box::new(ports.claim().expect("a port")))
    .with_settings_path(std::env::temp_dir().join(format!(
        "fontelle-chase-{}-{:?}.json",
        std::process::id(),
        std::thread::current().id()
    )));
    Rig {
        session,
        live,
        graphs: source,
    }
}

fn master(session: &Session) -> usize {
    session.mixer_strips().len() - 1
}

fn master_gain(session: &Session) -> fontelle_types::ParamAddress {
    let id = session.mixer_track_id(master(session)).expect("a master");
    ParamTarget::TrackGain(id).address()
}

/// The master fader automated flat at -60 dB — silence — over `span`, with
/// the fader itself left at 0 dB.
fn silenced_by_automation(rig: &mut Rig, span: Option<(Tick, Tick)>) {
    let strip = master(&rig.session);
    let address = master_gain(&rig.session);
    rig.session.set_loop_range(span);
    rig.session.set_track_gain_db(strip, -60.0);
    rig.session.end_gesture();
    rig.session
        .create_automation(&address, "Master \u{2014} gain", BAR * 8);
    rig.session.set_track_gain_db(strip, 0.0);
    rig.session.end_gesture();
    rig.session.set_loop_range(None);
}

fn chased(rig: &mut Rig) -> Vec<(String, f64)> {
    rig.live
        .drain(0, false)
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ParamValue { target, value } => {
                Some((target.as_str().to_string(), *value))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_loop_is_told_every_automated_value_at_its_start() {
    let mut rig = rig();
    silenced_by_automation(&mut rig, None);
    rig.session.set_loop_range(Some((BAR * 2, BAR * 4)));
    let at = rig.session.sample_of_song_tick(BAR * 2);
    let address = master_gain(&rig.session);
    let timeline = rig.session.compiled();
    let anchored = timeline.events.iter().any(|event| {
        event.sample == at
            && matches!(&event.payload, EventPayload::ParamValue { target, value }
                if *target == address && value.abs() < 0.01)
    });
    assert!(
        anchored,
        "no value for the master fader at the loop's start"
    );
}

#[test]
fn a_chase_sends_the_curves_value_at_the_playhead() {
    let mut rig = rig();
    silenced_by_automation(&mut rig, None);
    chased(&mut rig); // whatever the setup sent
    let at = rig.session.sample_of_song_tick(BAR * 3);
    StudioHost::chase_automation(&mut rig.session, at);
    let address = master_gain(&rig.session);
    let sent = chased(&mut rig);
    let value = sent
        .iter()
        .find(|(target, _)| *target == address.as_str())
        .map(|(_, value)| *value)
        .expect("the master fader was not chased");
    assert!(value.abs() < 0.01, "chased to {value}, not the curve's 0");
}

#[test]
fn a_chase_before_the_first_clip_hands_the_parameter_back_to_its_knob() {
    let mut rig = rig();
    silenced_by_automation(&mut rig, Some((BAR * 4, BAR * 8)));
    chased(&mut rig);
    StudioHost::chase_automation(&mut rig.session, 0);
    let address = master_gain(&rig.session);
    let sent = chased(&mut rig);
    let value = sent
        .iter()
        .find(|(target, _)| *target == address.as_str())
        .map(|(_, value)| *value)
        .expect("the master fader was not told anything");
    assert!(
        value.is_nan(),
        "before the clip it is the knob's, not {value}"
    );
}

/// Plays `blocks` blocks from `from` the way the audio callback does, and
/// returns the loudest sample.
fn play_from(rig: &mut Rig, transport: &Transport, from: i64, blocks: usize) -> f32 {
    let timeline = rig.session.compiled();
    let mut reader = TransportReader::new();
    transport.seek(from);
    // The window's order: the seek, then the chase, then roll.
    StudioHost::chase_automation(&mut rig.session, from);
    transport.play();
    let mut loudest = 0.0f32;
    for _ in 0..blocks {
        let live = rig.live.drain(reader.position(), false);
        rig.graphs.take_update();
        let graph = rig.graphs.current();
        let step = reader.next_step(transport, &timeline, BLOCK_SIZE, BLOCK_SIZE, false);
        if step.reset {
            graph.reset_sequenced();
        }
        if !step.process {
            continue;
        }
        graph.process_block_with_live(step.events, live, step.snapshot, step.range.clone());
        for bus in 0..2 {
            loudest = graph.buffer_pool.buffer_mut(bus)[..step.frames]
                .iter()
                .fold(loudest, |m, s| m.max(s.abs()));
        }
    }
    loudest
}

#[test]
fn playing_from_inside_a_clip_that_silences_the_master_is_silent() {
    let mut rig = rig();
    // Without automation the note is loud: the thing being silenced exists.
    let transport = Transport::new();
    let from = rig.session.sample_of_song_tick(BAR * 2);
    let blocks = (SR as usize / 2) / BLOCK_SIZE;
    let open = play_from(&mut rig, &transport, from, blocks);
    assert!(
        open > 0.05,
        "the note is not audible to begin with ({open})"
    );

    silenced_by_automation(&mut rig, None);
    let transport = Transport::new();
    let heard = play_from(&mut rig, &transport, from, blocks);
    assert!(
        heard < open * 0.05,
        "started inside the clip, the master played at its fader: {heard} against {open}"
    );
}
