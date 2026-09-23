use std::collections::HashMap;

use fontelle_types::{ChannelId, EffectConfig, EffectKind, MixerTrackId, PluginState};

// `PanLaw` lives in `fontelle-types` so `fontelle-engine`'s `MixerTrackNode`
// can share this exact type — the engine can't depend on this crate (TDD §4.1).
pub use fontelle_types::PanLaw;

/// One effect instance in an insert chain: which effect, how it is set, and
/// whether it is switched out.
///
/// The DSP lives in `fontelle-fx` and runs on the audio thread; what is here is
/// the parameters, which is all the document has an opinion about. The two
/// share one type — `fontelle_types::EffectConfig` — rather than each keeping
/// a copy with a translation between them, for the reason `PanLaw` is shared:
/// two parallel definitions of the same eight numbers is a place for them to
/// drift.
///
/// This was `{ effect_id: ParamAddress, bypassed: bool }` while §13.4 was
/// unbuilt — a name for an effect with nowhere to put a single parameter.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EffectSlot {
    /// The settings of whichever **built-in** effect this holds.
    ///
    /// Meaningless, and left at what it was, when [`plugin`](Self::plugin) is
    /// set. Read it through [`config`](Self::config) rather than directly, and
    /// the answer for a hosted plugin is `None` — an insert holding somebody
    /// else's plugin has no `EffectConfig` and never will, because an
    /// `EffectConfig` is a sum type over the effects that ship in this binary.
    pub config: EffectConfig,
    /// The plugin this insert holds instead, if it holds one (TDD §8.4).
    ///
    /// **A second field rather than a variant of [`EffectConfig`]**, and that
    /// is forced rather than chosen. `EffectConfig` is `Copy`, fixed-size and
    /// read on the audio thread; a plugin's state is a name, a list of
    /// parameters and an opaque blob, none of which is any of those things.
    /// Putting one inside the other would have made every built-in effect pay
    /// for a heap allocation it does not use.
    ///
    /// Defaulted and omitted when empty, so every project written before
    /// plugins could be hosted opens unchanged and is written back unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginState>,
    /// Switched out of the chain, keeping its settings. A bypass is not a
    /// delete: the reason to reach for one is to hear the difference and then
    /// put it back.
    pub bypassed: bool,
    /// Which track this insert's **detector** listens to instead of the signal
    /// passing through it — the external sidechain of TDD §13.4's compressor
    /// row and `docs/effects-catalogue.md` §2.1.
    ///
    /// A field on the slot rather than a parameter in the config, and that is
    /// forced rather than chosen: a `ParamSpec` is a float with a fixed range
    /// and permanent id (INVARIANT 7), and a track is neither. A "key" knob
    /// stepping through whatever tracks happen to exist would mean an
    /// automation lane that pointed at a different track after a rename, which
    /// is the second addressing scheme §8.2 forbids.
    ///
    /// It is a **routing edge**, and every rule that applies to
    /// [`Send::target`] applies to it: it feeds the track it sits on, so the
    /// graph schedules the key's bus first, and a key that closed a loop is
    /// refused by [`Mixer::has_cycle`] before it can reach the compiler.
    ///
    /// `None` on every effect that has no detector, and on every project
    /// written before this field existed.
    #[serde(default)]
    pub key: Option<MixerTrackId>,
    /// Which channel's **notes** this insert listens to — the melody to force
    /// or the scale to allow (`docs/tune-plan.md` §5.1).
    ///
    /// A field on the slot rather than a parameter in the config, for exactly
    /// the reason [`key`](Self::key) is one: a `ParamSpec` is a float with a
    /// fixed range and a permanent id (INVARIANT 7), and a channel is neither.
    /// A "source" knob stepping through whatever channels happen to exist
    /// would mean an automation lane that pointed somewhere else after a
    /// rename.
    ///
    /// Unlike the key it is **not** an edge in the audio graph: no sound
    /// travels along it, so it cannot make a cycle and nothing has to be
    /// scheduled before anything else. The node reads the source's own events
    /// out of the block both of them are given, which is why the order they
    /// run in does not matter (§5.2).
    ///
    /// A channel with **no instrument** is a perfectly good source: its notes
    /// still compile and it makes no sound. "Add a channel, leave it empty,
    /// write the melody in its roll, point the tuner at it" is the workflow,
    /// and it needs nothing new.
    ///
    /// Defaulted and omitted when empty, so every project written before this
    /// field existed opens and is written back unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<ChannelId>,
    /// What this insert **says**, when it is a notepad
    /// (`docs/effects-catalogue.md` §2.8).
    ///
    /// A second field rather than a variant of [`EffectConfig`], for exactly
    /// the reason [`plugin`](Self::plugin) is one and in the same words: an
    /// `EffectConfig` is `Copy`, fixed-size and read on the audio thread, and
    /// a page of lyrics is none of those three. Nothing here ever crosses to
    /// the engine — the notepad's signal path is a wire — so the words cost
    /// the audio thread nothing at all.
    ///
    /// `Some` on every notepad, from [`EffectSlot::new`] onwards, and `None`
    /// on all twenty of the effects that make a sound. Defaulted and omitted
    /// when empty, so every project written before the notepad existed opens
    /// unchanged and is written back unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notepad: Option<fontelle_types::NotepadPages>,
    /// What this insert **draws**, when it is a DisgustingBeat
    /// (`docs/disgusting-beat-plan.md` §3.2).
    ///
    /// A field beside [`notepad`](Self::notepad) for the reason that one is a
    /// field: an `EffectConfig` is `Copy`, fixed-size and read on the audio
    /// thread, and twelve scenes of four lanes is 48 KB. **Boxed**, so the
    /// twenty-one other effects pay one null pointer for it rather than the
    /// bank's whole size in every slot.
    ///
    /// Unlike a notepad's pages these *do* reach the engine, and they have to
    /// reach it while somebody is dragging a point — but not from here. The
    /// document stays the source of truth (INVARIANT 9) and a realised copy
    /// crosses on its own triple buffer (`fontelle_engine::disgusting_beat_channel`).
    ///
    /// A second field rather than one `SlotState` enum holding both this and
    /// the pad: the enum is tidier and it would rename `"notepad"` in every
    /// project file written before it, which would lose somebody's lyrics for
    /// a piece of neatness.
    ///
    /// `Some` on every DisgustingBeat from [`EffectSlot::new`] onwards.
    /// Defaulted and omitted when empty, so every project written before this
    /// opens and is written back unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disgusting_beat: Option<Box<fontelle_types::DisgustingBeatBank>>,
    /// The preset this device was loaded from, if it was loaded from one
    /// (`docs/flopsynth-plan.md` §P.5).
    ///
    /// **The name is remembered; the cleanliness is recognised.** This field
    /// survives every edit and never says whether the device still matches the
    /// file — that is computed by comparing the current state against the
    /// bank's (`Session::preset_state`), so an undo makes the bar's `*` go out
    /// with nothing to remember and no way for the two to disagree.
    ///
    /// Defaulted and omitted when empty, so every project written before the
    /// preset system opens and is written back unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<fontelle_types::PresetRef>,
}

impl EffectSlot {
    /// A fresh effect of `kind`, at settings that change nothing until they
    /// are touched.
    pub fn new(kind: EffectKind) -> Self {
        Self {
            config: EffectConfig::new(kind),
            plugin: None,
            bypassed: false,
            key: None,
            notes: None,
            // A notepad opens with one empty page, here rather than on first
            // use: a pad with nothing to type in is a window that looks
            // broken, and every path that makes a slot goes through this one.
            notepad: (kind == EffectKind::Notepad).then(fontelle_types::NotepadPages::new),
            // Twelve flat scenes, here rather than on first use: a
            // DisgustingBeat with no bank is a window with nothing to draw on,
            // and every path that makes a slot goes through this one.
            disgusting_beat: (kind == EffectKind::DisgustingBeat)
                .then(|| Box::new(fontelle_types::DisgustingBeatBank::new())),
            preset: None,
        }
    }

    /// An insert holding a plugin somebody else wrote.
    ///
    /// The `config` it carries is never read — see the field's own note — and
    /// is a `Utility` because that is the built-in that does nothing.
    pub fn hosting(state: PluginState) -> Self {
        Self {
            config: EffectConfig::new(EffectKind::Utility),
            plugin: Some(state),
            bypassed: false,
            preset: None,
            key: None,
            notes: None,
            notepad: None,
            disgusting_beat: None,
        }
    }

    /// Which built-in effect this is, or `None` for a hosted plugin.
    ///
    /// Everything that draws, schedules or automates an insert asks this
    /// first: `None` means the slot is somebody else's plugin and every
    /// question about `EffectConfig` is the wrong question.
    pub fn kind(&self) -> Option<EffectKind> {
        self.plugin.is_none().then(|| self.config.kind())
    }

    /// The built-in effect's settings, or `None` for a hosted plugin.
    pub fn config(&self) -> Option<&EffectConfig> {
        self.plugin.is_none().then_some(&self.config)
    }

    pub fn is_plugin(&self) -> bool {
        self.plugin.is_some()
    }

    /// What the strip says on this slot.
    ///
    /// A plugin's own name, and a **stored** one at that: it is read back out
    /// of the document rather than off the plugin, so a strip still says
    /// "Diva" on a machine where Diva is not installed. That is the one moment
    /// the name matters most and the plugin is not there to be asked.
    pub fn label(&self) -> &str {
        match &self.plugin {
            Some(plugin) => &plugin.name,
            None => self.config.kind().label(),
        }
    }

    /// The track this insert listens to, if it has a detector *and* has been
    /// given one.
    ///
    /// Both halves matter: a key left on a slot whose effect was changed to
    /// one with no detector is a routing edge that feeds nothing, and it would
    /// order the graph — and refuse a cycle — for a signal path nobody can
    /// hear. Read through this rather than the field wherever the answer is
    /// "does this edge exist".
    pub fn effective_key(&self) -> Option<MixerTrackId> {
        // A hosted plugin's sidechain is a second audio *port*, and whether
        // it has one is the host's knowledge rather than the document's — so
        // a key on a plugin slot is always an edge, and the host feeds it
        // or not. An edge that feeds nothing orders the graph and refuses a
        // cycle for a path nobody hears, which costs nothing anybody can.
        if self.is_plugin() {
            return self.key;
        }
        self.kind()
            .is_some_and(|kind| kind.takes_key())
            .then_some(self.key)
            .flatten()
    }

    /// The channel whose notes really reach this insert, or `None`.
    ///
    /// Reads through [`EffectKind::takes_notes`], so a channel left on an
    /// effect that has nothing to do with notes is not a source — the same
    /// rule [`effective_key`](Self::effective_key) applies to a detector.
    ///
    /// A hosted plugin takes none: a plugin's MIDI input is the host's to
    /// route and this build does not route it.
    pub fn effective_notes(&self) -> Option<ChannelId> {
        self.kind()
            .is_some_and(|kind| kind.takes_notes())
            .then_some(self.notes)
            .flatten()
    }
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
    /// Whether that output is connected at all.
    ///
    /// *"if i chose to not route it to master, i wont be hearing my own input
    /// but it will still be recording the audio clip."*
    ///
    /// [`output`](Self::output) says **where**, this says **whether**, and the
    /// two are separate so that switching a track off and back on puts it back
    /// where it was rather than at the master. `false` is a track whose signal
    /// arrives nowhere by the main path — which is not a mute: its sends still
    /// carry, which is how a track feeding only a reverb is built, and what a
    /// mute would take away as well.
    ///
    /// A project written before this field carries none, and absent reads as
    /// `true`: every track ever saved was routed, and one that reopened silent
    /// would be a song that had lost its mix.
    #[serde(default = "routed")]
    pub output_on: bool,
    /// Which audio input this track records from (TDD §15.4), by **name**.
    ///
    /// *"i click a input button that lets my select my mic input to feed to
    /// that mixer track."*
    ///
    /// A name rather than a handle or an index, because the answer is written
    /// into the document: a project reopened tomorrow has to find the same
    /// microphone, and a device index is not the same device twice. `None` is a
    /// track that records nothing, which is every track until somebody says
    /// otherwise.
    ///
    /// A project written before this field held a name carries `null`, which
    /// reads as `None` — the same value it always had.
    pub input: Option<String>,
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
            output_on: true,
            input: None,
        }
    }
}

/// What [`MixerTrack::output_on`] is when a project does not say — see the
/// field. A function because `serde`'s `default` takes one.
fn routed() -> bool {
    true
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Mixer {
    pub tracks: crate::arena::Arena<MixerTrackId, MixerTrack>,
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
    /// For each track, the tracks whose inserts listen to it — the **signal
    /// flow** direction of an insert's key.
    ///
    /// The document stores that edge the other way round, because it belongs
    /// to the insert: a compressor names what it listens *to*. Everything that
    /// reasons about the routing graph wants the reverse, because a key means
    /// the named track *feeds* the track the insert sits on, exactly as a send
    /// does. Getting that direction backwards is a cycle check that passes a
    /// loop and a schedule that reads the key one block late, so it is written
    /// once, here, beside the field it reverses.
    ///
    /// Reads through [`EffectSlot::effective_key`], so a key left on an effect
    /// with no detector is not an edge.
    pub fn key_listeners(&self) -> HashMap<MixerTrackId, Vec<MixerTrackId>> {
        let mut map: HashMap<MixerTrackId, Vec<MixerTrackId>> = HashMap::new();
        for (id, track) in self.tracks.iter() {
            for slot in &track.inserts {
                if let Some(key) = slot.effective_key()
                    && key != id
                {
                    map.entry(key).or_default().push(id);
                }
            }
        }
        map
    }

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
        // The third kind of edge — see `key_listeners` for why it is reversed
        // and why that direction is the one that matters.
        let listeners = self.key_listeners();
        let none: Vec<MixerTrackId> = Vec::new();

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
                // Three kinds of edge, not two: an insert's key is a signal
                // path like a send, and a key that closed a loop would be a
                // graph the compiler cannot order. See `EffectSlot::key`.
                let next = track
                    .output
                    .iter()
                    .copied()
                    .chain(track.sends.iter().map(|s| s.target))
                    .chain(listeners.get(&id).unwrap_or(&none).iter().copied())
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

    /// Whether `track`'s signal arrives at the master by the main path.
    ///
    /// Two things read this and they are the two halves of one report:
    /// whether monitoring an input is audible, and — when it is not — where a
    /// take has to be put so that *"your recording will actually be audible
    /// after playing it"*. One walk in one place, because two answers to
    /// "can this be heard" is a take that lands somewhere nobody can hear it.
    ///
    /// **Outputs only, not sends.** A send is a copy at a level, usually of a
    /// reverb, and a track heard only through one is not a track you have
    /// recorded onto — asking about it here would call a mic feeding a reverb
    /// "audible" and leave the take somewhere it cannot be heard dry.
    ///
    /// The master reaches itself. A track that is not there does not.
    pub fn reaches_master(&self, track: MixerTrackId) -> bool {
        let Some(master) = self.master else {
            return false;
        };
        let mut at = track;
        // Bounded by the number of tracks: `has_cycle` refuses a loop on every
        // mutation, but this must terminate on a damaged document too rather
        // than trusting that it did.
        for _ in 0..=self.tracks.len() {
            if at == master {
                return true;
            }
            let Some(node) = self.tracks.get(at) else {
                return false;
            };
            if !node.output_on {
                return false;
            }
            // `None` is the master — see `MixerTrack::output`.
            at = node.output.unwrap_or(master);
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
