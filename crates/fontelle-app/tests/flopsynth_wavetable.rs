//! The wavetable editor on the card (`docs/flopsynth-next.md` §4.3): a
//! bank table is *adopted* into the patch as its own copy; a table of the
//! patch's own has a tool chooser (position / draw / bars), the frame
//! actions, a formula and an export; and the tool decides which picture
//! the card draws. The edits themselves are `fontelle-core`'s
//! (`tests/wavetable_edit.rs`); this is the window's half through the
//! session.

mod common;

use fontelle_dsp::{SynthSource, WAVETABLE_LEN};
use fontelle_types::{InstrumentKind, ParamAddress, WaveTool, WavetableEdit};
use fontelle_ui::canvas::{FlopsynthPage, FlopsynthPicture, FlopsynthShowing, ParamKind};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn a_flopsynth() -> fontelle_app::Session {
    let mut session = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    session.set_channel_kind(0, InstrumentKind::Osc3);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    session
}

fn showing(tool: WaveTool) -> FlopsynthShowing {
    FlopsynthShowing {
        inspector: None,
        fx_slot: None,
        wave_tool: tool,
    }
}

fn card(
    session: &fontelle_app::Session,
    tool: WaveTool,
    name: &str,
) -> fontelle_ui::canvas::FlopsynthCard {
    session
        .flopsynth_showing(FlopsynthPage::Synth, showing(tool))
        .expect("a window")
        .cards
        .into_iter()
        .find(|c| c.group.name == name)
        .unwrap_or_else(|| panic!("no {name} card"))
}

fn addresses(card: &fontelle_ui::canvas::FlopsynthCard) -> Vec<String> {
    card.group
        .params
        .iter()
        .map(|p| p.address.as_str().to_string())
        .collect()
}

fn osc_a(session: &fontelle_app::Session) -> fontelle_dsp::SynthOsc {
    let patch = session.selected_patch().unwrap();
    let fontelle_core::Source::Synth(osc) = &patch.layers[0].source else {
        panic!("a synth layer");
    };
    *osc
}

/// A bank table's card offers *edit*, and nothing else of the editor's:
/// the bank is recipes, and an edit starts by taking a copy.
#[test]
fn a_bank_table_is_adopted_into_the_patch_before_it_is_edited() {
    let mut session = a_flopsynth();
    let osc = card(&session, WaveTool::Position, "OSC A");
    let list = addresses(&osc);
    assert!(
        list.contains(&"edit/layer[0]/table/adopt".to_string()),
        "{list:?}"
    );
    assert!(!list.iter().any(|a| a.ends_with("/menu")));
    assert!(!list.contains(&"ui/layer[0]/wave_tool".to_string()));
    let adopt = osc
        .group
        .params
        .iter()
        .find(|p| p.address.as_str() == "edit/layer[0]/table/adopt")
        .unwrap();
    assert_eq!(adopt.kind, ParamKind::Action);
    assert_eq!(adopt.label, "EDIT");
    // The bank's Saw, adopted: a table of the patch's own, named after it,
    // with the recipe's frames laid out at the table's length.
    let before = osc_a(&session);
    let SynthSource::Table(id) = before.source else {
        panic!("Init's OSC A is a bank table")
    };
    session.adopt_wavetable(0).expect("adopts");
    let after = osc_a(&session);
    let SynthSource::User(at) = after.source else {
        panic!("its own table now: {:?}", after.source)
    };
    let patch = session.selected_patch().unwrap();
    let table = &patch.wavetables[at as usize];
    assert_eq!(table.name, id.label());
    assert!(table.frames >= 1 && table.samples.len() == table.frames * WAVETABLE_LEN);
    // And it plays the same wave: the first frame's partials are the
    // recipe's.
    let bank = fontelle_dsp::wavetables().get(id);
    let read: Vec<f32> = (0..WAVETABLE_LEN)
        .map(|i| bank.read(0.0, i as f32 / WAVETABLE_LEN as f32, 0))
        .collect();
    let (own, theirs) = (table.frame(0), read);
    let apart = own
        .iter()
        .zip(&theirs)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(apart < 0.05, "the copy is the recipe's wave: {apart}");
    // Undone, it is the bank's again.
    session.undo();
    assert!(matches!(osc_a(&session).source, SynthSource::Table(_)));
}

/// A table of the patch's own: the tool chooser and the actions, on the
/// card, and the picture the tool asks for.
#[test]
fn a_tables_card_has_the_tool_the_frames_and_the_pictures() {
    let mut session = a_flopsynth();
    session.adopt_wavetable(0).expect("adopts");
    let osc = card(&session, WaveTool::Position, "OSC A");
    let list = addresses(&osc);
    // One button for the actions; the menu under it lists the seven.
    assert!(
        list.contains(&"edit/layer[0]/table/menu".to_string()),
        "{list:?}"
    );
    let verbs: Vec<&str> = fontelle_ui::canvas::WAVE_ACTIONS
        .iter()
        .map(|(verb, _)| *verb)
        .collect();
    assert_eq!(
        verbs,
        [
            "add_frame",
            "copy_frame",
            "remove_frame",
            "morph",
            "morph_spectral",
            "formula",
            "export"
        ]
    );
    assert!(!list.contains(&"edit/layer[0]/table/adopt".to_string()));
    let tool = osc
        .group
        .params
        .iter()
        .find(|p| p.address.as_str() == "ui/layer[0]/wave_tool")
        .expect("a tool chooser");
    let ParamKind::Choice(names) = &tool.kind else {
        panic!("a chooser")
    };
    assert_eq!(names, &["position", "draw", "bars"]);
    assert_eq!(tool.value, 0.0, "at the position tool");
    // The position tool draws the wave as ever; draw and bars draw their
    // own pictures, each naming the frame.
    assert!(matches!(osc.picture, FlopsynthPicture::Wave { .. }));
    let drawn = card(&session, WaveTool::Draw, "OSC A");
    let FlopsynthPicture::Draw {
        points,
        frame,
        frames,
    } = &drawn.picture
    else {
        panic!("the draw picture: {:?}", drawn.picture)
    };
    assert_eq!((*frame, *frames), (0, 1));
    assert_eq!(points.len(), WAVETABLE_LEN / 8, "a point per eight samples");
    let bars = card(&session, WaveTool::Bars, "OSC A");
    let FlopsynthPicture::Bars {
        amps,
        frame,
        frames,
    } = &bars.picture
    else {
        panic!("the bars picture: {:?}", bars.picture)
    };
    assert_eq!((*frame, *frames), (0, 1));
    assert_eq!(amps.len(), fontelle_core::EDIT_HARMONICS);
    assert!(amps[0] > 0.3, "a saw's fundamental: {}", amps[0]);
    // The chooser reads the tool it was built with.
    let drawn_tool = drawn
        .group
        .params
        .iter()
        .find(|p| p.address.as_str() == "ui/layer[0]/wave_tool")
        .unwrap();
    assert!((drawn_tool.value - 0.5).abs() < 1e-6);
}

/// The frame actions go through the host as edits, one undo each, and the
/// frame they act on is the one under the position knob.
#[test]
fn the_frame_actions_edit_the_frame_under_the_position() {
    let mut session = a_flopsynth();
    session.adopt_wavetable(0).expect("adopts");
    let frames = |session: &fontelle_app::Session| {
        let patch = session.selected_patch().unwrap();
        patch.wavetables[0].frames
    };
    assert_eq!(frames(&session), 1);
    session
        .edit_wavetable(0, WavetableEdit::AddFrame { after: 0 })
        .unwrap();
    session.end_gesture();
    assert_eq!(frames(&session), 2);
    // The second frame is silent; the position at the end is on it.
    session.set_instrument_param(&ParamAddress::new("patch/layer[0]/synth/position"), 1.0);
    let drawn = card(&session, WaveTool::Draw, "OSC A");
    let FlopsynthPicture::Draw {
        frame,
        frames: n,
        points,
    } = &drawn.picture
    else {
        panic!()
    };
    assert_eq!((*frame, *n), (1, 2));
    assert!(points.iter().all(|p| *p == 0.0), "silent");
    // Drawing on it, through the edit the picture's drag sends.
    session
        .edit_wavetable(
            0,
            WavetableEdit::Draw {
                frame: 1,
                from: (0.0, 1.0),
                to: (1.0, -1.0),
            },
        )
        .unwrap();
    // The release ends the stroke's gesture, as the window's does.
    session.end_gesture();
    let drawn = card(&session, WaveTool::Draw, "OSC A");
    let FlopsynthPicture::Draw { points, .. } = &drawn.picture else {
        panic!()
    };
    assert!(
        points[0] > 0.9 && points[points.len() - 1] < -0.9,
        "a falling ramp"
    );
    // A formula, through the text seam the prompt uses.
    session
        .apply_wavetable_formula(0, "sin(x*3)")
        .expect("a formula");
    let bars = card(&session, WaveTool::Bars, "OSC A");
    let FlopsynthPicture::Bars { amps, .. } = &bars.picture else {
        panic!()
    };
    assert!(
        amps[2] > 0.9 && amps[0] < 0.05,
        "the third harmonic: {:?}",
        &amps[..4]
    );
    let err = session.apply_wavetable_formula(0, "sin(x").unwrap_err();
    assert!(!err.is_empty());
    // Undo takes the formula back, then the draw, then the frame.
    session.undo();
    let drawn = card(&session, WaveTool::Draw, "OSC A");
    let FlopsynthPicture::Draw { points, .. } = &drawn.picture else {
        panic!()
    };
    assert!(points[0] > 0.9, "the ramp again");
    session.undo();
    session.undo();
    assert_eq!(frames(&session), 1);
    // The last formula is remembered for the prompt to seed with; a table
    // that never had one starts from a sine.
    assert_eq!(session.wavetable_formula(0), "sin(x*3)");
    assert_eq!(session.wavetable_formula(1), "sin(x)");
}

/// The export writes the table as a WAV the drop reads back.
#[test]
fn export_writes_a_wav_beside_the_project() {
    let mut session = a_flopsynth();
    session.adopt_wavetable(0).expect("adopts");
    let dir = std::env::temp_dir().join(format!("fontelle-wt-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Saw.wav");
    session.export_wavetable_to(0, &path).expect("exports");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[0..4], b"RIFF");
    // And back in, on OSC B, frame for frame.
    session.load_wavetable(1, &path).expect("loads");
    let patch = session.selected_patch().unwrap();
    assert_eq!(patch.wavetables.len(), 2);
    assert_eq!(patch.wavetables[1].frames, patch.wavetables[0].frames);
    std::fs::remove_dir_all(&dir).ok();
}
