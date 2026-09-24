//! Playing an audio clip (TDD §15).
//!
//! *"i want to also be able to record my voice into the daw or import different
//! sounds and loops and whatnot to make songs with."*
//!
//! A note clip becomes **events** and a sampler turns them into sound. An audio
//! clip is not events — it is a continuous stream that has to be somewhere on
//! the song and nowhere else — so it needs a node that reads the transport's
//! own position and decides, per block, which frame of which file belongs where.
//! That is what is tested here, off any device, one block at a time.
//!
//! The properties that matter and are each easy to get quietly wrong:
//!
//! - **A clip is silent outside its own range**, exactly. One sample early is a
//!   click at the start of every take.
//! - **Where it lands does not depend on the block boundary.** Rendering a
//!   clip in one block of 512 and in four of 128 has to give the same samples,
//!   because the device is under no obligation to deliver either.
//! - **The file's rate is not the device's.** A 44.1 kHz loop on a 48 kHz
//!   device is read slower than one frame per frame, or it plays sharp.

use std::sync::Arc;

use fontelle_core::{AudioBuffer, AudioStore};
use fontelle_engine::{
    AudioClipNode, AudioNode, PrepareContext, ProcessContext, TransportSnapshot, TransportState,
};
use fontelle_types::{
    AssetId, AssetKind, AssetRef, AudioClipData, AudioPlacement, ClipId, ClipLoopMode, ClipStretch,
    Fade, FadeCurve, NodeId,
};

const RATE: f32 = 48_000.0;

/// `n` **distinct** ids of one kind.
///
/// Out of one map, deliberately: a fresh `SlotMap`'s first key is the same key
/// every time, so minting two ids from two maps gives one id twice — which is a
/// test that cannot tell "addressed to me" from "addressed to somebody else"
/// apart, and passes either way.
fn ids<K: slotmap::Key>(n: usize) -> Vec<K> {
    let mut map: slotmap::SlotMap<K, ()> = slotmap::SlotMap::with_key();
    (0..n).map(|_| map.insert(())).collect()
}

fn an_id<K: slotmap::Key>() -> K {
    ids(1)[0]
}

/// A mono buffer whose frame *n* holds *n*, so where a sample came from is
/// readable off its value.
fn counting(frames: usize, sample_rate: u32) -> AudioBuffer {
    AudioBuffer {
        data: Arc::from((0..frames).map(|i| i as f32).collect::<Vec<f32>>()),
        sample_rate,
        channels: 1,
    }
}

fn an_asset(id: AssetId) -> AssetRef {
    AssetRef {
        id,
        path: "take.wav".into(),
        content_hash: 0,
        size: 0,
        kind: AssetKind::Sample,
    }
}

/// A node holding one buffer, and the ids to address it by.
struct Rig {
    node: AudioClipNode,
    node_id: NodeId,
    asset: AssetId,
    /// Where fresh clip ids come from, so two placements are two clips.
    clips: slotmap::SlotMap<ClipId, ()>,
}

fn rig(buffer: AudioBuffer) -> Rig {
    let asset: AssetId = an_id();
    let mut store = AudioStore::new();
    store.insert(asset, buffer);
    let mut node = AudioClipNode::new(Arc::new(store));
    node.prepare(&PrepareContext {
        sample_rate: RATE,
        max_block_size: 1024,
    });
    Rig {
        node,
        node_id: an_id(),
        asset,
        clips: slotmap::SlotMap::with_key(),
    }
}

impl Rig {
    fn clip(&self, frames: i64) -> AudioClipData {
        AudioClipData::whole(an_asset(self.asset), frames, 48_000)
    }

    fn placement(&mut self, data: AudioClipData, start: i64, length: i64) -> AudioPlacement {
        AudioPlacement {
            target: self.node_id,
            clip: self.clips.insert(()),
            range: start..start + length,
            repeat: 0,
            crossfade_in: 0,
            crossfade_out: 0,
            phase: 0,
            data,
        }
    }

    /// Renders `frames` of song time from `from`, in blocks of `block`.
    fn render(
        &mut self,
        placements: &[AudioPlacement],
        from: i64,
        frames: usize,
        block: usize,
    ) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames);
        let mut at = from;
        while out.len() < frames {
            let n = block.min(frames - out.len());
            let mut left = vec![0.0f32; n];
            let mut right = vec![0.0f32; n];
            {
                let mut outputs: [&mut [f32]; 2] = [&mut left, &mut right];
                let mut ctx = ProcessContext {
                    inputs: &[],
                    outputs: &mut outputs,
                    all_events: &[],
                    live_events: &[],
                    audio: placements,
                    node: self.node_id,
                    transport: TransportSnapshot {
                        state: TransportState::Playing,
                        position_sample: at,
                        bpm: 120.0,
                        ..Default::default()
                    },
                    sample_range: at..at + n as i64,
                };
                self.node.process(&mut ctx);
            }
            out.extend_from_slice(&left);
            at += n as i64;
        }
        out
    }
}

// ------------------------------------------------------------ where it is ---

#[test]
fn a_clip_sounds_where_the_arrangement_put_it_and_nowhere_else() {
    let mut r = rig(counting(100, 48_000));
    let clip = r.clip(100);
    let placements = vec![r.placement(clip, 200, 100)];

    let out = r.render(&placements, 0, 400, 400);
    for (i, value) in out.iter().enumerate() {
        if (200..300).contains(&i) {
            assert_eq!(*value, (i - 200) as f32, "frame {i} is the wrong frame");
        } else {
            assert_eq!(*value, 0.0, "frame {i} sounds and should not");
        }
    }
}

#[test]
fn a_block_that_starts_in_the_middle_of_a_clip_carries_on_from_there() {
    // Seeking into a take, and every block after the first.
    let mut r = rig(counting(100, 48_000));
    let clip = r.clip(100);
    let placements = vec![r.placement(clip, 0, 100)];
    let out = r.render(&placements, 40, 20, 20);
    assert_eq!(out[0], 40.0);
    assert_eq!(out[19], 59.0);
}

#[test]
fn where_a_clip_lands_does_not_depend_on_the_block_size() {
    // Real ALSA hardware here delivers 85 frames against a 128-frame buffer.
    // A node that counted blocks rather than samples would drift by the
    // difference every callback — the bug that made the graph sound
    // bitcrushed once already.
    let mut r = rig(counting(1000, 48_000));
    let clip = r.clip(1000);
    let placements = vec![r.placement(clip, 37, 900)];

    let whole = r.render(&placements, 0, 1000, 1000);
    for block in [1, 7, 85, 128, 512] {
        let split = r.render(&placements, 0, 1000, block);
        assert_eq!(split, whole, "blocks of {block} render differently");
    }
}

#[test]
fn two_clips_on_one_track_are_summed_rather_than_one_winning() {
    let mut r = rig(counting(100, 48_000));
    let clip = r.clip(100);
    let placements = vec![r.placement(clip.clone(), 0, 100), r.placement(clip, 0, 100)];
    let out = r.render(&placements, 0, 10, 10);
    assert_eq!(out[5], 10.0, "two copies of frame 5 is twice frame 5");
}

#[test]
fn a_clip_addressed_to_another_node_is_not_this_nodes_business() {
    let mut r = rig(counting(100, 48_000));
    let clip = r.clip(100);
    let mut elsewhere = r.placement(clip, 0, 100);
    elsewhere.target = ids::<NodeId>(2)[1];
    assert_ne!(elsewhere.target, r.node_id);
    let out = r.render(&[elsewhere], 0, 50, 50);
    assert!(
        out.iter().all(|v| *v == 0.0),
        "it played somebody else's clip"
    );
}

#[test]
fn a_stopped_transport_plays_nothing() {
    let mut r = rig(counting(100, 48_000));
    let clip = r.clip(100);
    let placements = vec![r.placement(clip, 0, 100)];
    let mut left = vec![0.0f32; 32];
    let mut right = vec![0.0f32; 32];
    let mut outputs: [&mut [f32]; 2] = [&mut left, &mut right];
    let mut ctx = ProcessContext {
        inputs: &[],
        outputs: &mut outputs,
        all_events: &[],
        live_events: &[],
        audio: &placements,
        node: r.node_id,
        transport: TransportSnapshot {
            state: TransportState::Stopped,
            position_sample: 0,
            bpm: 120.0,
            ..Default::default()
        },
        sample_range: 0..32,
    };
    r.node.process(&mut ctx);
    assert!(left.iter().all(|v| *v == 0.0));
}

// ------------------------------------------------------------ how it reads ---

#[test]
fn a_file_recorded_at_another_rate_is_read_at_the_ratio_between_them() {
    // A 24 kHz file on a 48 kHz device advances half a frame per frame. Read
    // one-for-one it would play an octave sharp and half as long, which is the
    // single most common bug in this whole area.
    let mut r = rig(counting(100, 24_000));
    let clip = r.clip(100);
    let placements = vec![r.placement(clip, 0, 200)];
    let out = r.render(&placements, 0, 20, 20);
    assert_eq!(out[0], 0.0);
    assert!((out[10] - 5.0).abs() < 1e-4, "frame 10 read {}", out[10]);
}

#[test]
fn a_trimmed_clip_starts_at_the_frame_the_trim_names() {
    let mut r = rig(counting(1000, 48_000));
    let mut clip = r.clip(1000);
    clip.source_start = 400;
    clip.source_end = 500;
    let placements = vec![r.placement(clip, 0, 100)];
    let out = r.render(&placements, 0, 10, 10);
    assert_eq!(out[0], 400.0);
    assert_eq!(out[9], 409.0);
}

#[test]
fn a_clip_longer_than_its_own_audio_goes_quiet_rather_than_repeating() {
    let mut r = rig(counting(50, 48_000));
    let mut clip = r.clip(50);
    clip.loop_mode = ClipLoopMode::Once;
    let placements = vec![r.placement(clip, 0, 200)];
    let out = r.render(&placements, 0, 200, 200);
    assert_eq!(out[49], 49.0);
    assert!(
        out[50..].iter().all(|v| *v == 0.0),
        "a take that ran out is silence, not its own front again"
    );
}

#[test]
fn a_looping_clip_comes_round_again_for_as_long_as_the_block_is_long() {
    let mut r = rig(counting(50, 48_000));
    let mut clip = r.clip(50);
    clip.loop_mode = ClipLoopMode::Loop;
    let placements = vec![r.placement(clip, 0, 200)];
    let out = r.render(&placements, 0, 200, 64);
    assert_eq!(out[49], 49.0);
    assert_eq!(out[50], 0.0, "the loop came round");
    assert_eq!(out[75], 25.0);
    assert_eq!(out[199], 49.0);
}

#[test]
fn a_reversed_clip_plays_backwards_and_starts_on_a_real_sample() {
    let mut r = rig(counting(100, 48_000));
    let mut clip = r.clip(100);
    clip.reverse = true;
    let placements = vec![r.placement(clip, 0, 100)];
    let out = r.render(&placements, 0, 100, 100);
    assert_eq!(out[0], 99.0, "a reversed clip must not begin with a gap");
    assert_eq!(out[99], 0.0);
}

// -------------------------------------------------------------- how loud ---

#[test]
fn the_boost_multiplies_what_comes_out() {
    let mut r = rig(counting(100, 48_000));
    let mut clip = r.clip(100);
    clip.gain_db = 6.0206;
    let placements = vec![r.placement(clip, 0, 100)];
    let out = r.render(&placements, 0, 10, 10);
    assert!(
        (out[4] - 8.0).abs() < 0.01,
        "frame 4 at +6 dB is {}",
        out[4]
    );
}

#[test]
fn a_fade_in_arrives_from_silence() {
    let mut r = rig(counting(100, 48_000));
    let mut clip = r.clip(100);
    clip.fade_in = Fade {
        frames: 50,
        curve: FadeCurve::Linear,
        tension: 0.0,
    };
    let placements = vec![r.placement(clip, 0, 100)];
    let out = r.render(&placements, 0, 100, 100);
    assert_eq!(out[0], 0.0);
    // Frame 25 is worth 25, at half the fade.
    assert!((out[25] - 12.5).abs() < 0.1, "frame 25 is {}", out[25]);
    assert_eq!(out[60], 60.0, "past the fade it is untouched");
}

#[test]
fn a_muted_clip_is_not_placed_at_all() {
    // The sequencer does not compile a placement for a muted clip, which is
    // this node's half of the claim: given none, it sounds none. (The other
    // half is `fontelle-sequencer`'s own test.)
    let mut r = rig(counting(100, 48_000));
    let out = r.render(&[], 0, 50, 50);
    assert!(out.iter().all(|v| *v == 0.0));
}

// ------------------------------------------------------------- the panning ---

#[test]
fn a_mono_clip_is_heard_on_both_sides() {
    let mut r = rig(counting(100, 48_000));
    let clip = r.clip(100);
    let placements = vec![r.placement(clip, 0, 100)];
    let mut left = vec![0.0f32; 10];
    let mut right = vec![0.0f32; 10];
    let mut outputs: [&mut [f32]; 2] = [&mut left, &mut right];
    let mut ctx = ProcessContext {
        inputs: &[],
        outputs: &mut outputs,
        all_events: &[],
        live_events: &[],
        audio: &placements,
        node: r.node_id,
        transport: TransportSnapshot {
            state: TransportState::Playing,
            position_sample: 0,
            bpm: 120.0,
            ..Default::default()
        },
        sample_range: 0..10,
    };
    r.node.process(&mut ctx);
    assert_eq!(left[5], right[5], "a mono take came out of one speaker");
    assert!(left[5] > 0.0);
}

#[test]
fn panning_a_clip_hard_left_takes_it_out_of_the_right() {
    let mut r = rig(counting(100, 48_000));
    let mut clip = r.clip(100);
    clip.pan = -1.0;
    let placements = vec![r.placement(clip, 0, 100)];
    let mut left = vec![0.0f32; 10];
    let mut right = vec![0.0f32; 10];
    let mut outputs: [&mut [f32]; 2] = [&mut left, &mut right];
    let mut ctx = ProcessContext {
        inputs: &[],
        outputs: &mut outputs,
        all_events: &[],
        live_events: &[],
        audio: &placements,
        node: r.node_id,
        transport: TransportSnapshot {
            state: TransportState::Playing,
            position_sample: 0,
            bpm: 120.0,
            ..Default::default()
        },
        sample_range: 0..10,
    };
    r.node.process(&mut ctx);
    assert!(left[5] > 0.0);
    assert!(
        right[5].abs() < 1e-6,
        "hard left still came out of the right"
    );
}

// ---------------------------------------------- stretching to the tempo ---
//
// > *"audio clips arent stretching to match tempo changes in realtime right
// > now [and] soundclips dont have options right now for selecting their mixer
// > track and stretch mode."*
//
// The block a clip occupies is already the tempo map's answer: the sequencer
// turns the clip's **ticks** into a sample range, so a tempo change moves that
// range without anything here being told. All a stretching clip has to do is
// fill whatever range it is given rather than read its file at the file's own
// rate and stop when it runs out.
//
// Which is why this needs no clock and no new plumbing: change the range, and
// a stretched clip changes with it.

#[test]
fn a_clip_that_does_not_stretch_reads_its_file_at_the_files_own_rate() {
    // The default, and the identity this whole feature rests on: a file
    // dropped on the arrangement sounds exactly like the file.
    let clip = AudioClipData::whole(an_asset(an_id()), 1000, 48_000);
    assert_eq!(clip.stretch, ClipStretch::Off);
    assert!((clip.read_ratio(48_000, 48_000.0, 4000) - 1.0).abs() < 1e-9);
    // And a 44.1 kHz file on a 48 kHz device still advances slower than one
    // frame per frame, whatever the block is.
    let slow = clip.read_ratio(44_100, 48_000.0, 4000);
    assert!((slow - 44_100.0 / 48_000.0).abs() < 1e-9);
}

#[test]
fn a_stretching_clip_spreads_its_file_across_the_block_it_is_given() {
    let mut clip = AudioClipData::whole(an_asset(an_id()), 1000, 48_000);
    clip.stretch = ClipStretch::Resample;
    // A thousand frames of file over four thousand frames of song is a quarter
    // of a frame each — and over five hundred, two.
    assert!((clip.read_ratio(48_000, 48_000.0, 4000) - 0.25).abs() < 1e-9);
    assert!((clip.read_ratio(48_000, 48_000.0, 500) - 2.0).abs() < 1e-9);
    // The file's own rate stops mattering, which is the point: what decides
    // the read is how long the bar is.
    assert!((clip.read_ratio(44_100, 48_000.0, 4000) - 0.25).abs() < 1e-9);
}

#[test]
fn halving_the_tempo_doubles_the_block_and_the_clip_follows_it() {
    // *"stretching to match tempo changes"*, stated as the arithmetic it is:
    // the sequencer hands over twice the range and the same file fills it.
    let mut clip = AudioClipData::whole(an_asset(an_id()), 96_000, 48_000);
    clip.stretch = ClipStretch::Resample;
    let at_120 = clip.read_ratio(48_000, 48_000.0, 96_000);
    let at_60 = clip.read_ratio(48_000, 48_000.0, 192_000);
    assert!((at_120 - 1.0).abs() < 1e-9, "it should sit at its own rate");
    assert!(
        (at_60 - 0.5).abs() < 1e-9,
        "half the tempo, half the read rate"
    );
}

#[test]
fn a_block_of_nothing_asks_for_nothing_rather_than_dividing_by_it() {
    let mut clip = AudioClipData::whole(an_asset(an_id()), 1000, 48_000);
    clip.stretch = ClipStretch::Resample;
    assert!(clip.read_ratio(48_000, 48_000.0, 0).is_finite());
    let empty = AudioClipData::whole(an_asset(an_id()), 0, 48_000);
    assert!(empty.read_ratio(48_000, 48_000.0, 4000).is_finite());
}

#[test]
fn a_stretched_clip_is_heard_across_the_whole_block_it_was_given() {
    // The end of it, through the real node: a clip whose file is a quarter as
    // long as its block used to go silent three-quarters of the way through.
    let mut rig = rig(counting(1000, 48_000));
    let mut data = rig.clip(1000);
    data.stretch = ClipStretch::Resample;
    let placement = rig.placement(data, 0, 4000);
    let out = rig.render(&[placement], 0, 4000, 128);

    let tail = &out[3800..];
    assert!(
        tail.iter().any(|s| *s != 0.0),
        "the clip stopped before its block did"
    );
    // And it walked the file once, in order: the last frames are the end of
    // the file rather than the middle of it.
    assert!(out[3990] > 900.0, "it ended at frame {}", out[3990]);
}

// ------------------------------------------ the arrangement's own repeat ---
//
// *"resolve the issue of it trying to stretch while looping"*: what a loop
// on the arrangement (`AudioPlacement::repeat`) does to the read rate is
// decided by the clip's mode and nothing else. These two are the guarantee
// the Stretch switch on the toolbar rests on.

#[test]
fn an_arrangement_loop_that_does_not_stretch_plays_the_file_at_its_own_rate_every_pass() {
    // Fifty frames of file, a period of a hundred: each pass is the file, then
    // silence until the seam, then the file again from its first frame.
    let mut r = rig(counting(50, 48_000));
    let clip = r.clip(50);
    let mut placement = r.placement(clip, 0, 400);
    placement.repeat = 100;
    let out = r.render(&[placement], 0, 400, 64);
    assert_eq!(out[49], 49.0);
    assert_eq!(out[75], 0.0, "the file is over until the seam");
    assert_eq!(out[100], 0.0, "and starts again at it");
    assert_eq!(out[125], 25.0);
    assert_eq!(out[349], 49.0);
    assert_eq!(out[399], 0.0);
}

#[test]
fn an_arrangement_loop_that_stretches_fills_each_pass_with_the_whole_file() {
    let mut r = rig(counting(50, 48_000));
    let mut clip = r.clip(50);
    clip.stretch = ClipStretch::Resample;
    let mut placement = r.placement(clip, 0, 400);
    placement.repeat = 100;
    let out = r.render(&[placement], 0, 400, 64);
    // Half a frame of file per frame of song: the file's end lands at the
    // seam, not in the middle of the pass.
    assert!((out[98] - 49.0).abs() < 1.5, "{}", out[98]);
    assert!((out[150] - 25.0).abs() < 1.0, "{}", out[150]);
    assert!(out[100] < 2.0, "the pass starts over: {}", out[100]);
    assert!((out[398] - 49.0).abs() < 1.5, "{}", out[398]);
}

// ------------------------------------------------ pitch without stretching ---
//
// Reported from using the window: *"changing pitch is stretching the audio
// even when stretch is off and not set to resample."* With the switch off, a
// clip's pitch is a pitch and its speed is a speed: each moves without the
// other. `Resample` stays varispeed, because that is what it is.

/// A mono sine at `hz` for `seconds`, at 48 kHz.
fn sine(hz: f32, seconds: f32) -> AudioBuffer {
    let frames = (RATE * seconds) as usize;
    AudioBuffer {
        data: Arc::from(
            (0..frames)
                .map(|i| (i as f32 * hz * std::f32::consts::TAU / RATE).sin() * 0.8)
                .collect::<Vec<f32>>(),
        ),
        sample_rate: 48_000,
        channels: 1,
    }
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

/// The power at `hz` in `samples`, Hann-windowed.
fn power_at(samples: &[f32], hz: f32) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
        let phase = std::f32::consts::TAU * hz * i as f32 / RATE;
        re += sample * window * phase.cos();
        im -= sample * window * phase.sin();
    }
    (re * re + im * im) / (n * n)
}

#[test]
fn an_octave_up_with_stretch_off_is_an_octave_up_for_the_whole_length() {
    let mut rig = rig(sine(220.0, 1.0));
    let mut data = rig.clip(48_000);
    data.pitch_semitones = 12.0;
    assert_eq!(data.stretch, ClipStretch::Off);
    let placement = rig.placement(data, 0, 48_000);
    let out = rig.render(&[placement], 0, 48_000, 512);

    // Still sounding at the end: varispeed would have run out of file half
    // way along the block.
    assert!(
        rms(&out[43_200..48_000]) > 0.3,
        "the last tenth is quiet ({})",
        rms(&out[43_200..48_000])
    );
    // And it is 440 Hz now, not 220 — at the level it went in at, which is
    // what lining the grains up buys: unaligned, two grains a hundred
    // degrees apart cancel most of a tone.
    let middle = &out[12_000..36_000];
    assert!(
        rms(middle) > 0.45,
        "the tone should keep its level through the shift: {}",
        rms(middle)
    );
    let up = power_at(middle, 440.0);
    let down = power_at(middle, 220.0);
    assert!(
        up > down * 30.0,
        "440 Hz should dominate 220 Hz: {up:e} against {down:e}"
    );
}

#[test]
fn half_speed_with_stretch_off_takes_twice_as_long_at_the_same_pitch() {
    let mut rig = rig(sine(220.0, 1.0));
    let mut data = rig.clip(48_000);
    data.speed = 0.5;
    let placement = rig.placement(data, 0, 96_000);
    let out = rig.render(&[placement], 0, 96_000, 512);

    // Sounding a second and a half in, where the file's own rate would have
    // been silence for half a second.
    assert!(rms(&out[72_000..76_800]) > 0.3);
    let middle = &out[24_000..72_000];
    assert!(
        rms(middle) > 0.45,
        "the tone should keep its level through the stretch: {}",
        rms(middle)
    );
    let same = power_at(middle, 220.0);
    let low = power_at(middle, 110.0);
    assert!(
        same > low * 30.0,
        "220 Hz should dominate 110 Hz: {same:e} against {low:e}"
    );
}

#[test]
fn pitch_and_speed_that_agree_are_exactly_the_plain_read() {
    // The identity the stretch switch's "off" relies on: doubled speed *and*
    // an octave up is a plain read at twice the rate, sample for sample —
    // no grains, no windows, nothing between the file and the output.
    let mut rig = rig(counting(4000, 48_000));
    let mut data = rig.clip(4000);
    data.speed = 2.0;
    data.pitch_semitones = 12.0;
    let placement = rig.placement(data, 0, 1000);
    let out = rig.render(&[placement], 0, 1000, 256);
    for (n, sample) in out.iter().enumerate() {
        assert!(
            (sample - 2.0 * n as f32).abs() < 1e-3,
            "frame {n} read {sample}, wanted {}",
            2 * n
        );
    }
}

#[test]
fn the_grains_add_up_to_one_everywhere() {
    // Every grain's window sums to one with its neighbour's, so a constant
    // in is the same constant out at any shift: no dip at a grain boundary,
    // no swell where two overlap, and no fade at the clip's front where only
    // the first grain exists yet.
    let mut rig = rig(AudioBuffer {
        data: Arc::from(vec![0.7f32; 8000]),
        sample_rate: 48_000,
        channels: 1,
    });
    let mut data = rig.clip(8000);
    data.pitch_semitones = 7.0;
    let placement = rig.placement(data, 0, 3000);
    let out = rig.render(&[placement], 0, 3000, 512);
    for (n, sample) in out.iter().enumerate() {
        assert!((sample - 0.7).abs() < 1e-4, "frame {n} read {sample}");
    }
}

#[test]
fn a_stretched_clip_still_repitches_by_varispeed() {
    // `Resample` is varispeed by definition: an octave up over a fixed block
    // is through the file in half of it, and silent after.
    let mut rig = rig(sine(220.0, 1.0));
    let mut data = rig.clip(48_000);
    data.stretch = ClipStretch::Resample;
    data.pitch_semitones = 12.0;
    let placement = rig.placement(data, 0, 48_000);
    let out = rig.render(&[placement], 0, 48_000, 512);
    assert!(rms(&out[0..12_000]) > 0.3, "the front sounds");
    assert!(
        rms(&out[30_000..48_000]) < 1e-3,
        "the back is silence ({})",
        rms(&out[30_000..48_000])
    );
}
