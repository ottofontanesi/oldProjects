// Intent citation: .kiro/specs/split-inference-protocol/design.md Section 4
// Activation Codec — serialization/deserialization of activation tensors

use super::{NodeId, SessionId};
use serde::{Deserialize, Serialize};

/// Tensor data type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum TensorDtype {
    Float16,
    BFloat16,
    Float32,
}

impl TensorDtype {
    /// Bytes per element for this dtype.
    pub fn element_size(&self) -> usize {
        match self {
            Self::Float16 | Self::BFloat16 => 2,
            Self::Float32 => 4,
        }
    }
}

/// An activation tensor to be forwarded between nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationTensor {
    /// Raw tensor data (f16/bf16/f32 bytes).
    pub data: Vec<u8>,
    /// Data type.
    pub dtype: TensorDtype,
    /// Shape dimensions (e.g., [batch, seq_len, hidden_dim]).
    pub shape: Vec<u32>,
    /// Whether data is LZ4 compressed.
    pub compressed: bool,
}

impl ActivationTensor {
    /// Compute expected uncompressed size in bytes from shape and dtype.
    pub fn expected_size_bytes(&self) -> usize {
        let elements: usize = self.shape.iter().map(|&d| d as usize).product();
        elements * self.dtype.element_size()
    }

    /// Get actual data size (may be compressed).
    pub fn data_size(&self) -> usize {
        self.data.len()
    }
}

/// A complete activation packet sent between nodes during split inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationPacket {
    pub session_id: SessionId,
    pub request_id: uuid::Uuid,
    pub token_position: u32,
    pub generation_step: u32,
    pub source_node: NodeId,
    pub target_node: NodeId,
    pub tensor: ActivationTensor,
    /// CRC32 checksum of uncompressed tensor data.
    pub checksum: u32,
    pub timestamp_ns: u64,
}

/// Errors during codec operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CodecError {
    ChecksumMismatch { expected: u32, actual: u32 },
    DecompressionFailed { reason: String },
    InvalidShape { reason: String },
    DataSizeMismatch { expected: usize, actual: usize },
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChecksumMismatch { expected, actual } => {
                write!(f, "CRC32 mismatch: expected {:#010x}, got {:#010x}", expected, actual)
            }
            Self::DecompressionFailed { reason } => write!(f, "Decompression failed: {}", reason),
            Self::InvalidShape { reason } => write!(f, "Invalid tensor shape: {}", reason),
            Self::DataSizeMismatch { expected, actual } => {
                write!(f, "Data size mismatch: expected {} bytes, got {}", expected, actual)
            }
        }
    }
}

/// Compute CRC32 checksum of data.
pub fn compute_crc32(data: &[u8]) -> u32 {
    // Simple CRC32 implementation (in production, use the `crc32fast` crate)
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Serialize an activation tensor into a packet.
/// Optionally compresses with LZ4 if `compress` is true and data > 4KB.
pub fn serialize_activation(
    tensor: &ActivationTensor,
    session_id: SessionId,
    request_id: uuid::Uuid,
    token_position: u32,
    generation_step: u32,
    source_node: NodeId,
    target_node: NodeId,
    compress: bool,
    timestamp_ns: u64,
) -> ActivationPacket {
    let checksum = compute_crc32(&tensor.data);

    let final_tensor = if compress && tensor.data.len() > 4096 {
        // In production: use lz4::compress(&tensor.data)
        // For now: skip compression (data stays as-is, marked uncompressed)
        ActivationTensor {
            data: tensor.data.clone(),
            dtype: tensor.dtype,
            shape: tensor.shape.clone(),
            compressed: false, // Would be true with real LZ4
        }
    } else {
        tensor.clone()
    };

    ActivationPacket {
        session_id,
        request_id,
        token_position,
        generation_step,
        source_node,
        target_node,
        tensor: final_tensor,
        checksum,
        timestamp_ns,
    }
}

/// Deserialize and verify an activation packet.
/// Checks CRC32 integrity. Decompresses if needed.
pub fn deserialize_activation(packet: &ActivationPacket) -> Result<ActivationTensor, CodecError> {
    let data = if packet.tensor.compressed {
        // In production: lz4::decompress(&packet.tensor.data)
        // For now: assume uncompressed
        return Err(CodecError::DecompressionFailed {
            reason: "LZ4 decompression not yet implemented".to_string(),
        });
    } else {
        &packet.tensor.data
    };

    // Verify CRC32
    let actual_checksum = compute_crc32(data);
    if actual_checksum != packet.checksum {
        return Err(CodecError::ChecksumMismatch {
            expected: packet.checksum,
            actual: actual_checksum,
        });
    }

    // Verify data size matches shape
    let expected_size = packet.tensor.expected_size_bytes();
    if data.len() != expected_size && expected_size > 0 {
        return Err(CodecError::DataSizeMismatch {
            expected: expected_size,
            actual: data.len(),
        });
    }

    Ok(ActivationTensor {
        data: data.to_vec(),
        dtype: packet.tensor.dtype,
        shape: packet.tensor.shape.clone(),
        compressed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tensor(size: usize) -> ActivationTensor {
        ActivationTensor {
            data: vec![42u8; size],
            dtype: TensorDtype::Float16,
            shape: vec![1, 1, (size / 2) as u32], // batch=1, seq=1, hidden=size/2 (f16 = 2 bytes)
            compressed: false,
        }
    }

    #[test]
    fn test_crc32_deterministic() {
        let data = vec![1, 2, 3, 4, 5];
        let crc1 = compute_crc32(&data);
        let crc2 = compute_crc32(&data);
        assert_eq!(crc1, crc2);
    }

    #[test]
    fn test_crc32_detects_corruption() {
        let data1 = vec![1, 2, 3, 4, 5];
        let data2 = vec![1, 2, 3, 4, 6]; // One bit different
        assert_ne!(compute_crc32(&data1), compute_crc32(&data2));
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let tensor = make_tensor(8192); // 8KB — typical activation size
        let session = uuid::Uuid::new_v4();
        let request = uuid::Uuid::new_v4();
        let src = uuid::Uuid::new_v4();
        let dst = uuid::Uuid::new_v4();

        let packet = serialize_activation(
            &tensor, session, request, 0, 0, src, dst, false, 1000,
        );

        let result = deserialize_activation(&packet);
        assert!(result.is_ok());

        let decoded = result.unwrap();
        assert_eq!(decoded.data, tensor.data);
        assert_eq!(decoded.dtype, tensor.dtype);
        assert_eq!(decoded.shape, tensor.shape);
    }

    #[test]
    fn test_corruption_detected() {
        let tensor = make_tensor(100);
        let session = uuid::Uuid::new_v4();
        let request = uuid::Uuid::new_v4();
        let src = uuid::Uuid::new_v4();
        let dst = uuid::Uuid::new_v4();

        let mut packet = serialize_activation(
            &tensor, session, request, 0, 0, src, dst, false, 1000,
        );

        // Corrupt one byte
        packet.tensor.data[50] ^= 0xFF;

        let result = deserialize_activation(&packet);
        assert!(matches!(result, Err(CodecError::ChecksumMismatch { .. })));
    }

    #[test]
    fn test_expected_size() {
        let tensor = ActivationTensor {
            data: vec![0u8; 8192],
            dtype: TensorDtype::Float16,
            shape: vec![1, 1, 4096], // 1 * 1 * 4096 * 2 bytes = 8192
            compressed: false,
        };
        assert_eq!(tensor.expected_size_bytes(), 8192);
    }

    #[test]
    fn test_dtype_element_size() {
        assert_eq!(TensorDtype::Float16.element_size(), 2);
        assert_eq!(TensorDtype::BFloat16.element_size(), 2);
        assert_eq!(TensorDtype::Float32.element_size(), 4);
    }

    #[test]
    fn test_empty_tensor() {
        let tensor = ActivationTensor {
            data: vec![],
            dtype: TensorDtype::Float16,
            shape: vec![0],
            compressed: false,
        };

        let session = uuid::Uuid::new_v4();
        let packet = serialize_activation(
            &tensor, session, uuid::Uuid::new_v4(), 0, 0,
            uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), false, 0,
        );

        let result = deserialize_activation(&packet);
        assert!(result.is_ok());
    }
}
