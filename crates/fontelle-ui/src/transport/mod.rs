//! The transport bar's view-model — item 7 of `docs/first-usable-plan.md`.
//!
//! This is the first place the window and the audio thread meet, and the plan
//! puts it before the piano roll deliberately: prove the threading shape on the
//! simplest feature. The shape is TDD §2.2's, and [`TransportHost`] is that
//! sentence written down —
//!
//! - **Commands down.** A click becomes one call on the host, which becomes one
//!   relaxed store into the engine's atomics. Never a lock, never a wait.
//! - **State up.** [`TransportView`] is a snapshot read once per frame. The
//!   window never holds a reference into engine state and never asks it a
//!   second question in the same frame, so what it draws is one consistent
//!   picture rather than several taken microseconds apart.
//!
//! The trait is also what keeps this crate off `fontelle-engine`: the app is
//! the layer allowed to see both sides, so it implements the trait and this
//! side stays testable with a fake.
//!
//! Everything here is a pure function or a small state machine, per §2.5 of the
//! plan. There is no window in this file.

use fontelle_types::PPQN;

use crate::layout::Rect;
use crate::theme::Metrics;

/// What the meter treats as silence, and the bottom of its scale.
///
/// -60 dBFS rather than -90: the bar is about eighty pixels tall, and spending
/// a third of them on levels nobody can hear makes the range that matters
/// unreadably compressed.
pub const METER_FLOOR_DB: f32 = -60.0;

/// How fast a peak meter falls. Roughly PPM ballistics — 20 dB per second is
/// slow enough to read and fast enough to follow a phrase.
pub const RELEASE_DB_PER_SECOND: f32 = 20.0;

/// How long the peak-hold marker stays where it was hit.
pub const HOLD_SECONDS: f32 = 1.5;

/// Everything the window can see of the engine, read once per frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportView {
    /// False when there is no engine behind the window — no audio device, or
    /// no project open yet. The bar is still drawn, quietly, because a window
    /// that changes shape when the sound card goes away is worse than one that
    /// tells you.
    pub available: bool,
    pub playing: bool,
    pub recording: bool,
    /// Where playback actually is, as published by the RT side.
    pub position_sample: i64,
    /// The same position in beats, converted through the song's own
    /// `TempoMap` by whoever implements the host — never by arithmetic on a
    /// BPM here, because a song with a tempo change has no single BPM
    /// (INVARIANT 5).
    pub position_beats: f64,
    pub length_samples: i64,
    pub sample_rate: f64,
    pub looping: bool,
    pub loop_range_samples: (i64, i64),
    /// Peak per channel since the last read, linear.
    pub peaks: [f32; 2],
    /// How hard the master limiter worked, in positive decibels.
    pub reduction_db: f32,
}

impl TransportView {
    /// A window with nothing behind it.
    pub fn unavailable() -> Self {
        Self {
            available: false,
            playing: false,
            recording: false,
            position_sample: 0,
            position_beats: 0.0,
            length_samples: 0,
            sample_rate: 48_000.0,
            looping: false,
            loop_range_samples: (0, 0),
            peaks: [0.0; 2],
            reduction_db: 0.0,
        }
    }

    pub fn position_seconds(&self) -> f64 {
        if self.sample_rate <= 0.0 {
            return 0.0;
        }
        self.position_sample as f64 / self.sample_rate
    }
}

impl Default for TransportView {
    fn default() -> Self {
        Self::unavailable()
    }
}

/// The engine, as far as the window is concerned.
///
/// Implemented by `fontelle-app` over `Arc<Transport>` and `Arc<MasterMeter>`.
/// Every method is expected to be a handful of atomic operations; nothing here
/// may block, because it is all called from inside a frame.
pub trait TransportHost {
    fn view(&mut self) -> TransportView;
    fn play(&mut self);
    fn stop(&mut self);
    fn seek(&mut self, sample: i64);
    fn set_looping(&mut self, on: bool);
}

/// Where the bar's pieces are, left to right.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportBarLayout {
    pub bar: Rect,
    pub play: Rect,
    pub stop: Rect,
    pub loop_toggle: Rect,
    /// The position read-out. A label, not a control.
    pub readout: Rect,
    /// The song, end to end. Clicking it seeks.
    pub ruler: Rect,
    pub meter: Rect,
}

/// Wide enough for `999.4.959  99:59.999` at the chrome's font size.
const READOUT_WIDTH: f32 = 160.0;
const METER_WIDTH: f32 = 96.0;

pub fn transport_bar_layout(bar: Rect, metrics: &Metrics) -> TransportBarLayout {
    let pad = metrics.panel_padding;
    let inner = bar.inset(pad * 0.5);
    // Square buttons the height of the bar's inside, so they stay round-ish
    // whatever the theme says the bar's height is.
    let button = inner.height.max(0.0);

    let mut x = inner.x;
    let mut take = |width: f32| {
        let width = width.min((inner.right() - x).max(0.0));
        let r = Rect::new(x, inner.y, width, inner.height).clamped();
        x += width + pad * 0.5;
        r
    };

    let play = take(button);
    let stop = take(button);
    let loop_toggle = take(button);
    let readout = take(READOUT_WIDTH);

    // The ruler gets what is left after the meter is reserved on the right —
    // the meter's width is fixed because a meter that changes size changes
    // what a given bar height *means*.
    let meter = Rect::new(
        (inner.right() - METER_WIDTH).max(x),
        inner.y,
        METER_WIDTH.min((inner.right() - x).max(0.0)),
        inner.height,
    )
    .clamped();
    let ruler = Rect::new(x, inner.y, (meter.x - pad * 0.5 - x).max(0.0), inner.height).clamped();

    TransportBarLayout {
        bar,
        play,
        stop,
        loop_toggle,
        readout,
        ruler,
        meter,
    }
}

/// What a click on the bar means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportHit {
    Play,
    Stop,
    ToggleLoop,
    /// Seek to this sample.
    Scrub(i64),
}

/// What, if anything, is under `(x, y)`.
///
/// Returns `None` for a dead transport: a bar with no engine behind it is
/// drawn, but pressing play on nothing is a lie.
pub fn hit(
    layout: &TransportBarLayout,
    view: &TransportView,
    x: f32,
    y: f32,
) -> Option<TransportHit> {
    if !view.available {
        return None;
    }
    if layout.play.contains(x, y) {
        return Some(TransportHit::Play);
    }
    if layout.stop.contains(x, y) {
        return Some(TransportHit::Stop);
    }
    if layout.loop_toggle.contains(x, y) {
        return Some(TransportHit::ToggleLoop);
    }
    if layout.ruler.contains(x, y) {
        return Some(TransportHit::Scrub(sample_at(
            layout.ruler,
            x,
            view.length_samples,
        )));
    }
    None
}

/// Turns a hit into commands on the engine.
///
/// The one place a click becomes a write, so "commands down" is a single
/// function rather than a habit. It reads the view first, which is what makes
/// the loop button a *toggle* — asking for the opposite of what was last seen —
/// rather than a button that only ever turns looping on.
pub fn apply(host: &mut dyn TransportHost, hit: TransportHit) {
    let view = host.view();
    match hit {
        // Play on a transport that is already rolling would restart it, and
        // with a button held down it would do so every frame.
        TransportHit::Play if !view.playing => host.play(),
        TransportHit::Play => {}
        TransportHit::Stop => host.stop(),
        TransportHit::ToggleLoop => host.set_looping(!view.looping),
        TransportHit::Scrub(sample) => host.seek(sample),
    }
}

/// Where on the ruler a given sample sits, clamped to the ruler.
///
/// A song that has run past its own end — the release tail is real audio —
/// must not draw its playhead into the meter.
pub fn playhead_x(ruler: Rect, position: i64, length: i64) -> f32 {
    if length <= 0 || ruler.width <= 0.0 {
        return ruler.x;
    }
    let fraction = (position as f64 / length as f64).clamp(0.0, 1.0);
    ruler.x + ruler.width * fraction as f32
}

/// Which sample a click at `x` is asking for, clamped to the song.
pub fn sample_at(ruler: Rect, x: f32, length: i64) -> i64 {
    if length <= 0 || ruler.width <= 0.0 {
        return 0;
    }
    let fraction = ((x - ruler.x) / ruler.width).clamp(0.0, 1.0) as f64;
    (fraction * length as f64).round() as i64
}

/// A peak meter with a hold marker.
///
/// Instant attack, slow release — the standard shape, and the reason it is a
/// state machine rather than a formula: what it reads depends on what it read
/// last. Fed from `MasterMeter::take_peaks`, which is itself a
/// highest-since-last-read, so nothing between two frames is missed even when
/// the frames are far apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meter {
    pub level_db: f32,
    pub hold_db: f32,
    hold_remaining: f32,
}

impl Meter {
    pub fn new() -> Self {
        Self {
            level_db: METER_FLOOR_DB,
            hold_db: METER_FLOOR_DB,
            hold_remaining: 0.0,
        }
    }

    /// Folds in the highest linear peak seen over the last `dt` seconds.
    pub fn update(&mut self, peak_linear: f32, dt: f32) {
        let db = linear_to_db(peak_linear);

        self.level_db = if db >= self.level_db {
            db
        } else {
            (self.level_db - RELEASE_DB_PER_SECOND * dt).max(db)
        }
        .max(METER_FLOOR_DB);

        if db >= self.hold_db {
            self.hold_db = db.max(METER_FLOOR_DB);
            self.hold_remaining = HOLD_SECONDS;
        } else {
            self.hold_remaining -= dt;
            if self.hold_remaining <= 0.0 {
                // Never below the level it is holding for — a hold marker
                // under its own bar is not a marker.
                self.hold_db = (self.hold_db - RELEASE_DB_PER_SECOND * dt).max(self.level_db);
            }
        }
    }
}

impl Default for Meter {
    fn default() -> Self {
        Self::new()
    }
}

/// How much of the meter's box a level fills, 0 to 1.
pub fn meter_fill(db: f32) -> f32 {
    ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0)
}

fn linear_to_db(linear: f32) -> f32 {
    if linear <= 0.0 {
        return METER_FLOOR_DB;
    }
    (20.0 * linear.log10()).max(METER_FLOOR_DB)
}

/// `m:ss.mmm`. Before the start of the song reads as the start of the song.
pub fn format_clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let whole = seconds.floor() as i64;
    let millis = ((seconds - whole as f64) * 1000.0).round() as i64;
    // Rounding 59.9996 up must carry into the seconds, not print `:59.1000`.
    let (whole, millis) = if millis >= 1000 {
        (whole + 1, 0)
    } else {
        (whole, millis)
    };
    format!("{}:{:02}.{:03}", whole / 60, whole % 60, millis)
}

/// The bar's read-out: the position in the document's units, then on the clock.
///
/// Both, because they answer different questions — "where am I in the song"
/// and "how long is this" — and a bar narrow enough that only one fits is a
/// problem for the layout, not a reason to make the user choose.
pub fn format_readout(view: &TransportView, beats_per_bar: u32) -> String {
    format!(
        "{}  {}",
        format_bars_beats(view.position_beats, beats_per_bar),
        format_clock(view.position_seconds())
    )
}

/// `bar.beat.tick`, one-based, with the tick inside the beat at [`PPQN`]
/// resolution — the read-out every DAW has, in the units the document uses.
pub fn format_bars_beats(beats: f64, beats_per_bar: u32) -> String {
    let beats_per_bar = beats_per_bar.max(1) as f64;
    let beats = beats.max(0.0);
    let bar = (beats / beats_per_bar).floor();
    let in_bar = beats - bar * beats_per_bar;
    let beat = in_bar.floor();
    let tick = ((in_bar - beat) * PPQN as f64).round() as i64;
    format!(
        "{}.{}.{:03}",
        bar as i64 + 1,
        beat as i64 + 1,
        tick.min(PPQN - 1)
    )
}
