//! Typed local control commands and responses.

use muxyard_contracts::ApiError;
use serde::{Deserialize, Serialize};

pub use muxyard_contracts::{HostStatusRequest, HostStatusResponse};

/// Method name for the local host status query.
pub const HOST_STATUS_METHOD: &str = "host.status";

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
