/// Direct-draw, not a widget tree (TDD §16.4). Virtualisation is mandatory: build
/// geometry only for the visible time window and visible lanes — a project with
/// 200 lanes and 100,000 notes must scroll at full framerate. Grid, playhead, and
/// note geometry are separate layers with independent invalidation, so the
/// playhead moving never redirties note geometry.
pub struct TimelineCanvas {
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub zoom_x: f32,
    pub zoom_y: f32,
}

impl TimelineCanvas {
    pub fn visible_tick_range(&self, _viewport_width_px: f32) -> std::ops::Range<i64> {
        todo!("scroll_x/zoom_x -> visible Tick range")
    }

    pub fn visible_lane_range(&self, _viewport_height_px: f32) -> std::ops::Range<usize> {
        todo!("scroll_y/zoom_y -> visible lane index range")
    }
}
