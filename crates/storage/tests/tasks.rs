//! Durable task lifecycle, optimistic versions, links, and audit events.

use muxyard_contracts::TaskState;
use muxyard_storage::{
    Database, TaskError, create_task, get_task, link_agent, link_session, list_audit_events,
    list_tasks, update_task,
};
use rusqlite::params;

fn database() -> (tempfile::TempDir, Database) {
    let root = tempfile::tempdir().unwrap();
    let mut database = Database::open(root.path().join("muxyard.sqlite3")).unwrap();
    database.migrate().unwrap();
    (root, database)
}

fn project(connection: &rusqlite::Connection, id: &str) {
    connection
        .execute(
            "INSERT INTO projects (
                id, name, root_path, execution_environment_id, repo_id,
                trust_state, created_ms, last_opened_ms
             ) VALUES (?1, 'project', ?2, 'native-mac', NULL, 'untrusted', 1, 1)",
            params![id, format!("/tmp/{id}")],
        )
        .unwrap();
}

#[test]
fn task_lifecycle_persists_and_audits_valid_transitions() {
    let (_root, database) = database();
    let connection = database.connection();
    project(connection, "p1");

    let mut task = create_task(
        connection,
        "t1",
        "p1",
        "Implement login",
        "Make the login flow reliable",
        &["no schema changes".to_string()],
        Some("abc123"),
        10,
    )
    .unwrap();
    assert_eq!(task.state, TaskState::Draft);
    assert_eq!(task.state_version, 1);
    assert_eq!(task.constraints, vec!["no schema changes"]);

    task = update_task(
        connection,
        "t1",
        1,
        None,
        None,
        None,
        Some(TaskState::Ready),
        "user",
        20,
    )
    .unwrap();
    assert_eq!(task.state, TaskState::Ready);
    assert_eq!(task.state_version, 2);

    for (version, state) in [
        (2, TaskState::Running),
        (3, TaskState::Review),
        (4, TaskState::Approved),
        (5, TaskState::Integrated),
    ] {
        task = update_task(
            connection,
            "t1",
            version,
            None,
            None,
            None,
            Some(state),
            "user",
            30 + version,
        )
        .unwrap();
        assert_eq!(task.state, state);
    }

    let audit = list_audit_events(connection, "t1").unwrap();
    assert_eq!(audit.len(), 5);
    assert_eq!(audit.last().unwrap().from_state, TaskState::Draft);
    assert_eq!(audit.last().unwrap().to_state, TaskState::Ready);
    assert_eq!(list_tasks(connection, Some("p1")).unwrap().len(), 1);
}

#[test]
fn invalid_transition_and_stale_version_are_rejected() {
    let (_root, database) = database();
    let connection = database.connection();
    project(connection, "p1");
    create_task(connection, "t1", "p1", "Task", "", &[], None, 10).unwrap();

    assert!(matches!(
        update_task(
            connection,
            "t1",
            1,
            None,
            None,
            None,
            Some(TaskState::Running),
            "user",
            20
        ),
        Err(TaskError::InvalidTransition {
            from: TaskState::Draft,
            to: TaskState::Running
        })
    ));

    update_task(
        connection,
        "t1",
        1,
        None,
        None,
        None,
        Some(TaskState::Ready),
        "user",
        20,
    )
    .unwrap();

    assert!(matches!(
        update_task(
            connection,
            "t1",
            1,
            None,
            None,
            None,
            Some(TaskState::Running),
            "user",
            30
        ),
        Err(TaskError::Conflict {
            expected: 1,
            actual: 2,
            ..
        })
    ));
    assert_eq!(
        get_task(connection, "t1").unwrap().unwrap().state,
        TaskState::Ready
    );
}

#[test]
fn task_links_are_persisted_and_project_deletion_is_restricted() {
    let (_root, database) = database();
    let connection = database.connection();
    project(connection, "p1");
    create_task(connection, "t1", "p1", "Task", "", &[], None, 10).unwrap();
    connection
        .execute(
            "INSERT INTO terminal_sessions (
                id, project_id, task_id, host_instance_id, execution_environment_id,
                cwd, shell_executable, state, created_ms, updated_ms, last_event_seq
             ) VALUES ('s1', 'p1', 't1', 'host', 'native-mac', '/tmp', '/bin/sh', 'running', 1, 1, 0)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO agent_sessions (
                id, task_id, terminal_session_id, provider, adapter_version,
                external_conversation_id, state, state_confidence, capabilities_json,
                last_event_seq, created_ms, updated_ms
             ) VALUES ('a1', 't1', 's1', 'mock', '1', NULL, 'idle', 'observed', '[]', 0, 1, 1)",
            [],
        )
        .unwrap();

    link_session(connection, "t1", "s1", 20).unwrap();
    link_agent(connection, "t1", "a1", "implementer", 20).unwrap();
    let sessions: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM task_sessions WHERE task_id = 't1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let agents: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM task_agents WHERE task_id = 't1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!((sessions, agents), (1, 1));

    assert!(
        connection
            .execute("DELETE FROM projects WHERE id = 'p1'", [])
            .is_err()
    );
}
