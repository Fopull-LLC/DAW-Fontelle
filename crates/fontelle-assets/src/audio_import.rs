//! Reading a sound off disk (TDD §15, §17.4).
//!
//! Reported from using the window:
//!
//! > *"right now we can basically only do things with soundfonts but i want to
//! > also be able to record my voice into the daw or import different sounds
//! > and loops and whatnot to make songs with."*
//!
//! Both halves of that start here. An audio clip is a **reference to decoded
//! samples** and everything after it — the waveform in the block, the fades,
//! the per-clip filter, the take that lands as a new file — is something done
//! to those samples at playback time, never to the file (§15.1: *"the source
//! file is never modified"*).
//!
//! # What this deliberately does not do
//!
//! **It does not resample.** A file records the rate it was written at and the
//! clip player reads it at whatever ratio the device asks for, because
//! resampling on import throws away the original and there is nothing to go
//! back to. §7.6's interpolation lives in the player for exactly this reason.
//!
//! **It does not stream.** §7.7's threshold is about soundfonts, which are
//! gigabytes; an audio clip is a take or a loop and holding it is the simple
//! thing that works. A long file is a real cost and is called out in
//! `PROGRESS.md` rather than pretended away.

use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::ImportError;

/// Decoded audio, held whole.
///
/// Interleaved rather than planar because that is what every decoder hands
/// back and what a clip player reads a frame at a time; [`sample`](Self::sample)
/// is the only place the arithmetic lives.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioAsset {
    /// The rate the file was written at. **Not** the device's — see the module
    /// note on resampling.
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: usize,
    /// `frames * channels` values, interleaved, in −1..=1.
    pub samples: Vec<f32>,
}

impl AudioAsset {
    /// One sample, by frame and channel. Silence off either end, so a player
    /// running past the last frame fades out rather than panicking on the RT
    /// thread.
    pub fn sample(&self, frame: usize, channel: u16) -> f32 {
        if frame >= self.frames || channel >= self.channels {
            return 0.0;
        }
        self.samples[frame * self.channels as usize + channel as usize]
    }

    /// How long it is, at its own rate.
    pub fn seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frames as f64 / f64::from(self.sample_rate)
    }
}

/// Reads and decodes the file at `path`.
pub fn import_audio(path: &Path) -> Result<AudioAsset, ImportError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let bytes =
        std::fs::read(path).map_err(|e| ImportError(format!("could not read {name}: {e}")))?;
    read_audio(&bytes, &name)
}

/// How long the file at `path` is — frames, and the rate they are at —
/// **without decoding it** when the container says.
///
/// > *"the preview for dragging in things into the arrangement ... showed
/// > the preview just taking up the entire lane."*
///
/// The block a dragged sound will become has to be drawn while the sound is
/// still in the air, and decoding a whole file to find out how wide to draw
/// a rectangle is a stall at the moment the drag enters the window. A `.wav`
/// and a `.flac` carry their length in the header; an MP3 with a Xing frame
/// does too. A stream that does not say is decoded, once — the answer has to
/// be right, because the block drawn is the block that lands.
pub fn audio_length(path: &Path) -> Result<(u64, u32), ImportError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let bytes =
        std::fs::read(path).map_err(|e| ImportError(format!("could not read {name}: {e}")))?;
    if bytes.is_empty() {
        return Err(ImportError(format!("{name} is empty")));
    }
    let (probed, wrapped) = match ogg_in_wav(&bytes) {
        Some(ogg) => (ogg, true),
        None => (bytes.as_slice(), false),
    };
    let source = std::io::Cursor::new(probed.to_vec());
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let mut hint = Hint::new();
    if wrapped {
        hint.with_extension("ogg");
    } else if let Some(extension) = Path::new(&name).extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| ImportError(format!("{name} is not a readable sound file: {e}")))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| ImportError(format!("{name} holds no audio track")))?;
    let rate = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .and_then(|p| p.sample_rate);
    if let (Some(frames), Some(rate)) = (track.num_frames, rate)
        && frames > 0
        && rate > 0
    {
        return Ok((frames, rate));
    }
    let decoded = read_audio(&bytes, &name)?;
    Ok((decoded.frames as u64, decoded.sample_rate))
}

/// The same, on bytes already in hand — what a test uses, and what a
/// recording's own buffer would use if it ever needed decoding.
///
/// `name` is only ever used in the error message, and it is there because a
/// window that says *"could not decode"* with no file in it is a window you
/// cannot act on.
pub fn read_audio(bytes: &[u8], name: &str) -> Result<AudioAsset, ImportError> {
    if bytes.is_empty() {
        // Refused rather than read as silence: a zero-length clip is one that
        // cannot be dragged, resized or heard, and it would arrive on the
        // arrangement looking like a bug in the arrangement.
        return Err(ImportError(format!("{name} is empty")));
    }
    // **A `.wav` that is really an Ogg is unwrapped first.** Every sample in
    // the packs Fontelle is pointed at is one of these; see `ogg_in_wav`.
    let (bytes, wrapped) = match ogg_in_wav(bytes) {
        Some(ogg) => (ogg, true),
        None => (bytes, false),
    };
    let source = std::io::Cursor::new(bytes.to_vec());
    let stream = MediaSourceStream::new(Box::new(source), Default::default());

    // The extension as a hint, which is what lets a `.wav` be probed as one
    // rather than sniffed for. A wrong extension still works: the probe reads
    // the bytes, the hint only orders the candidates. An unwrapped stream is
    // hinted by what it *is* rather than by what the file was called, since
    // the name still ends in `.wav` and that is now the wrong answer.
    let mut hint = Hint::new();
    if wrapped {
        hint.with_extension("ogg");
    } else if let Some(extension) = Path::new(name).extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| ImportError(format!("{name} is not a readable sound file: {e}")))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| ImportError(format!("{name} holds no audio track")))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| ImportError(format!("{name} does not say what it is encoded as")))?
        .clone();

    let sample_rate = params.sample_rate.unwrap_or(0);
    let channels = params
        .channels
        .as_ref()
        .map(|c| c.count() as u16)
        .unwrap_or(0);
    if sample_rate == 0 || channels == 0 {
        return Err(ImportError(format!(
            "{name} does not say its sample rate or channel count"
        )));
    }

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| ImportError(format!("nothing here can decode {name}: {e}")))?;

    let mut samples: Vec<f32> = Vec::new();
    let mut packet_buffer: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // A truncated file: keep what decoded. A take whose tail was lost
            // to a crash is still a take, and §15.4 promises exactly that of a
            // recording killed mid-write.
            Err(_) => break,
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                packet_buffer.resize(decoded.samples_interleaved(), 0.0);
                decoded.copy_to_slice_interleaved(&mut packet_buffer);
                samples.extend_from_slice(&packet_buffer);
            }
            // One bad packet is not a bad file — symphonia's own guidance, and
            // the difference between a dropout and a refusal.
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(ImportError(format!("{name} failed to decode: {e}"))),
        }
    }

    let frames = samples.len() / channels as usize;
    if frames == 0 {
        return Err(ImportError(format!("{name} decoded to nothing at all")));
    }
    samples.truncate(frames * channels as usize);
    Ok(AudioAsset {
        sample_rate,
        channels,
        frames,
        samples,
    })
}

/// The Ogg stream inside a RIFF/WAVE file that is really Vorbis, if it is one.
///
/// > *"i currently cannot drag audio files from the import audio tab."*
///
/// FL Studio ships its sample packs as RIFF/WAVE containers whose `fmt ` chunk
/// carries a **Vorbis ACM format tag** and whose `data` chunk is a whole Ogg
/// stream, headers and all. Every `.wav` in the folder that report was made
/// against is one, so "cannot drag audio files" was, underneath, "cannot read
/// any of these files": symphonia's RIFF reader knows PCM and ADPCM and quite
/// rightly refuses a tag it has never heard of.
///
/// Unwrapping is the whole fix. The Ogg reader is already in this build, and
/// what comes out of it is a normal Vorbis stream that says its own rate and
/// channel count — which is just as well, because the `fmt ` chunk around it
/// describes nothing: a block align of one, sixteen bits per sample, and a
/// channel count that is whatever the encoder felt like writing.
///
/// `None` for anything else, **including a PCM `.wav`**, which must go on being
/// read by the reader that has always read it.
pub fn ogg_in_wav(bytes: &[u8]) -> Option<&[u8]> {
    // The six tags the Vorbis ACM ever used: modes 1, 2 and 3, and the same
    // three again with the "+" bitstream layout. Listed rather than
    // range-checked, because the numbers either side of them mean other codecs.
    const VORBIS_TAGS: [u16; 6] = [0x674F, 0x6750, 0x6751, 0x676F, 0x6770, 0x6771];

    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let (mut vorbis, mut data) = (false, None);
    // **Bounded by the buffer, not by the header.** A RIFF size field is a
    // claim, and a file that was cut short claims a length that is not there.
    let mut at = 12usize;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let from = at + 8;
        let to = from.checked_add(size)?;
        if to > bytes.len() {
            // A chunk that runs off the end: whatever this file is, it is not
            // one to hand a decoder half of.
            return None;
        }
        match id {
            b"fmt " if size >= 2 => {
                let tag = u16::from_le_bytes([bytes[from], bytes[from + 1]]);
                vorbis = VORBIS_TAGS.contains(&tag);
                // Nothing to gain by reading on once it is not Vorbis: this is
                // the ordinary path, and it is every other file in the world.
                if !vorbis {
                    return None;
                }
            }
            b"data" => data = Some(&bytes[from..to]),
            _ => {}
        }
        // Chunks are word-aligned: an odd length is followed by a pad byte
        // that is not part of it.
        at = to + (size & 1);
    }
    match (vorbis, data) {
        (true, Some(ogg)) if !ogg.is_empty() => Some(ogg),
        _ => None,
    }
}
