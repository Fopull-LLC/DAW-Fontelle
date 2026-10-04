//! Reads one plugin bundle and prints what it holds — the child process of
//! `fontelle_host::BundleProber`, for the host's own tests. The studio
//! answers the same flag itself.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(fontelle_host::probe_main(&args).unwrap_or(2));
}
