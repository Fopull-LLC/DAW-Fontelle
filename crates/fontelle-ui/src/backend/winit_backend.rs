use std::sync::Arc;

use super::WindowBackend;

/// Used by `fontelle-app` for the DAW's own docked-panel window.
pub struct WinitBackend {
    window: Option<Arc<winit::window::Window>>,
}

impl WinitBackend {
    pub fn new() -> Self {
        Self { window: None }
    }
}

impl Default for WinitBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowBackend for WinitBackend {
    fn size(&self) -> (u32, u32) {
        self.window
            .as_ref()
            .map(|w| w.inner_size().into())
            .unwrap_or((0, 0))
    }

    fn request_redraw(&mut self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
