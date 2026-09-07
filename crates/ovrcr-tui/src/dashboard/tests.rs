use super::event_loop::{
    dashboard_hello_result, dashboard_message_channel, next_dashboard_message,
};
use crate::protocol::{ErrorCode, Response, ServerMessage};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn dashboard_surfaces_hello_refusal_and_reader_disconnect() {
    let refusal = ServerMessage::Response {
        request_id: 1,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "another dashboard is already connected".into(),
        },
    };
    let error = dashboard_hello_result(&refusal).unwrap_err().to_string();
    assert!(error.contains("dashboard hello failed"));
    assert!(error.contains("another dashboard is already connected"));

    let (sender, receiver) = dashboard_message_channel();
    drop(sender);
    let error = next_dashboard_message(&receiver).unwrap_err().to_string();
    assert_eq!(error, "dashboard connection lost");
}

#[test]
fn dashboard_output_wake_interrupts_idle_wait() {
    let (wake_receiver, mut wake_sender) = UnixStream::pair().unwrap();
    wake_receiver.set_nonblocking(true).unwrap();
    wake_sender.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        wake_sender.write_all(&[1]).unwrap();
    });

    let started = Instant::now();
    let activity = super::event_loop::wait_for_dashboard_activity(
        -1,
        Some(&wake_receiver),
        Duration::from_millis(500),
    )
    .unwrap();
    assert!(activity.server_ready);
    assert!(!activity.input_ready);
    assert!(started.elapsed() < Duration::from_millis(250));
}

#[test]
fn dashboard_wait_reports_input_and_output_together() {
    let (input_receiver, mut input_sender) = UnixStream::pair().unwrap();
    let (wake_receiver, mut wake_sender) = UnixStream::pair().unwrap();
    input_receiver.set_nonblocking(true).unwrap();
    input_sender.set_nonblocking(true).unwrap();
    wake_receiver.set_nonblocking(true).unwrap();
    wake_sender.set_nonblocking(true).unwrap();
    input_sender.write_all(&[1]).unwrap();
    wake_sender.write_all(&[1]).unwrap();

    let activity = super::event_loop::wait_for_dashboard_activity(
        input_receiver.as_raw_fd(),
        Some(&wake_receiver),
        Duration::from_millis(500),
    )
    .unwrap();
    assert!(activity.input_ready);
    assert!(activity.server_ready);
}
