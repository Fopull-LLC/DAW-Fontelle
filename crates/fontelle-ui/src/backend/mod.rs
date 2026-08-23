mod baseview_backend;
mod winit_backend;

pub use baseview_backend::BaseviewBackend;
pub use winit_backend::WinitBackend;

/// The single seam between the two host environments the sampler editor must
/// render into: a docked panel inside the DAW (`winit`) and an OS child window
/// inside a foreign plugin host (`baseview`). Everything above this trait — the
/// widget tree, the canvases — is shared (TDD §9, §16.2).
pub trait WindowBackend {
    fn size(&self) -> (u32, u32);
    fn request_redraw(&mut self);
}
