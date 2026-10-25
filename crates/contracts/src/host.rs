//! Typed local host status commands and bridge events.

use serde::{Deserialize, Serialize};

use crate::ApiError;

/// Empty request body for `host.status`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct HostStatusRequest {}

/// Host identity and readiness returned by `host.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct HostStatusResponse {
    /// Host package version.
    pub host_version: String,
    /// Local control protocol version.
    pub protocol_version: u8,
    /// Current process instance identifier.
    pub instance_id: String,
    /// Operating-system process ID.
    pub pid: u32,
    /// OS process start identity for PID-reuse detection.
    pub process_start: String,
    /// Start time in Unix epoch milliseconds.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub started_ms: i64,
    /// Whether startup completed and the host is accepting clients.
    pub ready: bool,
}

/// Status event forwarded by the desktop Rust bridge to the webview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum HostStatusEvent {
    /// Initial status snapshot after a successful connection.
    Snapshot {
        /// Current host status.
        status: HostStatusResponse,
    },
    /// Updated status after a reconnect or watch poll.
    Status {
        /// Current host status.
        status: HostStatusResponse,
    },
    /// The bridge is retrying a failed status request.
    Reconnecting {
        /// Consecutive failed attempt count.
        attempt: u8,
    },
    /// The host is offline or the bridge cannot continue.
    Offline {
        /// Stable typed failure.
        error: ApiError,
    },
}
