// fontelle-asio — hosts an ASIO driver for Fontelle in a separate process.
// Copyright (C) 2026 Fopull LLC
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation, version 3. It is distributed WITHOUT ANY WARRANTY; see
// the GNU General Public License (`LICENSE` beside `Cargo.toml`) for details.
//
// The sample type numbers are ASIO's, from Steinberg's ASIO SDK
// (`common/asio.h`), used under the SDK's GPLv3 option. ASIO is a registered
// trademark of Steinberg Media Technologies GmbH.

//! Between Fontelle's interleaved `f32` and a driver's own format.
//!
//! An ASIO driver owns its buffers and says per channel what is in them —
//! most often 32-bit integers, sometimes 24-bit packed, floats on a few. The
//! conversion is here and only here; Fontelle only ever sees `f32`.

/// A driver channel's sample format, by ASIO's numbering (`ASIOSampleType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleType {
    Int16Msb,
    Int24Msb,
    Int32Msb,
    Float32Msb,
    Float64Msb,
    Int32Msb16,
    Int32Msb18,
    Int32Msb20,
    Int32Msb24,
    Int16Lsb,
    Int24Lsb,
    Int32Lsb,
    Float32Lsb,
    Float64Lsb,
    Int32Lsb16,
    Int32Lsb18,
    Int32Lsb20,
    Int32Lsb24,
}

impl SampleType {
    /// From ASIO's number; `None` for DSD and anything newer.
    pub fn from_asio(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::Int16Msb,
            1 => Self::Int24Msb,
            2 => Self::Int32Msb,
            3 => Self::Float32Msb,
            4 => Self::Float64Msb,
            8 => Self::Int32Msb16,
            9 => Self::Int32Msb18,
            10 => Self::Int32Msb20,
            11 => Self::Int32Msb24,
            16 => Self::Int16Lsb,
            17 => Self::Int24Lsb,
            18 => Self::Int32Lsb,
            19 => Self::Float32Lsb,
            20 => Self::Float64Lsb,
            24 => Self::Int32Lsb16,
            25 => Self::Int32Lsb18,
            26 => Self::Int32Lsb20,
            27 => Self::Int32Lsb24,
            _ => return None,
        })
    }

    /// ASIO's name for it, as the ready line says it: "Int32LSB".
    pub fn name(self) -> &'static str {
        match self {
            Self::Int16Msb => "Int16MSB",
            Self::Int24Msb => "Int24MSB",
            Self::Int32Msb => "Int32MSB",
            Self::Float32Msb => "Float32MSB",
            Self::Float64Msb => "Float64MSB",
            Self::Int32Msb16 => "Int32MSB16",
            Self::Int32Msb18 => "Int32MSB18",
            Self::Int32Msb20 => "Int32MSB20",
            Self::Int32Msb24 => "Int32MSB24",
            Self::Int16Lsb => "Int16LSB",
            Self::Int24Lsb => "Int24LSB",
            Self::Int32Lsb => "Int32LSB",
            Self::Float32Lsb => "Float32LSB",
            Self::Float64Lsb => "Float64LSB",
            Self::Int32Lsb16 => "Int32LSB16",
            Self::Int32Lsb18 => "Int32LSB18",
            Self::Int32Lsb20 => "Int32LSB20",
            Self::Int32Lsb24 => "Int32LSB24",
        }
    }

    /// Bytes per sample in the driver's buffer.
    pub fn bytes(self) -> usize {
        match self.shape().0 {
            Shape::Int { bytes, .. } => bytes,
            Shape::F32 => 4,
            Shape::F64 => 8,
        }
    }

    /// How the sample is stored, and whether it is big-endian.
    fn shape(self) -> (Shape, bool) {
        let int = |bytes, bits| Shape::Int { bytes, bits };
        match self {
            Self::Int16Msb => (int(2, 16), true),
            Self::Int24Msb => (int(3, 24), true),
            Self::Int32Msb => (int(4, 32), true),
            Self::Float32Msb => (Shape::F32, true),
            Self::Float64Msb => (Shape::F64, true),
            Self::Int32Msb16 => (int(4, 16), true),
            Self::Int32Msb18 => (int(4, 18), true),
            Self::Int32Msb20 => (int(4, 20), true),
            Self::Int32Msb24 => (int(4, 24), true),
            Self::Int16Lsb => (int(2, 16), false),
            Self::Int24Lsb => (int(3, 24), false),
            Self::Int32Lsb => (int(4, 32), false),
            Self::Float32Lsb => (Shape::F32, false),
            Self::Float64Lsb => (Shape::F64, false),
            Self::Int32Lsb16 => (int(4, 16), false),
            Self::Int32Lsb18 => (int(4, 18), false),
            Self::Int32Lsb20 => (int(4, 20), false),
            Self::Int32Lsb24 => (int(4, 24), false),
        }
    }
}

/// An integer of `bits` significant bits, right-aligned in `bytes` (ASIO's
/// "32 bit data with 24 bit alignment" is 4 bytes, 24 bits), or a float.
#[derive(Clone, Copy)]
enum Shape {
    Int { bytes: usize, bits: u32 },
    F32,
    F64,
}

/// `x` brought inside ±1, with not-a-number as silence.
fn clip(x: f32) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        f64::from(x.clamp(-1.0, 1.0))
    }
}

/// Writes channel `channel` of `interleaved` (`channels` to a frame) into
/// `dst` in `ty`, one sample per frame, as many frames as `dst` holds.
/// Samples past ±1.0 are clipped, not wrapped.
pub fn write_channel(
    ty: SampleType,
    interleaved: &[f32],
    channels: usize,
    channel: usize,
    dst: &mut [u8],
) {
    let (shape, big) = ty.shape();
    let width = ty.bytes();
    let frames = (dst.len() / width).min(interleaved.len() / channels.max(1));
    for (i, out) in dst.chunks_exact_mut(width).take(frames).enumerate() {
        let x = clip(interleaved[i * channels + channel]);
        match shape {
            Shape::Int { bits, .. } => {
                let full = ((1i64 << (bits - 1)) - 1) as f64;
                let v = (x * full).round() as i64;
                let le = v.to_le_bytes();
                // The low `width` bytes, little-endian; reversed for MSB.
                out.copy_from_slice(&le[..width]);
                if big {
                    out.reverse();
                }
            }
            Shape::F32 => {
                let b = if big {
                    (x as f32).to_be_bytes()
                } else {
                    (x as f32).to_le_bytes()
                };
                out.copy_from_slice(&b);
            }
            Shape::F64 => {
                let b = if big {
                    x.to_be_bytes()
                } else {
                    x.to_le_bytes()
                };
                out.copy_from_slice(&b);
            }
        }
    }
}

/// Reads `src` in `ty` into channel `channel` of `interleaved`.
pub fn read_channel(
    ty: SampleType,
    src: &[u8],
    interleaved: &mut [f32],
    channels: usize,
    channel: usize,
) {
    let (shape, big) = ty.shape();
    let width = ty.bytes();
    let frames = (src.len() / width).min(interleaved.len() / channels.max(1));
    for (i, sample) in src.chunks_exact(width).take(frames).enumerate() {
        let mut b = [0u8; 8];
        b[..width].copy_from_slice(sample);
        if big {
            b[..width].reverse();
        }
        let x = match shape {
            Shape::Int { bits, bytes } => {
                // Sign-extend from the top of the stored bytes.
                let shift = 64 - 8 * bytes as u32;
                let v = (i64::from_le_bytes(b) << shift) >> shift;
                let full = ((1i64 << (bits - 1)) - 1) as f64;
                (v as f64 / full) as f32
            }
            Shape::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            Shape::F64 => f64::from_le_bytes(b) as f32,
        };
        interleaved[i * channels + channel] = x;
    }
}
