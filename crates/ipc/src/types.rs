//! Typed local control commands and responses.

use muxyard_contracts::ApiError;
use serde::{Deserialize, Serialize};

pub use muxyard_contracts::{HostStatusRequest, HostStatusResponse};

/// Method name for the local host status query.
pub const HOST_STATUS_METHOD: &str = "host.status";

/// Method name for subscribing a connection to a terminal stream.
pub const TERMINAL_ATTACH_METHOD: &str = "terminal.attach";

/// Method name for creating a native PTY shell.
pub const TERMINAL_SPAWN_METHOD: &str = "terminal.spawn";

/// Method name for writing terminal input.
pub const TERMINAL_INPUT_METHOD: &str = "terminal.input";

/// Method name for resizing a terminal session.
pub const TERMINAL_RESIZE_METHOD: &str = "terminal.resize";

/// Method name for closing a terminal session.
pub const TERMINAL_CLOSE_METHOD: &str = "terminal.close";

/// Method name for reading a bounded recent output tail.
pub const TERMINAL_SNAPSHOT_METHOD: &str = "terminal.snapshot";

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

/// Host-owned persistence for the desktop workspace layout.
pub trait WorkspaceLayoutStore: Send + Sync {
    /// Load the saved layout and host identity metadata.
    fn load_layout(&self) -> Result<muxyard_contracts::WorkspaceLayoutSnapshot, String>;
    /// Persist the layout for the current host.
    fn save_layout(&self, layout: &muxyard_contracts::WorkspaceLayout) -> Result<(), String>;
}

/// Method name for reading the persisted workspace layout.
pub const WORKSPACE_LAYOUT_GET_METHOD: &str = "workspace.layout.get";

/// Method name for saving the workspace layout.
pub const WORKSPACE_LAYOUT_SAVE_METHOD: &str = "workspace.layout.save";
