use std::ops::Range;
use std::sync::atomic::{AtomicI64, AtomicU8, AtomicU64, Ordering};

use fontelle_types::{CompiledTimeline, Sample, Tick, TimedEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransportState {
    Stopped = 0,
    Playing = 1,
    Recording = 2,
    Rendering = 3,
}

impl TransportState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Playing,
            2 => Self::Recording,
            3 => Self::Rendering,
            _ => Self::Stopped,
        }
    }

    /// Whether the graph runs at all. Everything except `Stopped` does: a
    /// recording pass still plays the rest of the arrangement back to the
    /// person performing over it, and an offline render is playback that
    /// isn't in real time.
    pub fn is_processing(self) -> bool {
        !matches!(self, Self::Stopped)
    }
}

/// Read by the RT thread, written by the model thread — an atomic struct, no lock
/// (TDD §6.3). When `Stopped`, the graph is not processed: the callback fills
/// silence and returns immediately, which is what delivers the near-zero idle-CPU
/// target. This must be designed in from the start, not optimised in later.
///
/// **Who writes what matters more than it looks.** The playhead is written by
/// the *RT* side, once per block, because that is the only place that knows
/// where playback actually got to. A seek is therefore not a write to the
/// playhead — it is a **request**, carried by `seek_request` plus a generation
/// counter, which the RT side applies and then publishes the result of. Having
/// both sides store into one position field means the next block silently
/// overwrites the seek before anything acts on it, and the bug shows up as a
/// transport that ignores roughly half the clicks on it.
///
/// The generation counter, rather than a comparison against the current
/// position, is also what makes "seek to where we already are" a real seek:
/// re-cueing to the point you are stopped at is an ordinary thing to ask for.
pub struct Transport {
    state: AtomicU8,
    /// Published by the RT side (see the type docs), read by everyone.
    position_sample: AtomicI64,
    seek_request: AtomicI64,
    seek_generation: AtomicU64,
    loop_start_tick: AtomicI64,
    loop_end_tick: AtomicI64,
    loop_start_sample: AtomicI64,
    loop_end_sample: AtomicI64,
    looping: AtomicU8,
}

impl Transport {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(TransportState::Stopped as u8),
            position_sample: AtomicI64::new(0),
            seek_request: AtomicI64::new(0),
            seek_generation: AtomicU64::new(0),
            loop_start_tick: AtomicI64::new(0),
            loop_end_tick: AtomicI64::new(0),
            loop_start_sample: AtomicI64::new(0),
            loop_end_sample: AtomicI64::new(0),
            looping: AtomicU8::new(0),
        }
    }

    pub fn state(&self) -> TransportState {
        TransportState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn set_state(&self, state: TransportState) {
        self.state.store(state as u8, Ordering::Release);
    }

    pub fn play(&self) {
        self.set_state(TransportState::Playing);
    }

    pub fn stop(&self) {
        self.set_state(TransportState::Stopped);
    }

    pub fn is_playing(&self) -> bool {
        self.state().is_processing()
    }

    /// Where playback actually is, as last published by the RT side. This is
    /// what a playhead should be drawn from.
    pub fn position_sample(&self) -> Sample {
        self.position_sample.load(Ordering::Acquire)
    }

    /// Asks the RT side to move the playhead. Takes effect at the top of the
    /// next block; `position_sample` keeps reporting where playback is until
    /// then. Negative targets land on the start of the song.
    pub fn seek(&self, sample: Sample) {
        self.seek_request.store(sample, Ordering::Relaxed);
        // Release *after* the value, so an RT thread that observes the new
        // generation is guaranteed to see the request that goes with it.
        self.seek_generation.fetch_add(1, Ordering::Release);
    }

    /// RT side: the pending seek, if there is one this caller hasn't applied.
    /// `seen` is the caller's own record of what it has already handled.
    pub fn take_seek(&self, seen: &mut u64) -> Option<Sample> {
        let generation = self.seek_generation.load(Ordering::Acquire);
        if generation == *seen {
            return None;
        }
        *seen = generation;
        Some(self.seek_request.load(Ordering::Relaxed))
    }

    /// RT side: publishes where playback got to.
    pub fn publish_position(&self, sample: Sample) {
        self.position_sample.store(sample, Ordering::Release);
    }

    pub fn loop_range_tick(&self) -> (Tick, Tick) {
        (
            self.loop_start_tick.load(Ordering::Acquire),
            self.loop_end_tick.load(Ordering::Acquire),
        )
    }

    /// The same range the RT side actually loops on.
    pub fn loop_range_sample(&self) -> (Sample, Sample) {
        (
            self.loop_start_sample.load(Ordering::Acquire),
            self.loop_end_sample.load(Ordering::Acquire),
        )
    }

    /// Loop points are ticks (TDD §6.1) and the RT thread needs samples, so
    /// both go in together.
    ///
    /// The resolution happens here, on the model thread, rather than in the
    /// callback: `TempoMap` lives in the document, the model thread may be
    /// editing it, and INVARIANT 5 says the conversion goes through the map
    /// rather than by ad-hoc arithmetic. Publishing both halves in one call is
    /// also what stops them drifting apart after a tempo edit — a loop whose
    /// ticks and samples disagree is a loop that lands somewhere else than the
    /// bar line drawn on screen.
    pub fn set_loop_range(&self, ticks: (Tick, Tick), samples: (Sample, Sample)) {
        self.loop_start_tick.store(ticks.0, Ordering::Release);
        self.loop_end_tick.store(ticks.1, Ordering::Release);
        self.loop_start_sample.store(samples.0, Ordering::Release);
        self.loop_end_sample.store(samples.1, Ordering::Release);
    }

    pub fn is_looping(&self) -> bool {
        self.looping.load(Ordering::Acquire) != 0
    }

    pub fn set_looping(&self, looping: bool) {
        self.looping.store(looping as u8, Ordering::Release);
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

/// A cheap-to-copy read of `Transport`, handed to nodes through `ProcessContext`
/// once per block rather than re-touching the atomics per sample.
#[derive(Debug, Clone, Copy)]
pub struct TransportSnapshot {
    pub state: TransportState,
    pub position_sample: Sample,
}

/// What the caller should do with the next chunk of the buffer it is filling.
/// Returned by [`TransportReader::next_step`].
pub struct Step<'a> {
    /// Frames this step covers. Always at least one when the caller asked for
    /// at least one, so a driving loop always terminates.
    pub frames: usize,
    /// The song-time range to render. Empty when `process` is false.
    pub range: Range<Sample>,
    /// The timeline events falling in `range`. Empty when `process` is false.
    pub events: &'a [TimedEvent],
    pub snapshot: TransportSnapshot,
    /// The graph must be reset *before* this step: a stop, a seek, or a loop
    /// wrap has cut the audio off from what came before it.
    pub reset: bool,
    /// False means fill `frames` frames with silence and run no nodes.
    pub process: bool,
}

/// The RT half of the transport: the piece that turns "what does `Transport`
/// say" into "what do I render this block".
///
/// It exists as a type of its own, rather than as code inside the audio
/// callback, for one reason: the callback can only be driven by a real sound
/// card, and every interesting transport behaviour — stop cutting the tails, a
/// seek rewinding the event cursor, a loop splitting a block at its seam — is
/// then testable only by ear. All of it lives here, and the callback is a loop
/// around `next_step`.
///
/// Everything it does is allocation-free and lock-free, so it runs where it
/// has to (INVARIANT 1).
pub struct TransportReader {
    position: Sample,
    event_cursor: usize,
    seen_seek: u64,
    processing: bool,
}

impl TransportReader {
    pub fn new() -> Self {
        Self {
            position: 0,
            event_cursor: 0,
            seen_seek: 0,
            processing: false,
        }
    }

    /// Where this reader thinks playback is. `Transport::position_sample` is
    /// the published copy of the same thing.
    pub fn position(&self) -> Sample {
        self.position
    }

    /// Decides the next chunk of at most `max_block` frames, out of the
    /// `frames_remaining` the caller still has to fill.
    ///
    /// Called in a loop until the caller's buffer is full — a callback is
    /// usually one step, and becomes two when a loop seam falls inside it.
    ///
    /// `awake` is the caller's answer to "does the graph have to run even
    /// though the transport is stopped" — see [`crate::IdleGate`]. It produces
    /// an *audition* step: the graph runs, but the playhead does not move and
    /// the timeline contributes nothing, because nothing is playing back. That
    /// is what makes a keyboard audible with the transport stopped without
    /// making the song creep forward under it.
    /// Repositions the event cursor into `timeline` for the current playhead.
    ///
    /// Called when a **new** timeline has been published mid-playback: the old
    /// cursor is an index into a `Vec` that no longer exists, and carrying it
    /// across the swap either replays events already played or skips ones that
    /// have not been. A binary search, once, only when something changed —
    /// which is why [`crate::TimelineSource::has_update`] is asked separately.
    pub fn retarget(&mut self, timeline: &CompiledTimeline) {
        self.event_cursor = timeline.cursor_at(self.position);
    }

    pub fn next_step<'t>(
        &mut self,
        transport: &Transport,
        timeline: &'t CompiledTimeline,
        frames_remaining: usize,
        max_block: usize,
        awake: bool,
    ) -> Step<'t> {
        let state = transport.state();
        let mut reset = false;

        // A seek is applied whether or not the transport is running: moving
        // the playhead while stopped is what cueing up a section *is*.
        if let Some(target) = transport.take_seek(&mut self.seen_seek) {
            self.position = target.max(0);
            self.event_cursor = timeline.cursor_at(self.position);
            reset = true;
        }

        let process = state.is_processing();
        // The transition, not the state: resetting the graph on every idle
        // block is exactly the per-block work stopping was meant to avoid.
        if self.processing && !process {
            reset = true;
        }
        self.processing = process;

        if !process {
            transport.publish_position(self.position);
            let snapshot = TransportSnapshot {
                state,
                position_sample: self.position,
            };
            if awake {
                // Audition: run the graph over a block's worth of time without
                // moving the playhead, and hand it no timeline events — the
                // song is stopped, and only what is being played live should
                // sound. The range repeats rather than advances, which is
                // exactly right: nodes take their frame count from its length
                // and none of them cares where a stopped playhead sits.
                let frames = frames_remaining.min(max_block).max(1);
                return Step {
                    frames,
                    range: self.position..self.position + frames as i64,
                    events: &[],
                    snapshot,
                    reset,
                    process: true,
                };
            }
            return Step {
                // Silence is not chunked. A block size is what the graph
                // renders in; zeroing a buffer has no such constraint, and
                // walking a stopped callback in 128-frame steps is per-block
                // work in exactly the state that is supposed to have none
                // (TDD §6.3).
                frames: frames_remaining.max(1),
                range: self.position..self.position,
                events: &[],
                snapshot,
                reset,
                process: false,
            };
        }

        let frames = frames_remaining.min(max_block).max(1);

        // An empty or inverted range is what a half-finished drag produces,
        // and it has no honest interpretation: clamping to it yields
        // zero-frame steps, which spin the audio callback forever.
        let (loop_start, loop_end) = transport.loop_range_sample();
        let looping = transport.is_looping() && loop_end > loop_start;

        if looping && self.position >= loop_end {
            // Lazily, at the top of the step that would have run past the
            // seam, so the wrap and a seek share one code path. A playhead
            // that was outside the loop when looping was switched on is
            // pulled into it here too.
            self.position = loop_start;
            self.event_cursor = timeline.cursor_at(loop_start);
            // A hard cut, like a seek: a note still sounding at the seam has
            // its note-off on the far side of it, so carrying voices across
            // leaves them stuck for as long as the loop runs. Crossfading the
            // seam instead is a real refinement and a later one.
            reset = true;
        }

        let frames = if looping {
            frames.min((loop_end - self.position) as usize)
        } else {
            frames
        };

        let range = self.position..self.position + frames as i64;
        let events = timeline.events_for_block(&mut self.event_cursor, range.clone());
        self.position = range.end;
        transport.publish_position(self.position);

        Step {
            frames,
            snapshot: TransportSnapshot {
                state,
                // The block's *start*: what a node asks "where am I" about is
                // the audio it is being asked to render, not where the
                // playhead will be once it has.
                position_sample: range.start,
            },
            range,
            events,
            reset,
            process: true,
        }
    }
}

impl Default for TransportReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontelle_types::{CompiledTimeline, EventPayload, NodeId, TimedEvent};

    const BLOCK: usize = 128;

    fn note_on(sample: Sample) -> TimedEvent {
        TimedEvent {
            sample,
            target: NodeId::default(),
            payload: EventPayload::NoteOn {
                key: 60,
                velocity: 100,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                voice_context: 0,
            },
        }
    }

    fn timeline(samples: &[Sample]) -> CompiledTimeline {
        CompiledTimeline {
            events: samples.iter().copied().map(note_on).collect(),
            index: Vec::new(),
        }
    }

    fn playing() -> Transport {
        let transport = Transport::new();
        transport.set_state(TransportState::Playing);
        transport
    }

    #[test]
    fn a_new_transport_is_stopped_at_the_start_of_the_song() {
        let transport = Transport::new();
        assert_eq!(transport.state(), TransportState::Stopped);
        assert_eq!(transport.position_sample(), 0);
    }

    #[test]
    fn a_seek_is_a_request_the_rt_side_observes_exactly_once() {
        let transport = Transport::new();
        let mut seen = 0;
        assert_eq!(
            transport.take_seek(&mut seen),
            None,
            "nothing asked for yet"
        );

        transport.seek(48_000);
        assert_eq!(transport.take_seek(&mut seen), Some(48_000));
        assert_eq!(
            transport.take_seek(&mut seen),
            None,
            "a seek already applied must not be applied again every block"
        );
    }

    #[test]
    fn seeking_to_where_the_playhead_already_is_still_seeks() {
        // The generation counter, not the value, is what makes a seek a seek:
        // stopping at bar 5 and pressing "go to bar 5" is a real request to
        // re-cue, and comparing positions would swallow it.
        let transport = Transport::new();
        let mut seen = 0;
        transport.seek(0);
        assert_eq!(transport.take_seek(&mut seen), Some(0));
    }

    #[test]
    fn the_playhead_is_published_by_the_rt_side_and_a_seek_does_not_write_it() {
        // Two writers to one atomic is the bug this shape exists to prevent:
        // the RT thread advances the playhead every block, so if `seek` wrote
        // the same field the next block would overwrite it before anything
        // acted on it.
        let transport = Transport::new();
        transport.publish_position(1_000);
        assert_eq!(transport.position_sample(), 1_000);

        transport.seek(0);
        assert_eq!(
            transport.position_sample(),
            1_000,
            "the playhead still reads where playback actually is until the RT side applies the seek"
        );
    }

    #[test]
    fn a_stopped_transport_produces_silence_and_does_not_advance() {
        let transport = Transport::new();
        let timeline = timeline(&[0, 100]);
        let mut reader = TransportReader::new();

        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert!(
            !step.process,
            "TDD §6.3: stopped means the graph is not processed"
        );
        assert_eq!(
            step.frames, BLOCK,
            "the callback still has a buffer to fill"
        );
        assert!(step.events.is_empty());

        reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert_eq!(
            transport.position_sample(),
            0,
            "a stopped playhead stays where it is"
        );
    }

    #[test]
    fn silence_is_served_whole_rather_than_walked_in_blocks() {
        let transport = Transport::new();
        let timeline = CompiledTimeline::empty();
        let mut reader = TransportReader::new();

        let step = reader.next_step(&transport, &timeline, 4 * BLOCK, BLOCK, false);
        assert_eq!(
            step.frames,
            4 * BLOCK,
            "a stopped callback is one memset, not four trips round the driving loop"
        );
    }

    #[test]
    fn stopping_resets_the_graph_once_and_not_every_block_afterwards() {
        let transport = playing();
        let timeline = timeline(&[0]);
        let mut reader = TransportReader::new();
        reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);

        transport.set_state(TransportState::Stopped);
        let stop = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert!(stop.reset, "voices sounding at the stop must be cut");

        let idle = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert!(
            !idle.reset,
            "resetting every idle block is work the near-zero idle CPU target cannot afford"
        );
    }

    #[test]
    fn playing_walks_the_timeline_in_block_sized_steps() {
        let transport = playing();
        let timeline = timeline(&[0, 200]);
        let mut reader = TransportReader::new();

        let first = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert!(first.process);
        assert_eq!(first.range, 0..BLOCK as Sample);
        assert_eq!(first.events.len(), 1, "the event at 0");
        assert_eq!(transport.position_sample(), BLOCK as Sample);

        let second = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert_eq!(second.range, BLOCK as Sample..2 * BLOCK as Sample);
        assert_eq!(second.events.len(), 1, "the event at 200");
    }

    #[test]
    fn a_short_callback_is_served_in_one_step_not_padded_to_a_full_block() {
        let transport = playing();
        let timeline = CompiledTimeline::empty();
        let mut reader = TransportReader::new();

        let step = reader.next_step(&transport, &timeline, 85, BLOCK, false);
        assert_eq!(step.frames, 85);
        assert_eq!(step.range, 0..85);
    }

    #[test]
    fn a_seek_forwards_skips_the_events_it_jumped_over() {
        let transport = playing();
        let timeline = timeline(&[0, 48_000]);
        let mut reader = TransportReader::new();

        transport.seek(47_990);
        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert!(
            step.reset,
            "the audio now belongs to a different part of the song"
        );
        assert_eq!(step.range.start, 47_990);
        assert_eq!(
            step.events.len(),
            1,
            "the event at 48000 falls in this block; the one at 0 was jumped over"
        );
        assert_eq!(step.events[0].sample, 48_000);
    }

    #[test]
    fn a_seek_backwards_replays_the_events_after_it() {
        // The event cursor only ever advanced. Seeking back to the start
        // would leave it past every event in the piece, so playback from bar 1
        // would be silent — the single most obvious way a first transport
        // implementation goes wrong.
        let transport = playing();
        let timeline = timeline(&[0, 200]);
        let mut reader = TransportReader::new();
        for _ in 0..4 {
            reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        }

        transport.seek(0);
        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert_eq!(step.range.start, 0);
        assert_eq!(step.events.len(), 1, "the event at 0 plays again");
    }

    #[test]
    fn a_seek_is_applied_while_stopped_so_the_playhead_can_be_scrubbed() {
        let transport = Transport::new();
        let timeline = timeline(&[0, 200]);
        let mut reader = TransportReader::new();

        transport.seek(5_000);
        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert!(!step.process);
        assert_eq!(
            transport.position_sample(),
            5_000,
            "a stopped playhead still follows the seek — that is what scrubbing is"
        );

        transport.set_state(TransportState::Playing);
        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert_eq!(
            step.range.start, 5_000,
            "play starts from where it was cued"
        );
    }

    #[test]
    fn a_negative_seek_lands_on_the_start_of_the_song() {
        let transport = playing();
        let timeline = CompiledTimeline::empty();
        let mut reader = TransportReader::new();

        transport.seek(-4_800);
        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert_eq!(step.range.start, 0);
    }

    #[test]
    fn a_loop_never_renders_past_its_end() {
        let transport = playing();
        transport.set_loop_range((0, 960), (0, 1_000));
        transport.set_looping(true);
        let timeline = CompiledTimeline::empty();
        let mut reader = TransportReader::new();

        for _ in 0..40 {
            let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
            assert!(
                step.frames > 0,
                "a zero-frame step would spin the callback forever"
            );
            assert!(
                step.range.end <= 1_000,
                "rendered {:?}, past the loop end",
                step.range
            );
        }
    }

    #[test]
    fn a_loop_splits_the_block_that_crosses_it_and_wraps_to_the_start() {
        let transport = playing();
        transport.set_loop_range((0, 96), (0, 100));
        transport.set_looping(true);
        let timeline = timeline(&[0]);
        let mut reader = TransportReader::new();

        let first = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert_eq!(
            first.frames, 100,
            "clipped to the loop end, not the full block"
        );
        assert_eq!(first.events.len(), 1);

        let wrapped = reader.next_step(&transport, &timeline, BLOCK - 100, BLOCK, false);
        assert_eq!(
            wrapped.range.start, 0,
            "the rest of the callback comes from the loop start"
        );
        assert!(
            wrapped.reset,
            "a note still sounding at the seam has its note-off on the other side of it"
        );
        assert_eq!(
            wrapped.events.len(),
            1,
            "the event at 0 plays on every pass"
        );
    }

    #[test]
    fn playback_starting_before_the_loop_runs_into_it_rather_than_jumping() {
        // A lead-in. Pressing play from a bar before the loop and hearing the
        // bar before the loop is what every DAW does, and it is how you get a
        // running start into the section you are working on.
        let transport = playing();
        transport.set_loop_range((0, 96), (1_000, 2_000));
        transport.set_looping(true);
        let timeline = CompiledTimeline::empty();
        let mut reader = TransportReader::new();

        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert_eq!(step.range, 0..BLOCK as Sample);
        assert!(
            !step.reset,
            "nothing has been cut off — this is ordinary playback"
        );
    }

    #[test]
    fn a_playhead_past_the_loop_end_is_pulled_back_into_it() {
        // Past the end is the case with no honest reading: the loop is on, so
        // that stretch of the song is not going to be reached by playing
        // forwards.
        let transport = playing();
        transport.seek(5_000);
        transport.set_loop_range((0, 96), (1_000, 2_000));
        transport.set_looping(true);
        let timeline = CompiledTimeline::empty();
        let mut reader = TransportReader::new();

        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert_eq!(step.range.start, 1_000);
    }

    #[test]
    fn a_degenerate_loop_range_plays_straight_through() {
        // An empty or inverted range is what a half-finished drag in the UI
        // produces. Honouring it literally means either a zero-frame step
        // (which spins the audio callback forever) or silence with no
        // explanation.
        let transport = playing();
        transport.set_loop_range((960, 960), (1_000, 1_000));
        transport.set_looping(true);
        let timeline = CompiledTimeline::empty();
        let mut reader = TransportReader::new();

        let first = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert_eq!(first.range, 0..BLOCK as Sample);
        let second = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert_eq!(
            second.range.start, BLOCK as Sample,
            "not stuck at the loop point"
        );
    }

    #[test]
    fn loop_points_are_ticks_and_samples_together() {
        // TDD §6.1: musical time is ticks, audio time is samples, and they are
        // never conflated. The RT thread cannot run a `TempoMap` lookup
        // against a map the model thread may be editing, so the resolution
        // happens off-thread and both forms are published in one call — which
        // is also what stops them drifting apart.
        let transport = Transport::new();
        transport.set_loop_range((960, 3_840), (24_000, 96_000));
        assert_eq!(transport.loop_range_tick(), (960, 3_840));
        assert_eq!(transport.loop_range_sample(), (24_000, 96_000));
    }

    #[test]
    fn recording_and_rendering_process_the_graph_and_only_stopped_does_not() {
        assert!(TransportState::Playing.is_processing());
        assert!(TransportState::Recording.is_processing());
        assert!(TransportState::Rendering.is_processing());
        assert!(!TransportState::Stopped.is_processing());
    }
}

#[cfg(test)]
mod audition_tests {
    use super::*;
    use fontelle_types::{CompiledTimeline, EventPayload, NodeId, TimedEvent};

    const BLOCK: usize = 128;

    fn timeline_with_a_note_at(sample: Sample) -> CompiledTimeline {
        CompiledTimeline {
            events: vec![TimedEvent {
                sample,
                target: NodeId::default(),
                payload: EventPayload::NoteOn {
                    key: 60,
                    velocity: 100,
                    pan: 0,
                    fine_pitch: 0,
                    release: 0,
                    mod_x: 0,
                    mod_y: 0,
                    voice_context: 0,
                },
            }],
            index: Vec::new(),
        }
    }

    #[test]
    fn an_awake_stopped_transport_runs_the_graph_over_a_real_block() {
        let transport = Transport::new();
        let timeline = CompiledTimeline::empty();
        let mut reader = TransportReader::new();

        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, true);
        assert!(step.process, "somebody is playing; the graph has to run");
        assert_eq!(
            step.range.end - step.range.start,
            BLOCK as i64,
            "a block's worth of time, so nodes render a block's worth of audio"
        );
    }

    #[test]
    fn auditioning_does_not_move_the_playhead() {
        // Holding a chord down with the transport stopped must not walk the
        // song forward underneath it.
        let transport = Transport::new();
        transport.seek(50_000);
        let timeline = CompiledTimeline::empty();
        let mut reader = TransportReader::new();

        for _ in 0..10 {
            reader.next_step(&transport, &timeline, BLOCK, BLOCK, true);
        }
        assert_eq!(transport.position_sample(), 50_000);
    }

    #[test]
    fn auditioning_plays_nothing_from_the_timeline() {
        // The transport is stopped, so the song is not playing — only what is
        // being played live. A stopped transport that dribbled out the
        // arrangement's notes would be the worst of both states.
        let transport = Transport::new();
        let timeline = timeline_with_a_note_at(0);
        let mut reader = TransportReader::new();

        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, true);
        assert!(step.process);
        assert!(step.events.is_empty());
    }

    #[test]
    fn auditioning_does_not_consume_the_events_playback_will_need() {
        // The audition path must leave the event cursor alone: pressing play
        // after noodling has to start the song from the top, not from
        // wherever the cursor was dragged to.
        let transport = Transport::new();
        let timeline = timeline_with_a_note_at(0);
        let mut reader = TransportReader::new();

        for _ in 0..4 {
            reader.next_step(&transport, &timeline, BLOCK, BLOCK, true);
        }

        transport.play();
        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert_eq!(step.events.len(), 1, "the note at 0 is still there to play");
    }

    #[test]
    fn a_stopped_transport_with_nothing_playing_still_runs_nothing() {
        let transport = Transport::new();
        let timeline = CompiledTimeline::empty();
        let mut reader = TransportReader::new();

        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, false);
        assert!(!step.process, "TDD §6.3's idle path is still the default");
    }

    #[test]
    fn being_awake_makes_no_difference_once_the_transport_is_rolling() {
        let transport = Transport::new();
        transport.play();
        let timeline = CompiledTimeline::empty();
        let mut reader = TransportReader::new();

        let step = reader.next_step(&transport, &timeline, BLOCK, BLOCK, true);
        assert_eq!(step.range, 0..BLOCK as Sample, "ordinary playback");
    }
}
