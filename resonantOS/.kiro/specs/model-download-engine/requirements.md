# Requirements Document

## Introduction

This document specifies the requirements for a production-ready model weight download engine. The current `network/download.rs` module provides basic download orchestration but lacks resume support, integrity verification, bandwidth throttling, and progress tracking. This feature extends it into a full download engine that integrates with the unified scheduler's `PendingDownload` type, supports parallel downloads with priority ordering, and provides real-time progress events to the frontend.

## Glossary

- **DownloadManager**: The central coordinator that manages all active and queued downloads, enforces priority ordering, and reports progress.
- **DownloadTask**: A single model weight download operation with its own progress, retry state, and integrity verification.
- **PendingDownload**: The existing type from the unified scheduler representing a model that needs to be downloaded to a specific node.
- **BandwidthThrottle**: A token-bucket rate limiter that caps download speed to avoid saturating the user's network.
- **IntegrityVerifier**: The component that computes SHA256 checksums on downloaded files and compares against expected hashes.
- **ResumeState**: Persisted metadata (bytes downloaded, temp file path, ETag) enabling download resumption after interruption.

## Requirements

### Requirement 1: Download Initiation

**User Story:** As a ResonantOS node, I want to initiate model weight downloads from HTTP URLs, so that models can be fetched from registries and stored locally.

#### Acceptance Criteria

1. WHEN a `PendingDownload` is submitted to the DownloadManager, THE DownloadManager SHALL create a DownloadTask and begin fetching the file via HTTP GET.
2. THE DownloadManager SHALL support HTTPS URLs with TLS 1.2+ certificate validation.
3. THE DownloadManager SHALL send a `User-Agent: ResonantOS/1.0` header with all requests.
4. THE DownloadManager SHALL follow HTTP redirects (up to 5 hops).
5. IF the URL is unreachable, THEN THE DownloadManager SHALL retry with exponential backoff (1s, 2s, 4s, 8s) up to 4 attempts.

### Requirement 2: Progress Tracking

**User Story:** As a ResonantOS user, I want to see real-time download progress, so that I know how long downloads will take and can monitor their status.

#### Acceptance Criteria

1. THE DownloadManager SHALL emit progress events every 500ms (or every 1MB, whichever comes first) containing: download_id, bytes_downloaded, total_bytes, speed_bytes_per_sec, eta_seconds.
2. THE DownloadManager SHALL emit progress events via Tauri's event system on channel `download-progress`.
3. WHEN a download starts, THE DownloadManager SHALL emit a `download-started` event with: download_id, model_id, total_bytes, priority.
4. WHEN a download completes, THE DownloadManager SHALL emit a `download-complete` event with: download_id, model_id, file_path, duration_ms.
5. WHEN a download fails, THE DownloadManager SHALL emit a `download-failed` event with: download_id, model_id, error_reason, retries_remaining.

### Requirement 3: Resume Support

**User Story:** As a ResonantOS user, I want interrupted downloads to resume from where they left off, so that large model files don't need to be re-downloaded from scratch.

#### Acceptance Criteria

1. THE DownloadManager SHALL persist ResumeState (bytes_downloaded, temp_file_path, etag, last_modified) to the persistence layer.
2. WHEN resuming a download, THE DownloadManager SHALL send an HTTP `Range: bytes={offset}-` header to request only the remaining bytes.
3. IF the server responds with 206 Partial Content, THEN THE DownloadManager SHALL append to the existing temp file.
4. IF the server responds with 200 OK (range not supported), THEN THE DownloadManager SHALL restart the download from the beginning.
5. IF the server's ETag or Last-Modified has changed since the partial download, THEN THE DownloadManager SHALL discard the partial file and restart.
6. THE DownloadManager SHALL automatically resume incomplete downloads on application restart.

### Requirement 4: SHA256 Integrity Verification

**User Story:** As a ResonantOS node, I want downloaded model files verified against known checksums, so that corrupted or tampered files are detected before use.

#### Acceptance Criteria

1. THE IntegrityVerifier SHALL compute SHA256 incrementally during download (streaming hash).
2. WHEN a download completes, THE IntegrityVerifier SHALL compare the computed hash against the expected hash from the model catalog.
3. IF the hash matches, THEN THE DownloadManager SHALL move the file from the temp location to the final model directory.
4. IF the hash does not match, THEN THE DownloadManager SHALL delete the downloaded file, log the mismatch, and retry the download (up to 2 retries).
5. IF no expected hash is provided, THEN THE DownloadManager SHALL skip verification and log a warning.

### Requirement 5: Bandwidth Throttling

**User Story:** As a ResonantOS user, I want to limit download bandwidth, so that model downloads don't saturate my network and interfere with other activities.

#### Acceptance Criteria

1. THE BandwidthThrottle SHALL support a configurable maximum download speed in bytes per second (default: unlimited).
2. THE BandwidthThrottle SHALL use a token-bucket algorithm with 1-second refill interval.
3. WHEN the throttle limit is reached, THE DownloadManager SHALL pause reading from the HTTP stream until tokens are available.
4. THE user SHALL be able to change the bandwidth limit at runtime without restarting downloads.
5. THE BandwidthThrottle SHALL apply globally across all concurrent downloads (shared budget).

### Requirement 6: Parallel Downloads

**User Story:** As a ResonantOS node, I want multiple models to download simultaneously, so that the system can prepare several models in parallel when bandwidth allows.

#### Acceptance Criteria

1. THE DownloadManager SHALL support a configurable maximum number of concurrent downloads (default: 3).
2. WHEN the concurrent limit is reached, THE DownloadManager SHALL queue additional downloads and start them as slots become available.
3. THE DownloadManager SHALL process the queue in priority order (highest priority first).
4. THE user SHALL be able to change the concurrency limit at runtime.

### Requirement 7: Priority Ordering

**User Story:** As a ResonantOS node, I want downloads prioritized by urgency, so that models needed for active workloads are fetched before speculative pre-fetches.

#### Acceptance Criteria

1. EACH PendingDownload SHALL have a priority field (u8, 0=highest, 255=lowest).
2. THE DownloadManager SHALL always start the highest-priority queued download when a slot opens.
3. IF a higher-priority download is submitted while all slots are full, THEN THE DownloadManager SHALL pause the lowest-priority active download and start the higher-priority one.
4. PAUSED downloads SHALL resume from their current offset when a slot becomes available.
5. THE DownloadManager SHALL re-evaluate priorities when the optimizer produces a new placement plan.

### Requirement 8: Cancellation

**User Story:** As a ResonantOS user, I want to cancel downloads in progress, so that I can free bandwidth or stop unwanted downloads.

#### Acceptance Criteria

1. WHEN a cancel request is received for a download_id, THE DownloadManager SHALL abort the HTTP connection and stop writing to disk.
2. THE DownloadManager SHALL delete the partial temp file on cancellation (unless resume is explicitly requested).
3. THE DownloadManager SHALL emit a `download-cancelled` event with the download_id.
4. CANCELLATION SHALL take effect within 1 second of the request.

### Requirement 9: Disk Space Management

**User Story:** As a ResonantOS node, I want the download engine to check available disk space before starting, so that downloads don't fail mid-way due to full disk.

#### Acceptance Criteria

1. BEFORE starting a download, THE DownloadManager SHALL verify that available disk space exceeds the file size plus a 1GB buffer.
2. IF insufficient disk space is detected, THEN THE DownloadManager SHALL reject the download with a clear error message.
3. THE DownloadManager SHALL monitor disk space during download and pause if available space drops below 500MB.
4. THE DownloadManager SHALL store temp files in a configurable directory (default: `$APPDATA/resonantos-vnext/downloads/`).

### Requirement 10: Integration with Unified Scheduler

**User Story:** As the unified scheduler, I want to submit download requests and receive completion notifications, so that placement plans can be executed.

#### Acceptance Criteria

1. THE DownloadManager SHALL accept `PendingDownload` structs from the scheduler as download requests.
2. WHEN a download completes successfully, THE DownloadManager SHALL notify the scheduler via callback so the model can be loaded.
3. WHEN a download fails permanently (all retries exhausted), THE DownloadManager SHALL notify the scheduler so the plan can be adjusted.
4. THE DownloadManager SHALL report active download count and total queued bytes to the scheduler for capacity planning.

### Requirement 11: Error Handling and Resilience

**User Story:** As a ResonantOS node, I want the download engine to handle network errors gracefully, so that transient issues don't permanently fail downloads.

#### Acceptance Criteria

1. IF a network error occurs mid-download, THEN THE DownloadManager SHALL save resume state and retry after exponential backoff.
2. IF the server returns HTTP 429 (Too Many Requests), THEN THE DownloadManager SHALL respect the `Retry-After` header.
3. IF the server returns HTTP 5xx, THEN THE DownloadManager SHALL retry up to 4 times with backoff.
4. THE DownloadManager SHALL track per-URL error history and deprioritize consistently failing sources.
5. THE DownloadManager SHALL expose a `health_status()` reporting: active downloads, queued count, total bytes downloaded, error rate.
