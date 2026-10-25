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
            .args(["-p", &pid.to_string(), "-o", "lstart="])
            .output()
            .map_err(|error| PlatformError::Process(format!("ps could not run: {error}")))?;
        if !output.status.success() {
            return Ok(None);
        }
        let start_identity = String::from_utf8_lossy(&output.stdout).trim().to_string();
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
        let script = format!(
            "$p = Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}'; if ($p) {{ $p.CreationDate.ToString('o') }}"
        );
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .map_err(|error| {
                PlatformError::Process(format!("PowerShell could not run: {error}"))
            })?;
        if !output.status.success() {
            return Ok(None);
        }
        let start_identity = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if start_identity.is_empty() {
            return Ok(None);
        }
        Ok(Some(ProcessIdentity {
            pid,
            start_identity,
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
}
