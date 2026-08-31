//! The step that turns a document into something the engine can play
//! (`fontelle_app::realise`).
//!
//! Everything the DAW does to a project — save it, undo an edit, move a
//! fader, choose an instrument — changes the document and then comes back
//! through here. Before this existed `fontelle-app` hand-assigned node ids and
//! buses from its own `Song` type, `Project::mixer` was read by nothing, and
//! `Channel::patch_data` was left empty; three of PROGRESS.md's open questions
//! were the same missing owner.

use std::sync::Arc;

use fontelle_app::{RealiseError, RealiseOptions, SampleLibrary, realise, render_offline};
use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, Source,
    VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_model::Arena;
use fontelle_model::{
    Channel, Clip, ClipSource, Command, FlagTarget, Lane, MixerTrack, Note, NoteData, NumberTarget,
    Project, SetFlag, SetNumber,
};
use fontelle_types::{ChannelId, MixerTrackId, PPQN};

const SR: u32 = 48_000;

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: Interpolation::Draft,
    }
}

/// A patch that renders a constant `level` for as long as the note is held —
/// so a rendered level is a statement about gain staging and nothing else.
fn flat_patch(library: &mut SampleLibrary, name: &str, level: f32) -> Patch {
    let asset = library.insert_synthetic(
        name,
        SampleBuffer {
            data: Arc::from(vec![level; 100_000]),
            sample_rate: SR,
        },
    );
    let disabled = FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
    };
    let instant = EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 0.001,
        curve: EnvelopeCurve::Linear,
    };
    Patch {
        layers: vec![Layer {
            source: Source::Sample { file: asset },
            key_range: (0, 127),
            vel_range: (0, 127),
            root_key: 60,
            fine_tune_cents: 0.0,
            playback: PlaybackConfig {
                loop_mode: LoopMode::Off,
                interpolation: Some(Interpolation::Draft),
                end_offset: 100_000.0,
                ..PlaybackConfig::default()
            },
            gain_db: 0.0,
            pan: 0.0,
        }],
        filters: [disabled, disabled],
        envelopes: vec![instant, instant],
        lfos: Vec::new(),
        mod_matrix: ModMatrix::default(),
        voice_config: VoiceConfig::default(),
    }
}

struct Rig {
    project: Project,
    library: SampleLibrary,
    lane: fontelle_types::LaneId,
}

impl Rig {
    fn new() -> Self {
        let mut project = Project::new("realise");
        project.tempo_map = fontelle_model::TempoMap::new(120.0, SR as f64);
        let lane = project.lanes.insert(Lane {
            name: "lane".into(),
            height: 32.0,
            color: [0; 4],
            muted: false,
            locked: false,
        });
        Self {
            project,
            library: SampleLibrary::new(),
            lane,
        }
    }

    fn master(&self) -> MixerTrackId {
        self.project.mixer.master.expect("a project has a master")
    }

    /// A mixer track feeding `output` (master when `None`).
    fn add_track(&mut self, name: &str, output: Option<MixerTrackId>) -> MixerTrackId {
        let master = self.master();
        let id = self.project.mixer.tracks.insert(MixerTrack::new(name));
        self.project.mixer.tracks[id].output = Some(output.unwrap_or(master));
        id
    }

    /// A channel with a flat instrument, on `track`, holding one long note.
    fn add_channel(&mut self, name: &str, level: f32, track: MixerTrackId) -> ChannelId {
        let patch = flat_patch(&mut self.library, name, level);
        let channel = self.project.channels.insert(Channel {
            name: name.into(),
            color: [0; 4],
            mixer_track: Some(track),
            patch_data: None,
            pan: 0.0,
            muted: false,
            soloed: false,
        });
        fontelle_app::set_channel_patch(&mut self.project, channel, &patch, &self.library)
            .expect("a patch this build built must serialise");
        self.hold_a_note(channel);
        channel
    }

    fn hold_a_note(&mut self, channel: ChannelId) {
        let mut notes = Arena::default();
        notes.insert(Note {
            start: 0,
            length: PPQN * 8,
            key: 60,
            velocity: 127,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        });
        self.project.clips.insert(Clip {
            lane: self.lane,
            start: 0,
            length: PPQN * 8,
            source: ClipSource::Notes(NoteData { channel, notes }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        });
    }

    /// Interleaved stereo, straight through the realised graph.
    fn render(&self, frames: usize) -> Vec<f32> {
        let mut realised =
            realise(&self.project, &self.library, options()).expect("this project must realise");
        let timeline = fontelle_sequencer::compile(
            &self.project,
            &realised.channel_nodes,
            &Default::default(),
        );
        render_offline(&timeline, &mut realised.graph, frames as i64)
    }
}

/// Every mixer change below goes through a command (INVARIANT 9), which is
/// also the shortest way to say that the command set covers what the mixer UI
/// will need.
fn set_number(project: &mut Project, target: NumberTarget, value: f64) {
    SetNumber::new(target, value)
        .apply(project)
        .expect("the target must exist");
}

fn set_flag(project: &mut Project, target: FlagTarget, value: bool) {
    SetFlag::new(target, value)
        .apply(project)
        .expect("the target must exist");
}

/// Peak of one channel of an interleaved stereo buffer, skipping the first
/// block so a note-on that lands mid-block is not the thing being measured.
fn peak(interleaved: &[f32], channel: usize) -> f32 {
    interleaved
        .as_chunks::<2>()
        .0
        .iter()
        .skip(fontelle_engine::BLOCK_SIZE)
        .fold(0.0f32, |m, f| m.max(f[channel].abs()))
}

fn peak_mono(interleaved: &[f32]) -> f32 {
    peak(interleaved, 0).max(peak(interleaved, 1))
}

#[test]
fn each_channel_becomes_a_sampler_node_the_sequencer_can_target() {
    let mut rig = Rig::new();
    let a_track = rig.add_track("A", None);
    let b_track = rig.add_track("B", None);
    let a = rig.add_channel("A", 0.2, a_track);
    let b = rig.add_channel("B", 0.2, b_track);

    let realised = realise(&rig.project, &rig.library, options()).unwrap();

    assert_eq!(realised.channel_nodes.len(), 2);
    let (node_a, node_b) = (realised.channel_nodes[&a], realised.channel_nodes[&b]);
    assert_ne!(
        node_a, node_b,
        "two channels sharing one node would play each other's parts"
    );

    // The mapping is the whole reason this step exists: it is what
    // `fontelle_sequencer::compile` needs and cannot work out for itself.
    let timeline =
        fontelle_sequencer::compile(&rig.project, &realised.channel_nodes, &Default::default());
    assert!(timeline.events.iter().any(|e| e.target == node_a));
    assert!(timeline.events.iter().any(|e| e.target == node_b));
}

#[test]
fn two_channels_on_one_mixer_track_share_its_fader() {
    // TDD §13.1 allows several channels on one track. Pulling that track down
    // has to pull both of them down.
    let mut rig = Rig::new();
    let shared = rig.add_track("Shared", None);
    rig.add_channel("A", 0.2, shared);
    rig.add_channel("B", 0.2, shared);
    let together = peak_mono(&rig.render(4_000));

    set_number(&mut rig.project, NumberTarget::TrackGainDb(shared), -20.0);
    let pulled_down = peak_mono(&rig.render(4_000));

    let ratio = pulled_down / together;
    assert!(
        (ratio - 0.1).abs() < 0.01,
        "-20 dB on the shared track should scale both parts by 0.1, got {ratio}"
    );
}

#[test]
fn a_track_routed_into_a_group_passes_through_both_faders() {
    // The `output` field, which nothing read before. A group bus is the case
    // that needs the schedule ordered: the group's fader must run after every
    // track that feeds it has been summed in.
    let mut rig = Rig::new();
    let group = rig.add_track("Group", None);
    let part = rig.add_track("Part", Some(group));
    rig.add_channel("A", 0.5, part);

    let flat = peak_mono(&rig.render(4_000));

    set_number(&mut rig.project, NumberTarget::TrackGainDb(part), -6.0);
    set_number(&mut rig.project, NumberTarget::TrackGainDb(group), -6.0);
    let both = peak_mono(&rig.render(4_000));

    let ratio = both / flat;
    let expected = 10f32.powf(-12.0 / 20.0);
    assert!(
        (ratio - expected).abs() < 0.01,
        "-6 dB at the part and -6 dB at the group is -12 dB, got a ratio of {ratio}"
    );
}

#[test]
fn a_muted_track_is_silent_and_its_neighbour_is_not() {
    let mut rig = Rig::new();
    let a_track = rig.add_track("A", None);
    let b_track = rig.add_track("B", None);
    rig.add_channel("A", 0.4, a_track);
    rig.add_channel("B", 0.4, b_track);

    set_flag(&mut rig.project, FlagTarget::TrackMute(a_track), true);
    let audio = rig.render(4_000);

    assert!(peak_mono(&audio) > 0.1, "B must still be heard");
    set_flag(&mut rig.project, FlagTarget::TrackMute(b_track), true);
    assert_eq!(
        peak_mono(&rig.render(4_000)),
        0.0,
        "with both muted there is nothing left"
    );
}

#[test]
fn soloing_one_track_silences_the_others_but_not_the_group_carrying_it() {
    // The subtle half: a soloed track routed into a group is inaudible unless
    // the group stays open. Muting "everything not soloed" gets that wrong.
    let mut rig = Rig::new();
    let group = rig.add_track("Group", None);
    let inside = rig.add_track("Inside", Some(group));
    let outside = rig.add_track("Outside", None);
    rig.add_channel("Inside", 0.4, inside);
    rig.add_channel("Outside", 0.4, outside);

    let both = peak_mono(&rig.render(4_000));
    set_flag(&mut rig.project, FlagTarget::TrackSolo(inside), true);
    let soloed = peak_mono(&rig.render(4_000));

    assert!(
        soloed > 0.1,
        "the soloed track must be audible, got {soloed}"
    );
    assert!(
        soloed < both,
        "the other track must have gone away: {soloed} against {both}"
    );
}

#[test]
fn a_mixer_with_a_routing_cycle_is_refused_rather_than_compiled() {
    // TDD §13.2. A feedback loop must never reach the graph compiler — which
    // here means never reaching the topological sort that lays out the buses.
    let mut rig = Rig::new();
    let a = rig.add_track("A", None);
    let b = rig.add_track("B", Some(a));
    rig.add_channel("A", 0.2, a);
    rig.project.mixer.tracks[a].output = Some(b);

    assert!(matches!(
        realise(&rig.project, &rig.library, options()),
        Err(RealiseError::MixerCycle)
    ));
}

#[test]
fn a_channel_with_no_instrument_keeps_its_place_but_nothing_answers_for_it() {
    // A channel exists from the moment it is created, before a soundfont has
    // been dropped on it, and it plays nothing rather than playing a default.
    //
    // It still gets a node id. The compiled timeline's shape then depends only
    // on the notes, so choosing or changing an instrument does not invalidate
    // it — the events simply reach a node that is not in the schedule and are
    // heard by nobody.
    let mut rig = Rig::new();
    let track = rig.add_track("A", None);
    let silent = rig.project.channels.insert(Channel {
        name: "empty".into(),
        color: [0; 4],
        mixer_track: Some(track),
        patch_data: None,
        pan: 0.0,
        muted: false,
        soloed: false,
    });
    rig.hold_a_note(silent);

    let realised = realise(&rig.project, &rig.library, options()).unwrap();
    let node = realised.channel_nodes[&silent];
    assert!(
        !realised.graph.schedule.iter().any(|n| n.id == node),
        "there is no instrument to schedule"
    );
    let timeline =
        fontelle_sequencer::compile(&rig.project, &realised.channel_nodes, &Default::default());
    assert!(
        timeline.events.iter().any(|e| e.target == node),
        "the notes are still in the timeline"
    );
    assert_eq!(peak_mono(&rig.render(4_000)), 0.0);
}

#[test]
fn a_channel_whose_samples_are_missing_still_realises_and_plays_silence() {
    // TDD §17.4: a project with a broken link opens and plays. It does not
    // refuse, and the reference is reported so a relink dialog can act on it.
    let mut rig = Rig::new();
    let track = rig.add_track("A", None);
    let channel = rig.add_channel("A", 0.5, track);

    // A library that has never heard of this patch's audio, which is what a
    // project reopened next to a moved soundfont looks like.
    let empty = SampleLibrary::new();
    let realised = realise(&rig.project, &empty, options()).expect("a broken link still opens");

    assert_eq!(realised.unresolved.len(), 1);
    assert_eq!(realised.unresolved[0].0, channel);
    let mut realised = realised;
    let timeline =
        fontelle_sequencer::compile(&rig.project, &realised.channel_nodes, &Default::default());
    let audio = render_offline(&timeline, &mut realised.graph, 4_000);
    assert_eq!(peak_mono(&audio), 0.0);
}

#[test]
fn the_channels_own_pan_places_it_in_the_stereo_field() {
    // Placement, on the constant-power taper, at the voice — not the track's
    // balance control. Two channels can share one track and still sit in
    // different places, which is the case that made this a channel field.
    let mut rig = Rig::new();
    let shared = rig.add_track("Shared", None);
    // Different levels, so that mirroring the two pans is visible at all.
    // Two identical sources swapped left for right render the same audio, and
    // a test built on that passes with the feature absent.
    let left = rig.add_channel("Left", 0.4, shared);
    let right = rig.add_channel("Right", 0.15, shared);
    set_number(&mut rig.project, NumberTarget::ChannelPan(left), -1.0);
    set_number(&mut rig.project, NumberTarget::ChannelPan(right), 1.0);

    let audio = rig.render(4_000);
    let (l, r) = (peak(&audio, 0), peak(&audio, 1));
    assert!(l > 0.1 && r > 0.05, "both sides carry one of the two parts");
    assert!(l > r, "the louder part is on the left");

    // Swap them and the picture mirrors; a pan that reached nothing would
    // leave the two renders identical.
    set_number(&mut rig.project, NumberTarget::ChannelPan(left), 1.0);
    set_number(&mut rig.project, NumberTarget::ChannelPan(right), -1.0);
    let swapped = rig.render(4_000);
    assert!(
        peak(&swapped, 1) > peak(&swapped, 0),
        "swapping the pans must move the louder part to the right"
    );
}

#[test]
fn a_channel_on_a_mixer_track_that_no_longer_exists_still_reaches_the_master() {
    // A part you can hear and fix beats a part that vanished silently.
    let mut rig = Rig::new();
    let track = rig.add_track("A", None);
    rig.add_channel("A", 0.4, track);
    rig.project.mixer.tracks.remove(track);

    assert!(peak_mono(&rig.render(4_000)) > 0.1);
}
