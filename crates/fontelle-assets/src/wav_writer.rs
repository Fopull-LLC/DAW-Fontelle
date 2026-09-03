//! Writing a take to disk while it is still being recorded (TDD §15.4).
//!
//! > *"Recording writes directly to the configured recordings directory as
//! > WAV, streaming from the RT thread through a lock-free ring to the disk
//! > thread. The RT thread never touches the filesystem. ... A recording in
//! > progress is crash-safe: the WAV header is finalised incrementally so a
//! > killed process leaves a playable file."*
//!
//! # How the crash-safety works
//!
//! Every write does two things: append the samples, then **seek back and
//! rewrite the two length fields**. So the file on disk is a complete, valid
//! WAV after every block, and a process killed at any moment leaves a file that
//! opens and holds everything that had been captured up to then.
//!
//! The alternative — write the header once at the end — costs one seek less per
//! block and produces a silently zero-length file whenever anything goes wrong,
//! which is exactly the moment the take mattered most.
//!
//! # Sixteen bits, and why
//!
//! It is what a take is expected to be, it is half the size of a float file,
//! and it is what every other program will open without asking. Samples past
//! full scale are **clamped**, not wrapped: an input that peaks over is an
//! ordinary thing that happens, and a wrapped sample is a full-scale click in
//! the middle of somebody's vocal.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use crate::ImportError;

/// Where the `RIFF` chunk's own size field sits, and where `data`'s does.
///
/// The header this writes is the canonical 44-byte one, so both are constants
/// rather than something to track.
const RIFF_SIZE_AT: u64 = 4;
const DATA_SIZE_AT: u64 = 40;
const HEADER_BYTES: u32 = 44;

/// A take, being written.
#[derive(Debug)]
pub struct WavWriter {
    file: File,
    channels: u16,
    /// Samples written so far, across every channel.
    written: u64,
}

impl WavWriter {
    /// Creates `path` and writes the header for an empty take.
    pub fn create(path: &Path, sample_rate: u32, channels: u16) -> Result<Self, ImportError> {
        let channels = channels.max(1);
        let mut file = File::create(path)
            .map_err(|e| ImportError(format!("could not write {}: {e}", path.display())))?;
        let block_align = channels * 2;
        let byte_rate = sample_rate * u32::from(block_align);

        let mut header = Vec::with_capacity(HEADER_BYTES as usize);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&(HEADER_BYTES - 8).to_le_bytes());
        header.extend_from_slice(b"WAVE");
        header.extend_from_slice(b"fmt ");
        header.extend_from_slice(&16u32.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes()); // PCM
        header.extend_from_slice(&channels.to_le_bytes());
        header.extend_from_slice(&sample_rate.to_le_bytes());
        header.extend_from_slice(&byte_rate.to_le_bytes());
        header.extend_from_slice(&block_align.to_le_bytes());
        header.extend_from_slice(&16u16.to_le_bytes()); // bits
        header.extend_from_slice(b"data");
        header.extend_from_slice(&0u32.to_le_bytes());
        file.write_all(&header)
            .map_err(|e| ImportError(format!("could not write {}: {e}", path.display())))?;

        Ok(Self {
            file,
            channels,
            written: 0,
        })
    }

    /// How many whole frames have been written.
    pub fn frames(&self) -> u64 {
        self.written / u64::from(self.channels)
    }

    /// Appends `block`, then rewrites the header so what is on disk is a
    /// playable file again.
    pub fn write(&mut self, block: &[f32]) -> Result<(), ImportError> {
        if block.is_empty() {
            return Ok(());
        }
        let mut bytes = Vec::with_capacity(block.len() * 2);
        for sample in block {
            // Symmetric and clamped, which is what every encoder does: scaling
            // by 32768 lets +1.0 wrap to -32768, and one wrapped sample is a
            // click.
            let scaled = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            bytes.extend_from_slice(&scaled.to_le_bytes());
        }
        self.file
            .write_all(&bytes)
            .map_err(|e| ImportError(format!("could not write the take: {e}")))?;
        self.written += block.len() as u64;
        self.finalise()
    }

    /// Flushes and closes. Not required for the file to be readable — see this
    /// module's own note — but it is where an error gets reported rather than
    /// swallowed by a drop.
    pub fn finish(mut self) -> Result<(), ImportError> {
        self.finalise()?;
        self.file
            .flush()
            .map_err(|e| ImportError(format!("could not close the take: {e}")))
    }

    /// Rewrites the two length fields against what has actually been written.
    fn finalise(&mut self) -> Result<(), ImportError> {
        let data_bytes = (self.written * 2).min(u64::from(u32::MAX - HEADER_BYTES)) as u32;
        let seek = |file: &mut File, at: u64, value: u32| -> Result<(), ImportError> {
            file.seek(SeekFrom::Start(at))
                .and_then(|_| file.write_all(&value.to_le_bytes()))
                .map_err(|e| ImportError(format!("could not update the take's header: {e}")))
        };
        seek(&mut self.file, RIFF_SIZE_AT, HEADER_BYTES - 8 + data_bytes)?;
        seek(&mut self.file, DATA_SIZE_AT, data_bytes)?;
        // Back to the end, or the next block would overwrite the header.
        self.file
            .seek(SeekFrom::End(0))
            .map_err(|e| ImportError(format!("could not seek to the end of the take: {e}")))?;
        Ok(())
    }
}
