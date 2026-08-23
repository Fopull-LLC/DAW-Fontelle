//! Custom widget layer over `wgpu`, windowed by `winit` (app) or `baseview`
//! (plugin editor) — the same widget code above the backend seam either way
//! (FONTELLE_TDD.md §16.2). Chrome only: the timeline and piano roll are custom
//! canvases, not widgets (§16.4). Don't let this crate grow into a general-purpose
//! GUI framework — it only needs to be good enough for menus, panels, knobs, lists.

pub mod backend;
pub mod canvas;
pub mod render;
pub mod text;
pub mod theme;
pub mod widget;

pub use backend::WindowBackend;
pub use render::GpuContext;
pub use text::TextContext;
pub use theme::Theme;
pub use widget::{WidgetId, WidgetTree};
