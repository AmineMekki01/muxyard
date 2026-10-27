//! Database connection setup and migration entry points.

use std::{
    fs::{self},
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::Connection;

use crate::{MIGRATIONS, Migration, MigrationReport, StorageError, io_error, migrations::migrate};

/// Connection policy for a database file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatabaseOptions {
    /// Time SQLite waits for a competing writer before returning `SQLITE_BUSY`.
    pub busy_timeout_ms: u64,
    /// Require the filesystem to accept WAL journaling.
    pub require_wal: bool,
}

impl Default for DatabaseOptions {
    fn default() -> Self {
        Self {
            busy_timeout_ms: 5_000,
            require_wal: true,
        }
    }
}

/// An open SQLite connection with foreign keys, WAL, and a busy timeout.
pub struct Database {
    connection: Connection,
    path: PathBuf,
}

impl Database {
    /// Open or create a database file with default options.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        Self::open_with_options(path, DatabaseOptions::default())
    }

    /// Open or create a database file with explicit connection options.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        options: DatabaseOptions,
    ) -> Result<Self, StorageError> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| StorageError::MissingParent(path.to_path_buf()))?;
        match fs::metadata(parent) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(StorageError::NotDirectory(parent.to_path_buf())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(StorageError::MissingDirectory(parent.to_path_buf()));
            }
            Err(error) => return Err(io_error(parent, error)),
        }
        reject_symlink_and_non_file(path)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(path)
                .map_err(|error| io_error(path, error))?;
            drop(file);
        }

        let connection = Connection::open(path)?;
        configure(&connection, options)?;
        secure_file(path)?;
        Ok(Self {
            connection,
            path: path.to_path_buf(),
        })
    }

    /// Open an in-memory database for tests and short-lived fixtures.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let connection = Connection::open_in_memory()?;
        configure(
            &connection,
            DatabaseOptions {
                busy_timeout_ms: 5_000,
                require_wal: false,
            },
        )?;
        Ok(Self {
            connection,
            path: PathBuf::from(":memory:"),
        })
    }

    /// Borrow the connection.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Borrow the connection mutably for transactions.
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }

    /// File path, or `:memory:` for an in-memory database.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Apply all built-in migrations.
    pub fn migrate(&mut self) -> Result<MigrationReport, StorageError> {
        migrate(&mut self.connection, MIGRATIONS)
    }

    /// Apply an explicit migration list. Used by migration failure fixtures.
    pub fn migrate_with(
        &mut self,
        migrations: &[Migration],
    ) -> Result<MigrationReport, StorageError> {
        migrate(&mut self.connection, migrations)
    }

    /// Current integer schema version recorded by the migration runner.
    pub fn schema_version(&self) -> Result<i64, StorageError> {
        Ok(self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }
}

fn configure(connection: &Connection, options: DatabaseOptions) -> Result<(), StorageError> {
    connection.busy_timeout(Duration::from_millis(options.busy_timeout_ms))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let foreign_keys: i64 = connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    if foreign_keys != 1 {
        return Err(StorageError::ForeignKeysDisabled);
    }
    connection.pragma_update(None, "trusted_schema", "OFF")?;

    let journal_mode: String =
        connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if options.require_wal && !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(StorageError::WalUnavailable(journal_mode));
    }
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn reject_symlink_and_non_file(path: &Path) -> Result<(), StorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(StorageError::Symlink(path.to_path_buf()));
            }
            if !metadata.is_file() {
                return Err(StorageError::NotFile(path.to_path_buf()));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(path, error)),
    }
    Ok(())
}

fn secure_file(path: &Path) -> Result<(), StorageError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| io_error(path, error))?;
        let mode = fs::metadata(path)
            .map_err(|error| io_error(path, error))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err(StorageError::InsecurePermissions(path.to_path_buf()));
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
