//! Typed local control commands and responses.

use muxyard_contracts::ApiError;
use serde::{Deserialize, Serialize};

/// Method name for the local host status query.
pub const HOST_STATUS_METHOD: &str = "host.status";

/// Empty request body for `host.status`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostStatusRequest {}

/// Host identity and readiness returned by `host.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    pub started_ms: i64,
    /// Whether startup completed and the host is accepting clients.
    pub ready: bool,
}

/// Success or typed failure for one local request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase", deny_unknown_fields)]
pub enum IpcResponse<T> {
    /// Successful method result.
    Ok {
        /// Method-specific payload.
        data: T,
    },
    /// Stable typed failure.
    Error {
        /// Machine-readable failure.
        error: ApiError,
    },
}

impl<T> IpcResponse<T> {
    /// Build a success response.
    pub fn ok(data: T) -> Self {
        Self::Ok { data }
    }

    /// Build a typed failure response.
    pub fn error(error: ApiError) -> Self {
        Self::Error { error }
    }
}
