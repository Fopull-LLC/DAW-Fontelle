//! `cargo xtask <command>` — build/bundle/package automation (FONTELLE_TDD.md §21).

mod presets;

fn main() {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("bundle") => {
            todo!("build fontelle-app + fontelle-plugin, assemble a per-OS bundle")
        }
        Some("package") => {
            todo!("AppImage / Flatpak / AUR / tarball packaging, TDD §21")
        }
        Some("export-factory-presets") => match presets::export() {
            Ok(report) => println!("{report}"),
            Err(why) => {
                eprintln!("export-factory-presets: {why}");
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!("usage: cargo xtask <bundle|package|export-factory-presets>");
            std::process::exit(1);
        }
    }
}
