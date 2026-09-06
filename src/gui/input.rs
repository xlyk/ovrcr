use crate::tui::{KeyEncoding, encode_key, encode_paste};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use eframe::egui::{self, Event, Key};

pub fn encode_event(
    event: &Event,
    screen: &vt100::Screen,
    rect: egui::Rect,
    cell: egui::Vec2,
) -> Vec<u8> {
    match event {
        Event::Key {
            key: Key::K,
            pressed: true,
            modifiers,
            ..
        } if modifiers.mac_cmd => b"\x07:".to_vec(),
        Event::Text(text) | Event::Ime(egui::ImeEvent::Commit(text)) => text.as_bytes().to_vec(),
        Event::Paste(text) => encode_paste(text, screen.bracketed_paste()),
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } if !modifiers.mac_cmd => {
            if modifiers.ctrl {
                let name = key.name();
                let control = match key {
                    Key::Space | Key::Num2 => Some(0),
                    Key::OpenBracket => Some(27),
                    Key::Backslash => Some(28),
                    Key::CloseBracket => Some(29),
                    Key::Num6 => Some(30),
                    Key::Minus => Some(31),
                    _ if name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic() => {
                        Some(name.as_bytes()[0] & 31)
                    }
                    _ => None,
                };
                if let Some(control) = control {
                    return vec![control];
                }
            }
            let code = match key {
                Key::ArrowUp => KeyCode::Up,
                Key::ArrowDown => KeyCode::Down,
                Key::ArrowLeft => KeyCode::Left,
                Key::ArrowRight => KeyCode::Right,
                Key::Enter => KeyCode::Enter,
                Key::Escape => KeyCode::Esc,
                Key::Backspace => KeyCode::Backspace,
                Key::Tab if modifiers.shift => KeyCode::BackTab,
                Key::Tab => KeyCode::Tab,
                Key::Home => KeyCode::Home,
                Key::End => KeyCode::End,
                Key::Insert => KeyCode::Insert,
                Key::Delete => KeyCode::Delete,
                Key::PageUp => KeyCode::PageUp,
                Key::PageDown => KeyCode::PageDown,
                Key::F1 => KeyCode::F(1),
                Key::F2 => KeyCode::F(2),
                Key::F3 => KeyCode::F(3),
                Key::F4 => KeyCode::F(4),
                Key::F5 => KeyCode::F(5),
                Key::F6 => KeyCode::F(6),
                Key::F7 => KeyCode::F(7),
                Key::F8 => KeyCode::F(8),
                Key::F9 => KeyCode::F(9),
                Key::F10 => KeyCode::F(10),
                Key::F11 => KeyCode::F(11),
                Key::F12 => KeyCode::F(12),
                // Printable keys arrive as Text; forwarding both would type twice.
                _ => return Vec::new(),
            };
            match encode_key(
                KeyEvent::new(code, KeyModifiers::NONE),
                screen.application_cursor(),
            ) {
                KeyEncoding::Bytes(bytes) => bytes,
                _ => Vec::new(),
            }
        }
        Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
        } => {
            let mode = screen.mouse_protocol_mode();
            if mode == vt100::MouseProtocolMode::None
                || (!pressed && mode == vt100::MouseProtocolMode::Press)
            {
                return Vec::new();
            }
            if !rect.contains(*pos) {
                return Vec::new();
            }
            let col = ((pos.x - rect.min.x) / cell.x).floor() as u16;
            let row = ((pos.y - rect.min.y) / cell.y).floor() as u16;
            let (rows, cols) = screen.size();
            if row >= rows || col >= cols {
                return Vec::new();
            }
            let button = match button {
                egui::PointerButton::Primary => 0,
                egui::PointerButton::Middle => 1,
                egui::PointerButton::Secondary => 2,
                _ => return Vec::new(),
            };
            let modifiers = u16::from(modifiers.shift) * 4
                + u16::from(modifiers.alt) * 8
                + u16::from(modifiers.ctrl) * 16;
            match screen.mouse_protocol_encoding() {
                vt100::MouseProtocolEncoding::Sgr => format!(
                    "\x1b[<{};{};{}{}",
                    button + modifiers,
                    col + 1,
                    row + 1,
                    if *pressed { 'M' } else { 'm' }
                )
                .into_bytes(),
                encoding => {
                    let code = if *pressed { button } else { 3 } + modifiers;
                    let values = [code + 32, col + 33, row + 33];
                    let mut bytes = b"\x1b[M".to_vec();
                    for value in values {
                        if encoding == vt100::MouseProtocolEncoding::Utf8 {
                            let mut buffer = [0; 4];
                            bytes.extend_from_slice(
                                char::from_u32(u32::from(value))
                                    .unwrap()
                                    .encode_utf8(&mut buffer)
                                    .as_bytes(),
                            );
                        } else if let Ok(value) = u8::try_from(value) {
                            bytes.push(value);
                        } else {
                            return Vec::new();
                        }
                    }
                    bytes
                }
            }
        }
        _ => Vec::new(),
    }
}
