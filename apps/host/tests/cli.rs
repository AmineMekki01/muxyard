//! Exercise the real host executable, including initialization and failures.

use std::{fs, path::Path, process::Command};

use muxyard_storage::Database;

fn host() -> Command {
    Command::new(env!("CARGO_BIN_EXE_muxyard-host"))
}

fn isolated_host(root: &Path) -> Command {
    let mut command = host();
    command
        .env("MUXYARD_DATA_DIR", root.join("data"))
        .env("MUXYARD_CONFIG_DIR", root.join("config"))
        .env("MUXYARD_RUNTIME_DIR", root.join("runtime"))
        .env("USER", "test-user");
    command
}

#[test]
fn version_reports_semver_and_protocol_on_repeated_starts() {
    for _ in 0..2 {
        let output = host().arg("--version").output().expect("host should start");
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).expect("UTF-8 version"),
            format!("muxyard-host {} (protocol 1)\n", env!("CARGO_PKG_VERSION"))
        );
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn no_arguments_and_help_disclose_that_no_services_start() {
    for args in [vec![], vec!["--help"], vec!["-h"]] {
        let output = host().args(args).output().expect("host should start");
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
        assert!(stdout.contains("No services are started."));
        assert!(stdout.contains("--init"));
    }
}

#[test]
fn unsupported_or_extra_arguments_fail_without_starting_services() {
    for args in [vec!["--serve"], vec!["--version", "--serve"]] {
        let output = host().args(args).output().expect("host should start");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("UTF-8 diagnostic")
                .contains("Run muxyard-host --help")
        );
    }
}

#[test]
fn init_creates_storage_and_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        let output = isolated_host(root.path())
            .arg("--init")
            .output()
            .expect("host should start");
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains("Local storage initialized (schema 1).")
        );
    }

    let database_path = root.path().join("data/muxyard.sqlite3");
    assert!(database_path.is_file());
    assert!(root.path().join("config/config.json").is_file());
    assert!(root.path().join("data/logs/muxyard.log").is_file());

    let database = Database::open(&database_path).unwrap();
    assert_eq!(database.schema_version().unwrap(), 1);
    let migrations: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(migrations, 1);
    let instances: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM host_instances", [], |row| row.get(0))
        .unwrap();
    assert_eq!(instances, 2);
    let states: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM host_instances WHERE state = 'stopped'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(states, 2);

    let log = fs::read_to_string(root.path().join("data/logs/muxyard.log")).unwrap();
    assert!(log.contains("host bootstrap complete"));
    assert!(!log.contains("test-user"));
}

#[test]
fn init_rejects_malformed_config_without_overwriting_it() {
    let root = tempfile::tempdir().unwrap();
    let config_dir = root.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let config_path = config_dir.join("config.json");
    let original = br#"{ "secret": "do-not-print", broken"#;
    fs::write(&config_path, original).unwrap();

    let output = isolated_host(root.path())
        .arg("--init")
        .output()
        .expect("host should start");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Initialization failed"));
    assert!(!stderr.contains("do-not-print"));
    assert_eq!(fs::read(&config_path).unwrap(), original);
    assert!(!root.path().join("data/muxyard.sqlite3").exists());
}

#[cfg(unix)]
#[test]
fn init_reports_an_unwritable_data_parent() {
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
    let readonly = root.path().join("readonly");
    fs::create_dir(&readonly).unwrap();
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o500)).unwrap();

    let mut command = host();
    command
        .env("MUXYARD_DATA_DIR", readonly.join("data"))
        .env("MUXYARD_CONFIG_DIR", root.path().join("config"))
        .env("MUXYARD_RUNTIME_DIR", root.path().join("runtime"))
        .env("USER", "test-user");
    let output = command.arg("--init").output().expect("host should start");
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Initialization failed"));
    assert!(!readonly.join("data").exists());
}
