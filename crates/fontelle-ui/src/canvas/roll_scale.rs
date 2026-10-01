//! The piano roll's scale tool: which rows a key dims, which key each note
//! fits to, and the two menus the toolbar's chips drop.
//!
//! > *"a scale tool so you can chose between any note and the mode or
//! > whatever and it will snap all of your notes to that scale and dim out
//! > all the lanes that arent in that scale."*
//!
//! The key itself is the song's (`Project::key`); this is only what the roll
//! does with it. The catalogue is `fontelle_types::SCALES`.

use fontelle_model::{Arena, Note};
use fontelle_types::{KeyScale, NoteId, PITCH_NAMES, SCALES, ScaleFamily, fit_to_scale};

use super::KeyStyle;
use super::menu::{CHOSEN_MARK, MenuEntry};

/// A key as the roll uses it: the root's pitch class and the twelve-bit mask
/// of the pitch classes in it (bit 0 is C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollScale {
    pub root: u8,
    pub mask: u16,
}

impl RollScale {
    /// `None` for a scale this build does not know.
    pub fn of(key: &KeyScale) -> Option<Self> {
        key.mask().map(|mask| Self {
            root: key.root % 12,
            mask,
        })
    }

    pub fn contains(self, key: u8) -> bool {
        self.mask & (1 << (key % 12)) != 0
    }
}

/// How one row of the grid is shaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowShade {
    Plain,
    /// A black key's row, when there is no scale — the keyboard's own
    /// pattern, which a scale replaces.
    Accidental,
    /// Outside the scale: dimmed.
    OutOfScale,
    /// The scale's root, in every octave, so the key can be read at a glance.
    Root,
    /// A key the instrument does not play. Wins over everything: a row that
    /// makes no sound says so whatever the scale thinks of it.
    Dead,
}

/// The one decision about a row's shade, shared by the grid and the tests.
pub fn row_shade(key: u8, plays: bool, style: KeyStyle, scale: Option<RollScale>) -> RowShade {
    if !plays {
        return RowShade::Dead;
    }
    match scale {
        Some(scale) if key % 12 == scale.root => RowShade::Root,
        Some(scale) if !scale.contains(key) => RowShade::OutOfScale,
        Some(_) => RowShade::Plain,
        None if style == KeyStyle::Piano && matches!(key % 12, 1 | 3 | 6 | 8 | 10) => {
            RowShade::Accidental
        }
        None => RowShade::Plain,
    }
}

/// The notes that are out of the scale and the key each fits to — `only`
/// when it names any, every note otherwise. Notes already in are left out, so
/// fitting twice is nothing the second time.
pub fn scale_fit(notes: &Arena<NoteId, Note>, only: &[NoteId], mask: u16) -> Vec<(NoteId, u8)> {
    notes
        .iter()
        .filter(|(id, _)| only.is_empty() || only.contains(id))
        .filter_map(|(id, note)| {
            let key = fit_to_scale(note.key, mask);
            (key != note.key).then_some((id, key))
        })
        .collect()
}

/// The sliding notes whose paths land outside the scale, and the path each
/// fits to — measured from the key [`scale_fit`] gives the note, so the two
/// together put every point it reaches in the scale.
pub fn scale_fit_paths(
    notes: &Arena<NoteId, Note>,
    only: &[NoteId],
    mask: u16,
) -> Vec<(NoteId, Vec<fontelle_model::PathPoint>)> {
    notes
        .iter()
        .filter(|(id, note)| note.has_path() && (only.is_empty() || only.contains(id)))
        .filter_map(|(id, note)| {
            let key = i16::from(fit_to_scale(note.key, mask));
            let path: Vec<fontelle_model::PathPoint> = note
                .path
                .iter()
                .map(|point| {
                    let reached = (i16::from(note.key) + i16::from(point.offset)).clamp(0, 127);
                    let fitted = i16::from(fit_to_scale(reached as u8, mask));
                    fontelle_model::PathPoint {
                        at: point.at,
                        offset: (fitted - key).clamp(-128, 127) as i8,
                    }
                })
                .collect();
            (path != note.path).then_some((id, path))
        })
        .collect()
}

/// How many characters the scale chip shows before it cuts a name short.
const CHIP_CHARS: usize = 14;

/// What the scale chip says: the scale's name, or *scale* when there is
/// none, with the caret every chip that drops a list carries.
pub fn scale_caption(key: Option<&KeyScale>) -> String {
    let name = key
        .and_then(|key| fontelle_types::scale(&key.scale))
        .map_or_else(|| "scale".to_string(), |s| s.name.to_lowercase());
    let name = if name.chars().count() > CHIP_CHARS {
        let cut: String = name.chars().take(CHIP_CHARS - 1).collect();
        format!("{}\u{2026}", cut.trim_end())
    } else {
        name
    };
    format!("{name} \u{25be}")
}

/// What the root chip says.
pub fn root_caption(root: u8) -> String {
    format!("{} \u{25be}", PITCH_NAMES[usize::from(root % 12)])
}

/// The twelve roots, the current one ticked.
pub fn root_menu(current: u8) -> Vec<MenuEntry> {
    PITCH_NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            MenuEntry::new(if i == usize::from(current % 12) {
                format!("{CHOSEN_MARK}{name}")
            } else {
                format!("   {name}")
            })
        })
        .collect()
}

/// What a row of the scale menu means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleMenuRow {
    Heading,
    /// Turn the scale off. The notes stay where they are.
    NoScale,
    /// Fit the notes to the scale that is on — for notes drawn with Alt, or
    /// brought in from elsewhere since.
    FitNotes,
    Scale(&'static str),
}

/// The scale chooser's rows for what has been typed, and what each means.
///
/// Every scale under its family's heading; typing keeps the scales any of
/// whose names match (`fontelle_types::scale_matches`) and the headings of
/// the families they are in.
pub fn scale_menu(query: &str, current: Option<&KeyScale>) -> (Vec<MenuEntry>, Vec<ScaleMenuRow>) {
    let query = query.trim();
    let mut entries = vec![MenuEntry::disabled(if query.is_empty() {
        "Scales \u{2014} type to filter".to_string()
    } else {
        format!("Scales \u{2014} {query}")
    })];
    let mut rows = vec![ScaleMenuRow::Heading];

    if query.is_empty() {
        entries.push(MenuEntry::new(match current {
            None => format!("{CHOSEN_MARK}No scale"),
            Some(_) => "   No scale".to_string(),
        }));
        rows.push(ScaleMenuRow::NoScale);
        let fit = format!(
            "   Fit notes to {}",
            current.map_or("the scale".into(), KeyScale::label)
        );
        entries.push(if current.is_some() {
            MenuEntry::new(fit)
        } else {
            MenuEntry::disabled(fit)
        });
        rows.push(ScaleMenuRow::FitNotes);
    }

    let chosen = current.map(|key| key.scale.as_str());
    let mut found = false;
    for family in ScaleFamily::ALL {
        let mut heading = false;
        for scale in SCALES
            .iter()
            .filter(|s| s.family == family && fontelle_types::scale_matches(s, query))
        {
            if !heading {
                entries.push(MenuEntry::disabled(family.label()).after_rule());
                rows.push(ScaleMenuRow::Heading);
                heading = true;
            }
            entries.push(MenuEntry::new(if chosen == Some(scale.id) {
                format!("{CHOSEN_MARK}{}", scale.name)
            } else {
                format!("   {}", scale.name)
            }));
            rows.push(ScaleMenuRow::Scale(scale.id));
            found = true;
        }
    }
    if !found {
        entries.push(MenuEntry::disabled("nothing matches"));
        rows.push(ScaleMenuRow::Heading);
    }
    (entries, rows)
}
