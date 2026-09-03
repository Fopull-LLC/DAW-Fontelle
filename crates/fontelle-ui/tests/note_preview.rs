//! A note clip shows the notes that are in it (TDD §16.4).
//!
//! Reported from using the window: *"make it so the midi clips in the
//! arrangement arent just blank rectangles but instead actually show a preview
//! of the notes drawn out inside of it like how other daws do. ensure it
//! actually displays cleanly so the sections actually line up with what youre
//! editing."*
//!
//! Two claims are doing the work here, and the second is the one that is easy
//! to get wrong:
//!
//! - **It lines up.** A note at bar 3 of the clip is drawn at bar 3 of the
//!   block, against the same axis the ruler above it is drawn against. The
//!   preview is not a decoration; it is how you find the bar you meant.
//! - **A looped clip shows every pass**, in the same places the seams say
//!   they are (`loop_marks`), because one clip whose content repeats is not
//!   several copies and a preview that drew the pattern once would say it was.

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    NOTE_PREVIEW_MIN_KEYS, TimelineView, clip_bands, clip_notes, clip_rect, loop_marks,
    timeline_layout,
};
use fontelle_ui::document::{ClipInfo, ClipKind, NotePreview};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

const BAR: Tick = PPQN * 4;

fn view() -> TimelineView {
    TimelineView {
        // A bar is 192 pixels: wide enough to see a sixteenth.
        pixels_per_tick: 0.05,
        lane_height: 40.0,
        ..TimelineView::default()
    }
}

fn grid() -> Rect {
    let m = Theme::dark_default().metrics;
    timeline_layout(Rect::new(0.0, 0.0, 1200.0, 300.0), &m).grid
}

fn note(start: Tick, length: Tick, key: u8) -> NotePreview {
    NotePreview { start, length, key }
}

/// A clip on lane 0 from bar 1, `length` long, holding `notes`.
fn clip(length: Tick, loop_length: Option<Tick>, notes: Vec<NotePreview>) -> ClipInfo {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    ClipInfo {
        id: arena.insert(()),
        lane: 0,
        start: 0,
        length,
        name: "Keys".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length,
        kind: ClipKind::Notes,
        curve: Vec::new(),
        notes,
        audio: Default::default(),
    }
}

// ------------------------------------------------------------ the bands ---

#[test]
fn a_clip_block_is_a_caption_band_over_the_room_its_content_gets() {
    // The same two bands an automation block has, and deliberately: a note
    // clip and an automation clip are two kinds of one thing, and a caption
    // sitting over the content of one and beside the content of the other is
    // two pictures where there should be one.
    let block = Rect::new(100.0, 40.0, 400.0, 40.0);
    let (header, content) = clip_bands(block);

    assert!(!header.is_empty() && !content.is_empty());
    assert!(header.bottom() <= content.y + 0.001, "the band is above");
    assert!(!header.intersects(&content));
    assert_eq!(header.union(&content), block, "and together they are the block");
    assert!(content.height > header.height, "the content gets most of it");
}

#[test]
fn a_block_squeezed_to_nothing_yields_empty_bands_never_negative_ones() {
    for (w, h) in [(0.0, 0.0), (1.0, 1.0), (400.0, 3.0), (2.0, 40.0)] {
        let (header, content) = clip_bands(Rect::new(0.0, 0.0, w, h));
        for r in [header, content] {
            assert!(r.width >= 0.0 && r.height >= 0.0, "{w}x{h} gave {r:?}");
        }
    }
}

// -------------------------------------------------------- lining up ---

#[test]
fn a_note_is_drawn_at_the_bar_it_is_written_at() {
    // The claim the whole feature rests on. A note starting at bar 3 of a
    // four-bar clip is drawn three quarters of the way along it — against the
    // block's own time axis, which is the ruler's.
    let c = clip(BAR * 4, None, vec![note(BAR * 3, PPQN, 60)]);
    let block = clip_rect(&view(), grid(), &c);
    let rects = clip_notes(block, grid(), &c);

    assert_eq!(rects.len(), 1);
    let (_, content) = clip_bands(block);
    let expected_x = content.x + content.width * 0.75;
    assert!(
        (rects[0].x - expected_x).abs() < 1.0,
        "a note at bar 3 of four is at {} and should be at {expected_x}",
        rects[0].x
    );
    // And it is a quarter note long, which is a sixteenth of the clip.
    assert!(
        (rects[0].width - content.width / 16.0).abs() < 1.0,
        "a quarter note in a four-bar clip is a sixteenth of it, got {}",
        rects[0].width
    );
}

#[test]
fn a_notes_length_is_how_wide_it_is_drawn() {
    let c = clip(
        BAR * 4,
        None,
        vec![note(0, PPQN, 60), note(BAR, BAR, 60)],
    );
    let block = clip_rect(&view(), grid(), &c);
    let rects = clip_notes(block, grid(), &c);
    assert_eq!(rects.len(), 2);
    assert!(
        rects[1].width > rects[0].width * 3.5,
        "a bar-long note is four times a beat-long one: {:?}",
        rects
    );
}

#[test]
fn a_note_is_never_thinner_than_a_pixel() {
    // A sixteenth at a zoom where the whole clip is 40 pixels rounds to
    // nothing, and a note you cannot see is a clip that looks empty.
    let c = clip(BAR * 64, None, vec![note(0, PPQN / 4, 60)]);
    let mut v = view();
    v.pixels_per_tick = 0.0005;
    let block = clip_rect(&v, grid(), &c);
    let rects = clip_notes(block, grid(), &c);
    assert_eq!(rects.len(), 1);
    assert!(rects[0].width >= 1.0, "got {:?}", rects[0]);
    assert!(rects[0].height >= 1.0, "got {:?}", rects[0]);
}

// ------------------------------------------------------------- pitch ---

#[test]
fn a_higher_note_is_drawn_higher() {
    let c = clip(BAR, None, vec![note(0, PPQN, 48), note(PPQN, PPQN, 72)]);
    let block = clip_rect(&view(), grid(), &c);
    let rects = clip_notes(block, grid(), &c);
    assert_eq!(rects.len(), 2);
    assert!(
        rects[1].y < rects[0].y,
        "key 72 must sit above key 48: {rects:?}"
    );
}

#[test]
fn every_note_stays_inside_the_blocks_content_band() {
    // A preview that escaped its block would be drawn over the lane above,
    // which is the one mistake that makes an arrangement unreadable.
    let keys = [0u8, 12, 60, 100, 127];
    let notes: Vec<NotePreview> = keys
        .iter()
        .enumerate()
        .map(|(i, key)| note(PPQN * i as Tick, PPQN, *key))
        .collect();
    let c = clip(BAR * 2, None, notes);
    let block = clip_rect(&view(), grid(), &c);
    let (_, content) = clip_bands(block);
    for rect in clip_notes(block, grid(), &c) {
        assert!(
            rect.y >= content.y - 0.01 && rect.bottom() <= content.bottom() + 0.01,
            "{rect:?} escaped {content:?}"
        );
        assert!(
            rect.x >= content.x - 0.01 && rect.right() <= content.right() + 0.01,
            "{rect:?} escaped {content:?}"
        );
    }
}

#[test]
fn a_clip_with_one_note_does_not_draw_it_as_a_slab() {
    // Scaled to the notes present, a single note would fill the block's whole
    // height and read as a solid bar rather than as a note. The pitch axis
    // therefore has a floor.
    let c = clip(BAR, None, vec![note(0, PPQN, 60)]);
    let block = clip_rect(&view(), grid(), &c);
    let (_, content) = clip_bands(block);
    let rects = clip_notes(block, grid(), &c);
    assert!(
        rects[0].height <= content.height / (NOTE_PREVIEW_MIN_KEYS as f32) + 0.51,
        "one note filled {} of a {}-tall band",
        rects[0].height,
        content.height
    );
    const { assert!(NOTE_PREVIEW_MIN_KEYS >= 8, "an octave or so, or it is a slab") }
}

// -------------------------------------------------------------- loops ---

#[test]
fn a_looped_clip_draws_every_pass_where_its_seams_say_they_are() {
    // One clip whose content repeats is not several copies (see
    // `fontelle_model::Clip::loop_length`), and a preview that drew the
    // pattern once would say the rest of the clip was empty.
    let c = clip(BAR * 4, Some(BAR), vec![note(0, PPQN, 60)]);
    let v = view();
    let block = clip_rect(&v, grid(), &c);
    let rects = clip_notes(block, grid(), &c);
    assert_eq!(rects.len(), 4, "four bars of a one-bar pattern is four passes");

    // Every pass after the first begins on a seam, which is what makes the
    // picture and the tiling one picture rather than two.
    let seams = loop_marks(&v, grid(), &c);
    assert_eq!(seams.len(), 3);
    for (rect, seam) in rects.iter().skip(1).zip(seams.iter()) {
        assert!(
            (rect.x - seam).abs() < 1.5,
            "a pass starts at {} and its seam is at {seam}",
            rect.x
        );
    }
}

#[test]
fn a_note_past_the_loops_period_is_left_out_of_the_preview() {
    // The compiler drops it — content the loop does not contain — so drawing
    // it would be a picture of something the song does not play.
    let c = clip(
        BAR * 2,
        Some(BAR),
        vec![note(0, PPQN, 60), note(BAR + PPQN, PPQN, 64)],
    );
    let block = clip_rect(&view(), grid(), &c);
    assert_eq!(clip_notes(block, grid(), &c).len(), 2, "two passes of one note");
}

#[test]
fn a_pass_that_runs_past_the_clips_end_is_cut_there() {
    // The same rule the compiler follows: a loop that rings past its own end
    // is a loop whose last pass sounds different from the others.
    let c = clip(BAR + PPQN * 2, Some(BAR), vec![note(0, BAR, 60)]);
    let block = clip_rect(&view(), grid(), &c);
    let (_, content) = clip_bands(block);
    let rects = clip_notes(block, grid(), &c);
    assert_eq!(rects.len(), 2);
    assert!(
        rects[1].right() <= content.right() + 0.01,
        "the last pass runs out of the block: {:?} in {content:?}",
        rects[1]
    );
}

// ---------------------------------------------------- virtualisation ---

#[test]
fn passes_off_the_side_of_the_grid_are_not_built() {
    // §16.4 is explicit: build geometry for what can be seen and nothing
    // else. A two-hundred-bar loop is a screenful of rectangles or it is a
    // frame nobody can afford.
    let c = clip(BAR * 200, Some(BAR), vec![note(0, PPQN, 60)]);
    let block = clip_rect(&view(), grid(), &c);
    let all = clip_notes(block, grid(), &c);
    assert!(
        all.len() < 40,
        "a 200-bar loop built {} rectangles for a grid {} wide",
        all.len(),
        grid().width
    );
    assert!(!all.is_empty(), "and the ones on screen are there");
    for rect in &all {
        assert!(rect.intersects(&grid()), "{rect:?} is off the grid");
    }
}

// ------------------------------------------------------- degenerate ---

#[test]
fn a_clip_with_nothing_in_it_draws_nothing() {
    let c = clip(BAR, None, Vec::new());
    let block = clip_rect(&view(), grid(), &c);
    assert!(clip_notes(block, grid(), &c).is_empty());
}

#[test]
fn an_automation_block_has_no_note_preview() {
    let mut c = clip(BAR, None, vec![note(0, PPQN, 60)]);
    c.kind = ClipKind::Automation;
    let block = clip_rect(&view(), grid(), &c);
    assert!(
        clip_notes(block, grid(), &c).is_empty(),
        "an automation block draws its curve, not notes"
    );
}

#[test]
fn nothing_divides_by_a_missing_length_or_a_missing_block() {
    let mut c = clip(0, None, vec![note(0, PPQN, 60)]);
    for block in [Rect::ZERO, Rect::new(0.0, 0.0, 100.0, 2.0)] {
        for rect in clip_notes(block, grid(), &c) {
            assert!(rect.x.is_finite() && rect.y.is_finite(), "{rect:?}");
        }
    }
    c.length = BAR;
    c.loop_length = Some(0);
    let block = clip_rect(&view(), grid(), &c);
    for rect in clip_notes(block, grid(), &c) {
        assert!(rect.x.is_finite() && rect.width.is_finite(), "{rect:?}");
    }
}

#[test]
fn a_block_too_short_to_draw_a_note_in_draws_none() {
    // Zoomed out to a few pixels a row of one-pixel smudges is less readable
    // than a plain block, which is what the arrangement had before.
    let c = clip(BAR, None, vec![note(0, PPQN, 60)]);
    let mut v = view();
    v.lane_height = 5.0;
    let block = clip_rect(&v, grid(), &c);
    assert!(clip_notes(block, grid(), &c).is_empty());
}
