//! Safe Git repository discovery using argv-only process calls.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Result of resolving a directory as a Git repository or plain project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryDiscovery {
    /// Canonical project root.
    pub root: PathBuf,
    /// Whether the root was resolved by Git.
    pub is_git: bool,
}

/// Repository discovery failure.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// The selected path does not exist.
    #[error("project path does not exist: {0}")]
    MissingPath(PathBuf),
    /// The selected path is not a directory.
    #[error("project path is not a directory: {0}")]
    NotDirectory(PathBuf),
    /// The selected path is a symbolic link.
    #[error("project path is a symbolic link: {0}")]
    Symlink(PathBuf),
    /// Filesystem access failed.
    #[error("project path could not be inspected: {0}")]
    Io(#[from] std::io::Error),
}

/// Resolve `path` to a canonical project root.
///
/// Git is invoked with an argument array and never through a shell. A directory
/// that is not a Git repository remains a valid plain project. Nested
/// repositories resolve to the nearest Git top-level, and no repository script
/// or hook is executed.
pub fn discover_repository(path: impl AsRef<Path>) -> Result<RepositoryDiscovery, GitError> {
    let selected = path.as_ref();
    let metadata = fs::symlink_metadata(selected).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            GitError::MissingPath(selected.to_path_buf())
        } else {
            GitError::Io(error)
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(GitError::Symlink(selected.to_path_buf()));
    }
    if !metadata.is_dir() {
        return Err(GitError::NotDirectory(selected.to_path_buf()));
    }
    let canonical = fs::canonicalize(selected)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&canonical)
        .args(["rev-parse", "--show-toplevel"])
        .output();
    let Ok(output) = output else {
        return Ok(RepositoryDiscovery {
            root: canonical,
            is_git: false,
        });
    };
    if !output.status.success() {
        return Ok(RepositoryDiscovery {
            root: canonical,
            is_git: false,
        });
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let root = text.trim();
    if root.is_empty() {
        return Ok(RepositoryDiscovery {
            root: canonical,
            is_git: false,
        });
    }
    let root = fs::canonicalize(root)?;
    if !root.is_dir() {
        return Err(GitError::NotDirectory(root));
    }
    Ok(RepositoryDiscovery { root, is_git: true })
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    fn git_repo(path: &Path) {
        let status = Command::new("git")
            .args(["init", "-q"])
            .arg(path)
            .status()
            .expect("git should run");
        assert!(status.success());
    }

    #[test]
    fn plain_directory_is_a_plain_project() {
        let temp = tempfile::tempdir().unwrap();
        let result = discover_repository(temp.path()).unwrap();
        assert!(!result.is_git);
        assert_eq!(result.root, temp.path().canonicalize().unwrap());
    }

    #[test]
    fn git_directory_resolves_to_its_top_level() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git_repo(&repo);
        let nested = repo.join("nested");
        fs::create_dir(&nested).unwrap();

        let result = discover_repository(&nested).unwrap();
        assert!(result.is_git);
        assert_eq!(result.root, repo.canonicalize().unwrap());
    }

    #[test]
    fn missing_and_file_paths_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        assert!(matches!(
            discover_repository(temp.path().join("missing")),
            Err(GitError::MissingPath(_))
        ));
        let file = temp.path().join("file");
        fs::write(&file, b"x").unwrap();
        assert!(matches!(
            discover_repository(file),
            Err(GitError::NotDirectory(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_project_root_is_rejected() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = temp.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(matches!(
            discover_repository(link),
            Err(GitError::Symlink(_))
        ));
    }
}
