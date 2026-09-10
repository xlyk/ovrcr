use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ovrcr::protocol::{
    HierarchySnapshot, ProjectSummary, Response, ServerEvent, ServerMessage, WorkspaceSummary,
};
use ovrcr::session::{AgentActivity, SessionId, SessionPhase, SessionSummary, TerminalSize};
use ovrcr::task_manager::{TaskRequest, TaskResponse};
use ovrcr::task_tui::{TasksView, Transcript, draw_tasks};
use ovrcr::tasks::*;
use ovrcr::tui::{Dashboard, DashboardAction, InputMode};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use std::path::PathBuf;
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

fn ready_dashboard() -> Dashboard {
    let session = SessionSummary {
        id: SessionId(1),
        project: "project".into(),
        workspace: "workspace".into(),
        name: "local".into(),
        label: "shell".into(),
        pid: Some(1),
        started_unix_ms: 0,
        phase: SessionPhase::Running,
        activity: AgentActivity::Unknown,
        context_usage: None,
    };
    let mut dashboard = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
    dashboard.outer_area = Rect::new(0, 0, 80, 24);
    dashboard.hierarchy = HierarchySnapshot {
        projects: vec![ProjectSummary {
            name: "project".into(),
            workspaces: vec![WorkspaceSummary {
                project: "project".into(),
                name: "workspace".into(),
                path: PathBuf::from("/tmp/workspace"),
                sessions: vec![session],
            }],
        }],
    };
    dashboard.select_request(SessionId(1), 1);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::Screen {
            session: SessionId(1),
            revision: dashboard.view_revision,
            size: dashboard.panes[dashboard.focused_pane].desired_size,
            bytes: b"ready".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::Ok,
    });
    dashboard
}

#[test]
fn browse_opens_tasks_and_terminal_ctrl_t_is_literal() {
    let mut d = ready_dashboard();
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
    let mut v = TasksView::with_projects(
        &HierarchySnapshot {
            projects: vec![ProjectSummary {
                name: "ovrcr".into(),
                workspaces: vec![],
            }],
        },
        "ovrcr",
    );
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
fn invalid_concurrency_shows_error_with_edit_controls_and_recovers() {
    for width in [40, 80] {
        for invalid in ["", "0"] {
            let mut view = TasksView::default();
            view.event(key(KeyCode::Char('c')));
            for ch in invalid.chars() {
                view.event(key(KeyCode::Char(ch)));
            }
            view.event(key(KeyCode::Enter));
            assert!(view.take_request().is_none());
            assert_eq!(view.concurrency, 3);
            let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
            terminal.draw(|f| draw_tasks(f, &view)).unwrap();
            let rows = (0..12)
                .map(|y| {
                    (0..width)
                        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>();
            assert!(
                rows.iter().any(|row| row.contains("positive integer")),
                "{rows:?}"
            );
            assert!(
                rows.iter().any(|row| row.contains("Concurrency:")),
                "{rows:?}"
            );
            assert!(rows[11].contains("Esc cancel"), "{rows:?}");
            assert!(rows[11].contains("Enter save"), "{rows:?}");

            view.event(key(KeyCode::Backspace));
            view.event(key(KeyCode::Char('5')));
            view.event(key(KeyCode::Enter));
            assert_eq!(view.take_request(), Some(TaskRequest::Concurrency(Some(5))));
            assert_eq!(view.concurrency, 3, "wait for the server acknowledgement");
            view.receive(
                &TaskRequest::Concurrency(Some(5)),
                Ok(TaskResponse::Concurrency(5)),
            );
            assert_eq!(view.concurrency, 5);

            view.event(key(KeyCode::Char('c')));
            assert!(!view.event(key(KeyCode::Esc)), "cancel stays in tasks");
            assert!(view.take_request().is_none());
            assert_eq!(view.concurrency, 5);
        }
    }
}

#[test]
fn empty_task_views_only_hint_actions_with_available_targets() {
    let footer = |view: &TasksView, width| {
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal.draw(|f| draw_tasks(f, view)).unwrap();
        (0..width)
            .map(|x| terminal.backend().buffer()[(x, 11)].symbol())
            .collect::<String>()
    };
    for width in [40, 120] {
        let mut view = TasksView::default();
        let text = footer(&view, width);
        for available in ["Esc back", "n new", "h all history", "c limit"] {
            assert!(text.contains(available), "{text}");
        }
        for unavailable in ["e edit", "p pause", "r run", "d del"] {
            assert!(!text.contains(unavailable), "{text}");
        }
        for command in ['e', 'p', 'r', 'd'] {
            view.event(key(KeyCode::Char(command)));
            assert!(view.take_request().is_none());
        }
        view.event(key(KeyCode::Char('h')));
        assert_eq!(view.take_request(), Some(TaskRequest::ListRuns(None)));
        view.receive(&TaskRequest::ListRuns(None), Ok(TaskResponse::Runs(vec![])));
        let text = footer(&view, width);
        assert_eq!(text.trim(), "Esc tasks");
        for command in ['x', 'd'] {
            view.event(key(KeyCode::Char(command)));
            assert!(view.take_request().is_none());
        }
        assert!(!view.event(key(KeyCode::Esc)));
        assert!(footer(&view, width).contains("n new"));
    }

    let mut view = fixture();
    for available in ["e edit", "p pause/resume", "r run", "d del"] {
        assert!(footer(&view, 120).contains(available));
    }
    let mut store = TaskStore::default();
    let task = store.create(spec(), 0).unwrap();
    let run = store.enqueue(task.id, RunTrigger::Manual, 1).unwrap();
    view.event(key(KeyCode::Char('h')));
    let request = view.take_request().unwrap();
    view.receive(&request, Ok(TaskResponse::Runs(vec![run])));
    for available in ["↑/↓ run", "x cancel run", "d cleanup"] {
        assert!(footer(&view, 120).contains(available));
    }
}

#[test]
fn task_confirmations_honor_default_no_and_case_insensitive_answers() {
    for cleanup in [false, true] {
        for answer in [
            KeyCode::Enter,
            KeyCode::Char('n'),
            KeyCode::Char('N'),
            KeyCode::Esc,
            KeyCode::Char('y'),
            KeyCode::Char('Y'),
        ] {
            let mut view = fixture();
            let expected = if cleanup {
                let mut store = TaskStore::default();
                let task = store.create(spec(), 0).unwrap();
                let run = store.enqueue(task.id, RunTrigger::Manual, 1).unwrap();
                let id = run.id;
                view.event(key(KeyCode::Char('h')));
                let request = view.take_request().unwrap();
                view.receive(&request, Ok(TaskResponse::Runs(vec![run])));
                assert!(matches!(
                    view.take_request(),
                    Some(TaskRequest::ReadLog { .. })
                ));
                TaskRequest::Clean {
                    id,
                    confirmed: true,
                }
            } else {
                TaskRequest::Delete(TaskId(1))
            };
            let mut terminal = Terminal::new(TestBackend::new(120, 12)).unwrap();
            let mut confirmation_visible = |view: &TasksView| {
                terminal.draw(|f| draw_tasks(f, view)).unwrap();
                terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect::<String>()
                    .contains("[y/N]")
            };
            view.event(key(KeyCode::Char('d')));
            assert!(
                confirmation_visible(&view),
                "confirmation must be visible first"
            );
            assert!(view.take_request().is_none());
            assert!(!view.event(key(answer)), "confirmation stays in task UI");
            assert!(
                !confirmation_visible(&view),
                "answer {answer:?}, cleanup={cleanup}"
            );
            if matches!(answer, KeyCode::Char('y' | 'Y')) {
                assert_eq!(view.take_request(), Some(expected));
            } else {
                assert!(view.take_request().is_none(), "No must not mutate");
            }
            assert_eq!(
                view.tasks.len(),
                1,
                "no mutation before server acknowledgement"
            );
        }
    }
}
#[test]
fn confirmations_show_only_answer_controls_at_narrow_widths() {
    for width in [24, 40, 120] {
        let mut view = fixture();
        view.event(key(KeyCode::Char('d')));
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal.draw(|f| draw_tasks(f, &view)).unwrap();
        let footer = (0..width)
            .map(|x| terminal.backend().buffer()[(x, 11)].symbol())
            .collect::<String>();
        assert!(
            footer.contains("n no") && footer.contains("y yes"),
            "{footer}"
        );
        assert!(
            !footer.contains("new") && !footer.contains("run"),
            "{footer}"
        );
        if width >= 40 {
            assert!(footer.contains("Enter/Esc no"), "{footer}");
        }
        view.event(key(KeyCode::Char('r')));
        assert!(view.take_request().is_none());
        view.event(key(KeyCode::Esc));
        assert!(view.take_request().is_none());
        terminal.draw(|f| draw_tasks(f, &view)).unwrap();
        let footer = (0..width)
            .map(|x| terminal.backend().buffer()[(x, 11)].symbol())
            .collect::<String>();
        assert!(footer.contains("Esc back"), "{footer}");
        assert!(!footer.contains("y yes"), "{footer}");
    }
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
    use ovrcr::protocol::Request;
    let mut d = ready_dashboard();
    d.ctrl('t');
    d.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        revision: d.view_revision,
        bytes: b"still draining".to_vec(),
    }));
    assert!(
        d.panes[d.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("still draining")
    );
    let revision = d.view_revision;
    let requests = d.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
        session: SessionId(1),
        revision,
    }));
    assert_eq!(requests.len(), 1);
    assert!(matches!(requests[0].request, Request::SetView { .. }));
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
#[test]
fn transcript_wrap_is_cached_between_frames() {
    let mut v = fixture();
    v.event(key(KeyCode::Char('h')));
    v.transcript.append(
        "a plain transcript line that is long enough to wrap 界 twice in a narrow pane\n"
            .as_bytes(),
        1,
        false,
    );
    let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
    terminal.draw(|f| draw_tasks(f, &v)).unwrap();
    terminal.draw(|f| draw_tasks(f, &v)).unwrap();
    assert_eq!(v.transcript_wraps(), 1);
    v.transcript.append(b"more\n", 2, false);
    terminal.draw(|f| draw_tasks(f, &v)).unwrap();
    assert_eq!(v.transcript_wraps(), 2);
}

#[test]
fn ux_empty_history_preserves_status_without_stale_working() {
    let mut view = TasksView::default();
    view.event(key(KeyCode::Char('h')));
    let request = view.take_request().unwrap();
    assert!(matches!(request, TaskRequest::ListRuns(None)));
    view.receive(&request, Ok(TaskResponse::Runs(vec![])));
    assert_ne!(view.message, "Working…");
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| draw_tasks(f, &view)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("No runs to display"), "{text}");
    for status in ["creation refused", "Working…", "Saved"] {
        view.event(key(KeyCode::Esc));
        view.message = status.into();
        view.event(key(KeyCode::Char('h')));
        let request = view.take_request().unwrap();
        view.receive(&request, Ok(TaskResponse::Runs(vec![])));
        assert_eq!(view.message, status);
    }
}

#[test]
fn ux_narrow_task_footer_keeps_escape_and_save_complete() {
    for (width, field) in [(40, 0), (60, 0), (80, 0), (40, 2), (60, 2), (80, 2)] {
        let mut view = TasksView::default();
        view.event(key(KeyCode::Char('n')));
        for _ in 0..field {
            view.event(key(KeyCode::Tab));
        }
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal.draw(|f| draw_tasks(f, &view)).unwrap();
        let footer = (0..width)
            .map(|x| terminal.backend().buffer()[(x, 11)].symbol())
            .collect::<String>();
        assert!(footer.contains("Esc cancel"), "{footer}");
        assert!(footer.contains("Ctrl-s save"), "{footer}");
        let allowed = [
            "Esc cancel",
            "Ctrl-s save",
            "Tab/Shift-Tab field",
            "Ctrl-a clear",
            "Enter newline",
            "↑/↓ pick",
            "Tab/Enter accept",
            "Shift-Tab back",
            "Ctrl-u clear",
        ];
        assert!(
            footer
                .trim_end()
                .split("  ")
                .all(|hint| allowed.contains(&hint)),
            "{footer}"
        );
        if field == 2 {
            assert!(footer.contains("↑/↓ pick"), "{footer}");
            assert!(!footer.contains("Enter newline"), "{footer}");
        }
        view.event(key(KeyCode::Esc));
        assert!(view.editor.is_none());
    }
}

#[test]
fn task_project_picker_uses_dashboard_projects_and_filters_before_save() {
    let mut d = ready_dashboard();
    d.hierarchy.projects.push(ProjectSummary {
        name: "ovrcr".into(),
        workspaces: vec![],
    });
    d.ctrl('t');
    d.event_action(key(KeyCode::Char('n')));
    for value in ["review", "git"] {
        d.event_action(ctrl('a'));
        d.event_action(Event::Paste(value.into()));
        d.event_action(key(KeyCode::Tab));
    }
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|f| draw_tasks(f, d.tasks.as_ref().unwrap()))
        .unwrap();
    let rendered = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(
        rendered.contains("ovrcr"),
        "project options must be shown: {rendered}"
    );
    d.event_action(Event::Paste("ovr".into()));
    d.event_action(key(KeyCode::Enter));
    for value in [
        "origin",
        "main",
        "interval",
        "1h",
        "UTC",
        "provider/model",
        "off",
        "1h",
        "prompt",
    ] {
        d.event_action(ctrl('a'));
        d.event_action(Event::Paste(value.into()));
        d.event_action(key(KeyCode::Tab));
    }
    d.event_action(ctrl('s'));
    let Some(TaskRequest::Create(spec)) = d.tasks.as_mut().unwrap().take_request() else {
        panic!("save task");
    };
    assert_eq!(
        spec.target,
        TaskTarget::Git {
            project: "ovrcr".into(),
            remote: "origin".into(),
            branch: "main".into()
        }
    );
}

#[test]
fn task_project_picker_preserves_edit_target_and_rejects_unmatched_save() {
    let mut d = ready_dashboard();
    d.hierarchy.projects.push(ProjectSummary {
        name: "other".into(),
        workspaces: vec![],
    });
    d.ctrl('t');
    let v = d.tasks.as_mut().unwrap();
    let mut task_spec = spec();
    task_spec.target = TaskTarget::Git {
        project: "other".into(),
        remote: "origin".into(),
        branch: "main".into(),
    };
    v.receive(
        &TaskRequest::ListTasks,
        Ok(TaskResponse::Tasks(vec![Task {
            id: TaskId(8),
            spec: task_spec.clone(),
            enabled: true,
            next_due_at: None,
        }])),
    );
    v.event(key(KeyCode::Char('e')));
    v.event(ctrl('s'));
    let expected = TaskRequest::Update {
        id: TaskId(8),
        spec: task_spec,
    };
    assert_eq!(v.take_request(), Some(expected.clone()));
    v.receive(&expected, Err("retry".into()));
    v.event(key(KeyCode::Tab));
    v.event(key(KeyCode::Tab));
    v.event(key(KeyCode::Tab));
    assert_eq!(v.message, "retry", "acceptance must retain server errors");
    v.event(key(KeyCode::BackTab));
    v.event(Event::Paste("missing".into()));
    v.event(ctrl('s'));
    assert!(v.take_request().is_none());
    assert!(v.message.contains("Select an available project"));
    v.event(ctrl('u'));
    v.event(key(KeyCode::Down));
    for (width, height) in [(100, 30), (40, 12), (12, 5), (1, 1)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw_tasks(f, v)).unwrap();
    }
    v.event(key(KeyCode::Enter));
    assert!(
        v.message.is_empty(),
        "valid acceptance clears the picker error"
    );
    assert!(v.take_request().is_none());
    v.event(ctrl('s'));
    assert_eq!(v.take_request(), Some(expected));
}

fn task_mouse(
    d: &mut Dashboard,
    kind: crossterm::event::MouseEventKind,
    x: u16,
    y: u16,
    area: Rect,
) -> DashboardAction {
    d.mouse_action(
        crossterm::event::MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        area,
    )
}
fn task_click_text(d: &mut Dashboard, text: &str, width: u16, height: u16) -> DashboardAction {
    task_click_text_at(d, text, Rect::new(0, 0, width, height))
}
fn task_click_text_at(d: &mut Dashboard, text: &str, area: Rect) -> DashboardAction {
    let mut terminal = Terminal::with_options(
        TestBackend::new(area.right(), area.bottom()),
        ratatui::TerminalOptions {
            viewport: ratatui::Viewport::Fixed(area),
        },
    )
    .unwrap();
    terminal
        .draw(|f| draw_tasks(f, d.tasks.as_ref().unwrap()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let row = (x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            if row.starts_with(text) {
                return task_mouse(
                    d,
                    crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                    x,
                    y,
                    area,
                );
            }
        }
    }
    panic!("missing visible task control {text}");
}

#[test]
fn task_mouse_selects_scrolled_rows_and_dispatches_controls_with_confirmation() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut d = ready_dashboard();
    let mut v = fixture();
    v.tasks = (1..=30)
        .map(|id| Task {
            id: TaskId(id),
            spec: spec(),
            enabled: true,
            next_due_at: None,
        })
        .collect();
    d.tasks = Some(v);
    let area = Rect::new(0, 0, 120, 12);
    task_mouse(&mut d, MouseEventKind::Down(MouseButton::Left), 20, 4, area);
    task_click_text(&mut d, "r run", 120, 12);
    assert_eq!(
        d.tasks.as_mut().unwrap().take_request(),
        Some(TaskRequest::Enqueue(TaskId(2)))
    );
    for _ in 0..12 {
        task_mouse(&mut d, MouseEventKind::ScrollDown, 20, 5, area);
    }
    // The selected row is kept at the bottom; click the first visible row.
    task_mouse(&mut d, MouseEventKind::Down(MouseButton::Left), 20, 3, area);
    task_click_text(&mut d, "d del", 120, 12);
    assert!(d.tasks.as_mut().unwrap().take_request().is_none());
    task_click_text(&mut d, "n no", 120, 12);
    assert!(d.tasks.as_mut().unwrap().take_request().is_none());
    task_click_text(&mut d, "d del", 120, 12);
    task_click_text(&mut d, "y yes", 120, 12);
    assert_eq!(
        d.tasks.as_mut().unwrap().take_request(),
        Some(TaskRequest::Delete(TaskId(9)))
    );
    assert!(d.mouse_capture_required());
}
#[test]
fn task_mouse_edits_unicode_fields_and_multiline_prompt_and_submits_once() {
    let mut d = ready_dashboard();
    d.tasks = Some(fixture());
    task_click_text(&mut d, "e edit", 120, 24);
    task_click_text(&mut d, "Model: ", 120, 24);
    d.event_action(ctrl('a'));
    d.event_action(Event::Paste("provider/a界z".into()));
    task_click_text(&mut d, "界", 120, 24);
    d.event_action(key(KeyCode::Char('X')));
    task_click_text(&mut d, "world", 120, 24);
    d.event_action(key(KeyCode::Char('!')));
    task_click_text(&mut d, "Ctrl-s save", 120, 24);
    let Some(TaskRequest::Update { spec, .. }) = d.tasks.as_mut().unwrap().take_request() else {
        panic!("update request")
    };
    assert_eq!(spec.model, "provider/aX界z");
    assert_eq!(spec.prompt, "hello\n!world");
    task_click_text(&mut d, "Ctrl-s save", 120, 24);
    assert!(d.tasks.as_mut().unwrap().take_request().is_none());
    task_click_text(&mut d, "Esc cancel", 120, 24);
    assert!(d.tasks.as_ref().unwrap().editor.is_none());
}
#[test]
fn task_mouse_project_picker_and_transcript_wheel_use_visible_rows() {
    use crossterm::event::MouseEventKind;
    let mut d = ready_dashboard();
    let hierarchy = HierarchySnapshot {
        projects: (0..14)
            .map(|i| ProjectSummary {
                name: format!("project-{i:02}"),
                workspaces: vec![],
            })
            .collect(),
    };
    let mut v = TasksView::with_projects(&hierarchy, "project-00");
    v.tasks = fixture().tasks;
    d.tasks = Some(v);
    task_click_text(&mut d, "e edit", 120, 24);
    task_click_text(&mut d, "Target", 120, 24);
    d.event_action(ctrl('a'));
    d.event_action(Event::Paste("git".into()));
    task_click_text(&mut d, "Project", 120, 24);
    for _ in 0..10 {
        task_mouse(
            &mut d,
            MouseEventKind::ScrollDown,
            10,
            6,
            Rect::new(0, 0, 120, 24),
        );
    }
    task_click_text(&mut d, "project-05", 120, 24);
    task_click_text(&mut d, "Ctrl-s save", 120, 24);
    let Some(TaskRequest::Update { spec, .. }) = d.tasks.as_mut().unwrap().take_request() else {
        panic!("update")
    };
    assert!(matches!(spec.target,TaskTarget::Git{project,..} if project=="project-05"));
    task_click_text(&mut d, "Esc cancel", 120, 24);
    task_click_text(&mut d, "h/H history/all", 120, 24);
    let v = d.tasks.as_mut().unwrap();
    v.transcript.append(
        b"one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n",
        100,
        false,
    );
    task_mouse(
        &mut d,
        MouseEventKind::ScrollUp,
        10,
        17,
        Rect::new(0, 0, 120, 24),
    );
    assert_eq!(d.tasks.as_ref().unwrap().transcript.scroll, 1);
    task_mouse(
        &mut d,
        MouseEventKind::ScrollDown,
        10,
        17,
        Rect::new(0, 0, 120, 24),
    );
    assert_eq!(d.tasks.as_ref().unwrap().transcript.scroll, 0);
}

#[test]
fn task_mouse_narrow_offset_editor_maps_scrolled_prompt_and_ignores_hidden_rows() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut d = ready_dashboard();
    let mut v = fixture();
    v.tasks[0].spec.prompt = "zero\none\ntwo\nthree\nfour\nlong 界 tail".into();
    d.tasks = Some(v);
    let area = Rect::new(7, 5, 42, 14);
    task_click_text_at(&mut d, "e edit", area);
    // Navigate to Prompt so the small viewport is scrolled there.
    for _ in 0..11 {
        d.event_action(key(KeyCode::Tab));
    }
    task_click_text_at(&mut d, "界", area);
    d.event_action(key(KeyCode::Char('X')));
    // A click outside the offset frame must not move the editor cursor.
    task_mouse(&mut d, MouseEventKind::Down(MouseButton::Left), 1, 1, area);
    d.event_action(key(KeyCode::Char('Y')));
    task_click_text_at(&mut d, "Ctrl-s save", area);
    let Some(TaskRequest::Update { spec, .. }) = d.tasks.as_mut().unwrap().take_request() else {
        panic!("save")
    };
    assert_eq!(spec.prompt, "zero\none\ntwo\nthree\nfour\nlong XY界 tail");
    task_click_text_at(&mut d, "Esc cancel", area);
    task_click_text_at(&mut d, "c limit", area);
    d.event_action(key(KeyCode::Char('2')));
    task_click_text_at(&mut d, "Enter save", area);
    assert_eq!(
        d.tasks.as_mut().unwrap().take_request(),
        Some(TaskRequest::Concurrency(Some(2)))
    );
    task_click_text_at(&mut d, "p pause/resume", area);
    assert_eq!(
        d.tasks.as_mut().unwrap().take_request(),
        Some(TaskRequest::Pause(TaskId(1)))
    );
}
#[test]
fn task_mouse_run_rows_cancel_and_cleanup_use_selected_run() {
    let mut d = ready_dashboard();
    d.tasks = Some(fixture());
    task_click_text(&mut d, "h/H history/all", 120, 24);
    assert_eq!(
        d.tasks.as_mut().unwrap().take_request(),
        Some(TaskRequest::ListRuns(Some(TaskId(1))))
    );
    let mut store = TaskStore::default();
    let task = store.create(spec(), 0).unwrap();
    let template = store.enqueue(task.id, RunTrigger::Manual, 1).unwrap();
    let runs = (1..=3)
        .map(|id| {
            let mut run = template.clone();
            run.id = RunId(id);
            run.status = RunStatus::Running;
            run
        })
        .collect();
    d.tasks.as_mut().unwrap().receive(
        &TaskRequest::ListRuns(Some(TaskId(1))),
        Ok(TaskResponse::Runs(runs)),
    );
    d.tasks.as_mut().unwrap().take_request();
    task_mouse(
        &mut d,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        10,
        3,
        Rect::new(0, 0, 120, 24),
    );
    assert!(matches!(
        d.tasks.as_mut().unwrap().take_request(),
        Some(TaskRequest::ReadLog {
            id: RunId(2),
            offset: 0,
            ..
        })
    ));
    task_click_text(&mut d, "x cancel run", 120, 24);
    assert_eq!(
        d.tasks.as_mut().unwrap().take_request(),
        Some(TaskRequest::Cancel(RunId(2)))
    );
    task_click_text(&mut d, "d cleanup", 120, 24);
    assert!(d.tasks.as_mut().unwrap().take_request().is_none());
    task_click_text(&mut d, "y yes", 120, 24);
    assert_eq!(
        d.tasks.as_mut().unwrap().take_request(),
        Some(TaskRequest::Clean {
            id: RunId(2),
            confirmed: true
        })
    );
}
