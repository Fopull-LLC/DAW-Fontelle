//! Two transient overlays that make an action visible and reversible.
//!
//! - a **toast**: a banner that says what just happened — "No longer searching
//!   …" — and, when the action can be taken back, offers an **Undo** for a few
//!   seconds before it fades. This is the do-then-tell shape: the action
//!   happens at once, and the banner is how you learn it did and how you undo
//!   it, rather than a prompt in the way of every press.
//! - a **confirm**: a small modal for the one kind of press that a click cannot
//!   take back (uninstalling an extension is a re-download). It asks first.
//!
//! Both are pure geometry here — where the banner sits, where its Undo is,
//! where the dialog's two buttons are — so the window only has to draw them and
//! ask "was this point on the Undo".

use crate::layout::Rect;
use crate::theme::Metrics;

/// How long a toast stays up before it fades, in seconds. Long enough to read a
/// line and reach for Undo, short enough not to sit in the way.
pub const TOAST_SECONDS: f32 = 6.0;

/// A toast banner, laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToastLayout {
    /// The whole banner — its background, and "did the click miss it".
    pub frame: Rect,
    /// The Undo button, when the action can be undone.
    pub undo: Option<Rect>,
}

/// Lays a toast out along the bottom of `window`, above where the status line
/// sits. Centred, and no wider than it needs so a short note is not a bar
/// across the whole window.
pub fn toast_layout(window: Rect, metrics: &Metrics, has_undo: bool) -> ToastLayout {
    if window.is_empty() {
        return ToastLayout {
            frame: Rect::ZERO,
            undo: None,
        };
    }
    let row = metrics.row_height.max(1.0);
    let pad = metrics.panel_padding.max(1.0);
    let height = row * 1.7;
    let width = (window.width * 0.6)
        .clamp(0.0, 460.0)
        .min(window.width - pad * 2.0);
    let x = window.x + (window.width - width) / 2.0;
    // A row's worth of air off the very bottom, so it clears the status line.
    let y = (window.bottom() - height - row - pad).max(window.y);
    let frame = Rect::new(x, y, width, height)
        .intersection(&window)
        .clamped();

    let undo = if has_undo && !frame.is_empty() {
        let btn_w = (frame.width * 0.28).clamp(0.0, 96.0);
        let inset = (frame.height * 0.16).min(6.0);
        Some(
            Rect::new(
                frame.right() - btn_w - pad * 0.5,
                frame.y + inset,
                btn_w,
                (frame.height - inset * 2.0).max(0.0),
            )
            .intersection(&frame)
            .clamped(),
        )
    } else {
        None
    };
    ToastLayout { frame, undo }
}

/// A confirmation dialog, laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConfirmLayout {
    /// The whole dialog. A press outside it is a cancel.
    pub frame: Rect,
    /// Where the question is written.
    pub question: Rect,
    /// The "no" button, on the left.
    pub cancel: Rect,
    /// The "yes, do it" button, on the right.
    pub confirm: Rect,
}

/// Lays a confirm dialog in the middle of `window`.
pub fn confirm_layout(window: Rect, metrics: &Metrics) -> ConfirmLayout {
    if window.is_empty() {
        return ConfirmLayout {
            frame: Rect::ZERO,
            question: Rect::ZERO,
            cancel: Rect::ZERO,
            confirm: Rect::ZERO,
        };
    }
    let row = metrics.row_height.max(1.0);
    let pad = metrics.panel_padding.max(1.0);
    // The question takes two lines, then a row of buttons, with air around.
    let width = (window.width * 0.7)
        .clamp(0.0, 380.0)
        .min(window.width - pad * 2.0);
    let height = row * 2.0 + row + pad * 3.0;
    let x = window.x + (window.width - width) / 2.0;
    let y = window.y + (window.height - height) / 2.0;
    let frame = Rect::new(x, y, width, height)
        .intersection(&window)
        .clamped();

    let inner = frame.width - pad * 2.0;
    let question = Rect::new(frame.x + pad, frame.y + pad, inner.max(0.0), row * 2.0)
        .intersection(&frame)
        .clamped();
    let btn_w = ((inner - pad) / 2.0).max(0.0);
    let btn_y = frame.bottom() - pad - row;
    let cancel = Rect::new(frame.x + pad, btn_y, btn_w, row)
        .intersection(&frame)
        .clamped();
    let confirm = Rect::new(frame.right() - pad - btn_w, btn_y, btn_w, row)
        .intersection(&frame)
        .clamped();
    ConfirmLayout {
        frame,
        question,
        cancel,
        confirm,
    }
}
