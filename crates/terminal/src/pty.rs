//! Native PTY-backed shell sessions.

use std::{
    ffi::OsString,
    io::{self, Read, Write},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use muxyard_platform::{PlatformError, resolve_shell, validate_working_directory};
use portable_pty::{
    ChildKiller, CommandBuilder, ExitStatus, MasterPty, PtySize, native_pty_system,
};
use thiserror::Error;
use uuid::Uuid;

use crate::{TerminalHub, TerminalHubError};

const READ_CHUNK_BYTES: usize = 16 * 1024;

/// Validated shell launch settings.
#[derive(Debug, Clone)]
pub struct PtySessionConfig {
    /// Optional explicit shell executable. Relative paths are rejected.
    pub shell: Option<PathBuf>,
    /// Working directory; canonicalized before launch.
    pub cwd: PathBuf,
    /// Initial rows.
    pub rows: u16,
    /// Initial columns.
    pub cols: u16,
    /// Extra environment entries applied after the inherited environment.
    pub environment: Vec<(OsString, OsString)>,
}

impl Default for PtySessionConfig {
    fn default() -> Self {
        Self {
            shell: None,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            rows: 24,
            cols: 80,
            environment: Vec::new(),
        }
    }
}

impl PtySessionConfig {
    /// Build a default-sized session in `cwd`.
    pub fn in_directory(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            ..Self::default()
        }
    }
}

/// Stable identity and launch context for one PTY session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtySessionIdentity {
    /// Stream/session identifier.
    pub stream_id: Uuid,
    /// Resolved shell executable.
    pub shell: PathBuf,
    /// Canonicalized working directory.
    pub cwd: PathBuf,
}

/// Portable exit status captured for a PTY child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyExitStatus {
    /// Process exit code.
    pub code: u32,
    /// Signal description when the child was terminated by a signal.
    pub signal: Option<String>,
}

impl PtyExitStatus {
    /// Whether the child exited successfully.
    pub fn success(&self) -> bool {
        self.signal.is_none() && self.code == 0
    }
}

impl From<ExitStatus> for PtyExitStatus {
    fn from(status: ExitStatus) -> Self {
        Self {
            code: status.exit_code(),
            signal: status.signal().map(str::to_string),
        }
    }
}

/// A live PTY shell and its output pump.
pub struct PtySession {
    stream_id: Uuid,
    shell: PathBuf,
    cwd: PathBuf,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    child_pid: Option<u32>,
    exit: Arc<(Mutex<Option<PtyExitStatus>>, Condvar)>,
    reader_thread: Option<JoinHandle<()>>,
    wait_thread: Option<JoinHandle<()>>,
    hub: Arc<TerminalHub>,
    closed: bool,
}

impl PtySession {
    /// Open a native PTY and launch the resolved shell into it.
    pub fn spawn(config: PtySessionConfig, hub: Arc<TerminalHub>) -> Result<Self, PtyError> {
        if config.rows == 0 || config.cols == 0 {
            return Err(PtyError::InvalidSize);
        }
        let shell = resolve_shell(config.shell.as_deref())?;
        let cwd = validate_working_directory(&config.cwd)?;
        let stream_id = hub.create_session()?;

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: config.rows,
                cols: config.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| {
                let _ = hub.remove_session(stream_id);
                PtyError::Pty(error.to_string())
            })?;

        let mut command = CommandBuilder::new(&shell);
        command.cwd(&cwd);
        command.env("TERM", "xterm-256color");
        for (key, value) in config.environment {
            command.env(key, value);
        }

        let mut child = match pair.slave.spawn_command(command) {
            Ok(child) => child,
            Err(error) => {
                let _ = hub.remove_session(stream_id);
                return Err(PtyError::Pty(error.to_string()));
            }
        };
        let mut killer = child.clone_killer();
        let child_pid = child.process_id();
        let reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(error) => {
                let _ = child.kill();
                let _ = hub.remove_session(stream_id);
                return Err(PtyError::Pty(error.to_string()));
            }
        };
        let writer = match pair.master.take_writer() {
            Ok(writer) => writer,
            Err(error) => {
                let _ = child.kill();
                let _ = hub.remove_session(stream_id);
                return Err(PtyError::Pty(error.to_string()));
            }
        };
        let master = pair.master;

        let exit = Arc::new((Mutex::new(None), Condvar::new()));
        let reader_hub = Arc::clone(&hub);
        let reader_thread = match thread::Builder::new()
            .name("muxyard-pty-reader".to_string())
            .spawn(move || {
                let mut reader = reader;
                let mut buffer = [0_u8; READ_CHUNK_BYTES];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => {
                            if reader_hub.publish(stream_id, &buffer[..count]).is_err() {
                                break;
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
            }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = child.kill();
                let _ = hub.remove_session(stream_id);
                return Err(PtyError::Io(error));
            }
        };

        let wait_exit = Arc::clone(&exit);
        let wait_thread = match thread::Builder::new()
            .name("muxyard-pty-wait".to_string())
            .spawn(move || {
                let status = child.wait().ok().map(PtyExitStatus::from);
                let (lock, ready) = &*wait_exit;
                *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = status;
                ready.notify_all();
            }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = killer.kill();
                let _ = hub.remove_session(stream_id);
                let _ = reader_thread.join();
                return Err(PtyError::Io(error));
            }
        };

        Ok(Self {
            stream_id,
            shell,
            cwd,
            master: Some(master),
            writer: Some(writer),
            killer,
            child_pid,
            exit,
            reader_thread: Some(reader_thread),
            wait_thread: Some(wait_thread),
            hub,
            closed: false,
        })
    }

    /// Stream identifier used by `TerminalHub`.
    pub fn stream_id(&self) -> Uuid {
        self.stream_id
    }

    /// Stable identity and launch context.
    pub fn identity(&self) -> PtySessionIdentity {
        PtySessionIdentity {
            stream_id: self.stream_id,
            shell: self.shell.clone(),
            cwd: self.cwd.clone(),
        }
    }

    /// Resolved shell executable.
    pub fn shell(&self) -> &PathBuf {
        &self.shell
    }

    /// Canonicalized working directory.
    pub fn cwd(&self) -> &PathBuf {
        &self.cwd
    }

    /// Write input bytes to the PTY.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), PtyError> {
        let writer = self.writer.as_mut().ok_or(PtyError::Closed)?;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    /// Resize the PTY.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        if rows == 0 || cols == 0 {
            return Err(PtyError::InvalidSize);
        }
        self.master
            .as_ref()
            .ok_or(PtyError::Closed)?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| PtyError::Pty(error.to_string()))
    }

    /// Current kernel PTY size.
    pub fn size(&self) -> Result<(u16, u16), PtyError> {
        let size = self
            .master
            .as_ref()
            .ok_or(PtyError::Closed)?
            .get_size()
            .map_err(|error| PtyError::Pty(error.to_string()))?;
        Ok((size.rows, size.cols))
    }

    /// Terminate the child process and its process group where supported.
    pub fn kill(&mut self) -> Result<(), PtyError> {
        if self.closed {
            return Err(PtyError::Closed);
        }
        let result = self.killer.kill();
        terminate_process_group(self.child_pid);
        result.map_err(|error| PtyError::Pty(error.to_string()))
    }

    /// Wait for the child to exit, returning `None` on timeout.
    pub fn wait_for_exit(&self, timeout: Duration) -> Option<PtyExitStatus> {
        let (lock, ready) = &*self.exit;
        let guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (guard, _) = ready
            .wait_timeout_while(guard, timeout, |status| status.is_none())
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    }

    /// Current exit status, if the child has exited.
    pub fn exit_status(&self) -> Option<PtyExitStatus> {
        self.exit
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Whether the child is still running.
    pub fn is_running(&self) -> bool {
        self.exit_status().is_none() && !self.closed
    }

    /// Kill the child, close the PTY, and join worker threads.
    pub fn close(mut self) -> Result<Option<PtyExitStatus>, PtyError> {
        self.shutdown();
        Ok(self.exit_status())
    }

    fn shutdown(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        terminate_process_group(self.child_pid);
        let _ = self.killer.kill();
        self.writer.take();
        self.master.take();
        self.hub.remove_session(self.stream_id);
        if let Some(thread) = self.reader_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.wait_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(unix)]
fn terminate_process_group(child_pid: Option<u32>) {
    if let Some(pid) = child_pid {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

#[cfg(not(unix))]
fn terminate_process_group(_child_pid: Option<u32>) {}

/// PTY launch, I/O, or lifecycle failure.
#[derive(Debug, Error)]
pub enum PtyError {
    /// Shell or working-directory validation failed.
    #[error(transparent)]
    Platform(#[from] PlatformError),
    /// Terminal stream registration failed.
    #[error(transparent)]
    Hub(#[from] TerminalHubError),
    /// PTY or child-process operation failed.
    #[error("PTY operation failed: {0}")]
    Pty(String),
    /// PTY or thread I/O failed.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The session is closed.
    #[error("terminal session is closed")]
    Closed,
    /// Rows or columns were zero.
    #[error("terminal size must have positive rows and columns")]
    InvalidSize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_status_reports_success_and_failure() {
        assert!(
            PtyExitStatus {
                code: 0,
                signal: None
            }
            .success()
        );
        assert!(
            !PtyExitStatus {
                code: 7,
                signal: None
            }
            .success()
        );
        assert!(
            !PtyExitStatus {
                code: 1,
                signal: Some("SIGTERM".to_string())
            }
            .success()
        );
    }

    #[test]
    fn invalid_size_is_rejected_before_spawning() {
        let config = PtySessionConfig {
            rows: 0,
            ..PtySessionConfig::default()
        };
        assert!(matches!(
            PtySession::spawn(
                config,
                Arc::new(TerminalHub::new(crate::TerminalLimits::default()).unwrap())
            ),
            Err(PtyError::InvalidSize)
        ));
    }

    #[test]
    fn invalid_cwd_does_not_register_a_stream() {
        let hub = Arc::new(TerminalHub::new(crate::TerminalLimits::default()).unwrap());
        let config = PtySessionConfig::in_directory("/muxyard-test/not-a-real-directory");
        assert!(matches!(
            PtySession::spawn(config, Arc::clone(&hub)),
            Err(PtyError::Platform(_))
        ));
        assert_eq!(hub.session_count(), 0);
    }
}
