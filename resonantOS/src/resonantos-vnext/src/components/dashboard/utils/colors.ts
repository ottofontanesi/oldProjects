// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// Colors — transport, model family, status color mappings

import type { TransportType, HealthStatus, FreeRiderStatus, DownloadPriority } from '../types/dashboard';

// ─── Transport Colors ────────────────────────────────────────────────────────

export const TRANSPORT_COLORS: Record<TransportType, string> = {
  lan: '#22c55e',        // Green
  wireguard: '#3b82f6',  // Blue
  reticulum: '#f97316',  // Orange
};

export const TRANSPORT_LABELS: Record<TransportType, string> = {
  lan: 'LAN',
  wireguard: 'WireGuard',
  reticulum: 'Reticulum',
};

// ─── Status Colors ───────────────────────────────────────────────────────────

export const STATUS_COLORS: Record<HealthStatus, string> = {
  green: '#22c55e',
  yellow: '#eab308',
  red: '#ef4444',
};

export const STATUS_BG_COLORS: Record<HealthStatus, string> = {
  green: '#dcfce7',
  yellow: '#fef9c3',
  red: '#fee2e2',
};

// ─── Utility Gauge Colors ────────────────────────────────────────────────────

export function getGaugeColor(percent: number): string {
  if (percent >= 70) return '#22c55e'; // Green
  if (percent >= 40) return '#eab308'; // Yellow
  return '#ef4444';                     // Red
}

// ─── Model Family Colors ─────────────────────────────────────────────────────

const MODEL_FAMILY_COLORS: Record<string, string> = {
  qwen: '#8b5cf6',      // Purple
  llama: '#06b6d4',     // Cyan
  mistral: '#f59e0b',   // Amber
  gemma: '#10b981',     // Emerald
  phi: '#ec4899',       // Pink
  deepseek: '#6366f1',  // Indigo
  command: '#14b8a6',   // Teal
};

export function getModelFamilyColor(family: string): string {
  const normalized = family.toLowerCase();
  return MODEL_FAMILY_COLORS[normalized] ?? '#6b7280'; // Gray fallback
}

// ─── Free-Rider Status Colors ────────────────────────────────────────────────

export const FREE_RIDER_COLORS: Record<FreeRiderStatus, string> = {
  good: '#22c55e',
  warning: '#eab308',
  deprioritized: '#f97316',
  excluded: '#ef4444',
};

// ─── Download Priority Colors ────────────────────────────────────────────────

export const PRIORITY_COLORS: Record<DownloadPriority, string> = {
  critical: '#ef4444',
  high: '#f97316',
  normal: '#3b82f6',
  prefetch: '#8b5cf6',
  background: '#6b7280',
};

// ─── Device Type Icons ───────────────────────────────────────────────────────

export const DEVICE_ICONS: Record<string, string> = {
  desktop: '🖥️',
  laptop: '💻',
  server: '🖧',
  phone: '📱',
  tablet: '📱',
  unknown: '❓',
};
