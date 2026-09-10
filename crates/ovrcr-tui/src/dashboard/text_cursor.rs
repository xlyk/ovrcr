use crossterm::event::KeyCode;
use ratatui::{style::Style, text::Span};
use unicode_width::UnicodeWidthStr;

/// Byte position shared by keyboard editing and cell-based pointer placement.
/// None follows the end when a field is populated by an existing form default.
#[derive(Clone, Copy, Default)]
pub(crate) struct TextCursor(Option<usize>);

fn boundaries(text: &str) -> Vec<usize> {
    let span = Span::raw(text);
    span.styled_graphemes(Style::default())
        .scan(0, |offset, grapheme| {
            let start = *offset;
            *offset += grapheme.symbol.len();
            Some(start)
        })
        .chain(std::iter::once(text.len()))
        .collect()
}
impl TextCursor {
    /// Place a byte cursor on a rendered grapheme, including wide/combining cells.
    pub(crate) fn byte_at_column(text: &str, column: usize) -> usize {
        let mut used = 0;
        for pair in boundaries(text).windows(2) {
            let width = text[pair[0]..pair[1]].width();
            if column < used + width {
                return pair[0];
            }
            used += width;
        }
        text.len()
    }

    fn position(self, text: &str) -> usize {
        let requested = self.0.unwrap_or(text.len()).min(text.len());
        boundaries(text)
            .into_iter()
            .take_while(|i| *i <= requested)
            .last()
            .unwrap_or(0)
    }
    pub(crate) fn insert(&mut self, text: &mut String, value: &str) {
        let position = self.position(text);
        text.insert_str(position, value);
        self.0 = Some(position + value.len());
    }
    pub(crate) fn key(&mut self, text: &mut String, key: KeyCode) {
        let position = self.position(text);
        let points = boundaries(text);
        let previous = points
            .iter()
            .copied()
            .take_while(|i| *i < position)
            .last()
            .unwrap_or(0);
        let next = points
            .iter()
            .copied()
            .find(|i| *i > position)
            .unwrap_or(text.len());
        self.0 = Some(match key {
            KeyCode::Left => previous,
            KeyCode::Right => next,
            KeyCode::Home => 0,
            KeyCode::End => text.len(),
            KeyCode::Backspace => {
                text.replace_range(previous..position, "");
                previous
            }
            KeyCode::Delete => {
                text.replace_range(position..next, "");
                position
            }
            _ => position,
        });
    }
    /// Returns the rendered text and its first visible byte. Always reserves
    /// a cell for the cursor, and never clips through a wide character.
    pub(crate) fn display(self, text: &str, width: usize) -> (String, usize) {
        if width == 0 {
            return (String::new(), self.position(text));
        }
        let position = self.position(text);
        let mut start = 0;
        for index in boundaries(text) {
            if index > position {
                break;
            }
            start = index;
            if text[index..position].width() < width {
                break;
            }
        }
        let mut rendered = String::new();
        let mut used = 0;
        for pair in boundaries(text).windows(2).filter(|pair| pair[0] >= start) {
            if pair[0] == position {
                rendered.push('▏');
                used += 1;
            }
            let grapheme = &text[pair[0]..pair[1]];
            let cells = grapheme.width();
            if used + cells > width {
                break;
            }
            rendered.push_str(grapheme);
            used += cells;
        }
        if position == text.len() && used < width {
            rendered.push('▏');
        }
        (rendered, start)
    }
    pub(crate) fn click(&mut self, text: &str, width: usize, column: usize) {
        let (_, start) = self.display(text, width);
        let old = self.position(text);
        let mut cell = 0;
        for pair in boundaries(text).windows(2).filter(|pair| pair[0] >= start) {
            let index = pair[0];
            if index == old {
                if column <= cell {
                    self.0 = Some(index);
                    return;
                }
                cell += 1;
            }
            let cells = text[pair[0]..pair[1]].width();
            if column < cell + cells {
                self.0 = Some(index);
                return;
            }
            cell += cells;
        }
        self.0 = Some(text.len());
    }
}
