//! Typed client for the host's protected local control endpoint.

use std::time::{Duration, Instant};

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::prelude::*;
use muxyard_contracts::{ControlEnvelope, ControlMessageType, Id, ProtocolVersion, decode_control};
use muxyard_platform::PlatformPaths;
use muxyard_terminal::{
    TerminalAttachRequest, TerminalAttachResponse, TerminalFrame, TerminalSequence,
};
use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;

use crate::{
    HOST_STATUS_METHOD, HostStatusRequest, HostStatusResponse, IpcError, IpcResponse,
    TERMINAL_ATTACH_METHOD, read_frame, read_frame_until, write_frame_until,
};

/// Connected local control client.
pub struct IpcClient {
    stream: LocalSocketStream,
    timeout: Duration,
}

impl IpcClient {
    /// Connect with the default five-second request timeout.
    pub fn connect(paths: &PlatformPaths) -> Result<Self, IpcError> {
        Self::connect_with_timeout(paths, Duration::from_secs(5))
    }

    /// Connect with an explicit request timeout.
    pub fn connect_with_timeout(
        paths: &PlatformPaths,
        timeout: Duration,
    ) -> Result<Self, IpcError> {
        let stream = connect_stream(paths)?;
        configure_stream(&stream, timeout)?;
        Ok(Self { stream, timeout })
    }

    /// Send one typed request and validate response correlation.
    pub fn request<T: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        data: T,
    ) -> Result<IpcResponse<R>, IpcError> {
        let id = Id(Uuid::new_v4().to_string());
        let request = ControlEnvelope {
            v: ProtocolVersion,
            id: id.clone(),
            message_type: ControlMessageType::Request,
            method: method.to_string(),
            data,
        };
        let bytes = serde_json::to_vec(&request)?;
        let deadline = Instant::now() + self.timeout;
        write_frame_until(&mut self.stream, &bytes, deadline).map_err(classify_timeout)?;

        let frame = read_frame_until(&mut self.stream, deadline)
            .map_err(classify_timeout)?
            .ok_or(IpcError::Closed)?;
        let response: ControlEnvelope<IpcResponse<R>> = decode_control(&frame)?;
        if response.message_type != ControlMessageType::Response
            || response.id != id
            || response.method != method
        {
            return Err(IpcError::Protocol);
        }
        Ok(response.data)
    }

    /// Query host identity and readiness.
    pub fn host_status(&mut self) -> Result<HostStatusResponse, IpcError> {
        match self.request(HOST_STATUS_METHOD, HostStatusRequest {})? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { error } => Err(IpcError::Api(error)),
        }
    }

    /// Attach this connection to a terminal stream and switch it to binary frames.
    pub fn attach_terminal(
        mut self,
        stream_id: Uuid,
        queue_bytes: Option<u32>,
    ) -> Result<TerminalStream, IpcError> {
        let response: IpcResponse<TerminalAttachResponse> = self.request(
            TERMINAL_ATTACH_METHOD,
            TerminalAttachRequest {
                stream_id,
                queue_bytes,
            },
        )?;
        match response {
            IpcResponse::Ok { data } => {
                if data.stream_id != stream_id {
                    return Err(IpcError::TerminalProtocol);
                }
                configure_terminal_stream(&self.stream)?;
                Ok(TerminalStream {
                    stream: self.stream,
                    stream_id,
                    sequence: TerminalSequence::starting_at(data.next_sequence),
                })
            }
            IpcResponse::Error { error } => Err(IpcError::Api(error)),
        }
    }
}

/// A connection switched to one terminal stream after `terminal.attach`.
pub struct TerminalStream {
    stream: LocalSocketStream,
    stream_id: Uuid,
    sequence: TerminalSequence,
}

impl TerminalStream {
    /// Stream identifier.
    pub fn stream_id(&self) -> Uuid {
        self.stream_id
    }

    /// Read the next terminal frame, blocking until output or connection close.
    pub fn next_frame(&mut self) -> Result<TerminalFrame, IpcError> {
        let frame = read_frame(&mut self.stream)?.ok_or(IpcError::Closed)?;
        let frame = TerminalFrame::decode(&frame)?;
        if frame.stream_id() != self.stream_id {
            return Err(IpcError::TerminalProtocol);
        }
        self.sequence.accept(frame.sequence())?;
        Ok(frame)
    }
}

#[cfg(unix)]
fn configure_stream(stream: &LocalSocketStream, timeout: Duration) -> Result<(), IpcError> {
    stream.set_nonblocking(false)?;
    stream.set_recv_timeout(Some(timeout))?;
    stream.set_send_timeout(Some(timeout))?;
    Ok(())
}

#[cfg(windows)]
fn configure_stream(stream: &LocalSocketStream, _timeout: Duration) -> Result<(), IpcError> {
    stream.set_nonblocking(true)?;
    Ok(())
}

#[cfg(unix)]
fn configure_terminal_stream(stream: &LocalSocketStream) -> Result<(), IpcError> {
    stream.set_nonblocking(false)?;
    stream.set_recv_timeout(None)?;
    Ok(())
}

#[cfg(windows)]
fn configure_terminal_stream(stream: &LocalSocketStream) -> Result<(), IpcError> {
    stream.set_nonblocking(false)?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn configure_terminal_stream(_stream: &LocalSocketStream) -> Result<(), IpcError> {
    Err(IpcError::UnsupportedPlatform)
}

#[cfg(not(any(unix, windows)))]
fn configure_stream(_stream: &LocalSocketStream, _timeout: Duration) -> Result<(), IpcError> {
    Err(IpcError::UnsupportedPlatform)
}

fn classify_timeout(error: IpcError) -> IpcError {
    match error {
        IpcError::Io(source)
            if matches!(
                source.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) =>
        {
            IpcError::Timeout
        }
        other => other,
    }
}

#[cfg(unix)]
fn connect_stream(paths: &PlatformPaths) -> Result<LocalSocketStream, IpcError> {
    let path = paths.local_socket_path();
    let name = path.as_path().to_fs_name::<GenericFilePath>()?;
    LocalSocketStream::connect(name).map_err(IpcError::Io)
}

#[cfg(windows)]
fn connect_stream(_paths: &PlatformPaths) -> Result<LocalSocketStream, IpcError> {
    let pipe_name = crate::server::windows_pipe_name()
        .ok_or_else(|| IpcError::Endpoint("current-user SID is unavailable".to_string()))?;
    let name = pipe_name.as_str().to_ns_name::<GenericNamespaced>()?;
    LocalSocketStream::connect(name).map_err(IpcError::Io)
}

#[cfg(not(any(unix, windows)))]
fn connect_stream(_paths: &PlatformPaths) -> Result<LocalSocketStream, IpcError> {
    Err(IpcError::UnsupportedPlatform)
}
