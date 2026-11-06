use crate::tui::{KeyEncoding, encode_key, encode_mouse, encode_paste};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use eframe::egui::{self, Event, Key, MouseWheelUnit};

/// The mouse state [`encode_event`] carries between events: wheel distance too
/// small to report yet, the held button with the last report sent for it, and
/// the last motion report, so one cell is not reported twice.
#[derive(Default)]
pub struct Mouse {
    carry: egui::Vec2,
    held: Option<(MouseButton, MouseEvent)>,
    last_motion: Option<MouseEvent>,
}

/// Enhanced keys held by the outer GUI until their matching release reaches the
/// Dashboard. Aggregate macOS Command transitions can stand in for missing physical
/// Super edges; focus loss owns the state needed to release either form safely.
#[derive(Default)]
pub struct KeyboardState {
    kitty_enabled: bool,
    focus_reporting_enabled: bool,
    unfocused: bool,
    focus_quarantined: bool,
    left_super: bool,
    right_super: bool,
    physical_left_super: bool,
    physical_right_super: bool,
    aggregate_super: bool,
    synthetic_super: bool,
    digits: [Option<u8>; 10],
    command_controls: [Option<u8>; 2],
}

impl KeyboardState {
    fn super_held(&self) -> bool {
        self.left_super || self.right_super
    }

    /// A key-up for a shortcut begun in the terminal must be delivered even
    /// if egui moved focus to another GUI control before the physical release.
    pub fn tracks_release(&self, event: &Event) -> bool {
        match event {
            Event::ModifiersChanged(modifiers) => !modifiers.mac_cmd && self.aggregate_super,
            Event::Key {
                key,
                pressed: false,
                ..
            } => match key {
                Key::SuperLeft => self.physical_left_super,
                Key::SuperRight => self.physical_right_super,
                Key::Backspace => self.command_controls[0].is_some(),
                Key::Escape => self.command_controls[1].is_some(),
                _ => digit(*key)
                    .is_some_and(|value| self.digits[(value - u32::from(b'0')) as usize].is_some()),
            },
            _ => false,
        }
    }

    fn reset(&mut self) {
        let kitty_enabled = self.kitty_enabled;
        let focus_reporting_enabled = self.focus_reporting_enabled;
        let unfocused = self.unfocused;
        let focus_quarantined = self.focus_quarantined;
        *self = Self {
            kitty_enabled,
            focus_reporting_enabled,
            unfocused,
            focus_quarantined,
            ..Self::default()
        };
    }

    /// Track the mode observed from the Dashboard's output, not a user preference.
    /// Changing modes clears key state because the old terminal no longer expects
    /// the matching enhanced-protocol releases.
    pub fn set_kitty_enabled(&mut self, enabled: bool) {
        if self.kitty_enabled != enabled {
            self.reset();
            self.kitty_enabled = enabled;
        }
    }

    pub fn set_focus_reporting_enabled(&mut self, enabled: bool) {
        self.focus_reporting_enabled = enabled;
    }

    /// A clear modifier snapshot can rearm aggregate Super only after native
    /// input has refreshed winit's modifier state. Focus regain alone reports
    /// egui's reset snapshot, which does not prove that Command was released.
    pub fn observe_focus_frame(
        &mut self,
        focused: bool,
        modifiers: egui::Modifiers,
        events: &[Event],
    ) {
        self.unfocused = !focused;
        let last_gain = events
            .iter()
            .rposition(|event| matches!(event, Event::WindowFocused(true)));
        let observed_clear_modifiers = events.iter().enumerate().any(|(index, event)| {
            last_gain.is_none_or(|gain| index > gain) && native_modifier_observation(event)
        });
        if focused && !modifiers.mac_cmd && observed_clear_modifiers {
            self.focus_quarantined = false;
        }
    }

    fn release_all(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (digit, modifiers) in self.digits.iter_mut().enumerate() {
            if let Some(modifiers) = modifiers.take() {
                bytes.extend_from_slice(&kitty_key_event(
                    u32::from(b'0' + digit as u8),
                    modifiers,
                    3,
                ));
            }
        }
        for (index, codepoint) in [(0, 127), (1, 27)] {
            if let Some(modifiers) = self.command_controls[index].take() {
                bytes.extend_from_slice(&kitty_key_event(codepoint, modifiers, 3));
            }
        }
        for (left, codepoint) in [(true, 57444), (false, 57450)] {
            let held = if left {
                self.left_super
            } else {
                self.right_super
            };
            if held {
                if left {
                    self.left_super = false;
                } else {
                    self.right_super = false;
                }
                bytes.extend_from_slice(&kitty_key_event(
                    codepoint,
                    u8::from(self.super_held()) * 8,
                    3,
                ));
            }
        }
        self.physical_left_super = false;
        self.physical_right_super = false;
        self.aggregate_super = false;
        self.synthetic_super = false;
        bytes
    }
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
        // Only an event's dominant axis scrolls, and the idle axis keeps no
        // carry: a thumb never swipes straight, and lateral drift that survived
        // the swipe would eventually cross a column and scroll sideways.
        let vertical = delta.y.abs() >= delta.x.abs();
        let (steps, kept) = if vertical {
            (egui::vec2(0.0, steps.y), egui::vec2(0.0, self.carry.y))
        } else {
            (egui::vec2(steps.x, 0.0), egui::vec2(self.carry.x, 0.0))
        };
        if !steps.is_finite() {
            return (0, 0);
        }
        self.carry = kept + steps;
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
            repeat,
            modifiers,
            ..
        } if !modifiers.mac_cmd => {
            if modifiers.ctrl {
                // Raw control bytes lose repeat identity before reaching the Dashboard.
                if *key == Key::C && *repeat {
                    return Vec::new();
                }
                let name = key.name();
                if (modifiers.alt || modifiers.shift)
                    && name.len() == 1
                    && name.as_bytes()[0].is_ascii_alphabetic()
                {
                    return format!(
                        "\x1b[{};{}u",
                        name.as_bytes()[0].to_ascii_lowercase(),
                        5 + u8::from(modifiers.shift) + 2 * u8::from(modifiers.alt)
                    )
                    .into_bytes();
                }
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
            // A button change starts a new gesture.
            mouse.last_motion = None;
            let at = pointer_cell(*pos, screen, rect, cell);
            if *pressed {
                let Some((column, row)) = at else {
                    return Vec::new();
                };
                let down = MouseEvent {
                    kind: MouseEventKind::Down(button),
                    column,
                    row,
                    modifiers: key_modifiers(*modifiers),
                };
                mouse.held = Some((button, down));
                return report(down, screen);
            }
            // Nothing is held when the press landed on a GUI control rather than the
            // terminal, and the child must not see an `Up` without its `Down`. The
            // dashboard drops the same release.
            let Some((_, last)) = mouse.held.take_if(|(held, _)| *held == button) else {
                return Vec::new();
            };
            // A release off the terminal still ends its drag, at the cell the
            // gesture last reported, or the child app keeps a selection live.
            let release = at.map_or(
                MouseEvent {
                    kind: MouseEventKind::Up(button),
                    ..last
                },
                |(column, row)| MouseEvent {
                    kind: MouseEventKind::Up(button),
                    column,
                    row,
                    modifiers: key_modifiers(*modifiers),
                },
            );
            report(release, screen)
        }
        // egui reports motion without modifiers or a button, so a drag carries
        // the ones its press did; free motion carries none.
        Event::PointerMoved(pos) => {
            let Some((column, row)) = pointer_cell(*pos, screen, rect, cell) else {
                return Vec::new();
            };
            let (kind, modifiers) = match mouse.held {
                Some((button, press)) => (MouseEventKind::Drag(button), press.modifiers),
                None => (MouseEventKind::Moved, KeyModifiers::NONE),
            };
            let moved = MouseEvent {
                kind,
                column,
                row,
                modifiers,
            };
            // Motion arrives per pixel; one cell earns one report.
            if mouse.last_motion.replace(moved) == Some(moved) {
                return Vec::new();
            }
            let bytes = report(moved, screen);
            // A reported drag moves where a release off the terminal must land.
            if let Some((_, last)) = &mut mouse.held
                && !bytes.is_empty()
            {
                *last = moved;
            }
            bytes
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
            let modifiers = key_modifiers(*modifiers);
            let mut bytes = Vec::new();
            // A positive delta moves the content down and right, revealing what
            // lies above and to its left.
            for (steps, positive, negative) in [
                (lines, MouseEventKind::ScrollUp, MouseEventKind::ScrollDown),
                (
                    columns,
                    MouseEventKind::ScrollLeft,
                    MouseEventKind::ScrollRight,
                ),
            ] {
                if steps == 0 {
                    continue;
                }
                let report = report(
                    MouseEvent {
                        kind: if steps > 0 { positive } else { negative },
                        column,
                        row,
                        modifiers,
                    },
                    screen,
                );
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

/// Encode an outer-GUI input event for the Dashboard. Enhanced key events require
/// the observed Kitty mode; focus reports require the observed DEC 1004 mode. Other
/// text, paste and ordinary keys retain the existing input encoder.
pub fn encode_event_with_keyboard(
    event: &Event,
    screen: &vt100::Screen,
    rect: egui::Rect,
    cell: egui::Vec2,
    pointer: egui::Pos2,
    mouse: &mut Mouse,
    keyboard: &mut KeyboardState,
) -> Vec<u8> {
    match event {
        Event::WindowFocused(false) => {
            keyboard.unfocused = true;
            keyboard.focus_quarantined = true;
            let mut bytes = if keyboard.focus_reporting_enabled {
                b"\x1b[O".to_vec()
            } else {
                Vec::new()
            };
            if keyboard.kitty_enabled {
                bytes.extend_from_slice(&keyboard.release_all());
            }
            return bytes;
        }
        Event::WindowFocused(true) if keyboard.focus_reporting_enabled => {
            keyboard.unfocused = false;
            return b"\x1b[I".to_vec();
        }
        Event::WindowFocused(true) => {
            keyboard.unfocused = false;
            return Vec::new();
        }
        Event::ModifiersChanged(modifiers) if !modifiers.mac_cmd && !keyboard.unfocused => {
            keyboard.focus_quarantined = false;
        }
        _ => {}
    }
    if !keyboard.kitty_enabled {
        return encode_event(event, screen, rect, cell, pointer, mouse);
    }

    match event {
        Event::ModifiersChanged(modifiers) => {
            if keyboard.focus_quarantined && modifiers.mac_cmd {
                return Vec::new();
            }
            if keyboard.aggregate_super == modifiers.mac_cmd {
                return Vec::new();
            }
            keyboard.aggregate_super = modifiers.mac_cmd;
            if modifiers.mac_cmd {
                if keyboard.super_held() {
                    Vec::new()
                } else {
                    keyboard.synthetic_super = true;
                    keyboard.left_super = true;
                    kitty_key_event(57444, kitty_modifier_mask(*modifiers, false), 1)
                }
            } else if keyboard.synthetic_super {
                keyboard.synthetic_super = false;
                keyboard.left_super = false;
                kitty_key_event(57444, kitty_modifier_mask(*modifiers, false), 3)
            } else {
                let mut bytes = Vec::new();
                for (left, codepoint) in [(true, 57444), (false, 57450)] {
                    let physical = if left {
                        keyboard.physical_left_super
                    } else {
                        keyboard.physical_right_super
                    };
                    let held = if left {
                        keyboard.left_super
                    } else {
                        keyboard.right_super
                    };
                    if held && !physical {
                        if left {
                            keyboard.left_super = false;
                        } else {
                            keyboard.right_super = false;
                        }
                        bytes.extend_from_slice(&kitty_key_event(
                            codepoint,
                            kitty_modifier_mask(*modifiers, keyboard.super_held()),
                            3,
                        ));
                    }
                }
                bytes
            }
        }
        Event::Key {
            key: Key::SuperLeft | Key::SuperRight,
            pressed,
            repeat,
            modifiers,
            ..
        } => {
            let left = matches!(
                event,
                Event::Key {
                    key: Key::SuperLeft,
                    ..
                }
            );
            let held = if left {
                keyboard.left_super
            } else {
                keyboard.right_super
            };
            let physical = if left {
                keyboard.physical_left_super
            } else {
                keyboard.physical_right_super
            };
            let codepoint = if left { 57444 } else { 57450 };
            // If egui only reported the aggregate modifier edge first, its
            // synthetic left-Super event already represents the whole native
            // Command hold. Physical side events are duplicates until Cmd lifts.
            if keyboard.synthetic_super {
                return Vec::new();
            }
            if *pressed {
                if *repeat {
                    if physical && held {
                        return kitty_key_event(
                            codepoint,
                            kitty_modifier_mask(*modifiers, keyboard.super_held()),
                            2,
                        );
                    }
                    return Vec::new();
                }
                if keyboard.focus_quarantined && !keyboard.unfocused {
                    keyboard.focus_quarantined = false;
                }
                if left {
                    keyboard.physical_left_super = true;
                } else {
                    keyboard.physical_right_super = true;
                }
                if held {
                    return Vec::new();
                }
                let modifier_mask = kitty_modifier_mask(*modifiers, keyboard.super_held());
                if left {
                    keyboard.left_super = true;
                } else {
                    keyboard.right_super = true;
                }
                kitty_key_event(codepoint, modifier_mask, 1)
            } else if physical {
                if left {
                    keyboard.physical_left_super = false;
                } else {
                    keyboard.physical_right_super = false;
                }
                if !held {
                    return Vec::new();
                }
                let other_held = if left {
                    keyboard.right_super
                } else {
                    keyboard.left_super
                };
                if keyboard.aggregate_super && !other_held {
                    return Vec::new();
                }
                if left {
                    keyboard.left_super = false;
                } else {
                    keyboard.right_super = false;
                }
                kitty_key_event(
                    codepoint,
                    kitty_modifier_mask(*modifiers, keyboard.super_held()),
                    3,
                )
            } else {
                Vec::new()
            }
        }
        Event::Key {
            key: Key::Backspace | Key::Escape,
            pressed,
            repeat,
            modifiers,
            ..
        } if modifiers.mac_cmd
            || command_control_index(event)
                .is_some_and(|index| keyboard.command_controls[index].is_some()) =>
        {
            if keyboard.focus_quarantined && modifiers.mac_cmd && *pressed {
                return Vec::new();
            }
            let index = command_control_index(event).expect("matched Command control");
            let codepoint = if index == 0 { 127 } else { 27 };
            if *pressed {
                if let Some(held_modifiers) = keyboard.command_controls[index] {
                    if *repeat {
                        kitty_key_event(codepoint, held_modifiers, 2)
                    } else {
                        Vec::new()
                    }
                } else if modifiers.mac_cmd && !*repeat {
                    let modifier_mask = kitty_modifier_mask(*modifiers, true);
                    keyboard.command_controls[index] = Some(modifier_mask);
                    kitty_key_event(codepoint, modifier_mask, 1)
                } else {
                    Vec::new()
                }
            } else if let Some(modifier_mask) = keyboard.command_controls[index].take() {
                kitty_key_event(codepoint, modifier_mask, 3)
            } else {
                Vec::new()
            }
        }
        Event::Key {
            key,
            pressed,
            repeat,
            modifiers,
            ..
        } if digit(*key).is_some() => {
            let value = digit(*key).unwrap();
            let index = (value - u32::from(b'0')) as usize;
            let held = keyboard.digits[index].is_some();
            if *pressed {
                if modifiers.mac_cmd {
                    if keyboard.focus_quarantined {
                        return Vec::new();
                    }
                    let modifier_mask = kitty_modifier_mask(*modifiers, true);
                    if *repeat {
                        if held {
                            kitty_key_event(value, modifier_mask, 2)
                        } else {
                            Vec::new()
                        }
                    } else if !held {
                        keyboard.digits[index] = Some(modifier_mask);
                        kitty_key_event(value, modifier_mask, 1)
                    } else {
                        Vec::new()
                    }
                } else if held {
                    Vec::new()
                } else {
                    encode_event(event, screen, rect, cell, pointer, mouse)
                }
            } else if !*pressed && held {
                keyboard.digits[index] = None;
                kitty_key_event(
                    value,
                    kitty_modifier_mask(*modifiers, keyboard.super_held()),
                    3,
                )
            } else {
                encode_event(event, screen, rect, cell, pointer, mouse)
            }
        }
        _ => encode_event(event, screen, rect, cell, pointer, mouse),
    }
}

fn digit(key: Key) -> Option<u32> {
    match key {
        Key::Num0 => Some(u32::from(b'0')),
        Key::Num1 => Some(u32::from(b'1')),
        Key::Num2 => Some(u32::from(b'2')),
        Key::Num3 => Some(u32::from(b'3')),
        Key::Num4 => Some(u32::from(b'4')),
        Key::Num5 => Some(u32::from(b'5')),
        Key::Num6 => Some(u32::from(b'6')),
        Key::Num7 => Some(u32::from(b'7')),
        Key::Num8 => Some(u32::from(b'8')),
        Key::Num9 => Some(u32::from(b'9')),
        _ => None,
    }
}

fn command_control_index(event: &Event) -> Option<usize> {
    let Event::Key { key, .. } = event else {
        return None;
    };
    match key {
        Key::Backspace => Some(0),
        Key::Escape => Some(1),
        _ => None,
    }
}

fn kitty_modifier_mask(modifiers: egui::Modifiers, super_held: bool) -> u8 {
    u8::from(modifiers.shift)
        | (u8::from(modifiers.alt) << 1)
        | (u8::from(modifiers.ctrl) << 2)
        | (u8::from(super_held) << 3)
}

fn kitty_key_event(codepoint: u32, modifier_mask: u8, event_type: u8) -> Vec<u8> {
    format!("\x1b[{codepoint};{}:{event_type}u", modifier_mask + 1).into_bytes()
}

/// The native focus and modifier snapshot accompanying one GUI event batch.
pub struct TerminalFrameInput<'a> {
    pub events: &'a [Event],
    pub terminal_focused: bool,
    pub focused: bool,
    pub modifiers: egui::Modifiers,
}

/// The events of one frame that the terminal should receive.
///
/// macOS composes Option with a letter into a glyph egui delivers as `Text`
/// right after the `Key` press. [`encode_event`] answers that press with the
/// meta form, so forwarding the composed text as well would type twice.
///
/// winit accepts the first mouse, so the click that activates the window
/// arrives in the same frame, after the focus change. That press only
/// focuses the window: the Dashboard never sees it, and with nothing held
/// [`encode_event`] drops its release too.
pub fn terminal_events(events: &[Event]) -> impl Iterator<Item = &Event> {
    let activation = events
        .iter()
        .position(|event| matches!(event, Event::WindowFocused(true)))
        .and_then(|focus| {
            events[focus..]
                .iter()
                .position(|event| matches!(event, Event::PointerButton { pressed: true, .. }))
                .map(|offset| focus + offset)
        });
    let mut forwarded: Vec<usize> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            let composed = matches!(event, Event::Text(_))
                && index.checked_sub(1).is_some_and(|previous| {
                    matches!(
                        &events[previous],
                        Event::Key { key, pressed: true, modifiers, .. }
                            if meta_key(*key, *modifiers).is_some()
                    )
                });
            (!composed && Some(index) != activation).then_some(index)
        })
        .collect();

    // winit queues this synthetic clear before Focused(false). Move the focus
    // event ahead of that exact reset so Dashboard cancellation precedes the
    // Super release that the reset can synthesize.
    for (focus, event) in events.iter().enumerate() {
        if focus == 0
            || !matches!(event, Event::WindowFocused(false))
            || !matches!(
                &events[focus - 1],
                Event::ModifiersChanged(modifiers) if *modifiers == egui::Modifiers::NONE
            )
        {
            continue;
        }
        let Some(reset_position) = forwarded.iter().position(|&index| index == focus - 1) else {
            continue;
        };
        let Some(focus_position) = forwarded.iter().position(|&index| index == focus) else {
            continue;
        };
        forwarded.swap(reset_position, focus_position);
    }

    forwarded.into_iter().map(|index| &events[index])
}

/// Encode and dispatch the terminal-facing events from one native GUI frame.
/// The focus/modifier snapshot is observed only after every event was handled,
/// so a later clear state cannot make earlier stale events in this frame fresh.
pub fn encode_terminal_frame(
    frame: TerminalFrameInput<'_>,
    screen: &vt100::Screen,
    rect: egui::Rect,
    cell: egui::Vec2,
    pointer: egui::Pos2,
    mouse: &mut Mouse,
    keyboard: &mut KeyboardState,
) -> Vec<Vec<u8>> {
    let mut encoded = Vec::new();
    for event in terminal_events(frame.events) {
        if !frame.terminal_focused
            && !matches!(
                event,
                Event::PointerButton { .. }
                    | Event::MouseWheel { .. }
                    | Event::WindowFocused(true)
                    | Event::WindowFocused(false)
            )
            && !keyboard.tracks_release(event)
        {
            continue;
        }
        let bytes = encode_event_with_keyboard(event, screen, rect, cell, pointer, mouse, keyboard);
        if !bytes.is_empty() {
            encoded.push(bytes);
        }
    }
    keyboard.observe_focus_frame(frame.focused, frame.modifiers, frame.events);
    encoded
}

fn native_modifier_observation(event: &Event) -> bool {
    matches!(
        event,
        Event::Key { .. }
            | Event::PointerMoved(_)
            | Event::PointerButton { .. }
            | Event::MouseWheel { .. }
    )
}

/// One mouse report in the screen's own protocol mode and encoding. Empty when
/// the mode does not report that event or the cell is past the encoding's limit.
fn report(event: MouseEvent, screen: &vt100::Screen) -> Vec<u8> {
    encode_mouse(
        event,
        screen.mouse_protocol_mode(),
        screen.mouse_protocol_encoding(),
    )
    .unwrap_or_default()
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

    fn key_edge(key: Key, pressed: bool, repeat: bool, modifiers: egui::Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat,
            modifiers,
        }
    }

    fn encode_keyboard(
        event: &Event,
        screen: &vt100::Screen,
        keyboard: &mut KeyboardState,
        kitty_enabled: bool,
    ) -> Vec<u8> {
        keyboard.set_kitty_enabled(kitty_enabled);
        encode_event_with_keyboard(
            event,
            screen,
            egui::Rect::NOTHING,
            egui::vec2(8.0, 16.0),
            egui::pos2(0.0, 0.0),
            &mut Mouse::default(),
            keyboard,
        )
    }

    fn command() -> egui::Modifiers {
        egui::Modifiers {
            mac_cmd: true,
            command: true,
            ..Default::default()
        }
    }

    #[test]
    fn kitty_super_and_command_digit_edges_are_encoded_only_in_enhanced_mode() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();

        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperLeft, true, false, command()),
                screen,
                &mut keyboard,
                false,
            ),
            Vec::<u8>::new(),
            "the GUI must not claim bare-modifier support before the Dashboard enables Kitty mode"
        );
        assert!(
            encode_keyboard(
                &key_edge(Key::Num1, true, false, command()),
                screen,
                &mut keyboard,
                false,
            )
            .is_empty()
        );

        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperLeft, true, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:1u"
        );
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::Num1, true, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[49;9:1u"
        );
        assert!(keyboard.tracks_release(&key_edge(Key::Num1, false, false, command(),)));
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::Num1, true, true, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[49;9:2u"
        );
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::Num1, false, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[49;9:3u"
        );
        assert!(!keyboard.tracks_release(&key_edge(Key::Num1, false, false, command(),)));
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperLeft, false, false, egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:3u"
        );
    }

    #[test]
    fn native_modifier_change_alone_emits_a_super_hold_and_release() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:1u"
        );
        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:3u"
        );
    }

    #[test]
    fn aggregate_and_physical_super_edges_are_deduplicated_in_either_order() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();

        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:1u"
        );
        assert!(
            encode_keyboard(
                &key_edge(Key::SuperLeft, true, false, command()),
                screen,
                &mut keyboard,
                true,
            )
            .is_empty()
        );
        assert!(
            encode_keyboard(
                &key_edge(Key::SuperRight, true, false, command()),
                screen,
                &mut keyboard,
                true,
            )
            .is_empty()
        );
        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:3u"
        );
        assert!(
            encode_keyboard(
                &key_edge(Key::SuperLeft, false, false, egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            )
            .is_empty()
        );
        assert!(
            encode_keyboard(
                &key_edge(Key::SuperRight, false, false, egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            )
            .is_empty()
        );

        // Winit can report the physical edge first. Preserve it, then keep its
        // release live until the matching aggregate modifier update arrives.
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperLeft, true, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:1u"
        );
        assert!(
            encode_keyboard(
                &Event::ModifiersChanged(command()),
                screen,
                &mut keyboard,
                true,
            )
            .is_empty()
        );
        assert!(
            encode_keyboard(
                &key_edge(Key::SuperLeft, false, false, command()),
                screen,
                &mut keyboard,
                true,
            )
            .is_empty()
        );
        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:3u"
        );

        // And in the opposite release order, aggregate false cannot release a
        // physical key whose native key-up has not arrived yet.
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperRight, true, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57450;1:1u"
        );
        assert!(
            encode_keyboard(
                &Event::ModifiersChanged(egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            )
            .is_empty()
        );
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperRight, false, false, egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57450;1:3u"
        );
    }

    #[test]
    fn command_backspace_and_escape_use_enhanced_edges_only_for_command() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        let backspace_press = key_edge(Key::Backspace, true, false, command());
        assert_eq!(
            encode_keyboard(&backspace_press, screen, &mut keyboard, true),
            b"\x1b[127;9:1u"
        );
        let backspace_repeat = key_edge(Key::Backspace, true, true, command());
        assert_eq!(
            encode_keyboard(&backspace_repeat, screen, &mut keyboard, true),
            b"\x1b[127;9:2u"
        );
        let backspace_release = key_edge(Key::Backspace, false, false, egui::Modifiers::NONE);
        assert!(keyboard.tracks_release(&backspace_release));
        assert_eq!(
            encode_keyboard(&backspace_release, screen, &mut keyboard, true),
            b"\x1b[127;9:3u"
        );
        assert!(!keyboard.tracks_release(&backspace_release));

        assert_eq!(
            encode_keyboard(
                &key_edge(Key::Escape, true, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[27;9:1u"
        );
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::Escape, false, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[27;9:3u"
        );
        assert!(
            encode_keyboard(
                &key_edge(Key::K, true, false, command()),
                screen,
                &mut keyboard,
                true,
            )
            .is_empty()
        );
    }

    #[test]
    fn aggregate_super_preserves_extra_modifiers_and_focus_loss_releases_state() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        let with_shift = egui::Modifiers {
            mac_cmd: true,
            command: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(with_shift),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;2:1u"
        );
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::Backspace, true, false, with_shift),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[127;10:1u"
        );
        assert_eq!(
            encode_keyboard(&Event::WindowFocused(false), screen, &mut keyboard, true),
            b"\x1b[127;10:3u\x1b[57444;1:3u"
        );
        assert!(
            encode_keyboard(
                &Event::ModifiersChanged(egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            )
            .is_empty()
        );
    }

    #[test]
    fn requested_focus_reports_precede_keyboard_releases() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        keyboard.set_kitty_enabled(true);
        keyboard.set_focus_reporting_enabled(true);
        assert_eq!(
            encode_event_with_keyboard(
                &Event::ModifiersChanged(command()),
                screen,
                egui::Rect::NOTHING,
                egui::vec2(8.0, 16.0),
                egui::pos2(0.0, 0.0),
                &mut Mouse::default(),
                &mut keyboard,
            ),
            b"\x1b[57444;1:1u"
        );
        assert!(keyboard.tracks_release(&Event::ModifiersChanged(egui::Modifiers::NONE)));
        assert_eq!(
            encode_event_with_keyboard(
                &Event::WindowFocused(false),
                screen,
                egui::Rect::NOTHING,
                egui::vec2(8.0, 16.0),
                egui::pos2(0.0, 0.0),
                &mut Mouse::default(),
                &mut keyboard,
            ),
            b"\x1b[O\x1b[57444;1:3u"
        );
        assert_eq!(
            encode_event_with_keyboard(
                &Event::WindowFocused(true),
                screen,
                egui::Rect::NOTHING,
                egui::vec2(8.0, 16.0),
                egui::pos2(0.0, 0.0),
                &mut Mouse::default(),
                &mut keyboard,
            ),
            b"\x1b[I"
        );
        assert!(!keyboard.tracks_release(&Event::ModifiersChanged(egui::Modifiers::NONE)));
    }

    #[test]
    fn stale_aggregate_command_after_focus_regain_does_not_restart_a_hold() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        keyboard.set_focus_reporting_enabled(true);

        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:1u"
        );
        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:3u"
        );
        assert_eq!(
            encode_keyboard(&Event::WindowFocused(false), screen, &mut keyboard, true),
            b"\x1b[O"
        );
        assert_eq!(
            encode_keyboard(&Event::WindowFocused(true), screen, &mut keyboard, true),
            b"\x1b[I"
        );
        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(command()),
                screen,
                &mut keyboard,
                true,
            ),
            Vec::<u8>::new(),
            "winit can restore Cmd from a mouse event without a fresh Super key press"
        );
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::Num1, true, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            Vec::<u8>::new(),
            "a stale aggregate must not make a digit into a Command shortcut"
        );
    }

    #[test]
    fn focused_native_clear_frame_rearms_after_dispatch() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        keyboard.set_kitty_enabled(true);
        keyboard.set_focus_reporting_enabled(true);
        let command = command();

        assert_eq!(
            encode_keyboard(&Event::WindowFocused(false), screen, &mut keyboard, true),
            b"\x1b[O"
        );
        let gain = [Event::WindowFocused(true)];
        assert_eq!(
            encode_terminal_frame(
                TerminalFrameInput {
                    events: &gain,
                    terminal_focused: true,
                    focused: true,
                    modifiers: egui::Modifiers::NONE,
                },
                screen,
                egui::Rect::NOTHING,
                egui::vec2(8.0, 16.0),
                egui::pos2(0.0, 0.0),
                &mut Mouse::default(),
                &mut keyboard,
            ),
            vec![b"\x1b[I".to_vec()]
        );

        let stale = [
            Event::ModifiersChanged(command),
            Event::PointerMoved(egui::pos2(0.0, 0.0)),
        ];
        assert!(
            encode_terminal_frame(
                TerminalFrameInput {
                    events: &stale,
                    terminal_focused: true,
                    focused: true,
                    modifiers: command,
                },
                screen,
                egui::Rect::NOTHING,
                egui::vec2(8.0, 16.0),
                egui::pos2(0.0, 0.0),
                &mut Mouse::default(),
                &mut keyboard,
            )
            .is_empty()
        );

        let clear = [Event::PointerMoved(egui::pos2(0.0, 0.0))];
        assert!(
            encode_terminal_frame(
                TerminalFrameInput {
                    events: &clear,
                    terminal_focused: true,
                    focused: true,
                    modifiers: egui::Modifiers::NONE,
                },
                screen,
                egui::Rect::NOTHING,
                egui::vec2(8.0, 16.0),
                egui::pos2(0.0, 0.0),
                &mut Mouse::default(),
                &mut keyboard,
            )
            .is_empty()
        );

        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(command),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:1u"
        );
    }

    #[test]
    fn native_input_before_focus_regain_cannot_rearm_command() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        keyboard.set_kitty_enabled(true);
        let command = command();

        let batched_focus_change = [
            Event::PointerMoved(egui::pos2(0.0, 0.0)),
            Event::WindowFocused(false),
            Event::WindowFocused(true),
        ];
        encode_terminal_frame(
            TerminalFrameInput {
                events: &batched_focus_change,
                terminal_focused: true,
                focused: true,
                modifiers: egui::Modifiers::NONE,
            },
            screen,
            egui::Rect::NOTHING,
            egui::vec2(8.0, 16.0),
            egui::pos2(0.0, 0.0),
            &mut Mouse::default(),
            &mut keyboard,
        );
        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(command),
                screen,
                &mut keyboard,
                true,
            ),
            Vec::<u8>::new(),
            "only input after the latest focus gain can rearm aggregate Cmd"
        );

        let after_gain = [Event::PointerMoved(egui::pos2(0.0, 0.0))];
        encode_terminal_frame(
            TerminalFrameInput {
                events: &after_gain,
                terminal_focused: true,
                focused: true,
                modifiers: egui::Modifiers::NONE,
            },
            screen,
            egui::Rect::NOTHING,
            egui::vec2(8.0, 16.0),
            egui::pos2(0.0, 0.0),
            &mut Mouse::default(),
            &mut keyboard,
        );
        assert_eq!(
            encode_keyboard(
                &Event::ModifiersChanged(command),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:1u"
        );
    }

    #[test]
    fn physical_super_press_rearms_after_focus_loss() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        keyboard.set_kitty_enabled(true);
        assert!(
            encode_keyboard(&Event::WindowFocused(false), screen, &mut keyboard, true).is_empty()
        );
        assert!(
            encode_keyboard(&Event::WindowFocused(true), screen, &mut keyboard, true).is_empty()
        );
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperLeft, true, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:1u"
        );
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperLeft, false, false, egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:3u"
        );
    }

    #[test]
    fn both_super_keys_remain_held_until_the_last_release() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperLeft, true, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;1:1u"
        );
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperRight, true, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57450;9:1u"
        );
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperLeft, false, false, command()),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57444;9:3u"
        );
        assert_eq!(
            encode_keyboard(
                &key_edge(Key::SuperRight, false, false, egui::Modifiers::NONE),
                screen,
                &mut keyboard,
                true,
            ),
            b"\x1b[57450;1:3u"
        );
    }

    #[test]
    fn focus_loss_releases_a_held_digit_before_the_super_key() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        encode_keyboard(
            &key_edge(Key::SuperLeft, true, false, command()),
            screen,
            &mut keyboard,
            true,
        );
        encode_keyboard(
            &key_edge(Key::Num1, true, false, command()),
            screen,
            &mut keyboard,
            true,
        );
        assert_eq!(
            encode_keyboard(&Event::WindowFocused(false), screen, &mut keyboard, true,),
            b"\x1b[49;9:3u\x1b[57444;1:3u"
        );
        assert_eq!(
            encode_keyboard(&Event::WindowFocused(false), screen, &mut keyboard, true,),
            Vec::<u8>::new(),
            "the same focus loss cannot send duplicate releases"
        );
    }

    #[test]
    fn focus_moved_to_another_control_keeps_the_matching_key_release() {
        let parser = vt100::Parser::new(24, 80, 0);
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        encode_keyboard(
            &key_edge(Key::SuperLeft, true, false, command()),
            screen,
            &mut keyboard,
            true,
        );
        let release = key_edge(Key::SuperLeft, false, false, egui::Modifiers::NONE);
        assert!(keyboard.tracks_release(&release));
        assert_eq!(
            encode_keyboard(&release, screen, &mut keyboard, true),
            b"\x1b[57444;1:3u"
        );
        assert!(!keyboard.tracks_release(&release));
    }

    #[test]
    fn enhanced_adapter_preserves_ordinary_input_and_drops_other_command_shortcuts() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?2004h");
        let screen = parser.screen();
        let mut keyboard = KeyboardState::default();
        let mut mouse = Mouse::default();
        let encode = |event: &Event, keyboard: &mut KeyboardState, mouse: &mut Mouse| {
            keyboard.set_kitty_enabled(true);
            encode_event_with_keyboard(
                event,
                screen,
                egui::Rect::NOTHING,
                egui::vec2(8.0, 16.0),
                egui::pos2(0.0, 0.0),
                mouse,
                keyboard,
            )
        };

        assert_eq!(
            encode(
                &key_edge(Key::Num2, true, false, egui::Modifiers::CTRL),
                &mut keyboard,
                &mut mouse,
            ),
            b"\0",
            "enhanced keyboard support must keep ordinary control digits intact"
        );
        assert_eq!(
            encode(&Event::Text("typed".into()), &mut keyboard, &mut mouse,),
            b"typed"
        );
        assert_eq!(
            encode(&Event::Paste("pasted".into()), &mut keyboard, &mut mouse,),
            b"\x1b[200~pasted\x1b[201~"
        );
        assert!(
            encode(
                &key_edge(Key::K, true, false, command()),
                &mut keyboard,
                &mut mouse,
            )
            .is_empty()
        );
    }

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

    fn pointer_button(pos: egui::Pos2, pressed: bool, modifiers: egui::Modifiers) -> Event {
        Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        }
    }

    fn wheel(unit: egui::MouseWheelUnit, delta: egui::Vec2) -> Event {
        Event::MouseWheel {
            unit,
            delta,
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn wheel_points_accumulate_into_lines() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1000h\x1b[?1006h");
        // A trackpad reports point deltas far smaller than a cell, so scroll
        // distance must follow the pixels travelled, not the event count.
        // egui's positive y moves the content down, revealing what is above.
        let down = wheel(egui::MouseWheelUnit::Point, egui::vec2(0.0, -6.0));
        let mut mouse = Mouse::default();
        let reports: Vec<Vec<u8>> = (0..3)
            .map(|_| encode_pointer(&down, parser.screen(), &mut mouse))
            .collect();
        assert_eq!(
            reports,
            vec![Vec::new(), Vec::new(), b"\x1b[<65;1;1M".to_vec()],
            "three sixth-of-a-cell deltas are one line, not three"
        );
        // Positive x moves the content right, revealing what is to its left,
        // which is ScrollLeft (66); negative x is ScrollRight (67).
        assert_eq!(
            encode_pointer(
                &wheel(egui::MouseWheelUnit::Line, egui::vec2(1.0, 0.0)),
                parser.screen(),
                &mut mouse
            ),
            b"\x1b[<66;1;1M"
        );
        assert_eq!(
            encode_pointer(
                &wheel(egui::MouseWheelUnit::Line, egui::vec2(-1.0, 0.0)),
                parser.screen(),
                &mut mouse
            ),
            b"\x1b[<67;1;1M"
        );
    }

    #[test]
    fn wheel_ignores_off_axis_drift() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1000h\x1b[?1006h");
        // A thumb never swipes perfectly straight. Only the dominant axis of an
        // event scrolls, so lateral drift must not accumulate across a swipe
        // until it crosses a column and injects a horizontal report.
        let drifting = wheel(egui::MouseWheelUnit::Point, egui::vec2(0.5, -6.0));
        let mut mouse = Mouse::default();
        let reports: Vec<Vec<u8>> = (0..10)
            .map(|_| encode_pointer(&drifting, parser.screen(), &mut mouse))
            .filter(|bytes| !bytes.is_empty())
            .collect();
        assert_eq!(reports, vec![b"\x1b[<65;1;1M".to_vec(); 3]);
        assert_eq!(
            mouse.carry.x, 0.0,
            "ten events of half a point sideways must leave no horizontal carry"
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
                &pointer_button(egui::pos2(4.0, 8.0), true, egui::Modifiers::SHIFT),
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
                &pointer_button(egui::pos2(4.0, 8.0), true, egui::Modifiers::NONE),
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
    fn release_with_nothing_held_reports_nothing() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1002h\x1b[?1006h");
        let screen = parser.screen();
        let mut mouse = Mouse::default();
        // The press landed on a GUI control outside the terminal, so nothing is held.
        assert_eq!(
            encode_pointer(
                &pointer_button(egui::pos2(-10.0, -10.0), true, egui::Modifiers::NONE),
                screen,
                &mut mouse
            ),
            Vec::<u8>::new()
        );
        // Releasing over the terminal must not hand the child an `Up` without a `Down`.
        assert_eq!(
            encode_pointer(
                &pointer_button(egui::pos2(4.0, 8.0), false, egui::Modifiers::NONE),
                screen,
                &mut mouse
            ),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn the_click_that_activates_the_window_only_focuses_it() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1000h\x1b[?1006h");
        let screen = parser.screen();
        let mut mouse = Mouse::default();
        let mut forward = |events: &[Event]| -> Vec<u8> {
            terminal_events(events)
                .flat_map(|event| encode_pointer(event, screen, &mut mouse))
                .collect()
        };
        let at = egui::pos2(4.0, 8.0);
        // winit reports the focus change, then the motion and press of the
        // activating click, in one frame.
        let activation = [
            Event::WindowFocused(true),
            Event::PointerMoved(at),
            pointer_button(at, true, egui::Modifiers::NONE),
        ];
        assert_eq!(forward(&activation), Vec::<u8>::new());
        // Its release in a later frame has no press to end.
        assert_eq!(
            forward(&[pointer_button(at, false, egui::Modifiers::NONE)]),
            Vec::<u8>::new()
        );
        // The next click in the focused window reaches the terminal.
        assert_eq!(
            forward(&[
                pointer_button(at, true, egui::Modifiers::NONE),
                pointer_button(at, false, egui::Modifiers::NONE),
            ]),
            b"\x1b[<0;1;1M\x1b[<0;1;1m"
        );
        // Focus regained without a click (Cmd-Tab) leaves the next frame's click alone.
        assert_eq!(forward(&[Event::WindowFocused(true)]), Vec::<u8>::new());
        assert_eq!(
            forward(&[pointer_button(at, true, egui::Modifiers::NONE)]),
            b"\x1b[<0;1;1M"
        );
    }

    #[test]
    fn focus_loss_precedes_winit_modifier_reset_in_the_forwarded_frame() {
        let events = [
            press(Key::A, egui::Modifiers::NONE),
            Event::ModifiersChanged(egui::Modifiers::NONE),
            Event::WindowFocused(false),
        ];
        let forwarded: Vec<&Event> = terminal_events(&events).collect();
        assert_eq!(forwarded.len(), 3);
        assert!(matches!(forwarded[0], Event::Key { key: Key::A, .. }));
        assert!(matches!(forwarded[1], Event::WindowFocused(false)));
        assert!(matches!(
            forwarded[2],
            Event::ModifiersChanged(modifiers) if *modifiers == egui::Modifiers::NONE
        ));
    }

    #[test]
    fn drag_released_outside_ends_at_the_last_reported_cell() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1002h\x1b[?1006h");
        let screen = parser.screen();
        let mut mouse = Mouse::default();
        // Press on cell (2, 1), drag to (3, 1).
        assert_eq!(
            encode_pointer(
                &pointer_button(egui::pos2(20.0, 24.0), true, egui::Modifiers::NONE),
                screen,
                &mut mouse
            ),
            b"\x1b[<0;3;2M"
        );
        assert_eq!(
            encode_pointer(
                &Event::PointerMoved(egui::pos2(28.0, 24.0)),
                screen,
                &mut mouse
            ),
            b"\x1b[<32;4;2M"
        );
        // Releasing off the terminal must still end the drag where it left it,
        // or the child app keeps its selection live forever.
        assert_eq!(
            encode_pointer(
                &pointer_button(egui::pos2(-10.0, -10.0), false, egui::Modifiers::NONE),
                screen,
                &mut mouse
            ),
            b"\x1b[<0;4;2m"
        );
    }

    #[test]
    fn mixed_alphabetic_controls_keep_alt_and_shift() {
        let parser = vt100::Parser::new(24, 80, 0);
        for (alt, shift, parameter) in [(true, false, 7), (false, true, 6), (true, true, 8)] {
            for key in [Key::C, Key::U, Key::K, Key::W] {
                let modifiers = egui::Modifiers {
                    ctrl: true,
                    alt,
                    shift,
                    ..Default::default()
                };
                assert_eq!(
                    encode(&press(key, modifiers), parser.screen()),
                    format!(
                        "\x1b[{};{parameter}u",
                        key.name().as_bytes()[0].to_ascii_lowercase()
                    )
                    .as_bytes(),
                    "{key:?} {modifiers:?}"
                );
            }
        }
    }

    #[test]
    fn ctrl_c_repeat_cannot_become_another_press() {
        let parser = vt100::Parser::new(24, 80, 0);
        for (key, modifiers, pressed, repeat, expected) in [
            (Key::C, egui::Modifiers::CTRL, true, false, vec![3]),
            (Key::C, egui::Modifiers::CTRL, true, true, vec![]),
            (Key::C, egui::Modifiers::CTRL, false, false, vec![]),
            (Key::C, egui::Modifiers::CTRL, false, true, vec![]),
            // A separate physical press still sends the normal terminal interrupt.
            (Key::C, egui::Modifiers::CTRL, true, false, vec![3]),
            (Key::A, egui::Modifiers::CTRL, true, true, vec![1]),
            (Key::G, egui::Modifiers::CTRL, true, true, vec![7]),
            (
                Key::ArrowLeft,
                egui::Modifiers::NONE,
                true,
                true,
                b"\x1b[D".to_vec(),
            ),
            (Key::Backspace, egui::Modifiers::NONE, true, true, vec![127]),
            (Key::Enter, egui::Modifiers::NONE, true, true, vec![13]),
            (Key::B, egui::Modifiers::ALT, true, true, b"\x1bb".to_vec()),
        ] {
            let event = Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat,
                modifiers,
            };
            assert_eq!(
                encode(&event, parser.screen()),
                expected,
                "{key:?} {modifiers:?} pressed={pressed} repeat={repeat}"
            );
        }
        let events = [
            press(Key::A, egui::Modifiers::NONE),
            Event::Text("a".into()),
            Event::Key {
                key: Key::A,
                physical_key: None,
                pressed: true,
                repeat: true,
                modifiers: egui::Modifiers::NONE,
            },
            Event::Text("a".into()),
        ];
        let bytes: Vec<u8> = terminal_events(&events)
            .flat_map(|event| encode(event, parser.screen()))
            .collect();
        assert_eq!(
            bytes, b"aa",
            "ordinary printable repetition remains text-driven"
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
