// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// MetricsPanel — utility gauges with sparklines

import React from 'react';
import type { UtilityScores, SparklineData } from './types/dashboard';
import { getGaugeColor } from './utils/colors';
import { formatPercent } from './utils/formatters';

interface MetricsPanelProps {
  scores: UtilityScores | null;
  history?: { quality: SparklineData; speed: SparklineData; mass: SparklineData; total: SparklineData };
}

interface GaugeProps {
  label: string;
  value: number;
  weight: number;
  tooltip: string;
}

function CircularGauge({ label, value, weight, tooltip }: GaugeProps) {
  const percent = value * 100;
  const color = getGaugeColor(percent);
  const circumference = 2 * Math.PI * 40;
  const offset = circumference - (percent / 100) * circumference;

  return (
    <div className="gauge" role="meter" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100} aria-label={`${label}: ${formatPercent(percent)}`} title={tooltip}>
      <svg width="100" height="100" viewBox="0 0 100 100">
        <circle cx="50" cy="50" r="40" fill="none" stroke="#e5e7eb" strokeWidth="8" />
        <circle cx="50" cy="50" r="40" fill="none" stroke={color} strokeWidth="8"
          strokeDasharray={circumference} strokeDashoffset={offset}
          strokeLinecap="round" transform="rotate(-90 50 50)" />
        <text x="50" y="45" textAnchor="middle" fontSize="16" fontWeight="bold" fill="currentColor">
          {formatPercent(percent, 0)}
        </text>
        <text x="50" y="62" textAnchor="middle" fontSize="10" fill="#6b7280">
          w: {formatPercent(weight * 100, 0)}
        </text>
      </svg>
      <span className="gauge-label">{label}</span>
    </div>
  );
}

function Sparkline({ data, color }: { data: SparklineData; color: string }) {
  if (!data || data.length === 0) return null;

  const width = 120;
  const height = 30;
  const max = Math.max(...data.map(d => d.value), 0.01);
  const min = Math.min(...data.map(d => d.value), 0);

  const points = data.map((d, i) => {
    const x = (i / (data.length - 1)) * width;
    const y = height - ((d.value - min) / (max - min || 1)) * height;
    return `${x},${y}`;
  }).join(' ');

  return (
    <svg width={width} height={height} className="sparkline" aria-hidden="true">
      <polyline points={points} fill="none" stroke={color} strokeWidth="1.5" />
    </svg>
  );
}

export const MetricsPanel = React.memo(function MetricsPanel({ scores, history }: MetricsPanelProps) {
  if (!scores) {
    return (
      <div className="metrics-panel metrics-panel-loading" role="region" aria-label="Utility metrics">
        <p>Loading metrics...</p>
      </div>
    );
  }

  return (
    <div className="metrics-panel" role="region" aria-label="Utility metrics">
      <h3>Network Utility</h3>
      <div className="gauges-row">
        <CircularGauge label="Quality" value={scores.quality} weight={scores.weights.quality}
          tooltip="How capable your loaded models are (parameter count, benchmark scores)" />
        <CircularGauge label="Speed" value={scores.speed} weight={scores.weights.speed}
          tooltip="How fast inference runs (tokens per second across all models)" />
        <CircularGauge label="Coverage" value={scores.mass} weight={scores.weights.mass}
          tooltip="How many different tasks your models can handle well" />
        <CircularGauge label="Total" value={scores.total} weight={1.0}
          tooltip="Weighted combination of Quality, Speed, and Coverage" />
      </div>
      {history && (
        <div className="sparklines-row">
          <Sparkline data={history.quality} color="#22c55e" />
          <Sparkline data={history.speed} color="#3b82f6" />
          <Sparkline data={history.mass} color="#f97316" />
          <Sparkline data={history.total} color="#6b7280" />
        </div>
      )}
    </div>
  );
});
