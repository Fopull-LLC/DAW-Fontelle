//! The guide's animations (`docs/ux-routing-and-learning-plan.md` step 7).
//!
//! > *"the windowed tutorials [should] show related gifs of the mentioned
//! > actions being performed so the animations will make it more interactive
//! > and easy to follow"* — Ty
//!
//! Each clip is recorded from the real binary on the tour's own song and
//! stored as an animated PNG under `assets/guide/`: the same thing as a GIF to
//! whoever watches it, without GIF's 256 colours, and decoded by the `png`
//! crate the tree already has. A frame holds only the part of the picture that
//! changed, so a clip is small, and playing one means laying each frame over
//! the picture so far — [`Player`] does that, one frame at a time, rather than
//! holding every frame whole (a clip is ~150 frames of 650 kB each).

use std::sync::Arc;

use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

use crate::canvas::GuideMedia;

/// Every clip's size in pixels. The pages lay them out at this shape.
pub const MEDIA_WIDTH: u32 = 540;
pub const MEDIA_HEIGHT: u32 = 304;

/// The file behind `media`.
pub fn bytes(media: GuideMedia) -> &'static [u8] {
    match media {
        GuideMedia::Transport => include_bytes!("../../../assets/guide/transport.png"),
        GuideMedia::Rack => include_bytes!("../../../assets/guide/rack.png"),
        GuideMedia::Browser => include_bytes!("../../../assets/guide/browser.png"),
        GuideMedia::Clips => include_bytes!("../../../assets/guide/clips.png"),
        GuideMedia::Lanes => include_bytes!("../../../assets/guide/lanes.png"),
        GuideMedia::Roll => include_bytes!("../../../assets/guide/roll.png"),
        GuideMedia::Slide => include_bytes!("../../../assets/guide/slide.png"),
        GuideMedia::SlideEdit => include_bytes!("../../../assets/guide/slide_edit.png"),
        GuideMedia::Mixer => include_bytes!("../../../assets/guide/mixer.png"),
        GuideMedia::Export => include_bytes!("../../../assets/guide/export.png"),
        GuideMedia::Settings => include_bytes!("../../../assets/guide/settings.png"),
    }
}

/// What to do with a frame's area before the next frame is laid down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispose {
    Keep,
    Clear,
    Restore,
}

/// One frame as stored: a rectangle of pixels and how to lay it down.
#[derive(Debug, Clone)]
pub struct Frame {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// RGBA, `width × height`.
    pub rgba: Vec<u8>,
    pub delay_ms: u32,
    /// Blended over what is there, rather than replacing it.
    pub over: bool,
    pub dispose: Dispose,
}

#[derive(Debug, Clone)]
pub struct Animation {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Frame>,
}

impl Animation {
    /// One time round, in milliseconds.
    pub fn total_ms(&self) -> u64 {
        self.frames.iter().map(|f| f.delay_ms as u64).sum()
    }

    /// The frame showing `ms` after the clip began, looping.
    pub fn frame_at(&self, ms: u64) -> usize {
        let total = self.total_ms();
        if total == 0 {
            return 0;
        }
        let mut t = ms % total;
        for (i, frame) in self.frames.iter().enumerate() {
            if t < frame.delay_ms as u64 {
                return i;
            }
            t -= frame.delay_ms as u64;
        }
        self.frames.len().saturating_sub(1)
    }
}

/// Decodes an animated PNG into its frames as stored. A still PNG is one
/// frame, which [`Player`] shows as it is.
pub fn decode_apng(file: &[u8]) -> Result<Animation, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(file));
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    let count = reader
        .info()
        .animation_control()
        .map_or(1, |control| control.num_frames as usize);
    // An image the animation does not include (no fcTL before IDAT) comes
    // first and is not shown.
    let skip_default =
        reader.info().animation_control().is_some() && reader.info().frame_control().is_none();
    let mut frames = Vec::with_capacity(count);
    let mut buffer = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| "an unbounded image".to_string())?
    ];
    let mut index = 0;
    while frames.len() < count {
        let out = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
        if skip_default && index == 0 {
            index += 1;
            continue;
        }
        index += 1;
        let control = reader.info().frame_control().copied();
        let rgba = to_rgba(&buffer[..out.buffer_size()], out.color_type)?;
        let frame = match control {
            Some(fc) => Frame {
                x: fc.x_offset,
                y: fc.y_offset,
                width: fc.width,
                height: fc.height,
                rgba,
                delay_ms: delay_ms(fc.delay_num, fc.delay_den),
                over: fc.blend_op == png::BlendOp::Over,
                dispose: match fc.dispose_op {
                    png::DisposeOp::None => Dispose::Keep,
                    png::DisposeOp::Background => Dispose::Clear,
                    png::DisposeOp::Previous => Dispose::Restore,
                },
            },
            None => Frame {
                x: 0,
                y: 0,
                width: out.width,
                height: out.height,
                rgba,
                delay_ms: 100,
                over: false,
                dispose: Dispose::Keep,
            },
        };
        if frame.x + frame.width > width
            || frame.y + frame.height > height
            || frame.rgba.len() != (frame.width * frame.height * 4) as usize
        {
            return Err("a frame runs off the picture".to_string());
        }
        frames.push(frame);
    }
    if frames.is_empty() {
        return Err("no frames".to_string());
    }
    Ok(Animation {
        width,
        height,
        frames,
    })
}

/// A frame's delay; a zero denominator means hundredths. A zero delay is
/// kept: the clips store one moment's far-apart changes (the timer and the
/// playhead) as several frames, all but the last taking no time, and the
/// player lays them together.
fn delay_ms(num: u16, den: u16) -> u32 {
    let den = if den == 0 { 100 } else { den as u32 };
    num as u32 * 1000 / den
}

fn to_rgba(pixels: &[u8], color: png::ColorType) -> Result<Vec<u8>, String> {
    Ok(match color {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("an unexpanded palette".to_string()),
    })
}

/// Plays an [`Animation`]: the picture at a moment, built by laying frames
/// over one another in order.
#[derive(Debug, Clone)]
pub struct Player {
    animation: Animation,
    canvas: Vec<u8>,
    /// What a `Restore` frame puts back.
    saved: Vec<u8>,
    shown: Option<usize>,
    image: Option<ImageData>,
}

impl Player {
    pub fn new(animation: Animation) -> Self {
        let size = (animation.width * animation.height * 4) as usize;
        Self {
            animation,
            canvas: vec![0; size],
            saved: Vec::new(),
            shown: None,
            image: None,
        }
    }

    pub fn animation(&self) -> &Animation {
        &self.animation
    }

    /// Moves the picture to `ms` after the clip began; `true` when it changed.
    pub fn advance_to(&mut self, ms: u64) -> bool {
        let target = self.animation.frame_at(ms);
        let from = match self.shown {
            Some(shown) if shown == target => return false,
            Some(shown) if shown < target => shown + 1,
            // The first picture, or round again: from a clean slate.
            _ => {
                self.canvas.fill(0);
                0
            }
        };
        for index in from..=target {
            if index > 0 {
                self.dispose(index - 1);
            }
            if self.animation.frames[index].dispose == Dispose::Restore {
                self.saved.clone_from(&self.canvas);
            }
            self.lay(index);
        }
        self.shown = Some(target);
        self.image = None;
        true
    }

    /// The picture as RGBA, `width × height`.
    pub fn rgba(&self) -> &[u8] {
        &self.canvas
    }

    /// The picture, for drawing. Made once per frame shown.
    pub fn image(&mut self) -> ImageData {
        if self.image.is_none() {
            self.image = Some(ImageData {
                data: Blob::new(Arc::new(self.canvas.clone())),
                format: ImageFormat::Rgba8,
                alpha_type: ImageAlphaType::Alpha,
                width: self.animation.width,
                height: self.animation.height,
            });
        }
        self.image.clone().expect("just made")
    }

    fn dispose(&mut self, index: usize) {
        let frame = &self.animation.frames[index];
        match frame.dispose {
            Dispose::Keep => {}
            Dispose::Clear => {
                let stride = self.animation.width as usize * 4;
                for row in 0..frame.height as usize {
                    let at = (frame.y as usize + row) * stride + frame.x as usize * 4;
                    self.canvas[at..at + frame.width as usize * 4].fill(0);
                }
            }
            Dispose::Restore => {
                if self.saved.len() == self.canvas.len() {
                    self.canvas.clone_from(&self.saved);
                }
            }
        }
    }

    fn lay(&mut self, index: usize) {
        let frame = &self.animation.frames[index];
        let stride = self.animation.width as usize * 4;
        let row_len = frame.width as usize * 4;
        for row in 0..frame.height as usize {
            let at = (frame.y as usize + row) * stride + frame.x as usize * 4;
            let src = &frame.rgba[row * row_len..(row + 1) * row_len];
            let dst = &mut self.canvas[at..at + row_len];
            if !frame.over {
                dst.copy_from_slice(src);
                continue;
            }
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0)
            {
                let sa = s[3] as u32;
                if sa == 255 {
                    d.copy_from_slice(s);
                } else if sa > 0 {
                    let da = d[3] as u32;
                    // Straight alpha: out = s·sa + d·da·(1 − sa), over out_a.
                    let oa = sa * 255 + da * (255 - sa);
                    for c in 0..3 {
                        let v = (s[c] as u32 * sa * 255 + d[c] as u32 * da * (255 - sa))
                            .checked_div(oa)
                            .unwrap_or(0);
                        d[c] = v.min(255) as u8;
                    }
                    d[3] = (oa / 255).min(255) as u8;
                }
            }
        }
    }
}
