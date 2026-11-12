//! Install and script the deterministic fake `pi` used by Operator tests.
//!
//! The fixture executable is a Node script (`tests/fixtures/operator-pi`)
//! wrapped by a generated `/bin/sh` launcher that names an absolute `node`.
//! It speaks pi 0.87.1's RPC framing and every command/event the worker drives
//! and records each observable step in a JSONL receipt.
//!
//! Two ways to script it:
//!
//! * worker-driven suites write the scenario into the instance's private
//!   `operator-pi/fake-pi-scenario.json`, where the spawned worker's Pi process
//!   reads it; rewriting it changes the next turn's profile defaults.
//! * direct RPC suites pass `FAKE_PI_SCENARIO` to the process they spawn.

#![allow(dead_code)]

use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The scenario document the fake pi executes.
#[derive(Clone, Debug)]
pub struct Scenario {
    value: Value,
}

impl Default for Scenario {
    fn default() -> Self {
        Self {
            value: json!({
                "startup": "ok",
                "contract": "ok",
                "switch": "ok",
                "prompt_response": "ok",
                "models": [
                    {"provider": "fixture", "id": "default", "name": "Fixture Default"},
                    {"provider": "fixture", "id": "alternate", "name": "Fixture Alternate"}
                ],
                "default_model": {"provider": "fixture", "id": "default", "name": "Fixture Default"},
                "thinking_level": "medium",
                "steps": []
            }),
        }
    }
}

impl Scenario {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: &str, value: impl Into<Value>) -> &mut Self {
        self.value[key] = value.into();
        self
    }

    pub fn startup(&mut self, mode: &str) -> &mut Self {
        self.set("startup", json!(mode))
    }

    pub fn contract(&mut self, mode: &str) -> &mut Self {
        self.set("contract", json!(mode))
    }

    pub fn switch(&mut self, mode: &str) -> &mut Self {
        self.set("switch", json!(mode))
    }

    pub fn prompt_response(&mut self, mode: &str) -> &mut Self {
        self.set("prompt_response", json!(mode))
    }

    pub fn default_model(&mut self, provider: &str, id: &str) -> &mut Self {
        self.set(
            "default_model",
            json!({"provider": provider, "id": id, "name": id}),
        )
    }

    pub fn no_default_model(&mut self) -> &mut Self {
        self.set("default_model", Value::Null)
    }

    pub fn models(&mut self, models: Vec<(&str, &str, &str)>) -> &mut Self {
        let models: Vec<Value> = models
            .into_iter()
            .map(|(provider, id, name)| json!({"provider": provider, "id": id, "name": name}))
            .collect();
        self.set("models", Value::Array(models))
    }

    pub fn startup_stderr(&mut self, text: &str) -> &mut Self {
        self.set("stderr_startup", json!(text))
    }

    /// Raw bytes emitted before the first command. Used for framing cases.
    pub fn startup_records(&mut self, records: Vec<String>) -> &mut Self {
        self.set(
            "startup_records",
            Value::Array(records.into_iter().map(Value::String).collect()),
        )
    }

    pub fn steps(&mut self, steps: Vec<Step>) -> &mut Self {
        self.set(
            "steps",
            Value::Array(steps.into_iter().map(Step::into_value).collect()),
        )
    }

    /// A separate scripted step list for each prompt accepted by one warm Pi
    /// process. Any later prompt uses the final list, matching a stable script.
    pub fn prompt_steps(&mut self, prompts: Vec<Vec<Step>>) -> &mut Self {
        self.set(
            "prompt_steps",
            Value::Array(
                prompts
                    .into_iter()
                    .map(|steps| Value::Array(steps.into_iter().map(Step::into_value).collect()))
                    .collect(),
            ),
        )
    }

    pub fn exit_after_steps(&mut self) -> &mut Self {
        self.set("exit_after_steps", Value::Bool(true))
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn install_at(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(path, serde_json::to_vec_pretty(&self.value).unwrap()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

/// One scripted step after `prompt` acceptance.
#[derive(Clone, Debug)]
pub enum Step {
    /// Start one assistant message.
    Start,
    /// Append one streamed text delta.
    Delta(String),
    /// Append one streamed text delta at an explicit content index.
    TextDelta {
        index: usize,
        text: String,
    },
    /// Start one thinking content block.
    ThinkingStart {
        index: usize,
    },
    /// Append one streamed thinking delta at an explicit content index.
    ThinkingDelta {
        index: usize,
        text: String,
    },
    /// Authoritative content ending for one thinking block.
    ThinkingEnd {
        index: usize,
        content: String,
    },
    /// The final assistant message carries thinking and answer blocks; it does
    /// not itself emit deltas.
    ThinkingMessageEnd {
        thinking: String,
        text: String,
        stop: String,
    },
    /// Park until abort or stdin close (an in-flight turn for Stop tests).
    Hold,
    /// Authoritative assistant message delivered without streaming.
    Assistant {
        text: String,
        stop: String,
    },
    /// A streamed prefix followed by an authoritative message with more text.
    Partial {
        streamed: String,
        text: String,
        stop: String,
    },
    /// An authoritative message that contradicts an already streamed prefix.
    Contradiction {
        streamed: String,
        text: String,
    },
    /// One admitted tool call with its result dialog.
    Tool {
        id: String,
        name: String,
        input: Value,
        output: String,
        status: &'static str,
        truncated: bool,
        marker: Option<PathBuf>,
    },
    /// A second admission offered before the first result is settled.
    SecondAdmission {
        id: String,
        name: String,
        input: Value,
        output: String,
        second: String,
        second_name: String,
        second_input: Value,
        second_marker: Option<PathBuf>,
    },
    /// One assistant message with two tool calls executed in order.
    TwoCallBatch {
        id: String,
        name: String,
        input: Value,
        output: String,
        second: String,
        second_name: String,
        second_input: Value,
        second_output: String,
    },
    /// A result for a call that was never admitted.
    UnknownResult {
        id: String,
        output: String,
        marker: Option<PathBuf>,
    },
    /// An execution that ends without the admission dialog, as Pi does for an
    /// unknown tool or invalid arguments.
    UnadmittedExecution {
        id: String,
        name: String,
        input: Value,
        output: String,
    },
    /// Receipt for a further model request (must never follow a timeout).
    ModelRequest,
    Event(Value),
    /// Emit the watch extension's status snapshot.
    WatchStatus(usize),
    /// Simulate the extension's admitted idle `sendUserMessage` and its one
    /// native Pi turn, without issuing a second worker prompt command.
    AsyncWakeup {
        text: String,
        answer: String,
    },
    Stderr(String),
    Sleep(u64),
    WaitFile(PathBuf),
    Settle,
    Exit(i32),
}

impl Step {
    pub fn assistant(text: &str) -> Self {
        Self::Assistant {
            text: text.into(),
            stop: "stop".into(),
        }
    }

    pub fn thinking_delta(index: usize, text: &str) -> Self {
        Self::ThinkingDelta {
            index,
            text: text.into(),
        }
    }

    pub fn tool(id: &str, name: &str, input: Value, output: &str) -> Self {
        Self::Tool {
            id: id.into(),
            name: name.into(),
            input,
            output: output.into(),
            status: "succeeded",
            truncated: false,
            marker: None,
        }
    }

    pub fn async_wakeup(text: &str, answer: &str) -> Self {
        Self::AsyncWakeup {
            text: text.into(),
            answer: answer.into(),
        }
    }

    fn into_value(self) -> Value {
        match self {
            Self::Start => json!({"kind": "start"}),
            Self::Delta(text) => json!({"kind": "delta", "text": text}),
            Self::TextDelta { index, text } => {
                json!({"kind": "text_delta", "index": index, "text": text})
            }
            Self::ThinkingStart { index } => {
                json!({"kind": "thinking_start", "index": index})
            }
            Self::ThinkingDelta { index, text } => {
                json!({"kind": "thinking_delta", "index": index, "text": text})
            }
            Self::ThinkingEnd { index, content } => {
                json!({"kind": "thinking_end", "index": index, "content": content})
            }
            Self::ThinkingMessageEnd {
                thinking,
                text,
                stop,
            } => json!({
                "kind": "thinking_message_end",
                "thinking": thinking,
                "text": text,
                "stop": stop,
            }),
            Self::Hold => json!({"kind": "hold"}),
            Self::Assistant { text, stop } => {
                json!({"kind": "assistant", "text": text, "stop": stop})
            }
            Self::Partial {
                streamed,
                text,
                stop,
            } => {
                json!({"kind": "assistant_partial", "streamed": streamed, "text": text, "stop": stop})
            }
            Self::Contradiction { streamed, text } => {
                json!({"kind": "assistant_contradiction", "streamed": streamed, "text": text})
            }
            Self::Tool {
                id,
                name,
                input,
                output,
                status,
                truncated,
                marker,
            } => json!({
                "kind": "tool",
                "id": id,
                "name": name,
                "input": input,
                "output": output,
                "status": status,
                "truncated": truncated,
                "marker": marker.map(|path| path.display().to_string()),
            }),
            Self::SecondAdmission {
                id,
                name,
                input,
                output,
                second,
                second_name,
                second_input,
                second_marker,
            } => json!({
                "kind": "tool_then_second_admission",
                "id": id,
                "name": name,
                "input": input,
                "output": output,
                "second": {
                    "id": second,
                    "name": second_name,
                    "input": second_input,
                    "marker": second_marker.map(|path| path.display().to_string()),
                },
            }),
            Self::UnknownResult { id, output, marker } => json!({
                "kind": "result_for_unknown_call",
                "id": id,
                "output": output,
                "marker": marker.map(|path| path.display().to_string()),
            }),
            Self::UnadmittedExecution {
                id,
                name,
                input,
                output,
            } => json!({
                "kind": "unadmitted_execution",
                "id": id,
                "name": name,
                "input": input,
                "output": output,
            }),
            Self::ModelRequest => json!({"kind": "model_request"}),
            Self::TwoCallBatch {
                id,
                name,
                input,
                output,
                second,
                second_name,
                second_input,
                second_output,
            } => json!({
                "kind": "two_call_batch",
                "id": id,
                "name": name,
                "input": input,
                "output": output,
                "second": {
                    "id": second,
                    "name": second_name,
                    "input": second_input,
                    "output": second_output,
                },
            }),
            Self::Event(event) => json!({"kind": "event", "event": event}),
            Self::WatchStatus(active) => json!({"kind": "watch_status", "active": active}),
            Self::AsyncWakeup { text, answer } => json!({
                "kind": "async_wakeup",
                "text": text,
                "answer": answer,
            }),
            Self::Stderr(text) => json!({"kind": "stderr", "text": text}),
            Self::Sleep(ms) => json!({"kind": "sleep", "ms": ms}),
            Self::WaitFile(path) => {
                json!({"kind": "wait_file", "path": path.display().to_string()})
            }
            Self::Settle => json!({"kind": "settle"}),
            Self::Exit(code) => json!({"kind": "exit", "code": code}),
        }
    }
}

/// A fake pi launcher plus the private profile it scripts.
pub struct FakePi {
    pub wrapper: PathBuf,
    pub profile: PathBuf,
    log: PathBuf,
    scenario: PathBuf,
}

impl FakePi {
    /// Install a launcher under `root` and script the instance profile at
    /// `<instance>/operator-pi`. The profile directory is private, as the
    /// production path requires.
    pub fn install(instance: &Path, root: &Path) -> Self {
        let profile = instance.join("operator-pi");
        fs::create_dir_all(&profile).unwrap();
        fs::set_permissions(&profile, fs::Permissions::from_mode(0o700)).unwrap();
        let bin = root.join("fake-pi");
        fs::create_dir_all(&bin).unwrap();
        let wrapper = bin.join("pi");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/operator-pi/operator_fake_pi.mjs");
        assert!(
            fixture.is_file(),
            "fake pi fixture missing: {}",
            fixture.display()
        );
        let node = find_node();
        fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nexec '{}' '{}' \"$@\"\n",
                node.display(),
                fixture.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        let log = profile.join("fake-pi.log");
        let scenario = profile.join("fake-pi-scenario.json");
        Self {
            wrapper,
            profile,
            log,
            scenario,
        }
    }

    /// Write the next process's scenario (mode 0600, private profile).
    pub fn script(&self, scenario: &Scenario) {
        scenario.install_at(&self.scenario);
    }

    pub fn scenario_path(&self) -> &Path {
        &self.scenario
    }

    pub fn log_path(&self) -> &Path {
        &self.log
    }

    pub fn clear_log(&self) {
        let _ = fs::remove_file(self.active_log_path());
    }

    /// Every receipt line, in order.
    pub fn log(&self) -> Vec<Value> {
        read_log(&self.active_log_path())
    }

    fn active_log_path(&self) -> PathBuf {
        fs::read(&self.scenario)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|scenario| scenario["log"].as_str().map(PathBuf::from))
            .unwrap_or_else(|| self.log.clone())
    }

    /// Wait until a receipt line satisfies `predicate`.
    pub fn wait_log(&self, budget: Duration, predicate: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let deadline = Instant::now() + budget;
        loop {
            let lines = self.log();
            if predicate(&lines) {
                return lines;
            }
            assert!(
                Instant::now() < deadline,
                "fake pi receipt deadline; lines: {lines:#?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Receipt lines for one event name.
    pub fn events(&self, name: &str) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|line| line["event"] == name)
            .collect()
    }

    /// A direct-RPC scenario file outside the instance profile.
    pub fn standalone_script(&self, scenario: &Scenario, path: &Path) {
        scenario.install_at(path);
    }
}

pub fn read_log(path: &Path) -> Vec<Value> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// An absolute `node`, since the worker's Pi environment inherits PATH but the
/// fixture launcher must not depend on which shell profile defined it.
pub fn find_node() -> PathBuf {
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join("node");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    panic!("node is required to run the fake pi fixture");
}

/// The environment pair every worker-driven fixture server needs.
pub fn executable_env(fake: &FakePi) -> (&'static str, &std::ffi::OsStr) {
    ("OVRCR_PI_EXECUTABLE", fake.wrapper.as_os_str())
}
