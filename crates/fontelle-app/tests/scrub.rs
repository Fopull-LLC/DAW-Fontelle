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
