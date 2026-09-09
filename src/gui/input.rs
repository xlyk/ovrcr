use crate::tui::{KeyEncoding, encode_key, encode_mouse, encode_paste};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use eframe::egui::{self, Event, Key};

pub fn encode_event(
    event: &Event,
    screen: &vt100::Screen,
    rect: egui::Rect,
    cell: egui::Vec2,
    pointer: egui::Pos2,
) -> Vec<u8> {
    match event {
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
            if let Some(meta) = meta_key(*key, *modifiers) {
                return meta.to_vec();
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
                KeyEvent::new(code, key_modifiers(*modifiers)),
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
        Event::MouseWheel {
            delta, modifiers, ..
        } => {
            let Some((column, row)) = pointer_cell(pointer, screen, rect, cell) else {
                return Vec::new();
            };
            let kind = if delta.y.abs() >= delta.x.abs() {
                if delta.y > 0.0 {
                    MouseEventKind::ScrollUp
                } else if delta.y < 0.0 {
                    MouseEventKind::ScrollDown
                } else {
                    return Vec::new();
                }
            } else if delta.x > 0.0 {
                MouseEventKind::ScrollRight
            } else if delta.x < 0.0 {
                MouseEventKind::ScrollLeft
            } else {
                return Vec::new();
            };
            encode_mouse(
                MouseEvent {
                    kind,
                    column,
                    row,
                    modifiers: key_modifiers(*modifiers),
                },
                screen.mouse_protocol_mode(),
                screen.mouse_protocol_encoding(),
            )
            .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// The events of one frame that the terminal should receive.
///
/// macOS composes Option with a letter into a glyph egui delivers as `Text`
/// right after the `Key` press. [`encode_event`] answers that press with the
/// meta form, so forwarding the composed text as well would type twice.
pub fn terminal_events(events: &[Event]) -> impl Iterator<Item = &Event> {
    events.iter().enumerate().filter_map(|(index, event)| {
        let composed = matches!(event, Event::Text(_))
            && index.checked_sub(1).is_some_and(|previous| {
                matches!(
                    &events[previous],
                    Event::Key { key, pressed: true, modifiers, .. }
                        if meta_key(*key, *modifiers).is_some()
                )
            });
        (!composed).then_some(event)
    })
}

fn pointer_cell(
    pos: egui::Pos2,
    screen: &vt100::Screen,
    rect: egui::Rect,
    cell: egui::Vec2,
) -> Option<(u16, u16)> {
    if !rect.contains(pos) {
        return None;
    }
    let column = ((pos.x - rect.min.x) / cell.x).floor() as u16;
    let row = ((pos.y - rect.min.y) / cell.y).floor() as u16;
    let (rows, cols) = screen.size();
    (row < rows && column < cols).then_some((column, row))
}

/// The meta encoding for Option with a single-character key, which macOS
/// otherwise composes into a glyph (Option-B becomes the integral sign).
/// Control and Command take precedence, as the arms above them do.
fn meta_key(key: Key, modifiers: egui::Modifiers) -> Option<[u8; 2]> {
    let &[byte] = key.name().as_bytes() else {
        return None;
    };
    (modifiers.alt && !modifiers.ctrl && !modifiers.mac_cmd).then_some([
        0x1b,
        if modifiers.shift {
            byte
        } else {
            byte.to_ascii_lowercase()
        },
    ])
}

fn key_modifiers(modifiers: egui::Modifiers) -> KeyModifiers {
    let mut bits = KeyModifiers::NONE;
    if modifiers.shift {
        bits |= KeyModifiers::SHIFT;
    }
    if modifiers.alt {
        bits |= KeyModifiers::ALT;
    }
    if modifiers.ctrl {
        bits |= KeyModifiers::CONTROL;
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key, modifiers: egui::Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn encode(event: &Event, screen: &vt100::Screen) -> Vec<u8> {
        encode_event(
            event,
            screen,
            egui::Rect::NOTHING,
            egui::vec2(8.0, 16.0),
            egui::pos2(0.0, 0.0),
        )
    }

    #[test]
    fn special_keys_carry_modifiers() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        assert_eq!(
            encode(&press(Key::ArrowUp, egui::Modifiers::CTRL), screen),
            b"\x1b[1;5A"
        );
        assert_eq!(
            encode(&press(Key::Enter, egui::Modifiers::SHIFT), screen),
            b"\x1b[13;2u"
        );
        assert_eq!(
            encode(&press(Key::Backspace, egui::Modifiers::ALT), screen),
            b"\x1b\x7f"
        );
    }

    #[test]
    fn alt_letter_encodes_meta() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        // macOS composes Option-B into the glyph egui delivers as `Text`
        // immediately after the key press; only the meta form may reach the PTY.
        let events = vec![
            press(Key::B, egui::Modifiers::ALT),
            Event::Text("\u{222b}".to_owned()),
        ];
        let encoded: Vec<Vec<u8>> = terminal_events(&events)
            .map(|event| encode(event, screen))
            .collect();
        assert_eq!(encoded, vec![b"\x1bb".to_vec()]);
    }
}
