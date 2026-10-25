//! Durable host-instance records used for restart reconciliation.

use rusqlite::{Connection, OptionalExtension, params};

use crate::StorageError;

/// Host lifecycle state recorded in SQLite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostState {
    /// Process has started but has not validated configuration.
    Starting,
    /// Schema migrations are running.
    MigratingDb,
    /// The local endpoint is being established.
    BindingLocalApi,
    /// Host is accepting clients.
    Ready,
    /// Host is refusing new work while shutting down.
    Draining,
    /// Host exited cleanly.
    Stopped,
    /// Host entered an error state.
    Failed,
}

impl HostState {
    /// Every supported state.
    pub const ALL: [Self; 7] = [
        Self::Starting,
        Self::MigratingDb,
        Self::BindingLocalApi,
        Self::Ready,
        Self::Draining,
        Self::Stopped,
        Self::Failed,
    ];

    /// SQL and diagnostic spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::MigratingDb => "migrating_db",
            Self::BindingLocalApi => "binding_local_api",
            Self::Ready => "ready",
            Self::Draining => "draining",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Result<Self, StorageError> {
        Self::ALL
            .into_iter()
            .find(|state| state.as_str() == value)
            .ok_or_else(|| StorageError::InvalidHostState(value.to_string()))
    }
}

/// One host process identity. The start token prevents PID reuse from proving liveness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInstance {
    /// Unique process instance identifier.
    pub id: String,
    /// Operating-system process ID.
    pub pid: u32,
    /// Operating-system user that owns the host.
    pub os_user: String,
    /// OS process start identity used with PID to reject PID reuse.
    pub process_start_token: String,
    /// Host package version.
    pub host_version: String,
    /// Wire protocol version.
    pub protocol_version: u8,
    /// Current lifecycle state.
    pub state: HostState,
    /// Process start time in Unix epoch milliseconds.
    pub started_ms: i64,
    /// Last recorded heartbeat in Unix epoch milliseconds.
    pub heartbeat_ms: i64,
}

/// Insert a host instance or update the same instance idempotently.
pub fn record_host_instance(
    connection: &Connection,
    instance: &HostInstance,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO host_instances (
            id, pid, os_user, process_start_token, host_version,
            protocol_version, state, started_ms, heartbeat_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET
            pid = excluded.pid,
            os_user = excluded.os_user,
            process_start_token = excluded.process_start_token,
            host_version = excluded.host_version,
            protocol_version = excluded.protocol_version,
            state = excluded.state,
            started_ms = excluded.started_ms,
            heartbeat_ms = excluded.heartbeat_ms",
        params![
            instance.id,
            i64::from(instance.pid),
            instance.os_user,
            instance.process_start_token,
            instance.host_version,
            i64::from(instance.protocol_version),
            instance.state.as_str(),
            instance.started_ms,
            instance.heartbeat_ms,
        ],
    )?;
    Ok(())
}

/// Update lifecycle state and heartbeat. Returns whether the instance exists.
pub fn update_host_instance_state(
    connection: &Connection,
    id: &str,
    state: HostState,
    heartbeat_ms: i64,
) -> Result<bool, StorageError> {
    let changed = connection.execute(
        "UPDATE host_instances SET state = ?2, heartbeat_ms = ?3 WHERE id = ?1",
        params![id, state.as_str(), heartbeat_ms],
    )?;
    Ok(changed > 0)
}

/// Mark older non-terminal host rows stopped after a fresh single-instance start.
pub fn reconcile_stale_host_instances(
    connection: &Connection,
    live_instance_id: &str,
    heartbeat_ms: i64,
) -> Result<usize, StorageError> {
    let changed = connection.execute(
        "UPDATE host_instances
         SET state = 'stopped', heartbeat_ms = ?2
         WHERE id != ?1 AND state NOT IN ('stopped', 'failed')",
        params![live_instance_id, heartbeat_ms],
    )?;
    Ok(changed)
}

/// Mark sessions owned by previous host instances as lost.
pub fn mark_stale_terminal_sessions(
    connection: &Connection,
    live_instance_id: &str,
    updated_ms: i64,
) -> Result<usize, StorageError> {
    let changed = connection.execute(
        "UPDATE terminal_sessions
         SET state = 'lost', updated_ms = ?2
         WHERE host_instance_id != ?1 AND state NOT IN ('exited', 'lost')",
        params![live_instance_id, updated_ms],
    )?;
    Ok(changed)
}

/// Read one host instance, if present.
pub fn get_host_instance(
    connection: &Connection,
    id: &str,
) -> Result<Option<HostInstance>, StorageError> {
    let row = connection
        .query_row(
            "SELECT id, pid, os_user, process_start_token, host_version,
                    protocol_version, state, started_ms, heartbeat_ms
             FROM host_instances WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .optional()?;

    let Some((
        id,
        pid,
        os_user,
        process_start_token,
        host_version,
        protocol_version,
        state,
        started_ms,
        heartbeat_ms,
    )) = row
    else {
        return Ok(None);
    };
    let pid =
        u32::try_from(pid).map_err(|_| StorageError::InvalidHostIdentity(format!("pid {pid}")))?;
    let protocol_version = u8::try_from(protocol_version)
        .map_err(|_| StorageError::InvalidHostIdentity(format!("protocol {protocol_version}")))?;
    Ok(Some(HostInstance {
        id,
        pid,
        os_user,
        process_start_token,
        host_version,
        protocol_version,
        state: HostState::parse(&state)?,
        started_ms,
        heartbeat_ms,
    }))
}
