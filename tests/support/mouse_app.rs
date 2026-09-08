use anyhow::{Context, Result};
use std::io::{self, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;

pub fn run() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let fd = stdin.as_raw_fd();
    let mut original = MaybeUninit::<libc::termios>::uninit();
    anyhow::ensure!(
        unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } == 0,
        "tcgetattr failed"
    );
    let original = unsafe { original.assume_init() };
    let _guard = TermiosGuard { fd, original };
    let mut raw = original;
    unsafe { libc::cfmakeraw(&mut raw) };
    anyhow::ensure!(
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } == 0,
        "tcsetattr failed"
    );

    let mut stdout = stdout.lock();
    write!(stdout, "MOUSE_FIXTURE_READY\r\n")?;
    stdout.flush()?;

    let mut stdin = stdin.lock();
    let mut observed = Vec::new();
    let mut report = Vec::new();
    let mut checkpoint = 0u32;
    let mut byte = [0u8; 1];
    loop {
        stdin.read_exact(&mut byte).context("read fixture stdin")?;
        let b = byte[0];
        if !report.is_empty() {
            report.push(b);
            let complete_sgr = report.first() == Some(&0x1b)
                && report.get(1) == Some(&b'[')
                && report.get(2) == Some(&b'<')
                && matches!(b, b'M' | b'm');
            let aborted_prefix =
                (report.len() == 2 && b != b'[') || (report.len() == 3 && b != b'<');
            if complete_sgr || aborted_prefix {
                observed.extend_from_slice(&report);
                report.clear();
            }
            continue;
        }
        match b {
            0x1b => report.push(b),
            b'E' => {
                write!(stdout, "\x1b[?1002h\x1b[?1006hMOUSE_ENABLED\r\n")?;
                stdout.flush()?;
            }
            b'D' => {
                write!(stdout, "\x1b[?1002lMOUSE_DISABLED\r\n")?;
                stdout.flush()?;
            }
            b'Q' => {
                checkpoint += 1;
                let hex = observed
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                write!(stdout, "\r\nMOUSE_CHECK_{checkpoint}:{hex}:END\r\n")?;
                stdout.flush()?;
                observed.clear();
            }
            _ => observed.push(b),
        }
    }
}

struct TermiosGuard {
    fd: i32,
    original: libc::termios,
}

impl Drop for TermiosGuard {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}
