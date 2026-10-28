//! Durable project registration and recent-project ordering.

use rusqlite::{OptionalExtension, params};

use crate::StorageError;

/// Persisted project trust state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTrust {
    /// The project may be opened, but scripts are not trusted to run.
    Untrusted,
    /// The user explicitly trusted project-provided scripts.
    Trusted,
}

impl ProjectTrust {
    /// SQL spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Untrusted => "untrusted",
            Self::Trusted => "trusted",
        }
    }
}

/// One registered project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProject {
    /// Stable project ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Canonical root path.
    pub root_path: String,
    /// Execution environment identifier.
    pub execution_environment_id: String,
    /// Optional repository identity.
    pub repo_id: Option<String>,
    /// Persisted trust state.
    pub trust: ProjectTrust,
    /// Creation time in Unix epoch milliseconds.
    pub created_ms: i64,
    /// Last open time in Unix epoch milliseconds.
    pub last_opened_ms: i64,
}

/// List registered projects, most recently opened first.
pub fn list_projects(
    connection: &rusqlite::Connection,
) -> Result<Vec<StoredProject>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT id, name, root_path, execution_environment_id, repo_id,
                trust_state, created_ms, last_opened_ms
         FROM projects
         ORDER BY last_opened_ms DESC, name ASC",
    )?;
    let rows = statement.query_map([], |row| {
        let trust = row.get::<_, String>(5)?;
        Ok(StoredProject {
            id: row.get(0)?,
            name: row.get(1)?,
            root_path: row.get(2)?,
            execution_environment_id: row.get(3)?,
            repo_id: row.get(4)?,
            trust: if trust == "trusted" {
                ProjectTrust::Trusted
            } else {
                ProjectTrust::Untrusted
            },
            created_ms: row.get(6)?,
            last_opened_ms: row.get(7)?,
        })
    })?;
    let mut projects = Vec::new();
    for row in rows {
        projects.push(row?);
    }
    Ok(projects)
}

/// Register a project or refresh an existing project with the same root and
/// execution environment.
pub fn register_project(
    connection: &rusqlite::Connection,
    id: &str,
    name: &str,
    root_path: &str,
    execution_environment_id: &str,
    repo_id: Option<&str>,
    now_ms: i64,
) -> Result<StoredProject, StorageError> {
    if let Some(existing) = find_project(connection, root_path, execution_environment_id)? {
        connection.execute(
            "UPDATE projects
             SET name = ?2, repo_id = ?3, last_opened_ms = ?4
             WHERE id = ?1",
            params![existing.id, name, repo_id, now_ms],
        )?;
        return Ok(StoredProject {
            name: name.to_string(),
            repo_id: repo_id.map(str::to_string),
            last_opened_ms: now_ms,
            ..existing
        });
    }
    connection.execute(
        "INSERT INTO projects (
            id, name, root_path, execution_environment_id, repo_id,
            trust_state, created_ms, last_opened_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'untrusted', ?6, ?6)",
        params![
            id,
            name,
            root_path,
            execution_environment_id,
            repo_id,
            now_ms
        ],
    )?;
    Ok(StoredProject {
        id: id.to_string(),
        name: name.to_string(),
        root_path: root_path.to_string(),
        execution_environment_id: execution_environment_id.to_string(),
        repo_id: repo_id.map(str::to_string),
        trust: ProjectTrust::Untrusted,
        created_ms: now_ms,
        last_opened_ms: now_ms,
    })
}

/// Mark a project as most recently opened.
pub fn touch_project(
    connection: &rusqlite::Connection,
    id: &str,
    now_ms: i64,
) -> Result<bool, StorageError> {
    let changed = connection.execute(
        "UPDATE projects SET last_opened_ms = ?2 WHERE id = ?1",
        params![id, now_ms],
    )?;
    Ok(changed > 0)
}

/// Remove a project by ID.
pub fn remove_project(connection: &rusqlite::Connection, id: &str) -> Result<bool, StorageError> {
    let changed = connection.execute("DELETE FROM projects WHERE id = ?1", [id])?;
    Ok(changed > 0)
}

fn find_project(
    connection: &rusqlite::Connection,
    root_path: &str,
    execution_environment_id: &str,
) -> Result<Option<StoredProject>, StorageError> {
    connection
        .query_row(
            "SELECT id, name, root_path, execution_environment_id, repo_id,
                    trust_state, created_ms, last_opened_ms
             FROM projects
             WHERE root_path = ?1 AND execution_environment_id = ?2",
            params![root_path, execution_environment_id],
            |row| {
                let trust = row.get::<_, String>(5)?;
                Ok(StoredProject {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    root_path: row.get(2)?,
                    execution_environment_id: row.get(3)?,
                    repo_id: row.get(4)?,
                    trust: if trust == "trusted" {
                        ProjectTrust::Trusted
                    } else {
                        ProjectTrust::Untrusted
                    },
                    created_ms: row.get(6)?,
                    last_opened_ms: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::from)
}
