/// Notes render as instanced quads in a single draw call, with a separate pass for
/// selection and velocity overlays (TDD §16.4). Keybinds and mouse behaviour track
/// FL Studio closely (§16.5) — full keymap lives in `KEYMAP.md`, produced at M3.
pub struct PianoRollCanvas {
    pub scroll_ticks: i64,
    pub scroll_key: u8,
    pub zoom_x: f32,
    pub zoom_y: f32,
    pub snap: SnapDivision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapDivision {
    Bar,
    Beat,
    Step,
    Division(u8),
    Triplet,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Draw,
    Paint,
    Delete,
    Select,
    Slice,
    Mute,
    Slip,
}
