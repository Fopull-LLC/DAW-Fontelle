//! lilv 0.28.0, serd 0.32.10, sord 0.16.22, sratom 0.6.22 and zix 0.8.2 —
//! the versions a current Linux distribution ships — compiled from `c/` into
//! one static `lilv-0`, which `lilv-sys`'s `#[link(name = "lilv-0")]` then
//! finds before any system copy.
//!
//! Each library's own build is meson; what it decides is said here instead:
//! C11 (GCC's default C23 turns zix's `constexpr` on and its Windows code
//! does not compile under it), the POSIX feature macros meson passes, the
//! `*_STATIC` and `*_INTERNAL` defines a static build of each takes, and
//! zix's per-system files. Each library's own config header detects the rest.

use std::path::{Path, PathBuf};

const LIBRARIES: [&str; 5] = [
    "lilv-0.28.0",
    "serd-0.32.10",
    "sord-0.16.22",
    "sratom-0.6.22",
    "zix-0.8.2",
];

fn sources_in(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
        .collect();
    files.sort();
    files
}

fn main() {
    let c = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("c");
    println!("cargo:rerun-if-changed=c");
    println!("cargo:rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let windows = os == "windows";
    // A lint of another system from this one (`.github/scripts/preflight.sh`
    // for macOS, whose SDK is not here): clippy never links, so the C is
    // not needed and cannot be compiled.
    println!("cargo:rerun-if-env-changed=FONTELLE_LILV_LINT_ONLY");
    if std::env::var_os("FONTELLE_LILV_LINT_ONLY").is_some() {
        return;
    }

    let mut build = cc::Build::new();
    build.warnings(false);
    // C11 for GCC and clang, whose newest default (C23) turns zix's
    // `constexpr` on. **Not for MSVC**: there `/std:c11` makes zix's
    // `ZIX_STATIC_STRING` a compound literal, which MSVC will not take as a
    // static initializer (C2099, `path.c`) — and meson, which the libraries
    // are tested with, passes MSVC no standard at all.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        build.std("c11");
    }
    for library in LIBRARIES {
        let dir = c.join(library);
        build.include(dir.join("include")).include(dir.join("src"));
        build.files(sources_in(&dir.join("src")));
    }
    build.include(c.join("lv2-1.18.10/include"));

    // zix's system layer, as its meson.build picks it.
    let zix = c.join("zix-0.8.2/src");
    if windows {
        for file in [
            "environment_win32.c",
            "filesystem_win32.c",
            "system_win32.c",
            "win32_util.c",
            "sem_win32.c",
            "thread_win32.c",
        ] {
            build.file(zix.join("win32").join(file));
        }
    } else {
        for file in ["environment_posix.c", "filesystem_posix.c", "system_posix.c", "thread_posix.c"] {
            build.file(zix.join("posix").join(file));
        }
        if os == "macos" {
            build.file(zix.join("darwin/sem_darwin.c"));
        } else {
            build.file(zix.join("posix/sem_posix.c"));
        }
    }

    for define in [
        "LILV_STATIC",
        "SERD_STATIC",
        "SORD_STATIC",
        "SRATOM_STATIC",
        "ZIX_STATIC",
        "LILV_INTERNAL",
        "SERD_INTERNAL",
        "SORD_INTERNAL",
        "SRATOM_INTERNAL",
        "ZIX_INTERNAL",
    ] {
        build.define(define, None);
    }
    match os.as_str() {
        // glibc hides `copy_file_range` and friends without it.
        "linux" => {
            build.define("_GNU_SOURCE", None);
        }
        "macos" => {
            build.define("_DARWIN_C_SOURCE", None);
            // zix and serd guess `posix_fadvise` from the POSIX version;
            // macOS says POSIX.1-2001 and has no such function. Meson would
            // have found out by linking one.
            build.define("HAVE_POSIX_FADVISE", "0");
        }
        _ => {}
    }
    if windows {
        build.define("WIN32_LEAN_AND_MEAN", None);
    }
    build.compile("lilv-0");
}
