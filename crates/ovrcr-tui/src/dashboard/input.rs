use super::{Dashboard, InputMode, KeyEncoding};
use crate::protocol::ClientMessage;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ovrcr_terminal::encode_paste;

pub fn encode_key(event: KeyEvent, application_cursor: bool) -> KeyEncoding {
    if is_browse_key(event) {
        return KeyEncoding::Browse;
    }

    let modifier = modifier_param(event.modifiers);
    let bytes = match event.code {
        KeyCode::Char(ch) if event.modifiers.contains(KeyModifiers::CONTROL) => {
            let Some(byte) = control_byte(ch) else {
                return KeyEncoding::Ignore;
            };
            vec![byte]
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
        // Modified Enter uses the CSI-u form so Shift+Enter can insert a
        // newline instead of submitting; legacy terminals never deliver it.
        KeyCode::Enter => match modifier {
            Some(modifier) => format!("\x1b[13;{modifier}u").into_bytes(),
            None => vec![b'\r'],
        },
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace if event.modifiers.contains(KeyModifiers::ALT) => vec![0x1b, 0x7f],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => cursor_sequence(application_cursor, modifier, b'D'),
        KeyCode::Right => cursor_sequence(application_cursor, modifier, b'C'),
        KeyCode::Up => cursor_sequence(application_cursor, modifier, b'A'),
        KeyCode::Down => cursor_sequence(application_cursor, modifier, b'B'),
        KeyCode::Home => cursor_sequence(false, modifier, b'H'),
        KeyCode::End => cursor_sequence(false, modifier, b'F'),
        KeyCode::Insert => tilde_sequence(2, modifier),
        KeyCode::Delete => tilde_sequence(3, modifier),
        KeyCode::PageUp => tilde_sequence(5, modifier),
        KeyCode::PageDown => tilde_sequence(6, modifier),
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

/// Control bytes for letters and the punctuation xterm maps below 0x20.
/// Legacy terminals report Ctrl with `\`, `]`, `^`, and `_` as the digits
/// `4` through `7`; the kitty keyboard protocol sends the punctuation itself.
fn control_byte(ch: char) -> Option<u8> {
    let ch = ch.to_ascii_lowercase();
    Some(match ch {
        'a'..='z' => ch as u8 - b'a' + 1,
        ' ' | '@' => 0x00,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' => 0x1f,
        _ => return None,
    })
}

/// The xterm modifier parameter: 1 plus shift (1), alt (2), and control (4).
fn modifier_param(modifiers: KeyModifiers) -> Option<u8> {
    let mut param = 1;
    if modifiers.contains(KeyModifiers::SHIFT) {
        param += 1;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        param += 2;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        param += 4;
    }
    (param > 1).then_some(param)
}

fn cursor_sequence(application: bool, modifier: Option<u8>, suffix: u8) -> Vec<u8> {
    match modifier {
        Some(modifier) => format!("\x1b[1;{modifier}{}", suffix as char).into_bytes(),
        None if application => vec![0x1b, b'O', suffix],
        None => vec![0x1b, b'[', suffix],
    }
}

fn tilde_sequence(number: u8, modifier: Option<u8>) -> Vec<u8> {
    match modifier {
        Some(modifier) => format!("\x1b[{number};{modifier}~").into_bytes(),
        None => format!("\x1b[{number}~").into_bytes(),
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
