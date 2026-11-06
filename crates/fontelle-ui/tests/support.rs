//! Help ▸ Report a bug, Request a feature, Support Fontelle, and the one
//! quiet line on the start menu (hub card 0427).
//!
//! Ty: *"accessibility is the product; nothing may nag or gate."* So the
//! links are links — nothing is sent until somebody presses one, and then
//! only what is in the address — and the line is said once, never over a
//! song somebody could lose.

use fontelle_ui::canvas::{
    HELP_LINKS, HelpHit, HelpLink, SUPPORT_LINE_LAUNCH, SUPPORT_LINE_LEAD, SUPPORT_LINE_LINK,
    SUPPORT_LINE_SPACING_DAYS, SUPPORT_LINE_TEXT, StartLine, WEBSITE_URL, WelcomeHit,
    WelcomeLayout, help_hit, help_layout, support_after_update_due, support_line_due, welcome_hit,
    welcome_layout, with_learn, with_recovery, with_support_line,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::support::{
    ENV_OS_MAX, ENV_PLUGIN_MAX, ENV_VERSION_MAX, INSTALL_PING_URL, REPORT_URL, ReportEnvironment,
    ReportKind, SUPPORT_URL, SUPPORT_URL_START_MENU, SUPPORT_URL_UPDATE, report_url,
};
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
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

fn env(version: &str, os: &str, plugins: &str) -> ReportEnvironment {
    ReportEnvironment {
        version: version.to_string(),
        os: os.to_string(),
        plugins: plugins.to_string(),
    }
}

/// The query's value for `name`, still percent-encoded.
fn param<'a>(url: &'a str, name: &str) -> Option<&'a str> {
    let (_, query) = url.split_once('?')?;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

/// Undoes the encoding, so a test can say what the form will show.
fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(&value[i + 1..i + 3], 16).expect("two hex digits"));
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).expect("UTF-8")
}

// ------------------------------------------------------------- the links

#[test]
fn the_addresses_are_the_live_boards() {
    assert!(SUPPORT_URL.starts_with("https://fopull.com/support?product=fontelle&"));
    assert_eq!(REPORT_URL, "https://fopull.com/roadmap/fontelle/report");
}

// Hub card 0620: *"Every link the app opens to fopull.com carries a campaign
// tag, so W can count what each one brings in."* Plausible reads `utm_*`
// with no change on the site; `product=fontelle` stays on the support page.

#[test]
fn every_address_into_the_site_says_it_came_from_the_app_and_where() {
    assert_eq!(
        SUPPORT_URL,
        "https://fopull.com/support?product=fontelle&utm_source=fontelle-app&utm_medium=help-menu"
    );
    assert_eq!(
        SUPPORT_URL_START_MENU,
        "https://fopull.com/support?product=fontelle&utm_source=fontelle-app&utm_medium=start-menu"
    );
    assert_eq!(
        SUPPORT_URL_UPDATE,
        "https://fopull.com/support?product=fontelle&utm_source=fontelle-app&utm_medium=update"
    );
    assert_eq!(
        WEBSITE_URL,
        "https://fopull.com/?utm_source=fontelle-app&utm_medium=start-menu"
    );
    // The query goes before the fragment, or the browser never sends it.
    assert_eq!(
        INSTALL_PING_URL,
        "https://fopull.com/privacy?utm_source=fontelle-app&utm_medium=start-menu#install-ping"
    );
}

#[test]
fn a_report_says_it_came_from_the_guide_after_its_fields() {
    for kind in [ReportKind::Bug, ReportKind::Feature] {
        let url = report_url(kind, &env("0.34.0", "Windows", "LV2"));
        assert!(
            url.ends_with("&utm_source=fontelle-app&utm_medium=help-menu"),
            "{url}"
        );
        assert_eq!(url.matches('?').count(), 1, "{url}");
    }
    // With no fields to fill, still one query.
    let url = report_url(ReportKind::Bug, &env("", "", ""));
    assert_eq!(
        url,
        "https://fopull.com/roadmap/fontelle/report?kind=bug&utm_source=fontelle-app&utm_medium=help-menu"
    );
}

#[test]
fn each_start_line_goes_to_its_own_tagged_address() {
    assert_eq!(StartLine::Support.url(), SUPPORT_URL_START_MENU);
    assert_eq!(StartLine::AfterUpdate.url(), SUPPORT_URL_UPDATE);
    assert_eq!(StartLine::Ping.url(), INSTALL_PING_URL);
    // The same words wherever it is said (hub card 0620: "same words as 0427").
    assert_eq!(StartLine::AfterUpdate.lead(), StartLine::Support.lead());
    assert_eq!(StartLine::AfterUpdate.link(), StartLine::Support.link());
}

#[test]
fn a_bug_report_carries_the_version_the_system_and_the_formats() {
    let url = report_url(
        ReportKind::Bug,
        &env("0.29.0-beta.1", "Linux (Fedora Linux 42)", "CLAP, VST3"),
    );
    assert!(url.starts_with("https://fopull.com/roadmap/fontelle/report?kind=bug&"));
    assert_eq!(param(&url, "env_version"), Some("0.29.0-beta.1"));
    assert_eq!(
        param(&url, "env_os"),
        Some("Linux%20%28Fedora%20Linux%2042%29")
    );
    assert_eq!(param(&url, "env_plugin"), Some("CLAP%2C%20VST3"));
}

#[test]
fn a_feature_request_carries_the_version_and_the_system_too() {
    let url = report_url(ReportKind::Feature, &env("0.29.0", "Windows", "LV2"));
    assert!(url.starts_with("https://fopull.com/roadmap/fontelle/report?kind=feature&"));
    assert_eq!(param(&url, "env_version"), Some("0.29.0"));
    assert_eq!(param(&url, "env_os"), Some("Windows"));
    assert_eq!(param(&url, "env_plugin"), Some("LV2"));
}

#[test]
fn a_song_with_no_plugins_sends_no_plugin_field() {
    let url = report_url(ReportKind::Bug, &env("0.29.0", "macOS", ""));
    assert_eq!(param(&url, "env_plugin"), None, "{url}");
    assert!(!url.contains("env_plugin"));
    // Nor an empty system, should one not be known.
    let url = report_url(ReportKind::Bug, &env("0.29.0", "", ""));
    assert!(!url.contains("env_os"), "{url}");
}

#[test]
fn anything_that_could_break_the_query_is_encoded() {
    // An `&` or `=` or `#` in a distribution's name must not start a field
    // of its own or end the address; and the address must be plain ASCII,
    // which is all `cmd` and `xdg-open` can be trusted to pass along.
    let url = report_url(
        ReportKind::Bug,
        &env("1.0", "Linux (Tom & Jerry = #1 ünïcode/OS?)", ""),
    );
    assert!(url.is_ascii(), "{url}");
    // Kind, version, os — then the campaign's two, which are ours.
    assert_eq!(url.matches('&').count(), 4, "kind, version, os: {url}");
    assert_eq!(url.matches('?').count(), 1, "{url}");
    assert!(!url.contains('#') && !url.contains(' '), "{url}");
    assert_eq!(
        decode(param(&url, "env_os").unwrap()),
        "Linux (Tom & Jerry = #1 ünïcode/OS?)"
    );
}

#[test]
fn each_field_is_kept_within_what_the_form_accepts() {
    // The form's inputs are maxlength 40, 120 and 160: longer and the
    // browser cuts it anyway, somewhere less tidy.
    assert_eq!(
        (ENV_VERSION_MAX, ENV_OS_MAX, ENV_PLUGIN_MAX),
        (40, 120, 160)
    );
    let long = |c: char, n: usize| std::iter::repeat_n(c, n).collect::<String>();
    let url = report_url(
        ReportKind::Bug,
        &env(&long('9', 90), &long('é', 300), &long('V', 500)),
    );
    let field = |name| decode(param(&url, name).unwrap());
    assert_eq!(field("env_version").chars().count(), 40);
    assert_eq!(field("env_os").chars().count(), 120);
    assert_eq!(field("env_plugin").chars().count(), 160);
}

#[test]
fn the_guide_offers_three_ways_out_in_its_heading() {
    let window = Rect::new(0.0, 0.0, 1280.0, 720.0);
    let l = help_layout(window, &metrics(), 0, &[], 0.0);
    assert_eq!(
        HELP_LINKS.map(|(_, label)| label),
        ["Report a bug", "Request a feature", "Support Fontelle"]
    );
    assert_eq!(l.links.len(), 3);
    for (i, (link, rect)) in l.links.iter().enumerate() {
        assert_eq!(*link, HELP_LINKS[i].0);
        assert!(!rect.is_empty(), "{link:?} has no room");
        assert!(within(*rect, l.frame), "{link:?} leaves the card");
        // In the heading's row: the nav column is full at this height (its
        // fourteen sections and the shortcuts page reach the card's foot),
        // and a link that leaves the app is not a page of the guide.
        assert!(rect.y >= l.title.y - 0.01 && rect.bottom() <= l.body.y);
        assert!(!overlaps(*rect, l.close), "{link:?} under the close button");
        assert!(rect.right() <= l.close.x, "{link:?} past the close button");
        assert_eq!(help_hit(&l, centre(*rect).0, centre(*rect).1), {
            HelpHit::Link(*link)
        });
        for (other, r) in &l.links[i + 1..] {
            assert!(!overlaps(*rect, *r), "{link:?} on {other:?}");
        }
    }
    assert!(
        l.links.iter().all(|(_, r)| !overlaps(*r, l.title)),
        "the title keeps its word"
    );
    assert_eq!(
        l.links.iter().map(|(link, _)| *link).collect::<Vec<_>>(),
        vec![
            HelpLink::ReportBug,
            HelpLink::RequestFeature,
            HelpLink::Support
        ]
    );
}

#[test]
fn a_window_too_narrow_for_the_links_drops_them_rather_than_overlapping() {
    let window = Rect::new(0.0, 0.0, 300.0, 500.0);
    let l = help_layout(window, &metrics(), 0, &[], 0.0);
    for (link, rect) in &l.links {
        assert!(
            rect.is_empty() || (within(*rect, l.frame) && !overlaps(*rect, l.close)),
            "{link:?} at {rect:?}"
        );
    }
}

// --------------------------------------------------------- the quiet line

#[test]
fn the_line_says_exactly_what_the_card_says() {
    assert_eq!(
        SUPPORT_LINE_TEXT,
        "Fontelle is free. If it has earned something, fopull.com/support"
    );
    assert_eq!(
        format!("{SUPPORT_LINE_LEAD}{SUPPORT_LINE_LINK}"),
        SUPPORT_LINE_TEXT
    );
    // Ty, 2026-11-03 (hub card 0620): the fifth, not the tenth.
    assert_eq!(SUPPORT_LINE_LAUNCH, 5);
}

#[test]
fn the_line_is_due_from_the_fifth_launch_once_and_never_over_unsaved_work() {
    // (launches, already said, recovery offered, unsaved song open)
    assert!(!support_line_due(1, false, false, false));
    assert!(!support_line_due(4, false, false, false), "fourth: no");
    assert!(support_line_due(5, false, false, false), "fifth: yes");
    assert!(
        !support_line_due(5, true, false, false),
        "said: never again"
    );
    assert!(!support_line_due(6, true, false, false), "sixth: no");
    // Deferred, not lost: a fifth launch offering a recovery, or with a song
    // unsaved, says nothing — and the next clean launch says it.
    assert!(
        !support_line_due(5, false, true, false),
        "recovery: not now"
    );
    assert!(!support_line_due(5, false, false, true), "unsaved: not now");
    assert!(
        support_line_due(6, false, false, false),
        "the next clean one"
    );
}

#[test]
fn after_an_update_the_line_is_said_at_most_once_a_month() {
    // Hub card 0620: the first start menu of a new version is the one time a
    // returning user is reading something from us — "at most once every 30
    // days, closable … never modal".
    assert_eq!(SUPPORT_LINE_SPACING_DAYS, 30);
    // (just updated, days since the line was last said, recovery, unsaved)
    assert!(
        support_after_update_due(true, None, false, false),
        "never said"
    );
    assert!(support_after_update_due(true, Some(30), false, false));
    assert!(support_after_update_due(true, Some(400), false, false));
    assert!(
        !support_after_update_due(true, Some(29), false, false),
        "said last week: not again"
    );
    assert!(
        !support_after_update_due(true, Some(0), false, false),
        "said today"
    );
    assert!(
        !support_after_update_due(false, None, false, false),
        "an ordinary launch is not an update"
    );
    assert!(
        !support_after_update_due(true, None, true, false),
        "recovery"
    );
    assert!(
        !support_after_update_due(true, None, false, true),
        "unsaved"
    );
}

fn card() -> WelcomeLayout {
    with_learn(
        welcome_layout(Rect::new(0.0, 0.0, 1280.0, 720.0), &metrics(), 6, true),
        &metrics(),
        false,
    )
}

#[test]
fn not_shown_the_card_is_exactly_what_it_was() {
    let plain = card();
    assert_eq!(with_support_line(plain.clone(), &metrics(), false), plain);
    assert_eq!(plain.support, None);
    assert_eq!(plain.support_dismiss, None);
}

#[test]
fn shown_it_is_one_row_above_the_footer_with_its_own_cross() {
    let plain = card();
    let l = with_support_line(plain.clone(), &metrics(), true);
    let line = l.support.expect("the line");
    let cross = l.support_dismiss.expect("its ×");
    let row = metrics().row_height;
    assert!(within(line, l.frame) && within(cross, l.frame));
    assert!((line.height - row).abs() < 0.5, "one line: {line:?}");
    assert!(line.bottom() <= l.footer.y, "above the footer");
    assert!(cross.x >= line.right() - 0.01, "the × after the words");
    assert!((cross.y - line.y).abs() < 0.5, "on the same row");
    // Across the card: the words want more room than one column has.
    assert!(line.width + cross.width > 0.7 * l.footer.width, "{line:?}");
    // Nothing pressable sits under it.
    let mut pressables = vec![
        l.new_button,
        l.open_button,
        l.join_button,
        l.website,
        l.repository,
        l.logs,
        l.learn,
        l.help,
        l.settings,
    ];
    pressables.extend(l.rows.iter().map(|r| r.frame));
    pressables.extend(l.update_button);
    for r in pressables {
        assert!(!overlaps(r, line) && !overlaps(r, cross), "{r:?} under it");
        assert!(within(r, l.frame), "{r:?} pushed off the card");
    }
    assert!(!overlaps(l.message, line));
    // The ways in are still the ways in, a row higher.
    assert!(l.join_button.y < plain.join_button.y);
    assert_eq!(l.footer, plain.footer, "the footer stays put");
    assert_eq!(
        welcome_hit(&l, centre(line).0, centre(line).1),
        Some(WelcomeHit::Support)
    );
    assert_eq!(
        welcome_hit(&l, centre(cross).0, centre(cross).1),
        Some(WelcomeHit::DismissSupport)
    );
}

#[test]
fn on_a_short_card_with_everything_on_it_nothing_collides() {
    let m = metrics();
    let l = with_support_line(
        with_recovery(
            with_learn(
                welcome_layout(Rect::new(0.0, 0.0, 900.0, 560.0), &m, 9, true),
                &m,
                true,
            ),
            false,
        ),
        &m,
        true,
    );
    let line = l.support.unwrap();
    let mut pressables = vec![l.new_button, l.open_button, l.join_button, l.learn];
    pressables.extend(l.learn_dismiss);
    pressables.extend(l.update_button);
    pressables.extend(l.rows.iter().map(|r| r.frame));
    for (i, a) in pressables.iter().enumerate() {
        assert!(!overlaps(*a, line), "{a:?} under the line");
        for b in &pressables[i + 1..] {
            assert!(!overlaps(*a, *b), "{a:?} on {b:?}");
        }
    }
    if let Some(card) = l.learn_card {
        assert!(!overlaps(card, line));
    }
}

/// Hub card 0665: the button goes straight to this version's entry on the
/// releases page (W keeps the old address redirecting for 0.34.0).
#[test]
fn whats_new_opens_this_versions_notes() {
    assert_eq!(
        fontelle_ui::support::whats_new_url("0.35.0"),
        "https://fopull.com/fontelle/releases?utm_source=fontelle-app&utm_medium=update#v0.35.0"
    );
    // A window told no version: the page, at its newest.
    assert_eq!(
        fontelle_ui::support::whats_new_url(""),
        "https://fopull.com/fontelle/releases?utm_source=fontelle-app&utm_medium=update"
    );
}
