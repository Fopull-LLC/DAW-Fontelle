//! The song's meter on the audio thread (Ty, 2026-10-19: *"the metronome
//! absolutely needs to match the correct signature you are in"*).
//!
//! The transport tells each block which bar it is in — its signature, where
//! it starts and ends — and which beats fall inside it and how hard. The
//! click plays those; a plugin is told the bar; a count-in counts the bar
//! the playhead stands in. All of it read off the compiled timeline's meter
//! and tempo table, so a signature change or a tempo change lands exactly.

use std::sync::Arc;

use fontelle_engine::{
    AudioNode, BLOCK_SIZE, Metronome, MetronomeNode, PrepareContext, ProcessContext, Transport,
    TransportReader, TransportSnapshot, TransportState,
};
use fontelle_types::{Accent, CompiledTimeline, Meter, MeterChange, PPQN, Signature, TempoSpan};

const SR: f64 = 48_000.0;
/// At 120 bpm and 48 kHz a tick is 25 samples.
const PER_TICK: i64 = 25;

fn tempo(bpm: f32, start: i64, tick: i64) -> TempoSpan {
    TempoSpan {
        start,
        bpm,
        tick,
        ticks_per_sample: f64::from(bpm) / 60.0 * PPQN as f64 / SR,
    }
}

/// A bar of 4/4, then 7/8 felt 2+2+3, at 120.
fn four_then_seven() -> CompiledTimeline {
    CompiledTimeline {
        tempo: vec![tempo(120.0, 0, 0)],
        meter: Meter::with_song(
            &Signature::new(4, 4),
            &[MeterChange::new(1, 7, 8).with_groups(vec![2, 2, 3])],
        ),
        ..Default::default()
    }
}

/// Every click the transport hands out over `frames` from the top, as
/// (sample, accent), and every snapshot.
fn play(timeline: &CompiledTimeline, frames: i64) -> (Vec<(i64, Accent)>, Vec<TransportSnapshot>) {
    let transport = Transport::new();
    transport.set_state(TransportState::Playing);
    let mut reader = TransportReader::new();
    let mut clicks = Vec::new();
    let mut snapshots = Vec::new();
    let mut done = 0;
    while done < frames {
        let step = reader.next_step(&transport, timeline, BLOCK_SIZE, BLOCK_SIZE, false);
        for (offset, accent) in step.snapshot.clicks.iter() {
            assert!((offset as usize) < step.frames, "a click past its block");
            clicks.push((step.range.start + i64::from(offset), accent));
        }
        snapshots.push(step.snapshot);
        done += step.frames as i64;
    }
    // The last block may run past `frames`; what it heard there is not asked.
    clicks.retain(|c| c.0 < frames);
    (clicks, snapshots)
}

#[test]
fn the_beats_are_where_the_meter_puts_them() {
    let timeline = four_then_seven();
    let end = timeline.meter.tick_of_bar(2) * PER_TICK;
    let (clicks, _) = play(&timeline, end);
    let q = PPQN * PER_TICK;
    let at: Vec<i64> = clicks.iter().map(|c| c.0).collect();
    // 4/4: every quarter; then 7/8: every eighth.
    let eighth = q / 2;
    let mut want = vec![0, q, 2 * q, 3 * q];
    want.extend((0..7).map(|i| 4 * q + i * eighth));
    assert_eq!(at, want);
    use Accent::{Bar, Beat, Group};
    let accents: Vec<Accent> = clicks.iter().map(|c| c.1).collect();
    assert_eq!(
        accents,
        vec![
            Bar, Beat, Beat, Beat, Bar, Beat, Group, Beat, Group, Beat, Beat
        ]
    );
}

/// A tempo change moves the clicks with it: the beats are ticks, and the
/// samples come off the tempo table.
#[test]
fn a_tempo_change_moves_the_clicks() {
    let mut timeline = four_then_seven();
    // From bar 1 (tick 3840, sample 96 000) at 60 bpm: a tick is 50 samples.
    timeline.tempo.push(tempo(60.0, 96_000, 4 * PPQN));
    let end = 96_000 + 7 * (PPQN / 2) * 50;
    let (clicks, _) = play(&timeline, end);
    let after: Vec<i64> = clicks
        .iter()
        .map(|c| c.0)
        .filter(|s| *s >= 96_000)
        .collect();
    let want: Vec<i64> = (0..7).map(|i| 96_000 + i * (PPQN / 2) * 50).collect();
    assert_eq!(after, want);
}

#[test]
fn each_block_knows_its_bar() {
    let timeline = four_then_seven();
    let (_, snapshots) = play(&timeline, 5 * PPQN * PER_TICK);
    let first = snapshots[0];
    assert_eq!((first.bar.top, first.bar.bottom, first.bar.bar), (4, 4, 0));
    let later = snapshots
        .iter()
        .find(|s| s.position_tick >= (4 * PPQN + 10) as f64)
        .unwrap();
    assert_eq!((later.bar.top, later.bar.bottom, later.bar.bar), (7, 8, 1));
    assert_eq!(later.bar.bar_start, 4 * PPQN);
    assert_eq!(later.bar.next_bar, 4 * PPQN + 7 * PPQN / 2);
}

/// What a plugin is told: the bar's own signature, where it started in
/// quarter notes, and which bar it is.
#[test]
fn a_plugin_hears_the_bar_it_is_in() {
    let timeline = four_then_seven();
    let (_, snapshots) = play(&timeline, 5 * PPQN * PER_TICK);
    let later = snapshots
        .iter()
        .find(|s| s.position_tick >= (4 * PPQN + 10) as f64)
        .unwrap();
    let t = fontelle_engine::plugin_transport(later, SR);
    assert_eq!((t.numerator, t.denominator), (7, 8));
    assert_eq!(t.bar_number, 1);
    assert!(
        (t.bar_start_beats - 4.0).abs() < 1e-9,
        "{}",
        t.bar_start_beats
    );
}

/// The count-in counts the bar the playhead is in: seven eighths into a
/// 7/8 bar, accented the way the bar is grouped, the last a beat before
/// the playhead.
#[test]
fn a_count_in_counts_the_bar_it_leads_into() {
    let timeline = four_then_seven();
    let marker = 4 * PPQN * PER_TICK;
    let eighth = PPQN / 2 * PER_TICK;
    let transport = Transport::new();
    transport.seek(marker);
    transport.set_count_in(7 * eighth);
    transport.set_state(TransportState::Recording);
    let mut reader = TransportReader::new();
    let mut clicks = Vec::new();
    loop {
        let step = reader.next_step(&transport, &timeline, BLOCK_SIZE, BLOCK_SIZE, false);
        if step.snapshot.state != TransportState::CountingIn {
            break;
        }
        for (offset, accent) in step.snapshot.clicks.iter() {
            clicks.push((step.range.start + i64::from(offset), accent));
        }
    }
    let at: Vec<i64> = clicks.iter().map(|c| marker - c.0).collect();
    assert_eq!(at, (1..=7).rev().map(|k| k * eighth).collect::<Vec<_>>());
    use Accent::{Bar, Beat, Group};
    let accents: Vec<Accent> = clicks.iter().map(|c| c.1).collect();
    assert_eq!(accents, vec![Bar, Beat, Group, Beat, Group, Beat, Beat]);
}

/// The click plays what it is handed, three ways: the bar's first beat
/// loudest, a group's first next, the rest softest.
#[test]
fn the_click_plays_three_strengths() {
    let timeline = four_then_seven();
    let metronome = Arc::new(Metronome::new());
    metronome.set_on(true);
    let mut node = MetronomeNode::new(Arc::clone(&metronome));
    node.prepare(&PrepareContext {
        sample_rate: SR as f32,
        max_block_size: BLOCK_SIZE as u32,
    });
    let transport = Transport::new();
    transport.set_state(TransportState::Playing);
    let mut reader = TransportReader::new();
    let mut peaks: Vec<(i64, f32)> = Vec::new();
    let end = timeline.meter.tick_of_bar(2) * PER_TICK;
    let mut done = 0;
    while done < end {
        let step = reader.next_step(&transport, &timeline, BLOCK_SIZE, BLOCK_SIZE, false);
        let mut buffer = vec![0.0f32; step.frames];
        {
            let mut outputs: Vec<&mut [f32]> = vec![&mut buffer];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut outputs,
                all_events: &[],
                live_events: &[],
                audio: &[],
                node: Default::default(),
                transport: step.snapshot,
                sample_range: step.range.clone(),
            };
            node.process(&mut ctx);
        }
        let peak = buffer.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        peaks.push((step.range.start, peak));
        done += step.frames as i64;
    }
    let loudest_near = |sample: i64| {
        peaks
            .iter()
            .filter(|(at, _)| *at >= sample && *at < sample + 1_000)
            .fold(0.0f32, |a, (_, p)| a.max(*p))
    };
    let q = PPQN * PER_TICK;
    let bar = loudest_near(4 * q);
    let group = loudest_near(4 * q + q);
    let beat = loudest_near(4 * q + q / 2);
    assert!(
        bar > group && group > beat && beat > 0.0,
        "{bar} {group} {beat}"
    );
}

/// A timeline nobody compiled still counts 4/4 at the default tempo.
#[test]
fn an_uncompiled_timeline_clicks_in_four() {
    let (clicks, _) = play(&CompiledTimeline::empty(), 4 * PPQN * PER_TICK);
    assert_eq!(clicks.len(), 4);
}
