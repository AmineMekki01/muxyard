//! Protected same-user local control transport.
//!
//! The host listens on a Unix-domain socket protected by a private runtime
//! directory and owner-only socket permissions, or on a Windows named pipe
//! protected by a current-user security descriptor. Messages are bounded,
//! versioned JSON frames with explicit request identifiers.

mod client;
mod frame;
mod server;
mod types;

use std::io;

use muxyard_contracts::ApiError;
use thiserror::Error;

pub use client::{IpcClient, TerminalStream};
pub use frame::{MAX_FRAME_BYTES, read_frame, write_frame};
pub(crate) use frame::{read_frame_until, write_frame_until};
pub use server::{IpcOptions, IpcServer};
pub use types::{
    HOST_STATUS_METHOD, HostStatusRequest, HostStatusResponse, IpcResponse, PROJECT_LIST_METHOD,
    PROJECT_REGISTER_METHOD, PROJECT_REMOVE_METHOD, ProjectStore, TASK_AUDIT_METHOD,
    TASK_CREATE_METHOD, TASK_LIST_METHOD, TASK_UPDATE_METHOD, TERMINAL_ATTACH_METHOD,
    TERMINAL_CLOSE_METHOD, TERMINAL_INPUT_METHOD, TERMINAL_RESIZE_METHOD, TERMINAL_SNAPSHOT_METHOD,
    TERMINAL_SPAWN_METHOD, TaskStore, WORKSPACE_LAYOUT_GET_METHOD, WORKSPACE_LAYOUT_SAVE_METHOD,
    WorkspaceLayoutStore,
};

/// Local transport, framing, or dispatch failure.
#[derive(Debug, Error)]
pub enum IpcError {
    /// Local socket or named-pipe I/O failed.
    #[error("local IPC I/O failed: {0}")]
    Io(#[from] io::Error),
    /// The frame header declared more than the permitted payload size.
    #[error("local IPC frame exceeds the 1 MiB limit")]
    FrameTooLarge,
    /// The peer closed in the middle of a frame.
    #[error("local IPC frame is truncated")]
    TruncatedFrame,
    /// A frame had a zero length or otherwise invalid length.
    #[error("local IPC frame is invalid")]
    InvalidFrame,
    /// The connected principal is not the current operating-system user.
    #[error("local IPC peer is not the current user")]
    UnauthorizedPeer,
    /// The endpoint permissions are broader than the local transport allows.
    #[error("local IPC endpoint permissions are not private")]
    InsecureEndpoint,
    /// The endpoint could not be constructed on this platform.
    #[error("local IPC endpoint is unsupported on this platform")]
    UnsupportedPlatform,
    /// The endpoint name or security descriptor could not be constructed.
    #[error("local IPC endpoint configuration failed: {0}")]
    Endpoint(String),
    /// The connection was idle longer than the configured timeout.
    #[error("local IPC request timed out")]
    Timeout,
    /// The peer closed before a response was available.
    #[error("local IPC peer closed the connection")]
    Closed,
    /// A response did not correlate with its request.
    #[error("local IPC response does not match its request")]
    Protocol,
    /// A typed API failure returned by the host.
    #[error(transparent)]
    Api(#[from] ApiError),
    /// A control envelope could not be encoded or decoded.
    #[error("local IPC control payload is invalid")]
    Json(#[from] serde_json::Error),
    /// A terminal binary frame is invalid.
    #[error(transparent)]
    TerminalFrame(#[from] muxyard_terminal::TerminalFrameError),
    /// A terminal subscriber fell behind and was detached.
    #[error("terminal subscriber fell behind")]
    TerminalDetached,
    /// A terminal stream response did not match its subscription.
    #[error("terminal stream protocol is invalid")]
    TerminalProtocol,
}
