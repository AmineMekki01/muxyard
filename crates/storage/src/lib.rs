//! SQLite persistence with transactional, checksummed migrations.

mod database;
mod host;
mod migrations;
mod workspace;

use std::{io, path::PathBuf};

pub use database::{Database, DatabaseOptions};
pub use host::{
    HostInstance, HostState, get_host_instance, mark_stale_terminal_sessions,
    reconcile_stale_host_instances, record_host_instance, update_host_instance_state,
};
pub use migrations::{MIGRATIONS, Migration, MigrationReport};
pub use workspace::{
    StoredWorkspaceLayout, TerminalSessionState, delete_workspace_layout, get_workspace_layout,
    record_terminal_session, save_workspace_layout, update_terminal_session_state,
};

/// A storage initialization or query failure.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The database path has no parent directory.
    #[error("database path {0} has no parent directory")]
    MissingParent(PathBuf),
    /// The parent directory is absent.
    #[error("database directory {0} does not exist")]
    MissingDirectory(PathBuf),
    /// The parent path is not a directory.
    #[error("database directory {0} is not a directory")]
    NotDirectory(PathBuf),
    /// The database path is a symbolic link.
    #[error("database path {0} is a symbolic link")]
    Symlink(PathBuf),
    /// The database path exists but is not a regular file.
    #[error("database path {0} is not a regular file")]
    NotFile(PathBuf),
    /// The database file can be accessed by other users.
    #[error("database file {0} has insecure permissions")]
    InsecurePermissions(PathBuf),
    /// SQLite rejected an operation.
    #[error("database operation failed: {0}")]
    Sqlite(#[source] rusqlite::Error),
    /// Another connection currently holds the required write lock.
    #[error("database is busy")]
    DatabaseBusy,
    /// A filesystem operation failed.
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        /// Path involved in the failure.
        path: PathBuf,
        /// Underlying operating-system error.
        #[source]
        source: io::Error,
    },
    /// Foreign key enforcement could not be enabled.
    #[error("foreign key enforcement could not be enabled")]
    ForeignKeysDisabled,
    /// The filesystem does not support WAL journaling.
    #[error("WAL journal mode is unavailable (reported {0})")]
    WalUnavailable(String),
    /// The configured migration list is not ordered and unique.
    #[error("migration set is invalid: {0}")]
    InvalidMigrationSet(&'static str),
    /// Recorded migrations and the database version disagree.
    #[error("database schema history is inconsistent")]
    InvalidSchemaHistory,
    /// The database was written by a newer host.
    #[error("database schema version {found} is newer than supported version {supported}")]
    SchemaNewerThanHost {
        /// Version recorded in the database.
        found: i64,
        /// Highest version known to this build.
        supported: i64,
    },
    /// An applied migration's SQL no longer matches its recorded checksum.
    #[error("migration {version} checksum does not match the recorded checksum")]
    ChecksumMismatch {
        /// Migration version.
        version: i64,
    },
    /// A migration failed and its transaction was rolled back.
    #[error("migration {version} failed: {source}")]
    MigrationFailed {
        /// Migration version.
        version: i64,
        /// SQLite failure that caused the rollback.
        #[source]
        source: rusqlite::Error,
    },
    /// A stored host state is not in the supported vocabulary.
    #[error("stored host state is not supported: {0}")]
    InvalidHostState(String),
    /// A stored host identity is outside the supported numeric range.
    #[error("stored host identity is invalid: {0}")]
    InvalidHostIdentity(String),
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(code, _) = &error
            && matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
        {
            return Self::DatabaseBusy;
        }
        Self::Sqlite(error)
    }
}

pub(crate) fn io_error(path: &std::path::Path, source: io::Error) -> StorageError {
    StorageError::Io {
        path: path.to_path_buf(),
        source,
    }
}
