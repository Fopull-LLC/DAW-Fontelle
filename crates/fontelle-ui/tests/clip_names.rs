//! A clip's name, in the middle of its block, and the menu it opens.
//!
//! Ty: *"there should be a name on each clip that is in the center of the
//! clip so its not overlapping any of the end of clip controls like looping
//! extending etc. and make it so you can click that name to open a little
//! menu thats like a right click menu and in it gives you a bunch of options,
//! like rendering just that clip into audio, renaming it, and a new option for
//! audio, analyze musically."*

use fontelle_types::{ClipId, PPQN};
use fontelle_ui::canvas::{
    ClipMenuRow, ClipPart, TimelineHit, TimelineView, clip_caption, clip_grip, clip_menu,
    clip_name_hit, clip_name_rect, clip_name_slot, clip_rect, fade_anatomy, timeline_hit,
    timeline_layout,
};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn body() -> Rect {
    Rect::new(0.0, 0.0, 900.0, 300.0)
}

fn view() -> TimelineView {
    TimelineView {
        pixels_per_tick: 0.05,
        ..TimelineView::default()
    }
}

fn a_clip(kind: ClipKind, length: i64) -> ClipInfo {
    let mut arena: fontelle_model::Arena<ClipId, ()> = fontelle_model::Arena::default();
    ClipInfo {
        id: arena.insert(()),
        lane: 0,
        start: PPQN,
        length,
        name: "Grand Piano".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind,
        curve: Vec::new(),
        notes: Vec::new(),
        audio: Default::default(),
        prefab: None,
    }
}

/// Seven pixels a character: what the tests measure text with.
fn measure(text: &str) -> f32 {
    text.chars().count() as f32 * 7.0
}

// ---------------------------------------------------------- the caption ---

#[test]
fn a_name_that_fits_is_written_whole() {
    assert_eq!(
        clip_caption("Grand Piano", 200.0, measure),
        Some("Grand Piano".to_string())
    );
}

#[test]
fn a_name_too_long_is_cut_short_with_an_ellipsis_that_fits() {
    let caption = clip_caption("Grand Piano Layered Strings", 80.0, measure).unwrap();
    assert!(caption.ends_with('\u{2026}'), "{caption}");
    assert!(measure(&caption) <= 80.0, "{caption}");
    assert!(caption.starts_with("Grand"), "{caption}");
}

#[test]
fn a_block_too_narrow_for_a_word_shows_no_name() {
    assert_eq!(clip_caption("Grand Piano", 12.0, measure), None);
    assert_eq!(clip_caption("", 200.0, measure), None);
}

// ---------------------------------------------------- where it is drawn ---

#[test]
fn the_name_sits_between_the_edge_grips_and_the_fade_handles() {
    let l = timeline_layout(body(), &Theme::dark_default().metrics);
    for kind in [ClipKind::Notes, ClipKind::Audio, ClipKind::Automation] {
        let clip = a_clip(kind, PPQN * 8);
        let block = clip_rect(&view(), l.grid, &clip);
        let slot = clip_name_slot(block, l.grid, &clip);
        let grip = clip_grip(block);
        assert!(slot.right() <= block.right() - grip, "{kind:?}: {slot:?}");
        if kind == ClipKind::Audio {
            assert!(slot.x >= block.x + grip, "{kind:?}: {slot:?}");
            let fades = fade_anatomy(block, &clip).expect("an audio block has fades");
            assert!(!slot.intersects(&fades.handle_in), "{kind:?}");
            assert!(!slot.intersects(&fades.handle_out), "{kind:?}");
        }
        let name = clip_name_rect(slot, 70.0).expect("room for it");
        assert!(
            (name.x + name.width / 2.0 - (slot.x + slot.width / 2.0)).abs() < 0.5,
            "{kind:?}: centred"
        );
    }
}

#[test]
fn a_name_wider_than_its_room_has_no_place() {
    let slot = Rect::new(0.0, 0.0, 50.0, 14.0);
    assert_eq!(clip_name_rect(slot, 60.0), None);
}

// --------------------------------------------------------- pressing it ---

#[test]
fn pressing_the_name_is_the_name_and_the_grips_are_still_the_grips() {
    let l = timeline_layout(body(), &Theme::dark_default().metrics);
    let clip = a_clip(ClipKind::Audio, PPQN * 8);
    let clips = vec![clip.clone()];
    let block = clip_rect(&view(), l.grid, &clip);
    let name = clip_name_rect(clip_name_slot(block, l.grid, &clip), 70.0).unwrap();
    let width = |_: &ClipInfo| Some(70.0);
    let (cx, cy) = (name.x + name.width / 2.0, name.y + name.height / 2.0);
    assert_eq!(
        clip_name_hit(&view(), &l, &clips, width, cx, cy),
        Some(clip.id)
    );
    // The right-hand grip: a resize, not the menu.
    let (gx, gy) = (block.right() - 2.0, block.y + block.height / 2.0);
    assert_eq!(clip_name_hit(&view(), &l, &clips, width, gx, gy), None);
    assert_eq!(
        timeline_hit(&view(), &l, &clips, gx, gy),
        TimelineHit::Clip(clip.id, ClipPart::RightEdge)
    );
    // Away from the name, in the body: not the name.
    assert_eq!(
        clip_name_hit(
            &view(),
            &l,
            &clips,
            width,
            block.x + 30.0,
            block.bottom() - 3.0
        ),
        None
    );
    // A block with no name drawn has nothing to press.
    assert_eq!(clip_name_hit(&view(), &l, &clips, |_| None, cx, cy), None);
}

// ------------------------------------------------------------ the menu ---

fn labels(entries: &[fontelle_ui::canvas::MenuEntry]) -> Vec<String> {
    entries.iter().map(|e| e.label.clone()).collect()
}

#[test]
fn every_clip_can_be_renamed_rendered_duplicated_and_deleted() {
    for kind in [ClipKind::Notes, ClipKind::Audio, ClipKind::Automation] {
        let (entries, rows) = clip_menu(&a_clip(kind, PPQN * 4));
        assert_eq!(entries.len(), rows.len());
        assert_eq!(rows[0], ClipMenuRow::Heading);
        assert_eq!(entries[0].label, "Grand Piano", "the menu says whose it is");
        assert!(!entries[0].enabled);
        for row in [
            ClipMenuRow::Rename,
            ClipMenuRow::Render,
            ClipMenuRow::Duplicate,
            ClipMenuRow::Delete,
            ClipMenuRow::Mute,
            ClipMenuRow::Loop,
        ] {
            assert!(rows.contains(&row), "{kind:?} has no {row:?}");
        }
        let all = labels(&entries);
        for label in ["Rename\u{2026}", "Render to audio", "Duplicate", "Delete"] {
            assert!(all.iter().any(|l| l == label), "{kind:?}: {all:?}");
        }
    }
}

#[test]
fn only_an_audio_clip_is_analysed_musically() {
    let (_, rows) = clip_menu(&a_clip(ClipKind::Audio, PPQN * 4));
    assert!(rows.contains(&ClipMenuRow::AnalyzeMusically));
    let (entries, _) = clip_menu(&a_clip(ClipKind::Audio, PPQN * 4));
    assert!(labels(&entries).iter().any(|l| l == "Analyze Musically"));
    for kind in [ClipKind::Notes, ClipKind::Automation] {
        let (_, rows) = clip_menu(&a_clip(kind, PPQN * 4));
        assert!(!rows.contains(&ClipMenuRow::AnalyzeMusically), "{kind:?}");
    }
}

#[test]
fn the_switches_say_what_pressing_them_would_do() {
    let mut clip = a_clip(ClipKind::Notes, PPQN * 8);
    let (entries, rows) = clip_menu(&clip);
    let label = |row| {
        entries[rows.iter().position(|r| *r == row).unwrap()]
            .label
            .clone()
    };
    assert_eq!(label(ClipMenuRow::Mute), "Mute");
    assert_eq!(label(ClipMenuRow::Loop), "Loop");
    clip.muted = true;
    clip.loop_length = Some(PPQN * 4);
    let (entries, rows) = clip_menu(&clip);
    let label = |row| {
        entries[rows.iter().position(|r| *r == row).unwrap()]
            .label
            .clone()
    };
    assert_eq!(label(ClipMenuRow::Mute), "Unmute");
    assert_eq!(label(ClipMenuRow::Loop), "Stop looping");
}
