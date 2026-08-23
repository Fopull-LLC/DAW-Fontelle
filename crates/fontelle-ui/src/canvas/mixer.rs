/// Channel-strip layout for the mixer panel: fader, pan, insert chain, sends,
/// metering (TDD §13, §16). Direct-draw for the same reason the timeline is: many
/// tracks, redrawn every frame the meters move.
pub struct MixerCanvas {
    pub scroll_x: f32,
    pub strip_width_px: f32,
}
