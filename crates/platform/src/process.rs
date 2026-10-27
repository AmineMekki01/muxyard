//! OS process identity and detached child setup.

use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::PlatformError;

/// PID plus the operating system's process start identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    /// Operating-system process ID.
    pub pid: u32,
    /// Stable start-time text for this PID, used to detect PID reuse.
    pub start_identity: String,
}

/// Return the current process identity.
pub fn current_process_identity() -> Result<ProcessIdentity, PlatformError> {
    process_identity(std::process::id())?.ok_or_else(|| {
        PlatformError::Process("current process start identity is unavailable".to_string())
    })
}

/// Return an identity when `pid` exists, or `None` when it does not.
pub fn process_identity(pid: u32) -> Result<Option<ProcessIdentity>, PlatformError> {
    #[cfg(unix)]
    {
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "lstart=", "-o", "stat="])
            .output()
            .map_err(|error| PlatformError::Process(format!("ps could not run: {error}")))?;
        if !output.status.success() {
            return Ok(None);
        }
        let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let Some((start_identity, state)) = line.rsplit_once(char::is_whitespace) else {
            return Ok(None);
        };
        // A zombie still appears in `ps`, but it cannot own a live host.
        if state.starts_with('Z') {
            return Ok(None);
        }
        let start_identity = start_identity.trim().to_string();
        if start_identity.is_empty() {
            return Ok(None);
        }
        Ok(Some(ProcessIdentity {
            pid,
            start_identity,
        }))
    }
    #[cfg(windows)]
    {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

        let process_id = Pid::from_u32(pid);
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[process_id]),
            true,
            ProcessRefreshKind::nothing(),
        );
        let Some(process) = system.process(process_id) else {
            return Ok(None);
        };
        if process.start_time() == 0 {
            return Err(PlatformError::Process(
                "process start identity is unavailable".to_string(),
            ));
        }
        Ok(Some(ProcessIdentity {
            pid,
            start_identity: format!("windows:{}", process.start_time()),
        }))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err(PlatformError::UnsupportedPlatform)
    }
}

/// Put a child in its own process group/session and detach it from terminal IO.
pub fn configure_detached(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = command;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_identity_has_a_start_time() {
        let identity = current_process_identity().unwrap();
        assert_eq!(identity.pid, std::process::id());
        assert!(!identity.start_identity.is_empty());
    }

    #[test]
    fn dead_pid_has_no_identity() {
        assert_eq!(process_identity(u32::MAX).unwrap(), None);
    }

    #[test]
    fn repeated_process_identity_is_stable() {
        let first = current_process_identity().unwrap();
        for _ in 0..10 {
            assert_eq!(process_identity(first.pid).unwrap(), Some(first.clone()));
        }
    }

    #[cfg(windows)]
    #[test]
    fn exited_process_has_no_identity() {
        let mut child = Command::new("cmd")
            .args(["/D", "/C", "exit", "0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        assert!(child.wait().unwrap().success());
        assert_eq!(process_identity(pid).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn zombie_pid_has_no_identity() {
        let mut child = Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(process_identity(child.id()).unwrap(), None);
        let _ = child.wait();
    }
}
