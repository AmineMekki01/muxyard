//! Typed durable task contracts.

use serde::{Deserialize, Serialize};

use crate::{Id, TaskState};

/// A durable task associated with a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Task {
    /// Stable task ID.
    pub id: Id,
    /// Owning project.
    pub project_id: Id,
    /// Short task title.
    pub title: String,
    /// Task goal.
    pub goal: String,
    /// User-supplied constraints.
    pub constraints: Vec<String>,
    /// Current lifecycle state.
    pub state: TaskState,
    /// Optional worktree identity.
    pub worktree_id: Option<Id>,
    /// Optional base revision.
    pub base_revision: Option<String>,
    /// Creation time in Unix epoch milliseconds.
    pub created_ms: i64,
    /// Last update time in Unix epoch milliseconds.
    pub updated_ms: i64,
    /// Optimistic concurrency version.
    pub state_version: i64,
}

/// Create a task in draft state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskCreateRequest {
    /// Owning project.
    pub project_id: Id,
    /// Short task title.
    pub title: String,
    /// Task goal.
    pub goal: String,
    /// User-supplied constraints.
    pub constraints: Vec<String>,
    /// Optional base revision.
    pub base_revision: Option<String>,
}

/// List tasks, optionally scoped to one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskListRequest {
    /// Optional project filter.
    pub project_id: Option<Id>,
}

/// Durable task list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskListResponse {
    /// Tasks ordered by most recent update.
    pub tasks: Vec<Task>,
}

/// Update editable task fields and/or transition state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskUpdateRequest {
    /// Target task.
    pub task_id: Id,
    /// Version the caller last observed.
    pub expected_version: i64,
    /// Optional title replacement.
    pub title: Option<String>,
    /// Optional goal replacement.
    pub goal: Option<String>,
    /// Optional constraints replacement.
    pub constraints: Option<Vec<String>>,
    /// Optional lifecycle transition.
    pub state: Option<TaskState>,
}

/// Audit event for one lifecycle transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskAuditEvent {
    /// Stable event ID.
    pub id: Id,
    /// Task that changed.
    pub task_id: Id,
    /// Previous state.
    pub from_state: TaskState,
    /// New state.
    pub to_state: TaskState,
    /// Actor that requested the transition.
    pub actor: String,
    /// Event time in Unix epoch milliseconds.
    pub created_ms: i64,
}

/// Link an agent session to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskLinkAgentRequest {
    /// Target task.
    pub task_id: Id,
    /// Agent session.
    pub agent_session_id: Id,
    /// Task role.
    pub role: String,
}

/// Link a terminal session to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskLinkSessionRequest {
    /// Target task.
    pub task_id: Id,
    /// Terminal session.
    pub terminal_session_id: Id,
}

/// Request audit history for one task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskAuditRequest {
    /// Target task.
    pub task_id: Id,
}

/// Task audit history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskAuditResponse {
    /// Audit events, newest first.
    pub events: Vec<TaskAuditEvent>,
}
