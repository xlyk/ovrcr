use crate::tui::{KeyEncoding, encode_key, encode_mouse, encode_paste};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use eframe::egui::{self, Event, Key, MouseWheelUnit};

/// The mouse state [`encode_event`] carries between events: wheel distance too
/// small to report yet, the button a drag belongs to, and the last motion
/// report, so one cell is not reported twice.
#[derive(Default)]
pub struct Mouse {
    carry: egui::Vec2,
    held: Option<(MouseButton, egui::Modifiers)>,
    last_motion: Option<MouseEvent>,
}

impl Mouse {
    /// The whole wheel steps `delta` travelled as (columns, rows), keeping the
    /// fraction short of one step so slow trackpad scrolling still covers the
    /// distance it was given instead of one line per event.
    fn accumulate(
        &mut self,
        delta: egui::Vec2,
        unit: MouseWheelUnit,
        cell: egui::Vec2,
        rows: u16,
    ) -> (i32, i32) {
        let steps = match unit {
            MouseWheelUnit::Point => egui::vec2(delta.x / cell.x, delta.y / cell.y),
            MouseWheelUnit::Line => delta,
            MouseWheelUnit::Page => delta * f32::from(rows),
        };
        if !steps.is_finite() {
            return (0, 0);
        }
        self.carry += steps;
        let whole = egui::vec2(self.carry.x.trunc(), self.carry.y.trunc());
        self.carry -= whole;
        // One event never scrolls past a screenful, and a nonsense delta must
        // not spin the UI thread emitting reports.
        let limit = i32::from(rows.max(1));
        (
            (whole.x as i32).clamp(-limit, limit),
            (whole.y as i32).clamp(-limit, limit),
        )
    }
}

pub fn encode_event(
    event: &Event,
    screen: &vt100::Screen,
    rect: egui::Rect,
    cell: egui::Vec2,
    pointer: egui::Pos2,
    mouse: &mut Mouse,
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
            let Some(button) = mouse_button(*button) else {
                return Vec::new();
            };
            // A button change starts a new gesture, and a release ends its drag
            // even where the position is not reportable, so a held button never
            // outlives the gesture it belongs to.
            mouse.last_motion = None;
            if !*pressed && mouse.held.is_some_and(|(held, _)| held == button) {
                mouse.held = None;
            }
            let Some((column, row)) = pointer_cell(*pos, screen, rect, cell) else {
                return Vec::new();
            };
            if *pressed {
                mouse.held = Some((button, *modifiers));
            }
            encode_mouse(
                MouseEvent {
                    kind: if *pressed {
                        MouseEventKind::Down(button)
                    } else {
                        MouseEventKind::Up(button)
                    },
                    column,
                    row,
                    modifiers: key_modifiers(*modifiers),
                },
                screen.mouse_protocol_mode(),
                screen.mouse_protocol_encoding(),
            )
            .unwrap_or_default()
        }
        // egui reports motion without modifiers or a button, so a drag carries
        // the ones its press did; free motion carries none.
        Event::PointerMoved(pos) => {
            let Some((column, row)) = pointer_cell(*pos, screen, rect, cell) else {
                return Vec::new();
            };
            let (kind, modifiers) = match mouse.held {
                Some((button, modifiers)) => (MouseEventKind::Drag(button), modifiers),
                None => (MouseEventKind::Moved, egui::Modifiers::NONE),
            };
            let moved = MouseEvent {
                kind,
                column,
                row,
                modifiers: key_modifiers(modifiers),
            };
            // Motion arrives per pixel; one cell earns one report.
            if mouse.last_motion.replace(moved) == Some(moved) {
                return Vec::new();
            }
            encode_mouse(
                moved,
                screen.mouse_protocol_mode(),
                screen.mouse_protocol_encoding(),
            )
            .unwrap_or_default()
        }
        Event::MouseWheel {
            unit,
            delta,
            modifiers,
            ..
        } => {
            let Some((column, row)) = pointer_cell(pointer, screen, rect, cell) else {
                return Vec::new();
            };
            let (rows, _) = screen.size();
            let (columns, lines) = mouse.accumulate(*delta, *unit, cell, rows);
            let mode = screen.mouse_protocol_mode();
            let encoding = screen.mouse_protocol_encoding();
            let modifiers = key_modifiers(*modifiers);
            let mut bytes = Vec::new();
            for (steps, positive, negative) in [
                (lines, MouseEventKind::ScrollUp, MouseEventKind::ScrollDown),
                (
                    columns,
                    MouseEventKind::ScrollRight,
                    MouseEventKind::ScrollLeft,
                ),
            ] {
                if steps == 0 {
                    continue;
                }
                let Some(report) = encode_mouse(
                    MouseEvent {
                        kind: if steps > 0 { positive } else { negative },
                        column,
                        row,
                        modifiers,
                    },
                    mode,
                    encoding,
                ) else {
                    continue;
                };
                // Each whole step is one report, so a long swipe travels as far
                // as it was pushed.
                for _ in 0..steps.unsigned_abs() {
                    bytes.extend_from_slice(&report);
                }
            }
            bytes
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

fn mouse_button(button: egui::PointerButton) -> Option<MouseButton> {
    match button {
        egui::PointerButton::Primary => Some(MouseButton::Left),
        egui::PointerButton::Middle => Some(MouseButton::Middle),
        egui::PointerButton::Secondary => Some(MouseButton::Right),
        _ => None,
    }
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
    use crossterm::event::MouseButton;

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
            &mut Mouse::default(),
        )
    }

    /// A 24x80 terminal of 8x16 cells with the pointer resting on cell (0, 0).
    fn encode_pointer(event: &Event, screen: &vt100::Screen, mouse: &mut Mouse) -> Vec<u8> {
        encode_event(
            event,
            screen,
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(640.0, 384.0)),
            egui::vec2(8.0, 16.0),
            egui::pos2(4.0, 8.0),
            mouse,
        )
    }

    fn pointer_button(pressed: bool, modifiers: egui::Modifiers) -> Event {
        Event::PointerButton {
            pos: egui::pos2(4.0, 8.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        }
    }

    #[test]
    fn wheel_points_accumulate_into_lines() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1000h\x1b[?1006h");
        // A trackpad reports point deltas far smaller than a cell, so scroll
        // distance must follow the pixels travelled, not the event count.
        // egui's positive y scrolls up, so a downward swipe is negative.
        let wheel = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -6.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        };
        let mut mouse = Mouse::default();
        let reports: Vec<Vec<u8>> = (0..3)
            .map(|_| encode_pointer(&wheel, parser.screen(), &mut mouse))
            .collect();
        assert_eq!(
            reports,
            vec![Vec::new(), Vec::new(), b"\x1b[<65;1;1M".to_vec()],
            "three sixth-of-a-cell deltas are one line, not three"
        );
    }

    #[test]
    fn press_encoding_matches_encode_mouse() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        // X10 reports presses without modifier bits.
        parser.process(b"\x1b[?9h");
        let screen = parser.screen();
        let expected = encode_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 0,
                row: 0,
                modifiers: KeyModifiers::SHIFT,
            },
            screen.mouse_protocol_mode(),
            screen.mouse_protocol_encoding(),
        );
        assert_eq!(
            Some(encode_pointer(
                &pointer_button(true, egui::Modifiers::SHIFT),
                screen,
                &mut Mouse::default()
            )),
            expected
        );
    }

    #[test]
    fn drag_is_forwarded_in_button_motion_mode() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1002h\x1b[?1006h");
        let screen = parser.screen();
        let mut mouse = Mouse::default();
        assert_eq!(
            encode_pointer(
                &pointer_button(true, egui::Modifiers::NONE),
                screen,
                &mut mouse
            ),
            b"\x1b[<0;1;1M"
        );
        // Motion while the button is held is a drag report, button bit plus 32.
        assert_eq!(
            encode_pointer(
                &Event::PointerMoved(egui::pos2(20.0, 24.0)),
                screen,
                &mut mouse
            ),
            b"\x1b[<32;3;2M"
        );
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

    #[test]
    fn unmodified_letter_keeps_its_text() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        // Without Option the key press encodes nothing and the text is the only
        // source of the character, so the filter must leave it alone.
        let events = vec![
            press(Key::A, egui::Modifiers::NONE),
            Event::Text("a".to_owned()),
        ];
        let encoded: Vec<Vec<u8>> = terminal_events(&events)
            .map(|event| encode(event, screen))
            .filter(|bytes| !bytes.is_empty())
            .collect();
        assert_eq!(encoded, vec![b"a".to_vec()]);
    }
}
