//! Reads one plugin bundle and prints what it holds — the child process of
//! `fontelle_host::BundleProber`, for the host's own tests. The studio
//! answers the same flag itself, and the EGL probe's (`alpha_egl`).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // And the EGL probe, for the same tests' sake (`alpha_egl`).
    if let Some(code) = fontelle_host::egl_probe_main(&args) {
        std::process::exit(code);
    }
    std::process::exit(fontelle_host::probe_main(&args).unwrap_or(2));
}
