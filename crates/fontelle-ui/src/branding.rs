//! The mark: the logo the start menu draws and the icon the window wears.
//!
//! Both are PNGs from `assets/branding/`, compiled in with `include_bytes!`
//! so a build carries its own face and a binary copied somewhere on its own
//! still has one. They are decoded on first use and cached — a 512-pixel
//! square is a megabyte of pixels, and the logo is asked for once a frame
//! while the menu is up.
//!
//! # The logo is tinted, not drawn as shipped
//!
//! The file is a white mark on nothing, which is the right thing to keep in
//! the repository (it composes onto anything) and the wrong thing to draw on
//! the light theme, where white on off-white is no mark at all. So the
//! decode keeps the file's *alpha* and paints the theme's ink into the
//! colour channels — the same reason the icon set is geometry rather than
//! bitmaps (`icon.rs`): the picture takes the theme's colour.
//!
//! The window icon is different: it goes to the desktop, which draws it on
//! a taskbar of its own colour, so it is the mark on a plate of the dark
//! theme's panel colour and is used as shipped.

use std::sync::{Arc, Mutex, OnceLock};

use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

use crate::theme::Color;

const LOGO_PNG: &[u8] = include_bytes!("../../../assets/branding/fontelle-logo-512.png");
const ICON_PNG: &[u8] = include_bytes!("../../../assets/branding/fontelle-icon-256.png");

/// A decoded PNG: straight (un-premultiplied) RGBA, row-major.
struct Decoded {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

fn decode(png: &[u8]) -> Decoded {
    try_decode(png).expect("the compiled-in PNG decodes")
}

/// Any PNG, as straight RGBA — for a texture read off the disk at run time
/// (`skin.rs`), which may be anything at all.
pub fn decode_png(png: &[u8]) -> Result<ImageData, String> {
    let Decoded {
        width,
        height,
        rgba,
    } = try_decode(png)?;
    if width == 0 || height == 0 {
        return Err("an empty image".to_string());
    }
    Ok(ImageData {
        data: Blob::new(Arc::new(rgba)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    })
}

fn try_decode(png: &[u8]) -> Result<Decoded, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    // Sixteen-bit and palette files come out as eight-bit colour; how many
    // channels that is still depends on the file, and is dealt with below.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| "an unbounded image".to_string())?
    ];
    let info = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    pixels.truncate(info.buffer_size());
    // Out as RGBA whatever the file held. The logo, being white on nothing,
    // is stored as grey-plus-alpha by any encoder that notices — which is
    // exactly the file an `include_bytes!` should not be fussy about.
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => pixels,
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[g, a]| [g, g, g, a])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => unreachable!("normalize_to_color8 expands a palette"),
    };
    Ok(Decoded {
        width: info.width,
        height: info.height,
        rgba,
    })
}

/// The logo, in `tint`, ready for `Scene::draw_image`.
///
/// Cached per tint: a theme asks for one colour, and the menu asks every
/// frame.
pub fn logo(tint: Color) -> ImageData {
    static CACHE: OnceLock<Mutex<Vec<(Color, ImageData)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    let mut cache = cache.lock().unwrap();
    if let Some((_, image)) = cache.iter().find(|(c, _)| *c == tint) {
        return image.clone();
    }
    let Decoded {
        width,
        height,
        mut rgba,
    } = decode(LOGO_PNG);
    for px in rgba.as_chunks_mut::<4>().0 {
        px[..3].copy_from_slice(&tint.0[..3]);
    }
    let image = ImageData {
        data: Blob::new(Arc::new(rgba)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    };
    cache.push((tint, image.clone()));
    image
}

/// The window icon as it is shipped, for the desktop entry a Linux session
/// needs beside the window (`fontelle_app::desktop::register_desktop_entry`).
pub fn window_icon_png() -> &'static [u8] {
    ICON_PNG
}

/// The window icon as `(width, height, rgba)` — what `winit::window::Icon`
/// is built from.
pub fn window_icon() -> (u32, u32, Vec<u8>) {
    let Decoded {
        width,
        height,
        rgba,
    } = decode(ICON_PNG);
    (width, height, rgba)
}
