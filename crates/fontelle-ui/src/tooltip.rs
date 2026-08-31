//! Hover tips: what a control does, said out loud when you wait on it.
//!
//! Reported from using the window: *"when hovering my mouse over an option, it
//! should show me a tiny textbox explaining what that button does if i hold my
//! mouse there long enough, that way if i'm confused what an icon is / does i
//! can just hover my mouse over it for a little bit and read what it does."*
//!
//! This window is nearly all icons — the transport bar, both toolbars, four
//! switches on every rack row — and an icon set is a private language until
//! something translates it. §16.6 gives the drawing rules for chrome and says
//! nothing about this; it is an accessibility floor rather than a style.
//!
//! What is here is the **placement**, which is arithmetic: a box near the
//! pointer that never leaves the window. The **words** are a `tip()` on each
//! hover enum, next to the `label()` and `icon()` those enums already carry,
//! so that adding a control and forgetting to explain it is a hole in one
//! match rather than a lookup table somewhere else that silently misses it.
//!
//! Pure, per §2.5 of `docs/first-usable-plan.md` — see
//! `fontelle-ui/tests/tooltips.rs`.

use std::time::Duration;

use crate::layout::Rect;

/// How long the pointer has to sit still on something before its tip appears.
///
/// A judgement, and the two failure modes bracket it: under about 300 ms a tip
/// flashes on every pointer that merely crosses a toolbar, and over about a
/// second nobody waits long enough to find out the feature exists.
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(550);

/// Air between the words and the edge of the box.
pub const TOOLTIP_PAD: f32 = 5.0;

/// How far the box clears the pointer, above or below.
///
/// Enough to be out from under a normal cursor, which is about sixteen pixels
/// tall on every desktop this runs on.
const CLEARANCE: f32 = 18.0;

/// Where a tip of `text` size goes, with the pointer at `pointer`, inside
/// `bounds`.
///
/// **Below the pointer by preference** — a box drawn over the thing it is
/// describing hides the thing it is describing — and above it when there is no
/// room below. Slid horizontally rather than shrunk when it would run off an
/// edge: a tip cut in half at the right-hand side of the window would be
/// unreadable exactly where the icons are least obvious, since the master
/// strip and the bar's last buttons both live there.
///
/// Empty when the window cannot hold the box at all, which draws as nothing.
pub fn tooltip_layout(text: (f32, f32), pointer: (f32, f32), bounds: Rect) -> Rect {
    let (text_width, text_height) = text;
    let width = text_width + TOOLTIP_PAD * 2.0;
    let height = text_height + TOOLTIP_PAD * 2.0;
    if bounds.is_empty() || width > bounds.width || height > bounds.height {
        return Rect::ZERO;
    }

    let (px, py) = pointer;
    let below = py + CLEARANCE;
    let y = if below + height <= bounds.bottom() {
        below
    } else {
        // Above, clearing the pointer the other way. Clamped into the window
        // afterwards, so a pointer at the very top still gets a tip rather
        // than one drawn off the edge.
        (py - CLEARANCE - height).clamp(bounds.y, (bounds.bottom() - height).max(bounds.y))
    };
    let x = px.clamp(bounds.x, (bounds.right() - width).max(bounds.x));

    Rect::new(x, y, width, height).intersection(&bounds)
}
