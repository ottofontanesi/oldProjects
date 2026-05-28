// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 10
// Observability — metrics export, audit trail, explain placement API

use super::catalog::ModelId;
use super::lifecycle::OptimizerEvent;
use super::registry::NodeId;
use super::solver::PlacementPlan;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Metrics ─────────────────────────────────────────────────────────────────

/// Network-level optimizer metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerMetrics {
    pub total_utility: f64,
    pub quality_score: f64,
    pub speed_score: f64,
    pub mass_score: f64,
    pub total_loaded_params_b: f64,
    pub total_nodes_online: u32,
    pub total_models_loaded: u32,
    pub last_solve_duration_ms: u64,
    pub solve_count: u64,
    pub timeout_count: u64,
    pub active_downloads: u32,
    pub cache_hit_rate: f64,
    pub prefetch_accuracy: f64,
    pub prefetch_active_count: u32,
}

impl Default for OptimizerMetrics {
    fn default() -> Self {
        Self {
            total_utility: 0.0,
            quality_score: 0.0,
            speed_score: 0.0,
            mass_score: 0.0,
            total_loaded_params_b: 0.0,
            total_nodes_online: 0,
            total_models_loaded: 0,
            last_solve_duration_ms: 0,
            solve_count: 0,
            timeout_count: 0,
            active_downloads: 0,
            cache_hit_rate: 0.0,
            prefetch_accuracy: 0.0,
            prefetch_active_count: 0,
        }
    }
}

/// Per-node metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetrics {
    pub node_id: NodeId,
    pub hostname: String,
    pub device_type: String,
    pub is_online: bool,
    pub stability_score: f64,
    pub models_hosted: Vec<ModelId>,
    pub cpu_percent: f32,
    pub ram_percent: f64,
    pub gpu_percent: Option<f32>,
    pub queue_depth: u32,
}

// ─── Audit Trail ─────────────────────────────────────────────────────────────

/// A single audit entry recording an optimization decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp_ms: u64,
    pub plan_id: uuid::Uuid,
    pub trigger: OptimizerEvent,
    pub decisions: Vec<PlacementDecision>,
    pub utility_before: f64,
    pub utility_after: f64,
    pub duration_ms: u64,
}

/// A single placement decision with reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementDecision {
    pub model_id: ModelId,
    pub action: DecisionAction,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DecisionAction {
    Load { node: NodeId },
    Unload { node: NodeId },
    Migrate { from: NodeId, to: NodeId },
    Keep,
}

/// Append-only audit trail.
pub struct AuditTrail {
    entries: Vec<AuditEntry>,
    max_entries: usize,
}

impl AuditTrail {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    pub fn record(&mut self, entry: AuditEntry) {
        self.entries.push(entry);
        // Trim oldest if over limit
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }
    }

    pub fn recent(&self, count: usize) -> &[AuditEntry] {
        let start = self.entries.len().saturating_sub(count);
        &self.entries[start..]
    }

    pub fn all(&self) -> &[AuditEntry] {
        &self.entries
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for AuditTrail {
    fn default() -> Self {
        Self::new(1000) // Keep last 1000 decisions
    }
}

// ─── Explain Placement API ───────────────────────────────────────────────────

/// Scoring breakdown for a single candidate node (used by explain_placement).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementFactor {
    pub node_id: NodeId,
    pub hostname: String,
    pub speed_score: f64,
    pub stability_score: f64,
    pub cache_score: f64,
    pub headroom_score: f64,
    pub total_score: f64,
    pub is_winner: bool,
    pub rejection_reason: Option<String>,
}

/// Full explanation of why a model is placed where it is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementExplanation {
    pub model_id: ModelId,
    pub placed_on: Vec<NodeId>,
    pub protocol: String,
    pub candidates_evaluated: Vec<PlacementFactor>,
    pub summary: String,
}

/// Generate an explanation for a model's placement.
pub fn explain_placement(
    model_id: &str,
    plan: &PlacementPlan,
    node_scores: &HashMap<NodeId, (String, f64, f64, f64, f64)>, // (hostname, speed, stability, cache, headroom)
) -> Option<PlacementExplanation> {
    let placement = plan.placements.iter().find(|p| p.model_id == model_id)?;

    let mut candidates: Vec<PlacementFactor> = node_scores
        .iter()
        .map(|(node_id, (hostname, speed, stability, cache, headroom))| {
            let total = speed * 0.4 + stability * 0.2 + cache * 0.2 + headroom * 0.2;
            let is_winner = placement.assigned_nodes.contains(node_id);
            PlacementFactor {
                node_id: *node_id,
                hostname: hostname.clone(),
                speed_score: *speed,
                stability_score: *stability,
                cache_score: *cache,
                headroom_score: *headroom,
                total_score: total,
                is_winner,
                rejection_reason: None,
            }
        })
        .collect();

    candidates.sort_by(|a, b| b.total_score.partial_cmp(&a.total_score).unwrap_or(std::cmp::Ordering::Equal));

    let winner = candidates.iter().find(|c| c.is_winner);
    let summary = match winner {
        Some(w) => format!(
            "{} placed on {} (score {:.2}): speed={:.2}, stability={:.2}, cache={:.2}, headroom={:.2}",
            model_id, w.hostname, w.total_score, w.speed_score, w.stability_score, w.cache_score, w.headroom_score
        ),
        None => format!("{} placement not found in candidates", model_id),
    };

    Some(PlacementExplanation {
        model_id: model_id.to_string(),
        placed_on: placement.assigned_nodes.clone(),
        protocol: format!("{:?}", placement.protocol),
        candidates_evaluated: candidates,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::solver::{ModelPlacement, ParallelismProtocol, UtilityScores};

    #[test]
    fn test_audit_trail_record_and_retrieve() {
        let mut trail = AuditTrail::new(100);

        trail.record(AuditEntry {
            timestamp_ms: 1000,
            plan_id: uuid::Uuid::new_v4(),
            trigger: OptimizerEvent::Timer,
            decisions: vec![],
            utility_before: 0.5,
            utility_after: 0.7,
            duration_ms: 150,
        });

        assert_eq!(trail.count(), 1);
        assert_eq!(trail.recent(10).len(), 1);
    }

    #[test]
    fn test_audit_trail_max_entries() {
        let mut trail = AuditTrail::new(3);

        for i in 0..5 {
            trail.record(AuditEntry {
                timestamp_ms: i * 1000,
                plan_id: uuid::Uuid::new_v4(),
                trigger: OptimizerEvent::Timer,
                decisions: vec![],
                utility_before: 0.5,
                utility_after: 0.6,
                duration_ms: 100,
            });
        }

        assert_eq!(trail.count(), 3); // Capped at max
        assert_eq!(trail.all()[0].timestamp_ms, 2000); // Oldest trimmed
    }

    #[test]
    fn test_explain_placement() {
        let node_a = uuid::Uuid::new_v4();
        let node_b = uuid::Uuid::new_v4();

        let plan = PlacementPlan {
            plan_id: uuid::Uuid::new_v4(),
            created_at_ms: 1000,
            solver_duration_ms: 50,
            utility_scores: UtilityScores { quality: 0.7, speed: 0.6, mass: 0.5, total: 0.63, agent_utility: 0.0, contention_cost: 0.0, unified_total: 0.63 },
            placements: vec![ModelPlacement {
                model_id: "qwen:7b".to_string(),
                instance_id: uuid::Uuid::new_v4(),
                assigned_nodes: vec![node_a],
                protocol: ParallelismProtocol::SingleNode,
                estimated_tok_s: 45.0,
            }],
            agent_placements: vec![],
            pending_downloads: vec![],
            diagnostics: vec![],
        };

        let mut scores = HashMap::new();
        scores.insert(node_a, ("desktop".to_string(), 0.9, 0.95, 0.5, 0.7));
        scores.insert(node_b, ("laptop".to_string(), 0.5, 0.8, 0.0, 0.9));

        let explanation = explain_placement("qwen:7b", &plan, &scores);
        assert!(explanation.is_some());

        let exp = explanation.unwrap();
        assert_eq!(exp.model_id, "qwen:7b");
        assert_eq!(exp.placed_on, vec![node_a]);
        assert_eq!(exp.candidates_evaluated.len(), 2);
        assert!(exp.summary.contains("desktop"));

        // Winner should be node_a
        let winner = exp.candidates_evaluated.iter().find(|c| c.is_winner).unwrap();
        assert_eq!(winner.node_id, node_a);
    }

    #[test]
    fn test_explain_placement_not_found() {
        let plan = PlacementPlan {
            plan_id: uuid::Uuid::new_v4(),
            created_at_ms: 1000,
            solver_duration_ms: 50,
            utility_scores: UtilityScores { quality: 0.5, speed: 0.5, mass: 0.5, total: 0.5, agent_utility: 0.0, contention_cost: 0.0, unified_total: 0.5 },
            placements: vec![],
            agent_placements: vec![],
            pending_downloads: vec![],
            diagnostics: vec![],
        };

        let scores = HashMap::new();
        let explanation = explain_placement("nonexistent", &plan, &scores);
        assert!(explanation.is_none());
    }
}
