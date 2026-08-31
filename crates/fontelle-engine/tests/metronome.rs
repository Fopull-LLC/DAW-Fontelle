//! The metronome (item 9 of `docs/first-usable-plan.md`).
//!
//! The last piece of that item along with record-arm, and the one that makes
//! recording a take usable at all: playing in time to a part you have not
//! written yet needs something to play in time *to*.
//!
//! # Why it is a node and not a clip
//!
//! A click is not document data. It is not saved, it does not bounce, it does
//! not belong to a channel and it has no notes — a project emailed to somebody
//! else must not arrive with a woodblock on every beat. So it is an engine
//! node reading the transport's own position, switched by an atomic like every
//! other live control in this window (see `fontelle_engine::TrackControls`).
//!
//! Reading the **position** rather than counting elapsed blocks is what makes
//! a seek and a loop land on the beat: the click is a function of where the
//! playhead is, not of how long the node has been running.

use fontelle_engine::{AudioNode, Metronome, MetronomeNode, PrepareContext, ProcessContext};
use fontelle_engine::{TransportSnapshot, TransportState};
use std::sync::Arc;

const SR: f32 = 48_000.0;
/// One beat at 120 BPM.
const BEAT: i64 = 24_000;

fn node(metronome: &Arc<Metronome>) -> MetronomeNode {
    let mut node = MetronomeNode::new(Arc::clone(metronome));
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    node
}

/// Renders `frames` starting at `from`, in `block`-frame chunks, and returns
/// the peak of each chunk — which is where the clicks are.
fn peaks(node: &mut MetronomeNode, from: i64, frames: usize, block: usize) -> Vec<(i64, f32)> {
    let mut out = Vec::new();
    let mut at = from;
    let mut left = frames;
    while left > 0 {
        let n = left.min(block);
        let mut buffer = vec![0.0f32; n];
        {
            let mut outputs: Vec<&mut [f32]> = vec![&mut buffer];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut outputs,
                all_events: &[],
                live_events: &[],
                node: Default::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: at,
                },
                sample_range: at..at + n as i64,
            };
            node.process(&mut ctx);
        }
        let peak = buffer.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        out.push((at, peak));
        at += n as i64;
        left -= n;
    }
    out
}

/// Where each click **began**.
///
/// The runs, not the loud blocks: a click is thirty milliseconds, which at a
/// 128-frame block is eleven of them. Counting blocks would count one click
/// eleven times, which is how this test first read.
fn clicked(peaks: &[(i64, f32)]) -> Vec<i64> {
    let mut starts = Vec::new();
    let mut sounding = false;
    for (at, peak) in peaks {
        let loud = *peak > 0.01;
        if loud && !sounding {
            starts.push(*at);
        }
        sounding = loud;
    }
    starts
}

/// The peak of each click, in order — its first block's, which is the loudest
/// because the click decays from there.
fn click_peaks(peaks: &[(i64, f32)]) -> Vec<f32> {
    let mut out = Vec::new();
    let mut sounding = false;
    for (_, peak) in peaks {
        let loud = *peak > 0.01;
        if loud && !sounding {
            out.push(*peak);
        }
        sounding = loud;
    }
    out
}

#[test]
fn a_metronome_that_is_off_is_silent() {
    // It is off by default: a window that clicks at you the first time you
    // press play is one you have to go and find the switch for.
    let metronome = Arc::new(Metronome::new());
    assert!(!metronome.is_on());
    let mut node = node(&metronome);
    assert!(clicked(&peaks(&mut node, 0, BEAT as usize * 4, 128)).is_empty());
}

#[test]
fn a_metronome_that_is_on_clicks_once_a_beat() {
    let metronome = Arc::new(Metronome::new());
    metronome.set_on(true);
    metronome.set_beat(BEAT as u32, 4);
    let mut node = node(&metronome);

    let clicks = clicked(&peaks(&mut node, 0, BEAT as usize * 4, 128));
    assert_eq!(clicks.len(), 4, "four beats, four clicks: {clicks:?}");
    // Each one inside the block that holds its beat.
    for (index, at) in clicks.iter().enumerate() {
        let beat = BEAT * index as i64;
        assert!(
            (*at..*at + 128).contains(&beat),
            "click {index} at {at} is not the block holding beat {beat}"
        );
    }
}

#[test]
fn the_first_beat_of_a_bar_is_louder_than_the_others() {
    // Which is the whole point of a metronome rather than a pulse: it says
    // where you are, not only how fast.
    let metronome = Arc::new(Metronome::new());
    metronome.set_on(true);
    metronome.set_beat(BEAT as u32, 4);
    let mut node = node(&metronome);

    let all = peaks(&mut node, 0, BEAT as usize * 4, 128);
    let loud = click_peaks(&all);
    assert_eq!(loud.len(), 4);
    assert!(
        loud[0] > loud[1] * 1.2,
        "the downbeat should be clearly louder: {loud:?}"
    );
}

#[test]
fn a_seek_lands_on_the_beat_rather_than_restarting_the_count() {
    // The reason the node reads the transport's *position* instead of counting
    // its own elapsed blocks: jumping to bar 5 has to click on bar 5's beats,
    // not on a grid measured from wherever playback happened to start.
    let metronome = Arc::new(Metronome::new());
    metronome.set_on(true);
    metronome.set_beat(BEAT as u32, 4);
    let mut node = node(&metronome);

    // Start halfway through a beat, so a node counting from zero would be
    // permanently offset.
    let from = BEAT * 8 + BEAT / 2;
    let clicks = clicked(&peaks(&mut node, from, BEAT as usize * 2, 128));
    assert_eq!(clicks.len(), 2, "two beats in that span: {clicks:?}");
    for at in clicks {
        let beat = (at + 128) / BEAT * BEAT;
        assert!(
            (at..at + 128).contains(&beat),
            "a click at {at} is not on a beat"
        );
    }
}

#[test]
fn a_click_does_not_bleed_into_the_next_beat() {
    // It has to be short: a click still ringing when the next one lands is a
    // drone, and a drone is not a time reference.
    let metronome = Arc::new(Metronome::new());
    metronome.set_on(true);
    metronome.set_beat(BEAT as u32, 4);
    let mut node = node(&metronome);

    let all = peaks(&mut node, 0, BEAT as usize * 2, 128);
    let silent = all.iter().filter(|(_, p)| *p <= 0.01).count();
    assert!(
        silent > all.len() * 3 / 4,
        "most of a beat is silence: {silent} of {}",
        all.len()
    );
}

#[test]
fn a_metronome_with_no_tempo_yet_is_silent_rather_than_dividing_by_zero() {
    let metronome = Arc::new(Metronome::new());
    metronome.set_on(true);
    metronome.set_beat(0, 4);
    let mut node = node(&metronome);
    assert!(clicked(&peaks(&mut node, 0, 4096, 128)).is_empty());
}

#[test]
fn the_metronome_adds_into_its_bus_rather_than_replacing_it() {
    // It shares the master pair with the music. A node that overwrote would
    // mute the song on every beat, which is a memorable way to find out.
    let metronome = Arc::new(Metronome::new());
    metronome.set_on(true);
    metronome.set_beat(BEAT as u32, 4);
    let mut node = node(&metronome);

    let mut buffer = vec![0.5f32; 128];
    {
        let mut outputs: Vec<&mut [f32]> = vec![&mut buffer];
        let mut ctx = ProcessContext {
            inputs: &[],
            outputs: &mut outputs,
            all_events: &[],
            live_events: &[],
            node: Default::default(),
            transport: TransportSnapshot {
                state: TransportState::Playing,
                position_sample: 0,
            },
            sample_range: 0..128,
        };
        node.process(&mut ctx);
    }
    assert!(
        buffer.iter().any(|s| *s > 0.5),
        "the click was mixed in, not written over the music"
    );
    // And the bed survived: a click is a sine about zero, so its average
    // contribution over a cycle is nothing and the mean stays where the music
    // put it. A node that overwrote would leave a mean of zero.
    let mean = buffer.iter().sum::<f32>() / buffer.len() as f32;
    assert!(
        (mean - 0.5).abs() < 0.1,
        "the music underneath is gone: mean {mean}"
    );
}

#[test]
fn a_stopped_transport_does_not_click() {
    let metronome = Arc::new(Metronome::new());
    metronome.set_on(true);
    metronome.set_beat(BEAT as u32, 4);
    let mut node = node(&metronome);

    let mut buffer = vec![0.0f32; 128];
    {
        let mut outputs: Vec<&mut [f32]> = vec![&mut buffer];
        let mut ctx = ProcessContext {
            inputs: &[],
            outputs: &mut outputs,
            all_events: &[],
            live_events: &[],
            node: Default::default(),
            transport: TransportSnapshot {
                state: TransportState::Stopped,
                position_sample: 0,
            },
            sample_range: 0..128,
        };
        node.process(&mut ctx);
    }
    assert!(buffer.iter().all(|s| *s == 0.0));
}
