// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// Dashboard TypeScript interfaces

// ─── Core Types ──────────────────────────────────────────────────────────────

export type NodeId = string;
export type ModelId = string;
export type TransportId = string;

export type DeviceType = 'desktop' | 'laptop' | 'server' | 'phone' | 'tablet' | 'unknown';
export type TransportType = 'lan' | 'wireguard' | 'reticulum';
export type HealthStatus = 'green' | 'yellow' | 'red';
export type ParallelismProtocol = 'single' | 'tensor_parallel' | 'pipeline_parallel';
export type DownloadPriority = 'critical' | 'high' | 'normal' | 'prefetch' | 'background';
export type PrefetchStatus = 'pending' | 'loading' | 'loaded' | 'cancelled' | 'wrong';
export type FreeRiderStatus = 'good' | 'warning' | 'deprioritized' | 'excluded';

// ─── Network State ───────────────────────────────────────────────────────────

export interface NetworkState {
  nodes: NodeInfo[];
  connections: ConnectionInfo[];
  currentPlan: PlacementPlan | null;
  utilityScores: UtilityScores;
  downloads: DownloadProgress[];
  prefetchActivity: PrefetchActivity[];
  lastUpdated: string;
  optimizerOnline: boolean;
}

export interface NodeInfo {
  nodeId: NodeId;
  hostname: string;
  deviceType: DeviceType;
  isOnline: boolean;
  hardware: HardwareInfo;
  utilization: UtilizationInfo;
  modelsHosted: string[];
  stabilityScore: number;
  incentiveStatus?: IncentiveInfo;
  position?: { x: number; y: number };
}

export interface HardwareInfo {
  cpuName: string;
  ramTotalMb: number;
  gpuName: string | null;
  vramTotalMb: number | null;
  thermalState: ThermalState;
}

export interface ThermalState {
  temperatureC: number;
  isThrottling: boolean;
  fanSpeedPercent: number | null;
}

export interface UtilizationInfo {
  cpuPercent: number;
  ramUsedMb: number;
  ramPercent: number;
  gpuPercent: number | null;
  vramUsedMb: number | null;
  vramPercent: number | null;
}

export interface IncentiveInfo {
  contributionBalance: number;
  reputationScore: number;
  freeRiderStatus: FreeRiderStatus;
  consecutiveNegativeCycles: number;
}

// ─── Connections ─────────────────────────────────────────────────────────────

export interface ConnectionInfo {
  sourceNode: NodeId;
  targetNode: NodeId;
  transport: TransportType;
  latencyMs: number;
  bandwidthMbps: number;
  isHealthy: boolean;
  isFailedOver: boolean;
  failoverReason?: string;
}

// ─── Placement Plan ──────────────────────────────────────────────────────────

export interface PlacementPlan {
  planId: string;
  createdAt: string;
  cycleNumber: number;
  placements: ModelPlacement[];
  utilityScores: UtilityScores;
}

export interface ModelPlacement {
  modelId: ModelId;
  modelName: string;
  parameterCountB: number;
  assignedNodes: NodeId[];
  protocol: ParallelismProtocol;
  estimatedTokS: number;
  utilizationPercent: number;
  modelFamily: string;
  layerRanges?: LayerRange[];
}

export interface LayerRange {
  nodeId: NodeId;
  startLayer: number;
  endLayer: number;
}

export interface UtilityScores {
  total: number;
  quality: number;
  speed: number;
  mass: number;
  weights: { quality: number; speed: number; mass: number };
}

// ─── Downloads ───────────────────────────────────────────────────────────────

export interface DownloadProgress {
  downloadId: string;
  modelId: ModelId;
  modelName: string;
  targetNode: NodeId;
  source: string;
  progressPercent: number;
  speedMbps: number;
  etaSeconds: number | null;
  priority: DownloadPriority;
  totalSizeMb: number;
  downloadedMb: number;
}

// ─── Prefetch ────────────────────────────────────────────────────────────────

export interface PrefetchActivity {
  modelId: ModelId;
  modelName: string;
  predictedTime: string;
  confidencePercent: number;
  status: PrefetchStatus;
}

export interface PrefetchMetrics {
  totalPredictions: number;
  correctPredictions: number;
  accuracyPercent: number;
}

// ─── Transport Health ────────────────────────────────────────────────────────

export interface TransportHealth {
  transportId: TransportId;
  transportType: TransportType;
  status: HealthStatus;
  peersReachable: number;
  errorRatePercent: number;
  lastSuccessfulSendMs: number | null;
}

// ─── Debug Types ─────────────────────────────────────────────────────────────

export interface RequestTrace {
  traceId: string;
  timestamp: string;
  modelId: ModelId;
  totalDurationMs: number;
  hops: TraceHop[];
  status: 'success' | 'error' | 'timeout';
}

export interface TraceHop {
  nodeId: NodeId;
  hostname: string;
  transport: TransportType;
  networkTransferMs: number;
  queueWaitMs: number;
  computeMs: number;
  layerRange?: { start: number; end: number };
  transportReason?: string;
}

export interface LatencyMatrixEntry {
  sourceNode: NodeId;
  targetNode: NodeId;
  latencyMs: number;
  status: HealthStatus;
}

export interface NodeExecutionMetrics {
  nodeId: NodeId;
  actualTokS: number;
  queueDepth: number;
  queueDepthHistory: { timestamp: string; depth: number }[];
  thermalHistory: { timestamp: string; tempC: number }[];
  memoryBreakdown: MemoryBreakdown;
}

export interface MemoryBreakdown {
  modelWeightsMb: number;
  kvCacheMb: number;
  buffersMb: number;
  freeMb: number;
  totalMb: number;
  evictionRate: number;
}

export interface ExplainPlacementResult {
  modelId: ModelId;
  candidates: PlacementCandidate[];
  selectedNode: NodeId;
  selectionReason: string;
}

export interface PlacementCandidate {
  nodeId: NodeId;
  hostname: string;
  score: number;
  qualityComponent: number;
  speedComponent: number;
  capacityFit: number;
  constraints: string[];
}

export interface WhatIfResult {
  hypotheticalNodes: NodeInfo[];
  predictedPlan: PlacementPlan;
  utilityChange: number;
  modelsGained: string[];
  modelsLost: string[];
}

// ─── Preferences ─────────────────────────────────────────────────────────────

export interface DashboardPreferences {
  weights: { quality: number; speed: number; mass: number };
  familyBoosts: Record<string, number>;
  modelVetoes: string[];
  optimizationIntervalMin: number;
}

// ─── Sparkline Data ──────────────────────────────────────────────────────────

export interface SparklinePoint {
  timestamp: string;
  value: number;
}

export type SparklineData = SparklinePoint[];
