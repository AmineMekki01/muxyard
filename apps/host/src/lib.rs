//! One-shot host bootstrap for configuration, logging, and durable storage.

use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};

use muxyard_contracts::PROTOCOL_VERSION;
use muxyard_platform::{
    ConfigError, ConfigStore, HostConfig, LoggingError, PlatformError, PlatformPaths,
    init_file_logging,
};
use muxyard_storage::{
    Database, DatabaseOptions, HostInstance, HostState, StorageError, record_host_instance,
    update_host_instance_state,
};
use uuid::Uuid;

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

/// Host bootstrap failure.
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
    /// The operating-system user name is unavailable.
    #[error("operating-system user name is not available")]
    MissingOsUser,
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

    let config_store = ConfigStore::new(paths.config_path());
    let config = config_store.load_or_create_default()?;
    init_file_logging(&config.logging, &paths.log_path())?;

    let mut database =
        Database::open_with_options(paths.database_path(), DatabaseOptions::default())?;
    let migration = database.migrate()?;

    let now = now_ms();
    let instance = HostInstance {
        id: Uuid::new_v4().to_string(),
        pid: std::process::id(),
        os_user: os_user()?,
        process_start_token: Uuid::new_v4().to_string(),
        host_version: env!("CARGO_PKG_VERSION").to_string(),
        protocol_version: PROTOCOL_VERSION,
        state: HostState::Ready,
        started_ms: now,
        heartbeat_ms: now,
    };

    let transaction = database
        .connection_mut()
        .transaction()
        .map_err(StorageError::from)?;
    record_host_instance(&transaction, &instance)?;
    update_host_instance_state(&transaction, &instance.id, HostState::Stopped, now_ms())?;
    transaction.commit().map_err(StorageError::from)?;

    tracing::info!(
        schema_version = migration.current_version,
        applied_migrations = migration.applied.len(),
        "host bootstrap complete"
    );

    Ok(Bootstrap {
        paths,
        config,
        host_instance_id: instance.id,
        schema_version: migration.current_version,
    })
}

fn os_user() -> Result<String, HostError> {
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
