//! One way to learn Fontelle: the guide, the pages behind `?`, and the tour.
//!
//! `docs/ux-routing-and-learning-plan.md` §5. Ty chose all of an interactive
//! tutorial, in-app guide pages, "Coming from…" pages and a web manual, and
//! asked that they be **one clean experience, not intrusive and not hard to
//! use**. So there is one catalogue of guide content ([`GUIDE`]) that the
//! tour draws as an overlay and the help page draws as reading, and the two
//! cannot disagree.

use fontelle_ui::canvas::{
    GUIDE, GuideChoice, GuideKind, GuideMedia, HelpHit, TourHit, help_hit, help_layout,
    help_scroll_max, help_scrolled, tour_hit, tour_layout, tour_steps,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn window() -> Rect {
    Rect::new(0.0, 0.0, 1280.0, 720.0)
}

fn within(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x - 0.01
        && inner.y >= outer.y - 0.01
        && inner.right() <= outer.right() + 0.01
        && inner.bottom() <= outer.bottom() + 0.01
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.right() - 0.01
        && b.x < a.right() - 0.01
        && a.y < b.bottom() - 0.01
        && b.y < a.bottom() - 0.01
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

// ------------------------------------------------------------ the content

#[test]
fn every_page_has_a_title_and_something_to_read() {
    for section in GUIDE {
        assert!(!section.title.is_empty());
        assert!(!section.pages.is_empty(), "{} has no pages", section.title);
        for page in section.pages {
            assert!(
                !page.title.is_empty(),
                "a page of {} has no title",
                section.title
            );
            assert!(!page.paragraphs.is_empty(), "{} is empty", page.title);
            for paragraph in page.paragraphs {
                assert!(!paragraph.trim().is_empty());
                assert!(
                    paragraph.chars().count() <= 420,
                    "{}: a paragraph is a paragraph, not a page",
                    page.title
                );
            }
        }
    }
}

#[test]
fn no_two_sections_share_a_title() {
    let mut titles: Vec<&str> = GUIDE.iter().map(|s| s.title).collect();
    titles.sort_unstable();
    let count = titles.len();
    titles.dedup();
    assert_eq!(titles.len(), count);
}

#[test]
fn there_is_a_coming_from_page_for_each_daw_ty_named() {
    // *"Coming from…" pages (Reaper, Logic, Ableton, FL)*.
    for daw in ["Reaper", "Logic", "Ableton", "FL Studio"] {
        assert!(
            GUIDE
                .iter()
                .any(|s| s.kind == GuideKind::ComingFrom && s.title.contains(daw)),
            "no Coming from {daw}"
        );
    }
}

#[test]
fn the_tour_sets_up_routing_and_vst2_once_each() {
    // *"it sets up what a user needs as it goes: their preferences (the
    // routing mode among them), and things like enabling VST 2 support"*.
    let steps = tour_steps();
    let choices: Vec<GuideChoice> = steps
        .iter()
        .filter_map(|&(s, p)| GUIDE[s].pages[p].choice)
        .collect();
    assert_eq!(
        choices
            .iter()
            .filter(|c| **c == GuideChoice::Routing)
            .count(),
        1
    );
    assert_eq!(
        choices.iter().filter(|c| **c == GuideChoice::Vst2).count(),
        1
    );
}

#[test]
fn the_tour_walks_the_tour_sections_in_order_and_nothing_else() {
    let steps = tour_steps();
    assert!(steps.len() >= 8, "a tour worth taking");
    for pair in steps.windows(2) {
        assert!(pair[0] < pair[1], "in order");
    }
    for &(s, _) in &steps {
        assert_eq!(GUIDE[s].kind, GuideKind::Tour);
    }
    assert_eq!(steps[0], (0, 0), "it starts at the start");
}

// ---------------------------------------------------------- the help page

/// Every paragraph three lines of 18 points.
fn heights(section: usize) -> Vec<Vec<f32>> {
    GUIDE[section]
        .pages
        .iter()
        .map(|page| page.paragraphs.iter().map(|_| 54.0).collect())
        .collect()
}

#[test]
fn the_help_page_lists_every_section_then_the_shortcuts() {
    let l = help_layout(window(), &metrics(), 0, &heights(0), 0.0);
    assert!(within(l.frame, window()));
    assert_eq!(l.nav.len(), GUIDE.len());
    assert!(!l.shortcuts.is_empty());
    for (a, b) in l.nav.iter().zip(l.nav.iter().skip(1)) {
        assert!(b.y >= a.bottom() - 0.01);
    }
    let last = l.nav.last().unwrap();
    assert!(l.shortcuts.y >= last.bottom() - 0.01, "shortcuts last");
    for entry in l.nav.iter().chain([&l.shortcuts]) {
        assert!(entry.right() <= l.body.x, "the list is left of the body");
    }
    assert!(within(l.close, l.frame));
}

#[test]
fn a_section_reads_as_its_pages_titles_and_paragraphs_in_order() {
    let section = 2;
    let l = help_layout(window(), &metrics(), section, &heights(section), 0.0);
    let expected: usize = GUIDE[section]
        .pages
        .iter()
        .map(|p| 1 + p.paragraphs.len() + p.media.is_some() as usize)
        .sum();
    let visible = l
        .blocks
        .iter()
        .filter(|b| b.rect.intersects(&l.body))
        .count();
    assert!(visible > 0);
    assert!(l.blocks.len() <= expected);
    for pair in l.blocks.windows(2) {
        assert!(pair[1].rect.y >= pair[0].rect.bottom() - 0.01, "stacked");
    }
    for block in &l.blocks {
        assert!(block.rect.x >= l.body.x && block.rect.right() <= l.body.right() + 0.01);
    }
    assert!(
        l.blocks[0].paragraph.is_none(),
        "a page starts with its title"
    );
}

#[test]
fn a_long_section_scrolls_within_its_ends() {
    let tall: Vec<Vec<f32>> = (0..12).map(|_| vec![200.0, 200.0]).collect();
    let l = help_layout(window(), &metrics(), 0, &tall, 0.0);
    let max = help_scroll_max(&l);
    assert!(max > 0.0);
    assert!(help_scrolled(&l, 0.0, -2.0) > 0.0);
    assert_eq!(help_scrolled(&l, 0.0, 2.0), 0.0);
    let l = help_layout(window(), &metrics(), 0, &tall, 1.0e7);
    assert_eq!(l.scroll, max);
}

#[test]
fn a_press_on_the_help_page_is_named() {
    let l = help_layout(window(), &metrics(), 0, &heights(0), 0.0);
    let (x, y) = centre(l.close);
    assert_eq!(help_hit(&l, x, y), HelpHit::Close);
    let (x, y) = centre(l.nav[1]);
    assert_eq!(help_hit(&l, x, y), HelpHit::Section(1));
    let (x, y) = centre(l.shortcuts);
    assert_eq!(help_hit(&l, x, y), HelpHit::Shortcuts);
    assert_eq!(help_hit(&l, 2.0, 2.0), HelpHit::Outside);
}

// ---------------------------------------------------------------- the tour

#[test]
fn the_tour_card_sits_beside_what_it_points_at_and_never_on_it() {
    let m = metrics();
    for target in [
        Rect::new(8.0, 50.0, 248.0, 280.0),
        Rect::new(264.0, 50.0, 1008.0, 300.0),
        Rect::new(264.0, 360.0, 1008.0, 350.0),
        Rect::new(8.0, 8.0, 1264.0, 36.0),
    ] {
        let l = tour_layout(window(), &m, Some(target), 120.0, 0, false, false, false);
        assert!(within(l.card, window()), "{target:?}: {:?}", l.card);
        assert!(!overlaps(l.card, target), "the card covers {target:?}");
        let spot = l.spotlight.expect("a spotlight");
        assert!(spot.x <= target.x && spot.right() >= target.right());
    }
}

#[test]
fn a_step_with_nothing_to_point_at_is_a_card_in_the_middle() {
    let l = tour_layout(window(), &metrics(), None, 120.0, 0, true, false, false);
    assert!(l.spotlight.is_none());
    let (cx, cy) = centre(l.card);
    assert!((cx - 640.0).abs() < 2.0 && (cy - 360.0).abs() < 60.0);
    assert!(l.back.is_empty(), "no Back on the first step");
}

#[test]
fn the_card_has_its_buttons_and_a_choice_has_its_options() {
    let l = tour_layout(window(), &metrics(), None, 120.0, 2, false, true, false);
    assert_eq!(l.choices.len(), 2);
    for r in l
        .choices
        .iter()
        .chain([&l.next, &l.back, &l.close, &l.steps])
    {
        assert!(!r.is_empty());
        assert!(within(*r, l.card));
    }
    for (a, b) in [(l.next, l.back), (l.next, l.steps), (l.back, l.steps)] {
        assert!(!overlaps(a, b));
    }
    let (x, y) = centre(l.next);
    assert_eq!(tour_hit(&l, x, y), TourHit::Next);
    let (x, y) = centre(l.back);
    assert_eq!(tour_hit(&l, x, y), TourHit::Back);
    let (x, y) = centre(l.close);
    assert_eq!(tour_hit(&l, x, y), TourHit::Close);
    let (x, y) = centre(l.steps);
    assert_eq!(tour_hit(&l, x, y), TourHit::Steps);
    let (x, y) = centre(l.choices[1]);
    assert_eq!(tour_hit(&l, x, y), TourHit::Choice(1));
    // Off the card the press is the studio's: the tour is not in the way.
    assert_eq!(tour_hit(&l, 2.0, 2.0), TourHit::Through);
}

// ------------------------------------------------------- the animations
//
// `docs/ux-routing-and-learning-plan.md` step 7: *"the windowed tutorials
// [should] show related gifs of the mentioned actions being performed"*.

/// A page describes an action when it is a tour page that is neither the
/// welcome nor a choice — the choice is itself the thing to do.
fn describes_an_action(section: usize, page: usize) -> bool {
    let p = &GUIDE[section].pages[page];
    GUIDE[section].kind == GuideKind::Tour && section != 0 && p.choice.is_none()
}

#[test]
fn every_page_that_describes_an_action_shows_it_being_done() {
    for (s, section) in GUIDE.iter().enumerate() {
        for (p, page) in section.pages.iter().enumerate() {
            assert_eq!(
                page.media.is_some(),
                describes_an_action(s, p),
                "{} / {}",
                section.title,
                page.title
            );
        }
    }
}

#[test]
fn no_two_pages_show_the_same_animation() {
    let mut seen: Vec<GuideMedia> = GUIDE
        .iter()
        .flat_map(|s| s.pages.iter().filter_map(|p| p.media))
        .collect();
    let count = seen.len();
    seen.sort_unstable_by_key(|m| *m as usize);
    seen.dedup();
    assert_eq!(seen.len(), count);
    assert_eq!(count, GuideMedia::ALL.len(), "every clip is on a page");
}

/// The four places a tour step points at in a 1280×720 window.
fn targets() -> [Rect; 5] {
    [
        Rect::new(8.0, 8.0, 1264.0, 36.0),      // the transport
        Rect::new(8.0, 50.0, 248.0, 280.0),     // the rack
        Rect::new(8.0, 335.0, 248.0, 375.0),    // the browser
        Rect::new(264.0, 50.0, 1008.0, 300.0),  // the arrangement
        Rect::new(264.0, 360.0, 1008.0, 350.0), // the editor and mixer
    ]
}

#[test]
fn a_card_with_an_animation_still_sits_beside_what_it_points_at() {
    let m = metrics();
    for target in targets() {
        let l = tour_layout(window(), &m, Some(target), 150.0, 0, false, false, true);
        assert!(within(l.card, window()), "{target:?}: {:?}", l.card);
        assert!(!overlaps(l.card, target), "the card covers {target:?}");
        assert!(!l.media.is_empty());
        assert!(within(l.media, l.card));
    }
}

#[test]
fn the_animation_is_wide_and_beside_the_words_when_there_is_room() {
    let l = tour_layout(window(), &metrics(), None, 150.0, 0, false, false, true);
    let aspect = l.media.width / l.media.height;
    assert!(
        (aspect - 540.0 / 304.0).abs() < 0.02,
        "the clip's own shape"
    );
    assert!(l.media.width >= 400.0, "big enough to follow");
    assert!(l.media.right() <= l.body.x, "the words are to its right");
    for r in [l.title, l.body, l.next, l.back, l.steps, l.close] {
        assert!(!overlaps(l.media, r), "{r:?}");
    }
}

#[test]
fn a_narrow_window_stacks_the_animation_over_the_words() {
    let narrow = Rect::new(0.0, 0.0, 720.0, 900.0);
    let l = tour_layout(narrow, &metrics(), None, 150.0, 0, false, false, true);
    assert!(within(l.card, narrow));
    assert!(within(l.media, l.card));
    assert!(l.media.bottom() <= l.body.y + 0.01, "the words under it");
    let aspect = l.media.width / l.media.height;
    assert!((aspect - 540.0 / 304.0).abs() < 0.02);
}

#[test]
fn a_card_without_an_animation_is_as_it_was() {
    let with = tour_layout(window(), &metrics(), None, 150.0, 0, false, false, false);
    assert!(with.media.is_empty());
    assert!(with.card.width <= 400.0);
}

#[test]
fn the_guide_page_shows_each_animation_after_its_title() {
    // Tall enough that every page of a section is on screen at once: the
    // page lays out only what can be seen, and *Writing notes* has three
    // clips since the slide's two pages joined it.
    let tall = Rect::new(0.0, 0.0, 1280.0, 4000.0);
    for (s, section) in GUIDE.iter().enumerate() {
        let l = help_layout(tall, &metrics(), s, &heights(s), 0.0);
        for (p, page) in section.pages.iter().enumerate() {
            let blocks: Vec<_> = l.blocks.iter().filter(|b| b.page == p).collect();
            let media: Vec<_> = blocks.iter().filter(|b| b.media).collect();
            assert_eq!(media.len(), page.media.is_some() as usize, "{}", page.title);
            if let Some(b) = media.first() {
                assert!(
                    blocks[0].paragraph.is_none() && !blocks[0].media,
                    "the title first"
                );
                assert!(blocks[1].media, "then the animation");
                let aspect = b.rect.width / b.rect.height;
                assert!((aspect - 540.0 / 304.0).abs() < 0.02);
                assert!(
                    b.rect.width <= 540.0 + 0.01,
                    "never blown up past its pixels"
                );
                assert!(b.rect.x >= l.body.x && b.rect.right() <= l.body.right() + 0.01);
            }
        }
    }
}

/// The piano roll's page teaches the slide (`docs/note-paths-plan.md`): S
/// while drawing, and the FL page says where FL's slide notes went.
#[test]
fn the_guide_teaches_sliding_a_note() {
    let text = |title: &str| -> String {
        GUIDE
            .iter()
            .flat_map(|section| section.pages)
            .find(|page| page.title == title)
            .unwrap_or_else(|| panic!("a page called {title}"))
            .paragraphs
            .join(" ")
    };
    let roll = text("Sliding notes");
    assert!(roll.contains("press S") && roll.contains("slide"), "{roll}");
    assert!(roll.contains("Backspace"), "{roll}");
    let fl = text("Channel rack, playlist and mixer");
    assert!(fl.contains("slide"), "{fl}");
}

/// Ty, 2026-10-01: *"you need to get some animations of examples of how to
/// use the slide notes, and there should be a tutorial for how to do them."*
/// Two pages under *Writing notes*, each showing it being done: drawing a
/// slide (a melody from one note, a chord sliding apart), and shaping one.
#[test]
fn sliding_notes_have_a_tutorial_with_clips() {
    let writing = GUIDE
        .iter()
        .find(|section| section.title == "Writing notes")
        .expect("the roll's section");
    let page = |title: &str| {
        writing
            .pages
            .iter()
            .find(|page| page.title == title)
            .unwrap_or_else(|| panic!("a page called {title}"))
    };
    let slide = page("Sliding notes");
    assert_eq!(slide.media, Some(GuideMedia::Slide));
    let text = slide.paragraphs.join(" ");
    assert!(
        text.contains("S") && text.contains("Backspace") && text.contains("chord"),
        "{text}"
    );
    let shape = page("Shaping a slide");
    assert_eq!(shape.media, Some(GuideMedia::SlideEdit));
    let text = shape.paragraphs.join(" ");
    assert!(
        text.contains("double-click") || text.contains("Double-click"),
        "{text}"
    );
    assert!(
        text.contains("MPE"),
        "where an LV2 or VST 2 synth gets it: {text}"
    );
}

/// W, on card 0346: *"the browser page says 'Fontelle hosts CLAP, VST 3 and
/// LV2 plugins as they are'. The product page says LV2 is not hosted on
/// Windows or macOS."* LV2 is Linux's alone (`lv2_stub.rs`), so the guide
/// says so wherever it names LV2 as hosted.
#[test]
fn the_guide_says_lv2_is_linux_only() {
    let hosting: Vec<&str> = GUIDE
        .iter()
        .flat_map(|section| section.pages)
        .flat_map(|page| page.paragraphs.iter().copied())
        .filter(|text| text.contains("hosts") && text.contains("LV2"))
        .collect();
    assert!(
        !hosting.is_empty(),
        "the page that says what Fontelle hosts"
    );
    for text in hosting {
        assert!(text.contains("Linux"), "{text}");
    }
}
