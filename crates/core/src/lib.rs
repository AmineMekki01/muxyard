//! Shared native foundation, independent of the desktop UI and host lifecycle.

use muxyard_contracts::PROTOCOL_VERSION;

/// Return the host package and protocol versions without starting any services.
pub fn host_version() -> String {
    format!(
        "muxyard-host {} (protocol {})",
        env!("CARGO_PKG_VERSION"),
        PROTOCOL_VERSION
    )
}
