// Intent citation: .kiro/specs/lan-transport-adapter/design.md — Frame Codec
// Frame encoding/decoding with 4-byte length-prefixed MessagePack serialization.

use super::{LanError, WireMessage};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Encode a WireMessage into a length-prefixed frame.
/// Returns a Vec<u8> with 4-byte big-endian length header followed by MessagePack payload.
pub fn encode_frame(message: &WireMessage) -> Result<Vec<u8>, LanError> {
    let payload = rmp_serde::to_vec(message).map_err(|e| LanError::SerializationError {
        reason: e.to_string(),
    })?;
    let len = payload.len() as u32;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Decode a MessagePack payload (without length header) into a WireMessage.
pub fn decode_frame(data: &[u8]) -> Result<WireMessage, LanError> {
    rmp_serde::from_slice(data).map_err(|e| LanError::DeserializationError {
        reason: e.to_string(),
    })
}

/// Write a length-prefixed frame to a TcpStream.
/// Writes 4-byte big-endian length header followed by the payload bytes.
pub async fn write_frame(stream: &mut TcpStream, data: &[u8]) -> Result<(), LanError> {
    let len = data.len() as u32;
    stream
        .write_all(&len.to_be_bytes())
        .await
        .map_err(|e| LanError::ConnectionFailed {
            peer: uuid::Uuid::nil(),
            reason: format!("write header failed: {}", e),
        })?;
    stream
        .write_all(data)
        .await
        .map_err(|e| LanError::ConnectionFailed {
            peer: uuid::Uuid::nil(),
            reason: format!("write payload failed: {}", e),
        })?;
    Ok(())
}

/// Read a length-prefixed frame from a TcpStream with size validation and timeout.
/// Returns the payload bytes (without the length header).
pub async fn read_frame(
    stream: &mut TcpStream,
    max_size: u64,
    timeout: Duration,
) -> Result<Vec<u8>, LanError> {
    // Read 4-byte length header with timeout
    let mut len_buf = [0u8; 4];
    let read_header = tokio::time::timeout(timeout, stream.read_exact(&mut len_buf)).await;

    match read_header {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            return Err(LanError::ConnectionFailed {
                peer: uuid::Uuid::nil(),
                reason: format!("read header failed: {}", e),
            });
        }
        Err(_) => {
            return Err(LanError::FrameTimeout);
        }
    }

    let len = u32::from_be_bytes(len_buf) as u64;

    // Validate size
    if len > max_size {
        return Err(LanError::FrameTooLarge {
            size: len,
            max: max_size,
        });
    }

    // Read payload with timeout
    let mut payload = vec![0u8; len as usize];
    let read_payload = tokio::time::timeout(timeout, stream.read_exact(&mut payload)).await;

    match read_payload {
        Ok(Ok(_)) => Ok(payload),
        Ok(Err(e)) => Err(LanError::ConnectionFailed {
            peer: uuid::Uuid::nil(),
            reason: format!("read payload failed: {}", e),
        }),
        Err(_) => Err(LanError::FrameTimeout),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::trait_def::{MessagePriority, RequestType, TransportMessage};

    #[test]
    fn test_encode_decode_roundtrip_data() {
        let msg = WireMessage::Data(TransportMessage::new(
            vec![1, 2, 3, 4, 5],
            MessagePriority::Normal,
            RequestType::InferenceRequest,
        ));

        let frame = encode_frame(&msg).unwrap();
        // First 4 bytes are the length header
        let payload = &frame[4..];
        let decoded = decode_frame(payload).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_encode_decode_roundtrip_ping() {
        let msg = WireMessage::Ping {
            timestamp_ns: 1234567890,
        };
        let frame = encode_frame(&msg).unwrap();
        let decoded = decode_frame(&frame[4..]).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_encode_decode_roundtrip_pong() {
        let msg = WireMessage::Pong {
            timestamp_ns: 9876543210,
        };
        let frame = encode_frame(&msg).unwrap();
        let decoded = decode_frame(&frame[4..]).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_encode_decode_roundtrip_goodbye() {
        let msg = WireMessage::Goodbye;
        let frame = encode_frame(&msg).unwrap();
        let decoded = decode_frame(&frame[4..]).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_encode_frame_length_header() {
        let msg = WireMessage::Ping { timestamp_ns: 42 };
        let frame = encode_frame(&msg).unwrap();

        // Extract length from header
        let len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        assert_eq!(len, frame.len() - 4);
    }

    #[test]
    fn test_decode_invalid_data() {
        let garbage = vec![0xFF, 0xFE, 0xFD, 0xFC];
        let result = decode_frame(&garbage);
        assert!(result.is_err());
        match result {
            Err(LanError::DeserializationError { .. }) => {}
            _ => panic!("Expected DeserializationError"),
        }
    }
}
