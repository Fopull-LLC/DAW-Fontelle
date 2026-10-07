//! The little of a zip a compressed MusicXML needs: the central directory,
//! stored and deflated entries. Names are UTF-8 (MusicXML's own rule).

/// Every file in the archive, `(name, bytes)`.
pub fn read(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let u16_at = |at: usize| -> Option<usize> {
        Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?) as usize)
    };
    let u32_at = |at: usize| -> Option<usize> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
    };
    // The end record is in the last 64 KiB (its comment is at most that).
    let floor = bytes.len().saturating_sub(65_557);
    let end = (floor..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or("not a zip archive")?;
    let count = u16_at(end + 10).ok_or("a damaged zip")?;
    let mut at = u32_at(end + 16).ok_or("a damaged zip")?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(at..at + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err("a damaged zip directory".into());
        }
        let method = u16_at(at + 10).ok_or("a damaged zip")?;
        let packed = u32_at(at + 20).ok_or("a damaged zip")?;
        let size = u32_at(at + 24).ok_or("a damaged zip")?;
        let name_len = u16_at(at + 28).ok_or("a damaged zip")?;
        let extra_len = u16_at(at + 30).ok_or("a damaged zip")?;
        let comment_len = u16_at(at + 32).ok_or("a damaged zip")?;
        let local = u32_at(at + 42).ok_or("a damaged zip")?;
        let name = String::from_utf8_lossy(
            bytes
                .get(at + 46..at + 46 + name_len)
                .ok_or("a damaged zip")?,
        )
        .into_owned();
        at += 46 + name_len + extra_len + comment_len;
        if name.ends_with('/') {
            continue;
        }
        let local_name = u16_at(local + 26).ok_or("a damaged zip")?;
        let local_extra = u16_at(local + 28).ok_or("a damaged zip")?;
        let start = local + 30 + local_name + local_extra;
        let data = bytes
            .get(start..start + packed)
            .ok_or("a zip entry runs past the end")?;
        let data = match method {
            0 => data.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(data, size.max(1) * 2 + 1024)
                .map_err(|e| format!("a zip entry would not inflate: {e:?}"))?,
            other => return Err(format!("a zip entry packed by method {other}")),
        };
        out.push((name, data));
    }
    Ok(out)
}
