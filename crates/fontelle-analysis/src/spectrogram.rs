//! The picture the note lane can show behind the notes instead of the
//! waveform (plan §3.3, Tab): basic-pitch's pitch contour, a third of a
//! semitone a row, as bytes.
//!
//! Small on purpose — what the window keeps, rather than the posteriorgrams
//! (27 MB for three minutes): the contour, put on uniform columns in time
//! and squeezed to a byte a cell.

use crate::transcribe::notes::{BEND_CENTRE_BIN, MIDI_OFFSET, N_CONTOUR_BINS, Posteriorgrams};

/// Columns a second the image is made at: 20 ms each, finer than a note
/// and coarser than the model's 11.6 ms frames.
pub const COLUMNS_PER_SECOND: f64 = 50.0;

/// The contour as an image: `data[column * rows + row]`, row 0 lowest.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ContourImage {
    pub columns: usize,
    pub rows: usize,
    pub columns_per_second: f64,
    pub data: Vec<u8>,
}

impl ContourImage {
    /// The image of `post`, over `duration` seconds of audio.
    ///
    /// Each column takes the strongest frame whose time falls in it (frames
    /// are not evenly spaced: see [`Posteriorgrams::frame_time`]), so a short
    /// note is not averaged away. A cell is the square root of the
    /// activation, which is what makes a quiet partial visible without a
    /// loud note burning out.
    pub fn of(post: &Posteriorgrams, duration: f64) -> Self {
        let rows = N_CONTOUR_BINS;
        let columns = (duration * COLUMNS_PER_SECOND).round().max(0.0) as usize;
        let mut peak = vec![0.0f32; columns * rows];
        for frame in 0..post.frames {
            let column = (Posteriorgrams::frame_time(frame) * COLUMNS_PER_SECOND) as usize;
            if column >= columns {
                break;
            }
            let Some(row) = post.contour.get(frame * rows..(frame + 1) * rows) else {
                break;
            };
            let cells = &mut peak[column * rows..(column + 1) * rows];
            for (cell, v) in cells.iter_mut().zip(row) {
                *cell = cell.max(*v);
            }
        }
        Self {
            columns,
            rows,
            columns_per_second: COLUMNS_PER_SECOND,
            data: peak
                .into_iter()
                .map(|v| (v.clamp(0.0, 1.0).sqrt() * 255.0).round() as u8)
                .collect(),
        }
    }

    /// One cell; 0 outside the image.
    pub fn at(&self, column: usize, row: usize) -> u8 {
        if column >= self.columns || row >= self.rows {
            return 0;
        }
        self.data[column * self.rows + row]
    }

    /// The pitch a row stands for, in MIDI (fractional): a key's own pitch
    /// is the middle of its three rows.
    pub fn midi_of_row(&self, row: usize) -> f32 {
        f32::from(MIDI_OFFSET) + (row as f32 - f32::from(BEND_CENTRE_BIN)) / 3.0
    }

    /// The other way, fractional.
    pub fn row_of_midi(&self, midi: f32) -> f32 {
        (midi - f32::from(MIDI_OFFSET)) * 3.0 + f32::from(BEND_CENTRE_BIN)
    }

    /// The bytes [`crate::cache::store_image`] writes: columns, rows (u32
    /// LE), columns a second (f64 LE), then the cells.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.data.len());
        out.extend_from_slice(&(self.columns as u32).to_le_bytes());
        out.extend_from_slice(&(self.rows as u32).to_le_bytes());
        out.extend_from_slice(&self.columns_per_second.to_le_bytes());
        out.extend_from_slice(&self.data);
        out
    }

    /// Read back; `None` for anything that is not exactly one.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let columns = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?) as usize;
        let rows = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
        let columns_per_second = f64::from_le_bytes(bytes.get(8..16)?.try_into().ok()?);
        let data = bytes.get(16..)?;
        (data.len() == columns.checked_mul(rows)? && columns_per_second.is_finite()).then(|| Self {
            columns,
            rows,
            columns_per_second,
            data: data.to_vec(),
        })
    }
}
