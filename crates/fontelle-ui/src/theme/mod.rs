/// Token-based theme loaded from a file — colours, metrics, radii, font. Ship a
/// dark default and a light variant; user themes are just files in the config
/// directory. No in-app theme editor in v1 (TDD §16.6).
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub background: [u8; 4],
    pub foreground: [u8; 4],
    pub accent: [u8; 4],
    pub grid_line: [u8; 4],
}

impl Theme {
    pub fn dark_default() -> Self {
        todo!("ship the default dark token set")
    }

    pub fn light_default() -> Self {
        todo!("ship the default light token set")
    }

    pub fn load_from_file(_path: &std::path::Path) -> Result<Self, std::io::Error> {
        todo!("documented theme file format, TDD §16.6")
    }
}
