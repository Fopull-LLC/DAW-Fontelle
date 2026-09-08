//! The one encoding a plugin's opaque state can travel through JSON in.

use fontelle_types::{decode_base64, encode_base64};

#[test]
fn nothing_encodes_to_nothing() {
    assert_eq!(encode_base64(&[]), "");
    assert_eq!(decode_base64(""), Some(Vec::new()));
}

#[test]
fn the_worked_examples_from_the_standard_match() {
    assert_eq!(encode_base64(b"f"), "Zg==");
    assert_eq!(encode_base64(b"fo"), "Zm8=");
    assert_eq!(encode_base64(b"foo"), "Zm9v");
    assert_eq!(encode_base64(b"foob"), "Zm9vYg==");
    assert_eq!(encode_base64(b"fooba"), "Zm9vYmE=");
    assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
}

#[test]
fn every_byte_survives_the_round_trip() {
    let bytes: Vec<u8> = (0..=255u8).collect();
    for length in 0..bytes.len() {
        let slice = &bytes[..length];
        let encoded = encode_base64(slice);
        assert_eq!(decode_base64(&encoded).as_deref(), Some(slice), "{length}");
    }
}

#[test]
fn what_is_written_is_only_the_standard_alphabet() {
    let encoded = encode_base64(&(0..=255u8).collect::<Vec<_>>());
    assert!(
        encoded
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='),
        "{encoded}"
    );
}

#[test]
fn damaged_text_is_refused_rather_than_guessed_at() {
    assert_eq!(decode_base64("Zg="), None);
    assert_eq!(decode_base64("Zg"), None);
    assert_eq!(decode_base64("Z g=="), None);
    assert_eq!(decode_base64("Zm9v!!!!"), None);
    assert_eq!(decode_base64("=Zm9v"), None);
}
