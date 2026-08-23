/// The wgpu device/surface/pipeline state shared by every canvas and widget
/// (TDD §16.2). Vector content (icons, curves, non-instanced widget chrome) goes
/// through `vello`; the timeline and piano roll draw instanced quads directly
/// (see `crate::canvas`).
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub vector_scene: vello::Scene,
}

impl GpuContext {
    pub fn new() -> Self {
        Self {
            instance: wgpu::Instance::default(),
            vector_scene: vello::Scene::new(),
        }
    }

    pub fn request_adapter(
        &self,
    ) -> impl std::future::Future<Output = Result<wgpu::Adapter, wgpu::RequestAdapterError>> + '_
    {
        self.instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
    }
}

impl Default for GpuContext {
    fn default() -> Self {
        Self::new()
    }
}
