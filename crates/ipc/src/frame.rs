//! Four-byte big-endian length-prefixed JSON control frames.

use std::{
    io::{self, Read, Write},
    thread,
    time::{Duration, Instant},
};

use muxyard_contracts::MAX_CONTROL_BYTES;

use crate::IpcError;

/// Maximum payload bytes in one control frame.
pub const MAX_FRAME_BYTES: usize = MAX_CONTROL_BYTES;

/// Read one frame. `Ok(None)` means the peer closed cleanly before a frame.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>, IpcError> {
    read_frame_inner(reader, None)
}

pub(crate) fn read_frame_until<R: Read>(
    reader: &mut R,
    deadline: Instant,
) -> Result<Option<Vec<u8>>, IpcError> {
    read_frame_inner(reader, Some(deadline))
}

fn read_frame_inner<R: Read>(
    reader: &mut R,
    deadline: Option<Instant>,
) -> Result<Option<Vec<u8>>, IpcError> {
    let mut header = [0_u8; 4];
    let mut filled = 0;
    while filled < header.len() {
        match reader.read(&mut header[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(IpcError::TruncatedFrame),
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if retryable_timeout(&error) => {
                wait_for_deadline(deadline, &error)?;
            }
            Err(error) => return Err(IpcError::Io(error)),
        }
    }
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 {
        return Err(IpcError::InvalidFrame);
    }
    if length > MAX_FRAME_BYTES {
        return Err(IpcError::FrameTooLarge);
    }
    let mut payload = vec![0_u8; length];
    let mut filled = 0;
    while filled < payload.len() {
        match reader.read(&mut payload[filled..]) {
            Ok(0) => return Err(IpcError::TruncatedFrame),
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if retryable_timeout(&error) => {
                wait_for_deadline(deadline, &error)?;
            }
            Err(error) => return Err(IpcError::Io(error)),
        }
    }
    Ok(Some(payload))
}

/// Write one frame and flush it.
pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> Result<(), IpcError> {
    write_frame_inner(writer, payload, None)
}

pub(crate) fn write_frame_until<W: Write>(
    writer: &mut W,
    payload: &[u8],
    deadline: Instant,
) -> Result<(), IpcError> {
    write_frame_inner(writer, payload, Some(deadline))
}

fn write_frame_inner<W: Write>(
    writer: &mut W,
    payload: &[u8],
    deadline: Option<Instant>,
) -> Result<(), IpcError> {
    if payload.is_empty() {
        return Err(IpcError::InvalidFrame);
    }
    if payload.len() > MAX_FRAME_BYTES {
        return Err(IpcError::FrameTooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| IpcError::FrameTooLarge)?;
    write_all_until(writer, &length.to_be_bytes(), deadline)?;
    write_all_until(writer, payload, deadline)?;
    writer.flush()?;
    Ok(())
}

fn write_all_until<W: Write>(
    writer: &mut W,
    mut bytes: &[u8],
    deadline: Option<Instant>,
) -> Result<(), IpcError> {
    while !bytes.is_empty() {
        match writer.write(bytes) {
            Ok(0) => return Err(IpcError::Io(io::ErrorKind::WriteZero.into())),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if retryable_timeout(&error) => {
                wait_for_deadline(deadline, &error)?;
            }
            Err(error) => return Err(IpcError::Io(error)),
        }
    }
    Ok(())
}

fn retryable_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

fn wait_for_deadline(deadline: Option<Instant>, error: &io::Error) -> Result<(), IpcError> {
    let Some(deadline) = deadline else {
        return Err(IpcError::Io(io::Error::new(
            error.kind(),
            error.to_string(),
        )));
    };
    if Instant::now() >= deadline {
        return Err(IpcError::Timeout);
    }
    thread::sleep(Duration::from_millis(5));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn roundtrips_a_frame() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, b"{\"v\":1}").unwrap();
        let mut cursor = Cursor::new(buffer);
        assert_eq!(
            read_frame(&mut cursor).unwrap(),
            Some(b"{\"v\":1}".to_vec())
        );
        assert_eq!(read_frame(&mut cursor).unwrap(), None);
    }

    #[test]
    fn rejects_zero_overlong_and_truncated_frames() {
        let mut zero = Cursor::new(0_u32.to_be_bytes().to_vec());
        assert!(matches!(read_frame(&mut zero), Err(IpcError::InvalidFrame)));

        let mut overlong = Cursor::new(
            u32::try_from(MAX_FRAME_BYTES + 1)
                .unwrap()
                .to_be_bytes()
                .to_vec(),
        );
        assert!(matches!(
            read_frame(&mut overlong),
            Err(IpcError::FrameTooLarge)
        ));

        let mut truncated = Cursor::new(vec![0, 0, 0, 4, 1, 2]);
        assert!(matches!(
            read_frame(&mut truncated),
            Err(IpcError::TruncatedFrame)
        ));

        let mut partial_header = Cursor::new(vec![0, 0]);
        assert!(matches!(
            read_frame(&mut partial_header),
            Err(IpcError::TruncatedFrame)
        ));
    }

    #[test]
    fn refuses_to_write_empty_or_overlong_payloads() {
        let mut buffer = Vec::new();
        assert!(matches!(
            write_frame(&mut buffer, b""),
            Err(IpcError::InvalidFrame)
        ));
        assert!(matches!(
            write_frame(&mut buffer, &vec![0_u8; MAX_FRAME_BYTES + 1]),
            Err(IpcError::FrameTooLarge)
        ));
    }

    #[test]
    fn read_deadline_stops_a_would_block_loop() {
        struct WouldBlock;
        impl Read for WouldBlock {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::WouldBlock.into())
            }
        }

        let mut reader = WouldBlock;
        let deadline = Instant::now() + Duration::from_millis(20);
        assert!(matches!(
            read_frame_until(&mut reader, deadline),
            Err(IpcError::Timeout)
        ));
    }

    #[test]
    fn write_deadline_stops_a_would_block_loop() {
        struct WouldBlock;
        impl Write for WouldBlock {
            fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::WouldBlock.into())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut writer = WouldBlock;
        let deadline = Instant::now() + Duration::from_millis(20);
        assert!(matches!(
            write_frame_until(&mut writer, b"{}", deadline),
            Err(IpcError::Timeout)
        ));
    }
}
