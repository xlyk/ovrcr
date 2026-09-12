use super::KeyEncoding;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ovrcr_terminal::vt100;

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

pub fn encode_mouse(
    event: MouseEvent,
    mode: vt100::MouseProtocolMode,
    encoding: vt100::MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    if mode == vt100::MouseProtocolMode::None {
        return None;
    }
    let (button, release) = mouse_report(event.kind, mode)?;
    let modifiers = if mode == vt100::MouseProtocolMode::Press {
        0
    } else {
        mouse_modifiers(event.modifiers)
    };
    let x = u32::from(event.column) + 1;
    let y = u32::from(event.row) + 1;
    match encoding {
        vt100::MouseProtocolEncoding::Sgr => {
            let code = button + modifiers;
            let suffix = if release { 'm' } else { 'M' };
            Some(format!("\x1b[<{code};{x};{y}{suffix}").into_bytes())
        }
        vt100::MouseProtocolEncoding::Default | vt100::MouseProtocolEncoding::Utf8 => {
            let code = if release { 3 } else { button } + modifiers;
            let limit = if encoding == vt100::MouseProtocolEncoding::Utf8 {
                2015
            } else {
                223
            };
            if x > limit || y > limit {
                return None;
            }
            let mut bytes = b"\x1b[M".to_vec();
            encode_legacy_field(&mut bytes, code + 32, encoding)?;
            encode_legacy_field(&mut bytes, x + 32, encoding)?;
            encode_legacy_field(&mut bytes, y + 32, encoding)?;
            Some(bytes)
        }
    }
}

fn mouse_report(kind: MouseEventKind, mode: vt100::MouseProtocolMode) -> Option<(u32, bool)> {
    let x10 = mode == vt100::MouseProtocolMode::Press;
    match kind {
        MouseEventKind::Down(button) => Some((physical_button(button), false)),
        MouseEventKind::Up(button) if !x10 => Some((physical_button(button), true)),
        MouseEventKind::Drag(button)
            if matches!(
                mode,
                vt100::MouseProtocolMode::ButtonMotion | vt100::MouseProtocolMode::AnyMotion
            ) =>
        {
            Some((physical_button(button) + 32, false))
        }
        MouseEventKind::Moved if mode == vt100::MouseProtocolMode::AnyMotion => Some((35, false)),
        MouseEventKind::ScrollUp if !x10 => Some((64, false)),
        MouseEventKind::ScrollDown if !x10 => Some((65, false)),
        MouseEventKind::ScrollLeft if !x10 => Some((66, false)),
        MouseEventKind::ScrollRight if !x10 => Some((67, false)),
        _ => None,
    }
}

fn physical_button(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

fn mouse_modifiers(modifiers: KeyModifiers) -> u32 {
    let mut bits = 0;
    if modifiers.contains(KeyModifiers::SHIFT) {
        bits += 4;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        bits += 8;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        bits += 16;
    }
    bits
}

fn encode_legacy_field(
    bytes: &mut Vec<u8>,
    value: u32,
    encoding: vt100::MouseProtocolEncoding,
) -> Option<()> {
    match encoding {
        vt100::MouseProtocolEncoding::Default => {
            let byte = u8::try_from(value).ok()?;
            bytes.push(byte);
            Some(())
        }
        vt100::MouseProtocolEncoding::Utf8 => {
            let ch = char::from_u32(value)?;
            let mut buf = [0; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            Some(())
        }
        vt100::MouseProtocolEncoding::Sgr => None,
    }
}
