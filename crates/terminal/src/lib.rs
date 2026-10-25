//! Binary terminal stream framing and bounded per-client backpressure.
//!
//! This crate does not spawn a PTY. It defines the transport-safe frame
//! contract and the bounded delivery model that a PTY owner can feed.

mod frame;
mod hub;

pub use frame::{
    MAX_TERMINAL_FRAME_BYTES, MAX_TERMINAL_PAYLOAD, TERMINAL_HEADER_BYTES,
    TERMINAL_PROTOCOL_VERSION, TerminalFrame, TerminalFrameError, TerminalFrameKind,
    TerminalSequence,
};
pub use hub::{
    DEFAULT_CLIENT_QUEUE_BYTES, MAX_CLIENT_QUEUE_BYTES, PublishReport, TerminalAttachRequest,
    TerminalAttachResponse, TerminalHub, TerminalHubError, TerminalLimits, TerminalSession,
    TerminalSubscription,
};
