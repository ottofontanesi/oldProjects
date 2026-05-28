// Intent citation: .kiro/specs/lan-transport-adapter/design.md — mDNS Discovery
// mDNS advertisement and browsing logic using mdns-sd crate.

use super::{DiscoveredPeerEvent, LanError};
use super::config::LanAdapterConfig;
use crate::transport::trait_def::NodeId;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

/// Events emitted by the mDNS discovery subsystem.
#[derive(Debug, Clone)]
pub enum MdnsEvent {
    /// A new peer was discovered.
    PeerDiscovered(DiscoveredPeerEvent),
    /// A peer's mDNS record was removed.
    PeerRemoved(NodeId),
}

/// mDNS discovery subsystem for advertising and browsing LAN peers.
pub struct MdnsDiscovery {
    config: LanAdapterConfig,
    local_node_id: NodeId,
    daemon: Mutex<Option<ServiceDaemon>>,
    /// Channel for emitting discovery events.
    event_tx: mpsc::Sender<MdnsEvent>,
    /// Whether the discovery is running.
    is_running: Arc<std::sync::atomic::AtomicBool>,
    /// Handle to the browse task.
    browse_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl MdnsDiscovery {
    /// Create a new MdnsDiscovery instance.
    pub fn new(
        config: LanAdapterConfig,
        local_node_id: NodeId,
        event_tx: mpsc::Sender<MdnsEvent>,
    ) -> Self {
        Self {
            config,
            local_node_id,
            daemon: Mutex::new(None),
            event_tx,
            is_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            browse_handle: Mutex::new(None),
        }
    }

    /// Start mDNS advertisement and browsing.
    /// Registers the local service and begins browsing for peers.
    pub async fn start(&self, port: u16, hostname: &str) -> Result<(), LanError> {
        let daemon = self.create_daemon_with_retry().await?;

        // Register our service
        self.register_service(&daemon, port, hostname).await?;

        // Start browsing
        let browse_receiver = daemon
            .browse(&self.config.mdns_service_type)
            .map_err(|e| LanError::MdnsBrowseFailed {
                reason: e.to_string(),
            })?;

        // Store daemon
        {
            let mut d = self.daemon.lock().await;
            *d = Some(daemon);
        }

        self.is_running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        // Spawn browse event processing task
        let event_tx = self.event_tx.clone();
        let local_node_id = self.local_node_id;
        let is_running = self.is_running.clone();

        let handle = tokio::task::spawn(async move {
            Self::browse_loop(browse_receiver, event_tx, local_node_id, is_running).await;
        });

        {
            let mut bh = self.browse_handle.lock().await;
            *bh = Some(handle);
        }

        Ok(())
    }

    /// Create the mDNS daemon with retry and exponential backoff.
    async fn create_daemon_with_retry(&self) -> Result<ServiceDaemon, LanError> {
        let max_attempts = self.config.max_retry_attempts;
        let base_backoff = self.config.mdns_retry_backoff_base;

        let mut last_error = None;

        for attempt in 0..max_attempts {
            if attempt > 0 {
                let backoff = base_backoff * (1 << (attempt - 1));
                tokio::time::sleep(backoff).await;
            }

            match ServiceDaemon::new() {
                Ok(daemon) => return Ok(daemon),
                Err(e) => {
                    last_error = Some(e.to_string());
                }
            }
        }

        Err(LanError::MdnsRegistrationFailed {
            reason: last_error.unwrap_or_else(|| "unknown error".to_string()),
        })
    }

    /// Register our mDNS service with retry and exponential backoff.
    async fn register_service(
        &self,
        daemon: &ServiceDaemon,
        port: u16,
        hostname: &str,
    ) -> Result<(), LanError> {
        let max_attempts = self.config.max_retry_attempts;
        let base_backoff = self.config.mdns_retry_backoff_base;

        let service_type = &self.config.mdns_service_type;
        let _instance_name = format!("{}._resonantos._tcp.local.", hostname);

        // Build TXT properties with node_id
        let node_id_string = self.local_node_id.to_string();
        let properties = [("node_id", node_id_string.as_str())];

        let mut last_error = None;

        for attempt in 0..max_attempts {
            if attempt > 0 {
                let backoff = base_backoff * (1 << (attempt - 1));
                tokio::time::sleep(backoff).await;
            }

            // Create service info
            let service_info = match ServiceInfo::new(
                service_type,
                hostname,
                hostname,
                "",
                port,
                &properties[..],
            ) {
                Ok(info) => info,
                Err(e) => {
                    last_error = Some(e.to_string());
                    continue;
                }
            };

            match daemon.register(service_info) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    last_error = Some(e.to_string());
                }
            }
        }

        // Graceful degradation: log error but don't crash
        // After max retries, return error but caller can continue without mDNS
        Err(LanError::MdnsRegistrationFailed {
            reason: last_error.unwrap_or_else(|| "unknown error".to_string()),
        })
    }

    /// Process mDNS browse events in a loop.
    async fn browse_loop(
        receiver: mdns_sd::Receiver<ServiceEvent>,
        event_tx: mpsc::Sender<MdnsEvent>,
        local_node_id: NodeId,
        is_running: Arc<std::sync::atomic::AtomicBool>,
    ) {
        while is_running.load(std::sync::atomic::Ordering::SeqCst) {
            // Use recv_timeout to allow periodic checking of is_running
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(event) => {
                    match event {
                        ServiceEvent::ServiceResolved(info) => {
                            if let Some(peer_event) =
                                Self::handle_service_resolved(&info, local_node_id)
                            {
                                let _ = event_tx
                                    .send(MdnsEvent::PeerDiscovered(peer_event))
                                    .await;
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            if let Some(node_id) = Self::extract_node_id_from_name(&fullname) {
                                if node_id != local_node_id {
                                    let _ =
                                        event_tx.send(MdnsEvent::PeerRemoved(node_id)).await;
                                }
                            }
                        }
                        _ => {
                            // Ignore SearchStarted, ServiceFound (wait for Resolved)
                        }
                    }
                }
                Err(_) => {
                    // Timeout — check is_running and continue
                }
            }
        }
    }

    /// Handle a resolved mDNS service record.
    /// Extracts node_id from TXT, IP from addresses, port from service info.
    /// Filters self (node_id == local_node_id).
    fn handle_service_resolved(
        info: &ServiceInfo,
        local_node_id: NodeId,
    ) -> Option<DiscoveredPeerEvent> {
        // Extract node_id from TXT record
        let node_id_str = info.get_property_val_str("node_id")?;
        let node_id: NodeId = uuid::Uuid::parse_str(node_id_str).ok()?;

        // Filter self
        if node_id == local_node_id {
            return None;
        }

        // Get IP address (prefer first available)
        let ip = info.get_addresses().iter().next()?;
        let port = info.get_port();
        let address = SocketAddr::new((*ip).into(), port);

        let hostname = info.get_hostname().to_string();

        Some(DiscoveredPeerEvent {
            node_id,
            address,
            hostname,
        })
    }

    /// Try to extract a node_id from a service fullname (for removal events).
    fn extract_node_id_from_name(_fullname: &str) -> Option<NodeId> {
        // In removal events, we may not have TXT records available.
        // The fullname format is: <instance>._resonantos._tcp.local.
        // We cannot reliably extract node_id from the name alone.
        // This will be handled by the heartbeat monitor detecting the peer going offline.
        None
    }

    /// Stop mDNS advertisement and browsing.
    pub async fn stop(&self) -> Result<(), LanError> {
        self.is_running
            .store(false, std::sync::atomic::Ordering::SeqCst);

        // Abort browse task
        if let Some(handle) = self.browse_handle.lock().await.take() {
            handle.abort();
        }

        // Shutdown daemon (unregisters service)
        if let Some(daemon) = self.daemon.lock().await.take() {
            let _ = daemon.shutdown();
        }

        Ok(())
    }

    /// Check if discovery is running.
    pub fn is_running(&self) -> bool {
        self.is_running.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Parse a node_id UUID from an mDNS TXT record value.
/// Used for property testing of mDNS record parsing.
pub fn parse_node_id_from_txt(txt_value: &str) -> Option<NodeId> {
    uuid::Uuid::parse_str(txt_value).ok()
}

/// Parse a SocketAddr from IP string and port.
/// Used for property testing of mDNS record parsing.
pub fn parse_socket_addr(ip: &str, port: u16) -> Option<SocketAddr> {
    let ip_addr: std::net::IpAddr = ip.parse().ok()?;
    Some(SocketAddr::new(ip_addr, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_node_id_from_txt_valid() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let result = parse_node_id_from_txt(uuid_str);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), uuid_str);
    }

    #[test]
    fn test_parse_node_id_from_txt_invalid() {
        let result = parse_node_id_from_txt("not-a-uuid");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_socket_addr_valid() {
        let result = parse_socket_addr("192.168.1.100", 9741);
        assert!(result.is_some());
        let addr = result.unwrap();
        assert_eq!(addr.port(), 9741);
        assert_eq!(addr.ip().to_string(), "192.168.1.100");
    }

    #[test]
    fn test_parse_socket_addr_invalid() {
        let result = parse_socket_addr("not-an-ip", 9741);
        assert!(result.is_none());
    }

    #[test]
    fn test_mdns_event_variants() {
        let event = MdnsEvent::PeerDiscovered(DiscoveredPeerEvent {
            node_id: uuid::Uuid::new_v4(),
            address: "192.168.1.1:9741".parse().unwrap(),
            hostname: "test".to_string(),
        });
        assert!(matches!(event, MdnsEvent::PeerDiscovered(_)));

        let event = MdnsEvent::PeerRemoved(uuid::Uuid::new_v4());
        assert!(matches!(event, MdnsEvent::PeerRemoved(_)));
    }

    #[tokio::test]
    async fn test_mdns_discovery_not_running_initially() {
        let (tx, _rx) = mpsc::channel(16);
        let discovery = MdnsDiscovery::new(
            LanAdapterConfig::default(),
            uuid::Uuid::new_v4(),
            tx,
        );
        assert!(!discovery.is_running());
    }
}
