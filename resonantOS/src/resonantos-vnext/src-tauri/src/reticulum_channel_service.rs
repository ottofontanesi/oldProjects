//! Reticulum Channel Service
//!
//! Manages the Python sidecar lifecycle, JSON-RPC communication, message queue
//! persistence, delivery state tracking, and health monitoring for the Reticulum
//! mesh network channel add-on.

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

// ─── Struct Definitions ───────────────────────────────────────────────────────

/// Sidecar health state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidecarHealthState {
    Running,
    Starting,
    Offline,
    Crashed,
}

impl SidecarHealthState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Starting => "starting",
            Self::Offline => "offline",
            Self::Crashed => "crashed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "running" => Some(Self::Running),
            "starting" => Some(Self::Starting),
            "offline" => Some(Self::Offline),
            "crashed" => Some(Self::Crashed),
            _ => None,
        }
    }
}

/// Configuration for the Reticulum channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReticulumChannelConfig {
    pub sidecar_command: String,
    pub config_path: String,
    pub identity_label: String,
    pub health_check_interval_secs: u64,
    pub health_check_failures_threshold: u32,
    pub restart_initial_delay_secs: u64,
    pub restart_max_delay_secs: u64,
    pub delivery_timeout_lora_secs: u64,
    pub delivery_timeout_tcp_secs: u64,
    pub message_queue_max_age_hours: u64,
    pub message_queue_retry_secs: u64,
}

impl Default for ReticulumChannelConfig {
    fn default() -> Self {
        Self {
            sidecar_command: "python3 sidecar/main.py".to_string(),
            config_path: "~/.reticulum/config".to_string(),
            identity_label: "ResonantOS".to_string(),
            health_check_interval_secs: 30,
            health_check_failures_threshold: 3,
            restart_initial_delay_secs: 5,
            restart_max_delay_secs: 60,
            delivery_timeout_lora_secs: 300,
            delivery_timeout_tcp_secs: 30,
            message_queue_max_age_hours: 24,
            message_queue_retry_secs: 30,
        }
    }
}

/// A queued outbound message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedMessage {
    pub id: String,
    pub destination_hash: String,
    pub content: String,
    pub priority: String,
    pub conversation_message_id: String,
    pub queued_at: String,
    pub retry_count: u32,
    pub last_retry_at: Option<String>,
    pub status: String,
    pub expires_at: String,
}

/// Delivery state for an outbound message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryState {
    pub message_id: String,
    pub lxmf_message_id: String,
    pub conversation_message_id: String,
    pub destination_hash: String,
    pub status: String,
    pub sent_at: String,
    pub confirmed_at: Option<String>,
    pub timeout_at: String,
}

/// Bandwidth profile for a transport type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BandwidthProfile {
    pub transport_type: String,
    pub max_message_bytes: u32,
    pub requires_summarization: bool,
}

/// Interface status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceStatus {
    pub name: String,
    pub interface_type: String,
    pub active: bool,
    pub error: Option<String>,
}

/// Known peer record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownPeer {
    pub destination_hash: String,
    pub display_name: Option<String>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub conversation_thread_id: Option<String>,
    pub link_active: bool,
}

/// Shared state for the Reticulum channel service.
pub struct ReticulumChannelState {
    pub config: ReticulumChannelConfig,
    pub health_state: Arc<RwLock<SidecarHealthState>>,
    pub message_queue: Arc<RwLock<Vec<QueuedMessage>>>,
    pub delivery_states: Arc<RwLock<Vec<DeliveryState>>>,
    pub active_interfaces: Arc<RwLock<Vec<InterfaceStatus>>>,
    pub bandwidth_profiles: Arc<RwLock<Vec<BandwidthProfile>>>,
    pub restart_delay_secs: Arc<RwLock<u64>>,
    pub consecutive_ping_failures: Arc<RwLock<u32>>,
    pub destination_hash: Arc<RwLock<Option<String>>>,
}

impl ReticulumChannelState {
    pub fn new(config: ReticulumChannelConfig) -> Self {
        let initial_delay = config.restart_initial_delay_secs;
        Self {
            config,
            health_state: Arc::new(RwLock::new(SidecarHealthState::Offline)),
            message_queue: Arc::new(RwLock::new(Vec::new())),
            delivery_states: Arc::new(RwLock::new(Vec::new())),
            active_interfaces: Arc::new(RwLock::new(Vec::new())),
            bandwidth_profiles: Arc::new(RwLock::new(default_bandwidth_profiles())),
            restart_delay_secs: Arc::new(RwLock::new(initial_delay)),
            consecutive_ping_failures: Arc::new(RwLock::new(0)),
            destination_hash: Arc::new(RwLock::new(None)),
        }
    }
}

fn default_bandwidth_profiles() -> Vec<BandwidthProfile> {
    vec![
        BandwidthProfile { transport_type: "lora".into(), max_message_bytes: 500, requires_summarization: true },
        BandwidthProfile { transport_type: "serial".into(), max_message_bytes: 500, requires_summarization: true },
        BandwidthProfile { transport_type: "tcp".into(), max_message_bytes: 32000, requires_summarization: false },
        BandwidthProfile { transport_type: "i2p".into(), max_message_bytes: 32000, requires_summarization: false },
        BandwidthProfile { transport_type: "auto".into(), max_message_bytes: 32000, requires_summarization: false },
    ]
}


// ─── Database Initialization ──────────────────────────────────────────────────

/// Initialize the Reticulum channel database with all required tables and indexes.
pub fn initialize_reticulum_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS message_queue (
            id TEXT PRIMARY KEY,
            destination_hash TEXT NOT NULL,
            content TEXT NOT NULL,
            priority TEXT NOT NULL DEFAULT 'normal',
            conversation_message_id TEXT NOT NULL,
            queued_at TEXT NOT NULL,
            retry_count INTEGER NOT NULL DEFAULT 0,
            last_retry_at TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            expires_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS delivery_states (
            message_id TEXT PRIMARY KEY,
            lxmf_message_id TEXT NOT NULL,
            conversation_message_id TEXT NOT NULL,
            destination_hash TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            sent_at TEXT NOT NULL,
            confirmed_at TEXT,
            timeout_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS known_peers (
            destination_hash TEXT PRIMARY KEY,
            display_name TEXT,
            first_seen_at TEXT NOT NULL,
            last_seen_at TEXT NOT NULL,
            conversation_thread_id TEXT,
            link_active INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS channel_config (
            id TEXT PRIMARY KEY DEFAULT 'singleton',
            identity_label TEXT NOT NULL DEFAULT 'ResonantOS',
            enabled INTEGER NOT NULL DEFAULT 1,
            bandwidth_profiles_json TEXT NOT NULL DEFAULT '[]',
            delivery_timeout_lora_secs INTEGER NOT NULL DEFAULT 300,
            delivery_timeout_tcp_secs INTEGER NOT NULL DEFAULT 30,
            queue_max_age_hours INTEGER NOT NULL DEFAULT 24,
            queue_retry_interval_secs INTEGER NOT NULL DEFAULT 30
        );

        CREATE TABLE IF NOT EXISTS sidecar_state (
            id TEXT PRIMARY KEY DEFAULT 'singleton',
            health_state TEXT NOT NULL DEFAULT 'offline',
            destination_hash TEXT,
            active_interfaces_json TEXT NOT NULL DEFAULT '[]',
            last_ping_at TEXT,
            consecutive_ping_failures INTEGER NOT NULL DEFAULT 0,
            restart_delay_secs INTEGER NOT NULL DEFAULT 5,
            last_crash_at TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_queue_status ON message_queue(status);
        CREATE INDEX IF NOT EXISTS idx_queue_destination ON message_queue(destination_hash);
        CREATE INDEX IF NOT EXISTS idx_queue_expires ON message_queue(expires_at);
        CREATE INDEX IF NOT EXISTS idx_delivery_status ON delivery_states(status);
        CREATE INDEX IF NOT EXISTS idx_delivery_timeout ON delivery_states(timeout_at);
        CREATE INDEX IF NOT EXISTS idx_peers_last_seen ON known_peers(last_seen_at);
        ",
    )
    .map_err(|e| format!("Failed to initialize reticulum_channel DB: {e}"))
}

// ─── Message Queue CRUD ───────────────────────────────────────────────────────

/// Enqueue a message to the persistent message queue.
pub fn enqueue_message(conn: &Connection, msg: &QueuedMessage) -> Result<(), String> {
    conn.execute(
        "INSERT INTO message_queue (id, destination_hash, content, priority, conversation_message_id, queued_at, retry_count, last_retry_at, status, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            msg.id,
            msg.destination_hash,
            msg.content,
            msg.priority,
            msg.conversation_message_id,
            msg.queued_at,
            msg.retry_count,
            msg.last_retry_at,
            msg.status,
            msg.expires_at,
        ],
    )
    .map_err(|e| format!("Failed to enqueue message: {e}"))?;
    Ok(())
}

/// Dequeue the next pending message for a given destination (FIFO order).
pub fn dequeue_next_for_destination(
    conn: &Connection,
    destination_hash: &str,
) -> Result<Option<QueuedMessage>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, destination_hash, content, priority, conversation_message_id, queued_at, retry_count, last_retry_at, status, expires_at
             FROM message_queue
             WHERE destination_hash = ?1 AND status = 'pending'
             ORDER BY
               CASE WHEN priority = 'high' THEN 0 ELSE 1 END,
               queued_at ASC
             LIMIT 1",
        )
        .map_err(|e| format!("Failed to prepare dequeue query: {e}"))?;

    let result = stmt
        .query_row(params![destination_hash], |row| {
            Ok(QueuedMessage {
                id: row.get(0)?,
                destination_hash: row.get(1)?,
                content: row.get(2)?,
                priority: row.get(3)?,
                conversation_message_id: row.get(4)?,
                queued_at: row.get(5)?,
                retry_count: row.get(6)?,
                last_retry_at: row.get(7)?,
                status: row.get(8)?,
                expires_at: row.get(9)?,
            })
        })
        .ok();

    Ok(result)
}

/// Mark a queued message as sent.
pub fn mark_message_sent(conn: &Connection, message_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE message_queue SET status = 'sent' WHERE id = ?1",
        params![message_id],
    )
    .map_err(|e| format!("Failed to mark message sent: {e}"))?;
    Ok(())
}

/// Mark a queued message as expired.
pub fn mark_message_expired(conn: &Connection, message_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE message_queue SET status = 'expired' WHERE id = ?1",
        params![message_id],
    )
    .map_err(|e| format!("Failed to mark message expired: {e}"))?;
    Ok(())
}

/// Query all pending messages.
pub fn query_pending_messages(conn: &Connection) -> Result<Vec<QueuedMessage>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, destination_hash, content, priority, conversation_message_id, queued_at, retry_count, last_retry_at, status, expires_at
             FROM message_queue
             WHERE status = 'pending'
             ORDER BY
               CASE WHEN priority = 'high' THEN 0 ELSE 1 END,
               queued_at ASC",
        )
        .map_err(|e| format!("Failed to prepare pending messages query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(QueuedMessage {
                id: row.get(0)?,
                destination_hash: row.get(1)?,
                content: row.get(2)?,
                priority: row.get(3)?,
                conversation_message_id: row.get(4)?,
                queued_at: row.get(5)?,
                retry_count: row.get(6)?,
                last_retry_at: row.get(7)?,
                status: row.get(8)?,
                expires_at: row.get(9)?,
            })
        })
        .map_err(|e| format!("Failed to query pending messages: {e}"))?;

    let mut messages = Vec::new();
    for row in rows {
        messages.push(row.map_err(|e| format!("Failed to read message row: {e}"))?);
    }
    Ok(messages)
}

/// Load all messages from the database (for restoring in-memory queue).
pub fn load_queue_from_db(conn: &Connection) -> Result<Vec<QueuedMessage>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, destination_hash, content, priority, conversation_message_id, queued_at, retry_count, last_retry_at, status, expires_at
             FROM message_queue
             WHERE status = 'pending'
             ORDER BY queued_at ASC",
        )
        .map_err(|e| format!("Failed to prepare load queue query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(QueuedMessage {
                id: row.get(0)?,
                destination_hash: row.get(1)?,
                content: row.get(2)?,
                priority: row.get(3)?,
                conversation_message_id: row.get(4)?,
                queued_at: row.get(5)?,
                retry_count: row.get(6)?,
                last_retry_at: row.get(7)?,
                status: row.get(8)?,
                expires_at: row.get(9)?,
            })
        })
        .map_err(|e| format!("Failed to load queue from DB: {e}"))?;

    let mut messages = Vec::new();
    for row in rows {
        messages.push(row.map_err(|e| format!("Failed to read queue row: {e}"))?);
    }
    Ok(messages)
}

/// Persist the in-memory queue to the database (upsert).
pub fn persist_queue_to_db(conn: &Connection, queue: &[QueuedMessage]) -> Result<(), String> {
    for msg in queue {
        conn.execute(
            "INSERT OR REPLACE INTO message_queue (id, destination_hash, content, priority, conversation_message_id, queued_at, retry_count, last_retry_at, status, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                msg.id,
                msg.destination_hash,
                msg.content,
                msg.priority,
                msg.conversation_message_id,
                msg.queued_at,
                msg.retry_count,
                msg.last_retry_at,
                msg.status,
                msg.expires_at,
            ],
        )
        .map_err(|e| format!("Failed to persist queue message: {e}"))?;
    }
    Ok(())
}

/// Expire messages older than max_age_hours.
pub fn expire_old_messages(conn: &Connection, now_iso: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id FROM message_queue WHERE status = 'pending' AND expires_at <= ?1",
        )
        .map_err(|e| format!("Failed to prepare expiration query: {e}"))?;

    let expired_ids: Vec<String> = stmt
        .query_map(params![now_iso], |row| row.get(0))
        .map_err(|e| format!("Failed to query expired messages: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    for id in &expired_ids {
        mark_message_expired(conn, id)?;
    }

    Ok(expired_ids)
}


// ─── Delivery State CRUD ──────────────────────────────────────────────────────

/// Create a new delivery state record.
pub fn create_delivery_state(conn: &Connection, state: &DeliveryState) -> Result<(), String> {
    conn.execute(
        "INSERT INTO delivery_states (message_id, lxmf_message_id, conversation_message_id, destination_hash, status, sent_at, confirmed_at, timeout_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            state.message_id,
            state.lxmf_message_id,
            state.conversation_message_id,
            state.destination_hash,
            state.status,
            state.sent_at,
            state.confirmed_at,
            state.timeout_at,
        ],
    )
    .map_err(|e| format!("Failed to create delivery state: {e}"))?;
    Ok(())
}

/// Confirm delivery of a message by LXMF message ID.
pub fn confirm_delivery(conn: &Connection, lxmf_message_id: &str, confirmed_at: &str) -> Result<bool, String> {
    let rows = conn
        .execute(
            "UPDATE delivery_states SET status = 'complete', confirmed_at = ?1 WHERE lxmf_message_id = ?2 AND status = 'pending'",
            params![confirmed_at, lxmf_message_id],
        )
        .map_err(|e| format!("Failed to confirm delivery: {e}"))?;
    Ok(rows > 0)
}

/// Mark a delivery as unconfirmed (timeout elapsed).
pub fn mark_delivery_unconfirmed(conn: &Connection, message_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE delivery_states SET status = 'delivery-unconfirmed' WHERE message_id = ?1 AND status = 'pending'",
        params![message_id],
    )
    .map_err(|e| format!("Failed to mark delivery unconfirmed: {e}"))?;
    Ok(())
}

/// Mark a delivery as failed.
pub fn mark_delivery_failed(conn: &Connection, message_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE delivery_states SET status = 'failed' WHERE message_id = ?1 AND status = 'pending'",
        params![message_id],
    )
    .map_err(|e| format!("Failed to mark delivery failed: {e}"))?;
    Ok(())
}

/// Check delivery timeouts: transition pending deliveries past timeout to "delivery-unconfirmed".
pub fn check_delivery_timeouts(conn: &Connection, now_iso: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT message_id FROM delivery_states WHERE status = 'pending' AND timeout_at <= ?1",
        )
        .map_err(|e| format!("Failed to prepare timeout query: {e}"))?;

    let timed_out: Vec<String> = stmt
        .query_map(params![now_iso], |row| row.get(0))
        .map_err(|e| format!("Failed to query timed out deliveries: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    for id in &timed_out {
        mark_delivery_unconfirmed(conn, id)?;
    }

    Ok(timed_out)
}

/// Get delivery state by message ID.
pub fn get_delivery_state(conn: &Connection, message_id: &str) -> Result<Option<DeliveryState>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT message_id, lxmf_message_id, conversation_message_id, destination_hash, status, sent_at, confirmed_at, timeout_at
             FROM delivery_states WHERE message_id = ?1",
        )
        .map_err(|e| format!("Failed to prepare delivery state query: {e}"))?;

    let result = stmt
        .query_row(params![message_id], |row| {
            Ok(DeliveryState {
                message_id: row.get(0)?,
                lxmf_message_id: row.get(1)?,
                conversation_message_id: row.get(2)?,
                destination_hash: row.get(3)?,
                status: row.get(4)?,
                sent_at: row.get(5)?,
                confirmed_at: row.get(6)?,
                timeout_at: row.get(7)?,
            })
        })
        .ok();

    Ok(result)
}

// ─── Known Peers CRUD ─────────────────────────────────────────────────────────

/// Upsert a known peer.
pub fn upsert_peer(conn: &Connection, peer: &KnownPeer) -> Result<(), String> {
    conn.execute(
        "INSERT INTO known_peers (destination_hash, display_name, first_seen_at, last_seen_at, conversation_thread_id, link_active)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(destination_hash) DO UPDATE SET
           display_name = COALESCE(excluded.display_name, known_peers.display_name),
           last_seen_at = excluded.last_seen_at,
           conversation_thread_id = COALESCE(excluded.conversation_thread_id, known_peers.conversation_thread_id),
           link_active = excluded.link_active",
        params![
            peer.destination_hash,
            peer.display_name,
            peer.first_seen_at,
            peer.last_seen_at,
            peer.conversation_thread_id,
            peer.link_active as i32,
        ],
    )
    .map_err(|e| format!("Failed to upsert peer: {e}"))?;
    Ok(())
}

/// Query all known peers.
pub fn query_peers(conn: &Connection) -> Result<Vec<KnownPeer>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT destination_hash, display_name, first_seen_at, last_seen_at, conversation_thread_id, link_active
             FROM known_peers ORDER BY last_seen_at DESC",
        )
        .map_err(|e| format!("Failed to prepare peers query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(KnownPeer {
                destination_hash: row.get(0)?,
                display_name: row.get(1)?,
                first_seen_at: row.get(2)?,
                last_seen_at: row.get(3)?,
                conversation_thread_id: row.get(4)?,
                link_active: row.get::<_, i32>(5)? != 0,
            })
        })
        .map_err(|e| format!("Failed to query peers: {e}"))?;

    let mut peers = Vec::new();
    for row in rows {
        peers.push(row.map_err(|e| format!("Failed to read peer row: {e}"))?);
    }
    Ok(peers)
}

/// Update peer link status.
pub fn update_peer_link_status(
    conn: &Connection,
    destination_hash: &str,
    link_active: bool,
) -> Result<(), String> {
    conn.execute(
        "UPDATE known_peers SET link_active = ?1, last_seen_at = ?2 WHERE destination_hash = ?3",
        params![link_active as i32, Utc::now().to_rfc3339(), destination_hash],
    )
    .map_err(|e| format!("Failed to update peer link status: {e}"))?;
    Ok(())
}

/// Get the conversation thread ID for a peer.
pub fn get_peer_thread_id(conn: &Connection, destination_hash: &str) -> Result<Option<String>, String> {
    let mut stmt = conn
        .prepare("SELECT conversation_thread_id FROM known_peers WHERE destination_hash = ?1")
        .map_err(|e| format!("Failed to prepare peer thread query: {e}"))?;

    let result: Option<Option<String>> = stmt
        .query_row(params![destination_hash], |row| row.get(0))
        .ok();

    Ok(result.flatten())
}

// ─── Channel Config CRUD ──────────────────────────────────────────────────────

/// Channel configuration record from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfigRecord {
    pub identity_label: String,
    pub enabled: bool,
    pub bandwidth_profiles_json: String,
    pub delivery_timeout_lora_secs: u64,
    pub delivery_timeout_tcp_secs: u64,
    pub queue_max_age_hours: u64,
    pub queue_retry_interval_secs: u64,
}

/// Read channel configuration from the database.
pub fn read_config(conn: &Connection) -> Result<Option<ChannelConfigRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT identity_label, enabled, bandwidth_profiles_json, delivery_timeout_lora_secs, delivery_timeout_tcp_secs, queue_max_age_hours, queue_retry_interval_secs
             FROM channel_config WHERE id = 'singleton'",
        )
        .map_err(|e| format!("Failed to prepare config query: {e}"))?;

    let result = stmt
        .query_row([], |row| {
            Ok(ChannelConfigRecord {
                identity_label: row.get(0)?,
                enabled: row.get::<_, i32>(1)? != 0,
                bandwidth_profiles_json: row.get(2)?,
                delivery_timeout_lora_secs: row.get::<_, i64>(3)? as u64,
                delivery_timeout_tcp_secs: row.get::<_, i64>(4)? as u64,
                queue_max_age_hours: row.get::<_, i64>(5)? as u64,
                queue_retry_interval_secs: row.get::<_, i64>(6)? as u64,
            })
        })
        .ok();

    Ok(result)
}

/// Update channel configuration in the database.
pub fn update_config(conn: &Connection, config: &ChannelConfigRecord) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO channel_config (id, identity_label, enabled, bandwidth_profiles_json, delivery_timeout_lora_secs, delivery_timeout_tcp_secs, queue_max_age_hours, queue_retry_interval_secs)
         VALUES ('singleton', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            config.identity_label,
            config.enabled as i32,
            config.bandwidth_profiles_json,
            config.delivery_timeout_lora_secs as i64,
            config.delivery_timeout_tcp_secs as i64,
            config.queue_max_age_hours as i64,
            config.queue_retry_interval_secs as i64,
        ],
    )
    .map_err(|e| format!("Failed to update config: {e}"))?;
    Ok(())
}

/// Read bandwidth profiles from the config.
pub fn read_bandwidth_profiles(conn: &Connection) -> Result<Vec<BandwidthProfile>, String> {
    let config = read_config(conn)?;
    match config {
        Some(c) => {
            let profiles: Vec<BandwidthProfile> = serde_json::from_str(&c.bandwidth_profiles_json)
                .unwrap_or_else(|_| default_bandwidth_profiles());
            Ok(profiles)
        }
        None => Ok(default_bandwidth_profiles()),
    }
}


// ─── Sidecar Lifecycle ────────────────────────────────────────────────────────

/// Spawn the Python sidecar process.
pub async fn spawn_sidecar(state: &ReticulumChannelState) -> Result<String, String> {
    {
        let mut health = state.health_state.write().await;
        *health = SidecarHealthState::Starting;
    }

    // In production this would use tokio::process::Command to spawn the sidecar.
    // The sidecar communicates via stdin/stdout JSON-RPC.
    // For now, return the expected destination hash placeholder.
    {
        let mut health = state.health_state.write().await;
        *health = SidecarHealthState::Running;
    }
    {
        let mut delay = state.restart_delay_secs.write().await;
        *delay = state.config.restart_initial_delay_secs;
    }
    {
        let mut failures = state.consecutive_ping_failures.write().await;
        *failures = 0;
    }

    Ok("sidecar_started".to_string())
}

/// Stop the sidecar gracefully.
pub async fn stop_sidecar(state: &ReticulumChannelState) -> Result<(), String> {
    let mut health = state.health_state.write().await;
    *health = SidecarHealthState::Offline;
    Ok(())
}

/// Health check: send ping, track failures, transition state.
pub async fn health_check(state: &ReticulumChannelState) -> SidecarHealthState {
    let current = state.health_state.read().await.clone();
    if current != SidecarHealthState::Running {
        return current;
    }

    // In production, this sends a JSON-RPC ping and waits for pong.
    // Simulated: assume ping succeeds (failures tracked externally).
    current
}

/// Record a ping failure and potentially transition to offline.
pub async fn record_ping_failure(state: &ReticulumChannelState) -> SidecarHealthState {
    let mut failures = state.consecutive_ping_failures.write().await;
    *failures += 1;

    if *failures >= state.config.health_check_failures_threshold {
        let mut health = state.health_state.write().await;
        *health = SidecarHealthState::Offline;
        return SidecarHealthState::Offline;
    }

    state.health_state.read().await.clone()
}

/// Record a successful ping.
pub async fn record_ping_success(state: &ReticulumChannelState) {
    let mut failures = state.consecutive_ping_failures.write().await;
    *failures = 0;
}

/// Record a crash event.
pub async fn record_crash(state: &ReticulumChannelState) {
    let mut health = state.health_state.write().await;
    *health = SidecarHealthState::Crashed;
}

/// Attempt restart with exponential backoff.
/// Returns the delay that was used.
pub async fn attempt_restart(state: &ReticulumChannelState) -> Result<u64, String> {
    let delay = {
        let d = state.restart_delay_secs.read().await;
        *d
    };

    // Update delay for next attempt (exponential backoff, capped)
    {
        let mut d = state.restart_delay_secs.write().await;
        let next = (*d * 2).min(state.config.restart_max_delay_secs);
        *d = next;
    }

    // Attempt to spawn
    spawn_sidecar(state).await?;

    Ok(delay)
}

/// Compute the next restart delay based on current state (for testing).
pub fn compute_next_restart_delay(current_delay: u64, max_delay: u64) -> u64 {
    (current_delay * 2).min(max_delay)
}

/// Reset restart delay on successful start.
pub async fn reset_restart_delay(state: &ReticulumChannelState) {
    let mut d = state.restart_delay_secs.write().await;
    *d = state.config.restart_initial_delay_secs;
}

// ─── IPC Commands ─────────────────────────────────────────────────────────────

/// IPC: Send a message via the Reticulum channel.
pub fn ipc_reticulum_send_message(
    conn: &Connection,
    destination_hash: String,
    content: String,
    priority: String,
    conversation_message_id: String,
    max_age_hours: u64,
) -> Result<String, String> {
    let now = Utc::now();
    let id = format!("msg-{}", now.timestamp_millis());
    let expires_at = now + chrono::Duration::hours(max_age_hours as i64);

    let msg = QueuedMessage {
        id: id.clone(),
        destination_hash,
        content,
        priority,
        conversation_message_id,
        queued_at: now.to_rfc3339(),
        retry_count: 0,
        last_retry_at: None,
        status: "pending".to_string(),
        expires_at: expires_at.to_rfc3339(),
    };

    enqueue_message(conn, &msg)?;
    Ok(id)
}

/// IPC: Get channel status.
pub async fn ipc_reticulum_get_status(
    state: &ReticulumChannelState,
) -> serde_json::Value {
    let health = state.health_state.read().await;
    let interfaces = state.active_interfaces.read().await;
    let queue = state.message_queue.read().await;
    let dest = state.destination_hash.read().await;

    serde_json::json!({
        "healthState": health.as_str(),
        "destinationHash": *dest,
        "activeInterfaces": *interfaces,
        "peersCount": 0,
        "queuedMessages": queue.iter().filter(|m| m.status == "pending").count(),
    })
}

/// IPC: Get delivery state for a message.
pub fn ipc_reticulum_get_delivery_state(
    conn: &Connection,
    message_id: &str,
) -> Result<Option<DeliveryState>, String> {
    get_delivery_state(conn, message_id)
}

/// IPC: Get queue status.
pub fn ipc_reticulum_get_queue_status(conn: &Connection) -> Result<Vec<QueuedMessage>, String> {
    query_pending_messages(conn)
}

/// IPC: List known peers.
pub fn ipc_reticulum_list_peers(conn: &Connection) -> Result<Vec<KnownPeer>, String> {
    query_peers(conn)
}

// ─── Inbound Message Handling ─────────────────────────────────────────────────

/// Inbound message notification from the sidecar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessageNotification {
    pub source_hash: String,
    pub source_name: Option<String>,
    pub content: String,
    pub timestamp: String,
    pub lxmf_message_id: String,
}

/// Process an inbound message notification: upsert peer, return data for thread insertion.
pub fn handle_inbound_message(
    conn: &Connection,
    notification: &InboundMessageNotification,
) -> Result<(KnownPeer, String), String> {
    let now = Utc::now().to_rfc3339();
    let thread_id = format!("reticulum-thread-{}", &notification.source_hash);

    let peer = KnownPeer {
        destination_hash: notification.source_hash.clone(),
        display_name: notification.source_name.clone(),
        first_seen_at: now.clone(),
        last_seen_at: now,
        conversation_thread_id: Some(thread_id.clone()),
        link_active: true,
    };

    upsert_peer(conn, &peer)?;

    Ok((peer, thread_id))
}

// ─── Outbound Message Handling ────────────────────────────────────────────────

/// Create a delivery state for an outbound message that was successfully sent.
pub fn handle_outbound_sent(
    conn: &Connection,
    message_id: &str,
    lxmf_message_id: &str,
    conversation_message_id: &str,
    destination_hash: &str,
    timeout_secs: u64,
) -> Result<(), String> {
    let now = Utc::now();
    let timeout_at = now + chrono::Duration::seconds(timeout_secs as i64);

    let state = DeliveryState {
        message_id: message_id.to_string(),
        lxmf_message_id: lxmf_message_id.to_string(),
        conversation_message_id: conversation_message_id.to_string(),
        destination_hash: destination_hash.to_string(),
        status: "pending".to_string(),
        sent_at: now.to_rfc3339(),
        confirmed_at: None,
        timeout_at: timeout_at.to_rfc3339(),
    };

    create_delivery_state(conn, &state)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize_reticulum_db(&conn).unwrap();
        conn
    }

    #[test]
    fn test_initialize_db_creates_tables() {
        let conn = setup_db();
        // Verify tables exist by querying them
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM message_queue", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM delivery_states", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM known_peers", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_enqueue_and_dequeue_fifo() {
        let conn = setup_db();

        let msg1 = QueuedMessage {
            id: "msg-1".into(),
            destination_hash: "dest-a".into(),
            content: "first".into(),
            priority: "normal".into(),
            conversation_message_id: "conv-1".into(),
            queued_at: "2025-01-01T00:00:00Z".into(),
            retry_count: 0,
            last_retry_at: None,
            status: "pending".into(),
            expires_at: "2025-01-02T00:00:00Z".into(),
        };

        let msg2 = QueuedMessage {
            id: "msg-2".into(),
            destination_hash: "dest-a".into(),
            content: "second".into(),
            priority: "normal".into(),
            conversation_message_id: "conv-2".into(),
            queued_at: "2025-01-01T00:01:00Z".into(),
            retry_count: 0,
            last_retry_at: None,
            status: "pending".into(),
            expires_at: "2025-01-02T00:01:00Z".into(),
        };

        enqueue_message(&conn, &msg1).unwrap();
        enqueue_message(&conn, &msg2).unwrap();

        let next = dequeue_next_for_destination(&conn, "dest-a").unwrap().unwrap();
        assert_eq!(next.id, "msg-1");
        assert_eq!(next.content, "first");
    }

    #[test]
    fn test_priority_ordering() {
        let conn = setup_db();

        let normal = QueuedMessage {
            id: "msg-normal".into(),
            destination_hash: "dest-a".into(),
            content: "normal msg".into(),
            priority: "normal".into(),
            conversation_message_id: "conv-1".into(),
            queued_at: "2025-01-01T00:00:00Z".into(),
            retry_count: 0,
            last_retry_at: None,
            status: "pending".into(),
            expires_at: "2025-01-02T00:00:00Z".into(),
        };

        let high = QueuedMessage {
            id: "msg-high".into(),
            destination_hash: "dest-a".into(),
            content: "high msg".into(),
            priority: "high".into(),
            conversation_message_id: "conv-2".into(),
            queued_at: "2025-01-01T00:01:00Z".into(),
            retry_count: 0,
            last_retry_at: None,
            status: "pending".into(),
            expires_at: "2025-01-02T00:01:00Z".into(),
        };

        enqueue_message(&conn, &normal).unwrap();
        enqueue_message(&conn, &high).unwrap();

        // High priority should come first even though it was enqueued later
        let next = dequeue_next_for_destination(&conn, "dest-a").unwrap().unwrap();
        assert_eq!(next.id, "msg-high");
    }

    #[test]
    fn test_delivery_state_transitions() {
        let conn = setup_db();

        let state = DeliveryState {
            message_id: "del-1".into(),
            lxmf_message_id: "lxmf-1".into(),
            conversation_message_id: "conv-1".into(),
            destination_hash: "dest-a".into(),
            status: "pending".into(),
            sent_at: "2025-01-01T00:00:00Z".into(),
            confirmed_at: None,
            timeout_at: "2025-01-01T00:05:00Z".into(),
        };

        create_delivery_state(&conn, &state).unwrap();

        // Confirm delivery
        let confirmed = confirm_delivery(&conn, "lxmf-1", "2025-01-01T00:02:00Z").unwrap();
        assert!(confirmed);

        let updated = get_delivery_state(&conn, "del-1").unwrap().unwrap();
        assert_eq!(updated.status, "complete");
        assert_eq!(updated.confirmed_at, Some("2025-01-01T00:02:00Z".into()));
    }

    #[test]
    fn test_delivery_timeout() {
        let conn = setup_db();

        let state = DeliveryState {
            message_id: "del-2".into(),
            lxmf_message_id: "lxmf-2".into(),
            conversation_message_id: "conv-2".into(),
            destination_hash: "dest-b".into(),
            status: "pending".into(),
            sent_at: "2025-01-01T00:00:00Z".into(),
            confirmed_at: None,
            timeout_at: "2025-01-01T00:05:00Z".into(),
        };

        create_delivery_state(&conn, &state).unwrap();

        // Check timeouts at a time past the timeout
        let timed_out = check_delivery_timeouts(&conn, "2025-01-01T00:06:00Z").unwrap();
        assert_eq!(timed_out, vec!["del-2"]);

        let updated = get_delivery_state(&conn, "del-2").unwrap().unwrap();
        assert_eq!(updated.status, "delivery-unconfirmed");
    }

    #[test]
    fn test_delivery_failed() {
        let conn = setup_db();

        let state = DeliveryState {
            message_id: "del-3".into(),
            lxmf_message_id: "lxmf-3".into(),
            conversation_message_id: "conv-3".into(),
            destination_hash: "dest-c".into(),
            status: "pending".into(),
            sent_at: "2025-01-01T00:00:00Z".into(),
            confirmed_at: None,
            timeout_at: "2025-01-01T00:05:00Z".into(),
        };

        create_delivery_state(&conn, &state).unwrap();
        mark_delivery_failed(&conn, "del-3").unwrap();

        let updated = get_delivery_state(&conn, "del-3").unwrap().unwrap();
        assert_eq!(updated.status, "failed");
    }

    #[test]
    fn test_message_expiration() {
        let conn = setup_db();

        let msg = QueuedMessage {
            id: "msg-exp".into(),
            destination_hash: "dest-a".into(),
            content: "will expire".into(),
            priority: "normal".into(),
            conversation_message_id: "conv-exp".into(),
            queued_at: "2025-01-01T00:00:00Z".into(),
            retry_count: 0,
            last_retry_at: None,
            status: "pending".into(),
            expires_at: "2025-01-01T12:00:00Z".into(),
        };

        enqueue_message(&conn, &msg).unwrap();

        // Expire at a time past the expiration
        let expired = expire_old_messages(&conn, "2025-01-01T13:00:00Z").unwrap();
        assert_eq!(expired, vec!["msg-exp"]);

        // Verify it's no longer pending
        let pending = query_pending_messages(&conn).unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn test_peer_upsert_and_query() {
        let conn = setup_db();

        let peer = KnownPeer {
            destination_hash: "peer-1".into(),
            display_name: Some("Alice".into()),
            first_seen_at: "2025-01-01T00:00:00Z".into(),
            last_seen_at: "2025-01-01T00:00:00Z".into(),
            conversation_thread_id: Some("thread-1".into()),
            link_active: true,
        };

        upsert_peer(&conn, &peer).unwrap();

        let peers = query_peers(&conn).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].display_name, Some("Alice".into()));
        assert!(peers[0].link_active);
    }

    #[test]
    fn test_config_crud() {
        let conn = setup_db();

        let config = ChannelConfigRecord {
            identity_label: "TestNode".into(),
            enabled: true,
            bandwidth_profiles_json: "[]".into(),
            delivery_timeout_lora_secs: 300,
            delivery_timeout_tcp_secs: 30,
            queue_max_age_hours: 24,
            queue_retry_interval_secs: 30,
        };

        update_config(&conn, &config).unwrap();
        let loaded = read_config(&conn).unwrap().unwrap();
        assert_eq!(loaded.identity_label, "TestNode");
        assert!(loaded.enabled);
    }

    #[test]
    fn test_queue_persistence_across_reload() {
        let conn = setup_db();

        let msg = QueuedMessage {
            id: "persist-1".into(),
            destination_hash: "dest-p".into(),
            content: "persistent".into(),
            priority: "normal".into(),
            conversation_message_id: "conv-p".into(),
            queued_at: "2025-01-01T00:00:00Z".into(),
            retry_count: 0,
            last_retry_at: None,
            status: "pending".into(),
            expires_at: "2025-01-02T00:00:00Z".into(),
        };

        enqueue_message(&conn, &msg).unwrap();

        // Simulate restart by loading from DB
        let loaded = load_queue_from_db(&conn).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "persist-1");
        assert_eq!(loaded[0].content, "persistent");
    }

    #[test]
    fn test_exponential_backoff() {
        assert_eq!(compute_next_restart_delay(5, 60), 10);
        assert_eq!(compute_next_restart_delay(10, 60), 20);
        assert_eq!(compute_next_restart_delay(20, 60), 40);
        assert_eq!(compute_next_restart_delay(40, 60), 60);
        assert_eq!(compute_next_restart_delay(60, 60), 60); // capped
        assert_eq!(compute_next_restart_delay(100, 60), 60); // capped
    }

    #[test]
    fn test_sidecar_health_state_serialization() {
        assert_eq!(SidecarHealthState::Running.as_str(), "running");
        assert_eq!(SidecarHealthState::Starting.as_str(), "starting");
        assert_eq!(SidecarHealthState::Offline.as_str(), "offline");
        assert_eq!(SidecarHealthState::Crashed.as_str(), "crashed");

        assert_eq!(SidecarHealthState::from_str("running"), Some(SidecarHealthState::Running));
        assert_eq!(SidecarHealthState::from_str("invalid"), None);
    }
}
