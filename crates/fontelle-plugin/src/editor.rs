use fontelle_ui::backend::BaseviewBackend;

/// The sampler editor embedded in a foreign host via `baseview` (TDD §9). Reuses
/// `fontelle-ui`'s widget/canvas layer above the backend seam — the same code that
/// renders the docked panel inside the DAW itself.
pub struct PluginEditor {
    backend: BaseviewBackend,
}

impl PluginEditor {
    pub fn new() -> Self {
        Self {
            backend: BaseviewBackend::new(),
        }
    }

    pub fn backend(&self) -> &BaseviewBackend {
        &self.backend
    }
}

impl Default for PluginEditor {
    fn default() -> Self {
        Self::new()
    }
}
