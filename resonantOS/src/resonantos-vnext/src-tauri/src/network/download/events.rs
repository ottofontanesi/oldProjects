// Intent citation: .kiro/specs/model-download-engine/design.md — Events
// Download event types for progress reporting and lifecycle notifications.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Unique identifier for a download operation.
pub type DownloadId = Uuid;

/// Events emitted during the download lifecycle.
/// These are sent via Tauri's event system to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DownloadEvent {
    /// Emitted when a download begins execution.
    Started {
        id: DownloadId,
        model_id: String,
        total_bytes: u64,
        priority: u8,
    },
    /// Emitted periodically (every 500ms or 1MB) with progress info.
    Progress {
        id: DownloadId,
        bytes_downloaded: u64,
        total_bytes: u64,
        speed_bps: u64,
        eta_secs: u64,
    },
    /// Emitted when a download finishes successfully.
    Completed {
        id: DownloadId,
        model_id: String,
        file_path: PathBuf,
        duration_ms: u64,
    },
    /// Emitted when a download fails (may still have retries left).
    Failed {
        id: DownloadId,
        model_id: String,
        reason: String,
        retries_remaining: u32,
    },
    /// Emitted when a download is cancelled by the user or system.
    Cancelled {
        id: DownloadId,
    },
    /// Emitted when a download is paused (preemption or user request).
    Paused {
        id: DownloadId,
        bytes_so_far: u64,
    },
    /// Emitted when a paused download resumes.
    Resumed {
        id: DownloadId,
        bytes_so_far: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_event_serialization() {
        let event = DownloadEvent::Started {
            id: Uuid::new_v4(),
            model_id: "llama-7b".to_string(),
            total_bytes: 4_000_000_000,
            priority: 1,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"Started\""));
        assert!(json.contains("\"model_id\":\"llama-7b\""));
    }

    #[test]
    fn test_progress_event_serialization() {
        let id = Uuid::new_v4();
        let event = DownloadEvent::Progress {
            id,
            bytes_downloaded: 500_000_000,
            total_bytes: 4_000_000_000,
            speed_bps: 10_000_000,
            eta_secs: 350,
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: DownloadEvent = serde_json::from_str(&json).unwrap();
        match deserialized {
            DownloadEvent::Progress { bytes_downloaded, .. } => {
                assert_eq!(bytes_downloaded, 500_000_000);
            }
            _ => panic!("Expected Progress event"),
        }
    }

    #[test]
    fn test_all_event_variants_serialize() {
        let id = Uuid::new_v4();
        let events = vec![
            DownloadEvent::Started { id, model_id: "m".to_string(), total_bytes: 100, priority: 0 },
            DownloadEvent::Progress { id, bytes_downloaded: 50, total_bytes: 100, speed_bps: 10, eta_secs: 5 },
            DownloadEvent::Completed { id, model_id: "m".to_string(), file_path: PathBuf::from("/tmp/m"), duration_ms: 1000 },
            DownloadEvent::Failed { id, model_id: "m".to_string(), reason: "timeout".to_string(), retries_remaining: 2 },
            DownloadEvent::Cancelled { id },
            DownloadEvent::Paused { id, bytes_so_far: 50 },
            DownloadEvent::Resumed { id, bytes_so_far: 50 },
        ];
        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let _: DownloadEvent = serde_json::from_str(&json).unwrap();
        }
    }
}
