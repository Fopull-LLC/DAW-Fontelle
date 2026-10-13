//! The singer picker as a grid of cards: each singer's portrait, name, kind
//! (UTAU, DiffSinger, Engine), languages, whether its terms are still to be
//! read and whether it is research-only; the Fontelle Singer's voicebanks
//! in a group above the engines'; a search that narrows it; the arrow keys
//! moving between cards as they are laid out.

use fontelle_ui::canvas::{
    PickerGroup, PickerHit, PickerMove, picker_filter, picker_move, singer_picker_hit,
    singer_picker_layout,
};
use fontelle_ui::document::{SingerChoice, SingerKind};
use fontelle_ui::layout::Rect;

fn singer(name: &str, kind: SingerKind, languages: &[&str]) -> SingerChoice {
    SingerChoice {
        engine: if kind == SingerKind::Engine {
            "Some Engine".into()
        } else {
            "fontelle-utau".into()
        },
        engine_name: "Some Engine".into(),
        id: name.to_lowercase(),
        name: name.into(),
        languages: languages.iter().map(|l| l.to_string()).collect(),
        kind,
        ..Default::default()
    }
}

fn singers() -> Vec<SingerChoice> {
    vec![
        singer("Engine Alto", SingerKind::Engine, &["en"]),
        singer("Kasane Teto", SingerKind::Utau, &["ja"]),
        singer("Qixuan", SingerKind::DiffSinger, &["zh", "ja", "en"]),
        singer("Canary", SingerKind::Utau, &["en"]),
        singer("Engine Bass", SingerKind::Engine, &["en"]),
    ]
}

const WINDOW: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 1200.0,
    height: 800.0,
};

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

#[test]
fn voicebanks_come_first_in_their_own_group_then_the_engines() {
    let s = singers();
    let shown = picker_filter(&s, "");
    assert_eq!(shown, vec![1, 2, 3, 0, 4]);
    let l = singer_picker_layout(WINDOW, &s, &shown, 0.0, 0);
    assert_eq!(l.groups.len(), 2);
    assert_eq!(l.groups[0].0, PickerGroup::Voicebanks);
    assert_eq!(l.groups[1].0, PickerGroup::Engines);
    let cards: Vec<usize> = l.cards.iter().map(|c| c.singer).collect();
    assert_eq!(cards, shown);
    // The engines' group starts below every voicebank card.
    let banks_bottom = l.cards[..3]
        .iter()
        .map(|c| c.frame.bottom())
        .fold(0.0, f32::max);
    assert!(l.groups[1].1.y >= banks_bottom);
    assert!(l.cards[3].frame.y > l.groups[1].1.y);
    // A card holds its portrait, and a chip per language.
    let qx = &l.cards[1];
    assert!(qx.frame.contains(qx.portrait.x, qx.portrait.y));
    assert_eq!(qx.languages.len(), 3);
    assert!(
        qx.languages
            .iter()
            .all(|(_, r)| qx.frame.contains(r.x, r.y))
    );
}

#[test]
fn the_search_narrows_by_name_language_kind_and_author() {
    let mut s = singers();
    s[3].terms = Some("by someone".into());
    assert_eq!(picker_filter(&s, "teto"), vec![1]);
    assert_eq!(picker_filter(&s, "Chinese"), vec![2], "by language name");
    assert_eq!(picker_filter(&s, "zh"), vec![2], "by language tag");
    assert_eq!(picker_filter(&s, "diffsinger"), vec![2]);
    assert_eq!(picker_filter(&s, "utau"), vec![1, 3]);
    assert_eq!(picker_filter(&s, "engine"), vec![0, 4]);
    assert_eq!(picker_filter(&s, "  "), vec![1, 2, 3, 0, 4]);
    assert!(picker_filter(&s, "nobody at all").is_empty());
    // Only one group shows when only one has anything in it.
    let shown = picker_filter(&s, "engine");
    let l = singer_picker_layout(WINDOW, &s, &shown, 0.0, 0);
    assert_eq!(l.groups.len(), 1);
    assert_eq!(l.groups[0].0, PickerGroup::Engines);
}

#[test]
fn presses_land_on_cards_languages_and_the_ways_to_get_more() {
    let s = singers();
    let shown = picker_filter(&s, "");
    let l = singer_picker_layout(WINDOW, &s, &shown, 0.0, 0);
    let qx = &l.cards[1];
    let (x, y) = centre(qx.portrait);
    assert_eq!(singer_picker_hit(&l, x, y), Some(PickerHit::Card(2)));
    let (x, y) = centre(qx.languages[1].1);
    assert_eq!(
        singer_picker_hit(&l, x, y),
        Some(PickerHit::Language(2, "ja".into()))
    );
    let (x, y) = centre(l.search);
    assert_eq!(singer_picker_hit(&l, x, y), Some(PickerHit::Search));
    let (x, y) = centre(l.nobody);
    assert_eq!(singer_picker_hit(&l, x, y), Some(PickerHit::Nobody));
    let (x, y) = centre(l.install);
    assert_eq!(singer_picker_hit(&l, x, y), Some(PickerHit::Install));
    let (x, y) = centre(l.manage);
    assert_eq!(singer_picker_hit(&l, x, y), Some(PickerHit::Manage));
    let (x, y) = centre(l.close);
    assert_eq!(singer_picker_hit(&l, x, y), Some(PickerHit::Close));
    assert_eq!(singer_picker_hit(&l, 1.0, 1.0), Some(PickerHit::Outside));
}

#[test]
fn the_arrow_keys_move_between_cards_as_they_are_laid_out() {
    // Enough voicebanks for two rows.
    let mut s: Vec<SingerChoice> = (0..9)
        .map(|i| singer(&format!("Bank {i}"), SingerKind::Utau, &["ja"]))
        .collect();
    s.push(singer("Engine", SingerKind::Engine, &["en"]));
    let shown = picker_filter(&s, "");
    let l = singer_picker_layout(WINDOW, &s, &shown, 0.0, 0);
    let per_row = l
        .cards
        .iter()
        .take_while(|c| (c.frame.y - l.cards[0].frame.y).abs() < 1.0)
        .count();
    assert!((3..9).contains(&per_row), "{per_row} a row");
    assert_eq!(picker_move(&l, None, PickerMove::Right), Some(0));
    assert_eq!(picker_move(&l, Some(0), PickerMove::Right), Some(1));
    assert_eq!(picker_move(&l, Some(0), PickerMove::Left), Some(0));
    assert_eq!(picker_move(&l, Some(0), PickerMove::Down), Some(per_row));
    assert_eq!(picker_move(&l, Some(per_row), PickerMove::Up), Some(0));
    // Down from the last bank row reaches the engines' group.
    assert_eq!(picker_move(&l, Some(8), PickerMove::Down), Some(9));
    assert_eq!(picker_move(&l, Some(9), PickerMove::Down), Some(9));
    assert_eq!(picker_move(&l, Some(3), PickerMove::Home), Some(0));
    assert_eq!(picker_move(&l, Some(3), PickerMove::End), Some(9));
}

#[test]
fn a_long_list_scrolls_and_only_what_shows_is_hit() {
    let s: Vec<SingerChoice> = (0..60)
        .map(|i| singer(&format!("Bank {i}"), SingerKind::Utau, &["ja"]))
        .collect();
    let shown = picker_filter(&s, "");
    let top = singer_picker_layout(WINDOW, &s, &shown, 0.0, 0);
    assert!(top.content_height > top.list.height);
    let down = singer_picker_layout(WINDOW, &s, &shown, 400.0, 0);
    assert!(down.cards[0].frame.y < top.cards[0].frame.y);
    // A card scrolled above the list is not pressed through it.
    let hidden = down.cards.iter().find(|c| c.frame.bottom() < down.list.y);
    if let Some(c) = hidden {
        let (x, y) = centre(c.frame);
        assert_ne!(
            singer_picker_hit(&down, x, y),
            Some(PickerHit::Card(c.singer))
        );
    }
    // Scrolled past the end, it stops at the end.
    let far = singer_picker_layout(WINDOW, &s, &shown, 1e6, 0);
    let last = far.cards.last().unwrap();
    assert!(last.frame.bottom() <= far.list.bottom() + 1.0);
}

// ------------------------------------- one instrument's singers, bigger

/// Each instrument's picker shows only its own singers: the Fontelle
/// Singer its voicebanks, an engine its own; the other is reached through
/// Change instrument.
#[test]
fn each_instrument_offers_only_its_own_singers() {
    use fontelle_ui::canvas::{picker_filter_for, singer_picker_layout_for};
    let mut s = singers();
    s.push(SingerChoice {
        engine: "Other Engine".into(),
        ..singer("Other Tenor", SingerKind::Engine, &["en"])
    });
    assert_eq!(picker_filter_for(&s, "", None), vec![1, 2, 3]);
    assert_eq!(picker_filter_for(&s, "", Some("Some Engine")), vec![0, 4]);
    assert_eq!(picker_filter_for(&s, "", Some("Other Engine")), vec![5]);
    assert_eq!(picker_filter_for(&s, "bass", Some("Some Engine")), vec![4]);
    assert!(picker_filter_for(&s, "teto", Some("Some Engine")).is_empty());
    // The Fontelle Singer's picker installs voicebanks; an engine's does
    // not. Both can change instrument.
    let shown = picker_filter_for(&s, "", None);
    let l = singer_picker_layout_for(WINDOW, &s, &shown, 0.0, 0, true);
    assert!(!l.install.is_empty() && !l.manage.is_empty());
    let (x, y) = centre(l.change);
    assert_eq!(
        singer_picker_hit(&l, x, y),
        Some(PickerHit::ChangeInstrument)
    );
    let shown = picker_filter_for(&s, "", Some("Some Engine"));
    let l = singer_picker_layout_for(WINDOW, &s, &shown, 0.0, 0, false);
    assert!(l.install.is_empty() && l.manage.is_empty());
    assert_eq!(l.groups.len(), 1);
    let (x, y) = centre(l.change);
    assert_eq!(
        singer_picker_hit(&l, x, y),
        Some(PickerHit::ChangeInstrument)
    );
}

/// > *"increase the scale of the singer select menu more (kind of tiny to
/// > read currently)"*
///
/// Portraits twice what they were, cards to match, and fewer, bigger cards
/// across a smaller window (or a larger UI scale, which is a smaller window
/// in logical pixels).
#[test]
fn the_cards_are_big_and_fewer_across_a_small_window() {
    use fontelle_ui::canvas::PORTRAIT_SIZE;
    const { assert!(PORTRAIT_SIZE >= 128.0) };
    let s: Vec<SingerChoice> = (0..9)
        .map(|i| singer(&format!("Bank {i}"), SingerKind::Utau, &["ja"]))
        .collect();
    let shown = picker_filter(&s, "");
    let across = |window: Rect| {
        let l = singer_picker_layout(window, &s, &shown, 0.0, 0);
        let first = l.cards[0].frame;
        assert!(first.width >= 280.0, "{first:?}");
        assert!(l.cards[0].portrait.width >= 128.0);
        assert!(first.contains(
            l.cards[0].portrait.right() - 1.0,
            l.cards[0].portrait.bottom() - 1.0
        ));
        l.cards
            .iter()
            .take_while(|c| (c.frame.y - first.y).abs() < 1.0)
            .count()
    };
    let big = across(Rect::new(0.0, 0.0, 1600.0, 1000.0));
    let at_150 = across(Rect::new(0.0, 0.0, 1100.0 / 1.5, 700.0 / 1.5));
    assert!((3..=5).contains(&big), "{big}");
    assert!((1..=2).contains(&at_150), "{at_150}");
}
