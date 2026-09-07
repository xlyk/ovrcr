use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ovrcr::session::TerminalSize;
use ovrcr::task_manager::{TaskRequest, TaskResponse};
use ovrcr::task_tui::{TasksView, Transcript, draw_tasks};
use ovrcr::tasks::*;
use ovrcr::tui::{Dashboard, DashboardAction, InputMode};
use ratatui::{Terminal, backend::TestBackend};
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn ctrl(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}
fn spec() -> TaskSpec {
    TaskSpec {
        name: "review".into(),
        prompt: "hello\nworld".into(),
        target: TaskTarget::Scratch,
        schedule: Schedule::Interval { seconds: 60 },
        model: "provider/model".into(),
        thinking: "high".into(),
        timeout_seconds: 300,
    }
}
fn fixture() -> TasksView {
    let mut v = TasksView::default();
    v.receive(
        &TaskRequest::ListTasks,
        Ok(TaskResponse::Tasks(vec![Task {
            id: TaskId(1),
            spec: spec(),
            enabled: true,
            next_due_at: Some(60),
        }])),
    );
    v
}
#[test]
fn browse_opens_tasks_and_terminal_ctrl_t_is_literal() {
    let mut d = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
    d.ctrl('t');
    assert!(d.tasks.is_some());
    d.event_action(key(KeyCode::Esc));
    assert!(d.tasks.is_none());
    d.mode = InputMode::Terminal;
    assert_eq!(d.ctrl('t'), DashboardAction::PtyBytes(vec![20]));
    assert!(d.tasks.is_none());
}
#[test]
fn create_and_edit_every_field_and_multiline_prompt_in_terminal() {
    let mut v = TasksView::default();
    v.event(key(KeyCode::Char('n')));
    let values = [
        "review",
        "git",
        "ovrcr",
        "upstream",
        "release",
        "cron",
        "0 9 * * 1",
        "America/Los_Angeles",
        "provider/model",
        "high",
        "5m",
        "First 界\nSecond line",
    ];
    for value in values {
        v.event(ctrl('a'));
        v.event(Event::Paste(value.into()));
        v.event(key(KeyCode::Tab));
    }
    v.event(ctrl('s'));
    let TaskRequest::Create(created) = v.take_request().unwrap() else {
        panic!("create request")
    };
    assert_eq!(created.prompt, "First 界\nSecond line");
    assert_eq!(created.timeout_seconds, 300);
    assert_eq!(
        created.target,
        TaskTarget::Git {
            project: "ovrcr".into(),
            remote: "upstream".into(),
            branch: "release".into()
        }
    );
    assert_eq!(
        created.schedule,
        Schedule::Cron {
            expression: "0 9 * * 1".into(),
            timezone: "America/Los_Angeles".into()
        }
    );
    assert_eq!(
        (
            created.name.as_str(),
            created.model.as_str(),
            created.thinking.as_str()
        ),
        ("review", "provider/model", "high")
    );
    v.receive(&TaskRequest::Create(created.clone()), Ok(TaskResponse::Ok));
    v.receive(
        &TaskRequest::ListTasks,
        Ok(TaskResponse::Tasks(vec![Task {
            id: TaskId(9),
            spec: created.clone(),
            enabled: true,
            next_due_at: None,
        }])),
    );
    v.event(key(KeyCode::Char('e')));
    v.event(ctrl('s'));
    assert_eq!(
        v.take_request(),
        Some(TaskRequest::Update {
            id: TaskId(9),
            spec: created
        })
    );
}
#[test]
fn destructive_actions_require_confirmation_and_escape_cancels() {
    let mut v = fixture();
    v.event(key(KeyCode::Char('d')));
    assert!(v.take_request().is_none());
    v.event(key(KeyCode::Esc));
    assert!(v.take_request().is_none());
    v.event(key(KeyCode::Char('d')));
    v.event(key(KeyCode::Char('y')));
    assert_eq!(v.take_request(), Some(TaskRequest::Delete(TaskId(1))));
}
#[test]
fn transcript_preserves_split_unicode_json_and_bounds_memory() {
    let mut t = Transcript::default();
    let record=serde_json::json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"hello 界\nnext"}}).to_string()+"\n";
    let bytes = record.as_bytes();
    let split = bytes.iter().position(|b| *b == 0xe7).unwrap() + 1;
    t.append(&bytes[..split], split as u64, false);
    assert!(t.text().is_empty());
    t.append(&bytes[split..], bytes.len() as u64, true);
    assert_eq!(t.text(), "hello 界\nnext");
    assert_eq!(t.offset, bytes.len() as u64);
    for _ in 0..5000 {
        t.append(bytes, t.offset + bytes.len() as u64, false);
    }
    assert!(t.text().len() <= 256 * 1024);
}
#[test]
fn dense_task_list_and_editor_render_with_square_borders() {
    let mut v = fixture();
    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|f| draw_tasks(f, &v)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(screen.contains("Tasks"));
    assert!(screen.contains("review"));
    assert!(screen.contains("concurrency: 3"));
    assert!(screen.contains('┌'));
    assert!(!screen.contains('╭'));
    v.event(key(KeyCode::Char('e')));
    terminal.draw(|f| draw_tasks(f, &v)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    for text in [
        "Name", "Target", "Project", "Remote", "Branch", "Schedule", "When", "Timezone", "Model",
        "Thinking", "Timeout", "Prompt", "hello", "world",
    ] {
        assert!(screen.contains(text), "missing {text}");
    }
}

#[test]
fn history_status_live_transcript_cancel_and_confirmed_cleanup() {
    let mut store = TaskStore::default();
    let task = store.create(spec(), 0).unwrap();
    let mut run = store.enqueue(task.id, RunTrigger::Manual, 1).unwrap();
    run.status = RunStatus::Running;
    let mut v = fixture();
    v.event(key(KeyCode::Char('h')));
    assert_eq!(v.take_request(), Some(TaskRequest::ListRuns(Some(task.id))));
    v.receive(
        &TaskRequest::ListRuns(Some(task.id)),
        Ok(TaskResponse::Runs(vec![run.clone()])),
    );
    let request = v.take_request().unwrap();
    assert_eq!(
        request,
        TaskRequest::ReadLog {
            id: run.id,
            offset: 0,
            max_bytes: 65536
        }
    );
    let bytes=(serde_json::json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"Inspect 界\n"}}).to_string()+"\n").into_bytes();
    v.receive(
        &request,
        Ok(TaskResponse::Log {
            next_offset: bytes.len() as u64,
            bytes,
            terminal: false,
        }),
    );
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|f| draw_tasks(f, &v)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(screen.contains("Running"));
    assert!(screen.contains("Inspect 界"));
    v.event(key(KeyCode::PageUp));
    assert_eq!(v.transcript.scroll, 10);
    v.event(key(KeyCode::End));
    assert_eq!(v.transcript.scroll, 0);
    v.event(key(KeyCode::Char('x')));
    assert_eq!(v.take_request(), Some(TaskRequest::Cancel(run.id)));
    v.event(key(KeyCode::Char('d')));
    assert!(v.take_request().is_none());
    v.event(key(KeyCode::Char('n')));
    assert!(v.take_request().is_none());
    v.event(key(KeyCode::Char('d')));
    v.event(key(KeyCode::Char('y')));
    assert_eq!(
        v.take_request(),
        Some(TaskRequest::Clean {
            id: run.id,
            confirmed: true
        })
    );
}
#[test]
fn pause_resume_manual_run_and_concurrency() {
    let mut v = fixture();
    v.event(key(KeyCode::Char('p')));
    assert_eq!(v.take_request(), Some(TaskRequest::Pause(TaskId(1))));
    v.tasks[0].enabled = false;
    v.event(key(KeyCode::Char('p')));
    assert_eq!(v.take_request(), Some(TaskRequest::Resume(TaskId(1))));
    v.event(key(KeyCode::Char('r')));
    assert_eq!(v.take_request(), Some(TaskRequest::Enqueue(TaskId(1))));
    v.event(key(KeyCode::Char('c')));
    v.event(key(KeyCode::Char('5')));
    v.event(key(KeyCode::Enter));
    assert_eq!(v.take_request(), Some(TaskRequest::Concurrency(Some(5))));
}
#[test]
fn multiline_cursor_edits_unicode_without_corruption() {
    let mut v = fixture();
    v.event(key(KeyCode::Char('e')));
    for _ in 0..11 {
        v.event(key(KeyCode::Tab));
    }
    v.event(ctrl('a'));
    v.event(Event::Paste("one 界\nlast".into()));
    v.event(key(KeyCode::Home));
    v.event(key(KeyCode::Up));
    v.event(key(KeyCode::End));
    v.event(key(KeyCode::Backspace));
    v.event(key(KeyCode::Char('猫')));
    v.event(key(KeyCode::Enter));
    v.event(key(KeyCode::Char('!')));
    v.event(ctrl('s'));
    let TaskRequest::Update { spec, .. } = v.take_request().unwrap() else {
        panic!()
    };
    assert_eq!(spec.prompt, "one 猫\n!\nlast");
}

#[test]
fn dashboard_keeps_consuming_output_and_screen_dirty_while_tasks_open() {
    use ovrcr::protocol::{Request, ServerEvent, ServerMessage};
    use ovrcr::session::SessionId;
    let mut d = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
    d.select_request(SessionId(1), 1);
    d.ctrl('t');
    d.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        bytes: b"still draining".to_vec(),
    }));
    assert!(d.parser.screen().contents().contains("still draining"));
    let requests = d.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
        session: SessionId(1),
    }));
    assert_eq!(requests.len(), 1);
    assert!(matches!(requests[0].request, Request::Select { .. }));
    assert!(d.tasks.is_some());
}

#[test]
fn unicode_prompt_render_is_safe_while_another_field_has_focus() {
    let mut v = fixture();
    v.tasks[0].spec.name = "xx".into();
    v.tasks[0].spec.prompt = "界 hello".into();
    v.event(key(KeyCode::Char('e')));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| draw_tasks(f, &v)).unwrap();
}
#[test]
fn long_editor_fields_keep_the_cursor_text_visible() {
    let mut v = fixture();
    v.event(key(KeyCode::Char('e')));
    v.event(ctrl('a'));
    v.event(Event::Paste(format!("{}END", "a".repeat(100))));
    let mut terminal = Terminal::new(TestBackend::new(50, 24)).unwrap();
    terminal.draw(|f| draw_tasks(f, &v)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(screen.contains("END"));
}

#[test]
fn new_task_defaults_match_cli_off_thinking_and_one_hour_timeout() {
    let mut v = TasksView::default();
    v.event(key(KeyCode::Char('n')));
    v.event(Event::Paste("check".into()));
    for _ in 0..8 {
        v.event(key(KeyCode::Tab));
    }
    v.event(Event::Paste("provider/model".into()));
    for _ in 0..3 {
        v.event(key(KeyCode::Tab));
    }
    v.event(Event::Paste("Review the repo".into()));
    v.event(ctrl('s'));
    let TaskRequest::Create(spec) = v.take_request().unwrap() else {
        panic!()
    };
    assert_eq!(spec.thinking, "off");
    assert_eq!(spec.timeout_seconds, 3600);
}

#[test]
fn rejected_save_retains_every_field_and_prompt_for_correction() {
    let mut v = fixture();
    v.event(key(KeyCode::Char('e')));
    v.event(ctrl('a'));
    v.event(Event::Paste("corrected name".into()));
    v.event(ctrl('s'));
    let first = v.take_request().unwrap();
    v.receive(&first, Err("Project unavailable".into()));
    assert!(v.editor.is_some());
    assert_eq!(v.message, "Project unavailable");
    v.event(ctrl('s'));
    let retry = v.take_request().unwrap();
    assert_eq!(first, retry);
    v.receive(&retry, Ok(TaskResponse::Ok));
    assert!(v.editor.is_none());
}

#[test]
fn unicode_duration_keeps_editor_fields_available_for_correction() {
    for field in [6, 10] {
        let mut v = fixture();
        v.event(key(KeyCode::Char('e')));
        for _ in 0..field {
            v.event(key(KeyCode::Tab));
        }
        v.event(ctrl('a'));
        v.event(Event::Paste("1秒".into()));
        v.event(ctrl('s'));
        assert!(v.take_request().is_none());
        assert!(v.editor.is_some());
        assert!(v.message.contains("duration"));
        v.event(ctrl('a'));
        v.event(Event::Paste("60s".into()));
        v.event(ctrl('s'));
        let Some(TaskRequest::Update {
            spec: corrected, ..
        }) = v.take_request()
        else {
            panic!("corrected form should save")
        };
        assert_eq!(corrected.prompt, "hello\nworld");
        assert_eq!(corrected.name, "review");
    }
}
