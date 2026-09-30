//! Which mixer strips each clip plays into — the host's half of "highlight
//! items that are linked to a mixer track" (`docs/ux-routing-and-learning-plan.md`
//! §3). The panel works out a strip's sources from this and the rack's routes
//! (`fontelle-ui/tests/linked_sources.rs`).

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::{AddAudioClip, AddMixerTrack, ClipSource, Command, Note};
use fontelle_types::{AssetKind, AssetRef, AudioClipData, CompiledTimeline, PPQN};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

#[test]
fn a_clip_reaches_the_strips_of_every_channel_it_plays_and_audio_the_one_it_is_routed_to() {
    let mut project = common::a_project_with_a_clip(2, 120.0, SR);
    let clip = Session::first_clip(&project).unwrap();
    let home = project.channels.keys().next().unwrap();
    // A second instrument, on its own track; the first on another.
    let mut bus = AddMixerTrack::new("Keys");
    bus.apply(&mut project).unwrap();
    let keys = bus.track().unwrap();
    let mut bus = AddMixerTrack::new("Low");
    bus.apply(&mut project).unwrap();
    let low = bus.track().unwrap();
    let mut second = project.channels.get(home).unwrap().clone();
    second.name = "Bass".into();
    second.mixer_track = Some(low);
    let bass = project.channels.insert(second);
    project.channels.get_mut(home).unwrap().mixer_track = Some(keys);
    // One note on the clip's own channel, one on the bass.
    let note = |key: u8, channel| Note {
        start: 0,
        length: PPQN,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel,
    };
    if let ClipSource::Notes(data) = &mut project.clips.get_mut(clip).unwrap().source {
        data.notes.insert(note(60, None));
        data.notes.insert(note(36, Some(bass)));
    }
    let asset = |name: &str| AssetRef {
        id: fontelle_types::AssetId::default(),
        path: name.into(),
        content_hash: 0,
        size: 0,
        kind: AssetKind::Sample,
    };
    let mut routed = AudioClipData::whole(asset("Vox.wav"), i64::from(SR), SR);
    routed.mixer_track = Some(low);
    let mut vox = AddAudioClip::new("Vox.wav", routed, 0, PPQN * 4);
    vox.apply(&mut project).unwrap();
    let mut dry = AddAudioClip::new(
        "Dry.wav",
        AudioClipData::whole(asset("Dry.wav"), i64::from(SR), SR),
        0,
        PPQN * 4,
    );
    dry.apply(&mut project).unwrap();

    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let library = SampleLibrary::new();
    let realised = fontelle_app::realise(&project, &library, options).unwrap();
    let (graphs, _source) = graph_channel(realised.graph);
    let session = Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls);

    // Strips run tracks first, master last: Keys 0, Low 1, Master 2.
    let names = session.route_names();
    assert_eq!(names[..2], ["Keys".to_string(), "Low".to_string()]);
    let routes = session.clip_routes();
    let of = |id| {
        routes
            .iter()
            .find(|(clip, _)| *clip == id)
            .map(|(_, strips)| strips.clone())
            .unwrap_or_default()
    };
    assert_eq!(of(clip), vec![0, 1], "the piano's track and the bass's");
    assert_eq!(
        of(vox.clip().unwrap()),
        vec![1],
        "the audio clip's own track"
    );
    assert_eq!(of(dry.clip().unwrap()), vec![2], "none is the master");
}

/// *"Auto palette, user can change"* — a strip's colour is changed from its
/// menu, and the undo puts it back.
#[test]
fn a_strip_is_recoloured_and_the_undo_puts_it_back() {
    let mut project = common::a_project_with_a_clip(2, 120.0, SR);
    let clip = Session::first_clip(&project).unwrap();
    AddMixerTrack::new("Keys").apply(&mut project).unwrap();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let library = SampleLibrary::new();
    let realised = fontelle_app::realise(&project, &library, options).unwrap();
    let (graphs, _source) = graph_channel(realised.graph);
    let mut session = Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls);
    let was = session.mixer_strips()[0].color;
    let wanted = fontelle_model::TRACK_PALETTE[7];
    assert_ne!(was, wanted);
    session.set_track_color(0, wanted);
    assert_eq!(session.mixer_strips()[0].color, wanted);
    session.undo();
    assert_eq!(session.mixer_strips()[0].color, was);
}
