//! Same-user single-instance lock and stop marker.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use muxyard_platform::{
    PlatformError, ProcessIdentity, open_private_new, process_identity, write_private_file,
};
use serde::{Deserialize, Serialize};

/// Lock schema version understood by this host.
pub const LOCK_SCHEMA_VERSION: u32 = 1;

/// Identity and version recorded while the host owns the lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostLockInfo {
    /// Lock record schema version.
    pub schema_version: u32,
    /// Unique process instance.
    pub instance_id: String,
    /// Operating-system process ID.
    pub pid: u32,
    /// OS process start identity for PID-reuse detection.
    pub process_start: String,
    /// Start time in Unix epoch milliseconds.
    pub started_ms: i64,
    /// Host package version.
    pub host_version: String,
    /// Wire protocol version.
    pub protocol_version: u8,
}

impl HostLockInfo {
    /// Build the current process's lock record.
    pub fn new(instance_id: String, identity: &ProcessIdentity, started_ms: i64) -> Self {
        Self {
            schema_version: LOCK_SCHEMA_VERSION,
            instance_id,
            pid: identity.pid,
            process_start: identity.start_identity.clone(),
            started_ms,
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: muxyard_contracts::PROTOCOL_VERSION,
        }
    }
}

/// Result of inspecting the lock path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockState {
    /// No host lock exists.
    Missing,
    /// A live host owns the lock.
    Running(HostLockInfo),
    /// The recorded PID is dead or has a different start identity.
    Stale(HostLockInfo),
    /// The lock exists but cannot be trusted.
    Corrupt(PathBuf),
}

/// Single-instance lock failure.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another live host already owns the lock.
    #[error("host is already running")]
    AlreadyRunning(HostLockInfo),
    /// The lock is malformed or belongs to an unsupported schema.
    #[error("host lock {0} is corrupt; verify no host is running before removing it")]
    Corrupt(PathBuf),
    /// Repeated acquisition races did not settle.
    #[error("host lock {0} could not be acquired")]
    Contended(PathBuf),
    /// Filesystem or process identity failure.
    #[error(transparent)]
    Platform(#[from] PlatformError),
    /// Lock file I/O failed.
    #[error("host lock I/O failed for {path}: {source}")]
    Io {
        /// Lock path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// JSON encoding failed.
    #[error("host lock serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// An acquired host lock. Dropping it removes the file when still owned.
#[derive(Debug)]
pub struct HostLock {
    path: PathBuf,
    info: HostLockInfo,
}

impl HostLock {
    /// Inspect the lock without modifying it.
    pub fn inspect(path: &Path) -> Result<LockState, LockError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(LockState::Missing);
            }
            Err(error) => {
                return Err(LockError::Io {
                    path: path.to_path_buf(),
                    source: error,
                });
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Ok(LockState::Corrupt(path.to_path_buf()));
        }
        let bytes = fs::read(path).map_err(|error| LockError::Io {
            path: path.to_path_buf(),
            source: error,
        })?;
        let info: HostLockInfo = match serde_json::from_slice(&bytes) {
            Ok(info) => info,
            Err(_) => return Ok(LockState::Corrupt(path.to_path_buf())),
        };
        if info.schema_version != LOCK_SCHEMA_VERSION
            || info.pid == 0
            || info.instance_id.is_empty()
            || info.process_start.is_empty()
        {
            return Ok(LockState::Corrupt(path.to_path_buf()));
        }
        match process_identity(info.pid)? {
            Some(identity) if identity.start_identity == info.process_start => {
                Ok(LockState::Running(info))
            }
            Some(_) if cfg!(windows) && !info.process_start.starts_with("windows:") => {
                // A live older host has a different timestamp format; it is not verified stale.
                Ok(LockState::Corrupt(path.to_path_buf()))
            }
            _ => Ok(LockState::Stale(info)),
        }
    }

    /// Acquire the lock, removing only verified stale records.
    pub fn acquire(path: &Path, info: HostLockInfo) -> Result<Self, LockError> {
        for _ in 0..3 {
            match Self::inspect(path)? {
                LockState::Running(info) => return Err(LockError::AlreadyRunning(info)),
                LockState::Corrupt(_) => return Err(LockError::Corrupt(path.to_path_buf())),
                LockState::Stale(_) => match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(LockError::Io {
                            path: path.to_path_buf(),
                            source: error,
                        });
                    }
                },
                LockState::Missing => {}
            }

            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let temp_path = parent.join(format!(
                ".host-lock-{}-{}.tmp",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default()
            ));
            let mut file = match open_private_new(&temp_path) {
                Ok(file) => file,
                Err(error) => return Err(LockError::Platform(error)),
            };
            let bytes = serde_json::to_vec_pretty(&info)?;
            if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
                drop(file);
                let _ = fs::remove_file(&temp_path);
                return Err(LockError::Io {
                    path: temp_path,
                    source: error,
                });
            }
            drop(file);
            match fs::hard_link(&temp_path, path) {
                Ok(()) => {
                    let _ = fs::remove_file(&temp_path);
                    return Ok(Self {
                        path: path.to_path_buf(),
                        info,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let _ = fs::remove_file(&temp_path);
                    continue;
                }
                Err(error) => {
                    let _ = fs::remove_file(&temp_path);
                    return Err(LockError::Io {
                        path: path.to_path_buf(),
                        source: error,
                    });
                }
            }
        }
        Err(LockError::Contended(path.to_path_buf()))
    }

    /// Lock record currently owned by this process.
    pub fn info(&self) -> &HostLockInfo {
        &self.info
    }
}

impl Drop for HostLock {
    fn drop(&mut self) {
        let Ok(bytes) = fs::read(&self.path) else {
            return;
        };
        let Ok(info) = serde_json::from_slice::<HostLockInfo>(&bytes) else {
            return;
        };
        if info.instance_id == self.info.instance_id {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Publish readiness for a live host instance.
pub fn write_ready_marker(path: &Path, info: &HostLockInfo) -> Result<(), LockError> {
    let marker = serde_json::json!({
        "instance_id": info.instance_id,
        "pid": info.pid,
    });
    let bytes = serde_json::to_vec_pretty(&marker)?;
    write_private_file(path, &bytes)?;
    Ok(())
}

/// Whether the ready marker belongs to this instance.
pub fn ready_marker_matches(path: &Path, instance_id: &str) -> Result<bool, LockError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(LockError::Io {
                path: path.to_path_buf(),
                source: error,
            });
        }
    };
    let marker: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| LockError::Corrupt(path.to_path_buf()))?;
    let found = marker
        .get("instance_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| LockError::Corrupt(path.to_path_buf()))?;
    Ok(found == instance_id)
}

/// Remove a ready marker, ignoring an absent file.
pub fn clear_ready_marker(path: &Path) -> Result<(), LockError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(LockError::Io {
            path: path.to_path_buf(),
            source: error,
        }),
    }
}

/// Ask the host instance to stop gracefully.
pub fn write_stop_request(path: &Path, instance_id: &str) -> Result<(), LockError> {
    let request = serde_json::json!({ "instance_id": instance_id });
    let bytes = serde_json::to_vec_pretty(&request)?;
    write_private_file(path, &bytes)?;
    Ok(())
}

/// Whether the stop marker targets this host instance.
pub fn stop_requested(path: &Path, instance_id: &str) -> Result<bool, LockError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(LockError::Io {
                path: path.to_path_buf(),
                source: error,
            });
        }
    };
    let request: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| LockError::Corrupt(path.to_path_buf()))?;
    let requested = request
        .get("instance_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| LockError::Corrupt(path.to_path_buf()))?;
    Ok(requested == instance_id)
}

/// Remove a stop marker, ignoring an absent file.
pub fn clear_stop_request(path: &Path) -> Result<(), LockError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(LockError::Io {
            path: path.to_path_buf(),
            source: error,
        }),
    }
}

#[cfg(test)]
mod tests {
    use muxyard_platform::current_process_identity;

    use super::*;

    fn info(instance_id: &str) -> HostLockInfo {
        let identity = current_process_identity().unwrap();
        HostLockInfo {
            schema_version: LOCK_SCHEMA_VERSION,
            instance_id: instance_id.to_string(),
            pid: identity.pid,
            process_start: identity.start_identity,
            started_ms: 1,
            host_version: "0.1.0".to_string(),
            protocol_version: 1,
        }
    }

    #[test]
    fn lock_is_exclusive_and_removed_on_drop() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("host.lock");
        let lock = HostLock::acquire(&path, info("one")).unwrap();
        assert!(matches!(
            HostLock::inspect(&path).unwrap(),
            LockState::Running(ref found) if found.instance_id == "one"
        ));
        assert!(matches!(
            HostLock::acquire(&path, info("two")),
            Err(LockError::AlreadyRunning(_))
        ));
        drop(lock);
        assert_eq!(HostLock::inspect(&path).unwrap(), LockState::Missing);
    }

    #[test]
    fn stale_lock_is_replaced() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("host.lock");
        let mut stale = info("stale");
        stale.pid = u32::MAX;
        stale.process_start = "dead-start".to_string();
        fs::write(&path, serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!(matches!(
            HostLock::inspect(&path).unwrap(),
            LockState::Stale(_)
        ));
        let lock = HostLock::acquire(&path, info("fresh")).unwrap();
        assert!(matches!(
            HostLock::inspect(&path).unwrap(),
            LockState::Running(ref found) if found.instance_id == "fresh"
        ));
        drop(lock);
    }

    #[cfg(windows)]
    #[test]
    fn live_host_with_legacy_identity_is_not_replaced() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("host.lock");
        let mut legacy = info("existing");
        legacy.process_start = "2026-10-27T10:00:00.1234567+01:00".to_string();
        let original = serde_json::to_vec(&legacy).unwrap();
        fs::write(&path, &original).unwrap();
        assert!(matches!(
            HostLock::inspect(&path).unwrap(),
            LockState::Corrupt(_)
        ));
        assert!(matches!(
            HostLock::acquire(&path, info("replacement")),
            Err(LockError::Corrupt(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn corrupt_lock_is_not_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("host.lock");
        fs::write(&path, b"{not json").unwrap();
        assert!(matches!(
            HostLock::inspect(&path).unwrap(),
            LockState::Corrupt(_)
        ));
        assert!(matches!(
            HostLock::acquire(&path, info("fresh")),
            Err(LockError::Corrupt(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), b"{not json");
    }

    #[test]
    fn stop_marker_targets_one_instance() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("host.stop");
        assert!(!stop_requested(&path, "one").unwrap());
        write_stop_request(&path, "one").unwrap();
        assert!(stop_requested(&path, "one").unwrap());
        assert!(!stop_requested(&path, "two").unwrap());
        clear_stop_request(&path).unwrap();
        assert!(!stop_requested(&path, "one").unwrap());
    }

    #[test]
    fn ready_marker_targets_one_instance() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("host.ready");
        let record = info("one");
        assert!(!ready_marker_matches(&path, "one").unwrap());
        write_ready_marker(&path, &record).unwrap();
        assert!(ready_marker_matches(&path, "one").unwrap());
        assert!(!ready_marker_matches(&path, "two").unwrap());
        clear_ready_marker(&path).unwrap();
        assert!(!ready_marker_matches(&path, "one").unwrap());
    }
}
