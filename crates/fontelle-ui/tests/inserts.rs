//! The insert rack on a mixer strip, and the EQ editor behind it (TDD §13.4).
//!
//! Geometry, hit-testing and the editor's own arithmetic, all pure, per §2.5
//! of `docs/first-usable-plan.md`. Nothing here knows what a `Project` is.
//!
//! The reason this file exists at all is the pattern PROGRESS.md has recorded
//! eight times now: a control that exists in the model and nothing on screen
//! can reach. The chain is in the document, it is compiled into the graph, and
//! it is audible — and until there is a rectangle to click, none of that is a
//! feature anybody has.

use fontelle_types::{BandChannel, BandType, EqBand, EqConfig};
use fontelle_ui::canvas::{
    EqHit, InsertInfo, MixerHit, eq_freq_at, eq_gain_at, eq_hit, eq_layout, eq_x_of_freq,
    eq_y_of_gain, mixer_hit, mixer_layout,
};
use fontelle_ui::document::MixerStrip;
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn metrics() -> fontelle_ui::theme::Metrics {
    Theme::dark_default().metrics
}

fn strip(name: &str, inserts: Vec<InsertInfo>) -> MixerStrip {
    MixerStrip {
        name: name.into(),
        gain_db: 0.0,
        pan: 0.0,
        mute: false,
        solo: false,
        is_master: false,
        color: [0x60, 0x60, 0x68, 0xff],
        inserts,
    }
}

fn an_insert(label: &str) -> InsertInfo {
    InsertInfo {
        label: label.into(),
        bypassed: false,
    }
}

fn body() -> Rect {
    Rect::new(0.0, 0.0, 400.0, 320.0)
}

fn a_track() -> fontelle_types::MixerTrackId {
    fontelle_types::MixerTrackId::from(slotmap::KeyData::from_ffi((1 << 32) | 3))
}

// ------------------------------------------------------------- the rack

#[test]
fn a_strip_with_no_effects_still_offers_somewhere_to_put_one() {
    // The empty state is the one every track starts in, and a rack with no
    // visible way to add to it is a rack nobody finds.
    let strips = vec![strip("Keys", Vec::new())];
    let layout = mixer_layout(body(), &metrics(), &strips, 0);
    let first = &layout.strips[0];
    assert!(first.inserts.is_empty());
    assert!(!first.add.is_empty(), "the add row is there from the start");
}

#[test]
fn every_insert_gets_a_row_of_its_own() {
    let strips = vec![strip("Keys", vec![an_insert("EQ"), an_insert("EQ")])];
    let layout = mixer_layout(body(), &metrics(), &strips, 0);
    assert_eq!(layout.strips[0].inserts.len(), 2);
}

#[test]
fn the_rows_are_stacked_in_chain_order_and_do_not_overlap() {
    // Top to bottom is first to last, because that is the order the sound goes
    // through them and a rack that drew them in some other order would be
    // lying about the signal path.
    let strips = vec![strip(
        "Keys",
        vec![an_insert("EQ"), an_insert("EQ"), an_insert("EQ")],
    )];
    let layout = mixer_layout(body(), &metrics(), &strips, 0);
    let rows = &layout.strips[0].inserts;
    for pair in rows.windows(2) {
        assert!(
            pair[0].bottom() <= pair[1].y + 0.01,
            "row at {} runs into the one at {}",
            pair[0].y,
            pair[1].y
        );
    }
    assert!(
        rows.last().unwrap().bottom() <= layout.strips[0].add.y + 0.01,
        "and the add row comes after all of them"
    );
}

#[test]
fn the_rack_sits_above_the_fader_and_does_not_eat_it() {
    // A rack that grew into the fader would take the mixer's one essential
    // control away as soon as somebody used its newest one.
    let strips = vec![strip(
        "Keys",
        vec![
            an_insert("EQ"),
            an_insert("EQ"),
            an_insert("EQ"),
            an_insert("EQ"),
        ],
    )];
    let layout = mixer_layout(body(), &metrics(), &strips, 0);
    let s = &layout.strips[0];
    assert!(s.add.bottom() <= s.fader.y + 0.01, "the rack is above it");
    assert!(s.fader.height > 0.0, "and the fader still has room to drag");
}

#[test]
fn a_rack_never_takes_the_fader_below_what_it_needs_to_be_a_control() {
    // Panels get dragged small. What has to survive is the thing the panel is
    // *for*, and the claim is comparative rather than absolute: at a height
    // where a strip has a fader at all, adding four effects must not take it
    // away. (A panel too short for a fader in the first place is a different
    // problem and not this feature's.)
    for height in [90.0, 120.0, 160.0, 200.0, 260.0, 400.0] {
        let area = Rect::new(0.0, 0.0, 400.0, height);
        let bare = mixer_layout(area, &metrics(), &[strip("Keys", Vec::new())], 0);
        let full = mixer_layout(
            area,
            &metrics(),
            &[strip(
                "Keys",
                vec![
                    an_insert("EQ"),
                    an_insert("EQ"),
                    an_insert("EQ"),
                    an_insert("EQ"),
                ],
            )],
            0,
        );
        let (bare, full) = (&bare.strips[0], &full.strips[0]);
        if bare.fader.height <= 0.0 {
            continue;
        }
        assert!(
            full.fader.height > 0.0,
            "at {height} px a strip has a fader with nothing on it and none with four"
        );
        for row in full.inserts.iter().chain(std::iter::once(&full.add)) {
            assert!(
                row.height <= 0.01 || row.bottom() <= full.fader.y + 0.01,
                "at {height} px a rack row runs into the fader"
            );
        }
    }
}

// -------------------------------------------------------------- clicking

#[test]
fn clicking_an_insert_row_names_the_strip_and_the_slot() {
    let strips = vec![strip("Keys", vec![an_insert("EQ"), an_insert("EQ")])];
    let layout = mixer_layout(body(), &metrics(), &strips, 0);
    let row = layout.strips[0].inserts[1];
    let (x, y) = (row.right() - 2.0, row.y + row.height / 2.0);
    assert_eq!(mixer_hit(&layout, x, y), MixerHit::Insert(0, 1));
}

#[test]
fn the_left_end_of_a_row_is_its_bypass_switch() {
    // A switch you can reach without opening the effect, because comparing
    // with and without is the reason to have one.
    let strips = vec![strip("Keys", vec![an_insert("EQ")])];
    let layout = mixer_layout(body(), &metrics(), &strips, 0);
    let row = layout.strips[0].inserts[0];
    let (x, y) = (row.x + 2.0, row.y + row.height / 2.0);
    assert_eq!(mixer_hit(&layout, x, y), MixerHit::BypassInsert(0, 0));
}

#[test]
fn clicking_the_add_row_asks_for_a_new_effect() {
    let strips = vec![strip("Keys", Vec::new())];
    let layout = mixer_layout(body(), &metrics(), &strips, 0);
    let add = layout.strips[0].add;
    assert_eq!(
        mixer_hit(&layout, add.x + add.width / 2.0, add.y + add.height / 2.0),
        MixerHit::AddInsert(0)
    );
}

#[test]
fn the_master_strips_rack_is_clickable_too() {
    // Master is where a mix bus compressor goes, so its chain is the one most
    // people reach for first.
    let mut master = strip("Master", vec![an_insert("EQ")]);
    master.is_master = true;
    let strips = vec![strip("Keys", Vec::new()), master];
    let layout = mixer_layout(body(), &metrics(), &strips, 0);
    let row = layout.master.as_ref().unwrap().inserts[0];
    assert_eq!(
        mixer_hit(&layout, row.right() - 2.0, row.y + row.height / 2.0),
        MixerHit::Insert(1, 0)
    );
}

#[test]
fn the_fader_still_wins_where_the_fader_is() {
    // The regression a new control in a crowded strip invites.
    let strips = vec![strip("Keys", vec![an_insert("EQ")])];
    let layout = mixer_layout(body(), &metrics(), &strips, 0);
    let fader = layout.strips[0].fader;
    assert_eq!(
        mixer_hit(&layout, fader.x + 1.0, fader.y + fader.height / 2.0),
        MixerHit::Fader(0)
    );
}

// ------------------------------------------------------------ the editor

fn a_bell(freq_hz: f32, gain_db: f32) -> EqBand {
    EqBand {
        band_type: BandType::Bell,
        freq_hz,
        gain_db,
        q: 1.0,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    }
}

fn one_band(band: EqBand) -> EqConfig {
    let mut config = EqConfig::default();
    config.bands[0] = band;
    config
}

#[test]
fn frequency_runs_left_to_right_on_a_log_scale() {
    // Log, because that is how hearing works: the octave from 100 to 200 Hz
    // has to take as much of the width as the one from 5 to 10 kHz, or the
    // bottom half of the spectrum is three pixels wide.
    let area = Rect::new(0.0, 0.0, 600.0, 200.0);
    let low = eq_x_of_freq(area, 20.0);
    let high = eq_x_of_freq(area, 20_000.0);
    assert!((low - area.x).abs() < 0.5, "20 Hz is the left edge");
    assert!((high - area.right()).abs() < 0.5, "20 kHz is the right");

    let first_octave = eq_x_of_freq(area, 200.0) - eq_x_of_freq(area, 100.0);
    let last_octave = eq_x_of_freq(area, 10_000.0) - eq_x_of_freq(area, 5_000.0);
    assert!(
        (first_octave - last_octave).abs() < 0.5,
        "every octave is the same width: {first_octave} against {last_octave}"
    );
}

#[test]
fn a_frequency_survives_the_round_trip_through_a_pixel() {
    let area = Rect::new(10.0, 0.0, 600.0, 200.0);
    for freq in [30.0, 100.0, 440.0, 1_000.0, 8_000.0, 18_000.0] {
        let back = eq_freq_at(area, eq_x_of_freq(area, freq));
        assert!(
            (back / freq - 1.0).abs() < 0.02,
            "{freq} Hz came back as {back}"
        );
    }
}

#[test]
fn gain_runs_up_the_middle_with_zero_in_the_centre() {
    let area = Rect::new(0.0, 0.0, 600.0, 200.0);
    let centre = eq_y_of_gain(area, 0.0);
    assert!(
        (centre - (area.y + area.height / 2.0)).abs() < 0.5,
        "0 dB is the middle line"
    );
    assert!(
        eq_y_of_gain(area, 12.0) < centre,
        "a lift is above it, because up is louder"
    );
    assert!(eq_y_of_gain(area, -12.0) > centre);
}

#[test]
fn a_gain_survives_the_round_trip_too() {
    let area = Rect::new(0.0, 5.0, 600.0, 200.0);
    for gain in [-18.0, -6.0, 0.0, 3.5, 18.0] {
        let back = eq_gain_at(area, eq_y_of_gain(area, gain));
        assert!((back - gain).abs() < 0.2, "{gain} dB came back as {back}");
    }
}

#[test]
fn a_drag_past_the_end_of_the_scale_stops_at_the_end() {
    let area = Rect::new(0.0, 0.0, 600.0, 200.0);
    assert!(eq_gain_at(area, area.y - 500.0) <= 24.1);
    assert!(eq_gain_at(area, area.bottom() + 500.0) >= -24.1);
    assert!(eq_freq_at(area, area.x - 500.0) >= 19.0);
    assert!(eq_freq_at(area, area.right() + 500.0) <= 20_001.0);
}

#[test]
fn every_enabled_band_gets_a_handle_where_its_numbers_put_it() {
    let area = Rect::new(0.0, 0.0, 600.0, 240.0);
    let mut config = EqConfig::default();
    config.bands[0] = a_bell(200.0, 6.0);
    config.bands[3] = a_bell(5_000.0, -9.0);

    let layout = eq_layout(area, &metrics(), &config);
    assert_eq!(layout.handles.len(), 2, "one per enabled band, not eight");

    let first = layout.handles.iter().find(|h| h.band == 0).unwrap();
    assert!(
        (first.rect.x + first.rect.width / 2.0 - eq_x_of_freq(layout.curve, 200.0)).abs() < 1.0
    );
    assert!((first.rect.y + first.rect.height / 2.0 - eq_y_of_gain(layout.curve, 6.0)).abs() < 1.0);
}

#[test]
fn a_band_that_has_no_gain_puts_its_handle_on_the_zero_line() {
    // A low-pass has a corner, not a gain. Its handle moves left and right and
    // stays where the ear expects it vertically.
    let area = Rect::new(0.0, 0.0, 600.0, 240.0);
    let mut config = EqConfig::default();
    config.bands[0] = EqBand {
        band_type: BandType::LowPass24,
        freq_hz: 2_000.0,
        gain_db: 0.0,
        q: 1.0,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    };
    let layout = eq_layout(area, &metrics(), &config);
    let handle = &layout.handles[0];
    assert!(
        (handle.rect.y + handle.rect.height / 2.0 - eq_y_of_gain(layout.curve, 0.0)).abs() < 1.0
    );
}

#[test]
fn clicking_a_handle_picks_up_that_band() {
    let area = Rect::new(0.0, 0.0, 600.0, 240.0);
    let layout = eq_layout(area, &metrics(), &one_band(a_bell(1_000.0, 6.0)));
    let handle = layout.handles[0].rect;
    assert_eq!(
        eq_hit(
            &layout,
            handle.x + handle.width / 2.0,
            handle.y + handle.height / 2.0
        ),
        EqHit::Handle(0)
    );
}

#[test]
fn clicking_empty_curve_is_not_a_handle() {
    let area = Rect::new(0.0, 0.0, 600.0, 240.0);
    let layout = eq_layout(area, &metrics(), &one_band(a_bell(1_000.0, 6.0)));
    assert_eq!(
        eq_hit(&layout, layout.curve.x + 2.0, layout.curve.y + 2.0),
        EqHit::Curve
    );
}

#[test]
fn clicking_outside_the_editor_hits_nothing() {
    let area = Rect::new(0.0, 0.0, 600.0, 240.0);
    let layout = eq_layout(area, &metrics(), &EqConfig::default());
    assert_eq!(eq_hit(&layout, -5.0, -5.0), EqHit::Nothing);
}

#[test]
fn the_curve_is_drawn_as_points_across_the_whole_width() {
    let area = Rect::new(4.0, 0.0, 600.0, 240.0);
    let layout = eq_layout(area, &metrics(), &one_band(a_bell(1_000.0, 12.0)));
    let points = fontelle_ui::canvas::eq_curve_points(&layout, &one_band(a_bell(1_000.0, 12.0)));

    assert!(points.len() > 32, "enough points to read as a curve");
    assert!((points[0].0 - layout.curve.x).abs() < 1.0);
    assert!((points.last().unwrap().0 - layout.curve.right()).abs() < 1.0);

    // And it bulges where the bell is.
    let at_bell = points
        .iter()
        .min_by(|a, b| {
            let target = eq_x_of_freq(layout.curve, 1_000.0);
            (a.0 - target)
                .abs()
                .partial_cmp(&(b.0 - target).abs())
                .unwrap()
        })
        .unwrap();
    assert!(
        at_bell.1 < eq_y_of_gain(layout.curve, 0.0) - 4.0,
        "the curve rises at the bell's frequency"
    );
}

#[test]
fn an_empty_editor_draws_a_flat_line() {
    let area = Rect::new(0.0, 0.0, 600.0, 240.0);
    let config = EqConfig::default();
    let layout = eq_layout(area, &metrics(), &config);
    let points = fontelle_ui::canvas::eq_curve_points(&layout, &config);
    let zero = eq_y_of_gain(layout.curve, 0.0);
    assert!(
        points.iter().all(|(_, y)| (y - zero).abs() < 0.5),
        "nothing switched on is a flat line down the middle"
    );
}

// ------------------------------------------------------------ the fx menu

use fontelle_types::{EffectConfig, EffectKind};
use fontelle_ui::canvas::{effect_menu_hit, effect_menu_layout, effect_view, eq_nudge_q};

#[test]
fn the_add_row_offers_every_effect_that_ships() {
    // One row per kind, from `EffectKind::ALL` — the same list the document
    // builds from, so a new effect appears here by existing rather than by
    // somebody remembering to add it twice.
    let anchor = Rect::new(100.0, 100.0, 60.0, 12.0);
    let menu = effect_menu_layout(anchor, body(), &metrics());
    assert_eq!(menu.items.len(), EffectKind::ALL.len());
    for kind in EffectKind::ALL {
        assert!(menu.items.iter().any(|(k, _)| *k == kind));
    }
}

#[test]
fn clicking_a_menu_row_names_the_effect_it_would_add() {
    let anchor = Rect::new(100.0, 100.0, 60.0, 12.0);
    let menu = effect_menu_layout(anchor, body(), &metrics());
    let (kind, rect) = menu.items[1];
    assert_eq!(
        effect_menu_hit(&menu, rect.x + 2.0, rect.y + rect.height / 2.0),
        Some(kind)
    );
}

#[test]
fn a_menu_stays_inside_the_window_it_drops_into() {
    // Dropped from a strip near the bottom edge, it opens upward rather than
    // off-screen — the same rule the route menu follows.
    let anchor = Rect::new(100.0, 300.0, 60.0, 12.0);
    let bounds = body();
    let menu = effect_menu_layout(anchor, bounds, &metrics());
    assert!(menu.frame.y >= bounds.y - 0.01);
    assert!(menu.frame.bottom() <= bounds.bottom() + 0.01);
}

#[test]
fn clicking_off_the_menu_is_a_miss() {
    let anchor = Rect::new(100.0, 100.0, 60.0, 12.0);
    let menu = effect_menu_layout(anchor, body(), &metrics());
    assert_eq!(effect_menu_hit(&menu, 5.0, 5.0), None);
}

// ------------------------------------------------- the generic editor

#[test]
fn any_effect_can_be_drawn_from_its_own_parameter_list() {
    // The payoff of §8.2's one addressing scheme: an effect that declares its
    // parameters gets an editor without anybody writing one, and every control
    // on it hands back the address automation would use.
    let config = EffectConfig::new(EffectKind::Compressor);
    let view = effect_view("Master", 0, a_track(), &config);
    let count: usize = view.groups.iter().map(|g| g.params.len()).sum();
    assert_eq!(count, config.specs().len(), "a control per parameter");
}

#[test]
fn every_control_carries_the_address_that_automates_it() {
    let config = EffectConfig::new(EffectKind::Compressor);
    let view = effect_view("Master", 2, a_track(), &config);
    for group in &view.groups {
        for param in &group.params {
            let target = fontelle_types::ParamTarget::parse(&param.address)
                .expect("every control's address parses");
            match target {
                fontelle_types::ParamTarget::Insert { slot, .. } => assert_eq!(slot, 2),
                other => panic!("an insert's control addressed {other:?}"),
            }
        }
    }
}

#[test]
fn a_switch_is_drawn_as_a_switch_and_a_choice_as_a_choice() {
    // Not everything is a knob. A detection mode drawn as a dial is a control
    // whose two positions are somewhere in a sweep.
    let config = EffectConfig::new(EffectKind::Compressor);
    let view = effect_view("Master", 0, a_track(), &config);
    let find = |name: &str| {
        view.groups
            .iter()
            .flat_map(|g| &g.params)
            .find(|p| p.label == name)
            .unwrap_or_else(|| panic!("no control called {name}"))
            .clone()
    };
    assert_eq!(
        find("Auto makeup").kind,
        fontelle_ui::canvas::ParamKind::Switch
    );
    assert!(matches!(
        find("Detection").kind,
        fontelle_ui::canvas::ParamKind::Choice(_)
    ));
    assert_eq!(find("Threshold").kind, fontelle_ui::canvas::ParamKind::Knob);
}

#[test]
fn a_controls_read_out_is_in_the_units_the_parameter_is_in() {
    // A knob whose number you cannot read is a knob you cannot set.
    let mut config = EffectConfig::new(EffectKind::Compressor);
    config.set("threshold", -12.0);
    config.set("ratio", 4.0);
    let view = effect_view("Master", 0, a_track(), &config);
    let display = |name: &str| {
        view.groups
            .iter()
            .flat_map(|g| &g.params)
            .find(|p| p.label == name)
            .unwrap()
            .display
            .clone()
    };
    assert!(
        display("Threshold").contains("dB"),
        "{}",
        display("Threshold")
    );
    assert!(display("Ratio").contains(":1"), "{}", display("Ratio"));
}

#[test]
fn the_panel_says_which_track_and_which_effect_it_is_showing() {
    let config = EffectConfig::new(EffectKind::Compressor);
    let view = effect_view("Drums", 0, a_track(), &config);
    assert!(view.title.contains("Drums"));
    assert!(view.title.contains("Comp"));
}

// -------------------------------------------------------------- the Q knob

#[test]
fn a_scroll_over_a_band_changes_its_q_by_a_ratio() {
    // Multiplicative, because Q is a ratio: a step that takes 0.5 to 1.5 would
    // take 8 to 9 and be useless at the top of the range.
    let up = eq_nudge_q(1.0, 1.0);
    let down = eq_nudge_q(1.0, -1.0);
    assert!(up > 1.0 && down < 1.0);
    assert!(
        (eq_nudge_q(8.0, 1.0) / 8.0 - up).abs() < 0.01,
        "the same step wherever it starts"
    );
}

#[test]
fn a_q_cannot_be_scrolled_out_of_the_range_a_filter_has() {
    assert!(eq_nudge_q(0.1, -100.0) >= 0.1 - 1e-6);
    assert!(eq_nudge_q(20.0, 100.0) <= 24.0 + 1e-6);
}
