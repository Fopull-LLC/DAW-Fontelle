//! Voicebank portraits: UTAU's `character.txt` names a BMP more often than
//! not (a 100×100 picture from the Windows XP days), DiffSinger banks a PNG
//! or a JPEG. Each is decoded to RGBA, made small enough to keep many of
//! them, and kept by path so a frame never decodes one twice.

use fontelle_ui::portrait::{MAX_PORTRAIT, Portraits, decode_bmp, decode_portrait, downscale};

/// A BMP as Paint writes it: BITMAPINFOHEADER, rows padded to four bytes.
fn bmp(width: i32, height: i32, bits: u16, palette: &[[u8; 4]], pixels: &[u8]) -> Vec<u8> {
    let palette_bytes = palette.len() * 4;
    let offset = 14 + 40 + palette_bytes;
    let mut out = Vec::new();
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((offset + pixels.len()) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&(palette.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    for p in palette {
        out.extend_from_slice(p);
    }
    out.extend_from_slice(pixels);
    out
}

#[test]
fn a_24_bit_bmp_is_read_bottom_up_with_its_rows_padded() {
    // 3×2, bottom row first, BGR, each row padded from 9 to 12 bytes.
    let pixels = [
        // bottom: blue, green, red
        255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, //
        // top: white, black, grey
        255, 255, 255, 0, 0, 0, 128, 128, 128, 0, 0, 0,
    ];
    let (w, h, rgba) = decode_bmp(&bmp(3, 2, 24, &[], &pixels)).unwrap();
    assert_eq!((w, h), (3, 2));
    assert_eq!(&rgba[0..4], &[255, 255, 255, 255], "top-left is white");
    assert_eq!(&rgba[8..12], &[128, 128, 128, 255]);
    assert_eq!(&rgba[12..16], &[0, 0, 255, 255], "bottom-left is blue");
    assert_eq!(&rgba[20..24], &[255, 0, 0, 255], "bottom-right is red");
}

#[test]
fn a_top_down_32_bit_bmp_and_an_8_bit_palette_one() {
    // Negative height: top row first.
    let pixels = [10, 20, 30, 0, 40, 50, 60, 0];
    let (w, h, rgba) = decode_bmp(&bmp(1, -2, 32, &[], &pixels)).unwrap();
    assert_eq!((w, h), (1, 2));
    // An alpha byte of 0 throughout means no alpha at all.
    assert_eq!(rgba, vec![30, 20, 10, 255, 60, 50, 40, 255]);

    let palette = [[0, 0, 255, 0], [0, 255, 0, 0]]; // BGRx: red, green
    let pixels = [1, 0, 0, 0, 0, 1, 0, 0]; // 2×2, rows padded to 4
    let (_, _, rgba) = decode_bmp(&bmp(2, 2, 8, &palette, &pixels)).unwrap();
    // Top row (stored second): red, green.
    assert_eq!(&rgba[0..8], &[255, 0, 0, 255, 0, 255, 0, 255]);
    assert_eq!(&rgba[8..16], &[0, 255, 0, 255, 255, 0, 0, 255]);
}

#[test]
fn what_is_not_a_picture_says_so() {
    assert!(decode_bmp(b"BM").is_err());
    assert!(decode_portrait(b"hello").is_err());
    // A header promising more pixels than the file has.
    let mut cut = bmp(64, 64, 24, &[], &[0; 64 * 64 * 3]);
    cut.truncate(200);
    assert!(decode_bmp(&cut).is_err());
}

#[test]
fn a_big_portrait_is_made_small() {
    let rgba = vec![200u8; 1000 * 500 * 4];
    let (w, h, small) = downscale(1000, 500, &rgba, MAX_PORTRAIT);
    assert_eq!((w, h), (MAX_PORTRAIT, MAX_PORTRAIT / 2));
    assert_eq!(small.len(), (w * h * 4) as usize);
    assert!(small.iter().all(|&b| b == 200), "a flat picture stays flat");
    let (w, h, _) = downscale(100, 100, &vec![0; 100 * 100 * 4], MAX_PORTRAIT);
    assert_eq!((w, h), (100, 100), "a small one is left alone");
}

#[test]
fn portraits_are_decoded_once_and_a_missing_one_remembered() {
    let dir = std::env::temp_dir().join(format!("fontelle-portraits-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.bmp");
    std::fs::write(&path, bmp(1, 1, 24, &[], &[0, 0, 255, 0])).unwrap();
    let mut cache = Portraits::default();
    let key = path.to_string_lossy().into_owned();
    let image = cache.get(&key).expect("decoded");
    assert_eq!((image.width, image.height), (1, 1));
    // Changed on disk after: the cache keeps what it decoded.
    std::fs::write(&path, b"garbage").unwrap();
    assert!(cache.get(&key).is_some());
    assert!(cache.get("/nowhere/at/all.png").is_none());
    assert_eq!(cache.len(), 2, "the miss is remembered too");
}

#[test]
fn a_webp_portrait_is_read_whatever_its_name_says() {
    // As a DiffSinger bank shipped one: WebP bytes in a file named .jpg.
    let (w, h) = (4u32, 3u32);
    let rgba: Vec<u8> = (0..w * h)
        .flat_map(|i| [i as u8 * 20, 100, 200, 255])
        .collect();
    let mut bytes = Vec::new();
    image_webp::WebPEncoder::new(&mut bytes)
        .encode(&rgba, w, h, image_webp::ColorType::Rgba8)
        .unwrap();
    let image = decode_portrait(&bytes).unwrap();
    assert_eq!((image.width, image.height), (4, 3));
    assert_eq!(&image.data.data()[..4], &rgba[..4]);
}
