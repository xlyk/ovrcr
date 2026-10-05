//! JSON contract for the local native notification client. Callers bound each
//! request and reply to [`crate::MAX_FRAME_BYTES`] and check both versions.

use crate::ReadySoundChoice;
use serde::{Deserialize, Serialize};

pub const BRIDGE_SCHEMA_VERSION: u32 = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeRequest {
    pub schema: u32,
    pub server_wire: u32,
    pub op: BridgeOperation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeOperation {
    Status,
    Authorize,
    Settings,
    /// Explicit user setup only; independent from notification permission.
    #[serde(rename = "iterm_setup")]
    ITermSetup {
        target: BridgeActivationTarget,
    },
    /// Observe the explicit setup result; never starts authorization.
    #[serde(rename = "iterm_status")]
    ITermStatus {
        target: BridgeActivationTarget,
    },
    /// A notification containing approved display labels and an allowlisted sound choice.
    Deliver {
        title: String,
        subtitle: String,
        body: String,
        #[serde(default)]
        sound: Option<ReadySoundChoice>,
        navigation: BridgeNavigationTicket,
    },
}

/// Opaque navigation metadata. It carries no prompt, input request or generated title.
/// The receiving Server must revalidate its lifetime and retained run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeNavigationTicket {
    pub schema: u32,
    pub server_wire: u32,
    pub server_socket: String,
    pub callback_executable: String,
    pub callback_executable_sha256: String,
    pub server_lifetime: String,
    pub session: crate::SessionId,
    pub run: crate::SessionRunId,
}

impl BridgeNavigationTicket {
    pub fn validate(&self) -> bool {
        self.schema == BRIDGE_SCHEMA_VERSION
            && self.server_wire == crate::PROTOCOL_VERSION
            && bounded_absolute_path(&self.server_socket)
            && bounded_absolute_path(&self.callback_executable)
            && self.callback_executable_sha256.len() == 64
            && self
                .callback_executable_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && canonical_uuid(&self.server_lifetime)
            && self.session.0 != 0
            && self.run.0 != 0
    }
}

fn bounded_absolute_path(value: &str) -> bool {
    value.len() <= 4096
        && std::path::Path::new(value).is_absolute()
        && !value.chars().any(char::is_control)
        && !std::path::Path::new(value)
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
}

pub fn canonical_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeContext {
    pub server_socket: String,
    pub callback_executable: String,
    pub callback_executable_sha256: String,
    pub server_lifetime: String,
}

impl BridgeContext {
    pub fn validate(&self) -> bool {
        bounded_absolute_path(&self.server_socket)
            && bounded_absolute_path(&self.callback_executable)
            && canonical_uuid(&self.server_lifetime)
            && self.callback_executable_sha256.len() == 64
            && self
                .callback_executable_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeNavigationOffer {
    pub navigation: String,
    pub ticket: BridgeNavigationTicket,
}

/// Current Dashboard identity, obtained from the admitted socket peer. The
/// birth timestamp fences PID reuse before native parent-app activation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeActivationTarget {
    pub dashboard_pid: u32,
    pub dashboard_start_seconds: u64,
    pub dashboard_start_microseconds: u64,
    pub iterm_session_id: Option<String>,
    pub iterm_focus: bool,
    pub owner: Option<Box<crate::BridgeOwnerTicket>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeNavigationResult {
    pub schema: u32,
    pub server_wire: u32,
    pub applied: bool,
    pub activation: Option<BridgeActivationTarget>,
}

impl BridgeNavigationResult {
    pub fn ignored() -> Self {
        Self {
            schema: BRIDGE_SCHEMA_VERSION,
            server_wire: crate::PROTOCOL_VERSION,
            applied: false,
            activation: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeReply {
    pub schema: u32,
    pub server_wire: u32,
    pub status: BridgeStatus,
    /// Only with `Submitted`: a selected custom sound was unavailable, so the
    /// otherwise eligible banner was submitted silently. No raw error or path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound_unavailable: Option<BridgeSoundFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeSoundFailure {
    MissingResource,
    UnreadableResource,
    InvalidResource,
    LookupConflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeStatus {
    Available,
    NotDetermined,
    PermissionPending,
    Denied,
    Submitted,
    SettingsOpened,
    Incompatible,
    Failed,
    #[serde(rename = "iterm_pending")]
    ITermPending,
    #[serde(rename = "iterm_authorized")]
    ITermAuthorized,
    #[serde(rename = "iterm_authorization_required")]
    ITermAuthorizationRequired,
    #[serde(rename = "iterm_denied")]
    ITermDenied,
    #[serde(rename = "iterm_unavailable")]
    ITermUnavailable,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ticket() -> BridgeNavigationTicket {
        BridgeNavigationTicket {
            schema: BRIDGE_SCHEMA_VERSION,
            server_wire: crate::PROTOCOL_VERSION,
            server_socket: "/tmp/server.sock".into(),
            callback_executable: "/tmp/ovrcr".into(),
            callback_executable_sha256: "0".repeat(64),
            server_lifetime: "12345678-1234-4234-8234-123456789abc".into(),
            session: crate::SessionId(1),
            run: crate::SessionRunId(2),
        }
    }

    #[test]
    fn operations_use_nested_snake_case_json() {
        for (op, expected) in [
            (BridgeOperation::Status, json!({"type": "status"})),
            (BridgeOperation::Authorize, json!({"type": "authorize"})),
            (BridgeOperation::Settings, json!({"type": "settings"})),
            (
                BridgeOperation::Deliver {
                    title: "ovrcr".into(),
                    subtitle: "project / workspace".into(),
                    body: "manual title".into(),
                    sound: None,
                    navigation: ticket(),
                },
                json!({
                    "type": "deliver",
                    "title": "ovrcr",
                    "subtitle": "project / workspace",
                    "body": "manual title",
                    "sound": null,
                    "navigation": ticket(),
                }),
            ),
        ] {
            let request = BridgeRequest {
                schema: BRIDGE_SCHEMA_VERSION,
                server_wire: crate::PROTOCOL_VERSION,
                op,
            };
            let expected = json!({
                "schema": BRIDGE_SCHEMA_VERSION,
                "server_wire": crate::PROTOCOL_VERSION,
                "op": expected,
            });
            assert_eq!(serde_json::to_value(&request).unwrap(), expected);
            assert_eq!(
                serde_json::from_value::<BridgeRequest>(expected).unwrap(),
                request
            );
        }
    }

    #[test]
    fn explicit_iterm_operations_require_a_current_owner_target() {
        let context = BridgeContext {
            server_socket: "/tmp/server.sock".into(),
            callback_executable: "/tmp/ovrcr".into(),
            callback_executable_sha256: "0".repeat(64),
            server_lifetime: "12345678-1234-4234-8234-123456789abc".into(),
        };
        let target = BridgeActivationTarget {
            dashboard_pid: 7,
            dashboard_start_seconds: 8,
            dashboard_start_microseconds: 9,
            iterm_session_id: Some("w0t0p0:12345678-1234-4234-8234-123456789abc".into()),
            iterm_focus: true,
            owner: Some(Box::new(crate::BridgeOwnerTicket {
                schema: BRIDGE_SCHEMA_VERSION,
                server_wire: crate::PROTOCOL_VERSION,
                context,
                dashboard_owner: "12345678-1234-4234-8234-123456789abd".into(),
            })),
        };
        for (operation, name) in [
            (
                BridgeOperation::ITermSetup {
                    target: target.clone(),
                },
                "iterm_setup",
            ),
            (
                BridgeOperation::ITermStatus {
                    target: target.clone(),
                },
                "iterm_status",
            ),
        ] {
            let value = json!({"type": name, "target": target});
            assert_eq!(serde_json::to_value(&operation).unwrap(), value);
            assert_eq!(
                serde_json::from_value::<BridgeOperation>(value.clone()).unwrap(),
                operation
            );
            let mut missing = value;
            missing["target"]
                .as_object_mut()
                .unwrap()
                .remove("iterm_focus");
            assert!(serde_json::from_value::<BridgeOperation>(missing).is_err());
            assert!(serde_json::from_value::<BridgeOperation>(json!({"type": name})).is_err());
        }
    }

    #[test]
    fn replies_contain_only_versions_and_a_typed_status() {
        for (status, spelling) in [
            (BridgeStatus::Available, "available"),
            (BridgeStatus::NotDetermined, "not_determined"),
            (BridgeStatus::PermissionPending, "permission_pending"),
            (BridgeStatus::Denied, "denied"),
            (BridgeStatus::Submitted, "submitted"),
            (BridgeStatus::SettingsOpened, "settings_opened"),
            (BridgeStatus::Incompatible, "incompatible"),
            (BridgeStatus::Failed, "failed"),
            (BridgeStatus::ITermPending, "iterm_pending"),
            (BridgeStatus::ITermAuthorized, "iterm_authorized"),
            (
                BridgeStatus::ITermAuthorizationRequired,
                "iterm_authorization_required",
            ),
            (BridgeStatus::ITermDenied, "iterm_denied"),
            (BridgeStatus::ITermUnavailable, "iterm_unavailable"),
        ] {
            let reply = BridgeReply {
                schema: BRIDGE_SCHEMA_VERSION,
                server_wire: crate::PROTOCOL_VERSION,
                status,
                sound_unavailable: None,
            };
            let expected = json!({
                "schema": BRIDGE_SCHEMA_VERSION,
                "server_wire": crate::PROTOCOL_VERSION,
                "status": spelling,
            });
            assert_eq!(serde_json::to_value(&reply).unwrap(), expected);
            assert_eq!(
                serde_json::from_value::<BridgeReply>(expected).unwrap(),
                reply
            );
        }
    }

    #[test]
    fn malformed_operations_and_untyped_statuses_are_rejected() {
        for op in [
            json!({"type": "unknown"}),
            json!({"type": "deliver", "title": "ovrcr", "subtitle": "p/w"}),
            json!({"type": "deliver", "title": 1, "subtitle": "p/w", "body": "n"}),
        ] {
            assert!(serde_json::from_value::<BridgeOperation>(op).is_err());
        }
        assert!(serde_json::from_str::<BridgeStatus>("\"arbitrary error\"").is_err());
    }

    #[test]
    fn delivery_sound_is_an_optional_allowlisted_id() {
        for sound in [
            None,
            Some("default"),
            Some("tap"),
            Some("chime"),
            Some("rise"),
        ] {
            let mut json = json!({"type":"deliver", "title":"ready", "subtitle":"manual", "body":"p/w/n", "navigation":ticket()});
            if let Some(id) = sound {
                json["sound"] = id.into();
            }
            let decoded: BridgeOperation = serde_json::from_value(json.clone()).unwrap();
            assert!(matches!(
                &decoded,
                BridgeOperation::Deliver { sound: choice, .. }
                if choice.map(ReadySoundChoice::as_str) == sound
            ));
            json["sound"] = sound.map_or(serde_json::Value::Null, |id| json!(id));
            assert_eq!(serde_json::to_value(decoded).unwrap(), json);
        }
        let null_sound = json!({"type":"deliver", "title":"ready", "subtitle":"manual", "body":"p/w/n", "sound":null,"navigation":ticket()});
        assert!(matches!(
            serde_json::from_value::<BridgeOperation>(null_sound).unwrap(),
            BridgeOperation::Deliver { sound: None, .. }
        ));
        for invalid in [json!("Glass"), json!("/tmp/tap.wav"), json!(true), json!(7)] {
            let json = json!({"type":"deliver", "title":"ready", "subtitle":"manual", "body":"p/w/n", "sound":invalid,"navigation":ticket()});
            assert!(serde_json::from_value::<BridgeOperation>(json).is_err());
        }
    }

    #[test]
    fn submitted_silent_sound_failures_use_fixed_reasons() {
        for (reason, spelling) in [
            (BridgeSoundFailure::MissingResource, "missing_resource"),
            (
                BridgeSoundFailure::UnreadableResource,
                "unreadable_resource",
            ),
            (BridgeSoundFailure::InvalidResource, "invalid_resource"),
            (BridgeSoundFailure::LookupConflict, "lookup_conflict"),
        ] {
            let reply = BridgeReply {
                schema: BRIDGE_SCHEMA_VERSION,
                server_wire: crate::PROTOCOL_VERSION,
                status: BridgeStatus::Submitted,
                sound_unavailable: Some(reason),
            };
            let json = json!({"schema":BRIDGE_SCHEMA_VERSION,"server_wire":crate::PROTOCOL_VERSION,"status":"submitted","sound_unavailable":spelling});
            assert_eq!(serde_json::to_value(&reply).unwrap(), json);
            assert_eq!(serde_json::from_value::<BridgeReply>(json).unwrap(), reply);
        }
        for unavailable in [None, Some(serde_json::Value::Null)] {
            let mut json = json!({"schema":BRIDGE_SCHEMA_VERSION,"server_wire":crate::PROTOCOL_VERSION,"status":"submitted"});
            if let Some(unavailable) = unavailable {
                json["sound_unavailable"] = unavailable;
            }
            assert_eq!(
                serde_json::from_value::<BridgeReply>(json)
                    .unwrap()
                    .sound_unavailable,
                None
            );
        }
        assert!(serde_json::from_str::<BridgeSoundFailure>("\"raw path/error\"").is_err());
    }

    #[test]
    fn navigation_ticket_is_bounded_versioned_and_has_no_extra_payload() {
        let original = ticket();
        assert!(original.validate());
        for invalid in [
            "relative",
            "/tmp/../socket",
            "/tmp/control\n",
            &format!("/{}", "x".repeat(4096)),
        ] {
            let mut value = original.clone();
            value.server_socket = invalid.into();
            assert!(!value.validate());
        }
        let mut value = original.clone();
        value.server_wire += 1;
        assert!(!value.validate());
        let mut value = original.clone();
        value.schema += 1;
        assert!(!value.validate());
        let mut value = original.clone();
        value.server_lifetime = "not-a-lifetime".into();
        assert!(!value.validate());
        let mut value = serde_json::to_value(original).unwrap();
        value["prompt"] = json!("PRIVATE");
        assert!(serde_json::from_value::<BridgeNavigationTicket>(value).is_err());
    }
}
