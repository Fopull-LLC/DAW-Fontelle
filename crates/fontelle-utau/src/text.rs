//! Reading the text files of a voicebank in whatever encoding its author's
//! system wrote them in.
//!
//! UTAU was a Japanese Windows program, so most banks are Shift-JIS
//! (code page 932); Chinese banks are GBK; banks made with OpenUtau or
//! recent tools are UTF-8. Nothing in the files says which, so: valid UTF-8
//! is UTF-8; otherwise the double-byte decodings are tried and the one that
//! reads as its own script wins. GBK hanzi often decode as *valid*
//! Shift-JIS too, but as a jumble without kana, which is what tells them
//! apart.

use encoding_rs::{Encoding, GBK, SHIFT_JIS, UTF_8};

/// The encodings UTAU banks are found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    ShiftJis,
    Gbk,
}

impl TextEncoding {
    fn codec(self) -> &'static Encoding {
        match self {
            TextEncoding::Utf8 => UTF_8,
            TextEncoding::ShiftJis => SHIFT_JIS,
            TextEncoding::Gbk => GBK,
        }
    }
}

/// Decodes `bytes`, using `hint` when given, else guessing.
pub fn decode(bytes: &[u8], hint: Option<TextEncoding>) -> (String, TextEncoding) {
    // A byte-order mark settles it.
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (
            String::from_utf8_lossy(rest).into_owned(),
            TextEncoding::Utf8,
        );
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let (enc, _) = Encoding::for_bom(bytes).unwrap_or((encoding_rs::UTF_16LE, 2));
        let (text, _, _) = enc.decode(bytes);
        return (text.into_owned(), TextEncoding::Utf8);
    }
    if let Some(enc) = hint {
        let (text, _) = enc.codec().decode_without_bom_handling(bytes);
        return (text.into_owned(), enc);
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.to_string(), TextEncoding::Utf8);
    }
    let attempt = |enc: TextEncoding| {
        let (text, had_errors) = enc.codec().decode_without_bom_handling(bytes);
        (text.into_owned(), had_errors)
    };
    let (sjis, sjis_bad) = attempt(TextEncoding::ShiftJis);
    let (gbk, gbk_bad) = attempt(TextEncoding::Gbk);
    let kana_share = |s: &str| {
        let wide = s.chars().filter(|c| !c.is_ascii()).count().max(1);
        let kana = s
            .chars()
            .filter(|c| matches!(*c as u32, 0x3040..=0x30FF))
            .count();
        kana as f32 / wide as f32
    };
    let choice = match (sjis_bad, gbk_bad) {
        (false, true) => TextEncoding::ShiftJis,
        (true, false) => TextEncoding::Gbk,
        _ if kana_share(&sjis) >= 0.2 => TextEncoding::ShiftJis,
        (false, false) => TextEncoding::Gbk,
        (true, true) => TextEncoding::ShiftJis,
    };
    match choice {
        TextEncoding::Gbk => (gbk, TextEncoding::Gbk),
        _ => (sjis, TextEncoding::ShiftJis),
    }
}

/// `character.yaml`'s `text_file_encoding` names.
pub fn encoding_from_name(name: &str) -> Option<TextEncoding> {
    let n: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    match n.as_str() {
        "utf8" => Some(TextEncoding::Utf8),
        "shiftjis" | "sjis" | "cp932" | "windows31j" | "ms932" => Some(TextEncoding::ShiftJis),
        "gbk" | "gb2312" | "gb18030" | "cp936" => Some(TextEncoding::Gbk),
        _ => None,
    }
}
