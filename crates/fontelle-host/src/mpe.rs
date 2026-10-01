//! **MPE out** (`docs/note-paths-plan.md` §6): the bookkeeping that lets a
//! plugin which hears only MIDI bend one note of a chord.
//!
//! MIDI has no pitch per note. MPE's answer is to give each sounding note a
//! channel of its own — a *member* channel — so the pitch bend on that
//! channel is that note's alone. A note path's slide reaches an LV2 or
//! bridged VST 2 synth this way when the synth is in its MPE mode; one that
//! is not would read the bends as the channel's and the 48-semitone range as
//! its own two, which is why the switch is the player's (`PluginState::mpe`).
//!
//! Fixed arrays and no allocation: this runs on the audio thread
//! (INVARIANT 1).

/// How many member channels the zone has: all of them but the manager.
const MEMBERS: usize = 15;

/// The bend range each member is told, in semitones — MPE's own default,
/// said rather than assumed.
pub const MPE_BEND_RANGE: f32 = 48.0;

/// A lower MPE zone, as a host speaks it to a plugin: channel 1 (`0` on the
/// wire) is the manager, 2–16 the members.
#[derive(Debug, Clone)]
pub struct MpeZone {
    /// Which member each key is sounding on, `0` for none.
    channel_of: [u8; 128],
    /// How many notes each member holds.
    holding: [u8; MEMBERS],
    /// When each member was last started or let go, in calls — so a new note
    /// takes the one quiet longest, and the release of the note that just
    /// ended rings on unbent by the next note's slide.
    touched: [u64; MEMBERS],
    clock: u64,
    /// Whether the zone has been described to the plugin since the last
    /// reset.
    configured: bool,
}

impl Default for MpeZone {
    fn default() -> Self {
        Self {
            channel_of: [0; 128],
            holding: [0; MEMBERS],
            touched: [0; MEMBERS],
            clock: 0,
            configured: false,
        }
    }
}

impl MpeZone {
    /// Starts `key` on a member of its own. The first note after a reset
    /// says the zone first: RPN 6 on the manager (fifteen members), then RPN
    /// 0 on each member (48 semitones). Then the member's bend is put back
    /// to centre, so the new note does not inherit the last one's slide.
    pub fn note_on(&mut self, key: u8, velocity: u8, out: &mut dyn FnMut([u8; 3])) {
        if !self.configured {
            self.configured = true;
            for (number, value) in [(101, 0), (100, 6), (6, MEMBERS as u8)] {
                out([0xB0, number, value]);
            }
            for member in 1..=MEMBERS as u8 {
                for (number, value) in [(101, 0), (100, 0), (6, MPE_BEND_RANGE as u8), (38, 0)] {
                    out([0xB0 | member, number, value]);
                }
            }
        }
        // A free member, the one quiet longest; with all fifteen busy, the
        // one touched longest ago — a sixteenth note shares rather than
        // being dropped.
        let pick = (0..MEMBERS)
            .filter(|&i| self.holding[i] == 0)
            .min_by_key(|&i| self.touched[i])
            .or_else(|| (0..MEMBERS).min_by_key(|&i| self.touched[i]))
            .unwrap_or(0);
        self.clock += 1;
        self.touched[pick] = self.clock;
        self.holding[pick] = self.holding[pick].saturating_add(1);
        let member = pick as u8 + 1;
        self.channel_of[usize::from(key.min(127))] = member;
        out(bend(member, 0.0));
        out([0x90 | member, key.min(127), velocity.clamp(1, 127)]);
    }

    /// Ends `key` on the member it is sounding on. Nothing for a key that is
    /// not.
    pub fn note_off(&mut self, key: u8, out: &mut dyn FnMut([u8; 3])) {
        let slot = usize::from(key.min(127));
        let member = self.channel_of[slot];
        if member == 0 {
            return;
        }
        self.channel_of[slot] = 0;
        let index = usize::from(member - 1);
        self.holding[index] = self.holding[index].saturating_sub(1);
        self.clock += 1;
        self.touched[index] = self.clock;
        out([0x80 | member, key.min(127), 0]);
    }

    /// Bends `key` alone to `semitones` from where it started, on its own
    /// member's wheel. A bend past the range stops at its end. Nothing for a
    /// key that is not sounding.
    pub fn note_bend(&mut self, key: u8, semitones: f32, out: &mut dyn FnMut([u8; 3])) {
        let member = self.channel_of[usize::from(key.min(127))];
        if member != 0 {
            out(bend(member, semitones));
        }
    }

    /// Forgets every note, and says the zone again before the next one: a
    /// reset is where a plugin may have dropped what it was told.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// A pitch-bend message on `channel`, `semitones` over [`MPE_BEND_RANGE`].
fn bend(channel: u8, semitones: f32) -> [u8; 3] {
    let raw = ((semitones / MPE_BEND_RANGE * 8192.0).round() as i32 + 8192).clamp(0, 16383) as u16;
    [0xE0 | channel, (raw & 0x7F) as u8, (raw >> 7) as u8]
}
