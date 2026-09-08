//! Ogg Vorbis wearing a `.wav` file's clothes (TDD §15, §17.4).
//!
//! Reported from using the window:
//!
//! > *"i currently cannot drag audio files from the import audio tab. i want to
//! > be able to click and drag them into the sampler or into the channel rack
//! > to make it have a sampler with that clip sampled."*
//!
//! Every `.wav` in the folder that report was made against — FL Studio's
//! `Data/Patches/Packs`, 4054 files of it — is a RIFF/WAVE container whose
//! `fmt ` chunk says format tag **`0x674F`** and whose `data` chunk holds a
//! complete Ogg stream. That is the Vorbis ACM tagging (`WAVE_FORMAT_VORBIS1`
//! and its five siblings), and it is how FL ships compressed samples under a
//! name every host will open. Symphonia's RIFF reader knows PCM and ADPCM and
//! quite correctly refuses this, so *every* file the user could reach came
//! back as *"is not a readable sound file"* — the drag was not broken so much
//! as it had nothing it could carry.
//!
//! The fix is a container question, not a codec one: **unwrap the RIFF and
//! hand the Ogg stream to the Ogg reader**, which this build already has. So
//! the tests below are mostly about the wrapper — what counts as one, what
//! does not, and what happens to a file that lies about what is inside it.

use fontelle_assets::audio_import::{ogg_in_wav, read_audio};
use fontelle_assets::fixtures::{build_vorbis_wav, build_wav};

/// A real Ogg Vorbis stream: a quarter-second 440 Hz tone, 8 kHz mono, written
/// once with `ffmpeg -f lavfi -i sine=frequency=440:sample_rate=8000:duration=0.25
/// -c:a libvorbis -q:a -1`.
///
/// A blob, unlike every other fixture in this workspace, because a Vorbis
/// stream cannot be hand-built — there is no encoder here and writing one to
/// test a decoder would be its own project. It lives beside the test rather
/// than in `fixtures`, which is deliberately data-free.
const TONE_OGG: &[u8] = include_bytes!("data/tone.ogg");

/// What the tone above actually is, so the assertions below say what they mean.
const TONE_RATE: u32 = 8_000;
const TONE_CHANNELS: u16 = 1;

/// The six format tags the Vorbis ACM ever used: modes 1, 2 and 3, and the
/// same three again with the "+" bitstream layout.
const VORBIS_TAGS: [u16; 6] = [0x674F, 0x6750, 0x6751, 0x676F, 0x6770, 0x6771];

// ------------------------------------------------------------ the wrapper ---

#[test]
fn a_vorbis_tagged_wav_hands_back_the_ogg_stream_inside_it() {
    let wav = build_vorbis_wav(TONE_RATE, TONE_CHANNELS, VORBIS_TAGS[0], TONE_OGG);
    assert_eq!(
        ogg_in_wav(&wav),
        Some(TONE_OGG),
        "the payload came back changed, or not at all"
    );
}

#[test]
fn every_tag_the_vorbis_acm_ever_used_is_recognised() {
    // From the list rather than from the one tag the reported files happen to
    // carry: the same pack ships mode 2 and 3 files, and a reader that knew
    // only 0x674F would fail on those exactly as it failed on all of them.
    for tag in VORBIS_TAGS {
        let wav = build_vorbis_wav(TONE_RATE, TONE_CHANNELS, tag, TONE_OGG);
        assert_eq!(
            ogg_in_wav(&wav),
            Some(TONE_OGG),
            "format tag {tag:#06x} was not recognised as Vorbis-in-WAV"
        );
    }
}

#[test]
fn a_plain_pcm_wav_is_not_an_ogg_in_disguise() {
    // The important negative: a PCM file must go on being read by the RIFF
    // reader, not diverted into an Ogg reader that will refuse it.
    let wav = build_wav(48_000, 1, &[0.0, 0.25, -0.25]);
    assert_eq!(ogg_in_wav(&wav), None);
}

#[test]
fn something_that_is_not_a_riff_file_at_all_is_left_alone() {
    assert_eq!(ogg_in_wav(TONE_OGG), None, "a bare .ogg is not a wrapper");
    assert_eq!(ogg_in_wav(b"not a file"), None);
    assert_eq!(ogg_in_wav(&[]), None);
}

#[test]
fn a_wav_that_stops_in_the_middle_of_a_chunk_is_refused_rather_than_read_past() {
    // Every length here is a place a chunk walk can run off the end of the
    // buffer. None of them may panic, and none of them may answer `Some`.
    let wav = build_vorbis_wav(TONE_RATE, TONE_CHANNELS, VORBIS_TAGS[0], TONE_OGG);
    // The tone is an odd number of bytes long, so the file ends with a RIFF
    // pad byte that is not part of the chunk — `len() - 2` is the first cut
    // that actually loses sound.
    for cut in [1, 4, 8, 12, 16, 20, 36, 44, 52, wav.len() - 2] {
        assert_eq!(
            ogg_in_wav(&wav[..cut]),
            None,
            "a {cut}-byte fragment was read as a whole file"
        );
    }
}

#[test]
fn a_file_missing_only_its_pad_byte_is_still_whole() {
    // The other side of the cut above, stated rather than left to chance: the
    // byte a RIFF writer adds to word-align an odd chunk carries nothing, and
    // a file that lost it has lost no sound.
    let wav = build_vorbis_wav(TONE_RATE, TONE_CHANNELS, VORBIS_TAGS[0], TONE_OGG);
    assert_eq!(
        TONE_OGG.len() % 2,
        1,
        "this test needs an odd-length payload"
    );
    assert_eq!(ogg_in_wav(&wav[..wav.len() - 1]), Some(TONE_OGG));
}

#[test]
fn a_vorbis_tagged_wav_with_no_data_chunk_has_nothing_to_hand_back() {
    // The `fmt ` chunk says Vorbis and the stream is simply not there. A
    // reader that trusted the tag alone would hand back an empty slice and
    // the error would surface two layers away as "holds no audio track".
    let mut wav = build_vorbis_wav(TONE_RATE, TONE_CHANNELS, VORBIS_TAGS[0], TONE_OGG);
    let at = find(&wav, b"data").expect("the fixture has a data chunk");
    wav.truncate(at);
    // The RIFF size field now overstates what follows; a walk must stop at the
    // buffer's end regardless of what the header claims.
    assert_eq!(ogg_in_wav(&wav), None);
}

// -------------------------------------------------------------- the sound ---

#[test]
fn an_ogg_inside_a_wav_decodes_to_the_same_sound_as_the_bare_ogg() {
    let bare = read_audio(TONE_OGG, "tone.ogg").expect("a bare .ogg must decode");
    let wav = build_vorbis_wav(TONE_RATE, TONE_CHANNELS, VORBIS_TAGS[0], TONE_OGG);
    let wrapped = read_audio(&wav, "tone.wav").expect("a Vorbis-in-WAV must decode");

    assert_eq!(wrapped.sample_rate, bare.sample_rate);
    assert_eq!(wrapped.channels, bare.channels);
    assert_eq!(wrapped.frames, bare.frames);
    assert_eq!(
        wrapped.samples, bare.samples,
        "the wrapper changed the sound"
    );
}

#[test]
fn the_sound_inside_is_believed_over_what_the_wrapper_claims() {
    // FL's `fmt ` chunk describes the *decoded* stream badly: the reported
    // files say 44100 Hz stereo with a block align of 1, which is not a
    // description of anything. The Ogg stream says what it is, and taking the
    // wrapper's word for it would play every one of these at the wrong speed.
    let wav = build_vorbis_wav(44_100, 2, VORBIS_TAGS[0], TONE_OGG);
    let asset = read_audio(&wav, "lying.wav").expect("decodes");
    assert_eq!(asset.sample_rate, TONE_RATE, "the wrapper's rate was used");
    assert_eq!(
        asset.channels, TONE_CHANNELS,
        "the wrapper's count was used"
    );
}

#[test]
fn a_vorbis_tagged_wav_holding_rubbish_says_so_rather_than_panicking() {
    let wav = build_vorbis_wav(TONE_RATE, TONE_CHANNELS, VORBIS_TAGS[0], b"OggSnonsense");
    let error = read_audio(&wav, "broken.wav").expect_err("must refuse");
    assert!(
        error.0.contains("broken.wav"),
        "the error does not say which file it is about: {error:?}"
    );
}

#[test]
fn a_plain_pcm_wav_still_reads_the_way_it_always_did() {
    // The regression this whole change could cause, stated as a test: the
    // ordinary path must not go anywhere near the Ogg reader.
    let samples: Vec<f32> = (0..480).map(|i| (i as f32 / 480.0) - 0.5).collect();
    let wav = build_wav(48_000, 1, &samples);
    let asset = read_audio(&wav, "plain.wav").expect("decodes");
    assert_eq!(asset.sample_rate, 48_000);
    assert_eq!(asset.channels, 1);
    assert_eq!(asset.frames, samples.len());
}

/// Where `needle` first appears in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
