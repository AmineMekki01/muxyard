//! Rust-authoritative protocol metadata. Control DTOs are implemented in FND-002.

/// Version of the locked local control protocol, not a running IPC endpoint.
pub const PROTOCOL_VERSION: u8 = 1;

#[cfg(test)]
mod tests {
    use super::PROTOCOL_VERSION;

    #[test]
    fn protocol_matches_the_architecture_lock() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }
}
