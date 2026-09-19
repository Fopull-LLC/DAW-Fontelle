//! The gestures on a Flopsynth knob, as arithmetic (`docs/flopsynth-next.md`
//! §3.3): a drag at three precisions, a nudge by keys or the wheel, and a
//! typed value read into a number the host can place against the knob's
//! own read-out. Pure, and tested in `tests/flopsynth_gestures.rs`; the
//! window dispatches (plan §13).

use super::flopsynth::{FlopsynthCard, FlopsynthHit, FlopsynthPicture};
use super::instrument::ParamKind;
use crate::layout::Rect;

/// How fine a drag or a nudge is: the plain gesture, Shift, or Ctrl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    Coarse,
    /// Shift: six times slower — the modifier every knob here has had.
    Fine,
    /// Ctrl: twenty times slower — a knob set to the cent.
    Finer,
}

impl Precision {
    /// Ctrl wins over Shift: the finer of the two is what a held pair means.
    pub fn from_modifiers(shift: bool, ctrl: bool) -> Self {
        if ctrl {
            Self::Finer
        } else if shift {
            Self::Fine
        } else {
            Self::Coarse
        }
    }

    fn slower(self) -> f32 {
        match self {
            Self::Coarse => 1.0,
            Self::Fine => 6.0,
            Self::Finer => 20.0,
        }
    }
}

/// The whole travel of a knob, in pixels of drag — `instrument::KNOB_THROW_PX`'s
/// number, kept here beside the precisions that divide it.
const THROW_PX: f32 = 150.0;

/// The value a vertical drag of `dy` pixels from `start` asks for, at
/// `precision`. Up is more, hence the sign.
pub fn knob_drag(start: f32, dy: f32, precision: Precision) -> f32 {
    (start - dy / (THROW_PX * precision.slower())).clamp(0.0, 1.0)
}

/// One nudge — an arrow key, a wheel notch with Ctrl held — as a share of
/// the travel: a hundredth, and a thousandth when the gesture is fine.
pub const NUDGE: f32 = 0.01;
pub const NUDGE_FINE: f32 = 0.001;

pub fn nudged(value: f32, steps: i32, precision: Precision) -> f32 {
    let step = match precision {
        Precision::Coarse => NUDGE,
        Precision::Fine | Precision::Finer => NUDGE_FINE,
    };
    (value + steps as f32 * step).clamp(0.0, 1.0)
}

/// The wheel, with Ctrl held, over a control (the wheel alone only scrolls
/// — that rule stands): a knob nudges a hundredth a notch, a chooser steps
/// an option and stops at its ends, a switch flips.
pub fn wheel_nudge(value: f32, notches: f32, kind: &ParamKind) -> f32 {
    match kind {
        ParamKind::Knob => (value + notches * NUDGE).clamp(0.0, 1.0),
        ParamKind::Switch => {
            if value >= 0.5 {
                0.0
            } else {
                1.0
            }
        }
        ParamKind::Choice(options) => {
            let last = options.len().saturating_sub(1).max(1) as f32;
            let at = (value * last).round();
            ((at + notches.signum()).clamp(0.0, last)) / last
        }
    }
}

/// A typed value, read from what somebody wrote in the field — or from a
/// read-out, which is written the same way. The number, in its unit, with
/// an SI prefix already applied ("2.4k" is 2400, "9.00 kHz" is 9000 Hz).
#[derive(Debug, Clone, PartialEq)]
pub struct Typed {
    pub value: f32,
    /// The unit as written, lowercase, without its prefix: "hz", "ms",
    /// "db", "st", "c", "l", "r"; empty for a bare number; "%" for a
    /// percentage, whose `value` is the share (0.37); "/" for a fraction,
    /// whose `value` is the quotient.
    pub unit: String,
}

impl Typed {
    pub fn new(value: f32, unit: &str) -> Self {
        Self {
            value,
            unit: unit.to_string(),
        }
    }

    pub fn percent(share: f32) -> Self {
        Self::new(share, "%")
    }

    pub fn fraction(quotient: f32) -> Self {
        Self::new(quotient, "/")
    }

    /// Whether `typed` is in this read-out's unit — or names none, in which
    /// case the read-out's is meant.
    pub fn matches_unit(&self, typed: &Typed) -> bool {
        typed.unit.is_empty() || typed.unit == self.unit
    }
}

/// Reads `text` as a [`Typed`] value, or `None` for anything that is not a
/// number: a bare number, one with an SI prefix ("2.4k", "900m"), a
/// percentage, a fraction, or a number with a unit after it with or without
/// a space ("9.00 kHz", "-11.2dB", "55L").
pub fn parse_typed(text: &str) -> Option<Typed> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // A fraction: two whole numbers either side of a slash — and only
    // whole ones, so a division's dotted "1/8." and triplet "1/8T" are
    // their own names and not an eighth.
    if let Some((num, den)) = text.split_once('/') {
        let num: u32 = num.trim().parse().ok()?;
        let den: u32 = den.trim().parse().ok()?;
        if den == 0 {
            return None;
        }
        return Some(Typed::fraction(num as f32 / den as f32));
    }
    // The number is the longest prefix that parses; the rest is the unit.
    let split = text
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || matches!(c, '+' | '-' | '.')))
        .map_or(text.len(), |(at, _)| at);
    let (number, rest) = text.split_at(split);
    let mut value: f32 = number.parse().ok()?;
    let rest = rest.trim().to_lowercase();
    if rest == "%" {
        return Some(Typed::percent(value / 100.0));
    }
    // An SI prefix, when what follows it is a unit or nothing: "k" alone,
    // "kHz", "ms" (which is a unit of its own, not a milli-second-ish "s").
    let unit = match rest.as_str() {
        "k" => {
            value *= 1000.0;
            String::new()
        }
        "m" => {
            value /= 1000.0;
            String::new()
        }
        "ms" | "st" | "s" | "c" | "l" | "r" | "" => rest,
        other if other.starts_with('k') && other.len() > 1 => {
            value *= 1000.0;
            other[1..].to_string()
        }
        other => other.to_string(),
    };
    Some(Typed { value, unit })
}

/// What is true of a knob when its menu is asked for (§3.3).
#[derive(Debug, Clone)]
pub struct FlopKnobMenu<'a> {
    pub name: &'a str,
    pub kind: &'a ParamKind,
    /// Whether the channel came from a preset there is a value to go back
    /// to.
    pub has_preset: bool,
    /// The sources routed to this control, in the matrix's order.
    pub routes: &'a [String],
    /// Whether the matrix can reach this control at all.
    pub is_destination: bool,
    /// Whether a value has been copied from a knob.
    pub clipboard: bool,
}

/// What each row of the knob's menu means, for the window to dispatch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlopKnobMenuItem {
    Heading,
    ResetPreset,
    ResetDefault,
    TypeValue,
    ModulateFrom,
    /// Take route `n` of `FlopKnobMenu::routes` off.
    RemoveRoute(usize),
    AssignMacro,
    CreateAutomation,
    CopyValue,
    PasteValue,
}

/// The knob's right-click menu, row by row: *Reset to preset · Reset to
/// default · Type value…* — *Modulate from… · Remove <source> (one per
/// route) · Assign to macro…* — *Create automation clip · Copy value ·
/// Paste value*. A row that would do nothing is greyed rather than
/// missing: a menu that hides the entry teaches nothing about why it is
/// not there. A chooser can be typed into (an option's name) but has no
/// number to copy; a switch has neither.
pub fn flop_knob_menu(menu: &FlopKnobMenu<'_>) -> Vec<(super::MenuEntry, FlopKnobMenuItem)> {
    use super::MenuEntry;
    use FlopKnobMenuItem as Item;
    let enabled = |label: &str, on: bool| {
        if on {
            MenuEntry::new(label)
        } else {
            MenuEntry::disabled(label)
        }
    };
    let knob = matches!(menu.kind, ParamKind::Knob);
    let typed = !matches!(menu.kind, ParamKind::Switch);
    let mut rows = vec![
        (MenuEntry::disabled(menu.name), Item::Heading),
        (
            enabled("Reset to preset", menu.has_preset),
            Item::ResetPreset,
        ),
        (MenuEntry::new("Reset to default"), Item::ResetDefault),
    ];
    if typed {
        rows.push((MenuEntry::new("Type value\u{2026}"), Item::TypeValue));
    }
    rows.push((
        enabled("Modulate from\u{2026}", menu.is_destination).after_rule(),
        Item::ModulateFrom,
    ));
    for (index, source) in menu.routes.iter().enumerate() {
        rows.push((
            MenuEntry::new(format!("Remove {source}")),
            Item::RemoveRoute(index),
        ));
    }
    rows.push((
        enabled("Assign to macro\u{2026}", menu.is_destination),
        Item::AssignMacro,
    ));
    rows.push((
        MenuEntry::new("Create automation clip").after_rule(),
        Item::CreateAutomation,
    ));
    if knob {
        rows.push((MenuEntry::new("Copy value"), Item::CopyValue));
        rows.push((enabled("Paste value", menu.clipboard), Item::PasteValue));
    }
    rows
}

/// The tip for what the pointer is on (§3.3): one sentence, what it does
/// and how. `None` for a thing with nothing to say — a source card's
/// header is its name.
pub fn flopsynth_tip(hit: FlopsynthHit, cards: &[FlopsynthCard]) -> Option<String> {
    Some(match hit {
        FlopsynthHit::Control { card, param } => {
            let control = cards.get(card)?.group.params.get(param)?;
            match control.kind {
                ParamKind::Knob => "Drag to set \u{b7} Shift or Ctrl for finer \u{b7} \
                                    Alt-click resets \u{b7} double-click to type a value \u{b7} \
                                    right-click for more"
                    .to_string(),
                ParamKind::Choice(_) => {
                    "Click to choose \u{b7} double-click to type a name \u{b7} \
                                         right-click for more"
                        .to_string()
                }
                ParamKind::Switch => "Click to switch \u{b7} right-click for more".to_string(),
            }
        }
        FlopsynthHit::Picture { card } => match cards.get(card)?.picture {
            FlopsynthPicture::Wave { .. } => "Drag sideways to move the position".to_string(),
            FlopsynthPicture::Response { .. } => {
                "Drag to set the cutoff and the resonance".to_string()
            }
            FlopsynthPicture::Envelope(_) => {
                "Drag a corner to shape the envelope \u{b7} drag a stage's middle to bend it"
                    .to_string()
            }
            FlopsynthPicture::Sound { .. } => {
                "Drag sideways to move the start \u{b7} right-click for a sound".to_string()
            }
            FlopsynthPicture::Partials { .. } => "The string's partials".to_string(),
            FlopsynthPicture::Lfo { .. } => "The LFO's cycle \u{b7} DRAW to edit it".to_string(),
            FlopsynthPicture::LfoShape { .. } => {
                "Drag a point \u{b7} drag between points to bend \u{b7} double-click to add \u{b7} \
                 right-click for shapes"
                    .to_string()
            }
            FlopsynthPicture::Curve { .. } => "What the effect does".to_string(),
            FlopsynthPicture::None => return None,
        },
        FlopsynthHit::Header { card } => {
            if cards.get(card)?.removable {
                "Drag to move this effect along the chain".to_string()
            } else {
                return None;
            }
        }
        FlopsynthHit::Remove { .. } => "Take this effect off the chain".to_string(),
        FlopsynthHit::AddEffect => "Add an effect to the end of the chain".to_string(),
        FlopsynthHit::Scale => "Window scale".to_string(),
        FlopsynthHit::InspectorClose => "Close the inspector".to_string(),
    })
}

/// The tip for a cell of the Matrix page's table (§3.4).
pub fn matrix_tip(hit: super::MatrixHit) -> Option<String> {
    use super::MatrixHit;
    Some(match hit {
        MatrixHit::Grip(_) => "Drag to move this route up or down".to_string(),
        MatrixHit::Source(_) => "The source \u{b7} click to choose another".to_string(),
        MatrixHit::Destination(_) => "The destination \u{b7} click to choose another".to_string(),
        MatrixHit::Depth(_) => {
            "Drag to set the depth \u{b7} left of the middle is negative".to_string()
        }
        MatrixHit::Via(_) => "A second source scaling this route's depth".to_string(),
        MatrixHit::Curve(_) => "How the source's value is shaped on the way".to_string(),
        MatrixHit::Invert(_) => "Read 1 \u{2212} source instead of the source".to_string(),
        MatrixHit::Bypass(_) => "Whether this route is heard".to_string(),
        MatrixHit::Remove(_) => "Take this route out of the matrix".to_string(),
        MatrixHit::Add => "Add a route".to_string(),
        MatrixHit::SortSource => "Sort the routes by source".to_string(),
        MatrixHit::SortDestination => "Sort the routes by destination".to_string(),
    })
}

/// How far a bubble stands off its knob.
const BUBBLE_GAP: f32 = 4.0;
const BUBBLE_PAD: f32 = 5.0;

/// Where a knob's hover bubble goes (§3.3): centred over the knob, a gap
/// above it, and never over it — below it at the top of the window, and
/// kept inside the window at its edges.
pub fn hover_bubble_rect(knob: Rect, text: (f32, f32), bounds: Rect) -> Rect {
    let (width, height) = (text.0 + BUBBLE_PAD * 2.0, text.1 + BUBBLE_PAD * 2.0);
    let above = knob.y - BUBBLE_GAP - height;
    let y = if above >= bounds.y {
        above
    } else {
        knob.bottom() + BUBBLE_GAP
    };
    let x = (knob.x + knob.width / 2.0 - width / 2.0)
        .clamp(bounds.x, (bounds.right() - width).max(bounds.x));
    Rect::new(x, y, width, height).intersection(&bounds)
}

/// How far a badge may wander under a pressed button and still be a click
/// (§3.4). Four pixels: a hand that presses is not still, and a badge is
/// small enough that the far side of it is a drag nobody meant.
pub const BADGE_CLICK_SLOP: f32 = 4.0;

/// What a press on a badge turned out to be by the time it was let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeGesture {
    /// Let go where it was pressed: open or close the source in the
    /// inspector.
    Click,
    /// Carried off: a drag to a knob, which the release over the knob
    /// makes a route of.
    Drag,
}

/// A click or a drag, by distance — not by time, because a slow click is
/// still a click.
pub fn badge_gesture(pressed: (f32, f32), released: (f32, f32)) -> BadgeGesture {
    let (dx, dy) = (released.0 - pressed.0, released.1 - pressed.1);
    if dx.abs() <= BADGE_CLICK_SLOP && dy.abs() <= BADGE_CLICK_SLOP {
        BadgeGesture::Click
    } else {
        BadgeGesture::Drag
    }
}

/// The inspector after a badge is clicked: the source clicked, or closed
/// if it was the one already showing.
pub fn inspector_after_click(open: Option<usize>, source: usize) -> Option<usize> {
    if open == Some(source) {
        None
    } else {
        Some(source)
    }
}
