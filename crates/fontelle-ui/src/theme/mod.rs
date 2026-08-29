//! Token-based theming (TDD §16.6).
//!
//! A theme is a file. Fontelle ships a dark default and a light variant, and a
//! user theme is one of those written out, edited, and pointed at — there is no
//! theme editor in v1 and there does not need to be, because the format is
//! small enough to read.
//!
//! # The file format
//!
//! JSON, pretty-printed, for the same reasons §17.2 chose it for the document:
//! diffable, greppable, and fixable by hand when something goes wrong.
//!
//! ```json
//! {
//!   "format_version": 2,
//!   "name": "Fontelle Dark",
//!   "palette": {
//!     "window": "#0e0e11",
//!     "panel": "#17171c",
//!     ...
//!   },
//!   "metrics": { "panel_margin": 8.0, ... },
//!   "font": { "family": "sans-serif", "size": 13.0, "line_height": 1.35 }
//! }
//! ```
//!
//! Colours are `#rrggbb`, or `#rrggbbaa` when they are not opaque. Every field
//! is required: a half-specified theme would render half in someone else's
//! colours, which is worse than refusing to load. `format_version` is read
//! before the body, so a theme from a newer build is refused by version rather
//! than by whichever field happened to change shape first — the same rule the
//! project document follows.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The revision of the theme format this build writes.
///
/// Its own number, separate from the project's and the patch's: a colour token
/// added to the chrome has nothing to do with either.
pub const THEME_FORMAT_VERSION: u32 = 3;

/// An 8-bit sRGB colour with alpha, written to file as hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color(pub [u8; 4]);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 0xff])
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

    /// `#rrggbb` when opaque, `#rrggbbaa` when not.
    ///
    /// Dropping a redundant `ff` matters more than it sounds: nearly every
    /// token is opaque, and a file full of `#0e0e11ff` is a file people stop
    /// reading.
    pub fn to_hex(&self) -> String {
        let [r, g, b, a] = self.0;
        if a == 0xff {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }

    pub fn parse(s: &str) -> Result<Self, ColorParseError> {
        let hex = s
            .strip_prefix('#')
            .ok_or_else(|| ColorParseError(s.to_string()))?;
        if hex.len() != 6 && hex.len() != 8 {
            return Err(ColorParseError(s.to_string()));
        }
        let byte = |i: usize| {
            u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| ColorParseError(s.to_string()))
        };
        Ok(Self([
            byte(0)?,
            byte(2)?,
            byte(4)?,
            if hex.len() == 8 { byte(6)? } else { 0xff },
        ]))
    }

    /// The form `vello` paints with.
    pub fn to_peniko(self) -> vello::peniko::Color {
        let [r, g, b, a] = self.0;
        vello::peniko::Color::from_rgba8(r, g, b, a)
    }
}

/// A string that was meant to be a colour and was not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorParseError(pub String);

impl std::fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} is not a colour — write #rrggbb, or #rrggbbaa for transparency",
            self.0
        )
    }
}

impl std::error::Error for ColorParseError {}

impl TryFrom<String> for Color {
    type Error = ColorParseError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl From<Color> for String {
    fn from(c: Color) -> Self {
        c.to_hex()
    }
}

/// Every colour the chrome draws with.
///
/// Deliberately a closed list rather than a map of arbitrary names: a theme
/// that can be missing a token is a theme that renders a hole, and the point of
/// naming them here is that adding one is a visible change to the format.
///
/// The set covers the panels named in §16.1 rather than only the one panel item
/// 6 draws — a token added later is a format revision, and the timeline,
/// piano roll and mixer already know which colours they will ask for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    /// Behind everything, where no panel reaches.
    pub window: Color,
    /// A panel's body.
    pub panel: Color,
    /// The strip along a panel's top carrying its name.
    pub panel_header: Color,
    /// Between panels and around controls.
    pub border: Color,
    pub text: Color,
    /// Labels, units, counts — quieter, still readable.
    pub text_muted: Color,
    /// The one colour that says "this is Fontelle": focus rings, active
    /// controls, the record button.
    pub accent: Color,
    /// Beat lines in the timeline and piano roll.
    pub grid_line: Color,
    /// Bar lines, which have to be tellable from beat lines at a glance.
    pub grid_line_strong: Color,
    pub playhead: Color,
    /// Fill over selected notes and clips; usually translucent.
    pub selection: Color,
    /// A meter below its warning point.
    pub meter: Color,
    /// A meter at or above it, and the limiter's gain-reduction readout.
    pub meter_peak: Color,

    // --- the piano roll and the timeline (added in theme format v2) ---
    /// A note block's default fill. Per-channel colours override it later.
    pub note: Color,
    pub note_selected: Color,
    /// The natural keys down the side of the piano roll.
    pub key_white: Color,
    pub key_black: Color,
    /// The roll's row behind a black key — a shade off the panel, so octaves
    /// are countable without drawing a line for every one.
    pub row_accidental: Color,
}

/// Sizes and radii, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    /// Between the window edge and the panels inside it.
    pub panel_margin: f32,
    pub panel_header_height: f32,
    /// Between a panel's frame and its contents.
    pub panel_padding: f32,
    pub border_width: f32,
    pub corner_radius: f32,
    /// One line in a list: channels, presets, the browser.
    pub row_height: f32,
    /// The strip across the top of the window carrying play/stop and the
    /// playhead. Added in theme format v1.
    pub transport_bar_height: f32,
    /// How wide the left-hand column of docked panels — the channel rack and
    /// the soundfont browser — is. Added in theme format v3.
    ///
    /// Wide enough for a soundfont's name and narrow enough that the piano
    /// roll is still the thing the window is mostly made of.
    pub sidebar_width: f32,
}

/// The chrome's typeface.
///
/// `family` is a `cosmic-text` family name, so the generic names
/// (`sans-serif`, `monospace`) work and resolve to whatever the machine has —
/// which is what a theme file should say unless its author is shipping a font
/// alongside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontTokens {
    pub family: String,
    /// In logical pixels.
    pub size: f32,
    /// A multiple of `size`.
    pub line_height: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub format_version: u32,
    pub name: String,
    pub palette: Palette,
    pub metrics: Metrics,
    pub font: FontTokens,
}

impl Theme {
    /// The default. Chosen against WCAG contrast rather than by eye — the
    /// chrome is small and dense, and a DAW is looked at for hours.
    pub fn dark_default() -> Self {
        Self {
            format_version: THEME_FORMAT_VERSION,
            name: "Fontelle Dark".to_string(),
            palette: Palette {
                // The primary ramp's dark end carries the structure. Kept
                // near-neutral on purpose: a fully saturated teal UI reads as
                // a skin, and the brief was FL Studio's newer look — neutral
                // surfaces, colour reserved for things that mean something.
                window: Color::rgb(0x06, 0x10, 0x13),
                panel: Color::rgb(0x0e, 0x1f, 0x26),
                panel_header: Color::rgb(0x11, 0x26, 0x2e),
                border: Color::rgb(0x1f, 0x40, 0x4c),
                text: Color::rgb(0xdc, 0xe8, 0xec),
                text_muted: Color::rgb(0x6e, 0x93, 0xa0),
                // The top of the primary ramp: the one colour that says
                // Fontelle.
                accent: Color::rgb(0x40, 0x85, 0x9c),
                grid_line: Color::rgb(0x11, 0x26, 0x2e),
                grid_line_strong: Color::rgb(0x1f, 0x40, 0x4c),
                // Green, from the first secondary ramp, so the playhead never
                // competes with the teal chrome it travels over.
                playhead: Color::rgb(0x40, 0xa4, 0x88),
                // Blue, from the second, translucent over whatever it covers.
                selection: Color::rgba(0x49, 0x6f, 0xa4, 0x59),
                meter: Color::rgb(0x40, 0xa4, 0x88),
                // The one colour not in the three ramps, and deliberately so:
                // clipping is a signal, not a brand. Nothing in a teal, green
                // and blue palette can say "too loud", and a meter that cannot
                // is not a meter.
                meter_peak: Color::rgb(0xc4, 0x60, 0x5c),
                note: Color::rgb(0x49, 0x6f, 0xa4),
                note_selected: Color::rgb(0xa8, 0xc4, 0xe4),
                key_white: Color::rgb(0xc9, 0xd6, 0xda),
                key_black: Color::rgb(0x11, 0x26, 0x2e),
                row_accidental: Color::rgb(0x0a, 0x18, 0x1e),
            },
            metrics: METRICS,
            font: FontTokens {
                family: "sans-serif".to_string(),
                size: 13.0,
                line_height: 1.35,
            },
        }
    }

    /// The same theme in daylight. Same metrics, same font, inverted ground —
    /// which is what keeps it a variant rather than a second design.
    pub fn light_default() -> Self {
        Self {
            format_version: THEME_FORMAT_VERSION,
            name: "Fontelle Light".to_string(),
            palette: Palette {
                // The same three ramps, read from the other end.
                window: Color::rgb(0xc9, 0xd6, 0xda),
                panel: Color::rgb(0xe4, 0xed, 0xef),
                panel_header: Color::rgb(0xd3, 0xe0, 0xe4),
                border: Color::rgb(0xa9, 0xc0, 0xc7),
                text: Color::rgb(0x06, 0x10, 0x13),
                text_muted: Color::rgb(0x31, 0x62, 0x72),
                accent: Color::rgb(0x31, 0x62, 0x72),
                grid_line: Color::rgb(0xc9, 0xd6, 0xda),
                grid_line_strong: Color::rgb(0xa9, 0xc0, 0xc7),
                playhead: Color::rgb(0x1e, 0x4f, 0x42),
                selection: Color::rgba(0x49, 0x6f, 0xa4, 0x40),
                meter: Color::rgb(0x31, 0x77, 0x64),
                meter_peak: Color::rgb(0xa8, 0x43, 0x3f),
                note: Color::rgb(0x37, 0x52, 0x78),
                note_selected: Color::rgb(0x49, 0x6f, 0xa4),
                key_white: Color::rgb(0xf5, 0xf9, 0xfa),
                key_black: Color::rgb(0x23, 0x36, 0x50),
                row_accidental: Color::rgb(0xd8, 0xe4, 0xe7),
            },
            metrics: METRICS,
            font: FontTokens {
                family: "sans-serif".to_string(),
                size: 13.0,
                line_height: 1.35,
            },
        }
    }

    pub fn to_json(&self) -> String {
        let mut theme = self.clone();
        theme.format_version = THEME_FORMAT_VERSION;
        // Pretty, because the format exists to be edited.
        serde_json::to_string_pretty(&theme).expect("a Theme is always serialisable")
    }

    /// Reads a theme, checking its version before its body.
    pub fn from_json(text: &str) -> Result<Self, ThemeError> {
        let json: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| ThemeError::Format(format!("this is not readable JSON: {e}")))?;

        let found = json
            .get("format_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                ThemeError::Format(
                    "this file has no format_version — it is not a Fontelle theme".to_string(),
                )
            })? as u32;
        if found > THEME_FORMAT_VERSION {
            return Err(ThemeError::FromTheFuture {
                found,
                newest: THEME_FORMAT_VERSION,
            });
        }
        let json = migrate(json, found)?;

        serde_json::from_value(json)
            .map_err(|e| ThemeError::Format(format!("this theme could not be read: {e}")))
    }

    pub fn load_from_file(path: &Path) -> Result<Self, ThemeError> {
        let text = std::fs::read_to_string(path).map_err(|source| ThemeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_json(&text).map_err(|e| match e {
            // Say which file, since a theme is loaded by path from settings and
            // the path is the only way to tell two of them apart.
            ThemeError::Format(why) => ThemeError::Format(format!("{}: {why}", path.display())),
            other => other,
        })
    }
}

/// The one set of metrics both defaults share.
const METRICS: Metrics = Metrics {
    panel_margin: 8.0,
    panel_header_height: 26.0,
    panel_padding: 8.0,
    border_width: 1.0,
    corner_radius: 4.0,
    row_height: 22.0,
    transport_bar_height: 34.0,
    sidebar_width: 248.0,
};

/// Brings a theme written by an older build up to [`THEME_FORMAT_VERSION`],
/// one step per revision, each rewriting the document from `N` to `N + 1`.
///
/// The rule this follows, and the reason it is not just `serde(default)`: a
/// missing token is only allowed to be filled in *by a migration that says
/// which version it is filling it in for*. `deny_unknown_fields` plus required
/// fields is what makes a typo in a hand-edited theme an error rather than a
/// silently ignored line, and defaulting everything would give that up
/// everywhere to solve it in one place. See `fontelle_model::storage`, which
/// does the same for the document.
fn migrate(mut json: serde_json::Value, mut from: u32) -> Result<serde_json::Value, ThemeError> {
    if from == 0 {
        // v1 added the transport bar (item 7 of `docs/first-usable-plan.md`).
        // A v0 file predates it and cannot have an opinion about its height.
        if let Some(metrics) = json.get_mut("metrics").and_then(|m| m.as_object_mut()) {
            metrics
                .entry("transport_bar_height")
                .or_insert_with(|| serde_json::json!(METRICS.transport_bar_height));
        }
        from = 1;
    }

    if from == 1 {
        // v2 added the piano roll's own colours (item 8). A v1 file was
        // written before there was a roll to have an opinion about.
        let dark = Theme::dark_default().palette;
        if let Some(palette) = json.get_mut("palette").and_then(|p| p.as_object_mut()) {
            for (key, value) in [
                ("note", dark.note),
                ("note_selected", dark.note_selected),
                ("key_white", dark.key_white),
                ("key_black", dark.key_black),
                ("row_accidental", dark.row_accidental),
            ] {
                palette
                    .entry(key)
                    .or_insert_with(|| serde_json::json!(value.to_hex()));
            }
        }
        from = 2;
    }

    if from == 2 {
        // v3 added the docked sidebar (item 9). A v2 file was written when the
        // window was a transport bar and one panel.
        if let Some(metrics) = json.get_mut("metrics").and_then(|m| m.as_object_mut()) {
            metrics
                .entry("sidebar_width")
                .or_insert_with(|| serde_json::json!(METRICS.sidebar_width));
        }
        from = 3;
    }

    if from != THEME_FORMAT_VERSION {
        return Err(ThemeError::Format(format!(
            "no migration from theme format version {from} to {THEME_FORMAT_VERSION}"
        )));
    }
    // A migrated theme *is* a current-version theme. Leaving the old stamp on
    // it would make the loaded value disagree with what it now contains, and
    // the next thing to write it out would silently claim a version it had
    // already left.
    if let Some(object) = json.as_object_mut() {
        object.insert(
            "format_version".to_string(),
            serde_json::json!(THEME_FORMAT_VERSION),
        );
    }
    Ok(json)
}

#[derive(Debug)]
pub enum ThemeError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Written by a build newer than this one.
    FromTheFuture {
        found: u32,
        newest: u32,
    },
    Format(String),
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::FromTheFuture { found, newest } => write!(
                f,
                "this theme is in format version {found}, and this build of Fontelle \
                 understands up to version {newest} — upgrade Fontelle to use it"
            ),
            Self::Format(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ThemeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
