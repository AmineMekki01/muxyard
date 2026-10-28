//! Single-instance host lifecycle and detached start/stop/status operations.

use std::{
    env, fs, io,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use muxyard_contracts::PROTOCOL_VERSION;
use muxyard_ipc::{HostStatusResponse, IpcServer};
use muxyard_platform::{
    ConfigStore, PlatformPaths, configure_detached, current_process_identity, ensure_private_dir,
    open_private_append, write_private_file,
};
use muxyard_storage::{
    Database, DatabaseOptions, HostInstance, HostState, mark_stale_terminal_sessions,
    reconcile_stale_host_instances, record_host_instance, update_host_instance_state,
};
use muxyard_terminal::{
    DEFAULT_OUTPUT_TAIL_BYTES, TerminalHub, TerminalLimits, TerminalService, TerminalSessionStore,
};
use uuid::Uuid;

use crate::{
    HostError,
    lock::{
        HostLock, HostLockInfo, LockError, LockState, clear_ready_marker, clear_stop_request,
        ready_marker_matches, stop_requested, write_ready_marker, write_stop_request,
    },
    now_ms, os_user,
    store::HostStore,
};

const START_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const HEARTBEAT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Outcome of a start request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartResult {
    /// A new detached host became ready.
    Started(HostLockInfo),
    /// A ready host already owned the lock.
    AlreadyRunning(HostLockInfo),
}

/// Outcome of a stop request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopResult {
    /// A host stopped gracefully.
    Stopped(HostLockInfo),
    /// No live host was present.
    NotRunning,
}

/// Current host status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostStatus {
    /// A live host owns the lock.
    Running {
        /// Lock record.
        info: HostLockInfo,
        /// Ready marker matches the live instance.
        ready: bool,
    },
    /// A stale lock remains.
    Stale(HostLockInfo),
    /// No lock is present.
    NotRunning,
}

/// A live host process holding the single-instance lock and database.
pub struct HostRuntime {
    paths: PlatformPaths,
    database: Arc<Mutex<Database>>,
    lock: HostLock,
    instance_id: String,
    stop: Arc<AtomicBool>,
    ipc: IpcServer,
    terminal: Arc<TerminalService>,
}

impl HostRuntime {
    /// Acquire the lock and prepare logging, storage, and host identity.
    pub fn start(paths: PlatformPaths) -> Result<Self, HostError> {
        paths.ensure()?;
        let identity = current_process_identity()?;
        let instance_id = Uuid::new_v4().to_string();
        let lock_info = HostLockInfo::new(instance_id.clone(), &identity, now_ms());
        let lock = HostLock::acquire(&paths.host_lock_path(), lock_info)?;
        clear_ready_marker(&paths.host_ready_path())?;

        let config = ConfigStore::new(paths.config_path()).load_or_create_default()?;
        muxyard_platform::init_file_logging(&config.logging, &paths.log_path())?;

        let mut database =
            Database::open_with_options(paths.database_path(), DatabaseOptions::default())?;
        let migration = database.migrate()?;

        let now = now_ms();
        let instance = HostInstance {
            id: instance_id.clone(),
            pid: identity.pid,
            os_user: os_user()?,
            process_start_token: identity.start_identity.clone(),
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION,
            state: HostState::Starting,
            started_ms: now,
            heartbeat_ms: now,
        };
        record_host_instance(database.connection(), &instance)?;
        reconcile_stale_host_instances(database.connection(), &instance_id, now)?;
        mark_stale_terminal_sessions(database.connection(), &instance_id, now)?;
        update_host_instance_state(database.connection(), &instance_id, HostState::Ready, now)?;

        let history_dir = paths.data_dir.join("terminal-history");
        ensure_private_dir(&history_dir)?;
        let database = Arc::new(Mutex::new(database));
        let store = Arc::new(HostStore::new(
            Arc::clone(&database),
            instance_id.clone(),
            identity.start_identity.clone(),
        ));

        let stop = Arc::new(AtomicBool::new(false));
        let stop_handler = Arc::clone(&stop);
        if let Err(error) = ctrlc::set_handler(move || stop_handler.store(true, Ordering::SeqCst)) {
            #[cfg(unix)]
            return Err(HostError::Signal(error.to_string()));
            #[cfg(windows)]
            tracing::warn!(%error, "console signal handler unavailable; stop marker remains active");
        }
        let terminal = Arc::new(
            TerminalService::with_store_and_history(
                Arc::new(TerminalHub::new(TerminalLimits::default())?),
                Arc::clone(&store) as Arc<dyn TerminalSessionStore>,
                history_dir,
            )
            .with_output_tail_bytes(
                usize::try_from(config.limits.pty_output_ring_bytes)
                    .unwrap_or(DEFAULT_OUTPUT_TAIL_BYTES),
            ),
        );
        let ipc = IpcServer::start_with_terminal_service_and_store(
            &paths,
            HostStatusResponse {
                host_version: env!("CARGO_PKG_VERSION").to_string(),
                protocol_version: PROTOCOL_VERSION,
                instance_id: instance_id.clone(),
                pid: identity.pid,
                process_start: identity.start_identity.clone(),
                started_ms: lock.info().started_ms,
                ready: true,
            },
            Arc::clone(&terminal),
            store,
        )?;
        write_ready_marker(&paths.host_ready_path(), lock.info())?;

        tracing::info!(
            pid = identity.pid,
            instance_id = %instance_id,
            schema_version = migration.current_version,
            "host ready"
        );

        Ok(Self {
            paths,
            database,
            lock,
            instance_id,
            stop,
            ipc,
            terminal,
        })
    }

    /// Run until a stop marker or OS termination signal is received.
    pub fn run(self) -> Result<(), HostError> {
        let Self {
            paths,
            database,
            lock,
            instance_id,
            stop,
            mut ipc,
            terminal,
        } = self;
        let mut last_heartbeat = Instant::now();
        let reason = loop {
            if stop.load(Ordering::SeqCst) {
                break "signal";
            }
            match stop_requested(&paths.stop_request_path(), &instance_id) {
                Ok(true) => break "request",
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(%error, "ignoring unreadable stop marker");
                    let _ = clear_stop_request(&paths.stop_request_path());
                }
            }
            if last_heartbeat.elapsed() >= HEARTBEAT {
                if let Err(error) = update_host_instance_state(
                    database
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .connection(),
                    &instance_id,
                    HostState::Ready,
                    now_ms(),
                ) {
                    tracing::warn!(%error, "host heartbeat failed");
                }
                last_heartbeat = Instant::now();
            }
            thread::sleep(POLL_INTERVAL);
        };

        tracing::info!(reason, "host stopping");
        ipc.stop();
        terminal.close_all();
        let mut result = Ok(());
        if let Err(error) = update_host_instance_state(
            database
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .connection(),
            &instance_id,
            HostState::Draining,
            now_ms(),
        ) {
            tracing::warn!(%error, "could not record draining state");
            result = Err(HostError::Storage(error));
        }
        if let Err(error) = clear_stop_request(&paths.stop_request_path()) {
            tracing::warn!(%error, "could not clear stop marker");
        }
        if let Err(error) = clear_ready_marker(&paths.host_ready_path()) {
            tracing::warn!(%error, "could not clear ready marker");
        }
        if let Err(error) = update_host_instance_state(
            database
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .connection(),
            &instance_id,
            HostState::Stopped,
            now_ms(),
        ) {
            tracing::warn!(%error, "could not record stopped state");
            if result.is_ok() {
                result = Err(HostError::Storage(error));
            }
        }

        drop(database);
        drop(lock);
        tracing::info!("host stopped");
        result
    }
}

/// Run the host in the current process.
pub fn run_host() -> Result<(), HostError> {
    HostRuntime::start(PlatformPaths::for_current_user()?)?.run()
}

/// Start a detached host, or report an existing ready host.
pub fn start_host() -> Result<StartResult, HostError> {
    let paths = PlatformPaths::for_current_user()?;
    paths.ensure()?;
    match HostLock::inspect(&paths.host_lock_path())? {
        LockState::Running(info) => {
            if ready_marker_matches(&paths.host_ready_path(), &info.instance_id)? {
                return Ok(StartResult::AlreadyRunning(info));
            }
            return wait_for_existing_ready(&paths, info);
        }
        LockState::Corrupt(path) => return Err(LockError::Corrupt(path).into()),
        LockState::Stale(_) | LockState::Missing => {}
    }

    let startup_log = paths.startup_log_path();
    write_private_file(&startup_log, b"")?;
    let stderr = open_private_append(&startup_log)?;
    let executable = env::current_exe().map_err(|error| HostError::Io {
        path: PathBuf::from("current executable"),
        source: error,
    })?;
    let mut command = Command::new(&executable);
    command
        .arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr));
    configure_detached(&mut command);
    let mut child = command.spawn().map_err(|error| HostError::Io {
        path: executable.clone(),
        source: error,
    })?;

    let deadline = Instant::now() + START_TIMEOUT;
    while Instant::now() < deadline {
        match HostLock::inspect(&paths.host_lock_path())? {
            LockState::Running(info)
                if ready_marker_matches(&paths.host_ready_path(), &info.instance_id)? =>
            {
                return Ok(StartResult::Started(info));
            }
            LockState::Running(_) | LockState::Stale(_) | LockState::Missing => {}
            LockState::Corrupt(path) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(LockError::Corrupt(path).into());
            }
        }
        if let Some(status) = child.try_wait().map_err(|error| HostError::Io {
            path: executable.clone(),
            source: error,
        })? {
            return Err(HostError::StartFailed {
                status: status.code(),
                detail: read_startup_detail(&startup_log),
            });
        }
        thread::sleep(Duration::from_millis(100));
    }

    let _ = child.kill();
    let _ = child.wait();
    Err(HostError::StartTimeout {
        detail: read_startup_detail(&startup_log),
    })
}

/// Stop the current user's host gracefully.
pub fn stop_host() -> Result<StopResult, HostError> {
    let paths = PlatformPaths::for_current_user()?;
    match HostLock::inspect(&paths.host_lock_path())? {
        LockState::Running(info) => {
            write_stop_request(&paths.stop_request_path(), &info.instance_id)?;
            let deadline = Instant::now() + STOP_TIMEOUT;
            while Instant::now() < deadline {
                match HostLock::inspect(&paths.host_lock_path())? {
                    LockState::Missing => {
                        let _ = clear_stop_request(&paths.stop_request_path());
                        let _ = clear_ready_marker(&paths.host_ready_path());
                        return Ok(StopResult::Stopped(info));
                    }
                    LockState::Stale(_) => {
                        remove_lock(&paths)?;
                        let _ = clear_stop_request(&paths.stop_request_path());
                        let _ = clear_ready_marker(&paths.host_ready_path());
                        return Ok(StopResult::Stopped(info));
                    }
                    LockState::Running(_) => thread::sleep(Duration::from_millis(100)),
                    LockState::Corrupt(path) => return Err(LockError::Corrupt(path).into()),
                }
            }
            Err(HostError::StopTimeout { pid: info.pid })
        }
        LockState::Stale(_) => {
            remove_lock(&paths)?;
            let _ = clear_stop_request(&paths.stop_request_path());
            let _ = clear_ready_marker(&paths.host_ready_path());
            Ok(StopResult::NotRunning)
        }
        LockState::Missing => {
            let _ = clear_stop_request(&paths.stop_request_path());
            let _ = clear_ready_marker(&paths.host_ready_path());
            Ok(StopResult::NotRunning)
        }
        LockState::Corrupt(path) => Err(LockError::Corrupt(path).into()),
    }
}

/// Read current host status without modifying files.
pub fn status_host() -> Result<HostStatus, HostError> {
    let paths = PlatformPaths::for_current_user()?;
    match HostLock::inspect(&paths.host_lock_path())? {
        LockState::Running(info) => {
            let ready =
                ready_marker_matches(&paths.host_ready_path(), &info.instance_id).unwrap_or(false);
            Ok(HostStatus::Running { info, ready })
        }
        LockState::Stale(info) => Ok(HostStatus::Stale(info)),
        LockState::Missing => Ok(HostStatus::NotRunning),
        LockState::Corrupt(path) => Err(LockError::Corrupt(path).into()),
    }
}

fn wait_for_existing_ready(
    paths: &PlatformPaths,
    info: HostLockInfo,
) -> Result<StartResult, HostError> {
    let deadline = Instant::now() + START_TIMEOUT;
    while Instant::now() < deadline {
        match HostLock::inspect(&paths.host_lock_path())? {
            LockState::Running(current)
                if current.instance_id == info.instance_id
                    && ready_marker_matches(&paths.host_ready_path(), &info.instance_id)? =>
            {
                return Ok(StartResult::AlreadyRunning(current));
            }
            LockState::Running(_) => thread::sleep(Duration::from_millis(100)),
            LockState::Missing | LockState::Stale(_) => {
                return Err(HostError::StartTimeout {
                    detail: "existing host exited before becoming ready".to_string(),
                });
            }
            LockState::Corrupt(path) => return Err(LockError::Corrupt(path).into()),
        }
    }
    Err(HostError::StartTimeout {
        detail: "existing host did not become ready".to_string(),
    })
}

fn remove_lock(paths: &PlatformPaths) -> Result<(), HostError> {
    match fs::remove_file(paths.host_lock_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(HostError::Io {
            path: paths.host_lock_path(),
            source: error,
        }),
    }
}

fn read_startup_detail(path: &std::path::Path) -> String {
    let detail = fs::read_to_string(path).unwrap_or_default();
    let detail = detail.trim();
    if detail.is_empty() {
        "no startup diagnostics".to_string()
    } else if detail.len() > 2_000 {
        detail
            .chars()
            .rev()
            .take(2_000)
            .collect::<String>()
            .chars()
            .rev()
            .collect()
    } else {
        detail.to_string()
    }
}
