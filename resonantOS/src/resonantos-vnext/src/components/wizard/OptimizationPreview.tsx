// Intent citation: .kiro/specs/network-onboarding-wizard/design.md
// OptimizationPreview — plain language plan display with per-node benefits

import React from 'react';

export interface PlainLanguagePlacement {
  modelName: string;
  placementDescription: string;
  whyChosen: string;
  performanceNote: string;
}

export interface NodeBenefitExplanation {
  nodeName: string;
  benefit: string;
  before: string;
  after: string;
}

export interface OptimizationPreviewData {
  proposedPlan: PlainLanguagePlacement[];
  utilityBefore: number;
  utilityAfter: number;
  improvementPercent: number;
  perNodeBenefits: NodeBenefitExplanation[];
}

interface OptimizationPreviewProps {
  data: OptimizationPreviewData | null;
  loading?: boolean;
}

export function OptimizationPreview({ data, loading }: OptimizationPreviewProps) {
  if (loading) {
    return (
      <div className="optimization-preview" role="status">
        <p>Computing optimal setup...</p>
      </div>
    );
  }

  if (!data) return null;

  return (
    <div className="optimization-preview" role="region" aria-label="Optimization preview">
      <h3>Recommended setup</h3>

      {data.improvementPercent > 0 && (
        <p className="optimization-improvement">
          {data.improvementPercent.toFixed(0)}% improvement over single-machine setup
        </p>
      )}

      {/* Proposed placements */}
      <div className="optimization-placements">
        <h4>Models we recommend</h4>
        <ul className="placement-list">
          {data.proposedPlan.map((placement, i) => (
            <li key={i} className="placement-item">
              <div className="placement-model">{placement.modelName}</div>
              <div className="placement-where">{placement.placementDescription}</div>
              <div className="placement-why">{placement.whyChosen}</div>
              <div className="placement-perf">{placement.performanceNote}</div>
            </li>
          ))}
        </ul>
      </div>

      {/* Per-node benefits */}
      {data.perNodeBenefits.length > 0 && (
        <div className="optimization-benefits">
          <h4>What each device gains</h4>
          <ul className="benefits-list">
            {data.perNodeBenefits.map((benefit, i) => (
              <li key={i} className="benefit-item">
                <strong>{benefit.nodeName}</strong>
                <p className="benefit-text">{benefit.benefit}</p>
                <div className="benefit-comparison">
                  <span className="benefit-before">Before: {benefit.before}</span>
                  <span className="benefit-after">After: {benefit.after}</span>
                </div>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
