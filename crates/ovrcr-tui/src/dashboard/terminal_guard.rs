use super::PANIC_TERMINAL_RESTORED;
use anyhow::{Context, Result};
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture,
};
use crossterm::{cursor, execute, terminal as crossterm_terminal};
use std::io::{self, Write};

pub struct TerminalGuard<W: Write = io::Stdout> {
    writer: W,
    pub(super) raw: bool,
    pub(super) alternate: bool,
    pub(super) mouse: bool,
    pub(super) cursor_hidden: bool,
    pub(super) bracketed_paste: bool,
}

impl TerminalGuard<io::Stdout> {
    pub fn enter() -> Result<Self> {
        let mut guard = Self {
            writer: io::stdout(),
            raw: false,
            alternate: false,
            mouse: false,
            cursor_hidden: false,
            bracketed_paste: false,
        };
        crossterm_terminal::enable_raw_mode().context("enable raw terminal mode")?;
        guard.raw = true;
        guard.enter_modes()?;
        Ok(guard)
    }
}

impl<W: Write> TerminalGuard<W> {
    #[cfg(test)]
    pub fn enter_with_writer(writer: W) -> Result<Self> {
        let mut guard = Self::with_writer(writer);
        guard.enter_modes()?;
        Ok(guard)
    }

    pub fn with_writer(writer: W) -> Self {
        Self {
            writer,
            raw: false,
            alternate: false,
            mouse: false,
            cursor_hidden: false,
            bracketed_paste: false,
        }
    }

    pub fn writer_mut(&mut self) -> &mut W {
        &mut self.writer
    }

    fn enter_modes(&mut self) -> Result<()> {
        execute!(self.writer, crossterm_terminal::EnterAlternateScreen)
            .context("enter alternate screen")?;
        self.alternate = true;
        execute!(self.writer, EnableBracketedPaste).context("enable bracketed paste")?;
        self.bracketed_paste = true;
        execute!(self.writer, EnableMouseCapture).context("enable dashboard mouse")?;
        self.mouse = true;
        execute!(self.writer, EnableFocusChange).context("enable dashboard focus reporting")?;
        execute!(self.writer, cursor::Hide).context("hide dashboard cursor")?;
        self.cursor_hidden = true;
        Ok(())
    }

    pub fn restore_before<F: FnOnce()>(&mut self, prior: F) {
        self.restore();
        prior();
    }

    pub(super) fn mark_mouse(&mut self, enabled: bool) {
        self.mouse = enabled;
    }

    fn restore(&mut self) {
        // The dashboard loop toggles capture on its own, so do not trust the
        // flag: disabling capture is idempotent and cheap.
        let _ = execute!(self.writer, DisableMouseCapture);
        self.mouse = false;
        let _ = execute!(self.writer, DisableFocusChange);
        if self.bracketed_paste {
            let _ = execute!(self.writer, DisableBracketedPaste);
            self.bracketed_paste = false;
        }
        if self.cursor_hidden {
            let _ = execute!(self.writer, cursor::Show);
            self.cursor_hidden = false;
        }
        if self.alternate {
            let _ = execute!(self.writer, crossterm_terminal::LeaveAlternateScreen);
            self.alternate = false;
        }
        if self.raw {
            let _ = crossterm_terminal::disable_raw_mode();
            self.raw = false;
        }
    }
}

impl<W: Write> Drop for TerminalGuard<W> {
    fn drop(&mut self) {
        let restored_by_panic_hook = PANIC_TERMINAL_RESTORED.with(|restored| {
            let was_restored = restored.get();
            restored.set(false);
            was_restored
        });
        if !restored_by_panic_hook {
            self.restore();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct InjectedWriter {
        output: Arc<Mutex<Vec<u8>>>,
        writes: Arc<Mutex<usize>>,
        fail_after: usize,
    }

    impl InjectedWriter {
        fn new(fail_after: usize) -> (Self, Arc<Mutex<Vec<u8>>>) {
            let output = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    output: Arc::clone(&output),
                    writes: Arc::new(Mutex::new(0)),
                    fail_after,
                },
                output,
            )
        }
    }

    impl Write for InjectedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let mut writes = self.writes.lock().unwrap();
            if *writes == self.fail_after {
                *writes += 1;
                return Err(io::Error::other("injected writer failure"));
            }
            *writes += 1;
            self.output.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn occurrences(bytes: &[u8], needle: &[u8]) -> usize {
        bytes
            .windows(needle.len())
            .filter(|window| *window == needle)
            .count()
    }

    #[test]
    fn restores_each_enabled_mode_once_on_normal_return_and_unwind() {
        let (writer, output) = InjectedWriter::new(usize::MAX);
        let guard = TerminalGuard::enter_with_writer(writer).unwrap();
        drop(guard);
        let bytes = output.lock().unwrap().clone();
        assert_eq!(occurrences(&bytes, b"\x1b[?1049l"), 1);
        assert_eq!(occurrences(&bytes, b"\x1b[?2004l"), 1);
        assert_eq!(occurrences(&bytes, b"\x1b[?1000l"), 1);
        assert_eq!(occurrences(&bytes, b"\x1b[?25h"), 1);

        let (writer, output) = InjectedWriter::new(usize::MAX);
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _guard = TerminalGuard::enter_with_writer(writer).unwrap();
            panic!("exercise unwind restoration");
        }));
        assert!(result.is_err());
        let bytes = output.lock().unwrap().clone();
        assert_eq!(occurrences(&bytes, b"\x1b[?1049l"), 1);
        assert_eq!(occurrences(&bytes, b"\x1b[?2004l"), 1);
        assert_eq!(occurrences(&bytes, b"\x1b[?1000l"), 1);
        assert_eq!(occurrences(&bytes, b"\x1b[?25h"), 1);
    }

    #[test]
    fn restores_before_prior_hook_callback() {
        let (writer, output) = InjectedWriter::new(usize::MAX);
        let mut guard = TerminalGuard::enter_with_writer(writer).unwrap();
        let observed = Arc::clone(&output);
        guard.restore_before(|| {
            let bytes = observed.lock().unwrap().clone();
            assert_eq!(occurrences(&bytes, b"\x1b[?1049l"), 1);
            assert_eq!(occurrences(&bytes, b"\x1b[?2004l"), 1);
            assert_eq!(occurrences(&bytes, b"\x1b[?1000l"), 1);
            assert_eq!(occurrences(&bytes, b"\x1b[?25h"), 1);
        });
        drop(guard);
        let bytes = output.lock().unwrap().clone();
        assert_eq!(occurrences(&bytes, b"\x1b[?1049l"), 1);
    }

    #[test]
    fn rolls_back_modes_when_a_later_enter_write_fails() {
        let (writer, output) = InjectedWriter::new(2);
        assert!(TerminalGuard::enter_with_writer(writer).is_err());
        let bytes = output.lock().unwrap().clone();
        assert_eq!(occurrences(&bytes, b"\x1b[?1049l"), 1);
    }
}
