//! The Server's settings reading: what the Dashboard adopts, the footer line,
//! and the Settings editor. Readings come from the real loader and edits go
//! through the real writer, exactly as the Server answers `SetSetting`.

use crate::*;
use ovrcr::protocol::{ErrorCode, Request, Response, SettingsReport};
use std::path::Path;

#[cfg(target_os = "macos")]
use crate::live;

/// The Server's reading of `document`, as published after hello or an edit.
fn publish(dashboard: &mut Dashboard, document: &Path) {
    let text = std::fs::read_to_string(document).unwrap_or_default();
    publish_report(dashboard, ovrcr::settings::parse(document, &text));
}

fn publish_report(dashboard: &mut Dashboard, report: SettingsReport) {
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
        Box::new(report),
    )));
}

/// One `SetSetting` request, answered as the Server answers it: the writer
/// applies it, the new reading is published, then `Ok`; or the writer's
/// refusal comes back as the error, and nothing is published.
fn serve(dashboard: &mut Dashboard, document: &Path, key: KeyCode) -> (String, Option<String>) {
    let action = dashboard.key(key);
    answer(dashboard, document, action)
}

fn answer(
    dashboard: &mut Dashboard,
    document: &Path,
    action: DashboardAction,
) -> (String, Option<String>) {
    let DashboardAction::Request(message) = action else {
        panic!("expected a SetSetting request, got {action:?}");
    };
    let Request::SetSetting { path, value } = message.request else {
        panic!("expected SetSetting, got {:?}", message.request);
    };
    let response = match ovrcr::settings::set(document, &path, value.as_deref()) {
        Ok(()) => {
            publish(dashboard, document);
            Response::Ok
        }
        Err(error) => Response::Error {
            code: ErrorCode::InvalidRequest,
            message: format!("{error:#}"),
        },
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response,
    });
    (path, value)
}

fn screen(dashboard: &Dashboard) -> String {
    rendered_rows(dashboard, 120, 60).join("\n")
}

/// The rendered line holding `text`.
fn line_with(dashboard: &Dashboard, text: &str) -> String {
    rendered_rows(dashboard, 120, 60)
        .into_iter()
        .find(|line| line.contains(text))
        .unwrap_or_else(|| panic!("no line with {text:?}:\n{}", screen(dashboard)))
}

/// A Dashboard attached to a Server reading `contents`, with Settings open
/// from the palette.
fn editor(contents: &str) -> (Dashboard, tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    std::fs::write(&document, contents).unwrap();
    let mut dashboard = dashboard_fixture();
    publish(&mut dashboard, &document);
    dashboard.key(KeyCode::Char(':'));
    dashboard.event_action(Event::Paste("settings".into()));
    dashboard.key(KeyCode::Enter);
    (dashboard, root, document)
}

/// Select the first row the word filter `words` leaves, then leave the filter.
fn select(dashboard: &mut Dashboard, words: &str) {
    dashboard.key(KeyCode::Char('/'));
    dashboard.event_action(Event::Paste(words.into()));
    dashboard.key(KeyCode::Enter);
}

fn open(dashboard: &Dashboard) -> bool {
    screen(dashboard).contains("Settings · Enter edit")
}

#[test]
fn settings_footer_reports_findings_at_attach_and_when_the_count_changes() {
    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    let findings = |count: usize| {
        (0..count)
            .map(|index| format!("unknown_{index} = 1\n"))
            .collect::<String>()
    };
    let mut quiet = dashboard_fixture();
    publish(&mut quiet, &document);
    assert!(!rendered_footer(&quiet, 120).contains("settings finding"));

    let mut dashboard = dashboard_fixture();
    std::fs::write(&document, findings(2)).unwrap();
    publish(&mut dashboard, &document);
    assert!(
        rendered_footer(&dashboard, 120).contains("2 settings findings; see Settings"),
        "{}",
        rendered_footer(&dashboard, 120)
    );
    // Any key clears the notice; the same count does not bring it back.
    dashboard.key(KeyCode::Char('j'));
    publish(&mut dashboard, &document);
    assert!(!rendered_footer(&dashboard, 120).contains("settings finding"));
    std::fs::write(&document, findings(1)).unwrap();
    publish(&mut dashboard, &document);
    assert!(rendered_footer(&dashboard, 120).contains("1 settings finding; see Settings"));
    std::fs::write(&document, findings(0)).unwrap();
    publish(&mut dashboard, &document);
    assert!(rendered_footer(&dashboard, 120).contains("No settings findings"));
}

/// A startup warning is applied after hello's settings reading and takes the
/// footer. The findings line must wait behind that banner, including across a
/// key press, and show once the banner is dismissed.
#[test]
fn startup_error_banner_does_not_hide_the_settings_findings_notice() {
    let root = tempfile::tempdir().unwrap();
    let document = root.path().join("dashboard.toml");
    std::fs::write(&document, "unknown_0 = 1\nunknown_1 = 1\n").unwrap();
    let mut dashboard = dashboard_fixture();
    publish(&mut dashboard, &document);
    assert!(
        rendered_footer(&dashboard, 120).contains("2 settings findings; see Settings"),
        "{}",
        rendered_footer(&dashboard, 120)
    );

    // Same banner `run_dashboard` writes for a startup warning: `set_error`.
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 9_000,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "startup warning".into(),
        },
    });
    let banner = rendered_footer(&dashboard, 120);
    assert!(
        banner.starts_with("ERROR:"),
        "startup warning must own the footer, got {banner:?}"
    );
    assert!(
        !banner.contains("settings finding"),
        "the banner covers the findings line, got {banner:?}"
    );

    // A key while the banner is up used to drop the notice the user never saw.
    assert_eq!(dashboard.key(KeyCode::F(12)), DashboardAction::None);
    assert!(
        rendered_footer(&dashboard, 120).starts_with("ERROR:"),
        "the banner stays until it is dismissed"
    );

    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 9_001,
        response: Response::TerminalText {
            session: SessionId(1),
            size: TerminalSize { rows: 1, cols: 1 },
            text: String::new(),
        },
    });
    let footer = rendered_footer(&dashboard, 120);
    assert!(
        footer.contains("2 settings findings; see Settings"),
        "queued findings notice must show once the banner is dismissed, got {footer:?}"
    );
}

#[test]
fn settings_opens_from_the_menu_and_has_no_browse_key() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(','));
    assert!(!open(&dashboard));
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('v'));
    assert!(screen(&dashboard).contains(",  Settings"));
    dashboard.key(KeyCode::Char(','));
    assert!(open(&dashboard), "{}", screen(&dashboard));
    assert!(screen(&dashboard).contains("Waiting for the Server's settings reading."));
}

/// Groups in documented order, findings first, each row with its value,
/// source badge and the default beside an overridden value.
#[test]
fn rows_show_value_source_default_and_findings_with_unknown_keys_first() {
    let (mut dashboard, _root, document) = editor(
        "branch_prefix = \"kh/\"\ncolour = \"blue\"\nready_sound = \"loud\"\n\n[[agents]]\nname = \"claude\"\nargv = [\"claude\", \"--verbose\"]\n\n[launch_choices.demo]\nkind = \"Terminal\"\n",
    );
    // This overview checks every group and collection's Add row. Include the
    // Bridge, WIP, Cursor and Appearance rows and explanations without clipping
    // final groups; macOS adds platform explanations beyond Linux's.
    // At most 86 inner lines fit in 90 rows after the editor's borders/margins.
    let text = rendered_rows(&dashboard, 120, 90).join("\n");
    let line = |needle: &str| {
        text.lines()
            .find(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("no line with {needle:?}:\n{text}"))
    };
    assert!(
        text.contains(&format!("Document: {}", document.display())),
        "{text}"
    );
    let at = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("no {needle:?}:\n{text}"))
    };
    assert!(
        at("line 2, colour: unknown setting; ignored") < at("Alerts"),
        "{text}"
    );
    for pair in [
        "Alerts",
        "Workspaces",
        "Titles",
        "Usage",
        "Agents",
        "Remembered launches",
    ]
    .windows(2)
    {
        assert!(at(pair[0]) < at(pair[1]), "{pair:?}\n{text}");
    }
    assert!(
        line("Branch prefix").contains("kh/  set  (default: feature/)"),
        "{text}"
    );
    assert!(line("Ready sound").contains("off  default"));
    // A bad value defaults only itself and its finding shows on its row.
    assert!(at("! line 3: ") > at("Ready sound"), "{text}");
    assert!(line("Title model").contains("unset  default"));
    assert!(text.contains("Automatic titles off: set `title_model"));
    // Consent settings carry their side effect; turning one on asks nothing.
    assert!(text.contains("Setting a model makes paid calls to title sessions."));
    assert!(text.contains("may show an OS notification permission prompt."));
    assert!(text.contains("On by default while a Dashboard is attached."));
    assert!(line("Cursor dashboard usage").contains("off  default"));
    assert!(line("Cursor state database").contains("unset  default"));
    // Collections expand into child rows with an Add row.
    assert!(line("claude  [").contains("[\"claude\", \"--verbose\"]"));
    assert!(text.contains("+ Add agent"));
    assert!(line("demo  ").contains("Terminal"));
    assert!(text.contains("+ Add project"), "{text}");
    assert!(text.contains("+ Add root"), "{text}");
    // Added settings can put remembered launches below this viewport. Reach
    // their child rows through the editor's normal scrolling control.
    assert_eq!(dashboard.key(KeyCode::End), DashboardAction::Redraw);
    let text = screen(&dashboard);
    assert!(line_with(&dashboard, "demo  ").contains("Terminal"));
    assert!(text.contains("+ Add project"));
}

#[test]
fn toggle_saves_at_once_and_applies_only_the_servers_reading() {
    let (mut dashboard, _root, document) = editor("ready_sound = true # mine\n");
    select(&mut dashboard, "desktop");
    let action = dashboard.key(KeyCode::Enter);
    let DashboardAction::Request(message) = &action else {
        panic!("Enter on a toggle saves at once: {action:?}");
    };
    assert_eq!(
        message.request,
        Request::SetSetting {
            path: "desktop_notifications".into(),
            value: Some("true".into()),
        }
    );
    // Nothing changes until the Server's reading arrives; no confirmation.
    assert!(line_with(&dashboard, "Desktop notifications").contains("off  default"));
    assert!(line_with(&dashboard, "Desktop notifications").contains("saving…"));
    answer(&mut dashboard, &document, action);
    assert!(
        line_with(&dashboard, "Desktop notifications").contains("on  set  (default: off)"),
        "{}",
        screen(&dashboard)
    );
    assert!(!line_with(&dashboard, "Desktop notifications").contains("saving…"));
    assert_eq!(
        std::fs::read_to_string(&document).unwrap(),
        "ready_sound = true # mine\ndesktop_notifications = true\n"
    );
}

/// Collect actual Dashboard-role frames, retaining the request's reply and,
/// when expected, the Server's settings reading. Delivery to the Dashboard
/// stays under the test's control so two outstanding edits can interleave.
#[cfg(target_os = "macos")]
fn settings_frames(
    stream: &mut std::os::unix::net::UnixStream,
    message: &ClientMessage,
    expect_reading: bool,
) -> anyhow::Result<Vec<ServerMessage>> {
    use ovrcr::protocol::{read_frame, write_frame};
    use std::time::Instant;

    let deadline = Instant::now() + live::wait_deadline();
    write_frame(stream, message)?;
    let mut frames = Vec::new();
    let mut answered = false;
    let mut read = !expect_reading;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        anyhow::ensure!(!remaining.is_zero(), "settings request did not complete");
        stream.set_read_timeout(Some(remaining))?;
        let frame = read_frame::<ServerMessage>(stream)?;
        answered |= matches!(
            &frame,
            ServerMessage::Response { request_id, .. } if *request_id == message.request_id
        );
        read |= matches!(
            &frame,
            ServerMessage::Event(ServerEvent::SettingsChanged(_))
        );
        frames.push(frame);
        if answered && read {
            return Ok(frames);
        }
    }
}

#[cfg(target_os = "macos")]
fn picker_root_line(dashboard: &Dashboard, value: &str) -> String {
    let mut lines: Vec<_> = rendered_rows(dashboard, 120, 60)
        .into_iter()
        .filter(|line| {
            line.split('│').any(|part| {
                part.trim()
                    .trim_start_matches('›')
                    .split_whitespace()
                    .next()
                    == Some(value)
            })
        })
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "expected exactly one rendered root {value:?}:\n{}",
        screen(dashboard)
    );
    lines.remove(0)
}

#[cfg(target_os = "macos")]
#[test]
fn pending_root_move_and_sound_save_match_interleaved_server_replies() {
    use ovrcr::protocol::{ReadySoundChoice, exchange_preamble};
    use std::os::unix::net::UnixStream;

    let original = "\
desktop_notifications = false # keep banners off
ready_sound = true # keep saved sound-only opt-in
ready_sound_choice = \"tap\" # tone
picker_roots = [\"/a\", \"/b\", \"/c\"] # roots
branch_prefix = 'keep/' # unrelated
[quota]
enabled = false
";
    for refuse_root in [false, true] {
        for accept_first in [false, true] {
            let fixture = live::Live::idle().bounded();
            let document = fixture.config.join("dashboard.toml");
            std::fs::write(&document, original).unwrap();
            fixture.start_binary();
            let mut stream = UnixStream::connect(&fixture.socket).unwrap();
            stream.set_read_timeout(fixture.timeout()).unwrap();
            stream.set_write_timeout(fixture.timeout()).unwrap();
            exchange_preamble(&mut stream).unwrap();
            let mut dashboard = Dashboard::new(TerminalSize {
                rows: 60,
                cols: 120,
            });
            dashboard.install_area(Rect::new(0, 0, 120, 60));
            for frame in settings_frames(
                &mut stream,
                &ClientMessage {
                    request_id: u64::MAX,
                    request: Request::DashboardHello,
                },
                true,
            )
            .unwrap()
            {
                if let ServerMessage::Event(ServerEvent::SettingsChanged(report)) = &frame {
                    assert_eq!(report.path, document);
                    assert!(!report.settings.desktop_notifications);
                    assert!(report.settings.ready_sound);
                    assert_eq!(
                        report.settings.ready_sound_choice,
                        Some(ReadySoundChoice::Tap)
                    );
                }
                dashboard.handle_server_message(frame);
            }
            dashboard.key(KeyCode::Char(':'));
            dashboard.event_action(Event::Paste("settings".into()));
            dashboard.key(KeyCode::Enter);

            select(&mut dashboard, "picker_roots[1]");
            let DashboardAction::Request(root_move) = dashboard.key(KeyCode::Char(']')) else {
                panic!("moving a picker root sent no request");
            };
            assert_eq!(
                root_move.request,
                Request::SetSetting {
                    path: "picker_roots".into(),
                    value: Some("[\"/a\", \"/c\", \"/b\"]".into()),
                }
            );
            // Match the exact row value: the document's private path may
            // contain `/b`, and child rows do not display source badges.
            assert!(picker_root_line(&dashboard, "/b").contains("saving…"));

            select(&mut dashboard, "ready_sound_choice");
            assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
            dashboard.event_action(Event::Paste("Rise".into()));
            let DashboardAction::Request(sound_save) = dashboard.key(KeyCode::Enter) else {
                panic!("choosing a tone sent no request");
            };
            assert_eq!(
                sound_save.request,
                Request::SetSetting {
                    path: "ready_sound_choice".into(),
                    value: Some("\"rise\"".into()),
                }
            );
            assert_ne!(root_move.request_id, sound_save.request_id);
            let tone = line_with(&dashboard, "Ready sound choice");
            assert!(
                tone.contains("Tap  set") && tone.contains("saving…"),
                "{tone}"
            );
            assert_eq!(std::fs::read_to_string(&document).unwrap(), original);

            let (refused, accepted) = if refuse_root {
                (&root_move, &sound_save)
            } else {
                (&sound_save, &root_move)
            };
            // A real Server write refusal, without changing document bytes or
            // asking the watcher to observe a temporary invalid document.
            let permissions = std::fs::metadata(&document).unwrap().permissions();
            let mut read_only = permissions.clone();
            read_only.set_readonly(true);
            std::fs::set_permissions(&document, read_only).unwrap();
            let refusal = settings_frames(&mut stream, refused, false);
            std::fs::set_permissions(&document, permissions).unwrap();
            let refusal = refusal.unwrap();
            let refused_path = if refuse_root {
                "picker_roots"
            } else {
                "ready_sound_choice"
            };
            assert!(
                refusal.iter().any(|frame| matches!(
                    frame,
                    ServerMessage::Response {
                        request_id,
                        response: Response::Error { code: ErrorCode::InvalidRequest, message },
                    } if *request_id == refused.request_id
                        && message.contains(&format!("could not save {refused_path}"))
                        && message.contains("read-only")
                )),
                "{refusal:?}"
            );
            assert_eq!(std::fs::read_to_string(&document).unwrap(), original);
            let accepted_frames = settings_frames(&mut stream, accepted, true).unwrap();
            assert!(
                accepted_frames.iter().any(|frame| matches!(
                    frame,
                    ServerMessage::Response { request_id, response: Response::Ok }
                        if *request_id == accepted.request_id
                )),
                "{accepted_frames:?}"
            );
            let saved = original.replace(
                if refuse_root {
                    "\"tap\""
                } else {
                    "[\"/a\", \"/b\", \"/c\"]"
                },
                if refuse_root {
                    "\"rise\""
                } else {
                    "[\"/a\", \"/c\", \"/b\"]"
                },
            );
            assert_eq!(std::fs::read_to_string(&document).unwrap(), saved);

            // Both replies exist, but neither has been delivered. The
            // Dashboard still displays the old reading and both pending IDs.
            select(&mut dashboard, "picker_roots[1]");
            assert!(picker_root_line(&dashboard, "/b").contains("saving…"));
            select(&mut dashboard, "ready_sound_choice");
            let tone = line_with(&dashboard, "Ready sound choice");
            assert!(
                tone.contains("Tap  set") && tone.contains("saving…"),
                "{tone}"
            );
            let (first, second) = if accept_first {
                (accepted_frames, refusal)
            } else {
                (refusal, accepted_frames)
            };
            for (index, frames) in [first, second].into_iter().enumerate() {
                for frame in frames {
                    if let ServerMessage::Event(ServerEvent::SettingsChanged(report)) = &frame {
                        assert!(!report.settings.desktop_notifications);
                        assert!(report.settings.ready_sound);
                        assert_eq!(
                            report.settings.ready_sound_choice,
                            Some(if refuse_root {
                                ReadySoundChoice::Rise
                            } else {
                                ReadySoundChoice::Tap
                            })
                        );
                        assert_eq!(
                            report.settings.picker_roots,
                            (if refuse_root {
                                ["/a", "/b", "/c"]
                            } else {
                                ["/a", "/c", "/b"]
                            })
                            .map(PathBuf::from)
                        );
                    }
                    dashboard.handle_server_message(frame);
                }
                let refusal_arrived = !accept_first || index == 1;
                let acceptance_arrived = accept_first || index == 1;
                select(&mut dashboard, "picker_roots[1]");
                let root = picker_root_line(
                    &dashboard,
                    if !refuse_root && acceptance_arrived {
                        "/c"
                    } else {
                        "/b"
                    },
                );
                assert_eq!(
                    root.contains("saving…"),
                    if refuse_root {
                        !refusal_arrived
                    } else {
                        !acceptance_arrived
                    }
                );
                assert_eq!(
                    screen(&dashboard).contains("refused:"),
                    refuse_root && refusal_arrived
                );
                select(&mut dashboard, "ready_sound_choice");
                let tone = line_with(&dashboard, "Ready sound choice");
                assert!(
                    tone.contains(if refuse_root && acceptance_arrived {
                        "Rise  set"
                    } else {
                        "Tap  set"
                    }),
                    "{tone}"
                );
                assert_eq!(
                    tone.contains("saving…"),
                    if refuse_root {
                        !acceptance_arrived
                    } else {
                        !refusal_arrived
                    }
                );
                assert_eq!(
                    screen(&dashboard).contains("refused:"),
                    !refuse_root && refusal_arrived
                );
                assert!(!rendered_footer(&dashboard, 120).contains("could not save"));
                select(&mut dashboard, "desktop_notifications");
                assert!(line_with(&dashboard, "Desktop notifications").contains("off  set"));
                select(&mut dashboard, "ready_sound");
                assert!(line_with(&dashboard, "Ready sound  ").contains("on  set"));
            }
            assert_eq!(std::fs::read_to_string(&document).unwrap(), saved);
            drop(stream);
            assert_eq!(
                fixture.request(Request::Shutdown { kill: true }),
                Response::Ok
            );
            fixture.join();
        }
    }
}

#[test]
fn enum_edits_in_place_with_a_pick_list() {
    let (mut dashboard, _root, document) = editor("");
    select(&mut dashboard, "automatic");
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    let text = screen(&dashboard);
    assert!(text.contains("› default_branch_only"), "{text}");
    dashboard.key(KeyCode::Up);
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(
        (path.as_str(), value.as_deref()),
        ("automatic_local_terminals", Some("\"off\""))
    );
    assert!(
        line_with(&dashboard, "Automatic local terminals")
            .contains("off  set  (default: default_branch_only)")
    );
}

#[test]
fn string_edits_in_place_and_escape_cancels_only_that_edit() {
    let (mut dashboard, _root, document) = editor("# keep me\n");
    select(&mut dashboard, "branch prefix");
    dashboard.key(KeyCode::Enter);
    dashboard.event_action(Event::Paste("zz".into()));
    assert!(line_with(&dashboard, "Branch prefix").contains("[feature/zz"));
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
    assert!(open(&dashboard), "Escape cancels the edit, not the view");
    assert!(line_with(&dashboard, "Branch prefix").contains("feature/  default"));
    dashboard.key(KeyCode::Enter);
    for _ in 0.."feature/".len() {
        dashboard.key(KeyCode::Backspace);
    }
    for ch in "kh/".chars() {
        dashboard.key(KeyCode::Char(ch));
    }
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(
        (path.as_str(), value.as_deref()),
        ("branch_prefix", Some("\"kh/\""))
    );
    assert!(line_with(&dashboard, "Branch prefix").contains("kh/  set  (default: feature/)"));
    let saved = std::fs::read_to_string(&document).unwrap();
    assert!(saved.contains("# keep me\n"), "{saved}");
    assert!(saved.contains("branch_prefix = \"kh/\"\n"), "{saved}");
}

#[test]
fn collection_elements_edit_add_and_remove_without_reordering() {
    let (mut dashboard, _root, document) = editor(
        "[[agents]]\nname = \"claude\"\nargv = [\"claude\"]\n\n[launch_choices.demo]\nkind = \"Terminal\"\n",
    );
    // argv is edited as a TOML array literal.
    select(&mut dashboard, "agents claude");
    dashboard.key(KeyCode::Enter);
    for _ in 0..20 {
        dashboard.key(KeyCode::Backspace);
    }
    dashboard.event_action(Event::Paste("[\"claude\", \"--verbose\"]".into()));
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(path, "agents[0].argv");
    assert_eq!(value.as_deref(), Some("[\"claude\", \"--verbose\"]"));
    assert!(line_with(&dashboard, "claude  [").contains("[\"claude\", \"--verbose\"]"));

    // Add appends at the end.
    select(&mut dashboard, "add agent");
    dashboard.key(KeyCode::Enter);
    dashboard.event_action(Event::Paste("codex".into()));
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(path, "agents[1]");
    assert_eq!(
        value.as_deref(),
        Some("{ name = \"codex\", argv = [\"codex\"] }")
    );
    dashboard.key(KeyCode::Esc);
    let text = screen(&dashboard);
    assert!(
        text.find("claude  [").unwrap() < text.find("codex  [").unwrap(),
        "{text}"
    );

    // Remove is a child-row action; Reset is not.
    select(&mut dashboard, "launches demo");
    assert_eq!(dashboard.key(KeyCode::Char('r')), DashboardAction::Redraw);
    assert!(screen(&dashboard).contains("Reset applies to a whole setting; x removes this one"));
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Char('x'));
    assert_eq!((path.as_str(), value), ("launch_choices.demo", None));
    assert!(!screen(&dashboard).contains("demo  Terminal"));
    let saved = std::fs::read_to_string(&document).unwrap();
    assert!(!saved.contains("launch_choices"), "{saved}");
}

/// While `picker_roots` is its default, no list is in the document, so an
/// added root writes the default list with the new root appended.
#[test]
fn adding_to_a_default_list_keeps_its_default_elements() {
    let (mut dashboard, root, document) = editor("");
    let defaults = dashboard_reading(&document).settings.picker_roots;
    let added = root.path().display().to_string();
    select(&mut dashboard, "add root");
    dashboard.key(KeyCode::Enter);
    dashboard.event_action(Event::Paste(added.clone()));
    let (path, _) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(path, "picker_roots");
    let mut expected = defaults;
    expected.push(root.path().to_path_buf());
    assert_eq!(dashboard_reading(&document).settings.picker_roots, expected);
    dashboard.key(KeyCode::Esc);
    assert!(line_with(&dashboard, "Picker roots").contains("set"));
}

fn dashboard_reading(document: &Path) -> SettingsReport {
    ovrcr::settings::parse(
        document,
        &std::fs::read_to_string(document).unwrap_or_default(),
    )
}

#[test]
fn reset_is_offered_only_for_a_value_the_document_sets() {
    let (mut dashboard, _root, document) =
        editor("# top\nbranch_prefix = \"kh/\"\nready_sound = true\n");
    select(&mut dashboard, "title model");
    assert_eq!(dashboard.key(KeyCode::Char('r')), DashboardAction::Redraw);
    assert!(screen(&dashboard).contains("Title model is already its default"));
    select(&mut dashboard, "branch prefix");
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Char('r'));
    assert_eq!((path.as_str(), value), ("branch_prefix", None));
    assert!(line_with(&dashboard, "Branch prefix").contains("feature/  default"));
    assert_eq!(
        std::fs::read_to_string(&document).unwrap(),
        "# top\nready_sound = true\n"
    );
}

#[test]
fn a_rejected_value_shows_on_its_row_and_leaves_the_document_unchanged() {
    let contents = "title_model = \"anthropic/claude\"\n";
    let (mut dashboard, _root, document) = editor(contents);
    select(&mut dashboard, "title model");
    dashboard.key(KeyCode::Enter);
    for _ in 0..40 {
        dashboard.key(KeyCode::Backspace);
    }
    dashboard.event_action(Event::Paste("no-slash".into()));
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(
        (path.as_str(), value.as_deref()),
        ("title_model", Some("\"no-slash\""))
    );
    assert_eq!(std::fs::read_to_string(&document).unwrap(), contents);
    let text = screen(&dashboard);
    let refused = text
        .find("refused: could not save title_model")
        .unwrap_or_else(|| panic!("{text}"));
    assert!(refused > text.find("Title model").unwrap());
    assert!(line_with(&dashboard, "Title model").contains("anthropic/claude  set"));
    assert!(
        !rendered_footer(&dashboard, 120).contains("could not save"),
        "the row, not the footer"
    );
}

#[test]
fn an_unparseable_document_disables_editing_and_says_why() {
    let (mut dashboard, _root, _document) = editor("ready_sound = true\nnot toml {\n");
    let text = screen(&dashboard);
    assert!(
        text.contains("line 2, document: settings document is not valid TOML"),
        "{text}"
    );
    assert!(text.contains("Editing is off until dashboard.toml is fixed by hand."));
    assert!(line_with(&dashboard, "Ready sound").contains("off  default"));
    select(&mut dashboard, "ready");
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(dashboard.key(KeyCode::Char('r')), DashboardAction::Redraw);
}

#[test]
fn slash_filters_rows_across_groups() {
    let (mut dashboard, _root, _document) = editor("");
    dashboard.key(KeyCode::Char('/'));
    dashboard.event_action(Event::Paste("home".into()));
    let text = screen(&dashboard);
    assert!(
        text.contains("Codex home") && text.contains("Grok home"),
        "{text}"
    );
    assert!(
        !text.contains("Branch prefix") && !text.contains("Alerts"),
        "{text}"
    );
    // Escape clears the filter before it closes the view.
    dashboard.key(KeyCode::Esc);
    assert!(screen(&dashboard).contains("Branch prefix"));
    dashboard.key(KeyCode::Esc);
    assert!(!open(&dashboard));
}

/// A remembered launch edits kind and preset together in one pick list.
#[test]
fn remembered_launch_picks_kind_and_preset_together() {
    let (mut dashboard, _root, document) =
        editor("[launch_choices.demo]\nkind = \"Agent\"\npreset = \"fixture\"\n");
    select(&mut dashboard, "launches demo");
    line_with(&dashboard, "demo  Agent: fixture");
    dashboard.key(KeyCode::Enter);
    let text = screen(&dashboard);
    assert!(text.contains("› Agent: fixture"), "{text}");
    dashboard.event_action(Event::Paste("terminal".into()));
    let (path, value) = serve(&mut dashboard, &document, KeyCode::Enter);
    assert_eq!(path, "launch_choices.demo");
    assert_eq!(value.as_deref(), Some("{ kind = \"Terminal\" }"));
    line_with(&dashboard, "demo  Terminal");
    let saved = std::fs::read_to_string(&document).unwrap();
    assert!(!saved.contains("preset"), "{saved}");
}
