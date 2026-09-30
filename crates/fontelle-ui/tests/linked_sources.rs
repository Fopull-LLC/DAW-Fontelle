//! What feeds a mixer track, worked out once and drawn three ways.
//!
//! > *"it might be worth adding in a feature that highlights items that are
//! > linked to a mixer track so people can better see what is what."*
//!
//! Ty chose three answers (`docs/ux-routing-and-learning-plan.md` §3): a
//! selected strip's sources glow in the rack and the arrangement, hovering a
//! route lights the strip it goes to, and each strip says what feeds it.
//! All three are the same question — which channels and which clips arrive
//! on strip N — so it is one pure function, tested here.

use fontelle_types::ClipId;
use fontelle_ui::canvas::{fed_by_caption, route_strip, strip_sources};
use fontelle_ui::document::ChannelInfo;

fn channel(name: &str, route: Option<usize>) -> ChannelInfo {
    ChannelInfo {
        name: name.into(),
        muted: false,
        soloed: false,
        has_instrument: true,
        route,
    }
}

fn clip(n: u64) -> ClipId {
    use slotmap::{Key, KeyData};
    let id = ClipId::from(KeyData::from_ffi(n | (1 << 32)));
    assert!(!id.is_null());
    id
}

/// Strips 0..3 ordinary, 3 the master — the order `mixer_strips` gives.
const MASTER: Option<usize> = Some(3);

#[test]
fn a_route_of_none_is_the_master() {
    assert_eq!(route_strip(None, MASTER), MASTER);
    assert_eq!(route_strip(Some(1), MASTER), Some(1));
    assert_eq!(route_strip(None, None), None, "no master, nowhere");
}

#[test]
fn a_strips_sources_are_the_channels_routed_to_it_and_the_clips_that_play_into_it() {
    let channels = vec![
        channel("Piano", Some(0)),
        channel("Bass", Some(1)),
        channel("Pad", Some(0)),
        channel("Lead", None),
    ];
    // A note clip feeds every strip its notes' channels go to; an audio clip
    // feeds the one it is routed to.
    let routes = vec![
        (clip(1), vec![0]),
        (clip(2), vec![0, 1]),
        (clip(3), vec![1]),
        (clip(4), vec![3]),
    ];
    let zero = strip_sources(0, MASTER, &channels, &routes);
    assert_eq!(zero.channels, vec![0, 2]);
    assert_eq!(zero.clips, vec![clip(1), clip(2)]);

    let master = strip_sources(3, MASTER, &channels, &routes);
    assert_eq!(
        master.channels,
        vec![3],
        "a channel with no route is the master's"
    );
    assert_eq!(master.clips, vec![clip(4)]);

    let two = strip_sources(2, MASTER, &channels, &routes);
    assert!(two.channels.is_empty() && two.clips.is_empty());
}

#[test]
fn the_caption_names_what_feeds_it_and_counts_the_rest() {
    let channels = vec![
        channel("Piano", Some(0)),
        channel("Bass", Some(1)),
        channel("Pad", Some(0)),
        channel("Strings", Some(0)),
    ];
    let routes: Vec<(ClipId, Vec<usize>)> = vec![(clip(9), vec![1])];
    let zero = strip_sources(0, MASTER, &channels, &routes);
    assert_eq!(fed_by_caption(&zero, &channels, 0), "Piano +2");
    let one = strip_sources(1, MASTER, &channels, &routes);
    assert_eq!(
        fed_by_caption(&one, &channels, 1),
        "Bass +1",
        "an audio clip counts"
    );
    let audio_only = strip_sources(1, MASTER, &[], &routes);
    assert_eq!(fed_by_caption(&audio_only, &[], 1), "1 audio clip");
    let clips2 = vec![(clip(9), vec![1]), (clip(10), vec![1])];
    let audio_two = strip_sources(1, MASTER, &[], &clips2);
    assert_eq!(fed_by_caption(&audio_two, &[], 2), "2 audio clips");
    let nothing = strip_sources(2, MASTER, &channels, &routes);
    assert_eq!(fed_by_caption(&nothing, &channels, 0), "");
}

#[test]
fn a_route_chip_says_in_words_the_track_its_colour_stands_for() {
    // Colour only on the chip, the name on hover — Ty's choice.
    use fontelle_ui::canvas::route_tip;
    let names = vec!["Keys".to_string(), "Low".to_string(), "Master".to_string()];
    assert_eq!(
        route_tip(Some(1), &names),
        "Plays through Low \u{2014} click to change"
    );
    assert_eq!(
        route_tip(None, &names),
        "Plays through Master \u{2014} click to change"
    );
    assert_eq!(
        route_tip(Some(9), &[]),
        "Which mixer track this channel plays through"
    );
}
