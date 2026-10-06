//! Chop it (plan §3.8, P4): slice points, and the three ways slices land on
//! a keyboard.
//!
//! Pure. What comes out is a description — zones with sample offsets, keys
//! and tuning, and the notes that replay the source — which the app turns
//! into a sampler `Patch` and a note clip (`Session::add_sampler_slices`).
//! Offsets are **frames of the buffer handed in**, which is what a sampler
//! layer's `start_offset`/`end_offset` count when that buffer is the file it
//! plays.

use crate::onsets::OnsetSettings;

/// One slice: `start..end` in frames.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Slice {
    pub start: usize,
    pub end: usize,
    /// Its pitch, as fractional MIDI, when it has one (By pitch needs it):
    /// from the notes it was cut at, or [`slice_pitch`].
    pub pitch: Option<f32>,
}

impl Slice {
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The slices between `markers` over a buffer `len` long: n markers inside
/// it make n + 1 slices. Markers are sorted and deduplicated first; ones at
/// 0, at `len` or past it cut nothing.
pub fn slices_at(markers: &[usize], len: usize) -> Vec<Slice> {
    let mut cuts: Vec<usize> = markers
        .iter()
        .copied()
        .filter(|m| *m > 0 && *m < len)
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut edges = Vec::with_capacity(cuts.len() + 2);
    edges.push(0);
    edges.extend(cuts);
    edges.push(len);
    edges
        .windows(2)
        .map(|w| Slice {
            start: w[0],
            end: w[1],
            pitch: None,
        })
        .collect()
}

/// `n` slices of equal length (the last takes the remainder).
pub fn slices_equal(len: usize, n: usize) -> Vec<Slice> {
    let n = n.max(1);
    let markers: Vec<usize> = (1..n).map(|i| i * len / n).collect();
    slices_at(&markers, len)
}

/// A slice every `period` frames from `first` (a beat, a bar): the grid's
/// lines are the markers. `period` is fractional so a beat at 120 BPM and
/// 44.1 kHz does not drift.
pub fn slices_on_grid(len: usize, first: usize, period: f64) -> Vec<Slice> {
    if period <= 0.0 {
        return slices_at(&[first], len);
    }
    let mut markers = Vec::new();
    let mut k = 0u64;
    loop {
        let at = (first as f64 + k as f64 * period).round() as usize;
        if at >= len {
            break;
        }
        markers.push(at);
        k += 1;
    }
    slices_at(&markers, len)
}

/// Slices at the onsets [`crate::onsets::detect_onsets`] finds.
pub fn slices_at_transients(
    samples: &[f32],
    sample_rate: u32,
    settings: &OnsetSettings,
) -> Vec<Slice> {
    let markers: Vec<usize> = crate::onsets::detect_onsets(samples, sample_rate, settings)
        .into_iter()
        .map(|onset| onset.sample)
        .collect();
    slices_at(&markers, samples.len())
}

/// A detected note, for [`slices_from_notes`]: frames, and its pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteSpan {
    pub start: usize,
    pub end: usize,
    pub midi: f32,
}

/// One slice per note, cut at the note's start and running to the next
/// note's start (or its own end, for the last), carrying the note's pitch.
pub fn slices_from_notes(notes: &[NoteSpan], len: usize) -> Vec<Slice> {
    let mut notes: Vec<NoteSpan> = notes
        .iter()
        .copied()
        .filter(|n| n.start < len && n.end > n.start)
        .collect();
    notes.sort_by_key(|n| n.start);
    (0..notes.len())
        .filter_map(|i| {
            let note = notes[i];
            let end = notes
                .get(i + 1)
                .map_or(note.end.min(len), |next| next.start);
            (end > note.start).then_some(Slice {
                start: note.start,
                end,
                pitch: Some(note.midi),
            })
        })
        .collect()
}

/// A slice's pitch as fractional MIDI (pYIN, the median voiced frame);
/// `None` when it has none.
pub fn slice_pitch(samples: &[f32], sample_rate: u32) -> Option<f32> {
    let track = crate::mono::pyin(samples, sample_rate, &crate::mono::PyinParams::default());
    let mut cents: Vec<f32> = (0..track.len())
        .filter(|i| track.voicing[*i] > 0.5)
        .filter_map(|i| track.cents(i))
        .collect();
    if cents.is_empty() {
        return None;
    }
    cents.sort_by(f32::total_cmp);
    Some(cents[cents.len() / 2] / 100.0)
}

/// How slices land on the keyboard (plan §3.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SliceLayout {
    /// Slice i on key [`CHOP_FIRST_KEY`] + i, at its own pitch (Slicex).
    #[default]
    Chop,
    /// Each slice rooted at its own pitch, the keyboard split halfway
    /// between roots, tuned to play in tune: a melodic multisample.
    ByPitch,
    /// Kick, snare, hat and the rest on their General MIDI keys.
    DrumMap,
}

/// Where Chop starts: C3 (MIDI 48).
pub const CHOP_FIRST_KEY: u8 = 48;

/// What a drum slice sounds like, for [`SliceLayout::DrumMap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DrumClass {
    Kick,
    Snare,
    Hat,
    Other,
}

impl DrumClass {
    /// The General MIDI keys this class fills, in the order they are taken.
    pub fn keys(self) -> &'static [u8] {
        match self {
            Self::Kick => &[36, 35],
            Self::Snare => &[38, 40, 37],
            Self::Hat => &[42, 44, 46],
            Self::Other => &[39, 41, 43, 45, 47, 48, 50, 49, 51, 57, 52, 53, 54, 55, 56],
        }
    }
}

/// A drum slice's class: what **arrives** at its start — the spectrum of
/// its first 43 ms less that of the 43 ms before it, so the tail of the hit
/// before (a kick's boom under the next hat) is not counted as this one —
/// read by the share of that under 150 Hz, from 150 Hz to 1 kHz, and its
/// spectral centroid. `samples` is the whole buffer the slice is in.
pub fn classify_drum(samples: &[f32], slice: &Slice, sample_rate: u32) -> DrumClass {
    let (low, mid, centroid) = drum_features(samples, slice.start, sample_rate);
    if low > 0.4 {
        DrumClass::Kick
    } else if low + mid > 0.12 && centroid > 1_500.0 {
        DrumClass::Snare
    } else if centroid > 5_000.0 {
        DrumClass::Hat
    } else {
        DrumClass::Other
    }
}

/// One sampler zone: which slice, where it is, the keys that play it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SliceZone {
    /// Index into the slices handed in.
    pub slice: usize,
    /// Frames of the buffer: a layer's `start_offset` / `end_offset`.
    pub start: usize,
    pub end: usize,
    pub key_range: (u8, u8),
    pub root_key: u8,
    /// Cents to add so the slice plays in tune at its root (By pitch); 0
    /// otherwise.
    pub fine_tune_cents: f32,
    /// What the window calls it: "Slice 3", "Kick", "C4".
    pub name: String,
}

/// One note of the clip that replays the slices in order (Slicex's trick).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReplayNote {
    pub key: u8,
    /// Frames from the start of the buffer.
    pub start: usize,
    pub length: usize,
    pub velocity: u8,
}

/// The sampler a set of slices becomes.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SlicerPatch {
    pub zones: Vec<SliceZone>,
    /// Every slice that got a key, in time order, as notes.
    pub replay: Vec<ReplayNote>,
    /// Slices that got no key: past the top of the keyboard (Chop), no
    /// pitch or a root another longer slice already has (By pitch), or no
    /// General MIDI key left (Drum map). Their replay note plays the zone
    /// that does sit on their key, when there is one.
    pub unmapped: Vec<usize>,
}

/// Lays `slices` of `samples` out as `layout` says.
pub fn layout_slices(
    samples: &[f32],
    sample_rate: u32,
    slices: &[Slice],
    layout: SliceLayout,
) -> SlicerPatch {
    let mut patch = SlicerPatch::default();
    let zone = |slice: usize, key: u8, name: String| SliceZone {
        slice,
        start: slices[slice].start,
        end: slices[slice].end,
        key_range: (key, key),
        root_key: key,
        fine_tune_cents: 0.0,
        name,
    };
    // The key each slice's replay note plays, when it has one.
    let mut keys: Vec<Option<u8>> = vec![None; slices.len()];
    match layout {
        SliceLayout::Chop => {
            for (i, key) in keys.iter_mut().enumerate() {
                match u8::try_from(usize::from(CHOP_FIRST_KEY) + i) {
                    Ok(k) if k <= 127 => {
                        *key = Some(k);
                        patch.zones.push(zone(i, k, format!("Slice {}", i + 1)));
                    }
                    _ => patch.unmapped.push(i),
                }
            }
        }
        SliceLayout::ByPitch => {
            // Each pitched slice's root; the longest slice wins a root two
            // share.
            let mut by_root: Vec<(u8, usize, f32)> = Vec::new();
            for (i, slice) in slices.iter().enumerate() {
                let pitch = slice.pitch.or_else(|| {
                    samples
                        .get(slice.start..slice.end.min(samples.len()))
                        .and_then(|x| slice_pitch(x, sample_rate))
                });
                let Some(pitch) = pitch.filter(|p| (0.0..=127.0).contains(p)) else {
                    patch.unmapped.push(i);
                    continue;
                };
                let root = pitch.round() as u8;
                keys[i] = Some(root);
                match by_root.iter_mut().find(|(r, _, _)| *r == root) {
                    Some(held) if slices[held.1].len() >= slice.len() => patch.unmapped.push(i),
                    Some(held) => {
                        patch.unmapped.push(held.1);
                        *held = (root, i, pitch);
                    }
                    None => by_root.push((root, i, pitch)),
                }
            }
            by_root.sort_by_key(|(root, _, _)| *root);
            let roots: Vec<u8> = by_root.iter().map(|(root, _, _)| *root).collect();
            for ((root, i, pitch), range) in by_root.iter().zip(key_ranges(&roots)) {
                patch.zones.push(SliceZone {
                    key_range: range,
                    fine_tune_cents: -(pitch - f32::from(*root)) * 100.0,
                    ..zone(*i, *root, note_name(*root))
                });
            }
            patch.unmapped.sort_unstable();
        }
        SliceLayout::DrumMap => {
            let mut taken: Vec<u8> = Vec::new();
            let mut counts = [0usize; 4];
            for (i, slice) in slices.iter().enumerate() {
                let class = classify_drum(samples, slice, sample_rate);
                let key = class
                    .keys()
                    .iter()
                    .chain(DrumClass::Other.keys())
                    .copied()
                    .find(|k| !taken.contains(k));
                let Some(key) = key else {
                    patch.unmapped.push(i);
                    continue;
                };
                taken.push(key);
                keys[i] = Some(key);
                let index = class as usize;
                counts[index] += 1;
                let base = match class {
                    DrumClass::Kick => "Kick",
                    DrumClass::Snare => "Snare",
                    DrumClass::Hat => "Hat",
                    DrumClass::Other => "Hit",
                };
                let name = if counts[index] == 1 {
                    base.to_string()
                } else {
                    format!("{base} {}", counts[index])
                };
                patch.zones.push(zone(i, key, name));
            }
        }
    }
    patch.replay = slices
        .iter()
        .zip(&keys)
        .filter_map(|(slice, key)| {
            key.map(|key| ReplayNote {
                key,
                start: slice.start,
                length: slice.len(),
                velocity: 100,
            })
        })
        .collect();
    patch
}

/// The keys each of `roots` (sorted) serves: the whole keyboard split
/// halfway between neighbours, a key exactly halfway going to the lower —
/// the rule `fontelle_core::key_ranges` keeps for a multisample, restated
/// here because this crate does not depend on the sampler.
fn key_ranges(roots: &[u8]) -> Vec<(u8, u8)> {
    roots
        .iter()
        .enumerate()
        .map(|(i, &root)| {
            let low = if i == 0 {
                0
            } else {
                ((u16::from(roots[i - 1]) + u16::from(root)) / 2 + 1) as u8
            };
            let high = if i + 1 == roots.len() {
                127
            } else {
                ((u16::from(root) + u16::from(roots[i + 1])) / 2) as u8
            };
            (low, high)
        })
        .collect()
}

/// "C4" for 60.
fn note_name(key: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(key % 12)],
        i32::from(key / 12) - 1
    )
}

/// What arrives at `start` (see [`classify_drum`]): the share of the
/// energy under 150 Hz, the share from 150 Hz to 1 kHz, and the spectral
/// centroid in hertz.
fn drum_features(samples: &[f32], start: usize, sample_rate: u32) -> (f32, f32, f32) {
    const N: usize = 2048;
    if start >= samples.len() || sample_rate == 0 {
        return (0.0, 0.0, 0.0);
    }
    let mut planner = realfft::RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N);
    let mut time = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut power_at = |from: isize| -> Vec<f32> {
        for (i, t) in time.iter_mut().enumerate() {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos();
            let at = from + i as isize;
            let x = if at >= 0 {
                samples.get(at as usize).copied().unwrap_or(0.0)
            } else {
                0.0
            };
            *t = x * w;
        }
        let _ = fft.process(&mut time, &mut spectrum);
        spectrum.iter().map(|v| v.norm_sqr()).collect()
    };
    let after = power_at(start as isize);
    let before = power_at(start as isize - N as isize);
    let hz_per_bin = sample_rate as f32 / N as f32;
    let (mut low, mut mid, mut total, mut weighted) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (bin, (a, b)) in after.iter().zip(&before).enumerate().skip(1) {
        let hz = bin as f32 * hz_per_bin;
        let power = f64::from((a - b).max(0.0));
        total += power;
        weighted += power * f64::from(hz);
        if hz < 150.0 {
            low += power;
        } else if hz < 1_000.0 {
            mid += power;
        }
    }
    if total <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    (
        (low / total) as f32,
        (mid / total) as f32,
        (weighted / total) as f32,
    )
}
