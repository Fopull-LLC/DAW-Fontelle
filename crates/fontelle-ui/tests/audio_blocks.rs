//! The waveform inside an audio clip (TDD §15.3).
//!
//! Reported from using the window:
//!
//! > *"i should be able to see the waveform of the audio inside the clip."*
//!
//! The same idea the note preview already answers — *"make it so the midi clips
//! in the arrangement arent just blank rectangles"* — pointed at the other kind
//! of clip, and it lands in the same two bands: a caption across the top, the
//! content under it. One picture rather than two.
//!
//! Drawing a waveform is not drawing samples. A four-bar take is four hundred
//! thousand frames and the block is three hundred pixels wide, so what reaches
//! the canvas is one **column per pixel**, each holding the loudest and
//! quietest sample in that pixel's worth of time. `fontelle-assets` computes
//! those; this decides where they go.
//!
//! Everything here is geometry and therefore pure, which is what lets *"it
//! lines up with the ruler above it"* be a test rather than something to squint
//! at.

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{TimelineView, clip_bands, clip_rect, clip_waveform, timeline_layout};
use fontelle_ui::document::{AudioPreview, ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

const BAR: Tick = PPQN * 4;

fn view() -> TimelineView {
    TimelineView {
        pixels_per_tick: 0.05,
        lane_height: 40.0,
        ..TimelineView::default()
    }
}

fn grid() -> Rect {
    let m = Theme::dark_default().metrics;
    timeline_layout(Rect::new(0.0, 0.0, 1200.0, 300.0), &m).grid
}

/// An audio clip `length` long whose waveform is `peaks`.
fn clip(length: Tick, peaks: Vec<(f32, f32)>) -> ClipInfo {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    ClipInfo {
        id: arena.insert(()),
        lane: 0,
        start: 0,
        length,
        name: "Take".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind: ClipKind::Audio,
        curve: Vec::new(),
        notes: Vec::new(),
        audio: AudioPreview {
            peaks,
            ..AudioPreview::default()
        },
        prefab: None,
    }
}

/// A flat block of `n` buckets all at `level`.
fn flat(n: usize, level: f32) -> Vec<(f32, f32)> {
    vec![(-level, level); n]
}

#[test]
fn an_audio_clip_is_a_third_kind_of_block_and_says_so() {
    // The renderer, the hit-test and the editor all switch on this. A kind
    // that defaulted to Notes would draw a take as an empty note preview, which
    // is a blank rectangle — the exact thing that was reported.
    assert_ne!(ClipKind::Audio, ClipKind::Notes);
    assert_ne!(ClipKind::Audio, ClipKind::Automation);
}

#[test]
fn the_waveform_fills_the_content_band_and_never_the_caption() {
    // A waveform drawn across the name is a name you cannot read over a
    // waveform you cannot follow. The same rule the automation curve keeps.
    let c = clip(PPQN * 16, flat(64, 1.0));
    let block = clip_rect(&view(), grid(), &c);
    let (header, content) = clip_bands(block);
    let columns = clip_waveform(block, grid(), &c);

    assert!(!columns.is_empty(), "an audio clip drew no waveform at all");
    for column in &columns {
        assert!(
            !column.intersects(&header),
            "the waveform is over the caption"
        );
        assert!(
            column.y >= content.y - 0.01 && column.bottom() <= content.bottom() + 0.01,
            "a column {column:?} escapes the content band {content:?}"
        );
    }
}

#[test]
fn the_waveform_spans_the_whole_block_from_end_to_end() {
    // A picture that stops short says the take stops short.
    let c = clip(PPQN * 16, flat(200, 0.8));
    let block = clip_rect(&view(), grid(), &c);
    let columns = clip_waveform(block, grid(), &c);
    let left = columns.iter().map(|r| r.x).fold(f32::MAX, f32::min);
    let right = columns.iter().map(|r| r.right()).fold(f32::MIN, f32::max);
    let (_, content) = clip_bands(block);
    assert!(
        (left - content.x).abs() < 2.0,
        "it starts at {left}, band at {}",
        content.x
    );
    assert!(
        (right - content.right()).abs() < 2.0,
        "it ends at {right}, band at {}",
        content.right()
    );
}

#[test]
fn a_loud_take_is_drawn_taller_than_a_quiet_one() {
    // The one thing a waveform is *for*. A picture that ignored the values
    // would be a rectangle with extra steps.
    let block = clip_rect(&view(), grid(), &clip(PPQN * 16, Vec::new()));
    let loud = clip_waveform(block, grid(), &clip(PPQN * 16, flat(64, 1.0)));
    let quiet = clip_waveform(block, grid(), &clip(PPQN * 16, flat(64, 0.1)));

    let tallest = |cols: &[Rect]| cols.iter().map(|r| r.height).fold(0.0f32, f32::max);
    assert!(
        tallest(&loud) > tallest(&quiet) * 2.0,
        "loud {} against quiet {}",
        tallest(&loud),
        tallest(&quiet)
    );
}

#[test]
fn silence_is_drawn_as_a_line_rather_than_as_nothing() {
    // A gap in a take has to look like part of the take. Nothing at all reads
    // as "the clip ends here".
    let c = clip(PPQN * 16, flat(64, 0.0));
    let block = clip_rect(&view(), grid(), &c);
    let columns = clip_waveform(block, grid(), &c);
    assert!(!columns.is_empty());
    for column in &columns {
        assert!(column.height > 0.0, "a silent column has no height at all");
    }
}

#[test]
fn a_waveform_is_centred_on_the_middle_of_the_band() {
    // Zero is the middle. A picture hung off the top is one where a quiet take
    // and a loud one look the same shape.
    let c = clip(PPQN * 16, flat(64, 1.0));
    let block = clip_rect(&view(), grid(), &c);
    let (_, content) = clip_bands(block);
    let middle = content.y + content.height / 2.0;
    for column in clip_waveform(block, grid(), &c) {
        let centre = column.y + column.height / 2.0;
        assert!(
            (centre - middle).abs() < 1.0,
            "a column centred at {centre}, band centre {middle}"
        );
    }
}

#[test]
fn a_block_with_no_peaks_yet_draws_nothing_rather_than_a_slab() {
    // §15.3: *"display must remain responsive while peaks are still
    // generating — draw what exists"*. Nothing is what exists.
    let c = clip(PPQN * 16, Vec::new());
    let block = clip_rect(&view(), grid(), &c);
    assert!(clip_waveform(block, grid(), &c).is_empty());
}

#[test]
fn a_note_clip_has_no_waveform() {
    let mut c = clip(PPQN * 16, flat(64, 1.0));
    c.kind = ClipKind::Notes;
    let block = clip_rect(&view(), grid(), &c);
    assert!(clip_waveform(block, grid(), &c).is_empty());
}

#[test]
fn a_block_squeezed_to_nothing_draws_nothing_rather_than_negative_rectangles() {
    let c = clip(PPQN * 16, flat(64, 1.0));
    for block in [
        Rect::new(0.0, 0.0, 0.0, 0.0),
        Rect::new(10.0, 10.0, 400.0, 2.0),
        Rect::new(10.0, 10.0, 1.0, 40.0),
    ] {
        for column in clip_waveform(block, grid(), &c) {
            assert!(
                column.width >= 0.0 && column.height >= 0.0,
                "{block:?} gave {column:?}"
            );
        }
    }
}

#[test]
fn only_the_part_of_a_long_block_that_is_on_screen_is_built() {
    // The same rule the note preview follows. A twenty-minute take scrolled
    // mostly off screen must not cost twenty minutes of columns per frame.
    let c = clip(PPQN * 4000, flat(4096, 0.5));
    let block = clip_rect(&view(), grid(), &c);
    let columns = clip_waveform(block, grid(), &c);
    for column in &columns {
        assert!(
            column.right() >= grid().x - 2.0 && column.x <= grid().right() + 2.0,
            "a column at {column:?} is off the grid {:?}",
            grid()
        );
    }
    assert!(
        columns.len() as f32 <= grid().width + 2.0,
        "{} columns for a {}-pixel grid",
        columns.len(),
        grid().width
    );
}

#[test]
fn the_fades_shape_the_picture_the_way_they_shape_the_sound() {
    // *"changing a fade in or fade out"* — and what you see has to be what you
    // hear, or the editor is a set of numbers with a decoration beside it.
    let mut c = clip(PPQN * 16, flat(128, 1.0));
    c.audio.fade_in = 0.5;
    let block = clip_rect(&view(), grid(), &c);
    let columns = clip_waveform(block, grid(), &c);
    let first = columns.first().expect("columns").height;
    let middle = columns[columns.len() / 2].height;
    assert!(
        first < middle / 2.0,
        "the fade in is not drawn: {first} against {middle}"
    );
}

// --------------------------------------------------------- cutting one ---

/// *"should work cleanly with all the tools like cutting and whatnot."*
///
/// The blade is kind-blind by design — it asks where a stroke crosses a row's
/// middle and nothing else — but "by design" is worth a test, because an audio
/// clip is the one kind that arrived after the tool did.
#[test]
fn the_blade_cuts_a_take_like_it_cuts_anything_else() {
    use fontelle_ui::canvas::{SnapDivision, clip_cuts, lane_to_y};

    let mut c = clip(BAR, flat(64, 0.5));
    c.lane = 1;
    let clips = vec![c];
    let (v, grid) = (view(), grid());
    let row = lane_to_y(&v, grid, 1) + v.lane_height / 2.0;
    let x = fontelle_ui::canvas::timeline_tick_to_x(&v, grid, BAR / 2);

    let cuts = clip_cuts(
        &v,
        grid,
        &clips,
        (x, row - v.lane_height),
        (x, row + v.lane_height),
        SnapDivision::Step,
        4,
    );
    assert_eq!(cuts.len(), 1, "the blade passed straight through a take");
    assert_eq!(cuts[0].0, clips[0].id);
    assert!(cuts[0].1 > 0 && cuts[0].1 < BAR, "it cut at {}", cuts[0].1);
}

#[test]
fn a_stroke_that_starts_below_a_take_still_crosses_it() {
    // Which is the case the window found: the blade is dragged downwards from
    // the row above, and what matters is that the row's middle is somewhere
    // between the two ends of the stroke.
    use fontelle_ui::canvas::{SnapDivision, clip_cuts, lane_to_y};

    let mut c = clip(BAR, flat(64, 0.5));
    c.lane = 1;
    let clips = vec![c];
    let (v, grid) = (view(), grid());
    let row = lane_to_y(&v, grid, 1) + v.lane_height / 2.0;
    let x = fontelle_ui::canvas::timeline_tick_to_x(&v, grid, BAR / 2);
    let cuts = clip_cuts(
        &v,
        grid,
        &clips,
        (x, row + v.lane_height),
        (x, row - v.lane_height),
        SnapDivision::Step,
        4,
    );
    assert_eq!(cuts.len(), 1, "a stroke drawn upwards cuts nothing");
}
