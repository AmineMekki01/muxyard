//! Local socket listener, connection isolation, and bounded request dispatch.

use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::{
    fs,
    path::{Path, PathBuf},
};
#[cfg(windows)]
use std::{path::Path, sync::OnceLock};

#[cfg(windows)]
use std::process::Command;

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, prelude::*};
use muxyard_contracts::{
    ApiError, ControlEnvelope, ControlMessageType, ErrorCode, Id, ProtocolVersion,
    TerminalCloseRequest, TerminalInputRequest, TerminalResizeRequest, TerminalSnapshotRequest,
    TerminalSnapshotResponse, TerminalSpawnRequest, WorkspaceLayoutGetRequest,
    WorkspaceLayoutSaveRequest, decode_control,
};
use muxyard_platform::PlatformPaths;
use muxyard_terminal::{
    MAX_TERMINAL_PAYLOAD, TerminalAttachRequest, TerminalAttachResponse, TerminalHub,
    TerminalHubError, TerminalService, TerminalServiceError, TerminalSubscription,
};
use serde::Serialize;
use serde_json::Value;
use tracing::warn;

use crate::{
    HOST_STATUS_METHOD, HostStatusRequest, HostStatusResponse, IpcError, IpcResponse,
    PROJECT_LIST_METHOD, PROJECT_REGISTER_METHOD, PROJECT_REMOVE_METHOD, ProjectStore,
    TERMINAL_ATTACH_METHOD, TERMINAL_CLOSE_METHOD, TERMINAL_INPUT_METHOD, TERMINAL_RESIZE_METHOD,
    TERMINAL_SNAPSHOT_METHOD, TERMINAL_SPAWN_METHOD, WORKSPACE_LAYOUT_GET_METHOD,
    WORKSPACE_LAYOUT_SAVE_METHOD, WorkspaceLayoutStore, read_frame_until, write_frame_until,
};

/// Bounds applied to one local client connection.
#[derive(Debug, Clone, Copy)]
pub struct IpcOptions {
    /// Maximum time to wait for a request or a response write.
    pub request_timeout: Duration,
}

impl Default for IpcOptions {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(5),
        }
    }
}

/// Running local IPC listener and its connection workers.
pub struct IpcServer {
    stop: Arc<AtomicBool>,
    accept_thread: Option<JoinHandle<()>>,
    connections: Arc<Mutex<Vec<JoinHandle<()>>>>,
    #[cfg(unix)]
    socket_path: PathBuf,
    #[cfg(windows)]
    pipe_name: String,
}

impl IpcServer {
    /// Bind the protected local endpoint and begin accepting clients.
    ///
    /// On Unix the runtime directory must already be private and the socket is
    /// made owner-only after binding. On Windows the named pipe receives a
    /// security descriptor for the current user's SID.
    pub fn start(paths: &PlatformPaths, status: HostStatusResponse) -> Result<Self, IpcError> {
        Self::start_inner(paths, status, None, None, None, IpcOptions::default())
    }

    /// Bind the endpoint with explicit timeout settings.
    pub fn start_with_options(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        options: IpcOptions,
    ) -> Result<Self, IpcError> {
        Self::start_inner(paths, status, None, None, None, options)
    }

    /// Bind the endpoint and enable terminal stream attachments.
    pub fn start_with_terminal(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        terminal: Arc<TerminalHub>,
    ) -> Result<Self, IpcError> {
        Self::start_inner(
            paths,
            status,
            Some(Arc::new(TerminalService::new(terminal))),
            None,
            None,
            IpcOptions::default(),
        )
    }

    /// Bind the endpoint with terminal streams and explicit timeouts.
    pub fn start_with_terminal_and_options(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        terminal: Arc<TerminalHub>,
        options: IpcOptions,
    ) -> Result<Self, IpcError> {
        Self::start_inner(
            paths,
            status,
            Some(Arc::new(TerminalService::new(terminal))),
            None,
            None,
            options,
        )
    }

    /// Bind the endpoint with a managed terminal service.
    pub fn start_with_terminal_service(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        terminal: Arc<TerminalService>,
    ) -> Result<Self, IpcError> {
        Self::start_inner(
            paths,
            status,
            Some(terminal),
            None,
            None,
            IpcOptions::default(),
        )
    }

    /// Bind the endpoint with a managed terminal service and explicit timeouts.
    pub fn start_with_terminal_service_and_options(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        terminal: Arc<TerminalService>,
        options: IpcOptions,
    ) -> Result<Self, IpcError> {
        Self::start_inner(paths, status, Some(terminal), None, None, options)
    }

    /// Bind the endpoint with terminal and workspace persistence stores.
    pub fn start_with_terminal_service_and_store(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        terminal: Arc<TerminalService>,
        layout_store: Arc<dyn WorkspaceLayoutStore>,
    ) -> Result<Self, IpcError> {
        Self::start_inner(
            paths,
            status,
            Some(terminal),
            Some(layout_store),
            None,
            IpcOptions::default(),
        )
    }

    /// Bind the endpoint with project registration only.
    pub fn start_with_project_store(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        project_store: Arc<dyn ProjectStore>,
    ) -> Result<Self, IpcError> {
        Self::start_inner(
            paths,
            status,
            None,
            None,
            Some(project_store),
            IpcOptions::default(),
        )
    }

    /// Bind the endpoint with terminal, layout, and project persistence stores.
    pub fn start_with_terminal_service_and_stores(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        terminal: Arc<TerminalService>,
        layout_store: Arc<dyn WorkspaceLayoutStore>,
        project_store: Arc<dyn ProjectStore>,
    ) -> Result<Self, IpcError> {
        Self::start_inner(
            paths,
            status,
            Some(terminal),
            Some(layout_store),
            Some(project_store),
            IpcOptions::default(),
        )
    }

    fn start_inner(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        terminal: Option<Arc<TerminalService>>,
        layout_store: Option<Arc<dyn WorkspaceLayoutStore>>,
        project_store: Option<Arc<dyn ProjectStore>>,
        options: IpcOptions,
    ) -> Result<Self, IpcError> {
        #[cfg(windows)]
        let pipe_name = windows_pipe_name(paths)
            .ok_or_else(|| IpcError::Endpoint("current-user SID is unavailable".to_string()))?;
        let listener = create_listener(paths)?;
        #[cfg(unix)]
        let socket_path = paths.local_socket_path();

        let stop = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(Mutex::new(Vec::new()));
        let accept_stop = Arc::clone(&stop);
        let accept_connections = Arc::clone(&connections);
        let accept_status = Arc::new(status);
        let accept_terminal = terminal;
        let accept_layout = layout_store;
        let accept_project = project_store;
        let accept_thread = thread::Builder::new()
            .name("muxyard-ipc-accept".to_string())
            .spawn(move || {
                accept_loop(
                    listener,
                    accept_stop,
                    accept_connections,
                    accept_status,
                    accept_terminal,
                    accept_layout,
                    accept_project,
                    options,
                );
            })
            .map_err(IpcError::Io)?;

        Ok(Self {
            stop,
            accept_thread: Some(accept_thread),
            connections,
            #[cfg(unix)]
            socket_path,
            #[cfg(windows)]
            pipe_name,
        })
    }

    /// Stop accepting clients and wait for bounded connection workers.
    pub fn stop(&mut self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        #[cfg(windows)]
        if let Ok(name) = self.pipe_name.as_str().to_ns_name::<GenericNamespaced>() {
            // Wake the blocking accept loop so it can observe the stop flag.
            let _ = LocalSocketStream::connect(name);
        }
        if let Some(thread) = self.accept_thread.take()
            && thread.join().is_err()
        {
            warn!("local IPC accept thread panicked");
        }
        if let Ok(mut connections) = self.connections.lock() {
            for thread in connections.drain(..) {
                if thread.join().is_err() {
                    warn!("local IPC connection thread panicked");
                }
            }
        }
        #[cfg(unix)]
        {
            let _ = fs::remove_file(&self.socket_path);
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn accept_loop(
    listener: LocalSocketListener,
    stop: Arc<AtomicBool>,
    connections: Arc<Mutex<Vec<JoinHandle<()>>>>,
    status: Arc<HostStatusResponse>,
    terminal: Option<Arc<TerminalService>>,
    layout_store: Option<Arc<dyn WorkspaceLayoutStore>>,
    project_store: Option<Arc<dyn ProjectStore>>,
    options: IpcOptions,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok(stream) => {
                let connection_stop = Arc::clone(&stop);
                let connection_status = Arc::clone(&status);
                let connection_terminal = terminal.clone();
                let connection_layout = layout_store.clone();
                let connection_project = project_store.clone();
                match thread::Builder::new()
                    .name("muxyard-ipc-connection".to_string())
                    .spawn(move || {
                        if let Err(error) = handle_connection(
                            stream,
                            connection_status.as_ref(),
                            connection_terminal.as_deref(),
                            connection_layout.as_deref(),
                            connection_project.as_deref(),
                            connection_stop.as_ref(),
                            options,
                        ) {
                            warn!(%error, "local IPC connection ended");
                        }
                    }) {
                    Ok(handle) => {
                        if let Ok(mut connections) = connections.lock() {
                            connections.retain(|thread| !thread.is_finished());
                            connections.push(handle);
                        }
                    }
                    Err(error) => warn!(%error, "local IPC connection worker could not start"),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                warn!(%error, "local IPC accept failed");
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn handle_connection(
    mut stream: LocalSocketStream,
    status: &HostStatusResponse,
    terminal: Option<&TerminalService>,
    layout_store: Option<&dyn WorkspaceLayoutStore>,
    project_store: Option<&dyn ProjectStore>,
    stop: &AtomicBool,
    options: IpcOptions,
) -> Result<(), IpcError> {
    configure_stream(&stream, options.request_timeout)?;
    verify_peer(&stream)?;

    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let deadline = Instant::now() + options.request_timeout;
        let frame = match read_frame_until(&mut stream, deadline) {
            Ok(Some(frame)) => frame,
            Ok(None) => return Ok(()),
            Err(error) => return Err(error),
        };
        match handle_frame(
            &mut stream,
            &frame,
            status,
            terminal,
            layout_store,
            project_store,
            options.request_timeout,
        )? {
            FrameAction::Continue => {}
            FrameAction::Close => return Ok(()),
            FrameAction::Terminal(subscription) => {
                return stream_terminal(&mut stream, subscription, stop, options.request_timeout);
            }
        }
    }
}

enum FrameAction {
    Continue,
    Close,
    Terminal(TerminalSubscription),
}

fn handle_frame(
    stream: &mut LocalSocketStream,
    frame: &[u8],
    status: &HostStatusResponse,
    terminal: Option<&TerminalService>,
    layout_store: Option<&dyn WorkspaceLayoutStore>,
    project_store: Option<&dyn ProjectStore>,
    timeout: Duration,
) -> Result<FrameAction, IpcError> {
    let request = match decode_control::<Value>(frame) {
        Ok(request) => request,
        Err(error) => {
            let (id, method) = correlation(frame);
            if let Some(id) = id {
                write_error(stream, id, method, error, timeout)?;
            }
            return Ok(FrameAction::Close);
        }
    };

    if request.message_type != ControlMessageType::Request {
        write_error(
            stream,
            request.id,
            request.method,
            invalid_request("Only control requests are accepted on this endpoint."),
            timeout,
        )?;
        return Ok(FrameAction::Close);
    }
    if request.id.0.trim().is_empty() {
        write_error(
            stream,
            request.id,
            request.method,
            invalid_request("Control request identifiers must not be empty."),
            timeout,
        )?;
        return Ok(FrameAction::Close);
    }

    match request.method.as_str() {
        HOST_STATUS_METHOD => {
            if serde_json::from_value::<HostStatusRequest>(request.data).is_err() {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    invalid_request("The host.status request body is invalid."),
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            }
            let data = serde_json::to_value(status)?;
            write_response(
                stream,
                request.id,
                request.method,
                IpcResponse::ok(data),
                timeout,
            )?;
        }
        PROJECT_LIST_METHOD => {
            let Some(project_store) = project_store else {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::UnsupportedCapability,
                        message: "Project registration is not available on this host.".to_string(),
                    },
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            };
            if serde_json::from_value::<muxyard_contracts::ProjectListRequest>(request.data)
                .is_err()
            {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    invalid_request("The project.list request body is invalid."),
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            }
            match project_store.list_projects(&muxyard_contracts::ProjectListRequest {}) {
                Ok(projects) => write_response(
                    stream,
                    request.id,
                    request.method,
                    IpcResponse::ok(muxyard_contracts::ProjectListResponse { projects }),
                    timeout,
                )?,
                Err(_) => write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::Internal,
                        message: "Projects could not be loaded.".to_string(),
                    },
                    timeout,
                )?,
            }
        }
        PROJECT_REGISTER_METHOD => {
            let Some(project_store) = project_store else {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::UnsupportedCapability,
                        message: "Project registration is not available on this host.".to_string(),
                    },
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            };
            let register: muxyard_contracts::ProjectRegisterRequest =
                match serde_json::from_value(request.data) {
                    Ok(register) => register,
                    Err(_) => {
                        write_error(
                            stream,
                            request.id,
                            request.method,
                            invalid_request("The project.register request body is invalid."),
                            timeout,
                        )?;
                        return Ok(FrameAction::Continue);
                    }
                };
            match project_store.register_project(&register) {
                Ok(project) => write_response(
                    stream,
                    request.id,
                    request.method,
                    IpcResponse::ok(project),
                    timeout,
                )?,
                Err(_) => write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::InvalidRequest,
                        message: "The project directory could not be registered.".to_string(),
                    },
                    timeout,
                )?,
            }
        }
        PROJECT_REMOVE_METHOD => {
            let Some(project_store) = project_store else {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::UnsupportedCapability,
                        message: "Project registration is not available on this host.".to_string(),
                    },
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            };
            let remove: muxyard_contracts::ProjectRemoveRequest =
                match serde_json::from_value(request.data) {
                    Ok(remove) => remove,
                    Err(_) => {
                        write_error(
                            stream,
                            request.id,
                            request.method,
                            invalid_request("The project.remove request body is invalid."),
                            timeout,
                        )?;
                        return Ok(FrameAction::Continue);
                    }
                };
            match project_store.remove_project(&remove) {
                Ok(removed) => write_response(
                    stream,
                    request.id,
                    request.method,
                    IpcResponse::ok(muxyard_contracts::ProjectRemoveResponse { removed }),
                    timeout,
                )?,
                Err(_) => write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::Internal,
                        message: "The project could not be removed.".to_string(),
                    },
                    timeout,
                )?,
            }
        }
        WORKSPACE_LAYOUT_GET_METHOD => {
            let Some(layout_store) = layout_store else {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::UnsupportedCapability,
                        message: "Workspace layout persistence is not available on this host."
                            .to_string(),
                    },
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            };
            if serde_json::from_value::<WorkspaceLayoutGetRequest>(request.data).is_err() {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    invalid_request("The workspace.layout.get request body is invalid."),
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            }
            match layout_store.load_layout() {
                Ok(snapshot) => write_response(
                    stream,
                    request.id,
                    request.method,
                    IpcResponse::ok(snapshot),
                    timeout,
                )?,
                Err(_) => write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::Internal,
                        message: "The workspace layout could not be loaded.".to_string(),
                    },
                    timeout,
                )?,
            }
        }
        WORKSPACE_LAYOUT_SAVE_METHOD => {
            let Some(layout_store) = layout_store else {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::UnsupportedCapability,
                        message: "Workspace layout persistence is not available on this host."
                            .to_string(),
                    },
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            };
            let save: WorkspaceLayoutSaveRequest = match serde_json::from_value(request.data) {
                Ok(save) => save,
                Err(_) => {
                    write_error(
                        stream,
                        request.id,
                        request.method,
                        invalid_request("The workspace.layout.save request body is invalid."),
                        timeout,
                    )?;
                    return Ok(FrameAction::Continue);
                }
            };
            match layout_store.save_layout(&save.layout) {
                Ok(()) => write_response(
                    stream,
                    request.id,
                    request.method,
                    IpcResponse::ok(()),
                    timeout,
                )?,
                Err(_) => write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::InvalidRequest,
                        message: "The workspace layout could not be saved.".to_string(),
                    },
                    timeout,
                )?,
            }
        }
        TERMINAL_ATTACH_METHOD => {
            let Some(terminal) = terminal else {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::UnsupportedCapability,
                        message: "Terminal streams are not available on this host.".to_string(),
                    },
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            };
            let attach: TerminalAttachRequest = match serde_json::from_value(request.data) {
                Ok(attach) => attach,
                Err(_) => {
                    write_error(
                        stream,
                        request.id,
                        request.method,
                        invalid_request("The terminal.attach request body is invalid."),
                        timeout,
                    )?;
                    return Ok(FrameAction::Continue);
                }
            };
            let subscription = match terminal.attach(attach.stream_id, attach.queue_bytes) {
                Ok(subscription) => subscription,
                Err(error) => {
                    write_error(
                        stream,
                        request.id,
                        request.method,
                        terminal_service_error(error),
                        timeout,
                    )?;
                    return Ok(FrameAction::Continue);
                }
            };
            let response = TerminalAttachResponse {
                stream_id: subscription.stream_id(),
                queue_bytes: u32::try_from(subscription.queue_capacity()).unwrap_or(u32::MAX),
                max_frame_bytes: MAX_TERMINAL_PAYLOAD as u32,
                next_sequence: subscription.next_sequence(),
            };
            write_response(
                stream,
                request.id,
                request.method,
                IpcResponse::ok(response),
                timeout,
            )?;
            return Ok(FrameAction::Terminal(subscription));
        }
        TERMINAL_SNAPSHOT_METHOD => {
            let Some(terminal) = terminal else {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::UnsupportedCapability,
                        message: "Terminal sessions are not available on this host.".to_string(),
                    },
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            };
            let snapshot: TerminalSnapshotRequest = match serde_json::from_value(request.data) {
                Ok(snapshot) => snapshot,
                Err(_) => {
                    write_error(
                        stream,
                        request.id,
                        request.method,
                        invalid_request("The terminal.snapshot request body is invalid."),
                        timeout,
                    )?;
                    return Ok(FrameAction::Continue);
                }
            };
            match terminal.snapshot(&snapshot) {
                Ok(data) => write_response(
                    stream,
                    request.id,
                    request.method,
                    IpcResponse::ok(TerminalSnapshotResponse { data }),
                    timeout,
                )?,
                Err(error) => write_error(
                    stream,
                    request.id,
                    request.method,
                    terminal_service_error(error),
                    timeout,
                )?,
            }
        }
        TERMINAL_SPAWN_METHOD => {
            let Some(terminal) = terminal else {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::UnsupportedCapability,
                        message: "Terminal sessions are not available on this host.".to_string(),
                    },
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            };
            let spawn: TerminalSpawnRequest = match serde_json::from_value(request.data) {
                Ok(spawn) => spawn,
                Err(_) => {
                    write_error(
                        stream,
                        request.id,
                        request.method,
                        invalid_request("The terminal.spawn request body is invalid."),
                        timeout,
                    )?;
                    return Ok(FrameAction::Continue);
                }
            };
            match terminal.spawn(spawn) {
                Ok(response) => write_response(
                    stream,
                    request.id,
                    request.method,
                    IpcResponse::ok(response),
                    timeout,
                )?,
                Err(error) => write_error(
                    stream,
                    request.id,
                    request.method,
                    terminal_service_error(error),
                    timeout,
                )?,
            }
        }
        TERMINAL_INPUT_METHOD => {
            let Some(terminal) = terminal else {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::UnsupportedCapability,
                        message: "Terminal sessions are not available on this host.".to_string(),
                    },
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            };
            let input: TerminalInputRequest = match serde_json::from_value(request.data) {
                Ok(input) => input,
                Err(_) => {
                    write_error(
                        stream,
                        request.id,
                        request.method,
                        invalid_request("The terminal.input request body is invalid."),
                        timeout,
                    )?;
                    return Ok(FrameAction::Continue);
                }
            };
            match terminal.input(&input) {
                Ok(()) => write_response(
                    stream,
                    request.id,
                    request.method,
                    IpcResponse::ok(()),
                    timeout,
                )?,
                Err(error) => write_error(
                    stream,
                    request.id,
                    request.method,
                    terminal_service_error(error),
                    timeout,
                )?,
            }
        }
        TERMINAL_RESIZE_METHOD => {
            let Some(terminal) = terminal else {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::UnsupportedCapability,
                        message: "Terminal sessions are not available on this host.".to_string(),
                    },
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            };
            let resize: TerminalResizeRequest = match serde_json::from_value(request.data) {
                Ok(resize) => resize,
                Err(_) => {
                    write_error(
                        stream,
                        request.id,
                        request.method,
                        invalid_request("The terminal.resize request body is invalid."),
                        timeout,
                    )?;
                    return Ok(FrameAction::Continue);
                }
            };
            match terminal.resize(&resize) {
                Ok(()) => write_response(
                    stream,
                    request.id,
                    request.method,
                    IpcResponse::ok(()),
                    timeout,
                )?,
                Err(error) => write_error(
                    stream,
                    request.id,
                    request.method,
                    terminal_service_error(error),
                    timeout,
                )?,
            }
        }
        TERMINAL_CLOSE_METHOD => {
            let Some(terminal) = terminal else {
                write_error(
                    stream,
                    request.id,
                    request.method,
                    ApiError {
                        code: ErrorCode::UnsupportedCapability,
                        message: "Terminal sessions are not available on this host.".to_string(),
                    },
                    timeout,
                )?;
                return Ok(FrameAction::Continue);
            };
            let close: TerminalCloseRequest = match serde_json::from_value(request.data) {
                Ok(close) => close,
                Err(_) => {
                    write_error(
                        stream,
                        request.id,
                        request.method,
                        invalid_request("The terminal.close request body is invalid."),
                        timeout,
                    )?;
                    return Ok(FrameAction::Continue);
                }
            };
            match terminal.close(&close) {
                Ok(_) => write_response(
                    stream,
                    request.id,
                    request.method,
                    IpcResponse::ok(()),
                    timeout,
                )?,
                Err(error) => write_error(
                    stream,
                    request.id,
                    request.method,
                    terminal_service_error(error),
                    timeout,
                )?,
            }
        }
        _ => {
            write_error(
                stream,
                request.id,
                request.method,
                ApiError {
                    code: ErrorCode::UnsupportedCapability,
                    message: "The requested local control method is not supported.".to_string(),
                },
                timeout,
            )?;
        }
    }
    Ok(FrameAction::Continue)
}

fn terminal_service_error(error: TerminalServiceError) -> ApiError {
    match error {
        TerminalServiceError::UnknownSession(_) => ApiError {
            code: ErrorCode::NotFound,
            message: "The requested terminal session does not exist.".to_string(),
        },
        TerminalServiceError::InvalidStreamId(_) => ApiError {
            code: ErrorCode::InvalidRequest,
            message: "The requested terminal stream identifier is invalid.".to_string(),
        },
        TerminalServiceError::InvalidHistoryId(_) => ApiError {
            code: ErrorCode::InvalidRequest,
            message: "The requested terminal history identifier is invalid.".to_string(),
        },
        TerminalServiceError::Store(_) => ApiError {
            code: ErrorCode::Internal,
            message: "The terminal session metadata could not be persisted.".to_string(),
        },
        TerminalServiceError::Hub(TerminalHubError::TooManySessions(_)) => ApiError {
            code: ErrorCode::RateLimited,
            message: "Too many terminal streams are active.".to_string(),
        },
        TerminalServiceError::Hub(TerminalHubError::UnknownSession(_)) => ApiError {
            code: ErrorCode::NotFound,
            message: "The requested terminal stream does not exist.".to_string(),
        },
        TerminalServiceError::Hub(
            TerminalHubError::InvalidQueue { .. } | TerminalHubError::InvalidLimits(_),
        ) => ApiError {
            code: ErrorCode::InvalidRequest,
            message: "The requested terminal queue size is invalid.".to_string(),
        },
        TerminalServiceError::Hub(TerminalHubError::Closed) => ApiError {
            code: ErrorCode::NotFound,
            message: "The requested terminal stream is closed.".to_string(),
        },
        TerminalServiceError::Pty(error) => match error {
            muxyard_terminal::PtyError::Closed => ApiError {
                code: ErrorCode::Conflict,
                message: "The terminal session is closed.".to_string(),
            },
            muxyard_terminal::PtyError::Platform(_)
            | muxyard_terminal::PtyError::InvalidSize
            | muxyard_terminal::PtyError::InvalidOutputTail
            | muxyard_terminal::PtyError::Pty(_) => ApiError {
                code: ErrorCode::InvalidRequest,
                message: "The terminal session could not be started or updated.".to_string(),
            },
            muxyard_terminal::PtyError::Hub(_) | muxyard_terminal::PtyError::Io(_) => ApiError {
                code: ErrorCode::Internal,
                message: "The terminal session operation failed.".to_string(),
            },
        },
    }
}

fn stream_terminal(
    stream: &mut LocalSocketStream,
    subscription: TerminalSubscription,
    stop: &AtomicBool,
    timeout: Duration,
) -> Result<(), IpcError> {
    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        if let Some(frame) = subscription.wait_next(Duration::from_millis(100)) {
            let encoded = frame.encode();
            write_frame_until(stream, &encoded, Instant::now() + timeout)?;
        } else if subscription.is_detached() {
            return Err(IpcError::TerminalDetached);
        }
    }
}

fn write_response<T: Serialize>(
    stream: &mut LocalSocketStream,
    id: Id,
    method: String,
    data: IpcResponse<T>,
    timeout: Duration,
) -> Result<(), IpcError> {
    let envelope = ControlEnvelope {
        v: ProtocolVersion,
        id,
        message_type: ControlMessageType::Response,
        method,
        data,
    };
    let bytes = serde_json::to_vec(&envelope)?;
    write_frame_until(stream, &bytes, Instant::now() + timeout)
}

fn write_error(
    stream: &mut LocalSocketStream,
    id: Id,
    method: String,
    error: ApiError,
    timeout: Duration,
) -> Result<(), IpcError> {
    write_response(
        stream,
        id,
        method,
        IpcResponse::<Value>::error(error),
        timeout,
    )
}

fn invalid_request(message: &str) -> ApiError {
    ApiError {
        code: ErrorCode::InvalidRequest,
        message: message.to_string(),
    }
}

fn correlation(frame: &[u8]) -> (Option<Id>, String) {
    let Ok(value) = serde_json::from_slice::<Value>(frame) else {
        return (None, String::new());
    };
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(|id| Id(id.to_string()));
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    (id, method)
}

#[cfg(unix)]
fn configure_stream(stream: &LocalSocketStream, timeout: Duration) -> Result<(), IpcError> {
    // Some Unix platforms propagate the listener's nonblocking flag to accepted
    // sockets; explicitly restore blocking reads before applying timeouts.
    stream.set_nonblocking(false)?;
    stream.set_recv_timeout(Some(timeout))?;
    stream.set_send_timeout(Some(timeout))?;
    Ok(())
}

#[cfg(windows)]
fn configure_stream(stream: &LocalSocketStream, _timeout: Duration) -> Result<(), IpcError> {
    // Named pipes do not expose socket-style timeouts. The frame reader retries
    // empty/no-data Windows pipe reads while a deadline is active.
    stream.set_nonblocking(true)?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn configure_stream(_stream: &LocalSocketStream, _timeout: Duration) -> Result<(), IpcError> {
    Err(IpcError::UnsupportedPlatform)
}

#[cfg(unix)]
fn verify_peer(stream: &LocalSocketStream) -> Result<(), IpcError> {
    let peer = stream.peer_creds()?;
    if peer.euid() != Some(nix::unistd::geteuid().as_raw()) {
        return Err(IpcError::UnauthorizedPeer);
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_peer(_stream: &LocalSocketStream) -> Result<(), IpcError> {
    Ok(())
}

#[cfg(unix)]
fn create_listener(paths: &PlatformPaths) -> Result<LocalSocketListener, IpcError> {
    verify_private_runtime_dir(&paths.runtime_dir)?;
    let socket_path = paths.local_socket_path();
    let name = socket_path.as_path().to_fs_name::<GenericFilePath>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .nonblocking(ListenerNonblockingMode::Accept)
        .try_overwrite(true)
        .max_spin_time(Duration::from_secs(1))
        .create_sync()?;
    if let Err(error) = secure_socket(&socket_path) {
        drop(listener);
        let _ = fs::remove_file(&socket_path);
        return Err(error);
    }
    Ok(listener)
}

#[cfg(unix)]
fn verify_private_runtime_dir(path: &Path) -> Result<(), IpcError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(IpcError::InsecureEndpoint);
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(IpcError::InsecureEndpoint);
    }
    Ok(())
}

#[cfg(unix)]
fn secure_socket(path: &Path) -> Result<(), IpcError> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_socket()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(IpcError::InsecureEndpoint);
    }
    Ok(())
}

#[cfg(windows)]
fn create_listener(paths: &PlatformPaths) -> Result<LocalSocketListener, IpcError> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    let sid = windows_user_sid()
        .ok_or_else(|| IpcError::Endpoint("current-user SID is unavailable".to_string()))?;
    let pipe_name = windows_pipe_name_for_sid(&sid, &paths.runtime_dir);
    let name = pipe_name.as_str().to_ns_name::<GenericNamespaced>()?;
    let sddl = format!("D:P(A;;GA;;;{sid})");
    let wide =
        U16CString::from_str(&sddl).map_err(|error| IpcError::Endpoint(error.to_string()))?;
    let descriptor = SecurityDescriptor::deserialize(wide.as_ucstr()).map_err(IpcError::Io)?;
    ListenerOptions::new()
        .name(name)
        .nonblocking(ListenerNonblockingMode::Neither)
        .security_descriptor(descriptor)
        .create_sync()
        .map_err(IpcError::Io)
}

#[cfg(windows)]
fn windows_user_sid() -> Option<String> {
    static SID: OnceLock<Option<String>> = OnceLock::new();
    SID.get_or_init(discover_windows_user_sid).clone()
}

#[cfg(windows)]
fn discover_windows_user_sid() -> Option<String> {
    let output = Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let sid = text
        .split(',')
        .nth(1)?
        .trim()
        .trim_matches('"')
        .trim()
        .to_string();
    if sid.starts_with("S-1-")
        && sid
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        Some(sid)
    } else {
        None
    }
}

#[cfg(windows)]
pub(crate) fn windows_pipe_name(paths: &PlatformPaths) -> Option<String> {
    windows_user_sid().map(|sid| windows_pipe_name_for_sid(&sid, &paths.runtime_dir))
}

#[cfg(windows)]
fn windows_pipe_name_for_sid(sid: &str, runtime_dir: &Path) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(sid.as_bytes());
    hasher.update([0]);
    hasher.update(runtime_dir.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let suffix = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("muxyard-{suffix}-v1")
}

#[cfg(not(any(unix, windows)))]
fn create_listener(paths: &PlatformPaths) -> Result<LocalSocketListener, IpcError> {
    Err(IpcError::UnsupportedPlatform)
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn pipe_names_are_scoped_to_the_runtime_directory() {
        let sid = "S-1-5-21-1000";
        let first = windows_pipe_name_for_sid(sid, Path::new("C:\\Temp\\one"));
        let second = windows_pipe_name_for_sid(sid, Path::new("C:\\Temp\\two"));
        assert_eq!(
            first,
            windows_pipe_name_for_sid(sid, Path::new("C:\\Temp\\one"))
        );
        assert_ne!(first, second);
    }
}
