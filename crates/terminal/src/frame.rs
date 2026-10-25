//! Terminal binary frame codec.

use std::sync::Arc;

use thiserror::Error;
use uuid::Uuid;

/// Version of the terminal binary frame protocol.
pub const TERMINAL_PROTOCOL_VERSION: u8 = 1;

/// Maximum terminal payload bytes in one frame.
pub const MAX_TERMINAL_PAYLOAD: usize = 64 * 1024;

/// Fixed header bytes before the terminal payload.
pub const TERMINAL_HEADER_BYTES: usize = 30;

/// Maximum encoded terminal frame bytes, including its header.
pub const MAX_TERMINAL_FRAME_BYTES: usize = TERMINAL_HEADER_BYTES + MAX_TERMINAL_PAYLOAD;

/// Terminal frame kinds understood by the local and remote transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TerminalFrameKind {
    /// PTY output bytes.
    Output = 1,
    /// PTY input bytes.
    Input = 2,
    /// Terminal resize dimensions.
    Resize = 3,
    /// Stream end marker.
    End = 4,
    /// Credit/window update for flow control.
    FlowControl = 5,
    /// Acknowledgment of received bytes.
    Ack = 6,
}

impl TerminalFrameKind {
    /// Wire representation.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for TerminalFrameKind {
    type Error = TerminalFrameError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Output),
            2 => Ok(Self::Input),
            3 => Ok(Self::Resize),
            4 => Ok(Self::End),
            5 => Ok(Self::FlowControl),
            6 => Ok(Self::Ack),
            other => Err(TerminalFrameError::UnknownKind(other)),
        }
    }
}

/// One validated terminal frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalFrame {
    stream_id: Uuid,
    kind: TerminalFrameKind,
    sequence: u64,
    payload: Arc<[u8]>,
}

impl TerminalFrame {
    /// Build a frame, rejecting nil stream IDs and oversized payloads.
    pub fn new(
        stream_id: Uuid,
        kind: TerminalFrameKind,
        sequence: u64,
        payload: Vec<u8>,
    ) -> Result<Self, TerminalFrameError> {
        if stream_id.is_nil() {
            return Err(TerminalFrameError::NilStream);
        }
        if payload.len() > MAX_TERMINAL_PAYLOAD {
            return Err(TerminalFrameError::PayloadTooLarge(payload.len()));
        }
        Ok(Self {
            stream_id,
            kind,
            sequence,
            payload: Arc::from(payload),
        })
    }

    /// Build an output frame.
    pub fn output(
        stream_id: Uuid,
        sequence: u64,
        payload: Vec<u8>,
    ) -> Result<Self, TerminalFrameError> {
        Self::new(stream_id, TerminalFrameKind::Output, sequence, payload)
    }

    /// Build a flow-control frame carrying a little-endian credit count.
    pub fn flow_control(
        stream_id: Uuid,
        sequence: u64,
        credits: u32,
    ) -> Result<Self, TerminalFrameError> {
        Self::new(
            stream_id,
            TerminalFrameKind::FlowControl,
            sequence,
            credits.to_le_bytes().to_vec(),
        )
    }

    /// Stream identifier.
    pub fn stream_id(&self) -> Uuid {
        self.stream_id
    }

    /// Frame kind.
    pub fn kind(&self) -> TerminalFrameKind {
        self.kind
    }

    /// Monotonic per-stream sequence number.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Payload bytes.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Flow-control credits when this is a valid flow-control frame.
    pub fn flow_control_credits(&self) -> Option<u32> {
        if self.kind != TerminalFrameKind::FlowControl || self.payload.len() != 4 {
            return None;
        }
        let mut bytes = [0_u8; 4];
        bytes.copy_from_slice(&self.payload);
        Some(u32::from_le_bytes(bytes))
    }

    /// Encoded frame length.
    pub fn encoded_len(&self) -> usize {
        TERMINAL_HEADER_BYTES + self.payload.len()
    }

    /// Encode the frame without an outer transport length prefix.
    pub fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(self.encoded_len());
        encoded.push(TERMINAL_PROTOCOL_VERSION);
        encoded.push(self.kind.as_u8());
        encoded.extend_from_slice(self.stream_id.as_bytes());
        encoded.extend_from_slice(&self.sequence.to_le_bytes());
        encoded.extend_from_slice(
            &u32::try_from(self.payload.len())
                .expect("terminal payload is bounded below u32::MAX")
                .to_le_bytes(),
        );
        encoded.extend_from_slice(&self.payload);
        encoded
    }

    /// Decode one exact terminal frame payload.
    pub fn decode(encoded: &[u8]) -> Result<Self, TerminalFrameError> {
        if encoded.len() < TERMINAL_HEADER_BYTES {
            return Err(TerminalFrameError::TooShort(encoded.len()));
        }
        let version = encoded[0];
        if version != TERMINAL_PROTOCOL_VERSION {
            return Err(TerminalFrameError::UnsupportedVersion(version));
        }
        let kind = TerminalFrameKind::try_from(encoded[1])?;
        let stream_id =
            Uuid::from_slice(&encoded[2..18]).map_err(|_| TerminalFrameError::InvalidStreamId)?;
        if stream_id.is_nil() {
            return Err(TerminalFrameError::NilStream);
        }
        let sequence = u64::from_le_bytes(
            encoded[18..26]
                .try_into()
                .map_err(|_| TerminalFrameError::TooShort(encoded.len()))?,
        );
        let payload_len = u32::from_le_bytes(
            encoded[26..30]
                .try_into()
                .map_err(|_| TerminalFrameError::TooShort(encoded.len()))?,
        ) as usize;
        if payload_len > MAX_TERMINAL_PAYLOAD {
            return Err(TerminalFrameError::PayloadTooLarge(payload_len));
        }
        let expected = TERMINAL_HEADER_BYTES + payload_len;
        if encoded.len() != expected {
            return Err(TerminalFrameError::LengthMismatch {
                expected,
                actual: encoded.len(),
            });
        }
        let payload = encoded[TERMINAL_HEADER_BYTES..].to_vec();
        Self::new(stream_id, kind, sequence, payload)
    }
}

/// Monotonic sequence validator for one terminal stream.
#[derive(Debug, Clone, Copy)]
pub struct TerminalSequence {
    next: u64,
}

impl Default for TerminalSequence {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl TerminalSequence {
    /// Start expecting sequence 1.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start expecting an explicit next sequence.
    pub fn starting_at(next: u64) -> Self {
        Self { next }
    }

    /// Expected next sequence.
    pub fn expected(&self) -> u64 {
        self.next
    }

    /// Accept the next sequence or report an out-of-order frame.
    pub fn accept(&mut self, sequence: u64) -> Result<(), TerminalFrameError> {
        if sequence != self.next {
            return Err(TerminalFrameError::UnexpectedSequence {
                expected: self.next,
                actual: sequence,
            });
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or(TerminalFrameError::SequenceOverflow)?;
        Ok(())
    }
}

/// Terminal framing failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TerminalFrameError {
    /// Header is incomplete.
    #[error("terminal frame is shorter than its header: {0} bytes")]
    TooShort(usize),
    /// Unknown protocol version.
    #[error("unsupported terminal frame version {0}")]
    UnsupportedVersion(u8),
    /// Unknown frame kind.
    #[error("unknown terminal frame kind {0}")]
    UnknownKind(u8),
    /// UUID bytes were malformed.
    #[error("terminal stream identifier is malformed")]
    InvalidStreamId,
    /// Nil UUID is not a valid stream.
    #[error("terminal stream identifier is nil")]
    NilStream,
    /// Payload exceeds the 64 KiB frame limit.
    #[error("terminal payload exceeds 64 KiB: {0} bytes")]
    PayloadTooLarge(usize),
    /// Declared and actual lengths differ.
    #[error("terminal frame length mismatch: expected {expected}, got {actual}")]
    LengthMismatch {
        /// Expected total length.
        expected: usize,
        /// Actual input length.
        actual: usize,
    },
    /// A frame arrived out of order.
    #[error("terminal sequence out of order: expected {expected}, got {actual}")]
    UnexpectedSequence {
        /// Expected sequence.
        expected: u64,
        /// Received sequence.
        actual: u64,
    },
    /// Sequence counter exhausted.
    #[error("terminal sequence counter overflowed")]
    SequenceOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream() -> Uuid {
        Uuid::new_v4()
    }

    #[test]
    fn roundtrips_frames_and_flow_control() {
        let frame = TerminalFrame::output(stream(), 1, b"hello".to_vec()).unwrap();
        let encoded = frame.encode();
        assert_eq!(encoded.len(), TERMINAL_HEADER_BYTES + 5);
        assert_eq!(TerminalFrame::decode(&encoded).unwrap(), frame);

        let credits = TerminalFrame::flow_control(stream(), 2, 4096).unwrap();
        assert_eq!(credits.flow_control_credits(), Some(4096));
        assert_eq!(TerminalFrame::decode(&credits.encode()).unwrap(), credits);
    }

    #[test]
    fn rejects_invalid_length_kind_version_and_stream() {
        let stream = stream();
        let frame = TerminalFrame::output(stream, 1, vec![1, 2, 3]).unwrap();
        let mut encoded = frame.encode();

        encoded[0] = 2;
        assert!(matches!(
            TerminalFrame::decode(&encoded),
            Err(TerminalFrameError::UnsupportedVersion(2))
        ));

        let mut encoded = frame.encode();
        encoded[1] = 99;
        assert!(matches!(
            TerminalFrame::decode(&encoded),
            Err(TerminalFrameError::UnknownKind(99))
        ));

        let mut encoded = frame.encode();
        encoded[26..30].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            TerminalFrame::decode(&encoded),
            Err(TerminalFrameError::PayloadTooLarge(_))
        ));

        let nil = Uuid::nil();
        assert!(matches!(
            TerminalFrame::output(nil, 1, vec![]),
            Err(TerminalFrameError::NilStream)
        ));

        let mut nil_encoded = frame.encode();
        nil_encoded[2..18].fill(0);
        assert!(matches!(
            TerminalFrame::decode(&nil_encoded),
            Err(TerminalFrameError::NilStream)
        ));

        let mut trailing = frame.encode();
        trailing.push(0);
        assert!(matches!(
            TerminalFrame::decode(&trailing),
            Err(TerminalFrameError::LengthMismatch { .. })
        ));

        assert!(matches!(
            TerminalFrame::decode(&frame.encode()[..10]),
            Err(TerminalFrameError::TooShort(10))
        ));
    }

    #[test]
    fn sequence_rejects_out_of_order_frames() {
        let mut sequence = TerminalSequence::new();
        sequence.accept(1).unwrap();
        sequence.accept(2).unwrap();
        assert!(matches!(
            sequence.accept(4),
            Err(TerminalFrameError::UnexpectedSequence {
                expected: 3,
                actual: 4
            })
        ));
        sequence.accept(3).unwrap();
    }

    #[test]
    fn malformed_frames_never_panic() {
        let mut state = 0x1234_5678_9abc_def0_u64;
        for length in 0..256 {
            let mut bytes = vec![0_u8; length];
            for byte in &mut bytes {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                *byte = state as u8;
            }
            let _ = TerminalFrame::decode(&bytes);
        }
    }
}
