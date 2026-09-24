//! A right-click on a control opens a menu; it does not make a clip.
//!
//! > *"in the effect rack for a mixer track it immidiately creates the
//! > automation clip if i right click on a knob instead of opening a dropdown
//! > with the option to turn it into an automation clip that would be
//! > prefered so it works more like fl studio."*
//!
//! The effect window's own knobs already asked. Three places did not: the
//! mixer's fader and pan, the wet/dry knob on each insert in the mixer's
//! rack, and the EQ's bands and fields. What each right-click means is
//! decided by a pure function here, so "a control asks first" is a fact
//! about that function rather than about four call sites.

use fontelle_ui::canvas::{
    EqField, EqHit, MixerControl, MixerHit, MixerRightClick, OptionsHit, automate_menu,
    eq_right_click, mixer_right_click,
};

#[test]
fn the_fader_and_the_pan_ask_before_they_automate() {
    assert_eq!(
        mixer_right_click(MixerHit::Fader(2), 0),
        MixerRightClick::Automate(MixerControl::Gain(2))
    );
    assert_eq!(
        mixer_right_click(MixerHit::Pan(1), 0),
        MixerRightClick::Automate(MixerControl::Pan(1))
    );
}

#[test]
fn an_inserts_wet_dry_in_the_mixer_asks_before_it_automates() {
    // The rack's column belongs to the selected track, so that is the strip.
    assert_eq!(
        mixer_right_click(MixerHit::Options(OptionsHit::InsertMix(3)), 4),
        MixerRightClick::Automate(MixerControl::InsertMix { strip: 4, slot: 3 })
    );
}

#[test]
fn a_strips_body_still_opens_the_track_menu() {
    assert_eq!(
        mixer_right_click(MixerHit::Strip(5), 0),
        MixerRightClick::TrackMenu(5)
    );
    assert_eq!(
        mixer_right_click(MixerHit::Mute(5), 0),
        MixerRightClick::Nothing
    );
}

#[test]
fn an_eq_right_click_names_the_parameter_for_its_menu() {
    assert_eq!(
        eq_right_click(EqHit::Handle(1), 0),
        Some(("band2.gain".to_string(), "Band 2 gain".to_string()))
    );
    assert_eq!(
        eq_right_click(EqHit::Field(EqField::Freq), 2),
        Some(("band3.freq".to_string(), "Band 3 freq".to_string()))
    );
    assert_eq!(eq_right_click(EqHit::Curve, 0), None);
}

#[test]
fn the_menu_says_what_it_is_for_and_offers_the_clip() {
    let rows = automate_menu("Master \u{2014} gain");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].label, "Master \u{2014} gain");
    assert!(!rows[0].enabled, "the heading is not a choice");
    assert_eq!(rows[1].label, "Create automation clip");
    assert!(rows[1].enabled);
}
