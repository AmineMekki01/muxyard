//! Tauri commands that expose only typed host bridge operations to the webview.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use muxyard_contracts::{ApiError, HostStatusEvent, HostStatusResponse};
use tauri::{State, ipc::Channel};

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
