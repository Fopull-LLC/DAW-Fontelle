//! `Lfo.shape` (`docs/flopsynth-next.md` §3.4): a drawn shape kept on the
//! patch beside the wave, which keeps its meaning — a patch written before
//! the field reads with no shape and plays its wave, and a patch with no
//! shape writes what it always wrote.

use fontelle_core::flopsynth::flopsynth_init;
use fontelle_types::{LfoShape, LfoWave};

#[test]
fn a_patch_keeps_a_drawn_shape_beside_the_wave_and_writes_none_without_one() {
    let mut patch = flopsynth_init();
    assert!(patch.lfos.iter().all(|lfo| lfo.shape.is_none()));
    let plain = patch.to_data(&Default::default()).unwrap().body.to_string();
    assert!(!plain.contains("\"shape\""), "no shape is written for none");
    patch.lfos[0].shape = Some(LfoShape::from_wave(LfoWave::Triangle, 8));
    let data = patch.to_data(&Default::default()).unwrap();
    assert!(data.body.to_string().contains("\"shape\""));
    let back = fontelle_core::Patch::from_data(&data, |_| None)
        .unwrap()
        .patch;
    assert_eq!(back.lfos[0].shape, patch.lfos[0].shape);
    assert_eq!(
        back.lfos[0].wave, patch.lfos[0].wave,
        "the wave keeps its meaning"
    );
    assert!(back.lfos[1].shape.is_none());
}

/// The editors' own controls (`docs/flopsynth-next.md` §3.4), as
/// parameters with addresses of their own — **new** addresses; no existing
/// one changes its meaning: an envelope's loop as a choice of stage pairs,
/// and for an LFO whether it plays a drawn shape, the grid the drawing
/// snaps to, and whether the shape is read smooth or stepped.
#[test]
fn the_loop_the_draw_switch_the_grid_and_the_shape_mode_are_parameters() {
    use fontelle_core::patch_params::{ENV_LOOPS, LFO_GRIDS, set, value};
    use fontelle_dsp::EnvStage;
    use fontelle_types::LfoShapeMode;
    let mut patch = flopsynth_init();
    let addresses = fontelle_core::flopsynth::addresses(&patch);
    for address in [
        "patch/env[0]/loop",
        "patch/lfo[0]/draw",
        "patch/lfo[0]/grid",
        "patch/lfo[0]/shape_mode",
    ] {
        assert!(addresses.iter().any(|a| a == address), "no {address}");
        assert!(value(&patch, address).is_some(), "{address} reads nothing");
    }
    // The loop: off by default; each choice is a pair of stages in order.
    assert_eq!(value(&patch, "patch/env[0]/loop"), Some(0.0));
    assert_eq!(ENV_LOOPS[0].0, None);
    assert!(ENV_LOOPS.len() >= 4);
    for (index, (stages, label)) in ENV_LOOPS.iter().enumerate() {
        assert!(!label.is_empty());
        if let Some((from, to)) = *stages {
            assert!(from < to, "{label}: a loop runs forward");
        }
        let normalised = index as f32 / (ENV_LOOPS.len() - 1) as f32;
        assert!(set(&mut patch, "patch/env[0]/loop", normalised));
        assert_eq!(patch.envelopes[0].loop_stages, *stages);
        assert!((value(&patch, "patch/env[0]/loop").unwrap() - normalised).abs() < 1e-6);
    }
    assert!(
        ENV_LOOPS
            .iter()
            .any(|(s, _)| *s == Some((EnvStage::Attack, EnvStage::Decay)))
    );
    // Draw: off while there is no shape; on makes one from the wave, sixteen
    // points; off again puts the wave back and keeps nothing.
    assert_eq!(value(&patch, "patch/lfo[0]/draw"), Some(0.0));
    assert!(set(&mut patch, "patch/lfo[0]/draw", 1.0));
    let shape = patch.lfos[0].shape.clone().expect("drawn");
    assert_eq!(shape.points.len(), 16);
    assert_eq!(value(&patch, "patch/lfo[0]/draw"), Some(1.0));
    // The grid and the mode edit the shape; the grid's choices are the
    // divisions a musician counts in.
    assert!(LFO_GRIDS.contains(&0) && LFO_GRIDS.contains(&16) && LFO_GRIDS.contains(&64));
    let sixteen = LFO_GRIDS.iter().position(|g| *g == 16).unwrap();
    let normalised = sixteen as f32 / (LFO_GRIDS.len() - 1) as f32;
    assert!(set(&mut patch, "patch/lfo[0]/grid", normalised));
    assert_eq!(patch.lfos[0].shape.as_ref().unwrap().grid, 16);
    assert!((value(&patch, "patch/lfo[0]/grid").unwrap() - normalised).abs() < 1e-6);
    assert!(set(&mut patch, "patch/lfo[0]/shape_mode", 1.0));
    assert_eq!(
        patch.lfos[0].shape.as_ref().unwrap().mode,
        LfoShapeMode::Step
    );
    assert_eq!(value(&patch, "patch/lfo[0]/shape_mode"), Some(1.0));
    assert!(set(&mut patch, "patch/lfo[0]/draw", 0.0));
    assert!(patch.lfos[0].shape.is_none());
    // With no shape, the grid and the mode read their defaults and a set
    // makes the shape first — a grid chosen is a drawing begun.
    assert_eq!(value(&patch, "patch/lfo[0]/shape_mode"), Some(0.0));
    assert!(set(&mut patch, "patch/lfo[0]/shape_mode", 1.0));
    assert!(patch.lfos[0].shape.is_some());
}
