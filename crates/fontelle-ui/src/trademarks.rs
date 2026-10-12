//! Other people's marks, shown as their owners require.
//!
//! One today: Steinberg's **ASIO Compatible Logo**. Fontelle plays through
//! ASIO by way of `fontelle-asio`, which is built with the ASIO SDK, and
//! Steinberg's *ASIO Usage Guidelines* (in the SDK) ask that wherever a
//! product so built says "ASIO" the logo appears beside it — in every dialog
//! that switches ASIO on (§1(f), §2), beside a product name containing it
//! (§6), in the credits and About (§1(e)), in the documentation (§1(c)) —
//! with "ASIO®" at first use and the attribution sentence (§15). Logos
//! "must be used in unaltered form" (the SDK's README).
//!
//! So the file is Steinberg's own, byte for byte
//! (`assets/trademarks/asio/`, its NOTICE says whose it is and that no
//! licence of Fontelle's covers it), decoded **without** the tint
//! [`crate::branding`] gives Fontelle's own mark, and only ever scaled —
//! never recoloured, cropped or rearranged. The form used is §14's with
//! the attribution beneath the logo, so the sentence goes wherever the logo
//! does. Whether to show it at all is the host's call: only where ASIO is
//! offered (Windows, with `fontelle-asio` beside Fontelle).

use std::sync::OnceLock;

use vello::peniko::ImageData;

use crate::layout::Rect;

/// Steinberg's ASIO Compatible Logo with the attribution beneath it, as the
/// ASIO SDK 2.3.4 ships it (`Steinberg ASIO Logo Artwork/
/// ASIO-compatible-logo-Steinberg-TM-BW.png`).
///
/// Compiled into Windows builds, where ASIO is offered, and debug builds,
/// where the tests draw it; a released Linux or macOS Fontelle offers no
/// ASIO and carries no mark of Steinberg's.
#[cfg(any(windows, debug_assertions))]
pub const ASIO_LOGO_PNG: &[u8] =
    include_bytes!("../../../assets/trademarks/asio/ASIO-compatible-logo-Steinberg-TM-BW.png");
#[cfg(not(any(windows, debug_assertions)))]
pub const ASIO_LOGO_PNG: &[u8] = &[];

/// Its size in pixels, for laying it out at its own proportions.
pub const ASIO_LOGO_SIZE: (u32, u32) = (474, 443);

/// The attribution Steinberg's guidelines ask for in credits and
/// documentation (§15).
pub const ASIO_ATTRIBUTION: &str =
    "ASIO is a registered trademark of Steinberg Media Technologies GmbH.";

/// The start menu's credit beside the logo.
pub const ASIO_CREDIT: &str = "ASIO\u{ae} output by fontelle-asio, a separate GPLv3 program. \
                               ASIO is a registered trademark of Steinberg Media Technologies GmbH.";

/// The logo, decoded once, exactly as the file has it — `None` in a build
/// that does not carry it.
pub fn asio_logo() -> Option<&'static ImageData> {
    static LOGO: OnceLock<Option<ImageData>> = OnceLock::new();
    LOGO.get_or_init(|| crate::branding::decode_png(ASIO_LOGO_PNG).ok())
        .as_ref()
}

/// `height` tall at the logo's own proportions: how wide.
pub fn asio_logo_width(height: f32) -> f32 {
    let (w, h) = ASIO_LOGO_SIZE;
    (height * w as f32 / h as f32).round()
}

/// Where the logo goes in a settings row that offers ASIO: as tall as the
/// row allows less a margin, at its own proportions, just left of the
/// control.
pub fn settings_badge(row: Rect, control: Rect) -> Rect {
    let height = (row.height - 6.0).max(0.0).round();
    let width = asio_logo_width(height);
    Rect::new(
        (control.x - BADGE_GAP - width).round(),
        (row.y + (row.height - height) / 2.0).round(),
        width,
        height,
    )
}

/// Between the logo and the control it stands beside.
const BADGE_GAP: f32 = 16.0;

/// Draws the logo into `rect` at its own proportions (`rect` is laid out
/// that way): scaled, nothing else.
pub fn draw_asio_logo(scene: &mut vello::Scene, rect: Rect) {
    let Some(logo) = asio_logo() else {
        return;
    };
    if rect.is_empty() || logo.height == 0 {
        return;
    }
    let scale = (rect.height / logo.height as f32) as f64;
    scene.draw_image(
        logo,
        vello::kurbo::Affine::translate((rect.x as f64, rect.y as f64))
            * vello::kurbo::Affine::scale(scale),
    );
}
