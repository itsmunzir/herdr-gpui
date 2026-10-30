//! Text a terminal may have wrapped across rows. The surface carries no
//! soft-wrap flag, so a row is only known to end its line when it stops short
//! of the area's right edge, or when the row below starts with a blank. Any
//! other pair of rows may be one line, and is read as one when a link may
//! cross between them.

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

/// Whether `cells` from `index` to the end read without a blank.
pub(super) fn filled_from(cells: &[CellData], index: usize) -> bool {
    cells
        .get(index..)
        .is_some_and(|cells| !cells.iter().any(blank))
}

/// Whether `cells` up to and including `index` read without a blank.
pub(super) fn filled_to(cells: &[CellData], index: usize) -> bool {
    cells
        .get(..=index)
        .is_some_and(|cells| !cells.iter().any(blank))
}

/// Whether the text of row `above` may go on in the row below it: a terminal
/// wraps a line only once it fills a row to the right edge, and the next row
/// then starts with the rest of it.
pub(super) fn continues(frame: &FrameData, columns: &Range<u16>, above: u16) -> bool {
    let Some(below) = above.checked_add(1) else {
        return false;
    };
    let ends_filled = row_cells(frame, columns, above)
        .and_then(<[CellData]>::last)
        .is_some_and(|cell| !blank(cell));
    ends_filled
        && row_cells(frame, columns, below)
            .and_then(<[CellData]>::first)
            .is_some_and(|cell| !blank(cell))
}

/// Consecutive rows of one area read as one text, and where each cell's text
/// starts in it, so a link found in the text maps back to its cells.
pub(super) struct Run<'a> {
    columns: Range<u16>,
    first: u16,
    rows: Vec<&'a [CellData]>,
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
        let mut text = String::new();
        let mut starts = Vec::with_capacity(rows.iter().map(|cells| cells.len()).sum());
        for cell in rows.iter().copied().flatten() {
            starts.push(text.len());
            if cell.skip {
                continue;
            }
            let symbol = shown(cell);
            if text.len() + symbol.len() > MAX_ROW_BYTES {
                return None;
            }
            text.push_str(symbol);
        }
        Some(Self {
            columns: columns.clone(),
            first,
            rows,
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
            .zip(&self.rows)
            .zip(self.starts.chunks(width.max(1)))
            .filter_map(|((row, cells), starts)| {
                let on = |i: &usize| !cells[*i].skip && range.contains(&starts[*i]);
                let first = (0..cells.len()).find(on)?;
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
