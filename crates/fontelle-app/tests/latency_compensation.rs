//! Tracks that cost different amounts of latency still arrive together
//! (TDD §5.5).
//!
//! `AudioNode::latency_samples` has existed since the graph did and nothing
//! read it, so a look-ahead gate on one track put that track up to ten
//! milliseconds behind every other one. It is the sort of error nobody hears
//! as an error — a snare that sits late, a kit that sounds loose — which is
//! exactly why it has to be the compiler's job and not the user's.
//!
//! What this file measures is the **sound**: one click per track, and where
//! it lands in the master's own buffer.

mod common;

use std::sync::Arc;

use fontelle_app::{RealiseOptions, SampleLibrary};
use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, Source,
    VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_model::{
    Arena, Channel, Clip, ClipSource, EffectSlot, Lane, MixerTrack, Note, NoteData, Project,
};
use fontelle_types::{EffectConfig, GateConfig, MixerTrackId, PPQN};

use common::SR;

const BLOCK: usize = fontelle_engine::BLOCK_SIZE;
/// Five milliseconds at 48 kHz, which is 240 samples and nearly two blocks.
const LOOKAHEAD_MS: f32 = 5.0;

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: BLOCK,
        quality: Interpolation::Draft,
    }
}

/// A patch that plays **one sample** at full scale and then silence: a click,
/// so "where did it land" is a single index rather than a judgement.
fn click_patch(library: &mut SampleLibrary, name: &str) -> Patch {
    let mut data = vec![0.0f32; 64];
    data[0] = 1.0;
    let asset = library.insert_synthetic(
        name,
        SampleBuffer {
            data: Arc::from(data),
            sample_rate: SR,
        },
    );
    let off = FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
        ..Default::default()
    };
    let flat = EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 0.0,
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
                loop_start: 0.0,
                loop_end: 64.0,
                end_offset: 64.0,
                ..PlaybackConfig::default()
            },
            gain_db: 0.0,
            pan: 0.0,
        }],
        filters: [off, off],
        envelopes: vec![flat, flat],
        lfos: Vec::new(),
        mod_matrix: ModMatrix::default(),
        voice_config: VoiceConfig::default(),
        ..Default::default()
    }
}

/// A gate that never closes, so the only thing it does to the signal is
/// **delay** it by its look-ahead — the latency, with none of the gating to
/// confuse what is being measured.
fn open_gate(lookahead_ms: f32) -> EffectConfig {
    let mut gate = GateConfig::new();
    gate.threshold_db = -120.0;
    gate.ratio = 1.0;
    gate.range_db = 0.0;
    gate.lookahead_ms = lookahead_ms;
    EffectConfig::Gate(gate)
}

/// Two tracks, a click on each at tick zero, and `lookahead_ms` of gate on
/// the first. Returns the master's rendered left channel.
fn render_two_tracks(lookahead_ms: f32) -> Vec<f32> {
    let mut project = Project::new("latency");
    project.tempo_map = fontelle_model::TempoMap::new(120.0, SR as f64);
    let master = project.mixer.master.expect("a master");
    let mut library = SampleLibrary::new();
    let lane = project.lanes.insert(Lane {
        name: "lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });

    let mut track = |name: &str| -> MixerTrackId {
        let id = project.mixer.tracks.insert(MixerTrack::new(name));
        project.mixer.tracks[id].output = Some(master);
        id
    };
    let gated = track("Gated");
    let plain = track("Plain");
    if lookahead_ms > 0.0 {
        project.mixer.tracks[gated].inserts.push(EffectSlot {
            preset: None,
            config: open_gate(lookahead_ms),
            plugin: None,
            bypassed: false,
            key: None,
            notes: None,
        });
    }

    for (index, id) in [gated, plain].into_iter().enumerate() {
        let patch = click_patch(&mut library, &format!("click{index}"));
        let channel = project.channels.insert(Channel {
            preset: None,
            instrument: None,
            name: format!("click{index}"),
            color: [0; 4],
            mixer_track: Some(id),
            patch_data: None,
            plugin: None,
            pan: 0.0,
            muted: false,
            soloed: false,
            named_keys: false,
            gain_db: 0.0,
        });
        fontelle_app::set_channel_patch(&mut project, channel, &patch, &library)
            .expect("the patch serialises");
        let mut notes = Arena::default();
        notes.insert(Note {
            start: 0,
            length: PPQN,
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
            length: PPQN * 4,
            source: ClipSource::Notes(NoteData { channel, notes }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        });
    }

    let mut realised = fontelle_app::realise(&project, &library, options()).expect("a graph");
    let timeline =
        fontelle_sequencer::compile(&project, &realised.channel_nodes, &Default::default());
    let pcm = fontelle_app::render_offline(&timeline, &mut realised.graph, SR as i64 / 4);
    // Interleaved stereo: the left channel is every other sample.
    pcm.iter().step_by(2).copied().collect()
}

/// Where each click lands, in samples, loudest first.
fn hits(left: &[f32]) -> Vec<usize> {
    let mut found: Vec<usize> = Vec::new();
    for (index, sample) in left.iter().enumerate() {
        if sample.abs() > 0.05 {
            found.push(index);
        }
    }
    found
}

/// With no look-ahead anywhere the two clicks are one click: they land on the
/// same sample and sum. This is the reference the compensated case has to
/// match.
#[test]
fn two_plain_tracks_arrive_together() {
    let left = render_two_tracks(0.0);
    let hits = hits(&left);
    assert_eq!(hits.len(), 1, "one moment of sound, not two: {hits:?}");
}

/// The report this is all about: a look-ahead insert on one track used to put
/// that track behind the other, so the same click arrived twice.
#[test]
fn a_look_ahead_insert_does_not_pull_its_track_out_of_time() {
    let plain = hits(&render_two_tracks(0.0));
    let gated = hits(&render_two_tracks(LOOKAHEAD_MS));
    assert_eq!(
        gated.len(),
        1,
        "the gated track is still in time with the other: {gated:?}"
    );
    // Everything moves back by the look-ahead — that is what compensation
    // *is*, and the whole mix moving together is the point.
    let expected = (LOOKAHEAD_MS / 1000.0 * SR as f32).round() as usize;
    assert_eq!(gated[0], plain[0] + expected, "{gated:?} vs {plain:?}");
}

/// What the whole graph costs, so the audio settings page can say it — TDD
/// §5.5's "reported total latency is surfaced so the user can see what their
/// configuration actually costs".
#[test]
fn the_graph_reports_what_it_costs() {
    let mut project = Project::new("latency");
    project.tempo_map = fontelle_model::TempoMap::new(120.0, SR as f64);
    let master = project.mixer.master.expect("a master");
    let library = SampleLibrary::new();
    let plain = fontelle_app::realise(&project, &library, options()).expect("a graph");
    let base = plain.latency_samples;

    project.mixer.tracks[master].inserts.push(EffectSlot {
        preset: None,
        config: open_gate(LOOKAHEAD_MS),
        plugin: None,
        bypassed: false,
        key: None,
        notes: None,
    });
    let gated = fontelle_app::realise(&project, &library, options()).expect("a graph");
    let expected = (LOOKAHEAD_MS / 1000.0 * SR as f32).round() as u32;
    assert_eq!(
        gated.latency_samples,
        base + expected,
        "a look-ahead on the master costs the whole mix"
    );
}
