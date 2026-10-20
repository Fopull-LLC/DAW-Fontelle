//! Where the guide's three outward links go (hub card 0427): the issue
//! board, for a bug or a wish, and the support page.
//!
//! Ty: *"accessibility is the product; nothing may nag or gate."* Nothing
//! here talks to the network. A report's address carries what the board's
//! form would otherwise ask for — the version, the system and the plugin
//! formats the song uses — and only when somebody presses the link and the
//! browser opens it; what reaches the board is then what they choose to
//! send. No plugin's name, no path, nothing about the song but its formats.

/// The support page, with the product named so it opens on Fontelle.
pub const SUPPORT_URL: &str = "https://fopull.com/support?product=fontelle";

/// What the install ping sends, and why (hub card 0428, the contract's §5).
pub const INSTALL_PING_URL: &str = "https://fopull.com/privacy#install-ping";
/// The board's report form; `kind` says which.
pub const REPORT_URL: &str = "https://fopull.com/roadmap/fontelle/report";

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
