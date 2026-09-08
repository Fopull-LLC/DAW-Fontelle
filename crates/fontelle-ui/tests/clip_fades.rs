//! Fade handles on an audio clip's block (TDD §15.2).
//!
//! Reported from using the window:
//!
//! > *"you can manually (like fl studios clip fades) drag in from the start
//! > or end of an audio clip to create a clip fade thats just a volume fade
//! > from 0 to 1 and you can bend the control node to bend the curve like fl
//! > studios too, the fade length being however long you drag the fade in
//! > for on the clip. implement this seamlessly so it behaives nearly
//! > identically."*
//!
//! FL's anatomy, kept: a handle at each **top corner** of an audio block.
//! Drag the left one rightwards and the clip fades in over the distance
//! dragged; drag the right one leftwards and it fades out. With a fade in
//! place the handle sits where the fade ends, and a **node** appears at the
//! midpoint of the curve — drag it down and the fade holds back before
//! rising, up and it comes in early. The curve drawn on the block is the
//! shape the player uses.
//!
//! Everything here is geometry and gestures; the host turns a fraction of
//! the block into frames of the file (`fontelle-app/tests/clip_fades.rs`).

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    ArrangeEdit, ClipPart, FadeEnd, MouseButton, Timeline, TimelineHit, TimelineView, clip_bands,
    clip_rect, fade_anatomy, fade_curve, timeline_hit, timeline_layout,
};
use fontelle_ui::document::{AudioPreview, ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

const BAR: Tick = PPQN * 4;

fn view() -> TimelineView {
    TimelineView {
        pixels_per_tick: 0.05,
        lane_height: 60.0,
        ..TimelineView::default()
    }
}

fn layout() -> fontelle_ui::canvas::TimelineLayout {
    timeline_layout(
        Rect::new(0.0, 0.0, 1200.0, 300.0),
        &Theme::dark_default().metrics,
    )
}

fn a_clip(kind: ClipKind, fade_in: f32, fade_out: f32) -> ClipInfo {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    ClipInfo {
        id: arena.insert(()),
        lane: 0,
        start: BAR,
        length: BAR * 2,
        name: "Take".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind,
        curve: Vec::new(),
        notes: Vec::new(),
        audio: AudioPreview {
            peaks: vec![(-0.5, 0.5); 64],
            fade_in,
            fade_out,
            ..AudioPreview::default()
        },
        prefab: None,
    }
}

fn block_of(clip: &ClipInfo) -> Rect {
    clip_rect(&view(), layout().grid, clip)
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn hit(clips: &[ClipInfo], x: f32, y: f32) -> TimelineHit {
    timeline_hit(&view(), &layout(), clips, x, y)
}

// -------------------------------------------------------------- anatomy ---

#[test]
fn an_audio_block_has_a_fade_handle_at_each_top_corner() {
    let clip = a_clip(ClipKind::Audio, 0.0, 0.0);
    let block = block_of(&clip);
    let (header, _) = clip_bands(block);
    let anatomy = fade_anatomy(block, &clip).expect("an audio block has fades");
    assert!(
        (anatomy.handle_in.x - block.x).abs() < 0.01,
        "the in handle is at the left edge"
    );
    assert!(
        (anatomy.handle_out.right() - block.right()).abs() < 0.01,
        "the out handle is at the right edge"
    );
    for handle in [anatomy.handle_in, anatomy.handle_out] {
        assert!(handle.y >= header.y - 0.01 && handle.bottom() <= header.bottom() + 0.01);
        assert!(handle.width > 0.0 && handle.height > 0.0);
    }
    // No fade, no node.
    assert!(anatomy.node_in.is_none());
    assert!(anatomy.node_out.is_none());
}

#[test]
fn only_an_audio_block_has_fades() {
    for kind in [ClipKind::Notes, ClipKind::Automation] {
        let clip = a_clip(kind, 0.0, 0.0);
        assert!(
            fade_anatomy(block_of(&clip), &clip).is_none(),
            "{kind:?} grew fade handles"
        );
    }
}

#[test]
fn with_a_fade_in_place_the_handle_sits_where_the_fade_ends() {
    let clip = a_clip(ClipKind::Audio, 0.5, 0.25);
    let block = block_of(&clip);
    let anatomy = fade_anatomy(block, &clip).unwrap();
    let (x_in, _) = centre(anatomy.handle_in);
    assert!(
        (x_in - (block.x + block.width * 0.5)).abs() < 1.0,
        "in handle at {x_in}"
    );
    let (x_out, _) = centre(anatomy.handle_out);
    assert!(
        (x_out - (block.right() - block.width * 0.25)).abs() < 1.0,
        "out handle at {x_out}"
    );
}

#[test]
fn a_fade_has_a_node_at_the_midpoint_of_its_curve() {
    let clip = a_clip(ClipKind::Audio, 0.5, 0.0);
    let block = block_of(&clip);
    let (_, content) = clip_bands(block);
    let anatomy = fade_anatomy(block, &clip).unwrap();
    let node = anatomy.node_in.expect("a fade has a node");
    let (x, y) = centre(node);
    // Halfway along the fade, and — unbent — halfway up the content band.
    assert!(
        (x - (block.x + block.width * 0.25)).abs() < 1.0,
        "node at x {x}"
    );
    assert!(
        (y - (content.bottom() - content.height * 0.5)).abs() < 1.0,
        "node at y {y}"
    );
    assert!(anatomy.node_out.is_none());
}

#[test]
fn a_bent_fade_puts_its_node_where_the_curve_is() {
    let mut clip = a_clip(ClipKind::Audio, 0.5, 0.0);
    clip.audio.fade_in_tension = 0.8;
    let block = block_of(&clip);
    let (_, content) = clip_bands(block);
    let node = fade_anatomy(block, &clip).unwrap().node_in.unwrap();
    let (_, y) = centre(node);
    // Held back: the midpoint gain is under a half, so the node is lower.
    assert!(
        y > content.bottom() - content.height * 0.5 + 1.0,
        "node at y {y}"
    );
}

// ------------------------------------------------------------ the curve ---

#[test]
fn the_curve_runs_from_silence_at_the_corner_to_full_where_the_fade_ends() {
    let clip = a_clip(ClipKind::Audio, 0.5, 0.0);
    let block = block_of(&clip);
    let (_, content) = clip_bands(block);
    let points = fade_curve(block, &clip, FadeEnd::In);
    assert!(points.len() >= 8, "a curve needs points to be a curve");
    let first = points[0];
    let last = *points.last().unwrap();
    assert!((first.0 - content.x).abs() < 0.01 && (first.1 - content.bottom()).abs() < 0.01);
    assert!(
        (last.0 - (content.x + content.width * 0.5)).abs() < 0.5
            && (last.1 - content.y).abs() < 0.01
    );
    // Rising all the way: x grows, y shrinks.
    for pair in points.windows(2) {
        assert!(pair[1].0 >= pair[0].0 && pair[1].1 <= pair[0].1 + 1e-4);
    }
}

#[test]
fn the_out_curve_is_the_mirror_and_a_clip_with_no_fade_has_no_curve() {
    let clip = a_clip(ClipKind::Audio, 0.0, 0.5);
    let block = block_of(&clip);
    let (_, content) = clip_bands(block);
    assert!(fade_curve(block, &clip, FadeEnd::In).is_empty());
    let points = fade_curve(block, &clip, FadeEnd::Out);
    let first = points[0];
    let last = *points.last().unwrap();
    assert!((first.0 - (content.right() - content.width * 0.5)).abs() < 0.5);
    assert!(
        (first.1 - content.y).abs() < 0.01,
        "full where the fade starts"
    );
    assert!((last.0 - content.right()).abs() < 0.01 && (last.1 - content.bottom()).abs() < 0.01);
}

#[test]
fn the_curve_bends_with_the_tension() {
    let plain = a_clip(ClipKind::Audio, 0.5, 0.0);
    let mut held = a_clip(ClipKind::Audio, 0.5, 0.0);
    held.audio.fade_in_tension = 0.8;
    let block = block_of(&plain);
    let a = fade_curve(block, &plain, FadeEnd::In);
    let b = fade_curve(block, &held, FadeEnd::In);
    let mid = a.len() / 2;
    assert!(
        b[mid].1 > a[mid].1,
        "the held-back curve is lower in the middle"
    );
}

// -------------------------------------------------------------- hits ---

#[test]
fn the_corners_of_an_audio_block_are_its_fade_handles_and_the_rest_is_as_before() {
    let clip = a_clip(ClipKind::Audio, 0.0, 0.0);
    let clips = vec![clip.clone()];
    let block = block_of(&clip);
    let (header, content) = clip_bands(block);
    let anatomy = fade_anatomy(block, &clip).unwrap();
    let (x, y) = centre(anatomy.handle_in);
    assert_eq!(
        hit(&clips, x, y),
        TimelineHit::Clip(clip.id, ClipPart::FadeHandle(FadeEnd::In))
    );
    let (x, y) = centre(anatomy.handle_out);
    assert_eq!(
        hit(&clips, x, y),
        TimelineHit::Clip(clip.id, ClipPart::FadeHandle(FadeEnd::Out))
    );
    // The grip is still the right edge — below the caption band.
    let (_, cy) = centre(content);
    assert_eq!(
        hit(&clips, block.right() - 2.0, cy),
        TimelineHit::Clip(clip.id, ClipPart::RightEdge)
    );
    // And the middle of the caption is the body.
    let (hx, hy) = centre(header);
    assert_eq!(
        hit(&clips, hx, hy),
        TimelineHit::Clip(clip.id, ClipPart::Body)
    );
}

#[test]
fn a_fades_node_is_hit_before_the_body() {
    let clip = a_clip(ClipKind::Audio, 0.5, 0.0);
    let clips = vec![clip.clone()];
    let node = fade_anatomy(block_of(&clip), &clip)
        .unwrap()
        .node_in
        .unwrap();
    let (x, y) = centre(node);
    assert_eq!(
        hit(&clips, x, y),
        TimelineHit::Clip(clip.id, ClipPart::FadeNode(FadeEnd::In))
    );
}

#[test]
fn a_note_blocks_corners_are_still_its_body() {
    let clip = a_clip(ClipKind::Notes, 0.0, 0.0);
    let clips = vec![clip.clone()];
    let block = block_of(&clip);
    assert_eq!(
        hit(&clips, block.x + 2.0, block.y + 2.0),
        TimelineHit::Clip(clip.id, ClipPart::Body)
    );
}

// ----------------------------------------------------------- gestures ---

fn press_at(timeline: &mut Timeline, clips: &[ClipInfo], x: f32, y: f32) -> Vec<ArrangeEdit> {
    timeline.press(MouseButton::Left, x, y, &layout(), clips, 4)
}

fn drag_to(timeline: &mut Timeline, clips: &[ClipInfo], x: f32, y: f32) -> Vec<ArrangeEdit> {
    timeline.drag(x, y, &layout(), clips, 4)
}

#[test]
fn dragging_the_in_handle_fades_the_clip_in_over_the_distance_dragged() {
    let clip = a_clip(ClipKind::Audio, 0.0, 0.0);
    let clips = vec![clip.clone()];
    let block = block_of(&clip);
    let anatomy = fade_anatomy(block, &clip).unwrap();
    let (x, y) = centre(anatomy.handle_in);
    let mut timeline = Timeline::new(view());
    assert!(
        press_at(&mut timeline, &clips, x, y).is_empty(),
        "a press moves nothing yet"
    );

    let edits = drag_to(&mut timeline, &clips, block.x + block.width * 0.25, y);
    assert_eq!(edits.len(), 1);
    let ArrangeEdit::SetFade {
        clip: id,
        end,
        fraction,
    } = edits[0]
    else {
        panic!("asked for {edits:?}");
    };
    assert_eq!(id, clip.id);
    assert_eq!(end, FadeEnd::In);
    assert!((fraction - 0.25).abs() < 0.01, "fraction {fraction}");
}

#[test]
fn dragging_the_out_handle_measures_the_fade_from_the_end() {
    let clip = a_clip(ClipKind::Audio, 0.0, 0.0);
    let clips = vec![clip.clone()];
    let block = block_of(&clip);
    let anatomy = fade_anatomy(block, &clip).unwrap();
    let (x, y) = centre(anatomy.handle_out);
    let mut timeline = Timeline::new(view());
    press_at(&mut timeline, &clips, x, y);
    let edits = drag_to(&mut timeline, &clips, block.right() - block.width * 0.4, y);
    let ArrangeEdit::SetFade { end, fraction, .. } = edits[0] else {
        panic!("asked for {edits:?}");
    };
    assert_eq!(end, FadeEnd::Out);
    assert!((fraction - 0.4).abs() < 0.01, "fraction {fraction}");
}

#[test]
fn a_fade_dragged_past_the_block_is_the_whole_block_and_dragged_back_out_is_none() {
    let clip = a_clip(ClipKind::Audio, 0.0, 0.0);
    let clips = vec![clip.clone()];
    let block = block_of(&clip);
    let (x, y) = centre(fade_anatomy(block, &clip).unwrap().handle_in);
    let mut timeline = Timeline::new(view());
    press_at(&mut timeline, &clips, x, y);
    let edits = drag_to(&mut timeline, &clips, block.right() + 200.0, y);
    let ArrangeEdit::SetFade { fraction, .. } = edits[0] else {
        panic!()
    };
    assert_eq!(fraction, 1.0);
    let edits = drag_to(&mut timeline, &clips, block.x - 200.0, y);
    let ArrangeEdit::SetFade { fraction, .. } = edits[0] else {
        panic!()
    };
    assert_eq!(fraction, 0.0);
}

#[test]
fn a_stationary_pointer_asks_for_nothing_new_while_fading() {
    // The trap this codebase keeps falling into, checked here too.
    let clip = a_clip(ClipKind::Audio, 0.0, 0.0);
    let clips = vec![clip.clone()];
    let block = block_of(&clip);
    let (x, y) = centre(fade_anatomy(block, &clip).unwrap().handle_in);
    let mut timeline = Timeline::new(view());
    press_at(&mut timeline, &clips, x, y);
    let to = block.x + block.width * 0.3;
    assert_eq!(drag_to(&mut timeline, &clips, to, y).len(), 1);
    for _ in 0..3 {
        assert!(drag_to(&mut timeline, &clips, to, y).is_empty());
    }
    timeline.release();
    assert!(
        drag_to(&mut timeline, &clips, to + 50.0, y).is_empty(),
        "released"
    );
}

#[test]
fn dragging_the_node_down_holds_the_fade_back_and_up_brings_it_forward() {
    let clip = a_clip(ClipKind::Audio, 0.5, 0.0);
    let clips = vec![clip.clone()];
    let block = block_of(&clip);
    let (_, content) = clip_bands(block);
    let node = fade_anatomy(block, &clip).unwrap().node_in.unwrap();
    let (x, y) = centre(node);
    let mut timeline = Timeline::new(view());
    assert!(press_at(&mut timeline, &clips, x, y).is_empty());

    let low = content.bottom() - content.height * 0.2;
    let edits = drag_to(&mut timeline, &clips, x, low);
    let ArrangeEdit::SetFadeTension {
        clip: id,
        end,
        tension,
    } = edits[0]
    else {
        panic!("asked for {edits:?}");
    };
    assert_eq!(id, clip.id);
    assert_eq!(end, FadeEnd::In);
    assert!(
        (0.0..=1.0).contains(&tension) && tension > 0.0,
        "tension {tension}"
    );

    let high = content.bottom() - content.height * 0.8;
    let edits = drag_to(&mut timeline, &clips, x, high);
    let ArrangeEdit::SetFadeTension { tension, .. } = edits[0] else {
        panic!()
    };
    assert!((-1.0..0.0).contains(&tension), "tension {tension}");

    // Back to the middle is straight again.
    let mid = content.bottom() - content.height * 0.5;
    let edits = drag_to(&mut timeline, &clips, x, mid);
    let ArrangeEdit::SetFadeTension { tension, .. } = edits[0] else {
        panic!()
    };
    assert!(tension.abs() < 0.02, "tension {tension}");
}

#[test]
fn a_stationary_pointer_asks_for_nothing_new_while_bending() {
    let clip = a_clip(ClipKind::Audio, 0.5, 0.0);
    let clips = vec![clip.clone()];
    let block = block_of(&clip);
    let (_, content) = clip_bands(block);
    let (x, y) = centre(fade_anatomy(block, &clip).unwrap().node_in.unwrap());
    let mut timeline = Timeline::new(view());
    press_at(&mut timeline, &clips, x, y);
    let low = content.bottom() - content.height * 0.2;
    assert_eq!(drag_to(&mut timeline, &clips, x, low).len(), 1);
    for _ in 0..3 {
        assert!(drag_to(&mut timeline, &clips, x, low).is_empty());
    }
}

#[test]
fn taking_a_fade_handle_does_not_pick_the_clip_up() {
    // A fade drag moves the fade and nothing else: the clip stays put and
    // is not even opened, the way grabbing a resize grip does not open it.
    let clip = a_clip(ClipKind::Audio, 0.0, 0.0);
    let clips = vec![clip.clone()];
    let block = block_of(&clip);
    let (x, y) = centre(fade_anatomy(block, &clip).unwrap().handle_in);
    let mut timeline = Timeline::new(view());
    press_at(&mut timeline, &clips, x, y);
    let edits = drag_to(&mut timeline, &clips, x + 40.0, y + 30.0);
    assert!(
        edits
            .iter()
            .all(|e| matches!(e, ArrangeEdit::SetFade { .. })),
        "asked for {edits:?}"
    );
    assert_eq!(
        timeline.selection(),
        &[clip.id],
        "the clip is chosen, though"
    );
}

// ------------------------------------------- the curve against the file ---

/// The clip above, sized so its file exactly fills one bar of it — the shape
/// a drop makes, before anybody has dragged the block anywhere.
fn a_one_bar_take(fade_in: f32, fade_out: f32) -> ClipInfo {
    let mut clip = a_clip(ClipKind::Audio, fade_in, fade_out);
    clip.length = BAR;
    clip.audio.natural_length = BAR;
    clip
}

#[test]
fn dragging_a_loop_out_leaves_the_fades_the_length_they_were() {
    // Reported from using the window: *"extending a loop on a clip is
    // affecting the fade lengths on the clip please make it so it doesnt do
    // that."*
    //
    // A fade is frames of the **file** — that is what the handles are placed
    // against (`content_span`), what the host stores, and what the editor
    // reads out in milliseconds. The curve was measured against the *block*
    // instead, so dragging a one-bar loop out to four bars drew a fade four
    // times as long over a clip whose fade had not moved, and the curve and
    // its own handle stopped agreeing.
    let clip = a_one_bar_take(0.5, 0.25);
    let before = fade_curve(block_of(&clip), &clip, FadeEnd::In);

    let mut looped = clip.clone();
    looped.length = BAR * 4;
    looped.loop_length = Some(BAR);
    let after = fade_curve(block_of(&looped), &looped, FadeEnd::In);

    let (from, to) = (
        before.last().unwrap().0 - before[0].0,
        after.last().unwrap().0 - after[0].0,
    );
    assert!(
        (from - to).abs() < 0.5,
        "the fade was {from} points wide and the loop made it {to}"
    );
}

#[test]
fn the_curve_ends_where_its_own_handle_sits() {
    // One fact, two drawings: the handle marks where the fade ends and the
    // curve reaches full there. On a block longer than its file they used to
    // disagree, which is the visible half of the bug above.
    let mut clip = a_one_bar_take(0.5, 0.25);
    clip.length = BAR * 3;
    let block = block_of(&clip);
    let anatomy = fade_anatomy(block, &clip).unwrap();

    let end_in = fade_curve(block, &clip, FadeEnd::In).last().unwrap().0;
    assert!(
        (end_in - (anatomy.handle_in.x + anatomy.handle_in.width / 2.0)).abs() < 1.0,
        "the fade-in curve ends at {end_in} and its handle is at {:?}",
        anatomy.handle_in
    );

    let start_out = fade_curve(block, &clip, FadeEnd::Out)[0].0;
    assert!(
        (start_out - (anatomy.handle_out.x + anatomy.handle_out.width / 2.0)).abs() < 1.0,
        "the fade-out curve starts at {start_out} and its handle is at {:?}",
        anatomy.handle_out
    );
}
