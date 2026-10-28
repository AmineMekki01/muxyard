//! Typed local control commands and responses.

use muxyard_contracts::{
    ApiError, Project, ProjectListRequest, ProjectRegisterRequest, ProjectRemoveRequest, Task,
    TaskAuditEvent, TaskCreateRequest, TaskListRequest, TaskUpdateRequest,
};
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

/// Host-owned project registration and recent-project persistence.
pub trait ProjectStore: Send + Sync {
    /// List registered projects, most recently opened first.
    fn list_projects(&self, request: &ProjectListRequest) -> Result<Vec<Project>, String>;
    /// Register or reopen a project directory.
    fn register_project(&self, request: &ProjectRegisterRequest) -> Result<Project, String>;
    /// Remove a project registration.
    fn remove_project(&self, request: &ProjectRemoveRequest) -> Result<bool, String>;
}

/// Method name for listing projects.
pub const PROJECT_LIST_METHOD: &str = "project.list";

/// Method name for registering or reopening a project.
pub const PROJECT_REGISTER_METHOD: &str = "project.register";

/// Method name for removing a project registration.
pub const PROJECT_REMOVE_METHOD: &str = "project.remove";

/// Host-owned durable task persistence and transitions.
pub trait TaskStore: Send + Sync {
    /// List tasks, optionally filtered by project.
    fn list_tasks(&self, request: &TaskListRequest) -> Result<Vec<Task>, String>;
    /// Create a task in draft state.
    fn create_task(&self, request: &TaskCreateRequest) -> Result<Task, String>;
    /// Update task fields or transition state.
    fn update_task(&self, request: &TaskUpdateRequest) -> Result<Task, String>;
    /// Read audit events for a task.
    fn task_audit(&self, task_id: &str) -> Result<Vec<TaskAuditEvent>, String>;
}

/// Method name for listing tasks.
pub const TASK_LIST_METHOD: &str = "task.list";

/// Method name for creating a task.
pub const TASK_CREATE_METHOD: &str = "task.create";

/// Method name for updating or transitioning a task.
pub const TASK_UPDATE_METHOD: &str = "task.update";

/// Method name for reading task audit events.
pub const TASK_AUDIT_METHOD: &str = "task.audit";
