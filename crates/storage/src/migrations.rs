//! Ordered SQL migrations with recorded checksums and per-migration transactions.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::StorageError;

/// One immutable migration embedded in the host binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    /// Monotonic schema version.
    pub version: i64,
    /// Stable diagnostic name.
    pub name: &'static str,
    /// SQL executed in a single transaction.
    pub sql: &'static str,
}

/// Built-in migration set.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial_schema",
    sql: include_str!("../migrations/0001_initial.sql"),
}];

/// Result of applying pending migrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Versions applied during this call.
    pub applied: Vec<i64>,
    /// Version after the call.
    pub current_version: i64,
}

#[derive(Debug)]
struct AppliedMigration {
    version: i64,
    checksum: String,
}

/// Apply `migrations` in order, rejecting drift and newer schemas.
pub fn migrate(
    connection: &mut Connection,
    migrations: &[Migration],
) -> Result<MigrationReport, StorageError> {
    validate_set(migrations)?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            checksum TEXT NOT NULL,
            applied_ms INTEGER NOT NULL
        );",
    )?;

    let applied = read_applied(connection)?;
    let user_version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let supported = migrations
        .last()
        .map(|migration| migration.version)
        .unwrap_or(0);

    if applied.is_empty() && user_version > supported {
        return Err(StorageError::SchemaNewerThanHost {
            found: user_version,
            supported,
        });
    }
    if applied.len() > migrations.len() {
        let found = applied
            .last()
            .map(|migration| migration.version)
            .unwrap_or(0);
        return Err(if found > supported {
            StorageError::SchemaNewerThanHost { found, supported }
        } else {
            StorageError::InvalidSchemaHistory
        });
    }
    let recorded_version = applied
        .last()
        .map(|migration| migration.version)
        .unwrap_or(0);
    if user_version != recorded_version {
        return Err(StorageError::InvalidSchemaHistory);
    }

    for (index, applied_migration) in applied.iter().enumerate() {
        let expected = migrations[index];
        if applied_migration.version != expected.version {
            return Err(if applied_migration.version > supported {
                StorageError::SchemaNewerThanHost {
                    found: applied_migration.version,
                    supported,
                }
            } else {
                StorageError::InvalidSchemaHistory
            });
        }
        if applied_migration.checksum != checksum(expected.sql) {
            return Err(StorageError::ChecksumMismatch {
                version: expected.version,
            });
        }
    }

    let mut newly_applied = Vec::new();
    for migration in &migrations[applied.len()..] {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(migration.sql).map_err(|source| {
            StorageError::MigrationFailed {
                version: migration.version,
                source,
            }
        })?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, name, checksum, applied_ms)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                migration.version,
                migration.name,
                checksum(migration.sql),
                now_ms()
            ],
        )?;
        transaction.pragma_update(None, "user_version", migration.version)?;
        transaction.commit()?;
        newly_applied.push(migration.version);
    }

    Ok(MigrationReport {
        applied: newly_applied,
        current_version: supported,
    })
}

fn validate_set(migrations: &[Migration]) -> Result<(), StorageError> {
    let mut previous = 0;
    for migration in migrations {
        if migration.version <= previous {
            return Err(StorageError::InvalidMigrationSet(
                "versions must be positive and strictly increasing",
            ));
        }
        if migration.name.is_empty() || migration.sql.trim().is_empty() {
            return Err(StorageError::InvalidMigrationSet(
                "each migration needs a name and SQL",
            ));
        }
        previous = migration.version;
    }
    Ok(())
}

fn read_applied(connection: &Connection) -> Result<Vec<AppliedMigration>, StorageError> {
    let mut statement =
        connection.prepare("SELECT version, checksum FROM schema_migrations ORDER BY version")?;
    let rows = statement.query_map([], |row| {
        Ok(AppliedMigration {
            version: row.get(0)?,
            checksum: row.get(1)?,
        })
    })?;
    let mut applied = Vec::new();
    for row in rows {
        applied.push(row?);
    }
    Ok(applied)
}

fn checksum(sql: &str) -> String {
    let digest = Sha256::digest(sql.as_bytes());
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
