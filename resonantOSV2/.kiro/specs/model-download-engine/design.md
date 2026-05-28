# Design Document: Model Download Engine

## Overview

The Model Download Engine extends the existing `network/download.rs` into a production-ready download system that fetches model weight files from HTTP(S) sources, verifies integrity via SHA256, supports resume after interruption, throttles bandwidth, and integrates with the unified scheduler's `PendingDownload` type. It lives in `src/resonantos-vnext/src-tauri/src/network/download/` as a subdirectory module replacing the current single-file stub.

### Design Principles

1. **Resume-first**: Every download persists enough state to resume from the last byte on restart.
2. **Priority-driven**: Higher-priority downloads preempt lower-priority ones when slots are full.
3. **Streaming integrity**: SHA256 is computed incrementally during download, not as a separate pass.
4. **Non-blocking**: All I/O is async (tokio); the download engine never blocks the main thread.
5. **Observable**: Progress events flow to the frontend via Tauri events at 500ms intervals.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      DownloadManager                             │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ PriorityQueue│  │ ActiveSlots  │  │ BandwidthThrottle    │  │
│  │ (queued      │  │ (up to 3     │  │ (token-bucket,       │  │
│  │  downloads)  │  │  concurrent) │  │  shared across all)  │  │
│  └──────┬───────┘  └──────┬───────┘  └──────────┬───────────┘  │
│         │                  │                     │              │
│         ▼                  ▼                     ▼              │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    DownloadTask                            │   │
│  │  • HTTP GET with Range header (resume)                    │   │
│  │  • Streaming SHA256 hash computation                      │   │
│  │  • Write to temp file, move on completion                 │   │
│  │  • Emit progress events every 500ms / 1MB                │   │
│  │  • Retry with exponential backoff on failure              │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ ResumeStore  │  │ IntegrityVer │  │ DiskSpaceChecker     │  │
│  │ (persist     │  │ (SHA256      │  │ (pre-check +         │  │
│  │  state to    │  │  streaming   │  │  monitor during)     │  │
│  │  SQLite)     │  │  + compare)  │  │                      │  │
│  └──────────────┘  └──────────────┘  └──────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
         │                                        │
         ▼                                        ▼
┌─────────────────┐                    ┌─────────────────────┐
│ Unified Scheduler│                    │ Tauri Event System  │
│ (submits         │                    │ (download-progress, │
│  PendingDownload,│                    │  download-complete, │
│  receives        │                    │  download-failed)   │
│  completion)     │                    │                     │
└─────────────────┘                    └─────────────────────┘
```

## Components and Interfaces

### DownloadManager

```rust
pub struct DownloadManager {
    config: DownloadConfig,
    queue: Arc<Mutex<BinaryHeap<QueuedDownload>>>,
    active: Arc<Mutex<HashMap<DownloadId, ActiveDownload>>>,
    throttle: Arc<BandwidthThrottle>,
    resume_store: Arc<ResumeStore>,
    http_client: reqwest::Client,
    event_tx: mpsc::Sender<DownloadEvent>,
    cancel_tokens: Arc<Mutex<HashMap<DownloadId, CancellationToken>>>,
}

impl DownloadManager {
    pub fn new(config: DownloadConfig, resume_store: Arc<ResumeStore>) -> Self;
    pub async fn submit(&self, pending: PendingDownload) -> Result<DownloadId, DownloadError>;
    pub async fn cancel(&self, download_id: DownloadId) -> Result<(), DownloadError>;
    pub async fn pause(&self, download_id: DownloadId) -> Result<(), DownloadError>;
    pub async fn resume(&self, download_id: DownloadId) -> Result<(), DownloadError>;
    pub fn set_bandwidth_limit(&self, bytes_per_sec: Option<u64>);
    pub fn set_max_concurrent(&self, max: u32);
    pub fn status(&self) -> DownloadManagerStatus;
    pub async fn shutdown(&self);
}
```

### DownloadConfig

```rust
pub struct DownloadConfig {
    pub max_concurrent: u32,              // Default: 3
    pub bandwidth_limit_bps: Option<u64>, // Default: None (unlimited)
    pub max_retries: u32,                 // Default: 4
    pub retry_backoff_base_ms: u64,       // Default: 1000
    pub temp_dir: PathBuf,                // Default: $APPDATA/resonantos-vnext/downloads/
    pub model_dir: PathBuf,               // Default: $APPDATA/resonantos-vnext/models/
    pub min_disk_space_mb: u64,           // Default: 1024 (1GB buffer)
    pub progress_interval_ms: u64,        // Default: 500
    pub connect_timeout_secs: u64,        // Default: 30
    pub max_redirects: u32,               // Default: 5
}
```

### DownloadTask (internal)

```rust
struct DownloadTask {
    id: DownloadId,
    pending: PendingDownload,
    state: DownloadState,
    hasher: Sha256,
    bytes_downloaded: u64,
    total_bytes: Option<u64>,
    temp_path: PathBuf,
    final_path: PathBuf,
    started_at: Instant,
    retries_remaining: u32,
    etag: Option<String>,
    last_modified: Option<String>,
}

enum DownloadState {
    Queued,
    Active,
    Paused { bytes_so_far: u64 },
    Completed { duration_ms: u64, hash: String },
    Failed { reason: String, retries_left: u32 },
    Cancelled,
}
```

### BandwidthThrottle

```rust
pub struct BandwidthThrottle {
    limit_bps: AtomicU64,        // 0 = unlimited
    tokens: AtomicU64,           // Available bytes this interval
    last_refill: AtomicU64,      // Timestamp of last refill
}

impl BandwidthThrottle {
    pub fn new(limit_bps: Option<u64>) -> Self;
    pub async fn acquire(&self, bytes: u64);  // Blocks until tokens available
    pub fn set_limit(&self, bps: Option<u64>);
    pub fn current_limit(&self) -> Option<u64>;
}
```

### ResumeStore

```rust
pub struct ResumeStore {
    db: Arc<Mutex<Connection>>,  // SQLite via rusqlite
}

impl ResumeStore {
    pub fn new(db_path: &Path) -> Result<Self, DownloadError>;
    pub fn save_state(&self, id: DownloadId, state: &ResumeState) -> Result<(), DownloadError>;
    pub fn load_state(&self, id: DownloadId) -> Result<Option<ResumeState>, DownloadError>;
    pub fn remove_state(&self, id: DownloadId) -> Result<(), DownloadError>;
    pub fn list_incomplete(&self) -> Result<Vec<(DownloadId, ResumeState)>, DownloadError>;
}

pub struct ResumeState {
    pub download_id: DownloadId,
    pub url: String,
    pub temp_path: PathBuf,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub expected_hash: Option<String>,
    pub priority: u8,
    pub model_id: String,
    pub target_node: NodeId,
    pub saved_at_ms: u64,
}
```

### IntegrityVerifier

```rust
pub struct IntegrityVerifier {
    hasher: Sha256,
}

impl IntegrityVerifier {
    pub fn new() -> Self;
    pub fn update(&mut self, data: &[u8]);
    pub fn finalize(self) -> String;  // Returns hex-encoded SHA256
    pub fn verify(computed: &str, expected: &str) -> bool;
}
```

### Events

```rust
pub enum DownloadEvent {
    Started { id: DownloadId, model_id: String, total_bytes: u64, priority: u8 },
    Progress { id: DownloadId, bytes_downloaded: u64, total_bytes: u64, speed_bps: u64, eta_secs: u64 },
    Completed { id: DownloadId, model_id: String, file_path: PathBuf, duration_ms: u64 },
    Failed { id: DownloadId, model_id: String, reason: String, retries_remaining: u32 },
    Cancelled { id: DownloadId },
    Paused { id: DownloadId, bytes_so_far: u64 },
    Resumed { id: DownloadId, bytes_so_far: u64 },
}
```

## Download Lifecycle

```
submit(PendingDownload)
    │
    ├─ Check disk space (reject if insufficient)
    ├─ Check resume store (resume if partial exists)
    ├─ Enqueue with priority
    │
    ▼
Queue Processing Loop
    │
    ├─ If active_count < max_concurrent:
    │     Dequeue highest-priority item → start task
    │
    ├─ If active_count >= max_concurrent AND new item has higher priority:
    │     Pause lowest-priority active → start new item
    │
    ▼
DownloadTask Execution
    │
    ├─ HTTP GET (with Range header if resuming)
    ├─ Read response in chunks (64KB)
    │     ├─ Acquire bandwidth tokens
    │     ├─ Write chunk to temp file
    │     ├─ Update SHA256 hasher
    │     ├─ Emit progress event (every 500ms or 1MB)
    │     └─ Check cancellation token
    │
    ├─ On completion:
    │     ├─ Verify SHA256 against expected
    │     ├─ Move temp file → final path
    │     ├─ Remove resume state
    │     ├─ Emit download-complete event
    │     └─ Notify scheduler
    │
    ├─ On network error:
    │     ├─ Save resume state
    │     ├─ Retry with backoff (1s, 2s, 4s, 8s)
    │     └─ After max retries: emit download-failed, notify scheduler
    │
    └─ On cancellation:
          ├─ Abort HTTP connection
          ├─ Delete temp file (unless resume requested)
          └─ Emit download-cancelled event
```

## Priority Preemption

When a higher-priority download arrives and all slots are full:

1. Find the lowest-priority active download
2. If new priority > lowest active priority:
   - Save resume state for the active download
   - Pause it (cancel the HTTP stream, keep temp file)
   - Move it back to the queue
   - Start the new higher-priority download
3. If new priority <= lowest active priority:
   - Just enqueue it (it will start when a slot opens)

## Correctness Properties

### Property 1: Resume Correctness
For any interrupted download, resuming SHALL produce the same final file as downloading from scratch (byte-for-byte identical, same SHA256).

### Property 2: Priority Ordering
The download engine SHALL never start a lower-priority download while a higher-priority download is queued and a slot is available.

### Property 3: Bandwidth Limit Enforcement
The aggregate download speed across all active downloads SHALL NOT exceed the configured bandwidth limit (measured over any 1-second window).

### Property 4: Integrity Guarantee
A download SHALL only be marked as completed if its SHA256 matches the expected hash (or no hash was provided).

### Property 5: Disk Space Safety
A download SHALL NOT be started if available disk space is less than file_size + 1GB buffer.

### Property 6: Concurrency Bound
The number of active (non-paused) downloads SHALL never exceed max_concurrent.

### Property 7: Cancellation Timeliness
A cancelled download SHALL stop writing to disk within 1 second of the cancel request.

## Error Handling

| Error | Recovery |
|-------|----------|
| Network timeout | Save resume state, retry with backoff |
| HTTP 429 | Respect Retry-After header, re-queue |
| HTTP 5xx | Retry up to 4 times with backoff |
| SHA256 mismatch | Delete file, retry download (up to 2 times) |
| Disk full | Pause all downloads, emit alert, wait for space |
| URL unreachable | Retry with backoff, fail after 4 attempts |
| TLS error | Fail immediately (no retry for cert issues) |

## Algorithm Detail: Queue Processing Loop

The queue processor is a long-running tokio task spawned at DownloadManager initialization:

```pseudocode
async fn queue_processor_loop(manager: Arc<DownloadManager>):
    loop:
        // Wait for a trigger: new submission, task completion, or cancellation
        wait_for_trigger()

        active_count = manager.active.lock().len()
        max = manager.config.max_concurrent

        // Fill available slots from the priority queue
        while active_count < max AND !manager.queue.is_empty():
            next = manager.queue.lock().pop()  // Highest priority
            spawn_download_task(manager, next)
            active_count += 1

        // Check for preemption opportunity
        if !manager.queue.is_empty():
            highest_queued_priority = manager.queue.lock().peek().priority
            lowest_active = find_lowest_priority_active(manager)

            if highest_queued_priority < lowest_active.priority:  // Lower number = higher priority
                // Preempt: pause lowest active, start highest queued
                pause_download(manager, lowest_active.id)
                next = manager.queue.lock().pop()
                spawn_download_task(manager, next)
```

Triggers that wake the processor:
- `submit()` called (new download added to queue)
- A download task completes (slot freed)
- A download task fails permanently (slot freed)
- A download is cancelled (slot freed)
- `set_max_concurrent()` called (capacity changed)

## Algorithm Detail: Token-Bucket Bandwidth Throttle

The throttle uses a classic token-bucket with these parameters:
- **Capacity**: `limit_bps` tokens (1 second worth)
- **Refill rate**: `limit_bps / 10` tokens every 100ms
- **Consumption**: each `acquire(n)` call consumes `n` tokens

```pseudocode
async fn acquire(self, bytes: u64):
    if self.limit_bps == 0:
        return  // Unlimited, no throttling

    loop:
        now = Instant::now()
        elapsed_since_refill = now - self.last_refill

        // Refill tokens based on elapsed time
        if elapsed_since_refill >= 100ms:
            refill_amount = (elapsed_since_refill.as_secs_f64() * self.limit_bps as f64) as u64
            self.tokens = min(self.tokens + refill_amount, self.limit_bps)  // Cap at 1s worth
            self.last_refill = now

        // Try to consume
        if self.tokens >= bytes:
            self.tokens -= bytes
            return

        // Not enough tokens — sleep until enough accumulate
        needed = bytes - self.tokens
        wait_time = Duration::from_secs_f64(needed as f64 / self.limit_bps as f64)
        tokio::time::sleep(wait_time).await
```

Key behaviors:
- When limit is changed at runtime, tokens are NOT reset (gradual transition)
- When limit is set to None (unlimited), all pending `acquire()` calls return immediately
- The throttle is shared across ALL concurrent downloads (global budget)
- Minimum sleep granularity: 10ms (to avoid busy-spinning)

## Algorithm Detail: HTTP Resume Negotiation

When resuming a partial download:

```pseudocode
async fn execute_with_resume(task: &mut DownloadTask, client: &Client):
    let mut request = client.get(&task.url)
        .header("User-Agent", "ResonantOS/1.0")

    if task.bytes_downloaded > 0:
        // Request remaining bytes
        request = request.header("Range", format!("bytes={}-", task.bytes_downloaded))

        // Conditional request to detect server-side changes
        if let Some(etag) = &task.etag:
            request = request.header("If-Range", etag)
        elif let Some(lm) = &task.last_modified:
            request = request.header("If-Range", lm)

    let response = request.send().await?

    match response.status():
        206 Partial Content:
            // Server supports range, append to existing file
            let file = OpenOptions::new().append(true).open(&task.temp_path)?
            stream_body(response, file, task)

        200 OK:
            // Server doesn't support range OR file changed
            // Check if Content-Length matches what we expect
            if task.bytes_downloaded > 0:
                // File changed on server — restart from scratch
                task.bytes_downloaded = 0
                task.hasher = Sha256::new()
            let file = File::create(&task.temp_path)?
            stream_body(response, file, task)

        416 Range Not Satisfiable:
            // Our offset is beyond the file (file shrunk?)
            // Restart from scratch
            task.bytes_downloaded = 0
            task.hasher = Sha256::new()
            retry_from_start(task, client)

        429 Too Many Requests:
            let retry_after = parse_retry_after(response.headers())
            sleep(retry_after).await
            return Err(DownloadError::RateLimited)

        status if status.is_server_error():
            return Err(DownloadError::ServerError(status))

        status:
            return Err(DownloadError::UnexpectedStatus(status))
```

ETag/Last-Modified validation:
- On first request: store `ETag` and `Last-Modified` from response headers
- On resume: send `If-Range` header with stored value
- If server returns 200 instead of 206: the file changed, restart from scratch
- If server returns 206: file unchanged, safe to append

## State Machine: Download Task Lifecycle

```
                    submit()
                       │
                       ▼
                  ┌─────────┐
                  │  Queued  │◄──────────────────────────────┐
                  └────┬─────┘                               │
                       │ slot available                       │
                       ▼                                     │
                  ┌─────────┐    preempted                   │
          ┌──────│  Active  │────────────────────────────────┘
          │      └────┬─────┘                    (re-queued)
          │           │
          │     ┌─────┼──────────┬───────────────┐
          │     │     │          │               │
          │     ▼     ▼          ▼               ▼
          │  success  error    cancel         pause
          │     │     │          │               │
          │     ▼     │          ▼               ▼
          │  ┌──────┐ │    ┌──────────┐    ┌─────────┐
          │  │Verify│ │    │Cancelled │    │ Paused  │
          │  └──┬───┘ │    └──────────┘    └────┬────┘
          │     │     │                         │
          │  ┌──┼──┐  │                    resume()
          │  │  │  │  │                         │
          │  ▼  ▼  │  ▼                         ▼
          │ ok fail │ retryable?            ┌─────────┐
          │  │  │   │  │    │               │  Queued │
          │  │  │   │  yes  no              └─────────┘
          │  │  │   │  │    │
          │  │  │   │  ▼    ▼
          │  │  └───┼─►Retry ┌──────────┐
          │  │      │  │     │  Failed  │
          │  │      │  │     │(permanent)│
          │  │      │  │     └──────────┘
          │  │      │  │
          │  │      │  └──► back to Active (after backoff)
          │  ▼      │
          │┌────────┴──┐
          ││ Completed  │
          │└────────────┘
          │
          └─── (on any state) ──► shutdown() ──► save resume state
```

## Integration Contract: Unified Scheduler

The DownloadManager integrates with the unified scheduler via a callback interface:

```rust
/// Callback trait for scheduler notifications.
pub trait DownloadCallback: Send + Sync {
    /// Called when a download completes successfully.
    /// The scheduler should load the model onto the target node.
    fn on_download_complete(&self, model_id: &str, target_node: NodeId, file_path: &Path);

    /// Called when a download fails permanently (all retries exhausted).
    /// The scheduler should adjust the placement plan.
    fn on_download_failed(&self, model_id: &str, target_node: NodeId, reason: &str);

    /// Called periodically with download engine status for capacity planning.
    fn on_status_update(&self, status: DownloadManagerStatus);
}

pub struct DownloadManagerStatus {
    pub active_count: u32,
    pub queued_count: u32,
    pub total_bytes_downloading: u64,
    pub total_bytes_queued: u64,
    pub aggregate_speed_bps: u64,
    pub error_rate: f64,
}
```

The scheduler submits downloads via:
```rust
// In the solver's plan execution phase:
for pending in plan.pending_downloads {
    download_manager.submit(pending).await?;
}
```

## Integration Contract: Tauri Events

Progress events are emitted to the frontend via Tauri's event system:

```rust
// In the download task, every 500ms or 1MB:
app_handle.emit_all("download-progress", ProgressPayload {
    download_id: id.to_string(),
    model_id: model_id.clone(),
    bytes_downloaded,
    total_bytes,
    speed_bps,
    eta_secs,
    percent: (bytes_downloaded as f64 / total_bytes as f64 * 100.0) as u8,
})?;
```

Frontend subscribes:
```typescript
import { listen } from '@tauri-apps/api/event';

listen('download-progress', (event) => {
    const { download_id, percent, speed_bps, eta_secs } = event.payload;
    updateProgressBar(download_id, percent);
});
```

## Resume Store Schema (SQLite)

```sql
CREATE TABLE IF NOT EXISTS download_resume (
    id              TEXT PRIMARY KEY,
    url             TEXT NOT NULL,
    temp_path       TEXT NOT NULL,
    bytes_downloaded INTEGER NOT NULL DEFAULT 0,
    total_bytes     INTEGER,
    etag            TEXT,
    last_modified   TEXT,
    expected_hash   TEXT,
    priority        INTEGER NOT NULL DEFAULT 128,
    model_id        TEXT NOT NULL,
    target_node     TEXT NOT NULL,
    resource_type   TEXT NOT NULL DEFAULT 'Model',
    saved_at_ms     INTEGER NOT NULL,
    created_at_ms   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_resume_priority ON download_resume(priority ASC);
CREATE INDEX IF NOT EXISTS idx_resume_model ON download_resume(model_id);
```

This table is managed by the existing schema migration system (`schema_migration.rs`). A new migration is added:

```rust
Migration {
    version: 7,
    name: "create_download_resume_table",
    sql: "CREATE TABLE IF NOT EXISTS download_resume (...)",
}
```

## Disk Space Management Detail

```pseudocode
fn check_disk_space(config: &DownloadConfig, file_size: u64) -> Result<(), DownloadError>:
    available = get_available_disk_space(config.temp_dir)?
    required = file_size + (config.min_disk_space_mb * 1024 * 1024)  // file + 1GB buffer

    if available < required:
        return Err(DownloadError::DiskFull {
            available_mb: available / (1024 * 1024),
            required_mb: required / (1024 * 1024),
        })

    Ok(())

// During download, check every 10 seconds:
async fn monitor_disk_space(config: &DownloadConfig, pause_signal: Sender<()>):
    loop:
        sleep(10s).await
        available = get_available_disk_space(config.temp_dir)?
        if available < 500 * 1024 * 1024:  // < 500MB remaining
            pause_signal.send(()).await  // Pause all active downloads
            emit_event(DownloadEvent::DiskSpaceLow { available_mb: available / (1024*1024) })
```

## Speed Calculation and ETA

Speed is computed using a sliding window of the last 5 seconds:

```rust
struct SpeedTracker {
    samples: VecDeque<(Instant, u64)>,  // (timestamp, cumulative_bytes)
    window: Duration,                    // 5 seconds
}

impl SpeedTracker {
    fn record(&mut self, bytes: u64) {
        let now = Instant::now();
        self.samples.push_back((now, bytes));
        // Remove samples older than window
        while self.samples.front().map(|(t, _)| now - *t > self.window).unwrap_or(false) {
            self.samples.pop_front();
        }
    }

    fn speed_bps(&self) -> u64 {
        if self.samples.len() < 2 { return 0; }
        let (first_time, first_bytes) = self.samples.front().unwrap();
        let (last_time, last_bytes) = self.samples.back().unwrap();
        let elapsed = (*last_time - *first_time).as_secs_f64();
        if elapsed < 0.1 { return 0; }
        ((last_bytes - first_bytes) as f64 / elapsed) as u64
    }

    fn eta_secs(&self, remaining_bytes: u64) -> u64 {
        let speed = self.speed_bps();
        if speed == 0 { return u64::MAX; }
        remaining_bytes / speed
    }
}
```

## Retry Backoff Schedule

```
Attempt 1: immediate
Attempt 2: wait 1s    (base_ms * 2^0)
Attempt 3: wait 2s    (base_ms * 2^1)
Attempt 4: wait 4s    (base_ms * 2^2)
Attempt 5: wait 8s    (base_ms * 2^3)
--- max_retries (4) exceeded → permanent failure ---
```

For SHA256 mismatch (integrity failure):
```
Attempt 1: delete file, re-download from scratch
Attempt 2: delete file, re-download from scratch
--- 2 integrity retries exceeded → permanent failure, emit diagnostic ---
```

For HTTP 429 (rate limited):
```
Use Retry-After header value (or default 60s if header missing)
Does NOT count against max_retries (server-imposed, not a failure)
```

## Testing Strategy

### Property-Based Tests (proptest)

| Property | Generator | Assertion |
|----------|-----------|-----------|
| P1: Resume Correctness | Random byte sequences, random split points | Hash of resumed file == hash of full file |
| P2: Priority Ordering | Random priority sequences | Dequeue order always matches priority |
| P3: Bandwidth Enforcement | Random chunk sizes, random limits | Aggregate throughput ≤ limit |
| P4: Integrity Guarantee | Random data + random expected hashes | Completion only when hash matches |
| P5: Disk Space Safety | Random file sizes, random available space | Rejection when space insufficient |
| P6: Concurrency Bound | Random submit/complete sequences | Active count never exceeds max |
| P7: Cancellation | Random cancel timing | No writes after cancel acknowledged |

### Unit Tests (Example-Based)

- Submit single download, verify events emitted in order (Started → Progress → Completed)
- Submit download with wrong hash, verify retry then failure
- Submit 5 downloads with max_concurrent=3, verify only 3 active at once
- Submit high-priority while full, verify preemption of lowest-priority
- Cancel active download, verify temp file deleted
- Pause and resume, verify bytes_downloaded preserved
- Restart app with incomplete downloads, verify auto-resume
- Set bandwidth limit, verify speed doesn't exceed it
- Disk space check rejects oversized download
- HTTP 429 response respected (waits Retry-After seconds)

### Integration Tests

- Full lifecycle: submit → download (mock HTTP server) → verify → complete → notify scheduler
- Resume after simulated crash: save state → reload → resume → complete
- Priority preemption: 3 active low-priority → submit high-priority → verify preemption

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `reqwest` | 0.12+ | HTTP client with async streaming, TLS, redirect following |
| `sha2` | 0.10+ | SHA256 streaming hash computation |
| `tokio` | 1.x | Async runtime, file I/O, timers, channels |
| `rusqlite` | 0.31+ | Resume state persistence (SQLite) |
| `tokio-util` | 0.7+ | CancellationToken for task cancellation |

## File Structure

```
src/resonantos-vnext/src-tauri/src/network/download/
├── mod.rs              # DownloadManager, public API, queue processor loop
├── config.rs           # DownloadConfig with defaults
├── task.rs             # DownloadTask execution logic (HTTP, streaming, retry)
├── throttle.rs         # BandwidthThrottle (token-bucket algorithm)
├── resume.rs           # ResumeStore (SQLite persistence, schema)
├── integrity.rs        # IntegrityVerifier (streaming SHA256)
├── events.rs           # DownloadEvent enum, DownloadId, progress emission
├── priority.rs         # Priority queue (BinaryHeap), preemption logic
├── speed.rs            # SpeedTracker (sliding window speed/ETA calculation)
├── disk.rs             # Disk space checking and monitoring
└── tests.rs            # Unit + property tests (all 7 properties)
```
