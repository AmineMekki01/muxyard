//! Exercise typed host terminal control and binary output.

#![cfg(unix)]

use std::{
    path::Path,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use muxyard_contracts::{
    PROTOCOL_VERSION, TerminalCloseRequest, TerminalInputRequest, TerminalResizeRequest,
    TerminalSnapshotRequest, TerminalSnapshotResponse, TerminalSpawnRequest, TerminalSpawnResponse,
};
use muxyard_ipc::{
    HostStatusResponse, IpcClient, IpcOptions, IpcResponse, IpcServer, TERMINAL_CLOSE_METHOD,
    TERMINAL_INPUT_METHOD, TERMINAL_RESIZE_METHOD, TERMINAL_SNAPSHOT_METHOD, TERMINAL_SPAWN_METHOD,
};
use muxyard_platform::PlatformPaths;
use muxyard_terminal::{TerminalHub, TerminalLimits, TerminalService};
use uuid::Uuid;

fn sample_status() -> HostStatusResponse {
    HostStatusResponse {
        host_version: "0.1.0".to_string(),
        protocol_version: PROTOCOL_VERSION,
        instance_id: "terminal-control-test".to_string(),
        pid: 42,
        process_start: "start-test".to_string(),
        started_ms: 1,
        ready: true,
    }
}

fn test_paths() -> (tempfile::TempDir, PlatformPaths) {
    let temp = tempfile::tempdir().unwrap();
    let paths = PlatformPaths::from_root(temp.path());
    paths.ensure().unwrap();
    (temp, paths)
}

fn spawn_request(cwd: &std::path::Path) -> TerminalSpawnRequest {
    TerminalSpawnRequest {
        cwd: Some(cwd.to_string_lossy().into_owned()),
        shell: Some("/bin/sh".to_string()),
        history_id: None,
        project_id: None,
        rows: 24,
        cols: 80,
    }
}

#[test]
fn typed_spawn_input_resize_attach_and_close_work() {
    let (temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let service = Arc::new(TerminalService::new(hub));
    let mut server = IpcServer::start_with_terminal_service_and_options(
        &paths,
        sample_status(),
        Arc::clone(&service),
        IpcOptions {
            request_timeout: Duration::from_secs(2),
        },
    )
    .unwrap();

    let mut control = IpcClient::connect(&paths).unwrap();
    let spawned: IpcResponse<TerminalSpawnResponse> = control
        .request(TERMINAL_SPAWN_METHOD, spawn_request(temp.path()))
        .unwrap();
    let spawned = match spawned {
        IpcResponse::Ok { data } => data,
        IpcResponse::Error { error } => panic!("spawn failed: {error}"),
    };
    let stream_id = Uuid::parse_str(&spawned.stream_id.0).unwrap();
    assert_eq!(spawned.rows, 24);
    assert_eq!(spawned.cols, 80);

    let mut terminal = IpcClient::connect(&paths)
        .unwrap()
        .attach_terminal(stream_id, None)
        .unwrap();
    let resize: IpcResponse<()> = control
        .request(
            TERMINAL_RESIZE_METHOD,
            TerminalResizeRequest {
                stream_id: spawned.stream_id.clone(),
                rows: 31,
                cols: 101,
            },
        )
        .unwrap();
    assert!(matches!(resize, IpcResponse::Ok { .. }));

    let input: IpcResponse<()> = control
        .request(
            TERMINAL_INPUT_METHOD,
            TerminalInputRequest {
                stream_id: spawned.stream_id.clone(),
                data: "printf 'ipc-terminal-ok\\n'\n".to_string(),
            },
        )
        .unwrap();
    assert!(matches!(input, IpcResponse::Ok { .. }));

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = Vec::new();
    while Instant::now() < deadline {
        let frame = terminal.next_frame().unwrap();
        output.extend_from_slice(frame.payload());
        if output
            .windows(b"ipc-terminal-ok".len())
            .any(|window| window == b"ipc-terminal-ok")
        {
            break;
        }
    }
    assert!(
        String::from_utf8_lossy(&output).contains("ipc-terminal-ok"),
        "missing terminal output: {:?}",
        String::from_utf8_lossy(&output)
    );

    let closed: IpcResponse<()> = control
        .request(
            TERMINAL_CLOSE_METHOD,
            TerminalCloseRequest {
                stream_id: spawned.stream_id,
            },
        )
        .unwrap();
    assert!(matches!(closed, IpcResponse::Ok { .. }));
    assert_eq!(service.session_count(), 0);

    drop(terminal);
    drop(control);
    server.stop();
}

#[test]
fn snapshot_returns_recent_output_for_reattachment() {
    let (temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let service = Arc::new(TerminalService::new(hub));
    let mut server =
        IpcServer::start_with_terminal_service(&paths, sample_status(), Arc::clone(&service))
            .unwrap();
    let mut control = IpcClient::connect(&paths).unwrap();
    let spawned: IpcResponse<TerminalSpawnResponse> = control
        .request(
            TERMINAL_SPAWN_METHOD,
            TerminalSpawnRequest {
                cwd: Some(temp.path().to_string_lossy().into_owned()),
                shell: Some("/bin/sh".to_string()),
                history_id: None,
                project_id: None,
                rows: 24,
                cols: 80,
            },
        )
        .unwrap();
    let spawned = match spawned {
        IpcResponse::Ok { data } => data,
        IpcResponse::Error { error } => panic!("spawn failed: {error}"),
    };
    let stream_id = Uuid::parse_str(&spawned.stream_id.0).unwrap();
    let mut terminal = IpcClient::connect(&paths)
        .unwrap()
        .attach_terminal(stream_id, None)
        .unwrap();
    let input: IpcResponse<()> = control
        .request(
            TERMINAL_INPUT_METHOD,
            TerminalInputRequest {
                stream_id: spawned.stream_id.clone(),
                data: "printf 'muxyard-snapshot-marker\\n'\\n".to_string(),
            },
        )
        .unwrap();
    assert!(matches!(input, IpcResponse::Ok { .. }));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = Vec::new();
    while Instant::now() < deadline {
        let frame = terminal.next_frame().unwrap();
        output.extend_from_slice(frame.payload());
        if String::from_utf8_lossy(&output).contains("muxyard-snapshot-marker") {
            break;
        }
    }
    assert!(String::from_utf8_lossy(&output).contains("muxyard-snapshot-marker"));
    let snapshot: IpcResponse<TerminalSnapshotResponse> = control
        .request(
            TERMINAL_SNAPSHOT_METHOD,
            TerminalSnapshotRequest {
                stream_id: spawned.stream_id.clone(),
            },
        )
        .unwrap();
    match snapshot {
        IpcResponse::Ok { data } => assert!(
            String::from_utf8_lossy(&data.data).contains("muxyard-snapshot-marker"),
            "snapshot did not contain recent output"
        ),
        IpcResponse::Error { error } => panic!("snapshot failed: {error}"),
    }
    let closed: IpcResponse<()> = control
        .request(
            TERMINAL_CLOSE_METHOD,
            TerminalCloseRequest {
                stream_id: spawned.stream_id,
            },
        )
        .unwrap();
    assert!(matches!(closed, IpcResponse::Ok { .. }));
    drop(terminal);
    drop(control);
    server.stop();
}

#[test]
fn history_ids_map_to_distinct_private_history_files() {
    let (temp, paths) = test_paths();
    let history_dir = temp.path().join("history");
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let service = Arc::new(TerminalService::with_history(hub, history_dir.clone()));
    let mut server =
        IpcServer::start_with_terminal_service(&paths, sample_status(), Arc::clone(&service))
            .unwrap();
    let mut control = IpcClient::connect(&paths).unwrap();

    let mut histories = Vec::new();
    for history_id in ["pane-one", "pane-two"] {
        let spawned: IpcResponse<TerminalSpawnResponse> = control
            .request(
                TERMINAL_SPAWN_METHOD,
                TerminalSpawnRequest {
                    cwd: Some(temp.path().to_string_lossy().into_owned()),
                    shell: Some("/bin/sh".to_string()),
                    history_id: Some(muxyard_contracts::Id(history_id.to_string())),
                    project_id: None,
                    rows: 24,
                    cols: 80,
                },
            )
            .unwrap();
        let spawned = match spawned {
            IpcResponse::Ok { data } => data,
            IpcResponse::Error { error } => panic!("spawn failed: {error}"),
        };
        let stream_id = Uuid::parse_str(&spawned.stream_id.0).unwrap();
        let mut terminal = IpcClient::connect(&paths)
            .unwrap()
            .attach_terminal(stream_id, None)
            .unwrap();
        let input: IpcResponse<()> = control
            .request(
                TERMINAL_INPUT_METHOD,
                TerminalInputRequest {
                    stream_id: spawned.stream_id.clone(),
                    data: "printf '%s\n' \"$HISTFILE\"\n".to_string(),
                },
            )
            .unwrap();
        assert!(matches!(input, IpcResponse::Ok { .. }));
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        while Instant::now() < deadline {
            let frame = terminal.next_frame().unwrap();
            output.extend_from_slice(frame.payload());
            let text = String::from_utf8_lossy(&output);
            if text.contains(history_id) {
                histories.push(text.into_owned());
                break;
            }
        }
        let closed: IpcResponse<()> = control
            .request(
                TERMINAL_CLOSE_METHOD,
                TerminalCloseRequest {
                    stream_id: spawned.stream_id,
                },
            )
            .unwrap();
        assert!(matches!(closed, IpcResponse::Ok { .. }));
    }

    assert_eq!(histories.len(), 2);
    assert!(histories[0].contains("pane-one.history"));
    assert!(histories[1].contains("pane-two.history"));
    assert_ne!(histories[0], histories[1]);
    assert!(history_dir.is_dir());
    drop(control);
    server.stop();
}

#[test]
fn zsh_writes_commands_to_the_pane_history_file() {
    if !Path::new("/bin/zsh").exists() {
        return;
    }
    let (temp, paths) = test_paths();
    let history_dir = temp.path().join("history");
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let service = Arc::new(TerminalService::with_history(hub, history_dir.clone()));
    let mut server =
        IpcServer::start_with_terminal_service(&paths, sample_status(), Arc::clone(&service))
            .unwrap();
    let mut control = IpcClient::connect(&paths).unwrap();
    let spawned: IpcResponse<TerminalSpawnResponse> = control
        .request(
            TERMINAL_SPAWN_METHOD,
            TerminalSpawnRequest {
                cwd: Some(temp.path().to_string_lossy().into_owned()),
                shell: Some("/bin/zsh".to_string()),
                history_id: Some(muxyard_contracts::Id("pane-zsh".to_string())),
                project_id: None,
                rows: 24,
                cols: 80,
            },
        )
        .unwrap();
    let spawned = match spawned {
        IpcResponse::Ok { data } => data,
        IpcResponse::Error { error } => panic!("spawn failed: {error}"),
    };
    let stream_id = Uuid::parse_str(&spawned.stream_id.0).unwrap();
    let mut terminal = IpcClient::connect(&paths)
        .unwrap()
        .attach_terminal(stream_id, None)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut startup = Vec::new();
    while Instant::now() < deadline {
        let frame = terminal.next_frame().unwrap();
        startup.extend_from_slice(frame.payload());
        if String::from_utf8_lossy(&startup).contains('%') {
            break;
        }
    }
    let input: IpcResponse<()> = control
        .request(
            TERMINAL_INPUT_METHOD,
            TerminalInputRequest {
                stream_id: spawned.stream_id.clone(),
                data: "echo muxyard-history-marker\n".to_string(),
            },
        )
        .unwrap();
    assert!(matches!(input, IpcResponse::Ok { .. }));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = Vec::new();
    while Instant::now() < deadline {
        let frame = terminal.next_frame().unwrap();
        output.extend_from_slice(frame.payload());
        if String::from_utf8_lossy(&output).contains("muxyard-history-marker") {
            break;
        }
    }
    let input: IpcResponse<()> = control
        .request(
            TERMINAL_INPUT_METHOD,
            TerminalInputRequest {
                stream_id: spawned.stream_id.clone(),
                data: "exit\n".to_string(),
            },
        )
        .unwrap();
    assert!(matches!(input, IpcResponse::Ok { .. }));
    let path = history_dir.join("pane-zsh.history");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut content = String::new();
    while Instant::now() < deadline {
        if let Ok(next) = std::fs::read_to_string(&path) {
            content = next;
            if content.contains("muxyard-history-marker") {
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(
        content.contains("muxyard-history-marker"),
        "zsh history did not contain the command: {content:?}"
    );
    let _ = control.request::<_, IpcResponse<()>>(
        TERMINAL_CLOSE_METHOD,
        TerminalCloseRequest {
            stream_id: spawned.stream_id,
        },
    );
    drop(terminal);
    drop(control);
    server.stop();
}

#[test]
fn unknown_terminal_control_session_returns_not_found() {
    let (_temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let service = Arc::new(TerminalService::new(hub));
    let mut server =
        IpcServer::start_with_terminal_service(&paths, sample_status(), Arc::clone(&service))
            .unwrap();
    let mut control = IpcClient::connect(&paths).unwrap();
    let id = muxyard_contracts::Id(Uuid::new_v4().to_string());
    let response: IpcResponse<()> = control
        .request(
            TERMINAL_INPUT_METHOD,
            TerminalInputRequest {
                stream_id: id,
                data: "x".to_string(),
            },
        )
        .unwrap();
    assert!(matches!(
        response,
        IpcResponse::Error { error }
            if error.code == muxyard_contracts::ErrorCode::NotFound
    ));
    drop(control);
    server.stop();
}

#[test]
fn restart_after_close_and_missing_shell_are_bounded() {
    let (temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let service = Arc::new(TerminalService::new(hub));
    let mut server =
        IpcServer::start_with_terminal_service(&paths, sample_status(), Arc::clone(&service))
            .unwrap();
    let mut control = IpcClient::connect(&paths).unwrap();

    let first: IpcResponse<TerminalSpawnResponse> = control
        .request(TERMINAL_SPAWN_METHOD, spawn_request(temp.path()))
        .unwrap();
    let first = match first {
        IpcResponse::Ok { data } => data,
        IpcResponse::Error { error } => panic!("first spawn failed: {error}"),
    };
    let first_id = first.stream_id.clone();
    let closed: IpcResponse<()> = control
        .request(
            TERMINAL_CLOSE_METHOD,
            TerminalCloseRequest {
                stream_id: first.stream_id,
            },
        )
        .unwrap();
    assert!(matches!(closed, IpcResponse::Ok { .. }));

    let second: IpcResponse<TerminalSpawnResponse> = control
        .request(TERMINAL_SPAWN_METHOD, spawn_request(temp.path()))
        .unwrap();
    let second = match second {
        IpcResponse::Ok { data } => data,
        IpcResponse::Error { error } => panic!("restart spawn failed: {error}"),
    };
    assert_ne!(second.stream_id.0, first_id.0);
    assert_eq!(service.session_count(), 1);

    let missing_shell: IpcResponse<TerminalSpawnResponse> = control
        .request(
            TERMINAL_SPAWN_METHOD,
            TerminalSpawnRequest {
                cwd: Some(temp.path().to_string_lossy().into_owned()),
                shell: Some("/muxyard-test/missing-shell".to_string()),
                history_id: None,
                project_id: None,
                rows: 24,
                cols: 80,
            },
        )
        .unwrap();
    assert!(matches!(
        missing_shell,
        IpcResponse::Error { error }
            if error.code == muxyard_contracts::ErrorCode::InvalidRequest
    ));
    assert_eq!(service.session_count(), 1);

    let closed: IpcResponse<()> = control
        .request(
            TERMINAL_CLOSE_METHOD,
            TerminalCloseRequest {
                stream_id: second.stream_id,
            },
        )
        .unwrap();
    assert!(matches!(closed, IpcResponse::Ok { .. }));
    assert_eq!(service.session_count(), 0);
    drop(control);
    server.stop();
}

#[test]
fn buffered_output_is_delivered_after_first_attach() {
    let (temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let service = Arc::new(TerminalService::new(hub));
    let mut server =
        IpcServer::start_with_terminal_service(&paths, sample_status(), Arc::clone(&service))
            .unwrap();
    let mut control = IpcClient::connect(&paths).unwrap();
    let spawned: IpcResponse<TerminalSpawnResponse> = control
        .request(TERMINAL_SPAWN_METHOD, spawn_request(temp.path()))
        .unwrap();
    let spawned = match spawned {
        IpcResponse::Ok { data } => data,
        IpcResponse::Error { error } => panic!("spawn failed: {error}"),
    };

    let input: IpcResponse<()> = control
        .request(
            TERMINAL_INPUT_METHOD,
            TerminalInputRequest {
                stream_id: spawned.stream_id.clone(),
                data: "printf 'deferred-initial-ok\\n'\n".to_string(),
            },
        )
        .unwrap();
    assert!(matches!(input, IpcResponse::Ok { .. }));
    thread::sleep(Duration::from_millis(250));

    let stream_id = Uuid::parse_str(&spawned.stream_id.0).unwrap();
    let terminal = IpcClient::connect(&paths)
        .unwrap()
        .attach_terminal(stream_id, None)
        .unwrap();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut terminal = terminal;
        let mut output = Vec::new();
        loop {
            match terminal.next_frame() {
                Ok(frame) => {
                    output.extend_from_slice(frame.payload());
                    if output
                        .windows(b"deferred-initial-ok".len())
                        .any(|window| window == b"deferred-initial-ok")
                    {
                        let _ = sender.send(Ok(output));
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    let output = receiver
        .recv_timeout(Duration::from_secs(3))
        .expect("timed out waiting for deferred terminal output")
        .expect("terminal stream failed");
    assert!(
        String::from_utf8_lossy(&output).contains("deferred-initial-ok"),
        "missing deferred output: {:?}",
        String::from_utf8_lossy(&output)
    );

    let closed: IpcResponse<()> = control
        .request(
            TERMINAL_CLOSE_METHOD,
            TerminalCloseRequest {
                stream_id: spawned.stream_id,
            },
        )
        .unwrap();
    assert!(matches!(closed, IpcResponse::Ok { .. }));
    drop(control);
    server.stop();
}

#[test]
fn spawn_rejects_invalid_cwd_without_registering_a_session() {
    let (_temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let service = Arc::new(TerminalService::new(hub));
    let mut server =
        IpcServer::start_with_terminal_service(&paths, sample_status(), Arc::clone(&service))
            .unwrap();
    let mut control = IpcClient::connect(&paths).unwrap();
    let response: IpcResponse<TerminalSpawnResponse> = control
        .request(
            TERMINAL_SPAWN_METHOD,
            TerminalSpawnRequest {
                cwd: Some("/muxyard-test/missing-cwd".to_string()),
                shell: Some("/bin/sh".to_string()),
                history_id: None,
                project_id: None,
                rows: 24,
                cols: 80,
            },
        )
        .unwrap();
    assert!(matches!(
        response,
        IpcResponse::Error { error }
            if error.code == muxyard_contracts::ErrorCode::InvalidRequest
    ));
    assert_eq!(service.session_count(), 0);
    drop(control);
    server.stop();
}
