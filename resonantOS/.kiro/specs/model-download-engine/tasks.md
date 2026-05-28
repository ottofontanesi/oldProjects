# Implementation Plan: Model Download Engine

## Overview

Replace the stub `network/download.rs` with a production-ready download engine implemented as a subdirectory module at `src/resonantos-vnext/src-tauri/src/network/download/`. The engine supports HTTP(S) downloads with resume, SHA256 integrity verification, bandwidth throttling, priority preemption, parallel downloads, and real-time progress events.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [x] 1. Module setup and core types
  - [x] 1.1 Convert `network/download.rs` to `network/download/` subdirectory module
    - Create `src/network/download/mod.rs` with `DownloadManager` struct skeleton
    - Create submodule files: `config.rs`, `task.rs`, `throttle.rs`, `resume.rs`, `integrity.rs`, `events.rs`, `priority.rs`
    - Update `network/mod.rs` to reference the subdirectory module
    - Move any existing download logic from the old file into the new module
    - _Requirements: 1.1, 10.1_

  - [x] 1.2 Implement `config.rs` with `DownloadConfig`
    - Define all config fields with defaults: max_concurrent (3), bandwidth_limit_bps (None), max_retries (4), retry_backoff_base_ms (1000), temp_dir, model_dir, min_disk_space_mb (1024), progress_interval_ms (500), connect_timeout_secs (30), max_redirects (5)
    - Implement `Default` trait
    - _Requirements: 5.1, 6.1, 9.4, 11.1_

  - [x] 1.3 Implement `events.rs` with `DownloadEvent` enum and `DownloadId` type
    - Define `DownloadId` as `uuid::Uuid`
    - Define `DownloadEvent` enum: Started, Progress, Completed, Failed, Cancelled, Paused, Resumed
    - Derive `Debug, Clone, Serialize, Deserialize` on all event types
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [x] 1.4 Implement `DownloadError` enum
    - Variants: NetworkError, DiskFull, IntegrityMismatch, Cancelled, InvalidUrl, TlsError, ServerError, Timeout, ResumeCorrupted
    - Implement `Display` and `Error` traits
    - _Requirements: 11.1, 11.2, 11.3_

  - [x] 1.5 Add Cargo dependencies if needed
    - Ensure `reqwest` with `stream` feature is in dependencies
    - Ensure `sha2` is in dependencies
    - Ensure `rusqlite` is available (for resume store)
    - _Requirements: 1.2, 4.1_

- [x] 2. Integrity verification
  - [x] 2.1 Implement `integrity.rs` with streaming SHA256
    - `IntegrityVerifier::new()` — create fresh SHA256 hasher
    - `IntegrityVerifier::update(&mut self, data: &[u8])` — feed bytes incrementally
    - `IntegrityVerifier::finalize(self) -> String` — return hex-encoded hash
    - `IntegrityVerifier::verify(computed: &str, expected: &str) -> bool` — constant-time comparison
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [x]* 2.2 Write property test for integrity verification
    - **Property 4: Integrity Guarantee** — for any byte sequence, streaming hash equals batch hash; mismatched expected hash always fails verification
    - _Validates: Requirements 4.1, 4.2_

- [x] 3. Bandwidth throttle
  - [x] 3.1 Implement `throttle.rs` with token-bucket rate limiter
    - `BandwidthThrottle::new(limit_bps: Option<u64>)` — create throttle (None = unlimited)
    - `BandwidthThrottle::acquire(&self, bytes: u64)` — async, blocks until tokens available
    - `BandwidthThrottle::set_limit(&self, bps: Option<u64>)` — change limit at runtime
    - Token refill: every 100ms, add `limit_bps / 10` tokens (capped at 1-second worth)
    - When unlimited: `acquire` returns immediately
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

  - [x]* 3.2 Write property test for bandwidth enforcement
    - **Property 3: Bandwidth Limit Enforcement** — simulated downloads never exceed configured rate over any 1-second window
    - _Validates: Requirements 5.1, 5.2_

- [x] 4. Resume store
  - [x] 4.1 Implement `resume.rs` with SQLite-backed persistence
    - `ResumeStore::new(db_path)` — open/create SQLite database with resume table
    - `ResumeStore::save_state(id, state)` — upsert resume state
    - `ResumeStore::load_state(id)` — retrieve resume state
    - `ResumeStore::remove_state(id)` — delete on completion
    - `ResumeStore::list_incomplete()` — return all saved states (for restart recovery)
    - Schema: `CREATE TABLE IF NOT EXISTS download_resume (id TEXT PRIMARY KEY, url TEXT, temp_path TEXT, bytes_downloaded INTEGER, total_bytes INTEGER, etag TEXT, last_modified TEXT, expected_hash TEXT, priority INTEGER, model_id TEXT, target_node TEXT, saved_at_ms INTEGER)`
    - _Requirements: 3.1, 3.2, 3.5, 3.6_

  - [x]* 4.2 Write property test for resume correctness
    - **Property 1: Resume Correctness** — save state then load returns identical state; remove then load returns None
    - _Validates: Requirements 3.1, 3.5_

- [x] 5. Checkpoint - Verify core components compile
  - Ensure all tests pass with `cargo test --lib --no-run`.

- [x] 6. Priority queue
  - [x] 6.1 Implement `priority.rs` with priority-ordered download queue
    - `PriorityQueue::new()` — create empty queue
    - `PriorityQueue::push(download)` — insert with priority ordering
    - `PriorityQueue::pop()` — remove and return highest-priority item
    - `PriorityQueue::peek()` — view highest-priority without removing
    - `PriorityQueue::remove(id)` — remove specific download by ID
    - `PriorityQueue::len()` and `is_empty()`
    - Use `BinaryHeap` with custom `Ord` implementation (lower priority number = higher priority)
    - _Requirements: 7.1, 7.2, 7.3_

  - [x]* 6.2 Write property test for priority ordering
    - **Property 2: Priority Ordering** — items always dequeued in priority order (lowest number first)
    - _Validates: Requirements 7.1, 7.2_

- [x] 7. Download task execution
  - [x] 7.1 Implement `task.rs` with single-download execution logic
    - `execute_download(config, pending, throttle, resume_state, cancel_token) -> Result<CompletedDownload, DownloadError>`
    - HTTP GET with `reqwest` streaming response
    - If resuming: send `Range: bytes={offset}-` header, validate ETag/Last-Modified
    - Read response body in 64KB chunks
    - For each chunk: acquire throttle tokens, write to temp file, update hasher, check cancel token
    - On completion: verify SHA256, move temp → final path
    - On error: save resume state, return error for retry logic
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 3.2, 3.3, 3.4, 4.1, 4.2, 4.3_

  - [x] 7.2 Implement retry logic with exponential backoff
    - On retryable error: wait `base_ms * 2^attempt` (1s, 2s, 4s, 8s)
    - On HTTP 429: use `Retry-After` header value
    - On HTTP 5xx: retry up to max_retries
    - On TLS/cert error: fail immediately (no retry)
    - On SHA256 mismatch: delete file, retry up to 2 times
    - _Requirements: 1.5, 4.4, 11.1, 11.2, 11.3_

  - [x] 7.3 Implement progress emission
    - Track bytes_downloaded, compute speed (bytes in last 1s window)
    - Emit progress event every `progress_interval_ms` (500ms) or every 1MB
    - Compute ETA: `remaining_bytes / speed_bps`
    - _Requirements: 2.1, 2.2_

- [x] 8. DownloadManager orchestration
  - [x] 8.1 Implement `mod.rs` with DownloadManager public API
    - `submit(pending)` — validate disk space, check resume store, enqueue
    - `cancel(id)` — trigger cancellation token, delete temp file, emit event
    - `pause(id)` — save resume state, cancel HTTP stream, keep temp file
    - `resume(id)` — re-submit with resume state
    - `status()` — return active count, queued count, total bytes, error rate
    - `shutdown()` — cancel all active, save all resume states
    - _Requirements: 1.1, 8.1, 8.2, 8.3, 8.4, 10.1, 10.4_

  - [x] 8.2 Implement queue processing loop
    - Spawn a tokio task that monitors the queue
    - When active_count < max_concurrent: dequeue and start
    - When higher-priority arrives and slots full: preempt lowest-priority active
    - On task completion: start next queued item
    - _Requirements: 6.1, 6.2, 6.3, 7.2, 7.3, 7.4_

  - [x] 8.3 Implement disk space checking
    - Before starting: verify available space > file_size + min_disk_space_mb
    - During download: monitor every 10 seconds, pause if < 500MB remaining
    - _Requirements: 9.1, 9.2, 9.3_

  - [x] 8.4 Implement scheduler integration
    - Accept `PendingDownload` from unified scheduler
    - On completion: notify scheduler callback (model ready to load)
    - On permanent failure: notify scheduler (plan needs adjustment)
    - Report active/queued stats for capacity planning
    - _Requirements: 10.1, 10.2, 10.3, 10.4_

  - [x]* 8.5 Write property tests for concurrency and preemption
    - **Property 6: Concurrency Bound** — active downloads never exceed max_concurrent
    - **Property 5: Disk Space Safety** — downloads rejected when space insufficient
    - _Validates: Requirements 6.1, 9.1_

- [x] 9. Startup recovery
  - [x] 9.1 Implement automatic resume on application restart
    - On DownloadManager initialization: call `resume_store.list_incomplete()`
    - For each incomplete download: re-submit with saved resume state
    - Maintain original priority ordering
    - _Requirements: 3.6_

- [x] 10. Final checkpoint
  - Ensure all tests pass with `cargo test --lib --no-run`.
  - Verify integration with unified scheduler's PendingDownload type.

## Notes

- Tasks marked with `*` are optional property tests
- The download engine reuses the existing `PendingDownload` type from `network/solver_contention.rs`
- Temp files use `.part` extension during download, renamed on completion
- The resume SQLite table is managed by the existing schema migration system
- All async operations use tokio; no blocking I/O on the main thread
