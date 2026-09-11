//! The mark: the logo the start menu draws and the icon the window wears.
//!
//! Both come from `assets/branding/`, decoded from the PNGs compiled into
//! the binary, so a build has its own face without a file beside it. The
//! logo is a white mark on nothing, and is *tinted* rather than drawn as
//! shipped: on the light theme white on off-white is no mark at all.

use fontelle_ui::branding::{logo, window_icon};
use fontelle_ui::theme::Color;

fn pixels(data: &[u8]) -> impl Iterator<Item = [u8; 4]> + '_ {
    data.as_chunks::<4>().0.iter().copied()
}

#[test]
fn the_logo_is_a_square_with_the_mark_in_it_in_the_ink_it_was_asked_for() {
    let tint = Color::rgb(0x31, 0x62, 0x72);
    let image = logo(tint);
    assert_eq!(image.width, image.height);
    assert!(
        image.width >= 256,
        "big enough for a 2x display: {}",
        image.width
    );
    let data = image.data.data();
    assert_eq!(data.len(), (image.width * image.height * 4) as usize);
    let marked = pixels(data).filter(|px| px[3] > 0).count();
    let total = (image.width * image.height) as usize;
    // A mark, not a blank and not a block: the monogram covers a fraction of
    // its square.
    assert!(
        marked > total / 20 && marked < total * 3 / 5,
        "{marked} of {total}"
    );
    // Every pixel that is fully there is the ink, not white.
    for px in pixels(data).filter(|px| px[3] == 255) {
        assert_eq!([px[0], px[1], px[2]], [0x31, 0x62, 0x72]);
    }
}

#[test]
fn the_tint_changes_the_ink_and_nothing_else() {
    let a = logo(Color::rgb(0xff, 0xff, 0xff));
    let b = logo(Color::rgb(0x00, 0x00, 0x00));
    let alphas = |img: &fontelle_ui::vello::peniko::ImageData| -> Vec<u8> {
        pixels(img.data.data()).map(|px| px[3]).collect()
    };
    assert_eq!(
        alphas(&a),
        alphas(&b),
        "the shape is the file's; only the colour is ours"
    );
}

#[test]
fn the_window_icon_is_square_and_has_a_ground_under_the_mark() {
    let (width, height, rgba) = window_icon();
    assert_eq!(width, height);
    assert!(width >= 64);
    assert_eq!(rgba.len(), (width * height * 4) as usize);
    // The centre of an app icon is opaque — a mark on a plate, not a mark
    // on nothing, so it reads on a light taskbar as well as a dark one.
    let centre = ((height / 2) * width + width / 2) as usize * 4;
    assert_eq!(rgba[centre + 3], 255);
    // And the corners are rounded off: the very corner is empty.
    assert_eq!(rgba[3], 0);
}
