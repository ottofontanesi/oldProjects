// Intent citation: .kiro/specs/network-onboarding-wizard/design.md
// TrustExplainer — visual explanation of trust tiers with icons and plain language

import React from 'react';

export type TrustTier = 'public' | 'invited_friend' | 'local_owned';

interface TrustExplainerProps {
  offeredTier: TrustTier;
  showAllTiers?: boolean;
}

interface TierInfo {
  tier: TrustTier;
  label: string;
  icon: string;
  description: string;
  canDo: string[];
  cannotDo: string[];
}

const TIER_INFO: TierInfo[] = [
  {
    tier: 'local_owned',
    label: 'Full Trust',
    icon: '🏠',
    description: 'Your own devices — complete access to everything',
    canDo: [
      'See all your prompts and conversations',
      'Run any model, including sensitive tasks',
      'Vote on mesh decisions',
      'Invite new members',
    ],
    cannotDo: [],
  },
  {
    tier: 'invited_friend',
    label: 'Trusted Friend',
    icon: '🤝',
    description: "A friend's device — can help with non-private tasks",
    canDo: [
      'Run models for non-sensitive requests',
      'Share computing power with you',
      'See non-private prompts routed to them',
    ],
    cannotDo: [
      'See private or sensitive conversations',
      'Vote on mesh governance decisions',
      'Access your personal data',
    ],
  },
  {
    tier: 'public',
    label: 'Relay Only',
    icon: '📡',
    description: 'A public node — only helps route traffic, never sees content',
    canDo: [
      'Forward encrypted messages between nodes',
      'Help with network connectivity',
    ],
    cannotDo: [
      'See any prompt content (encrypted end-to-end)',
      'Run models on your behalf',
      'Access any of your data',
      'Vote on decisions',
    ],
  },
];

export function TrustExplainer({ offeredTier, showAllTiers = false }: TrustExplainerProps) {
  const tiersToShow = showAllTiers
    ? TIER_INFO
    : TIER_INFO.filter(t => t.tier === offeredTier);

  return (
    <div className="trust-explainer" role="region" aria-label="Trust tier explanation">
      <h3>Understanding trust levels</h3>
      <p className="trust-intro">
        Trust tiers control what a device can see and do in your network.
        Higher trust means more access.
      </p>

      <div className="trust-tiers">
        {tiersToShow.map(tier => (
          <div
            key={tier.tier}
            className={`trust-tier-card ${tier.tier === offeredTier ? 'trust-tier-highlighted' : ''}`}
            aria-current={tier.tier === offeredTier ? 'true' : undefined}
          >
            <div className="trust-tier-header">
              <span className="trust-tier-icon" aria-hidden="true">{tier.icon}</span>
              <h4 className="trust-tier-label">{tier.label}</h4>
              {tier.tier === offeredTier && (
                <span className="trust-tier-badge">Your level</span>
              )}
            </div>

            <p className="trust-tier-description">{tier.description}</p>

            {tier.canDo.length > 0 && (
              <div className="trust-tier-permissions">
                <h5>Can do:</h5>
                <ul>
                  {tier.canDo.map((item, i) => (
                    <li key={i} className="trust-can">✓ {item}</li>
                  ))}
                </ul>
              </div>
            )}

            {tier.cannotDo.length > 0 && (
              <div className="trust-tier-restrictions">
                <h5>Cannot do:</h5>
                <ul>
                  {tier.cannotDo.map((item, i) => (
                    <li key={i} className="trust-cannot">✗ {item}</li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
