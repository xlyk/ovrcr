//! Native-root durable receipts translate exact cloud runs through the shared Reporter.
use super::{
    InvocationLease,
    reporter::{self, Frames, Reporter},
};
use ovrcr_protocol::AgentProvider;
use ovrcr_runtime::agent_runner::HookHandler;
use std::time::Instant;

pub fn receiver(lease: InvocationLease) -> HookHandler {
    Reporter::new(AgentProvider::CursorCloud, Some(lease), None).handler(Preview::default())
}
#[derive(Default)]
struct Preview {
    active: Option<(String, String, String)>,
}
impl Preview {
    fn committed(
        &mut self,
        reporter: &mut Reporter,
        command: &ovrcr_protocol::CloudCreationCommand,
        deadline: Instant,
    ) -> Vec<u8> {
        use ovrcr_protocol::{
            ActivitySample, AgentActivity as A, AgentObservation, CloudCreationCommand as C,
            SampleQuality,
        };
        let (agent, run, key, state) = match command {
            C::ObserveSend {
                key,
                agent_id,
                run_id,
            } => {
                let identity = format!("send:{run_id}");
                if reporter.published(&identity) {
                    return reporter::ACCEPTED.to_vec();
                }
                if self.active.is_some() {
                    return reporter::UNAVAILABLE.to_vec();
                }
                if reporter
                    .binding()
                    .is_some_and(|b| b.conversation != *agent_id)
                    || !reporter.bind(agent_id, deadline, false)
                {
                    return reporter::UNAVAILABLE.to_vec();
                }
                if let Some(answer) = reporter.admit_cycle(&identity) {
                    return answer;
                }
                self.active = Some((agent_id.clone(), run_id.clone(), key.clone()));
                (agent_id, run_id, key, A::Busy)
            }
            C::CompleteSend {
                key,
                agent_id,
                run_id,
                status,
            } => {
                if self.active.as_ref() != Some(&(agent_id.clone(), run_id.clone(), key.clone())) {
                    return reporter::IGNORED.to_vec();
                }
                if let Some(answer) = reporter.admit_cycle(&format!("terminal:{run_id}")) {
                    return answer;
                }
                self.active = None;
                (
                    agent_id,
                    run_id,
                    key,
                    if *status == ovrcr_protocol::CloudRunStatus::Finished {
                        A::ResponseReady
                    } else {
                        A::Error
                    },
                )
            }
            C::ObservationLost {
                key,
                agent_id,
                run_id,
            } => {
                if self.active.as_ref() != Some(&(agent_id.clone(), run_id.clone(), key.clone())) {
                    return reporter::IGNORED.to_vec();
                }
                (agent_id, run_id, key, A::Unknown)
            }
            _ => return reporter::ACCEPTED.to_vec(),
        };
        let _ = (agent, key);
        reporter.publish(
            AgentObservation::Activity(ActivitySample {
                state,
                quality: SampleQuality::Observed,
                turn: Some(run.clone()),
            }),
            deadline,
        )
    }
}
impl Frames for Preview {
    fn frame(
        &mut self,
        reporter: &mut Reporter,
        input: &[u8],
        native_root: bool,
        deadline: Instant,
    ) -> Vec<u8> {
        let envelope = serde_json::from_slice::<serde_json::Value>(input).ok();
        if native_root
            && envelope
                .as_ref()
                .is_some_and(|v| v["provider"] == "cursor-cloud" && v["origin"] == "creation")
        {
            if let Some(command) = envelope.and_then(|v| {
                serde_json::from_value::<ovrcr_protocol::CloudCreationCommand>(v["payload"].clone())
                    .ok()
            }) && reporter.cloud_creation(command.clone(), deadline)
            {
                return self.committed(reporter, &command, deadline);
            }
            return reporter::UNAVAILABLE.to_vec();
        }
        if !reporter.closed() && native_root && serde_json::from_slice::<serde_json::Value>(input).ok().is_some_and(|value| value == serde_json::json!({"provider":"cursor-cloud","origin":"preview-admission","payload":{}})) {
            reporter::ACCEPTED.to_vec()
        } else { b"admission-unavailable\n".to_vec() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::reporter::scripted::Supervisor;
    use ovrcr_protocol::{CloudCreationCommand, CloudRunStatus};
    use serde_json::json;
    use std::time::Duration;
    fn frame(command: CloudCreationCommand) -> Vec<u8> {
        serde_json::to_vec(
            &json!({"provider":"cursor-cloud","origin":"creation","payload":command}),
        )
        .unwrap()
    }
    #[test]
    fn exact_run_receipt_reports_busy_then_ready_once_and_rejects_stale_run() {
        let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::CursorCloud);
        let mut receiver = Preview::default();
        let deadline = Instant::now() + Duration::from_secs(3);
        let ack = frame(CloudCreationCommand::ObserveSend {
            key: "d".repeat(64),
            agent_id: "agent-374".into(),
            run_id: "run-1".into(),
        });
        assert_eq!(
            receiver.frame(&mut reporter, &ack, false, deadline),
            reporter::UNAVAILABLE
        );
        assert_eq!(
            receiver.frame(&mut reporter, &ack, true, deadline),
            reporter::ACCEPTED
        );
        let finish = frame(CloudCreationCommand::CompleteSend {
            key: "d".repeat(64),
            agent_id: "agent-374".into(),
            run_id: "run-1".into(),
            status: CloudRunStatus::Finished,
        });
        receiver.frame(&mut reporter, &finish, true, deadline);
        receiver.frame(&mut reporter, &finish, true, deadline);
        let foreign = frame(CloudCreationCommand::CompleteSend {
            key: "d".repeat(64),
            agent_id: "agent-374".into(),
            run_id: "foreign".into(),
            status: CloudRunStatus::Finished,
        });
        receiver.frame(&mut reporter, &foreign, true, deadline);
        reporter.disable();
        let samples = Supervisor::activity(&supervisor.observed());
        assert_eq!(samples.len(), 2, "one Busy and one Ready for the exact run");
        assert_eq!(samples[0].2, ovrcr_protocol::AgentActivity::Busy);
        assert_eq!(samples[1].2, ovrcr_protocol::AgentActivity::ResponseReady);
    }
}
