/// Shaping, layout, and font-fallback via `cosmic-text` (TDD §16.2).
pub struct TextContext {
    pub font_system: cosmic_text::FontSystem,
}

impl TextContext {
    pub fn new() -> Self {
        Self {
            font_system: cosmic_text::FontSystem::new(),
        }
    }
}

impl Default for TextContext {
    fn default() -> Self {
        Self::new()
    }
}
