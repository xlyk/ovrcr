//! JSON contract for the local native notification client. Callers bound each
//! request and reply to [`crate::MAX_FRAME_BYTES`] and check both versions.

use serde::{Deserialize, Serialize};

pub const BRIDGE_SCHEMA_VERSION: u32 = 1;

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
    /// A silent notification. These fields contain only approved display labels.
    Deliver {
        title: String,
        subtitle: String,
        body: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeReply {
    pub schema: u32,
    pub server_wire: u32,
    pub status: BridgeStatus,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
                },
                json!({
                    "type": "deliver",
                    "title": "ovrcr",
                    "subtitle": "project / workspace",
                    "body": "manual title",
                }),
            ),
        ] {
            let request = BridgeRequest {
                schema: BRIDGE_SCHEMA_VERSION,
                server_wire: crate::PROTOCOL_VERSION,
                op,
            };
            let expected = json!({
                "schema": 1,
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
        ] {
            let reply = BridgeReply {
                schema: BRIDGE_SCHEMA_VERSION,
                server_wire: crate::PROTOCOL_VERSION,
                status,
            };
            let expected = json!({
                "schema": 1,
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
}
