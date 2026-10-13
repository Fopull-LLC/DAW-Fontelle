//! Synthetic fixtures: small voices made here, never recorded ones.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub const FS: u32 = 44_100;

/// A fresh, empty folder under the target directory.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("fontelle-utau")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Formants (centre, bandwidth) of a few vowels.
pub fn formants(vowel: char) -> [(f32, f32); 3] {
    match vowel {
        'i' => [(300.0, 80.0), (2300.0, 120.0), (3000.0, 200.0)],
        'u' => [(350.0, 80.0), (1250.0, 120.0), (2400.0, 200.0)],
        'e' => [(450.0, 80.0), (1900.0, 120.0), (2600.0, 200.0)],
        'o' => [(450.0, 80.0), (850.0, 100.0), (2500.0, 200.0)],
        'n' => [(250.0, 60.0), (1100.0, 200.0), (2500.0, 300.0)],
        _ => [(800.0, 90.0), (1200.0, 110.0), (2500.0, 200.0)],
    }
}

/// A sung vowel: harmonics of `f0` under a three-formant envelope, a
/// slight vibrato-free steadiness, amplitude ~0.3.
pub fn vowel(f0: f32, secs: f32, vowel: char) -> Vec<f32> {
    let n = (secs * FS as f32) as usize;
    let fm = formants(vowel);
    let env = |f: f32| -> f32 {
        let mut a = 0.0;
        for (c, b) in fm {
            a += 1.0 / (1.0 + ((f - c) / b).powi(2));
        }
        a * (300.0 / f.max(100.0)).sqrt()
    };
    let mut out = vec![0.0f32; n];
    let mut k = 1;
    while (k as f32) * f0 < 8000.0 {
        let f = k as f32 * f0;
        let a = env(f);
        let phase = (k * k) as f32 * 0.7;
        for (i, o) in out.iter_mut().enumerate() {
            let t = i as f32 / FS as f32;
            *o += a * (std::f32::consts::TAU * f * t + phase).sin();
        }
        k += 1;
    }
    let peak = out.iter().fold(0.0f32, |m, &v| m.max(v.abs())).max(1e-9);
    for o in &mut out {
        *o *= 0.3 / peak;
    }
    // 10 ms fades so the joins are clean.
    let fade = (0.01 * FS as f32) as usize;
    for i in 0..fade.min(n / 2) {
        let g = i as f32 / fade as f32;
        out[i] *= g;
        out[n - 1 - i] *= g;
    }
    out
}

/// A deterministic noise burst, brightened (a fricative-ish consonant).
pub fn noise(secs: f32, amp: f32, seed: u32) -> Vec<f32> {
    let n = (secs * FS as f32) as usize;
    let mut s = seed.wrapping_mul(2_654_435_761).max(1);
    let mut prev = 0.0;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let w = (s as f32 / u32::MAX as f32) * 2.0 - 1.0;
            let v = w - prev;
            prev = w;
            v * amp * 0.5
        })
        .collect()
}

pub fn silence(secs: f32) -> Vec<f32> {
    vec![0.0; (secs * FS as f32) as usize]
}

/// 16-bit mono PCM, written here so the crate's own reader is tested
/// against an independent writer.
pub fn write_wav(path: &Path, samples: &[f32], fs: u32) {
    let mut b = Vec::new();
    let data_len = (samples.len() * 2) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&fs.to_le_bytes());
    b.extend_from_slice(&(fs * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, b).unwrap();
}

/// F0 by plain autocorrelation over `[from, to)` seconds of `x`: the median
/// of 40 ms windows, an estimator independent of the crate's.
pub fn measure_f0(x: &[f32], fs: u32, from: f64, to: f64) -> f32 {
    let win = (0.04 * fs as f64) as usize;
    let hop = win / 2;
    let a = (from * fs as f64).max(0.0) as usize;
    let b = ((to * fs as f64) as usize).min(x.len());
    let min_lag = (fs as f32 / 1000.0) as usize;
    let max_lag = (fs as f32 / 60.0) as usize;
    let mut found = Vec::new();
    let mut i = a;
    while i + win + max_lag < b {
        let seg = &x[i..i + win + max_lag];
        let e0: f32 = seg[..win].iter().map(|v| v * v).sum();
        if e0 < 1e-6 {
            i += hop;
            continue;
        }
        let r: Vec<f32> = (0..=max_lag)
            .map(|lag| {
                let mut s = 0.0;
                let mut e1 = 0.0;
                for j in 0..win {
                    s += seg[j] * seg[j + lag];
                    e1 += seg[j + lag] * seg[j + lag];
                }
                s / (e0 * e1).sqrt().max(1e-12)
            })
            .collect();
        // The first peak above 0.8 of the best, past the first zero crossing.
        let best = r[min_lag..].iter().cloned().fold(f32::MIN, f32::max);
        let mut lag = min_lag;
        while lag < max_lag && r[lag] > 0.0 {
            lag += 1;
        }
        let mut pick = None;
        for l in lag.max(min_lag)..max_lag {
            if r[l] > 0.8 * best && r[l] >= r[l - 1] && r[l] >= r[l + 1] {
                pick = Some(l);
                break;
            }
        }
        if let Some(l) = pick {
            let (y0, y1, y2) = (r[l - 1], r[l], r[l + 1]);
            let d = 0.5 * (y0 - y2) / (y0 - 2.0 * y1 + y2);
            found.push(fs as f32 / (l as f32 + d));
        }
        i += hop;
    }
    if found.is_empty() {
        return 0.0;
    }
    found.sort_by(|a, b| a.partial_cmp(b).unwrap());
    found[found.len() / 2]
}

pub fn cents(a: f32, b: f32) -> f32 {
    1200.0 * (a / b).log2()
}

pub fn midi_hz(m: f32) -> f32 {
    440.0 * 2f32.powf((m - 69.0) / 12.0)
}

pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// The consonant each test mora starts with (as noise), if any.
pub fn consonant_secs(alias: &str) -> f32 {
    let first = alias.chars().next().unwrap_or('a');
    if "あいうえおんaiueon".contains(first) {
        0.0
    } else {
        0.08
    }
}

/// The vowel a test mora ends on.
pub fn vowel_char(alias: &str) -> char {
    match alias.chars().last().unwrap_or('a') {
        'あ' | 'か' | 'さ' | 'た' | 'な' | 'ら' | 'a' => 'a',
        'い' | 'き' | 'し' | 'i' => 'i',
        'う' | 'く' | 'す' | 'u' => 'u',
        'え' | 'け' | 'せ' | 'e' => 'e',
        'お' | 'こ' | 'そ' | 'o' => 'o',
        'ん' | 'n' => 'n',
        _ => 'a',
    }
}

/// A CV bank with one sample per alias at `f0`: 100 ms of silence, the
/// consonant (noise) if the mora has one, 600 ms of the vowel. Its oto
/// puts the offset at 80 ms and the preutterance on the vowel's onset.
pub fn write_cv_bank(dir: &Path, aliases: &[&str], f0: f32) {
    let mut oto = String::new();
    for (i, alias) in aliases.iter().enumerate() {
        let c = consonant_secs(alias);
        let mut x = silence(0.1);
        x.extend(noise(c, 0.2, i as u32 + 1));
        x.extend(vowel(f0, 0.6, vowel_char(alias)));
        let file = format!("s{i}.wav");
        write_wav(&dir.join(&file), &x, FS);
        let pre = 20.0 + c * 1000.0;
        oto.push_str(&format!(
            "{file}={alias},80,{},{},{pre},10\n",
            pre + 60.0,
            -(pre + 560.0)
        ));
    }
    std::fs::write(dir.join("oto.ini"), oto).unwrap();
}
