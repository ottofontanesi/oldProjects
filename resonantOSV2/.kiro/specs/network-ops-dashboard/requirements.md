# Requirements: Network Ops Dashboard (Phase 12)

## Overview

The Network Ops Dashboard is a React-based UI panel within ResonantOS that provides real-time visibility into the local network and mesh state: network topology, model placement, utility metrics, transport health, per-node contribution, download progress, prefetch activity, and user controls for preferences and manual re-optimization.

It polls backend state every 5 seconds and degrades gracefully when the optimizer is unavailable.

## User Stories

### US-1: Network Overview
As a user with a multi-machine setup, I want a single dashboard showing all my nodes, which models are loaded where, and how well the network is performing, so I can understand my AI infrastructure at a glance.

### US-2: Performance Monitoring
As a user, I want to see real-time utility metrics (quality, speed, mass) as gauges with trend lines, so I can tell if my network is improving or degrading over time.

### US-3: Model Management
As a user, I want to see which models are loaded, their placement across nodes, download progress for pending models, and prefetch activity, so I know what's happening with my model inventory.

### US-4: Node Health
As a user, I want to see per-node status (online/offline, CPU/GPU/RAM utilization, stability score, models hosted), so I can identify problematic nodes.

### US-5: User Controls
As a user, I want sliders to adjust quality/speed/mass weights, model preferences, and a "Re-optimize Now" button, so I can influence the optimizer's decisions without editing config files.

### US-6: Mesh Economics (if mesh joined)
As a mesh participant, I want to see my contribution balance, reputation score, and per-node contribution leaderboard, so I understand my standing in the mesh.

## Functional Requirements

### FR-1: Network Topology View
- FR-1.1: Visual graph showing all nodes as icons (desktop/laptop/server/phone) with connections between them
- FR-1.2: Connection lines colored by transport type (LAN=green, WireGuard=blue, Reticulum=orange)
- FR-1.3: Connection line thickness indicates bandwidth; dashed lines indicate degraded paths
- FR-1.4: Node icons show online/offline status (green dot = online, red = offline, yellow = degraded)
- FR-1.5: Click node to expand details (hardware, utilization, models hosted, incentive explanation)
- FR-1.6: Auto-layout with manual drag-to-reposition (positions persisted)

### FR-2: Model Placement View
- FR-2.1: Table/card view of all loaded models with: model name, parameter count, node assignment, protocol (single/tensor/pipeline), tok/s, utilization
- FR-2.2: Split models shown with visual indicator connecting the nodes they span
- FR-2.3: Color coding by model family (Qwen=blue, Gemma=green, Llama=purple, etc.)
- FR-2.4: Sort/filter by: model size, node, protocol, utilization

### FR-3: Utility Metrics
- FR-3.1: Three gauge widgets showing Quality, Speed, Mass scores (0-100%)
- FR-3.2: Combined Total Utility gauge (weighted combination)
- FR-3.3: Trend sparklines showing last 24 hours of each metric (5-minute resolution)
- FR-3.4: Tooltip on hover showing exact values and weight configuration

### FR-4: Transport Health
- FR-4.1: Per-transport status indicators (LAN, Reticulum, WireGuard) with green/yellow/red
- FR-4.2: Latency matrix: grid showing RTT between all node pairs
- FR-4.3: Bandwidth indicators per path
- FR-4.4: Failover status: show if any paths are currently failed over

### FR-5: Per-Node Contribution
- FR-5.1: Table showing each node's: hostname, device type, models hosted, CPU/GPU/RAM utilization, stability score, incentive status
- FR-5.2: For mesh: contribution balance, reputation score, free-rider status
- FR-5.3: Expandable row with detailed hardware info and historical utilization chart

### FR-6: Download Progress
- FR-6.1: List of active downloads with: model name, target node, source, progress bar, speed, ETA
- FR-6.2: Priority indicator (critical/prefetch/background)
- FR-6.3: Cancel button for prefetch/background downloads

### FR-7: Prefetch Activity
- FR-7.1: List of prefetch predictions: model, predicted time, confidence, status (pending/loading/loaded/cancelled)
- FR-7.2: Prefetch accuracy metric (correct predictions / total)
- FR-7.3: Historical prefetch log (last 7 days)

### FR-8: User Controls
- FR-8.1: Quality/Speed/Mass weight sliders (sum to 1.0, auto-normalize)
- FR-8.2: Model family preference dropdown (add/remove preferences with weight boost)
- FR-8.3: Model veto list (add/remove vetoed models)
- FR-8.4: "Re-optimize Now" button (triggers immediate optimization cycle)
- FR-8.5: Optimization interval selector (1min, 5min, 15min, 30min)
- FR-8.6: Changes apply immediately and trigger re-optimization

### FR-9: Graceful Degradation
- FR-9.1: If optimizer is unavailable, show "Optimizer Offline" banner but continue showing last-known state
- FR-9.2: If a node is unreachable, show stale data with "Last seen: X minutes ago" indicator
- FR-9.3: If polling fails, show connection error with retry button
- FR-9.4: Dashboard never crashes — all data fetches wrapped in error boundaries

### FR-10: Debug Mode (toggle-able advanced view)
- FR-10.1: Debug mode toggle in dashboard header — reveals advanced panels hidden from regular users
- FR-10.2: Debug mode state persisted to localStorage (survives refresh)

### FR-11: Request Tracing (Debug Mode — Panel A)
- FR-11.1: Every inference request gets a unique trace_id that propagates through the entire system (router → transport → inference → response)
- FR-11.2: Show full request path as waterfall diagram: user → router decision → node selection → model inference → response
- FR-11.3: For split inference: show each hop with layer ranges (node A layers 0-15 → node B layers 16-31 → response)
- FR-11.4: Latency breakdown per hop: network_transfer_ms + queue_wait_ms + compute_ms
- FR-11.5: For mesh requests: show which transport was used per hop (LAN/WireGuard/Reticulum) and why that path was selected
- FR-11.6: Trace list: last 100 requests with filtering by model, node, status (success/error/timeout)
- FR-11.7: Click any trace to expand full waterfall detail

### FR-12: Enhanced Model Distribution Map (Debug Mode — Panel B)
- FR-12.1: Show which of YOUR requests hit which model instances (request heatmap overlay on model placement view)
- FR-12.2: Request count per model per node over time as heatmap (color intensity = request volume)
- FR-12.3: For split models: show layer distribution across nodes visually (layer 0-15 on node A, 16-31 on node B)
- FR-12.4: Show exploration budget status: which model is being explored, how many requests it has received

### FR-13: Per-Node Execution Metrics (Debug Mode — Panel C)
- FR-13.1: Token generation time per node: actual measured tok/s (not estimated), updated per-request
- FR-13.2: Queue depth over time: line chart showing requests waiting per node (last 1 hour)
- FR-13.3: Thermal state: current temperature, throttling indicator, historical thermal chart
- FR-13.4: Memory pressure: RAM/VRAM usage breakdown (model weights vs KV-cache vs buffers vs free), KV-cache eviction rate

### FR-14: Network Topology Debug (Debug Mode — Panel D)
- FR-14.1: Latency matrix: real-time grid of all node pairs with RTT values, color-coded (green <5ms, yellow 5-50ms, red >50ms)
- FR-14.2: Bandwidth utilization per link: show current throughput as percentage of measured capacity
- FR-14.3: Failover status: highlight paths currently in failover state with reason and time since failover
- FR-14.4: Multi-hop routes: for any node pair, show the actual path packets take (direct or via relay nodes)
- FR-14.5: Cross-mesh connectivity: if user is in multiple meshes, show unified graph of all meshes and their nodes with inter-mesh connections

### FR-15: Optimizer Decision Transparency (Debug Mode — Panel E)
- FR-15.1: Current utility scores with full breakdown: quality component + speed component + mass component + weights applied
- FR-15.2: Explain Placement integration: click any model to see why it's placed where it is (scoring breakdown per candidate node)
- FR-15.3: Constraint visualization: show which constraints are binding (close to limits) — memory headroom bars, latency vs threshold, stability vs threshold
- FR-15.4: "What-if" simulation: user inputs hypothetical node specs, system shows how the plan would change (uses Network Simulator from Phase 9A.5)
- FR-15.5: Exploration budget panel: which model is being explored, confidence level, requests received, days until rotation

### FR-16: Network Statistics (Debug Mode — Panel F)
- FR-16.1: Aggregate metrics: total requests/min, total tok/s across network, total loaded parameters (billions)
- FR-16.2: Per-metric trends: quality/speed/mass over 24h/7d/30d with selectable time range
- FR-16.3: Parsimony score: count of split models vs could-be-single-node models, with list of "unnecessarily split" models if any
- FR-16.4: Stability score: network-wide uptime percentage, node churn rate (joins/leaves per day), mean time between failures
- FR-16.5: Hop distance distribution: histogram showing how many hops requests travel (1-hop, 2-hop, 3-hop, etc.)
- FR-16.6: Hardware utilization efficiency: actual_compute_time / theoretical_max_compute_time as percentage
- FR-16.7: Network capacity summary: total RAM/VRAM available vs used, total models loadable vs loaded

### FR-10: Polling and Updates
- FR-10.1: Poll backend every 5 seconds for: node states, utility scores, download progress
- FR-10.2: Poll every 30 seconds for: topology changes, model placement changes
- FR-10.3: Immediate update on user action (preference change, re-optimize trigger)
- FR-10.4: Visual indicator when data is stale (>15 seconds since last successful poll)

## Non-Functional Requirements

### NFR-1: Performance
- NFR-1.1: Dashboard renders within 500ms of data arrival
- NFR-1.2: Polling does not impact inference performance (lightweight API calls)
- NFR-1.3: Smooth animations for gauge updates and topology changes
- NFR-1.4: Works with up to 100 nodes in topology view without lag

### NFR-2: Usability
- NFR-2.1: Responsive layout (works on 1080p and 4K displays)
- NFR-2.2: Dark mode support (matches ResonantOS theme)
- NFR-2.3: Keyboard accessible (tab navigation, screen reader labels)
- NFR-2.4: Tooltips on all metrics explaining what they mean

### NFR-3: Reliability
- NFR-3.1: Dashboard never crashes regardless of backend state
- NFR-3.2: Graceful handling of partial data (some nodes responding, others not)
- NFR-3.3: No memory leaks from polling intervals (proper cleanup on unmount)

## Correctness Properties

### Property 1: Data freshness
Displayed data SHALL be no older than 2x the polling interval (10 seconds for fast-poll data, 60 seconds for slow-poll data), or SHALL display a "stale" indicator.

### Property 2: Consistency
All metrics displayed at the same time SHALL come from the same backend snapshot. No mixing of data from different time points.

### Property 3: Control responsiveness
User preference changes SHALL trigger a re-optimization within 5 seconds and the dashboard SHALL reflect the new plan within 15 seconds.

### Property 4: Graceful degradation
Backend unavailability SHALL NOT cause dashboard crashes. Last-known-good data SHALL be displayed with appropriate staleness indicators.

### Property 5: Topology accuracy
The topology view SHALL show all nodes known to the registry. No phantom nodes (removed but still displayed) or missing nodes (present but not displayed).
