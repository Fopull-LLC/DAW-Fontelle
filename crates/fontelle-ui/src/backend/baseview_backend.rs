use super::WindowBackend;

/// Used by `fontelle-plugin` to embed the sampler editor as a child window inside
/// a foreign host (TDD §9). Floating, non-dockable — that asymmetry with our own
/// panels is expected (§16.1).
pub struct BaseviewBackend {
    size: (u32, u32),
}

impl BaseviewBackend {
    pub fn new() -> Self {
        Self { size: (0, 0) }
    }
}

impl Default for BaseviewBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowBackend for BaseviewBackend {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn request_redraw(&mut self) {
        todo!("baseview::Window redraw request")
    }
}
