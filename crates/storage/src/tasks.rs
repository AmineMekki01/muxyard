//! Durable task state, optimistic updates, links, and audit events.

use muxyard_contracts::TaskState;
use rusqlite::{OptionalExtension, params};
use thiserror::Error;

use crate::StorageError;

/// A persisted task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTask {
    /// Stable task ID.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Short task title.
    pub title: String,
    /// Task goal.
    pub goal: String,
    /// Constraints supplied by the user.
    pub constraints: Vec<String>,
    /// Current lifecycle state.
    pub state: TaskState,
    /// Optional worktree identity.
    pub worktree_id: Option<String>,
    /// Optional base revision.
    pub base_revision: Option<String>,
    /// Creation time in Unix epoch milliseconds.
    pub created_ms: i64,
    /// Last update time in Unix epoch milliseconds.
    pub updated_ms: i64,
    /// Optimistic concurrency version.
    pub state_version: i64,
}

/// A task audit record for a lifecycle transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskAuditEvent {
    /// Stable event ID.
    pub id: String,
    /// Task that changed.
    pub task_id: String,
    /// Previous state.
    pub from_state: TaskState,
    /// New state.
    pub to_state: TaskState,
    /// Actor that requested the transition.
    pub actor: String,
    /// Event time in Unix epoch milliseconds.
    pub created_ms: i64,
}

/// Task creation or transition failure.
#[derive(Debug, Error)]
pub enum TaskError {
    /// Task was not found.
    #[error("task {0} does not exist")]
    NotFound(String),
    /// Optimistic concurrency conflict.
    #[error("task {id} changed: expected version {expected}, found {actual}")]
    Conflict {
        /// Task ID.
        id: String,
        /// Expected version.
        expected: i64,
        /// Actual version.
        actual: i64,
    },
    /// Invalid lifecycle transition.
    #[error("task transition {from:?} -> {to:?} is not allowed")]
    InvalidTransition {
        /// Current state.
        from: TaskState,
        /// Requested state.
        to: TaskState,
    },
    /// Stored state is outside the supported vocabulary.
    #[error("stored task state is invalid: {0}")]
    InvalidState(String),
    /// Database operation failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// SQLite operation failed.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

/// Create a task in draft state.
#[allow(clippy::too_many_arguments)]
pub fn create_task(
    connection: &rusqlite::Connection,
    id: &str,
    project_id: &str,
    title: &str,
    goal: &str,
    constraints: &[String],
    base_revision: Option<&str>,
    now_ms: i64,
) -> Result<StoredTask, TaskError> {
    let constraints_json = serde_json::to_string(constraints).map_err(|error| {
        TaskError::Storage(StorageError::Sqlite(
            rusqlite::Error::ToSqlConversionFailure(Box::new(error)),
        ))
    })?;
    connection.execute(
        "INSERT INTO tasks (
            id, project_id, title, goal, constraints_json, state,
            worktree_id, base_revision, created_ms, updated_ms, state_version
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'draft', NULL, ?6, ?7, ?7, 1)",
        params![
            id,
            project_id,
            title,
            goal,
            constraints_json,
            base_revision,
            now_ms
        ],
    )?;
    get_task(connection, id)?.ok_or_else(|| TaskError::NotFound(id.to_string()))
}

/// Read one task.
pub fn get_task(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Option<StoredTask>, TaskError> {
    connection
        .query_row(
            "SELECT id, project_id, title, goal, constraints_json, state,
                    worktree_id, base_revision, created_ms, updated_ms, state_version
             FROM tasks WHERE id = ?1",
            [id],
            row_to_task,
        )
        .optional()
        .map_err(StorageError::from)?
        .transpose()
}

/// List tasks, optionally filtered by project, newest first.
pub fn list_tasks(
    connection: &rusqlite::Connection,
    project_id: Option<&str>,
) -> Result<Vec<StoredTask>, TaskError> {
    let (sql, params): (&str, Vec<String>) = match project_id {
        Some(project_id) => (
            "SELECT id, project_id, title, goal, constraints_json, state,
                    worktree_id, base_revision, created_ms, updated_ms, state_version
             FROM tasks WHERE project_id = ?1
             ORDER BY updated_ms DESC",
            vec![project_id.to_string()],
        ),
        None => (
            "SELECT id, project_id, title, goal, constraints_json, state,
                    worktree_id, base_revision, created_ms, updated_ms, state_version
             FROM tasks ORDER BY updated_ms DESC",
            Vec::new(),
        ),
    };
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(params), row_to_task)?;
    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row??);
    }
    Ok(tasks)
}

/// Update editable fields and/or transition state with optimistic concurrency.
#[allow(clippy::too_many_arguments)]
pub fn update_task(
    connection: &rusqlite::Connection,
    id: &str,
    expected_version: i64,
    title: Option<&str>,
    goal: Option<&str>,
    constraints: Option<&[String]>,
    next_state: Option<TaskState>,
    actor: &str,
    now_ms: i64,
) -> Result<StoredTask, TaskError> {
    let current = get_task(connection, id)?.ok_or_else(|| TaskError::NotFound(id.to_string()))?;
    if current.state_version != expected_version {
        return Err(TaskError::Conflict {
            id: id.to_string(),
            expected: expected_version,
            actual: current.state_version,
        });
    }
    let target_state = next_state.unwrap_or(current.state);
    if target_state != current.state && !can_transition(current.state, target_state) {
        return Err(TaskError::InvalidTransition {
            from: current.state,
            to: target_state,
        });
    }
    let constraints_json = match constraints {
        Some(constraints) => Some(serde_json::to_string(constraints).map_err(|error| {
            TaskError::Storage(StorageError::Sqlite(
                rusqlite::Error::ToSqlConversionFailure(Box::new(error)),
            ))
        })?),
        None => None,
    };
    let changed = connection.execute(
        "UPDATE tasks
         SET title = COALESCE(?2, title),
             goal = COALESCE(?3, goal),
             constraints_json = COALESCE(?4, constraints_json),
             state = ?5,
             updated_ms = ?6,
             state_version = state_version + 1
         WHERE id = ?1 AND state_version = ?7",
        params![
            id,
            title,
            goal,
            constraints_json,
            task_state_str(target_state),
            now_ms,
            expected_version
        ],
    )?;
    if changed == 0 {
        let actual = get_task(connection, id)?
            .ok_or_else(|| TaskError::NotFound(id.to_string()))?
            .state_version;
        return Err(TaskError::Conflict {
            id: id.to_string(),
            expected: expected_version,
            actual,
        });
    }
    if target_state != current.state {
        connection.execute(
            "INSERT INTO task_audit_events
             (id, task_id, from_state, to_state, actor, created_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                uuid::Uuid::new_v4().to_string(),
                id,
                task_state_str(current.state),
                task_state_str(target_state),
                actor,
                now_ms
            ],
        )?;
    }
    get_task(connection, id)?.ok_or_else(|| TaskError::NotFound(id.to_string()))
}

/// Link an agent session to a task.
pub fn link_agent(
    connection: &rusqlite::Connection,
    task_id: &str,
    agent_session_id: &str,
    role: &str,
    now_ms: i64,
) -> Result<(), TaskError> {
    connection.execute(
        "INSERT INTO task_agents (task_id, agent_session_id, role, linked_ms)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(task_id, agent_session_id)
         DO UPDATE SET role = excluded.role, linked_ms = excluded.linked_ms",
        params![task_id, agent_session_id, role, now_ms],
    )?;
    Ok(())
}

/// Link a terminal session to a task.
pub fn link_session(
    connection: &rusqlite::Connection,
    task_id: &str,
    terminal_session_id: &str,
    now_ms: i64,
) -> Result<(), TaskError> {
    connection.execute(
        "INSERT INTO task_sessions (task_id, terminal_session_id, linked_ms)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(task_id, terminal_session_id)
         DO UPDATE SET linked_ms = excluded.linked_ms",
        params![task_id, terminal_session_id, now_ms],
    )?;
    Ok(())
}

/// Read task audit events, newest first.
pub fn list_audit_events(
    connection: &rusqlite::Connection,
    task_id: &str,
) -> Result<Vec<TaskAuditEvent>, TaskError> {
    let mut statement = connection.prepare(
        "SELECT id, task_id, from_state, to_state, actor, created_ms
         FROM task_audit_events WHERE task_id = ?1
         ORDER BY created_ms DESC",
    )?;
    let rows = statement.query_map([task_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
        ))
    })?;
    let mut events = Vec::new();
    for row in rows {
        let (id, task_id, from, to, actor, created_ms) = row?;
        events.push(TaskAuditEvent {
            id,
            task_id,
            from_state: parse_task_state(&from)?,
            to_state: parse_task_state(&to)?,
            actor,
            created_ms,
        });
    }
    Ok(events)
}

/// Validate the task lifecycle transition graph.
pub fn can_transition(from: TaskState, to: TaskState) -> bool {
    use TaskState::*;
    matches!(
        (from, to),
        (Draft, Ready)
            | (Draft, Canceled)
            | (Ready, Running)
            | (Ready, Canceled)
            | (Running, Blocked)
            | (Running, Review)
            | (Running, Failed)
            | (Running, Canceled)
            | (Blocked, Running)
            | (Blocked, Review)
            | (Blocked, Failed)
            | (Blocked, Canceled)
            | (Review, Approved)
            | (Review, Running)
            | (Review, Failed)
            | (Review, Canceled)
            | (Approved, Integrated)
            | (Approved, Canceled)
    )
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<StoredTask, TaskError>> {
    let constraints_json: String = row.get(4)?;
    let state: String = row.get(5)?;
    let constraints = match serde_json::from_str::<Vec<String>>(&constraints_json) {
        Ok(constraints) => constraints,
        Err(_) => {
            return Ok(Err(TaskError::InvalidState(
                "constraints_json is invalid".to_string(),
            )));
        }
    };
    let state = match parse_task_state(&state) {
        Ok(state) => state,
        Err(error) => return Ok(Err(error)),
    };
    Ok(Ok(StoredTask {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        goal: row.get(3)?,
        constraints,
        state,
        worktree_id: row.get(6)?,
        base_revision: row.get(7)?,
        created_ms: row.get(8)?,
        updated_ms: row.get(9)?,
        state_version: row.get(10)?,
    }))
}

fn parse_task_state(value: &str) -> Result<TaskState, TaskError> {
    match value {
        "draft" => Ok(TaskState::Draft),
        "ready" => Ok(TaskState::Ready),
        "running" => Ok(TaskState::Running),
        "blocked" => Ok(TaskState::Blocked),
        "review" => Ok(TaskState::Review),
        "approved" => Ok(TaskState::Approved),
        "integrated" => Ok(TaskState::Integrated),
        "failed" => Ok(TaskState::Failed),
        "canceled" => Ok(TaskState::Canceled),
        other => Err(TaskError::InvalidState(other.to_string())),
    }
}

fn task_state_str(state: TaskState) -> &'static str {
    match state {
        TaskState::Draft => "draft",
        TaskState::Ready => "ready",
        TaskState::Running => "running",
        TaskState::Blocked => "blocked",
        TaskState::Review => "review",
        TaskState::Approved => "approved",
        TaskState::Integrated => "integrated",
        TaskState::Failed => "failed",
        TaskState::Canceled => "canceled",
    }
}
