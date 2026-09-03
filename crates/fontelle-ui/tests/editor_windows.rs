//! Instruments and effects open as **windows**, not as tabs (TDD §7.2, §7.5,
//! §12, §13.4).
//!
//! Reported from using the window: *"the eq effect you added opens as a new
//! tab in the same window as the mixer and piano roll and instrument, even
//! though we had already decided that effects and instruments should be
//! windows that are opened up — that way we could support third party VSTs and
//! effects, which all would expect to be opened as windows."*
//!
//! That is not a preference, it is §7.5. A VST or a CLAP editor is handed a
//! *parent window* and draws into it; an editor that can only exist as one of
//! the editor column's tabs is one the plugin path can never be built on. So
//! the two views that are genuinely the document — the notes and the mixer —
//! stay as tabs, and everything that edits one **device** is a window.
//!
//! An automation clip is neither: it is edited inside its own block on the
//! arrangement (`tests/automation_blocks.rs`), so there is no window kind for
//! it and the lists below have two entries, not three.
//!
//! What is testable without a compositor is the geometry and the vocabulary:
//! that the tab strip has two tabs and cannot name a third, and that a
//! floating window's insides are laid out inside it. The windowing itself —
//! creating, routing events, closing one without closing the studio — is
//! `fontelle-ui/src/app.rs`, and is only checkable by opening one.

use fontelle_ui::layout::{
    EditorKind, EditorTab, editor_tab_at, editor_tabs, editor_window_layout,
};
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

// ------------------------------------------------------------- the tabs ---

#[test]
fn the_editor_column_offers_the_roll_and_the_mixer_and_nothing_else() {
    // Stated over the whole surface rather than over the two rectangles: the
    // claim is that there is nowhere in the header you can press and be given
    // an instrument, which is what "it is not a tab any more" means.
    let m = metrics();
    let header = fontelle_ui::layout::Rect::new(264.0, 256.0, 728.0, 26.0);
    let tabs = editor_tabs(header, &m);
    let mut seen = Vec::new();
    let mut x = header.x;
    while x < header.right() {
        if let Some(tab) = editor_tab_at(&tabs, x, header.y + header.height / 2.0)
            && !seen.contains(&tab)
        {
            seen.push(tab);
        }
        x += 2.0;
    }
    seen.sort_by_key(|t| format!("{t:?}"));
    assert_eq!(seen, vec![EditorTab::Mixer, EditorTab::Roll]);
}

// --------------------------------------------------------- the windows ---

#[test]
fn every_kind_of_editor_window_has_a_name_and_a_size_to_open_at() {
    // A window with no title is one you cannot find in a task switcher, and
    // one with no size opens at whatever the compositor feels like.
    for kind in [EditorKind::Instrument, EditorKind::Effect] {
        assert!(!kind.title().is_empty(), "{kind:?} has no name");
        let (w, h) = kind.default_size();
        let (min_w, min_h) = kind.minimum_size();
        assert!(w > 0 && h > 0, "{kind:?} opens at {w}x{h}");
        assert!(
            w >= min_w && h >= min_h,
            "{kind:?} opens at {w}x{h}, smaller than the {min_w}x{min_h} it may be dragged to"
        );
    }
}

#[test]
fn a_floating_editor_is_a_header_and_a_body_and_they_fit_inside_it() {
    // The same shape a docked panel has, deliberately: the panels drawn into
    // these windows are the ones that were drawn into the editor column, and
    // they take a body rectangle either way.
    let m = metrics();
    let (w, h) = EditorKind::Effect.default_size();
    let panel = editor_window_layout(w as f32, h as f32, &m);

    assert_eq!(panel.frame.x, 0.0, "a window's own origin is its corner");
    assert_eq!(panel.frame.y, 0.0);
    assert_eq!(panel.frame.width, w as f32);
    assert_eq!(panel.frame.height, h as f32);
    assert!(!panel.header.is_empty(), "nowhere to say what is open");
    assert!(!panel.body.is_empty(), "nowhere to draw the effect");
    assert!(!panel.header.intersects(&panel.body));
    assert_eq!(
        panel.body.intersection(&panel.frame),
        panel.body,
        "the body {:?} escapes the window {:?}",
        panel.body,
        panel.frame
    );
    assert!(panel.header.bottom() <= panel.body.y + 0.001);
}

#[test]
fn a_window_dragged_to_nothing_yields_empty_rects_never_negative_ones() {
    // A compositor can hand back a zero-sized window mid-resize, and on
    // Wayland it does.
    let m = metrics();
    for (w, h) in [(0.0, 0.0), (1.0, 1.0), (320.0, 10.0), (10.0, 200.0)] {
        let panel = editor_window_layout(w, h, &m);
        for r in [panel.frame, panel.header, panel.body] {
            assert!(r.width >= 0.0 && r.height >= 0.0, "{w}x{h} gave {r:?}");
        }
    }
}

#[test]
fn each_kind_opens_big_enough_to_use_the_panel_it_carries() {
    // A window that opens smaller than its own contents is one you have to
    // resize before you can see the thing beside it — which is the whole
    // reason these are windows rather than tabs.
    let m = metrics();
    for kind in [EditorKind::Instrument, EditorKind::Effect] {
        let (w, h) = kind.default_size();
        let panel = editor_window_layout(w as f32, h as f32, &m);
        assert!(
            panel.body.width > 200.0 && panel.body.height > 100.0,
            "{kind:?} opens with only {:?} to draw in",
            panel.body
        );
    }
}
