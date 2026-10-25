//! Managed PTY sessions exposed to the host control endpoint.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use muxyard_contracts::{
    Id, TerminalCloseRequest, TerminalInputRequest, TerminalResizeRequest, TerminalSpawnRequest,
    TerminalSpawnResponse,
};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    PtyError, PtyExitStatus, PtySession, PtySessionConfig, TerminalHub, TerminalHubError,
    TerminalSubscription,
};

/// Host-owned registry of live PTY sessions.
pub struct TerminalService {
    hub: Arc<TerminalHub>,
    sessions: Mutex<HashMap<Uuid, PtySession>>,
}

impl TerminalService {
    /// Create a service around a terminal output hub.
    pub fn new(hub: Arc<TerminalHub>) -> Self {
        Self {
            hub,
            sessions: Mutex::new(HashMap::new()),
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
        let session = PtySession::spawn(
            PtySessionConfig {
                shell,
                cwd,
                rows: request.rows,
                cols: request.cols,
                environment: Vec::new(),
            },
            Arc::clone(&self.hub),
        )?;
        let identity = session.identity();
        let (rows, cols) = session.size()?;
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
        Ok(session.close()?)
    }

    /// Subscribe a client to a session's output.
    pub fn attach(
        &self,
        stream_id: Uuid,
        queue_bytes: Option<u32>,
    ) -> Result<TerminalSubscription, TerminalServiceError> {
        Ok(self.hub.subscribe(stream_id, queue_bytes)?)
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
            let _ = session.close();
        }
    }
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
}
