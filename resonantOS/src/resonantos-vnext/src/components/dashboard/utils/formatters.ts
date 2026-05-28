// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// Formatters — number, byte, percentage, duration, time formatting

/**
 * Format bytes to human-readable string.
 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

/**
 * Format megabytes to human-readable string.
 */
export function formatMb(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`;
  return `${mb.toFixed(0)} MB`;
}

/**
 * Format percentage (0-100).
 */
export function formatPercent(value: number, decimals = 0): string {
  return `${value.toFixed(decimals)}%`;
}

/**
 * Format a fraction (0-1) as percentage.
 */
export function formatFraction(value: number, decimals = 0): string {
  return formatPercent(value * 100, decimals);
}

/**
 * Format duration in milliseconds to human-readable.
 */
export function formatDurationMs(ms: number): string {
  if (ms < 1) return '<1ms';
  if (ms < 1000) return `${ms.toFixed(0)}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  const minutes = Math.floor(ms / 60000);
  const seconds = Math.floor((ms % 60000) / 1000);
  return `${minutes}m ${seconds}s`;
}

/**
 * Format seconds to human-readable duration.
 */
export function formatDurationSec(seconds: number): string {
  if (seconds < 60) return `${seconds.toFixed(0)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s`;
  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  return `${hours}h ${mins}m`;
}

/**
 * Format speed in MB/s.
 */
export function formatSpeed(mbps: number): string {
  if (mbps >= 1000) return `${(mbps / 1000).toFixed(1)} GB/s`;
  if (mbps >= 1) return `${mbps.toFixed(1)} MB/s`;
  return `${(mbps * 1024).toFixed(0)} KB/s`;
}

/**
 * Format tokens per second.
 */
export function formatTokS(tokS: number): string {
  if (tokS >= 100) return `${tokS.toFixed(0)} tok/s`;
  if (tokS >= 10) return `${tokS.toFixed(1)} tok/s`;
  return `${tokS.toFixed(2)} tok/s`;
}

/**
 * Format parameter count in billions.
 */
export function formatParams(billions: number): string {
  if (billions >= 1) return `${billions.toFixed(1)}B`;
  return `${(billions * 1000).toFixed(0)}M`;
}

/**
 * Format relative time (e.g., "2 minutes ago").
 */
export function formatRelativeTime(isoString: string): string {
  const now = Date.now();
  const then = new Date(isoString).getTime();
  const diffMs = now - then;

  if (diffMs < 5000) return 'just now';
  if (diffMs < 60000) return `${Math.floor(diffMs / 1000)}s ago`;
  if (diffMs < 3600000) return `${Math.floor(diffMs / 60000)}m ago`;
  if (diffMs < 86400000) return `${Math.floor(diffMs / 3600000)}h ago`;
  return `${Math.floor(diffMs / 86400000)}d ago`;
}

/**
 * Format absolute time (HH:MM:SS).
 */
export function formatTime(isoString: string): string {
  const date = new Date(isoString);
  return date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

/**
 * Format a number with thousands separators.
 */
export function formatNumber(n: number): string {
  return n.toLocaleString();
}
