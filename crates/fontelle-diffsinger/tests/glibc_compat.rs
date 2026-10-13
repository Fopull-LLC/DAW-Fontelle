//! The five symbols the prebuilt ONNX Runtime asks of a newer glibc and
//! libstdc++ than the Linux release is built against (Ubuntu 22.04: glibc
//! 2.35, GCC 12), provided by Fontelle itself (`src/glibc_compat.rs`).
//!
//! > *"we should pick the option that leaves us with a overall better
//! > product all around"*
//!
//! The release links them only with the `old-glibc` feature; the functions
//! behind them are plain Rust and are checked here on every machine.

#![cfg(all(target_os = "linux", target_env = "gnu"))]

use std::ffi::{CStr, CString};

use fontelle_diffsinger::glibc_compat::{replace_cold, strtol_c23, strtoll_c23, strtoull_c23};

/// Parses `text` with `base`, returning the value and how much was read.
fn parse<T>(
    f: unsafe extern "C" fn(*const CChar, *mut *mut CChar, i32) -> T,
    text: &str,
    base: i32,
) -> (T, usize) {
    let c = CString::new(text).unwrap();
    let mut end: *mut CChar = std::ptr::null_mut();
    let value = unsafe { f(c.as_ptr(), &mut end, base) };
    let read = end as usize - c.as_ptr() as usize;
    (value, read)
}

type CChar = std::ffi::c_char;

#[test]
fn c23_strtol_reads_what_the_old_one_did() {
    assert_eq!(parse(strtol_c23, "123", 10), (123, 3));
    assert_eq!(parse(strtol_c23, "  42abc", 10), (42, 4));
    assert_eq!(parse(strtol_c23, "-17", 0), (-17, 3));
    assert_eq!(parse(strtol_c23, "0x1f", 0), (31, 4));
    assert_eq!(parse(strtol_c23, "017", 0), (15, 3));
    assert_eq!(parse(strtol_c23, "zz", 36), (36 * 35 + 35, 2));
    // Nothing to read: nothing read, and the end is the start.
    assert_eq!(parse(strtol_c23, "abc", 10), (0, 0));
}

#[test]
fn c23_strtol_also_reads_binary() {
    // What C23 added, and the one way the new names differ from the old.
    assert_eq!(parse(strtol_c23, "0b101", 0), (5, 5));
    assert_eq!(parse(strtol_c23, "0B11", 2), (3, 4));
    assert_eq!(parse(strtol_c23, " -0b10x", 0), (-2, 6));
    assert_eq!(parse(strtol_c23, "+0b1", 0), (1, 4));
    // `0b` with no binary digit after it is the number 0, then a `b`.
    assert_eq!(parse(strtol_c23, "0b", 0), (0, 1));
    assert_eq!(parse(strtol_c23, "0b2", 0), (0, 1));
    // Base 10 and 16 are not binary: `0b1` in hex is the number 0xb1.
    assert_eq!(parse(strtol_c23, "0b1", 10), (0, 1));
    assert_eq!(parse(strtol_c23, "0b1", 16), (0xb1, 3));
}

#[test]
fn the_long_long_and_unsigned_forms_agree() {
    assert_eq!(parse(strtoll_c23, "-9000000000", 10), (-9_000_000_000, 11));
    assert_eq!(parse(strtoll_c23, "0b1111", 0), (15, 6));
    assert_eq!(
        parse(strtoull_c23, "18446744073709551615", 10),
        (u64::MAX, 20)
    );
    assert_eq!(parse(strtoull_c23, "0b100", 0), (4, 5));
    assert_eq!(parse(strtoull_c23, "0xff", 16), (255, 4));
}

/// What `std::string::replace(pos, len1, s, len2)` must leave, with `s`
/// pointing into the string itself — the case libstdc++ hands to
/// `_M_replace_cold`: the prefix, then what `s` named before the call,
/// then the tail.
fn expected(old: &[u32], pos: usize, len1: usize, soff: usize, len2: usize) -> Vec<u32> {
    let mut out = old[..pos].to_vec();
    out.extend_from_slice(&old[soff..soff + len2]);
    out.extend_from_slice(&old[pos + len1..]);
    out
}

#[test]
fn replace_cold_is_replace_with_an_overlapping_source_in_every_shape() {
    // Every position, every length replaced, every source inside the
    // string and every length of it, for one short string: each branch of
    // the upstream routine (shrinking, same size, growing with the source
    // before, after and across the replaced part) is in here many times.
    let old: Vec<u32> = (1..=9).collect();
    let n = old.len();
    let mut checked = 0;
    for pos in 0..=n {
        for len1 in 0..=n - pos {
            for len2 in 0..=n {
                for soff in 0..=n - len2 {
                    let how_much = n - pos - len1;
                    let size = n - len1 + len2;
                    let mut buf = vec![0u32; n.max(size)];
                    buf[..n].copy_from_slice(&old);
                    let base = buf.as_mut_ptr();
                    unsafe {
                        replace_cold(base.add(pos), len1, base.add(soff), len2, how_much);
                    }
                    assert_eq!(
                        &buf[..size],
                        &expected(&old, pos, len1, soff, len2)[..],
                        "pos {pos} len1 {len1} s {soff} len2 {len2}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 1000, "{checked}");
}

#[test]
fn replace_cold_works_on_bytes_too() {
    let mut buf = *b"hello world\0\0\0\0";
    let base = buf.as_mut_ptr();
    // "hello world" -> replace "world" (pos 6, 5) with "hello" from itself.
    unsafe { replace_cold(base.add(6), 5, base, 5, 0) };
    assert_eq!(&buf[..11], b"hello hello");
    let _ = CStr::from_bytes_until_nul(&buf).unwrap();
}
