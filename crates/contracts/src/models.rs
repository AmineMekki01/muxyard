//! Shared wire identifiers, execution environments, and lifecycle vocabulary.

use serde::{Deserialize, Serialize};

/// Opaque wire identifier. Format and ownership are validated by its domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Id(pub String);

/// Distinct native and WSL execution contexts; paths must not cross implicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum ExecutionEnvironment {
    /// Native macOS process environment.
    NativeMac {},
    /// Native Linux process environment.
    NativeLinux {},
    /// Native Windows process environment, distinct from WSL.
    NativeWindows {},
    /// A particular WSL distribution.
    Wsl {
        /// Distribution name as configured on the Windows host.
        distro: String,
    },
}

/// Durable task state vocabulary, not permission to perform a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum TaskState {
    /// Goal is still being drafted.
    Draft,
    /// Task is ready to start.
    Ready,
    /// Work is running.
    Running,
    /// Work is blocked.
    Blocked,
    /// Changes await review.
    Review,
    /// A user approved the revision-bound review.
    Approved,
    /// A recorded, user-approved integration succeeded.
    Integrated,
    /// Work failed.
    Failed,
    /// Work was canceled.
    Canceled,
}

/// Provider session status; evidence and authorization are separate concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum AgentStatus {
    /// Provider process is starting.
    Starting,
    /// Provider is working.
    Working,
    /// Provider is idle.
    Idle,
    /// Provider requires user input.
    NeedsInput,
    /// Provider requires a native permission decision.
    NeedsApproval,
    /// A turn finished, without implying verified task completion.
    TurnFinished,
    /// Provider reported failure.
    Failed,
    /// Provider process exited.
    Exited,
    /// Current status has not been established.
    Unknown,
}
