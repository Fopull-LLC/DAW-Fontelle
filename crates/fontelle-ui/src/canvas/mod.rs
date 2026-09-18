mod audio_clip;
mod automation;
mod browser;
mod carry;
mod effect;
mod favorites;
mod flopsynth;
mod glide;
mod instrument;
mod keybinds;
mod keymap;
mod menu;
mod mixer;
mod overlays;
mod piano_roll;
mod prefabs;
mod preset_bar;
mod rack;
mod text_entry;
mod timeline;
mod tools;
mod tune;
mod welcome;

pub use audio_clip::{
    AUDIO_ROWS, AudioControl, AudioEditorLayout, AudioField, MASTER_ROUTE, MAX_CLIP_PITCH,
    MIN_CLIP_PITCH, audio_editor_hit, audio_editor_layout, audio_knob_rect, audio_row_choices,
    audio_row_chosen, audio_row_control, audio_row_control_rect, audio_row_fraction,
    audio_row_is_on, audio_row_label, audio_row_neutral, audio_row_tip, audio_row_value,
    audio_row_value_at, audio_slider_at, audio_slider_x_of, choose_audio_route, choose_audio_row,
    nudge_audio_row, nudge_route, set_audio_row_fraction, toggle_audio_row,
};
pub use automation::{
    AutomationBlock, CURVE_SHAPES, automation_block, automation_polyline, block_tick_at,
    block_value_at, block_x_of_tick, block_y_of_value, curve_label, next_curve,
};
pub use browser::{
    BrowserHit, BrowserLayout, BrowserMode, SettingControl, TAB_WORD_MIN_WIDTH,
    browser_file_share_at, browser_focus_step, browser_hit, browser_layout, browser_layout_for,
    browser_layout_split, browser_row_carries, kind_icon, row_under, scrolled,
    setting_control_rect, setting_slider_at, setting_slider_groove, setting_slider_x_of,
    tab_shows_word,
};
pub use carry::{
    CARRY_PAD, Carried, CarryOscillator, CarryRack, CarryRelease, CarryScene, CarryTarget,
    CarryTimeline, carry_chip, carry_chip_lifted, carry_note, carry_release, carry_target,
    held_note,
};
pub use effect::{
    EQ_MAX_DB, EQ_MAX_HZ, EQ_MIN_HZ, EqField, EqHandle, EqHit, EqLayout, InsertInfo, NO_KEY,
    SPECTRUM_BANDS, SPECTRUM_BOTTOM_DB, SPECTRUM_TOP_DB, band_home_hz, effect_params, effect_view,
    eq_band_curve_points, eq_curve_points, eq_field_caption, eq_freq_at, eq_gain_at, eq_hit,
    eq_layout, eq_layout_for, eq_nudge_freq, eq_nudge_gain, eq_nudge_mix, eq_nudge_q, eq_x_of_freq,
    eq_y_of_gain, format_hz, next_band_channel, next_band_type, spectrum_band_hz, spectrum_points,
};
pub use favorites::{
    EffectRow, FAVORITES_HEADING, InstrumentRow, PickerRow, RESCAN_PLUGINS, effect_menu_rows,
    instrument_menu_rows, plugin_picker_rows,
};
pub mod gestures;
pub use flopsynth::{
    ADD_EFFECT, ADD_ROUTE, BADGE_H, BADGE_W, BadgeAnatomy, CANOPY_HEIGHT, CAPTION_ROOM, CARD_GAP,
    CARD_HEADER, CARD_PAD, CELL_FLOOR, CELL_TEXT_H, CHIP_CHEVRON, CHIP_INSET, CHIP_TEXT_INDENT,
    CHIP_TEXT_ROOM, CardLayout, CellAnatomy, EnvNode, FLOP_CELL_H, FLOP_CELL_HALF, FLOP_CELL_W,
    FLOP_GRID, FlopsynthCard, FlopsynthHit, FlopsynthLayout, FlopsynthPage, FlopsynthPicture,
    FlopsynthRoute, FlopsynthView, Grid, INSPECTOR_ROW, KNOB_LARGE, KNOB_MEDIUM, KNOB_SMALL,
    KnobSize, MATRIX_HEADS, MATRIX_ROW, MATRIX_ROWS_LEAST, MatrixHeader, MatrixHit, MatrixRow,
    Measure, NAMEPLATE_CHIP_W, NO_VIA, NODE_GRAB, PICTURE_FLOOR, PICTURE_HEIGHT, PRESET_ROW,
    PresetBrowse, PresetShelf, PresetsHit, PresetsLayout, RESPONSE_BOTTOM_DB, RESPONSE_TOP_DB,
    RING_BAND, RING_GAP, SCALE_CHIP_W, SCALES, STRIP_HEIGHT, TAB_HEIGHT, TUNE_GRID, badge_anatomy,
    badge_at, badge_caption, badge_points, canopy_eyes, cell_anatomy, cell_span,
    cell_span_measured, control_at, effect_card_at, env_curve_points, env_node_at, env_node_drag,
    estimated_width, filter_xy_at, flop_knob_rect, flopsynth_hit, flopsynth_layout,
    flopsynth_layout_with, flopsynth_tab_at, is_nameplate_control, lamp_dots, lfo_curve_points,
    matrix_depth_at, matrix_hit, picture_control, preset_about, preset_page_rows, preset_shelves,
    presets_hit, response_curve_points, ring_band, ring_bands, ring_depth, ring_dot, ring_hit,
    ring_hit_index, ring_live, ring_range, route_landing, sound_outline_points, spectrum_bars,
    wave_curve_points, wave_position_at,
};
pub use gestures::{
    BADGE_CLICK_SLOP, BadgeGesture, FlopKnobMenu, FlopKnobMenuItem, NUDGE, NUDGE_FINE, Precision,
    Typed, badge_gesture, flop_knob_menu, flopsynth_tip, hover_bubble_rect, inspector_after_click,
    knob_drag, matrix_tip, nudged, parse_typed, wheel_nudge,
};
pub use instrument::{
    CELL_HEIGHT, CELL_WIDTH, CHIP_HEIGHT, CHIP_WIDTH, InstrumentGroup, InstrumentLayout,
    InstrumentParam, InstrumentView, MIXER_GAIN, MIXER_PAN, ParamKind, choice_index,
    instrument_hit, instrument_key_hit, instrument_layout, knob_value, next_value,
};
pub use keybinds::{
    KEYBIND_SECTIONS, KEYBINDS_CLOSE, KEYBINDS_HINT, KEYBINDS_LISTENING, KEYBINDS_PRESS,
    KEYBINDS_RESET, KEYBINDS_TITLE, KeybindEntry, KeybindRow, KeybindSection, KeybindsHit,
    KeybindsLayout, keybinds_hit, keybinds_layout, keybinds_scroll_max, keybinds_scrolled,
};
pub use keymap::{Action, Chord, ChordKey, Context, Keymap, Rebind};
pub use menu::{
    CHOSEN_MARK, ContextMenu, MENU_TEXT_INSET, MenuEntry, NAME_CARET, STAR_WIDTH, THUMB_H, THUMB_W,
    context_menu_hit, context_menu_layout, context_menu_layout_beside, context_menu_star_hit,
    export_menu_choice, export_menu_entries, input_menu_choice, input_menu_entries,
    instrument_menu_entries, menu_matches, name_prompt_entries, thumbnail_points,
};
pub use overlays::{ConfirmLayout, TOAST_SECONDS, ToastLayout, confirm_layout, toast_layout};
pub use preset_bar::{
    NO_PRESET, PRESET_MENU_HEADING, PresetBarHit, PresetBarLayout, PresetBarView, PresetChoice,
    PresetDevice, PresetMenuRow, USER_MARK, preset_bar_hit, preset_bar_layout, preset_bar_name,
    preset_menu,
};
/// Where a zoom should land, given the grid it is zooming and where the
/// pointer is.
///
/// > *"i dont like when i zoom in and out its based around where my playhead
/// > is and i dont like that i want it to be based on my cursor for maximum
/// > user control."*
///
/// **The pointer when it is over the grid, and the middle otherwise.** The
/// wheel already worked this way; the toolbar buttons and the `+`/`-` keys
/// took the middle unconditionally, which on a view scrolled to follow the
/// playhead is the playhead near enough — which is exactly what that reads as.
///
/// The fallback is not a compromise: a button pressed with the pointer down on
/// the toolbar has no meaningful anchor of its own, and the middle of what you
/// are looking at is the one answer that does not throw the view somewhere you
/// were not.
///
/// One function rather than the rule written at each of the three call sites,
/// because three copies is somewhere for them to disagree — which is how two
/// of them came to be wrong while the third was right.
pub fn zoom_anchor(grid: crate::layout::Rect, cursor: (f32, f32)) -> f32 {
    let (x, y) = cursor;
    if grid.contains(x, y) {
        x
    } else {
        grid.x + grid.width / 2.0
    }
}

pub use glide::{Glide, wheel_travel};
pub use mixer::{
    CHAIN_DOT, CHAIN_DOT_GAP, FADER_DETENT_PX, InsertRowLayout, MAX_FADER_DB, MAX_SEND_DB,
    MIN_FADER_DB, MIN_SEND_DB, MixerHit, MixerKey, MixerLayout, MixerStripLayout, NamePress,
    OPTIONS_WIDTH, OptionsHit, PAN_DETENT_PX, STRIP_WIDTH, SendRowLayout, TrackOptionsLayout,
    fader_db_at, fader_y_of_db, format_gain_db, format_mix, format_pan, format_send_db,
    insert_mix_dial, mixer_hit, mixer_key, mixer_layout, mixer_layout_for, name_press, pan_at,
    pan_x_of, send_level_at, send_x_of_level, unity_fraction,
};
pub use piano_roll::{
    Audition, DEFAULT_LANE_HEIGHT, DrawDrag, EdgeScroll, KEYBOARD_WIDTH, KeyStyle, LANE_PROPERTIES,
    LaneMenu, LaneProperty, MAX_LANE_FRACTION, MIN_LANE_HEIGHT, Modifiers, MouseButton,
    NAMED_KEYBOARD_WIDTH, NotePart, PianoRoll, RollControl, RollEdit, RollHit, RollLayout,
    RollView, SNAP_DIVISIONS, SnapDivision, Tool, ToolbarLayout, clamp_to_grid, edge_scroll_rate,
    hit_test, key_row, key_to_y, keyboard_width, keyboard_width_for, lane_baseline_y, lane_caption,
    lane_height_at, lane_menu_hit, lane_menu_layout, lane_value_of_y, lane_y_of_value,
    legato_edits, note_at_tick, note_marks, roll_layout, roll_layout_with_keys, roll_past_end,
    slice_cuts, snap_caption, snap_tick, snap_unit, subdivision_unit, tick_to_x, toolbar_hit,
    toolbar_layout, tools_caption, velocity_of_y, velocity_to_y, visible_keys, visible_ticks,
    x_to_tick, y_to_key, zoom_x, zoom_y,
};
pub use prefabs::{
    PrefabHit, PrefabLayout, PrefabRow, prefab_hit, prefab_layout,
    scroll_to_show as prefab_scroll_to_show,
};
pub use rack::{
    NEW_TRACK, NO_OUTPUT, RackHit, RackLayout, RackRow, RouteChoice, RouteMenu, output_menu_layout,
    rack_hit, rack_layout, route_label, route_menu_hit, route_menu_layout,
    route_menu_layout_excluding, scroll_to_show, tab_at, tab_strip,
};
pub use text_entry::{TextEntry, TextKey, text_key};
pub use timeline::{
    ArrangeEdit, CLIP_HEADER_PX, ClipOverlap, ClipPart, FadeAnatomy, FadeEnd, FadeGrip,
    MAX_LANE_ROW, MAX_TIMELINE_PPT, MIN_LANE_ROW, MIN_TIMELINE_PPT, NOTE_PREVIEW_MIN_KEYS,
    Timeline, TimelineControl, TimelineHit, TimelineLayout, TimelineTool, TimelineToolbar,
    TimelineView, arrival_row, clip_bands, clip_cuts, clip_grip, clip_notes, clip_overlaps,
    clip_rect, clip_waveform, clip_waveform_core, content_end, content_fraction, content_ticks,
    fade_anatomy, fade_caption, fade_curve, lane_scroll_to_show, lane_to_y, loop_marks,
    slice_marks, time_selection, timeline_hit, timeline_layout, timeline_snap, timeline_tick_to_x,
    timeline_toolbar_hit, timeline_toolbar_layout, timeline_visible_ticks, timeline_x_to_tick,
    timeline_zoom_x, timeline_zoom_y, visible_lanes, y_to_lane,
};
pub use tools::{
    TOOL_MENU, TOOL_ROWS, ToolAction, ToolKind, ToolMenuItem, ToolRow, Tools, ToolsDialog,
    tools_dialog_hit, tools_dialog_layout,
};
pub use tune::{
    KEYBOARD_FLOOR, KEYBOARD_HEIGHT, KEYBOARD_KEYS, KEYBOARD_OCTAVES, NO_MIDI, TRACE_SECONDS,
    TUNE_LOCK_CENTS, TUNE_SOURCE, TracePoint, TuneHit, TuneLayout, TuneView, VIEWPORT_FLOOR,
    VIEWPORT_HEIGHT, current_frame, key_click_mask, key_solo_mask, sung_class, target_class,
    trace_at, tune_caption, tune_hit, tune_keyboard_layout, tune_layout, tune_readout,
    tune_strings, viewport_points, viewport_rails,
};
pub use welcome::{
    FOOTER_TEXT, NEW_PROJECT_LABEL, NOTHING_RECENT, OPEN_PROJECT_LABEL, RECENT_HEADING,
    REPOSITORY_LABEL, REPOSITORY_URL, RecentRow, TITLE_HEIGHT, WEBSITE_LABEL, WEBSITE_URL,
    WelcomeHit, WelcomeLayout, transfer_fraction, transfer_text, update_line, update_progress,
    welcome_hit, welcome_layout,
};
