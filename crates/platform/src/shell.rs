//! Validated shell and working-directory selection for PTY launches.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::PlatformError;

/// Resolve the shell executable without invoking a shell string.
///
/// An explicit path must be absolute, a regular file, and executable on Unix.
/// Without an explicit path, `SHELL`/`COMSPEC` is used when valid, then the
/// platform default is selected.
pub fn resolve_shell(requested: Option<&Path>) -> Result<PathBuf, PlatformError> {
    if let Some(path) = requested {
        return validate_shell(path);
    }

    if let Some(path) = env::var_os("SHELL").map(PathBuf::from)
        && let Ok(path) = validate_shell(&path)
    {
        return Ok(path);
    }

    #[cfg(windows)]
    {
        if let Some(path) = env::var_os("COMSPEC").map(PathBuf::from)
            && let Ok(path) = validate_shell(&path)
        {
            return Ok(path);
        }
        if let Some(root) = env::var_os("SystemRoot") {
            let path = PathBuf::from(root).join("System32").join("cmd.exe");
            if let Ok(path) = validate_shell(&path) {
                return Ok(path);
            }
        }
        validate_shell(Path::new("cmd.exe"))
    }

    #[cfg(unix)]
    {
        let candidates: &[&str] = if cfg!(target_os = "macos") {
            &["/bin/zsh", "/bin/bash", "/bin/sh"]
        } else {
            &["/bin/bash", "/bin/sh"]
        };
        for candidate in candidates {
            if let Ok(path) = validate_shell(Path::new(candidate)) {
                return Ok(path);
            }
        }
        Err(PlatformError::ShellUnavailable(PathBuf::from("/bin/sh")))
    }

    #[cfg(not(any(unix, windows)))]
    {
        Err(PlatformError::UnsupportedPlatform)
    }
}

/// Resolve and canonicalize a working directory, rejecting files and missing paths.
pub fn validate_working_directory(path: &Path) -> Result<PathBuf, PlatformError> {
    let canonical = fs::canonicalize(path)
        .map_err(|_| PlatformError::InvalidWorkingDirectory(path.to_path_buf()))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|_| PlatformError::InvalidWorkingDirectory(path.to_path_buf()))?;
    if !metadata.is_dir() {
        return Err(PlatformError::InvalidWorkingDirectory(path.to_path_buf()));
    }
    Ok(canonical)
}

fn validate_shell(path: &Path) -> Result<PathBuf, PlatformError> {
    if !path.is_absolute() {
        return Err(PlatformError::ShellUnavailable(path.to_path_buf()));
    }
    let metadata =
        fs::metadata(path).map_err(|_| PlatformError::ShellUnavailable(path.to_path_buf()))?;
    if !metadata.is_file() {
        return Err(PlatformError::ShellUnavailable(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(PlatformError::ShellUnavailable(path.to_path_buf()));
        }
    }
    fs::canonicalize(path).map_err(|_| PlatformError::ShellUnavailable(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_resolves_to_an_executable() {
        let shell = resolve_shell(None).unwrap();
        assert!(shell.is_absolute());
        assert!(shell.is_file());
    }

    #[test]
    fn missing_and_relative_working_directories_are_rejected() {
        let missing = Path::new("/muxyard-test/missing-directory");
        assert!(matches!(
            validate_working_directory(missing),
            Err(PlatformError::InvalidWorkingDirectory(_))
        ));
        assert!(validate_working_directory(Path::new(".")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn relative_or_non_executable_shells_are_rejected() {
        assert!(matches!(
            resolve_shell(Some(Path::new("sh"))),
            Err(PlatformError::ShellUnavailable(_))
        ));
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("not executable");
        fs::write(&path, b"#!/bin/sh\n").unwrap();
        assert!(matches!(
            resolve_shell(Some(&path)),
            Err(PlatformError::ShellUnavailable(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn shell_paths_with_spaces_are_validated() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("shell with spaces");
        fs::copy("/bin/sh", &path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            resolve_shell(Some(&path)).unwrap(),
            path.canonicalize().unwrap()
        );
    }
}
