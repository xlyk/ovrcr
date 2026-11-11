//! Server-lifetime totals from accepted cumulative session reports.
use ovrcr_protocol::{AgentBinding, UsageScope, UsageSnapshot, UsageTotals};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counters {
    input: Option<u64>,
    output: Option<u64>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Scope {
    Conversation,
    Invocation,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct Key {
    session: u64,
    scope: Scope,
    identity: String,
}

pub(super) struct Lifetime {
    started_unix_ms: u64,
    total_tokens: u64,
    counters: HashMap<Key, Counters>,
}

impl Lifetime {
    pub(super) fn new(started_unix_ms: u64) -> Self {
        Self {
            started_unix_ms,
            total_tokens: 0,
            counters: HashMap::new(),
        }
    }

    pub(super) fn snapshot(&self) -> UsageSnapshot {
        UsageSnapshot {
            total_tokens: self.total_tokens,
            server_started_unix_ms: self.started_unix_ms,
        }
    }

    /// Add only the accepted increase for this session and cumulative scope.
    /// A new source may begin a fresh counter at a lower value; this starts a
    /// new contribution rather than subtracting from the lifetime total.
    pub(super) fn record(
        &mut self,
        session: u64,
        binding: &AgentBinding,
        usage: &UsageTotals,
    ) -> Option<UsageSnapshot> {
        let before = self.total_tokens;
        let (scope, identity) = match usage.scope {
            UsageScope::Conversation => (Scope::Conversation, binding.conversation.clone()),
            UsageScope::Invocation => (Scope::Invocation, binding.invocation.clone()),
        };
        let counters = self.counters.entry(Key {
            session,
            scope,
            identity,
        });
        let previous = counters.or_default();
        let delta = delta(&mut previous.input, usage.input_tokens)
            .saturating_add(delta(&mut previous.output, usage.output_tokens));
        self.total_tokens = self.total_tokens.saturating_add(delta);
        (self.total_tokens != before).then(|| self.snapshot())
    }
}

fn delta(previous: &mut Option<u64>, next: Option<u64>) -> u64 {
    let Some(next) = next else {
        return 0;
    };
    let increase = previous.map_or(next, |previous| {
        if next >= previous {
            next - previous
        } else {
            next
        }
    });
    *previous = Some(next);
    increase
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
        UsageTotals {
            scope: UsageScope::Conversation,
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
    fn scope_identity_and_unknown_components_do_not_share_or_erase_baselines() {
        let mut lifetime = Lifetime::new(1);
        let binding = binding("conversation", "invocation");
        lifetime.record(1, &binding, &totals(Some(10), Some(3)));
        lifetime.record(1, &binding, &totals(None, Some(5)));
        let mut invocation = totals(Some(2), Some(1));
        invocation.scope = UsageScope::Invocation;
        assert_eq!(
            lifetime
                .record(1, &binding, &invocation)
                .unwrap()
                .total_tokens,
            21
        );
    }
}
