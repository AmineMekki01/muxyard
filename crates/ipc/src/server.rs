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
use std::process::Command;

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, prelude::*};
use muxyard_contracts::{
    ApiError, ControlEnvelope, ControlMessageType, ErrorCode, Id, ProtocolVersion, decode_control,
};
use muxyard_platform::PlatformPaths;
use muxyard_terminal::{
    MAX_TERMINAL_PAYLOAD, TerminalAttachRequest, TerminalAttachResponse, TerminalHub,
    TerminalHubError, TerminalSubscription,
};
use serde::Serialize;
use serde_json::Value;
use tracing::warn;

use crate::{
    HOST_STATUS_METHOD, HostStatusRequest, HostStatusResponse, IpcError, IpcResponse,
    TERMINAL_ATTACH_METHOD, read_frame_until, write_frame_until,
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
}

impl IpcServer {
    /// Bind the protected local endpoint and begin accepting clients.
    ///
    /// On Unix the runtime directory must already be private and the socket is
    /// made owner-only after binding. On Windows the named pipe receives a
    /// security descriptor for the current user's SID.
    pub fn start(paths: &PlatformPaths, status: HostStatusResponse) -> Result<Self, IpcError> {
        Self::start_inner(paths, status, None, IpcOptions::default())
    }

    /// Bind the endpoint with explicit timeout settings.
    pub fn start_with_options(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        options: IpcOptions,
    ) -> Result<Self, IpcError> {
        Self::start_inner(paths, status, None, options)
    }

    /// Bind the endpoint and enable terminal stream attachments.
    pub fn start_with_terminal(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        terminal: Arc<TerminalHub>,
    ) -> Result<Self, IpcError> {
        Self::start_inner(paths, status, Some(terminal), IpcOptions::default())
    }

    /// Bind the endpoint with terminal streams and explicit timeouts.
    pub fn start_with_terminal_and_options(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        terminal: Arc<TerminalHub>,
        options: IpcOptions,
    ) -> Result<Self, IpcError> {
        Self::start_inner(paths, status, Some(terminal), options)
    }

    fn start_inner(
        paths: &PlatformPaths,
        status: HostStatusResponse,
        terminal: Option<Arc<TerminalHub>>,
        options: IpcOptions,
    ) -> Result<Self, IpcError> {
        let listener = create_listener(paths)?;
        #[cfg(unix)]
        let socket_path = paths.local_socket_path();

        let stop = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(Mutex::new(Vec::new()));
        let accept_stop = Arc::clone(&stop);
        let accept_connections = Arc::clone(&connections);
        let accept_status = Arc::new(status);
        let accept_terminal = terminal;
        let accept_thread = thread::Builder::new()
            .name("muxyard-ipc-accept".to_string())
            .spawn(move || {
                accept_loop(
                    listener,
                    accept_stop,
                    accept_connections,
                    accept_status,
                    accept_terminal,
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
        })
    }

    /// Stop accepting clients and wait for bounded connection workers.
    pub fn stop(&mut self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
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

fn accept_loop(
    listener: LocalSocketListener,
    stop: Arc<AtomicBool>,
    connections: Arc<Mutex<Vec<JoinHandle<()>>>>,
    status: Arc<HostStatusResponse>,
    terminal: Option<Arc<TerminalHub>>,
    options: IpcOptions,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok(stream) => {
                let connection_stop = Arc::clone(&stop);
                let connection_status = Arc::clone(&status);
                let connection_terminal = terminal.clone();
                match thread::Builder::new()
                    .name("muxyard-ipc-connection".to_string())
                    .spawn(move || {
                        if let Err(error) = handle_connection(
                            stream,
                            connection_status.as_ref(),
                            connection_terminal.as_deref(),
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
    terminal: Option<&TerminalHub>,
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
    terminal: Option<&TerminalHub>,
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
            let subscription = match terminal.subscribe(attach.stream_id, attach.queue_bytes) {
                Ok(subscription) => subscription,
                Err(error) => {
                    write_error(
                        stream,
                        request.id,
                        request.method,
                        terminal_hub_error(error),
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

fn terminal_hub_error(error: TerminalHubError) -> ApiError {
    match error {
        TerminalHubError::UnknownSession(_) => ApiError {
            code: ErrorCode::NotFound,
            message: "The requested terminal stream does not exist.".to_string(),
        },
        TerminalHubError::TooManySessions(_) => ApiError {
            code: ErrorCode::RateLimited,
            message: "Too many terminal streams are active.".to_string(),
        },
        TerminalHubError::InvalidQueue { .. } | TerminalHubError::InvalidLimits(_) => ApiError {
            code: ErrorCode::InvalidRequest,
            message: "The requested terminal queue size is invalid.".to_string(),
        },
        TerminalHubError::Closed => ApiError {
            code: ErrorCode::NotFound,
            message: "The requested terminal stream is closed.".to_string(),
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
    // Named pipes in this transport do not expose socket-style timeouts.
    // Nonblocking I/O plus the frame deadline provides the bound instead.
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
fn create_listener(_paths: &PlatformPaths) -> Result<LocalSocketListener, IpcError> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    let sid = windows_user_sid()
        .ok_or_else(|| IpcError::Endpoint("current-user SID is unavailable".to_string()))?;
    let pipe_name = windows_pipe_name_for_sid(&sid);
    let name = pipe_name.as_str().to_ns_name::<GenericNamespaced>()?;
    let sddl = format!("D:P(A;;GA;;;{sid})");
    let wide =
        U16CString::from_str(&sddl).map_err(|error| IpcError::Endpoint(error.to_string()))?;
    let descriptor = SecurityDescriptor::deserialize(wide.as_ucstr()).map_err(IpcError::Io)?;
    ListenerOptions::new()
        .name(name)
        .nonblocking(ListenerNonblockingMode::Accept)
        .security_descriptor(descriptor)
        .create_sync()
        .map_err(IpcError::Io)
}

#[cfg(windows)]
fn windows_user_sid() -> Option<String> {
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
pub(crate) fn windows_pipe_name() -> Option<String> {
    windows_user_sid().map(|sid| windows_pipe_name_for_sid(&sid))
}

#[cfg(windows)]
fn windows_pipe_name_for_sid(sid: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(sid.as_bytes());
    let suffix = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("muxyard-{suffix}-v1")
}

#[cfg(not(any(unix, windows)))]
fn create_listener(_paths: &PlatformPaths) -> Result<LocalSocketListener, IpcError> {
    Err(IpcError::UnsupportedPlatform)
}
