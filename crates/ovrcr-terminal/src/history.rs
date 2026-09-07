use anyhow::{Result, bail};
use ovrcr_protocol::{
    HistoryCell, HistoryColor, HistoryOpened, HistoryRow, HistoryRows, HistorySnapshotId,
    PAGE_COLS, PAGE_ROWS, SessionId,
};

#[derive(Debug)]
pub struct FrozenHistory {
    opened: HistoryOpened,
    screen: vt100::Screen,
}

impl FrozenHistory {
    pub fn capture(
        session: SessionId,
        snapshot: HistorySnapshotId,
        revision: u64,
        mut screen: vt100::Screen,
    ) -> Result<Self> {
        if screen.alternate_screen() {
            bail!("History is unavailable on the alternate screen");
        }
        let history_rows = {
            screen.set_scrollback(usize::MAX);
            let history_rows = screen.scrollback();
            screen.set_scrollback(0);
            history_rows
        };
        let (rows, cols) = screen.size();
        let total_rows = history_rows
            .checked_add(usize::from(rows))
            .and_then(|total| u32::try_from(total).ok())
            .ok_or_else(|| anyhow::anyhow!("history row count exceeds protocol bounds"))?;
        Ok(Self {
            opened: HistoryOpened {
                session,
                snapshot,
                revision,
                size: ovrcr_protocol::TerminalSize { rows, cols },
                history_rows: u32::try_from(history_rows)?,
                total_rows,
            },
            screen,
        })
    }

    pub fn opened(&self) -> &HistoryOpened {
        &self.opened
    }

    pub fn page(
        &mut self,
        start_row: u32,
        rows: u16,
        start_col: u16,
        cols: u16,
    ) -> Result<HistoryRows> {
        if rows > PAGE_ROWS || cols > PAGE_COLS {
            bail!("history page exceeds protocol limits");
        }
        let end_row = start_row
            .checked_add(u32::from(rows))
            .ok_or_else(|| anyhow::anyhow!("history row range overflows"))?;
        let end_col = start_col
            .checked_add(cols)
            .ok_or_else(|| anyhow::anyhow!("history column range overflows"))?;
        let total_rows = self.opened.total_rows;
        if start_row > total_rows {
            bail!("history row start is out of bounds");
        }
        let returned_end = end_row.min(total_rows);
        let history_rows = self.opened.history_rows;
        let mut output = Vec::with_capacity((returned_end - start_row) as usize);
        for row in start_row..returned_end {
            let visible_row = if row < history_rows {
                self.screen
                    .set_scrollback(usize::try_from(history_rows - row)?);
                0
            } else {
                self.screen.set_scrollback(0);
                u16::try_from(row - history_rows)?
            };
            let width = physical_width(&self.screen, visible_row);
            let cells = if start_col >= width {
                Vec::new()
            } else {
                let end = end_col.min(width);
                (start_col..end)
                    .filter_map(|col| self.screen.cell(visible_row, col))
                    .map(history_cell)
                    .collect()
            };
            output.push(HistoryRow {
                width,
                cells,
                wrapped: self.screen.row_wrapped(visible_row),
            });
        }
        self.screen.set_scrollback(0);
        Ok(HistoryRows {
            session: self.opened.session,
            snapshot: self.opened.snapshot,
            start_row,
            start_col,
            rows: output,
        })
    }
}

fn physical_width(screen: &vt100::Screen, row: u16) -> u16 {
    let mut low = 0u32;
    let mut high = u32::from(u16::MAX) + 1;
    while low < high {
        let mid = low + (high - low) / 2;
        let present = mid <= u32::from(u16::MAX) && screen.cell(row, mid as u16).is_some();
        if present {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low as u16
}

fn history_cell(cell: &vt100::Cell) -> HistoryCell {
    let width = if cell.is_wide_continuation() {
        0
    } else if cell.is_wide() {
        2
    } else {
        1
    };
    let attributes = u8::from(cell.bold())
        | (u8::from(cell.dim()) << 1)
        | (u8::from(cell.italic()) << 2)
        | (u8::from(cell.underline()) << 3)
        | (u8::from(cell.inverse()) << 4);
    HistoryCell {
        text: cell.contents().to_owned(),
        width,
        fg: history_color(cell.fgcolor()),
        bg: history_color(cell.bgcolor()),
        attributes,
    }
}

fn history_color(color: vt100::Color) -> HistoryColor {
    match color {
        vt100::Color::Default => HistoryColor::Default,
        vt100::Color::Idx(index) => HistoryColor::Indexed(index),
        vt100::Color::Rgb(red, green, blue) => HistoryColor::Rgb(red, green, blue),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::HISTORY_ROWS;
    #[test]
    fn history_retention_is_frozen_and_evicts_oldest() {
        let mut parser = vt100::Parser::new(2, 12, HISTORY_ROWS);
        parser.process(b"ONE\r\nTWO\r\nTHREE");
        let mut frozen = FrozenHistory::capture(
            SessionId(1),
            HistorySnapshotId(1),
            7,
            parser.screen().clone(),
        )
        .unwrap();
        assert_eq!(frozen.opened().history_rows, 1);
        let before = frozen.page(0, 1, 0, 12).unwrap();
        assert_eq!(
            before.rows[0]
                .cells
                .iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
                .trim_end(),
            "ONE"
        );
        for n in 0..600 {
            parser.process(format!("\r\nNEW_{n}").as_bytes());
        }
        assert_eq!(frozen.page(0, 1, 0, 12).unwrap(), before);
        parser.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(parser.screen().scrollback(), HISTORY_ROWS);
    }

    #[test]
    fn history_tiles_preserve_wide_combining_and_wrap() {
        let mut parser = vt100::Parser::new(2, 4, HISTORY_ROWS);
        parser.process("界e\u{301}XY".as_bytes());
        let mut frozen = FrozenHistory::capture(
            SessionId(1),
            HistorySnapshotId(2),
            1,
            parser.screen().clone(),
        )
        .unwrap();
        let rows = frozen.page(0, 2, 0, 4).unwrap();
        assert_eq!(rows.rows.len(), 2);
        assert_eq!(rows.rows[0].width, 4);
        assert!(rows.rows[0].wrapped);
        assert_eq!(rows.rows[0].cells[0].text, "界");
        assert_eq!(rows.rows[0].cells[0].width, 2);
        assert_eq!(rows.rows[0].cells[1].width, 0);
        assert_eq!(rows.rows[0].cells[2].text, "e\u{301}");
        assert_eq!(rows.rows[0].cells[2].width, 1);
        assert_eq!(rows.rows[0].cells[3].text, "X");
        assert_eq!(rows.rows[1].cells[0].text, "Y");
    }

    #[test]
    fn history_resize_keeps_old_width() {
        let mut parser = vt100::Parser::new(2, 12, HISTORY_ROWS);
        parser.process(b"OLDROW_12345\r\nSECOND_ROW\r\nCURRENT");
        let mut before_resize = FrozenHistory::capture(
            SessionId(1),
            HistorySnapshotId(3),
            2,
            parser.screen().clone(),
        )
        .unwrap();
        assert!(before_resize.opened().history_rows > 0);
        let original = before_resize.page(0, 1, 0, 12).unwrap();
        assert_eq!(original.rows[0].width, 12);
        assert_eq!(
            original.rows[0]
                .cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .trim_end(),
            "OLDROW_12345"
        );
        parser.screen_mut().set_size(2, 4);
        parser.screen_mut().set_size(2, 12);
        let mut frozen = FrozenHistory::capture(
            SessionId(1),
            HistorySnapshotId(4),
            3,
            parser.screen().clone(),
        )
        .unwrap();
        assert!(frozen.opened().history_rows > 0);
        assert_eq!(frozen.page(0, 1, 0, 12).unwrap().rows, original.rows);
    }

    #[test]
    fn history_alternate_screen_has_no_transcript() {
        let mut parser = vt100::Parser::new(2, 12, HISTORY_ROWS);
        parser.process(b"PRIMARY_OLD\r\nPRIMARY_NEW\r\nLIVE");
        let mut before_alternate = FrozenHistory::capture(
            SessionId(1),
            HistorySnapshotId(5),
            4,
            parser.screen().clone(),
        )
        .unwrap();
        let before_history_rows = before_alternate.opened().history_rows;
        let before_total_rows = before_alternate.opened().total_rows;
        assert!(before_history_rows > 0);
        let before_rows = before_alternate
            .page(0, u16::try_from(before_history_rows).unwrap(), 0, 12)
            .unwrap();
        parser.process(b"\x1b[?1049hALT");
        let error = FrozenHistory::capture(
            SessionId(1),
            HistorySnapshotId(6),
            5,
            parser.screen().clone(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("alternate screen"));
        parser.process(b"\x1b[?1049l");
        let mut frozen = FrozenHistory::capture(
            SessionId(1),
            HistorySnapshotId(7),
            6,
            parser.screen().clone(),
        )
        .unwrap();
        assert_eq!(frozen.opened().history_rows, before_history_rows);
        assert_eq!(frozen.opened().total_rows, before_total_rows);
        assert_eq!(
            frozen
                .page(0, u16::try_from(before_history_rows).unwrap(), 0, 12)
                .unwrap()
                .rows,
            before_rows.rows
        );
    }

    #[test]
    fn history_partial_escape_survives_capture() {
        let mut live = vt100::Parser::new(2, 12, HISTORY_ROWS);
        let mut uninterrupted = vt100::Parser::new(2, 12, HISTORY_ROWS);
        live.process(b"\x1b[31");
        let _ =
            FrozenHistory::capture(SessionId(1), HistorySnapshotId(6), 5, live.screen().clone())
                .unwrap();
        live.process(&[b'm', 0xe7]);
        let _ =
            FrozenHistory::capture(SessionId(1), HistorySnapshotId(7), 6, live.screen().clone())
                .unwrap();
        live.process(&[0x95, 0x8c]);
        uninterrupted.process("\x1b[31m界".as_bytes());
        assert_eq!(
            live.screen().state_formatted(),
            uninterrupted.screen().state_formatted()
        );
    }

    #[test]
    fn history_bounds_reject_bad_ranges() {
        let parser = vt100::Parser::new(2, 12, HISTORY_ROWS);
        let mut frozen = FrozenHistory::capture(
            SessionId(1),
            HistorySnapshotId(8),
            7,
            parser.screen().clone(),
        )
        .unwrap();
        assert!(frozen.page(u32::MAX, 1, 0, 1).is_err());
        assert!(frozen.page(u32::MAX - 1, 2, 0, 1).is_err());
        assert!(frozen.page(0, PAGE_ROWS + 1, 0, 1).is_err());
        assert!(frozen.page(0, 1, u16::MAX, 2).is_err());
    }
}
