//! The song's meter as one editor sees it (Ty, 2026-10-19: *"signatures
//! NEED to be flexible enough ... so youre not just locked to x/4"*).
//!
//! The arrangement counts in song ticks; the piano roll in its clip's own,
//! from the clip's start. Both ask this, never a beats-per-bar number: a bar
//! is as long as the signature it is in, a beat is that signature's beat,
//! and every finer grid starts again on each bar line — so a sixteenth grid
//! meets the downbeat of a 7/8 bar that follows a 5/7 one.

use std::ops::Range;

use fontelle_types::{GridStep, Meter, PPQN, Tick};

use super::SnapDivision;

/// The grid a snap division is, or `None` for no snap. Bars and beats are
/// the meter's; the note values (an eighth, a sixteenth, a triplet) are
/// note values, counted from each bar line.
pub fn grid_step(snap: SnapDivision) -> Option<GridStep> {
    match snap {
        SnapDivision::Bar => Some(GridStep::Bar),
        SnapDivision::Beat => Some(GridStep::Beat),
        SnapDivision::Step => Some(GridStep::Ticks(PPQN / 4)),
        SnapDivision::Division(n) => Some(GridStep::Ticks(PPQN / Tick::from(n.max(1)))),
        SnapDivision::Triplet => Some(GridStep::Ticks(PPQN / 3)),
        SnapDivision::None => None,
    }
}

/// The meter, seen from song tick `start`: every tick in and out of this is
/// the editor's own.
#[derive(Debug, Clone, PartialEq)]
pub struct MeterView {
    meter: Meter,
    start: Tick,
}

impl Default for MeterView {
    /// 4/4 from the top: what an editor with no song open counts in.
    fn default() -> Self {
        Self::song(Meter::default())
    }
}

impl MeterView {
    /// The arrangement's: song ticks.
    pub fn song(meter: Meter) -> Self {
        Self { meter, start: 0 }
    }

    /// A clip's, which starts at song tick `start`.
    pub fn clip(meter: Meter, start: Tick) -> Self {
        Self { meter, start }
    }

    pub fn meter(&self) -> &Meter {
        &self.meter
    }

    /// Where the editor's tick 0 is in the song.
    pub fn start(&self) -> Tick {
        self.start
    }

    fn song_tick(&self, local: Tick) -> Tick {
        local + self.start
    }

    /// The line of `snap`'s grid nearest `tick`. Never before the song; a
    /// clip's tick before its own start is before the song's grid only if
    /// the clip starts at the top, and is left alone otherwise.
    pub fn snap(&self, tick: Tick, snap: SnapDivision) -> Tick {
        let Some(step) = grid_step(snap) else {
            return tick;
        };
        let song = self.song_tick(tick);
        if song < 0 {
            return if self.start == 0 { 0 } else { tick };
        }
        self.meter.snap(song, step) - self.start
    }

    /// [`snap`](Self::snap) for something inside a clip that starts at
    /// `start` in this view's ticks — an automation point — in the clip's
    /// own ticks.
    pub fn snap_in(&self, start: Tick, tick: Tick, snap: SnapDivision) -> Tick {
        self.snap(start + tick, snap) - start
    }

    /// The distance from the line of `snap`'s grid at or before `tick` to the
    /// next one: one step there. Zero with no snap.
    pub fn unit_at(&self, tick: Tick, snap: SnapDivision) -> Tick {
        if grid_step(snap).is_none() {
            return 0;
        }
        let from = self.line_at_or_before(tick, snap);
        self.next(from, snap) - from
    }

    /// The next line of `snap`'s grid after `tick` — one step on.
    pub fn next(&self, tick: Tick, snap: SnapDivision) -> Tick {
        match grid_step(snap) {
            Some(step) => self.meter.next_line(self.song_tick(tick), step) - self.start,
            None => tick + 1,
        }
    }

    /// The last line before `tick` — one step back. Never before the song.
    pub fn previous(&self, tick: Tick, snap: SnapDivision) -> Tick {
        match grid_step(snap) {
            Some(step) => self.meter.previous_line(self.song_tick(tick), step) - self.start,
            None => tick - 1,
        }
    }

    /// The line at or before `tick`.
    pub fn line_at_or_before(&self, tick: Tick, snap: SnapDivision) -> Tick {
        match grid_step(snap) {
            Some(step) => self.meter.line_at_or_before(self.song_tick(tick), step) - self.start,
            None => tick,
        }
    }

    /// How long the bar `tick` is in.
    pub fn bar_ticks_at(&self, tick: Tick) -> Tick {
        let (bar, _) = self.meter.bar_at(self.song_tick(tick));
        self.meter.bar_ticks(bar)
    }

    /// How long a beat of the bar `tick` is in, to the nearest tick.
    pub fn beat_ticks_at(&self, tick: Tick) -> Tick {
        let (bar, _) = self.meter.bar_at(self.song_tick(tick));
        self.meter.beat_ticks(bar)
    }

    /// Where the bar `tick` is in starts.
    pub fn bar_start_at(&self, tick: Tick) -> Tick {
        let (bar, _) = self.meter.bar_at(self.song_tick(tick));
        self.meter.tick_of_bar(bar) - self.start
    }

    /// The first bar line at or after `tick`.
    pub fn bar_line_at_or_after(&self, tick: Tick) -> Tick {
        let start = self.bar_start_at(tick);
        if start >= tick {
            start
        } else {
            start + self.bar_ticks_at(tick)
        }
    }

    /// Where `times` copies of the stretch `from..to` start: each as many
    /// whole bars after the last as the stretch spans, the same distance
    /// past its bar line as the original — so a phrase copied into bars of
    /// another length still starts on the same beat of its bar.
    pub fn copy_starts(&self, from: Tick, to: Tick, times: usize) -> Vec<Tick> {
        let song_from = self.song_tick(from).max(0);
        let song_to = self.song_tick(to).max(song_from + 1);
        let (bar, into) = self.meter.bar_at(song_from);
        let mut bars = 1;
        while self.meter.tick_of_bar(bar + bars) + into < song_to {
            bars += 1;
        }
        (1..=times as u32)
            .map(|k| self.meter.tick_of_bar(bar + k * bars) + into - self.start)
            .collect()
    }

    /// The lines of a note-value grid `unit` ticks apart over `range`, each
    /// bar's starting on its own bar line.
    pub fn fine_lines(&self, range: Range<Tick>, unit: Tick) -> Vec<Tick> {
        let mut out = Vec::new();
        if unit <= 0 || range.end <= range.start {
            return out;
        }
        let from = self.song_tick(range.start).max(0);
        let to = self.song_tick(range.end);
        let (mut bar, _) = self.meter.bar_at(from);
        loop {
            let start = self.meter.tick_of_bar(bar);
            if start >= to {
                break;
            }
            let end = self.meter.tick_of_bar(bar + 1);
            let mut t = start;
            while t < end && t < to {
                if t >= from {
                    out.push(t - self.start);
                }
                t += unit;
            }
            bar += 1;
        }
        out
    }
}
