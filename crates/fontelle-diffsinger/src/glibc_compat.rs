//! Five symbols the prebuilt ONNX Runtime needs from a newer glibc and
//! libstdc++ than the Linux release is built against, provided here.
//!
//! pyke's static ONNX Runtime 1.28 was compiled against glibc 2.38 and GCC
//! 13. Linked on Ubuntu 22.04 (glibc 2.35, GCC 12), which the release builds
//! on so that Fontelle runs on 22.04, Debian 12 and Mint 21, it asks for
//! `__isoc23_strtol`, `__isoc23_strtoll`, `__isoc23_strtoull` and
//! libstdc++'s `basic_string::_M_replace_cold` (char and wchar_t), and
//! nothing else. v0.27.0's Linux build failed on exactly these.
//!
//! Defining them in Fontelle keeps the download one file — so an update from
//! an older Fontelle, whose updater replaces only the binary, brings the
//! singer whole — and keeps the one ONNX Runtime build every platform
//! links. The other ways out were a Linux that needs glibc 2.39, or a
//! separate library the old updater could not install.
//!
//! The functions are always compiled and tested (`tests/glibc_compat.rs`);
//! they take the C names only with the `old-glibc` feature, which the
//! release workflow's Linux build turns on (and CI's build on 22.04). On a
//! newer system the real ones are there and nothing here is needed. A new
//! ONNX Runtime that asks for more fails that CI build by name.

use std::ffi::{c_char, c_int, c_long, c_longlong, c_ulonglong};

unsafe extern "C" {
    // The pre-C23 names, which glibc 2.35 has.
    fn strtol(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_long;
    fn strtoll(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_longlong;
    fn strtoull(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_ulonglong;
}

/// Where a C23 `0b` literal's digits start, and whether it is negative —
/// or `None` when `s` is not one, and the pre-C23 function reads it as is.
///
/// C23's only change to these functions: with base 0 or 2, an optional
/// sign then `0b` or `0B` *followed by a binary digit* is a binary prefix.
unsafe fn binary_digits(s: *const c_char, base: c_int) -> Option<(*const c_char, bool)> {
    if base != 0 && base != 2 {
        return None;
    }
    let at = |i: usize| unsafe { *s.add(i) as u8 };
    let mut i = 0;
    // C's isspace in the C locale.
    while matches!(at(i), b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let negative = at(i) == b'-';
    if matches!(at(i), b'-' | b'+') {
        i += 1;
    }
    (at(i) == b'0' && at(i + 1) | 0x20 == b'b' && matches!(at(i + 2), b'0' | b'1'))
        .then(|| (unsafe { s.add(i + 2) }, negative))
}

/// `__isoc23_strtol`: `strtol`, reading C23's `0b` literals too.
///
/// # Safety
/// As `strtol`: `s` is a NUL-terminated string, `end` null or writable.
pub unsafe extern "C" fn strtol_c23(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_long {
    match unsafe { binary_digits(s, base) } {
        // A negative binary literal past `LONG_MAX` saturates at
        // `-LONG_MAX` rather than glibc's `LONG_MIN`: nothing ONNX Runtime
        // reads is one.
        Some((digits, negative)) => {
            let v = unsafe { strtol(digits, end, 2) };
            if negative { v.wrapping_neg() } else { v }
        }
        None => unsafe { strtol(s, end, base) },
    }
}

/// `__isoc23_strtoll`.
///
/// # Safety
/// As `strtoll`.
pub unsafe extern "C" fn strtoll_c23(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_longlong {
    match unsafe { binary_digits(s, base) } {
        Some((digits, negative)) => {
            let v = unsafe { strtoll(digits, end, 2) };
            if negative { v.wrapping_neg() } else { v }
        }
        None => unsafe { strtoll(s, end, base) },
    }
}

/// `__isoc23_strtoull`. A negative number is negated in unsigned
/// arithmetic, as `strtoull` does.
///
/// # Safety
/// As `strtoull`.
pub unsafe extern "C" fn strtoull_c23(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_ulonglong {
    match unsafe { binary_digits(s, base) } {
        Some((digits, negative)) => {
            let v = unsafe { strtoull(digits, end, 2) };
            if negative { v.wrapping_neg() } else { v }
        }
        None => unsafe { strtoull(s, end, base) },
    }
}

/// libstdc++ 13's `basic_string<T>::_M_replace_cold`, line for line: the
/// in-place half of `replace(pos, len1, s, len2)` when `s` points into the
/// string itself and the new length fits. `p` is where the replaced part
/// starts and `how_much` the length of the tail after it.
///
/// It reads nothing of the string object (`this`) — every step is a move
/// within the buffer — which is what makes it possible to provide from
/// outside libstdc++. `_S_copy` is a `memcpy` there; a `memmove` here,
/// which is the same where they do not overlap.
///
/// # Safety
/// `p .. p + max(len1 + how_much, len2 + how_much)` and `s .. s + len2` lie
/// in one writable buffer.
pub unsafe fn replace_cold<T: Copy>(
    p: *mut T,
    len1: usize,
    s: *const T,
    len2: usize,
    how_much: usize,
) {
    unsafe {
        if len2 != 0 && len2 <= len1 {
            std::ptr::copy(s, p, len2);
        }
        if how_much != 0 && len1 != len2 {
            std::ptr::copy(p.add(len1), p.add(len2), how_much);
        }
        if len2 > len1 {
            let replaced_end = p.add(len1) as *const T;
            if s.add(len2) <= replaced_end {
                std::ptr::copy(s, p, len2);
            } else if s >= replaced_end {
                let poff = s.offset_from(p) as usize + (len2 - len1);
                std::ptr::copy(p.add(poff), p, len2);
            } else {
                let nleft = replaced_end.offset_from(s) as usize;
                std::ptr::copy(s, p, nleft);
                std::ptr::copy(p.add(len2), p.add(nleft), len2 - nleft);
            }
        }
    }
}

/// The C names, for the release's Linux link only.
#[cfg(feature = "old-glibc")]
mod exported {
    use std::ffi::{c_char, c_int, c_long, c_longlong, c_ulonglong};

    #[unsafe(no_mangle)]
    unsafe extern "C" fn __isoc23_strtol(
        s: *const c_char,
        end: *mut *mut c_char,
        base: c_int,
    ) -> c_long {
        unsafe { super::strtol_c23(s, end, base) }
    }

    #[unsafe(no_mangle)]
    unsafe extern "C" fn __isoc23_strtoll(
        s: *const c_char,
        end: *mut *mut c_char,
        base: c_int,
    ) -> c_longlong {
        unsafe { super::strtoll_c23(s, end, base) }
    }

    #[unsafe(no_mangle)]
    unsafe extern "C" fn __isoc23_strtoull(
        s: *const c_char,
        end: *mut *mut c_char,
        base: c_int,
    ) -> c_ulonglong {
        unsafe { super::strtoull_c23(s, end, base) }
    }

    // Member functions: `this` comes first in the Itanium C++ ABI, and goes
    // unread (see `replace_cold`). wchar_t is four bytes on Linux.
    #[unsafe(export_name = "_ZNSt7__cxx1112basic_stringIcSt11char_traitsIcESaIcEE15_M_replace_coldEPcmPKcmm")]
    unsafe extern "C" fn replace_cold_char(
        _this: *mut u8,
        p: *mut u8,
        len1: usize,
        s: *const u8,
        len2: usize,
        how_much: usize,
    ) {
        unsafe { super::replace_cold(p, len1, s, len2, how_much) }
    }

    #[unsafe(export_name = "_ZNSt7__cxx1112basic_stringIwSt11char_traitsIwESaIwEE15_M_replace_coldEPwmPKwmm")]
    unsafe extern "C" fn replace_cold_wchar(
        _this: *mut u8,
        p: *mut u32,
        len1: usize,
        s: *const u32,
        len2: usize,
        how_much: usize,
    ) {
        unsafe { super::replace_cold(p, len1, s, len2, how_much) }
    }
}
