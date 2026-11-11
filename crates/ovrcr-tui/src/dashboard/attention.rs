//! "Who needs me?": the sessions asking for their user — an open Input request first, then an
//! Unread response. The sidebar's attention strip counts them and the jump key visits them, so
//! both read this one classification and this one order.
use super::render::{MUTED, TEAL, TEXT, YELLOW};
use super::state::find_session;
use super::{Dashboard, DashboardAction, TreeRow};
use ovrcr_protocol::{SessionId, SessionSummary};
use ratatui::Frame;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// What a session asks of its user. The order is the jump order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Attention {
    /// A running session whose effective activity is WaitingInput.
    Input,
    /// An Unread response from a session that is not busy again. A busy session's older
    /// Unread waits for the turn in progress: its row shows the spinner, not `✓`.
    Ready,
}

/// Paused sessions never need input (they cannot take an answer until resumed), and Operator
/// conversations stay out: the one Operator navigation row shows its own Unread.
pub(super) fn attention(session: &SessionSummary) -> Option<Attention> {
    super::status::SessionStatus::attention(session)
}

impl Dashboard {
    /// Sessions needing an answer and sessions with an Unread response, across every project,
    /// folded or scrolled away, the focused one included.
    pub(super) fn attention_counts(&self) -> (usize, usize) {
        self.hierarchy
            .projects
            .iter()
            .flat_map(|project| &project.workspaces)
            .flat_map(|workspace| &workspace.sessions)
            .filter_map(attention)
            .fold((0, 0), |(input, ready), attention| match attention {
                Attention::Input => (input + 1, ready),
                Attention::Ready => (input, ready + 1),
            })
    }

    /// Where the jump key goes. From a session in the queue (Input first, then Ready, each in
    /// fully expanded tree order) it is the next entry, so repeated presses visit every one;
    /// from anywhere else it is the first of the leading kind after the selection, wrapping.
    pub(super) fn next_attention(&self) -> Option<(Attention, SessionId)> {
        let rows = self.hierarchy_rows(true);
        let mut queue: Vec<_> = rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let TreeRow::Session { id } = row else {
                    return None;
                };
                attention(find_session(self, *id)?).map(|attention| (attention, index, *id))
            })
            .collect();
        queue.sort_by_key(|&(attention, index, _)| (attention, index));
        let here = match &self.selected_container {
            Some(container) => rows.iter().position(|row| row == container),
            None => self
                .focused_session()
                .and_then(|id| rows.iter().position(|row| *row == TreeRow::Session { id })),
        };
        let next = match here.and_then(|here| queue.iter().position(|entry| entry.1 == here)) {
            Some(at) => queue.get((at + 1) % queue.len()),
            None => {
                let lead = queue.first()?.0;
                let group = || queue.iter().filter(|entry| entry.0 == lead);
                group()
                    .find(|entry| here.is_some_and(|here| entry.1 > here))
                    .or_else(|| group().next())
            }
        };
        next.map(|&(attention, _, id)| (attention, id))
    }

    /// Select `next_attention` in the focused pane, unfolding its project and workspace so the
    /// selected row is on screen.
    pub(super) fn jump_to_attention(&mut self) -> DashboardAction {
        let Some((_, id)) = self.next_attention() else {
            self.show_notice("No session needs you");
            return DashboardAction::Redraw;
        };
        if let Some(session) = find_session(self, id) {
            let place = (session.project.clone(), session.workspace.clone());
            self.collapsed_projects.remove(&place.0);
            self.collapsed_workspaces.remove(&place);
        }
        self.select_session(id);
        self.request_selected()
    }

    /// The strip's line; `sidebar_rects` gives it one only while there is something to say.
    pub(super) fn draw_attention(&self, frame: &mut Frame<'_>) {
        let area = self.sidebar_rects(frame.area()).attention;
        if area.is_empty() {
            return;
        }
        let (input, ready) = self.attention_counts();
        frame.render_widget(
            Paragraph::new(attention_line(input, ready, usize::from(area.width))),
            area,
        );
    }
}

/// ` ? 1 needs input · ✓ 2 ready`, in jump order, or only the counts when the words do not fit.
pub(super) fn attention_line(input: usize, ready: usize, width: usize) -> Line<'static> {
    let line = |words: bool| {
        let mut spans = vec![Span::raw(" ")];
        for (count, glyph, color, label) in [
            (
                input,
                "?",
                YELLOW(),
                if input == 1 {
                    "needs input"
                } else {
                    "need input"
                },
            ),
            (ready, "✓", TEAL(), "ready"),
        ] {
            if count == 0 {
                continue;
            }
            if spans.len() > 1 {
                spans.push(Span::styled(" · ", Style::default().fg(MUTED())));
            }
            spans.push(Span::styled(glyph, Style::default().fg(color)));
            spans.push(Span::styled(
                if words {
                    format!(" {count} {label}")
                } else {
                    format!(" {count}")
                },
                Style::default().fg(TEXT()),
            ));
        }
        Line::from(spans)
    };
    let full = line(true);
    if full.width() <= width {
        full
    } else {
        line(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        ActivitySample, AgentActivity, AgentBinding, AgentProvider, AgentSnapshot, ClientMessage,
        HealthSample, HierarchySnapshot, InputKind, InputRequest, ProjectSummary, ReadyObservation,
        ReporterHealth, Request, SampleQuality, SessionKind, SessionPhase, SessionRunId,
        TerminalSize, WorkspaceSummary,
    };
    use ratatui::layout::Rect;

    fn observation(id: u64) -> ReadyObservation {
        ReadyObservation {
            binding: AgentBinding {
                provider: AgentProvider::Codex,
                invocation: format!("i{id}"),
                conversation: format!("c{id}"),
                generation: 1,
            },
            turn: Some("t".into()),
            activity_revision: 1,
        }
    }

    fn session(id: u64, workspace: &str, input: bool, unread: bool) -> SessionSummary {
        SessionSummary {
            scope: ovrcr_protocol::SessionScope::Workspace,
            archived: false,
            cwd: "/work".into(),
            id: SessionId(id),
            run: SessionRunId(1),
            kind: SessionKind::Agent {
                name: "codex".into(),
            },
            recovery: None,
            project: "p".into(),
            workspace: workspace.into(),
            name: format!("agent-{id}"),
            title: None,
            manual_title: None,
            assignment_control: None,
            label: "codex".into(),
            pid: Some(1),
            started_unix_ms: Some(0),
            last_activity_unix_ms: None,
            phase: SessionPhase::Running,
            activity: AgentActivity::Idle,
            context_usage: None,
            agent_epoch: 1,
            unread: unread.then(|| observation(id)),
            agent: Some(AgentSnapshot {
                binding: observation(id).binding,
                activity: Some(ActivitySample {
                    state: if unread {
                        AgentActivity::ResponseReady
                    } else {
                        AgentActivity::Busy
                    },
                    quality: SampleQuality::Observed,
                    turn: Some("t".into()),
                }),
                metrics: None,
                health: HealthSample {
                    state: ReporterHealth::Connected,
                    reason: None,
                },
                activity_revision: 1,
                metrics_revision: 0,
                health_revision: 0,
                input_requests: if input {
                    vec![InputRequest {
                        id: "question:q".into(),
                        kind: InputKind::Confirm,
                    }]
                } else {
                    Vec::new()
                },
                input_revision: 1,
                native_control: None,
                native_control_revision: 0,
            }),
        }
    }

    fn workspace(id: &str, sessions: Vec<SessionSummary>) -> WorkspaceSummary {
        WorkspaceSummary {
            project: "p".into(),
            name: id.into(),
            id: id.into(),
            root: false,
            warning: None,
            path: format!("/{id}").into(),
            sessions,
        }
    }

    /// Tree order: a/1 a/2 a/3 · b/4 b/5 · c/7 c/6. Input: 2, 5. Ready: 3, 6 (and 5, which
    /// counts once, as Input).
    fn dashboard() -> Dashboard {
        let mut dashboard = Dashboard::new(TerminalSize {
            rows: 30,
            cols: 120,
        });
        dashboard.install_area(Rect::new(0, 0, 120, 30));
        dashboard.install_hierarchy(HierarchySnapshot {
            usage: ovrcr_protocol::UsageSnapshot::default(),
            projects: vec![ProjectSummary {
                name: "p".into(),
                workspaces: vec![
                    workspace(
                        "a",
                        vec![
                            session(1, "a", false, false),
                            session(2, "a", true, false),
                            session(3, "a", false, true),
                        ],
                    ),
                    workspace(
                        "b",
                        vec![session(4, "b", false, false), session(5, "b", true, true)],
                    ),
                    workspace(
                        "c",
                        vec![session(7, "c", false, false), session(6, "c", false, true)],
                    ),
                ],
            }],
        });
        dashboard.install_focus(SessionId(1));
        dashboard
    }

    #[test]
    fn input_outranks_unread_and_only_a_running_session_waits() {
        let mut waiting = session(1, "a", true, true);
        assert_eq!(attention(&waiting), Some(Attention::Input));
        waiting.phase = SessionPhase::Paused;
        assert_eq!(
            attention(&waiting),
            Some(Attention::Ready),
            "a paused session cannot take the answer; its response is still unread"
        );
        waiting.unread = None;
        assert_eq!(attention(&waiting), None);
        let mut ready = session(2, "a", false, true);
        assert_eq!(attention(&ready), Some(Attention::Ready));
        ready
            .agent
            .as_mut()
            .unwrap()
            .activity
            .as_mut()
            .unwrap()
            .state = AgentActivity::Busy;
        assert_eq!(
            attention(&ready),
            None,
            "busy again: the older Unread waits, as the row's spinner says"
        );
        ready
            .agent
            .as_mut()
            .unwrap()
            .activity
            .as_mut()
            .unwrap()
            .state = AgentActivity::ResponseReady;
        ready.phase = SessionPhase::Exited {
            code: Some(0),
            signal: None,
        };
        assert_eq!(attention(&ready), Some(Attention::Ready));
        ready.unread = None;
        assert_eq!(attention(&ready), None);
        assert!(Attention::Input < Attention::Ready);
    }

    #[test]
    fn counts_cover_folded_sessions_and_the_focused_one() {
        let mut dashboard = dashboard();
        assert_eq!(dashboard.attention_counts(), (2, 2));
        dashboard.toggle_row_collapse(&TreeRow::Project { name: "p".into() });
        dashboard.install_focus(SessionId(2));
        assert_eq!(dashboard.attention_counts(), (2, 2));
    }

    #[test]
    fn jump_visits_input_then_ready_in_tree_order_and_wraps() {
        let mut dashboard = dashboard();
        let mut visited = Vec::new();
        for _ in 0..5 {
            assert_eq!(
                dashboard.key(crossterm::event::KeyCode::Char('i')),
                DashboardAction::Redraw
            );
            visited.push(dashboard.focused_session().unwrap().0);
        }
        assert_eq!(visited, [2, 5, 3, 6, 2]);
    }

    #[test]
    fn jump_starts_after_the_selection_within_the_leading_kind() {
        let mut dashboard = dashboard();
        dashboard.install_focus(SessionId(4));
        assert_eq!(
            dashboard.next_attention(),
            Some((Attention::Input, SessionId(5)))
        );
        dashboard.install_focus(SessionId(7));
        assert_eq!(
            dashboard.next_attention(),
            Some((Attention::Input, SessionId(2))),
            "past the last Input it wraps to the first Input, not to the Ready below"
        );
        dashboard.select_container(TreeRow::Workspace {
            project: "p".into(),
            id: "b".into(),
        });
        assert_eq!(
            dashboard.next_attention(),
            Some((Attention::Input, SessionId(5))),
            "a selected workspace row is a place in the tree too"
        );
        for id in [2, 5] {
            let summary = dashboard.hierarchy.projects[0]
                .workspaces
                .iter_mut()
                .flat_map(|workspace| &mut workspace.sessions)
                .find(|session| session.id == SessionId(id))
                .unwrap();
            summary.agent.as_mut().unwrap().input_requests.clear();
        }
        dashboard.install_focus(SessionId(4));
        assert_eq!(
            dashboard.next_attention(),
            Some((Attention::Ready, SessionId(5))),
            "with no Input left, Ready leads"
        );
        for workspace in &mut dashboard.hierarchy.projects[0].workspaces {
            for session in &mut workspace.sessions {
                session.unread = None;
            }
        }
        assert_eq!(dashboard.next_attention(), None);
        dashboard.drain_outbox();
        assert_eq!(
            dashboard.key(crossterm::event::KeyCode::Char('i')),
            DashboardAction::Redraw
        );
        assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
        assert_eq!(
            dashboard.desktop.notice.as_deref(),
            Some("No session needs you")
        );
        assert!(dashboard.drain_outbox().is_empty(), "nothing to view");
    }

    #[test]
    fn jump_unfolds_the_target_and_asks_for_its_screen() {
        let mut dashboard = dashboard();
        dashboard.drain_outbox();
        dashboard.toggle_row_collapse(&TreeRow::Workspace {
            project: "p".into(),
            id: "a".into(),
        });
        dashboard.toggle_row_collapse(&TreeRow::Project { name: "p".into() });
        dashboard.install_focus(SessionId(1));
        assert!(
            !dashboard
                .visible_rows()
                .contains(&TreeRow::Session { id: SessionId(2) })
        );
        dashboard.jump_to_attention();
        assert_eq!(dashboard.focused_session(), Some(SessionId(2)));
        assert!(
            dashboard
                .visible_rows()
                .contains(&TreeRow::Session { id: SessionId(2) }),
            "the selected row is drawn"
        );
        assert!(
            dashboard.drain_outbox().iter().any(|message| matches!(
                message,
                ClientMessage {
                    request: Request::SetView { view },
                    ..
                } if view.focused == Some(SessionId(2))
            )),
            "the focused pane follows the selection"
        );
    }

    fn screen(dashboard: &Dashboard) -> Vec<String> {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| super::super::draw_dashboard_at(frame, dashboard, 0))
            .unwrap();
        (0..30)
            .map(|y| {
                (0..39)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    fn click(row: u16) -> crossterm::event::MouseEvent {
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 8,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }
    }

    #[test]
    fn strip_sits_above_the_quota_section_and_a_click_jumps_like_the_key() {
        let mut dashboard = dashboard();
        let rows = screen(&dashboard);
        // The tree leads; the strip sits directly above the six-row quota
        // section, and the Operator dock keeps the sidebar's last row.
        assert!(rows[1].trim_end().starts_with(" P"), "{}", rows[1]);
        assert_eq!(rows[21].trim_end(), " ? 2 need input · ✓ 2 ready");
        assert!(rows[22].contains("QUOTA LEFT"), "{}", rows[22]);
        assert!(rows[28].contains("Operator"), "{}", rows[28]);
        assert!(!rows[28].contains("[+]") && !rows[28].contains("▸"));
        assert_eq!(
            dashboard.mouse_action(click(21), Rect::new(0, 0, 120, 30)),
            DashboardAction::Redraw
        );
        assert_eq!(dashboard.focused_session(), Some(SessionId(2)));
    }

    #[test]
    fn a_new_unread_moves_no_row_under_the_pointer() {
        let mut dashboard = dashboard();
        for workspace in &mut dashboard.hierarchy.projects[0].workspaces {
            for session in &mut workspace.sessions {
                session.unread = None;
                session.agent.as_mut().unwrap().input_requests.clear();
            }
        }
        let before = screen(&dashboard);
        assert!(
            !before.iter().any(|row| row.contains(" ready")),
            "no strip yet"
        );
        let y = before
            .iter()
            .position(|row| row.contains("agent-6"))
            .expect("agent-6 row");
        // The user aims at agent-6; a Ready for it lands before the click.
        let mut ready = session(6, "c", false, true);
        ready.project = "p".into();
        dashboard.handle_server_message(crate::protocol::ServerMessage::Event(
            crate::protocol::ServerEvent::SessionChanged(Box::new(ready)),
        ));
        let after = screen(&dashboard);
        assert!(
            after.iter().any(|row| row.trim_end() == " ✓ 1 ready"),
            "{after:?}"
        );
        assert_eq!(
            after[..y],
            before[..y],
            "the rows above the target stay put"
        );
        assert!(
            after[y].contains("✓ agent-6"),
            "the target keeps its line: {}",
            after[y]
        );
        dashboard.mouse_action(click(y as u16), Rect::new(0, 0, 120, 30));
        assert_eq!(dashboard.focused_session(), Some(SessionId(6)));
    }

    #[test]
    fn browse_footer_names_the_jump_only_while_a_session_needs_you() {
        let mut dashboard = dashboard();
        let footer = |dashboard: &Dashboard| super::super::hints::footer(dashboard, 120);
        assert!(
            footer(&dashboard).ends_with("? Help  i Next input needed"),
            "{}",
            footer(&dashboard)
        );
        for workspace in &mut dashboard.hierarchy.projects[0].workspaces {
            for session in &mut workspace.sessions {
                session.agent.as_mut().unwrap().input_requests.clear();
            }
        }
        assert!(footer(&dashboard).ends_with("? Help  i Next ready"));
        for workspace in &mut dashboard.hierarchy.projects[0].workspaces {
            for session in &mut workspace.sessions {
                session.unread = None;
            }
        }
        assert!(
            footer(&dashboard).ends_with("? Help"),
            "{}",
            footer(&dashboard)
        );
    }

    #[test]
    fn strip_gives_up_its_words_before_its_counts() {
        let text = |line: Line<'static>| line.to_string();
        assert_eq!(
            text(attention_line(1, 2, 40)),
            " ? 1 needs input · ✓ 2 ready"
        );
        assert_eq!(text(attention_line(0, 3, 40)), " ✓ 3 ready");
        assert_eq!(text(attention_line(4, 0, 40)), " ? 4 need input");
        assert_eq!(text(attention_line(1, 2, 20)), " ? 1 · ✓ 2");
    }
}
