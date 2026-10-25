//! Operating-system conventions for private application state.

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{PlatformError, io_error};

/// Per-user locations for durable state, configuration, runtime files, and logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformPaths {
    /// Durable application state, including the SQLite database.
    pub data_dir: PathBuf,
    /// User-editable configuration.
    pub config_dir: PathBuf,
    /// Short-lived sockets and lock files.
    pub runtime_dir: PathBuf,
}

impl PlatformPaths {
    /// Resolve the current user's standard locations.
    ///
    /// `MUXYARD_DATA_DIR`, `MUXYARD_CONFIG_DIR`, and `MUXYARD_RUNTIME_DIR`
    /// override individual locations and must be absolute.
    pub fn for_current_user() -> Result<Self, PlatformError> {
        let data_override = env_path("MUXYARD_DATA_DIR");
        let config_override = env_path("MUXYARD_CONFIG_DIR");
        let runtime_override = env_path("MUXYARD_RUNTIME_DIR");

        if let (Some(data_dir), Some(config_dir), Some(runtime_dir)) = (
            data_override.clone(),
            config_override.clone(),
            runtime_override.clone(),
        ) {
            let paths = Self {
                data_dir,
                config_dir,
                runtime_dir,
            };
            validate_absolute(&paths)?;
            return Ok(paths);
        }

        let mut paths = current_user_defaults()?;
        if let Some(path) = data_override {
            paths.data_dir = path;
        }
        if let Some(path) = config_override {
            paths.config_dir = path;
        }
        if let Some(path) = runtime_override {
            paths.runtime_dir = path;
        }
        validate_absolute(&paths)?;
        Ok(paths)
    }

    /// Build an isolated layout below a single directory, primarily for tests.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            data_dir: root.join("data"),
            config_dir: root.join("config"),
            runtime_dir: root.join("runtime"),
        }
    }

    /// SQLite database file.
    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("muxyard.sqlite3")
    }

    /// JSON configuration file.
    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }

    /// Append-only structured log file.
    pub fn log_path(&self) -> PathBuf {
        self.data_dir.join("logs").join("muxyard.log")
    }

    /// Single-instance host lock file.
    pub fn host_lock_path(&self) -> PathBuf {
        self.runtime_dir.join("host.lock")
    }

    /// Graceful stop request marker.
    pub fn stop_request_path(&self) -> PathBuf {
        self.runtime_dir.join("host.stop")
    }

    /// Ready marker written after startup completes.
    pub fn host_ready_path(&self) -> PathBuf {
        self.runtime_dir.join("host.ready")
    }

    /// Detached host startup diagnostics.
    pub fn startup_log_path(&self) -> PathBuf {
        self.runtime_dir.join("host-startup.log")
    }

    /// Create and secure every directory this layout owns.
    pub fn ensure(&self) -> Result<(), PlatformError> {
        ensure_private_dir(&self.data_dir)?;
        ensure_private_dir(&self.config_dir)?;
        ensure_private_dir(&self.runtime_dir)?;
        ensure_private_dir(&self.data_dir.join("logs"))?;
        Ok(())
    }
}

fn env_path(name: &'static str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn validate_absolute(paths: &PlatformPaths) -> Result<(), PlatformError> {
    for path in [&paths.data_dir, &paths.config_dir, &paths.runtime_dir] {
        if !path.is_absolute() {
            return Err(PlatformError::RelativePath(path.clone()));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn current_user_defaults() -> Result<PlatformPaths, PlatformError> {
    let home = PathBuf::from(env::var_os("HOME").ok_or(PlatformError::MissingEnvironment("HOME"))?);
    let base = home
        .join("Library")
        .join("Application Support")
        .join("Muxyard");
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(|value| PathBuf::from(value).join("Muxyard"))
        .unwrap_or_else(|| base.join("run"));
    Ok(PlatformPaths {
        data_dir: base.clone(),
        config_dir: base,
        runtime_dir,
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn current_user_defaults() -> Result<PlatformPaths, PlatformError> {
    let home = env::var_os("HOME").ok_or(PlatformError::MissingEnvironment("HOME"))?;
    let home = PathBuf::from(home);
    let data_root = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("share"));
    let config_root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let data_dir = data_root.join("muxyard");
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(|value| PathBuf::from(value).join("muxyard"))
        .unwrap_or_else(|| data_dir.join("run"));
    Ok(PlatformPaths {
        data_dir,
        config_dir: config_root.join("muxyard"),
        runtime_dir,
    })
}

#[cfg(windows)]
fn current_user_defaults() -> Result<PlatformPaths, PlatformError> {
    let local = env::var_os("LOCALAPPDATA")
        .or_else(|| {
            env::var_os("USERPROFILE").map(|home| {
                PathBuf::from(home)
                    .join("AppData")
                    .join("Local")
                    .into_os_string()
            })
        })
        .ok_or(PlatformError::MissingEnvironment("LOCALAPPDATA"))?;
    let roaming = env::var_os("APPDATA")
        .or_else(|| {
            env::var_os("USERPROFILE").map(|home| {
                PathBuf::from(home)
                    .join("AppData")
                    .join("Roaming")
                    .into_os_string()
            })
        })
        .ok_or(PlatformError::MissingEnvironment("APPDATA"))?;
    let data_dir = PathBuf::from(local).join("Muxyard");
    Ok(PlatformPaths {
        runtime_dir: data_dir.join("runtime"),
        data_dir,
        config_dir: PathBuf::from(roaming).join("Muxyard"),
    })
}

#[cfg(not(any(unix, windows)))]
fn current_user_defaults() -> Result<PlatformPaths, PlatformError> {
    Err(PlatformError::UnsupportedPlatform)
}

/// Create a directory if needed and restrict it to the current user.
pub fn ensure_private_dir(path: &Path) -> Result<(), PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(PlatformError::Symlink(path.to_path_buf()));
            }
            if !metadata.is_dir() {
                return Err(PlatformError::NotDirectory(path.to_path_buf()));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|error| io_error(path, error))?;
        }
        Err(error) => return Err(io_error(path, error)),
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| io_error(path, error))?;
        let mode = fs::metadata(path)
            .map_err(|error| io_error(path, error))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err(PlatformError::InsecurePermissions(path.to_path_buf()));
        }
    }
    Ok(())
}

/// Open an append-only file, creating it with private permissions when absent.
pub fn open_private_append(path: &Path) -> Result<File, PlatformError> {
    reject_symlink_and_non_file(path)?;
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|error| io_error(path, error))?;
    secure_file(path)?;
    Ok(file)
}

/// Replace a private file through a same-directory temporary file.
pub fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), PlatformError> {
    reject_symlink_and_non_file(path)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temp_path = parent.join(format!(
        ".muxyard-write-{}-{}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let mut file = open_private_new(&temp_path)?;
    if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(io_error(&temp_path, error));
    }
    drop(file);
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(io_error(path, error));
    }
    secure_file(path)
}

/// Create a new private file, refusing to replace an existing path.
pub fn open_private_new(path: &Path) -> Result<File, PlatformError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|error| io_error(path, error))?;
    secure_file(path)?;
    Ok(file)
}

fn reject_symlink_and_non_file(path: &Path) -> Result<(), PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(PlatformError::Symlink(path.to_path_buf()));
            }
            if !metadata.is_file() {
                return Err(PlatformError::NotFile(path.to_path_buf()));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(path, error)),
    }
    Ok(())
}

pub(crate) fn secure_file(path: &Path) -> Result<(), PlatformError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| io_error(path, error))?;
        let mode = fs::metadata(path)
            .map_err(|error| io_error(path, error))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err(PlatformError::InsecurePermissions(path.to_path_buf()));
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_layout_is_predictable() {
        let root = Path::new("/tmp/muxyard-test");
        let paths = PlatformPaths::from_root(root);
        assert_eq!(paths.data_dir, root.join("data"));
        assert_eq!(paths.config_dir, root.join("config"));
        assert_eq!(paths.runtime_dir, root.join("runtime"));
        assert_eq!(paths.database_path(), root.join("data/muxyard.sqlite3"));
        assert_eq!(paths.config_path(), root.join("config/config.json"));
        assert_eq!(paths.log_path(), root.join("data/logs/muxyard.log"));
    }

    #[test]
    fn ensure_creates_private_directories() {
        let root = tempfile::tempdir().unwrap();
        let paths = PlatformPaths::from_root(root.path());
        paths.ensure().unwrap();
        assert!(paths.data_dir.is_dir());
        assert!(paths.config_dir.is_dir());
        assert!(paths.runtime_dir.is_dir());
        assert!(paths.data_dir.join("logs").is_dir());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [
                &paths.data_dir,
                &paths.config_dir,
                &paths.runtime_dir,
                &paths.data_dir.join("logs"),
            ] {
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_rejected() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = root.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(matches!(
            ensure_private_dir(&link),
            Err(PlatformError::Symlink(_))
        ));
    }

    #[test]
    fn regular_file_is_not_a_directory() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        fs::write(&path, b"x").unwrap();
        assert!(matches!(
            ensure_private_dir(&path),
            Err(PlatformError::NotDirectory(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("log");
        drop(open_private_append(&path).unwrap());
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let new_path = root.path().join("new");
        drop(open_private_new(&new_path).unwrap());
        assert_eq!(
            fs::metadata(&new_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn append_file_is_not_truncated() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("log");
        fs::write(&path, b"first").unwrap();
        use std::io::Write;
        open_private_append(&path)
            .unwrap()
            .write_all(b"-second")
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first-second");
    }

    #[cfg(unix)]
    #[test]
    fn private_control_files_replace_atomically() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("control");
        write_private_file(&path, b"first").unwrap();
        write_private_file(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
    }

    #[test]
    fn new_file_does_not_replace_existing_content() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        fs::write(&path, b"keep").unwrap();
        let error = open_private_new(&path).unwrap_err();
        assert!(matches!(error, PlatformError::Io { .. }));
        assert_eq!(fs::read(&path).unwrap(), b"keep");
    }

    #[test]
    fn absent_environment_override_is_ignored() {
        assert_eq!(env_path("MUXYARD_TEST_ABSENT_VALUE"), None);
    }
}
