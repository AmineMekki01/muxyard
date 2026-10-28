//! Project registration persistence and trust defaults.

use muxyard_storage::{
    Database, ProjectTrust, list_projects, register_project, remove_project, touch_project,
};

#[test]
fn project_registration_is_recent_first_and_defaults_untrusted() {
    let root = tempfile::tempdir().unwrap();
    let mut database = Database::open(root.path().join("muxyard.sqlite3")).unwrap();
    database.migrate().unwrap();
    let connection = database.connection();

    let first =
        register_project(connection, "p1", "one", "/work/one", "native-mac", None, 10).unwrap();
    assert_eq!(first.trust, ProjectTrust::Untrusted);
    assert_eq!(first.created_ms, 10);

    let second = register_project(
        connection,
        "p2",
        "two",
        "/work/two",
        "native-mac",
        Some("repo-two"),
        20,
    )
    .unwrap();
    assert_eq!(second.repo_id.as_deref(), Some("repo-two"));

    let projects = list_projects(connection).unwrap();
    assert_eq!(
        projects
            .iter()
            .map(|project| project.id.as_str())
            .collect::<Vec<_>>(),
        vec!["p2", "p1"]
    );

    assert!(touch_project(connection, "p1", 30).unwrap());
    let projects = list_projects(connection).unwrap();
    assert_eq!(projects[0].id, "p1");
    assert_eq!(projects[0].last_opened_ms, 30);

    assert!(remove_project(connection, "p2").unwrap());
    assert_eq!(list_projects(connection).unwrap().len(), 1);
}

#[test]
fn registering_the_same_root_reuses_the_project_and_preserves_trust() {
    let root = tempfile::tempdir().unwrap();
    let mut database = Database::open(root.path().join("muxyard.sqlite3")).unwrap();
    database.migrate().unwrap();
    let connection = database.connection();

    let first =
        register_project(connection, "p1", "one", "/work/one", "native-mac", None, 10).unwrap();
    connection
        .execute(
            "UPDATE projects SET trust_state = 'trusted' WHERE id = 'p1'",
            [],
        )
        .unwrap();

    let second = register_project(
        connection,
        "replacement",
        "renamed",
        "/work/one",
        "native-mac",
        Some("repo"),
        20,
    )
    .unwrap();
    assert_eq!(second.id, first.id);
    assert_eq!(second.name, "renamed");
    assert_eq!(second.trust, ProjectTrust::Trusted);
    assert_eq!(list_projects(connection).unwrap().len(), 1);
}
