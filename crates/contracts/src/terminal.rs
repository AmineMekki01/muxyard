//! Typed local terminal session commands.

use serde::{Deserialize, Serialize};

use crate::Id;

/// Request to create a native PTY-backed shell session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TerminalSpawnRequest {
    /// Working directory; omitted means the host process directory.
    pub cwd: Option<String>,
    /// Optional explicit shell executable.
    pub shell: Option<String>,
    /// Initial rows.
    pub rows: u16,
    /// Initial columns.
    pub cols: u16,
}

/// Created terminal session identity and resolved launch context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TerminalSpawnResponse {
    /// Stream/session identifier.
    pub stream_id: Id,
    /// Resolved shell executable.
    pub shell: String,
    /// Canonical working directory.
    pub cwd: String,
    /// Initial rows.
    pub rows: u16,
    /// Initial columns.
    pub cols: u16,
}

/// Input bytes represented as a UTF-8 string from the terminal renderer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TerminalInputRequest {
    /// Target stream/session.
    pub stream_id: Id,
    /// Terminal input text.
    pub data: String,
}

/// Resize request for one terminal session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TerminalResizeRequest {
    /// Target stream/session.
    pub stream_id: Id,
    /// Rows.
    pub rows: u16,
    /// Columns.
    pub cols: u16,
}

/// Close request for one terminal session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TerminalCloseRequest {
    /// Target stream/session.
    pub stream_id: Id,
}
