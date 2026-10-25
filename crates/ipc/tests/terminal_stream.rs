//! Exercise terminal attach, bounded output, and control responsiveness.

use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use muxyard_contracts::PROTOCOL_VERSION;
use muxyard_ipc::{HostStatusResponse, IpcClient, IpcOptions, IpcServer};
use muxyard_platform::PlatformPaths;
use muxyard_terminal::{MAX_TERMINAL_PAYLOAD, TerminalHub, TerminalLimits};
use uuid::Uuid;

fn sample_status() -> HostStatusResponse {
    HostStatusResponse {
        host_version: "0.1.0".to_string(),
        protocol_version: PROTOCOL_VERSION,
        instance_id: "terminal-test".to_string(),
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

fn start_server(paths: &PlatformPaths, hub: Arc<TerminalHub>, timeout: Duration) -> IpcServer {
    IpcServer::start_with_terminal_and_options(
        paths,
        sample_status(),
        hub,
        IpcOptions {
            request_timeout: timeout,
        },
    )
    .unwrap()
}

#[test]
fn terminal_output_does_not_block_control_requests() {
    let (_temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let stream_id = hub.create_session().unwrap();
    let mut server = start_server(&paths, Arc::clone(&hub), Duration::from_secs(2));

    let mut control = IpcClient::connect(&paths).unwrap();
    assert_eq!(control.host_status().unwrap(), sample_status());

    let terminal = IpcClient::connect(&paths)
        .unwrap()
        .attach_terminal(stream_id, Some(4 * 1024 * 1024))
        .unwrap();
    let mut terminal = terminal;

    let producer = {
        let hub = Arc::clone(&hub);
        thread::spawn(move || {
            let chunk = vec![b'x'; MAX_TERMINAL_PAYLOAD];
            for _ in 0..160 {
                hub.publish(stream_id, &chunk).unwrap();
                thread::sleep(Duration::from_millis(1));
            }
        })
    };
    let reader = thread::spawn(move || {
        let mut bytes = 0_usize;
        while bytes < 10 * 1024 * 1024 {
            let frame = terminal.next_frame().unwrap();
            bytes += frame.payload().len();
        }
        bytes
    });

    thread::sleep(Duration::from_millis(20));
    let started = Instant::now();
    assert_eq!(control.host_status().unwrap(), sample_status());
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "control request was blocked by terminal output"
    );

    producer.join().unwrap();
    assert_eq!(reader.join().unwrap(), 10 * 1024 * 1024);
    drop(control);
    server.stop();
}

#[test]
fn slow_terminal_subscriber_is_disconnected_without_stopping_control() {
    let (_temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let stream_id = hub.create_session().unwrap();
    let mut server = start_server(&paths, Arc::clone(&hub), Duration::from_millis(100));

    let terminal = IpcClient::connect(&paths)
        .unwrap()
        .attach_terminal(stream_id, Some(16 * 1024))
        .unwrap();
    let session = hub.session(stream_id).unwrap();
    hub.publish(stream_id, &vec![b'y'; 1024 * 1024]).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    while session.subscriber_count() > 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(session.subscriber_count(), 0);
    assert!(session.detached_total() >= 1);

    let mut control = IpcClient::connect(&paths).unwrap();
    assert_eq!(control.host_status().unwrap(), sample_status());
    drop(terminal);
    drop(control);
    server.stop();
}

#[test]
fn server_stop_closes_an_active_terminal_stream() {
    let (_temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let stream_id = hub.create_session().unwrap();
    let mut server = start_server(&paths, hub, Duration::from_secs(2));
    let mut terminal = IpcClient::connect(&paths)
        .unwrap()
        .attach_terminal(stream_id, None)
        .unwrap();

    server.stop();
    let result = terminal.next_frame();
    assert!(matches!(
        result,
        Err(muxyard_ipc::IpcError::Closed)
            | Err(muxyard_ipc::IpcError::Io(_))
            | Err(muxyard_ipc::IpcError::Timeout)
    ));
}

#[test]
fn terminal_attach_rejects_an_out_of_bounds_queue() {
    let (_temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let stream_id = hub.create_session().unwrap();
    let mut server = start_server(&paths, hub, Duration::from_secs(2));

    let error = IpcClient::connect(&paths)
        .unwrap()
        .attach_terminal(stream_id, Some(0))
        .err()
        .expect("zero queue must be rejected");
    assert!(matches!(
        error,
        muxyard_ipc::IpcError::Api(ref api)
            if api.code == muxyard_contracts::ErrorCode::InvalidRequest
    ));

    let mut control = IpcClient::connect(&paths).unwrap();
    assert_eq!(control.host_status().unwrap(), sample_status());
    drop(control);
    server.stop();
}

#[test]
fn terminal_attach_without_a_terminal_hub_is_unsupported() {
    let (_temp, paths) = test_paths();
    let mut server = IpcServer::start_with_options(
        &paths,
        sample_status(),
        IpcOptions {
            request_timeout: Duration::from_secs(2),
        },
    )
    .unwrap();

    let error = IpcClient::connect(&paths)
        .unwrap()
        .attach_terminal(Uuid::new_v4(), None)
        .err()
        .expect("terminal attach without a hub must fail");
    assert!(matches!(
        error,
        muxyard_ipc::IpcError::Api(ref api)
            if api.code == muxyard_contracts::ErrorCode::UnsupportedCapability
    ));

    let mut control = IpcClient::connect(&paths).unwrap();
    assert_eq!(control.host_status().unwrap(), sample_status());
    drop(control);
    server.stop();
}

#[test]
fn unknown_terminal_stream_returns_typed_error_and_keeps_control_alive() {
    let (_temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let mut server = start_server(&paths, hub, Duration::from_secs(2));

    let error = IpcClient::connect(&paths)
        .unwrap()
        .attach_terminal(Uuid::new_v4(), None)
        .err()
        .expect("unknown stream must fail");
    assert!(matches!(
        error,
        muxyard_ipc::IpcError::Api(ref api) if api.code == muxyard_contracts::ErrorCode::NotFound
    ));

    let mut client = IpcClient::connect(&paths).unwrap();
    assert_eq!(client.host_status().unwrap(), sample_status());
    drop(client);
    server.stop();
}
