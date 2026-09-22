//! Keyboard encoding shared by the Dashboard and the Keystroke command.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    BackTab,
    Backspace,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,
    F(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// The bytes a terminal sends for `key`, or `None` when it has no encoding.
pub fn encode_key(key: Key, modifiers: Modifiers, application_cursor: bool) -> Option<Vec<u8>> {
    let modifier = modifier_param(modifiers);
    Some(match key {
        Key::Char(ch) if modifiers.ctrl => vec![control_byte(ch)?],
        Key::Char(ch) => {
            let mut bytes = Vec::new();
            if modifiers.alt {
                bytes.push(0x1b);
            }
            let mut encoded = [0; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            bytes
        }
        // Modified Enter uses the CSI-u form so Shift+Enter can insert a
        // newline instead of submitting; legacy terminals never deliver it.
        Key::Enter => match modifier {
            Some(modifier) => format!("\x1b[13;{modifier}u").into_bytes(),
            None => vec![b'\r'],
        },
        Key::Tab => vec![b'\t'],
        Key::BackTab => b"\x1b[Z".to_vec(),
        Key::Backspace if modifiers.alt => vec![0x1b, 0x7f],
        Key::Backspace => vec![0x7f],
        Key::Escape => vec![0x1b],
        Key::Left => cursor_sequence(application_cursor, modifier, b'D'),
        Key::Right => cursor_sequence(application_cursor, modifier, b'C'),
        Key::Up => cursor_sequence(application_cursor, modifier, b'A'),
        Key::Down => cursor_sequence(application_cursor, modifier, b'B'),
        Key::Home => cursor_sequence(false, modifier, b'H'),
        Key::End => cursor_sequence(false, modifier, b'F'),
        Key::Insert => tilde_sequence(2, modifier),
        Key::Delete => tilde_sequence(3, modifier),
        Key::PageUp => tilde_sequence(5, modifier),
        Key::PageDown => tilde_sequence(6, modifier),
        Key::F(number) => function_key(number)?,
    })
}

/// Parse a Keystroke name such as `:j:`, `:enter:` or `:ctrl-shift-enter:`.
///
/// Between the colons is one character or a lowercase key word, after
/// optional `ctrl-`, `alt-`, `shift-` prefixes in that order. Shift applies
/// only to named keys; a shifted character is written as that character.
pub fn parse_keystroke(name: &str) -> Result<(Key, Modifiers), String> {
    let invalid = || {
        format!(
            "invalid keystroke {name:?}: expected :KEY: such as :j:, :enter:, :down: or :ctrl-c:"
        )
    };
    let mut rest = name
        .strip_prefix(':')
        .and_then(|inner| inner.strip_suffix(':'))
        .filter(|inner| !inner.is_empty())
        .ok_or_else(invalid)?;
    let mut modifiers = Modifiers::default();
    for (prefix, flag) in [
        ("ctrl-", &mut modifiers.ctrl),
        ("alt-", &mut modifiers.alt),
        ("shift-", &mut modifiers.shift),
    ] {
        if let Some(stripped) = rest.strip_prefix(prefix).filter(|s| !s.is_empty()) {
            *flag = true;
            rest = stripped;
        }
    }
    let mut chars = rest.chars();
    let key = match (chars.next(), chars.next()) {
        (Some(ch), None) => Key::Char(ch),
        _ => match rest {
            "enter" => Key::Enter,
            "escape" => Key::Escape,
            "tab" if modifiers.shift => Key::BackTab,
            "tab" => Key::Tab,
            "backspace" => Key::Backspace,
            "space" => Key::Char(' '),
            "colon" => Key::Char(':'),
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            "insert" => Key::Insert,
            "delete" => Key::Delete,
            _ => match rest.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                Some(number @ 1..=12) if rest == format!("f{number}") => Key::F(number),
                _ => return Err(invalid()),
            },
        },
    };
    if (modifiers.shift && matches!(key, Key::Char(_)))
        || encode_key(key, modifiers, false).is_none()
    {
        return Err(invalid());
    }
    Ok((key, modifiers))
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
fn modifier_param(modifiers: Modifiers) -> Option<u8> {
    let param =
        1 + u8::from(modifiers.shift) + 2 * u8::from(modifiers.alt) + 4 * u8::from(modifiers.ctrl);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(name: &str, application_cursor: bool) -> Vec<u8> {
        let (key, modifiers) = parse_keystroke(name).unwrap();
        encode_key(key, modifiers, application_cursor).unwrap()
    }

    #[test]
    fn keystroke_names_encode_as_the_keyboard_would() {
        for (name, expected) in [
            (":j:", &b"j"[..]),
            (":J:", b"J"),
            (":enter:", b"\r"),
            (":escape:", b"\x1b"),
            (":tab:", b"\t"),
            (":shift-tab:", b"\x1b[Z"),
            (":backspace:", b"\x7f"),
            (":space:", b" "),
            (":colon:", b":"),
            (":ctrl-c:", b"\x03"),
            (":ctrl-g:", b"\x07"),
            (":alt-j:", b"\x1bj"),
            (":alt-down:", b"\x1b[1;3B"),
            (":ctrl-shift-enter:", b"\x1b[13;6u"),
            (":home:", b"\x1b[H"),
            (":pagedown:", b"\x1b[6~"),
            (":delete:", b"\x1b[3~"),
            (":f1:", b"\x1bOP"),
            (":f12:", b"\x1b[24~"),
        ] {
            assert_eq!(bytes(name, false), expected, "{name}");
        }
        assert_eq!(bytes(":down:", false), b"\x1b[B");
        assert_eq!(bytes(":down:", true), b"\x1bOB");
    }

    #[test]
    fn keystroke_names_are_strict() {
        for name in [
            ":hello:",
            ":Enter:",
            ":shift-j:",
            ":shift-ctrl-c:",
            ":alt-ctrl-c:",
            ":ctrl-ctrl-c:",
            ":ctrl-:",
            ":ctrl-1:",
            ":f0:",
            ":f13:",
            ":f01:",
            "::",
            "j",
            ":j",
            "enter",
        ] {
            assert!(parse_keystroke(name).is_err(), "{name} must be refused");
        }
    }
}
