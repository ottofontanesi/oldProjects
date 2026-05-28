/**
 * Cost Dashboard — Settings workspace panel for token consumption and cost visibility.
 *
 * Displays:
 * - Token consumption bar chart by agent (day/week toggle, CSS-based bars)
 * - Cost breakdown by ProviderCostPosture category
 * - Projected monthly spend card
 * - Recent records table with task type classification
 *
 * Integration note: Wire this component into SettingsWorkspace by adding
 * a "costs" entry to the SettingsSection type and settingsItems array,
 * then render <CostDashboard /> when settingsSection === "costs".
 */

import { useEffect, useState, useCallback, useRef } from "react";
import type {
  CostDashboardData,
  CostProjection,
  CostAggregation,
  CostRecord,
  CostLedgerQuery,
} from "../../core/data-infrastructure";
import { queryCostDashboard, queryCostProjection } from "../../core/data-infrastructure";

// ─── Types ──────────────────────────────────────────────────────────────────

export type CostPeriodType = "day" | "week";

export type CostPostureCategory = "free-local" | "subscription" | "paid-api" | "emergency-only";

const COST_POSTURE_LABELS: Record<CostPostureCategory, string> = {
  "free-local": "Free (Local)",
  subscription: "Subscription",
  "paid-api": "Paid API",
  "emergency-only": "Emergency Only",
};

const POLL_INTERVAL_MS = 5000;

// ─── Component ──────────────────────────────────────────────────────────────

export function CostDashboard() {
  const [data, setData] = useState<CostDashboardData | null>(null);
  const [projection, setProjection] = useState<CostProjection | null>(null);
  const [periodType, setPeriodType] = useState<CostPeriodType>("day");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  // ─── Controller: IPC calls ──────────────────────────────────────────────

  const fetchData = useCallback(async () => {
    try {
      const query: CostLedgerQuery = { periodType };
      const [dashboardData, projectionData] = await Promise.all([
        queryCostDashboard(query),
        queryCostProjection(),
      ]);
      if (mountedRef.current) {
        setData(dashboardData);
        setProjection(projectionData);
        setError(null);
        setLoading(false);
      }
    } catch {
      if (mountedRef.current) {
        setError("Cost data unavailable");
        setLoading(false);
      }
    }
  }, [periodType]);

  // ─── Initial load + poll every 5s when visible ──────────────────────────

  useEffect(() => {
    mountedRef.current = true;
    fetchData();

    const interval = setInterval(() => {
      if (mountedRef.current) {
        fetchData();
      }
    }, POLL_INTERVAL_MS);

    return () => {
      mountedRef.current = false;
      clearInterval(interval);
    };
  }, [fetchData]);

  // ─── Derived data ───────────────────────────────────────────────────────

  const aggregations = data?.aggregations ?? [];
  const recentRecords = data?.recentRecords ?? [];
  const effectiveProjection = projection ?? data?.projection ?? null;

  // Group aggregations by agent for bar chart
  const agentTotals = aggregations.reduce<Record<string, number>>((acc, agg) => {
    acc[agg.agentId] = (acc[agg.agentId] ?? 0) + agg.totalTokens;
    return acc;
  }, {});

  const maxTokens = Math.max(...Object.values(agentTotals), 1);

  // Group by cost posture
  const postureTotals = recentRecords.reduce<Record<CostPostureCategory, { tokens: number; cost: number }>>(
    (acc, record) => {
      const posture = record.costPosture as CostPostureCategory;
      if (acc[posture]) {
        acc[posture].tokens += record.totalTokens;
        acc[posture].cost += record.estimatedCostUsd;
      }
      return acc;
    },
    {
      "free-local": { tokens: 0, cost: 0 },
      subscription: { tokens: 0, cost: 0 },
      "paid-api": { tokens: 0, cost: 0 },
      "emergency-only": { tokens: 0, cost: 0 },
    },
  );

  // ─── Render ─────────────────────────────────────────────────────────────

  if (loading) {
    return (
      <div className="cost-dashboard" aria-label="Cost Dashboard" role="region">
        <p className="cost-dashboard-loading">Loading cost data…</p>
      </div>
    );
  }

  if (error && !data) {
    return (
      <div className="cost-dashboard" aria-label="Cost Dashboard" role="region">
        <p className="cost-dashboard-error">{error}</p>
      </div>
    );
  }

  return (
    <div className="cost-dashboard" aria-label="Cost Dashboard" role="region">
      <div className="cost-dashboard-header">
        <div>
          <p className="eyebrow">Cost tracking</p>
          <h3>Token Consumption &amp; Spend</h3>
        </div>
        <div className="cost-period-toggle" role="group" aria-label="Period toggle">
          <button
            type="button"
            className={`cost-period-btn ${periodType === "day" ? "active" : ""}`}
            onClick={() => setPeriodType("day")}
            aria-pressed={periodType === "day"}
          >
            Daily
          </button>
          <button
            type="button"
            className={`cost-period-btn ${periodType === "week" ? "active" : ""}`}
            onClick={() => setPeriodType("week")}
            aria-pressed={periodType === "week"}
          >
            Weekly
          </button>
        </div>
      </div>

      {/* Token Consumption Bar Chart */}
      <section className="cost-section" aria-label="Token consumption by agent">
        <h4>Token Consumption by Agent</h4>
        {Object.keys(agentTotals).length === 0 ? (
          <p className="cost-empty-state">No token consumption data for this period.</p>
        ) : (
          <div className="cost-bar-chart" role="img" aria-label="Token consumption bar chart">
            {Object.entries(agentTotals)
              .sort(([, a], [, b]) => b - a)
              .map(([agentId, tokens]) => (
                <div key={agentId} className="cost-bar-row">
                  <span className="cost-bar-label">{agentId}</span>
                  <div className="cost-bar-track">
                    <div
                      className="cost-bar-fill"
                      style={{ width: `${(tokens / maxTokens) * 100}%` }}
                      aria-label={`${agentId}: ${tokens.toLocaleString()} tokens`}
                    />
                  </div>
                  <span className="cost-bar-value">{tokens.toLocaleString()}</span>
                </div>
              ))}
          </div>
        )}
      </section>

      {/* Cost Breakdown by Posture */}
      <section className="cost-section" aria-label="Cost breakdown by provider category">
        <h4>Cost Breakdown</h4>
        <div className="cost-posture-grid">
          {(Object.keys(COST_POSTURE_LABELS) as CostPostureCategory[]).map((posture) => (
            <div key={posture} className={`cost-posture-card cost-posture-${posture}`}>
              <span className="cost-posture-label">{COST_POSTURE_LABELS[posture]}</span>
              <strong className="cost-posture-cost">
                ${postureTotals[posture].cost.toFixed(4)}
              </strong>
              <span className="cost-posture-tokens">
                {postureTotals[posture].tokens.toLocaleString()} tokens
              </span>
            </div>
          ))}
        </div>
      </section>

      {/* Projected Monthly Spend */}
      <section className="cost-section" aria-label="Projected monthly spend">
        <h4>Projected Monthly Spend</h4>
        {effectiveProjection ? (
          <div className="cost-projection-card">
            <div className="cost-projection-main">
              <span className="cost-projection-amount">
                ${effectiveProjection.projectedMonthlyUsd.toFixed(2)}
              </span>
              <span className="cost-projection-label">projected / month</span>
            </div>
            <div className="cost-projection-detail">
              <span>Daily average: ${effectiveProjection.dailyAverageUsd.toFixed(4)}</span>
              <span>Rolling window: {effectiveProjection.rollingWindowDays} days</span>
            </div>
          </div>
        ) : (
          <div className="cost-projection-card">
            <div className="cost-projection-main">
              <span className="cost-projection-amount">$0.00</span>
              <span className="cost-projection-label">projected / month</span>
            </div>
            <div className="cost-projection-detail">
              <span>Daily average: $0.0000</span>
              <span>No data available yet</span>
            </div>
          </div>
        )}
      </section>

      {/* Recent Records Table */}
      <section className="cost-section" aria-label="Recent cost records">
        <h4>Recent Records</h4>
        {recentRecords.length === 0 ? (
          <p className="cost-empty-state">No recent cost records.</p>
        ) : (
          <div className="cost-table-wrapper">
            <table className="cost-table" aria-label="Recent cost records table">
              <thead>
                <tr>
                  <th>Agent</th>
                  <th>Task Type</th>
                  <th>Model</th>
                  <th>Tokens</th>
                  <th>Cost</th>
                  <th>Time</th>
                </tr>
              </thead>
              <tbody>
                {recentRecords.map((record) => (
                  <tr key={record.id}>
                    <td>{record.agentId}</td>
                    <td>
                      <span className="cost-task-type">{record.taskType}</span>
                    </td>
                    <td>{record.model}</td>
                    <td>{record.totalTokens.toLocaleString()}</td>
                    <td>${record.estimatedCostUsd.toFixed(4)}</td>
                    <td>{new Date(record.recordedAt).toLocaleTimeString()}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}
