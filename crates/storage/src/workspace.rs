//! Durable workspace layout and terminal-session metadata.

use rusqlite::{OptionalExtension, params};

use crate::StorageError;

/// Persisted workspace layout row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredWorkspaceLayout {
    /// Serialized `WorkspaceLayout` JSON.
    pub structure_json: String,
    /// Host instance that saved it.
    pub host_instance_id: Option<String>,
    /// Host process start token that saved it.
    pub process_start_token: Option<String>,
    /// Last update in Unix epoch milliseconds.
    pub updated_ms: i64,
}

/// Terminal-session lifecycle state used in storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalSessionState {
    /// Shell is currently running.
    Running,
    /// Shell exited.
    Exited,
    /// Shell was lost with an old host instance.
    Lost,
}

impl TerminalSessionState {
    /// SQL spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Lost => "lost",
        }
    }
}

/// Record one newly spawned terminal session.
#[allow(clippy::too_many_arguments)]
pub fn record_terminal_session(
    connection: &rusqlite::Connection,
    id: &str,
    project_id: Option<&str>,
    host_instance_id: &str,
    execution_environment_id: &str,
    cwd: &str,
    shell_executable: &str,
    now_ms: i64,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO terminal_sessions (
            id, project_id, task_id, host_instance_id, execution_environment_id,
            cwd, shell_executable, state, created_ms, updated_ms, last_event_seq
         ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, 'running', ?7, ?7, 0)
         ON CONFLICT(id) DO UPDATE SET
            project_id = excluded.project_id,
            host_instance_id = excluded.host_instance_id,
            execution_environment_id = excluded.execution_environment_id,
            cwd = excluded.cwd,
            shell_executable = excluded.shell_executable,
            state = 'running',
            updated_ms = excluded.updated_ms",
        params![
            id,
            project_id,
            host_instance_id,
            execution_environment_id,
            cwd,
            shell_executable,
            now_ms
        ],
    )?;
    Ok(())
}

/// Update one terminal session lifecycle state.
pub fn update_terminal_session_state(
    connection: &rusqlite::Connection,
    id: &str,
    state: TerminalSessionState,
    now_ms: i64,
) -> Result<bool, StorageError> {
    let changed = connection.execute(
        "UPDATE terminal_sessions SET state = ?2, updated_ms = ?3 WHERE id = ?1",
        params![id, state.as_str(), now_ms],
    )?;
    Ok(changed > 0)
}

/// Read one saved workspace layout.
pub fn get_workspace_layout(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Option<StoredWorkspaceLayout>, StorageError> {
    connection
        .query_row(
            "SELECT structure_json, host_instance_id, process_start_token, updated_ms
             FROM workspace_layouts WHERE id = ?1",
            [id],
            |row| {
                Ok(StoredWorkspaceLayout {
                    structure_json: row.get(0)?,
                    host_instance_id: row.get(1)?,
                    process_start_token: row.get(2)?,
                    updated_ms: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::from)
}

/// Save one workspace layout, replacing the previous row.
pub fn save_workspace_layout(
    connection: &rusqlite::Connection,
    id: &str,
    structure_json: &str,
    host_instance_id: &str,
    process_start_token: &str,
    now_ms: i64,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO workspace_layouts (
            id, structure_json, host_instance_id, process_start_token, updated_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
            structure_json = excluded.structure_json,
            host_instance_id = excluded.host_instance_id,
            process_start_token = excluded.process_start_token,
            updated_ms = excluded.updated_ms",
        params![
            id,
            structure_json,
            host_instance_id,
            process_start_token,
            now_ms
        ],
    )?;
    Ok(())
}

/// Delete a saved layout, used when its JSON is corrupt.
pub fn delete_workspace_layout(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<(), StorageError> {
    connection.execute("DELETE FROM workspace_layouts WHERE id = ?1", [id])?;
    Ok(())
}
