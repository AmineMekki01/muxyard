//! Exercise framed local IPC, typed status, and malformed-input isolation.

#![cfg(unix)]

use std::{
    io::{self, Write},
    os::unix::fs::{FileTypeExt, PermissionsExt},
    time::Duration,
};

use interprocess::local_socket::{GenericFilePath, prelude::*};
use muxyard_contracts::{MAX_CONTROL_BYTES, PROTOCOL_VERSION};
use muxyard_ipc::{
    HOST_STATUS_METHOD, HostStatusResponse, IpcClient, IpcOptions, IpcResponse, IpcServer,
    read_frame, write_frame,
};
use muxyard_platform::PlatformPaths;
use serde_json::{Value, json};

fn sample_status() -> HostStatusResponse {
    HostStatusResponse {
        host_version: "0.1.0".to_string(),
        protocol_version: PROTOCOL_VERSION,
        instance_id: "instance-test".to_string(),
        pid: 4242,
        process_start: "start-test".to_string(),
        started_ms: 1_700_000_000_000,
        ready: true,
    }
}

fn test_paths() -> (tempfile::TempDir, PlatformPaths) {
    let temp = tempfile::tempdir().unwrap();
    let paths = PlatformPaths::from_root(temp.path());
    paths.ensure().unwrap();
    (temp, paths)
}

fn start_server(paths: &PlatformPaths, timeout: Duration) -> IpcServer {
    IpcServer::start_with_options(
        paths,
        sample_status(),
        IpcOptions {
            request_timeout: timeout,
        },
    )
    .unwrap()
}

fn raw_stream(paths: &PlatformPaths) -> LocalSocketStream {
    let socket_path = paths.local_socket_path();
    let name = socket_path
        .as_path()
        .to_fs_name::<GenericFilePath>()
        .unwrap();
    LocalSocketStream::connect(name).unwrap()
}

fn send_json(stream: &mut LocalSocketStream, value: &Value) {
    write_frame(stream, &serde_json::to_vec(value).unwrap()).unwrap();
}

fn read_json(stream: &mut LocalSocketStream) -> Value {
    let frame = read_frame(stream).unwrap().expect("response frame");
    serde_json::from_slice(&frame).unwrap()
}

fn request_frame(id: &str) -> Value {
    json!({
        "v": PROTOCOL_VERSION,
        "id": id,
        "type": "request",
        "method": HOST_STATUS_METHOD,
        "data": {},
    })
}

#[test]
fn typed_host_status_roundtrip_and_cleanup() {
    let (_temp, paths) = test_paths();
    let mut server = start_server(&paths, Duration::from_secs(2));
    let socket_path = paths.local_socket_path();

    #[cfg(unix)]
    {
        assert_eq!(
            std::fs::metadata(&socket_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(
            std::fs::metadata(&socket_path)
                .unwrap()
                .file_type()
                .is_socket()
        );
        assert_eq!(
            std::fs::metadata(&paths.runtime_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);

    server.stop();
    assert!(!socket_path.exists());
    assert!(IpcClient::connect(&paths).is_err());
}

#[test]
fn unknown_method_and_invalid_body_return_typed_errors() {
    let (_temp, paths) = test_paths();
    let mut server = start_server(&paths, Duration::from_secs(2));
    let mut client = IpcClient::connect(&paths).unwrap();

    let unknown: IpcResponse<Value> = client
        .request("host.unknown", json!({}))
        .expect("unknown method response");
    match unknown {
        IpcResponse::Error { error } => {
            assert_eq!(
                serde_json::to_value(error.code).unwrap(),
                "UNSUPPORTED_CAPABILITY"
            );
        }
        IpcResponse::Ok { .. } => panic!("unknown method unexpectedly succeeded"),
    }

    let invalid: IpcResponse<Value> = client
        .request(HOST_STATUS_METHOD, json!({"unexpected": true}))
        .expect("invalid body response");
    match invalid {
        IpcResponse::Error { error } => {
            assert_eq!(serde_json::to_value(error.code).unwrap(), "INVALID_REQUEST");
        }
        IpcResponse::Ok { .. } => panic!("invalid body unexpectedly succeeded"),
    }

    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    server.stop();
}

#[test]
fn version_mismatch_is_rejected_without_stopping_the_server() {
    let (_temp, paths) = test_paths();
    let mut server = start_server(&paths, Duration::from_secs(2));
    let mut raw = raw_stream(&paths);
    let mut request = request_frame("version-mismatch");
    request["v"] = json!(2);
    send_json(&mut raw, &request);

    let response = read_json(&mut raw);
    assert_eq!(response["id"], "version-mismatch");
    assert_eq!(response["v"], 1);
    assert_eq!(response["data"]["status"], "error");
    assert_eq!(response["data"]["error"]["code"], "UNSUPPORTED_VERSION");

    drop(raw);
    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    server.stop();
}

#[test]
fn wrong_message_type_returns_a_typed_error_and_closes() {
    let (_temp, paths) = test_paths();
    let mut server = start_server(&paths, Duration::from_secs(2));
    let mut raw = raw_stream(&paths);
    send_json(
        &mut raw,
        &json!({
            "v": PROTOCOL_VERSION,
            "id": "wrong-type",
            "type": "event",
            "method": HOST_STATUS_METHOD,
            "data": {},
        }),
    );

    let response = read_json(&mut raw);
    assert_eq!(response["id"], "wrong-type");
    assert_eq!(response["data"]["status"], "error");
    assert_eq!(response["data"]["error"]["code"], "INVALID_REQUEST");
    assert!(read_frame(&mut raw).unwrap().is_none());
    drop(raw);

    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    server.stop();
}

#[test]
fn empty_request_id_is_rejected_and_closes() {
    let (_temp, paths) = test_paths();
    let mut server = start_server(&paths, Duration::from_secs(2));
    let mut raw = raw_stream(&paths);
    send_json(&mut raw, &request_frame(""));

    let response = read_json(&mut raw);
    assert_eq!(response["id"], "");
    assert_eq!(response["data"]["status"], "error");
    assert_eq!(response["data"]["error"]["code"], "INVALID_REQUEST");
    assert!(read_frame(&mut raw).unwrap().is_none());
    drop(raw);

    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    server.stop();
}

#[test]
fn malformed_json_and_overlong_frames_close_only_that_connection() {
    let (_temp, paths) = test_paths();
    let mut server = start_server(&paths, Duration::from_secs(2));

    let mut malformed = raw_stream(&paths);
    write_frame(&mut malformed, b"{not-json").unwrap();
    assert!(
        read_frame(&mut malformed)
            .map(|frame| frame.is_none())
            .unwrap_or(true)
    );
    drop(malformed);

    let mut overlong = raw_stream(&paths);
    overlong
        .write_all(&u32::try_from(MAX_CONTROL_BYTES + 1).unwrap().to_be_bytes())
        .unwrap();
    overlong.flush().unwrap();
    assert!(
        read_frame(&mut overlong)
            .map(|frame| frame.is_none())
            .unwrap_or(true)
    );
    drop(overlong);

    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    server.stop();
}

#[test]
fn truncated_frame_and_peer_disconnect_do_not_stop_the_server() {
    let (_temp, paths) = test_paths();
    let mut server = start_server(&paths, Duration::from_secs(2));

    let mut truncated = raw_stream(&paths);
    truncated.write_all(&[0, 0, 0, 16, 1, 2]).unwrap();
    truncated.flush().unwrap();
    drop(truncated);

    let idle = raw_stream(&paths);
    drop(idle);

    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    server.stop();
}

#[test]
fn idle_connection_times_out_and_server_continues() {
    let (_temp, paths) = test_paths();
    let mut server = start_server(&paths, Duration::from_millis(100));
    let mut idle = raw_stream(&paths);
    idle.set_recv_timeout(Some(Duration::from_secs(1))).unwrap();
    std::thread::sleep(Duration::from_millis(250));

    match read_frame(&mut idle) {
        Ok(None) => {}
        Err(muxyard_ipc::IpcError::Io(error))
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) => {}
        Ok(Some(_)) => panic!("idle connection unexpectedly produced a frame"),
        Err(error) => panic!("unexpected idle error: {error}"),
    }
    drop(idle);

    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    server.stop();
}

#[test]
fn cancelled_request_and_restart_do_not_leave_a_broken_endpoint() {
    let (_temp, paths) = test_paths();
    let mut server = start_server(&paths, Duration::from_secs(2));

    let mut cancelled = raw_stream(&paths);
    send_json(&mut cancelled, &request_frame("cancelled"));
    drop(cancelled);

    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    server.stop();

    let mut restarted = start_server(&paths, Duration::from_secs(2));
    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    restarted.stop();
}

#[test]
fn pseudo_random_malformed_frames_do_not_crash_the_server() {
    let (_temp, paths) = test_paths();
    let mut server = start_server(&paths, Duration::from_secs(2));
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;

    for _ in 0..96 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let length = (state as usize % 48) + 1;
        let mut payload = vec![0_u8; length];
        for byte in &mut payload {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            *byte = state as u8;
        }
        let mut raw = raw_stream(&paths);
        raw.write_all(&u32::try_from(length).unwrap().to_be_bytes())
            .unwrap();
        raw.write_all(&payload).unwrap();
        raw.flush().unwrap();
        drop(raw);
    }

    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    server.stop();
}

#[test]
fn fixed_malformed_inputs_do_not_crash_the_server() {
    let (_temp, paths) = test_paths();
    let mut server = start_server(&paths, Duration::from_secs(2));
    let payloads: &[&[u8]] = &[
        b"",
        b"\0",
        b"\0\0\0",
        b"\0\0\0\0",
        b"\0\0\0\x05{}",
        b"null",
        b"[]",
        b"{\"v\":1}",
        b"{\"v\":1,\"id\":\"x\",\"type\":\"response\",\"method\":\"host.status\",\"data\":{}}",
        b"{\"v\":1,\"id\":\"x\",\"type\":\"request\",\"method\":\"host.status\"}",
    ];

    for payload in payloads {
        let mut raw = raw_stream(&paths);
        if !payload.is_empty() {
            write_frame(&mut raw, payload).unwrap();
        }
        let _ = read_frame(&mut raw);
        drop(raw);
    }

    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    server.stop();
}

#[test]
fn non_private_runtime_directory_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let paths = PlatformPaths::from_root(temp.path());
    std::fs::create_dir_all(&paths.runtime_dir).unwrap();
    std::fs::set_permissions(&paths.runtime_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    let error = IpcServer::start_with_options(&paths, sample_status(), IpcOptions::default())
        .err()
        .expect("non-private runtime directory must be rejected");
    assert!(matches!(error, muxyard_ipc::IpcError::InsecureEndpoint));
}
