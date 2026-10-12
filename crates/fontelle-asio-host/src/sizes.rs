// fontelle-asio — hosts an ASIO driver for Fontelle in a separate process.
// Copyright (C) 2026 Fopull LLC
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation, version 3. It is distributed WITHOUT ANY WARRANTY; see
// the GNU General Public License (`LICENSE` beside `Cargo.toml`) for details.

//! Which buffer size and sample rate to ask a driver for, and what to say
//! when it will not.

/// The rates asked about when a driver will not take the studio's, so the
/// refusal can say what it would take.
pub const COMMON_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// The buffer to ask the driver for: `requested` brought inside the
/// driver's range and onto its steps, or the driver's preferred size when
/// nothing was asked. `granularity` is ASIO's: `-1` powers of two, `0` the
/// preferred size only, `n > 0` steps of `n` from `min`.
pub fn fit_buffer_size(
    requested: Option<u32>,
    min: u32,
    max: u32,
    preferred: u32,
    granularity: i32,
) -> u32 {
    let max = max.max(min);
    let Some(asked) = requested else {
        return preferred.clamp(min, max);
    };
    match granularity {
        // Powers of two from the minimum up: the smallest one not below
        // what was asked, since a size smaller than asked is a crackle
        // nobody chose.
        -1 => {
            if asked <= min {
                return min;
            }
            let p = asked.checked_next_power_of_two().unwrap_or(max);
            p.clamp(min, max)
        }
        g if g > 0 => {
            let g = g as u32;
            let steps = asked.saturating_sub(min).div_ceil(g);
            min.saturating_add(steps.saturating_mul(g)).clamp(min, max)
        }
        _ => preferred.clamp(min, max),
    }
}

/// Why a driver offering `offered` cannot run Fontelle at `rate`, in words.
pub fn rate_refusal(driver: &str, offered: &[u32], rate: u32) -> String {
    let khz = |r: u32| {
        let k = f64::from(r) / 1000.0;
        if r.is_multiple_of(1000) {
            format!("{k:.0} kHz")
        } else {
            format!("{k:.1} kHz")
        }
    };
    let offers = match offered {
        [] => "says no rate it can run at".to_string(),
        [only] => format!("offers only {}", khz(*only)),
        [first @ .., last] => format!(
            "offers {} and {}",
            first.iter().map(|r| khz(*r)).collect::<Vec<_>>().join(", "),
            khz(*last)
        ),
    };
    format!(
        "{driver} cannot run at {} \u{2014} the driver {offers}; set it to {} in the driver's \
         control panel",
        khz(rate),
        khz(rate)
    )
}
