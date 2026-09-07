use ovrcr_protocol::{HistoryCell, HistoryRow, HistoryRows, HistorySnapshotId, SessionId};
use ovrcr_terminal::vt100;
use std::io;

pub(super) const MAX_COPY_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HistoryCopyPoint {
    pub row: u32,
    pub col: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryCopyRange {
    pub session: SessionId,
    pub snapshot: HistorySnapshotId,
    pub anchor: HistoryCopyPoint,
    pub cursor: HistoryCopyPoint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryCopyCompletion {
    pub id: u64,
    pub range: HistoryCopyRange,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryCopyJob {
    pub id: u64,
    pub range: HistoryCopyRange,
    start: HistoryCopyPoint,
    end: HistoryCopyPoint,
    next_row: u32,
    next_col: u16,
    row_width: Option<u16>,
    row_wrapped: Option<bool>,
    output: String,
    pending_blanks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CopyPoint {
    pub row: u16,
    pub col: u16,
}

pub struct CopySelection {
    pub session: SessionId,
    pub screen: vt100::Screen,
    pub cursor: CopyPoint,
    pub anchor: Option<CopyPoint>,
}

#[derive(Clone, Copy)]
pub enum CopyMotion {
    Left,
    Right,
    Up,
    Down,
    RowStart,
    RowEnd,
    First,
    Last,
}

impl CopySelection {
    pub fn capture(session: SessionId, screen: &vt100::Screen) -> Self {
        let (row, col) = screen.cursor_position();
        let (rows, cols) = screen.size();
        let cursor = normalize_point(
            screen,
            CopyPoint {
                row: row.min(rows.saturating_sub(1)),
                col: col.min(cols.saturating_sub(1)),
            },
        );
        Self {
            session,
            screen: screen.clone(),
            cursor,
            anchor: None,
        }
    }

    pub fn move_cursor(&mut self, motion: CopyMotion) {
        let (rows, cols) = self.screen.size();
        if rows == 0 || cols == 0 {
            self.cursor = CopyPoint { row: 0, col: 0 };
            return;
        }

        self.cursor = normalize_point(&self.screen, self.cursor);
        let last_row = rows - 1;
        let last_col = cols - 1;
        match motion {
            CopyMotion::Left => {
                self.cursor.col = self.cursor.col.saturating_sub(1);
            }
            CopyMotion::Right => {
                let width = self
                    .screen
                    .cell(self.cursor.row, self.cursor.col)
                    .filter(|cell| cell.is_wide())
                    .map_or(1, |_| 2);
                self.cursor.col = self.cursor.col.saturating_add(width).min(last_col);
            }
            CopyMotion::Up => {
                self.cursor.row = self.cursor.row.saturating_sub(1);
            }
            CopyMotion::Down => {
                self.cursor.row = self.cursor.row.saturating_add(1).min(last_row);
            }
            CopyMotion::RowStart => {
                self.cursor.col = 0;
            }
            CopyMotion::RowEnd => {
                self.cursor.col = last_col;
            }
            CopyMotion::First => {
                self.cursor = CopyPoint { row: 0, col: 0 };
            }
            CopyMotion::Last => {
                self.cursor = CopyPoint {
                    row: last_row,
                    col: last_col,
                };
            }
        }
        self.cursor = normalize_point(&self.screen, self.cursor);
    }

    pub fn set_anchor(&mut self) {
        self.cursor = normalize_point(&self.screen, self.cursor);
        self.anchor = Some(self.cursor);
    }

    pub fn selected_text(&self) -> Option<String> {
        let anchor = normalize_point(&self.screen, self.anchor?);
        let cursor = normalize_point(&self.screen, self.cursor);
        let (start, end) = if anchor <= cursor {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };
        let (_, cols) = self.screen.size();
        let width = self
            .screen
            .cell(end.row, end.col)
            .filter(|cell| cell.is_wide())
            .map_or(1, |_| 2);
        let end_col = end.col.saturating_add(width).min(cols);
        Some(
            self.screen
                .contents_between(start.row, start.col, end.row, end_col),
        )
    }

    pub fn contains(&self, point: CopyPoint) -> bool {
        let Some(anchor) = self.anchor else {
            return false;
        };
        let anchor = normalize_point(&self.screen, anchor);
        let cursor = normalize_point(&self.screen, self.cursor);
        let (start, mut end) = if anchor <= cursor {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };
        let (_, cols) = self.screen.size();
        if self
            .screen
            .cell(end.row, end.col)
            .is_some_and(|cell| cell.is_wide())
        {
            end.col = end.col.saturating_add(1).min(cols.saturating_sub(1));
        }
        let (rows, cols) = self.screen.size();
        if point.row >= rows || point.col >= cols {
            return false;
        }
        start <= point && point <= end
    }
}

impl HistoryCopyJob {
    pub fn new(id: u64, range: HistoryCopyRange) -> Self {
        let (start, end) = if range.anchor <= range.cursor {
            (range.anchor, range.cursor)
        } else {
            (range.cursor, range.anchor)
        };
        Self {
            id,
            range,
            start,
            end,
            next_row: start.row,
            next_col: start.col,
            row_width: None,
            row_wrapped: None,
            output: String::new(),
            pending_blanks: 0,
        }
    }

    pub fn page_needed(&self) -> (u32, u16, u16, u16) {
        let start_col = self.next_col.saturating_sub(1);
        let cols = u16::try_from(
            u32::from(u16::MAX)
                .saturating_sub(u32::from(start_col))
                .min(128),
        )
        .unwrap_or(0);
        (self.next_row, 1, start_col, cols)
    }

    pub fn consume_page(&mut self, page: &HistoryRows) -> io::Result<bool> {
        let (expected_row, expected_rows, expected_col, expected_cols) = self.page_needed();
        if page.start_row != expected_row
            || page.rows.len() != usize::from(expected_rows)
            || page.start_col != expected_col
            || expected_cols == 0
        {
            return Err(invalid_history_data(
                "history copy page does not match the job",
            ));
        }
        let row = page
            .rows
            .first()
            .ok_or_else(|| invalid_history_data("history copy page has no row"))?;
        let cells_len = u32::try_from(row.cells.len())
            .map_err(|_| invalid_history_data("history row has too many cells"))?;
        let slice_end = u32::from(page.start_col)
            .checked_add(cells_len)
            .ok_or_else(|| invalid_history_data("history row slice overflows"))?;
        if page.start_col > row.width || slice_end > u32::from(row.width) {
            return Err(invalid_history_data(
                "history row slice is outside its width",
            ));
        }
        let expected_slice_end = u32::from(page.start_col)
            .checked_add(u32::from(expected_cols))
            .ok_or_else(|| invalid_history_data("history page bounds overflow"))?;
        if slice_end < u32::from(row.width).min(expected_slice_end) {
            return Err(invalid_history_data("history copy page is incomplete"));
        }
        if let Some(width) = self.row_width {
            if width != row.width || self.row_wrapped != Some(row.wrapped) {
                return Err(invalid_history_data(
                    "history row metadata changed within a copy job",
                ));
            }
        } else {
            self.row_width = Some(row.width);
            self.row_wrapped = Some(row.wrapped);
        }

        let mut next_col = self.next_col;
        if next_col < u16::try_from(slice_end).unwrap_or(u16::MAX)
            && cell_at(row, page.start_col, next_col).is_some_and(|cell| cell.width == 0)
        {
            if next_col == 0 {
                return Err(invalid_history_data(
                    "history copy starts on a continuation",
                ));
            }
            let leader_col = next_col - 1;
            let leader = cell_at(row, page.start_col, leader_col)
                .ok_or_else(|| invalid_history_data("history copy continuation lacks a leader"))?;
            if leader.width != 2 {
                return Err(invalid_history_data(
                    "history copy continuation lacks a wide leader",
                ));
            }
            next_col = leader_col;
            if self.next_row == self.start.row {
                self.start.col = leader_col;
            }
        }
        self.next_col = next_col;

        let endpoint_known = if self.next_row != self.end.row {
            true
        } else if row.width == 0 {
            if self.end.col != 0 {
                return Err(invalid_history_data(
                    "history copy endpoint is outside an empty row",
                ));
            }
            true
        } else if self.end.col < u16::try_from(slice_end).unwrap_or(u16::MAX) {
            if self.end.col >= row.width {
                return Err(invalid_history_data(
                    "history copy endpoint is outside the row",
                ));
            }
            if cell_at(row, page.start_col, self.end.col).is_some_and(|cell| cell.width == 0) {
                let leader_col =
                    self.end.col.checked_sub(1).ok_or_else(|| {
                        invalid_history_data("history copy endpoint lacks a leader")
                    })?;
                let leader = cell_at(row, page.start_col, leader_col)
                    .ok_or_else(|| invalid_history_data("history copy endpoint lacks a leader"))?;
                if leader.width != 2 {
                    return Err(invalid_history_data(
                        "history copy endpoint lacks a wide leader",
                    ));
                }
                self.end.col = leader_col;
            }
            true
        } else {
            false
        };
        if self.next_row == self.end.row && self.end.col >= row.width && row.width != 0 {
            return Err(invalid_history_data(
                "history copy endpoint is outside the row",
            ));
        }

        let target_end = if self.next_row != self.end.row {
            u32::from(row.width)
        } else if row.width == 0 {
            0
        } else if endpoint_known {
            let endpoint = cell_at(row, page.start_col, self.end.col)
                .ok_or_else(|| invalid_history_data("history copy endpoint is not present"))?;
            if endpoint.width == 0 {
                return Err(invalid_history_data(
                    "history copy endpoint was not normalized",
                ));
            }
            let end = u32::from(self.end.col)
                .checked_add(u32::from(endpoint.width))
                .ok_or_else(|| invalid_history_data("history copy endpoint overflows"))?;
            if end > u32::from(row.width) {
                return Err(invalid_history_data("wide endpoint exceeds row width"));
            }
            end
        } else {
            u32::from(self.end.col)
        };
        if self.next_col > u16::try_from(target_end).unwrap_or(u16::MAX) {
            return Err(invalid_history_data(
                "history copy cursor passed its endpoint",
            ));
        }

        let scan_limit = target_end.min(slice_end);
        let mut consumed_end = u32::from(self.next_col);
        let mut col = consumed_end;
        while col < scan_limit {
            let physical_col = u16::try_from(col)
                .map_err(|_| invalid_history_data("history physical column overflows"))?;
            let cell = cell_at(row, page.start_col, physical_col)
                .ok_or_else(|| invalid_history_data("history copy page omits a selected cell"))?;
            match cell.width {
                0 => {
                    return Err(invalid_history_data(
                        "history copy selected an orphan continuation",
                    ));
                }
                1 => {
                    col += 1;
                }
                2 => {
                    let end = col
                        .checked_add(2)
                        .ok_or_else(|| invalid_history_data("wide cell bounds overflow"))?;
                    if end > target_end {
                        return Err(invalid_history_data(
                            "history copy endpoint cuts through a wide cell",
                        ));
                    }
                    if end > slice_end {
                        if slice_end < u32::from(row.width) {
                            break;
                        }
                        return Err(invalid_history_data(
                            "wide cell is missing its continuation",
                        ));
                    }
                    let continuation = cell_at(
                        row,
                        page.start_col,
                        u16::try_from(col + 1)
                            .map_err(|_| invalid_history_data("wide cell column overflows"))?,
                    )
                    .ok_or_else(|| invalid_history_data("wide cell lacks a continuation"))?;
                    if continuation.width != 0 {
                        return Err(invalid_history_data(
                            "wide cell continuation has the wrong width",
                        ));
                    }
                    col = end;
                }
                _ => return Err(invalid_history_data("history cell width is invalid")),
            }
            consumed_end = col;
        }

        if consumed_end == u32::from(self.next_col) && u32::from(self.next_col) < target_end {
            if slice_end <= u32::from(self.next_col) {
                return Err(invalid_history_data("history copy page made no progress"));
            }
            return Ok(false);
        }
        if consumed_end > u32::from(self.next_col) {
            append_history_selection(
                &mut self.output,
                row,
                page.start_col,
                self.next_col,
                u16::try_from(consumed_end)
                    .map_err(|_| invalid_history_data("history copy range overflows"))?,
                &mut self.pending_blanks,
            )?;
            self.next_col = u16::try_from(consumed_end)
                .map_err(|_| invalid_history_data("history copy range overflows"))?;
        }

        let row_complete = u32::from(self.next_col) >= target_end
            && (self.next_row != self.end.row || endpoint_known);
        if !row_complete {
            return Ok(false);
        }
        self.pending_blanks = 0;
        if self.next_row == self.end.row {
            return Ok(true);
        }
        if !row.wrapped {
            append_history_text(&mut self.output, "\n")?;
        }
        self.next_row = self
            .next_row
            .checked_add(1)
            .ok_or_else(|| invalid_history_data("history row index overflows"))?;
        self.next_col = 0;
        self.row_width = None;
        self.row_wrapped = None;
        Ok(false)
    }

    pub fn into_completion(self) -> HistoryCopyCompletion {
        HistoryCopyCompletion {
            id: self.id,
            range: self.range,
            text: self.output,
        }
    }
}

fn invalid_history_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn cell_at<'a>(row: &'a HistoryRow, slice_start_col: u16, col: u16) -> Option<&'a HistoryCell> {
    col.checked_sub(slice_start_col)
        .and_then(|offset| row.cells.get(usize::from(offset)))
}

fn append_history_text(output: &mut String, text: &str) -> io::Result<()> {
    if output
        .len()
        .checked_add(text.len())
        .is_none_or(|length| length > MAX_COPY_BYTES)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Selection exceeds 65536 bytes",
        ));
    }
    output.push_str(text);
    Ok(())
}

fn append_nonempty_history_text(
    output: &mut String,
    text: &str,
    pending_blanks: &mut usize,
) -> io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let extra = pending_blanks.checked_add(text.len()).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Selection exceeds 65536 bytes")
    })?;
    if output
        .len()
        .checked_add(extra)
        .is_none_or(|length| length > MAX_COPY_BYTES)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Selection exceeds 65536 bytes",
        ));
    }
    output.extend(std::iter::repeat(' ').take(*pending_blanks));
    *pending_blanks = 0;
    output.push_str(text);
    Ok(())
}

pub fn append_history_selection(
    output: &mut String,
    row: &HistoryRow,
    slice_start_col: u16,
    start_col: u16,
    end_col_exclusive: u16,
    pending_blanks: &mut usize,
) -> io::Result<()> {
    if start_col > end_col_exclusive || start_col < slice_start_col {
        return Err(invalid_history_data("history selection bounds are invalid"));
    }
    let slice_end = u32::from(slice_start_col)
        .checked_add(
            u32::try_from(row.cells.len())
                .map_err(|_| invalid_history_data("history row has too many cells"))?,
        )
        .ok_or_else(|| invalid_history_data("history row slice overflows"))?;
    if slice_end > u32::from(row.width) || u32::from(end_col_exclusive) > slice_end {
        return Err(invalid_history_data("history selection exceeds its page"));
    }
    let mut col = start_col;
    if col < end_col_exclusive
        && cell_at(row, slice_start_col, col).is_some_and(|cell| cell.width == 0)
    {
        let leader_col = col
            .checked_sub(1)
            .ok_or_else(|| invalid_history_data("history selection starts on a continuation"))?;
        let leader = cell_at(row, slice_start_col, leader_col)
            .ok_or_else(|| invalid_history_data("history selection continuation lacks a leader"))?;
        if leader.width != 2 {
            return Err(invalid_history_data(
                "history selection continuation lacks a wide leader",
            ));
        }
        col = leader_col;
    }
    while col < end_col_exclusive {
        let cell = cell_at(row, slice_start_col, col)
            .ok_or_else(|| invalid_history_data("history selection omits a cell"))?;
        match cell.width {
            0 => {
                return Err(invalid_history_data(
                    "history selection contains an orphan continuation",
                ));
            }
            1 => {
                if cell.text.is_empty() {
                    *pending_blanks = pending_blanks.checked_add(1).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "Selection exceeds 65536 bytes")
                    })?;
                } else {
                    append_nonempty_history_text(output, &cell.text, pending_blanks)?;
                }
                col = col
                    .checked_add(1)
                    .ok_or_else(|| invalid_history_data("history selection overflows"))?;
            }
            2 => {
                let continuation_col = col
                    .checked_add(1)
                    .ok_or_else(|| invalid_history_data("wide cell column overflows"))?;
                if continuation_col >= end_col_exclusive {
                    return Err(invalid_history_data(
                        "history selection cuts through a wide cell",
                    ));
                }
                let continuation = cell_at(row, slice_start_col, continuation_col)
                    .ok_or_else(|| invalid_history_data("wide cell lacks a continuation"))?;
                if continuation.width != 0 {
                    return Err(invalid_history_data(
                        "wide cell continuation has the wrong width",
                    ));
                }
                if !cell.text.is_empty() {
                    append_nonempty_history_text(output, &cell.text, pending_blanks)?;
                }
                col = continuation_col
                    .checked_add(1)
                    .ok_or_else(|| invalid_history_data("wide cell column overflows"))?;
            }
            _ => return Err(invalid_history_data("history cell width is invalid")),
        }
    }
    Ok(())
}

fn clamp_point(screen: &vt100::Screen, point: CopyPoint) -> CopyPoint {
    let (rows, cols) = screen.size();
    CopyPoint {
        row: point.row.min(rows.saturating_sub(1)),
        col: point.col.min(cols.saturating_sub(1)),
    }
}

fn normalize_point(screen: &vt100::Screen, point: CopyPoint) -> CopyPoint {
    let mut point = clamp_point(screen, point);
    if screen
        .cell(point.row, point.col)
        .is_some_and(|cell| cell.is_wide_continuation())
    {
        point.col = point.col.saturating_sub(1);
    }
    point
}

pub fn write_clipboard(writer: &mut impl std::io::Write, text: &str) -> std::io::Result<()> {
    if text.is_empty() || text.len() > MAX_COPY_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Selection must contain 1..=65536 UTF-8 bytes",
        ));
    }
    crossterm::execute!(
        writer,
        crossterm::clipboard::CopyToClipboard::to_clipboard_from(text)
    )
}

#[cfg(test)]
mod tests {
    use super::{
        HistoryCopyJob, HistoryCopyPoint, HistoryCopyRange, MAX_COPY_BYTES,
        append_history_selection, write_clipboard,
    };
    use ovrcr_protocol::{HistoryCell, HistoryColor, HistoryRow, HistoryRows, HistorySnapshotId};
    use std::io::{self, Write};

    fn cell(text: &str, width: u8) -> HistoryCell {
        HistoryCell {
            text: text.into(),
            width,
            fg: HistoryColor::Default,
            bg: HistoryColor::Default,
            attributes: 0,
        }
    }

    fn row(width: u16, cells: Vec<HistoryCell>, wrapped: bool) -> HistoryRow {
        HistoryRow {
            width,
            cells,
            wrapped,
        }
    }

    fn page(start_row: u32, start_col: u16, row: HistoryRow) -> HistoryRows {
        HistoryRows {
            session: ovrcr_protocol::SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row,
            start_col,
            rows: vec![row],
        }
    }

    fn range(start: HistoryCopyPoint, end: HistoryCopyPoint) -> HistoryCopyRange {
        HistoryCopyRange {
            session: ovrcr_protocol::SessionId(1),
            snapshot: HistorySnapshotId(7),
            anchor: start,
            cursor: end,
        }
    }

    #[test]
    fn history_copy_append_normalizes_wide_cells_and_preserves_spaces() {
        let wide = row(
            5,
            vec![
                cell("", 1),
                cell("界", 2),
                cell("", 0),
                cell("e\u{301}", 1),
                cell(" ", 1),
            ],
            false,
        );
        let mut output = String::new();
        let mut pending_blanks = 0;
        append_history_selection(&mut output, &wide, 0, 1, 5, &mut pending_blanks).unwrap();
        assert_eq!(output, "界e\u{301} ");
        assert_eq!(pending_blanks, 0);

        let mut tiled_output = String::new();
        let mut tiled_pending = 0;
        append_history_selection(
            &mut tiled_output,
            &row(2, vec![cell("", 1)], false),
            0,
            0,
            1,
            &mut tiled_pending,
        )
        .unwrap();
        append_history_selection(
            &mut tiled_output,
            &row(2, vec![cell("x", 1)], false),
            1,
            1,
            2,
            &mut tiled_pending,
        )
        .unwrap();
        assert_eq!(tiled_output, " x");
        assert_eq!(tiled_pending, 0);
    }

    #[test]
    fn history_copy_job_streams_overlapping_tiles_and_soft_wraps() {
        let mut job = HistoryCopyJob::new(
            4,
            range(
                HistoryCopyPoint { row: 0, col: 0 },
                HistoryCopyPoint { row: 2, col: 0 },
            ),
        );
        assert_eq!(job.page_needed(), (0, 1, 0, 128));
        assert!(
            !job.consume_page(&page(
                0,
                0,
                row(
                    4,
                    vec![cell("a", 1), cell("b", 1), cell("c", 1), cell("d", 1)],
                    true
                ),
            ))
            .unwrap()
        );
        assert_eq!(job.page_needed(), (1, 1, 0, 128));
        assert!(
            !job.consume_page(&page(
                1,
                0,
                row(
                    4,
                    vec![cell("e", 1), cell("f", 1), cell("g", 1), cell("h", 1)],
                    false
                ),
            ))
            .unwrap()
        );
        assert_eq!(job.page_needed(), (2, 1, 0, 128));
        assert!(
            job.consume_page(&page(2, 0, row(1, vec![cell("i", 1)], false),))
                .unwrap()
        );
        assert_eq!(job.into_completion().text, "abcdefgh\ni");

        let wide_row: Vec<HistoryCell> = (0..260)
            .map(|col| match col {
                127 => cell("界", 2),
                128 => cell("", 0),
                _ => cell("x", 1),
            })
            .collect();
        let mut tiled = HistoryCopyJob::new(
            5,
            range(
                HistoryCopyPoint { row: 0, col: 126 },
                HistoryCopyPoint { row: 0, col: 259 },
            ),
        );
        let first = tiled.page_needed();
        assert_eq!(first, (0, 1, 125, 128));
        assert!(
            !tiled
                .consume_page(&page(
                    0,
                    first.2,
                    row(260, wide_row[125..253].to_vec(), false)
                ))
                .unwrap()
        );
        let second = tiled.page_needed();
        assert_eq!(second, (0, 1, 252, 128));
        assert!(
            tiled
                .consume_page(&page(
                    0,
                    second.2,
                    row(260, wide_row[252..260].to_vec(), false)
                ))
                .unwrap()
        );
        assert_eq!(
            tiled.into_completion().text,
            format!("x界{}", "x".repeat(131))
        );

        let mut edge_row: Vec<HistoryCell> = (0..260).map(|_| cell("x", 1)).collect();
        edge_row[252] = cell("界", 2);
        edge_row[253] = cell("", 0);
        let mut edge = HistoryCopyJob::new(
            8,
            range(
                HistoryCopyPoint { row: 0, col: 126 },
                HistoryCopyPoint { row: 0, col: 259 },
            ),
        );
        assert_eq!(edge.page_needed(), (0, 1, 125, 128));
        assert!(
            !edge
                .consume_page(&page(0, 125, row(260, edge_row[125..253].to_vec(), false),))
                .unwrap()
        );
        assert_eq!(edge.page_needed(), (0, 1, 251, 128));
        assert!(
            edge.consume_page(&page(0, 251, row(260, edge_row[251..260].to_vec(), false),))
                .unwrap()
        );
        let edge_text = edge.into_completion().text;
        assert_eq!(&edge_text[126..129], "界");
        assert_eq!(edge_text.chars().count(), 133);

        let endpoint_continuation = HistoryCopyJob::new(
            6,
            range(
                HistoryCopyPoint { row: 0, col: 0 },
                HistoryCopyPoint { row: 0, col: 1 },
            ),
        );
        let mut endpoint_continuation = endpoint_continuation;
        assert!(
            endpoint_continuation
                .consume_page(&page(
                    0,
                    0,
                    row(3, vec![cell("界", 2), cell("", 0), cell("z", 1)], false),
                ))
                .unwrap()
        );
        assert_eq!(endpoint_continuation.into_completion().text, "界");

        let mut trailing_spaces = HistoryCopyJob::new(
            7,
            range(
                HistoryCopyPoint { row: 0, col: 0 },
                HistoryCopyPoint { row: 0, col: 3 },
            ),
        );
        assert!(
            trailing_spaces
                .consume_page(&page(
                    0,
                    0,
                    row(
                        4,
                        vec![cell("x", 1), cell("", 1), cell("", 1), cell(" ", 1)],
                        false,
                    ),
                ))
                .unwrap()
        );
        assert_eq!(trailing_spaces.into_completion().text, "x   ");

        let mut empty_middle = HistoryCopyJob::new(
            9,
            range(
                HistoryCopyPoint { row: 0, col: 0 },
                HistoryCopyPoint { row: 2, col: 0 },
            ),
        );
        assert!(
            !empty_middle
                .consume_page(&page(0, 0, row(1, vec![cell("x", 1)], false)))
                .unwrap()
        );
        assert!(
            !empty_middle
                .consume_page(&page(1, 0, row(0, vec![], false)))
                .unwrap()
        );
        assert!(
            empty_middle
                .consume_page(&page(2, 0, row(1, vec![cell("y", 1)], false)))
                .unwrap()
        );
        assert_eq!(empty_middle.into_completion().text, "x\n\ny");
    }

    #[test]
    fn history_copy_append_rejects_malformed_cells_and_byte_overflow() {
        let mut output = String::new();
        let mut pending_blanks = 0;
        let orphan = row(2, vec![cell("", 0), cell("x", 1)], false);
        let error = append_history_selection(&mut output, &orphan, 0, 0, 2, &mut pending_blanks)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);

        let missing_continuation = row(1, vec![cell("界", 2)], false);
        let error = append_history_selection(
            &mut output,
            &missing_continuation,
            0,
            0,
            1,
            &mut pending_blanks,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);

        let mut output = "x".repeat(MAX_COPY_BYTES - 2);
        let mut pending_blanks = 1;
        let error = append_history_selection(
            &mut output,
            &row(2, vec![cell("x", 1), cell("界", 1)], false),
            0,
            0,
            2,
            &mut pending_blanks,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(output.len(), MAX_COPY_BYTES);
    }

    #[test]
    fn copy_clipboard_emits_osc52() {
        let mut output = Vec::new();
        write_clipboard(&mut output, "foo").unwrap();
        assert_eq!(output, b"\x1b]52;c;Zm9v\x1b\\");
    }

    #[test]
    fn copy_clipboard_rejects_invalid_payload_and_io_failure() {
        let mut output = Vec::new();
        let error = write_clipboard(&mut output, "").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(output.is_empty());

        let oversized = "x".repeat(65_537);
        let error = write_clipboard(&mut output, &oversized).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(output.is_empty());

        let mut broken_pipe = BrokenPipeWriter;
        let error = write_clipboard(&mut broken_pipe, "foo").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);

        let mut flush_failure = FlushFailureWriter::default();
        let error = write_clipboard(&mut flush_failure, "foo").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(flush_failure.output, b"\x1b]52;c;Zm9v\x1b\\");
    }

    struct BrokenPipeWriter;

    impl Write for BrokenPipeWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FlushFailureWriter {
        output: Vec<u8>,
    }

    impl Write for FlushFailureWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "flush failed",
            ))
        }
    }
}
