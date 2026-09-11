use super::*;
use crate::server::ReportingSender;
use std::io::{self, Read};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub(super) fn read_pty(
    mut reader: Box<dyn Read + Send>,
    session: Arc<Session>,
    events: ReportingSender<SessionEvent>,
) {
    let mut buffer = [0_u8; 8192];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        if read == 0 {
            break;
        }
        if events
            .send(SessionEvent::Output {
                id: session.summary.id,
                bytes: buffer[..read].to_vec(),
            })
            .is_err()
        {
            break;
        }
    }
    let mut done = session.reader_done.lock().unwrap();
    *done = true;
    session.reader_changed.notify_all();
}

pub(super) fn wait_for_child(
    child: &mut (dyn portable_pty::Child + Send + Sync),
    session: Arc<Session>,
    events: ReportingSender<SessionEvent>,
) {
    let phase = match child.wait() {
        Ok(status) => SessionPhase::Exited {
            code: Some(status.exit_code()),
            signal: status.signal().map(str::to_owned),
        },
        Err(error) => SessionPhase::Exited {
            code: None,
            signal: Some(error.to_string()),
        },
    };
    let mut reader_done = session.reader_done.lock().unwrap();
    while !*reader_done {
        reader_done = session.reader_changed.wait(reader_done).unwrap();
    }
    drop(reader_done);
    while group_exists(session.pgid).unwrap_or(true) {
        thread::park_timeout(Duration::from_millis(5));
    }
    let _ = events.send(SessionEvent::Exited {
        id: session.summary.id,
        phase,
    });
}
