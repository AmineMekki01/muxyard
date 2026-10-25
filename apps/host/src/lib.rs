//! Host bootstrap, single-instance lifecycle, and detached start/stop/status.

mod lock;
mod runtime;

use std::{
    env, io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use muxyard_contracts::PROTOCOL_VERSION;
use muxyard_platform::{
    ConfigError, ConfigStore, HostConfig, LoggingError, PlatformError, PlatformPaths,
    current_process_identity, init_file_logging,
};
use muxyard_storage::{
    Database, DatabaseOptions, HostInstance, HostState, StorageError, mark_stale_terminal_sessions,
    reconcile_stale_host_instances, record_host_instance,
};
use uuid::Uuid;

pub use lock::{HostLock, HostLockInfo, LockError, LockState};
pub use runtime::{
    HostStatus, StartResult, StopResult, run_host, start_host, status_host, stop_host,
};

/// Initialized host state for the current one-shot invocation.
#[derive(Debug)]
pub struct Bootstrap {
    /// Resolved per-user locations.
    pub paths: PlatformPaths,
    /// Validated configuration.
    pub config: HostConfig,
    /// Newly recorded process instance.
    pub host_instance_id: String,
    /// Schema version after migrations.
    pub schema_version: i64,
}

/// Host bootstrap or lifecycle failure.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// Per-user paths could not be resolved or secured.
    #[error(transparent)]
    Platform(#[from] PlatformError),
    /// Configuration could not be loaded or created.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// Logging could not be initialized.
    #[error(transparent)]
    Logging(#[from] LoggingError),
    /// Storage could not be opened or migrated.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// The single-instance lock failed.
    #[error(transparent)]
    Lock(#[from] LockError),
    /// The operating-system user name is unavailable.
    #[error("operating-system user name is not available")]
    MissingOsUser,
    /// The OS signal handler could not be installed.
    #[error("host signal handler could not be installed: {0}")]
    Signal(String),
    /// A process or startup-file operation failed.
    #[error("host I/O failed for {path}: {source}")]
    Io {
        /// Path or executable involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// The detached host exited before becoming ready.
    #[error("host exited before becoming ready (status {status:?}): {detail}")]
    StartFailed {
        /// Exit status, if any.
        status: Option<i32>,
        /// Startup diagnostics, bounded by the caller.
        detail: String,
    },
    /// The detached host did not become ready in time.
    #[error("host did not become ready: {detail}")]
    StartTimeout {
        /// Startup diagnostics, bounded by the caller.
        detail: String,
    },
    /// A live host did not stop in time.
    #[error("host pid {pid} did not stop within the timeout")]
    StopTimeout {
        /// Host process ID.
        pid: u32,
    },
}

/// Resolve current-user paths and initialize configuration, logging, and storage.
///
/// Call at most once per process because logging installs a global subscriber.
pub fn initialize() -> Result<Bootstrap, HostError> {
    initialize_paths(PlatformPaths::for_current_user()?)
}

/// Initialize an explicit layout. Useful for isolated tests and embedding.
///
/// Call at most once per process because logging installs a global subscriber.
pub fn initialize_paths(paths: PlatformPaths) -> Result<Bootstrap, HostError> {
    paths.ensure()?;
    let identity = current_process_identity()?;
    let instance_id = Uuid::new_v4().to_string();
    let lock = HostLock::acquire(
        &paths.host_lock_path(),
        HostLockInfo::new(instance_id.clone(), &identity, now_ms()),
    )?;
    lock::clear_stop_request(&paths.stop_request_path())?;
    lock::clear_ready_marker(&paths.host_ready_path())?;

    let config_store = ConfigStore::new(paths.config_path());
    let config = config_store.load_or_create_default()?;
    init_file_logging(&config.logging, &paths.log_path())?;

    let mut database =
        Database::open_with_options(paths.database_path(), DatabaseOptions::default())?;
    let migration = database.migrate()?;
    let now = now_ms();
    reconcile_stale_host_instances(database.connection(), &instance_id, now)?;
    mark_stale_terminal_sessions(database.connection(), &instance_id, now)?;

    let instance = HostInstance {
        id: instance_id.clone(),
        pid: identity.pid,
        os_user: os_user()?,
        process_start_token: identity.start_identity,
        host_version: env!("CARGO_PKG_VERSION").to_string(),
        protocol_version: PROTOCOL_VERSION,
        state: HostState::Stopped,
        started_ms: now,
        heartbeat_ms: now,
    };
    record_host_instance(database.connection(), &instance)?;

    drop(database);
    drop(lock);
    tracing::info!(
        schema_version = migration.current_version,
        applied_migrations = migration.applied.len(),
        "host bootstrap complete"
    );

    Ok(Bootstrap {
        paths,
        config,
        host_instance_id: instance_id,
        schema_version: migration.current_version,
    })
}

pub(crate) fn os_user() -> Result<String, HostError> {
    #[cfg(unix)]
    let user = env::var("USER").or_else(|_| env::var("LOGNAME"));
    #[cfg(windows)]
    let user = env::var("USERNAME");
    #[cfg(not(any(unix, windows)))]
    let user = env::var("USER");

    user.ok()
        .filter(|value| !value.is_empty())
        .ok_or(HostError::MissingOsUser)
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
