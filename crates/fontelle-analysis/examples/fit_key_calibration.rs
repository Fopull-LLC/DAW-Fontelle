//! Fits the key confidence's logistic (`key.rs`, constants `A`..`E` and
//! `T`) on synthetic material from `testsignals::key_material`: random keys,
//! 4 to 64 notes, anywhere from none to all of them random pitch classes.
//! Prints the coefficients to paste back.
//!
//! `cargo run --release -p fontelle-analysis --example fit_key_calibration`

// Small dense linear algebra, indexed the way it is written on paper.
#![allow(clippy::needless_range_loop)]

use fontelle_analysis::key::detect_key;
use fontelle_analysis::testsignals::key_material;
use fontelle_types::KeyScale;

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// Plain Newton's method on the log-loss, with a touch of ridge.
fn logistic(xs: &[Vec<f64>], ys: &[f64]) -> Vec<f64> {
    let k = xs[0].len();
    let mut w = vec![0.0; k];
    for _ in 0..50 {
        let mut grad = vec![0.0; k];
        let mut hess = vec![vec![0.0; k]; k];
        for (x, &y) in xs.iter().zip(ys) {
            let p = sigmoid(x.iter().zip(&w).map(|(a, b)| a * b).sum());
            for i in 0..k {
                grad[i] += (p - y) * x[i];
                for j in 0..k {
                    hess[i][j] += p * (1.0 - p) * x[i] * x[j];
                }
            }
        }
        for (i, row) in hess.iter_mut().enumerate() {
            row[i] += 1e-3;
            grad[i] += 1e-3 * w[i];
        }
        // Solve hess · step = grad (Gaussian elimination).
        let mut a: Vec<Vec<f64>> = hess
            .iter()
            .zip(&grad)
            .map(|(r, g)| [r.clone(), vec![*g]].concat())
            .collect();
        for c in 0..k {
            let p = (c..k)
                .max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))
                .unwrap();
            a.swap(c, p);
            for r in 0..k {
                if r != c {
                    let f = a[r][c] / a[c][c];
                    for cc in c..=k {
                        a[r][cc] -= f * a[c][cc];
                    }
                }
            }
        }
        for i in 0..k {
            w[i] -= a[i][k] / a[i][i];
        }
    }
    w
}

fn main() {
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    let (mut tx, mut ty) = (Vec::new(), Vec::new());
    let mut state = 0x1234_5678u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    for seed in 0..20_000u64 {
        let root = (next() * 12.0) as u8 % 12;
        let minor = next() < 0.5;
        let chromatic = next() as f32;
        let n = 4 + (next() * 61.0) as usize;
        let notes = key_material(seed + 50_000, root, minor, chromatic, n);
        let Some(reading) = detect_key(&notes) else {
            continue;
        };
        let truth = KeyScale::new(root, if minor { "natural-minor" } else { "major" });
        let right_set =
            reading.key.mask() == truth.mask() || reading.relative.mask() == truth.mask();
        let e = reading.evidence;
        xs.push(vec![
            f64::from(e.margin),
            f64::from(e.best),
            f64::from(e.mass).ln_1p(),
            f64::from(e.inside),
            1.0,
        ]);
        ys.push(if right_set { 1.0 } else { 0.0 });
        if right_set && (reading.key == truth || reading.relative == truth) {
            tx.push(vec![f64::from(e.tonic_margin)]);
            ty.push(if reading.key == truth { 1.0 } else { 0.0 });
        }
    }
    let w = logistic(&xs, &ys);
    let t = logistic(&tx, &ty);
    let rate = ys.iter().sum::<f64>() / ys.len() as f64;
    let tonic_rate = ty.iter().sum::<f64>() / ty.len() as f64;
    println!(
        "{} readings, pitch set right {:.1} %; tonic right {:.1} % of those",
        ys.len(),
        100.0 * rate,
        100.0 * tonic_rate
    );
    println!(
        "const A: f32 = {:.3};\nconst B: f32 = {:.3};\nconst C: f32 = {:.3};\nconst E: f32 = {:.3};\nconst D: f32 = {:.3};\nconst T: f32 = {:.3};",
        w[0], w[1], w[2], w[3], w[4], t[0]
    );
    // Reliability: predicted against observed, in deciles.
    let mut bins = vec![(0.0, 0.0, 0usize); 10];
    for (x, &y) in xs.iter().zip(&ys) {
        let p = sigmoid(x.iter().zip(&w).map(|(a, b)| a * b).sum());
        let b = ((p * 10.0) as usize).min(9);
        bins[b].0 += p;
        bins[b].1 += y;
        bins[b].2 += 1;
    }
    for (i, (p, y, n)) in bins.iter().enumerate() {
        if *n > 0 {
            println!(
                "  {:>3}-{:>3} %: predicted {:.2}, observed {:.2} ({n})",
                i * 10,
                i * 10 + 10,
                p / *n as f64,
                y / *n as f64
            );
        }
    }
}
