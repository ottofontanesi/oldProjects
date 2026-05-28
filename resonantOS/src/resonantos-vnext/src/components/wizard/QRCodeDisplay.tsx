// Intent citation: .kiro/specs/network-onboarding-wizard/design.md
// QRCodeDisplay — QR code with countdown timer and status indicator

import React, { useState, useEffect, useCallback } from 'react';

export interface PairingInitData {
  pairingToken: string;
  desktopLanAddress: string;
  networkId: string;
  protocolVersion: number;
  createdAt: string;
  expiresAt: string;
  qrCodeData: string;
}

export type PairingStatus = 'waiting' | 'connected' | 'completed' | 'expired' | 'failed';

interface QRCodeDisplayProps {
  initData: PairingInitData | null;
  status: PairingStatus;
  onRegenerate?: () => void;
  onPollStatus?: () => void;
  pollIntervalMs?: number;
}

export function QRCodeDisplay({
  initData,
  status,
  onRegenerate,
  onPollStatus,
  pollIntervalMs = 2000,
}: QRCodeDisplayProps) {
  const [remainingSeconds, setRemainingSeconds] = useState<number>(0);

  // Countdown timer
  useEffect(() => {
    if (!initData) return;

    const expiresAt = new Date(initData.expiresAt).getTime();

    const interval = setInterval(() => {
      const now = Date.now();
      const remaining = Math.max(0, Math.floor((expiresAt - now) / 1000));
      setRemainingSeconds(remaining);

      if (remaining <= 0) {
        clearInterval(interval);
      }
    }, 1000);

    return () => clearInterval(interval);
  }, [initData]);

  // Poll for connection status
  useEffect(() => {
    if (status !== 'waiting' || !onPollStatus) return;

    const interval = setInterval(onPollStatus, pollIntervalMs);
    return () => clearInterval(interval);
  }, [status, onPollStatus, pollIntervalMs]);

  if (!initData) {
    return (
      <div className="qr-display" role="status">
        <p>Generating pairing code...</p>
      </div>
    );
  }

  const isExpired = status === 'expired' || remainingSeconds <= 0;
  const minutes = Math.floor(remainingSeconds / 60);
  const seconds = remainingSeconds % 60;

  return (
    <div className="qr-display" role="region" aria-label="Phone pairing QR code">
      {/* Status indicator */}
      <div className={`qr-status qr-status-${status}`} role="status" aria-live="polite">
        {status === 'waiting' && !isExpired && (
          <p>Scan this QR code with the ResonantOS mobile app</p>
        )}
        {status === 'connected' && (
          <p>✅ Phone connected! Completing handshake...</p>
        )}
        {status === 'completed' && (
          <p>✅ Pairing successful!</p>
        )}
        {(status === 'expired' || isExpired) && (
          <p>⏰ QR code expired</p>
        )}
        {status === 'failed' && (
          <p>❌ Pairing failed</p>
        )}
      </div>

      {/* QR Code area */}
      <div className={`qr-code-container ${isExpired ? 'qr-expired' : ''}`}>
        {!isExpired ? (
          <div
            className="qr-code-placeholder"
            aria-label={`QR code for pairing. Data: ${initData.qrCodeData}`}
            role="img"
          >
            {/* In production, render actual QR code using a library like qrcode.react */}
            <div className="qr-code-visual" style={{ width: 200, height: 200, background: '#f0f0f0', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
              QR Code
            </div>
          </div>
        ) : (
          <div className="qr-expired-overlay">
            <p>Code expired</p>
            {onRegenerate && (
              <button
                type="button"
                className="qr-regenerate-btn"
                onClick={onRegenerate}
                aria-label="Generate new QR code"
              >
                Generate new code
              </button>
            )}
          </div>
        )}
      </div>

      {/* Countdown */}
      {!isExpired && status === 'waiting' && (
        <div className="qr-countdown" aria-label={`Expires in ${minutes} minutes ${seconds} seconds`}>
          <span className="qr-countdown-time">
            {minutes}:{seconds.toString().padStart(2, '0')}
          </span>
          <span className="qr-countdown-label">remaining</span>
        </div>
      )}

      {/* Instructions */}
      {status === 'waiting' && !isExpired && (
        <div className="qr-instructions">
          <ol>
            <li>Open the ResonantOS app on your phone</li>
            <li>Tap "Join Network"</li>
            <li>Point your camera at this QR code</li>
          </ol>
        </div>
      )}
    </div>
  );
}
