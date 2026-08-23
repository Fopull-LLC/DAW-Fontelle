//! `cargo xtask <command>` — build/bundle/package automation (FONTELLE_TDD.md §21).

fn main() {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("bundle") => {
            todo!("build fontelle-app + fontelle-plugin, assemble a per-OS bundle")
        }
        Some("package") => {
            todo!("AppImage / Flatpak / AUR / tarball packaging, TDD §21")
        }
        _ => {
            eprintln!("usage: cargo xtask <bundle|package>");
            std::process::exit(1);
        }
    }
}
