// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// DownloadPanel — active downloads and prefetch predictions

import React from 'react';
import type { DownloadProgress, PrefetchActivity } from './types/dashboard';
import { PRIORITY_COLORS } from './utils/colors';
import { formatPercent, formatSpeed, formatDurationSec, formatMb } from './utils/formatters';

interface DownloadPanelProps {
  downloads: DownloadProgress[];
  prefetch: PrefetchActivity[];
  onCancelDownload?: (downloadId: string) => void;
}

export const DownloadPanel = React.memo(function DownloadPanel({ downloads, prefetch, onCancelDownload }: DownloadPanelProps) {
  return (
    <div className="download-panel" role="region" aria-label="Downloads and prefetch">
      {/* Active Downloads */}
      <div className="downloads-section">
        <h3>Downloads ({downloads.length})</h3>
        {downloads.length === 0 ? (
          <p className="empty-state">No active downloads</p>
        ) : (
          <ul className="download-list">
            {downloads.map(dl => (
              <li key={dl.downloadId} className="download-item">
                <div className="download-header">
                  <span className="download-model">{dl.modelName}</span>
                  <span className="download-priority" style={{ color: PRIORITY_COLORS[dl.priority] }}>
                    {dl.priority}
                  </span>
                </div>
                <div className="download-progress" role="progressbar" aria-valuenow={dl.progressPercent} aria-valuemin={0} aria-valuemax={100}>
                  <div className="download-progress-bar" style={{ width: `${dl.progressPercent}%` }} />
                </div>
                <div className="download-stats">
                  <span>{formatPercent(dl.progressPercent)} — {formatMb(dl.downloadedMb)} / {formatMb(dl.totalSizeMb)}</span>
                  <span>{formatSpeed(dl.speedMbps)}</span>
                  {dl.etaSeconds != null && <span>ETA: {formatDurationSec(dl.etaSeconds)}</span>}
                </div>
                {(dl.priority === 'prefetch' || dl.priority === 'background') && onCancelDownload && (
                  <button
                    type="button"
                    className="download-cancel"
                    onClick={() => onCancelDownload(dl.downloadId)}
                    aria-label={`Cancel download of ${dl.modelName}`}
                  >
                    Cancel
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* Prefetch Predictions */}
      {prefetch.length > 0 && (
        <div className="prefetch-section">
          <h3>Prefetch Predictions</h3>
          <ul className="prefetch-list">
            {prefetch.map((p, i) => (
              <li key={i} className="prefetch-item">
                <span className="prefetch-model">{p.modelName}</span>
                <span className={`prefetch-status prefetch-status-${p.status}`}>{p.status}</span>
                <span className="prefetch-confidence">{formatPercent(p.confidencePercent)} confidence</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
});
