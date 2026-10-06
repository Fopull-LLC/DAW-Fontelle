//! Analyze Musically keeps its own time (Ty, trying the preview build):
//!
//! > *"the playhead inside analyze musically only moves when the arrangement
//! > playhead is moving, which should not happen they should have no
//! > correlation ... pressing space to pause and play inside of the analyze
//! > musically window should not play and pause the arrangement."*
//!
//! The window's half, pure: which of the studio's transport keys the window
//! answers itself (all of them that would start, stop, move or arm the
//! song), and that a listen asks the window for frames of its own rather
//! than riding on the song's. The engine's half is
//! `fontelle-engine/tests/study_player.rs`, the host's
//! `fontelle-app/tests/analyze_render.rs`.

use fontelle_ui::canvas::{
    Action, AnalyzeState, AnalyzeTransportKey, AnalyzeView, Chord, ChordKey, Context, Keymap,
    analyze_transport_key,
};

/// Every global action that would touch the song's transport is the
/// window's own in the Analyze window; the history, the file and help stay
/// the studio's.
#[test]
fn the_windows_transport_keys_are_its_own() {
    assert_eq!(
        analyze_transport_key(Action::Play),
        Some(AnalyzeTransportKey::PlayStop)
    );
    assert_eq!(
        analyze_transport_key(Action::Stop),
        Some(AnalyzeTransportKey::ToStart)
    );
    assert_eq!(
        analyze_transport_key(Action::Record),
        Some(AnalyzeTransportKey::Record)
    );
    for action in Action::ALL {
        if action.context() != Context::Global {
            continue;
        }
        let ours = analyze_transport_key(action).is_some();
        let transport = matches!(action, Action::Play | Action::Stop | Action::Record);
        assert_eq!(ours, transport, "{action:?}");
    }
    assert_eq!(analyze_transport_key(Action::Undo), None);
    assert_eq!(analyze_transport_key(Action::Save), None);
}

/// The keys as the keymap hands them to the window: Space and Home reach
/// it as Play and Stop in the editor's context (so the window, asked first,
/// takes them), and Enter is its own play-the-selection, never the song's.
#[test]
fn space_home_and_enter_reach_the_window() {
    let map = Keymap::default();
    let press = |key: ChordKey| {
        map.action_of_press(&Chord::new(false, false, false, key), None, Context::Editor)
    };
    let space = press(Chord::parse("Space").unwrap().key).unwrap();
    assert_eq!(
        analyze_transport_key(space),
        Some(AnalyzeTransportKey::PlayStop)
    );
    let home = press(Chord::parse("Home").unwrap().key).unwrap();
    assert_eq!(
        analyze_transport_key(home),
        Some(AnalyzeTransportKey::ToStart)
    );
    let enter = press(Chord::parse("Enter").unwrap().key).unwrap();
    assert_eq!(enter, Action::AnalyzePlaySelection);
    assert_eq!(enter.context(), Context::Editor);
}

/// A listen asks for frames on its own: the playhead moves at the frame
/// rate whether or not the song is rolling.
#[test]
fn a_listen_asks_for_frames_of_its_own() {
    let mut state = AnalyzeState::default();
    assert!(!state.wants_frames(), "a still window sleeps");
    state.playhead = Some(1.25);
    assert!(state.wants_frames());
    state.playhead = None;
    assert!(!state.wants_frames());
}

/// Space with the cursor at (or past) the end plays from the start, as
/// every transport does, rather than a listen that ends before it begins.
#[test]
fn space_at_the_end_plays_from_the_start() {
    let view = AnalyzeView {
        duration: 4.0,
        ..AnalyzeView::default()
    };
    let mut state = AnalyzeState::default();
    state.cursor = 4.0;
    assert_eq!(state.space_range(&view), (0.0, None, false));
    state.cursor = 2.0;
    assert_eq!(state.space_range(&view), (2.0, None, false));
}
