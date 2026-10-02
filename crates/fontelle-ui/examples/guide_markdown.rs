//! Writes the guide as Markdown: the `guide.md` the web manual is built from
//! (hub cards 0346 and 0358). The text is the app's own `GUIDE`, so the
//! manual and the app never disagree; regenerate it whenever `guide.rs`
//! changes rather than editing the file by hand.
//!
//! `cargo run --example guide_markdown -p fontelle-ui > guide.md`

use fontelle_ui::canvas::{GUIDE, GuideKind, GuideMedia};

/// The clip's name in the manual's folder, `guide-<name>.png`, matching the
/// one `tools/guide-clips` records into `assets/guide/`.
fn clip(media: GuideMedia) -> &'static str {
    match media {
        GuideMedia::Transport => "guide-transport.png",
        GuideMedia::Rack => "guide-rack.png",
        GuideMedia::Browser => "guide-browser.png",
        GuideMedia::Clips => "guide-clips.png",
        GuideMedia::Lanes => "guide-lanes.png",
        GuideMedia::Roll => "guide-roll.png",
        GuideMedia::Slide => "guide-slide.png",
        GuideMedia::SlideEdit => "guide-slide-edit.png",
        GuideMedia::Mixer => "guide-mixer.png",
        GuideMedia::Export => "guide-export.png",
        GuideMedia::Settings => "guide-settings.png",
    }
}

fn main() {
    println!("# Fontelle guide\n");
    println!(
        "Generated from `crates/fontelle-ui/src/canvas/guide.rs` (the `GUIDE` catalogue the app's tour and `?` guide read).\n"
    );
    for section in GUIDE {
        match section.kind {
            GuideKind::Tour => println!("## {}\n", section.title),
            GuideKind::ComingFrom => println!("## {} (Coming from…)\n", section.title),
        }
        for page in section.pages {
            println!("### {}\n", page.title);
            if let Some(media) = page.media {
                println!("![{}]({})\n", page.title, clip(media));
            }
            for paragraph in page.paragraphs {
                println!("{paragraph}\n");
            }
            if let Some(choice) = page.choice {
                println!("*In the app, this page asks a question here ({choice:?}).*\n");
            }
        }
    }
}
