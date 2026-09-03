//! The piano roll's Tools panel.
//!
//! Asked for as *"a tools tab which has various tools such as a randomizer, a
//! transposer — and it should also let you transpose all of your selections
//! velocity or pan or whatever all at once adding or subtracting a value —
//! and importantly a import/midi and import/fsc option"*.
//!
//! # The shape, and why it is this one
//!
//! **Rows of a name and a value, where a click steps the value forward and a
//! Ctrl+click steps it back.** That is the settings tab's shape, already in
//! this window and already understood; no text field is involved, because
//! there is not one in this window and a value you can reach in a handful of
//! clicks is quicker than one you have to type anyway. What the settings tab
//! does not have is *action* rows — "Transpose", "Randomize", "Import MIDI" —
//! which are buttons rather than values, and [`Tools::action`] is what tells
//! the two apart so a press on one can never quietly step the other.
//!
//! Everything here is pure: the panel is geometry, the settings are numbers,
//! and what a tool *does* is a list of [`RollEdit`]s. Reading a file is not
//! something this crate may do (INVARIANT 2), so the two import rows name the
//! action and produce no edit — the window carries them out.

use fontelle_model::{Arena, Note, RandomMode, RandomSpec, randomised};
use fontelle_types::NoteId;

use super::piano_roll::{LANE_PROPERTIES, LaneProperty, RollEdit};
use crate::layout::Rect;
use crate::theme::Metrics;

/// One row of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRow {
    /// A section title. Nothing to set, and a click does nothing — it is what
    /// makes "Amount" read as the *offset's* amount rather than the
    /// randomizer's, when both are on the same panel.
    Heading(&'static str),
    /// How far [`ToolAction::Transpose`] moves things, in semitones.
    Transpose,
    /// Runs it.
    TransposeNow,
    /// Which property the offset and the randomizer act on. *"velocity or pan
    /// or whatever"* — this is "or whatever".
    Property,
    /// How much [`ToolAction::Add`] and [`ToolAction::Subtract`] move it by.
    Amount,
    AddNow,
    SubtractNow,
    /// How far the randomizer strays.
    RandomAmount,
    /// Which sense it strays in — see [`RandomMode`].
    RandomMode,
    RandomizeNow,
    ImportMidiNow,
    ImportScoreNow,
}

/// What an action row does when it is pressed.
///
/// Separate from [`ToolRow`] because the *window* switches on this and has no
/// business knowing which row of the panel it came off — and because two rows
/// (add and subtract) are the same tool pointed two ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAction {
    Transpose,
    Add,
    Subtract,
    Randomize,
    /// Open the MIDI file browser. Not an edit — see this module's own note.
    ImportMidi,
    /// Open the FL score browser.
    ImportScore,
}

/// Every row, in the order the panel draws them.
///
/// Grouped by what you are doing rather than by what kind of control it is:
/// the thing that moves notes in pitch, then the thing that changes a
/// property, then the thing that makes a mess of one, then the two that bring
/// something in from outside.
pub const TOOL_ROWS: [ToolRow; 15] = [
    ToolRow::Heading("Transpose"),
    ToolRow::Transpose,
    ToolRow::TransposeNow,
    ToolRow::Heading("Adjust"),
    ToolRow::Property,
    ToolRow::Amount,
    ToolRow::AddNow,
    ToolRow::SubtractNow,
    ToolRow::Heading("Randomize"),
    ToolRow::RandomAmount,
    ToolRow::RandomMode,
    ToolRow::RandomizeNow,
    ToolRow::Heading("Import"),
    ToolRow::ImportMidiNow,
    ToolRow::ImportScoreNow,
];

/// How far a transpose goes either way. Two octaves is as far as anybody moves
/// a part; past it you have chosen the wrong octave.
const MAX_TRANSPOSE: i32 = 24;

/// The offsets the amount row walks through.
///
/// A ladder rather than a step of one, because stepping by one would be twenty
/// clicks to reach twenty — the difference between a tool and a chore. The
/// values are the ones people actually use: a nudge, a noticeable change, a
/// third of the range, half of it.
const AMOUNT_LADDER: [i32; 12] = [1, 2, 3, 5, 8, 10, 15, 20, 25, 32, 48, 64];

/// The same idea for the randomizer's dial, which is a percentage and so may
/// legitimately be nothing at all.
const RANDOM_LADDER: [i32; 11] = [0, 5, 10, 15, 20, 25, 30, 40, 50, 75, 100];

/// What the Tools panel is set to.
///
/// Window state, not document state: which property you last adjusted is no
/// more part of a song than which tool is selected is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tools {
    pub semitones: i32,
    /// Which property [`ToolAction::Add`], [`ToolAction::Subtract`] and
    /// [`ToolAction::Randomize`] all act on.
    pub property: LaneProperty,
    /// The **magnitude** of the offset. The sign is the action's — two rows
    /// pointed two ways, rather than a signed number and one row, because
    /// "add ten" and "take ten off" are two things you do and not two values
    /// of one thing.
    pub amount: i32,
    pub random_amount: i32,
    pub random_mode: RandomMode,
    /// Stepped on every roll, so pressing Randomize twice rolls twice — and
    /// so the same panel in the same state is reproducible, which is what
    /// makes the whole thing testable.
    pub seed: u64,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            // An octave: the transpose everybody reaches for first.
            semitones: 12,
            // Velocity, for the same reason the property lane opens on it.
            property: LaneProperty::Velocity,
            amount: 10,
            // Enough to hear, little enough not to destroy a phrase.
            random_amount: 20,
            random_mode: RandomMode::Around,
            seed: 1,
        }
    }
}

impl Tools {
    /// The name in the row's left-hand column.
    ///
    /// Short enough for a panel that hangs off a chip, and a **verb** on every
    /// action row — a row that says "Transpose" beside a row that says
    /// "Transpose" is two read-outs; "Transpose selection" is a button.
    pub fn label(&self, row: ToolRow) -> String {
        match row {
            ToolRow::Heading(title) => title.to_string(),
            ToolRow::Transpose => "Semitones".to_string(),
            ToolRow::TransposeNow => "Transpose selection".to_string(),
            ToolRow::Property => "Property".to_string(),
            ToolRow::Amount => "Amount".to_string(),
            ToolRow::AddNow => format!("Add {} to selection", self.amount),
            ToolRow::SubtractNow => format!("Take {} off selection", self.amount),
            ToolRow::RandomAmount => "Strength".to_string(),
            ToolRow::RandomMode => "Sense".to_string(),
            ToolRow::RandomizeNow => "Randomize selection".to_string(),
            ToolRow::ImportMidiNow => "Import MIDI file\u{2026}".to_string(),
            ToolRow::ImportScoreNow => "Import FL score\u{2026}".to_string(),
        }
    }

    /// What it is set to, in the row's right-hand column. Empty for a heading
    /// and for an action, both of which are all name.
    pub fn value(&self, row: ToolRow) -> String {
        match row {
            // With its sign, always: "+0" against "0" is the difference
            // between a number that can go either way and one that might not.
            ToolRow::Transpose => format!("{:+} st", self.semitones),
            ToolRow::Property => self.property.label().to_string(),
            ToolRow::Amount => self.amount.to_string(),
            ToolRow::RandomAmount => format!("{}%", self.random_amount),
            ToolRow::RandomMode => self.random_mode.label().to_string(),
            _ => String::new(),
        }
    }

    /// What pressing this row does, for the rows that do something rather
    /// than hold something.
    pub fn action(&self, row: ToolRow) -> Option<ToolAction> {
        Some(match row {
            ToolRow::TransposeNow => ToolAction::Transpose,
            ToolRow::AddNow => ToolAction::Add,
            ToolRow::SubtractNow => ToolAction::Subtract,
            ToolRow::RandomizeNow => ToolAction::Randomize,
            ToolRow::ImportMidiNow => ToolAction::ImportMidi,
            ToolRow::ImportScoreNow => ToolAction::ImportScore,
            _ => return None,
        })
    }

    /// What a hover tip says about the row.
    pub fn tip(&self, row: ToolRow) -> Option<&'static str> {
        Some(match row {
            ToolRow::Heading(_) => return None,
            ToolRow::Transpose => "How far to move notes \u{2014} click to step, Ctrl+click back",
            ToolRow::TransposeNow => "Move every selected note by that many semitones",
            ToolRow::Property => "Which property Adjust and Randomize act on",
            ToolRow::Amount => "How much to add or take off",
            ToolRow::AddNow => "Add it to every selected note, keeping their differences",
            ToolRow::SubtractNow => "Take it off every selected note",
            ToolRow::RandomAmount => "How far the randomizer may stray. 0% leaves it alone",
            ToolRow::RandomMode => {
                "Around: wobble each note. Anywhere: forget what was there"
            }
            ToolRow::RandomizeNow => "Roll again \u{2014} press it twice for a different answer",
            ToolRow::ImportMidiNow => "Bring in a .mid file",
            ToolRow::ImportScoreNow => "Bring in an FL Studio .fsc score",
        })
    }

    /// Steps this row's value one place in the direction `delta` says.
    ///
    /// **A choice wraps and a number stops**, the same rule the settings tab
    /// follows: six properties are a set with no ends, so stopping at one
    /// would mean knowing to go back through; ±24 semitones are the ends of a
    /// range, and running off one and arriving at the other is a control that
    /// cannot be trusted to a held press.
    pub fn nudge(&mut self, row: ToolRow, delta: i32) {
        if delta == 0 {
            return;
        }
        let step = delta.signum();
        match row {
            // Neither is a value. An action row that also stepped something
            // would change the amount you were about to apply, on the press
            // that applied it.
            ToolRow::Heading(_) => {}
            _ if self.action(row).is_some() => {}
            ToolRow::Transpose => {
                self.semitones = (self.semitones + step).clamp(-MAX_TRANSPOSE, MAX_TRANSPOSE);
            }
            ToolRow::Property => {
                let at = LANE_PROPERTIES
                    .iter()
                    .position(|p| *p == self.property)
                    .unwrap_or(0) as i32;
                let next = (at + step).rem_euclid(LANE_PROPERTIES.len() as i32) as usize;
                self.property = LANE_PROPERTIES[next];
            }
            ToolRow::Amount => self.amount = ladder_step(&AMOUNT_LADDER, self.amount, step),
            ToolRow::RandomAmount => {
                self.random_amount = ladder_step(&RANDOM_LADDER, self.random_amount, step);
            }
            ToolRow::RandomMode => self.random_mode = self.random_mode.next(),
            // Unreachable: every remaining variant is an action, caught above.
            _ => {}
        }
    }

    /// The edits `action` makes to `selection`.
    ///
    /// `&mut self` because rolling the randomizer steps the seed: "give me
    /// another one" is what pressing it a second time means.
    pub fn run(
        &mut self,
        action: ToolAction,
        selection: &[NoteId],
        notes: &Arena<NoteId, Note>,
    ) -> Vec<RollEdit> {
        // An edit over an empty list would land in the history as an entry
        // that did nothing, which is an undo that appears to do nothing.
        if selection.is_empty() {
            return Vec::new();
        }
        match action {
            ToolAction::Transpose => {
                if self.semitones == 0 {
                    return Vec::new();
                }
                vec![RollEdit::Move {
                    ids: selection.to_vec(),
                    tick_delta: 0,
                    key_delta: self.semitones as i16,
                }]
            }
            ToolAction::Add | ToolAction::Subtract => {
                let sign = if action == ToolAction::Add { 1 } else { -1 };
                vec![RollEdit::NudgeProperty {
                    ids: selection.to_vec(),
                    property: self.property.property(),
                    delta: self.amount * sign,
                }]
            }
            ToolAction::Randomize => {
                let property = self.property.property();
                // Read from the notes that are there: `Around` is a wobble
                // around what is already written, and a randomizer that
                // started from zero would flatten the phrase and then jitter
                // the flat version.
                let starting: Vec<i32> = selection
                    .iter()
                    .filter_map(|id| notes.get(*id).map(|note| property.get(note)))
                    .collect();
                if starting.len() != selection.len() {
                    // A stale selection naming a note that has gone. Refusing
                    // is right: the alternative writes the third note's value
                    // onto the fourth.
                    return Vec::new();
                }
                let values = randomised(
                    &starting,
                    property,
                    RandomSpec {
                        amount: self.random_amount,
                        mode: self.random_mode,
                    },
                    self.seed,
                );
                self.seed = self.seed.wrapping_add(1);
                vec![RollEdit::SetPropertyEach {
                    ids: selection.to_vec(),
                    property,
                    values,
                }]
            }
            // The window's to carry out — this crate may not read a file.
            ToolAction::ImportMidi | ToolAction::ImportScore => Vec::new(),
        }
    }
}

/// `value` moved one rung along `ladder`, stopping at both ends.
///
/// A value that is not on the ladder — which cannot happen through the panel,
/// but can through a settings file somebody edited — lands on the nearest rung
/// in the direction of travel rather than jumping to the bottom.
fn ladder_step(ladder: &[i32], value: i32, step: i32) -> i32 {
    if step > 0 {
        ladder
            .iter()
            .copied()
            .find(|rung| *rung > value)
            .unwrap_or_else(|| ladder.last().copied().unwrap_or(value))
    } else {
        ladder
            .iter()
            .copied()
            .rev()
            .find(|rung| *rung < value)
            .unwrap_or_else(|| ladder.first().copied().unwrap_or(value))
    }
}

// ------------------------------------------------------------- geometry ---

/// The Tools panel, laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolsPanel {
    /// The whole panel — for the background, and for "did the click miss".
    pub frame: Rect,
    /// One rectangle per row, in [`TOOL_ROWS`] order. A row with no room
    /// left is an **empty** rectangle,
    /// which draws as nothing and hit-tests as absent — the list stays the
    /// same length either way, so a caller can index it.
    pub rows: Vec<(ToolRow, Rect)>,
}

/// A little air around the rows.
const PANEL_PAD: f32 = 4.0;

/// Wide enough for "Take 64 off selection" and a value beside it.
const PANEL_WIDTH: f32 = 188.0;

/// Drops the panel from `chip`, kept inside `bounds`.
///
/// Below the chip by preference — a panel over the control it belongs to hides
/// what it is doing — then above it, and if it fits in neither it is pinned to
/// the top of `bounds` and **clipped**. That last case is where this differs
/// from every other menu in this window, which draw nothing rather than draw
/// part of themselves: the Tools panel is the only way to reach the importers,
/// so a short window must not be a window with no importers.
pub fn tools_panel_layout(chip: Rect, bounds: Rect, metrics: &Metrics) -> ToolsPanel {
    let rows = TOOL_ROWS;
    let row_height = metrics.row_height.max(1.0);
    let width = PANEL_WIDTH.min(bounds.width);
    let height = row_height * rows.len() as f32 + PANEL_PAD * 2.0;

    if bounds.is_empty() || width <= 0.0 {
        return ToolsPanel {
            frame: Rect::ZERO,
            rows: rows.iter().map(|row| (*row, Rect::ZERO)).collect(),
        };
    }

    let below = chip.bottom();
    let y = if below + height <= bounds.bottom() {
        below
    } else if chip.y - height >= bounds.y {
        chip.y - height
    } else {
        // Neither. Against the top, and as much of it as fits.
        bounds.y
    };
    let x = chip
        .x
        .clamp(bounds.x, (bounds.right() - width).max(bounds.x));
    let frame = Rect::new(x, y, width, height).intersection(&bounds);

    let laid = rows
        .iter()
        .copied()
        .enumerate()
        .map(|(index, row)| {
            let rect = Rect::new(
                frame.x + PANEL_PAD,
                y + PANEL_PAD + row_height * index as f32,
                (frame.width - PANEL_PAD * 2.0).max(0.0),
                row_height,
            );
            // Clipped to the frame rather than dropped, so a row that ran off
            // the end of a short panel is an empty rectangle: it draws as
            // nothing and hit-tests as absent, and the list keeps its length.
            (row, rect.intersection(&frame).clamped())
        })
        .collect();

    ToolsPanel { frame, rows: laid }
}

/// Which row `(x, y)` is on, if it is on one.
///
/// A **heading** hit-tests as itself rather than as nothing, unlike a greyed
/// menu entry: the caller has to know the press landed on the panel so it does
/// not also reach the grid underneath, and `Tools::nudge` already does nothing
/// to a heading.
pub fn tools_panel_hit(panel: &ToolsPanel, x: f32, y: f32) -> Option<ToolRow> {
    panel
        .rows
        .iter()
        .find(|(_, rect)| !rect.is_empty() && rect.contains(x, y))
        .map(|(row, _)| *row)
}
