/**
 * ChannelsStep — Enable/disable available communication channels.
 * This step is optional and can be skipped.
 */

import { useState } from "react";

interface ChannelConfig {
  desktop: boolean;
  telegram: boolean;
  reticulum: boolean;
}

interface ChannelsStepProps {
  channelConfig: Record<string, unknown>;
  onUpdateChannels: (config: ChannelConfig) => void;
  onNext: () => void;
  onBack: () => void;
  onSkip: () => void;
}

const channelDescriptions: Record<keyof ChannelConfig, string> = {
  desktop: "Native desktop notifications and interactions",
  telegram: "Telegram bot integration for remote access",
  reticulum: "Reticulum mesh network for decentralized communication",
};

export function ChannelsStep({
  channelConfig,
  onUpdateChannels,
  onNext,
  onBack,
  onSkip,
}: ChannelsStepProps) {
  const [config, setConfig] = useState<ChannelConfig>({
    desktop: (channelConfig.desktop as boolean) ?? true,
    telegram: (channelConfig.telegram as boolean) ?? false,
    reticulum: (channelConfig.reticulum as boolean) ?? false,
  });

  const handleToggle = (channel: keyof ChannelConfig) => {
    const updated = { ...config, [channel]: !config[channel] };
    setConfig(updated);
    onUpdateChannels(updated);
  };

  return (
    <div className="wizard-step wizard-step-channels" role="region" aria-label="Channels step">
      <div className="wizard-step-header">
        <h2>Communication Channels</h2>
        <p>
          Choose which channels ResonantOS can use to communicate with you.
          Desktop is enabled by default.
        </p>
      </div>

      <div className="wizard-channel-list" role="group" aria-label="Available channels">
        {(Object.keys(channelDescriptions) as Array<keyof ChannelConfig>).map((channel) => (
          <label key={channel} className={`wizard-channel-item ${config[channel] ? "enabled" : ""}`}>
            <input
              type="checkbox"
              checked={config[channel]}
              onChange={() => handleToggle(channel)}
              aria-label={`Enable ${channel} channel`}
            />
            <div className="wizard-channel-info">
              <strong>{channel.charAt(0).toUpperCase() + channel.slice(1)}</strong>
              <p>{channelDescriptions[channel]}</p>
            </div>
          </label>
        ))}
      </div>

      <div className="wizard-step-actions">
        <button type="button" className="button-secondary" onClick={onBack} aria-label="Go back">
          Back
        </button>
        <button type="button" className="button-quiet" onClick={onSkip} aria-label="Skip this step">
          Skip
        </button>
        <button type="button" className="button-primary" onClick={onNext} aria-label="Continue to next step">
          Continue
        </button>
      </div>
    </div>
  );
}
