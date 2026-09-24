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
//! - a **save prompt**: the confirm's sibling with three answers, for leaving
//!   a project that has changes not on disk.
//! - **"Saved!"**, which rises out of the top of the window and fades, and the
//!   **job card**, a progress bar for a bounce running beside the window.
//!
//! All are pure geometry here — where the banner sits, where its Undo is,
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

/// How long "Saved!" takes to rise out of sight, in seconds.
pub const SAVED_FLASH_SECONDS: f32 = 1.2;

/// The words that rise when a save lands.
pub const SAVED_FLASH_TEXT: &str = "Saved!";

/// Where "Saved!" is drawn at one moment of its rise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedFlash {
    pub center_x: f32,
    pub center_y: f32,
    /// 1 when it appears, 0 when it is gone.
    pub alpha: f32,
}

/// "Saved!" `elapsed` seconds after the save, or `None` once it has gone.
///
/// > *"a Saved! text that appears in the top center and moves upwards as it
/// > fades out. this makes it obvious when your save works."*
///
/// It starts a few rows down from the top edge — over the arrangement, not on
/// the window's border — and climbs two rows, easing out so the start of the
/// move is the quick part and the eye is drawn to it. The fade is eased the
/// other way, so the word is readable for most of its life and goes at the
/// end.
pub fn saved_flash(window: Rect, metrics: &Metrics, elapsed: f32) -> Option<SavedFlash> {
    if !(0.0..=SAVED_FLASH_SECONDS).contains(&elapsed) || window.is_empty() {
        return None;
    }
    let row = metrics.row_height.max(1.0);
    let t = elapsed / SAVED_FLASH_SECONDS;
    let rise = 1.0 - (1.0 - t) * (1.0 - t);
    let start = window.y + row * 3.5;
    Some(SavedFlash {
        center_x: window.x + window.width / 2.0,
        center_y: (start - row * 2.0 * rise).max(window.y),
        alpha: 1.0 - t * t,
    })
}

/// The project's name as a title shows it: with a `*` after it while there
/// are changes that are not on disk.
pub fn project_caption(name: &str, dirty: bool) -> String {
    if dirty {
        format!("{name}*")
    } else {
        name.to_string()
    }
}

/// The three buttons on the save prompt.
pub const SAVE_PROMPT_SAVE: &str = "Save";
pub const SAVE_PROMPT_DISCARD: &str = "Don't Save";

/// The save-before-leaving prompt, laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavePromptLayout {
    pub frame: Rect,
    pub question: Rect,
    /// Leave without saving, on the left, away from the other two.
    pub discard: Rect,
    /// Stay.
    pub cancel: Rect,
    /// Save, then leave — the weighted answer, on the right.
    pub save: Rect,
}

/// Lays the save prompt out: [`confirm_layout`]'s card, a little wider, with
/// its row of buttons in three.
pub fn save_prompt_layout(window: Rect, metrics: &Metrics) -> SavePromptLayout {
    let card = confirm_layout(window, metrics);
    if card.frame.is_empty() {
        return SavePromptLayout {
            frame: Rect::ZERO,
            question: Rect::ZERO,
            discard: Rect::ZERO,
            cancel: Rect::ZERO,
            save: Rect::ZERO,
        };
    }
    let pad = metrics.panel_padding.max(1.0);
    let width = (window.width * 0.8)
        .clamp(0.0, 440.0)
        .min(window.width - pad * 2.0);
    let frame = Rect::new(
        window.x + (window.width - width) / 2.0,
        card.frame.y,
        width,
        card.frame.height,
    )
    .intersection(&window)
    .clamped();
    let inner = (frame.width - pad * 2.0).max(0.0);
    let question = Rect::new(frame.x + pad, card.question.y, inner, card.question.height)
        .intersection(&frame)
        .clamped();
    let btn_w = ((inner - pad * 2.0) / 3.0).max(0.0);
    let button = |i: f32| {
        Rect::new(
            frame.x + pad + (btn_w + pad) * i,
            card.confirm.y,
            btn_w,
            card.confirm.height,
        )
        .intersection(&frame)
        .clamped()
    };
    SavePromptLayout {
        frame,
        question,
        discard: button(0.0),
        cancel: button(1.0),
        save: button(2.0),
    }
}

/// A job's card, laid out: what is running, and how far it has got.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JobCardLayout {
    pub frame: Rect,
    pub label: Rect,
    pub bar: Rect,
}

/// Lays the job card along the bottom of `window`, directly above where a
/// toast would sit — the finish raises one, and the two must not overlap on
/// the frame they share.
pub fn job_card_layout(window: Rect, metrics: &Metrics) -> JobCardLayout {
    let toast = toast_layout(window, metrics, false).frame;
    if window.is_empty() || toast.is_empty() {
        return JobCardLayout {
            frame: Rect::ZERO,
            label: Rect::ZERO,
            bar: Rect::ZERO,
        };
    }
    let row = metrics.row_height.max(1.0);
    let pad = metrics.panel_padding.max(1.0);
    let height = row * 2.0 + pad * 2.0;
    let gap = pad;
    let frame = Rect::new(toast.x, toast.y - gap - height, toast.width, height)
        .intersection(&window)
        .clamped();
    let inner = (frame.width - pad * 2.0).max(0.0);
    let label = Rect::new(frame.x + pad, frame.y + pad, inner, row)
        .intersection(&frame)
        .clamped();
    let bar = Rect::new(frame.x + pad, label.bottom(), inner, row)
        .intersection(&frame)
        .clamped();
    JobCardLayout { frame, label, bar }
}
