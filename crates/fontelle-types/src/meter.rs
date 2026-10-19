//! The meter: a song's time signature, and the bars where it changes.
//!
//! Ty, 2026-10-18: *"it lets you place a point in your arrangement where the
//! time signature changes ... the project time signature i guess would be
//! considered the default."* The song's own signature holds from bar 1;
//! each change holds from the bar it starts on until the next. Changes sit
//! on bar lines, as in notation and every DAW, which keeps bar numbers
//! meaningful.
//!
//! And 2026-10-19: *"signatures NEED to be flexible enough to let you chose
//! whatever top or division number you want so youre not just locked to
//! x/4 ... so that advanced artists could use this as well as beginners."*
//! So any top from 1 to [`MAX_TOP`] over any bottom from 1 to
//! [`MAX_BOTTOM`] — 7/8, 5/16, and the irrational 4/3 alike — and the beats
//! of a bar may be grouped (3+3+2/8), which is what the metronome accents
//! and the grid marks.
//!
//! # Why any bottom works with a tick per 1/960 of a quarter
//!
//! A beat of 1/`bottom` is `4 × PPQN / bottom` ticks: whole for every bottom
//! that divides 3840, which is most of them. For the rest (a 5/7 bar is
//! 2742.857… ticks) **every bar line is rounded from the start of its
//! stretch**, `start + round(n × top × 3840 / bottom)`, rather than by adding
//! rounded bars up — so bar 700 of a 5/7 song is exactly where it should be,
//! and nothing drifts however long the song runs. Beats inside a bar are
//! rounded from its bar line the same way. Everything that counts bars asks
//! here, at a tick, instead of reading one number for the whole song.

use std::ops::Range;

use crate::time::{PPQN, Tick};

/// The most beats a bar may have.
pub const MAX_TOP: u32 = 64;
/// The smallest beat: a 64th.
pub const MAX_BOTTOM: u32 = 64;

/// Ticks in a whole note.
const WHOLE: i64 = 4 * PPQN;

/// A time signature: `top` beats of 1/`bottom` a bar, in `groups`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Signature {
    pub top: u32,
    pub bottom: u32,
    /// How the beats are grouped, in order, adding up to `top` — 3+3+2 for a
    /// bar of eight eighths felt in three. Empty is the signature's own
    /// grouping ([`Signature::groups`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<u32>,
}

impl Signature {
    pub fn new(top: u32, bottom: u32) -> Self {
        Self {
            top,
            bottom,
            groups: Vec::new(),
        }
    }

    /// The same, grouped `groups`. A grouping that is the signature's own
    /// anyway is kept as none, so two ways of saying 7/8 are one signature.
    pub fn with_groups(mut self, groups: Vec<u32>) -> Self {
        self.groups = groups;
        if self.groups == default_groups(self.top, self.bottom) {
            self.groups.clear();
        }
        self
    }

    /// Whether this is a signature the meter can keep, and if not, why — in
    /// words for the person who typed it.
    pub fn check(&self) -> Result<(), String> {
        if !(1..=MAX_TOP).contains(&self.top) {
            return Err(format!("a bar has 1 to {MAX_TOP} beats"));
        }
        if !(1..=MAX_BOTTOM).contains(&self.bottom) {
            return Err(format!("the beat is 1 to 1/{MAX_BOTTOM} of a whole note"));
        }
        if !self.groups.is_empty() {
            if self.groups.contains(&0) {
                return Err("a group has at least one beat".into());
            }
            if self.groups.iter().sum::<u32>() != self.top {
                return Err(format!(
                    "the groups add up to {}, and the bar has {}",
                    self.groups.iter().sum::<u32>(),
                    self.top
                ));
            }
        }
        Ok(())
    }

    /// How the beats are grouped: as said, or the signature's own —
    /// compound bars (6/8, 9/8, 12/8) in threes, odd eighths and sixteenths
    /// in twos ending on a three (7/8 is 2+2+3), anything else a beat at a
    /// time.
    pub fn groups(&self) -> Vec<u32> {
        if self.groups.is_empty() || self.groups.iter().sum::<u32>() != self.top {
            default_groups(self.top, self.bottom)
        } else {
            self.groups.clone()
        }
    }

    /// How it is written: `7/8`, or `3+2+2/8` when the grouping is not the
    /// signature's own.
    pub fn label(&self) -> String {
        let groups = self.groups();
        if groups == default_groups(self.top, self.bottom) {
            format!("{}/{}", self.top, self.bottom)
        } else {
            let parts: Vec<String> = groups.iter().map(u32::to_string).collect();
            format!("{}/{}", parts.join("+"), self.bottom)
        }
    }

    /// Reads `7/8`, `5 / 4` or `3+3+2/8`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (top, bottom) = text
            .split_once('/')
            .ok_or("write it as beats/beat, like 7/8 or 3+3+2/8")?;
        let bottom: u32 = bottom
            .trim()
            .parse()
            .map_err(|_| "the number under the line is the beat: 4, 8, 16…")?;
        let parts: Vec<u32> = top
            .split('+')
            .map(|p| p.trim().parse::<u32>())
            .collect::<Result<_, _>>()
            .map_err(|_| "the number over the line is how many beats, or groups like 3+2")?;
        let signature = if parts.len() > 1 {
            Signature::new(parts.iter().sum(), bottom).with_groups(parts)
        } else {
            Signature::new(parts[0], bottom)
        };
        signature.check()?;
        Ok(signature)
    }
}

/// See [`Signature::groups`].
fn default_groups(top: u32, bottom: u32) -> Vec<u32> {
    let top = top.max(1);
    if bottom >= 8 && top > 3 && top.is_multiple_of(3) {
        vec![3; (top / 3) as usize]
    } else if bottom >= 8 && top >= 5 && !top.is_multiple_of(2) {
        let mut groups = vec![2; ((top - 3) / 2) as usize];
        groups.push(3);
        groups
    } else {
        vec![1; top as usize]
    }
}

/// A change of signature, from bar `bar` (counted from 0) on.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MeterChange {
    pub bar: u32,
    pub top: u32,
    pub bottom: u32,
    /// See [`Signature::groups`]. Written only when there are some, so a
    /// song from before groups saves as it did.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<u32>,
}

impl MeterChange {
    pub fn new(bar: u32, top: u32, bottom: u32) -> Self {
        Self {
            bar,
            top,
            bottom,
            groups: Vec::new(),
        }
    }

    pub fn with_groups(mut self, groups: Vec<u32>) -> Self {
        self.groups = Signature::new(self.top, self.bottom)
            .with_groups(groups)
            .groups;
        self
    }

    /// `signature` from bar `bar`.
    pub fn at(bar: u32, signature: &Signature) -> Self {
        Self {
            bar,
            top: signature.top,
            bottom: signature.bottom,
            groups: signature.groups.clone(),
        }
    }

    pub fn signature(&self) -> Signature {
        Signature {
            top: self.top,
            bottom: self.bottom,
            groups: self.groups.clone(),
        }
    }
}

/// What a line on a time grid is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterLine {
    /// The start of a bar, by its number counted from 0.
    Bar(u32),
    /// The start of a group of beats that is not the bar's first.
    Group,
    Beat,
}

/// How hard a beat is: what the metronome plays it as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accent {
    /// The bar's first beat.
    Bar,
    /// The first beat of a group, in a bar whose beats are grouped.
    Group,
    Beat,
}

/// What a grid is laid in: the bars, the bars' own beats, or a note value
/// (a sixteenth is `Ticks(PPQN / 4)`) counted from each bar line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridStep {
    Bar,
    Beat,
    Ticks(Tick),
}

/// The bar around a tick, as the audio thread is told it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarHere {
    pub bar: u32,
    pub top: u32,
    pub bottom: u32,
    pub bar_start: Tick,
    pub next_bar: Tick,
    /// Where the stretch of this signature starts: what a pattern longer
    /// than a bar counts its bars from.
    pub span_start: Tick,
}

impl Default for BarHere {
    /// The first bar of a song in 4/4 — built without a [`Meter`], which
    /// allocates, so a snapshot made on the audio thread does not.
    fn default() -> Self {
        Self {
            bar: 0,
            top: 4,
            bottom: 4,
            bar_start: 0,
            next_bar: WHOLE,
            span_start: 0,
        }
    }
}

/// One stretch of one signature: from `bar`, starting at tick `start`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Span {
    bar: u32,
    start: Tick,
    signature: Signature,
    /// Where each group starts, in beats from the bar line, the first left
    /// out; empty when the beats are not grouped.
    group_starts: Vec<u32>,
}

impl Span {
    fn new(bar: u32, start: Tick, signature: Signature) -> Self {
        let groups = signature.groups();
        let mut group_starts = Vec::new();
        if groups.iter().any(|g| *g > 1) {
            let mut at = 0;
            for g in &groups[..groups.len() - 1] {
                at += g;
                group_starts.push(at);
            }
        }
        Self {
            bar,
            start,
            signature,
            group_starts,
        }
    }

    fn top(&self) -> i64 {
        i64::from(self.signature.top)
    }

    fn bottom(&self) -> i64 {
        i64::from(self.signature.bottom)
    }

    /// Bar `n` of this stretch (from 0) starts here.
    fn bar_tick(&self, n: i64) -> Tick {
        self.start + round_div(n * self.top() * WHOLE, self.bottom())
    }

    /// Beat `beat` of bar `n` of this stretch.
    fn beat_tick(&self, n: i64, beat: i64) -> Tick {
        let bar = n * self.top() + beat;
        self.start + round_div(bar * WHOLE, self.bottom())
    }

    /// Which bar of this stretch `tick` is in (`tick` at or after `start`).
    fn bar_of(&self, tick: Tick) -> i64 {
        let into = tick - self.start;
        let mut n = into * self.bottom() / (self.top() * WHOLE);
        // The division is the exact bar; rounding the lines may put the
        // line itself a tick either side.
        while n > 0 && self.bar_tick(n) > tick {
            n -= 1;
        }
        while self.bar_tick(n + 1) <= tick {
            n += 1;
        }
        n
    }

    fn accent(&self, beat: u32) -> Accent {
        if beat == 0 {
            Accent::Bar
        } else if self.group_starts.contains(&beat) {
            Accent::Group
        } else {
            Accent::Beat
        }
    }
}

/// `a / b` to the nearest whole number, `b` positive.
fn round_div(a: i64, b: i64) -> i64 {
    (2 * a + b).div_euclid(2 * b)
}

/// A song's meter, laid out once: ask it which bar a tick is in, where a bar
/// starts, how many beats it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meter {
    spans: Vec<Span>,
}

impl Default for Meter {
    fn default() -> Self {
        Self::constant(4)
    }
}

/// A signature the meter can keep: one that is not is put in range rather
/// than refused, since a meter is laid out from whatever a file said.
fn kept(signature: &Signature) -> Signature {
    let mut s = Signature {
        top: signature.top.clamp(1, MAX_TOP),
        bottom: signature.bottom.clamp(1, MAX_BOTTOM),
        groups: signature.groups.clone(),
    };
    if s.check().is_err() {
        s.groups.clear();
    }
    s
}

impl Meter {
    /// The song's own `top`/4 from bar 1, then `changes`.
    pub fn new(top: u32, changes: &[MeterChange]) -> Self {
        Self::with_song(&Signature::new(top, 4), changes)
    }

    /// `song` from bar 1, then `changes`.
    pub fn with_song(song: &Signature, changes: &[MeterChange]) -> Self {
        let mut changes: Vec<MeterChange> = changes.iter().filter(|c| c.bar > 0).cloned().collect();
        // In order; of two on one bar, the last said.
        changes.sort_by_key(|c| c.bar);
        let mut kept_changes: Vec<MeterChange> = Vec::new();
        for c in changes {
            match kept_changes.last_mut() {
                Some(last) if last.bar == c.bar => *last = c,
                _ => kept_changes.push(c),
            }
        }
        let mut spans = vec![Span::new(0, 0, kept(song))];
        for c in kept_changes {
            let last = spans.last().expect("there is a first span");
            let start = last.bar_tick(i64::from(c.bar - last.bar));
            spans.push(Span::new(c.bar, start, kept(&c.signature())));
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

    /// Bar `bar`'s signature, top and bottom.
    pub fn signature(&self, bar: u32) -> (u32, u32) {
        let s = &self.span_of_bar(bar).signature;
        (s.top, s.bottom)
    }

    /// Bar `bar`'s signature, grouping and all.
    pub fn signature_of(&self, bar: u32) -> Signature {
        self.span_of_bar(bar).signature.clone()
    }

    /// The song's own top and bottom, without copying its grouping — for the
    /// audio thread, which may not allocate.
    pub fn song_size(&self) -> (u32, u32) {
        let s = &self.spans[0].signature;
        (s.top, s.bottom)
    }

    /// The song's own signature.
    pub fn song(&self) -> Signature {
        self.spans[0].signature.clone()
    }

    pub fn beats_in_bar(&self, bar: u32) -> u32 {
        self.span_of_bar(bar).signature.top
    }

    /// How long one of bar `bar`'s beats is, to the nearest tick. Where the
    /// beat is not a whole number of ticks, [`beat_tick`](Self::beat_tick)
    /// says where each one really falls.
    pub fn beat_ticks(&self, bar: u32) -> Tick {
        round_div(WHOLE, self.span_of_bar(bar).bottom())
    }

    /// Where beat `beat` (from 0) of bar `bar` falls.
    pub fn beat_tick(&self, bar: u32, beat: u32) -> Tick {
        let s = self.span_of_bar(bar);
        s.beat_tick(i64::from(bar - s.bar), i64::from(beat))
    }

    /// How long bar `bar` is.
    pub fn bar_ticks(&self, bar: u32) -> Tick {
        self.tick_of_bar(bar + 1) - self.tick_of_bar(bar)
    }

    /// Where bar `bar` starts.
    pub fn tick_of_bar(&self, bar: u32) -> Tick {
        let s = self.span_of_bar(bar);
        s.bar_tick(i64::from(bar - s.bar))
    }

    /// The bar `tick` is in, and how far into it. Before the song is bar 1's
    /// start.
    pub fn bar_at(&self, tick: Tick) -> (u32, Tick) {
        if tick <= 0 {
            return (0, 0);
        }
        let s = self.span_at(tick);
        let n = s.bar_of(tick);
        (s.bar + n as u32, tick - s.bar_tick(n))
    }

    /// The bar, the beat in it (from 0) and the ticks into that beat, at
    /// `tick` — what a read-out says.
    pub fn beat_at(&self, tick: Tick) -> (u32, u32, Tick) {
        let (bar, _) = self.bar_at(tick);
        let top = self.beats_in_bar(bar);
        let tick = tick.max(0);
        let mut beat = top - 1;
        while beat > 0 && self.beat_tick(bar, beat) > tick {
            beat -= 1;
        }
        (bar, beat, tick - self.beat_tick(bar, beat))
    }

    /// The bar around `tick`, as the audio thread is told it.
    pub fn here(&self, tick: Tick) -> BarHere {
        let (bar, _) = self.bar_at(tick);
        let span = self.span_of_bar(bar);
        BarHere {
            bar,
            top: span.signature.top,
            bottom: span.signature.bottom,
            bar_start: self.tick_of_bar(bar),
            next_bar: self.tick_of_bar(bar + 1),
            span_start: span.start,
        }
    }

    /// How hard beat `beat` of bar `bar` is.
    pub fn accent(&self, bar: u32, beat: u32) -> Accent {
        self.span_of_bar(bar).accent(beat)
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

    /// The last line of `step`'s grid at or before `tick`. Every grid starts
    /// again on each bar line, so a sixteenth grid in a 5/7 bar and the 7/8
    /// one after it both have a line on the 7/8 bar's downbeat.
    pub fn line_at_or_before(&self, tick: Tick, step: GridStep) -> Tick {
        let tick = tick.max(0);
        let (bar, _) = self.bar_at(tick);
        let start = self.tick_of_bar(bar);
        match step {
            GridStep::Bar => start,
            GridStep::Beat => {
                let mut beat = self.beats_in_bar(bar) - 1;
                while beat > 0 && self.beat_tick(bar, beat) > tick {
                    beat -= 1;
                }
                self.beat_tick(bar, beat)
            }
            GridStep::Ticks(unit) if unit > 0 => start + (tick - start) / unit * unit,
            GridStep::Ticks(_) => tick,
        }
    }

    /// The first line of `step`'s grid after `tick`.
    pub fn next_line(&self, tick: Tick, step: GridStep) -> Tick {
        let tick = tick.max(0);
        let (bar, _) = self.bar_at(tick);
        let next_bar = self.tick_of_bar(bar + 1);
        let next = match step {
            GridStep::Bar => next_bar,
            GridStep::Beat => (0..self.beats_in_bar(bar))
                .map(|b| self.beat_tick(bar, b))
                .find(|t| *t > tick)
                .unwrap_or(next_bar),
            GridStep::Ticks(unit) if unit > 0 => self.line_at_or_before(tick, step) + unit,
            GridStep::Ticks(_) => tick + 1,
        };
        next.min(next_bar)
    }

    /// The last line of `step`'s grid before `tick`; the song's start when
    /// there is none.
    pub fn previous_line(&self, tick: Tick, step: GridStep) -> Tick {
        if tick <= 0 {
            return 0;
        }
        self.line_at_or_before(tick - 1, step)
    }

    /// The line of `step`'s grid nearest `tick` — a magnet, not a ratchet: a
    /// note dropped a hair past a line stays where it looks like it is.
    pub fn snap(&self, tick: Tick, step: GridStep) -> Tick {
        if matches!(step, GridStep::Ticks(unit) if unit <= 0) {
            return tick;
        }
        let before = self.line_at_or_before(tick, step);
        let after = self.next_line(before, step);
        if tick - before >= after - tick {
            after
        } else {
            before
        }
    }

    /// Every beat in `range`, in order, with how hard it is: what the
    /// metronome plays.
    pub fn clicks(&self, range: Range<Tick>) -> Vec<(Tick, Accent)> {
        let mut out = Vec::new();
        self.each_beat(range, |t, accent| out.push((t, accent)));
        out
    }

    /// [`clicks`](Self::clicks) without the list, for the audio thread.
    pub fn each_beat(&self, range: Range<Tick>, mut f: impl FnMut(Tick, Accent)) {
        if range.end <= range.start {
            return;
        }
        let (mut bar, _) = self.bar_at(range.start.max(0));
        loop {
            if self.tick_of_bar(bar) >= range.end {
                break;
            }
            for beat in 0..self.beats_in_bar(bar) {
                let t = self.beat_tick(bar, beat);
                if t >= range.end {
                    return;
                }
                if t >= range.start {
                    f(t, self.accent(bar, beat));
                }
            }
            bar += 1;
        }
    }

    /// Every bar, group and beat line in `range`, in order.
    pub fn lines(&self, range: Range<Tick>) -> Vec<(Tick, MeterLine)> {
        let mut out = Vec::new();
        self.each_beat(range, |t, accent| {
            out.push((
                t,
                match accent {
                    Accent::Bar => MeterLine::Bar(self.bar_at(t).0),
                    Accent::Group => MeterLine::Group,
                    Accent::Beat => MeterLine::Beat,
                },
            ))
        });
        out
    }

    /// The changes, as a song keeps them (the song's own signature is not
    /// one).
    pub fn changes(&self) -> Vec<MeterChange> {
        self.spans[1..]
            .iter()
            .map(|s| MeterChange::at(s.bar, &s.signature))
            .collect()
    }
}
