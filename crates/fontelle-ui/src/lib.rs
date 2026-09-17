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

pub mod activation;
pub mod app;
pub mod audition;
pub mod backend;
pub mod branding;
pub mod canvas;
pub mod document;
pub mod file_drag;
pub mod icon;
pub mod layout;
pub mod pointer;
pub mod render;
pub mod skin;
pub mod sky;
pub mod text;
pub mod theme;
pub mod tooltip;
pub mod transport;
pub mod widget;

pub use vello;

pub use app::{WindowApp, WindowError, WindowOptions, run, run_window};
pub use audition::{AuditionAction, Auditions, MAX_AUDITION, MIN_AUDITION};
pub use backend::WindowBackend;
pub use document::{
    AudioPreview, ChannelInfo, ClipInfo, ClipKind, DocumentHost, GhostFilter, GhostNote, LaneInfo,
    LibraryEntry, PluginListing, PrefabInfo, RackTab, RecentProject, SendInfo, StudioHost,
    UpdateStatus,
};
pub use layout::{
    DEFAULT_TIMELINE_HEIGHT, EditorKind, EditorTab, EditorTabs, MIN_EDITOR_HEIGHT,
    MIN_TIMELINE_HEIGHT, PanelLayout, Rect, WindowLayout, editor_tab_at, editor_tabs,
    editor_window_layout, timeline_height_at, window_layout,
};
pub use pointer::{Pointer, PointerScene, pointer_at};
pub use render::{Chrome, Headless, RenderError, RollChrome, TransportChrome, draw_window};
pub use text::{GlyphRun, TextContext, TextLayout};
pub use theme::{Color, THEME_FORMAT_VERSION, Theme, ThemeError};
pub use transport::{
    TransportAction, TransportBarLayout, TransportHit, TransportHost, TransportView,
};
pub use widget::{Redraw, WidgetId, WidgetTree};
