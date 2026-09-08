//! Where two clips on one row overlap, on screen.
//!
//! Reported from using the window:
//!
//! > *"right now when clips are overlapping they kind of blend together which
//! > makes it hard to tell when theres an overlap or the start and end of a
//! > clip. please make it so that the edges of clips are always visible and
//! > dont blend into eachother when they get close or even overlapped, and
//! > when theyre overlapped, there should be a kind of diagonal stripe
//! > pattern on the overlapping part so the user can see what parts are
//! > overlapping."*
//!
//! The stripes need the overlaps as rectangles, and that is geometry the
//! renderer should not be working out for itself: `canvas::clip_overlaps`
//! is the one answer to "which parts of which blocks lie over each other",
//! measured on the same block rectangles the pointer is tested against.

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    TimelineView, clip_bands, clip_overlaps, clip_rect, timeline_layout, timeline_tick_to_x,
};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

const BAR: Tick = PPQN * 4;

fn view() -> TimelineView {
    TimelineView {
        pixels_per_tick: 0.02,
        lane_height: 40.0,
        ..TimelineView::default()
    }
}

fn grid() -> Rect {
    timeline_layout(
        Rect::new(0.0, 0.0, 1200.0, 300.0),
        &Theme::dark_default().metrics,
    )
    .grid
}

fn clips(items: &[(usize, Tick, Tick)]) -> Vec<ClipInfo> {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    items
        .iter()
        .map(|(lane, start, length)| ClipInfo {
            id: arena.insert(()),
            lane: *lane,
            start: *start,
            length: *length,
            name: "Part".to_string(),
            muted: false,
            open: false,
            color: [0x4f, 0x8f, 0xd0, 0xff],
            loop_length: None,
            kind: ClipKind::Audio,
            curve: Vec::new(),
            notes: Vec::new(),
            audio: Default::default(),
            prefab: None,
        })
        .collect()
}

#[test]
fn two_clips_that_overlap_on_one_row_give_the_part_they_share() {
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 2), (0, BAR, BAR * 2)]);
    let overlaps = clip_overlaps(&v, g, &clips);
    assert_eq!(overlaps.len(), 1);
    let shared = overlaps[0].area;
    let from = timeline_tick_to_x(&v, g, BAR);
    let to = timeline_tick_to_x(&v, g, BAR * 2);
    assert!(
        (shared.x - from).abs() < 0.01,
        "starts at {} not {from}",
        shared.x
    );
    assert!(
        (shared.right() - to).abs() < 0.01,
        "ends at {} not {to}",
        shared.right()
    );
    // The row's own height: the whole of both blocks, top to bottom.
    let block = clip_rect(&v, g, &clips[0]);
    assert!((shared.y - block.y).abs() < 0.01);
    assert!((shared.height - block.height).abs() < 0.01);
}

#[test]
fn clips_that_only_touch_do_not_overlap() {
    // End-to-end is the ordinary arrangement of a song; a stripe at every
    // seam would say every song was wrong.
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR), (0, BAR, BAR)]);
    assert!(clip_overlaps(&v, g, &clips).is_empty());
}

#[test]
fn clips_on_different_rows_do_not_overlap_however_they_line_up() {
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 2), (1, 0, BAR * 2)]);
    assert!(clip_overlaps(&v, g, &clips).is_empty());
}

#[test]
fn a_clip_inside_another_is_the_inner_one_entirely() {
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 4), (0, BAR, BAR)]);
    let overlaps = clip_overlaps(&v, g, &clips);
    assert_eq!(overlaps.len(), 1);
    let inner = clip_rect(&v, g, &clips[1]);
    assert!((overlaps[0].area.x - inner.x).abs() < 0.01);
    assert!((overlaps[0].area.width - inner.width).abs() < 0.01);
}

#[test]
fn three_clips_piled_up_give_every_pair() {
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 2), (0, BAR, BAR * 2), (0, BAR * 2, BAR * 2)]);
    // 0 with 1, and 1 with 2; 0 and 2 only touch.
    assert_eq!(clip_overlaps(&v, g, &clips).len(), 2);
}

#[test]
fn an_overlap_off_the_screen_is_not_built() {
    let v = TimelineView {
        scroll_tick: BAR * 40,
        ..view()
    };
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 2), (0, BAR, BAR * 2)]);
    assert!(clip_overlaps(&v, g, &clips).is_empty());
}

#[test]
fn an_overlap_half_off_the_screen_is_clipped_to_the_grid() {
    let v = view();
    let g = grid();
    // The second clip starts at bar 1 and runs off the right edge.
    let clips = clips(&[(0, 0, BAR * 400), (0, BAR, BAR * 400)]);
    let overlaps = clip_overlaps(&v, g, &clips);
    assert_eq!(overlaps.len(), 1);
    assert!(overlaps[0].area.right() <= g.right() + 0.01);
    assert!(overlaps[0].area.x >= g.x - 0.01);
}

// ---------------------------------------------- the crossfade, drawn ---
//
// > *"i do want it to also show the graph line drawn to show the fade on the
// > overlap as well so it resembles the other fades but just with them
// > crossing through eachother like how fls displays."*
//
// Two audio clips that overlap already crossfade — the earlier one out over
// the overlap, the later one in — and the overlap already says *something*
// is happening there by being striped. What it did not say is **what**: the
// shape of the blend, which is the thing a person adjusts by dragging the
// clips. So the overlap carries the two curves as well, crossing, drawn over
// the stripes. They are the player's own envelope, not a decoration: the
// equal-power crossfade `AudioPlacement::auto_gain` applies, times whatever
// the clip's own fade is doing there.

/// The gain a drawn point stands for, 0 at the foot of the content band and
/// 1 at its top — the inverse of what the geometry does.
fn gain_at(content: Rect, point: (f32, f32)) -> f32 {
    (content.bottom() - point.1) / content.height
}

fn audio_pair() -> Vec<ClipInfo> {
    clips(&[(0, 0, BAR * 2), (0, BAR, BAR * 2)])
}

#[test]
fn two_overlapping_audio_clips_carry_the_two_crossing_curves() {
    let v = view();
    let g = grid();
    let clips = audio_pair();
    let overlaps = clip_overlaps(&v, g, &clips);
    assert_eq!(overlaps.len(), 1);
    let shared = &overlaps[0];
    assert!(
        shared.fade_in.len() >= 8,
        "a curve needs points to be a curve"
    );
    assert_eq!(shared.fade_out.len(), shared.fade_in.len());

    let block = clip_rect(&v, g, &clips[0]);
    let (_, content) = clip_bands(block);
    // The later clip rises out of silence; the earlier one falls into it.
    assert!(gain_at(content, shared.fade_in[0]).abs() < 0.01);
    assert!((gain_at(content, *shared.fade_in.last().unwrap()) - 1.0).abs() < 0.01);
    assert!((gain_at(content, shared.fade_out[0]) - 1.0).abs() < 0.01);
    assert!(gain_at(content, *shared.fade_out.last().unwrap()).abs() < 0.01);
    // Monotonic, both of them: a crossfade that wobbles is not a crossfade.
    for pair in shared.fade_in.windows(2) {
        assert!(pair[1].1 <= pair[0].1 + 1e-3, "the rising curve fell");
    }
    for pair in shared.fade_out.windows(2) {
        assert!(pair[1].1 >= pair[0].1 - 1e-3, "the falling curve rose");
    }
}

#[test]
fn the_curves_span_the_overlap_and_no_further() {
    let v = view();
    let g = grid();
    let clips = audio_pair();
    let shared = &clip_overlaps(&v, g, &clips)[0];
    let from = timeline_tick_to_x(&v, g, BAR);
    let to = timeline_tick_to_x(&v, g, BAR * 2);
    for curve in [&shared.fade_in, &shared.fade_out] {
        assert!((curve[0].0 - from).abs() < 0.01, "starts at {}", curve[0].0);
        let last = curve.last().unwrap().0;
        assert!((last - to).abs() < 0.01, "ends at {last}");
    }
}

#[test]
fn the_curves_cross_in_the_middle_and_hold_a_constant_power() {
    // The whole reason for the shape: what the one loses the other gains, so
    // the pair never dips. Two different recordings blended linearly do.
    let v = view();
    let g = grid();
    let clips = audio_pair();
    let shared = &clip_overlaps(&v, g, &clips)[0];
    let (_, content) = clip_bands(clip_rect(&v, g, &clips[0]));
    for (a, b) in shared.fade_in.iter().zip(&shared.fade_out) {
        assert!(
            (a.0 - b.0).abs() < 0.01,
            "the curves are drawn at different x"
        );
        let (up, down) = (gain_at(content, *a), gain_at(content, *b));
        assert!(
            (up * up + down * down - 1.0).abs() < 0.01,
            "power {up} and {down}"
        );
    }
    // And they cross halfway, at three decibels down apiece.
    let middle = shared.fade_in.len() / 2;
    let up = gain_at(content, shared.fade_in[middle]);
    let down = gain_at(content, shared.fade_out[middle]);
    assert!(
        (up - down).abs() < 0.02,
        "they do not cross: {up} and {down}"
    );
    assert!(
        (up - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.02,
        "crossing at {up}"
    );
}

#[test]
fn the_curves_live_in_the_content_band_like_every_other_fade() {
    let v = view();
    let g = grid();
    let clips = audio_pair();
    let shared = &clip_overlaps(&v, g, &clips)[0];
    let (header, content) = clip_bands(clip_rect(&v, g, &clips[0]));
    for point in shared.fade_in.iter().chain(&shared.fade_out) {
        assert!(
            point.1 >= content.y - 0.01 && point.1 <= content.bottom() + 0.01,
            "a curve point at {} is outside the content band",
            point.1
        );
        assert!(
            point.1 >= header.bottom() - 0.01,
            "a curve crosses the caption"
        );
    }
}

#[test]
fn blocks_that_are_not_both_audio_are_striped_and_not_faded() {
    // A note block over an audio one overlaps and does not crossfade — the
    // sequencer places no crossfade there either, and what is drawn is what
    // is heard.
    for kind in [ClipKind::Notes, ClipKind::Automation] {
        let mut clips = audio_pair();
        clips[1].kind = kind;
        let shared = &clip_overlaps(&view(), grid(), &clips)[0];
        assert!(!shared.area.is_empty(), "{kind:?} should still be striped");
        assert!(shared.fade_in.is_empty(), "{kind:?} drew a crossfade");
        assert!(shared.fade_out.is_empty(), "{kind:?} drew a crossfade");
    }
}

#[test]
fn a_muted_clip_crossfades_with_nothing() {
    // It is not placed, so nothing is faded against it: the clip beside it
    // plays plain, as it would with the muted one deleted.
    let mut clips = audio_pair();
    clips[0].muted = true;
    let shared = &clip_overlaps(&view(), grid(), &clips)[0];
    assert!(
        !shared.area.is_empty(),
        "a muted overlap is still an overlap"
    );
    assert!(shared.fade_in.is_empty() && shared.fade_out.is_empty());
}

#[test]
fn a_clip_wholly_inside_another_only_rises() {
    // The compiler's rule, drawn: the inner clip comes in over the overlap,
    // and the outer one has no end inside it to fade out over.
    let v = view();
    let g = grid();
    let clips = clips(&[(0, 0, BAR * 4), (0, BAR, BAR)]);
    let shared = &clip_overlaps(&v, g, &clips)[0];
    assert!(!shared.fade_in.is_empty());
    assert!(
        shared.fade_out.is_empty(),
        "the outer clip does not end here"
    );
}

#[test]
fn a_clips_own_fade_pulls_its_crossfade_curve_down() {
    // Both apply in the player — the crossfade is about the clip beside it
    // and the clip's own fade goes with it wherever it is put — so both
    // apply in the picture.
    let v = view();
    let g = grid();
    let plain = clip_overlaps(&v, g, &audio_pair())[0].fade_in.clone();
    let mut clips = audio_pair();
    clips[1].audio.fade_in = 1.0; // the later clip fades in across all of itself
    let faded = clip_overlaps(&v, g, &clips)[0].fade_in.clone();
    let middle = plain.len() / 2;
    assert!(
        faded[middle].1 > plain[middle].1 + 1.0,
        "the clip's own fade was ignored: {} against {}",
        faded[middle].1,
        plain[middle].1
    );
}
