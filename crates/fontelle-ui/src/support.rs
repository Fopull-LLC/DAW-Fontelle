//! Where the guide's three outward links go (hub card 0427): the issue
//! board, for a bug or a wish, and the support page.
//!
//! Ty: *"accessibility is the product; nothing may nag or gate."* Nothing
//! here talks to the network. A report's address carries what the board's
//! form would otherwise ask for — the version, the system and the plugin
//! formats the song uses — and only when somebody presses the link and the
//! browser opens it; what reaches the board is then what they choose to
//! send. No plugin's name, no path, nothing about the song but its formats.

// Every address into the site says it came from the app and from where
// (hub card 0620): *"so W can count what each one brings in"*. Plausible
// reads `utm_source`/`utm_medium` with no change on the site. Written out
// whole rather than built, so a grep for an address finds it.

/// The support page, with the product named so it opens on Fontelle — the
/// guide's *Support Fontelle*, always there beside *Report a bug*.
pub const SUPPORT_URL: &str =
    "https://fopull.com/support?product=fontelle&utm_source=fontelle-app&utm_medium=help-menu";
/// The same page from the start menu's quiet line (hub card 0427).
pub const SUPPORT_URL_START_MENU: &str =
    "https://fopull.com/support?product=fontelle&utm_source=fontelle-app&utm_medium=start-menu";
/// And from the start menu's footer link (Ty, 2026-11-12).
pub const SUPPORT_URL_FOOTER: &str = "https://fopull.com/support?product=fontelle&utm_source=fontelle-app&utm_medium=start-menu-footer";
/// And from that line on the first start menu after an update.
pub const SUPPORT_URL_UPDATE: &str =
    "https://fopull.com/support?product=fontelle&utm_source=fontelle-app&utm_medium=update";
/// *What's new*, on the first start menu after an update: the releases
/// page on the site, so the visit counts there (Ty: *"send them to the site
/// so it counts as a visit"*), open at this version's notes (hub card 0665:
/// the old address landed on Fontelle's page, not a changelog). The page
/// reads each release's notes from GitHub, so the notes must be written
/// (`release-notes/`). No version, no anchor: the page opens at its newest.
/// `medium` says where it was pressed — `update` for the first start menu
/// after an update, `start-menu` for the link under the version — for W's
/// counts.
pub fn whats_new_url(version: &str, medium: &str) -> String {
    let page =
        format!("https://fopull.com/fontelle/releases?utm_source=fontelle-app&utm_medium={medium}");
    if version.is_empty() {
        page
    } else {
        format!("{page}#v{version}")
    }
}

/// What the install ping sends, and why (hub card 0428, the contract's §5).
/// The query before the fragment: after it, the browser never sends it.
pub const INSTALL_PING_URL: &str =
    "https://fopull.com/privacy?utm_source=fontelle-app&utm_medium=start-menu#install-ping";
/// The board's report form; `kind` says which.
pub const REPORT_URL: &str = "https://fopull.com/roadmap/fontelle/report";
/// What a report's address ends with: it is pressed in the guide.
const REPORT_CAMPAIGN: &str = "utm_source=fontelle-app&utm_medium=help-menu";

/// The form's `maxlength` for each environment field. Longer and the
/// browser would cut it anyway, wherever it fell; cut here, it is cut
/// between characters.
pub const ENV_VERSION_MAX: usize = 40;
pub const ENV_OS_MAX: usize = 120;
pub const ENV_PLUGIN_MAX: usize = 160;

/// Which of the board's two forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    Bug,
    Feature,
}

impl ReportKind {
    fn query(self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Feature => "feature",
        }
    }
}

/// What a report says about where it came from — the host's to fill in
/// (`DocumentHost::report_environment`), because only it knows the system
/// and the song.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportEnvironment {
    /// This build's version.
    pub version: String,
    /// "Linux (Fedora Linux 42)", "Windows", "macOS".
    pub os: String,
    /// The plugin formats the open song uses, "CLAP, VST3"; empty for none.
    pub plugins: String,
}

/// The board's form for `kind`, its environment fields filled from `env`.
/// An empty field is left out rather than sent blank, so the form shows its
/// own placeholder.
pub fn report_url(kind: ReportKind, env: &ReportEnvironment) -> String {
    let mut url = format!("{REPORT_URL}?kind={}", kind.query());
    for (name, value, max) in [
        ("env_version", &env.version, ENV_VERSION_MAX),
        ("env_os", &env.os, ENV_OS_MAX),
        ("env_plugin", &env.plugins, ENV_PLUGIN_MAX),
    ] {
        let value: String = value.trim().chars().take(max).collect();
        let value = value.trim_end();
        if value.is_empty() {
            continue;
        }
        url.push('&');
        url.push_str(name);
        url.push('=');
        url.push_str(&percent_encode(value));
    }
    url.push('&');
    url.push_str(REPORT_CAMPAIGN);
    url
}

/// Every byte but RFC 3986's unreserved ones as `%XX`, so the address is
/// plain ASCII with no `&`, `#`, space or `%` of its own to misread — the
/// browser, `xdg-open` and Windows' handler all take it as one word.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
