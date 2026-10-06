//! Writes the reference fixtures' samples as raw little-endian `f32` (mono,
//! 22 050 Hz) into a directory, for `tests/fixtures/make_reference.py`.
//!
//! `cargo run -p fontelle-analysis --example write_fixtures -- <dir>`

use std::io::Write;

fn main() -> std::io::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: write_fixtures <dir>");
    std::fs::create_dir_all(&dir)?;
    for fixture in fontelle_analysis::testsignals::reference_fixtures() {
        let path = std::path::Path::new(&dir).join(format!("{}.f32", fixture.name));
        let mut file = std::io::BufWriter::new(std::fs::File::create(&path)?);
        for s in &fixture.samples {
            file.write_all(&s.to_le_bytes())?;
        }
        println!("{} ({} samples)", path.display(), fixture.samples.len());
    }
    Ok(())
}
