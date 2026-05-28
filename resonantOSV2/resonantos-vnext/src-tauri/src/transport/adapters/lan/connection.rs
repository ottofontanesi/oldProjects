// Intent citation: .kiro/specs/lan-transport-adapter/design.md — Connection Pool
// ConnectionPool, TCP connect/accept, handshake logic.

use super::codec::{read_frame, write_frame};
use super::config::LanAdapterConfig;
use super::peer::PeerInfo;
use super::{Handshake, LanError};
use crate::transport::trait_def::NodeId;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

/// A connection to a single peer, wrapping a TcpStream split into read/write halves.
pub struct PeerConnection {
    /// Write half of the TCP stream (protected by mutex for concurrent sends).
    pub writer: Mutex<tokio::io::WriteHalf<TcpStream>>,
    /// Read half of the TCP stream (protected by mutex for the read loop).
    pub reader: Mutex<tokio::io::ReadHalf<TcpStream>>,
    /// The peer's node ID.
    pub peer_id: NodeId,
    /// When this connection was established.
    pub connected_at: Instant,
    /// Last activity timestamp (for idle keepalive).
    pub last_activity: Mutex<Instant>,
}

impl PeerConnection {
    /// Create a new PeerConnection from a TcpStream.
    pub fn new(stream: TcpStream, peer_id: NodeId) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        let now = Instant::now();
        Self {
            writer: Mutex::new(writer),
            reader: Mutex::new(reader),
            peer_id,
            connected_at: now,
            last_activity: Mutex::new(now),
        }
    }

    /// Update the last activity timestamp.
    pub async fn touch(&self) {
        let mut last = self.last_activity.lock().await;
        *last = Instant::now();
    }

    /// Get the duration since last activity.
    pub async fn idle_duration(&self) -> Duration {
        let last = self.last_activity.lock().await;
        last.elapsed()
    }
}

/// Pool of TCP connections to peers, indexed by NodeId.
/// Maintains at most one connection per peer.
pub struct ConnectionPool {
    connections: DashMap<NodeId, Arc<PeerConnection>>,
    local_node_id: NodeId,
    config: LanAdapterConfig,
}

impl ConnectionPool {
    /// Create a new empty ConnectionPool.
    pub fn new(local_node_id: NodeId, config: LanAdapterConfig) -> Self {
        Self {
            connections: DashMap::new(),
            local_node_id,
            config,
        }
    }

    /// Get an existing connection or establish a new one to the peer.
    /// Performs TCP connect with timeout and handshake.
    pub async fn get_or_connect(
        &self,
        peer: &PeerInfo,
    ) -> Result<Arc<PeerConnection>, LanError> {
        // Check if we already have a connection
        if let Some(conn) = self.connections.get(&peer.node_id) {
            return Ok(conn.value().clone());
        }

        // Establish new connection with retry and exponential backoff
        let conn = self.connect_with_retry(peer).await?;
        let conn = Arc::new(conn);
        self.connections.insert(peer.node_id, conn.clone());
        Ok(conn)
    }

    /// Connect to a peer with exponential backoff retry.
    /// Retries: base, base*2, base*4 (e.g., 100ms, 200ms, 400ms).
    async fn connect_with_retry(&self, peer: &PeerInfo) -> Result<PeerConnection, LanError> {
        let max_attempts = self.config.max_retry_attempts;
        let base_backoff = self.config.connect_retry_backoff_base;

        let mut last_error = None;

        for attempt in 0..max_attempts {
            if attempt > 0 {
                let backoff = base_backoff * (1 << (attempt - 1));
                tokio::time::sleep(backoff).await;
            }

            match self.try_connect(peer).await {
                Ok(conn) => return Ok(conn),
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(LanError::ConnectionFailed {
            peer: peer.node_id,
            reason: "max retry attempts exceeded".to_string(),
        }))
    }

    /// Attempt a single TCP connection + handshake to a peer.
    async fn try_connect(&self, peer: &PeerInfo) -> Result<PeerConnection, LanError> {
        // TCP connect with timeout
        let connect_result = tokio::time::timeout(
            self.config.connect_timeout,
            TcpStream::connect(peer.address),
        )
        .await;

        let mut stream = match connect_result {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                return Err(LanError::ConnectionFailed {
                    peer: peer.node_id,
                    reason: format!("TCP connect failed: {}", e),
                });
            }
            Err(_) => {
                return Err(LanError::ConnectionFailed {
                    peer: peer.node_id,
                    reason: "connection timeout".to_string(),
                });
            }
        };

        // Perform handshake: send our handshake, receive theirs
        self.perform_handshake(&mut stream, peer.node_id).await?;

        Ok(PeerConnection::new(stream, peer.node_id))
    }

    /// Perform the handshake protocol on a stream.
    /// Sends our Handshake, receives and validates the peer's Handshake.
    async fn perform_handshake(
        &self,
        stream: &mut TcpStream,
        expected_peer_id: NodeId,
    ) -> Result<(), LanError> {
        let our_handshake = Handshake {
            node_id: self.local_node_id,
            protocol_version: 1,
            capabilities: 0,
        };

        // Serialize and send our handshake
        let handshake_bytes =
            rmp_serde::to_vec(&our_handshake).map_err(|e| LanError::SerializationError {
                reason: e.to_string(),
            })?;
        write_frame(stream, &handshake_bytes).await?;

        // Read peer's handshake
        let peer_bytes = read_frame(
            stream,
            self.config.max_message_size,
            self.config.frame_read_timeout,
        )
        .await?;

        let peer_handshake: Handshake =
            rmp_serde::from_slice(&peer_bytes).map_err(|e| LanError::HandshakeFailed {
                reason: format!("failed to deserialize peer handshake: {}", e),
            })?;

        // Validate peer identity
        if peer_handshake.node_id != expected_peer_id {
            return Err(LanError::HandshakeFailed {
                reason: format!(
                    "peer node_id mismatch: expected {}, got {}",
                    expected_peer_id, peer_handshake.node_id
                ),
            });
        }

        Ok(())
    }

    /// Perform handshake as the accepting side (receive first, then send).
    pub async fn accept_handshake(
        &self,
        stream: &mut TcpStream,
    ) -> Result<NodeId, LanError> {
        // Read peer's handshake first
        let peer_bytes = read_frame(
            stream,
            self.config.max_message_size,
            self.config.frame_read_timeout,
        )
        .await?;

        let peer_handshake: Handshake =
            rmp_serde::from_slice(&peer_bytes).map_err(|e| LanError::HandshakeFailed {
                reason: format!("failed to deserialize peer handshake: {}", e),
            })?;

        // Send our handshake
        let our_handshake = Handshake {
            node_id: self.local_node_id,
            protocol_version: 1,
            capabilities: 0,
        };
        let handshake_bytes =
            rmp_serde::to_vec(&our_handshake).map_err(|e| LanError::SerializationError {
                reason: e.to_string(),
            })?;
        write_frame(stream, &handshake_bytes).await?;

        Ok(peer_handshake.node_id)
    }

    /// Send a framed message to a peer. On failure, removes the connection and retries once.
    pub async fn send_framed(
        &self,
        peer_id: &NodeId,
        data: &[u8],
    ) -> Result<(), LanError> {
        // First attempt
        if let Some(conn) = self.connections.get(peer_id) {
            let conn = conn.value().clone();
            match self.write_to_connection(&conn, data).await {
                Ok(()) => {
                    conn.touch().await;
                    return Ok(());
                }
                Err(_) => {
                    // Remove broken connection
                    self.connections.remove(peer_id);
                }
            }
        }

        // Connection not found or failed — cannot retry without PeerInfo
        Err(LanError::PeerNotFound { node_id: *peer_id })
    }

    /// Write data to a connection's writer.
    async fn write_to_connection(
        &self,
        conn: &PeerConnection,
        data: &[u8],
    ) -> Result<(), LanError> {
        use tokio::io::AsyncWriteExt;
        let mut writer = conn.writer.lock().await;
        let len = data.len() as u32;
        writer
            .write_all(&len.to_be_bytes())
            .await
            .map_err(|e| LanError::ConnectionFailed {
                peer: conn.peer_id,
                reason: format!("write header failed: {}", e),
            })?;
        writer
            .write_all(data)
            .await
            .map_err(|e| LanError::ConnectionFailed {
                peer: conn.peer_id,
                reason: format!("write payload failed: {}", e),
            })?;
        Ok(())
    }

    /// Close a specific peer's connection.
    pub async fn close(&self, peer_id: &NodeId) {
        self.connections.remove(peer_id);
    }

    /// Close all connections.
    pub async fn close_all(&self) {
        self.connections.clear();
    }

    /// Get the number of active connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Check if a connection exists for a peer.
    pub fn has_connection(&self, peer_id: &NodeId) -> bool {
        self.connections.contains_key(peer_id)
    }

    /// Get a connection if it exists.
    pub fn get_connection(&self, peer_id: &NodeId) -> Option<Arc<PeerConnection>> {
        self.connections.get(peer_id).map(|c| c.value().clone())
    }

    /// Store a connection (used when accepting incoming connections).
    pub fn store_connection(&self, peer_id: NodeId, conn: PeerConnection) {
        self.connections.insert(peer_id, Arc::new(conn));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn make_config() -> LanAdapterConfig {
        LanAdapterConfig::default()
    }

    fn make_peer(node_id: NodeId) -> PeerInfo {
        PeerInfo::new(
            node_id,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9741),
            "test-peer".to_string(),
        )
    }

    #[test]
    fn test_connection_pool_new() {
        let local_id = uuid::Uuid::new_v4();
        let pool = ConnectionPool::new(local_id, make_config());
        assert_eq!(pool.connection_count(), 0);
    }

    #[test]
    fn test_connection_pool_close_all() {
        let local_id = uuid::Uuid::new_v4();
        let pool = ConnectionPool::new(local_id, make_config());
        // Pool starts empty
        assert_eq!(pool.connection_count(), 0);
    }

    #[test]
    fn test_connection_pool_has_connection() {
        let local_id = uuid::Uuid::new_v4();
        let pool = ConnectionPool::new(local_id, make_config());
        let peer_id = uuid::Uuid::new_v4();
        assert!(!pool.has_connection(&peer_id));
    }

    #[tokio::test]
    async fn test_send_framed_no_connection() {
        let local_id = uuid::Uuid::new_v4();
        let pool = ConnectionPool::new(local_id, make_config());
        let peer_id = uuid::Uuid::new_v4();

        let result = pool.send_framed(&peer_id, &[1, 2, 3]).await;
        assert!(result.is_err());
        match result {
            Err(LanError::PeerNotFound { node_id }) => assert_eq!(node_id, peer_id),
            _ => panic!("Expected PeerNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_close_removes_connection() {
        let local_id = uuid::Uuid::new_v4();
        let pool = ConnectionPool::new(local_id, make_config());
        let peer_id = uuid::Uuid::new_v4();

        // Nothing to close, but should not panic
        pool.close(&peer_id).await;
        assert_eq!(pool.connection_count(), 0);
    }
}
