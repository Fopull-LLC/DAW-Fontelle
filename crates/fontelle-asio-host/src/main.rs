// fontelle-asio — hosts an ASIO driver for Fontelle in a separate process.
// Copyright (C) 2026 Fopull LLC. GPL-3.0-only; see `LICENSE` beside
// `Cargo.toml`. ASIO is a registered trademark of Steinberg Media
// Technologies GmbH.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(fontelle_asio_host::run::main(&args));
}
