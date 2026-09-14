use super::scripted::{Answer, Observed, Supervisor};
use super::*;
use ovrcr_protocol::{
    ActivitySample, AgentActivity, AgentObservation, InputKind, InputRequest, ReporterHealth,
    SampleQuality,
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

fn busy(turn: &str) -> AgentObservation {
    AgentObservation::Activity(ActivitySample {
        state: AgentActivity::Busy,
        quality: SampleQuality::Observed,
        turn: Some(turn.to_owned()),
    })
}

fn unbound() -> Reporter {
    Reporter::new(AgentProvider::Pi, None, None)
}

#[test]
fn preflight_names_the_first_reason_a_launch_cannot_be_reported_on() {
    // The terminal is a fact the caller supplies, so the order these are answered in is
    // the same whether or not the test binary itself has one.
    let argv = vec![OsString::from("pi")];
    assert_eq!(
        preflight(false, &argv, |_| false, false),
        Some("no managed reservation"),
        "a reservation is checked before anything about the launch"
    );
    assert_eq!(
        preflight(true, &argv, |_| false, false),
        Some("unsupported launch arguments"),
        "an ineligible launch is named before the terminal it would have needed"
    );
    assert_eq!(
        preflight(true, &argv, |_| true, false),
        Some("interactive terminal required")
    );
    assert_eq!(preflight(true, &argv, |_| true, true), None);
}

#[test]
fn admit_fences_gaps_retired_producers_and_an_unobserved_replacement() {
    let mut reporter = unbound();
    assert_eq!(
        reporter.admit_producer("a", 1, false),
        Fence::Ignored,
        "only a frame that announces an instance admits an unknown one"
    );
    assert_eq!(reporter.admit_producer("a", 5, true), Fence::Accepted);
    assert_eq!(reporter.admit_producer("a", 5, false), Fence::Ignored);
    assert_eq!(reporter.admit_producer("a", 4, false), Fence::Ignored);
    assert_eq!(
        reporter.admit_producer("a", 7, false),
        Fence::Gap,
        "a missing source sequence is a gap, not an acceptance"
    );
    assert_eq!(
        reporter.admit_producer("a", 8, false),
        Fence::Accepted,
        "the gap advanced the sequence so the same hole is reported once"
    );
    // An unobserved factory replacement: a new instance announces itself while the
    // first is still live. The old producer retires and the new one is admitted.
    assert_eq!(reporter.admit_producer("b", 1, true), Fence::LostClose);
    // A -> B -> A: every delayed frame from the retired first A is ignored, however
    // high its sequence, and it can never be re-admitted by its own announcement.
    assert_eq!(reporter.admit_producer("a", 99, true), Fence::Ignored);
    assert_eq!(reporter.admit_producer("a", 99, false), Fence::Ignored);
    assert_eq!(reporter.admit_producer("b", 2, false), Fence::Accepted);
}

#[test]
fn a_retired_producer_leaves_no_producer_behind_and_cannot_return() {
    let mut reporter = unbound();
    assert_eq!(reporter.admit_producer("a", 1, true), Fence::Accepted);
    reporter.retire("a");
    assert_eq!(
        reporter.admit_producer("a", 2, true),
        Fence::Ignored,
        "an observed shutdown retires the instance for good"
    );
    assert_eq!(
        reporter.admit_producer("b", 1, true),
        Fence::Accepted,
        "a successor is admitted without being counted as an unobserved loss"
    );
    assert!(!reporter.closed());
}

#[test]
fn the_identity_budget_answers_duplicates_before_capacity() {
    let mut reporter = unbound();
    assert_eq!(
        reporter.admit_cycle("a:1"),
        None,
        "a fresh identity is charged"
    );
    assert!(reporter.published("a:1"));
    assert_eq!(reporter.admit_cycle("a:1"), Some(IGNORED.to_vec()));
    assert!(!reporter.published("a:2"));

    // A duplicate is answered with no capacity left at all, so a repeat can never
    // exhaust the budget or disable a healthy reporter.
    let mut reporter = unbound();
    assert_eq!(reporter.admit_cycle("a:1"), None);
    reporter.charged = MAX_IDENTITY_BYTES;
    assert_eq!(reporter.admit_cycle("a:1"), Some(IGNORED.to_vec()));
    assert!(!reporter.closed());
    let retained = reporter.cycles.clone();
    assert_eq!(
        reporter.admit_cycle("a:2"),
        Some(UNAVAILABLE.to_vec()),
        "an identity that cannot be retained is identity ambiguity"
    );
    assert!(reporter.closed());
    assert_eq!(
        reporter.cycles, retained,
        "exhaustion disables instead of evicting an identity it already published"
    );
}

#[test]
fn the_identity_budget_counts_fences_and_cycles_against_one_ceiling() {
    let mut reporter = unbound();
    for index in 0..MAX_IDENTITIES - 1 {
        assert_eq!(reporter.admit_cycle(&index.to_string()), None);
    }
    reporter.admit_producer("p", 1, true);
    reporter.retire("p");
    let retained = reporter.cycles.clone();
    assert_eq!(
        reporter.admit_cycle("one-too-many"),
        Some(UNAVAILABLE.to_vec()),
        "the fence took the last slot the cycles had left"
    );
    assert!(reporter.closed());
    assert_eq!(
        reporter.cycles, retained,
        "nothing was evicted to make room"
    );

    // The byte ceiling is reached by actual retained string sizes, independently
    // calculated rather than by setting the counter to its limit.
    let mut reporter = unbound();
    let mut index = 0;
    loop {
        let identity = format!("{index:0256}");
        if reporter.charged + identity.len() + std::mem::size_of::<String>() > MAX_IDENTITY_BYTES {
            let retained = reporter.cycles.clone();
            assert_eq!(reporter.admit_cycle(&identity), Some(UNAVAILABLE.to_vec()));
            assert_eq!(reporter.cycles, retained);
            break;
        }
        assert_eq!(reporter.admit_cycle(&identity), None);
        index += 1;
    }
    assert!(reporter.charged <= MAX_IDENTITY_BYTES);
    assert!(reporter.closed());
}

#[test]
fn one_revision_set_numbers_every_observation_kind_in_order() {
    // The server keeps a watermark per observation kind, so one monotonic counter is
    // newer than each of them and no two kinds can disagree about ordering.
    let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Pi);
    assert!(reporter.bind("sess-a", deadline(), false));
    assert_eq!(reporter.publish(busy("a:1"), deadline()), ACCEPTED);
    assert_eq!(
        reporter.publish(
            AgentObservation::Input(vec![InputRequest {
                id: "a:p1".into(),
                kind: InputKind::Select,
            }]),
            deadline()
        ),
        ACCEPTED
    );
    assert!(reporter.health(Some("source_gap"), deadline()));
    assert!(reporter.paused());
    let observed = supervisor.observed();
    let revisions: Vec<u64> = observed
        .iter()
        .filter_map(|entry| match entry {
            Observed::Report(report) | Observed::Health(report) => Some(report.revision),
            _ => None,
        })
        .collect();
    assert_eq!(revisions, vec![1, 2, 3]);
    assert_eq!(
        Supervisor::health(&observed),
        vec![(ReporterHealth::Unavailable, "source_gap".to_owned())]
    );
}

#[test]
fn a_bind_resets_the_revision_set_and_a_forced_bind_recovers_the_pause() {
    let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Pi);
    assert!(reporter.bind("sess-a", deadline(), false));
    assert_eq!(reporter.publish(busy("a:1"), deadline()), ACCEPTED);
    assert!(
        reporter.bind("sess-a", deadline(), false),
        "a bind to the conversation already bound is already done"
    );
    assert_eq!(reporter.admit_cycle("a:1"), None);
    assert!(
        reporter.bind("sess-b", deadline(), false),
        "a conversation switch is a fresh generation"
    );
    assert_eq!(
        reporter.admit_cycle("a:1"),
        Some(IGNORED.to_vec()),
        "a switch does not restart the producer's own cycle counter"
    );
    assert_eq!(reporter.publish(busy("b:1"), deadline()), ACCEPTED);
    assert!(reporter.health(Some("source_gap"), deadline()));
    assert!(reporter.paused());
    assert!(reporter.bind("sess-b", deadline(), true));
    assert!(!reporter.paused(), "a forced bind ends the pause");
    assert_eq!(
        reporter.admit_cycle("a:1"),
        None,
        "a fresh generation reads the same turn as a new response"
    );
    assert_eq!(reporter.publish(busy("a:1"), deadline()), ACCEPTED);
    let observed = supervisor.observed();
    assert_eq!(
        observed
            .iter()
            .filter(|entry| matches!(entry, Observed::Bind { .. }))
            .count(),
        3,
        "the repeat bind never reached the supervisor"
    );
    assert_eq!(
        Supervisor::activity(&observed)
            .into_iter()
            .map(|(generation, revision, state, turn, _)| (generation, revision, state, turn))
            .collect::<Vec<_>>(),
        vec![
            (1, 1, AgentActivity::Busy, Some("a:1".into())),
            (2, 1, AgentActivity::Busy, Some("b:1".into())),
            (3, 1, AgentActivity::Busy, Some("a:1".into())),
        ]
    );
}

fn binds(observed: &[Observed]) -> usize {
    observed
        .iter()
        .filter(|entry| matches!(entry, Observed::Bind { .. }))
        .count()
}
fn statuses(observed: &[Observed]) -> usize {
    observed
        .iter()
        .filter(|entry| matches!(entry, Observed::Status(_)))
        .count()
}

#[test]
fn a_lost_bind_receipt_is_remembered_and_the_bind_is_never_issued_twice() {
    // `bind` spends the whole budget on the command, because its caller will be asked
    // again; the withheld reply leaves the operation to ask about next time.
    let (mut reporter, supervisor) =
        Supervisor::scripted(AgentProvider::Claude, vec![Answer::Withhold]);
    assert!(!reporter.bind(
        "expected",
        Instant::now() + Duration::from_millis(200),
        false
    ));
    assert!(
        !reporter.closed(),
        "a lost receipt is not a refusal: the reporter stays open to ask again"
    );
    assert!(reporter.bind("expected", deadline(), false));
    assert_eq!(
        reporter.binding().map(|binding| binding.generation),
        Some(1),
        "the recovered receipt is the generation the first Bind created"
    );
    let observed = supervisor.observed();
    assert_eq!(
        binds(&observed),
        1,
        "a second Bind would claim an unseen generation"
    );
    assert_eq!(
        statuses(&observed),
        1,
        "the retry asked for a receipt, not a bind"
    );
}

#[test]
fn a_receiver_with_no_second_chance_re_reads_the_lost_receipt_in_the_same_call() {
    // `bind_or_disable` keeps half the budget back, so a boundary that gets one attempt
    // still recovers a reply lost in transit without waiting for a later callback.
    let (mut reporter, supervisor) =
        Supervisor::scripted(AgentProvider::Pi, vec![Answer::Withhold]);
    assert_eq!(reporter.bind_or_disable("sess-a", deadline(), false), None);
    assert_eq!(
        reporter.binding().map(|binding| binding.generation),
        Some(1)
    );
    let observed = supervisor.observed();
    assert_eq!(binds(&observed), 1);
    assert_eq!(statuses(&observed), 1);

    // When the receipt cannot be read either, there is no later attempt to leave it to.
    let (mut reporter, supervisor) =
        Supervisor::scripted(AgentProvider::Pi, vec![Answer::Withhold, Answer::Withhold]);
    assert_eq!(
        reporter.bind_or_disable("sess-a", Instant::now() + Duration::from_millis(200), false),
        Some(UNAVAILABLE.to_vec())
    );
    assert!(reporter.closed() && reporter.binding().is_none());
    assert_eq!(binds(&supervisor.observed()), 1);
}

#[test]
fn a_refused_bind_closes_the_reporter_without_asking_again() {
    let (mut reporter, supervisor) =
        Supervisor::scripted(AgentProvider::Claude, vec![Answer::Refuse]);
    assert!(!reporter.bind("expected", deadline(), false));
    assert!(
        reporter.closed(),
        "this reservation will not bind this conversation"
    );
    assert!(reporter.binding().is_none());
    assert!(
        !reporter.bind("expected", deadline(), false),
        "a closed reporter never binds"
    );
    assert_eq!(
        supervisor
            .observed()
            .iter()
            .filter(|entry| matches!(entry, Observed::Bind { .. }))
            .count(),
        1
    );
}

#[test]
fn a_publication_that_cannot_be_delivered_disables_the_reporter() {
    let (mut reporter, supervisor) =
        Supervisor::scripted(AgentProvider::Pi, vec![Answer::Auto, Answer::Refuse]);
    assert!(reporter.bind("sess-a", deadline(), false));
    assert_eq!(
        reporter.publish(busy("a:1"), deadline()),
        UNAVAILABLE,
        "delivery uncertainty cannot leave a connected reporter claiming continuity"
    );
    assert!(reporter.closed() && reporter.binding().is_none());
    assert_eq!(
        reporter.publish(busy("a:2"), deadline()),
        UNAVAILABLE,
        "a disabled reporter publishes nothing"
    );
    let observed = supervisor.observed();
    assert_eq!(Supervisor::activity(&observed).len(), 1);
}

#[test]
fn unacknowledged_health_disables_and_an_unbound_reporter_has_none_to_report() {
    let mut reporter = unbound();
    assert!(
        reporter.health(Some("source_gap"), deadline()),
        "a reporter that never bound has no health to report"
    );
    assert!(!reporter.closed());

    let (mut reporter, supervisor) =
        Supervisor::scripted(AgentProvider::Pi, vec![Answer::Auto, Answer::Refuse]);
    assert!(reporter.bind("sess-a", deadline(), false));
    assert!(!reporter.health(Some("source_gap"), deadline()));
    assert!(reporter.closed());
    assert_eq!(
        Supervisor::health(&supervisor.observed()),
        vec![(ReporterHealth::Unavailable, "source_gap".to_owned())]
    );
}

#[test]
fn health_reports_a_reason_once_and_says_when_it_is_healthy_again() {
    let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Claude);
    assert!(reporter.bind("expected", deadline(), false));
    assert!(reporter.health(Some("collector_unavailable"), deadline()));
    assert!(reporter.health(Some("collector_unavailable"), deadline()));
    assert!(reporter.health(None, deadline()));
    assert!(!reporter.paused());
    assert_eq!(
        Supervisor::health(&supervisor.observed()),
        vec![
            (
                ReporterHealth::Unavailable,
                "collector_unavailable".to_owned()
            ),
            (ReporterHealth::Connected, String::new()),
        ],
        "an unchanged reason is not republished"
    );
}

#[test]
fn finalize_settles_a_bound_invocation_and_only_releases_an_unbound_one() {
    let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Claude);
    assert!(reporter.bind("expected", deadline(), false));
    assert_eq!(reporter.publish(busy("turn"), deadline()), ACCEPTED);
    reporter.finalize(Box::new(metrics()), deadline());
    assert!(reporter.closed() && reporter.binding().is_none());
    let observed = supervisor.observed();
    let finalized: Vec<_> = observed
        .iter()
        .filter_map(|entry| match entry {
            Observed::Finalize(report) => Some(report.revision),
            _ => None,
        })
        .collect();
    assert_eq!(
        finalized,
        vec![2],
        "the final accounting is numbered from the same revision set"
    );

    let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Claude);
    reporter.finalize(Box::new(metrics()), deadline());
    let observed = supervisor.observed();
    assert!(observed.contains(&Observed::Release));
    assert!(
        !observed
            .iter()
            .any(|entry| matches!(entry, Observed::Finalize(_))),
        "an invocation that never bound has no accounting to finalize"
    );
}

#[test]
fn a_disabled_reporter_removes_the_directory_it_materialized() {
    let root = tempfile::tempdir().unwrap();
    let scratch = root.path().join("extension");
    std::fs::create_dir(&scratch).unwrap();
    let mut reporter = Reporter::new(AgentProvider::Pi, None, Some(scratch.clone()));
    reporter.disable();
    assert!(!scratch.exists());

    // The accept loop can also be abandoned without a disabling callback.
    std::fs::create_dir(&scratch).unwrap();
    drop(Reporter::new(
        AgentProvider::Pi,
        None,
        Some(scratch.clone()),
    ));
    assert!(!scratch.exists());
}

fn metrics() -> ovrcr_protocol::MetricsSample {
    use ovrcr_protocol::*;
    fn uncertain<T>(value: T) -> Measurement<T> {
        Measurement {
            value,
            source: "test".into(),
        }
    }
    MetricsSample {
        model: None,
        context: uncertain(ContextSample {
            used_tokens: None,
            capacity_tokens: None,
            quality: SampleQuality::Observed,
        }),
        cost: uncertain(None),
        usage: uncertain(UsageTotals {
            scope: UsageScope::Conversation,
            coverage: UsageCoverage::Partial,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_output_tokens: None,
        }),
    }
}
