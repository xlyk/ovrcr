//! Server-lifetime totals from accepted cumulative session reports.
use ovrcr_protocol::{AgentBinding, UsageScope, UsageSnapshot, UsageTotals};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Component {
    baseline: Option<u64>,
    total: u64,
}

impl Component {
    fn record(&mut self, next: Option<u64>) {
        let Some(next) = next else {
            return;
        };
        let delta = self.baseline.map_or(next, |previous| {
            if next >= previous {
                next - previous
            } else {
                next
            }
        });
        self.total = self.total.saturating_add(delta);
        self.baseline = Some(next);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counters {
    input: Component,
    output: Component,
}

impl Counters {
    fn record(&mut self, usage: &UsageTotals) {
        self.input.record(usage.input_tokens);
        self.output.record(usage.output_tokens);
    }

    fn total(self) -> Option<u64> {
        (self.input.baseline.is_some() || self.output.baseline.is_some())
            .then(|| self.input.total.saturating_add(self.output.total))
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct ConversationKey {
    session: u64,
    provider: &'static str,
    conversation: String,
}

#[derive(Default)]
struct ConversationUsage {
    conversation: Counters,
    invocations: HashMap<String, Counters>,
    contribution: u64,
}

pub(super) struct Lifetime {
    started_unix_ms: u64,
    total_tokens: u64,
    conversations: HashMap<ConversationKey, ConversationUsage>,
}

impl Lifetime {
    pub(super) fn new(started_unix_ms: u64) -> Self {
        Self {
            started_unix_ms,
            total_tokens: 0,
            conversations: HashMap::new(),
        }
    }

    pub(super) fn snapshot(&self) -> UsageSnapshot {
        UsageSnapshot {
            total_tokens: self.total_tokens,
            server_started_unix_ms: self.started_unix_ms,
        }
    }

    /// Add only increases in the larger known conversation or invocation total.
    /// Invocation reports are scoped to their conversation so a provider's
    /// cumulative conversation snapshot cannot count the same tokens twice.
    pub(super) fn record(
        &mut self,
        session: u64,
        binding: &AgentBinding,
        usage: &UsageTotals,
    ) -> Option<UsageSnapshot> {
        let before = self.total_tokens;
        let conversation = self
            .conversations
            .entry(ConversationKey {
                session,
                provider: binding.provider.name(),
                conversation: binding.conversation.clone(),
            })
            .or_default();
        match usage.scope {
            UsageScope::Conversation => conversation.conversation.record(usage),
            UsageScope::Invocation => conversation
                .invocations
                .entry(binding.invocation.clone())
                .or_default()
                .record(usage),
        }
        let invocation_total = conversation
            .invocations
            .values()
            .filter_map(|counters| counters.total())
            .fold(0_u64, u64::saturating_add);
        let reported_total = conversation
            .conversation
            .total()
            .into_iter()
            .chain((!conversation.invocations.is_empty()).then_some(invocation_total))
            .max()
            .unwrap_or(0);
        let increase = reported_total.saturating_sub(conversation.contribution);
        conversation.contribution = conversation.contribution.max(reported_total);
        self.total_tokens = self.total_tokens.saturating_add(increase);
        (self.total_tokens != before).then(|| self.snapshot())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{AgentProvider, UsageCoverage};

    fn binding(conversation: &str, invocation: &str) -> AgentBinding {
        AgentBinding {
            provider: AgentProvider::Codex,
            invocation: invocation.into(),
            conversation: conversation.into(),
            generation: 1,
        }
    }

    fn totals(input: Option<u64>, output: Option<u64>) -> UsageTotals {
        totals_for(UsageScope::Conversation, input, output)
    }

    fn totals_for(scope: UsageScope, input: Option<u64>, output: Option<u64>) -> UsageTotals {
        UsageTotals {
            scope,
            coverage: UsageCoverage::Complete,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: Some(500),
            cache_write_tokens: Some(600),
            reasoning_output_tokens: Some(700),
        }
    }

    #[test]
    fn accepted_cumulative_reports_add_only_input_and_output_deltas_per_session() {
        let mut lifetime = Lifetime::new(42);
        let first = binding("conversation-a", "invocation-a");
        let second = binding("conversation-b", "invocation-b");

        assert_eq!(
            lifetime
                .record(1, &first, &totals(Some(100), Some(20)))
                .unwrap()
                .total_tokens,
            120
        );
        assert_eq!(
            lifetime
                .record(1, &first, &totals(Some(150), Some(25)))
                .unwrap()
                .total_tokens,
            175
        );
        assert_eq!(
            lifetime
                .record(2, &second, &totals(Some(2), Some(3)))
                .unwrap()
                .total_tokens,
            180
        );
        assert!(
            lifetime
                .record(1, &first, &totals(Some(150), Some(25)))
                .is_none()
        );
        assert_eq!(lifetime.snapshot().server_started_unix_ms, 42);
    }

    #[test]
    fn total_saturates_without_overflow() {
        let mut lifetime = Lifetime::new(1);
        let binding = binding("conversation", "invocation");
        lifetime.record(1, &binding, &totals(Some(u64::MAX), Some(1)));
        assert_eq!(lifetime.snapshot().total_tokens, u64::MAX);
    }

    #[test]
    fn overlapping_conversation_and_invocation_reports_count_the_larger_contribution() {
        let mut lifetime = Lifetime::new(1);
        let binding = binding("conversation", "invocation");
        assert_eq!(
            lifetime
                .record(1, &binding, &totals(Some(100), None))
                .unwrap()
                .total_tokens,
            100
        );
        assert!(
            lifetime
                .record(
                    1,
                    &binding,
                    &totals_for(UsageScope::Invocation, Some(10), None),
                )
                .is_none(),
            "the invocation total is a subset of the conversation total"
        );
        assert_eq!(
            lifetime
                .record(1, &binding, &totals(Some(110), None))
                .unwrap()
                .total_tokens,
            110
        );
    }

    #[test]
    fn invocation_first_reports_sum_distinct_invocations_without_double_counting_snapshots() {
        let mut lifetime = Lifetime::new(1);
        let first = binding("conversation", "invocation-a");
        let second = binding("conversation", "invocation-b");

        assert_eq!(
            lifetime
                .record(
                    7,
                    &first,
                    &totals_for(UsageScope::Invocation, Some(30), None),
                )
                .unwrap()
                .total_tokens,
            30
        );
        assert_eq!(
            lifetime
                .record(
                    7,
                    &second,
                    &totals_for(UsageScope::Invocation, Some(20), None),
                )
                .unwrap()
                .total_tokens,
            50
        );
        assert!(
            lifetime
                .record(7, &first, &totals(Some(40), None))
                .is_none(),
            "conversation snapshots do not replace the larger invocation sum"
        );
        assert_eq!(
            lifetime
                .record(7, &first, &totals(Some(80), None))
                .unwrap()
                .total_tokens,
            80
        );
        assert!(
            lifetime
                .record(
                    7,
                    &first,
                    &totals_for(UsageScope::Invocation, Some(25), None),
                )
                .is_none(),
            "a reset lower than the known conversation contribution cannot lower the total"
        );
        assert!(
            lifetime
                .record(
                    7,
                    &first,
                    &totals_for(UsageScope::Invocation, Some(25), None),
                )
                .is_none(),
            "repeated invocation reports preserve the counter baseline"
        );
        assert_eq!(
            lifetime
                .record(7, &first, &totals(Some(90), None))
                .unwrap()
                .total_tokens,
            90
        );
    }

    #[test]
    fn unknown_components_keep_baselines_and_subsets_are_not_counted() {
        let mut lifetime = Lifetime::new(1);
        let binding = binding("conversation", "invocation");
        assert_eq!(
            lifetime
                .record(1, &binding, &totals(Some(10), None))
                .unwrap()
                .total_tokens,
            10
        );
        assert_eq!(
            lifetime
                .record(1, &binding, &totals(None, Some(3)))
                .unwrap()
                .total_tokens,
            13
        );
        assert!(
            lifetime.record(1, &binding, &totals(None, None)).is_none(),
            "unknown values do not erase prior component totals"
        );
        assert_eq!(
            lifetime
                .record(1, &binding, &totals(Some(10), Some(5)))
                .unwrap()
                .total_tokens,
            15
        );
        assert!(
            lifetime
                .record(1, &binding, &totals(Some(10), Some(5)))
                .is_none(),
            "repeated cumulative reports do not add again"
        );
    }

    #[test]
    fn provider_namespaces_conversation_identities() {
        let mut lifetime = Lifetime::new(1);
        let codex = binding("shared-conversation", "same-invocation");
        let mut claude = binding("shared-conversation", "same-invocation");
        claude.provider = AgentProvider::Claude;
        assert_eq!(
            lifetime
                .record(1, &codex, &totals(Some(10), None))
                .unwrap()
                .total_tokens,
            10
        );
        assert_eq!(
            lifetime
                .record(1, &claude, &totals(Some(7), None))
                .unwrap()
                .total_tokens,
            17
        );
        assert_eq!(
            lifetime
                .record(1, &codex, &totals(Some(11), None))
                .unwrap()
                .total_tokens,
            18,
            "a same-named conversation from another provider keeps a separate baseline"
        );
    }
}
