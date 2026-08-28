//! Raw MIDI bytes to something with a name (TDD §14).
//!
//! Everything here is a pure function over a byte slice, because this is the
//! half of live MIDI that can be tested without hardware — and the half where
//! a wrong reading produces plausible-sounding nonsense rather than an error.

/// One decoded channel-voice or channel-mode message. System-exclusive and
/// system-common messages are not represented: nothing consumes them yet, and
/// a variant nothing reads is worse than an honest `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiMessage {
    NoteOn {
        channel: u8,
        key: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        key: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    /// Centred at 0, spanning -8192..=8191.
    PitchBend {
        channel: u8,
        value: i16,
    },
    ChannelPressure {
        channel: u8,
        value: u8,
    },
    ProgramChange {
        channel: u8,
        program: u8,
    },
    /// CC 123. Release everything sounding, respecting release tails.
    AllNotesOff {
        channel: u8,
    },
    /// CC 120. Stop everything now, tails included — the panic button.
    AllSoundOff {
        channel: u8,
    },
}

/// Controller numbers that are channel-mode messages rather than continuous
/// controllers. They arrive as ordinary CCs and mean something entirely
/// different; passing them through as CC values is how a panic button ends up
/// setting a parameter to zero.
const CC_ALL_SOUND_OFF: u8 = 120;
const CC_ALL_NOTES_OFF: u8 = 123;

/// Decodes one complete MIDI message.
///
/// Returns `None` for anything with no meaning here — system real-time,
/// system-exclusive, and truncated messages alike. **Truncated messages must
/// not panic**: this runs on a device callback thread owned by the MIDI
/// backend, and a panic there takes out a thread nothing is watching.
///
/// Running status is not handled, deliberately: a MIDI *port* delivers
/// complete messages, and running status is a property of the byte stream in a
/// `.mid` file (which `fontelle-assets` handles) or of a raw serial line.
/// Accepting a status-less message here would mean guessing at what came
/// before it.
pub fn decode(bytes: &[u8]) -> Option<MidiMessage> {
    let status = *bytes.first()?;
    // System messages: 0xF0..=0xFF carry no channel. Real-time clock (0xF8)
    // and active sensing (0xFE) arrive constantly — most keyboards send active
    // sensing several times a second — so silently ignoring them is not an
    // edge case, it is the common path.
    if status >= 0xF0 {
        return None;
    }
    let channel = status & 0x0F;
    let data1 = *bytes.get(1)?;

    match status & 0xF0 {
        0x80 => Some(MidiMessage::NoteOff {
            channel,
            key: data1 & 0x7F,
        }),
        0x90 => {
            let velocity = *bytes.get(2)? & 0x7F;
            // A note-on at velocity zero is a note-off, and almost every
            // device and file uses it in preference to an explicit one. Read
            // literally it starts a silent note that never ends.
            if velocity == 0 {
                Some(MidiMessage::NoteOff {
                    channel,
                    key: data1 & 0x7F,
                })
            } else {
                Some(MidiMessage::NoteOn {
                    channel,
                    key: data1 & 0x7F,
                    velocity,
                })
            }
        }
        0xA0 => None, // polyphonic aftertouch: nothing consumes it yet
        0xB0 => {
            let value = *bytes.get(2)? & 0x7F;
            match data1 & 0x7F {
                CC_ALL_SOUND_OFF => Some(MidiMessage::AllSoundOff { channel }),
                CC_ALL_NOTES_OFF => Some(MidiMessage::AllNotesOff { channel }),
                controller => Some(MidiMessage::ControlChange {
                    channel,
                    controller,
                    value,
                }),
            }
        }
        0xC0 => Some(MidiMessage::ProgramChange {
            channel,
            program: data1 & 0x7F,
        }),
        0xD0 => Some(MidiMessage::ChannelPressure {
            channel,
            value: data1 & 0x7F,
        }),
        0xE0 => {
            let msb = *bytes.get(2)? & 0x7F;
            // Fourteen bits, low seven first, centred at 8192 — so the value
            // at rest is 0 and a bend is symmetric. Reading the two bytes the
            // other way round gives a control that jumps in coarse steps and
            // sits off-centre, which sounds like a broken pitch wheel rather
            // than like a byte-order mistake.
            let raw = ((msb as i16) << 7) | (data1 & 0x7F) as i16;
            Some(MidiMessage::PitchBend {
                channel,
                value: raw - 8_192,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_on_carries_its_channel_key_and_velocity() {
        assert_eq!(
            decode(&[0x92, 60, 100]),
            Some(MidiMessage::NoteOn {
                channel: 2,
                key: 60,
                velocity: 100
            })
        );
    }

    #[test]
    fn a_note_on_at_velocity_zero_is_a_note_off() {
        // The single most common way a first MIDI implementation hangs every
        // note it plays.
        assert_eq!(
            decode(&[0x90, 60, 0]),
            Some(MidiMessage::NoteOff {
                channel: 0,
                key: 60
            })
        );
    }

    #[test]
    fn an_explicit_note_off_is_a_note_off() {
        assert_eq!(
            decode(&[0x81, 64, 40]),
            Some(MidiMessage::NoteOff {
                channel: 1,
                key: 64
            })
        );
    }

    #[test]
    fn a_control_change_is_not_confused_with_the_channel_mode_messages() {
        assert_eq!(
            decode(&[0xB0, 7, 100]),
            Some(MidiMessage::ControlChange {
                channel: 0,
                controller: 7,
                value: 100
            })
        );
        assert_eq!(
            decode(&[0xB0, 123, 0]),
            Some(MidiMessage::AllNotesOff { channel: 0 }),
            "CC 123 means release everything, not 'set controller 123 to 0'"
        );
        assert_eq!(
            decode(&[0xB0, 120, 0]),
            Some(MidiMessage::AllSoundOff { channel: 0 })
        );
    }

    #[test]
    fn pitch_bend_is_fourteen_bits_low_byte_first_and_centred_at_rest() {
        assert_eq!(
            decode(&[0xE0, 0x00, 0x40]),
            Some(MidiMessage::PitchBend {
                channel: 0,
                value: 0
            }),
            "the wheel at rest is 8192, which is no bend at all"
        );
        assert_eq!(
            decode(&[0xE0, 0x00, 0x00]),
            Some(MidiMessage::PitchBend {
                channel: 0,
                value: -8_192
            })
        );
        assert_eq!(
            decode(&[0xE0, 0x7F, 0x7F]),
            Some(MidiMessage::PitchBend {
                channel: 0,
                value: 8_191
            })
        );
        // The byte order is the part worth pinning: swapped, a small bend
        // reads as a large one and the wheel never returns to centre.
        assert_eq!(
            decode(&[0xE0, 0x40, 0x00]),
            Some(MidiMessage::PitchBend {
                channel: 0,
                value: -8_128
            })
        );
    }

    #[test]
    fn system_real_time_messages_are_ignored_rather_than_misread() {
        // A keyboard sends active sensing several times a second for as long
        // as it is plugged in. Anything that reads 0xFE as a channel message
        // produces a constant stream of garbage events.
        assert_eq!(decode(&[0xFE]), None, "active sensing");
        assert_eq!(decode(&[0xF8]), None, "MIDI clock");
        assert_eq!(decode(&[0xFA]), None, "start");
        assert_eq!(decode(&[0xF0, 0x7E, 0xF7]), None, "system exclusive");
    }

    #[test]
    fn a_truncated_message_is_refused_rather_than_panicking() {
        // This decodes on a thread the MIDI backend owns. A panic there kills
        // a thread nobody is watching and takes live input with it.
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[0x90]), None);
        assert_eq!(decode(&[0x90, 60]), None, "note-on with no velocity byte");
        assert_eq!(decode(&[0xE0, 0x00]), None, "half a pitch bend");
    }

    #[test]
    fn the_channel_nibble_is_read_from_the_status_byte() {
        for channel in 0..16u8 {
            assert_eq!(
                decode(&[0x90 | channel, 60, 64]),
                Some(MidiMessage::NoteOn {
                    channel,
                    key: 60,
                    velocity: 64
                })
            );
        }
    }
}
