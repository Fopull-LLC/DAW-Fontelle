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

use fontelle_types::{PPQN, Sample};

use crate::layout::Rect;
use crate::theme::Metrics;

/// What the meter treats as silence, and the bottom of its scale.
///
/// -60 dBFS rather than -90: the bar is about eighty pixels tall, and spending
/// a third of them on levels nobody can hear makes the range that matters
/// unreadably compressed.
pub const METER_FLOOR_DB: f32 = -60.0;

/// How fast a peak meter falls.
///
/// It was 20 dB a second — PPM ballistics, three seconds from full scale to
/// the floor — and that read as a fault: *"it plays then leaves it hanging
/// too long when nothings on anymore."* The bar's meter is looked at beside
/// the mixer's master strip, which draws the peak since the last frame and
/// falls at once, so the two have to agree about *when* the song went
/// quiet. Half a second across the whole scale keeps a fall you can see
/// without a tail the strip does not have.
pub const RELEASE_DB_PER_SECOND: f32 = 120.0;

/// How long the peak-hold marker stays where it was hit. Shorter than the
/// fall, for the reason above: a marker still up a second after the last
/// note is the meter "hanging".
pub const HOLD_SECONDS: f32 = 0.6;

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
    /// Armed: the next press of play records rather than plays.
    ///
    /// Separate from [`recording`](TransportView::recording), which is the
    /// transport actually rolling with the tape running. Arming is a decision
    /// you make before you press play, which is what makes it a button of its
    /// own rather than a fourth transport state.
    pub armed: bool,
    /// Whether the click is on.
    pub metronome: bool,
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
            armed: false,
            metronome: false,
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

    /// Arms or disarms. Arming does not start anything: the next press of
    /// play does, and it records.
    fn set_armed(&mut self, on: bool) {
        let _ = on;
    }

    /// Turns the click on or off. A session setting, not the document's — a
    /// project sent to somebody else must not arrive with a woodblock on
    /// every beat.
    fn set_metronome(&mut self, on: bool) {
        let _ = on;
    }

    /// Counts in for `frames` the next time the transport rolls: the playhead
    /// stands on the marker while the click counts, and the song rolls from
    /// there. See `fontelle_engine::Transport::set_count_in`.
    fn count_in(&mut self, frames: i64) {
        let _ = frames;
    }

    /// Whether a count-in is still running.
    fn counting_in(&mut self) -> bool {
        false
    }
}

/// Where the bar's pieces are, left to right.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportBarLayout {
    pub bar: Rect,
    pub play: Rect,
    pub stop: Rect,
    pub loop_toggle: Rect,
    /// Arm. Pressing play while it is lit records.
    pub record: Rect,
    /// The click.
    pub metronome: Rect,
    /// The position read-out. A label, not a control.
    pub readout: Rect,
    /// The tempo, in beats per minute. Dragged, and the only control on this
    /// bar that writes to the **document** rather than to the engine.
    pub tempo: Rect,
    /// The time signature — its numerator, over a quarter note. See
    /// [`format_signature`].
    pub signature: Rect,
    /// Song or clip: what pressing play plays. See
    /// [`PlayMode`](crate::document::PlayMode).
    pub mode: Rect,
    /// The `?`: the keyboard shortcuts page. Beside the boxes it explains
    /// rather than at the far end of the bar, and the first thing given up
    /// when the bar is narrow — F1 opens the same page.
    pub help: Rect,
    /// The song, end to end. Clicking it seeks.
    pub ruler: Rect,
    pub meter: Rect,
}

/// Wide enough for `999.4.959  99:59.999` at the chrome's font size.
const READOUT_WIDTH: f32 = 160.0;
/// Wide enough for `999.99`, framed.
const TEMPO_WIDTH: f32 = 72.0;
/// And for `16/4`.
const SIGNATURE_WIDTH: f32 = 48.0;
/// And for `Song` or `Clip`, framed like the two boxes beside it.
const MODE_WIDTH: f32 = 52.0;
const METER_WIDTH: f32 = 96.0;

/// The narrowest the ruler may be squeezed to before a box is left out
/// instead.
///
/// The ruler is the playhead and the scrub, and it is the one thing on this
/// bar there is no other way to reach. Adding the mode chip ahead of it took
/// it to *nothing* at 640 logical pixels — a width the window opens at — and
/// the playhead then drew at the same pixel wherever the song was, which is a
/// transport bar that has stopped telling you anything. So a box that will
/// not fit is left out rather than squeezed, the same rule the roll's toolbar
/// follows, and the ruler keeps a width you can aim at.
const MIN_RULER_WIDTH: f32 = 80.0;

pub fn transport_bar_layout(bar: Rect, metrics: &Metrics) -> TransportBarLayout {
    let pad = metrics.panel_padding;
    let gap = pad * 0.5;
    let inner = bar.inset(gap);
    // Square buttons the height of the bar's inside, so they stay round-ish
    // whatever the theme says the bar's height is.
    let button = inner.height.max(0.0);

    // The meter's width is fixed and it is reserved first, because a meter
    // that changes size changes what a given bar height *means*.
    let meter = Rect::new(
        (inner.right() - METER_WIDTH).max(inner.x),
        inner.y,
        METER_WIDTH.min(inner.width.max(0.0)),
        inner.height,
    )
    .clamped();

    let mut x = inner.x;
    let take = |x: &mut f32, width: f32| {
        let width = width.min((inner.right() - *x).max(0.0));
        let r = Rect::new(*x, inner.y, width, inner.height).clamped();
        *x += width + gap;
        r
    };

    let play = take(&mut x, button);
    let stop = take(&mut x, button);
    // Arm sits with the transport it belongs to, and the click next to it:
    // the two things you set before you press play, in the order you set them.
    let loop_toggle = take(&mut x, button);
    let record = take(&mut x, button);
    let metronome = take(&mut x, button);

    // The four read-outs. How many of them there is room for, given that the
    // ruler comes first: they are given up from the **right**, so the newest
    // and least essential — the mode chip — goes before the signature, the
    // signature before the tempo, and the position read-out is the last to
    // go, because "where am I in the song" is the other half of what this bar
    // is for. A box that is left out is an empty rectangle, which nothing
    // draws and nothing can be clicked on (`Rect::contains` is false for one).
    // The help button is the fifth, and the first to go: F1 opens the same
    // page, so nothing is lost with it but a hint.
    let boxes: [f32; 5] = [
        READOUT_WIDTH,
        TEMPO_WIDTH,
        SIGNATURE_WIDTH,
        MODE_WIDTH,
        button,
    ];
    let room = (meter.x - gap - x).max(0.0);
    let mut shown = boxes.len();
    while shown > 0 {
        let wanted: f32 = boxes[..shown].iter().map(|w| w + gap).sum();
        if room - wanted >= MIN_RULER_WIDTH {
            break;
        }
        shown -= 1;
    }

    let readout = if shown > 0 {
        take(&mut x, READOUT_WIDTH)
    } else {
        Rect::ZERO
    };
    // Beside the position, because "where am I" and "how fast is it going"
    // are read together. A tempo box at the far end of the bar is one you
    // have to go and find.
    let tempo = if shown > 1 {
        take(&mut x, TEMPO_WIDTH)
    } else {
        Rect::ZERO
    };
    let signature = if shown > 2 {
        take(&mut x, SIGNATURE_WIDTH)
    } else {
        Rect::ZERO
    };
    // The third box that says what a press of play will do: how fast, in what
    // metre, and of what.
    let mode = if shown > 3 {
        take(&mut x, MODE_WIDTH)
    } else {
        Rect::ZERO
    };
    // A square the height of the bar, like the transport buttons, so the
    // glyph in it is the same size as theirs.
    let help = if shown > 4 {
        take(&mut x, button)
    } else {
        Rect::ZERO
    };

    let ruler = Rect::new(x, inner.y, (meter.x - gap - x).max(0.0), inner.height).clamped();

    TransportBarLayout {
        bar,
        play,
        stop,
        loop_toggle,
        record,
        metronome,
        readout,
        tempo,
        signature,
        mode,
        help,
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
    /// Arm, or disarm.
    ToggleRecord,
    /// The click, on or off.
    ToggleMetronome,
    /// Mark this sample — and go there.
    Scrub(Sample),
    /// The tempo box. **Not the engine's business** — see [`action`].
    Tempo,
    /// The time-signature box, likewise.
    Signature,
    /// The song/clip chip. The studio's business, like the two boxes before
    /// it: what the timeline carries is the document host's to decide.
    Mode,
    /// The `?`: open the keyboard shortcuts page. The window's business,
    /// like the boxes — nothing about the engine changes.
    Help,
}

impl TransportHit {
    /// What a hover tip says about this control (see [`crate::tooltip`]).
    ///
    /// `None` for anything that is a *place* rather than a button: a tip
    /// following the pointer along the ruler would be a box in the way of the
    /// thing being scrubbed. The key that does the same is not in the words:
    /// it is [`action`](Self::action)'s, read off the keymap, because the
    /// page can change it.
    pub fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::Play => "Play from the marker",
            Self::Stop => "Stop, and go back to the marker",
            Self::ToggleLoop => "Loop between the markers",
            Self::ToggleRecord => "Arm recording: play, and keep the take",
            Self::ToggleMetronome => "The click, on every beat",
            Self::Tempo => "Tempo \u{2014} drag, or click and type",
            Self::Signature => "Beats in a bar \u{2014} click to choose",
            Self::Mode => "Song plays the arrangement; Clip plays only the clip you are editing",
            Self::Help => "Every keyboard shortcut",
            Self::Scrub(_) => return None,
        })
    }

    /// The keymap action that does what this button does, for the tip.
    pub fn action(self) -> Option<crate::canvas::Action> {
        use crate::canvas::Action;
        Some(match self {
            Self::Play => Action::Play,
            // Space stops too, and it is the key a person reaches for; the
            // square's own job — back to the start — is Home's.
            Self::Stop => Action::Stop,
            Self::ToggleMetronome => Action::Metronome,
            Self::Mode => Action::LegatoOrPlayMode,
            Self::Help => Action::Help,
            _ => return None,
        })
    }
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
    if layout.record.contains(x, y) {
        return Some(TransportHit::ToggleRecord);
    }
    if layout.metronome.contains(x, y) {
        return Some(TransportHit::ToggleMetronome);
    }
    if layout.tempo.contains(x, y) {
        return Some(TransportHit::Tempo);
    }
    if layout.signature.contains(x, y) {
        return Some(TransportHit::Signature);
    }
    if layout.mode.contains(x, y) {
        return Some(TransportHit::Mode);
    }
    if layout.help.contains(x, y) {
        return Some(TransportHit::Help);
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

/// What a press actually asks the engine to do, once the **time marker** is
/// taken into account.
///
/// The marker is the last place the user clicked on either ruler, and it is
/// what makes the transport behave the way people expect from FL Studio: play
/// starts *there*, stopping comes back *there*, and only the stop button goes
/// to the front of the song. A hit is not enough to decide any of that on its
/// own, so the decision is its own value and its own function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportAction {
    /// Roll, from the marker.
    Play,
    /// Stop, and put the playhead back on the marker. What the space bar and
    /// the play button do while it is rolling.
    Pause,
    /// Stop, and put the playhead **and the marker** at the front of the song.
    /// The stop button, and only it.
    Rewind,
    SetLooping(bool),
    /// Arm, or disarm. Nothing starts or stops.
    SetArmed(bool),
    SetMetronome(bool),
    /// Put the marker — and the playhead with it — at this sample.
    Mark(Sample),
}

/// What pressing `hit` means, given what the engine is doing.
///
/// The view is read here rather than inside [`apply`] so the decision is a pure
/// function of a snapshot: what the loop button toggles *from*, and whether the
/// play button is a play or a pause, are both answers about one moment.
///
/// `None` for the tempo and the signature. Every other control on this bar is
/// one atomic store into the transport; those two are `Command`s against the
/// **document**, which has to be undoable and has to be saved, and which this
/// crate cannot reach. Saying so with a `None` is better than inventing a
/// `TransportAction` that [`apply`] would then have to refuse — the window
/// starts a drag against `DocumentHost` instead.
pub fn action(hit: TransportHit, view: &TransportView) -> Option<TransportAction> {
    Some(match hit {
        // The same button both ways round, which is what every transport in
        // every DAW does and what the space bar has always done here.
        TransportHit::Play if view.playing => TransportAction::Pause,
        TransportHit::Play => TransportAction::Play,
        TransportHit::Stop => TransportAction::Rewind,
        TransportHit::ToggleLoop => TransportAction::SetLooping(!view.looping),
        TransportHit::ToggleRecord => TransportAction::SetArmed(!view.armed),
        TransportHit::ToggleMetronome => TransportAction::SetMetronome(!view.metronome),
        TransportHit::Scrub(sample) => TransportAction::Mark(sample),
        TransportHit::Tempo | TransportHit::Signature | TransportHit::Mode | TransportHit::Help => {
            return None;
        }
    })
}

// --------------------------------------------------------------- the tempo ---

/// The slowest a song may be dragged to. Not zero: a tempo of nothing is a
/// song that never plays, and a divide by it is worse.
pub const MIN_TEMPO: f64 = 20.0;

/// And the fastest. Beyond this a sixteenth note is shorter than the audio
/// device's own block.
pub const MAX_TEMPO: f64 = 999.0;

/// How much of a beat per minute one pixel of vertical drag is worth.
///
/// A quarter, so a hundred pixels — about a third of the window's height —
/// walks 120 to 145. Faster than that and the box is unsettable; slower and
/// getting from 90 to 174 is a gesture that runs off the screen.
const TEMPO_PER_PIXEL: f64 = 0.25;

/// How much slower a fine (Shift) drag is. Ten, so the fine drag's step is a
/// fortieth of a BPM and the second decimal place the box shows is reachable.
const TEMPO_FINE: f64 = 10.0;

/// The tempo a vertical drag of `dy` pixels from `start` is asking for.
///
/// Up is more, which is the way every tempo box and every knob works and the
/// opposite of the screen's y axis, hence the sign. Measured from where the
/// gesture *started* rather than from where the pointer is, or the value jumps
/// the moment the box is grabbed.
pub fn tempo_at(start: f64, dy: f32, fine: bool) -> f64 {
    let per_pixel = if fine {
        TEMPO_PER_PIXEL / TEMPO_FINE
    } else {
        TEMPO_PER_PIXEL
    };
    round_tempo(start - dy as f64 * per_pixel)
}

/// The tempo a string typed into the box means, or `None` for one that is
/// not a number.
///
/// > *"right now you can only slide the tempo up and down i cant click and
/// > type in the field like an input field to input my tempo."*
///
/// A click on the box opens it for typing and Enter hands the text here.
/// Whatever is a number is kept inside the same limits a drag has and rounded
/// to the two places the box shows, so what you typed is what it reads;
/// whatever is not — a letter, two dots, nothing — is refused, and the tempo
/// stays where it was rather than becoming somebody's guess at what was meant.
/// `inf` and `nan` parse as floats and are refused too: neither is a tempo.
pub fn parse_tempo(text: &str) -> Option<f64> {
    let bpm: f64 = text.trim().parse().ok()?;
    if !bpm.is_finite() {
        return None;
    }
    Some(round_tempo(bpm))
}

/// The tempo `steps` wheel notches from `current` — a beat per minute each,
/// or a tenth with Shift held.
pub fn nudge_tempo(current: f64, steps: f32, fine: bool) -> f64 {
    let step = if fine { 0.1 } else { 1.0 };
    round_tempo(current + steps as f64 * step)
}

/// Clamps to the settable range and rounds to what [`format_tempo`] can show.
///
/// Both, in one place: a value the box cannot display exactly is a number on
/// screen that is not the number in the document.
pub fn round_tempo(bpm: f64) -> f64 {
    (bpm.clamp(MIN_TEMPO, MAX_TEMPO) * 100.0).round() / 100.0
}

/// What the box says. Two decimal places, because a fine drag can put the
/// value between them.
pub fn format_tempo(bpm: f64) -> String {
    format!("{bpm:.2}")
}

// ------------------------------------------------------- the time signature ---

/// One beat in a bar: a bar line every beat. Odd, and legal.
pub const MIN_BEATS_PER_BAR: u32 = 1;

/// And the most. Past this the bar numbers on the roll's ruler are further
/// apart than the roll is wide at any useful zoom.
pub const MAX_BEATS_PER_BAR: u32 = 16;

/// The signature box's drop-down: every metre the box can hold, in order,
/// with the one in force greyed.
///
/// > *"remember we are trying to move away from iterating through options,
/// > instead it should be a dropdown."*
///
/// A click used to step the box to the next value and wrap at the top, which
/// made 3/4 from 4/4 fifteen clicks away. The list is the same answer the
/// settings rows and the clip editor's choices give: the choice is a row, and
/// the row you are on says where you are.
pub fn signature_menu_entries(current: u32) -> Vec<crate::canvas::MenuEntry> {
    (MIN_BEATS_PER_BAR..=MAX_BEATS_PER_BAR)
        .map(|beats| {
            let label = format_signature(beats);
            if beats == current {
                crate::canvas::MenuEntry::disabled(label)
            } else {
                crate::canvas::MenuEntry::new(label)
            }
        })
        .collect()
}

/// The metre the `row`th entry of [`signature_menu_entries`] stands for.
/// Clamped, so a row past the end is the widest metre rather than a panic.
pub fn beats_per_bar_at(row: usize) -> u32 {
    let offset = u32::try_from(row).unwrap_or(u32::MAX);
    MIN_BEATS_PER_BAR
        .saturating_add(offset)
        .clamp(MIN_BEATS_PER_BAR, MAX_BEATS_PER_BAR)
}

/// What the signature box says.
///
/// The denominator is not settable and the box does not pretend it is:
/// [`PPQN`] is ticks per *quarter* note, so a denominator other than four is a
/// change to what a tick means in every conversion in the project, not a
/// control. It is drawn because `4/4` is how a time signature reads and a bare
/// `4` is not.
pub fn format_signature(beats_per_bar: u32) -> String {
    format!("{beats_per_bar}/4")
}

/// Turns an action into commands on the engine, and hands back where the marker
/// now is.
///
/// The one place a click becomes a write, so "commands down" is a single
/// function rather than a habit. The marker comes in and goes out rather than
/// living here: this crate holds no state the window could disagree with.
pub fn apply(host: &mut dyn TransportHost, action: TransportAction, marker: Sample) -> Sample {
    let marker = marker.max(0);
    match action {
        TransportAction::Play => {
            // Seek first, then roll. The other order plays a few milliseconds
            // of wherever the playhead was left before jumping.
            host.seek(marker);
            host.play();
            marker
        }
        TransportAction::Pause => {
            host.stop();
            host.seek(marker);
            marker
        }
        TransportAction::Rewind => {
            host.stop();
            host.seek(0);
            // The marker comes back too. A stop button that returns the
            // playhead and then plays from bar 5 again is a stop button
            // nobody can use.
            0
        }
        TransportAction::SetLooping(on) => {
            host.set_looping(on);
            marker
        }
        TransportAction::SetArmed(on) => {
            host.set_armed(on);
            marker
        }
        TransportAction::SetMetronome(on) => {
            host.set_metronome(on);
            marker
        }
        TransportAction::Mark(sample) => {
            let sample = sample.max(0);
            host.seek(sample);
            sample
        }
    }
}

/// Starts a take that counts in: arms `frames` of count-in, puts the
/// playhead on the marker, and rolls — and hands the marker back **as it
/// was**.
///
/// > *"just put the playhead on the same spot frozen and count in, then play
/// > it from there instead of trying to do some weird calculations because
/// > thats causing it so when you are recording past the first section that
/// > all of your recordings will be offset by like a bar."*
///
/// The count-in used to be a pre-roll that moved the marker a bar back, and
/// the marker is where play returns to — so every later take counted in from
/// a bar earlier, and landed there. It also clamped at the song's start, so a
/// take from bar one had no count-in at all. The engine counts now, with the
/// playhead standing still, and nothing here moves.
///
/// The count is armed **before** the transport rolls, so the first block the
/// audio thread runs is already a counted one. The metronome switch is left
/// alone: the count clicks whatever it says.
pub fn start_counted_take(host: &mut dyn TransportHost, marker: Sample, frames: Sample) -> Sample {
    let marker = marker.max(0);
    host.count_in(frames.max(0));
    host.seek(marker);
    host.play();
    marker
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
                // Twice as fast as the bar once it lets go: it had its hold,
                // and a marker that then ambles down behind the bar is the
                // "hanging" the release time was shortened to stop. Never
                // below the level it is holding for — a hold marker under
                // its own bar is not a marker.
                self.hold_db = (self.hold_db - 2.0 * RELEASE_DB_PER_SECOND * dt).max(self.level_db);
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

/// The shortest a mouse audition sounds for.
///
/// Lives in [`crate::audition`] now, beside the state machine that enforces
/// it; re-exported here because this is where callers first looked for it.
pub use crate::audition::MIN_AUDITION;

/// When a note that started sounding at `started` and whose key came up at
/// `released` should actually be released, for the floor case.
///
/// The minimum is a **floor, not a length**: holding a key still sounds it for
/// as long as it is held. [`crate::audition::Auditions`] is the general form —
/// it carries a per-note hold, so clicking a half note sounds like a half note
/// rather than like the front 180 ms of one.
pub fn audition_release(
    started: std::time::Instant,
    released: std::time::Instant,
) -> std::time::Instant {
    released.max(started + MIN_AUDITION)
}

// ------------------------------------------------------------- recording ---

/// What the record button records (TDD §14.7, §15.4).
///
/// Reported from using the window: *"when i click record it prompts me what i
/// would like to record: notes, audio from mic, automation, etc."*
///
/// Arming used to mean one thing — keep the notes — because notes were the only
/// thing there was to keep. There are three now, and which one you meant is not
/// something a button can guess, so it asks. The answer is remembered:
/// somebody recording eight vocal takes should answer once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordMode {
    /// What a MIDI keyboard plays, into the clip that is open (TDD §14.7).
    ///
    /// The default, and deliberately: arming has meant this since MIDI
    /// recording landed, and a build that silently started recording a
    /// microphone instead would be a surprise of the worst kind.
    #[default]
    Notes,
    /// What arrives on a record-armed mixer track's input, as a new audio clip
    /// on the arrangement (TDD §15.4).
    Audio,
    /// Every control you move while it rolls, as automation.
    Automation,
}

impl RecordMode {
    pub const ALL: [Self; 3] = [Self::Notes, Self::Audio, Self::Automation];

    pub fn label(self) -> &'static str {
        match self {
            Self::Notes => "Notes",
            Self::Audio => "Audio from an input",
            Self::Automation => "Automation",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Self::Notes => "Keep what the keyboard plays, into the clip that is open",
            Self::Audio => "Keep what arrives on the armed mixer track's input",
            Self::Automation => "Keep every control you move while it rolls",
        }
    }
}

/// The menu the record button opens.
pub fn record_menu_entries() -> Vec<crate::canvas::MenuEntry> {
    RecordMode::ALL
        .iter()
        .map(|mode| crate::canvas::MenuEntry::new(mode.label()))
        .collect()
}

/// What is written in front of the mode that is on.
pub const RECORD_MODE_MARK: &str = "\u{2713} ";

/// The same menu, saying which mode is on — **and still offering it**.
///
/// Reported from using the window: *"i pressed the record button again but i
/// was locked out of the audio option and i couldnt record again."* The menu
/// used to grey out the current mode, which read as "this is the one" to the
/// person who wrote it and as "you cannot have this" to the person recording
/// a second take: after one audio take, *Audio* could never be chosen again,
/// and choosing is how the button arms. So the current mode is marked, not
/// disabled, and choosing it is the ordinary way to record the same thing
/// twice.
pub fn record_menu_entries_for(current: RecordMode) -> Vec<crate::canvas::MenuEntry> {
    RecordMode::ALL
        .iter()
        .map(|mode| {
            if *mode == current {
                crate::canvas::MenuEntry::new(format!("{RECORD_MODE_MARK}{}", mode.label()))
            } else {
                crate::canvas::MenuEntry::new(mode.label())
            }
        })
        .collect()
}

/// How many beats the click counts before the tape starts.
///
/// *"after the 4 tap metronome count in it starts recording"* — four, which is
/// what every studio does. Four **beats of the bar being counted in**, so a
/// count-in in 3/4 is three and one in 6/8 is six: a fixed four over a waltz
/// counts you in wrong.
pub const COUNT_IN_BEATS: u32 = 4;

/// The longest a count-in may be, in seconds.
///
/// A bar at 20 bpm is twelve seconds, and somebody who set a slow tempo to work
/// out a part and then pressed record would think the button was broken.
const LONGEST_COUNT_IN_SECONDS: i64 = 8;

/// How long the count-in is, in samples.
///
/// A count-in is **not** a delay before the transport rolls. The transport
/// rolls, the click sounds, and the tape starts a bar later — which is what
/// makes the first beat of the take land on the first beat of the bar rather
/// than a hand's reaction time after it.
///
/// Zero for a project with no tempo yet, rather than dividing by it. Trimmed to
/// whole beats when a bar this slow would run past
/// [`LONGEST_COUNT_IN_SECONDS`] — a count-in cut off mid-beat is a click that
/// stops in the wrong place, which is worse than a shorter one.
pub fn count_in_samples(samples_per_beat: Sample, beats: u32) -> Sample {
    if samples_per_beat <= 0 || beats == 0 {
        return 0;
    }
    // The cap is in seconds and nobody tells this function the device's rate —
    // but it does not need one to be *safe*: assuming the commonest rate makes
    // the count-in shorter than the cap on a faster device and never longer,
    // which is the direction to be wrong in. A count-in that is too short is a
    // bar you can still hear; one that is too long is a button that looks
    // broken.
    const ASSUMED_RATE: Sample = 48_000;
    let longest = LONGEST_COUNT_IN_SECONDS * ASSUMED_RATE;
    let allowed = (longest / samples_per_beat).clamp(1, i64::from(beats));
    samples_per_beat.saturating_mul(allowed)
}
