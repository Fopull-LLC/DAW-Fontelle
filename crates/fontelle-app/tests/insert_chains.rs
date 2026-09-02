//! An insert chain, in the graph, making a sound (TDD §13.4).
//!
//! `fontelle-model/tests/inserts.rs` is the document half and
//! `fontelle-engine/tests/inserts.rs` is the node. This is the seam between
//! them: what `realise` does with a track that has effects on it, which is
//! where a chain stops being a list and starts being an order things happen
//! in.
//!
//! The gap this closes was written in `realise`'s own comment — *"**Inserts
//! and sends are not compiled** — effects and sends are M4"* — and it is the
//! same shape as the seven dead controls before it: the document could hold a
//! chain, and nothing downstream read it.
//!
//! The source is a **sine at a known frequency**, so what an EQ did to it is a
//! number rather than an impression. The rest of the rig is `tests/realise.rs`'s,
//! because a chain has to work in the graph a real project realises to and not
//! in one built to suit it.

use std::sync::Arc;

use fontelle_app::{RealiseOptions, SampleLibrary, realise, render_offline};
use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, Source,
    VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_model::{
    AddInsert, Arena, Channel, Clip, ClipSource, Command, Lane, MixerTrack, Note, NoteData,
    Project, SetEqBand, SetInsertBypassed,
};
use fontelle_types::{BandChannel, BandType, EffectKind, EqBand, MixerTrackId, PPQN};

const SR: u32 = 48_000;

/// The tone everything here is measured at. 48 samples a cycle at 48 kHz, so
/// the fixture loops seamlessly and the pitch is exact rather than nearly.
const TONE_HZ: f32 = 1_000.0;

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: Interpolation::Draft,
    }
}

fn a_band(band_type: BandType, freq_hz: f32, gain_db: f32) -> EqBand {
    EqBand {
        band_type,
        freq_hz,
        gain_db,
        q: 1.0,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    }
}

/// A patch that holds one sine for as long as the note is held.
fn tone_patch(library: &mut SampleLibrary) -> Patch {
    let period = (SR as f32 / TONE_HZ) as usize;
    let data: Vec<f32> = (0..period)
        .map(|i| (std::f32::consts::TAU * i as f32 / period as f32).sin())
        .collect();
    let asset = library.insert_synthetic(
        "tone",
        SampleBuffer {
            data: Arc::from(data),
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
                loop_mode: LoopMode::Forward,
                interpolation: Some(Interpolation::Draft),
                loop_start: 0.0,
                loop_end: period as f64,
                end_offset: period as f64,
                ..PlaybackConfig::default()
            },
            // Quiet on purpose. Everything here is measured as a *ratio*,
            // and the master bus ends in a brickwall limiter (§13.1): a tone
            // at unity through a +12 dB bell comes out at the ceiling, and
            // the measurement then says the limiter works rather than that
            // the EQ does. The first draft of this file measured exactly
            // that.
            gain_db: -24.0,
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
}

impl Rig {
    /// A project holding one sine, on `track`, for eight beats.
    fn new(on_a_track: bool) -> Self {
        let mut project = Project::new("inserts");
        project.tempo_map = fontelle_model::TempoMap::new(120.0, SR as f64);
        let lane = project.lanes.insert(Lane {
            name: "lane".into(),
            height: 32.0,
            color: [0; 4],
            muted: false,
            locked: false,
            order: 0,
        });
        let mut library = SampleLibrary::new();
        let master = project.mixer.master.expect("a project has a master");
        let track = if on_a_track {
            let id = project.mixer.tracks.insert(MixerTrack::new("Keys"));
            project.mixer.tracks[id].output = Some(master);
            Some(id)
        } else {
            None
        };

        let patch = tone_patch(&mut library);
        let channel = project.channels.insert(Channel {
            name: "tone".into(),
            color: [0; 4],
            mixer_track: track,
            patch_data: None,
            pan: 0.0,
            muted: false,
            soloed: false,
            named_keys: false,
            gain_db: 0.0,
        });
        fontelle_app::set_channel_patch(&mut project, channel, &patch, &library)
            .expect("a patch this build built must serialise");

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

        Self { project, library }
    }

    fn master(&self) -> MixerTrackId {
        self.project.mixer.master.expect("a project has a master")
    }

    fn track(&self) -> MixerTrackId {
        self.project
            .mixer
            .tracks
            .keys()
            .find(|id| Some(*id) != self.project.mixer.master)
            .expect("this rig was built with a track")
    }

    fn realised(&self) -> fontelle_app::Realised {
        realise(&self.project, &self.library, options()).expect("this project must realise")
    }

    /// The tone's level once everything has settled, as a peak.
    ///
    /// The second half of the render, because a filter's first milliseconds
    /// are its ring-in and measuring those as though they were the passband is
    /// how a test asserts the wrong number confidently.
    fn level(&self) -> f32 {
        let mut realised = self.realised();
        let timeline = fontelle_sequencer::compile(
            &self.project,
            &realised.channel_nodes,
            &Default::default(),
        );
        let frames = SR as i64 / 4;
        let out = render_offline(&timeline, &mut realised.graph, frames);
        out[out.len() / 2..]
            .iter()
            .step_by(2)
            .fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// The same project, behind the trait the window actually calls.
    ///
    /// Everything above tests `realise` and the graph; the automation gesture
    /// is a `StudioHost` method, and the seam a right-click crosses is this
    /// one — so the tests for it go through here rather than round it.
    fn into_session(self) -> fontelle_app::Session {
        let clip = fontelle_app::Session::first_clip(&self.project).expect("the rig has a clip");
        let channel_nodes = fontelle_app::channel_nodes(&self.project);
        let (publisher, _timeline) =
            fontelle_engine::timeline_channel(fontelle_types::CompiledTimeline::empty());
        let realised =
            realise(&self.project, &self.library, options()).expect("this project must realise");
        let (graphs, _source) = fontelle_engine::graph_channel(realised.graph);
        fontelle_app::Session::new(
            self.project,
            self.library,
            channel_nodes,
            publisher,
            options(),
            clip,
            None,
        )
        .with_graphs(graphs, realised.track_controls)
        .with_param_nodes(realised.param_nodes)
    }

    /// Puts a fresh EQ on `track` with `band` in its first slot.
    fn add_eq(&mut self, track: MixerTrackId, band: EqBand) -> usize {
        AddInsert::new(track, EffectKind::Eq)
            .apply(&mut self.project)
            .unwrap();
        let slot = self.project.mixer.tracks[track].inserts.len() - 1;
        SetEqBand::new(track, slot, 0, band)
            .apply(&mut self.project)
            .unwrap();
        slot
    }

    /// Puts a fresh effect of `kind` on `track`, at its defaults.
    fn add_insert(&mut self, track: MixerTrackId, kind: EffectKind) -> usize {
        AddInsert::new(track, kind).apply(&mut self.project).unwrap();
        self.project.mixer.tracks[track].inserts.len() - 1
    }
}

#[test]
fn a_track_with_no_inserts_realises_exactly_as_it_always_did() {
    let rig = Rig::new(false);
    assert!(
        rig.realised().effect_controls.is_empty(),
        "no inserts, no control surfaces"
    );
}

#[test]
fn every_insert_on_every_track_gets_a_live_end() {
    // Keyed by track and position, because that is how the panel addresses
    // one: the strip you are looking at and the slot you clicked.
    let mut rig = Rig::new(true);
    let (track, master) = (rig.track(), rig.master());
    rig.add_eq(track, a_band(BandType::Bell, 1_000.0, 3.0));
    rig.add_eq(track, a_band(BandType::Bell, 2_000.0, 3.0));
    rig.add_eq(master, a_band(BandType::Bell, 4_000.0, 3.0));

    let realised = rig.realised();
    let keys: Vec<(MixerTrackId, usize)> = realised.effect_controls.keys().copied().collect();
    assert_eq!(keys.len(), 3);
    assert!(keys.contains(&(track, 0)));
    assert!(keys.contains(&(track, 1)));
    assert!(keys.contains(&(master, 0)));
}

#[test]
fn a_live_end_starts_at_what_the_document_says() {
    // One source of truth. A control surface that started blank would make the
    // first touch of a knob jump the sound somewhere nobody asked for.
    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.add_eq(master, a_band(BandType::Bell, 800.0, 5.0));

    let realised = rig.realised();
    let fontelle_types::EffectConfig::Eq(eq) = realised.effect_controls[&(master, 0)].config()
    else {
        unreachable!("this slot holds an EQ")
    };
    assert_eq!(eq.bands[0].freq_hz, 800.0);
    assert_eq!(eq.bands[0].gain_db, 5.0);
}

#[test]
fn a_bypassed_insert_is_built_bypassed() {
    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.add_eq(master, a_band(BandType::Bell, 800.0, 5.0));
    SetInsertBypassed::new(master, 0, true)
        .apply(&mut rig.project)
        .unwrap();
    assert!(rig.realised().effect_controls[&(master, 0)].bypassed());
}

// ---------------------------------------------------------------- the sound

#[test]
fn an_insert_on_the_master_is_heard() {
    // The end of the chain the whole feature is for.
    let plain = Rig::new(false).level();

    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.add_eq(master, a_band(BandType::Bell, TONE_HZ, 12.0));
    let lifted = rig.level();

    assert!(
        lifted > plain * 3.0,
        "a +12 dB bell on the master should be audible: {plain} became {lifted}"
    );
}

#[test]
fn an_insert_on_a_track_is_heard_too() {
    // Not just the master: the whole point is a chain per strip.
    let plain = Rig::new(true).level();

    let mut rig = Rig::new(true);
    let track = rig.track();
    rig.add_eq(track, a_band(BandType::Bell, TONE_HZ, 12.0));
    let lifted = rig.level();

    assert!(
        lifted > plain * 3.0,
        "an EQ on the track carrying the tone: {plain} became {lifted}"
    );
}

#[test]
fn a_bypassed_insert_is_not_heard() {
    let plain = Rig::new(false).level();

    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.add_eq(master, a_band(BandType::Bell, TONE_HZ, 12.0));
    SetInsertBypassed::new(master, 0, true)
        .apply(&mut rig.project)
        .unwrap();
    let bypassed = rig.level();

    assert!(
        (bypassed - plain).abs() < plain * 0.02,
        "switched out, it should sound like nothing is there: {plain} against {bypassed}"
    );
}

#[test]
fn two_eqs_in_a_chain_commute_because_both_are_linear() {
    // Not the test this wanted to be. The intent was "order is the sound" —
    // a low-pass then a lift is not a lift then a low-pass — and it is false
    // for these two, provably: filtering is linear, and linear operators
    // commute. Swapping two EQs produces the same samples, and a test
    // asserting otherwise would have been asserting a bug.
    //
    // Order *will* be observable as soon as one link in the chain is not
    // linear, which the compressor is, and that is where the real version of
    // this test belongs. What this one is worth in the meantime: a chain that
    // silently dropped one of its two effects fails here.
    let build = |lift_first: bool| {
        let mut rig = Rig::new(false);
        let master = rig.master();
        let bands = if lift_first {
            [
                a_band(BandType::Bell, TONE_HZ, 12.0),
                a_band(BandType::LowPass24, 200.0, 0.0),
            ]
        } else {
            [
                a_band(BandType::LowPass24, 200.0, 0.0),
                a_band(BandType::Bell, TONE_HZ, 12.0),
            ]
        };
        for band in bands {
            rig.add_eq(master, band);
        }
        rig.level()
    };

    let filter_first = build(false);
    let lift_first = build(true);
    let plain = Rig::new(false).level();

    assert!(
        (lift_first - filter_first).abs() < filter_first.max(1e-9) * 0.05,
        "two linear filters commute: {filter_first} against {lift_first}"
    );
    assert!(
        filter_first < plain * 0.05,
        "and both of them ran — a 1 kHz tone through a 200 Hz low-pass is gone: \
         {plain} became {filter_first}"
    );
}

#[test]
fn an_insert_runs_before_the_fader_on_its_own_track() {
    // Pre-fader, as every mixer's inserts are: the fader is the last thing on
    // a strip, so pulling it down turns down what the effects made rather than
    // starving them of input.
    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.add_eq(master, a_band(BandType::Bell, TONE_HZ, 12.0));
    let loud = rig.level();

    rig.project.mixer.tracks[master].gain_db = -20.0;
    let quiet = rig.level();
    assert!(
        quiet < loud * 0.2,
        "the fader must still turn the effect's output down: {loud} against {quiet}"
    );
}

#[test]
fn an_insert_on_one_track_does_not_touch_another() {
    // An EQ on an empty track must not be in the tone's path, which a chain
    // scheduled onto the wrong bus would break invisibly.
    let mut rig = Rig::new(true);
    let master = rig.master();
    let plain = rig.level();

    // The tone is on the Keys track; put a brutal low-pass on the master's
    // *sibling*, which does not exist — so instead assert the converse: an EQ
    // on the track the tone is not on.
    let idle = rig.project.mixer.tracks.insert(MixerTrack::new("Idle"));
    rig.project.mixer.tracks[idle].output = Some(master);
    rig.add_eq(idle, a_band(BandType::LowPass12, 20.0, 0.0));

    let after = rig.level();
    assert!(
        (after - plain).abs() < plain * 0.02,
        "an insert on an idle track should not be in this signal's path: \
         {plain} against {after}"
    );
}

#[test]
fn the_same_document_realises_to_the_same_sound_every_time() {
    // A rebuild seeds fresh control surfaces from the document, which is what
    // keeps the two from drifting: the sound after a rebuild is the sound the
    // document describes, not the one the last graph happened to be holding.
    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.add_eq(master, a_band(BandType::Bell, TONE_HZ, 12.0));

    let once = rig.level();
    let twice = rig.level();
    assert!(
        (once - twice).abs() < once * 0.001,
        "same document, same sound: {once} against {twice}"
    );
}

#[test]
fn a_chain_of_eight_still_realises() {
    // Nothing bounds a chain's length, and a scheduler that assumed one insert
    // per track would fail here rather than in a test built for it.
    let mut rig = Rig::new(false);
    let master = rig.master();
    for band in 0..8 {
        rig.add_eq(
            master,
            a_band(BandType::Bell, 200.0 * (band + 1) as f32, 1.0),
        );
    }
    assert_eq!(rig.realised().effect_controls.len(), 8);
    assert!(rig.level() > 0.0, "and still makes a sound");
}

// ------------------------------------------------------- automation (§12)

use fontelle_model::{AutomationData, AutomationPoint, CurveShape};
use fontelle_types::{ParamAddress, ParamTarget};

fn a_point(tick: fontelle_types::Tick, value: f64) -> AutomationPoint {
    AutomationPoint {
        tick,
        value,
        curve: CurveShape::Linear,
        tension: 0.0,
    }
}

impl Rig {
    /// Puts an automation clip on `target`, sweeping `from` to `to` in
    /// normalised units over **exactly the second that `ends` renders** — two
    /// beats at 120 BPM. A sweep longer than the render would be measured a
    /// quarter of the way through and read as a gentler one.
    fn automate(&mut self, target: ParamAddress, from: f64, to: f64) {
        let lane = self
            .project
            .lanes
            .keys()
            .next()
            .expect("the rig has a lane");
        let mut points = Arena::default();
        points.insert(a_point(0, from));
        points.insert(a_point(PPQN * 2, to));
        self.project.clips.insert(Clip {
            lane,
            start: 0,
            length: PPQN * 2,
            source: ClipSource::Automation(AutomationData { target, points }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        });
    }

    /// The tone's level in the first and last eighth of the render, so a sweep
    /// is a comparison rather than an impression.
    fn ends(&self) -> (f32, f32) {
        let mut realised = self.realised();
        let timeline = fontelle_sequencer::compile(
            &self.project,
            &realised.channel_nodes,
            &realised.param_nodes,
        );
        let frames = SR as i64;
        let out = render_offline(&timeline, &mut realised.graph, frames);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let eighth = left.len() / 8;
        let peak = |slice: &[f32]| slice.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        (
            peak(&left[eighth..eighth * 2]),
            peak(&left[left.len() - eighth..]),
        )
    }
}

#[test]
fn every_parameter_of_an_insert_has_an_address_in_the_graph() {
    // The user's sentence, checked against the thing that has to be true for
    // it: *any* value in a mixer effect can be turned into an automation lane.
    // If `specs` lists it and the graph has no node for it, the lane draws and
    // does nothing.
    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.add_eq(master, a_band(BandType::Bell, TONE_HZ, 0.0));
    let realised = rig.realised();

    let config = rig.project.mixer.tracks[master].inserts[0].config;
    for spec in config.specs() {
        let address = ParamTarget::Insert {
            track: master,
            slot: 0,
            param: spec.id.to_string(),
        }
        .address();
        assert!(
            realised.param_nodes.contains_key(&address),
            "{address} is listed by the effect and unreachable in the graph"
        );
    }
}

#[test]
fn a_tracks_fader_and_pan_are_addressable_too() {
    // §12.3: "every mixer track volume/pan/send level".
    let rig = Rig::new(true);
    let realised = rig.realised();
    for track in [rig.master(), rig.track()] {
        assert!(
            realised
                .param_nodes
                .contains_key(&ParamTarget::TrackGain(track).address())
        );
        assert!(
            realised
                .param_nodes
                .contains_key(&ParamTarget::TrackPan(track).address())
        );
    }
}

#[test]
fn automating_an_eqs_gain_is_audible_as_a_sweep() {
    // The whole feature, end to end: a curve drawn in the document moves a
    // number inside an effect on the audio thread, and the render is louder at
    // the end than at the start because of it.
    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.add_eq(master, a_band(BandType::Bell, TONE_HZ, 0.0));
    // The band has to be on for its gain to matter.
    rig.project.mixer.tracks[master].inserts[0]
        .config
        .set("band1.on", 1.0);

    // 0.5 of a -24..+24 dB range is 0 dB; 1.0 is +24.
    rig.automate(
        ParamTarget::Insert {
            track: master,
            slot: 0,
            param: "band1.gain".into(),
        }
        .address(),
        0.5,
        1.0,
    );
    let (start, end) = rig.ends();
    assert!(
        end > start * 4.0,
        "a +24 dB sweep on a band's gain should be plainly audible: \
         {start} became {end}"
    );
}

#[test]
fn automating_a_tracks_fader_is_audible_too() {
    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.automate(ParamTarget::TrackGain(master).address(), 1.0, 0.0);
    let (start, end) = rig.ends();
    assert!(
        end < start * 0.2,
        "a fader swept down should get quieter: {start} became {end}"
    );
}

#[test]
fn automation_outranks_the_knob_while_it_has_an_opinion() {
    // §12.2, and the rule that makes automation useful rather than confusing:
    // the fader sits where the document left it and the lane wins anyway.
    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.project.mixer.tracks[master].gain_db = 0.0;
    rig.automate(ParamTarget::TrackGain(master).address(), 0.0, 0.0);
    let (start, _) = rig.ends();
    assert!(
        start < 1e-3,
        "a lane holding the fader at the bottom should be silent whatever the \
         fader says, got {start}"
    );
}

#[test]
fn an_automation_clip_aimed_at_nothing_is_harmless() {
    // A project whose lane names a track that has been deleted since. It plays.
    let mut rig = Rig::new(false);
    let before = rig.level();
    rig.automate(ParamAddress::new("mixer:99999/gain"), 0.0, 1.0);
    let after = rig.level();
    assert!((after - before).abs() < before * 0.01);
}

// -------------------------------------------- making one from the window

/// The window's own surface, so this is the path a right-click takes.
use fontelle_ui::document::{DocumentHost, StudioHost};

#[test]
fn a_control_can_be_turned_into_an_automation_lane_and_it_plays() {
    // The user's sentence, end to end and through the trait the window calls:
    // *any* value in a mixer effect becomes an automation lane in the
    // timeline, and the lane is heard.
    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.add_eq(master, a_band(BandType::Bell, TONE_HZ, 0.0));
    rig.project.mixer.tracks[master].inserts[0]
        .config
        .set("band1.on", 1.0);

    // The panel builds the address the same way the right-click handler does.
    let address = ParamTarget::Insert {
        track: master,
        slot: 0,
        param: "band1.gain".into(),
    }
    .address();

    let mut session = rig.into_session();
    assert!(
        !session.is_automated(&address),
        "nothing is automated to begin with"
    );

    session.create_automation(&address, "Master \u{2014} band1.gain", 0);
    assert!(
        session.is_automated(&address),
        "and after the gesture, it is"
    );

    let view = session
        .automation()
        .expect("the clip it made is the one the editor opens");
    assert_eq!(view.points.len(), 2, "a segment, not a single point");
    assert!(view.title.contains("band1.gain"));
    assert_eq!(
        view.points[0].value, view.points[1].value,
        "flat to begin with: a lane that jumped the parameter the moment it \
         was made is one nobody trusts"
    );
}

#[test]
fn a_new_lane_starts_at_the_value_the_control_is_already_at() {
    let mut rig = Rig::new(false);
    let master = rig.master();
    rig.project.mixer.tracks[master].gain_db = -30.0;
    let address = ParamTarget::TrackGain(master).address();

    let mut session = rig.into_session();
    session.create_automation(&address, "Master \u{2014} gain", 0);
    let view = session.automation().unwrap();

    // -30 dB on a -60..+6 fader is nearly half way up.
    assert!(
        (view.points[0].value - 0.4545).abs() < 0.01,
        "the lane starts where the fader is, got {}",
        view.points[0].value
    );
}

#[test]
fn drawing_on_the_lane_is_heard() {
    // Points added and dragged through the editor's own edits, then rendered.
    let rig = Rig::new(false);
    let master = rig.master();
    let address = ParamTarget::TrackGain(master).address();

    let mut session = rig.into_session();
    session.create_automation(&address, "Master \u{2014} gain", 0);

    // Take it from full down to silence across the clip.
    let view = session.automation().unwrap();
    let ids: Vec<_> = view.points.iter().map(|point| point.id).collect();
    session.edit_automation(fontelle_ui::canvas::AutomationEdit::Move {
        ids: vec![ids[0]],
        tick_delta: 0,
        value_delta: 1.0,
    });
    session.end_gesture();
    session.edit_automation(fontelle_ui::canvas::AutomationEdit::Move {
        ids: vec![ids[1]],
        tick_delta: 0,
        value_delta: -1.0,
    });
    session.end_gesture();

    let timeline = session.compiled();
    let sweeps: Vec<f64> = timeline
        .events
        .iter()
        .filter_map(|event| match &event.payload {
            fontelle_types::EventPayload::ParamValue { value, .. } => Some(*value),
            _ => None,
        })
        .collect();
    assert!(
        sweeps.len() > 50,
        "a fade should be many steps, got {}",
        sweeps.len()
    );
    assert!(sweeps[0] > 0.9, "starting at the top: {}", sweeps[0]);
    assert!(
        sweeps[sweeps.len() - 1] < 0.1,
        "and ending at the bottom: {}",
        sweeps[sweeps.len() - 1]
    );
}

#[test]
fn undoing_a_drawn_point_takes_it_off_the_lane() {
    let rig = Rig::new(false);
    let master = rig.master();
    let address = ParamTarget::TrackGain(master).address();
    let mut session = rig.into_session();
    session.create_automation(&address, "Master \u{2014} gain", 0);

    let before = session.automation().unwrap().points.len();
    session.edit_automation(fontelle_ui::canvas::AutomationEdit::Add {
        tick: PPQN,
        value: 0.25,
    });
    assert_eq!(session.automation().unwrap().points.len(), before + 1);

    session.undo();
    assert_eq!(session.automation().unwrap().points.len(), before);
}

// ------------------------------------- automation clips on the arrangement ---
//
// Reported from using the window: *"automation clips aren't drawn on the
// arrangement — they play and open, but a lane of them looks like a lane of
// empty clips."*
//
// The cause was one line: `Session::clips` filtered to `ClipSource::Notes`, so
// an automation clip was in the document, compiled into the timeline, audible,
// and invisible. Two things follow from fixing it — a clip has to say *what
// kind* it is so the canvas can draw a curve instead of a block, and a lane of
// its own so it is not laid on top of the notes it is automating.

#[test]
fn an_automation_clip_shows_up_on_the_arrangement() {
    use fontelle_ui::document::StudioHost;

    let rig = Rig::new(false);
    let master = rig.master();
    let address = ParamTarget::TrackGain(master).address();
    let mut session = rig.into_session();

    let before = session.clips().len();
    session.create_automation(&address, "Master \u{2014} gain", 0);

    let clips = session.clips();
    assert_eq!(
        clips.len(),
        before + 1,
        "the clip is in the document, compiled, audible — and was invisible"
    );
    let made = clips
        .iter()
        .find(|clip| clip.kind == fontelle_ui::document::ClipKind::Automation)
        .expect("the new clip says it is automation");
    assert!(
        made.name.contains("gain"),
        "a block captioned with what it automates: {:?}",
        made.name
    );
}

#[test]
fn an_automation_block_carries_the_curve_it_will_draw() {
    use fontelle_ui::document::StudioHost;

    // The canvas may not see a `Project` (INVARIANT 2), so the shape has to
    // arrive with the block — flattened, like everything else in `ClipInfo`.
    let rig = Rig::new(false);
    let master = rig.master();
    let address = ParamTarget::TrackGain(master).address();
    let mut session = rig.into_session();
    session.create_automation(&address, "Master \u{2014} gain", 0);

    let clips = session.clips();
    let made = clips
        .iter()
        .find(|clip| clip.kind == fontelle_ui::document::ClipKind::Automation)
        .expect("automation clip");
    assert_eq!(
        made.curve.len(),
        2,
        "a fresh lane is a flat segment: two points"
    );
    for (tick, value) in &made.curve {
        assert!(
            *tick >= 0 && *tick <= made.length,
            "a point at {tick} is outside the clip it belongs to"
        );
        assert!(
            (0.0..=1.0).contains(value),
            "values are normalised, got {value}"
        );
    }
    // In time order, so the canvas can draw it as a polyline without sorting.
    assert!(made.curve.windows(2).all(|w| w[0].0 <= w[1].0));
}

#[test]
fn a_note_clip_stays_a_note_clip_and_carries_no_curve() {
    use fontelle_ui::document::StudioHost;

    let rig = Rig::new(false);
    let session = rig.into_session();
    for clip in session.clips() {
        assert_eq!(clip.kind, fontelle_ui::document::ClipKind::Notes);
        assert!(clip.curve.is_empty());
    }
}

#[test]
fn an_automation_clip_gets_a_lane_of_its_own() {
    use fontelle_ui::document::StudioHost;

    // It used to land on `lanes.keys().next()` — the first lane, which is
    // where the notes are. Two clips in the same pixels is exactly the "lane
    // of empty clips" the report describes, and it is worse than invisible:
    // one of them is drawn over the other.
    let rig = Rig::new(false);
    let master = rig.master();
    let address = ParamTarget::TrackGain(master).address();
    let mut session = rig.into_session();

    let lanes_before = session.lanes().len();
    session.create_automation(&address, "Master \u{2014} gain", 0);
    assert_eq!(
        session.lanes().len(),
        lanes_before + 1,
        "automation needs a strip of its own to be readable at all"
    );

    let clips = session.clips();
    let notes: Vec<usize> = clips
        .iter()
        .filter(|c| c.kind == fontelle_ui::document::ClipKind::Notes)
        .map(|c| c.lane)
        .collect();
    let made = clips
        .iter()
        .find(|c| c.kind == fontelle_ui::document::ClipKind::Automation)
        .expect("automation clip");
    assert!(
        !notes.contains(&made.lane),
        "the automation landed on lane {} with the notes",
        made.lane
    );
    assert!(
        session.lanes()[made.lane].name.contains("gain"),
        "the lane says what it automates: {:?}",
        session.lanes()[made.lane].name
    );
}

#[test]
fn automating_the_same_parameter_twice_reuses_its_lane() {
    use fontelle_ui::document::StudioHost;

    // Otherwise every right-click on the same fader adds a strip, and an
    // arrangement grows a lane per gesture rather than per parameter.
    let rig = Rig::new(false);
    let master = rig.master();
    let address = ParamTarget::TrackGain(master).address();
    let mut session = rig.into_session();

    session.create_automation(&address, "Master \u{2014} gain", 0);
    let after_one = session.lanes().len();
    session.create_automation(&address, "Master \u{2014} gain", PPQN * 8);
    assert_eq!(
        session.lanes().len(),
        after_one,
        "the second clip for one parameter belongs on the first one's lane"
    );

    let clips = session.clips();
    let lanes: Vec<usize> = clips
        .iter()
        .filter(|c| c.kind == fontelle_ui::document::ClipKind::Automation)
        .map(|c| c.lane)
        .collect();
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[0], lanes[1]);
}

#[test]
fn automating_a_different_parameter_gets_its_own_lane() {
    use fontelle_ui::document::StudioHost;

    let rig = Rig::new(false);
    let master = rig.master();
    let mut session = rig.into_session();

    session.create_automation(&ParamTarget::TrackGain(master).address(), "gain", 0);
    let after_one = session.lanes().len();
    session.create_automation(&ParamTarget::TrackPan(master).address(), "pan", 0);
    assert_eq!(
        session.lanes().len(),
        after_one + 1,
        "gain and pan are two curves and cannot share a strip"
    );
}

#[test]
fn opening_an_automation_block_opens_its_curve_rather_than_the_roll() {
    use fontelle_ui::document::StudioHost;

    // The handshake a note clip already has, for the other kind of clip:
    // clicking a block opens what is in it.
    let rig = Rig::new(false);
    let master = rig.master();
    let gain = ParamTarget::TrackGain(master).address();
    let pan = ParamTarget::TrackPan(master).address();
    let mut session = rig.into_session();

    session.create_automation(&gain, "Master \u{2014} gain", 0);
    session.create_automation(&pan, "Master \u{2014} pan", 0);
    assert!(
        session.automation().unwrap().title.contains("pan"),
        "the most recent one is open"
    );

    let clips = session.clips();
    let gain_clip = clips
        .iter()
        .find(|c| c.kind == fontelle_ui::document::ClipKind::Automation && c.name.contains("gain"))
        .expect("the gain lane's block");
    session.open_clip(gain_clip.id);

    assert!(
        session.automation().unwrap().title.contains("gain"),
        "clicking a block has to open the curve that is in it"
    );
}

#[test]
fn the_open_automation_block_says_that_it_is_open() {
    use fontelle_ui::document::StudioHost;

    // The same mark a note clip carries, so "which one am I editing" is
    // answerable by looking at the arrangement.
    let rig = Rig::new(false);
    let master = rig.master();
    let mut session = rig.into_session();
    session.create_automation(&ParamTarget::TrackGain(master).address(), "gain", 0);

    let clips = session.clips();
    let made = clips
        .iter()
        .find(|c| c.kind == fontelle_ui::document::ClipKind::Automation)
        .expect("automation clip");
    assert!(made.open, "the clip that was just made is the one open");
}

/// And the panel that knob lives on comes back **saying so**.
///
/// `is_automated` answered this correctly for a long time and nothing asked
/// it: the ring TDD §12.2 describes was drawn by nothing, so a knob a lane had
/// taken over looked exactly like one nobody had touched. This is the join
/// between the two — the session marking the view it builds — and it is the
/// piece that was missing rather than the answer, which was always there.
#[test]
fn a_panel_says_which_of_its_knobs_a_lane_has_taken_over() {
    let mut rig = Rig::new(false);
    let master = rig.master();
    // A compressor rather than an EQ: the EQ draws its own curve and has no
    // generic panel, which is the trait's documented `None`.
    rig.add_insert(master, EffectKind::Compressor);

    let address = ParamTarget::Insert {
        track: master,
        slot: 0,
        param: "ratio".into(),
    }
    .address();

    let mut session = rig.into_session();
    let before = session.insert_view(0, 0).expect("a compressor has a panel");
    assert!(
        before
            .groups
            .iter()
            .flat_map(|group| &group.params)
            .all(|param| !param.automated),
        "nothing is automated to begin with"
    );

    session.create_automation(&address, "Master \u{2014} ratio", 0);

    let after = session.insert_view(0, 0).expect("still a compressor");
    let marked: Vec<&str> = after
        .groups
        .iter()
        .flat_map(|group| &group.params)
        .filter(|param| param.automated)
        .map(|param| param.address.as_str())
        .collect();
    assert_eq!(
        marked,
        vec![address.as_str()],
        "the ratio knob, and only the ratio knob, is under a lane"
    );
}
