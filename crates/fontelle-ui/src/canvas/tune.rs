//! The pitch corrector's window: a ship's console (`docs/tune-plan.md` §7).
//!
//! Pure geometry and pure answers — no `Scene`, no theme, nothing drawn. What
//! is here is where every piece goes, what is under a point, and the two
//! computations the picture needs: a pitch's place on the trace, and which
//! keys a click on the keyboard asks for.
//!
//! # Why it is its own file rather than a page of Flopsynth's
//!
//! The two windows share their **grid** — `fit_cards` lays these cards out
//! with Flopsynth's cells, its shrink order and its floors, so a knob is the
//! same size in both — and share nothing else. Flopsynth's window is four
//! pages of a synthesiser; this one is a single page with a display across the
//! top of it, and the display is the point: §7.3's trace is what makes the
//! effect legible, and a tab strip over it would be a page you had to leave to
//! see the thing you are adjusting.

use fontelle_types::{TUNE_LOCKED, TUNE_VOICED, TuneFrame};

use crate::layout::Rect;

use super::flopsynth::{CARD_GAP, CardLayout, FlopsynthCard, fit_cards};

/// How tall the viewport is when there is room for it, and the floor it
/// shrinks to before the cards give anything.
///
/// The other way round from Flopsynth's pictures, and deliberately: there a
/// picture is a read-out beside the knobs that made it, and here it is the
/// instrument. A console with no screen is a row of switches.
pub const VIEWPORT_HEIGHT: f32 = 168.0;
pub const VIEWPORT_FLOOR: f32 = 96.0;

/// How tall the keyboard band is, and its own floor.
pub const KEYBOARD_HEIGHT: f32 = 64.0;
pub const KEYBOARD_FLOOR: f32 = 44.0;

/// How many octaves the keyboard shows. Two, so the sung note and the target
/// can be drawn at their real octave and the keyboard doubles as a display.
pub const KEYBOARD_OCTAVES: usize = 2;
/// And how many keys that is.
pub const KEYBOARD_KEYS: usize = KEYBOARD_OCTAVES * 12;

/// Which pitch classes are the black keys, bit 0 = C.
///
/// C#, D#, F#, G#, A# — classes 1, 3, 6, 8 and 10. Written out as bit 11 down
/// to bit 0, which is the order a binary literal reads and the **opposite** of
/// the order the classes are numbered in: the first version of this constant
/// had the pattern right and the direction wrong, and laid out a keyboard with
/// its black keys on D, F, G, A and B. It still had fourteen white keys and
/// ten black ones tiling the band, so every count in
/// `the_keyboard_is_two_octaves_with_the_accidentals_over_the_seams` passed —
/// which is why that test now names the classes.
const ACCIDENTALS: u16 = 0b0000_0101_0100_1010;

/// How tall an accidental is as a share of a natural. The piano roll's
/// proportion, so the two keyboards in this program look like each other.
const ACCIDENTAL_HEIGHT: f32 = 0.64;

/// How many seconds of pitch the viewport draws.
pub const TRACE_SECONDS: f32 = 4.0;

/// How close to a scale note counts as locked, for the reticle's brackets.
pub use fontelle_types::TUNE_LOCK_CENTS;

/// Everything the corrector's window shows.
///
/// The same shape as [`FlopsynthView`](super::FlopsynthView) where the two
/// overlap — cards of normalised values with stable addresses — plus the
/// display. The window knows nothing about a `TuneConfig`; turning an address
/// and a 0..1 into a change to one is `fontelle-app`'s job on the other side
/// of `StudioHost`, exactly as it is for every other panel.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TuneView {
    /// What the header says.
    pub title: String,
    /// The seven cards, in the order §7.2 lays them out. The twelve note
    /// switches are **not** among them: the keyboard is their control.
    pub cards: Vec<FlopsynthCard>,
    /// Which pitch classes are in, bit 0 = C — the scale and the switches
    /// already reconciled by the host.
    pub mask: u16,
    /// The scale's root as a pitch class.
    pub root: u8,
    /// Which classes are held on the channel this insert listens to.
    pub held: u16,
    /// The lowest key the keyboard draws, as a MIDI key.
    pub keyboard_from: u8,
    /// What the corrector is doing right now, newest last. Trimmed by the
    /// host to what the viewport draws.
    pub trace: Vec<TuneFrame>,
    /// The bottom and top of the viewport's own axis, in MIDI cents — the
    /// range's two frequencies, so the rails are where the notes are.
    pub floor_cents: f32,
    pub ceiling_cents: f32,
    /// What the frame's top-right captions say: the latency this costs, which
    /// engine is laying the grains, and which mode it is in.
    pub latency_ms: f32,
    pub engine: String,
    pub mode: String,
    /// What the MIDI card's drop-down offers: "no MIDI" first, then one entry
    /// per channel — and which is chosen.
    pub sources: Vec<String>,
    pub source: usize,
}

/// What the MIDI drop-down's first row says.
pub const NO_MIDI: &str = "no MIDI";

/// The address the MIDI source drop-down carries.
///
/// Every other control on the console is a parameter and hands back a real
/// [`ParamAddress`](fontelle_types::ParamAddress) the host turns into a write
/// to the config. The source is not a parameter: it is a **routing edge**,
/// `EffectSlot.notes`, the same kind of thing a sidechain key is (§2.3), and
/// a config that carried a channel id would be a preset that pointed at
/// whichever channel happened to be third in somebody else's project.
///
/// So it is drawn, measured and hit-tested as a chooser like any other — the
/// whole of the shared half stays shared — and the one place it differs is
/// the write, where the window sends this address to `set_insert_notes`
/// instead of `set_insert_param`. Row 0 is [`NO_MIDI`]; row *n* is the
/// channel at rack place *n − 1*.
pub const TUNE_SOURCE: &str = "tune/source";

/// Where everything ended up.
#[derive(Debug, Clone, PartialEq)]
pub struct TuneLayout {
    pub body: Rect,
    /// The pitch trace, full width across the top.
    pub viewport: Rect,
    /// The two octaves under it.
    pub keyboard: Rect,
    /// The twenty-four key rects, C first.
    pub keys: Vec<Rect>,
    /// The cards, in the order the view lists them.
    pub cards: Vec<CardLayout>,
}

impl Default for TuneLayout {
    /// A window with nothing in it: every rectangle empty, which the renderer
    /// skips and every hit test declines. What the host holds before it has
    /// been given a view.
    fn default() -> Self {
        Self {
            body: Rect::ZERO,
            viewport: Rect::ZERO,
            keyboard: Rect::ZERO,
            keys: Vec::new(),
            cards: Vec::new(),
        }
    }
}

/// What is under a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuneHit {
    /// One control, by its card and its place in that card's group.
    Control { card: usize, param: usize },
    /// A key of the on-screen keyboard, by its pitch class — **not** by its
    /// index, because both octaves show the same mask and clicking either
    /// means the same thing.
    Key { class: u8 },
    /// The trace. Hovering it reads a frame out; nothing else uses it.
    Viewport,
}

/// The two bands and the cards under them.
///
/// **The window does not scroll** and has no pages: a tuner is one page or it
/// is not easy to control (§7.2). When the body is short the viewport gives
/// first, then the keyboard, and the cards last — the opposite of Flopsynth's
/// order, because there the picture is a read-out and here it is the
/// instrument, but the same principle: what is being *used* gives last.
pub fn tune_layout(body: Rect, view: &TuneView) -> TuneLayout {
    if body.is_empty() {
        return TuneLayout {
            body,
            cards: view
                .cards
                .iter()
                .map(|_| CardLayout {
                    frame: Rect::ZERO,
                    header: Rect::ZERO,
                    picture: Rect::ZERO,
                    cells: Vec::new(),
                    remove: Rect::ZERO,
                })
                .collect(),
            ..Default::default()
        };
    }

    // What the cards need, measured by laying them out in a body of unlimited
    // height: the two bands take what is left over, down to their floors.
    let probe = Rect::new(body.x, body.y, body.width, f32::MAX / 4.0);
    let wanted_cards = fit_cards(probe, &view.cards);
    let cards_height = wanted_cards
        .iter()
        .map(|card| card.frame.bottom())
        .fold(body.y, f32::max)
        - body.y;

    let spare = body.height - cards_height - CARD_GAP * 2.0;
    let mut viewport_height = VIEWPORT_HEIGHT;
    let mut keyboard_height = KEYBOARD_HEIGHT;
    let mut over = viewport_height + keyboard_height - spare;
    if over > 0.0 {
        let give = over.min(VIEWPORT_HEIGHT - VIEWPORT_FLOOR);
        viewport_height -= give;
        over -= give;
    }
    if over > 0.0 {
        keyboard_height -= over.min(KEYBOARD_HEIGHT - KEYBOARD_FLOOR);
    }
    // **And the other way.** When the cards leave room over, it goes to the
    // trace rather than to a band of empty ground under them: the viewport is
    // the one thing on this window that is an instrument rather than a
    // control, and it is the thing somebody watches while they sing. The
    // keyboard does not grow with it — two octaves at twice the height is not
    // twice as easy to read — so the picture takes it, up to half again its
    // design height. Past that a held note is a thin line adrift in a lot of
    // empty axis, which reads as a bug rather than as room.
    if over < 0.0 {
        viewport_height = (viewport_height - over).min(VIEWPORT_HEIGHT * 1.6);
    }

    let viewport = Rect::new(body.x, body.y, body.width, viewport_height).intersection(&body);
    let keyboard = Rect::new(
        body.x,
        viewport.bottom() + CARD_GAP,
        body.width,
        keyboard_height,
    )
    .intersection(&body);
    let keys = tune_keyboard_layout(keyboard);

    let below = Rect::new(
        body.x,
        keyboard.bottom() + CARD_GAP,
        body.width,
        (body.bottom() - keyboard.bottom() - CARD_GAP).max(0.0),
    );
    let cards = fit_cards(below, &view.cards);

    TuneLayout {
        body,
        viewport,
        keyboard,
        keys,
        cards,
    }
}

/// The twenty-four key rects, C first, naturals full height and accidentals
/// two thirds.
///
/// Its own function rather than the piano roll's, which is welded to a
/// `RollLayout` and to a scrolling grid. Two octaves in a fixed band is a
/// different picture with the same proportions.
pub fn tune_keyboard_layout(band: Rect) -> Vec<Rect> {
    if band.is_empty() {
        return vec![Rect::ZERO; KEYBOARD_KEYS];
    }
    // The naturals share the width; the accidentals sit on the seams between
    // them, which is what makes it read as a keyboard rather than as a row of
    // twenty-four equal boxes.
    let naturals = KEYBOARD_OCTAVES * 7;
    let width = band.width / naturals as f32;
    let mut natural_x = Vec::with_capacity(naturals);
    let mut rects = vec![Rect::ZERO; KEYBOARD_KEYS];
    let mut index = 0usize;
    for (key, rect) in rects.iter_mut().enumerate() {
        if ACCIDENTALS & (1 << (key % 12)) == 0 {
            natural_x.push(band.x + index as f32 * width);
            *rect = Rect::new(band.x + index as f32 * width, band.y, width, band.height);
            index += 1;
        }
    }
    // Now the accidentals, each straddling the seam to the left of the
    // natural that follows it.
    let mut seen = 0usize;
    for (key, rect) in rects.iter_mut().enumerate() {
        if ACCIDENTALS & (1 << (key % 12)) == 0 {
            seen += 1;
            continue;
        }
        let seam = band.x + seen as f32 * width;
        let w = width * 0.62;
        *rect = Rect::new(seam - w / 2.0, band.y, w, band.height * ACCIDENTAL_HEIGHT)
            .intersection(&band);
    }
    rects
}

/// What is under `(x, y)`.
///
/// The accidentals are tested **first**: they are drawn over the naturals and
/// a hit test in the other order would make the black keys unclickable, which
/// is the piano roll's own rule.
pub fn tune_hit(layout: &TuneLayout, view: &TuneView, x: f32, y: f32) -> Option<TuneHit> {
    for pass in [true, false] {
        for (key, rect) in layout.keys.iter().enumerate() {
            let accidental = ACCIDENTALS & (1 << (key % 12)) != 0;
            if accidental != pass || rect.is_empty() {
                continue;
            }
            if rect.contains(x, y) {
                return Some(TuneHit::Key {
                    class: (key % 12) as u8,
                });
            }
        }
    }
    for (card, placed) in layout.cards.iter().enumerate() {
        for (param, cell) in &placed.cells {
            if cell.contains(x, y) && view.cards.get(card).is_some() {
                return Some(TuneHit::Control {
                    card,
                    param: *param,
                });
            }
        }
    }
    layout.viewport.contains(x, y).then_some(TuneHit::Viewport)
}

/// One point of the trace, in the viewport's own pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TracePoint {
    pub x: f32,
    /// Where the singer was, or `None` on an unvoiced hop — a **gap**, not a
    /// line along the floor.
    pub sung: Option<f32>,
    /// And where the corrector put them.
    pub corrected: Option<f32>,
    pub locked: bool,
    /// Whether the target this hop came from a **held key** rather than from
    /// the scale (§7.3's MIDI bars). The viewport draws a bar in the playhead
    /// ink for the runs where it is true, so what MIDI is doing to the take is
    /// visible in the same picture as what the scale is doing.
    pub from_midi: bool,
}

/// The trace, as points in `area`.
///
/// Pure, and tested: hertz onto the same axis the rails are drawn on, gaps
/// where the tracker was not sure, and **nothing at all** for an empty trace
/// rather than a line along the floor, which is what an empty spectrum's rule
/// says one level over.
pub fn viewport_points(area: Rect, view: &TuneView) -> Vec<TracePoint> {
    if area.is_empty() || view.trace.is_empty() {
        return Vec::new();
    }
    let span = (view.ceiling_cents - view.floor_cents).max(1.0);
    let count = view.trace.len();
    // Newest at the right edge, oldest at the left: the picture scrolls, and
    // what somebody is looking at is what is happening now.
    let step = if count > 1 {
        area.width / (count - 1) as f32
    } else {
        0.0
    };
    let y_of = |cents: f32| {
        let t = ((cents - view.floor_cents) / span).clamp(0.0, 1.0);
        area.bottom() - t * area.height
    };
    view.trace
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            let voiced = frame.flags & TUNE_VOICED != 0;
            TracePoint {
                x: area.x + index as f32 * step,
                sung: voiced.then(|| y_of(frame.sung_cents)),
                corrected: voiced.then(|| y_of(frame.out_cents)),
                locked: frame.flags & TUNE_LOCKED != 0,
                from_midi: frame.flags & fontelle_types::TUNE_FROM_MIDI != 0,
            }
        })
        .collect()
}

/// Where the rails go: one y per enabled pitch class in the visible range.
///
/// Returns `(y, is_root)`, so the root's rail can be drawn brighter — the one
/// line on the picture that says which key the song is in.
pub fn viewport_rails(area: Rect, view: &TuneView) -> Vec<(f32, bool)> {
    if area.is_empty() {
        return Vec::new();
    }
    let span = (view.ceiling_cents - view.floor_cents).max(1.0);
    let first = (view.floor_cents / 100.0).ceil() as i32;
    let last = (view.ceiling_cents / 100.0).floor() as i32;
    (first..=last)
        .filter(|semitone| {
            let class = semitone.rem_euclid(12) as u16;
            view.mask & (1 << class) != 0
        })
        .map(|semitone| {
            let cents = semitone as f32 * 100.0;
            let t = ((cents - view.floor_cents) / span).clamp(0.0, 1.0);
            (
                area.bottom() - t * area.height,
                semitone.rem_euclid(12) as u8 == view.root % 12,
            )
        })
        .collect()
}

/// What clicking a key on the keyboard asks the document for.
///
/// **A whole mask, not a switch.** §4.2: clicking a key copies the mask the
/// chooser currently derives into the twelve switches, flips the one clicked,
/// and sets the scale to Custom — one write and one undo entry, rather than a
/// switch flip that the next redraw would overwrite from the named scale.
pub fn key_click_mask(view: &TuneView, class: u8) -> u16 {
    (view.mask ^ (1 << (class % 12))) & 0x0FFF
}

/// Shift-clicking one: that class alone.
pub fn key_solo_mask(class: u8) -> u16 {
    1 << (class % 12)
}

/// The frame of the trace under `x`, for the viewport's tooltip.
pub fn trace_at(area: Rect, view: &TuneView, x: f32) -> Option<TuneFrame> {
    if area.is_empty() || view.trace.is_empty() {
        return None;
    }
    let t = ((x - area.x) / area.width).clamp(0.0, 1.0);
    let index = ((t * (view.trace.len() - 1) as f32).round() as usize).min(view.trace.len() - 1);
    Some(view.trace[index])
}

/// The newest frame the tap holds, or `None` for an empty trace.
///
/// What the keyboard's target key, its sung dot and the viewport's read-out
/// are all read from — one accessor, so the three cannot disagree about which
/// hop "now" is.
pub fn current_frame(view: &TuneView) -> Option<TuneFrame> {
    view.trace.last().copied()
}

/// The pitch class the correction is pulling towards, if it is tracking.
pub fn target_class(view: &TuneView) -> Option<u8> {
    let frame = current_frame(view)?;
    if frame.flags & fontelle_types::TUNE_VOICED == 0 {
        return None;
    }
    class_of(frame.target_cents)
}

/// And the class the singer is actually on.
pub fn sung_class(view: &TuneView) -> Option<u8> {
    let frame = current_frame(view)?;
    if frame.flags & fontelle_types::TUNE_VOICED == 0 {
        return None;
    }
    class_of(frame.sung_cents)
}

fn class_of(cents: f32) -> Option<u8> {
    let semitone = (cents / 100.0).round();
    if !semitone.is_finite() || !(0.0..=127.0).contains(&semitone) {
        return None;
    }
    Some((semitone as i32).rem_euclid(12) as u8)
}

/// What the reticle's read-out says: how far off the singer is, and what they
/// are being pulled onto — "−23 ¢ → A3".
///
/// `None` when nothing is being tracked, which is when the reticle is not
/// drawn either. Built here rather than in the renderer so the string the
/// window **shapes** and the string it **draws** come from one function; a
/// caption shaped from one recipe and drawn from another is a caption that is
/// blank exactly when it changes.
pub fn tune_readout(view: &TuneView) -> Option<String> {
    let frame = current_frame(view)?;
    if frame.flags & fontelle_types::TUNE_VOICED == 0 {
        return None;
    }
    let off = frame.sung_cents - frame.target_cents;
    let semitone = (frame.target_cents / 100.0).round();
    if !semitone.is_finite() || !(0.0..=127.0).contains(&semitone) {
        return None;
    }
    let semitone = semitone as i32;
    let name = fontelle_types::TUNE_ROOTS[semitone.rem_euclid(12) as usize];
    // MIDI 60 is C4, the convention the roll's own key names follow.
    let octave = semitone / 12 - 1;
    Some(format!("{off:+.0} \u{a2} \u{2192} {name}{octave}"))
}

/// The caption in the frame's top-right: what it costs and what is laying the
/// grains.
pub fn tune_caption(view: &TuneView) -> String {
    format!(
        "{} \u{b7} {} \u{b7} {:.1} ms",
        view.mode, view.engine, view.latency_ms
    )
}

/// Every string the console draws that is not a control's own caption.
///
/// The window shapes from this list and the renderer draws from the same two
/// functions, so a read-out cannot be drawn before it has been shaped.
pub fn tune_strings(view: &TuneView) -> Vec<String> {
    let mut out = vec![tune_caption(view)];
    out.extend(tune_readout(view));
    out
}
