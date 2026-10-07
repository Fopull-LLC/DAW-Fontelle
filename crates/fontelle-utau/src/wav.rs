//! WAV files in and out.

use std::path::Path;

/// Mono samples (channels averaged) and the rate.
pub fn read_wav(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_wav(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// PCM 8/16/24/32-bit and float 32/64, any channel count, mono out.
pub fn parse_wav(bytes: &[u8]) -> Result<(Vec<f32>, u32), String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at =
        |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32_at(pos + 4) as usize;
        let body = pos + 8;
        let end = (body + size).min(bytes.len());
        if id == b"fmt " && size >= 16 && body + 16 <= bytes.len() {
            let mut format = u16_at(body);
            if format == 0xFFFE && size >= 26 && body + 26 <= bytes.len() {
                format = u16_at(body + 24);
            }
            fmt = Some((
                format,
                u16_at(body + 2),
                u32_at(body + 4),
                u16_at(body + 14),
            ));
        } else if id == b"data" {
            let (format, channels, rate, bits) = fmt.ok_or("data before fmt")?;
            let channels = channels.max(1) as usize;
            let width = (bits as usize).div_ceil(8);
            if width == 0 {
                return Err("zero-width samples".into());
            }
            let data = &bytes[body..end];
            let frames = data.len() / (width * channels);
            let read = |i: usize| -> f32 {
                let b = &data[i * width..(i + 1) * width];
                match (format, width) {
                    (1, 1) => (f32::from(b[0]) - 128.0) / 128.0,
                    (1, 2) => f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0,
                    (1, 3) => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0,
                    (1, 4) => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
                    (3, 4) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                    (3, 8) => {
                        f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32
                    }
                    _ => 0.0,
                }
            };
            if !matches!((format, width), (1, 1..=4) | (3, 4) | (3, 8)) {
                return Err(format!("unsupported WAV format {format}/{bits}"));
            }
            let mut out = Vec::with_capacity(frames);
            for f in 0..frames {
                let mut s = 0.0;
                for c in 0..channels {
                    s += read(f * channels + c);
                }
                out.push(s / channels as f32);
            }
            return Ok((out, rate));
        }
        pos = body + size + (size & 1);
    }
    Err("no data chunk".into())
}

/// 16-bit mono.
pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let mut b = Vec::with_capacity(44 + samples.len() * 2);
    let data_len = (samples.len() * 2) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, b)
}

/// Band-limited resampling to `to` Hz: a Blackman-windowed sinc, 16 zero
/// crossings either side, cut off just under the lower Nyquist.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() || from == 0 || to == 0 {
        return x.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let n_out = (x.len() as f64 * ratio).round() as usize;
    let cutoff = 0.95 * ratio.min(1.0);
    const ZEROS: f64 = 16.0;
    let half = (ZEROS / cutoff).ceil() as isize;
    let mut out = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let p = i as f64 / ratio;
        let centre = p.floor() as isize;
        let mut acc = 0.0f64;
        for k in centre - half..=centre + half {
            if k < 0 || k as usize >= x.len() {
                continue;
            }
            let d = p - k as f64;
            let arg = d * cutoff;
            let sinc = if arg.abs() < 1e-9 {
                1.0
            } else {
                (std::f64::consts::PI * arg).sin() / (std::f64::consts::PI * arg)
            };
            let w = d / (half as f64 + 1.0);
            if w.abs() >= 1.0 {
                continue;
            }
            let u = 0.5 * (w + 1.0);
            let win = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * u).cos()
                + 0.08 * (4.0 * std::f64::consts::PI * u).cos();
            acc += f64::from(x[k as usize]) * cutoff * sinc * win;
        }
        out.push(acc as f32);
    }
    out
}
