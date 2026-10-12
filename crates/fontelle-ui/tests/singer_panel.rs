//! The Fontelle Singer's instrument panel, as geometry: the singer's stage
//! across the top (a big portrait, the name, author and kind, a chip per
//! language, the terms, Change singer…, ▶ Sample, Open <engine>), then a
//! card per section — Voice, Expression (with a picture of the vibrato and
//! glide it gives), Performance, Output — flowing into two columns when
//! there is room, every control inside its card, a greyed control's card
//! saying why underneath. Pure functions of the view, so the window and the
//! headless shots agree on where everything is.

use fontelle_types::ParamAddress;
use fontelle_ui::canvas::{
    InstrumentGroup, InstrumentParam, InstrumentView, ParamKind, SingerPanelButton, SingerPanelHit,
    instrument_hit, singer_panel_hit, singer_panel_layout,
};
use fontelle_ui::document::{SingerChoice, SingerKind, SingerPanelView};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn knob(address: &str, label: &str) -> InstrumentParam {
    InstrumentParam {
        address: ParamAddress::new(address),
        label: label.into(),
        value: 0.5,
        display: "0".into(),
        kind: ParamKind::Knob,
        automated: false,
    }
}

fn group(name: &str, params: Vec<InstrumentParam>) -> InstrumentGroup {
    InstrumentGroup {
        name: name.into(),
        params,
    }
}

fn controls() -> InstrumentView {
    InstrumentView {
        title: "Fontelle Singer".into(),
        keys: Vec::new(),
        key: None,
        groups: vec![
            group(
                "Voice",
                vec![
                    knob("vocal/lane/gender", "Gender"),
                    knob("vocal/lane/breathiness", "Breath"),
                    knob("vocal/lane/tension", "Tension"),
                    knob("vocal/lane/voicing", "Voicing"),
                ],
            ),
            group(
                "Expression",
                vec![
                    InstrumentParam {
                        kind: ParamKind::Switch,
                        ..knob("vocal/vibrato/on", "Vibrato")
                    },
                    knob("vocal/vibrato/depth", "Depth"),
                    knob("vocal/vibrato/rate", "Rate"),
                    knob("vocal/vibrato/delay", "Delay"),
                    knob("vocal/vibrato/fade_in", "Fade in"),
                    knob("vocal/vibrato/fade_out", "Fade out"),
                    knob("vocal/vibrato/min_length", "On notes over"),
                    InstrumentParam {
                        kind: ParamKind::Switch,
                        ..knob("vocal/glide/on", "Glide")
                    },
                    knob("vocal/glide/length", "Length"),
                    InstrumentParam {
                        kind: ParamKind::Choice(vec!["S".into(), "Linear".into()]),
                        ..knob("vocal/glide/shape", "Shape")
                    },
                    knob("vocal/consonant_speed", "Consonants"),
                ],
            ),
            group(
                "Performance",
                vec![InstrumentParam {
                    kind: ParamKind::Choice(vec!["Automatic".into(), "Japanese CV".into()]),
                    ..knob("vocal/setting/phonemizer", "Phonemizer")
                }],
            ),
            group(
                "Output",
                vec![knob("mixer/gain", "Volume"), knob("mixer/pan", "Pan")],
            ),
        ],
    }
}

fn header(sample: bool, has_editor: bool) -> SingerPanelView {
    SingerPanelView {
        instrument: "Fontelle Singer".into(),
        engine: None,
        singer: Some(SingerChoice {
            engine: "fontelle-utau".into(),
            id: "teto".into(),
            name: "重音テト".into(),
            languages: vec!["ja".into(), "en".into()],
            kind: SingerKind::Utau,
            ..Default::default()
        }),
        language: "ja".into(),
        author: Some("TWINDRILL".into()),
        bank: Some("KasaneTeto".into()),
        sample,
        has_editor,
        greyed: vec![(
            "vocal/lane/tension".into(),
            "重音テト doesn't use tension".into(),
        )],
        ..Default::default()
    }
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn inside(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x - 0.01
        && inner.y >= outer.y - 0.01
        && inner.right() <= outer.right() + 0.01
        && inner.bottom() <= outer.bottom() + 0.01
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

const WIDE: Rect = Rect {
    x: 0.0,
    y: 30.0,
    width: 980.0,
    height: 900.0,
};

#[test]
fn the_singer_stands_across_the_top_with_a_big_portrait() {
    let m = Theme::dark_default().metrics;
    let l = singer_panel_layout(WIDE, &m, &header(true, false), &controls(), 0.0);
    assert!(inside(WIDE, l.header));
    assert!(l.portrait.width >= 120.0 && (l.portrait.width - l.portrait.height).abs() < 0.5);
    assert!(inside(l.header, l.portrait));
    assert!(l.name.x > l.portrait.right());
    assert_eq!(l.languages.len(), 2, "a chip a language");
    let buttons: Vec<SingerPanelButton> = l.buttons.iter().map(|(b, _)| *b).collect();
    assert!(buttons.contains(&SingerPanelButton::ChangeSinger));
    assert!(buttons.contains(&SingerPanelButton::Sample));
    assert!(!buttons.contains(&SingerPanelButton::OpenEditor));
    for (_, r) in &l.buttons {
        assert!(inside(l.header, *r));
    }
    // Every card starts below the stage.
    for (_, card) in &l.cards {
        assert!(card.y >= l.header.bottom());
    }
    // No sample, no button; an engine with a window, its button.
    let l = singer_panel_layout(WIDE, &m, &header(false, true), &controls(), 0.0);
    let buttons: Vec<SingerPanelButton> = l.buttons.iter().map(|(b, _)| *b).collect();
    assert!(!buttons.contains(&SingerPanelButton::Sample));
    assert!(buttons.contains(&SingerPanelButton::OpenEditor));
}

#[test]
fn each_section_is_a_card_holding_its_controls() {
    let m = Theme::dark_default().metrics;
    let view = controls();
    let l = singer_panel_layout(WIDE, &m, &header(true, false), &view, 0.0);
    assert_eq!(l.cards.len(), view.groups.len());
    for (g, group) in view.groups.iter().enumerate() {
        let card = l.cards.iter().find(|(i, _)| *i == g).unwrap().1;
        assert!(inside(WIDE, card), "{} card in the body", group.name);
        for p in 0..group.params.len() {
            let cell = l.controls.control(g, p).expect("a cell per control");
            assert!(
                inside(card, cell),
                "{}'s control {p} in its card",
                group.name
            );
        }
    }
    for (i, (_, a)) in l.cards.iter().enumerate() {
        for (_, b) in &l.cards[i + 1..] {
            assert!(!overlaps(*a, *b), "cards overlap: {a:?} {b:?}");
        }
    }
    // Wide enough: two columns.
    let xs: std::collections::BTreeSet<i32> =
        l.cards.iter().map(|(_, r)| r.x.round() as i32).collect();
    assert_eq!(xs.len(), 2, "two columns at {}", WIDE.width);
    // The Expression card carries a picture of what it gives.
    let expression = l.cards[1].1;
    let picture = l.picture.expect("a picture of the vibrato");
    assert!(inside(expression, picture));
    assert!(picture.height >= 48.0);
}

#[test]
fn a_narrow_window_is_one_column_and_scrolls() {
    let m = Theme::dark_default().metrics;
    let narrow = Rect::new(0.0, 30.0, 460.0, 600.0);
    let l = singer_panel_layout(narrow, &m, &header(true, false), &controls(), 0.0);
    let xs: std::collections::BTreeSet<i32> =
        l.cards.iter().map(|(_, r)| r.x.round() as i32).collect();
    assert_eq!(xs.len(), 1);
    assert!(l.content_height > narrow.height, "more than fits");
    for (_, card) in &l.cards {
        assert!(card.right() <= narrow.right() + 0.01);
    }
    // Scrolled, everything moves up by as much; past the end, it stops.
    let scrolled = singer_panel_layout(narrow, &m, &header(true, false), &controls(), 100.0);
    assert!((l.header.y - scrolled.header.y - 100.0).abs() < 0.01);
    let far = singer_panel_layout(narrow, &m, &header(true, false), &controls(), 1e6);
    let last = far
        .cards
        .iter()
        .map(|(_, r)| r.bottom())
        .fold(0.0, f32::max);
    assert!(last <= narrow.bottom() + 0.5, "{last}");
    assert!(last >= narrow.bottom() - 40.0, "{last}");
}

#[test]
fn a_greyed_control_is_drawn_greyed_and_its_card_says_why() {
    let m = Theme::dark_default().metrics;
    let l = singer_panel_layout(WIDE, &m, &header(true, false), &controls(), 0.0);
    let voice = l.cards[0].1;
    let note = l
        .footnotes
        .iter()
        .find(|(g, _)| *g == 0)
        .expect("the Voice card has a footnote")
        .1;
    assert!(inside(voice, note));
    // The cells all sit above it.
    for p in 0..4 {
        assert!(l.controls.control(0, p).unwrap().bottom() <= note.y + 0.01);
    }
    // No greyed control in Output, so no footnote there.
    assert!(l.footnotes.iter().all(|(g, _)| *g != 3));
    assert!(l.greyed.contains(&(0, 2)), "tension is greyed");
}

#[test]
fn a_press_finds_the_button_the_language_or_the_control() {
    let m = Theme::dark_default().metrics;
    let l = singer_panel_layout(WIDE, &m, &header(true, false), &controls(), 0.0);
    let change = l
        .buttons
        .iter()
        .find(|(b, _)| *b == SingerPanelButton::ChangeSinger)
        .unwrap()
        .1;
    let (x, y) = centre(change);
    assert_eq!(
        singer_panel_hit(&l, x, y),
        Some(SingerPanelHit::Button(SingerPanelButton::ChangeSinger))
    );
    let (x, y) = centre(l.languages[1].1);
    assert_eq!(
        singer_panel_hit(&l, x, y),
        Some(SingerPanelHit::Language(1))
    );
    let (x, y) = centre(l.portrait);
    assert_eq!(singer_panel_hit(&l, x, y), Some(SingerPanelHit::Portrait));
    if let Some(terms) = l.terms {
        let (x, y) = centre(terms);
        assert_eq!(singer_panel_hit(&l, x, y), Some(SingerPanelHit::Terms));
    }
    let cell = l.controls.control(1, 3).unwrap();
    let (x, y) = centre(cell);
    assert_eq!(singer_panel_hit(&l, x, y), None, "a control is not chrome");
    assert_eq!(instrument_hit(&l.controls, x, y), Some((1, 3)));
}

/// A track nobody sings: the stage says so, and Change singer… is there.
#[test]
fn nobody_singing_still_offers_a_singer() {
    let m = Theme::dark_default().metrics;
    let view = SingerPanelView {
        instrument: "Some Engine".into(),
        engine: Some("Some Engine".into()),
        message: Some("Choose a singer".into()),
        ..Default::default()
    };
    let l = singer_panel_layout(WIDE, &m, &view, &controls(), 0.0);
    assert!(l.languages.is_empty());
    assert!(
        l.buttons
            .iter()
            .any(|(b, _)| *b == SingerPanelButton::ChangeSinger)
    );
}

/// A song from a computer with a bank this one has not got: the stage
/// names that bank (it said "No singer yet", which is not so), and its
/// button finds singers rather than listing the ones installed.
#[test]
fn a_singer_not_installed_here_is_named_and_offers_find_singers() {
    let m = Theme::dark_default().metrics;
    let view = SingerPanelView {
        instrument: "Fontelle Singer".into(),
        missing: Some("KasaneTeto_tougou_110401".into()),
        find_first: true,
        message: Some("Not installed on this computer".into()),
        ..Default::default()
    };
    let l = singer_panel_layout(WIDE, &m, &view, &controls(), 0.0);
    let buttons: Vec<SingerPanelButton> = l.buttons.iter().map(|(b, _)| *b).collect();
    assert!(
        buttons.contains(&SingerPanelButton::FindSingers),
        "{buttons:?}"
    );
    assert!(!buttons.contains(&SingerPanelButton::Voicebanks));
    assert_eq!(
        SingerPanelButton::FindSingers.label(""),
        "Find singers\u{2026}"
    );
    let strings = fontelle_ui::render::singer_panel::singer_panel_strings(&view, &controls());
    assert!(
        strings
            .iter()
            .any(|(s, style)| s == "KasaneTeto_tougou_110401"
                && *style == fontelle_ui::render::singer_panel::SINGER_NAME_STYLE),
        "{strings:?}"
    );
    assert!(!strings.iter().any(|(s, _)| s == "No singer yet"));
    assert!(
        strings.iter().any(|(s, _)| s.contains("NOT INSTALLED")),
        "{strings:?}"
    );
}
