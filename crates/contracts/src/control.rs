//! JSON control messages, independent of socket or named-pipe framing.

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeOwned, IgnoredAny},
};

use crate::{ApiError, ErrorCode, Id, MAX_CONTROL_BYTES, PROTOCOL_VERSION, ProtocolVersion};

/// Control message direction/category. It grants no action authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum ControlMessageType {
    /// A typed operation request.
    Request,
    /// Reply correlated to a request by its identifier.
    Response,
    /// An unsolicited control event.
    Event,
}

/// Versioned local control message. Terminal bytes use a separate binary stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ControlEnvelope<T> {
    /// Supported protocol version, serialized as the number 1.
    pub v: ProtocolVersion,
    /// Request/response correlation identifier, or an event identifier.
    pub id: Id,
    /// Wire key `type`, not a Rust or TypeScript discriminant for authorization.
    #[serde(rename = "type")]
    pub message_type: ControlMessageType,
    /// Scoped operation name. The host must validate and authorize dispatch.
    pub method: String,
    /// Method-specific typed data, never an unrestricted command string.
    pub data: T,
}

/// Decode one bounded JSON control payload, without its length prefix.
///
/// Checks version before decoding method-specific data. Errors use fixed safe
/// messages rather than returning parser excerpts of potentially sensitive input.
/// The receiving domain must still validate identifiers, methods, and authority.
///
/// # Errors
///
/// Returns `PAYLOAD_TOO_LARGE` above 1 MiB, `UNSUPPORTED_VERSION` for an unknown
/// unsigned integer version, or `INVALID_REQUEST` for malformed JSON or DTOs.
pub fn decode_control<T: DeserializeOwned>(bytes: &[u8]) -> Result<ControlEnvelope<T>, ApiError> {
    if bytes.len() > MAX_CONTROL_BYTES {
        return Err(ApiError {
            code: ErrorCode::PayloadTooLarge,
            message: "Control payload exceeds the 1 MiB limit.".into(),
        });
    }

    // Inspect the header without building a second JSON value tree.
    #[derive(Deserialize)]
    struct Header {
        v: u64,
        // A nullable payload is valid, but omitting the data field is not.
        #[serde(default, deserialize_with = "present_data")]
        data: bool,
    }
    fn present_data<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
        IgnoredAny::deserialize(deserializer)?;
        Ok(true)
    }
    let header: Header = serde_json::from_slice(bytes).map_err(|_| invalid_request())?;
    if header.v != u64::from(PROTOCOL_VERSION) {
        return Err(ApiError {
            code: ErrorCode::UnsupportedVersion,
            message: "Unsupported control protocol version.".into(),
        });
    }
    if !header.data {
        return Err(invalid_request());
    }

    serde_json::from_slice(bytes).map_err(|_| invalid_request())
}

fn invalid_request() -> ApiError {
    ApiError {
        code: ErrorCode::InvalidRequest,
        message: "Control payload does not match the expected contract.".into(),
    }
}
