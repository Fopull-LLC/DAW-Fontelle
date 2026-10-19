//! The meter: a song's time signature, and the bars where it changes.
//!
//! Ty, 2026-10-18: *"it lets you place a point in your arrangement where the
//! time signature changes ... the project time signature i guess would be
//! considered the default."* The song's own signature holds from bar 1;
//! each change holds from the bar it starts on until the next. Changes sit
//! on bar lines, as in notation and every DAW, which keeps bar numbers
//! meaningful.
//!
//! **A beat is `PPQN × 4 / bottom` ticks** — a quarter in 4, an eighth in 8
//! — whole for the denominators people use (2, 4, 8, 16), so nothing about
//! what a tick means changes. Everything that counts bars asks here, at a
//! tick, instead of reading one number for the whole song.

use std::ops::Range;

use crate::time::{PPQN, Tick};

/// A change of signature, from bar `bar` (counted from 0) on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MeterChange {
    pub bar: u32,
    pub top: u32,
    pub bottom: u32,
}

/// What a line on a time grid is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterLine {
    /// The start of a bar, by its number counted from 0.
    Bar(u32),
    Beat,
}

/// One stretch of one signature: from `bar`, starting at tick `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    bar: u32,
    start: Tick,
    top: u32,
    bottom: u32,
}

impl Span {
    fn beat(&self) -> Tick {
        PPQN * 4 / Tick::from(self.bottom.max(1))
    }

    fn bar_ticks(&self) -> Tick {
        self.beat() * Tick::from(self.top.max(1))
    }
}

/// A song's meter, laid out once: ask it which bar a tick is in, where a bar
/// starts, how many beats it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meter {
    spans: Vec<Span>,
}

/// The denominators a beat is a whole number of ticks for.
fn usable_bottom(bottom: u32) -> u32 {
    match bottom {
        1 | 2 | 4 | 8 | 16 => bottom,
        _ => 4,
    }
}

impl Meter {
    /// The song's own `top`/4 from bar 1, then `changes`.
    pub fn new(top: u32, changes: &[MeterChange]) -> Self {
        let mut changes: Vec<MeterChange> = changes.iter().copied().filter(|c| c.bar > 0).collect();
        // In order; of two on one bar, the last said.
        changes.sort_by_key(|c| c.bar);
        let mut kept: Vec<MeterChange> = Vec::new();
        for c in changes {
            match kept.last_mut() {
                Some(last) if last.bar == c.bar => *last = c,
                _ => kept.push(c),
            }
        }
        let mut spans = vec![Span {
            bar: 0,
            start: 0,
            top: top.max(1),
            bottom: 4,
        }];
        for c in kept {
            let last = *spans.last().expect("there is a first span");
            let start = last.start + Tick::from(c.bar - last.bar) * last.bar_ticks();
            spans.push(Span {
                bar: c.bar,
                start,
                top: c.top.max(1),
                bottom: usable_bottom(c.bottom),
            });
        }
        Self { spans }
    }

    /// One signature for the whole song.
    pub fn constant(top: u32) -> Self {
        Self::new(top, &[])
    }

    fn span_of_bar(&self, bar: u32) -> &Span {
        self.spans
            .iter()
            .rev()
            .find(|s| s.bar <= bar)
            .unwrap_or(&self.spans[0])
    }

    fn span_at(&self, tick: Tick) -> &Span {
        self.spans
            .iter()
            .rev()
            .find(|s| s.start <= tick)
            .unwrap_or(&self.spans[0])
    }

    /// Bar `bar`'s signature.
    pub fn signature(&self, bar: u32) -> (u32, u32) {
        let s = self.span_of_bar(bar);
        (s.top, s.bottom)
    }

    pub fn beats_in_bar(&self, bar: u32) -> u32 {
        self.span_of_bar(bar).top
    }

    /// How long one of bar `bar`'s beats is.
    pub fn beat_ticks(&self, bar: u32) -> Tick {
        self.span_of_bar(bar).beat()
    }

    /// How long bar `bar` is.
    pub fn bar_ticks(&self, bar: u32) -> Tick {
        self.span_of_bar(bar).bar_ticks()
    }

    /// Where bar `bar` starts.
    pub fn tick_of_bar(&self, bar: u32) -> Tick {
        let s = self.span_of_bar(bar);
        s.start + Tick::from(bar - s.bar) * s.bar_ticks()
    }

    /// The bar `tick` is in, and how far into it. Before the song is bar 1's
    /// start.
    pub fn bar_at(&self, tick: Tick) -> (u32, Tick) {
        if tick <= 0 {
            return (0, 0);
        }
        let s = self.span_at(tick);
        let into = tick - s.start;
        let bars = into / s.bar_ticks();
        (s.bar + bars as u32, into - bars * s.bar_ticks())
    }

    /// The bar, the beat in it (from 0) and the ticks into that beat, at
    /// `tick` — what a read-out says.
    pub fn beat_at(&self, tick: Tick) -> (u32, u32, Tick) {
        let (bar, into) = self.bar_at(tick);
        let beat = self.beat_ticks(bar);
        (bar, (into / beat) as u32, into % beat)
    }

    /// The bar line nearest `tick`.
    pub fn nearest_bar(&self, tick: Tick) -> Tick {
        let (bar, into) = self.bar_at(tick);
        let start = self.tick_of_bar(bar);
        if into * 2 >= self.bar_ticks(bar) {
            self.tick_of_bar(bar + 1)
        } else {
            start
        }
    }

    /// Every bar and beat line in `range`, in order.
    pub fn lines(&self, range: Range<Tick>) -> Vec<(Tick, MeterLine)> {
        let mut out = Vec::new();
        if range.end <= range.start {
            return out;
        }
        let (mut bar, _) = self.bar_at(range.start.max(0));
        loop {
            let start = self.tick_of_bar(bar);
            if start >= range.end {
                break;
            }
            let beat = self.beat_ticks(bar);
            if start >= range.start {
                out.push((start, MeterLine::Bar(bar)));
            }
            for b in 1..self.beats_in_bar(bar) {
                let t = start + Tick::from(b) * beat;
                if t >= range.end {
                    break;
                }
                if t >= range.start {
                    out.push((t, MeterLine::Beat));
                }
            }
            bar += 1;
        }
        out
    }

    /// The changes, as a song keeps them (the song's own signature is not
    /// one).
    pub fn changes(&self) -> Vec<MeterChange> {
        self.spans[1..]
            .iter()
            .map(|s| MeterChange {
                bar: s.bar,
                top: s.top,
                bottom: s.bottom,
            })
            .collect()
    }
}
