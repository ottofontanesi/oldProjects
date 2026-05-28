// Intent citation: .kiro/specs/phone-companion-app/design.md
// PairingScreen — QR code scanner integration, pairing status, error display

import React, { useState, useCallback } from 'react';

// ─── Types ───────────────────────────────────────────────────────────────────

export type PairingStatus = 'idle' | 'scanning' | 'connecting' | 'success' | 'error';

export interface PairingError {
  code: string;
  message: string;
}

export interface PairingSuccess {
  networkId: string;
  nodeId: string;
  coordinatorAddr: string;
}

interface PairingScreenProps {
  onStartScan?: () => void;
  onQrScanned?: (qrData: string) => Promise<PairingSuccess>;
  onCancel?: () => void;
  initialStatus?: PairingStatus;
}

// ─── Error Messages ──────────────────────────────────────────────────────────

function getErrorGuidance(code: string): string {
  switch (code) {
    case 'TOKEN_EXPIRED':
      return 'The QR code has expired. Please generate a new one on your desktop.';
    case 'SUBNET_MISMATCH':
      return 'Your phone and desktop are on different networks. Connect to the same WiFi.';
    case 'INVALID_QR':
    case 'INVALID_QR_FORMAT':
      return 'The scanned code is not a valid ResonantOS pairing code.';
    case 'NETWORK_UNREACHABLE':
      return 'Cannot reach the desktop. Check your network connection.';
    case 'HANDSHAKE_REJECTED':
      return 'The desktop rejected the pairing request. Try again.';
    default:
      return 'An unexpected error occurred. Please try again.';
  }
}

// ─── Component ───────────────────────────────────────────────────────────────

export function PairingScreen({
  onStartScan,
  onQrScanned,
  onCancel,
  initialStatus = 'idle',
}: PairingScreenProps) {
  const [status, setStatus] = useState<PairingStatus>(initialStatus);
  const [error, setError] = useState<PairingError | null>(null);
  const [pairingResult, setPairingResult] = useState<PairingSuccess | null>(null);
  const [cameraPermission, setCameraPermission] = useState<'granted' | 'denied' | 'pending'>('pending');

  const handleStartScan = useCallback(async () => {
    setError(null);
    setCameraPermission('granted');
    setStatus('scanning');
    onStartScan?.();
  }, [onStartScan]);

  const handleQrScanned = useCallback(async (qrData: string) => {
    setStatus('connecting');
    setError(null);

    if (!onQrScanned) {
      setStatus('error');
      setError({ code: 'NO_HANDLER', message: 'Pairing handler not configured' });
      return;
    }

    try {
      const result = await onQrScanned(qrData);
      setPairingResult(result);
      setStatus('success');
    } catch (err: unknown) {
      const pairingError: PairingError = err instanceof Error
        ? { code: 'UNKNOWN', message: err.message }
        : (err as PairingError);
      setError(pairingError);
      setStatus('error');
    }
  }, [onQrScanned]);

  const handleRetry = useCallback(() => {
    setError(null);
    setPairingResult(null);
    setStatus('idle');
  }, []);

  return (
    <div className="pairing-screen" role="region" aria-label="Phone pairing">
      <h2>Pair Phone to Mesh</h2>

      {/* Idle state — prompt to start scanning */}
      {status === 'idle' && (
        <div className="pairing-idle">
          <p>Scan the QR code displayed on your desktop to join the mesh network.</p>
          <button
            className="pairing-scan-btn"
            onClick={handleStartScan}
            aria-label="Start QR code scanner"
          >
            Scan QR Code
          </button>
          {onCancel && (
            <button
              className="pairing-cancel-btn"
              onClick={onCancel}
              aria-label="Cancel pairing"
            >
              Cancel
            </button>
          )}
        </div>
      )}

      {/* Scanning state — camera active */}
      {status === 'scanning' && (
        <div className="pairing-scanning" role="status" aria-live="polite">
          <div className="pairing-camera-viewfinder" aria-label="Camera viewfinder">
            <p>Point camera at QR code…</p>
          </div>
          {cameraPermission === 'denied' && (
            <p className="pairing-permission-error" role="alert">
              Camera permission denied. Please enable camera access in settings.
            </p>
          )}
          <button
            className="pairing-cancel-btn"
            onClick={handleRetry}
            aria-label="Cancel scanning"
          >
            Cancel
          </button>
        </div>
      )}

      {/* Connecting state — handshake in progress */}
      {status === 'connecting' && (
        <div className="pairing-connecting" role="status" aria-live="polite">
          <div className="pairing-spinner" aria-hidden="true" />
          <p>Connecting to mesh network…</p>
        </div>
      )}

      {/* Success state — paired successfully */}
      {status === 'success' && pairingResult && (
        <div className="pairing-success" role="status" aria-live="polite">
          <div className="pairing-success-icon" aria-hidden="true">✅</div>
          <h3>Paired Successfully</h3>
          <dl className="pairing-details">
            <dt>Network ID</dt>
            <dd>{pairingResult.networkId.slice(0, 8)}…</dd>
            <dt>Node ID</dt>
            <dd>{pairingResult.nodeId.slice(0, 8)}…</dd>
            <dt>Coordinator</dt>
            <dd>{pairingResult.coordinatorAddr}</dd>
          </dl>
        </div>
      )}

      {/* Error state — display error with guidance */}
      {status === 'error' && error && (
        <div className="pairing-error" role="alert">
          <div className="pairing-error-icon" aria-hidden="true">⚠️</div>
          <h3>Pairing Failed</h3>
          <p className="pairing-error-message">{error.message}</p>
          <p className="pairing-error-guidance">{getErrorGuidance(error.code)}</p>
          <button
            className="pairing-retry-btn"
            onClick={handleRetry}
            aria-label="Try pairing again"
          >
            Try Again
          </button>
          {onCancel && (
            <button
              className="pairing-cancel-btn"
              onClick={onCancel}
              aria-label="Cancel pairing"
            >
              Cancel
            </button>
          )}
        </div>
      )}
    </div>
  );
}
