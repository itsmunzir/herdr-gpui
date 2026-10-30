//! Text that may have been wrapped across rows. The surface carries no
//! soft-wrap flag, so a row is only known to end its line when it stops short
//! of the area's right edge, or when the row below holds no text. Any other
//! pair of rows may be one line, and is read as one when a link may cross
//! between them: a terminal's wrap goes on at the next row's first column, and
//! a program that wraps its own output, as an agent does, may go on after an
//! indent instead.

use super::{MAX_ROW_BYTES, shown};
use herdr_client::protocol::{CellData, FrameData};
use std::ops::{Range, RangeInclusive};

/// The most rows one wrapped link is read across. A longer one is not read,
/// so a screen of unbroken text costs a bounded amount per pointer move.
pub(super) const MAX_WRAPPED_ROWS: u16 = 16;

/// The cells `columns` covers in frame row `row`, if the frame has them.
fn row_cells<'a>(frame: &'a FrameData, columns: &Range<u16>, row: u16) -> Option<&'a [CellData]> {
    let offset = usize::from(row) * usize::from(frame.width);
    frame
        .cells
        .get(offset + usize::from(columns.start)..offset + usize::from(columns.end))
}

/// Whether a cell reads as nothing. A wide glyph's continuation cell is part
/// of that glyph, not a blank.
fn blank(cell: &CellData) -> bool {
    !cell.skip && shown(cell).chars().all(char::is_whitespace)
}

/// Whether the word at `index` runs to the last of `cells`.
pub(super) fn runs_to_end(cells: &[CellData], index: usize) -> bool {
    cells
        .get(index..)
        .is_some_and(|cells| !cells.iter().any(blank))
}

/// Whether the word at `index` is the first text of `cells`: nothing but
/// blanks, if anything, comes before it.
pub(super) fn first_text(cells: &[CellData], index: usize) -> bool {
    cells.get(..=index).is_some_and(|cells| {
        let blanks = indent(cells);
        blanks < cells.len() && !cells[blanks..].iter().any(blank)
    })
}

/// How many blanks start `cells`.
fn indent(cells: &[CellData]) -> usize {
    cells.iter().take_while(|cell| blank(cell)).count()
}

/// Whether the text of row `above` may go on in the row below it. Text only
/// wraps once it fills a row to the right edge. A terminal goes on at the
/// next row's first column; a program that wraps its own output may indent
/// the rest, as an agent does under a list item, and that is taken to go on
/// only from a path: a last word holding a separator.
pub(super) fn continues(frame: &FrameData, columns: &Range<u16>, above: u16) -> bool {
    let Some(below) = above.checked_add(1) else {
        return false;
    };
    let (Some(upper), Some(lower)) = (
        row_cells(frame, columns, above),
        row_cells(frame, columns, below),
    ) else {
        return false;
    };
    if upper.last().is_none_or(blank) {
        return false;
    }
    match indent(lower) {
        0 => true,
        blanks if blanks == lower.len() => false,
        _ => upper
            .iter()
            .rev()
            .take_while(|cell| !blank(cell))
            .any(|cell| shown(cell).contains(['/', '\\'])),
    }
}

/// Consecutive rows of one area read as one text, and where each cell's text
/// starts in it, so a link found in the text maps back to its cells.
pub(super) struct Run<'a> {
    columns: Range<u16>,
    first: u16,
    rows: Vec<&'a [CellData]>,
    /// The blanks each row after the first starts with, which are no part of
    /// the line it continues.
    indents: Vec<usize>,
    /// The byte where each cell's text starts, row after row.
    starts: Vec<usize>,
    pub(super) text: String,
}

impl<'a> Run<'a> {
    /// The text of `rows`, bounded like a single row's; `None` past that
    /// bound or the frame.
    pub(super) fn read(
        frame: &'a FrameData,
        columns: &Range<u16>,
        rows: RangeInclusive<u16>,
    ) -> Option<Self> {
        let first = *rows.start();
        let rows = rows
            .map(|row| row_cells(frame, columns, row))
            .collect::<Option<Vec<_>>>()?;
        let indents: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(index, cells)| if index == 0 { 0 } else { indent(cells) })
            .collect();
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
        Some(Self {
            columns: columns.clone(),
            first,
            rows,
            indents,
            starts,
            text,
        })
    }

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
