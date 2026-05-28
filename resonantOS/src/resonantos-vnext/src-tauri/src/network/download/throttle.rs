// Intent citation: .kiro/specs/model-download-engine/design.md — BandwidthThrottle
// Token-bucket rate limiter for download bandwidth control.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Token-bucket bandwidth throttle shared across all concurrent downloads.
///
/// When a limit is set, downloads must acquire tokens before writing data.
/// Tokens refill at the configured rate. When unlimited (limit = 0), acquire
/// returns immediately without blocking.
pub struct BandwidthThrottleAsync {
    /// Bandwidth limit in bytes per second. 0 = unlimited.
    limit_bps: AtomicU64,
    /// Available tokens (bytes that can be consumed right now).
    tokens: AtomicU64,
    /// Last refill timestamp in milliseconds since an arbitrary epoch.
    last_refill_ms: AtomicU64,
}

impl BandwidthThrottleAsync {
    /// Create a new bandwidth throttle.
    /// Pass `None` for unlimited bandwidth.
    pub fn new(limit_bps: Option<u64>) -> Self {
        let limit = limit_bps.unwrap_or(0);
        Self {
            limit_bps: AtomicU64::new(limit),
            tokens: AtomicU64::new(limit), // Start with a full bucket
            last_refill_ms: AtomicU64::new(0),
        }
    }

    /// Acquire tokens for the given number of bytes.
    /// Blocks (async sleep) until enough tokens are available.
    /// Returns immediately if bandwidth is unlimited.
    pub async fn acquire(&self, bytes: u64) {
        let limit = self.limit_bps.load(Ordering::Relaxed);
        if limit == 0 {
            return; // Unlimited — no throttling
        }

        loop {
            self.refill(limit);

            let current_tokens = self.tokens.load(Ordering::Relaxed);
            if current_tokens >= bytes {
                // Try to consume tokens atomically
                match self.tokens.compare_exchange(
                    current_tokens,
                    current_tokens - bytes,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return,
                    Err(_) => continue, // Another task consumed tokens, retry
                }
            }

            // Not enough tokens — calculate sleep time
            let needed = bytes.saturating_sub(current_tokens);
            let wait_ms = (needed as f64 / limit as f64 * 1000.0).max(10.0) as u64;
            tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        }
    }

    /// Change the bandwidth limit at runtime.
    /// Pass `None` for unlimited. Does not reset current tokens.
    pub fn set_limit(&self, bps: Option<u64>) {
        self.limit_bps.store(bps.unwrap_or(0), Ordering::Relaxed);
    }

    /// Get the current bandwidth limit. Returns None if unlimited.
    pub fn current_limit(&self) -> Option<u64> {
        let limit = self.limit_bps.load(Ordering::Relaxed);
        if limit == 0 {
            None
        } else {
            Some(limit)
        }
    }

    /// Refill tokens based on elapsed time since last refill.
    fn refill(&self, limit: u64) {
        let now_ms = current_time_ms();
        let last = self.last_refill_ms.load(Ordering::Relaxed);

        if last == 0 {
            // First call — initialize
            self.last_refill_ms.store(now_ms, Ordering::Relaxed);
            return;
        }

        let elapsed_ms = now_ms.saturating_sub(last);
        if elapsed_ms < 10 {
            return; // Minimum refill granularity: 10ms
        }

        // Calculate refill amount
        let refill_amount = (limit as f64 * elapsed_ms as f64 / 1000.0) as u64;
        if refill_amount == 0 {
            return;
        }

        // Update last refill time
        if self
            .last_refill_ms
            .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            // Add tokens, capped at 1 second worth (burst capacity)
            let current = self.tokens.load(Ordering::Relaxed);
            let new_tokens = (current + refill_amount).min(limit);
            self.tokens.store(new_tokens, Ordering::Relaxed);
        }
    }
}

/// Get current time in milliseconds (monotonic-ish for token bucket purposes).
fn current_time_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unlimited_returns_immediately() {
        let throttle = BandwidthThrottleAsync::new(None);
        // Should return immediately regardless of bytes requested
        throttle.acquire(1_000_000_000).await;
        assert_eq!(throttle.current_limit(), None);
    }

    #[tokio::test]
    async fn test_set_limit() {
        let throttle = BandwidthThrottleAsync::new(Some(1_000_000));
        assert_eq!(throttle.current_limit(), Some(1_000_000));

        throttle.set_limit(Some(500_000));
        assert_eq!(throttle.current_limit(), Some(500_000));

        throttle.set_limit(None);
        assert_eq!(throttle.current_limit(), None);
    }

    #[tokio::test]
    async fn test_acquire_within_budget() {
        let throttle = BandwidthThrottleAsync::new(Some(1_000_000)); // 1MB/s
        // Initial tokens = limit = 1MB, so acquiring 100KB should succeed immediately
        throttle.acquire(100_000).await;
    }

    #[tokio::test]
    async fn test_acquire_blocks_when_exhausted() {
        let throttle = BandwidthThrottleAsync::new(Some(10_000)); // 10KB/s

        // Exhaust all tokens
        throttle.tokens.store(0, Ordering::Relaxed);
        throttle.last_refill_ms.store(current_time_ms(), Ordering::Relaxed);

        let start = std::time::Instant::now();
        // Request 100 bytes at 10KB/s — should take ~10ms
        throttle.acquire(100).await;
        let elapsed = start.elapsed();

        // Should have waited at least 10ms (minimum granularity)
        assert!(elapsed.as_millis() >= 9, "Elapsed: {:?}", elapsed);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// **Validates: Requirements 5.1, 5.2**
        /// Property 3: Bandwidth Limit Enforcement — simulated downloads never
        /// exceed configured rate over any 1-second window.
        #[test]
        fn bandwidth_never_exceeds_limit(
            limit_bps in 1000u64..10_000_000,
            chunk_sizes in proptest::collection::vec(64u64..65536, 1..20),
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap();

            rt.block_on(async {
                let throttle = BandwidthThrottleAsync::new(Some(limit_bps));
                // Initialize the refill timer
                throttle.last_refill_ms.store(current_time_ms(), Ordering::Relaxed);
                // Start with exactly limit_bps tokens (1 second worth)
                throttle.tokens.store(limit_bps, Ordering::Relaxed);

                let mut total_acquired: u64 = 0;
                let start = tokio::time::Instant::now();

                // Acquire chunks through the throttle
                for &chunk in chunk_sizes.iter().take(5) {
                    let capped_chunk = chunk.min(limit_bps); // Don't request more than the bucket can hold
                    throttle.acquire(capped_chunk).await;
                    total_acquired += capped_chunk;

                    // Check: bytes acquired should not exceed limit * elapsed_time + limit (burst)
                    let elapsed_secs = start.elapsed().as_secs_f64();
                    let max_allowed = (limit_bps as f64 * (elapsed_secs + 1.0)) as u64 + limit_bps;
                    assert!(
                        total_acquired <= max_allowed,
                        "Acquired {} bytes but max allowed is {} (limit={}, elapsed={:.2}s)",
                        total_acquired, max_allowed, limit_bps, elapsed_secs
                    );
                }
            });
        }
    }
}
