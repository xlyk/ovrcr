//! Current-owner proof for explicit iTerm setup and bounded no-prompt focusing.
//! Banner tickets never contain this destination; the Server supplies it only
//! after routing to its current Dashboard owner.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeOwnerTicket {
    pub schema: u32,
    pub server_wire: u32,
    pub context: crate::BridgeContext,
    pub dashboard_owner: String,
}

impl BridgeOwnerTicket {
    pub fn validate(&self) -> bool {
        self.schema == crate::BRIDGE_SCHEMA_VERSION
            && self.server_wire == crate::PROTOCOL_VERSION
            && self.context.validate()
            && crate::bridge::canonical_uuid(&self.dashboard_owner)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ITermFocusStatus {
    Authorized,
    SelectionRequested,
    AuthorizationRequired,
    Denied,
    Unavailable,
}

impl ITermFocusStatus {
    pub fn guidance(self) -> &'static str {
        match self {
            Self::Authorized => {
                "iTerm focus authorized; clicks may select this Dashboard's existing session"
            }
            Self::SelectionRequested => {
                "Existing iTerm session selection requested; visible focus is not confirmed"
            }
            Self::AuthorizationRequired => {
                "iTerm focus needs explicit setup; choose Set up iTerm focus in the palette"
            }
            Self::Denied => {
                "iTerm control denied; allow OVRCR in System Settings → Privacy & Security → Automation, then explicitly set up again"
            }
            Self::Unavailable => {
                "Exact iTerm focus unavailable; parent-app activation cannot select an exact terminal window"
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeOwnerCall {
    pub owner: BridgeOwnerTicket,
    pub outcome: Option<ITermFocusStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeOwnerResult {
    pub schema: u32,
    pub server_wire: u32,
    pub target: Option<crate::BridgeActivationTarget>,
}

impl BridgeOwnerResult {
    pub fn unavailable() -> Self {
        Self {
            schema: crate::BRIDGE_SCHEMA_VERSION,
            server_wire: crate::PROTOCOL_VERSION,
            target: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner() -> BridgeOwnerTicket {
        BridgeOwnerTicket {
            schema: crate::BRIDGE_SCHEMA_VERSION,
            server_wire: crate::PROTOCOL_VERSION,
            context: crate::BridgeContext {
                server_socket: "/tmp/owned.sock".into(),
                callback_executable: "/tmp/ovrcr".into(),
                callback_executable_sha256: "0".repeat(64),
                server_lifetime: "12345678-1234-4234-8234-123456789abc".into(),
            },
            dashboard_owner: "12345678-1234-4234-8234-123456789abd".into(),
        }
    }
    #[test]
    fn current_owner_proof_checks_every_version_context_and_owner_boundary() {
        let valid = owner();
        assert!(valid.validate());
        for field in ["schema", "wire", "owner", "lifetime", "socket", "hash"] {
            let mut changed = valid.clone();
            match field {
                "schema" => changed.schema += 1,
                "wire" => changed.server_wire += 1,
                "owner" => changed.dashboard_owner = "remembered dashboard".into(),
                "lifetime" => changed.context.server_lifetime = "not uuid".into(),
                "socket" => changed.context.server_socket = "/tmp/../foreign.sock".into(),
                _ => changed.context.callback_executable_sha256 = "g".repeat(64),
            }
            assert!(!changed.validate(), "{field}");
        }
        let mut value = serde_json::to_value(valid).unwrap();
        value["extra"] = true.into();
        assert!(serde_json::from_value::<BridgeOwnerTicket>(value).is_err());
    }
}
