//! Send to sampler (Analyze Musically P4, plan §3.8):
//! `Session::add_sampler_slices`.

mod common;

use std::path::PathBuf;

use fontelle_analysis::slice::{CHOP_FIRST_KEY, SliceLayout, slices_equal};
use fontelle_app::render_offline;
use fontelle_assets::fixtures::build_wav;
use fontelle_model::{AddClip, Arena, Clip, ClipSource, Command, Note, NoteData};
use fontelle_types::{ChannelId, InstrumentKind, PPQN};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

/// A slice: two beats at the rig's 120 BPM, in frames. On whole ticks, so
/// a replayed note is scheduled on the sample its slice started at, **and**
/// a whole number of engine blocks — the sampler starts a note at the top of
/// the block its event falls in (`SamplerNode::process` triggers as it reads
/// the block's events), so a slice at frame 24 000 would replay 64 frames
/// early and the residual would measure the engine's block, not the slicing.
const SLICE: usize = SR as usize;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fontelle-analyze-sampler-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("creatable");
    dir
}

/// Four beats, each a different pitch and colour, so a slice can be told
/// from its neighbours by correlation.
fn four_tones() -> Vec<f32> {
    let hz = [110.0, 233.0, 370.0, 523.0];
    let mut out = Vec::with_capacity(4 * SLICE);
    for (i, f) in hz.iter().enumerate() {
        for n in 0..SLICE {
            let t = n as f64 / f64::from(SR);
            let v = (std::f64::consts::TAU * f * t).sin() * 0.3
                + (std::f64::consts::TAU * f * (2.0 + i as f64) * t).sin() * 0.1;
            out.push(v as f32);
        }
    }
    out
}

fn wav_of(dir: &std::path::Path, samples: &[f32]) -> PathBuf {
    let path = dir.join("Phrase slices.wav");
    std::fs::write(&path, build_wav(SR, 1, samples)).expect("writable");
    path
}

/// The file as the sampler will hold it: 16 bits, read back.
fn as_heard(samples: &[f32]) -> Vec<f32> {
    samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() / 32767.0)
        .collect()
}

/// Renders `notes` (key, start tick, length ticks) on `channel` alone.
fn render_notes(
    session: &fontelle_app::Session,
    channel: ChannelId,
    notes: &[(u8, i64, i64)],
    frames: usize,
) -> Vec<f32> {
    let mut project = session.project().clone();
    // Only this part: every other clip off the arrangement.
    let ids: Vec<_> = project.clips.keys().collect();
    for id in ids {
        project.clips.remove(id);
    }
    let lane = project.lane_ids()[0];
    let mut arena = Arena::default();
    for &(key, start, length) in notes {
        arena.insert(Note {
            start,
            length,
            key,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            path: Vec::new(),
            channel: None,
        });
    }
    AddClip::new(Clip {
        name: None,
        lane,
        start: 0,
        length: PPQN * 16,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: arena,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    })
    .apply(&mut project)
    .expect("a clip");
    let (mut realised, timeline) =
        common::realise_at(&project, session.library(), fontelle_app::PLAYBACK_QUALITY);
    // Rendered long by room for the master's look-ahead, which the caller
    // takes off the front with `aligned`: the graph's own latency is not the
    // slicing's business.
    let stereo = render_offline(&timeline, &mut realised.graph, (frames + LOOK) as i64);
    stereo.chunks(2).map(|f| f[0]).collect()
}

/// Room for the graph's latency (the master limiter's look-ahead).
const LOOK: usize = 512;

/// `out` with the graph's latency taken off the front: the lag, up to
/// [`LOOK`], at which it best matches `reference`.
fn aligned<'a>(out: &'a [f32], reference: &[f32]) -> &'a [f32] {
    let n = reference.len().min(out.len() - LOOK);
    let lag = (0..=LOOK)
        .max_by(|a, b| {
            correlation(&out[*a..*a + n], &reference[..n])
                .total_cmp(&correlation(&out[*b..*b + n], &reference[..n]))
        })
        .unwrap_or(0);
    &out[lag..lag + n]
}

/// The correlation of `a` and `b`, normalised: 1 is the same shape.
fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    let (mut ab, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..n {
        let (x, y) = (f64::from(a[i]), f64::from(b[i]));
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    ab / (aa.sqrt() * bb.sqrt()).max(1e-12)
}

#[test]
fn one_undo_removes_the_channel_and_the_clip() {
    let dir = scratch("undo");
    let audio = four_tones();
    let path = wav_of(&dir, &audio);
    let mut session = common::a_session_for(common::a_clip_project(4));
    let (channels, clips, rows) = (
        session.project().channels.len(),
        session.project().clips.len(),
        session.project().lanes.len(),
    );

    let made = session
        .add_sampler_slices(
            &path,
            &slices_equal(audio.len(), 4),
            SliceLayout::Chop,
            Some(0),
        )
        .expect("adds");
    assert_eq!(made.unmapped, 0);
    assert_eq!(session.project().channels.len(), channels + 1);
    assert_eq!(session.project().clips.len(), clips + 1);
    assert_eq!(
        session.channel_kind(session.channels().len() - 1),
        Some(InstrumentKind::Sampler)
    );
    let clip = made.clip.expect("a replay clip was asked for");
    match &session.project().clips[clip].source {
        ClipSource::Notes(data) => {
            assert_eq!(data.channel, made.channel);
            let mut keys: Vec<(i64, u8)> = data.notes.values().map(|n| (n.start, n.key)).collect();
            keys.sort();
            assert_eq!(
                keys,
                (0..4)
                    .map(|i| (i * 2 * PPQN, CHOP_FIRST_KEY + i as u8))
                    .collect::<Vec<_>>()
            );
        }
        other => panic!("the replay clip is {other:?}"),
    }

    session.undo();
    assert_eq!(session.project().channels.len(), channels);
    assert_eq!(session.project().clips.len(), clips);
    assert_eq!(session.project().lanes.len(), rows, "the replay row stayed");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn each_key_plays_its_slice() {
    let dir = scratch("keys");
    let audio = four_tones();
    let path = wav_of(&dir, &audio);
    let heard = as_heard(&audio);
    let mut session = common::a_session_for(common::a_clip_project(4));
    let slices = slices_equal(audio.len(), 4);
    let made = session
        .add_sampler_slices(&path, &slices, SliceLayout::Chop, None)
        .expect("adds");
    assert!(made.clip.is_none());

    for i in 0..slices.len() {
        let out = render_notes(
            &session,
            made.channel,
            &[(CHOP_FIRST_KEY + i as u8, 0, 2 * PPQN)],
            SLICE,
        );
        let out = aligned(&out, &heard[slices[i].start..slices[i].end]);
        // Away from the very start, so a voice's first block does not decide.
        let window = 1024..SLICE - 1024;
        for (j, other) in slices.iter().enumerate() {
            let c = correlation(
                &out[window.clone()],
                &heard[other.start + window.start..other.start + window.end],
            );
            if i == j {
                assert!(c > 0.99, "key {} plays its slice at only {c:.3}", 48 + i);
            } else {
                assert!(
                    c.abs() < 0.3,
                    "key {} sounds like slice {j} ({c:.3})",
                    48 + i
                );
            }
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_replay_clip_reproduces_the_source_within_minus_30_db() {
    let dir = scratch("replay");
    let audio = four_tones();
    let path = wav_of(&dir, &audio);
    let heard = as_heard(&audio);
    let mut session = common::a_session_for(common::a_clip_project(4));
    let made = session
        .add_sampler_slices(
            &path,
            &slices_equal(audio.len(), 4),
            SliceLayout::Chop,
            Some(0),
        )
        .expect("adds");
    let clip = made.clip.expect("asked for");
    let notes: Vec<(u8, i64, i64)> = match &session.project().clips[clip].source {
        ClipSource::Notes(data) => data
            .notes
            .values()
            .map(|n| (n.key, n.start, n.length))
            .collect(),
        _ => unreachable!(),
    };
    let out = render_notes(&session, made.channel, &notes, audio.len());
    let out = aligned(&out, &heard);
    // One gain fitted (velocity and pan law set the level, not the slicing);
    // what is left over is what the slicing got wrong.
    let gain = out
        .iter()
        .zip(&heard)
        .map(|(o, h)| f64::from(*o) * f64::from(*h))
        .sum::<f64>()
        / heard
            .iter()
            .map(|h| f64::from(*h) * f64::from(*h))
            .sum::<f64>();
    assert!(gain > 0.1, "the replay is silent");
    let residual: f64 = out
        .iter()
        .zip(&heard)
        .map(|(o, h)| (f64::from(*o) - gain * f64::from(*h)).powi(2))
        .sum();
    let signal: f64 = heard.iter().map(|h| (gain * f64::from(*h)).powi(2)).sum();
    let db = 10.0 * (residual / signal).log10();
    assert!(db < -30.0, "the replay is {db:.1} dB off the source");
    std::fs::remove_dir_all(&dir).ok();
}
