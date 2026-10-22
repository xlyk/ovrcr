//! Native delivery is separate from the fresh same-turn settlement it may produce.
use anyhow::{Context, Result, bail};
use ovrcr_protocol::{
    AgentActivity, AgentBinding, AgentProvider, AgentSnapshot, AssignmentIdentity, ReporterHealth,
    SampleQuality, validate_agent_id,
};
use serde::{Deserialize, Serialize};

/// Content-free evidence captured under the final retained/paired-Session fence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InterruptBaseline {
    pub server_lifetime: String,
    pub identity: AssignmentIdentity,
    pub result_version: u32,
    pub binding: AgentBinding,
    pub turn: String,
    pub activity_revision: u64,
    pub input_revision: u64,
    pub native_control_revision: u64,
}

impl InterruptBaseline {
    pub(crate) fn capture(
        server_lifetime: &str,
        identity: &AssignmentIdentity,
        result_version: u32,
        agent: &AgentSnapshot,
    ) -> Result<Self> {
        validate_agent_id(server_lifetime)?;
        agent.binding.validate()?;
        if identity.session.is_none() || identity.run.is_none() || result_version == 0 {
            bail!("interrupt requires an allocated assignment and result version");
        }
        if agent.binding.provider != AgentProvider::Pi
            || agent.health.state != ReporterHealth::Connected
            || agent.effective_activity() != AgentActivity::Busy
            || agent.activity_revision == 0
        {
            bail!("native interrupt requires connected busy Pi reporting without open Input");
        }
        let activity = agent.activity.as_ref().context("activity is unavailable")?;
        if activity.quality == SampleQuality::Estimated {
            bail!("estimated activity cannot authorize native interruption");
        }
        let turn = activity.turn.as_ref().context("busy turn is unavailable")?;
        validate_agent_id(turn)?;
        let native = agent
            .native_control
            .as_ref()
            .context("native interrupt capability is unavailable")?;
        if native.turn != *turn
            || native.settled
            || native.interrupt != ovrcr_protocol::NativeInterruptCapability::Escape
            || agent.native_control_revision == 0
        {
            bail!("current native interrupt binding is unsupported or unavailable");
        }
        Ok(Self {
            server_lifetime: server_lifetime.to_owned(),
            identity: identity.clone(),
            result_version,
            binding: agent.binding.clone(),
            turn: turn.clone(),
            activity_revision: agent.activity_revision,
            input_revision: agent.input_revision,
            native_control_revision: agent.native_control_revision,
        })
    }

    /// Called only after current reconciliation evidence and the exact control
    /// barrier are checked. This never changes a delivery receipt or other fence.
    pub(crate) fn confirm_settlement(
        &self,
        server_lifetime: &str,
        identity: &AssignmentIdentity,
        result_version: u32,
        agent: &AgentSnapshot,
    ) -> Result<()> {
        if self.server_lifetime != server_lifetime
            || &self.identity != identity
            || self.result_version != result_version
            || self.binding != agent.binding
            || agent.health.state != ReporterHealth::Connected
        {
            bail!("interrupt confirmation identity or reporting lifetime changed");
        }
        let activity = agent.activity.as_ref().context("activity is unavailable")?;
        if activity.turn.as_ref() != Some(&self.turn)
            || agent.activity_revision <= self.activity_revision
            || activity.quality == SampleQuality::Estimated
            || !agent.input_requests.is_empty()
            || !matches!(
                activity.state,
                AgentActivity::Idle | AgentActivity::Error | AgentActivity::ResponseReady
            )
        {
            bail!(
                "interrupt confirmation requires a fresh same-turn settled observation without open Input"
            );
        }
        if agent
            .native_control
            .as_ref()
            .is_none_or(|sample| sample.turn != self.turn || !sample.settled)
            || agent.native_control_revision <= self.native_control_revision
        {
            bail!("interrupt confirmation requires explicit fresh Pi agent_settled evidence");
        }
        Ok(())
    }
}
