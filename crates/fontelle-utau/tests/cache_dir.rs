//! Where the UTAU analysis cache goes (hub card 0429).
//!
//! Steam starts a program in its install folder, so a cache folder that comes
//! out relative is a cache written into the folder Steam verifies file by
//! file. The rule answers an absolute folder or none at all.

use std::path::PathBuf;

use fontelle_utau::cache::{CacheOs, default_cache_dir_from};

fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.to_string())
    }
}

#[test]
fn each_platform_has_its_usual_cache_folder() {
    assert_eq!(
        default_cache_dir_from(&env_of(&[("HOME", "/home/a")]), CacheOs::Linux),
        Some(PathBuf::from("/home/a/.cache/fontelle/utau"))
    );
    assert_eq!(
        default_cache_dir_from(
            &env_of(&[("HOME", "/home/a"), ("XDG_CACHE_HOME", "/c")]),
            CacheOs::Linux
        ),
        Some(PathBuf::from("/c/fontelle/utau"))
    );
    assert_eq!(
        default_cache_dir_from(&env_of(&[("HOME", "/Users/a")]), CacheOs::Macos),
        Some(PathBuf::from("/Users/a/Library/Caches/fontelle/utau"))
    );
    assert_eq!(
        default_cache_dir_from(
            &env_of(&[("LOCALAPPDATA", r"C:\Users\a\AppData\Local")]),
            CacheOs::Windows
        ),
        Some(
            PathBuf::from(r"C:\Users\a\AppData\Local")
                .join("fontelle")
                .join("utau")
        )
    );
}

#[test]
fn an_empty_or_relative_variable_is_no_folder_at_all() {
    for os in [CacheOs::Linux, CacheOs::Macos] {
        assert_eq!(default_cache_dir_from(&env_of(&[("HOME", "")]), os), None);
        assert_eq!(
            default_cache_dir_from(&env_of(&[("HOME", "rel")]), os),
            None
        );
    }
    // A relative XDG variable is ignored, as the spec says, not honoured.
    assert_eq!(
        default_cache_dir_from(
            &env_of(&[("XDG_CACHE_HOME", "rel"), ("HOME", "/home/a")]),
            CacheOs::Linux
        ),
        Some(PathBuf::from("/home/a/.cache/fontelle/utau"))
    );
    assert_eq!(
        default_cache_dir_from(&env_of(&[("LOCALAPPDATA", "rel")]), CacheOs::Windows),
        None
    );
    assert_eq!(default_cache_dir_from(&env_of(&[]), CacheOs::Windows), None);
}
