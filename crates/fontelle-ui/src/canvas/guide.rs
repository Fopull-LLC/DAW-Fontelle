//! One way to learn Fontelle: the guide, the pages behind `?`, and the tour.
//!
//! `docs/ux-routing-and-learning-plan.md` §5. Ty chose an interactive tour,
//! in-app guide pages, "Coming from…" pages and a web manual, and asked that
//! they be *one clean experience, not intrusive and not hard to use*. So the
//! content is written once, here: [`GUIDE`], drawn by the tour as a card
//! beside the part of the window it is about ([`tour_layout`]) and by the help
//! page as reading ([`help_layout`]) — a feature is documented once, and the
//! two can never disagree. The web manual is built from the same text.
//!
//! Geometry only, like every canvas here (INVARIANT 2): what a
//! [`GuideTarget`] is on screen, and what a [`GuideChoice`] sets, are the
//! window's and the host's.

use crate::layout::Rect;
use crate::theme::Metrics;

/// Which of the two readings a section is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideKind {
    /// Part of the tour, and a chapter of the guide.
    Tour,
    /// "Coming from…": how Fontelle maps onto a DAW somebody already knows.
    /// In the guide only.
    ComingFrom,
}

/// The part of the window a tour step points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideTarget {
    Transport,
    Rack,
    Browser,
    Arrangement,
    /// The piano roll, shown for the step.
    Editor,
    /// The mixer, swapped in for the step.
    Mixer,
}

/// Something a tour step sets up as it goes, by asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideChoice {
    /// Rack-style or lane-style routing — for this song and for new ones.
    Routing,
    /// VST 2 plugins, which are an extension to download.
    Vst2,
}

/// A short looping clip of a page's action being done — recorded from the
/// real binary on the tour's song (`docs/ux-routing-and-learning-plan.md`
/// step 7). The files are `crate::guide_media`'s; this is only which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuideMedia {
    Transport,
    Rack,
    Browser,
    Clips,
    Lanes,
    Roll,
    Mixer,
    Export,
    Settings,
}

impl GuideMedia {
    pub const ALL: &'static [GuideMedia] = &[
        GuideMedia::Transport,
        GuideMedia::Rack,
        GuideMedia::Browser,
        GuideMedia::Clips,
        GuideMedia::Lanes,
        GuideMedia::Roll,
        GuideMedia::Mixer,
        GuideMedia::Export,
        GuideMedia::Settings,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuidePage {
    pub title: &'static str,
    pub paragraphs: &'static [&'static str],
    pub target: Option<GuideTarget>,
    pub choice: Option<GuideChoice>,
    /// The action it describes, being done. Only a page that describes one
    /// has it: a choice is already something to do, and the welcome and the
    /// *Coming from…* pages are reading.
    pub media: Option<GuideMedia>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuideSection {
    pub title: &'static str,
    pub kind: GuideKind,
    pub pages: &'static [GuidePage],
}

const fn page(
    title: &'static str,
    paragraphs: &'static [&'static str],
    target: Option<GuideTarget>,
) -> GuidePage {
    GuidePage {
        title,
        paragraphs,
        target,
        choice: None,
        media: None,
    }
}

/// `page`, showing `media`.
const fn shows(page: GuidePage, media: GuideMedia) -> GuidePage {
    GuidePage {
        media: Some(media),
        ..page
    }
}

const fn ask(
    title: &'static str,
    paragraphs: &'static [&'static str],
    target: Option<GuideTarget>,
    choice: GuideChoice,
) -> GuidePage {
    GuidePage {
        title,
        paragraphs,
        target,
        choice: Some(choice),
        media: None,
    }
}

use GuideTarget::*;

/// Everything the guide says, in the order it says it.
pub const GUIDE: &[GuideSection] = &[
    GuideSection {
        title: "Welcome",
        kind: GuideKind::Tour,
        pages: &[page(
            "Welcome to Fontelle",
            &[
                "This tour walks through a small song, one part of the window at a time. Try things as you go: the tour stays out of the way, and anything you change here is only the demo's.",
                "Next and Back move through it, and the list at the top jumps to any part. Everything here is also in the guide behind the ? on the transport bar, whenever you want it again.",
            ],
            None,
        )],
    },
    GuideSection {
        title: "Playing the song",
        kind: GuideKind::Tour,
        pages: &[shows(
            page(
                "The transport",
                &[
                    "Play and stop are here, and on Space and Home. Beside them are loop, record (R) and the metronome (Ctrl+M).",
                    "Click the tempo to type one, or drag it. Song plays the whole arrangement; switch it to Clip to loop only the clip you are editing.",
                ],
                Some(Transport),
            ),
            GuideMedia::Transport,
        )],
    },
    GuideSection {
        title: "Instruments",
        kind: GuideKind::Tour,
        pages: &[shows(
            page(
                "The channel rack",
                &[
                    "Every instrument in the song is a row here. + Add instrument adds a built-in synth, a sampler, a soundfont or a plugin.",
                    "The \u{2261} button opens a row's instrument in a window of its own. The headphones solo it and the speaker mutes it. Click a row to choose it: that is the instrument new clips and a MIDI keyboard play.",
                ],
                Some(Rack),
            ),
            GuideMedia::Rack,
        )],
    },
    GuideSection {
        title: "Sounds and files",
        kind: GuideKind::Tour,
        pages: &[
            shows(
                page(
                    "The browser",
                    &[
                        "The tabs along the top are sounds, presets, projects, files to import, and settings. Click a sound to hear it; double-click it, or drag it onto a channel, to use it.",
                        "The Import tab lists your MIDI files, FL Studio scores and audio from folders you choose. Drop a file from your desktop onto the arrangement to bring it in.",
                    ],
                    Some(Browser),
                ),
                GuideMedia::Browser,
            ),
            ask(
                "VST 2 plugins",
                &[
                    "Fontelle hosts CLAP, VST 3 and LV2 plugins as they are. VST 2 plugins need a small extension, downloaded when you ask for it.",
                    "Turn it on here if you use VST 2 plugins. You can change your mind any time in Settings, under Extensions.",
                ],
                None,
                GuideChoice::Vst2,
            ),
        ],
    },
    GuideSection {
        title: "Arranging",
        kind: GuideKind::Tour,
        pages: &[
            shows(
                page(
                    "Lanes and clips",
                    &[
                        "The arrangement is the song from left to right. Double-click an empty spot to draw a clip of the chosen instrument; drag a clip to move it, and drag its right edge to make it longer. Shift and that edge makes it loop.",
                        "Ctrl+B cuts clips at the marker, Ctrl+D duplicates, Delete removes. Ctrl and the wheel zooms.",
                    ],
                    Some(Arrangement),
                ),
                GuideMedia::Clips,
            ),
            shows(
                page(
                    "Lanes",
                    &[
                        "Click a lane's name to select it: recordings and imports land there. Its speaker and headphones mute and solo it, and right-clicking the name renames, moves, adds or deletes lanes.",
                    ],
                    Some(Arrangement),
                ),
                GuideMedia::Lanes,
            ),
        ],
    },
    GuideSection {
        title: "Writing notes",
        kind: GuideKind::Tour,
        pages: &[shows(
            page(
                "The piano roll",
                &[
                    "Open a clip to edit its notes here. With the pencil, click to add a note and drag a note to move it; drag its right edge to set its length, and right-click it to delete it. E selects, and Ctrl drags a box around notes with any tool.",
                    "The chooser beside the tools sets the grid, and scale dims the notes outside a key. Tools holds transpose, legato, arpeggiate, and MIDI import and export.",
                    "To make a note slide, keep the button down after drawing it and press S: the note holds to there, then follows the pointer, flat along a row or sliding up and down. Press S again for each new point, Backspace to take one back, and let go to end it. Drag a point to move it, and double-click a note to add one.",
                ],
                Some(Editor),
            ),
            GuideMedia::Roll,
        )],
    },
    GuideSection {
        title: "Mixing",
        kind: GuideKind::Tour,
        pages: &[shows(
            page(
                "The mixer",
                &[
                    "Press 2, or the Mixer tab, to swap the piano roll for the mixer. Each strip is a track with its fader, pan, mute and solo; + makes a new one.",
                    "Select a strip to see its inspector: where it sends its sound, its effects (+ Add effect) and its sends. The cables along the bottom show where everything goes, and what feeds the selected track lights up.",
                    "Pick a cable up and drop it on another track to send it there; the knob on a send's plug sets how much goes. Ctrl and the wheel widen the strips, and the cables' top edge drags to give them more room.",
                ],
                Some(Mixer),
            ),
            GuideMedia::Mixer,
        )],
    },
    GuideSection {
        title: "Routing",
        kind: GuideKind::Tour,
        pages: &[ask(
            "Rack-style or lane-style",
            &[
                "Rack-style, as in FL Studio: each instrument chooses its mixer track with the coloured chip on its row, and lanes are just places to put clips.",
                "Lane-style, as in Reaper or Logic: each lane owns a mixer track, and everything on the lane plays through it. An instrument belongs to one lane.",
                "Choose how new songs start. Any song can be switched in Settings, under Project.",
            ],
            Some(Rack),
            GuideChoice::Routing,
        )],
    },
    GuideSection {
        title: "Recording and exporting",
        kind: GuideKind::Tour,
        pages: &[shows(
            page(
                "Getting sound in and out",
                &[
                    "A MIDI keyboard plays the chosen instrument as soon as it is plugged in; record catches what you play. To record audio, give a mixer track an input in its inspector, then press record.",
                    "Ctrl+E exports the song as a WAV and Ctrl+Shift+E as a MIDI file. Ctrl+S saves; Fontelle also keeps backups as you work.",
                ],
                Some(Transport),
            ),
            GuideMedia::Export,
        )],
    },
    GuideSection {
        title: "Settings and help",
        kind: GuideKind::Tour,
        pages: &[shows(
            page(
                "Where to find things",
                &[
                    "The gear in the browser opens Settings: your MIDI keyboard, folders, plugins, extensions and this song's routing. The ? on the transport bar, or F1, opens this guide and every keyboard shortcut.",
                    "That is the tour. Keep playing with the demo, or close it and start a song of your own.",
                ],
                Some(Browser),
            ),
            GuideMedia::Settings,
        )],
    },
    GuideSection {
        title: "Coming from Reaper",
        kind: GuideKind::ComingFrom,
        pages: &[page(
            "Tracks, items and FX",
            &[
                "Choose lane-style routing and a lane works like a Reaper track: what is on it plays through its own mixer track. Items are clips, and the MIDI editor is the piano roll under the arrangement.",
                "Instruments live in the channel rack rather than on the track: add one there, select it, and draw it onto a lane. Effects go on the mixer track, in its inspector, and sends and outputs are there too.",
                "Space plays, Ctrl+B splits at the marker, and every shortcut can be changed on the shortcuts page.",
            ],
            None,
        )],
    },
    GuideSection {
        title: "Coming from Logic",
        kind: GuideKind::ComingFrom,
        pages: &[page(
            "Tracks, regions and channel strips",
            &[
                "Lane-style routing is Logic's way: each lane is a track with a channel strip in the mixer. Regions are clips, and the piano roll sits under the arrangement.",
                "The Library is the browser on the left. A software instrument is a row of the channel rack, opened in its own window with \u{2261}; drawn onto a lane, it plays through that lane's strip.",
            ],
            None,
        )],
    },
    GuideSection {
        title: "Coming from Ableton",
        kind: GuideKind::ComingFrom,
        pages: &[page(
            "Arrangement, clips and devices",
            &[
                "Fontelle is an arrangement-view DAW; there is no session view. With lane-style routing a lane is a track: its clips play through its mixer track.",
                "An instrument device is a row of the channel rack, with its own window. Audio effects go on mixer tracks rather than in a device chain on the clip's track. The browser on the left is where sounds and files come from.",
            ],
            None,
        )],
    },
    GuideSection {
        title: "Coming from FL Studio",
        kind: GuideKind::ComingFrom,
        pages: &[page(
            "Channel rack, playlist and mixer",
            &[
                "Rack-style routing is FL's: the channel rack is the channel rack, and the coloured chip on a row is its target mixer track. The arrangement is the playlist, where any clip can go on any lane.",
                "Clips hold notes for one or more instruments, like patterns, and prefabs let one set of notes appear in many places. Most of FL's keys work the same: Space, Ctrl+S, P for the pencil, B for paint, E to select, D to delete.",
                "A slide is one note here rather than a second note on top: press S while drawing and it slides wherever you take it, as many times as you like. Each note of a chord can slide somewhere different.",
            ],
            None,
        )],
    },
];

/// The tour: every page of every tour section, in order, as `(section,
/// page)` into [`GUIDE`].
pub fn tour_steps() -> Vec<(usize, usize)> {
    GUIDE
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == GuideKind::Tour)
        .flat_map(|(s, section)| (0..section.pages.len()).map(move |p| (s, p)))
        .collect()
}

// ------------------------------------------------------------ the help page

/// What the help page is called, its close glyph, and its last entry.
pub const HELP_TITLE: &str = "Guide";
pub const HELP_CLOSE: &str = "\u{2715}";
pub const HELP_SHORTCUTS: &str = "Keyboard shortcuts";

const CARD_WIDTH: f32 = 980.0;
const MARGIN: f32 = 24.0;
const PADDING: f32 = 20.0;
const NAV_WIDTH: f32 = 210.0;
const NAV_GAP: f32 = 24.0;
const BLOCK_GAP: f32 = 8.0;
const PAGE_GAP: f32 = 18.0;

/// One title or paragraph on the help page. `paragraph` is `None` for a
/// page's title.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HelpBlock {
    pub page: usize,
    /// `None` for the page's title, or for its animation when `media`.
    pub paragraph: Option<usize>,
    /// The page's animation, between its title and its words.
    pub media: bool,
    pub rect: Rect,
}

/// The shape every guide animation is, wide over tall.
fn media_height(width: f32) -> f32 {
    (width * crate::guide_media::MEDIA_HEIGHT as f32 / crate::guide_media::MEDIA_WIDTH as f32)
        .round()
}

#[derive(Debug, Clone, PartialEq)]
pub struct HelpLayout {
    pub frame: Rect,
    pub title: Rect,
    pub close: Rect,
    /// One entry per section of [`GUIDE`].
    pub nav: Vec<Rect>,
    /// The keyboard shortcuts page, last in the list.
    pub shortcuts: Rect,
    pub section: usize,
    pub body: Rect,
    /// The chosen section's titles and paragraphs that meet the body.
    pub blocks: Vec<HelpBlock>,
    pub content_height: f32,
    pub scroll: f32,
}

/// How wide the help page's paragraphs are in `window` — what the window
/// wraps them at before asking for the layout.
pub fn help_text_width(window: Rect) -> f32 {
    let width = (window.width - 2.0 * MARGIN).clamp(0.0, CARD_WIDTH) - 2.0 * PADDING;
    let nav = NAV_WIDTH.min(width * 0.3);
    (width - nav - NAV_GAP).max(0.0)
}

/// Lays the help page out, showing `section`, whose paragraphs are
/// `heights[page][paragraph]` tall at [`help_text_width`].
pub fn help_layout(
    window: Rect,
    metrics: &Metrics,
    section: usize,
    heights: &[Vec<f32>],
    scroll: f32,
) -> HelpLayout {
    let row = metrics.row_height.round().max(1.0);
    let width = (window.width - 2.0 * MARGIN).clamp(0.0, CARD_WIDTH);
    let height = (window.height - 2.0 * MARGIN).max(0.0);
    let frame = Rect::new(
        (window.x + (window.width - width) / 2.0).round(),
        (window.y + (window.height - height) / 2.0).round(),
        width,
        height,
    )
    .clamped();
    let inner = frame.inset(PADDING).clamped();
    let title_h = (row * 1.35).round();
    let close = Rect::new(inner.right() - title_h, inner.y, title_h, title_h)
        .intersection(&inner)
        .clamped();
    let title = Rect::new(
        inner.x,
        inner.y,
        (close.x - inner.x - 8.0).max(0.0),
        title_h,
    )
    .intersection(&inner)
    .clamped();
    let top = title.bottom() + 14.0;
    let nav_w = NAV_WIDTH.min(inner.width * 0.3).round();
    let nav_h = (row * 1.5).round();
    let nav: Vec<Rect> = (0..GUIDE.len())
        .map(|i| {
            Rect::new(inner.x, top + i as f32 * nav_h, nav_w, nav_h)
                .intersection(&inner)
                .clamped()
        })
        .collect();
    let shortcuts = Rect::new(
        inner.x,
        top + GUIDE.len() as f32 * nav_h + 10.0,
        nav_w,
        nav_h,
    )
    .intersection(&inner)
    .clamped();
    let body_x = inner.x + nav_w + NAV_GAP;
    let body = Rect::new(
        body_x,
        top,
        (inner.right() - body_x).max(0.0),
        (inner.bottom() - top).max(0.0),
    )
    .intersection(&inner)
    .clamped();

    let section = if section < GUIDE.len() { section } else { 0 };
    let heading = (row * 1.4).round();
    let mut placed: Vec<HelpBlock> = Vec::new();
    let mut y = 0.0_f32;
    for (page, paragraphs) in heights.iter().enumerate() {
        if page > 0 {
            y += PAGE_GAP;
        }
        placed.push(HelpBlock {
            page,
            paragraph: None,
            media: false,
            rect: Rect::new(body.x, y, body.width, heading),
        });
        y += heading + BLOCK_GAP;
        // At its own pixels at most: a clip blown up past them is a blur.
        if GUIDE[section]
            .pages
            .get(page)
            .is_some_and(|p| p.media.is_some())
        {
            let width = body.width.min(crate::guide_media::MEDIA_WIDTH as f32);
            let height = media_height(width);
            placed.push(HelpBlock {
                page,
                paragraph: None,
                media: true,
                rect: Rect::new(body.x, y, width, height),
            });
            y += height + BLOCK_GAP;
        }
        for (at, h) in paragraphs.iter().enumerate() {
            placed.push(HelpBlock {
                page,
                paragraph: Some(at),
                media: false,
                rect: Rect::new(body.x, y, body.width, h.max(0.0)),
            });
            y += h.max(0.0) + BLOCK_GAP;
        }
    }
    let content_height = y;
    let scroll = scroll.clamp(0.0, (content_height - body.height).max(0.0));
    let blocks = placed
        .into_iter()
        .map(|mut block| {
            block.rect.y = (block.rect.y + body.y - scroll).round();
            block
        })
        .filter(|block| block.rect.intersects(&body))
        .collect();
    HelpLayout {
        frame,
        title,
        close,
        nav,
        shortcuts,
        section,
        body,
        blocks,
        content_height,
        scroll,
    }
}

pub fn help_scroll_max(layout: &HelpLayout) -> f32 {
    (layout.content_height - layout.body.height).max(0.0)
}

/// The scroll `notches` wheel notches from `scroll`; up is positive.
pub fn help_scrolled(layout: &HelpLayout, scroll: f32, notches: f32) -> f32 {
    (scroll - notches * 48.0).clamp(0.0, help_scroll_max(layout))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpHit {
    Close,
    Section(usize),
    Shortcuts,
    Card,
    Outside,
}

pub fn help_hit(layout: &HelpLayout, x: f32, y: f32) -> HelpHit {
    if layout.close.contains(x, y) {
        return HelpHit::Close;
    }
    if let Some(at) = layout.nav.iter().position(|r| r.contains(x, y)) {
        return HelpHit::Section(at);
    }
    if layout.shortcuts.contains(x, y) {
        return HelpHit::Shortcuts;
    }
    if layout.frame.contains(x, y) {
        HelpHit::Card
    } else {
        HelpHit::Outside
    }
}

// ----------------------------------------------------------------- the tour

/// The tour card's width at most, and what its paragraphs are wrapped at.
pub const TOUR_CARD_WIDTH: f32 = 400.0;
const TOUR_PAD: f32 = 16.0;
const TOUR_GAP: f32 = 14.0;
/// Around what a step points at, so the ring does not sit on its edge.
const SPOT_PAD: f32 = 6.0;

/// The tour's buttons.
pub const TOUR_NEXT: &str = "Next";
pub const TOUR_DONE: &str = "Finish";
pub const TOUR_BACK: &str = "Back";

/// How wide a landscape card's clip is: four fifths of its pixels, which
/// keeps the card short enough to sit beside the arrangement or the editor
/// in a 720-high window.
pub const TOUR_MEDIA_WIDTH: f32 = 432.0;
/// The words beside a clip, a little wider than a plain card's so the card
/// stays as short as the clip.
const TOUR_TEXT_WIDE: f32 = 440.0;

/// Whether a card with a clip lies landscape in `window` — clip left, words
/// right — or stacks the clip over the words.
fn tour_landscape(window: Rect) -> bool {
    window.width - 24.0 >= TOUR_MEDIA_WIDTH + TOUR_TEXT_WIDE + 3.0 * TOUR_PAD
}

/// How wide a tour card's paragraphs are in `window`, for a page with a clip
/// (`media`) or without.
pub fn tour_text_width(window: Rect, media: bool) -> f32 {
    if media && tour_landscape(window) {
        return TOUR_TEXT_WIDE;
    }
    (TOUR_CARD_WIDTH.min(window.width - 24.0) - 2.0 * TOUR_PAD).max(0.0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct TourLayout {
    /// The ring round what the step is about; everything else is dimmed.
    pub spotlight: Option<Rect>,
    pub card: Rect,
    pub title: Rect,
    /// Where the paragraphs go.
    pub body: Rect,
    /// One button per option of the step's choice, stacked.
    pub choices: Vec<Rect>,
    /// Empty on the first step.
    pub back: Rect,
    pub next: Rect,
    pub close: Rect,
    /// "3 of 12 — Arranging", which drops down the list of sections.
    pub steps: Rect,
    /// The page's clip; empty when it has none.
    pub media: Rect,
}

/// Lays the tour card out beside `target` (or in the middle when a step
/// points at nothing), with `body_height` of paragraphs and `choices`
/// options. Beside, never on: the card must not hide what it describes, so
/// it goes below the target, then above, right and left, whichever fits
/// first.
///
/// With `media` the card shows the page's clip: to the left of everything
/// else when the window is wide enough, over the title otherwise.
#[allow(clippy::too_many_arguments)]
pub fn tour_layout(
    window: Rect,
    metrics: &Metrics,
    target: Option<Rect>,
    body_height: f32,
    choices: usize,
    first: bool,
    _last: bool,
    media: bool,
) -> TourLayout {
    let row = metrics.row_height.round().max(1.0);
    let landscape = media && tour_landscape(window);
    let text_w = tour_text_width(window, media);
    let width = if landscape {
        TOUR_MEDIA_WIDTH + text_w + 3.0 * TOUR_PAD
    } else {
        TOUR_CARD_WIDTH.min((window.width - 24.0).max(0.0))
    };
    let media_size = if landscape {
        (TOUR_MEDIA_WIDTH, media_height(TOUR_MEDIA_WIDTH))
    } else if media {
        let w = (width - 2.0 * TOUR_PAD).max(0.0);
        (w, media_height(w))
    } else {
        (0.0, 0.0)
    };
    let head = (row * 1.4).round();
    let button = (row * 1.5).round();
    let choice_h = (row * 1.6).round();
    // The header row (the step chip and ×), the title under it, then the
    // words, the choice's options and the buttons — beside the clip, or
    // under it.
    let column = head
        + head
        + 4.0
        + body_height.max(0.0)
        + choices as f32 * (choice_h + 6.0)
        + TOUR_GAP
        + button;
    let height = if landscape {
        column.max(media_size.1)
    } else if media {
        column + media_size.1 + TOUR_GAP
    } else {
        column
    } + 2.0 * TOUR_PAD;
    let height = height.min((window.height - 24.0).max(0.0));

    let spotlight = target.map(|t| t.inset(-SPOT_PAD).intersection(&window).clamped());
    let fits = |r: Rect| {
        r.x >= window.x + 11.9
            && r.y >= window.y + 11.9
            && r.right() <= window.right() - 11.9
            && r.bottom() <= window.bottom() - 11.9
    };
    let card_at = |x: f32, y: f32| Rect::new(x.round(), y.round(), width, height);
    let card = match spotlight {
        None => card_at(
            window.x + (window.width - width) / 2.0,
            window.y + (window.height - height) / 2.0,
        ),
        Some(spot) => {
            let clamp_x = |x: f32| {
                x.clamp(
                    window.x + 12.0,
                    (window.right() - 12.0 - width).max(window.x + 12.0),
                )
            };
            let clamp_y = |y: f32| {
                y.clamp(
                    window.y + 12.0,
                    (window.bottom() - 12.0 - height).max(window.y + 12.0),
                )
            };
            let candidates = [
                card_at(clamp_x(spot.x), spot.bottom() + TOUR_GAP),
                card_at(clamp_x(spot.x), spot.y - TOUR_GAP - height),
                card_at(spot.right() + TOUR_GAP, clamp_y(spot.y)),
                card_at(spot.x - TOUR_GAP - width, clamp_y(spot.y)),
            ];
            candidates
                .into_iter()
                .find(|c| fits(*c) && !c.intersects(&spot))
                // Nowhere clear: over the corner the target covers least.
                .unwrap_or_else(|| {
                    let corners = [
                        card_at(
                            window.right() - 12.0 - width,
                            window.bottom() - 12.0 - height,
                        ),
                        card_at(window.x + 12.0, window.bottom() - 12.0 - height),
                        card_at(window.right() - 12.0 - width, window.y + 12.0),
                        card_at(window.x + 12.0, window.y + 12.0),
                    ];
                    corners
                        .into_iter()
                        .min_by(|a, b| {
                            let area = |r: Rect| {
                                let i = r.intersection(&spot);
                                i.width.max(0.0) * i.height.max(0.0)
                            };
                            area(*a).total_cmp(&area(*b))
                        })
                        .expect("four corners")
                })
        }
    };

    let card_inner = card.inset(TOUR_PAD).clamped();
    // Everything but the clip goes in `inner`: the right-hand column of a
    // landscape card, or the whole card under the clip's row otherwise.
    let (media, inner) = if landscape {
        let media = Rect::new(card_inner.x, card_inner.y, media_size.0, media_size.1)
            .intersection(&card_inner)
            .clamped();
        let x = media.right() + TOUR_PAD;
        let inner = Rect::new(x, card_inner.y, card_inner.right() - x, card_inner.height)
            .intersection(&card_inner)
            .clamped();
        (media, inner)
    } else {
        (Rect::ZERO, card_inner)
    };
    let close = Rect::new(inner.right() - head, inner.y, head, head)
        .intersection(&inner)
        .clamped();
    let steps_w = (inner.width * 0.46).round();
    let steps = Rect::new(close.x - 6.0 - steps_w, inner.y, steps_w, head)
        .intersection(&inner)
        .clamped();
    // Stacked, the clip sits under the header row.
    let (media, below_head) = if media_size.0 > 0.0 && !landscape {
        let r = Rect::new(inner.x, inner.y + head + 4.0, media_size.0, media_size.1)
            .intersection(&inner)
            .clamped();
        (r, r.bottom() + TOUR_GAP - 4.0 - head)
    } else {
        (media, inner.y)
    };
    // The title sits under the header row (and a stacked clip), the
    // paragraphs under it.
    let title = Rect::new(inner.x, below_head + head, inner.width, head)
        .intersection(&inner)
        .clamped();
    let body = Rect::new(
        inner.x,
        title.bottom() + 4.0,
        inner.width,
        body_height.max(0.0),
    )
    .intersection(&inner)
    .clamped();
    let mut y = body.bottom() + 6.0;
    let choices = (0..choices)
        .map(|_| {
            let r = Rect::new(inner.x, y, inner.width, choice_h)
                .intersection(&inner)
                .clamped();
            y += choice_h + 6.0;
            r
        })
        .collect();
    let bottom = inner.bottom() - button;
    let next_w = 110.0_f32.min(inner.width / 2.0 - 4.0);
    let next = Rect::new(inner.right() - next_w, bottom, next_w, button)
        .intersection(&inner)
        .clamped();
    let back = if first {
        Rect::ZERO
    } else {
        Rect::new(inner.x, bottom, next_w, button)
            .intersection(&inner)
            .clamped()
    };
    TourLayout {
        spotlight,
        card,
        title,
        body,
        choices,
        back,
        next,
        close,
        steps,
        media,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TourHit {
    Back,
    Next,
    Close,
    Steps,
    Choice(usize),
    /// On the card, on nothing — nothing happens.
    Card,
    /// Off the card: the studio's press, as if the tour were not there.
    Through,
}

pub fn tour_hit(layout: &TourLayout, x: f32, y: f32) -> TourHit {
    if !layout.card.contains(x, y) {
        return TourHit::Through;
    }
    if layout.close.contains(x, y) {
        return TourHit::Close;
    }
    if layout.steps.contains(x, y) {
        return TourHit::Steps;
    }
    if layout.next.contains(x, y) {
        return TourHit::Next;
    }
    if layout.back.contains(x, y) {
        return TourHit::Back;
    }
    if let Some(at) = layout.choices.iter().position(|r| r.contains(x, y)) {
        return TourHit::Choice(at);
    }
    TourHit::Card
}
