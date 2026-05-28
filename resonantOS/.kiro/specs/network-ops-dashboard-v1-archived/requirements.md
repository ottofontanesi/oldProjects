# Requirements Document

## Introduction

Network Ops Dashboard is Phase 11 of the ResonantOS vNext improvement plan. It delivers a unified visual monitoring interface for both the Local Cluster (Phase 9) and Mesh Compute Network (Phase 10). The dashboard provides real-time topology visualization, node health monitoring, model distribution mapping, latency heatmaps, agent execution tracking, and historical metrics — enabling users to understand at a glance whether their network is healthy, where workloads are running, and how resources are distributed.

The dashboard serves two audiences: the single user monitoring their local cluster (2-5 machines), and the mesh network participant monitoring their contribution, consumption, and network-wide health. Both views share common components but present different scopes of data.

## Glossary

- **Network_Topology_View**: A visual graph representation showing all nodes, their connections, and current status
- **Latency_Heatmap**: A color-coded matrix showing measured latency between all node pairs
- **Model_Distribution_Map**: A visualization showing which models are loaded on which nodes with VRAM/RAM consumption
- **Agent_Execution_Log**: A real-time feed of agent executions across the network with node placement and duration
- **Node_Card**: A detailed status panel for a single node showing hardware, utilization, models, and workloads
- **Network_Health_Score**: An aggregate 0-100 score representing overall network operational quality
- **Capacity_Gauge**: A visual indicator showing current demand vs total capacity with tier breakdown
- **Historical_Metrics_Store**: Time-series storage for all network metrics enabling trend analysis and capacity planning

## Requirements

### Requirement 1: Network Topology Visualization

**User Story:** As a user, I want a visual map of all my network nodes and their connections, so that I can see the network structure at a glance.

#### Acceptance Criteria

1. THE dashboard SHALL display a graph visualization showing all nodes (local cluster + mesh) as interactive elements with status-colored indicators (green=ready, yellow=busy, orange=degraded, red=offline)
2. THE dashboard SHALL draw connection lines between nodes with line thickness proportional to measured bandwidth and color indicating latency quality (green < 10ms, yellow 10-100ms, orange 100-500ms, red > 500ms)
3. THE dashboard SHALL distinguish between local cluster nodes (solid border) and mesh network nodes (dashed border) visually
4. THE dashboard SHALL update node status and connection quality in real-time (refresh interval ≤ 5 seconds)
5. THE dashboard SHALL support click-to-expand on any node to show its Node_Card with full details
6. THE dashboard SHALL support layout modes: automatic force-directed layout, manual drag positioning, and geographic layout (if location data available)

### Requirement 2: Node Health Cards

**User Story:** As a user, I want detailed health information for each node, so that I can diagnose issues and understand individual node capabilities.

#### Acceptance Criteria

1. EACH Node_Card SHALL display: node name, hardware class, CPU/RAM/GPU utilization gauges, thermal state indicator, loaded models list with VRAM consumption, active workloads count, uptime duration, and network latency to orchestrator
2. FOR local cluster nodes, THE Node_Card SHALL additionally display: placement strategy preference, affinity rules, and workload history (last 10 executions)
3. FOR mesh network nodes, THE Node_Card SHALL additionally display: contribution score, fair share quota remaining, model tier guarantee, and attestation success rate
4. THE Node_Card SHALL highlight any active adaptations (thermal throttling, memory pressure, degraded state) with explanation of the triggering condition
5. THE Node_Card SHALL provide action buttons: load/unload model, set affinity, remove from cluster (local), or adjust contribution mode (mesh)

### Requirement 3: Model Distribution Visualization

**User Story:** As a user, I want to see which models are running where across my network, so that I can understand resource allocation and optimize placement.

#### Acceptance Criteria

1. THE dashboard SHALL display a model distribution view showing: each model as a card with instances listed per node, VRAM/RAM consumption per instance, and estimated tokens/sec per instance
2. THE dashboard SHALL color-code model instances by compatibility class: native-gpu (green), offloaded (yellow), cpu-only (blue)
3. THE dashboard SHALL show total network capacity per model: "qwen-35b: 3 instances across 2 nodes, serving capacity: ~45 tokens/sec aggregate"
4. THE dashboard SHALL support drag-and-drop model placement: drag a model to a node to trigger a load request (with compatibility validation)
5. THE dashboard SHALL display the current scaling tier (heavy/medium/light) for mesh networks with a visual indicator of why that tier is active

### Requirement 4: Latency Heatmap

**User Story:** As a user, I want to see network latency between all node pairs, so that I can identify connectivity issues and optimize routing.

#### Acceptance Criteria

1. THE dashboard SHALL display a matrix heatmap with nodes on both axes and cell color representing measured round-trip latency between each pair
2. THE heatmap SHALL update latency measurements at 30-second intervals using lightweight ping probes between all node pairs
3. THE heatmap SHALL highlight anomalous latency (> 2x historical average for that pair) with a warning indicator
4. THE heatmap SHALL support click-on-cell to show latency history (sparkline of last 1 hour) for that node pair
5. THE dashboard SHALL compute and display a Network_Health_Score (0-100) based on: percentage of nodes online, average latency vs baseline, QoS violation rate, and thermal state distribution

### Requirement 5: Agent and Workload Execution Log

**User Story:** As a user, I want to see where my agents and workloads are executing across the network, so that I can understand the system's routing decisions.

#### Acceptance Criteria

1. THE dashboard SHALL display a real-time execution log showing: workload ID, type (inference/agent/training), assigned node, model used, duration, status (running/completed/failed), and placement reason
2. THE execution log SHALL support filtering by: node, workload type, model, status, and time range
3. THE execution log SHALL display routing decisions: why a specific node was chosen (model loaded, best-fit, affinity, proximity)
4. FOR mesh network workloads, THE log SHALL additionally show: requester identity (anonymized), attestation status, and contribution credit earned
5. THE dashboard SHALL display aggregate execution statistics: workloads per hour, average duration, success rate, and distribution across nodes (pie chart)

### Requirement 6: Capacity and Demand Gauges

**User Story:** As a user, I want to see current capacity vs demand in real-time, so that I know if my network is under pressure or has headroom.

#### Acceptance Criteria

1. THE dashboard SHALL display a Capacity_Gauge showing: total capacity (CU), current demand (CU), and utilization percentage with color zones (green < 50%, yellow 50-80%, red > 80%)
2. THE dashboard SHALL display per-resource gauges: aggregate CPU utilization, aggregate RAM utilization, aggregate GPU utilization, and aggregate VRAM utilization across all nodes
3. FOR mesh networks, THE dashboard SHALL display: fractional reserve ratio, registered vs active users, current model tier, and scaling decision countdown (time until next evaluation)
4. THE dashboard SHALL display a 24-hour demand forecast line overlaid on the capacity gauge, showing predicted demand vs available capacity
5. THE dashboard SHALL alert (visual + notification) when utilization exceeds 80% sustained for 5 minutes

### Requirement 7: Historical Metrics and Trends

**User Story:** As a user, I want historical data on network performance, so that I can plan capacity and identify degradation trends.

#### Acceptance Criteria

1. THE dashboard SHALL store time-series metrics in a local database: per-node utilization (1-minute resolution, 30-day retention), per-model throughput, latency measurements, workload counts, and scaling events
2. THE dashboard SHALL display configurable time-range charts: 1 hour, 6 hours, 24 hours, 7 days, 30 days
3. THE dashboard SHALL display trend indicators: utilization trending up/down/stable, latency trending, capacity headroom trending
4. THE dashboard SHALL support export of historical metrics as CSV or JSON for external analysis
5. THE dashboard SHALL display capacity planning recommendations: "At current growth rate, you'll need additional capacity in X days"

### Requirement 8: Dual-Scope View (Local + Mesh)

**User Story:** As a user, I want to monitor both my local cluster and the mesh network from one dashboard, so that I have a single pane of glass for all network operations.

#### Acceptance Criteria

1. THE dashboard SHALL support two view scopes: "Local Cluster" (showing only LAN nodes) and "Mesh Network" (showing all mesh participants), switchable via tabs
2. THE "Local Cluster" view SHALL show: all LAN nodes with full detail, local model distribution, local workload log, and local capacity gauges
3. THE "Mesh Network" view SHALL show: all mesh nodes (with reduced detail for non-owned nodes), network-wide capacity, contribution/consumption balance, QoS metrics, and scaling state
4. THE dashboard SHALL support a "Combined" view showing both scopes simultaneously with clear visual separation
5. FOR mesh network view, THE dashboard SHALL respect privacy: non-owned nodes show only public metrics (online status, model tier, capacity contribution) — not internal utilization or workload details

### Requirement 9: Performance and Resource Impact

**User Story:** As a user, I want the dashboard to be lightweight, so that monitoring doesn't impact the system being monitored.

#### Acceptance Criteria

1. THE dashboard SHALL consume less than 50MB RAM when active and less than 5MB when minimized/hidden
2. THE dashboard SHALL reduce polling frequency when not visible (from 5s to 60s) to minimize resource impact
3. THE dashboard SHALL NOT impact inference latency or workload execution — all metric collection is passive (reads existing data, no additional probes beyond latency pings)
4. THE Historical_Metrics_Store SHALL enforce storage limits: maximum 500MB for time-series data, automatic downsampling of old data (1-min → 5-min → 1-hour resolution)
5. THE dashboard SHALL render smoothly (60fps) with up to 50 nodes displayed simultaneously

### Requirement 10: Behavioral Contract Integration

**User Story:** As a developer, I want the dashboard to ship with behavioral contracts for correctness verification.

#### Acceptance Criteria

1. THE system SHALL register Behavioral_Contracts covering: topology view accurately reflects node registry state, latency heatmap values match actual probe measurements, and model distribution matches cluster model registry
2. THE system SHALL register Behavioral_Contracts covering: dashboard resource usage stays within limits (50MB RAM, no inference impact), historical metrics respect retention policies, and privacy is maintained for non-owned mesh nodes
3. WHEN a Behavioral_Contract fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report
