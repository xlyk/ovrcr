use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{style::Style, text::Span};
use unicode_width::UnicodeWidthStr;

/// Byte position shared by keyboard editing and cell-based pointer placement.
/// None follows the end when a field is populated by an existing form default.
#[derive(Clone, Copy, Default)]
pub(crate) struct TextCursor(Option<usize>);

fn boundaries(text: &str) -> Vec<usize> {
    let mut points = Vec::new();
    let mut offset = 0;
    for segment in text.split_inclusive(char::is_control) {
        let body = segment.trim_end_matches(char::is_control);
        let span = Span::raw(body);
        let mut used = 0;
        for grapheme in span.styled_graphemes(Style::default()) {
            points.push(offset + used);
            used += grapheme.symbol.len();
        }
        for (index, _) in segment[body.len()..].char_indices() {
            points.push(offset + body.len() + index);
        }
        offset += segment.len();
    }
    points.push(text.len());
    points
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

    pub(crate) fn position(self, text: &str) -> usize {
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
    pub(crate) fn vertical(&mut self, text: &str, down: bool) {
        let position = self.position(text);
        let start = text[..position].rfind('\n').map_or(0, |index| index + 1);
        let column = text[start..position].width();
        let target = if down {
            let Some(end) = text[position..]
                .find('\n')
                .map(|index| position + index + 1)
            else {
                return;
            };
            end
        } else {
            if start == 0 {
                return;
            }
            text[..start - 1].rfind('\n').map_or(0, |index| index + 1)
        };
        let end = text[target..]
            .find('\n')
            .map_or(text.len(), |index| target + index);
        self.0 = Some(target + Self::byte_at_column(&text[target..end], column));
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
    /// Message-editor commands; other fields retain the basic `key` contract.
    pub(crate) fn edit_key(&mut self, text: &mut String, key: KeyEvent) -> bool {
        let control = key.modifiers == KeyModifiers::CONTROL;
        let alt = key.modifiers == KeyModifiers::ALT;
        if !control && !alt {
            return key.modifiers != KeyModifiers::NONE && key.modifiers != KeyModifiers::SHIFT;
        }
        let position = self.position(text);
        let start = text[..position].rfind('\n').map_or(0, |index| index + 1);
        let end = text[position..]
            .find('\n')
            .map_or(text.len(), |index| position + index);
        let KeyCode::Char(ch) = key.code else {
            return true;
        };
        let basic = match (control, ch) {
            (true, 'b') => Some(KeyCode::Left),
            (true, 'f') => Some(KeyCode::Right),
            (true, 'h') => Some(KeyCode::Backspace),
            (true, 'd') => Some(KeyCode::Delete),
            _ => None,
        };
        if let Some(code) = basic {
            self.key(text, code);
            return true;
        }
        let target = match (control, ch) {
            (true, 'a' | 'u') => start,
            (true, 'e' | 'k') => end,
            (true, 'w') | (false, 'b') => {
                let mut target = position;
                let mut word = false;
                for pair in boundaries(&text[start..position]).windows(2).rev() {
                    let whitespace = text[start + pair[0]..start + pair[1]]
                        .chars()
                        .all(char::is_whitespace);
                    if word && whitespace {
                        break;
                    }
                    word |= !whitespace;
                    target = start + pair[0];
                }
                target
            }
            (false, 'f') => {
                let mut target = position;
                let mut word = false;
                for pair in boundaries(&text[position..end]).windows(2) {
                    let whitespace = text[position + pair[0]..position + pair[1]]
                        .chars()
                        .all(char::is_whitespace);
                    if word && whitespace {
                        break;
                    }
                    word |= !whitespace;
                    target = position + pair[1];
                }
                target
            }
            _ => return true,
        };
        if control && matches!(ch, 'u' | 'k' | 'w') {
            text.replace_range(target.min(position)..target.max(position), "");
            self.0 = Some(target.min(position));
        } else {
            self.0 = Some(target);
        }
        true
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
