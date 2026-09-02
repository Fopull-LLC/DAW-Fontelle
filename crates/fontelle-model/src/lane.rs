/// Visual only (TDD §10.3): no audio identity, no routing, no mixer relationship,
/// no instrument. A lane is a horizontal organisational strip — the clip carries
/// its instrument, not the lane. Because lanes are this cheap, users will create
/// hundreds; the timeline renderer must virtualise (`fontelle-ui`), and "bounce
/// this lane" is not a command that gets to exist.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Lane {
    pub name: String,
    pub height: f32,
    pub color: [u8; 4],
    /// Sequencer-level mute — suppresses event emission at compile time. Not a
    /// mixer operation.
    pub muted: bool,
    pub locked: bool,
    /// Where this row sits in the stack, low first.
    ///
    /// The arrangement used to stack rows in the arena's own order, which is
    /// insertion order and cannot be changed — so an arrangement whose rows
    /// were made in the wrong order stayed that way. This is what
    /// [`Project::lane_ids`](crate::Project::lane_ids) sorts by.
    ///
    /// **Defaulted, and the sort is stable**, so every row in a project
    /// written before this existed carries the same number and keeps the order
    /// it has always had. A sort that broke that tie any other way would
    /// silently rearrange every saved arrangement.
    #[serde(default)]
    pub order: u32,
}
