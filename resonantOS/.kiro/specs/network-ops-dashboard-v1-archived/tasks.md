# Tasks: Network Ops Dashboard

## Phase 1: Metrics Aggregation Service

- [ ] 1.1 Create `src-tauri/src/network_metrics_service.rs` with struct definitions: TopologySnapshot, TopologyNode, TopologyConnection, LatencyMatrix, ModelDistribution, ExecutionLogEntry, CapacityMetrics, NetworkHealthScore, TimeSeriesQuery, TimeSeriesResult
- [ ] 1.2 Implement `initialize_metrics_db` creating all tables (metrics_1min, metrics_5min, metrics_1hour, latency_probes, execution_log, health_alerts) with indexes in `network_metrics.db`
- [ ] 1.3 Implement metrics collector: background task (every 60s) reading from Phase 7 hardware state, Phase 9 cluster nodes, Phase 10 mesh state — insert into metrics_1min
- [ ] 1.4 Implement latency prober: background task (every 30s) sending lightweight pings between all known node pairs, storing results in latency_probes table
- [ ] 1.5 Implement downsampler: background task (every 5 minutes) aggregating metrics_1min into metrics_5min (avg/min/max), and hourly into metrics_1hour
- [ ] 1.6 Implement retention enforcement: delete metrics_1min older than 30 days, metrics_5min older than 90 days, metrics_1hour older than 365 days; enforce 500MB total cap
- [ ] 1.7 Implement execution log writer: subscribe to Phase 9 workload completions and Phase 10 inference completions, insert into execution_log
- [ ] 1.8 Register IPC commands: netops_get_topology, netops_get_latency_matrix, netops_get_model_distribution, netops_get_execution_log, netops_get_capacity, netops_get_health_score, netops_query_timeseries, netops_export_metrics
- [ ] 1.9 Write unit tests for metrics collection, downsampling correctness, retention enforcement, storage cap

## Phase 2: Topology and Health Score

- [ ] 2.1 Implement `netops_get_topology`: assemble TopologySnapshot from Phase 9 Node Registry + Phase 10 Network Registry, classify connections by latency quality
- [ ] 2.2 Implement connection quality classification: excellent (<10ms), good (10-100ms), fair (100-500ms), poor (>500ms) based on latency probe history
- [ ] 2.3 Implement NetworkHealthScore computation: weighted average of (nodes_online_percent × 0.3 + latency_quality × 0.3 + qos_compliance × 0.25 + thermal_health × 0.15), scaled to 0-100
- [ ] 2.4 Implement health alerting: trigger alerts when utilization > 80% for 5min, node offline, latency anomaly (>2x baseline), thermal throttling
- [ ] 2.5 Implement scope filtering: "local" returns only Phase 9 cluster nodes, "mesh" returns Phase 10 nodes, "combined" returns both with scope tags
- [ ] 2.6 Write property-based tests (proptest) for Properties 1, 6: topology accuracy, health score determinism

## Phase 3: Model Distribution and Capacity

- [ ] 3.1 Implement `netops_get_model_distribution`: query Phase 9 Model Registry + Phase 10 coordinator model index, assemble per-model instance list with node mapping
- [ ] 3.2 Implement capacity computation: sum Compute_Units across all online nodes, compute current demand from active workloads, derive utilization percentage
- [ ] 3.3 Implement demand forecast integration: read Phase 10 Scaling Engine's 24h forecast, include as ForecastPoints in CapacityMetrics
- [ ] 3.4 Implement per-resource aggregation: compute cluster-wide CPU/RAM/GPU/VRAM utilization averages
- [ ] 3.5 Implement scaling state reporting: current model tier, reason for tier selection, time until next scaling evaluation
- [ ] 3.6 Write unit tests for capacity computation, model distribution assembly, forecast integration

## Phase 4: Dashboard UI — Topology and Nodes

- [ ] 4.1 Create `src/modules/network-ops/TopologyView.tsx`: force-directed graph using a lightweight graph library (e.g., react-force-graph or custom canvas), nodes as circles with status colors, connections as lines with latency-based styling
- [ ] 4.2 Implement node interaction: click to expand Node_Card overlay, hover for quick stats tooltip, drag to reposition
- [ ] 4.3 Create `src/modules/network-ops/NodeCard.tsx`: hardware class icon, utilization gauges (CPU/RAM/GPU/VRAM as circular progress), loaded models list, active workloads count, thermal indicator, action buttons
- [ ] 4.4 Implement scope visual distinction: local cluster nodes with solid borders, mesh nodes with dashed borders, different background tints
- [ ] 4.5 Implement layout modes: auto (force-directed), manual (drag-and-save positions), list view (table format for many nodes)
- [ ] 4.6 Implement real-time updates: poll topology every 5s when visible, 60s when hidden, animate status transitions
- [ ] 4.7 Write Vitest component tests for topology rendering, node card display, scope filtering

## Phase 5: Dashboard UI — Latency and Execution

- [ ] 5.1 Create `src/modules/network-ops/LatencyHeatmap.tsx`: NxN matrix grid with color-coded cells (green→yellow→orange→red), node labels on axes, click-to-expand sparkline
- [ ] 5.2 Implement latency anomaly highlighting: cells with latency > 2x historical average get a warning border/icon
- [ ] 5.3 Create `src/modules/network-ops/ExecutionLog.tsx`: real-time scrolling table with columns (time, type, node, model, duration, status, reason), filterable by all columns
- [ ] 5.4 Implement execution statistics panel: workloads/hour chart, success rate gauge, node distribution pie chart
- [ ] 5.5 Implement routing decision display: expandable row showing why a specific node was chosen (model loaded, best-fit, affinity, etc.)
- [ ] 5.6 Write Vitest component tests for heatmap rendering, log filtering, statistics computation

## Phase 6: Dashboard UI — Capacity and History

- [ ] 6.1 Create `src/modules/network-ops/CapacityGauge.tsx`: circular gauge showing demand/capacity ratio with color zones, per-resource sub-gauges, forecast overlay line
- [ ] 6.2 Create `src/modules/network-ops/HistoricalCharts.tsx`: time-series line charts with configurable range (1h/6h/24h/7d/30d), metric selector, trend indicators (up/down/stable arrows)
- [ ] 6.3 Implement capacity planning recommendation: "At current growth, additional capacity needed in X days" computed from 30-day utilization trend
- [ ] 6.4 Implement metrics export: CSV and JSON download for selected time range and metrics
- [ ] 6.5 Create `src/modules/network-ops/ModelDistributionView.tsx`: model cards with instance list, VRAM bars per node, drag-to-load interaction, compatibility validation on drop
- [ ] 6.6 Write Vitest component tests for gauge rendering, chart data binding, export functionality

## Phase 7: Scope Switching and MeshChat Integration

- [ ] 7.1 Create `src/modules/network-ops/ScopeSwitcher.tsx`: tab bar with "Local Cluster" / "Mesh Network" / "Combined" options, persistent selection
- [ ] 7.2 Implement privacy filtering for mesh scope: non-owned nodes show only (status, model_tier, capacity_contribution), hide internal utilization and workload details
- [ ] 7.3 Implement mesh-specific panels: contribution/consumption balance, QoS metrics, fair share quota display, scaling tier indicator
- [ ] 7.4 Create MeshChat webview integration wrapper: package dashboard components as a standalone HTML/JS bundle loadable in MeshChat's webview panel
- [ ] 7.5 Implement MeshChat communication bridge: JSON message passing between MeshChat Python host and dashboard webview for Reticulum identity and peer data
- [ ] 7.6 Write integration tests: scope switching preserves state, privacy filtering correctness, MeshChat webview loading

## Phase 8: Performance and Behavioral Contracts

- [ ] 8.1 Implement adaptive polling: 5s refresh when dashboard visible, 60s when hidden/minimized, pause when system idle
- [ ] 8.2 Implement resource monitoring: track dashboard's own RAM usage, enforce 50MB limit by reducing data retention in memory
- [ ] 8.3 Implement rendering optimization: virtualized lists for execution log (only render visible rows), canvas-based topology for 50+ nodes, debounced updates
- [ ] 8.4 Create behavioral contract JSON files: contract-netops-topology-accurate, contract-netops-latency-valid, contract-netops-privacy-enforced, contract-netops-storage-bounded, contract-netops-resource-limits
- [ ] 8.5 Write property-based tests (proptest) for Properties 2, 3, 4, 5: latency validity, privacy enforcement, storage bounds, resource limits
- [ ] 8.6 Write performance tests: topology render < 16ms (60fps) with 50 nodes, metrics query < 100ms for 30-day range, dashboard RAM < 50MB under load
