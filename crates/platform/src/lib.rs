//! Per-user paths, validated configuration, and secret-safe logging.

mod config;
mod logging;
mod paths;
mod process;
mod shell;

use std::{io, path::PathBuf};

pub use config::{
    CONFIG_VERSION, ConfigError, ConfigStore, HostConfig, LogFormat, LogLevel, LoggingConfig,
    ResourceLimits,
};
pub use logging::{LoggingError, Sensitive, build_subscriber, init_file_logging};
pub use paths::{
    PlatformPaths, ensure_private_dir, open_private_append, open_private_new, write_private_file,
};
pub use process::{
    ProcessIdentity, configure_detached, current_process_identity, process_identity,
};
pub use shell::{resolve_shell, validate_working_directory};

/// A failure while resolving or securing per-user filesystem locations.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// A required operating-system environment variable is absent.
    #[error("required environment variable {0} is not set")]
    MissingEnvironment(&'static str),
    /// The current operating system has no defined path convention here.
    #[error("per-user paths are not supported on this operating system")]
    UnsupportedPlatform,
    /// Overrides must be absolute so a working directory cannot redirect state.
    #[error("path {0} must be absolute")]
    RelativePath(PathBuf),
    /// A final path component is a symbolic link.
    #[error("path {0} is a symbolic link")]
    Symlink(PathBuf),
    /// A directory was expected.
    #[error("path {0} is not a directory")]
    NotDirectory(PathBuf),
    /// A regular file was expected.
    #[error("path {0} is not a regular file")]
    NotFile(PathBuf),
    /// Group or other users can access a private path.
    #[error("path {0} has insecure permissions")]
    InsecurePermissions(PathBuf),
    /// A filesystem operation failed for a named path.
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        /// Path involved in the failed operation.
        path: PathBuf,
        /// Underlying operating-system error.
        #[source]
        source: io::Error,
    },
    /// Process identity or detachment setup failed.
    #[error("process operation failed: {0}")]
    Process(String),
    /// A shell executable is missing, relative, or not executable.
    #[error("shell executable is unavailable: {0}")]
    ShellUnavailable(PathBuf),
    /// A working directory is missing or is not a directory.
    #[error("working directory is unavailable: {0}")]
    InvalidWorkingDirectory(PathBuf),
}

pub(crate) fn io_error(path: &std::path::Path, source: io::Error) -> PlatformError {
    PlatformError::Io {
        path: path.to_path_buf(),
        source,
    }
}
