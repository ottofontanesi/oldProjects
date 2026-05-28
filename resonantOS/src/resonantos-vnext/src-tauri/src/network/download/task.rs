// Intent citation: .kiro/specs/model-download-engine/design.md — DownloadTask
// Single-download execution logic: HTTP streaming, resume, integrity, retry.

use super::events::DownloadId;
use std::path::PathBuf;
use std::time::Duration;

/// Error types for download operations.
#[derive(Debug, Clone)]
pub enum DownloadError {
    /// Network connectivity error.
    NetworkError(String),
    /// Disk is full or below minimum threshold.
    DiskFull { available_mb: u64, required_mb: u64 },
    /// SHA256 hash mismatch after download.
    IntegrityMismatch { computed: String, expected: String },
    /// Download was cancelled by user or system.
    Cancelled,
    /// Invalid or malformed URL.
    InvalidUrl(String),
    /// TLS/certificate error (not retryable).
    TlsError(String),
    /// Server returned 5xx error.
    ServerError(u16),
    /// Connection or read timeout.
    Timeout,
    /// Resume state is corrupted or invalid.
    ResumeCorrupted(String),
    /// Rate limited by server (HTTP 429).
    RateLimited { retry_after_secs: u64 },
    /// Insufficient disk space.
    InsufficientSpace(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NetworkError(msg) => write!(f, "Network error: {}", msg),
            Self::DiskFull { available_mb, required_mb } => {
                write!(f, "Disk full: need {}MB, have {}MB", required_mb, available_mb)
            }
            Self::IntegrityMismatch { computed, expected } => {
                write!(f, "Integrity mismatch: computed={}, expected={}", computed, expected)
            }
            Self::Cancelled => write!(f, "Download cancelled"),
            Self::InvalidUrl(url) => write!(f, "Invalid URL: {}", url),
            Self::TlsError(msg) => write!(f, "TLS error: {}", msg),
            Self::ServerError(code) => write!(f, "Server error: HTTP {}", code),
            Self::Timeout => write!(f, "Connection timeout"),
            Self::ResumeCorrupted(msg) => write!(f, "Resume state corrupted: {}", msg),
            Self::RateLimited { retry_after_secs } => {
                write!(f, "Rate limited, retry after {}s", retry_after_secs)
            }
            Self::InsufficientSpace(msg) => write!(f, "Insufficient space: {}", msg),
        }
    }
}

impl std::error::Error for DownloadError {}

impl DownloadError {
    /// Whether this error is retryable with backoff.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::NetworkError(_) | Self::ServerError(_) | Self::Timeout | Self::RateLimited { .. }
        )
    }

    /// Whether this is a TLS error (never retry).
    pub fn is_tls_error(&self) -> bool {
        matches!(self, Self::TlsError(_))
    }
}

/// Result of a successful download.
#[derive(Debug, Clone)]
pub struct CompletedDownload {
    pub id: DownloadId,
    pub model_id: String,
    pub file_path: PathBuf,
    pub total_bytes: u64,
    pub duration_ms: u64,
    pub sha256: String,
}

/// Compute the backoff duration for a given retry attempt.
/// Uses exponential backoff: base_ms * 2^attempt.
pub fn compute_backoff(base_ms: u64, attempt: u32) -> Duration {
    let delay_ms = base_ms.saturating_mul(1u64 << attempt.min(10));
    Duration::from_millis(delay_ms.min(60_000)) // Cap at 60 seconds
}

/// Determine if an HTTP status code is retryable.
pub fn is_retryable_status(status: u16) -> bool {
    status == 429 || (500..600).contains(&status) || status == 408
}

/// Parse the Retry-After header value (seconds or HTTP-date).
/// Returns seconds to wait, defaulting to 60 if unparseable.
pub fn parse_retry_after(value: &str) -> u64 {
    // Try parsing as seconds first
    if let Ok(secs) = value.trim().parse::<u64>() {
        return secs;
    }
    // Default to 60 seconds if we can't parse
    60
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_backoff() {
        assert_eq!(compute_backoff(1000, 0), Duration::from_millis(1000));
        assert_eq!(compute_backoff(1000, 1), Duration::from_millis(2000));
        assert_eq!(compute_backoff(1000, 2), Duration::from_millis(4000));
        assert_eq!(compute_backoff(1000, 3), Duration::from_millis(8000));
        // Capped at 60 seconds
        assert_eq!(compute_backoff(1000, 20), Duration::from_millis(60_000));
    }

    #[test]
    fn test_is_retryable_status() {
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(502));
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(408));
        assert!(!is_retryable_status(200));
        assert!(!is_retryable_status(404));
        assert!(!is_retryable_status(403));
    }

    #[test]
    fn test_parse_retry_after() {
        assert_eq!(parse_retry_after("120"), 120);
        assert_eq!(parse_retry_after("30"), 30);
        assert_eq!(parse_retry_after("not-a-number"), 60); // Default
        assert_eq!(parse_retry_after("  45  "), 45); // Trimmed
    }

    #[test]
    fn test_download_error_display() {
        let err = DownloadError::NetworkError("connection reset".to_string());
        assert!(format!("{}", err).contains("connection reset"));

        let err = DownloadError::IntegrityMismatch {
            computed: "abc".to_string(),
            expected: "def".to_string(),
        };
        assert!(format!("{}", err).contains("abc"));
        assert!(format!("{}", err).contains("def"));
    }

    #[test]
    fn test_download_error_retryable() {
        assert!(DownloadError::NetworkError("timeout".to_string()).is_retryable());
        assert!(DownloadError::ServerError(500).is_retryable());
        assert!(DownloadError::Timeout.is_retryable());
        assert!(DownloadError::RateLimited { retry_after_secs: 60 }.is_retryable());

        assert!(!DownloadError::Cancelled.is_retryable());
        assert!(!DownloadError::TlsError("cert".to_string()).is_retryable());
        assert!(!DownloadError::InvalidUrl("bad".to_string()).is_retryable());
    }

    #[test]
    fn test_download_error_is_tls() {
        assert!(DownloadError::TlsError("bad cert".to_string()).is_tls_error());
        assert!(!DownloadError::NetworkError("timeout".to_string()).is_tls_error());
    }
}
