//! Custom widget layer over `wgpu`, windowed by `winit` (app) or `baseview`
//! (plugin editor) — the same widget code above the backend seam either way
//! (FONTELLE_TDD.md §16.2). Chrome only: the timeline and piano roll are custom
//! canvases, not widgets (§16.4). Don't let this crate grow into a general-purpose
//! GUI framework — it only needs to be good enough for menus, panels, knobs, lists.
//!
//! # How this crate is tested
//!
//! `docs/first-usable-plan.md` §2.5 is the rule: **everything that can be a pure
//! function is one, and the tests live there.** So the theme is data, the
//! geometry is arithmetic, the glyph positions are arithmetic, and the decision
//! of *whether a frame happens at all* is a value — all of which are tested
//! without a window, in `tests/`. [`render::Headless`] then renders the real
//! scene through the real pipeline with no device attached, which is to the
//! window what `fontelle_app::render_offline` is to the audio path.
//!
//! What is left in [`app`] is only the part that genuinely needs a window.

pub mod app;
pub mod backend;
pub mod canvas;
pub mod document;
pub mod layout;
pub mod render;
pub mod text;
pub mod theme;
pub mod transport;
pub mod widget;

pub use vello;

pub use app::{WindowApp, WindowError, WindowOptions, run, run_window};
pub use backend::WindowBackend;
pub use document::DocumentHost;
pub use layout::{PanelLayout, Rect, WindowLayout, window_layout};
pub use render::{Chrome, Headless, RenderError, RollChrome, TransportChrome, draw_window};
pub use text::{GlyphRun, TextContext, TextLayout};
pub use theme::{Color, THEME_FORMAT_VERSION, Theme, ThemeError};
pub use transport::{TransportBarLayout, TransportHit, TransportHost, TransportView};
pub use widget::{Redraw, WidgetId, WidgetTree};
