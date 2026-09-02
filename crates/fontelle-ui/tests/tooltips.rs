//! Hover tips: what a button does, said out loud when you wait on it.
//!
//! Reported from using the window:
//!
//! > *"when hovering my mouse over an option, it should show me a tiny textbox
//! > explaining what that button does if i hold my mouse there long enough,
//! > that way if i'm confused what an icon is / does i can just hover my mouse
//! > over it for a little bit and read what it does."*
//!
//! This window is nearly all icons — the transport bar, both toolbars, the
//! rack's four switches per row — and an icon set is a private language until
//! something translates it.
//!
//! # Two halves, and only one of them is here
//!
//! The **placement** is arithmetic and lives in `crate::tooltip`: a box near
//! the pointer that never leaves the window, which is the whole of what makes
//! a tip in the bottom-right corner readable rather than half off the screen.
//!
//! The **words** are a `tip()` on each hover enum, and they are checked here
//! for existing rather than for their wording — a test that restated every
//! caption would be a second copy of them to keep in step.

use std::time::Duration;

use fontelle_ui::canvas::{BrowserHit, BrowserMode, MixerHit, OptionsHit, RackHit, RollControl,
    TimelineControl, Tool};
use fontelle_ui::layout::{EditorTab, Rect};
use fontelle_ui::tooltip::{TOOLTIP_DELAY, TOOLTIP_PAD, tooltip_layout};
use fontelle_ui::transport::TransportHit;

/// A window's worth of room.
fn bounds() -> Rect {
    Rect::new(0.0, 0.0, 1280.0, 720.0)
}

/// A tip about the size of "Play from the marker".
fn text() -> (f32, f32) {
    (128.0, 14.0)
}

// ----------------------------------------------------------- where it goes ---

#[test]
fn a_tip_sits_near_the_pointer_and_below_it() {
    // Below rather than on: a box over the thing it is describing hides the
    // thing it is describing.
    let tip = tooltip_layout(text(), (400.0, 300.0), bounds());
    assert!(!tip.is_empty());
    assert!(tip.y > 300.0, "the tip {tip:?} is drawn over the pointer");
    assert!(tip.y < 300.0 + 60.0, "and it is not halfway down the window");
    assert!((tip.x - 400.0).abs() < 60.0, "nor is it across the room");
}

#[test]
fn a_tip_is_big_enough_for_its_words() {
    let (w, h) = text();
    let tip = tooltip_layout((w, h), (400.0, 300.0), bounds());
    assert!(
        tip.width >= w + TOOLTIP_PAD * 2.0,
        "the box {tip:?} is narrower than the text in it"
    );
    assert!(tip.height >= h + TOOLTIP_PAD * 2.0);
}

#[test]
fn a_tip_near_the_right_edge_slides_back_inside() {
    // The transport bar's last button and the mixer's master strip both live
    // there, and a tip that ran off the window would be unreadable exactly
    // where the icons are least obvious.
    let tip = tooltip_layout(text(), (bounds().right() - 4.0, 300.0), bounds());
    assert!(!tip.is_empty());
    assert!(
        tip.right() <= bounds().right() + 0.001,
        "the tip {tip:?} runs off the right of {:?}",
        bounds()
    );
    assert!(tip.x >= bounds().x - 0.001);
}

#[test]
fn a_tip_near_the_bottom_flips_above_the_pointer() {
    let tip = tooltip_layout(text(), (400.0, bounds().bottom() - 4.0), bounds());
    assert!(!tip.is_empty());
    assert!(
        tip.bottom() <= bounds().bottom() + 0.001,
        "the tip {tip:?} runs off the bottom"
    );
    assert!(
        tip.bottom() <= bounds().bottom() - 4.0 + 0.001,
        "a tip at the bottom edge has to go *above* the pointer, not be \
         squashed against it: {tip:?}"
    );
}

#[test]
fn a_tip_in_the_corner_stays_in_the_window() {
    for (x, y) in [
        (0.0, 0.0),
        (bounds().right(), 0.0),
        (0.0, bounds().bottom()),
        (bounds().right(), bounds().bottom()),
    ] {
        let tip = tooltip_layout(text(), (x, y), bounds());
        assert_eq!(
            tip.intersection(&bounds()),
            tip,
            "a tip at ({x}, {y}) escaped the window: {tip:?}"
        );
    }
}

#[test]
fn a_window_too_small_for_a_tip_gets_none() {
    // An empty rectangle draws as nothing, which is better than a sliver
    // showing two letters of an explanation.
    let tiny = Rect::new(0.0, 0.0, 20.0, 10.0);
    assert!(tooltip_layout(text(), (5.0, 5.0), tiny).is_empty());
    assert!(tooltip_layout(text(), (0.0, 0.0), Rect::ZERO).is_empty());
}

#[test]
fn the_dwell_is_long_enough_not_to_flicker_and_short_enough_to_wait_out() {
    // The number itself is a judgement; what a test can hold is that it is a
    // judgement somebody made. Under ~300 ms a tip flashes on every pointer
    // that crosses a toolbar; over about a second nobody waits for it.
    assert!(TOOLTIP_DELAY >= Duration::from_millis(300));
    assert!(TOOLTIP_DELAY <= Duration::from_millis(1000));
}

// -------------------------------------------------------------- the words ---

#[test]
fn every_transport_button_explains_itself() {
    for what in [
        TransportHit::Play,
        TransportHit::Stop,
        TransportHit::ToggleLoop,
        TransportHit::ToggleRecord,
        TransportHit::ToggleMetronome,
        TransportHit::Tempo,
        TransportHit::Signature,
    ] {
        let tip = what.tip();
        assert!(tip.is_some(), "{what:?} has nothing to say");
        assert!(!tip.unwrap().is_empty());
    }
    // The ruler is a place, not a button. A tip following the pointer along it
    // would be a box in the way of the thing being scrubbed.
    assert!(TransportHit::Scrub(0).tip().is_none());
}

#[test]
fn every_roll_toolbar_control_explains_itself() {
    let mut controls = vec![
        RollControl::Snap,
        RollControl::ZoomOutX,
        RollControl::ZoomInX,
        RollControl::ZoomOutY,
        RollControl::ZoomInY,
        RollControl::Velocity,
        RollControl::Lane,
        RollControl::Ghost,
        RollControl::Slide,
    ];
    controls.extend(
        [Tool::Draw, Tool::Paint, Tool::Delete, Tool::Select, Tool::Slice, Tool::Mute, Tool::Slip]
            .into_iter()
            .map(RollControl::Tool),
    );
    for control in controls {
        assert!(
            control.tip().is_some_and(|t| !t.is_empty()),
            "{control:?} has nothing to say"
        );
    }
}

#[test]
fn every_arrangement_toolbar_control_explains_itself() {
    for control in [
        TimelineControl::Draw,
        TimelineControl::Select,
        TimelineControl::Snap,
        TimelineControl::Repeat,
        TimelineControl::Loop,
        TimelineControl::Cut,
        TimelineControl::Copy,
        TimelineControl::Paste,
        TimelineControl::Mute,
        TimelineControl::ZoomOut,
        TimelineControl::ZoomIn,
    ] {
        assert!(
            control.tip().is_some_and(|t| !t.is_empty()),
            "{control:?} has nothing to say"
        );
    }
}

#[test]
fn every_editor_tab_explains_itself() {
    for tab in [
        EditorTab::Roll,
        EditorTab::Mixer,
    ] {
        assert!(tab.tip().is_some_and(|t| !t.is_empty()), "{tab:?} is silent");
    }
}

#[test]
fn every_mixer_control_explains_itself() {
    for what in [
        MixerHit::Name(0),
        MixerHit::Strip(0),
        MixerHit::Fader(0),
        MixerHit::Pan(0),
        MixerHit::Mute(0),
        MixerHit::Solo(0),
        MixerHit::Insert(0, 0),
        MixerHit::BypassInsert(0, 0),
        MixerHit::AddInsert(0),
        MixerHit::AddTrack,
        MixerHit::Options(OptionsHit::Rename),
        MixerHit::Options(OptionsHit::Output),
        MixerHit::Options(OptionsHit::Insert(0)),
        MixerHit::Options(OptionsHit::Bypass(0)),
        MixerHit::Options(OptionsHit::Remove(0)),
        MixerHit::Options(OptionsHit::Grip(0)),
        MixerHit::Options(OptionsHit::AddInsert),
    ] {
        assert!(
            what.tip().is_some_and(|t| !t.is_empty()),
            "{what:?} has nothing to say"
        );
    }
    assert!(MixerHit::Nothing.tip().is_none());
}

#[test]
fn every_browser_and_rack_button_explains_itself() {
    for what in [
        BrowserHit::Mode(BrowserMode::Sounds),
        BrowserHit::Mode(BrowserMode::Projects),
        BrowserHit::Search(BrowserMode::Sounds),
        BrowserHit::Search(BrowserMode::Projects),
        BrowserHit::OpenFolder(BrowserMode::Sounds),
        BrowserHit::OpenFolder(BrowserMode::Projects),
        BrowserHit::ChooseFolder(BrowserMode::Sounds),
        BrowserHit::ChooseFolder(BrowserMode::Projects),
        BrowserHit::NewProject,
        BrowserHit::Export,
    ] {
        assert!(
            what.tip().is_some_and(|t| !t.is_empty()),
            "{what:?} has nothing to say"
        );
    }
    // A row explains itself by being a row with a name on it.
    assert!(BrowserHit::File(0).tip().is_none());
    assert!(BrowserHit::Preset(0).tip().is_none());
    assert!(BrowserHit::Nothing.tip().is_none());

    for what in [
        RackHit::Mute(0),
        RackHit::Solo(0),
        RackHit::Edit(0),
        RackHit::Route(0),
        RackHit::Add,
    ] {
        assert!(
            what.tip().is_some_and(|t| !t.is_empty()),
            "{what:?} has nothing to say"
        );
    }
    assert!(RackHit::Row(0).tip().is_none());
    assert!(RackHit::Nothing.tip().is_none());
}

#[test]
fn a_tip_is_one_short_line_rather_than_a_paragraph() {
    // A tooltip is a label, not documentation. Anything long enough to need
    // wrapping is a box that covers the thing it is explaining.
    let mut all: Vec<&'static str> = Vec::new();
    all.extend([TransportHit::Play, TransportHit::ToggleMetronome].iter().filter_map(|h| h.tip()));
    all.extend(
        [RollControl::Snap, RollControl::Ghost, RollControl::Slide]
            .iter()
            .filter_map(|c| c.tip()),
    );
    all.extend(
        [TimelineControl::Repeat, TimelineControl::Loop]
            .iter()
            .filter_map(|c| c.tip()),
    );
    all.extend(
        [MixerHit::AddTrack, MixerHit::Options(OptionsHit::Output)]
            .iter()
            .filter_map(|h| h.tip()),
    );
    for tip in all {
        assert!(tip.len() <= 64, "\"{tip}\" is a paragraph, not a tip");
        assert!(!tip.contains('\n'), "\"{tip}\" has a line break in it");
    }
}
