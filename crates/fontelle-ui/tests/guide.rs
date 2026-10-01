//! One way to learn Fontelle: the guide, the pages behind `?`, and the tour.
//!
//! `docs/ux-routing-and-learning-plan.md` §5. Ty chose all of an interactive
//! tutorial, in-app guide pages, "Coming from…" pages and a web manual, and
//! asked that they be **one clean experience, not intrusive and not hard to
//! use**. So there is one catalogue of guide content ([`GUIDE`]) that the
//! tour draws as an overlay and the help page draws as reading, and the two
//! cannot disagree.

use fontelle_ui::canvas::{
    GUIDE, GuideChoice, GuideKind, HelpHit, TourHit, help_hit, help_layout, help_scroll_max,
    help_scrolled, tour_hit, tour_layout, tour_steps,
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
        .map(|p| 1 + p.paragraphs.len())
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
        let l = tour_layout(window(), &m, Some(target), 120.0, 0, false, false);
        assert!(within(l.card, window()), "{target:?}: {:?}", l.card);
        assert!(!overlaps(l.card, target), "the card covers {target:?}");
        let spot = l.spotlight.expect("a spotlight");
        assert!(spot.x <= target.x && spot.right() >= target.right());
    }
}

#[test]
fn a_step_with_nothing_to_point_at_is_a_card_in_the_middle() {
    let l = tour_layout(window(), &metrics(), None, 120.0, 0, true, false);
    assert!(l.spotlight.is_none());
    let (cx, cy) = centre(l.card);
    assert!((cx - 640.0).abs() < 2.0 && (cy - 360.0).abs() < 60.0);
    assert!(l.back.is_empty(), "no Back on the first step");
}

#[test]
fn the_card_has_its_buttons_and_a_choice_has_its_options() {
    let l = tour_layout(window(), &metrics(), None, 120.0, 2, false, true);
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
