//! Encodes a guide clip (`tools/guide-clips/`): `NNNN.png` frames and
//! `delays.txt` (milliseconds per frame) into one animated PNG.
//!
//! Each frame after the first stores only what changed since the last — as up
//! to three rectangles when the changes are far apart (the timer and the
//! playhead), all but the last with no delay of their own, which
//! `fontelle_ui::guide_media::Player` lays together. A frame that changed
//! nothing lengthens the one before. That is what keeps a fifteen-second clip
//! of a 540×304 window under 600 kB.
//!
//! `cargo run --release --example guide_clip_encode -p fontelle-ui -- <dir> <out.png>`

use std::fs;
use std::path::Path;

fn read(path: &Path) -> (u32, u32, Vec<u8>) {
    let mut d = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
    d.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut r = d.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

/// x, y, width, height.
type R = (u32, u32, u32, u32);

fn changed(a: &[u8], b: &[u8], w: u32, h: u32) -> Vec<R> {
    let diff = |x: u32, y: u32| {
        let k = ((y * w + x) * 4) as usize;
        a[k..k + 4] != b[k..k + 4]
    };
    // Runs of changed columns, split where a gap is wide.
    let cols: Vec<bool> = (0..w).map(|x| (0..h).any(|y| diff(x, y))).collect();
    let mut runs: Vec<(u32, u32)> = Vec::new();
    let mut x = 0;
    while x < w {
        if cols[x as usize] {
            let s = x;
            let mut e = x;
            let mut gap = 0;
            while x < w && gap < 24 {
                if cols[x as usize] {
                    e = x;
                    gap = 0
                } else {
                    gap += 1
                }
                x += 1;
            }
            runs.push((s, e + 1));
        } else {
            x += 1;
        }
    }
    let mut rects: Vec<R> = runs
        .iter()
        .map(|&(x0, x1)| {
            let rows: Vec<u32> = (0..h).filter(|&y| (x0..x1).any(|x| diff(x, y))).collect();
            (x0, rows[0], x1 - x0, rows[rows.len() - 1] + 1 - rows[0])
        })
        .collect();
    // At most three: merge the pair whose union grows least.
    while rects.len() > 3 {
        let mut best = (0, 1, u64::MAX);
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let u = union(rects[i], rects[j]);
                let grow = area(u) - area(rects[i]) - area(rects[j]);
                if grow < best.2 {
                    best = (i, j, grow)
                }
            }
        }
        let u = union(rects[best.0], rects[best.1]);
        rects.remove(best.1);
        rects[best.0] = u;
    }
    rects
}

fn area(r: R) -> u64 {
    r.2 as u64 * r.3 as u64
}
fn union(a: R, b: R) -> R {
    let x0 = a.0.min(b.0);
    let y0 = a.1.min(b.1);
    let x1 = (a.0 + a.2).max(b.0 + b.2);
    let y1 = (a.1 + a.3).max(b.1 + b.3);
    (x0, y0, x1 - x0, y1 - y0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = Path::new(&args[1]);
    let delays: Vec<u32> = fs::read_to_string(dir.join("delays.txt"))
        .unwrap()
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    let frames: Vec<_> = (0..delays.len())
        .map(|i| read(&dir.join(format!("{i:04}.png"))))
        .collect();
    let (w, h, _) = frames[0];
    // (rect, delay, source frame)
    let mut kept: Vec<(R, u32, usize)> = vec![((0, 0, w, h), delays[0], 0)];
    for i in 1..frames.len() {
        let rects = changed(&frames[i - 1].2, &frames[i].2, w, h);
        if rects.is_empty() {
            kept.last_mut().unwrap().1 += delays[i];
            continue;
        }
        let n = rects.len();
        for (k, r) in rects.into_iter().enumerate() {
            kept.push((r, if k + 1 == n { delays[i] } else { 0 }, i));
        }
    }
    let out = fs::File::create(&args[2]).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(out), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::High);
    enc.set_animated(kept.len() as u32, 0).unwrap();
    let mut wr = enc.write_header().unwrap();
    for (n, &((x, y, fw, fh), delay, src)) in kept.iter().enumerate() {
        if n > 0 {
            // Back to the corner first: a size checked against the last
            // frame's position can run off the picture.
            wr.set_frame_position(0, 0).unwrap();
            wr.set_frame_dimension(fw, fh).unwrap();
            wr.set_frame_position(x, y).unwrap();
        }
        wr.set_frame_delay(delay.min(65535) as u16, 1000).unwrap();
        wr.set_blend_op(png::BlendOp::Source).unwrap();
        wr.set_dispose_op(png::DisposeOp::None).unwrap();
        let px = &frames[src].2;
        let mut crop = Vec::with_capacity((fw * fh * 4) as usize);
        for row in y..y + fh {
            let k = ((row * w + x) * 4) as usize;
            crop.extend_from_slice(&px[k..k + (fw * 4) as usize]);
        }
        wr.write_image_data(&crop).unwrap();
    }
    wr.finish().unwrap();
    let total: u32 = kept.iter().map(|k| k.1).sum();
    println!(
        "{} png frames from {} recorded, {} ms",
        kept.len(),
        frames.len(),
        total
    );
}
