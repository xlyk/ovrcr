use ovrcr_protocol::SessionId;
use ovrcr_terminal::vt100;

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
