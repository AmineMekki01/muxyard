//! Tauri commands that expose only typed host bridge operations to the webview.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use muxyard_contracts::{
    ApiError, HostStatusEvent, HostStatusResponse, Project, ProjectListResponse,
    ProjectRegisterRequest, ProjectRemoveRequest, ProjectRemoveResponse, TerminalCloseRequest,
    TerminalInputRequest, TerminalRenderConfig, TerminalResizeRequest, TerminalSnapshotRequest,
    TerminalSnapshotResponse, TerminalSpawnRequest, TerminalSpawnResponse, WorkspaceLayout,
    WorkspaceLayoutSnapshot,
};
use muxyard_platform::{ConfigStore, PlatformPaths};
use muxyard_terminal::TerminalFrameKind;
use tauri::{
    State,
    ipc::{Channel, InvokeResponseBody},
};
use uuid::Uuid;

use crate::host_bridge::{HostBridge, HostBridgeOptions, internal, watch_loop};

/// State used to cancel a previous status watcher.
#[derive(Default)]
pub struct HostWatchState {
    stop: Mutex<Option<Arc<AtomicBool>>>,
}

impl HostWatchState {
    fn replace(&self) -> Arc<AtomicBool> {
        let mut stop = self
            .stop
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(previous) = stop.take() {
            previous.store(true, Ordering::SeqCst);
        }
        let next = Arc::new(AtomicBool::new(false));
        *stop = Some(Arc::clone(&next));
        next
    }

    /// Stop the active status watcher, if any.
    pub fn stop(&self) {
        let mut stop = self
            .stop
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(previous) = stop.take() {
            previous.store(true, Ordering::SeqCst);
        }
    }
}

/// Connect to the host, launching it if no ready host is reachable.
#[tauri::command]
pub async fn host_connect() -> Result<HostStatusResponse, ApiError> {
    tauri::async_runtime::spawn_blocking(|| HostBridge::for_current_user()?.connect_or_launch())
        .await
        .map_err(|_| internal("Host connection task failed."))?
}

/// Read host status without launching a host.
#[tauri::command]
pub async fn host_status() -> Result<HostStatusResponse, ApiError> {
    tauri::async_runtime::spawn_blocking(|| HostBridge::for_current_user()?.status())
        .await
        .map_err(|_| internal("Host status task failed."))?
}

/// Subscribe to bounded host status snapshots and reconnect events.
#[tauri::command]
pub fn host_watch(
    channel: Channel<HostStatusEvent>,
    state: State<'_, HostWatchState>,
) -> Result<(), ApiError> {
    let bridge = HostBridge::for_current_user()?;
    let stop = state.replace();
    let options = HostBridgeOptions::default();
    thread::Builder::new()
        .name("muxyard-host-watch".to_string())
        .spawn(move || watch_loop(bridge, channel, stop, options))
        .map_err(|_| internal("Host status watcher could not start."))?;
    Ok(())
}

/// Stop the active status watcher.
#[tauri::command]
pub fn host_unwatch(state: State<'_, HostWatchState>) {
    state.stop();
}

/// Read the bounded terminal renderer settings from the shared local config.
#[tauri::command]
pub async fn terminal_render_config() -> Result<TerminalRenderConfig, ApiError> {
    tauri::async_runtime::spawn_blocking(|| {
        let paths = PlatformPaths::for_current_user()
            .map_err(|_| internal("Terminal renderer settings are unavailable."))?;
        let config = ConfigStore::new(paths.config_path())
            .load_or_create_default()
            .map_err(|_| internal("Terminal renderer settings are unavailable."))?;
        Ok(TerminalRenderConfig {
            scrollback_lines: config.limits.terminal_scrollback_lines,
        })
    })
    .await
    .map_err(|_| internal("Terminal renderer settings task failed."))?
}

/// List registered projects.
#[tauri::command]
pub async fn project_list() -> Result<ProjectListResponse, ApiError> {
    tauri::async_runtime::spawn_blocking(|| HostBridge::for_current_user()?.project_list())
        .await
        .map_err(|_| internal("Project list task failed."))?
}

/// Register or reopen a project directory.
#[tauri::command]
pub async fn project_register(request: ProjectRegisterRequest) -> Result<Project, ApiError> {
    tauri::async_runtime::spawn_blocking(move || {
        HostBridge::for_current_user()?.project_register(request)
    })
    .await
    .map_err(|_| internal("Project registration task failed."))?
}

/// Remove a project registration.
#[tauri::command]
pub async fn project_remove(
    request: ProjectRemoveRequest,
) -> Result<ProjectRemoveResponse, ApiError> {
    tauri::async_runtime::spawn_blocking(move || {
        HostBridge::for_current_user()?.project_remove(request)
    })
    .await
    .map_err(|_| internal("Project removal task failed."))?
}

/// Create a native shell session in the host.
#[tauri::command]
pub async fn terminal_spawn(
    request: TerminalSpawnRequest,
) -> Result<TerminalSpawnResponse, ApiError> {
    tauri::async_runtime::spawn_blocking(move || {
        HostBridge::for_current_user()?.terminal_spawn(request)
    })
    .await
    .map_err(|_| internal("Terminal spawn task failed."))?
}

/// Write input to a host terminal session.
#[tauri::command]
pub async fn terminal_input(request: TerminalInputRequest) -> Result<(), ApiError> {
    tauri::async_runtime::spawn_blocking(move || {
        HostBridge::for_current_user()?.terminal_input(request)
    })
    .await
    .map_err(|_| internal("Terminal input task failed."))?
}

/// Resize a host terminal session.
#[tauri::command]
pub async fn terminal_resize(request: TerminalResizeRequest) -> Result<(), ApiError> {
    tauri::async_runtime::spawn_blocking(move || {
        HostBridge::for_current_user()?.terminal_resize(request)
    })
    .await
    .map_err(|_| internal("Terminal resize task failed."))?
}

/// Close a host terminal session.
#[tauri::command]
pub async fn terminal_close(request: TerminalCloseRequest) -> Result<(), ApiError> {
    tauri::async_runtime::spawn_blocking(move || {
        HostBridge::for_current_user()?.terminal_close(request)
    })
    .await
    .map_err(|_| internal("Terminal close task failed."))?
}

/// Read recent output for a restored terminal renderer.
#[tauri::command]
pub async fn terminal_snapshot(
    request: TerminalSnapshotRequest,
) -> Result<TerminalSnapshotResponse, ApiError> {
    tauri::async_runtime::spawn_blocking(move || {
        HostBridge::for_current_user()?.terminal_snapshot(request)
    })
    .await
    .map_err(|_| internal("Terminal snapshot task failed."))?
}

/// Read the persisted workspace layout.
#[tauri::command]
pub async fn workspace_layout_get() -> Result<WorkspaceLayoutSnapshot, ApiError> {
    tauri::async_runtime::spawn_blocking(|| HostBridge::for_current_user()?.workspace_layout_get())
        .await
        .map_err(|_| internal("Workspace layout load task failed."))?
}

/// Persist the workspace layout.
#[tauri::command]
pub async fn workspace_layout_save(layout: WorkspaceLayout) -> Result<(), ApiError> {
    tauri::async_runtime::spawn_blocking(move || {
        HostBridge::for_current_user()?.workspace_layout_save(layout)
    })
    .await
    .map_err(|_| internal("Workspace layout save task failed."))?
}

/// Attach a terminal session and stream raw PTY output through a Tauri Channel.
#[tauri::command(rename_all = "snake_case")]
pub fn terminal_attach(
    stream_id: String,
    channel: Channel<InvokeResponseBody>,
) -> Result<(), ApiError> {
    let stream_id = Uuid::parse_str(&stream_id).map_err(|_| ApiError {
        code: muxyard_contracts::ErrorCode::InvalidRequest,
        message: "The terminal stream identifier is invalid.".to_string(),
    })?;
    let bridge = HostBridge::for_current_user()?;
    let mut stream = bridge.attach_terminal(stream_id)?;
    thread::Builder::new()
        .name("muxyard-terminal-output".to_string())
        .spawn(move || {
            while let Ok(frame) = stream.next_frame() {
                match frame.kind() {
                    TerminalFrameKind::Output => {
                        if channel
                            .send(InvokeResponseBody::Raw(frame.payload().to_vec()))
                            .is_err()
                        {
                            break;
                        }
                    }
                    TerminalFrameKind::End => break,
                    _ => {}
                }
            }
        })
        .map_err(|_| internal("Terminal output worker could not start."))?;
    Ok(())
}
