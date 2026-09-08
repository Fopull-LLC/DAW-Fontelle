//! The one encoding an opaque blob travels through `project.json` in.
//!
//! A hosted plugin's state is bytes only it understands (TDD §8.4), and the
//! document is JSON (§17.2). `serde_json` writes a `Vec<u8>` as an array of
//! decimal numbers — four to five characters per byte, on a blob that can run
//! to megabytes for a sampler — which is the same reason `Channel::patch_data`
//! stopped being one.
//!
//! Written here rather than taken from a crate because it is forty lines and
//! six test vectors that have not changed since RFC 4648, and because a
//! dependency that reads a project file is one more thing that has to be
//! trusted to open somebody's song.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64, padded — RFC 4648 §4.
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let packed = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(packed >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(packed >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(packed >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[packed as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

/// Reads one back. `None` for anything that is not exactly what
/// [`encode_base64`] writes.
///
/// Strict rather than forgiving — no whitespace, no missing padding, no
/// alternative alphabet. This decodes one thing: a blob this program wrote
/// into a file it wrote. Text that does not match it is damage, and a
/// half-decoded plugin state is worse than a plugin that opens at its
/// defaults.
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let chunks = bytes.len() / 4;
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let padding = chunk.iter().rev().take_while(|b| **b == b'=').count();
        // Padding ends the text; a chunk with it in the middle is two blobs
        // stuck together, not a long one.
        if padding > 2 || (padding > 0 && index + 1 != chunks) {
            return None;
        }
        let mut packed = 0u32;
        for (place, byte) in chunk.iter().enumerate() {
            let value = if place >= 4 - padding {
                0
            } else {
                sextet(*byte)? as u32
            };
            packed = (packed << 6) | value;
        }
        out.push((packed >> 16) as u8);
        if padding < 2 {
            out.push((packed >> 8) as u8);
        }
        if padding < 1 {
            out.push(packed as u8);
        }
    }
    Some(out)
}

fn sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}
