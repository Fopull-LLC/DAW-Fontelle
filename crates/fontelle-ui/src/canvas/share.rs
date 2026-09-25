//! A shared song, from inside the studio: the Share panel and the card that
//! asks a question with more than two answers (`docs/collab-plan.md` §10).
//!
//! The panel hangs from the transport bar's Share button. Alone it offers
//! the two ways in; sharing it is the code, in the biggest type on it, with
//! Copy beside it, who is here, a line saying what is going on, and Stop
//! sharing. On the host's panel each person's row has *view only* and
//! *remove* (F49); a joiner's lists them and nothing more.
//!
//! Like every canvas here it is a pure view-model (INVARIANT 2): a layout
//! from rectangles and counts, a hit from a point. The session is the
//! host's; this only says where things are.

use crate::layout::Rect;
use crate::theme::Metrics;

/// The colours people are told apart by — in the panel's swatches and the
/// dot on the Share button — picked by a person's `colour` from the session.
/// The first is the host's.
///
/// Its own eight rather than the theme's: the plan borrowed "the theme's clip
/// palette", and there is none — a clip is its channel's colour. Saturated
/// enough to read on both themes as a mark, never as a surface.
pub const PEER_COLOURS: [[u8; 4]; 8] = [
    [0xe8, 0x8a, 0x3c, 0xff],
    [0x4f, 0x9d, 0xe0, 0xff],
    [0x6c, 0xc0, 0x5a, 0xff],
    [0xc8, 0x5a, 0xd0, 0xff],
    [0xe0, 0xc0, 0x3a, 0xff],
    [0x3a, 0xc4, 0xb8, 0xff],
    [0xe0, 0x5a, 0x6e, 0xff],
    [0x9a, 0x8a, 0xe0, 0xff],
];

/// A person's colour, whatever number the session gave them.
pub fn peer_colour(colour: u8) -> [u8; 4] {
    PEER_COLOURS[colour as usize % PEER_COLOURS.len()]
}

// The panel's words, in one place so the layout, the renderer and the
// window's label shaping agree on what is said.
pub const SHARE_THIS_SONG: &str = "Share this song";
pub const STOP_SHARING: &str = "Stop sharing";
pub const LEAVE_SESSION: &str = "Leave session";
pub const JOIN_A_SONG: &str = "Join a shared song\u{2026}";
pub const JOIN_INSTEAD: &str = "Join instead\u{2026}";
pub const COPY_CODE: &str = "Copy";
pub const VIEW_ONLY: &str = "view only";
pub const REMOVE_PEER: &str = "\u{00d7}";
/// Above the code, and the title in each other state.
pub const TITLE_ALONE: &str = "Work on this song with someone";
pub const TITLE_STARTING: &str = "Asking the relay for a code\u{2026}";
pub const TITLE_HOSTING: &str = "Anyone with this code can join";
pub const TITLE_JOINING: &str = "Joining\u{2026}";
pub const TITLE_JOINED: &str = "Working on this song together";
/// The row that stands in for an empty room.
pub const NOBODY_YET: &str = "Nobody has joined yet.";

/// Where this studio stands in a session — what the panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShareRole {
    #[default]
    Alone,
    /// Share was pressed; the relay has not answered with a code yet.
    Starting,
    Hosting,
    /// A code was typed; the song has not arrived yet.
    Joining,
    Joined,
}

impl ShareRole {
    /// Whether the panel lists who is here.
    fn lists_people(self) -> bool {
        matches!(self, Self::Hosting | Self::Joined)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Alone => TITLE_ALONE,
            Self::Starting => TITLE_STARTING,
            Self::Hosting => TITLE_HOSTING,
            Self::Joining => TITLE_JOINING,
            Self::Joined => TITLE_JOINED,
        }
    }

    /// The panel's main button: in, or out.
    pub fn primary(self) -> &'static str {
        match self {
            Self::Alone => SHARE_THIS_SONG,
            Self::Starting | Self::Hosting => STOP_SHARING,
            Self::Joining | Self::Joined => LEAVE_SESSION,
        }
    }

    /// The other way in, when there is one: nothing while one is under way.
    pub fn join(self) -> Option<&'static str> {
        match self {
            Self::Alone => Some(JOIN_A_SONG),
            Self::Hosting | Self::Joined => Some(JOIN_INSTEAD),
            Self::Starting | Self::Joining => None,
        }
    }
}

/// One person's row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeerRowLayout {
    pub frame: Rect,
    /// Their colour.
    pub swatch: Rect,
    pub name: Rect,
    /// The words *view only*, beside the switch; not pressable.
    pub view_only_label: Rect,
    /// The switch. Empty on a joiner's panel.
    pub view_only: Rect,
    /// The ×. Empty on a joiner's panel.
    pub remove: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SharePanelLayout {
    pub frame: Rect,
    pub title: Rect,
    /// The code, written at [`code_size`](Self::code_size). Empty but while
    /// hosting with a code to show.
    pub code: Rect,
    pub copy: Rect,
    /// As many people as fit.
    pub rows: Vec<PeerRowLayout>,
    /// Where [`NOBODY_YET`] goes when the room is empty.
    pub nobody: Rect,
    /// Two lines: who is fetching what, why the share ended — broken at the
    /// sentence's dash ([`status_lines`]).
    pub status: Rect,
    /// Share this song, Stop sharing, Leave session.
    pub primary: Rect,
    /// Join a shared song, Join instead. Empty while a session is starting.
    pub join: Rect,
    /// How big the code is written — the biggest text on the panel, because
    /// it is read across a room or typed off a screenshot.
    pub code_size: f32,
    /// How big everything else is.
    pub text_size: f32,
}

/// What a press on the panel means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareHit {
    /// The main button — whatever [`ShareRole::primary`] says it is.
    Primary,
    Copy,
    Join,
    /// The switch on the `n`th row.
    ViewOnly(usize),
    /// The × on the `n`th row.
    Remove(usize),
}

const PANEL_WIDTH: f32 = 320.0;
const COPY_WIDTH: f32 = 64.0;
const VIEW_ONLY_WIDTH: f32 = 64.0;

/// Lays the panel out under `anchor` (the Share button), inside `window`,
/// for `role` and `people` others in the room.
pub fn share_panel_layout(
    window: Rect,
    anchor: Rect,
    metrics: &Metrics,
    role: ShareRole,
    people: usize,
) -> SharePanelLayout {
    let row = metrics.row_height.max(1.0);
    let pad = metrics.panel_padding.max(1.0);
    let margin = metrics.panel_margin.max(0.0);
    let text_size = row * 0.6;
    let code_size = (text_size * 2.6).round();
    let button = (row * 1.3).round();

    let width = PANEL_WIDTH.min((window.width - 2.0 * margin).max(0.0));
    let x = (anchor.right() + pad - width)
        .min(window.right() - margin - width)
        .max(window.x + margin);
    let top = anchor.bottom() + pad * 0.5;
    let inner_w = (width - 2.0 * pad).max(0.0);
    let inner_x = x + pad;

    let hosting = role == ShareRole::Hosting;
    let code_h = if hosting { code_size + pad } else { 0.0 };
    let buttons = if role.join().is_some() {
        2.0 * button + pad * 0.5
    } else {
        button
    };
    // Everything but the rows, and then as many rows as the window leaves.
    let fixed = pad
        + row
        + pad
        + code_h
        + if hosting { pad } else { 0.0 }
        + 2.0 * row
        + pad
        + buttons
        + pad;
    let room = (window.bottom() - margin - top - fixed).max(0.0);
    let wanted = if role.lists_people() {
        people.max(1)
    } else {
        0
    };
    let fit = ((room / row).floor() as usize).min(wanted);
    let shown = if role.lists_people() {
        fit.min(people)
    } else {
        0
    };
    let height = fixed + fit as f32 * row;
    let frame = Rect::new(x, top, width, height)
        .intersection(&window)
        .clamped();

    let mut y = top + pad;
    let title = Rect::new(inner_x, y, inner_w, row);
    y += row + pad;
    let (code, copy) = if hosting {
        let copy_w = COPY_WIDTH.min(inner_w * 0.3);
        let code = Rect::new(inner_x, y, (inner_w - copy_w - pad).max(0.0), code_h);
        let copy = Rect::new(
            code.right() + pad,
            y + (code_h - button) / 2.0,
            copy_w,
            button,
        );
        y += code_h + pad;
        (code, copy)
    } else {
        (Rect::ZERO, Rect::ZERO)
    };
    let rows: Vec<PeerRowLayout> = (0..shown)
        .map(|i| {
            let frame = Rect::new(inner_x, y + i as f32 * row, inner_w, row);
            let side = (row * 0.5).round();
            let swatch = Rect::new(frame.x, frame.y + (row - side) / 2.0, side, side);
            let (view_only_label, view_only, remove) = if hosting {
                let remove = Rect::new(frame.right() - row, frame.y, row, row);
                let pill_w = (row * 1.4).round();
                let pill_h = (row * 0.6).round();
                let view_only = Rect::new(
                    remove.x - pad - pill_w,
                    frame.y + (row - pill_h) / 2.0,
                    pill_w,
                    pill_h,
                );
                let label_w = VIEW_ONLY_WIDTH.min((view_only.x - frame.x) * 0.4);
                let label = Rect::new(view_only.x - pad * 0.5 - label_w, frame.y, label_w, row);
                (label, view_only, remove)
            } else {
                (Rect::ZERO, Rect::ZERO, Rect::ZERO)
            };
            let name_end = if hosting {
                view_only_label.x - pad * 0.5
            } else {
                frame.right()
            };
            let name = Rect::new(
                swatch.right() + pad,
                frame.y,
                (name_end - swatch.right() - pad).max(0.0),
                row,
            );
            PeerRowLayout {
                frame,
                swatch,
                name,
                view_only_label,
                view_only,
                remove,
            }
        })
        .collect();
    let nobody = if role.lists_people() && shown == 0 && fit > 0 {
        Rect::new(inner_x, y, inner_w, row)
    } else {
        Rect::ZERO
    };
    y += fit as f32 * row;
    let status = Rect::new(inner_x, y, inner_w, 2.0 * row);
    y += 2.0 * row + pad;
    let primary = Rect::new(inner_x, y, inner_w, button);
    let join = if role.join().is_some() {
        Rect::new(inner_x, y + button + pad * 0.5, inner_w, button)
    } else {
        Rect::ZERO
    };

    let keep = |r: Rect| r.intersection(&frame).clamped();
    SharePanelLayout {
        frame,
        title: keep(title),
        code: keep(code),
        copy: keep(copy),
        rows: rows
            .into_iter()
            .map(|r| PeerRowLayout {
                frame: keep(r.frame),
                swatch: keep(r.swatch),
                name: keep(r.name),
                view_only_label: keep(r.view_only_label),
                view_only: keep(r.view_only),
                remove: keep(r.remove),
            })
            .collect(),
        nobody: keep(nobody),
        status: keep(status),
        primary: keep(primary),
        join: keep(join),
        code_size,
        text_size,
    }
}

/// What is under `(x, y)` on the panel, if anything is.
pub fn share_hit(layout: &SharePanelLayout, x: f32, y: f32) -> Option<ShareHit> {
    if layout.primary.contains(x, y) {
        return Some(ShareHit::Primary);
    }
    if layout.copy.contains(x, y) {
        return Some(ShareHit::Copy);
    }
    if layout.join.contains(x, y) {
        return Some(ShareHit::Join);
    }
    for (i, row) in layout.rows.iter().enumerate() {
        if row.view_only.contains(x, y) {
            return Some(ShareHit::ViewOnly(i));
        }
        if row.remove.contains(x, y) {
            return Some(ShareHit::Remove(i));
        }
    }
    None
}

/// Every string the panel draws at its ordinary size, for the window to shape
/// ahead of drawing. The code is not among them: it is drawn large, and
/// shaped at [`SharePanelLayout::code_size`].
pub fn share_panel_words(role: ShareRole, people: &[crate::document::SessionPeer]) -> Vec<String> {
    let mut words = vec![role.title().to_string(), role.primary().to_string()];
    words.extend(role.join().map(str::to_string));
    if role == ShareRole::Hosting {
        words.extend([COPY_CODE, VIEW_ONLY, REMOVE_PEER].map(str::to_string));
    }
    if role.lists_people() {
        words.push(NOBODY_YET.to_string());
        words.extend(people.iter().map(|p| p.name.clone()));
    }
    words
}

/// The dot on the Share button while this studio is in a session, in the
/// host's colour: the one thing in the studio that says the song is shared.
pub fn share_dot(button: Rect) -> Rect {
    if button.is_empty() {
        return Rect::ZERO;
    }
    let side = (button.height * 0.32).round().max(4.0).min(button.width);
    Rect::new(button.right() - side - 2.0, button.y + 2.0, side, side)
}

// ------------------------------------------------------- a many-answer card ---

/// A question with more than the two answers [`crate::canvas::confirm_layout`]
/// has room for — the join's (§4.3) and the fetch's (§7.1).
#[derive(Debug, Clone, PartialEq)]
pub struct ChoicePromptLayout {
    pub frame: Rect,
    /// A line of the question each.
    pub lines: Vec<Rect>,
    /// The answers, left to right in the order the question gives them —
    /// **the safe one first**, which is also the one Enter presses and the one
    /// drawn weighted.
    pub buttons: Vec<Rect>,
}

/// Lays the card out in `window` for `lines` lines and `buttons` answers.
pub fn choice_prompt_layout(
    window: Rect,
    metrics: &Metrics,
    lines: usize,
    buttons: usize,
) -> ChoicePromptLayout {
    let row = metrics.row_height.max(1.0);
    let pad = metrics.panel_padding.max(1.0);
    let button_h = (row * 1.3).round();
    let width = (window.width * 0.9)
        .min(520.0)
        .min((window.width - 2.0 * pad).max(0.0));
    let height = pad * 2.0 + lines as f32 * row + pad + button_h;
    let frame = Rect::new(
        window.x + (window.width - width) / 2.0,
        window.y + ((window.height - height) / 3.0).max(0.0),
        width,
        height,
    )
    .intersection(&window)
    .clamped();
    let inner = (frame.width - 2.0 * pad).max(0.0);
    let keep = |r: Rect| r.intersection(&frame).clamped();
    let lines_r: Vec<Rect> = (0..lines)
        .map(|i| {
            keep(Rect::new(
                frame.x + pad,
                frame.y + pad + i as f32 * row,
                inner,
                row,
            ))
        })
        .collect();
    let count = buttons.max(1) as f32;
    let button_w = ((inner - pad * (count - 1.0)) / count).max(0.0);
    let button_y = frame.y + pad + lines as f32 * row + pad;
    let buttons_r = (0..buttons)
        .map(|i| {
            keep(Rect::new(
                frame.x + pad + i as f32 * (button_w + pad),
                button_y,
                button_w,
                button_h,
            ))
        })
        .collect();
    ChoicePromptLayout {
        frame,
        lines: lines_r,
        buttons: buttons_r,
    }
}

/// Which answer is under `(x, y)`, if any.
pub fn choice_prompt_hit(layout: &ChoicePromptLayout, x: f32, y: f32) -> Option<usize> {
    layout.buttons.iter().position(|b| b.contains(x, y))
}

/// A status sentence as the panel's two lines: broken at its dash, which is
/// where "Alice removed you from the session — your copy is still open"
/// turns (F60). One without a dash is one line.
pub fn status_lines(sentence: &str) -> (&str, &str) {
    match sentence.split_once(" \u{2014} ") {
        Some((first, second)) => (first, second),
        None => (sentence, ""),
    }
}
