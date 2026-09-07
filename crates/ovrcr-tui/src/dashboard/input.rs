use super::{Dashboard, InputMode, KeyEncoding};
use crate::protocol::ClientMessage;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ovrcr_terminal::encode_paste;

pub fn encode_key(event: KeyEvent, application_cursor: bool) -> KeyEncoding {
    if is_browse_key(event) {
        return KeyEncoding::Browse;
    }

    let bytes = match event.code {
        KeyCode::Char(ch) if event.modifiers.contains(KeyModifiers::CONTROL) => {
            let ch = ch.to_ascii_lowercase();
            if ch.is_ascii_lowercase() {
                vec![ch as u8 - b'a' + 1]
            } else {
                return KeyEncoding::Ignore;
            }
        }
        KeyCode::Char(ch) => {
            let mut bytes = Vec::new();
            if event.modifiers.contains(KeyModifiers::ALT) {
                bytes.push(0x1b);
            }
            let mut encoded = [0; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            bytes
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => cursor_sequence(application_cursor, b'D'),
        KeyCode::Right => cursor_sequence(application_cursor, b'C'),
        KeyCode::Up => cursor_sequence(application_cursor, b'A'),
        KeyCode::Down => cursor_sequence(application_cursor, b'B'),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::F(number) => {
            let Some(bytes) = function_key(number) else {
                return KeyEncoding::Ignore;
            };
            bytes
        }
        _ => return KeyEncoding::Ignore,
    };
    KeyEncoding::Bytes(bytes)
}

pub(super) fn is_browse_key(key: KeyEvent) -> bool {
    (key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('g' | 'G')))
        || matches!(key.code, KeyCode::Char('\u{7}'))
}

fn cursor_sequence(application: bool, suffix: u8) -> Vec<u8> {
    if application {
        vec![0x1b, b'O', suffix]
    } else {
        vec![0x1b, b'[', suffix]
    }
}

fn function_key(number: u8) -> Option<Vec<u8>> {
    Some(match number {
        1 => b"\x1bOP".to_vec(),
        2 => b"\x1bOQ".to_vec(),
        3 => b"\x1bOR".to_vec(),
        4 => b"\x1bOS".to_vec(),
        5 => b"\x1b[15~".to_vec(),
        6 => b"\x1b[17~".to_vec(),
        7 => b"\x1b[18~".to_vec(),
        8 => b"\x1b[19~".to_vec(),
        9 => b"\x1b[20~".to_vec(),
        10 => b"\x1b[21~".to_vec(),
        11 => b"\x1b[23~".to_vec(),
        12 => b"\x1b[24~".to_vec(),
        _ => return None,
    })
}

pub fn event_to_request(
    dashboard: &mut Dashboard,
    event: Event,
    request_id: u64,
) -> Option<ClientMessage> {
    if dashboard.palette.is_some() || dashboard.mode != InputMode::Terminal {
        return None;
    }
    match event {
        Event::Paste(text) => dashboard.input_request(
            encode_paste(&text, dashboard.parser.screen().bracketed_paste()),
            request_id,
        ),
        Event::Key(key) => match encode_key(key, dashboard.parser.screen().application_cursor()) {
            KeyEncoding::Browse => {
                dashboard.mode = InputMode::Browse;
                None
            }
            KeyEncoding::Bytes(bytes) => dashboard.input_request(bytes, request_id),
            KeyEncoding::Ignore => None,
        },
        _ => None,
    }
}
