//! Dragging the time bar.
//!
//! > *"dragging the time bar in shared songs makes everything freeze and the
//! > audio glitch out wildly this also sometimes happens in normal songs but
//! > happens always in shared songs"* (2026-10-18)
//!
//! Every pointer move of a ruler drag used to seek the engine — a seek cuts
//! every sounding voice and re-cursors the timeline — and then run a whole
//! frame's work on the spot (`tick`), which in a shared song asked the disk
//! about every file the song names. A mouse reports hundreds of moves a
//! second. Now a drag seeks once a frame, at wherever the pointer is by
//! then, and once more as it is let go.

mod common;

use std::sync::{Arc, Mutex};

use fontelle_ui::transport::{TransportHost, TransportView};
use winit::event::MouseButton;

/// A transport that remembers every seek.
struct Counting {
    seeks: Arc<Mutex<Vec<i64>>>,
    view: TransportView,
}

impl TransportHost for Counting {
    fn view(&mut self) -> TransportView {
        let mut view = self.view;
        if let Some(last) = self.seeks.lock().unwrap().last() {
            view.position_sample = *last;
        }
        view
    }
    fn play(&mut self) {
        self.view.playing = true;
    }
    fn stop(&mut self) {
        self.view.playing = false;
    }
    fn seek(&mut self, sample: i64) {
        self.seeks.lock().unwrap().push(sample);
    }
    fn set_looping(&mut self, on: bool) {
        self.view.looping = on;
    }
}

fn window(playing: bool) -> (fontelle_ui::Windowless, Arc<Mutex<Vec<i64>>>) {
    let seeks = Arc::new(Mutex::new(Vec::new()));
    let view = TransportView {
        available: true,
        playing,
        length_samples: 48_000 * 60,
        sample_rate: 48_000.0,
        ..TransportView::default()
    };
    let session = common::a_session_for(common::a_clip_project(8));
    let w = fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 720),
    )
    .with_host(Box::new(Counting {
        seeks: seeks.clone(),
        view,
    }));
    (w, seeks)
}

#[test]
fn a_ruler_drag_seeks_once_a_frame_not_once_a_pointer_move() {
    for playing in [false, true] {
        let (mut w, seeks) = window(playing);
        let ruler = w.timeline_ruler();
        let y = ruler.y + ruler.height / 2.0;
        w.press_studio(MouseButton::Left, ruler.x + 20.0, y);
        for i in 0..200 {
            w.move_studio(ruler.x + 20.0 + i as f32 * 2.0, y);
        }
        let during = seeks.lock().unwrap().len();
        assert!(
            during <= 1,
            "200 pointer moves in one frame made {during} seeks (playing: {playing})"
        );
        w.frame(0.016);
        let after_frame = seeks.lock().unwrap().len();
        assert!(after_frame >= 1, "the frame seeks to where the pointer is");
        w.move_studio(ruler.x + 500.0, y);
        w.release_studio(MouseButton::Left);
        let all = seeks.lock().unwrap().clone();
        assert!(all.len() <= 3, "{all:?}");
        let last = *all.last().unwrap();
        assert!(
            last > all[0],
            "and the drag ends at the pointer, later than it began: {all:?}"
        );
    }
}

#[test]
fn a_click_on_the_ruler_still_moves_the_marker() {
    let (mut w, seeks) = window(false);
    let ruler = w.timeline_ruler();
    let y = ruler.y + ruler.height / 2.0;
    w.press_studio(MouseButton::Left, ruler.x + 300.0, y);
    w.release_studio(MouseButton::Left);
    let all = seeks.lock().unwrap().clone();
    assert_eq!(all.len(), 1, "one click, one seek: {all:?}");
    assert!(all[0] > 0);
}

/// A transport whose state the test can read while the window holds it.
#[derive(Default)]
struct Shared {
    playing: bool,
    /// Every call, in order: `play`, `stop`, or `seek <sample>`.
    calls: Vec<String>,
    at: i64,
}

struct Watched(Arc<Mutex<Shared>>);

impl TransportHost for Watched {
    fn view(&mut self) -> TransportView {
        let s = self.0.lock().unwrap();
        TransportView {
            available: true,
            playing: s.playing,
            position_sample: s.at,
            length_samples: 48_000 * 60,
            sample_rate: 48_000.0,
            ..TransportView::default()
        }
    }
    fn play(&mut self) {
        let mut s = self.0.lock().unwrap();
        s.playing = true;
        s.calls.push("play".into());
    }
    fn stop(&mut self) {
        let mut s = self.0.lock().unwrap();
        s.playing = false;
        s.calls.push("stop".into());
    }
    fn seek(&mut self, sample: i64) {
        let mut s = self.0.lock().unwrap();
        s.at = sample;
        s.calls.push(format!("seek {sample}"));
    }
    fn set_looping(&mut self, _on: bool) {}
}

fn watched(playing: bool) -> (fontelle_ui::Windowless, Arc<Mutex<Shared>>) {
    let shared = Arc::new(Mutex::new(Shared {
        playing,
        ..Shared::default()
    }));
    let session = common::a_session_for(common::a_clip_project(8));
    let mut w = fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 720),
    )
    .with_host(Box::new(Watched(shared.clone())));
    // A frame, so the window has read that the song is playing.
    w.frame(0.016);
    shared.lock().unwrap().calls.clear();
    (w, shared)
}

/// Ty, 2026-10-20: *"pause playback while moving around your time marker
/// via clicking and dragging and if you were playing before starting the
/// move just resume after."*
#[test]
fn dragging_the_marker_while_playing_pauses_and_plays_on_from_where_it_is_let_go() {
    let (mut w, shared) = watched(true);
    let ruler = w.timeline_ruler();
    let y = ruler.y + ruler.height / 2.0;
    w.press_studio(MouseButton::Left, ruler.x + 40.0, y);
    assert!(
        !shared.lock().unwrap().playing,
        "the song played on under the drag: {:?}",
        shared.lock().unwrap().calls
    );
    for i in 0..40 {
        w.move_studio(ruler.x + 40.0 + i as f32 * 10.0, y);
        w.frame(0.016);
        assert!(!shared.lock().unwrap().playing, "it started again mid-drag");
    }
    w.release_studio(MouseButton::Left);
    let s = shared.lock().unwrap();
    assert!(s.playing, "it did not play on: {:?}", s.calls);
    let play = s.calls.iter().rposition(|c| c == "play").unwrap();
    let seek = s.calls[..play]
        .iter()
        .rposition(|c| c.starts_with("seek"))
        .expect("a seek before the play");
    let to: i64 = s.calls[seek][5..].parse().unwrap();
    assert!(
        to > 0 && s.at >= to,
        "it plays from where the marker was let go: {:?}",
        s.calls
    );
}

/// Stopped, a drag stays stopped.
#[test]
fn dragging_the_marker_while_stopped_does_not_start_the_song() {
    let (mut w, shared) = watched(false);
    let ruler = w.timeline_ruler();
    let y = ruler.y + ruler.height / 2.0;
    w.press_studio(MouseButton::Left, ruler.x + 40.0, y);
    w.move_studio(ruler.x + 300.0, y);
    w.frame(0.016);
    w.release_studio(MouseButton::Left);
    let s = shared.lock().unwrap();
    assert!(!s.playing, "{:?}", s.calls);
    assert!(!s.calls.iter().any(|c| c == "play"), "{:?}", s.calls);
}
