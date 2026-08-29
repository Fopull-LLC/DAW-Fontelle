use std::collections::HashMap;

use fontelle_types::{AudioInputId, MixerTrackId, ParamAddress};

// `PanLaw` lives in `fontelle-types` so `fontelle-engine`'s `MixerTrackNode`
// can share this exact type — the engine can't depend on this crate (TDD §4.1).
pub use fontelle_types::PanLaw;

/// A document-level reference to one effect instance in an insert chain. The DSP
/// itself (`fontelle-fx`) is instantiated and processed by `fontelle-engine`; the
/// model only stores which effect, its parameters, and whether it's bypassed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EffectSlot {
    pub effect_id: ParamAddress,
    pub bypassed: bool,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Send {
    pub target: MixerTrackId,
    pub level_db: f32,
    pub pan: f32,
    pub pre_fader: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MixerTrack {
    pub name: String,
    pub color: [u8; 4],
    pub gain_db: f32,
    pub pan: f32,
    pub pan_law: PanLaw,
    pub mute: bool,
    pub solo: bool,
    pub phase_invert: bool,
    /// Ordered chain, each bypassable.
    pub inserts: Vec<EffectSlot>,
    /// May target any other track. The routing graph — both `output` and `sends` —
    /// must be validated acyclic on every mutation (TDD §13.2); reject the command,
    /// never let a feedback loop reach the graph compiler.
    pub sends: Vec<Send>,
    /// `None` = master.
    pub output: Option<MixerTrackId>,
    pub input: Option<AudioInputId>,
}

impl MixerTrack {
    /// A track at unity, centred, with nothing on it and its output to master.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            color: [0x60, 0x60, 0x68, 0xff],
            gain_db: 0.0,
            pan: 0.0,
            // A balance control, not a pan law: what arrives on a track's bus
            // has already been placed in the field by the voice, and a second
            // pan law on top attenuates every centred track by another 3 dB.
            pan_law: PanLaw::Linear,
            mute: false,
            solo: false,
            phase_invert: false,
            inserts: Vec::new(),
            sends: Vec::new(),
            output: None,
            input: None,
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Mixer {
    pub tracks: slotmap::SlotMap<MixerTrackId, MixerTrack>,
    pub master: Option<MixerTrackId>,
}

impl Mixer {
    /// Depth-first search over `output` + `sends` for every track; must be run
    /// before committing any routing mutation (TDD §13.2).
    ///
    /// Both edge kinds count. A send is a signal path like any other, and a
    /// cycle through one is the same feedback loop as a cycle through an
    /// output — it is just harder to see in the UI, which is exactly why the
    /// check cannot be limited to the obvious half.
    ///
    /// A track whose `output` is `None` routes to master and has no outgoing
    /// output edge, so master's own `None` does not close a loop.
    pub fn has_cycle(&self) -> bool {
        // Three colours rather than a visited set: a node reachable twice from
        // different branches is not a cycle, and a plain "seen" set would call
        // it one. `Grey` is "on the current path", and only an edge back into
        // that is a cycle.
        #[derive(Clone, Copy, PartialEq)]
        enum Colour {
            White,
            Grey,
            Black,
        }

        let mut colour: HashMap<MixerTrackId, Colour> =
            self.tracks.keys().map(|id| (id, Colour::White)).collect();

        // Iterative, not recursive: the routing graph is user-authored, and a
        // long chain must not be able to overflow the stack on the way to
        // reporting that it is fine.
        for root in self.tracks.keys() {
            if colour[&root] != Colour::White {
                continue;
            }
            let mut stack = vec![(root, 0usize)];
            colour.insert(root, Colour::Grey);
            while let Some((id, edge)) = stack.pop() {
                let Some(track) = self.tracks.get(id) else {
                    continue;
                };
                let next = track
                    .output
                    .iter()
                    .copied()
                    .chain(track.sends.iter().map(|s| s.target))
                    .nth(edge);
                match next {
                    Some(target) => {
                        stack.push((id, edge + 1));
                        match colour.get(&target) {
                            Some(Colour::Grey) => return true,
                            Some(Colour::White) => {
                                colour.insert(target, Colour::Grey);
                                stack.push((target, 0));
                            }
                            // Black, or a target that does not exist: neither
                            // can lead back into the current path.
                            _ => {}
                        }
                    }
                    None => {
                        colour.insert(id, Colour::Black);
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str) -> MixerTrack {
        MixerTrack::new(name)
    }

    #[test]
    fn a_plain_set_of_tracks_feeding_master_is_acyclic() {
        let mut mixer = Mixer::default();
        let master = mixer.tracks.insert(track("Master"));
        mixer.master = Some(master);
        for name in ["Drums", "Bass", "Keys"] {
            let id = mixer.tracks.insert(track(name));
            mixer.tracks[id].output = Some(master);
        }
        assert!(!mixer.has_cycle());
    }

    #[test]
    fn a_track_routed_into_itself_is_a_cycle() {
        let mut mixer = Mixer::default();
        let id = mixer.tracks.insert(track("Feedback"));
        mixer.tracks[id].output = Some(id);
        assert!(mixer.has_cycle());
    }

    #[test]
    fn two_tracks_routed_into_each_other_are_a_cycle() {
        let mut mixer = Mixer::default();
        let a = mixer.tracks.insert(track("A"));
        let b = mixer.tracks.insert(track("B"));
        mixer.tracks[a].output = Some(b);
        mixer.tracks[b].output = Some(a);
        assert!(mixer.has_cycle());
    }

    #[test]
    fn a_cycle_closed_by_a_send_counts_too() {
        // TDD §13.2 names both edge kinds. A send is a signal path like any
        // other, and a loop through one is the same feedback — just harder to
        // see in the UI, which is why the check cannot skip it.
        let mut mixer = Mixer::default();
        let master = mixer.tracks.insert(track("Master"));
        mixer.master = Some(master);
        let a = mixer.tracks.insert(track("A"));
        let reverb = mixer.tracks.insert(track("Reverb"));
        mixer.tracks[a].output = Some(master);
        mixer.tracks[reverb].output = Some(master);
        mixer.tracks[a].sends.push(Send {
            target: reverb,
            level_db: -6.0,
            pan: 0.0,
            pre_fader: false,
        });
        assert!(!mixer.has_cycle(), "a send to a bus is not a loop");

        mixer.tracks[reverb].sends.push(Send {
            target: a,
            level_db: -6.0,
            pan: 0.0,
            pre_fader: false,
        });
        assert!(mixer.has_cycle(), "but a send back is");
    }

    #[test]
    fn a_track_reachable_by_two_paths_is_not_a_cycle() {
        // A plain "already seen" set calls this one, which would reject a
        // perfectly ordinary two-tracks-into-one-group mixer.
        let mut mixer = Mixer::default();
        let master = mixer.tracks.insert(track("Master"));
        mixer.master = Some(master);
        let group = mixer.tracks.insert(track("Group"));
        mixer.tracks[group].output = Some(master);
        for name in ["A", "B"] {
            let id = mixer.tracks.insert(track(name));
            mixer.tracks[id].output = Some(group);
            mixer.tracks[id].sends.push(Send {
                target: group,
                level_db: -6.0,
                pan: 0.0,
                pre_fader: true,
            });
        }
        assert!(!mixer.has_cycle());
    }

    #[test]
    fn an_empty_mixer_and_a_lone_master_are_both_acyclic() {
        assert!(!Mixer::default().has_cycle());
        let mut mixer = Mixer::default();
        // Master's own `output: None` must not read as an edge back to itself.
        let master = mixer.tracks.insert(track("Master"));
        mixer.master = Some(master);
        assert!(!mixer.has_cycle());
    }

    #[test]
    fn a_long_chain_is_walked_without_overflowing_the_stack() {
        // The routing graph is user-authored; a deep chain must report that it
        // is fine rather than taking the process down on the way there.
        let mut mixer = Mixer::default();
        let master = mixer.tracks.insert(track("Master"));
        mixer.master = Some(master);
        let mut previous = master;
        for i in 0..50_000 {
            let id = mixer.tracks.insert(track(&format!("T{i}")));
            mixer.tracks[id].output = Some(previous);
            previous = id;
        }
        assert!(!mixer.has_cycle());
    }
}
