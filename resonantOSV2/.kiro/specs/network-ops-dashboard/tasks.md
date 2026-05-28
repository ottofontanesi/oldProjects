# Tasks: Network Ops Dashboard (Phase 12)

## Task Instructions
- Test: Vitest 3.2 + fast-check (TS)
- Frontend: React + TypeScript in `src/components/dashboard/`
- Depends on Phase 9A/9B (backend APIs), Phase 10 (topology), Phase 11 (split inference status)
- Use existing project UI patterns and component library

## Tasks

- [x] 1. Dashboard Infrastructure
  - [x] 1.1 Create `src/components/dashboard/` directory structure: components, hooks, types, utils
  - [x] 1.2 Create `src/components/dashboard/types/dashboard.ts`: all TypeScript interfaces (NetworkState, NodeInfo, PlacementPlan, UtilityScores, DownloadProgress, PrefetchActivity, etc.)
  - [x] 1.3 Create `src/components/dashboard/hooks/useNetworkPolling.ts`: polling hook with 5s fast poll (state, downloads, prefetch) and 30s slow poll (topology, transport health)
  - [x] 1.4 Create `src/components/dashboard/hooks/useDashboardState.ts`: centralized state management combining all polled data
  - [x] 1.5 Create `src/components/dashboard/hooks/usePreferences.ts`: preference management with optimistic updates and re-optimization trigger
  - [x] 1.6 Create `src/components/dashboard/utils/formatters.ts`: number formatting (bytes, percentages, durations), time formatting (relative, absolute)
  - [x] 1.7 Create `src/components/dashboard/utils/colors.ts`: transport colors, model family colors, status colors
  - [x] 1.8 Write tests: polling hook calls correct Tauri commands at correct intervals; state updates correctly on data arrival; stale detection fires after 15s

- [x] 2. Network Topology View
  - [x] 2.1 Create `src/components/dashboard/TopologyView.tsx`: force-directed graph using d3-force or react-force-graph
  - [x] 2.2 Implement node rendering: device-type icons (desktop/laptop/server/phone), online/offline status dot, utilization ring
  - [x] 2.3 Implement connection rendering: colored by transport (LAN=green, WireGuard=blue, Reticulum=orange), thickness by bandwidth, dashed if degraded
  - [x] 2.4 Implement node click interaction: expand detail panel showing hardware, utilization, models hosted, incentive explanation
  - [x] 2.5 Implement auto-layout with manual drag-to-reposition (persist positions to localStorage)
  - [x] 2.6 Implement zoom and pan controls
  - [x] 2.7 Write tests: renders correct number of nodes; connections match topology data; click expands detail panel; handles 100 nodes without lag (performance test)

- [x] 3. Utility Metrics Panel
  - [x] 3.1 Create `src/components/dashboard/MetricsPanel.tsx`: container for utility gauges
  - [x] 3.2 Implement circular gauge component: 0-100% with color coding (green >70%, yellow 40-70%, red <40%)
  - [x] 3.3 Implement four gauges: Quality, Speed, Mass, Total (weighted combination)
  - [x] 3.4 Implement sparkline component: 24h trend line (288 data points at 5-min resolution)
  - [x] 3.5 Implement tooltip: hover shows exact value, weight, and explanation of what the metric means
  - [x] 3.6 Write tests: gauges render correct percentages; color thresholds correct; sparkline handles missing data gracefully

- [x] 4. Model Placement Panel
  - [x] 4.1 Create `src/components/dashboard/ModelPlacementPanel.tsx`: table/card view of loaded models
  - [x] 4.2 Implement model card: model name, parameter count, node assignment, protocol badge (single/tensor/pipeline), tok/s, utilization bar
  - [x] 4.3 Implement split model indicator: visual connection line between nodes hosting parts of a split model
  - [x] 4.4 Implement color coding by model family
  - [x] 4.5 Implement sort/filter: by model size, node, protocol, utilization
  - [x] 4.6 Write tests: all models from plan displayed; split models show correct node connections; sort/filter work correctly

- [x] 5. Transport Health Panel
  - [x] 5.1 Create `src/components/dashboard/TransportHealthPanel.tsx`: per-transport status display
  - [x] 5.2 Implement transport status badges: green/yellow/red per transport (LAN, Reticulum, WireGuard)
  - [x] 5.3 Implement latency matrix: grid showing RTT between all node pairs, color-coded by threshold
  - [x] 5.4 Implement failover indicator: show which paths are currently failed over with reason
  - [x] 5.5 Write tests: status badges reflect health data; latency matrix renders correct values; failover status displayed

- [x] 6. Node Contribution Panel
  - [x] 6.1 Create `src/components/dashboard/NodeContributionPanel.tsx`: per-node table
  - [x] 6.2 Implement node row: hostname, device type icon, models hosted count, CPU/GPU/RAM utilization bars, stability score, incentive status
  - [x] 6.3 Implement expandable row: detailed hardware info, historical utilization chart (last 1h)
  - [x] 6.4 Implement mesh economics (conditional): contribution balance, reputation score, free-rider status badge
  - [x] 6.5 Write tests: all nodes displayed; utilization bars reflect correct percentages; expandable row shows details

- [x] 7. Download and Prefetch Panels
  - [x] 7.1 Create `src/components/dashboard/DownloadPanel.tsx`: active downloads list
  - [x] 7.2 Implement download row: model name, target node, source, progress bar with percentage, speed (MB/s), ETA, priority badge
  - [x] 7.3 Implement cancel button for prefetch/background priority downloads
  - [x] 7.4 Create `src/components/dashboard/PrefetchPanel.tsx`: prefetch predictions list
  - [x] 7.5 Implement prefetch row: model name, predicted time, confidence percentage, status badge (pending/loading/loaded/cancelled/wrong)
  - [x] 7.6 Implement prefetch accuracy metric display
  - [x] 7.7 Write tests: progress bars update correctly; cancel button calls correct Tauri command; prefetch statuses render correctly

- [x] 8. Controls Panel
  - [x] 8.1 Create `src/components/dashboard/ControlsPanel.tsx`: user preference controls
  - [x] 8.2 Implement weight sliders: Quality/Speed/Mass with auto-normalization (sum to 1.0)
  - [x] 8.3 Implement model family preference editor: add/remove families with weight boost slider
  - [x] 8.4 Implement model veto list: add/remove vetoed models from dropdown
  - [x] 8.5 Implement "Re-optimize Now" button with loading state
  - [x] 8.6 Implement optimization interval selector (1/5/15/30 min)
  - [x] 8.7 Implement "Apply" button that saves preferences and triggers re-optimization
  - [x] 8.8 Write tests: slider normalization always sums to 1.0; re-optimize button calls trigger_optimization; preferences saved on apply

- [x] 9. Main Dashboard Container
  - [x] 9.1 Create `src/components/dashboard/NetworkDashboard.tsx`: main container composing all panels
  - [x] 9.2 Implement responsive layout: panels arranged in grid, responsive to screen size
  - [x] 9.3 Implement status bar: connection status, last update time, stale data indicator
  - [x] 9.4 Implement error boundaries: each panel wrapped, crashes isolated
  - [x] 9.5 Implement dark mode support: respect ResonantOS theme
  - [x] 9.6 Implement loading states: skeleton UI while first data loads
  - [x] 9.7 Implement graceful degradation: optimizer offline banner, stale data indicators, retry buttons
  - [x] 9.8 Write tests: dashboard renders without crash with null data; error in one panel doesn't crash others; stale indicator shows after 15s; dark mode applies correctly

- [x] 10. Accessibility and Polish
  - [x] 10.1 Add ARIA labels to all interactive elements (gauges, buttons, sliders)
  - [x] 10.2 Implement keyboard navigation: tab through panels, enter to expand nodes
  - [x] 10.3 Add tooltips to all metrics explaining what they mean in plain language
  - [x] 10.4 Implement smooth animations for gauge updates and topology changes (300ms transitions)
  - [x] 10.5 Write accessibility tests: all interactive elements have labels; tab order is logical; screen reader can navigate all panels

- [x] 11. Debug Mode — Request Tracing (Panel A)
  - [x] 11.1 Implement trace_id generation and propagation: unique ID assigned at request entry, carried through router → transport → inference → response
  - [x] 11.2 Implement trace storage: last 100 traces stored in memory with full hop details
  - [x] 11.3 Implement Tauri command `get_request_traces(filter)`: returns traces with optional filtering by model, node, status, time range
  - [x] 11.4 Create `src/components/dashboard/debug/RequestTracePanel.tsx`: list of recent traces with expandable waterfall diagram
  - [x] 11.5 Implement waterfall visualization: horizontal bars showing network_transfer_ms + queue_wait_ms + compute_ms per hop, stacked
  - [x] 11.6 Implement split inference trace: show layer ranges per hop (node A: layers 0-15, node B: layers 16-31)
  - [x] 11.7 Implement transport annotation: show which transport (LAN/WireGuard/Reticulum) was used per hop and why
  - [x] 11.8 Write tests: trace_id propagates through full request path; waterfall renders correct timing; split inference hops shown correctly

- [x] 12. Debug Mode — Enhanced Model Distribution (Panel B)
  - [x] 12.1 Create `src/components/dashboard/debug/ModelHeatmapPanel.tsx`: request heatmap overlay on model placement
  - [x] 12.2 Implement request-count heatmap: color intensity per model per node based on request volume over configurable window
  - [x] 12.3 Implement split model layer visualization: visual diagram showing which layers are on which nodes
  - [x] 12.4 Implement exploration budget display: current exploration model, requests received, days until rotation
  - [x] 12.5 Write tests: heatmap intensity matches request counts; layer visualization matches placement plan

- [x] 13. Debug Mode — Per-Node Execution Metrics (Panel C)
  - [x] 13.1 Create `src/components/dashboard/debug/NodeExecutionPanel.tsx`: detailed per-node metrics
  - [x] 13.2 Implement actual tok/s tracking: measured from real inference completions (not estimates), updated per-request
  - [x] 13.3 Implement queue depth chart: line chart showing requests waiting per node over last 1 hour
  - [x] 13.4 Implement thermal state display: current temp, throttling indicator, historical chart
  - [x] 13.5 Implement memory pressure breakdown: stacked bar showing model_weights + kv_cache + buffers + free, with eviction rate indicator
  - [x] 13.6 Write tests: tok/s reflects actual measurements; queue depth chart updates in real-time; memory breakdown sums to total

- [x] 14. Debug Mode — Network Topology Debug (Panel D)
  - [x] 14.1 Create `src/components/dashboard/debug/TopologyDebugPanel.tsx`: advanced network view
  - [x] 14.2 Implement latency matrix: real-time grid of all node pairs, color-coded by threshold, updates every probe cycle
  - [x] 14.3 Implement bandwidth utilization overlay: show current throughput as % of capacity on each link
  - [x] 14.4 Implement failover highlighting: paths in failover state shown in red with reason tooltip and duration
  - [x] 14.5 Implement multi-hop route visualization: click any node pair to see the actual packet path (direct or via relays)
  - [x] 14.6 Implement cross-mesh graph: unified view of all meshes the user belongs to, showing inter-mesh node connections
  - [x] 14.7 Write tests: latency matrix matches transport metrics; failover status correctly highlighted; cross-mesh graph shows all memberships

- [x] 15. Debug Mode — Optimizer Transparency (Panel E)
  - [x] 15.1 Create `src/components/dashboard/debug/OptimizerDebugPanel.tsx`: optimizer decision visibility
  - [x] 15.2 Implement utility breakdown: show quality + speed + mass components separately with weights applied
  - [x] 15.3 Implement explain placement integration: click model → call `explain_placement` API → show scoring table for all candidate nodes
  - [x] 15.4 Implement constraint visualization: progress bars showing how close each constraint is to its limit (memory headroom, latency vs threshold, stability vs threshold)
  - [x] 15.5 Implement "What-if" simulation: input form for hypothetical node specs → call network simulator → show predicted plan changes
  - [x] 15.6 Implement exploration budget panel: current exploration model, confidence, request count, rotation schedule
  - [x] 15.7 Write tests: utility breakdown sums correctly; explain placement shows all candidates; constraint bars reflect actual values; what-if produces valid simulation results

- [x] 16. Debug Mode — Network Statistics (Panel F)
  - [x] 16.1 Create `src/components/dashboard/debug/NetworkStatsPanel.tsx`: comprehensive statistics
  - [x] 16.2 Implement aggregate metrics: total requests/min, total tok/s, total loaded params (billions), with real-time counters
  - [x] 16.3 Implement trend charts: quality/speed/mass over selectable time range (24h/7d/30d) with zoom
  - [x] 16.4 Implement parsimony score: count split vs single-node models, flag "unnecessarily split" models
  - [x] 16.5 Implement stability metrics: network uptime %, node churn rate, MTBF
  - [x] 16.6 Implement hop distance histogram: bar chart showing distribution of request hop counts
  - [x] 16.7 Implement hardware efficiency: actual_compute / theoretical_max as gauge with trend
  - [x] 16.8 Write tests: aggregate metrics match backend data; trends render correctly for all time ranges; parsimony correctly identifies unnecessary splits

- [x] 17. Debug Mode Infrastructure
  - [x] 17.1 Implement debug mode toggle: button in dashboard header, state persisted to localStorage
  - [x] 17.2 Implement conditional panel rendering: debug panels only mount when debug mode is active (no performance cost when hidden)
  - [x] 17.3 Implement debug-specific polling: debug panels poll at higher frequency (2s) only when visible
  - [x] 17.4 Implement Tauri commands for debug data: `get_request_traces`, `get_latency_matrix`, `get_node_execution_metrics`, `explain_placement`, `simulate_what_if`
  - [x] 17.5 Write tests: debug mode toggle works; panels don't render when hidden; debug polling stops when mode disabled
