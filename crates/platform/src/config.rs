//! Validated configuration with conservative defaults.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tracing::level_filters::LevelFilter;

use crate::{
    PlatformError,
    paths::{open_private_new, secure_file},
};

/// Current configuration schema version.
pub const CONFIG_VERSION: u32 = 1;

/// Supported log severity thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Errors only.
    Error,
    /// Errors and warnings.
    Warn,
    /// Normal operational events.
    Info,
    /// Diagnostic events.
    Debug,
    /// Detailed tracing.
    Trace,
}

impl LogLevel {
    pub(crate) fn as_filter(self) -> LevelFilter {
        match self {
            Self::Error => LevelFilter::ERROR,
            Self::Warn => LevelFilter::WARN,
            Self::Info => LevelFilter::INFO,
            Self::Debug => LevelFilter::DEBUG,
            Self::Trace => LevelFilter::TRACE,
        }
    }
}

/// Log encoding written to the host log file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// One structured JSON object per event.
    Json,
    /// Human-readable single-line events.
    Text,
}

/// Logging defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    /// Minimum event severity.
    pub level: LogLevel,
    /// File encoding.
    pub format: LogFormat,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            format: LogFormat::Json,
        }
    }
}

/// Bounded resource defaults shared by future host services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    /// Retained raw output per terminal session.
    pub pty_output_ring_bytes: u64,
    /// Bounded renderer scrollback lines per terminal pane.
    #[serde(default = "default_terminal_scrollback_lines")]
    pub terminal_scrollback_lines: u32,
    /// Maximum queued output for one client.
    pub client_output_queue_bytes: u64,
    /// Maximum room message body size.
    pub room_message_bytes: u64,
    /// Maximum remote WebSocket frame size.
    pub remote_ws_frame_bytes: u64,
    /// Maximum room events returned per page.
    pub room_events_per_page: u32,
    /// Concurrent Git status workers per repository.
    pub max_git_status_workers: u32,
    /// Concurrent agent launches requested by one client.
    pub max_active_agent_launches: u32,
}

const fn default_terminal_scrollback_lines() -> u32 {
    10_000
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            pty_output_ring_bytes: 4 * 1024 * 1024,
            terminal_scrollback_lines: default_terminal_scrollback_lines(),
            client_output_queue_bytes: 4 * 1024 * 1024,
            room_message_bytes: 64 * 1024,
            remote_ws_frame_bytes: 256 * 1024,
            room_events_per_page: 100,
            max_git_status_workers: 2,
            max_active_agent_launches: 5,
        }
    }
}

impl ResourceLimits {
    fn validate(&self) -> Result<(), &'static str> {
        if !(64 * 1024..=64 * 1024 * 1024).contains(&self.pty_output_ring_bytes) {
            return Err("pty_output_ring_bytes must be between 64 KiB and 64 MiB");
        }
        if !(64 * 1024..=64 * 1024 * 1024).contains(&self.client_output_queue_bytes) {
            return Err("client_output_queue_bytes must be between 64 KiB and 64 MiB");
        }
        if !(1_000..=100_000).contains(&self.terminal_scrollback_lines) {
            return Err("terminal_scrollback_lines must be between 1,000 and 100,000");
        }
        if !(1024..=1024 * 1024).contains(&self.room_message_bytes) {
            return Err("room_message_bytes must be between 1 KiB and 1 MiB");
        }
        if !(16 * 1024..=1024 * 1024).contains(&self.remote_ws_frame_bytes) {
            return Err("remote_ws_frame_bytes must be between 16 KiB and 1 MiB");
        }
        if !(1..=500).contains(&self.room_events_per_page) {
            return Err("room_events_per_page must be between 1 and 500");
        }
        if !(1..=8).contains(&self.max_git_status_workers) {
            return Err("max_git_status_workers must be between 1 and 8");
        }
        if !(1..=32).contains(&self.max_active_agent_launches) {
            return Err("max_active_agent_launches must be between 1 and 32");
        }
        Ok(())
    }
}

/// Host configuration loaded from the user's config file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    /// Schema version understood by this host.
    pub config_version: u32,
    /// Logging behavior.
    pub logging: LoggingConfig,
    /// Resource bounds.
    pub limits: ResourceLimits,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            logging: LoggingConfig::default(),
            limits: ResourceLimits::default(),
        }
    }
}

impl HostConfig {
    fn validate(&self) -> Result<(), &'static str> {
        if self.config_version != CONFIG_VERSION {
            return Err("unsupported config_version");
        }
        self.limits.validate()
    }
}

/// Configuration load and creation failures. Existing files are never rewritten.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A filesystem operation failed.
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        /// Path involved in the failure.
        path: PathBuf,
        /// Underlying operating-system error.
        #[source]
        source: io::Error,
    },
    /// The file is not valid JSON or does not match the expected shape.
    #[error("config file {path} is malformed at line {line}, column {column}")]
    Malformed {
        /// Config path.
        path: PathBuf,
        /// One-based line reported by the parser.
        line: usize,
        /// One-based column reported by the parser.
        column: usize,
    },
    /// The file is a symbolic link.
    #[error("config file {path} is a symbolic link")]
    Symlink {
        /// Config path.
        path: PathBuf,
    },
    /// The path is not a regular file.
    #[error("config file {path} is not a regular file")]
    NotFile {
        /// Config path.
        path: PathBuf,
    },
    /// The version is not understood by this host.
    #[error("config file {path} has an unsupported version")]
    UnsupportedVersion {
        /// Config path.
        path: PathBuf,
    },
    /// A field value is outside its accepted range.
    #[error("config file {path} contains an invalid value: {reason}")]
    Invalid {
        /// Config path.
        path: PathBuf,
        /// Stable validation reason.
        reason: &'static str,
    },
    /// Serialization of built-in defaults failed.
    #[error("could not serialize default configuration")]
    Serialize(#[from] serde_json::Error),
    /// The parent directory does not exist.
    #[error("config directory {0} does not exist")]
    MissingParent(PathBuf),
}

impl ConfigError {
    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

impl From<PlatformError> for ConfigError {
    fn from(error: PlatformError) -> Self {
        match error {
            PlatformError::Io { path, source } => Self::Io { path, source },
            PlatformError::Symlink(path) => Self::Symlink { path },
            PlatformError::NotFile(path) => Self::NotFile { path },
            PlatformError::NotDirectory(path) => Self::NotFile { path },
            PlatformError::InsecurePermissions(path) => Self::Io {
                path,
                source: io::Error::new(io::ErrorKind::PermissionDenied, "insecure permissions"),
            },
            PlatformError::RelativePath(path) => Self::Io {
                path,
                source: io::Error::new(io::ErrorKind::InvalidInput, "path must be absolute"),
            },
            PlatformError::MissingEnvironment(name) => Self::Io {
                path: PathBuf::from(name),
                source: io::Error::new(io::ErrorKind::NotFound, "missing environment variable"),
            },
            PlatformError::UnsupportedPlatform => Self::Io {
                path: PathBuf::new(),
                source: io::Error::new(io::ErrorKind::Unsupported, "unsupported platform"),
            },
            PlatformError::Process(message) => Self::Io {
                path: PathBuf::new(),
                source: io::Error::other(message),
            },
            PlatformError::ShellUnavailable(path) => Self::Io {
                path,
                source: io::Error::new(io::ErrorKind::NotFound, "shell is unavailable"),
            },
            PlatformError::InvalidWorkingDirectory(path) => Self::Io {
                path,
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "working directory is unavailable",
                ),
            },
        }
    }
}

/// Reads a configuration file and creates defaults only when no file exists.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    /// Use `path` as the JSON configuration file.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Configured path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the file, or return built-in defaults when it does not exist.
    pub fn load(&self) -> Result<HostConfig, ConfigError> {
        Ok(self.read_existing()?.unwrap_or_default())
    }

    /// Load the file, creating a private default file only when absent.
    pub fn load_or_create_default(&self) -> Result<HostConfig, ConfigError> {
        if let Some(config) = self.read_existing()? {
            return Ok(config);
        }
        self.write_default()?;
        self.read_existing()?.ok_or_else(|| ConfigError::Io {
            path: self.path.clone(),
            source: io::Error::new(io::ErrorKind::NotFound, "configuration was not created"),
        })
    }

    fn read_existing(&self) -> Result<Option<HostConfig>, ConfigError> {
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(ConfigError::Symlink {
                        path: self.path.clone(),
                    });
                }
                if !metadata.is_file() {
                    return Err(ConfigError::NotFile {
                        path: self.path.clone(),
                    });
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(ConfigError::io(&self.path, error)),
        }
        secure_file(&self.path)?;

        let bytes = fs::read(&self.path).map_err(|error| ConfigError::io(&self.path, error))?;
        let config: HostConfig =
            serde_json::from_slice(&bytes).map_err(|error| ConfigError::Malformed {
                path: self.path.clone(),
                line: error.line(),
                column: error.column(),
            })?;
        config.validate().map_err(|reason| {
            if reason == "unsupported config_version" {
                ConfigError::UnsupportedVersion {
                    path: self.path.clone(),
                }
            } else {
                ConfigError::Invalid {
                    path: self.path.clone(),
                    reason,
                }
            }
        })?;
        Ok(Some(config))
    }

    fn write_default(&self) -> Result<(), ConfigError> {
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| ConfigError::MissingParent(self.path.clone()))?;
        if !parent.is_dir() {
            return Err(ConfigError::MissingParent(parent.to_path_buf()));
        }

        let config = HostConfig::default();
        let mut bytes = serde_json::to_vec_pretty(&config)?;
        bytes.push(b'\n');

        let temp_path = parent.join(format!(
            ".config-{}-{}.tmp",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        let mut file = open_private_new(&temp_path)?;
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&temp_path);
            return Err(ConfigError::io(&temp_path, error));
        }
        drop(file);

        let result = match fs::hard_link(&temp_path, &self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(ConfigError::io(&self.path, error)),
        };
        let _ = fs::remove_file(&temp_path);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, ConfigStore) {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.json");
        (root, ConfigStore::new(path))
    }

    #[test]
    fn defaults_are_valid_and_conservative() {
        let config = HostConfig::default();
        assert_eq!(config.config_version, CONFIG_VERSION);
        assert_eq!(config.logging.level, LogLevel::Info);
        assert_eq!(config.logging.format, LogFormat::Json);
        assert_eq!(config.limits.room_message_bytes, 64 * 1024);
        assert_eq!(config.limits.terminal_scrollback_lines, 10_000);
        assert_eq!(config.limits.max_active_agent_launches, 5);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn default_file_is_created_once_and_not_rewritten() {
        let (root, store) = store();
        let first = store.load_or_create_default().unwrap();
        assert_eq!(first, HostConfig::default());
        let first_bytes = fs::read(store.path()).unwrap();
        let second = store.load_or_create_default().unwrap();
        assert_eq!(first, second);
        assert_eq!(fs::read(store.path()).unwrap(), first_bytes);
        assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
    }

    #[test]
    fn missing_file_loads_defaults_without_writing() {
        let (root, store) = store();
        assert_eq!(store.load().unwrap(), HostConfig::default());
        assert!(!store.path().exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn malformed_file_is_preserved_and_not_echoed() {
        let (root, store) = store();
        let content = br#"{ "secret": "never-print-this", broken"#;
        fs::write(store.path(), content).unwrap();
        let error = store.load_or_create_default().unwrap_err();
        assert!(matches!(error, ConfigError::Malformed { .. }));
        assert!(!error.to_string().contains("never-print-this"));
        assert_eq!(fs::read(store.path()).unwrap(), content);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn unknown_fields_are_rejected_without_overwrite() {
        let (_root, store) = store();
        let content = br#"{"config_version":1,"logging":{"level":"info","format":"json"},"limits":{"pty_output_ring_bytes":4194304,"client_output_queue_bytes":4194304,"room_message_bytes":65536,"remote_ws_frame_bytes":262144,"room_events_per_page":100,"max_git_status_workers":2,"max_active_agent_launches":5},"extra":true}"#;
        fs::write(store.path(), content).unwrap();
        let error = store.load_or_create_default().unwrap_err();
        assert!(matches!(error, ConfigError::Malformed { .. }));
        assert_eq!(fs::read(store.path()).unwrap(), content);
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let (_root, store) = store();
        let mut value = serde_json::to_value(HostConfig::default()).unwrap();
        value["config_version"] = serde_json::json!(2);
        let content = serde_json::to_vec(&value).unwrap();
        fs::write(store.path(), &content).unwrap();
        let error = store.load_or_create_default().unwrap_err();
        assert!(matches!(error, ConfigError::UnsupportedVersion { .. }));
        assert_eq!(fs::read(store.path()).unwrap(), content);
    }

    #[test]
    fn invalid_limits_are_rejected() {
        let (_root, store) = store();
        let mut value = serde_json::to_value(HostConfig::default()).unwrap();
        value["limits"]["room_message_bytes"] = serde_json::json!(0);
        let content = serde_json::to_vec(&value).unwrap();
        fs::write(store.path(), &content).unwrap();
        let error = store.load_or_create_default().unwrap_err();
        assert!(matches!(error, ConfigError::Invalid { .. }));
        assert_eq!(fs::read(store.path()).unwrap(), content);
    }

    #[test]
    fn invalid_scrollback_bound_is_rejected() {
        let (_root, store) = store();
        let mut value = serde_json::to_value(HostConfig::default()).unwrap();
        value["limits"]["terminal_scrollback_lines"] = serde_json::json!(0);
        let content = serde_json::to_vec(&value).unwrap();
        fs::write(store.path(), &content).unwrap();
        let error = store.load_or_create_default().unwrap_err();
        assert!(matches!(error, ConfigError::Invalid { .. }));
        assert_eq!(fs::read(store.path()).unwrap(), content);
    }

    #[cfg(unix)]
    #[test]
    fn config_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let (_root, store) = store();
        store.load_or_create_default().unwrap();
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn existing_loose_config_is_tightened_without_rewriting() {
        use std::os::unix::fs::PermissionsExt;
        let (_root, store) = store();
        let content = serde_json::to_vec(&HostConfig::default()).unwrap();
        fs::write(store.path(), &content).unwrap();
        fs::set_permissions(store.path(), fs::Permissions::from_mode(0o644)).unwrap();
        store.load().unwrap();
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read(store.path()).unwrap(), content);
    }

    #[test]
    fn symlinked_config_is_rejected() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let root = tempfile::tempdir().unwrap();
            let target = root.path().join("target.json");
            fs::write(&target, b"{}").unwrap();
            let link = root.path().join("config.json");
            symlink(&target, &link).unwrap();
            let store = ConfigStore::new(link);
            assert!(matches!(
                store.load_or_create_default(),
                Err(ConfigError::Symlink { .. })
            ));
        }
    }
}
