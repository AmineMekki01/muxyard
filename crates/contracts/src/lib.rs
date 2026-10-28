//! Rust-authoritative control DTOs and bounded JSON validation.
//!
//! These types carry data only. Transport framing, authentication, authorization,
//! and command dispatch are the responsibility of the host.

mod control;
mod errors;
mod host;
mod models;
mod terminal;
mod workspace;

pub use control::{ControlEnvelope, ControlMessageType, decode_control};
pub use errors::{ApiError, ErrorCode};
pub use host::{HostStatusEvent, HostStatusRequest, HostStatusResponse};
pub use models::{AgentStatus, ExecutionEnvironment, Id, TaskState};
pub use terminal::{
    TerminalCloseRequest, TerminalInputRequest, TerminalRenderConfig, TerminalResizeRequest,
    TerminalSnapshotRequest, TerminalSnapshotResponse, TerminalSpawnRequest, TerminalSpawnResponse,
};
pub use workspace::{
    MAX_WORKSPACE_LAYOUT_BYTES, TerminalLayoutNode, TerminalSplitDirection, TerminalTabLayout,
    WorkspaceLayout, WorkspaceLayoutGetRequest, WorkspaceLayoutSaveRequest,
    WorkspaceLayoutSnapshot,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Version of the local control protocol.
pub const PROTOCOL_VERSION: u8 = 1;

/// Maximum JSON control payload size, excluding the transport's length prefix.
pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;

/// A wire version that can only serialize as the supported numeric literal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "1"))]
pub struct ProtocolVersion;

impl Serialize for ProtocolVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(PROTOCOL_VERSION)
    }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let version = u64::deserialize(deserializer)?;
        if version != u64::from(PROTOCOL_VERSION) {
            return Err(de::Error::custom("unsupported protocol version"));
        }
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::{PROTOCOL_VERSION, ProtocolVersion};

    #[test]
    fn protocol_matches_the_architecture_lock() {
        assert_eq!(PROTOCOL_VERSION, 1);
        assert_eq!(serde_json::to_string(&ProtocolVersion).unwrap(), "1");
        assert!(serde_json::from_str::<ProtocolVersion>("2").is_err());
    }
}
