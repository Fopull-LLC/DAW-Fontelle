mod automation;
mod browser;
mod effect;
mod instrument;
mod menu;
mod mixer;
mod piano_roll;
mod rack;
mod timeline;
mod tools;

pub use automation::{
    AutomationBlock, CURVE_SHAPES, automation_block, automation_polyline,
    block_tick_at, block_value_at, block_x_of_tick, block_y_of_value, curve_label, next_curve,
};
pub use browser::{
    BrowserHit, BrowserLayout, BrowserMode, browser_hit, browser_layout, browser_layout_for,
    row_under, scrolled,
};
pub use effect::{
    EQ_MAX_DB, EQ_MAX_HZ, EQ_MIN_HZ, EffectMenu, EqField, EqHandle, EqHit, EqLayout, InsertInfo,
    NO_KEY,
    band_home_hz, effect_menu_hit, effect_menu_layout, effect_view, eq_band_curve_points,
    eq_curve_points,
    eq_field_caption, eq_freq_at, eq_gain_at, eq_hit, eq_layout, eq_layout_for, eq_nudge_freq,
    eq_nudge_gain, eq_nudge_mix, eq_nudge_q, eq_x_of_freq, eq_y_of_gain, format_hz,
    next_band_channel, next_band_type, SPECTRUM_BANDS, SPECTRUM_BOTTOM_DB, SPECTRUM_TOP_DB,
    spectrum_band_hz, spectrum_points,
};
pub use instrument::{
    CELL_HEIGHT, CELL_WIDTH, InstrumentGroup, InstrumentLayout, InstrumentParam, InstrumentView,
    MIXER_GAIN, MIXER_PAN, PRESET_HEIGHT, PRESET_WIDTH, ParamKind, choice_index, instrument_hit,
    instrument_key_hit, instrument_layout, instrument_preset_hit, knob_value,
    next_value,
};
pub use menu::{
    ContextMenu, MENU_TEXT_INSET, MenuEntry, context_menu_hit, context_menu_layout,
};
pub use mixer::{
    FADER_DETENT_PX, InsertRowLayout, MAX_FADER_DB, MAX_SEND_DB, MIN_FADER_DB, MIN_SEND_DB,
    MixerHit, MixerLayout, MixerStripLayout, OPTIONS_WIDTH, OptionsHit, PAN_DETENT_PX,
    STRIP_WIDTH, SendRowLayout, TrackOptionsLayout, fader_db_at, fader_y_of_db, format_gain_db,
    format_mix, format_pan, format_send_db, insert_mix_dial, mixer_hit, mixer_layout, mixer_layout_for, pan_at,
    pan_x_of, send_level_at, send_x_of_level, unity_fraction,
};
pub use piano_roll::{
    Audition, DEFAULT_LANE_HEIGHT, DrawDrag, KEYBOARD_WIDTH, KeyStyle, LANE_PROPERTIES, LaneMenu,
    LaneProperty, MAX_LANE_FRACTION, MIN_LANE_HEIGHT, Modifiers, MouseButton, NAMED_KEYBOARD_WIDTH,
    NotePart, PianoRoll, RollControl, RollEdit, RollHit, RollLayout, RollView, SnapDivision, Tool,
    ToolbarLayout, clamp_to_grid, edge_scroll, hit_test, key_to_y, keyboard_width, lane_baseline_y,
    lane_caption, lane_height_at, lane_menu_hit, lane_menu_layout, lane_value_of_y,
    key_row, keyboard_width_for, lane_y_of_value, note_at_tick, roll_layout,
    roll_layout_with_keys, slice_cuts, snap_tick,
    snap_unit, subdivision_unit, tick_to_x, toolbar_hit, toolbar_layout, tools_caption,
    velocity_of_y,
    velocity_to_y, visible_keys, visible_ticks, x_to_tick, y_to_key, zoom_x, zoom_y,
};
pub use rack::{
    NEW_TRACK, RackHit, RackLayout, RackRow, RouteChoice, RouteMenu, rack_hit, rack_layout,
    route_label, route_menu_hit, route_menu_layout, route_menu_layout_excluding, scroll_to_show,
};
pub use timeline::{
    ArrangeEdit, ClipPart, MAX_LANE_ROW, MAX_TIMELINE_PPT, MIN_LANE_ROW, MIN_TIMELINE_PPT,
    Timeline, TimelineControl, TimelineHit, TimelineLayout, TimelineTool, TimelineToolbar,
    CLIP_HEADER_PX, NOTE_PREVIEW_MIN_KEYS,
    TimelineView, clip_bands, clip_cuts, clip_grip, clip_notes, clip_rect, lane_to_y, loop_marks,
    time_selection, timeline_hit,
    timeline_layout, timeline_snap,
    timeline_tick_to_x, timeline_toolbar_hit, timeline_toolbar_layout, timeline_visible_ticks,
    timeline_x_to_tick, timeline_zoom_x, timeline_zoom_y, visible_lanes, y_to_lane,
};
pub use tools::{
    TOOL_ROWS, ToolAction, ToolRow, Tools, ToolsPanel, tools_panel_hit, tools_panel_layout,
};
