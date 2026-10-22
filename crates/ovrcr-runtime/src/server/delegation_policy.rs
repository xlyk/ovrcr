//! Delegation choice and admission policy. Inventory is discovered outside the
//! lifecycle lock; admission rechecks these bounds under that existing lock.
use ovrcr_protocol::{
    DelegationCapabilities, DelegationChoice, DelegationLimits, DelegationSelection,
    DelegationSettings,
};

pub(super) fn resolve(
    policy: &DelegationSettings,
    selection: &DelegationSelection,
    inventory: Vec<DelegationChoice>,
    live_sessions: u16,
    live_children: u16,
    unavailable: Option<String>,
) -> DelegationCapabilities {
    let requested = &selection.required_limits;
    let effective_limits = DelegationLimits {
        max_live_children: Some(
            requested
                .max_live_children
                .unwrap_or(50)
                .min(policy.max_live_children),
        ),
        max_execution_attempts: match (
            requested.max_execution_attempts,
            policy.max_execution_attempts,
        ) {
            (Some(request), Some(configured)) => Some(request.min(configured)),
            (request, configured) => request.or(configured),
        },
        ..requested.clone()
    };
    let mut choices: Vec<_> = inventory
        .into_iter()
        .filter(|choice| {
            policy.allowed_harnesses.contains(&choice.provider)
                && (policy.allowed_models.is_empty()
                    || policy
                        .allowed_models
                        .contains(&format!("{}/{}", choice.model_provider, choice.model)))
        })
        .collect();
    choices.sort_by(|a, b| {
        (a.provider.name(), &a.model_provider, &a.model).cmp(&(
            b.provider.name(),
            &b.model_provider,
            &b.model,
        ))
    });
    choices.dedup();
    let harness = selection.provider.unwrap_or(policy.default_harness);
    let capabilities = if harness == ovrcr_protocol::DelegationHarness::Codex {
        vec!["read", "result_file"]
    } else {
        vec![
            "read",
            "result_file",
            "isolated_workspace",
            "immutable_candidate",
            "write",
            "edit",
            "check",
        ]
    };
    let mut response = DelegationCapabilities {
        choices,
        resolved_choice: None,
        effective_limits,
        live_sessions,
        live_children,
        available_slots: 0,
        capabilities: capabilities.into_iter().map(str::to_owned).collect(),
        enforceable_limits: vec![
            "max_live_children".into(),
            "max_execution_attempts".into(),
            "min_free_slots_after_launch".into(),
        ],
        native_helpers: false,
        native_descendant_usage: false,
        reason: None,
    };
    if let Some(reason) = unavailable {
        response.reason = Some(reason);
        return response;
    }
    if !policy.enabled {
        response.reason = Some("delegation is disabled by Server settings".into());
        return response;
    }
    if let Err(reason) = requested.validate() {
        response.reason = Some(reason.into());
        return response;
    }
    let mut candidates: Vec<_> = response
        .choices
        .iter()
        .filter(|choice| choice.provider == harness)
        .collect();
    if selection.model_provider.is_some() || selection.model.is_some() {
        candidates.retain(|choice| {
            selection
                .model_provider
                .as_ref()
                .is_none_or(|backend| backend == &choice.model_provider)
                && selection
                    .model
                    .as_ref()
                    .is_none_or(|model| model == &choice.model)
        });
    } else if let Some(default) = &policy.default_model {
        candidates
            .retain(|choice| format!("{}/{}", choice.model_provider, choice.model) == *default);
    }
    match candidates.as_slice() {
        [choice] => response.resolved_choice = Some((*choice).clone()),
        [] => response.reason = Some("requested or default harness/model is unavailable or disallowed; no substitute selected".into()),
        _ => response.reason = Some("select one of the advertised allowed backend/model choices".into()),
    }
    response.available_slots = 50u16
        .saturating_sub(live_sessions)
        .saturating_sub(response.effective_limits.min_free_slots_after_launch)
        .min(
            response
                .effective_limits
                .max_live_children
                .unwrap_or(50)
                .saturating_sub(live_children),
        );
    response
}

impl super::ServerState {
    pub(super) fn policy_snapshot(&self) -> (DelegationSettings, Option<String>) {
        let watched = self.settings.lock().unwrap();
        let report = &watched.report;
        let invalid = report.unparseable
            || report.findings.iter().any(|finding| {
                finding
                    .key
                    .as_deref()
                    .is_none_or(|key| key == "delegation" || key.starts_with("delegation."))
            });
        (report.settings.delegation.clone(), invalid.then(|| "invalid delegation settings; correct the Server settings findings before dispatch".into()))
    }

    pub(super) fn delegation_live_counts(&self) -> anyhow::Result<(u16, u16)> {
        let ids: Vec<_> = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|session| session.is_live())
            .map(|session| session.id())
            .collect();
        let retained = self.retained.lock();
        let mut children = 0;
        for id in &ids {
            if retained.assigned(*id)? {
                children += 1;
            }
        }
        Ok((ids.len() as u16, children))
    }

    pub(super) fn contract_policy(
        &self,
        contract: &ovrcr_protocol::ResearchContract,
        available: bool,
    ) -> anyhow::Result<DelegationCapabilities> {
        let (settings, invalid) = self.policy_snapshot();
        let choice = DelegationChoice {
            provider: ovrcr_protocol::DelegationHarness::parse(&contract.provider).ok_or_else(
                || {
                    super::lifecycle_error(
                        ovrcr_protocol::ErrorCode::InvalidRequest,
                        "unsupported delegation harness",
                    )
                },
            )?,
            model_provider: contract.model_provider.clone(),
            model: contract.model.clone(),
        };
        let (live, children) = self.delegation_live_counts()?;
        let selected = DelegationSelection {
            provider: Some(choice.provider),
            model_provider: Some(choice.model_provider.clone()),
            model: Some(choice.model.clone()),
            required_limits: contract.limits.clone(),
        };
        Ok(resolve(
            &settings,
            &selected,
            if available { vec![choice] } else { Vec::new() },
            live,
            children,
            invalid,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::DelegationHarness;

    fn choice(backend: &str, model: &str) -> DelegationChoice {
        DelegationChoice {
            provider: DelegationHarness::Pi,
            model_provider: backend.into(),
            model: model.into(),
        }
    }

    #[test]
    fn explicit_choice_does_not_fall_back_and_no_default_requires_a_parent_choice() {
        let inventory = vec![choice("test", "a"), choice("test", "b")];
        let policy = DelegationSettings::default();
        let report = resolve(&policy, &Default::default(), inventory.clone(), 1, 0, None);
        assert!(report.resolved_choice.is_none());
        assert_eq!(report.choices.len(), 2);
        let selected = DelegationSelection {
            model: Some("b".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve(&policy, &selected, inventory.clone(), 1, 0, None).resolved_choice,
            Some(choice("test", "b"))
        );
        let policy = DelegationSettings {
            default_model: Some("test/a".into()),
            ..policy
        };
        let missing = DelegationSelection {
            model: Some("missing".into()),
            ..Default::default()
        };
        assert!(
            resolve(&policy, &missing, inventory.clone(), 1, 0, None)
                .resolved_choice
                .is_none()
        );
        assert_eq!(
            resolve(&policy, &Default::default(), inventory, 1, 0, None).resolved_choice,
            Some(choice("test", "a"))
        );
    }

    #[test]
    fn allowlists_and_current_policy_cannot_be_loosened_by_assignment_limits() {
        let policy = DelegationSettings {
            allowed_models: vec!["test/a".into()],
            max_live_children: 6,
            max_execution_attempts: Some(2),
            ..Default::default()
        };
        let selection = DelegationSelection {
            required_limits: DelegationLimits {
                max_live_children: Some(30),
                max_execution_attempts: Some(10),
                min_free_slots_after_launch: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        let report = resolve(
            &policy,
            &selection,
            vec![choice("test", "a"), choice("test", "b")],
            47,
            5,
            None,
        );
        assert_eq!(report.choices, [choice("test", "a")]);
        assert_eq!(report.resolved_choice, Some(choice("test", "a")));
        assert_eq!(report.effective_limits.max_live_children, Some(6));
        assert_eq!(report.effective_limits.max_execution_attempts, Some(2));
        assert_eq!(report.available_slots, 1);
        assert!(!report.native_helpers && !report.native_descendant_usage);
        assert_eq!(
            resolve(&policy, &selection, vec![choice("test", "a")], 48, 5, None).available_slots,
            0
        );
        assert_eq!(
            resolve(&policy, &selection, vec![choice("test", "a")], 1, 6, None).available_slots,
            0
        );
    }

    #[test]
    fn malformed_disabled_or_unenforceable_policy_never_resolves_a_launch() {
        let policy = DelegationSettings::default();
        let selection = DelegationSelection {
            required_limits: DelegationLimits {
                max_cost_microusd: Some(10),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(
            resolve(&policy, &selection, vec![choice("test", "a")], 1, 0, None)
                .resolved_choice
                .is_none()
        );
        assert!(
            resolve(
                &policy,
                &Default::default(),
                vec![choice("test", "a")],
                1,
                0,
                Some("invalid delegation settings".into())
            )
            .resolved_choice
            .is_none()
        );
        let policy = DelegationSettings {
            enabled: false,
            ..policy
        };
        let report = resolve(
            &policy,
            &Default::default(),
            vec![choice("test", "a")],
            1,
            0,
            None,
        );
        assert!(report.resolved_choice.is_none());
        assert_eq!(report.available_slots, 0);
    }

    #[test]
    fn codex_advertises_only_read_only_effects_while_pi_keeps_coding_capabilities() {
        let policy = DelegationSettings {
            allowed_harnesses: vec![DelegationHarness::Pi, DelegationHarness::Codex],
            ..Default::default()
        };
        for provider in [DelegationHarness::Pi, DelegationHarness::Codex] {
            let selection = DelegationSelection {
                provider: Some(provider),
                model_provider: Some("openai".into()),
                model: Some("exact-model".into()),
                ..Default::default()
            };
            let native = DelegationChoice {
                provider,
                model_provider: "openai".into(),
                model: "exact-model".into(),
            };
            for unavailable in [false, true] {
                let report = resolve(
                    &policy,
                    &selection,
                    if unavailable {
                        vec![]
                    } else {
                        vec![native.clone()]
                    },
                    1,
                    0,
                    unavailable.then(|| "unavailable native policy".into()),
                );
                if provider == DelegationHarness::Codex {
                    assert_eq!(report.capabilities, ["read", "result_file"]);
                } else {
                    for capability in [
                        "read",
                        "result_file",
                        "isolated_workspace",
                        "immutable_candidate",
                        "write",
                        "edit",
                        "check",
                    ] {
                        assert!(report.capabilities.iter().any(|value| value == capability));
                    }
                }
                assert!(!report.native_helpers && !report.native_descendant_usage);
                assert_eq!(report.resolved_choice.is_none(), unavailable);
            }
        }
    }
}
