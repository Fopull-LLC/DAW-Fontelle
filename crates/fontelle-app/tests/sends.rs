//! Sends, from the graph's side (TDD §13.2).
//!
//! `MixerTrack::sends` has been in the document since the mixer was written
//! and nothing compiled it, so a project with a reverb bus sounded exactly
//! like one without. The DSP is `fontelle-engine/tests/sends.rs`; this is the
//! wiring — where the tap goes, what order the buses run in, and what a solo
//! does to a send.
//!
//! # The order is the interesting part
//!
//! `realise` schedules tracks *deepest first*, so a group's fader runs only
//! after everything feeding it has been summed in. Depth used to be measured
//! along `output` alone, which is fine until a send crosses it: a send from a
//! shallow track into a deeper one would arrive after the deeper track's fader
//! had already run, and the block would be a block late — or, since the buffer
//! is cleared, gone.

use fontelle_app::{RealiseOptions, SampleLibrary, realise, render_offline};
use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, Source,
    VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_model::{
    AddSend, Arena, Channel, Clip, ClipSource, Command, Lane, MixerTrack, Note, NoteData, Project,
    SetSendLevel, SetSendPreFader, SetTrackOutput,
};
use fontelle_types::{MixerTrackId, PPQN};

const SR: u32 = 48_000;

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: Interpolation::Draft,
    }
}

/// A patch that renders a constant level for as long as the note is held — so
/// a rendered peak is a statement about gain staging and nothing else. The
/// same one `tests/realise.rs` uses, for the same reason.
fn flat_patch(library: &mut SampleLibrary) -> Patch {
    let asset = library.insert_synthetic(
        "flat",
        SampleBuffer {
            data: std::sync::Arc::from(vec![0.5f32; 100_000]),
            sample_rate: SR,
        },
    );
    let disabled = FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
        ..Default::default()
    };
    let instant = EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 0.001,
        curve: EnvelopeCurve::Linear,
        ..Default::default()
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
        ..Default::default()
    }
}

/// A project with one steady tone on `Source`, plus an empty `Bus`.
struct Rig {
    project: Project,
    library: SampleLibrary,
    source: MixerTrackId,
    bus: MixerTrackId,
}

impl Rig {
    fn new() -> Self {
        let mut project = Project::new("sends");
        project.tempo_map = fontelle_model::TempoMap::new(120.0, SR as f64);
        let master = project.mixer.master.expect("a project has a master");
        let source = project.mixer.tracks.insert(MixerTrack::new("Source"));
        project.mixer.tracks[source].output = Some(master);
        let bus = project.mixer.tracks.insert(MixerTrack::new("Bus"));
        project.mixer.tracks[bus].output = Some(master);

        let mut library = SampleLibrary::new();
        let patch = flat_patch(&mut library);
        let channel = project.channels.insert(Channel {
            preset: None,
            instrument: None,
            name: "tone".into(),
            color: [0; 4],
            mixer_track: Some(source),
            patch_data: None,
            plugin: None,
            pan: 0.0,
            muted: false,
            soloed: false,
            named_keys: false,
            ab: Default::default(),
            gain_db: 0.0,
        });
        fontelle_app::set_channel_patch(&mut project, channel, &patch, &library)
            .expect("a patch this build built must serialise");

        let lane = project.lanes.insert(Lane {
            name: "lane".into(),
            height: 32.0,
            color: [0; 4],
            muted: false,
            locked: false,
            order: 0,
        });
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
            channel: None,
        });
        project.clips.insert(Clip {
            lane,
            start: 0,
            length: PPQN * 8,
            source: ClipSource::Notes(NoteData { channel, notes }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        });

        Self {
            project,
            library,
            source,
            bus,
        }
    }

    fn master(&self) -> MixerTrackId {
        self.project.mixer.master.expect("a master")
    }

    fn run(&mut self, command: impl Command + 'static) {
        let mut command = command;
        command
            .apply(&mut self.project)
            .expect("the command applies");
    }

    /// What reaches the speakers, once everything has settled.
    fn level(&self) -> f32 {
        let mut realised = realise(&self.project, &self.library, options()).expect("must realise");
        let timeline = fontelle_sequencer::compile(
            &self.project,
            &realised.channel_nodes,
            &realised.param_nodes,
        );
        let out = render_offline(&timeline, &mut realised.graph, SR as i64 / 4);
        out[out.len() / 2..]
            .iter()
            .step_by(2)
            .fold(0.0f32, |m, s| m.max(s.abs()))
    }
}

// ------------------------------------------------------ the send is heard ---

#[test]
fn a_send_at_the_bottom_of_its_travel_changes_nothing() {
    // Which is where every send starts. Making one must not change the mix.
    let mut rig = Rig::new();
    let dry = rig.level();
    rig.run(AddSend::new(rig.source, rig.bus));
    assert!(
        (rig.level() - dry).abs() < 1e-3,
        "a new send was audible: {} against {dry}",
        rig.level()
    );
}

#[test]
fn bringing_a_send_up_puts_more_into_the_master() {
    // The signal arrives twice — once dry down its own path, once through the
    // bus — so the master gets louder. That is what a send *is*, and it is
    // the whole thing that was missing.
    let mut rig = Rig::new();
    let dry = rig.level();
    rig.run(AddSend::new(rig.source, rig.bus));
    rig.run(SetSendLevel::new(rig.source, 0, 0.0));
    let wet = rig.level();
    assert!(
        wet > dry * 1.2,
        "the send did not reach the master: {wet} against {dry}"
    );
}

#[test]
fn the_dry_path_is_still_there_with_the_send_wide_open() {
    // A send that moved the signal instead of copying it would leave the
    // source track silent, and the total would be the same rather than more.
    // Muting the *bus* is what separates the two readings.
    let mut rig = Rig::new();
    let dry = rig.level();
    rig.run(AddSend::new(rig.source, rig.bus));
    rig.run(SetSendLevel::new(rig.source, 0, 0.0));
    rig.project.mixer.tracks[rig.bus].mute = true;
    assert!(
        (rig.level() - dry).abs() < 1e-3,
        "with the bus muted only the dry path is left, and it should be \
         exactly what it was: {} against {dry}",
        rig.level()
    );
}

#[test]
fn the_send_level_is_what_decides_how_much_arrives() {
    let mut rig = Rig::new();
    rig.run(AddSend::new(rig.source, rig.bus));
    rig.run(SetSendLevel::new(rig.source, 0, -12.0));
    let quiet = rig.level();
    rig.run(SetSendLevel::new(rig.source, 0, 0.0));
    assert!(rig.level() > quiet, "the level did not change what arrived");
}

#[test]
fn a_send_can_feed_a_bus_that_is_deeper_than_its_source() {
    // The ordering case. `realise` schedules deepest first so a group's fader
    // runs after everything feeding it; a send that crosses from a shallow
    // track to a deeper one has to count as a feeding edge, or the tap arrives
    // after the bus has already been read and is simply gone.
    let mut rig = Rig::new();
    let master = rig.master();
    // A group in front of the bus, so the bus is two hops from master while
    // the source is one.
    let group = rig.project.mixer.tracks.insert(MixerTrack::new("Group"));
    rig.project.mixer.tracks[group].output = Some(master);
    rig.run(SetTrackOutput::new(rig.bus, Some(group)));

    let dry = rig.level();
    rig.run(AddSend::new(rig.source, rig.bus));
    rig.run(SetSendLevel::new(rig.source, 0, 0.0));
    assert!(
        rig.level() > dry * 1.2,
        "a send into a deeper bus was scheduled too late to be heard"
    );
}

// -------------------------------------------------- pre-fader and post ---

#[test]
fn a_post_fader_send_follows_the_fader_down() {
    // Which is what a reverb send wants: pull the part down and its reverb
    // goes with it.
    let mut rig = Rig::new();
    rig.run(AddSend::new(rig.source, rig.bus));
    rig.run(SetSendLevel::new(rig.source, 0, 0.0));
    let open = rig.level();

    rig.project.mixer.tracks[rig.source].gain_db = -60.0;
    // Only the bus is left, and a post-fader tap took the fader with it.
    assert!(
        rig.level() < open * 0.2,
        "a post-fader send did not follow its fader: {} against {open}",
        rig.level()
    );
}

#[test]
fn a_pre_fader_send_does_not() {
    // Which is what a cue mix wants, and the reason the switch exists at all.
    let mut rig = Rig::new();
    rig.run(AddSend::new(rig.source, rig.bus));
    rig.run(SetSendLevel::new(rig.source, 0, 0.0));
    rig.run(SetSendPreFader::new(rig.source, 0, true));

    rig.project.mixer.tracks[rig.source].gain_db = -60.0;
    let pre = rig.level();
    assert!(
        pre > 0.05,
        "the pre-fader tap was taken after the fader: {pre}"
    );
}

#[test]
fn a_pre_fader_send_is_still_taken_after_the_inserts() {
    // "Pre-fader" is a claim about the *fader*, not about the chain: an EQ on
    // a track is part of the track's sound, and a send that bypassed it would
    // be sending a different instrument.
    let mut rig = Rig::new();
    // A low shelf right up at the top of the band, so everything below it —
    // which is everything this patch renders — comes out far quieter than it
    // went in. A *high* shelf would not do: `flat_patch` holds a constant
    // level, and a constant is DC, which sits under every shelf corner there
    // is and would sail straight through one.
    rig.run(fontelle_model::AddInsert::new(
        rig.source,
        fontelle_types::EffectKind::Eq,
    ));
    rig.run(fontelle_model::SetEqBand::new(
        rig.source,
        0,
        0,
        fontelle_types::EqBand {
            band_type: fontelle_types::BandType::LowShelf,
            freq_hz: 20_000.0,
            gain_db: -40.0,
            q: 0.7,
            enabled: true,
            solo: false,
            channel: fontelle_types::BandChannel::Stereo,
        },
    ));

    rig.run(AddSend::new(rig.source, rig.bus));
    rig.run(SetSendLevel::new(rig.source, 0, 0.0));
    rig.run(SetSendPreFader::new(rig.source, 0, true));
    // The dry path out of the way, so what is measured is the send alone.
    rig.project.mixer.tracks[rig.source].gain_db = -60.0;

    assert!(
        rig.level() < 0.2,
        "the pre-fader tap was taken before the insert chain: {}",
        rig.level()
    );
}

// -------------------------------------------------------------- the solo ---

#[test]
fn a_solo_that_silences_a_track_silences_its_sends_too() {
    // Otherwise a reverb goes on ringing from a part nobody can hear, which is
    // both wrong and mystifying.
    let mut rig = Rig::new();
    rig.run(AddSend::new(rig.source, rig.bus));
    rig.run(SetSendLevel::new(rig.source, 0, 0.0));

    // Solo something else entirely.
    let other = rig.project.mixer.tracks.insert(MixerTrack::new("Other"));
    rig.project.mixer.tracks[other].output = rig.project.mixer.master;
    rig.project.mixer.tracks[other].solo = true;

    assert!(
        rig.level() < 1e-3,
        "the soloed track is silent and the send was still arriving: {}",
        rig.level()
    );
}

#[test]
fn soloing_a_track_keeps_the_bus_its_send_feeds_open() {
    // The same rule an output follows — *"a soloed track routed into a group
    // is inaudible unless the group stays open"* — and a send is a signal path
    // like any other.
    let mut rig = Rig::new();
    rig.run(AddSend::new(rig.source, rig.bus));
    rig.run(SetSendLevel::new(rig.source, 0, 0.0));
    let both = rig.level();

    rig.project.mixer.tracks[rig.source].solo = true;
    assert!(
        (rig.level() - both).abs() < 1e-3,
        "soloing the source closed the bus its send feeds: {} against {both}",
        rig.level()
    );
}

// ---------------------------------------------------- live, without rebuild ---

#[test]
fn a_send_level_is_reachable_without_rebuilding_the_graph() {
    // The same requirement a fader has: a drag has to be audible before the
    // mouse comes up, and `realise` deserialises every channel's patch.
    let mut rig = Rig::new();
    rig.run(AddSend::new(rig.source, rig.bus));
    let realised = realise(&rig.project, &rig.library, options()).expect("must realise");
    let controls = realised
        .send_controls
        .get(&(rig.source, 0))
        .expect("the send has a live control surface");
    assert!(controls.level_db() <= -60.0);
    controls.set_level_db(0.0);
    assert!((controls.level_db() - 0.0).abs() < 1e-6);
}
