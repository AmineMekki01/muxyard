//! Exercise typed workspace layout persistence over local IPC.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use muxyard_contracts::{
    PROTOCOL_VERSION, WorkspaceLayout, WorkspaceLayoutGetRequest, WorkspaceLayoutSaveRequest,
    WorkspaceLayoutSnapshot,
};
use muxyard_ipc::{
    HostStatusResponse, IpcClient, IpcOptions, IpcResponse, IpcServer, WORKSPACE_LAYOUT_GET_METHOD,
    WORKSPACE_LAYOUT_SAVE_METHOD, WorkspaceLayoutStore,
};
use muxyard_platform::PlatformPaths;
use muxyard_terminal::{TerminalHub, TerminalLimits, TerminalService};

fn sample_status() -> HostStatusResponse {
    HostStatusResponse {
        host_version: "0.1.0".to_string(),
        protocol_version: PROTOCOL_VERSION,
        instance_id: "layout-test".to_string(),
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

struct FakeLayoutStore {
    layout: Mutex<Option<WorkspaceLayout>>,
}

impl WorkspaceLayoutStore for FakeLayoutStore {
    fn load_layout(&self) -> Result<WorkspaceLayoutSnapshot, String> {
        Ok(WorkspaceLayoutSnapshot {
            layout: self.layout.lock().unwrap().clone(),
            host_instance_id: "layout-test".to_string(),
            process_start: "start-test".to_string(),
            layout_host_instance_id: Some("layout-test".to_string()),
            layout_process_start: Some("start-test".to_string()),
        })
    }

    fn save_layout(&self, layout: &WorkspaceLayout) -> Result<(), String> {
        *self.layout.lock().unwrap() = Some(layout.clone());
        Ok(())
    }
}

#[test]
fn workspace_layout_get_and_save_roundtrip() {
    let (_temp, paths) = test_paths();
    let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
    let service = Arc::new(TerminalService::new(hub));
    let store = Arc::new(FakeLayoutStore {
        layout: Mutex::new(None),
    });
    let mut server =
        IpcServer::start_with_terminal_service_and_store(&paths, sample_status(), service, store)
            .unwrap();
    let mut client = IpcClient::connect(&paths).unwrap();

    let empty: IpcResponse<WorkspaceLayoutSnapshot> = client
        .request(WORKSPACE_LAYOUT_GET_METHOD, WorkspaceLayoutGetRequest {})
        .unwrap();
    match empty {
        IpcResponse::Ok { data } => assert!(data.layout.is_none()),
        IpcResponse::Error { error } => panic!("layout get failed: {error}"),
    }

    let layout = WorkspaceLayout {
        version: 1,
        tabs: vec![],
        active_tab_id: None,
    };
    let saved: IpcResponse<()> = client
        .request(
            WORKSPACE_LAYOUT_SAVE_METHOD,
            WorkspaceLayoutSaveRequest {
                layout: layout.clone(),
            },
        )
        .unwrap();
    assert!(matches!(saved, IpcResponse::Ok { .. }));

    let loaded: IpcResponse<WorkspaceLayoutSnapshot> = client
        .request(WORKSPACE_LAYOUT_GET_METHOD, WorkspaceLayoutGetRequest {})
        .unwrap();
    match loaded {
        IpcResponse::Ok { data } => assert_eq!(data.layout, Some(layout)),
        IpcResponse::Error { error } => panic!("layout get failed: {error}"),
    }
    drop(client);
    server.stop();
}

#[test]
fn workspace_layout_without_store_is_unsupported() {
    let (_temp, paths) = test_paths();
    let mut server = IpcServer::start_with_options(
        &paths,
        sample_status(),
        IpcOptions {
            request_timeout: Duration::from_secs(2),
        },
    )
    .unwrap();
    let mut client = IpcClient::connect(&paths).unwrap();
    let response: IpcResponse<WorkspaceLayoutSnapshot> = client
        .request(WORKSPACE_LAYOUT_GET_METHOD, WorkspaceLayoutGetRequest {})
        .unwrap();
    assert!(matches!(
        response,
        IpcResponse::Error { error }
            if error.code == muxyard_contracts::ErrorCode::UnsupportedCapability
    ));
    drop(client);
    server.stop();
}
