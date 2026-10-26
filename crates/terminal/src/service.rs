//! Managed PTY sessions exposed to the host control endpoint.

use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use muxyard_contracts::{
    Id, TerminalCloseRequest, TerminalInputRequest, TerminalResizeRequest, TerminalSnapshotRequest,
    TerminalSpawnRequest, TerminalSpawnResponse,
};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    PtyError, PtyExitStatus, PtySession, PtySessionConfig, TerminalHub, TerminalHubError,
    TerminalSubscription,
};

/// Persistence hook for terminal session metadata.
pub trait TerminalSessionStore: Send + Sync {
    /// Record a newly spawned shell.
    fn record_spawned(
        &self,
        identity: &crate::PtySessionIdentity,
    ) -> Result<(), TerminalStoreError>;
    /// Record a closed shell lifecycle state.
    fn record_closed(&self, stream_id: Uuid, state: &str) -> Result<(), TerminalStoreError>;
}

/// Terminal persistence failure.
#[derive(Debug, Error)]
#[error("terminal session persistence failed: {0}")]
pub struct TerminalStoreError(pub String);

/// Host-owned registry of live PTY sessions.
pub struct TerminalService {
    hub: Arc<TerminalHub>,
    sessions: Mutex<HashMap<Uuid, PtySession>>,
    store: Option<Arc<dyn TerminalSessionStore>>,
    history_dir: Option<PathBuf>,
}

impl TerminalService {
    /// Create a service around a terminal output hub.
    pub fn new(hub: Arc<TerminalHub>) -> Self {
        Self {
            hub,
            sessions: Mutex::new(HashMap::new()),
            store: None,
            history_dir: None,
        }
    }

    /// Create a service with private per-pane history files and no durable store.
    pub fn with_history(hub: Arc<TerminalHub>, history_dir: PathBuf) -> Self {
        Self {
            hub,
            sessions: Mutex::new(HashMap::new()),
            store: None,
            history_dir: Some(history_dir),
        }
    }

    /// Create a service with durable session metadata and private per-pane history files.
    pub fn with_store_and_history(
        hub: Arc<TerminalHub>,
        store: Arc<dyn TerminalSessionStore>,
        history_dir: PathBuf,
    ) -> Self {
        Self {
            hub,
            sessions: Mutex::new(HashMap::new()),
            store: Some(store),
            history_dir: Some(history_dir),
        }
    }

    /// Output hub shared with IPC subscribers.
    pub fn hub(&self) -> &Arc<TerminalHub> {
        &self.hub
    }

    /// Spawn a shell and register its stream.
    pub fn spawn(
        &self,
        request: TerminalSpawnRequest,
    ) -> Result<TerminalSpawnResponse, TerminalServiceError> {
        let cwd = match request.cwd {
            Some(cwd) => PathBuf::from(cwd),
            None => std::env::current_dir().map_err(PtyError::Io)?,
        };
        let shell = request.shell.map(PathBuf::from);
        let mut environment = Vec::new();
        if let Some(history_id) = &request.history_id {
            if !valid_history_id(&history_id.0) {
                return Err(TerminalServiceError::InvalidHistoryId(history_id.clone()));
            }
            if let Some(history_dir) = &self.history_dir {
                fs::create_dir_all(history_dir).map_err(PtyError::Io)?;
                let history_path = history_dir.join(format!("{}.history", history_id.0));
                environment.push((
                    OsString::from("HISTFILE"),
                    history_path.clone().into_os_string(),
                ));
                environment.push((OsString::from("HISTSIZE"), OsString::from("10000")));
                environment.push((OsString::from("SAVEHIST"), OsString::from("10000")));
                let resolved_shell = muxyard_platform::resolve_shell(shell.as_deref())
                    .map_err(PtyError::Platform)?;
                if resolved_shell.file_name().is_some_and(|name| name == "zsh") {
                    let zdotdir = prepare_zsh_zdotdir(history_dir, &history_id.0, &history_path)?;
                    environment.push((OsString::from("ZDOTDIR"), zdotdir.into_os_string()));
                }
            }
        }
        let session = PtySession::spawn_deferred(
            PtySessionConfig {
                shell,
                cwd,
                rows: request.rows,
                cols: request.cols,
                environment,
            },
            Arc::clone(&self.hub),
        )?;
        let identity = session.identity();
        let (rows, cols) = session.size()?;
        if let Some(store) = &self.store
            && let Err(error) = store.record_spawned(&identity)
        {
            let _ = session.close();
            return Err(TerminalServiceError::Store(error));
        }
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(identity.stream_id, session);
        Ok(TerminalSpawnResponse {
            stream_id: Id(identity.stream_id.to_string()),
            shell: identity.shell.to_string_lossy().into_owned(),
            cwd: identity.cwd.to_string_lossy().into_owned(),
            rows,
            cols,
        })
    }

    /// Write input to a live session.
    pub fn input(&self, request: &TerminalInputRequest) -> Result<(), TerminalServiceError> {
        let stream_id = parse_stream_id(&request.stream_id)?;
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let session = sessions
            .get_mut(&stream_id)
            .ok_or(TerminalServiceError::UnknownSession(stream_id))?;
        session.write(request.data.as_bytes())?;
        Ok(())
    }

    /// Resize a live session.
    pub fn resize(&self, request: &TerminalResizeRequest) -> Result<(), TerminalServiceError> {
        let stream_id = parse_stream_id(&request.stream_id)?;
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let session = sessions
            .get(&stream_id)
            .ok_or(TerminalServiceError::UnknownSession(stream_id))?;
        session.resize(request.rows, request.cols)?;
        Ok(())
    }

    /// Close a live session and return its exit status when available.
    pub fn close(
        &self,
        request: &TerminalCloseRequest,
    ) -> Result<Option<PtyExitStatus>, TerminalServiceError> {
        let stream_id = parse_stream_id(&request.stream_id)?;
        let session = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&stream_id)
            .ok_or(TerminalServiceError::UnknownSession(stream_id))?;
        let status = session.close()?;
        if let Some(store) = &self.store {
            store.record_closed(stream_id, "exited")?;
        }
        Ok(status)
    }

    /// Subscribe a client to a session's output.
    pub fn attach(
        &self,
        stream_id: Uuid,
        queue_bytes: Option<u32>,
    ) -> Result<TerminalSubscription, TerminalServiceError> {
        let subscription = self.hub.subscribe(stream_id, queue_bytes)?;
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(session) = sessions.get(&stream_id) else {
            return Ok(subscription);
        };
        if !session.is_running() {
            let session = sessions
                .remove(&stream_id)
                .ok_or(TerminalServiceError::UnknownSession(stream_id))?;
            drop(sessions);
            let _ = session.close();
            if let Some(store) = &self.store {
                let _ = store.record_closed(stream_id, "exited");
            }
            return Err(TerminalServiceError::UnknownSession(stream_id));
        }
        session.start_reader();
        Ok(subscription)
    }

    /// Read the bounded recent output tail for a live session.
    pub fn snapshot(
        &self,
        request: &TerminalSnapshotRequest,
    ) -> Result<Vec<u8>, TerminalServiceError> {
        let stream_id = parse_stream_id(&request.stream_id)?;
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let session = sessions
            .get(&stream_id)
            .ok_or(TerminalServiceError::UnknownSession(stream_id))?;
        Ok(session.snapshot())
    }

    /// Number of live sessions.
    pub fn session_count(&self) -> usize {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Close every live session, for host shutdown.
    pub fn close_all(&self) {
        let sessions = {
            let mut sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            sessions
                .drain()
                .map(|(_, session)| session)
                .collect::<Vec<_>>()
        };
        for session in sessions {
            let stream_id = session.stream_id();
            let _ = session.close();
            if let Some(store) = &self.store {
                let _ = store.record_closed(stream_id, "exited");
            }
        }
    }
}

fn prepare_zsh_zdotdir(
    history_dir: &Path,
    history_id: &str,
    history_path: &Path,
) -> Result<PathBuf, PtyError> {
    let zdotdir = history_dir.join(format!("{history_id}.zdotdir"));
    fs::create_dir_all(&zdotdir).map_err(PtyError::Io)?;
    let user_zdotdir = std::env::var_os("ZDOTDIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from));
    let history = shell_quote(&history_path.to_string_lossy());
    let history_setup = format!("HISTFILE={history}\nHISTSIZE=10000\nSAVEHIST=10000\n");
    let wrapper = shell_quote(&zdotdir.to_string_lossy());
    let original_zdotdir = user_zdotdir
        .as_ref()
        .map(|path| shell_quote(&path.to_string_lossy()));
    let original_setup = original_zdotdir
        .as_ref()
        .map(|path| format!("ZDOTDIR={path}\n"))
        .unwrap_or_default();
    let source = |name: &str| {
        user_zdotdir.as_ref().map(|home| {
            let file = shell_quote(&home.join(name).to_string_lossy());
            format!("if [ -f {file} ]; then source {file}; fi\n")
        })
    };
    fs::write(
        zdotdir.join(".zshenv"),
        format!(
            "{}{}ZDOTDIR={wrapper}\n",
            original_setup,
            source(".zshenv").unwrap_or_default()
        ),
    )
    .map_err(PtyError::Io)?;
    fs::write(
        zdotdir.join(".zprofile"),
        format!(
            "{original_setup}{}ZDOTDIR={wrapper}\n",
            source(".zprofile").unwrap_or_default()
        ),
    )
    .map_err(PtyError::Io)?;
    fs::write(
        zdotdir.join(".zshrc"),
        format!(
            "{original_setup}{history_setup}{}{history_setup}{original_setup}",
            source(".zshrc").unwrap_or_default()
        ),
    )
    .map_err(PtyError::Io)?;
    fs::write(
        zdotdir.join(".zlogin"),
        format!(
            "{original_setup}{}ZDOTDIR={wrapper}\n",
            source(".zlogin").unwrap_or_default()
        ),
    )
    .map_err(PtyError::Io)?;
    Ok(zdotdir)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn valid_history_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn parse_stream_id(id: &Id) -> Result<Uuid, TerminalServiceError> {
    Uuid::parse_str(&id.0).map_err(|_| TerminalServiceError::InvalidStreamId(id.clone()))
}

/// Terminal service failure.
#[derive(Debug, Error)]
pub enum TerminalServiceError {
    /// PTY lifecycle failed.
    #[error(transparent)]
    Pty(#[from] PtyError),
    /// Output hub operation failed.
    #[error(transparent)]
    Hub(#[from] TerminalHubError),
    /// Requested stream is not live.
    #[error("terminal session {0} is not live")]
    UnknownSession(Uuid),
    /// Stream identifier is malformed.
    #[error("terminal stream identifier is malformed: {0:?}")]
    InvalidStreamId(Id),
    /// History identifier is malformed.
    #[error("terminal history identifier is malformed: {0:?}")]
    InvalidHistoryId(Id),
    /// Durable session metadata could not be recorded.
    #[error(transparent)]
    Store(#[from] TerminalStoreError),
}
