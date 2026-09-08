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
