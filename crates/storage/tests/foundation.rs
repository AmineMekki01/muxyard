//! Storage foundation integration tests.

use std::fs;

use muxyard_contracts::TaskState;
use muxyard_storage::{
    Database, DatabaseOptions, HostInstance, HostState, MIGRATIONS, Migration, StorageError,
    TerminalSessionState, get_host_instance, get_workspace_layout, mark_stale_terminal_sessions,
    reconcile_stale_host_instances, record_host_instance, record_terminal_session,
    save_workspace_layout, update_host_instance_state, update_terminal_session_state,
};
use rusqlite::{Connection, params};

fn temp_database() -> (tempfile::TempDir, Database) {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(root.path().join("muxyard.sqlite3")).unwrap();
    (root, database)
}

fn count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn insert_project(connection: &Connection, id: &str) {
    connection
        .execute(
            "INSERT INTO projects (
                id, name, root_path, execution_environment_id, repo_id,
                created_ms, last_opened_ms
             ) VALUES (?1, 'project', ?2, 'native-mac', NULL, 1, 1)",
            params![id, format!("/tmp/{id}")],
        )
        .unwrap();
}

fn insert_task(connection: &Connection, id: &str, project_id: &str, state: &str) {
    connection
        .execute(
            "INSERT INTO tasks (
                id, project_id, title, goal, constraints_json, state,
                worktree_id, base_revision, created_ms, updated_ms, state_version
             ) VALUES (?1, ?2, 'title', '', '[]', ?3, NULL, NULL, 1, 1, 1)",
            params![id, project_id, state],
        )
        .unwrap();
}

#[test]
fn fresh_database_initializes_and_repeats_idempotently() {
    let (_root, mut database) = temp_database();
    let first = database.migrate().unwrap();
    assert_eq!(first.applied, vec![1]);
    assert_eq!(first.current_version, 1);
    assert_eq!(database.schema_version().unwrap(), 1);

    let second = database.migrate().unwrap();
    assert!(second.applied.is_empty());
    assert_eq!(second.current_version, 1);
    assert_eq!(database.schema_version().unwrap(), 1);

    let journal_mode: String = database
        .connection()
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    let foreign_keys: i64 = database
        .connection()
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(foreign_keys, 1);
    assert_eq!(count(database.connection(), "schema_migrations"), 1);
    let fts5: i64 = database
        .connection()
        .query_row(
            "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(fts5, 1);
}

#[test]
fn restart_preserves_data_and_schema_version() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("muxyard.sqlite3");
    {
        let mut database = Database::open(&path).unwrap();
        database.migrate().unwrap();
        insert_project(database.connection(), "p1");
    }
    let mut database = Database::open(&path).unwrap();
    let report = database.migrate().unwrap();
    assert!(report.applied.is_empty());
    assert_eq!(count(database.connection(), "projects"), 1);
}

#[test]
fn foreign_keys_reject_orphans_and_restrict_project_deletion() {
    let (_root, mut database) = temp_database();
    database.migrate().unwrap();
    let connection = database.connection();

    assert!(
        connection
            .execute(
                "INSERT INTO tasks (
                    id, project_id, title, goal, constraints_json, state,
                    worktree_id, base_revision, created_ms, updated_ms, state_version
                 ) VALUES ('t1', 'missing', 'title', '', '[]', 'draft', NULL, NULL, 1, 1, 1)",
                [],
            )
            .is_err()
    );

    insert_project(connection, "p1");
    insert_task(connection, "t1", "p1", "draft");
    assert!(
        connection
            .execute("DELETE FROM projects WHERE id = 'p1'", [])
            .is_err()
    );
    assert_eq!(count(connection, "tasks"), 1);
}

#[test]
fn task_states_match_the_shared_contract() {
    let (_root, mut database) = temp_database();
    database.migrate().unwrap();
    let connection = database.connection();
    insert_project(connection, "p1");

    for (index, state) in [
        TaskState::Draft,
        TaskState::Ready,
        TaskState::Running,
        TaskState::Blocked,
        TaskState::Review,
        TaskState::Approved,
        TaskState::Integrated,
        TaskState::Failed,
        TaskState::Canceled,
    ]
    .into_iter()
    .enumerate()
    {
        let state = serde_json::to_value(state).unwrap();
        let state = state.as_str().unwrap();
        insert_task(connection, &format!("t{index}"), "p1", state);
    }
    assert!(
        connection
            .execute(
                "INSERT INTO tasks (
                    id, project_id, title, goal, constraints_json, state,
                    worktree_id, base_revision, created_ms, updated_ms, state_version
                 ) VALUES ('bad', 'p1', 'title', '', '[]', 'unknown', NULL, NULL, 1, 1, 1)",
                [],
            )
            .is_err()
    );
    assert_eq!(count(connection, "tasks"), 9);
}

#[test]
fn project_roots_are_unique_per_execution_environment() {
    let (_root, mut database) = temp_database();
    database.migrate().unwrap();
    insert_project(database.connection(), "p1");
    let duplicate = database.connection().execute(
        "INSERT INTO projects (
            id, name, root_path, execution_environment_id, repo_id,
            created_ms, last_opened_ms
         ) VALUES ('p2', 'other', '/tmp/p1', 'native-mac', NULL, 1, 1)",
        [],
    );
    assert!(duplicate.is_err());
}

#[test]
fn host_instance_roundtrip_and_upsert_are_idempotent() {
    let (_root, mut database) = temp_database();
    database.migrate().unwrap();
    let connection = database.connection();
    let mut instance = HostInstance {
        id: "host-1".to_string(),
        pid: 42,
        os_user: "user".to_string(),
        process_start_token: "token".to_string(),
        host_version: "0.1.0".to_string(),
        protocol_version: 1,
        state: HostState::Starting,
        started_ms: 100,
        heartbeat_ms: 100,
    };
    record_host_instance(connection, &instance).unwrap();
    assert_eq!(
        get_host_instance(connection, "host-1").unwrap(),
        Some(instance.clone())
    );

    instance.state = HostState::Ready;
    instance.heartbeat_ms = 200;
    record_host_instance(connection, &instance).unwrap();
    assert_eq!(
        get_host_instance(connection, "host-1").unwrap(),
        Some(instance.clone())
    );
    assert_eq!(count(connection, "host_instances"), 1);

    assert!(update_host_instance_state(connection, "host-1", HostState::Stopped, 300).unwrap());
    let stored = get_host_instance(connection, "host-1").unwrap().unwrap();
    assert_eq!(stored.state, HostState::Stopped);
    assert_eq!(stored.heartbeat_ms, 300);
    assert!(!update_host_instance_state(connection, "missing", HostState::Stopped, 300).unwrap());
}

#[test]
fn reconciles_old_hosts_and_marks_their_sessions_lost() {
    let (_root, mut database) = temp_database();
    database.migrate().unwrap();
    let connection = database.connection();
    let old = HostInstance {
        id: "old-host".to_string(),
        pid: 10,
        os_user: "user".to_string(),
        process_start_token: "old-start".to_string(),
        host_version: "0.1.0".to_string(),
        protocol_version: 1,
        state: HostState::Ready,
        started_ms: 1,
        heartbeat_ms: 1,
    };
    let current = HostInstance {
        id: "current-host".to_string(),
        pid: 20,
        os_user: "user".to_string(),
        process_start_token: "current-start".to_string(),
        host_version: "0.1.0".to_string(),
        protocol_version: 1,
        state: HostState::Starting,
        started_ms: 2,
        heartbeat_ms: 2,
    };
    record_host_instance(connection, &old).unwrap();
    record_host_instance(connection, &current).unwrap();
    connection
        .execute(
            "INSERT INTO terminal_sessions (
                id, project_id, task_id, host_instance_id, execution_environment_id,
                cwd, shell_executable, state, created_ms, updated_ms, last_event_seq
             ) VALUES ('session-1', NULL, NULL, 'old-host', 'native-mac',
                '/tmp', '/bin/zsh', 'running', 1, 1, 0)",
            [],
        )
        .unwrap();

    assert_eq!(
        reconcile_stale_host_instances(connection, "current-host", 100).unwrap(),
        1
    );
    assert_eq!(
        mark_stale_terminal_sessions(connection, "current-host", 100).unwrap(),
        1
    );
    assert_eq!(
        get_host_instance(connection, "old-host")
            .unwrap()
            .unwrap()
            .state,
        HostState::Stopped
    );
    assert_eq!(
        get_host_instance(connection, "current-host")
            .unwrap()
            .unwrap()
            .state,
        HostState::Starting
    );
    let session_state: String = connection
        .query_row(
            "SELECT state FROM terminal_sessions WHERE id = 'session-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(session_state, "lost");
}

#[test]
fn all_host_states_are_storable_and_unknown_values_are_rejected() {
    let (_root, mut database) = temp_database();
    database.migrate().unwrap();
    let connection = database.connection();
    for (index, state) in HostState::ALL.into_iter().enumerate() {
        let instance = HostInstance {
            id: format!("host-{index}"),
            pid: index as u32 + 1,
            os_user: "user".to_string(),
            process_start_token: format!("token-{index}"),
            host_version: "0.1.0".to_string(),
            protocol_version: 1,
            state,
            started_ms: 1,
            heartbeat_ms: 1,
        };
        record_host_instance(connection, &instance).unwrap();
    }
    assert_eq!(count(connection, "host_instances"), 7);
    assert!(
        connection
            .execute(
                "INSERT INTO host_instances (
                    id, pid, os_user, process_start_token, host_version,
                    protocol_version, state, started_ms, heartbeat_ms
                 ) VALUES ('bad', 1, 'user', 'token', '0.1.0', 1, 'unknown', 1, 1)",
                [],
            )
            .is_err()
    );
}

#[test]
fn failed_migration_rolls_back_and_database_remains_usable() {
    let (_root, mut database) = temp_database();
    let migrations = [
        Migration {
            version: 1,
            name: "stable",
            sql: "CREATE TABLE stable (id INTEGER PRIMARY KEY);",
        },
        Migration {
            version: 2,
            name: "broken",
            sql: "CREATE TABLE partial (id INTEGER); INSERT INTO missing VALUES (1);",
        },
    ];
    let error = database.migrate_with(&migrations).unwrap_err();
    assert!(matches!(
        error,
        StorageError::MigrationFailed { version: 2, .. }
    ));
    assert_eq!(database.schema_version().unwrap(), 1);
    assert_eq!(count(database.connection(), "schema_migrations"), 1);
    assert!(
        database
            .connection()
            .execute("INSERT INTO stable (id) VALUES (1)", [])
            .is_ok()
    );
    assert!(
        database
            .connection()
            .query_row("SELECT COUNT(*) FROM partial", [], |row| row
                .get::<_, i64>(0))
            .is_err()
    );

    let repaired = [
        migrations[0],
        Migration {
            version: 2,
            name: "repaired",
            sql: "CREATE TABLE partial (id INTEGER);",
        },
    ];
    let report = database.migrate_with(&repaired).unwrap();
    assert_eq!(report.applied, vec![2]);
    assert_eq!(database.schema_version().unwrap(), 2);
}

#[test]
fn checksum_mismatch_is_rejected_without_changes() {
    let (_root, mut database) = temp_database();
    let original = [Migration {
        version: 1,
        name: "initial",
        sql: "CREATE TABLE stable (id INTEGER PRIMARY KEY);",
    }];
    database.migrate_with(&original).unwrap();
    database
        .connection()
        .execute("INSERT INTO stable (id) VALUES (7)", [])
        .unwrap();

    let changed = [Migration {
        version: 1,
        name: "initial",
        sql: "CREATE TABLE stable (id INTEGER PRIMARY KEY, extra TEXT);",
    }];
    assert!(matches!(
        database.migrate_with(&changed).unwrap_err(),
        StorageError::ChecksumMismatch { version: 1 }
    ));
    assert_eq!(database.schema_version().unwrap(), 1);
    assert_eq!(
        database
            .connection()
            .query_row("SELECT id FROM stable", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        7
    );
}

#[test]
fn newer_schema_is_rejected() {
    let (_root, mut database) = temp_database();
    database.migrate_with(MIGRATIONS).unwrap();
    database
        .connection()
        .execute(
            "INSERT INTO schema_migrations (version, name, checksum, applied_ms)
             VALUES (2, 'future', 'checksum', 1)",
            [],
        )
        .unwrap();
    database
        .connection()
        .pragma_update(None, "user_version", 2)
        .unwrap();
    assert!(matches!(
        database.migrate().unwrap_err(),
        StorageError::SchemaNewerThanHost {
            found: 2,
            supported: 1
        }
    ));
}

#[test]
fn non_contiguous_history_is_rejected() {
    let (_root, mut database) = temp_database();
    let migrations = [
        Migration {
            version: 1,
            name: "one",
            sql: "CREATE TABLE one (id INTEGER PRIMARY KEY);",
        },
        Migration {
            version: 2,
            name: "two",
            sql: "CREATE TABLE two (id INTEGER PRIMARY KEY);",
        },
    ];
    database.migrate_with(&migrations).unwrap();
    database
        .connection()
        .execute("DELETE FROM schema_migrations WHERE version = 1", [])
        .unwrap();
    assert!(matches!(
        database.migrate_with(&migrations).unwrap_err(),
        StorageError::InvalidSchemaHistory
    ));
}

#[test]
fn extra_recorded_history_is_rejected_without_downgrade() {
    let (_root, mut database) = temp_database();
    let migrations = [
        Migration {
            version: 1,
            name: "one",
            sql: "CREATE TABLE one (id INTEGER PRIMARY KEY);",
        },
        Migration {
            version: 2,
            name: "two",
            sql: "CREATE TABLE two (id INTEGER PRIMARY KEY);",
        },
    ];
    database.migrate_with(&migrations).unwrap();
    assert!(matches!(
        database.migrate_with(&migrations[1..]).unwrap_err(),
        StorageError::InvalidSchemaHistory
    ));
    assert_eq!(database.schema_version().unwrap(), 2);
}

#[test]
fn database_locked_returns_busy_without_writing_schema() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("muxyard.sqlite3");
    let mut database = Database::open_with_options(
        &path,
        DatabaseOptions {
            busy_timeout_ms: 50,
            require_wal: true,
        },
    )
    .unwrap();
    let competing = Connection::open(&path).unwrap();
    competing.execute_batch("BEGIN IMMEDIATE;").unwrap();

    assert!(matches!(
        database.migrate().unwrap_err(),
        StorageError::DatabaseBusy
    ));
    assert_eq!(database.schema_version().unwrap(), 0);

    competing.execute_batch("ROLLBACK;").unwrap();
    let report = database.migrate().unwrap();
    assert_eq!(report.applied, vec![1]);
}

#[test]
fn non_directory_parent_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("not-a-directory");
    fs::write(&parent, b"x").unwrap();
    assert!(matches!(
        Database::open(parent.join("db.sqlite3")),
        Err(StorageError::NotDirectory(_))
    ));
}

#[cfg(unix)]
#[test]
fn symlinked_database_is_rejected() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target.sqlite3");
    fs::write(&target, b"").unwrap();
    let link = root.path().join("link.sqlite3");
    symlink(&target, &link).unwrap();
    assert!(matches!(
        Database::open(&link),
        Err(StorageError::Symlink(_))
    ));
}

#[cfg(unix)]
#[test]
fn database_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let (_root, database) = temp_database();
    assert_eq!(
        fs::metadata(database.path()).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn unreadable_directory_is_reported() {
    use std::os::unix::fs::PermissionsExt;
    if std::process::Command::new("id")
        .arg("-u")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "0")
        .unwrap_or(false)
    {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o500)).unwrap();
    let result = Database::open(directory.join("db.sqlite3"));
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(result, Err(StorageError::Io { .. })));
}

#[test]
fn workspace_layout_and_terminal_session_metadata_roundtrip() {
    let (_root, mut database) = temp_database();
    database.migrate().unwrap();
    let connection = database.connection();
    save_workspace_layout(
        connection,
        "default",
        "{\"version\":1,\"tabs\":[],\"active_tab_id\":null}",
        "host-1",
        "start-1",
        42,
    )
    .unwrap();
    let layout = get_workspace_layout(connection, "default")
        .unwrap()
        .expect("layout");
    assert_eq!(layout.host_instance_id.as_deref(), Some("host-1"));
    assert_eq!(layout.process_start_token.as_deref(), Some("start-1"));
    assert_eq!(layout.updated_ms, 42);

    record_terminal_session(
        connection,
        "session-1",
        "host-1",
        "native-mac",
        "/tmp",
        "/bin/zsh",
        50,
    )
    .unwrap();
    assert!(
        update_terminal_session_state(connection, "session-1", TerminalSessionState::Exited, 51)
            .unwrap()
    );
    let state: String = connection
        .query_row(
            "SELECT state FROM terminal_sessions WHERE id = 'session-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "exited");
}
