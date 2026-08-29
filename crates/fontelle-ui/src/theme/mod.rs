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
//!   "format_version": 0,
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
pub const THEME_FORMAT_VERSION: u32 = 0;

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
                window: Color::rgb(0x0e, 0x0e, 0x11),
                panel: Color::rgb(0x17, 0x17, 0x1c),
                panel_header: Color::rgb(0x1f, 0x1f, 0x26),
                border: Color::rgb(0x2c, 0x2c, 0x35),
                text: Color::rgb(0xe6, 0xe6, 0xeb),
                text_muted: Color::rgb(0x94, 0x94, 0xa2),
                accent: Color::rgb(0x4f, 0x8f, 0xd0),
                grid_line: Color::rgb(0x25, 0x25, 0x2d),
                grid_line_strong: Color::rgb(0x3a, 0x3a, 0x46),
                playhead: Color::rgb(0xf2, 0xc0, 0x4c),
                selection: Color::rgba(0x4f, 0x8f, 0xd0, 0x59),
                meter: Color::rgb(0x5c, 0xc9, 0x8a),
                meter_peak: Color::rgb(0xe0, 0x5c, 0x5c),
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
                window: Color::rgb(0xd8, 0xd8, 0xdd),
                panel: Color::rgb(0xf2, 0xf2, 0xf5),
                panel_header: Color::rgb(0xe4, 0xe4, 0xea),
                border: Color::rgb(0xc2, 0xc2, 0xcc),
                text: Color::rgb(0x1b, 0x1b, 0x22),
                text_muted: Color::rgb(0x5e, 0x5e, 0x6b),
                accent: Color::rgb(0x1f, 0x5f, 0xa8),
                grid_line: Color::rgb(0xdc, 0xdc, 0xe3),
                grid_line_strong: Color::rgb(0xbb, 0xbb, 0xc6),
                playhead: Color::rgb(0xb8, 0x7d, 0x0a),
                selection: Color::rgba(0x1f, 0x5f, 0xa8, 0x40),
                meter: Color::rgb(0x2e, 0x8b, 0x57),
                meter_peak: Color::rgb(0xc0, 0x2f, 0x2f),
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
};

/// Brings a theme written by an older build up to [`THEME_FORMAT_VERSION`], one
/// step per revision. There is nothing before v0, so the chain is empty — the
/// shape is written down because the first migration is the one most likely to
/// be added in a hurry. See `fontelle_model::storage`, which does the same.
fn migrate(json: serde_json::Value, from: u32) -> Result<serde_json::Value, ThemeError> {
    if from != THEME_FORMAT_VERSION {
        return Err(ThemeError::Format(format!(
            "no migration from theme format version {from} to {THEME_FORMAT_VERSION}"
        )));
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
