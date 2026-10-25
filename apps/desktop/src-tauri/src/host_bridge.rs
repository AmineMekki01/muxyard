//! Blocking bridge from Tauri commands to the protected host IPC endpoint.

use std::{
    env,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use muxyard_contracts::{
    ApiError, ErrorCode, HostStatusEvent, HostStatusResponse, PROTOCOL_VERSION,
    TerminalCloseRequest, TerminalInputRequest, TerminalResizeRequest, TerminalSpawnRequest,
    TerminalSpawnResponse,
};
use muxyard_ipc::{IpcClient, IpcError, IpcResponse, TerminalStream};
use muxyard_platform::PlatformPaths;
use serde::{Serialize, de::DeserializeOwned};
use tauri::ipc::Channel;
use uuid::Uuid;

/// Time bounds used while attaching to and watching a host.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HostBridgeOptions {
    /// Maximum time to wait for a launched host to answer.
    pub startup_timeout: Duration,
    /// Delay between startup status attempts.
    pub retry_interval: Duration,
    /// Delay between watch status attempts.
    pub watch_interval: Duration,
    /// Consecutive failed watch attempts before reporting offline.
    pub watch_failures: u8,
}

impl Default for HostBridgeOptions {
    fn default() -> Self {
        Self {
            startup_timeout: Duration::from_secs(12),
            retry_interval: Duration::from_millis(150),
            watch_interval: Duration::from_secs(2),
            watch_failures: 3,
        }
    }
}

/// Starts a host process when no ready host is reachable.
pub(crate) trait HostLauncher: Send + Sync {
    fn launch(&self) -> Result<(), ApiError>;
}

/// Launches the packaged or development host executable.
#[derive(Debug, Clone, Default)]
pub(crate) struct ProcessLauncher {
    binary: Option<PathBuf>,
}

impl ProcessLauncher {
    #[cfg(test)]
    pub(crate) fn new(binary: Option<PathBuf>) -> Self {
        Self { binary }
    }

    fn resolve_binary(&self) -> PathBuf {
        if let Some(binary) = &self.binary {
            return binary.clone();
        }
        if let Some(binary) = env::var_os("MUXYARD_HOST_BIN").filter(|value| !value.is_empty()) {
            return PathBuf::from(binary);
        }
        if let Ok(executable) = env::current_exe()
            && let Some(parent) = executable.parent()
        {
            let candidate = parent.join(if cfg!(windows) {
                "muxyard-host.exe"
            } else {
                "muxyard-host"
            });
            if candidate.is_file() {
                return candidate;
            }
        }
        PathBuf::from(if cfg!(windows) {
            "muxyard-host.exe"
        } else {
            "muxyard-host"
        })
    }
}

impl HostLauncher for ProcessLauncher {
    fn launch(&self) -> Result<(), ApiError> {
        let binary = self.resolve_binary();
        let output = Command::new(&binary)
            .arg("start")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|_| host_offline("The host executable could not be started."))?;
        if output.status.success() {
            return Ok(());
        }
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        let message = if detail.is_empty() {
            format!("Host launcher exited with {:?}.", output.status.code())
        } else {
            format!(
                "Host launcher failed: {}",
                detail.chars().take(300).collect::<String>()
            )
        };
        Err(host_offline(message))
    }
}

/// Typed connection helper shared by Tauri commands and the status watcher.
#[derive(Clone)]
pub(crate) struct HostBridge {
    paths: PlatformPaths,
    launcher: Arc<dyn HostLauncher>,
    options: HostBridgeOptions,
}

impl HostBridge {
    pub(crate) fn for_current_user() -> Result<Self, ApiError> {
        let paths = PlatformPaths::for_current_user()
            .map_err(|_| internal("Host paths could not be resolved."))?;
        Ok(Self {
            paths,
            launcher: Arc::new(ProcessLauncher::default()),
            options: HostBridgeOptions::default(),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_launcher(
        paths: PlatformPaths,
        launcher: Arc<dyn HostLauncher>,
        options: HostBridgeOptions,
    ) -> Self {
        Self {
            paths,
            launcher,
            options,
        }
    }

    /// Read status without launching a host.
    pub(crate) fn status(&self) -> Result<HostStatusResponse, ApiError> {
        self.status_once()
    }

    /// Create a native shell session in the host.
    pub(crate) fn terminal_spawn(
        &self,
        request: TerminalSpawnRequest,
    ) -> Result<TerminalSpawnResponse, ApiError> {
        self.request("terminal.spawn", request)
    }

    /// Write input to a host terminal session.
    pub(crate) fn terminal_input(&self, request: TerminalInputRequest) -> Result<(), ApiError> {
        self.request("terminal.input", request)
    }

    /// Resize a host terminal session.
    pub(crate) fn terminal_resize(&self, request: TerminalResizeRequest) -> Result<(), ApiError> {
        self.request("terminal.resize", request)
    }

    /// Close a host terminal session.
    pub(crate) fn terminal_close(&self, request: TerminalCloseRequest) -> Result<(), ApiError> {
        self.request("terminal.close", request)
    }

    /// Attach a binary output stream to a host terminal session.
    pub(crate) fn attach_terminal(&self, stream_id: Uuid) -> Result<TerminalStream, ApiError> {
        let client = IpcClient::connect(&self.paths).map_err(map_ipc)?;
        client.attach_terminal(stream_id, None).map_err(map_ipc)
    }

    fn request<T: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        data: T,
    ) -> Result<R, ApiError> {
        let mut client = IpcClient::connect(&self.paths).map_err(map_ipc)?;
        let response = client.request(method, data).map_err(map_ipc)?;
        match response {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { error } => Err(error),
        }
    }

    /// Read status, or launch a host and wait for it to become ready.
    pub(crate) fn connect_or_launch(&self) -> Result<HostStatusResponse, ApiError> {
        match self.status_once() {
            Ok(status) => return Ok(status),
            Err(error) if error.code == ErrorCode::HostOffline => {}
            Err(error) => return Err(error),
        }

        self.launcher.launch()?;
        let deadline = Instant::now() + self.options.startup_timeout;
        let mut last_error: ApiError;
        loop {
            match self.status_once() {
                Ok(status) => return Ok(status),
                Err(error) if error.code == ErrorCode::HostOffline => {
                    last_error = error;
                }
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(self.options.retry_interval);
        }

        Err(host_offline(format!(
            "Host did not become ready within {} seconds. {}",
            self.options.startup_timeout.as_secs(),
            last_error.message
        )))
    }

    fn status_once(&self) -> Result<HostStatusResponse, ApiError> {
        let mut client = IpcClient::connect(&self.paths).map_err(map_ipc)?;
        let status = client.host_status().map_err(map_ipc)?;
        validate_status(status)
    }
}

/// Poll the host and forward bounded status events to the webview.
pub(crate) fn watch_loop(
    bridge: HostBridge,
    channel: Channel<HostStatusEvent>,
    stop: Arc<AtomicBool>,
    options: HostBridgeOptions,
) {
    let mut failures = 0_u8;
    let mut first = true;
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match bridge.status() {
            Ok(status) => {
                failures = 0;
                let event = if first {
                    HostStatusEvent::Snapshot { status }
                } else {
                    HostStatusEvent::Status { status }
                };
                first = false;
                if channel.send(event).is_err() {
                    return;
                }
            }
            Err(error) => {
                if error.code == ErrorCode::UnsupportedCapability {
                    let _ = channel.send(HostStatusEvent::Offline { error });
                    return;
                }
                failures = failures.saturating_add(1);
                if failures >= options.watch_failures {
                    let _ = channel.send(HostStatusEvent::Offline { error });
                    return;
                }
                if channel
                    .send(HostStatusEvent::Reconnecting { attempt: failures })
                    .is_err()
                {
                    return;
                }
            }
        }
        sleep_interruptible(options.watch_interval, &stop);
    }
}

fn validate_status(status: HostStatusResponse) -> Result<HostStatusResponse, ApiError> {
    if status.protocol_version != PROTOCOL_VERSION {
        return Err(ApiError {
            code: ErrorCode::UnsupportedCapability,
            message: format!(
                "Host protocol {} is incompatible with this app (protocol {}).",
                status.protocol_version, PROTOCOL_VERSION
            ),
        });
    }
    Ok(status)
}

fn map_ipc(error: IpcError) -> ApiError {
    match error {
        IpcError::Api(error) if error.code == ErrorCode::UnsupportedVersion => ApiError {
            code: ErrorCode::UnsupportedCapability,
            message: error.message,
        },
        IpcError::Api(error) => error,
        IpcError::UnauthorizedPeer => ApiError {
            code: ErrorCode::Unauthorized,
            message: "The local host rejected the desktop bridge.".to_string(),
        },
        IpcError::InsecureEndpoint => internal("The local host endpoint is not private."),
        IpcError::FrameTooLarge
        | IpcError::TruncatedFrame
        | IpcError::InvalidFrame
        | IpcError::Protocol
        | IpcError::Json(_)
        | IpcError::TerminalFrame(_)
        | IpcError::TerminalDetached
        | IpcError::TerminalProtocol => internal("The local host returned an invalid response."),
        IpcError::UnsupportedPlatform => {
            internal("Local host IPC is unsupported on this platform.")
        }
        IpcError::Endpoint(_) | IpcError::Io(_) | IpcError::Timeout | IpcError::Closed => {
            host_offline("Host is offline.")
        }
    }
}

pub(crate) fn host_offline(message: impl Into<String>) -> ApiError {
    ApiError {
        code: ErrorCode::HostOffline,
        message: message.into(),
    }
}

pub(crate) fn internal(message: impl Into<String>) -> ApiError {
    ApiError {
        code: ErrorCode::Internal,
        message: message.into(),
    }
}

fn sleep_interruptible(duration: Duration, stop: &AtomicBool) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    use muxyard_contracts::{TerminalCloseRequest, TerminalInputRequest, TerminalSpawnRequest};
    use muxyard_ipc::IpcServer;
    use muxyard_platform::PlatformPaths;
    use muxyard_terminal::{TerminalFrameKind, TerminalHub, TerminalLimits, TerminalService};
    use tauri::ipc::InvokeResponseBody;
    use tempfile::TempDir;

    use super::*;

    fn status(instance: &str, protocol: u8) -> HostStatusResponse {
        HostStatusResponse {
            host_version: "0.1.0".to_string(),
            protocol_version: protocol,
            instance_id: instance.to_string(),
            pid: 1234,
            process_start: "test-start".to_string(),
            started_ms: 1,
            ready: true,
        }
    }

    fn paths() -> (TempDir, PlatformPaths) {
        let temp = tempfile::tempdir().unwrap();
        let paths = PlatformPaths::from_root(temp.path());
        paths.ensure().unwrap();
        (temp, paths)
    }

    fn options() -> HostBridgeOptions {
        HostBridgeOptions {
            startup_timeout: Duration::from_millis(300),
            retry_interval: Duration::from_millis(10),
            watch_interval: Duration::from_millis(10),
            watch_failures: 3,
        }
    }

    struct FakeLauncher {
        paths: PlatformPaths,
        status: HostStatusResponse,
        calls: AtomicUsize,
        server: Mutex<Option<IpcServer>>,
        fail: bool,
        start_server: bool,
    }

    impl FakeLauncher {
        fn new(paths: PlatformPaths, status: HostStatusResponse) -> Self {
            Self {
                paths,
                status,
                calls: AtomicUsize::new(0),
                server: Mutex::new(None),
                fail: false,
                start_server: true,
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn stop(&self) {
            if let Some(server) = self.server.lock().unwrap().take() {
                let mut server = server;
                server.stop();
            }
        }
    }

    impl HostLauncher for FakeLauncher {
        fn launch(&self) -> Result<(), ApiError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(host_offline("fake launch failure"));
            }
            if !self.start_server {
                return Ok(());
            }
            let mut server = self.server.lock().unwrap();
            if let Some(mut previous) = server.take() {
                previous.stop();
            }
            let listener = IpcServer::start(&self.paths, self.status.clone())
                .map_err(|_| host_offline("fake host could not bind"))?;
            *server = Some(listener);
            Ok(())
        }
    }

    #[test]
    fn launches_once_and_reuses_the_same_host() {
        let (_temp, paths) = paths();
        let fake = Arc::new(FakeLauncher::new(paths.clone(), status("instance-1", 1)));
        let bridge =
            HostBridge::with_launcher(paths, Arc::clone(&fake) as Arc<dyn HostLauncher>, options());

        let first = bridge.connect_or_launch().unwrap();
        assert_eq!(first.instance_id, "instance-1");
        assert_eq!(fake.calls(), 1);

        let second = bridge.status().unwrap();
        assert_eq!(second.instance_id, "instance-1");
        assert_eq!(fake.calls(), 1);
    }

    #[test]
    fn reconnects_after_host_loss() {
        let (_temp, paths) = paths();
        let fake = Arc::new(FakeLauncher::new(paths.clone(), status("instance-2", 1)));
        let bridge =
            HostBridge::with_launcher(paths, Arc::clone(&fake) as Arc<dyn HostLauncher>, options());

        assert_eq!(
            bridge.connect_or_launch().unwrap().instance_id,
            "instance-2"
        );
        fake.stop();
        assert_eq!(bridge.status().unwrap_err().code, ErrorCode::HostOffline);

        let reconnected = bridge.connect_or_launch().unwrap();
        assert_eq!(reconnected.instance_id, "instance-2");
        assert_eq!(fake.calls(), 2);
    }

    #[test]
    fn incompatible_host_is_reported_without_launching_again() {
        let (_temp, paths) = paths();
        let mut server = IpcServer::start(&paths, status("old-host", 2)).unwrap();
        let fake = Arc::new(FakeLauncher::new(paths.clone(), status("unused", 1)));
        let bridge =
            HostBridge::with_launcher(paths, Arc::clone(&fake) as Arc<dyn HostLauncher>, options());

        let error = bridge.connect_or_launch().unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedCapability);
        assert_eq!(fake.calls(), 0);
        server.stop();
    }

    #[test]
    fn failed_launch_and_bounded_retry_report_offline() {
        let (_temp, paths) = paths();
        let mut fake = FakeLauncher::new(paths.clone(), status("unused", 1));
        fake.fail = true;
        let fake = Arc::new(fake);
        let bridge =
            HostBridge::with_launcher(paths, Arc::clone(&fake) as Arc<dyn HostLauncher>, options());

        let error = bridge.connect_or_launch().unwrap_err();
        assert_eq!(error.code, ErrorCode::HostOffline);
        assert_eq!(fake.calls(), 1);
    }

    #[test]
    fn launch_success_without_a_host_stops_after_the_startup_deadline() {
        let (_temp, paths) = paths();
        let mut fake = FakeLauncher::new(paths.clone(), status("unused", 1));
        fake.start_server = false;
        let fake = Arc::new(fake);
        let bridge =
            HostBridge::with_launcher(paths, Arc::clone(&fake) as Arc<dyn HostLauncher>, options());

        let started = Instant::now();
        let error = bridge.connect_or_launch().unwrap_err();
        assert_eq!(error.code, ErrorCode::HostOffline);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(fake.calls(), 1);
    }

    #[test]
    fn watcher_sends_an_initial_snapshot_and_stops_cleanly() {
        let (_temp, paths) = paths();
        let fake = Arc::new(FakeLauncher::new(paths.clone(), status("watch-host", 1)));
        let bridge =
            HostBridge::with_launcher(paths, Arc::clone(&fake) as Arc<dyn HostLauncher>, options());
        assert_eq!(
            bridge.connect_or_launch().unwrap().instance_id,
            "watch-host"
        );

        let events = Arc::new(Mutex::new(Vec::new()));
        let channel = Channel::new({
            let events = Arc::clone(&events);
            move |body: InvokeResponseBody| {
                if let InvokeResponseBody::Json(json) = body
                    && let Ok(event) = serde_json::from_str::<HostStatusEvent>(&json)
                {
                    events.lock().unwrap().push(event);
                }
                Ok(())
            }
        });
        let stop = Arc::new(AtomicBool::new(false));
        let watch_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            watch_loop(bridge, channel, watch_stop, options());
        });

        for _ in 0..100 {
            if !events.lock().unwrap().is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        stop.store(true, Ordering::SeqCst);
        handle.join().unwrap();

        let events = events.lock().unwrap();
        assert!(matches!(
            events.first(),
            Some(HostStatusEvent::Snapshot { status })
                if status.instance_id == "watch-host"
        ));
    }

    #[test]
    fn watcher_reports_offline_after_bounded_failures() {
        let (_temp, paths) = paths();
        let fake = Arc::new(FakeLauncher::new(paths.clone(), status("lost-host", 1)));
        let bridge =
            HostBridge::with_launcher(paths, Arc::clone(&fake) as Arc<dyn HostLauncher>, options());
        assert_eq!(bridge.connect_or_launch().unwrap().instance_id, "lost-host");
        fake.stop();

        let events = Arc::new(Mutex::new(Vec::new()));
        let channel = Channel::new({
            let events = Arc::clone(&events);
            move |body: InvokeResponseBody| {
                if let InvokeResponseBody::Json(json) = body
                    && let Ok(event) = serde_json::from_str::<HostStatusEvent>(&json)
                {
                    events.lock().unwrap().push(event);
                }
                Ok(())
            }
        });
        let stop = Arc::new(AtomicBool::new(false));
        let watch_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            watch_loop(bridge, channel, watch_stop, options());
        });

        for _ in 0..200 {
            if events
                .lock()
                .unwrap()
                .iter()
                .any(|event| matches!(event, HostStatusEvent::Offline { .. }))
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        stop.store(true, Ordering::SeqCst);
        handle.join().unwrap();

        let events = events.lock().unwrap();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, HostStatusEvent::Reconnecting { attempt: 1 }))
        );
        assert!(matches!(
            events.last(),
            Some(HostStatusEvent::Offline { error })
                if error.code == ErrorCode::HostOffline
        ));
    }

    #[test]
    fn terminal_bridge_spawns_inputs_attaches_and_closes() {
        let (_temp, paths) = paths();
        let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
        let service = Arc::new(TerminalService::new(hub));
        let mut server = IpcServer::start_with_terminal_service(
            &paths,
            status("terminal-bridge", 1),
            Arc::clone(&service),
        )
        .unwrap();
        let fake = Arc::new(FakeLauncher::new(paths.clone(), status("unused", 1)));
        let bridge =
            HostBridge::with_launcher(paths, Arc::clone(&fake) as Arc<dyn HostLauncher>, options());

        let spawned = bridge
            .terminal_spawn(TerminalSpawnRequest {
                cwd: Some(
                    std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                ),
                shell: Some("/bin/sh".to_string()),
                rows: 24,
                cols: 80,
            })
            .unwrap();
        let stream_id = Uuid::parse_str(&spawned.stream_id.0).unwrap();
        let mut terminal = bridge.attach_terminal(stream_id).unwrap();
        bridge
            .terminal_input(TerminalInputRequest {
                stream_id: spawned.stream_id.clone(),
                data: "printf 'bridge-terminal-ok\\n'\n".to_string(),
            })
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        while Instant::now() < deadline {
            let frame = terminal.next_frame().unwrap();
            if frame.kind() == TerminalFrameKind::Output {
                output.extend_from_slice(frame.payload());
                if output
                    .windows(b"bridge-terminal-ok".len())
                    .any(|window| window == b"bridge-terminal-ok")
                {
                    break;
                }
            }
        }
        assert!(
            String::from_utf8_lossy(&output).contains("bridge-terminal-ok"),
            "missing bridge terminal output"
        );

        bridge
            .terminal_close(TerminalCloseRequest {
                stream_id: spawned.stream_id,
            })
            .unwrap();
        assert_eq!(service.session_count(), 0);
        drop(terminal);
        server.stop();
    }

    #[test]
    fn missing_host_binary_is_a_bounded_offline_error() {
        let launcher = ProcessLauncher::new(Some(PathBuf::from(
            "/muxyard-test/does-not-exist/muxyard-host",
        )));
        let error = launcher.launch().unwrap_err();
        assert_eq!(error.code, ErrorCode::HostOffline);
    }
}
