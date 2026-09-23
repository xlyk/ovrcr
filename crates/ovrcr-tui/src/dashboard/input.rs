use super::KeyEncoding;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ovrcr_terminal::key::{Key, Modifiers, encode_key as encode_terminal_key};
use ovrcr_terminal::vt100;

pub fn encode_key(event: KeyEvent, application_cursor: bool) -> KeyEncoding {
    if is_browse_key(event) {
        return KeyEncoding::Browse;
    }
    let key = match event.code {
        KeyCode::Char(ch) => Key::Char(ch),
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Esc => Key::Escape,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Insert => Key::Insert,
        KeyCode::Delete => Key::Delete,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::F(number) => Key::F(number),
        _ => return KeyEncoding::Ignore,
    };
    let modifiers = Modifiers {
        ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
        alt: event.modifiers.contains(KeyModifiers::ALT),
        shift: event.modifiers.contains(KeyModifiers::SHIFT),
    };
    encode_terminal_key(key, modifiers, application_cursor)
        .map_or(KeyEncoding::Ignore, KeyEncoding::Bytes)
}

pub(super) fn is_browse_key(key: KeyEvent) -> bool {
    (key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('g' | 'G')))
        || matches!(key.code, KeyCode::Char('\u{7}'))
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
