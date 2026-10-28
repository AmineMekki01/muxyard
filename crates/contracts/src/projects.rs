//! Typed project registration and recent-project DTOs.

use serde::{Deserialize, Serialize};

use crate::{ExecutionEnvironment, Id};

/// Persisted workspace trust state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum ProjectTrust {
    /// Scripts and project-provided commands are not trusted.
    Untrusted,
    /// The user explicitly trusted project-provided scripts.
    Trusted,
}

/// One registered project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Project {
    /// Stable project identifier.
    pub id: Id,
    /// Display name.
    pub name: String,
    /// Canonical project root.
    pub root_path: String,
    /// Execution environment that owns the path.
    pub execution_environment: ExecutionEnvironment,
    /// Optional repository identity.
    pub repo_id: Option<Id>,
    /// Workspace trust state.
    pub trust: ProjectTrust,
    /// Creation time in Unix epoch milliseconds.
    pub created_ms: i64,
    /// Last open time in Unix epoch milliseconds.
    pub last_opened_ms: i64,
}

/// Register or reopen a project directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ProjectRegisterRequest {
    /// Absolute directory path selected by the user.
    pub path: String,
    /// Optional display name; the directory name is used when omitted.
    pub name: Option<String>,
}

/// List registered projects, most recently opened first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ProjectListRequest {}

/// Registered project list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ProjectListResponse {
    /// Projects ordered by last open time.
    pub projects: Vec<Project>,
}

/// Remove a registered project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ProjectRemoveRequest {
    /// Project identifier.
    pub id: Id,
}

/// Project removal result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ProjectRemoveResponse {
    /// Whether a project row was removed.
    pub removed: bool,
}
