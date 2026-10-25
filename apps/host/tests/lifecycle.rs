//! Exercise the real detached host lifecycle and stale-record reconciliation.

use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use muxyard_ipc::IpcClient;
use muxyard_platform::PlatformPaths;
use muxyard_storage::Database;
use serde_json::Value;

struct HostGuard {
    root: PathBuf,
    _temp: tempfile::TempDir,
}

impl HostGuard {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        Self {
            root: temp.path().to_path_buf(),
            _temp: temp,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_muxyard-host"));
        command
            .env("MUXYARD_DATA_DIR", self.root.join("data"))
            .env("MUXYARD_CONFIG_DIR", self.root.join("config"))
            .env("MUXYARD_RUNTIME_DIR", self.root.join("runtime"))
            .env("USER", "test-user")
            .env("USERNAME", "test-user");
        command
    }

    fn output(&self, args: &[&str]) -> Output {
        self.command()
            .args(args)
            .output()
            .expect("host should start")
    }

    fn status_json(&self) -> (i32, Value) {
        let output = self.output(&["status", "--json"]);
        let value = serde_json::from_slice(&output.stdout).expect("status JSON");
        (output.status.code().unwrap_or(-1), value)
    }

    fn start(&self) -> Output {
        self.output(&["start"])
    }

    fn stop(&self) -> Output {
        self.output(&["stop"])
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join("runtime/host.lock")
    }

    fn database_path(&self) -> PathBuf {
        self.root.join("data/muxyard.sqlite3")
    }
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        let _ = self.output(&["stop"]);
    }
}

fn wait_for_status(
    host: &HostGuard,
    predicate: impl Fn(&Value) -> bool,
    timeout: Duration,
) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
        let (_, value) = host.status_json();
        if predicate(&value) {
            return value;
        }
        if Instant::now() >= deadline {
            panic!("status did not reach expected state: {value}");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn status_pid(host: &HostGuard) -> u32 {
    let value = wait_for_status(
        host,
        |value| value["running"] == true && value["ready"] == true,
        Duration::from_secs(10),
    );
    value["pid"].as_u64().unwrap() as u32
}

#[cfg(unix)]
fn process_group(pid: u32) -> u32 {
    let output = Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap()
}

#[test]
fn start_is_detached_and_single_instance() {
    let host = HostGuard::new();
    let started = host.start();
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    assert!(String::from_utf8_lossy(&started.stdout).contains("Host started"));

    let pid = status_pid(&host);
    assert!(host.lock_path().is_file());
    let (_, first) = host.status_json();
    assert_eq!(first["running"], true);
    assert_eq!(first["ready"], true);
    assert!(first["process_start"].as_str().unwrap().len() > 5);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(host.root.join("runtime"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(host.lock_path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(process_group(pid), pid);
    }

    let second = host.start();
    assert!(second.status.success());
    assert!(String::from_utf8_lossy(&second.stdout).contains("already running"));
    let (_, still_running) = host.status_json();
    assert_eq!(still_running["pid"].as_u64().unwrap() as u32, pid);

    let stopped = host.stop();
    assert!(stopped.status.success());
    let (code, stopped_status) = host.status_json();
    assert_eq!(code, 1);
    assert_eq!(stopped_status["running"], false);
    assert!(!host.lock_path().exists());
}

#[test]
fn stop_records_stopped_state_and_cleans_markers() {
    let host = HostGuard::new();
    assert!(host.start().status.success());
    status_pid(&host);
    let stopped = host.stop();
    assert!(stopped.status.success());
    let (code, status) = host.status_json();
    assert_eq!(code, 1);
    assert_eq!(status["running"], false);
    assert!(!host.lock_path().exists());
    assert!(!host.root.join("runtime/host.ready").exists());
    assert!(!host.root.join("runtime/host.stop").exists());

    let database = Database::open(host.database_path()).unwrap();
    let state: String = database
        .connection()
        .query_row(
            "SELECT state FROM host_instances ORDER BY started_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "stopped");
}

#[test]
fn stale_lock_is_reported_and_replaced() {
    let host = HostGuard::new();
    fs::create_dir_all(host.root.join("runtime")).unwrap();
    let stale = serde_json::json!({
        "schema_version": 1,
        "instance_id": "stale",
        "pid": u32::MAX,
        "process_start": "dead-start",
        "started_ms": 1,
        "host_version": "0.1.0",
        "protocol_version": 1
    });
    fs::write(host.lock_path(), serde_json::to_vec(&stale).unwrap()).unwrap();
    let (code, status) = host.status_json();
    assert_eq!(code, 1);
    assert_eq!(status["stale"], true);

    assert!(host.start().status.success());
    status_pid(&host);
    assert!(host.stop().status.success());
}

#[test]
fn corrupt_lock_fails_closed_without_overwriting() {
    let host = HostGuard::new();
    fs::create_dir_all(host.root.join("runtime")).unwrap();
    let original = b"{not a lock";
    fs::write(host.lock_path(), original).unwrap();
    let status = host.output(&["status", "--json"]);
    assert_eq!(status.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&status.stderr).contains("corrupt"));
    let started = host.start();
    assert_eq!(started.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&started.stderr).contains("corrupt"));
    assert_eq!(fs::read(host.lock_path()).unwrap(), original);

    fs::remove_file(host.lock_path()).unwrap();
    assert!(host.start().status.success());
    assert!(host.stop().status.success());
}

#[test]
fn malformed_config_reports_startup_failure_and_releases_lock() {
    let host = HostGuard::new();
    fs::create_dir_all(host.root.join("config")).unwrap();
    fs::write(host.root.join("config/config.json"), b"{broken").unwrap();
    let started = host.start();
    assert_eq!(started.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&started.stderr);
    assert!(stderr.contains("Host failed to start"));
    assert!(stderr.contains("malformed") || stderr.contains("startup"));
    assert!(!host.lock_path().exists());
    let (code, _) = host.status_json();
    assert_eq!(code, 1);
}

#[test]
fn second_foreground_run_reports_already_running() {
    let host = HostGuard::new();
    assert!(host.start().status.success());
    let pid = status_pid(&host);
    let second = host.output(&["run"]);
    assert!(second.status.success());
    assert!(String::from_utf8_lossy(&second.stdout).contains("already running"));
    let (_, status) = host.status_json();
    assert_eq!(status["pid"].as_u64().unwrap() as u32, pid);
    assert!(host.stop().status.success());
}

#[test]
fn init_fails_while_host_is_running() {
    let host = HostGuard::new();
    assert!(host.start().status.success());
    status_pid(&host);
    let init = host.output(&["--init"]);
    assert_eq!(init.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&init.stderr).contains("already running"));
    let (_, status) = host.status_json();
    assert_eq!(status["running"], true);
    assert!(host.stop().status.success());
}

#[test]
fn status_and_stop_are_safe_without_a_host() {
    let host = HostGuard::new();
    let (code, status) = host.status_json();
    assert_eq!(code, 1);
    assert_eq!(status["running"], false);
    let stopped = host.stop();
    assert!(stopped.status.success());
    assert!(String::from_utf8_lossy(&stopped.stdout).contains("not running"));
}

#[test]
fn old_host_sessions_are_reconciled_as_lost() {
    let host = HostGuard::new();
    assert!(host.output(&["--init"]).status.success());
    {
        let database = Database::open(host.database_path()).unwrap();
        database
            .connection()
            .execute(
                "INSERT INTO host_instances (
                    id, pid, os_user, process_start_token, host_version,
                    protocol_version, state, started_ms, heartbeat_ms
                 ) VALUES ('old-host', 10, 'test-user', 'old-start', '0.1.0', 1, 'ready', 1, 1)",
                [],
            )
            .unwrap();
        database
            .connection()
            .execute(
                "INSERT INTO terminal_sessions (
                    id, project_id, task_id, host_instance_id, execution_environment_id,
                    cwd, shell_executable, state, created_ms, updated_ms, last_event_seq
                 ) VALUES ('old-session', NULL, NULL, 'old-host', 'native-mac',
                    '/tmp', '/bin/zsh', 'running', 1, 1, 0)",
                [],
            )
            .unwrap();
    }
    assert!(host.start().status.success());
    status_pid(&host);
    assert!(host.stop().status.success());
    let database = Database::open(host.database_path()).unwrap();
    let old_state: String = database
        .connection()
        .query_row(
            "SELECT state FROM host_instances WHERE id = 'old-host'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let session_state: String = database
        .connection()
        .query_row(
            "SELECT state FROM terminal_sessions WHERE id = 'old-session'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(old_state, "stopped");
    assert_eq!(session_state, "lost");
}

#[test]
fn stop_command_stops_a_foreground_host() {
    let host = HostGuard::new();
    let mut child = host
        .command()
        .arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    status_pid(&host);
    let stopped = host.stop();
    assert!(stopped.status.success());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "foreground host did not exit");
        thread::sleep(Duration::from_millis(50));
    }
    let (code, _) = host.status_json();
    assert_eq!(code, 1);
}

#[cfg(unix)]
#[test]
fn sigint_stops_foreground_host_gracefully() {
    let host = HostGuard::new();
    let mut child = host
        .command()
        .arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = status_pid(&host);
    assert_eq!(child.id(), pid);
    let signal = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .unwrap();
    assert!(signal.success());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "host did not exit after SIGINT");
        thread::sleep(Duration::from_millis(50));
    }
    let (code, _) = host.status_json();
    assert_eq!(code, 1);
    assert!(!host.lock_path().exists());
}

#[cfg(unix)]
#[test]
fn sigterm_stops_foreground_host_gracefully() {
    let host = HostGuard::new();
    let mut child = host
        .command()
        .arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = status_pid(&host);
    let signal = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .unwrap();
    assert!(signal.success());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "host did not exit after SIGTERM");
        thread::sleep(Duration::from_millis(50));
    }
    let (code, _) = host.status_json();
    assert_eq!(code, 1);
}

#[cfg(unix)]
#[test]
fn killed_host_sessions_are_marked_lost_on_restart() {
    let host = HostGuard::new();
    assert!(host.start().status.success());
    let status = wait_for_status(
        &host,
        |value| value["running"] == true && value["ready"] == true,
        Duration::from_secs(10),
    );
    let pid = status["pid"].as_u64().unwrap() as u32;
    let instance_id = status["instance_id"].as_str().unwrap().to_string();
    {
        let database = Database::open(host.database_path()).unwrap();
        database
            .connection()
            .execute(
                "INSERT INTO terminal_sessions (
                    id, project_id, task_id, host_instance_id, execution_environment_id,
                    cwd, shell_executable, state, created_ms, updated_ms, last_event_seq
                 ) VALUES ('crashed-session', NULL, NULL, ?1, 'native-mac',
                    '/tmp', '/bin/zsh', 'running', 1, 1, 0)",
                [&instance_id],
            )
            .unwrap();
    }
    let killed = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let (code, _) = host.status_json();
        if code == 1 {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(host.start().status.success());
    status_pid(&host);
    assert!(host.stop().status.success());
    let database = Database::open(host.database_path()).unwrap();
    let session_state: String = database
        .connection()
        .query_row(
            "SELECT state FROM terminal_sessions WHERE id = 'crashed-session'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(session_state, "lost");
}

#[test]
fn local_ipc_reports_host_status_and_closes_with_host() {
    let host = HostGuard::new();
    assert!(host.start().status.success());
    let pid = status_pid(&host);
    let (_, lock_status) = host.status_json();
    let instance_id = lock_status["instance_id"].as_str().unwrap().to_string();
    let paths = PlatformPaths::from_root(&host.root);

    let mut client = IpcClient::connect(&paths).unwrap();
    let status = client.host_status().unwrap();
    assert_eq!(status.instance_id, instance_id);
    assert_eq!(status.pid, pid);
    assert_eq!(status.protocol_version, 1);
    assert!(status.ready);
    assert_eq!(status.host_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(
        status.started_ms,
        lock_status["started_ms"].as_i64().unwrap()
    );
    drop(client);

    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};
        let socket_path = paths.local_socket_path();
        assert_eq!(
            fs::metadata(&socket_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(fs::metadata(&socket_path).unwrap().file_type().is_socket());
    }

    assert!(host.stop().status.success());
    assert!(IpcClient::connect(&paths).is_err());
    #[cfg(unix)]
    assert!(!paths.local_socket_path().exists());
}

#[cfg(unix)]
#[test]
fn stop_marker_for_another_instance_is_ignored() {
    let host = HostGuard::new();
    assert!(host.start().status.success());
    let pid = status_pid(&host);
    let stop_marker = host.root.join("runtime/host.stop");
    fs::write(
        &stop_marker,
        serde_json::to_vec(&serde_json::json!({ "instance_id": "another" })).unwrap(),
    )
    .unwrap();
    thread::sleep(Duration::from_millis(500));
    let (_, status) = host.status_json();
    assert_eq!(status["pid"].as_u64().unwrap() as u32, pid);
    assert!(host.stop().status.success());
}
