//! Voicebank portraits: decoded once, made small, kept by path.
//!
//! A UTAU bank's `character.txt` names its picture with `image=`, more
//! often than not a BMP (a 100×100 picture from the Windows XP days, read
//! here by [`decode_bmp`]: 1/4/8-bit palettes, 16/24/32-bit, `BI_BITFIELDS`,
//! bottom-up or top-down); a DiffSinger bank's `character.yaml` names a PNG
//! or a JPEG (`portrait:`/`image:`), read by the decoders the theme pictures
//! already use — or a WebP, whatever its extension says ([`decode_webp`]). Anything larger than [`MAX_PORTRAIT`] a side is box-filtered
//! down: a 1000×1000 portrait drawn at 60 px is 4 MB for nothing.

use std::collections::HashMap;
use std::sync::Arc;

use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

/// The longest side a portrait is kept at.
pub const MAX_PORTRAIT: u32 = 256;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A BMP to (width, height, RGBA top-down).
pub fn decode_bmp(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let bad = || "not a BMP Fontelle can read".to_string();
    if !bytes.starts_with(b"BM") {
        return Err(bad());
    }
    let offset = u32_at(bytes, 10).ok_or_else(bad)? as usize;
    let header = u32_at(bytes, 14).ok_or_else(bad)? as usize;
    let (width, height, bits, compression, colours) = if header == 12 {
        // OS/2 BITMAPCOREHEADER.
        (
            i64::from(u16_at(bytes, 18).ok_or_else(bad)?),
            i64::from(u16_at(bytes, 20).ok_or_else(bad)?),
            u16_at(bytes, 24).ok_or_else(bad)?,
            0,
            0,
        )
    } else if header >= 40 {
        (
            i64::from(u32_at(bytes, 18).ok_or_else(bad)? as i32),
            i64::from(u32_at(bytes, 22).ok_or_else(bad)? as i32),
            u16_at(bytes, 28).ok_or_else(bad)?,
            u32_at(bytes, 30).ok_or_else(bad)?,
            u32_at(bytes, 46).ok_or_else(bad)?,
        )
    } else {
        return Err(bad());
    };
    let top_down = height < 0;
    let (w, h) = (width.unsigned_abs(), height.unsigned_abs());
    if w == 0 || h == 0 || w > 16_384 || h > 16_384 {
        return Err("a BMP of no size, or an enormous one".to_string());
    }
    let (w, h) = (w as usize, h as usize);
    // Masks for 16/32-bit: BI_BITFIELDS gives them, else the defaults.
    let (masks, has_alpha_mask) = match (compression, bits) {
        (3 | 6, 16 | 32) => {
            let at = if header >= 52 { 14 + 40 } else { 14 + header };
            let r = u32_at(bytes, at).ok_or_else(bad)?;
            let g = u32_at(bytes, at + 4).ok_or_else(bad)?;
            let b = u32_at(bytes, at + 8).ok_or_else(bad)?;
            let a = if compression == 6 || header >= 56 {
                u32_at(bytes, at + 12).unwrap_or(0)
            } else {
                0
            };
            ([r, g, b, a], a != 0)
        }
        (0, 16) => ([0x7c00, 0x03e0, 0x001f, 0], false),
        (0, 32) => ([0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000], true),
        (0, 1 | 4 | 8 | 24) => ([0; 4], false),
        _ => return Err("a compressed BMP (RLE), which Fontelle does not read".to_string()),
    };
    let palette_entry = if header == 12 { 3 } else { 4 };
    let palette: Vec<[u8; 3]> = if bits <= 8 {
        let n = if colours == 0 {
            1usize << bits
        } else {
            colours as usize
        };
        let at = 14 + header;
        (0..n.min(256))
            .map(|i| {
                let p = bytes.get(at + i * palette_entry..at + i * palette_entry + 3)?;
                Some([p[2], p[1], p[0]])
            })
            .collect::<Option<_>>()
            .ok_or_else(bad)?
    } else {
        Vec::new()
    };
    let stride = (w * bits as usize).div_ceil(32) * 4;
    if offset
        .checked_add(stride * h)
        .is_none_or(|end| end > bytes.len())
    {
        return Err("the BMP is cut short".to_string());
    }
    let channel = |v: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 0;
        }
        let shift = mask.trailing_zeros();
        let max = mask >> shift;
        (((v & mask) >> shift) * 255 / max.max(1)) as u8
    };
    let mut rgba = vec![0u8; w * h * 4];
    let mut any_alpha = false;
    for y in 0..h {
        let src_row = if top_down { y } else { h - 1 - y };
        let row = &bytes[offset + src_row * stride..offset + (src_row + 1) * stride];
        for x in 0..w {
            let px: [u8; 4] = match bits {
                1 | 4 | 8 => {
                    let bit = x * bits as usize;
                    let byte = row[bit / 8];
                    let index =
                        (byte >> (8 - bits as usize - bit % 8)) & ((1u16 << bits) - 1) as u8;
                    let c = palette.get(index as usize).copied().unwrap_or([0; 3]);
                    [c[0], c[1], c[2], 255]
                }
                24 => [row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255],
                16 | 32 => {
                    let v = if bits == 16 {
                        u32::from(u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]))
                    } else {
                        u32::from_le_bytes(row[x * 4..x * 4 + 4].try_into().unwrap_or([0; 4]))
                    };
                    let a = if has_alpha_mask {
                        channel(v, masks[3])
                    } else {
                        255
                    };
                    [
                        channel(v, masks[0]),
                        channel(v, masks[1]),
                        channel(v, masks[2]),
                        a,
                    ]
                }
                _ => return Err(bad()),
            };
            any_alpha |= px[3] != 0;
            rgba[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&px);
        }
    }
    // A 32-bit BMP whose alpha byte is 0 throughout has no alpha at all.
    if !any_alpha {
        for p in rgba.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
    }
    Ok((w as u32, h as u32, rgba))
}

/// Box-filters an RGBA picture so its longer side is at most `max`.
pub fn downscale(width: u32, height: u32, rgba: &[u8], max: u32) -> (u32, u32, Vec<u8>) {
    if width <= max && height <= max {
        return (width, height, rgba.to_vec());
    }
    let scale = f64::from(max) / f64::from(width.max(height));
    let nw = ((f64::from(width) * scale).round() as u32).max(1);
    let nh = ((f64::from(height) * scale).round() as u32).max(1);
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        let y0 = (u64::from(y) * u64::from(height) / u64::from(nh)) as u32;
        let y1 =
            ((u64::from(y + 1) * u64::from(height)).div_ceil(u64::from(nh)) as u32).min(height);
        for x in 0..nw {
            let x0 = (u64::from(x) * u64::from(width) / u64::from(nw)) as u32;
            let x1 =
                ((u64::from(x + 1) * u64::from(width)).div_ceil(u64::from(nw)) as u32).min(width);
            let mut sum = [0u64; 4];
            let mut n = 0u64;
            for sy in y0..y1.max(y0 + 1) {
                for sx in x0..x1.max(x0 + 1) {
                    let i = ((sy * width + sx) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += u64::from(rgba[i + c]);
                    }
                    n += 1;
                }
            }
            let o = ((y * nw + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (sum[c] / n.max(1)) as u8;
            }
        }
    }
    (nw, nh, out)
}

/// A WebP (some DiffSinger banks ship one, named `.jpg` as often as not).
pub fn decode_webp(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder =
        image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || w > 16_384 || h > 16_384 {
        return Err("a WebP of no size, or an enormous one".to_string());
    }
    let alpha = decoder.has_alpha();
    let size = decoder
        .output_buffer_size()
        .ok_or_else(|| "a WebP too large to read".to_string())?;
    let mut buf = vec![0u8; size];
    decoder.read_image(&mut buf).map_err(|e| e.to_string())?;
    let rgba = if alpha {
        buf
    } else {
        buf.as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect()
    };
    Ok((w, h, rgba))
}

/// A PNG, JPEG, BMP or WebP, by its first bytes, at most [`MAX_PORTRAIT`] a side.
pub fn decode_portrait(bytes: &[u8]) -> Result<ImageData, String> {
    let (w, h, rgba) = if bytes.starts_with(b"BM") {
        decode_bmp(bytes)?
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        decode_webp(bytes)?
    } else {
        let image = crate::theme::decode_image(bytes)?;
        (image.width, image.height, image.data.data().to_vec())
    };
    let (w, h, rgba) = downscale(w, h, &rgba, MAX_PORTRAIT);
    Ok(ImageData {
        data: Blob::new(Arc::new(rgba)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: w,
        height: h,
    })
}

/// Portraits by path, decoded the first time one is asked for; a file that
/// is missing or unreadable is remembered as such.
#[derive(Default)]
pub struct Portraits {
    cache: HashMap<String, Option<ImageData>>,
}

impl Portraits {
    pub fn get(&mut self, path: &str) -> Option<&ImageData> {
        self.cache
            .entry(path.to_string())
            .or_insert_with(|| {
                std::fs::read(path)
                    .ok()
                    .and_then(|b| decode_portrait(&b).ok())
            })
            .as_ref()
    }

    /// Already decoded (or known missing), without touching the disk.
    pub fn peek(&self, path: &str) -> Option<&ImageData> {
        self.cache.get(path).and_then(Option::as_ref)
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Forgets everything (after an install or uninstall).
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

impl std::fmt::Debug for Portraits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Portraits")
            .field("decoded", &self.cache.len())
            .finish()
    }
}
