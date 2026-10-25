//! Stable, machine-readable API failures without transport-specific details.

use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

/// Stable failure codes. Invalid wire data is distinct from host/domain errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum ErrorCode {
    /// Caller is not authenticated.
    Unauthorized,
    /// Caller lacks permission for this action.
    Forbidden,
    /// Requested entity does not exist within the caller's scope.
    NotFound,
    /// Requested mutation conflicts with current state.
    Conflict,
    /// Requested provider or host capability is unavailable.
    UnsupportedCapability,
    /// Supplied revision is no longer current.
    StaleRevision,
    /// Workspace host cannot be reached.
    HostOffline,
    /// Request exceeds an applicable rate limit.
    RateLimited,
    /// Provider operation failed.
    ProviderError,
    /// An internal operation failed.
    Internal,
    /// Message uses a protocol version other than the supported version.
    UnsupportedVersion,
    /// Message is malformed or does not match the expected DTO.
    InvalidRequest,
    /// JSON control payload exceeds the permitted byte length.
    PayloadTooLarge,
}

/// Public failure payload. Messages must not contain input bodies or secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ApiError {
    /// Stable classification for programmatic handling.
    pub code: ErrorCode,
    /// Human-readable, non-sensitive diagnostic.
    pub message: String,
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ApiError {}
