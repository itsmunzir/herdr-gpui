//! Text that may have been wrapped across rows. The surface carries no
//! soft-wrap flag, so a row is only known to end its line when it stops short
//! of the area's right edge, or when the row below holds no text. Any other
//! pair of rows may be one line, and is read as one when a link may cross
//! between them: a terminal's wrap goes on at the next row's first column, and
//! a program that wraps its own output, as an agent does, may go on after an
//! indent instead.
//!
//! Words end where `path::plain_path` ends them at a blank: a blank after an
//! escaping backslash, as in a file drop's `a\ b`, is part of the word, even
//! when the backslash ends the row above.

use super::super::selection::separates;
use super::{MAX_ROW_BYTES, path::starts_with_drive, shown};
use herdr_client::protocol::{CellData, FrameData};
use std::ops::{Range, RangeInclusive};

/// The most rows one wrapped link is read across. A longer one is not read,
/// so a screen of unbroken text costs a bounded amount per pointer move.
pub(super) const MAX_WRAPPED_ROWS: u16 = 16;

/// Whether a cell reads as nothing. A wide glyph's continuation cell is part
/// of that glyph, not a blank.
fn blank(cell: &CellData) -> bool {
    !cell.skip && shown(cell).chars().all(char::is_whitespace)
}

/// Whether `cells` start with a drive, as in `C:\`, in whose word a
/// backslash separates folders and escapes nothing.
fn starts_drive(cells: &[CellData]) -> bool {
    starts_with_drive(
        cells
            .iter()
            .filter(|cell| !cell.skip)
            .flat_map(|cell| shown(cell).chars())
            .take(3),
    )
}

/// Where `plain_path` is in its reading at the end of a row: whether a
/// backslash there escapes the next cell, and whether the word running into
/// the edge started with a drive.
#[derive(Clone, Copy)]
struct Reading {
    escape: bool,
    drive: bool,
}

/// Where the words of one row end.
struct Words {
    /// Whether each cell breaks a word: a blank no backslash escapes.
    breaks: Vec<bool>,
    /// The reading at the row's end, which the row below goes on from.
    end: Reading,
}

impl Words {
    /// The words of `cells`, read as `plain_path` reads them: on from the
    /// row above's reading, or afresh, as from the start of a text. A row
    /// read on from another drops its indent from the line, so the indent
    /// leaves the carried word, and whether it started with a drive, as it is.
    fn of(cells: &[CellData], from: Option<Reading>) -> Self {
        let mut breaks = Vec::with_capacity(cells.len());
        let mut indent = from.is_some_and(|reading| !reading.escape);
        let Reading {
            mut escape,
            mut drive,
        } = from.unwrap_or_else(|| Reading {
            escape: false,
            drive: starts_drive(cells),
        });
        for (index, cell) in cells.iter().enumerate() {
            if indent && blank(cell) {
                breaks.push(true);
                continue;
            }
            indent = false;
            if cell.skip || std::mem::take(&mut escape) {
                breaks.push(false);
                continue;
            }
            let first = shown(cell).chars().next().unwrap_or(' ');
            if first == '\\' && !drive {
                escape = true;
            } else if separates(first) {
                drive = cells.get(index + 1..).is_some_and(starts_drive);
            }
            breaks.push(blank(cell));
        }
        Self {
            breaks,
            end: Reading { escape, drive },
        }
    }

    /// How many breaks start the row: its indent.
    fn indent(&self) -> usize {
        self.breaks.iter().take_while(|&&stops| stops).count()
    }

    /// Whether the row holds no text.
    fn empty(&self) -> bool {
        self.indent() == self.breaks.len()
    }

    /// Where the row's last word starts.
    fn last_word(&self) -> usize {
        self.breaks
            .iter()
            .rposition(|&stops| stops)
            .map_or(0, |at| at + 1)
    }

    /// Whether the word at `index` is the row's first text: nothing but
    /// breaks, if anything, comes before it.
    fn first_text(&self, index: usize) -> bool {
        self.breaks.get(..=index).is_some_and(|before| {
            let indent = before.iter().take_while(|&&stops| stops).count();
            indent < before.len() && !before[indent..].contains(&true)
        })
    }

    /// Whether the word at `index` runs to the row's last cell.
    fn runs_to_end(&self, index: usize) -> bool {
        self.breaks
            .get(index..)
            .is_some_and(|after| !after.contains(&true))
    }
}

/// The cells of one pane or popup in its frame, which a link never leaves.
pub(super) struct Area<'a> {
    frame: &'a FrameData,
    columns: Range<u16>,
    rows: Range<u16>,
}

impl<'a> Area<'a> {
    pub(super) fn new(frame: &'a FrameData, columns: Range<u16>, rows: Range<u16>) -> Self {
        Self {
            frame,
            columns,
            rows,
        }
    }

    /// The area's cells in frame row `row`, if it has that row.
    fn cells(&self, row: u16) -> Option<&'a [CellData]> {
        if !self.rows.contains(&row) {
            return None;
        }
        let offset = usize::from(row) * usize::from(self.frame.width);
        self.frame
            .cells
            .get(offset + usize::from(self.columns.start)..offset + usize::from(self.columns.end))
    }

    /// The reading row `row` goes on from: that at the end of the row above,
    /// when that row fills to the right edge, followed down from the first of
    /// the filled rows above it, at most `MAX_WRAPPED_ROWS` up; `None` when
    /// `row` starts afresh.
    fn reading_into(&self, row: u16) -> Option<Reading> {
        let filled = |row: u16| {
            self.cells(row)
                .and_then(<[CellData]>::last)
                .is_some_and(|cell| !blank(cell))
        };
        let mut first = row;
        while row - first < MAX_WRAPPED_ROWS && first.checked_sub(1).is_some_and(&filled) {
            first -= 1;
        }
        (first..row).try_fold(None, |reading, row| {
            Some(Some(Words::of(self.cells(row)?, reading).end))
        })?
    }

    fn words(&self, row: u16) -> Option<Words> {
        Some(Words::of(self.cells(row)?, self.reading_into(row)))
    }

    /// The rows the text at cell `index` of `row` may span, read as one
    /// line. Most rows end their line before the right edge and are read
    /// alone; a word that starts its row, or runs into the right edge, where
    /// a neighbouring row may continue it, reads the rows it may span, at
    /// most `MAX_WRAPPED_ROWS`.
    pub(super) fn around(&self, row: u16, index: usize) -> RangeInclusive<u16> {
        let Some(words) = self.words(row) else {
            return row..=row;
        };
        let mut first = row;
        if words.first_text(index) {
            while first > self.rows.start
                && row - first + 1 < MAX_WRAPPED_ROWS
                && self.continues(first - 1)
            {
                first -= 1;
            }
        }
        let mut last = row;
        if words.runs_to_end(index) {
            while last + 1 < self.rows.end
                && last - first + 1 < MAX_WRAPPED_ROWS
                && self.continues(last)
            {
                last += 1;
            }
        }
        first..=last
    }

    /// Whether the text a run starting at row `first` begins with may be the
    /// tail of the row above.
    pub(super) fn continued_above(&self, first: u16) -> bool {
        first > self.rows.start && self.continues(first - 1)
    }

    /// Whether the text of row `above` may go on in the row below it. Text
    /// only wraps once it fills a row to the right edge. A terminal goes on
    /// at the next row's first column; a program that wraps its own output
    /// may indent the rest, as an agent does under a list item, and that is
    /// taken to go on only from a path.
    fn continues(&self, above: u16) -> bool {
        let Some(below) = above.checked_add(1) else {
            return false;
        };
        let (Some(upper), Some(lower)) = (self.cells(above), self.words(below)) else {
            return false;
        };
        if upper.last().is_none_or(blank) || lower.empty() {
            return false;
        }
        lower.indent() == 0 || self.ends_in_path(above)
    }

    /// Whether the word ending row `row` holds a path separator, there or on
    /// the rows above that it goes on from, as a long file name broken over
    /// several rows only names its folders on the first.
    fn ends_in_path(&self, mut row: u16) -> bool {
        for _ in 0..MAX_WRAPPED_ROWS {
            let (Some(cells), Some(words)) = (self.cells(row), self.words(row)) else {
                return false;
            };
            let start = words.last_word();
            let word = cells.get(start..).unwrap_or_default();
            if word.iter().any(|cell| shown(cell).contains(['/', '\\'])) {
                return true;
            }
            // The word goes on from the row above only as this row's first
            // text, under a row filled to the right edge.
            let filled_above = row
                .checked_sub(1)
                .and_then(|above| self.cells(above))
                .and_then(<[CellData]>::last)
                .is_some_and(|cell| !blank(cell));
            if start != words.indent() || !filled_above {
                return false;
            }
            row -= 1;
        }
        false
    }

    /// The text of `rows`, bounded like a single row's; `None` past that
    /// bound or the area.
    pub(super) fn read(&self, rows: RangeInclusive<u16>) -> Option<Run<'a>> {
        let first = *rows.start();
        let rows = rows
            .map(|row| self.cells(row))
            .collect::<Option<Vec<_>>>()?;
        // Read as `plain_path` reads the text: the first row afresh, and
        // each next row on from the one before.
        let mut reading = None;
        let mut indents = Vec::with_capacity(rows.len());
        for cells in &rows {
            let words = Words::of(cells, reading);
            indents.push(if reading.is_some() { words.indent() } else { 0 });
            reading = Some(words.end);
        }
        let mut text = String::new();
        let mut starts = Vec::with_capacity(rows.iter().map(|cells| cells.len()).sum());
        for (cells, &indent) in rows.iter().zip(&indents) {
            for (index, cell) in cells.iter().enumerate() {
                starts.push(text.len());
                if cell.skip || index < indent {
                    continue;
                }
                let symbol = shown(cell);
                if text.len() + symbol.len() > MAX_ROW_BYTES {
                    return None;
                }
                text.push_str(symbol);
            }
        }
        Some(Run {
            columns: self.columns.clone(),
            first,
            rows,
            indents,
            starts,
            text,
        })
    }
}

/// Consecutive rows of one area read as one text, and where each cell's text
/// starts in it, so a link found in the text maps back to its cells.
pub(super) struct Run<'a> {
    columns: Range<u16>,
    first: u16,
    rows: Vec<&'a [CellData]>,
    /// The breaks each row after the first starts with, which are no part of
    /// the line it continues.
    indents: Vec<usize>,
    /// The byte where each cell's text starts, row after row.
    starts: Vec<usize>,
    pub(super) text: String,
}

impl Run<'_> {
    /// Where the text of cell `index` of frame row `row` starts.
    pub(super) fn start_of(&self, row: u16, index: usize) -> Option<usize> {
        let width = usize::from(self.columns.end - self.columns.start);
        let offset = usize::from(row.checked_sub(self.first)?) * width;
        self.starts.get(offset + index).copied()
    }

    /// The frame columns the text in `range` covers on each row, top to
    /// bottom, leaving out rows it does not reach.
    pub(super) fn segments(&self, range: &Range<usize>) -> Vec<(u16, Range<u16>)> {
        let width = usize::from(self.columns.end - self.columns.start);
        // Cell indexes fit the row, whose width is a `u16`.
        let columns = |cells: Range<usize>| {
            self.columns.start + cells.start as u16..self.columns.start + cells.end as u16
        };
        (self.first..=u16::MAX)
            .zip(self.rows.iter().zip(&self.indents))
            .zip(self.starts.chunks(width.max(1)))
            .filter_map(|((row, (cells, &indent)), starts)| {
                let on = |i: &usize| !cells[*i].skip && range.contains(&starts[*i]);
                let first = (indent..cells.len()).find(on)?;
                let last = (0..cells.len()).rev().find(on)?;
                // A wide character's continuation cells belong to the link too.
                let end = (last + 1..cells.len())
                    .find(|&i| !cells[i].skip)
                    .unwrap_or(cells.len());
                Some((row, columns(first..end)))
            })
            .collect()
    }
}
