# Tasks: Reticulum Channel

## Phase 1: Add-on Manifest and Channel Registration

- [x] 1.1 Create `addons/reticulum-channel/` directory with `manifest.json` declaring runtimeType "channel-addon", category "channel", capabilities ["chat-interface", "notifications", "device-integration"], and localService with protocol "stdio-json-rpc"
- [x] 1.2 Create `addons/reticulum-channel/protocol/schema.json` with JSON Schema definitions for all request methods (start, stop, send_message, get_status, list_peers, ping) and notification methods (message_received, delivery_confirmed, link_established, link_lost, error)
- [x] 1.3 Implement channel registration in TypeScript: create ChannelDefinition with type "reticulum", unique channelId, owningAgentId set to Strategist
- [x] 1.4 Implement channel enable/disable: on enable create ConversationThreads for known peers, on disable/remove ensure zero impact on other channels
- [x] 1.5 Write unit tests for manifest validation (assertValidAddOnManifest), channel registration, enable/disable lifecycle

## Phase 2: Rust Host Service and Database

- [x] 2.1 Create `src-tauri/src/reticulum_channel_service.rs` with struct definitions: `ReticulumChannelConfig`, `SidecarHealthState`, `QueuedMessage`, `DeliveryState`, `BandwidthProfile`, `InterfaceStatus`, `ReticulumChannelState`
- [x] 2.2 Implement `initialize_reticulum_db` creating all tables (message_queue, delivery_states, known_peers, channel_config, sidecar_state) with indexes in `reticulum_channel.db`
- [x] 2.3 Implement message queue CRUD: `enqueue_message`, `dequeue_next_for_destination`, `mark_message_sent`, `mark_message_expired`, `query_pending_messages`, `persist_queue_to_db`, `load_queue_from_db`
- [x] 2.4 Implement delivery state CRUD: `create_delivery_state`, `confirm_delivery`, `mark_delivery_unconfirmed`, `mark_delivery_failed`, `check_delivery_timeouts`
- [x] 2.5 Implement known peers CRUD: `upsert_peer`, `query_peers`, `update_peer_link_status`, `get_peer_thread_id`
- [x] 2.6 Implement channel config CRUD: `read_config`, `update_config`, `read_bandwidth_profiles`
- [x] 2.7 Register IPC commands: reticulum_send_message, reticulum_get_status, reticulum_list_peers, reticulum_get_delivery_state, reticulum_get_queue_status
- [x] 2.8 Write Rust unit tests for schema initialization, queue FIFO ordering, delivery state transitions, message expiration

## Phase 3: Sidecar Lifecycle Management

- [x] 3.1 Implement `spawn_sidecar`: launch Python process with stdin/stdout pipes, send "start" request with config_path and identity_label, wait for response with destination_hash
- [x] 3.2 Implement `stop_sidecar`: send "stop" JSON-RPC request, wait up to 5 seconds for process exit, force terminate if needed
- [x] 3.3 Implement stdout reader task: `tokio::spawn` background task reading JSON-RPC responses and notifications from sidecar stdout, dispatching to appropriate handlers
- [x] 3.4 Implement health check task: `tokio::spawn` background task sending "ping" every 30 seconds, tracking consecutive failures, transitioning to "offline" after 3 failures
- [x] 3.5 Implement crash detection: monitor process exit, update SidecarHealthState to "crashed", emit shell notification
- [x] 3.6 Implement automatic restart with exponential backoff: initial 5s delay, double on repeated failure, cap at 60s, reset on successful start
- [x] 3.7 Write property-based tests (proptest) for Properties 1, 9, 10: health state transitions, ping failure detection, exponential backoff

## Phase 4: Python Sidecar Implementation

- [x] 4.1 Create `addons/reticulum-channel/sidecar/` directory with `main.py`, `requirements.txt` (rns, lxmf), and `__init__.py`
- [x] 4.2 Implement `ReticulumSidecar.start()`: initialize Reticulum from config, create/load identity, create destination, set up LXMF router, announce destination, return active interfaces
- [x] 4.3 Implement `ReticulumSidecar.send_message()`: encode content as LXMF message, establish link if needed, transmit, register delivery callback, return message_id
- [x] 4.4 Implement message chunking: when content exceeds transport MTU, split into multiple LXMF packets and transmit in sequence
- [x] 4.5 Implement `_on_message_received` callback: decode LXMF message, extract text content only (ignore attachments), emit "message_received" notification to stdout
- [x] 4.6 Implement `_on_delivery_confirmed` callback: emit "delivery_confirmed" notification with message_id and timestamp
- [x] 4.7 Implement `_process_stdin`: JSON-RPC request dispatcher, handle all 6 methods, emit valid JSON-RPC responses
- [x] 4.8 Implement LXMF display name: set source display name to configurable identity_label (default "ResonantOS")
- [x] 4.9 Implement transport auto-detection: read ~/.reticulum/config, initialize available interfaces, skip failed interfaces with logging
- [x] 4.10 Write Python unit tests for LXMF encoding/decoding, JSON-RPC request/response round-trip, message chunking

## Phase 5: Inbound Message Flow

- [x] 5.1 Implement inbound message handler in Rust: receive "message_received" notification, extract source_hash, source_name, content, timestamp
- [x] 5.2 Implement peer-to-thread mapping: look up or create ConversationThread for the source_hash, set title to source_name or destination hash
- [x] 5.3 Implement ConversationMessage insertion: role "user", author from source_name or hash, channelId set to Reticulum channel, content from LXMF text
- [x] 5.4 Implement Strategist response trigger: after inserting inbound message, trigger standard Strategist response flow on the Reticulum ConversationThread
- [x] 5.5 Implement text-only filtering: process only text content from LXMF, ignore binary attachments and extended fields
- [x] 5.6 Write property-based tests (proptest) for Property 2: inbound message insertion correctness

## Phase 6: Outbound Message Flow

- [x] 6.1 Implement outbound message routing: when Strategist produces response in Reticulum-channel thread, serialize and send via JSON-RPC "send_message" to sidecar
- [x] 6.2 Implement delivery state creation: on successful send, create DeliveryState with status "pending" and appropriate timeout
- [x] 6.3 Implement delivery confirmation handling: on "delivery_confirmed" notification, update DeliveryState to "complete", update ConversationMessage status
- [x] 6.4 Implement delivery timeout checking: background task checking pending deliveries past timeout, transitioning to "delivery-unconfirmed"
- [x] 6.5 Implement send failure handling: on JSON-RPC error response, update ConversationMessage status to "failed"
- [x] 6.6 Write property-based tests (proptest) for Properties 3, 6: outbound serialization round-trip, delivery state machine

## Phase 7: Message Queue and Retry

- [x] 7.1 Implement queue-on-unavailable: when sidecar reports link unavailable or is offline, enqueue message to persistent queue
- [x] 7.2 Implement queue persistence: write queue to reticulum_channel.db on enqueue, load on service start (survives restarts)
- [x] 7.3 Implement retry scheduler: background task every `retryIntervalSecs` (default 30s) attempting to send pending queued messages
- [x] 7.4 Implement FIFO transmission: when link becomes available, transmit queued messages in enqueue order
- [x] 7.5 Implement message expiration: mark messages older than `maxAgeHours` (default 24h) as "expired", notify user
- [x] 7.6 Implement priority ordering: user-originated messages dequeued before system notifications for low-bandwidth transports
- [x] 7.7 Write property-based tests (proptest) for Properties 4, 5, 8: FIFO ordering, persistence across restart, expiration enforcement

## Phase 8: Bandwidth-Aware Response Handling

- [x] 8.1 Implement BandwidthProfile configuration: default profiles for each transport type (LoRa: 500 bytes, TCP: 32000 bytes, serial: 500 bytes, I2P: 32000 bytes)
- [x] 8.2 Implement `shouldSummarize` logic: check response byte length against active transport's maxMessageBytes
- [x] 8.3 Implement summarization request: when response exceeds LoRa limit, request summarized version from provider constrained to fit within bandwidth limit
- [x] 8.4 Implement bandwidth profile settings UI: expose transport-specific size limits through add-on settings panel
- [x] 8.5 Write property-based tests (fast-check) for Property 7: summarization trigger correctness

## Phase 9: LXMF Interoperability and Transport Config

- [x] 9.1 Verify LXMF encoding compatibility: test outbound messages are decodable by MeshChat and Sideband
- [x] 9.2 Verify LXMF decoding compatibility: test inbound messages from MeshChat and Sideband are correctly decoded
- [x] 9.3 Implement LXMF propagation announcement: sidecar announces destination using standard LXMF propagation for peer discovery
- [x] 9.4 Implement transport configuration UI: allow user to enable/disable/modify transport interfaces through settings panel
- [x] 9.5 Implement transport failure resilience: skip failed interfaces on startup, continue on remaining, report status
- [x] 9.6 Write integration tests for LXMF round-trip with mock MeshChat/Sideband peers

## Phase 10: Graceful Degradation and Privacy

- [x] 10.1 Implement crash isolation: ensure sidecar crash does not propagate errors to host shell, other channels unaffected
- [x] 10.2 Implement diagnostic messaging: on sidecar start failure (missing Python, missing rns, invalid config), display clear diagnostic to user
- [x] 10.3 Implement recovery without intervention: on sidecar recovery, resume message processing, deliver queued messages, no user action needed
- [x] 10.4 Verify zero cloud transmission: audit all code paths to confirm no message content, metadata, or keys transmitted externally
- [x] 10.5 Verify offline operation: test full functionality on LoRa/serial transport with no internet connectivity
- [x] 10.6 Verify LLM token isolation: confirm channel operations (encoding, routing, queuing) consume zero LLM tokens; only AI response generation uses tokens
- [x] 10.7 Write property-based tests (proptest) for Properties 11, 12, 14: channel isolation, zero cloud, memory bound

## Phase 11: Behavioral Contracts and Integration Tests

- [x] 11.1 Create behavioral contract JSON files in `src/core/backtest-contracts/`: contract-reticulum-lifecycle-states, contract-reticulum-inbound-insertion, contract-reticulum-outbound-serialization
- [x] 11.2 Create behavioral contract JSON files: contract-reticulum-queue-fifo, contract-reticulum-delivery-ack, contract-reticulum-bandwidth-limit
- [x] 11.3 Create behavioral contract JSON files: contract-reticulum-channel-isolation, contract-reticulum-crash-isolation, contract-reticulum-transport-hotswap
- [x] 11.4 Write end-to-end integration test: full lifecycle from sidecar start -> inbound message -> Strategist response -> outbound send -> delivery confirmation
- [x] 11.5 Write integration test: queue persistence across sidecar restart, message expiration, retry behavior
- [x] 11.6 Write performance tests: JSON-RPC round-trip < 10ms, sidecar memory < 50MB with 10 peers, zero main-thread blocking during message processing
