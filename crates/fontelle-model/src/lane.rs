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
}
