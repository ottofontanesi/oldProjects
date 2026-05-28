# Technical Design: Mesh Network Optimizer (Phase 9B)

## 1. Architecture Overview

The Mesh Network Optimizer extends Phase 9A's local optimizer to multi-owner networks. It runs as an opt-in service alongside the local optimizer, communicating via the Unified Mesh Transport (Phase 10) and producing global placement plans that local optimizers can accept or reject.

### 1.1 System Context

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Mesh Network (up to 100 nodes)                    │
│                                                                           │
│  ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐   │
│  │  Node A (tier 3) │     │  Node B (tier 2) │     │  Node C (tier 1) │   │
│  │  ┌─────────────┐ │     │  ┌─────────────┐ │     │  ┌─────────────┐ │   │
│  │  │Local Optim. │ │     │  │Local Optim. │ │     │  │Local Optim. │ │   │
│  │  └──────┬──────┘ │     │  └──────┬──────┘ │     │  └──────┬──────┘ │   │
│  │         │         │     │         │         │     │         │         │   │
│  │  ┌──────▼──────┐ │     │  ┌──────▼──────┐ │     │  ┌──────▼──────┐ │   │
│  │  │Mesh Agent   │◄├─────├─▶│Mesh Agent   │◄├─────├─▶│Mesh Agent   │ │   │
│  │  │(full access)│ │     │  │(inference)  │ │     │  │(relay only) │ │   │
│  │  └──────┬──────┘ │     │  └─────────────┘ │     │  └─────────────┘ │   │
│  │         │         │     │                   │     │                   │   │
│  │  ┌──────▼──────┐ │     │                   │     │                   │   │
│  │  │Mesh Optim.  │ │     │                   │     │                   │   │
│  │  │(leader role)│ │     │                   │     │                   │   │
│  │  └─────────────┘ │     │                   │     │                   │   │
│  └─────────────────┘     └─────────────────┘     └─────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

### 1.2 Module Decomposition

| Module | Responsibility | Crate Path |
|--------|---------------|------------|
| `mesh_identity` | Ed25519 keypair, node identity, invitation tokens | `src-tauri/src/mesh/identity.rs` |
| `mesh_membership` | Join/leave, invitation flow, multi-mesh support | `src-tauri/src/mesh/membership.rs` |
| `trust_manager` | Trust tier assignment, promotion/demotion, enforcement | `src-tauri/src/mesh/trust.rs` |
| `sensitivity_classifier` | Prompt classification (sensitive/non-sensitive) | `src-tauri/src/mesh/classifier.rs` |
| `mesh_solver` | Extended solver with trust/reputation constraints | `src-tauri/src/mesh/solver.rs` |
| `accounting` | Contribution/consumption tracking, reputation scoring | `src-tauri/src/mesh/accounting.rs` |
| `incentive_enforcer` | Free-rider detection, escalation, recovery | `src-tauri/src/mesh/incentive.rs` |
| `consensus` | Proposal creation, voting, quorum checking | `src-tauri/src/mesh/consensus.rs` |
| `rate_limiter` | Per-node and aggregate rate limiting, anomaly detection | `src-tauri/src/mesh/rate_limiter.rs` |
| `mesh_transfer` | Cross-mesh model transfer coordination | `src-tauri/src/mesh/transfer.rs` |
| `mesh_observability` | Mesh-specific metrics, economic dashboard data | `src-tauri/src/mesh/observability.rs` |

### 1.3 Leader Election

The mesh optimizer runs on ONE node at a time (the "leader"). Leader election:

```
1. All tier-3 nodes are eligible to be leader
2. Leader is the tier-3 node with highest reputation + longest uptime (deterministic)
3. If leader goes offline, next-highest tier-3 node takes over within 5 minutes
4. Leader runs the mesh solver every 15 minutes and distributes plans
5. Non-leader tier-3 nodes validate the plan (can reject if it violates their constraints)
```

No complex consensus needed for leader election — it's deterministic based on observable metrics.

## 2. Data Models

### 2.1 Mesh Identity and Membership

```rust
use ed25519_dalek::{SigningKey, VerifyingKey, Signature};

pub type MeshId = uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshIdentity {
    pub node_id: NodeId,
    pub signing_key: SigningKey,         // Private — never transmitted
    pub verifying_key: VerifyingKey,     // Public — shared with mesh
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshMembership {
    pub mesh_id: MeshId,
    pub mesh_name: String,
    pub joined_at: chrono::DateTime<chrono::Utc>,
    pub trust_tier: TrustTier,
    pub invited_by: NodeId,
    pub status: MembershipStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustTier {
    Public = 1,         // Routing only, never sees prompts
    InvitedFriend = 2,  // Can serve non-sensitive inference
    LocalOwned = 3,     // Full trust, sees all prompts
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MembershipStatus {
    Active,
    Suspended { reason: String, since: chrono::DateTime<chrono::Utc> },
    Banned { reason: String, vote_id: uuid::Uuid },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationToken {
    pub token_id: uuid::Uuid,
    pub mesh_id: MeshId,
    pub inviter_node_id: NodeId,
    pub offered_tier: TrustTier,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub signature: Signature,           // Signed by inviter
    pub consumed: bool,
}
```

### 2.2 Trust and Reputation

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeReputation {
    pub node_id: NodeId,
    pub mesh_id: MeshId,
    pub reputation_score: f64,          // [0.0, 1.0], starts at 0.5
    pub contribution_balance: f64,      // Positive = net contributor, negative = net consumer
    pub consecutive_negative_cycles: u32,
    pub free_rider_status: FreeRiderStatus,
    pub last_updated: chrono::DateTime<chrono::Utc>,
    pub history: Vec<ReputationUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FreeRiderStatus {
    Good,
    Warning { since_cycle: u32 },
    Deprioritized { since_cycle: u32 },
    Excluded { since_cycle: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationUpdate {
    pub cycle_number: u32,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub contribution_delta: f64,
    pub consumption_delta: f64,
    pub reputation_change: f64,         // Capped at +/- 0.1 per cycle
    pub new_reputation: f64,
}
```

### 2.3 Accounting Records

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountingRecord {
    pub record_id: uuid::Uuid,
    pub mesh_id: MeshId,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub record_type: AccountingType,
    pub contributor_node: NodeId,
    pub consumer_node: NodeId,
    pub amount: AccountingAmount,
    pub contributor_signature: Signature,
    pub consumer_signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountingType {
    InferenceServed,
    ModelHosting,
    BandwidthRelay,
    ModelTransfer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountingAmount {
    pub gpu_seconds: f64,
    pub ram_seconds: f64,
    pub bandwidth_bytes: u64,
    pub request_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionSummary {
    pub node_id: NodeId,
    pub mesh_id: MeshId,
    pub period: AccountingPeriod,
    pub total_contributed: AccountingAmount,
    pub total_consumed: AccountingAmount,
    pub balance: f64,                   // Normalized score
    pub rank_in_mesh: u32,              // 1 = top contributor
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountingPeriod {
    LastCycle,          // Last 15 minutes
    Last24Hours,
    Last7Days,
    Last30Days,
}
```

### 2.4 Sensitivity Classification

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PromptSensitivity {
    Sensitive,          // Local-only, never leaves tier-3 nodes
    NonSensitive,       // Can be routed to tier-2+ nodes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationResult {
    pub sensitivity: PromptSensitivity,
    pub confidence: f64,
    pub method: ClassificationMethod,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClassificationMethod {
    UserExplicit,       // User marked as private
    KeywordMatch,       // Matched sensitive keyword
    ContextBased,       // Conversation context suggests sensitive
    DefaultPolicy,      // No signals, using default
    UserOverride,       // User explicitly overrode classification
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitivityConfig {
    pub default_policy: PromptSensitivity,  // Default: NonSensitive
    pub sensitive_keywords: Vec<String>,
    pub sensitive_personas: Vec<String>,     // Agent personas that imply sensitivity
    pub private_repo_patterns: Vec<String>, // Regex for private code repos
}
```

### 2.5 Consensus

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub proposal_id: uuid::Uuid,
    pub mesh_id: MeshId,
    pub proposer: NodeId,
    pub proposal_type: ProposalType,
    pub description: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub votes: Vec<Vote>,
    pub status: ProposalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProposalType {
    AddModel { model_id: ModelId },
    RemoveModel { model_id: ModelId },
    BanNode { node_id: NodeId, reason: String },
    ConfigChange { key: String, old_value: String, new_value: String },
    TrustChange { node_id: NodeId, new_tier: TrustTier },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub voter: NodeId,
    pub decision: VoteDecision,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VoteDecision {
    Yes,
    No,
    Abstain,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProposalStatus {
    Open,
    Passed,
    Rejected,
    Expired,
}
```

### 2.6 Rate Limiting

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub base_requests_per_minute: u32,      // Default: 60
    pub mesh_aggregate_per_minute: u32,     // Default: 1000
    pub burst_multiplier: f64,              // Default: 2.0
    pub burst_window_secs: u32,             // Default: 30
    pub max_concurrent_requests: u32,       // Default: 5
    pub reputation_bonus_multiplier: f64,   // Default: 2.0 (max for top reputation)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRateState {
    pub node_id: NodeId,
    pub requests_this_minute: u32,
    pub minute_start: chrono::DateTime<chrono::Utc>,
    pub concurrent_requests: u32,
    pub in_burst: bool,
    pub burst_start: Option<chrono::DateTime<chrono::Utc>>,
    pub effective_limit: u32,               // Adjusted by reputation
}
```

### 2.7 Mesh Placement Plan (extends Phase 9A PlacementPlan)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshPlacementPlan {
    pub plan_id: uuid::Uuid,
    pub mesh_id: MeshId,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub leader_node: NodeId,
    pub solver_duration_ms: u64,
    pub cycle_number: u32,
    pub utility_scores: UtilityScores,
    pub placements: Vec<MeshModelPlacement>,
    pub node_incentives: HashMap<NodeId, NodeIncentive>,
    pub capacity_offers: HashMap<NodeId, CapacityOffer>,
    pub pending_transfers: Vec<MeshTransfer>,
    pub acknowledgments: HashMap<NodeId, PlanAcknowledgment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshModelPlacement {
    pub model_id: ModelId,
    pub instance_id: uuid::Uuid,
    pub assigned_nodes: Vec<NodeId>,
    pub protocol: ParallelismProtocol,
    pub estimated_tok_s: f32,
    pub trust_requirement: TrustTier,       // Minimum tier to route to this instance
    pub owner_node: NodeId,                 // Which owner's capacity is being used
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityOffer {
    pub node_id: NodeId,
    pub spare_ram_mb: u64,
    pub spare_vram_mb: u64,
    pub spare_gpu_percent: f64,
    pub max_models_willing_to_host: u32,
    pub available_hours_per_day: f64,       // e.g., 16.0 if machine sleeps 8h
    // Phase 15 extension point: tools this node offers to the mesh
    pub available_tools: Vec<String>,       // tool_ids available for mesh use (empty until Phase 15)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshTransfer {
    pub model_id: ModelId,
    pub source_node: NodeId,
    pub target_node: NodeId,
    pub size_mb: u64,
    pub priority: DownloadPriority,
    pub max_bandwidth_percent: u8,          // Default: 30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanAcknowledgment {
    pub node_id: NodeId,
    pub accepted: bool,
    pub rejection_reason: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub signature: Signature,
}
```

## 3. Algorithm Design

### 3.1 Mesh Solver (extends Phase 9A solver)

The mesh solver reuses Phase 9A's two-phase approach with additional constraints:

```pseudocode
function mesh_solve(inputs: MeshSolverInputs, timeout: 5.seconds()):
    // Phase A: Model Selection (same as 9A + trust-aware)
    selected = mesh_select_models(inputs)
    
    // Phase B: Node Assignment (same as 9A + trust/reputation/fairness constraints)
    plan = mesh_assign_models(selected, inputs)
    
    // Phase C (new): Validate mesh-specific constraints
    plan = validate_mesh_constraints(plan, inputs)
    
    // Phase D (new): Compute accounting impact
    plan = compute_accounting_impact(plan, inputs)
    
    return plan
```

### 3.2 Trust-Aware Model Selection

```pseudocode
function mesh_select_models(inputs):
    // Same as Phase 9A select_models, but with trust filtering
    candidates = inputs.catalog.filter(|m| !vetoed(m))
    
    // For each candidate, determine minimum trust tier needed
    for model in candidates:
        // Models that will serve sensitive prompts need tier-3 nodes
        if model.serves_sensitive_workload(inputs.demand):
            model.min_trust_tier = TrustTier::LocalOwned
        else:
            model.min_trust_tier = TrustTier::InvitedFriend
    
    // Filter candidates by available capacity at required trust tier
    feasible = candidates.filter(|m| {
        available_capacity_at_tier(m.min_trust_tier, inputs.nodes) >= m.requirements
    })
    
    // Score and select (same greedy knapsack as 9A)
    return greedy_knapsack(feasible, inputs.demand, inputs.preferences)
```

### 3.3 Reputation-Weighted Placement

```pseudocode
function mesh_assign_models(selected, inputs):
    plan = MeshPlacementPlan::new()
    
    // Sort models by size descending (same as 9A)
    for model in selected.sorted_by_size_desc():
        placement = find_mesh_placement(model, inputs)
        if placement.is_some():
            plan.placements.push(placement)
    
    return plan

function find_mesh_placement(model, inputs):
    candidates = []
    
    for node in inputs.nodes.where(|n| n.trust_tier >= model.min_trust_tier):
        if !fits_on_node(model, node): continue
        if !satisfies_constraints(model, node): continue
        
        score = score_mesh_placement(model, node, inputs)
        candidates.push((node, score))
    
    // Also consider multi-node splits (same as 9A affinity clustering)
    for cluster in inputs.affinity_clusters:
        if cluster_can_fit(model, cluster):
            score = score_mesh_split(model, cluster, inputs)
            candidates.push((cluster, score))
    
    return candidates.max_by_score()

function score_mesh_placement(model, node, inputs):
    // Base score from Phase 9A
    base = score_single_placement(model, node)
    
    // Reputation bonus: prefer high-reputation nodes for hosting
    reputation_bonus = inputs.reputations[node.id].reputation_score * 0.3
    
    // Fairness penalty: penalize if this owner already hosts >60% of shared models
    owner_share = count_models_hosted_by_owner(node.owner, inputs.current_plan) / total_models
    fairness_penalty = if owner_share > 0.6 { (owner_share - 0.6) * 2.0 } else { 0.0 }
    
    // Capacity offer respect: only use what the node offered
    if model.requirements > node.capacity_offer:
        return -1.0  // Cannot place here
    
    return base + reputation_bonus - fairness_penalty
```

### 3.4 Mesh Constraint Validation

```pseudocode
function validate_mesh_constraints(plan, inputs):
    // Constraint 1: Trust routing invariant
    for placement in plan.placements:
        for node in placement.assigned_nodes:
            if inputs.nodes[node].trust_tier < placement.trust_requirement:
                // VIOLATION: remove this placement
                plan.remove(placement)
                log_error("Trust violation: {} on tier-{} node", placement.model_id, node.trust_tier)
    
    // Constraint 2: Cross-owner fairness (no owner > 60%)
    owner_counts = plan.placements.group_by(|p| owner_of(p.assigned_nodes[0]))
    for (owner, count) in owner_counts:
        if count > plan.placements.len() * 0.6:
            // Redistribute excess to other owners
            redistribute_from_owner(owner, plan, inputs)
    
    // Constraint 3: Capacity offer honoring
    for node in plan.participating_nodes():
        allocated = plan.total_allocated_to(node)
        offered = inputs.capacity_offers[node.id]
        if allocated.ram > offered.spare_ram_mb OR allocated.vram > offered.spare_vram_mb:
            // Over-allocated: reduce to offered amount
            trim_allocation(node, plan, offered)
    
    // Constraint 4: Pareto improvement (same as 9A)
    plan = validate_pareto_improvement(plan, inputs)
    
    return plan
```

### 3.5 Sensitivity Classification Algorithm

```pseudocode
function classify_prompt(prompt, conversation_context, config):
    // Priority 1: User explicit override
    if conversation_context.user_marked_private:
        return ClassificationResult {
            sensitivity: Sensitive,
            method: UserExplicit,
            confidence: 1.0,
            reasons: ["User marked conversation as private"],
        }
    
    if conversation_context.user_marked_public:
        return ClassificationResult {
            sensitivity: NonSensitive,
            method: UserOverride,
            confidence: 1.0,
            reasons: ["User explicitly allowed mesh routing"],
        }
    
    // Priority 2: Keyword matching
    matched_keywords = config.sensitive_keywords.filter(|kw| prompt.contains_ci(kw))
    if !matched_keywords.is_empty():
        return ClassificationResult {
            sensitivity: Sensitive,
            method: KeywordMatch,
            confidence: 0.9,
            reasons: matched_keywords.map(|kw| format!("Contains sensitive keyword: {}", kw)),
        }
    
    // Priority 3: Context-based
    if conversation_context.persona in config.sensitive_personas:
        return ClassificationResult {
            sensitivity: Sensitive,
            method: ContextBased,
            confidence: 0.8,
            reasons: [format!("Persona '{}' implies sensitive content", conversation_context.persona)],
        }
    
    if prompt.references_private_repo(config.private_repo_patterns):
        return ClassificationResult {
            sensitivity: Sensitive,
            method: ContextBased,
            confidence: 0.85,
            reasons: ["References private repository code"],
        }
    
    // Priority 4: Default policy
    return ClassificationResult {
        sensitivity: config.default_policy,
        method: DefaultPolicy,
        confidence: 0.5,
        reasons: ["No sensitivity signals detected, using default policy"],
    }
```

### 3.6 Accounting and Reputation Computation

```pseudocode
function update_accounting(completed_request, contributor_node, consumer_node, mesh_id):
    // Compute contribution amount
    amount = AccountingAmount {
        gpu_seconds: completed_request.duration_secs * gpu_utilization_fraction,
        ram_seconds: completed_request.duration_secs * model_ram_fraction,
        bandwidth_bytes: completed_request.total_bytes_transferred,
        request_count: 1,
    }
    
    // Create record signed by both parties
    record = AccountingRecord {
        record_id: uuid::new_v4(),
        mesh_id,
        timestamp: now(),
        record_type: InferenceServed,
        contributor_node,
        consumer_node,
        amount,
        contributor_signature: sign(amount, contributor_key),
        consumer_signature: sign(amount, consumer_key),
    }
    
    // Append to local ledger
    ledger.append(record)
    
    // Replicate to all tier-3 nodes
    broadcast_to_tier3(record)

function compute_reputation(node_id, mesh_id, period: Last30Days):
    records = ledger.query(node_id, mesh_id, period)
    
    contributed = records.where(|r| r.contributor_node == node_id).sum_amounts()
    consumed = records.where(|r| r.consumer_node == node_id).sum_amounts()
    
    // Normalize to a balance score
    total_network_activity = ledger.total_activity(mesh_id, period)
    contribution_score = normalize(contributed, total_network_activity)
    consumption_score = normalize(consumed, total_network_activity)
    
    balance = contribution_score - consumption_score
    
    // Convert balance to reputation change (capped at +/- 0.1 per cycle)
    reputation_delta = (balance * 0.05).clamp(-0.1, 0.1)
    
    // Apply to current reputation
    current = get_reputation(node_id, mesh_id)
    new_reputation = (current.reputation_score + reputation_delta).clamp(0.0, 1.0)
    
    return ReputationUpdate {
        cycle_number: current_cycle(),
        reputation_change: reputation_delta,
        new_reputation,
        contribution_delta: contribution_score,
        consumption_delta: consumption_score,
    }
```

### 3.7 Free-Rider Detection and Enforcement

```pseudocode
function enforce_incentives(reputations, mesh_id):
    for rep in reputations.where(|r| r.mesh_id == mesh_id):
        // Check if node is exempt (consumer-only designation)
        if is_consumer_only_exempt(rep.node_id):
            continue
        
        if rep.contribution_balance < 0.0:
            rep.consecutive_negative_cycles += 1
        else:
            // Recovery: positive balance resets counter
            if rep.consecutive_negative_cycles > 0 AND rep.contribution_balance > 0.0:
                rep.consecutive_positive_cycles += 1
                if rep.consecutive_positive_cycles >= 2:
                    // Fully recovered
                    rep.free_rider_status = Good
                    rep.consecutive_negative_cycles = 0
                    rep.consecutive_positive_cycles = 0
            continue
        
        // Escalation ladder
        match rep.consecutive_negative_cycles:
            1..=2 => {
                // Grace period - no action
                rep.free_rider_status = Good
            }
            3 => {
                // Warning
                rep.free_rider_status = Warning { since_cycle: current_cycle() }
                send_notification(rep.node_id, "Your contribution balance is negative for 3 cycles. Consider contributing more resources.")
            }
            4..=5 => {
                // Deprioritized
                rep.free_rider_status = Deprioritized { since_cycle: current_cycle() }
                // Requests from this node go to back of queue
            }
            6.. => {
                // Excluded from shared model allocation
                rep.free_rider_status = Excluded { since_cycle: current_cycle() }
                // Node still connected but doesn't get mesh-optimized models
                remove_from_mesh_plan(rep.node_id)
            }
```

### 3.8 Rate Limiting Algorithm

```pseudocode
function check_rate_limit(node_id, rate_states, config, reputations):
    state = rate_states.get_or_create(node_id)
    
    // Reset minute counter if new minute
    if now() - state.minute_start > 1.minute():
        state.requests_this_minute = 0
        state.minute_start = now()
        state.in_burst = false
    
    // Compute effective limit (reputation-adjusted)
    reputation = reputations[node_id].reputation_score
    reputation_multiplier = 1.0 + (reputation - 0.5) * config.reputation_bonus_multiplier
    // reputation 0.5 = 1.0x, reputation 1.0 = 2.0x, reputation 0.0 = 0.0x
    effective_limit = (config.base_requests_per_minute as f64 * reputation_multiplier) as u32
    state.effective_limit = effective_limit
    
    // Check concurrent requests
    if state.concurrent_requests >= config.max_concurrent_requests:
        return RateLimitResult::Rejected { reason: "Max concurrent requests reached", retry_after_ms: 1000 }
    
    // Check per-minute limit with burst allowance
    if state.requests_this_minute >= effective_limit:
        if !state.in_burst:
            // Enter burst mode
            state.in_burst = true
            state.burst_start = Some(now())
            burst_limit = (effective_limit as f64 * config.burst_multiplier) as u32
            if state.requests_this_minute < burst_limit:
                state.requests_this_minute += 1
                return RateLimitResult::Allowed
        
        // Check if burst window expired
        if state.in_burst AND now() - state.burst_start.unwrap() > config.burst_window_secs.seconds():
            return RateLimitResult::Rejected {
                reason: "Rate limit exceeded (burst window expired)",
                retry_after_ms: remaining_in_minute(state) * 1000,
            }
        
        return RateLimitResult::Rejected { reason: "Rate limit exceeded", retry_after_ms: 5000 }
    
    // Anomaly detection: 10x normal rate
    historical_avg = compute_historical_avg_rate(node_id)
    if state.requests_this_minute > historical_avg * 10:
        alert_mesh_admins(node_id, "Anomalous request rate detected")
        return RateLimitResult::Throttled { delay_ms: 2000 }
    
    // Allow
    state.requests_this_minute += 1
    return RateLimitResult::Allowed

enum RateLimitResult {
    Allowed,
    Throttled { delay_ms: u64 },
    Rejected { reason: String, retry_after_ms: u64 },
}
```

### 3.9 Consensus Protocol

```pseudocode
function create_proposal(proposer, proposal_type, mesh_id):
    // Only tier-3 nodes can create proposals
    if get_trust_tier(proposer, mesh_id) != LocalOwned:
        return Error("Only local-owned nodes can create proposals")
    
    timeout = match proposal_type:
        BanNode { .. } => 1.hour(),     // Emergency: shorter timeout
        _ => 24.hours(),                 // Normal: 24h
    
    proposal = Proposal {
        proposal_id: uuid::new_v4(),
        mesh_id,
        proposer,
        proposal_type,
        created_at: now(),
        expires_at: now() + timeout,
        votes: vec![],
        status: Open,
    }
    
    // Broadcast to all tier-3 nodes
    broadcast_to_tier3(ProposalCreated(proposal))
    
    return proposal

function cast_vote(voter, proposal_id, decision):
    proposal = get_proposal(proposal_id)
    
    // Validate voter eligibility
    if get_trust_tier(voter, proposal.mesh_id) != LocalOwned:
        return Error("Only local-owned nodes can vote")
    
    if proposal.status != Open:
        return Error("Proposal is no longer open")
    
    if now() > proposal.expires_at:
        proposal.status = Expired
        return Error("Proposal has expired")
    
    // Record vote
    vote = Vote {
        voter,
        decision,
        timestamp: now(),
        signature: sign((proposal_id, decision), voter_key),
    }
    proposal.votes.push(vote)
    
    // Check if quorum + threshold met
    check_proposal_outcome(proposal)

function check_proposal_outcome(proposal):
    eligible_voters = count_tier3_nodes(proposal.mesh_id)
    votes_cast = proposal.votes.len()
    
    // Quorum check: >50% must participate
    if votes_cast as f64 / eligible_voters as f64 <= 0.5:
        return  // Not enough votes yet
    
    yes_votes = proposal.votes.filter(|v| v.decision == Yes).len()
    no_votes = proposal.votes.filter(|v| v.decision == No).len()
    
    // Approval threshold
    threshold = match proposal.proposal_type:
        BanNode { .. } => 0.5,      // Emergency: simple majority
        _ => 0.66,                   // Normal: 2/3 majority
    
    if yes_votes as f64 / votes_cast as f64 > threshold:
        proposal.status = Passed
        execute_proposal(proposal)
    else if no_votes as f64 / votes_cast as f64 >= (1.0 - threshold):
        proposal.status = Rejected
```

## 4. Interface Design

### 4.1 Tauri Commands (Frontend API)

```rust
/// Join a mesh using an invitation token
#[tauri::command]
pub async fn join_mesh(
    invitation_token: String,
    state: State<'_, MeshState>,
) -> Result<MeshMembership, String> {
    let token = InvitationToken::decode(&invitation_token)?;
    token.validate()?;  // Check expiry, signature
    state.membership.join(token).await.map_err(|e| e.to_string())
}

/// Create an invitation for someone to join your mesh
#[tauri::command]
pub async fn create_invitation(
    mesh_id: MeshId,
    offered_tier: TrustTier,
    expires_in_hours: u32,
    state: State<'_, MeshState>,
) -> Result<String, String> {
    let token = state.membership.create_invitation(mesh_id, offered_tier, expires_in_hours).await?;
    Ok(token.encode())  // Returns shareable string (URL or QR-encodable)
}

/// Get mesh network status
#[tauri::command]
pub async fn get_mesh_status(
    mesh_id: MeshId,
    state: State<'_, MeshState>,
) -> Result<MeshStatus, String> {
    Ok(MeshStatus {
        members: state.membership.list_members(mesh_id).await,
        current_plan: state.solver.current_plan(mesh_id).await,
        my_reputation: state.accounting.my_reputation(mesh_id).await,
        my_contribution: state.accounting.my_summary(mesh_id).await,
        active_proposals: state.consensus.active_proposals(mesh_id).await,
    })
}

/// Get my contribution/consumption breakdown
#[tauri::command]
pub async fn get_my_accounting(
    mesh_id: MeshId,
    period: AccountingPeriod,
    state: State<'_, MeshState>,
) -> Result<ContributionSummary, String> {
    state.accounting.my_summary_for_period(mesh_id, period).await.map_err(|e| e.to_string())
}

/// Update my capacity offer to the mesh
#[tauri::command]
pub async fn update_capacity_offer(
    mesh_id: MeshId,
    offer: CapacityOffer,
    state: State<'_, MeshState>,
) -> Result<(), String> {
    state.mesh_agent.update_offer(mesh_id, offer).await.map_err(|e| e.to_string())
}

/// Change trust tier for a node (only tier-3 can do this)
#[tauri::command]
pub async fn change_trust_tier(
    mesh_id: MeshId,
    target_node: NodeId,
    new_tier: TrustTier,
    state: State<'_, MeshState>,
) -> Result<(), String> {
    state.trust_manager.change_tier(mesh_id, target_node, new_tier).await.map_err(|e| e.to_string())
}

/// Cast a vote on a proposal
#[tauri::command]
pub async fn vote_on_proposal(
    proposal_id: uuid::Uuid,
    decision: VoteDecision,
    state: State<'_, MeshState>,
) -> Result<(), String> {
    state.consensus.cast_vote(proposal_id, decision).await.map_err(|e| e.to_string())
}

/// Leave a mesh gracefully
#[tauri::command]
pub async fn leave_mesh(
    mesh_id: MeshId,
    state: State<'_, MeshState>,
) -> Result<(), String> {
    state.membership.leave(mesh_id).await.map_err(|e| e.to_string())
}

/// Override prompt sensitivity for current message
#[tauri::command]
pub async fn override_sensitivity(
    sensitivity: PromptSensitivity,
    state: State<'_, MeshState>,
) -> Result<(), String> {
    state.classifier.set_override(sensitivity).await;
    Ok(())
}
```

### 4.2 Mesh Protocol Messages

```rust
/// Messages exchanged between mesh nodes (over Unified Mesh Transport)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshMessage {
    // Membership
    JoinRequest { token: InvitationToken, capabilities: NodeCapabilities },
    JoinAccepted { mesh_id: MeshId, member_list: Vec<MeshMember> },
    JoinRejected { reason: String },
    LeaveNotification { node_id: NodeId, mesh_id: MeshId },
    
    // Heartbeat (5-minute timeout for mesh)
    MeshHeartbeat { node_id: NodeId, mesh_id: MeshId, utilization: NodeUtilization },
    
    // Capacity offers
    CapacityUpdate { node_id: NodeId, offer: CapacityOffer },
    
    // Plan distribution (from leader)
    MeshPlanProposal { plan: MeshPlacementPlan },
    MeshPlanAck { plan_id: uuid::Uuid, node_id: NodeId, accepted: bool, reason: Option<String> },
    
    // Inference routing
    InferenceRequest { request_id: uuid::Uuid, model_id: ModelId, encrypted_prompt: Vec<u8>, sender: NodeId },
    InferenceResponse { request_id: uuid::Uuid, encrypted_response: Vec<u8>, accounting: AccountingAmount },
    
    // Accounting
    AccountingRecord(AccountingRecord),
    ReputationUpdate { node_id: NodeId, new_score: f64, cycle: u32 },
    
    // Consensus
    ProposalCreated(Proposal),
    VoteCast(Vote),
    ProposalOutcome { proposal_id: uuid::Uuid, status: ProposalStatus },
    
    // Model transfer
    TransferOffer { model_id: ModelId, size_mb: u64, available_bandwidth_mbps: u32 },
    TransferRequest { model_id: ModelId, requester: NodeId },
    TransferChunk { model_id: ModelId, offset: u64, data: Vec<u8>, checksum: u32 },
    TransferComplete { model_id: ModelId, final_checksum: String },
    
    // Rate limiting
    RateLimitExceeded { node_id: NodeId, retry_after_ms: u64 },
}

/// All mesh messages are:
/// 1. Serialized with MessagePack
/// 2. Signed with sender's Ed25519 key
/// 3. Encrypted with recipient's public key (for point-to-point)
///    OR encrypted with mesh shared key (for broadcasts)
/// 4. Transmitted via Unified Mesh Transport (Phase 10)
```

### 4.3 Local-Mesh Optimizer Interface

```rust
/// Interface between Local Optimizer (9A) and Mesh Optimizer (9B)
pub trait MeshLocalInterface: Send + Sync {
    /// Local optimizer reports its capacity offer
    async fn report_capacity_offer(&self, offer: CapacityOffer);
    
    /// Local optimizer reports its demand request
    async fn report_demand_request(&self, demand: MeshDemandRequest);
    
    /// Mesh optimizer proposes a placement for this node
    /// Local optimizer can accept or reject
    async fn receive_mesh_proposal(&self, proposal: NodeProposal) -> ProposalResponse;
    
    /// Mesh optimizer notifies that a model is available on the mesh
    async fn notify_mesh_model_available(&self, model_id: ModelId, serving_node: NodeId);
}

pub struct MeshDemandRequest {
    pub models_wanted: Vec<ModelId>,        // Models we want but can't run locally
    pub task_types_needed: Vec<TaskType>,   // Task types we need better models for
    pub min_quality_threshold: f64,         // Don't bother if quality gain < this
}

pub struct NodeProposal {
    pub model_id: ModelId,
    pub action: ProposalAction,
    pub resource_requirement: ResourceAllocation,
}

pub enum ProposalAction {
    HostModel,              // "Please load this model for the mesh"
    UnloadModel,            // "Please free this capacity"
    AcceptInference,        // "Please serve inference for this model"
}

pub enum ProposalResponse {
    Accepted,
    Rejected { reason: String },
}
```

## 5. Cross-Network Model Transfer

### 5.1 Transfer Coordination

```pseudocode
function coordinate_mesh_transfer(model_id, target_node, mesh_nodes):
    // Find peers that have this model
    sources = mesh_nodes.filter(|n| n.has_model_downloaded(model_id) AND n.is_online)
    
    if sources.is_empty():
        // No mesh peer has it - fall back to internet download
        return fallback_to_internet_download(model_id, target_node)
    
    // Sort sources by: bandwidth to target (desc), current load (asc)
    ranked_sources = sources.sorted_by(|s| {
        let bw = measure_bandwidth(s, target_node)
        let load = s.utilization.gpu_percent
        bw * (1.0 - load / 100.0)  // Prefer high-bandwidth, low-load nodes
    })
    
    model_size = catalog.get(model_id).requirements.disk_size_mb
    
    // Single source if model is small or only one source
    if model_size < 2000 OR ranked_sources.len() == 1:  // < 2GB
        return single_source_transfer(model_id, ranked_sources[0], target_node)
    
    // Parallel chunk transfer for large models
    return parallel_chunk_transfer(model_id, ranked_sources, target_node)

function parallel_chunk_transfer(model_id, sources, target):
    model_size = catalog.get(model_id).disk_size_mb * 1024 * 1024  // bytes
    chunk_size = 64 * 1024 * 1024  // 64MB chunks
    num_chunks = ceil(model_size / chunk_size)
    
    // Distribute chunks across sources (round-robin weighted by bandwidth)
    assignments = distribute_chunks(num_chunks, sources)
    
    // Start parallel transfers with bandwidth limiting (30% max per link)
    handles = []
    for (source, chunk_range) in assignments:
        handle = spawn_transfer(source, target, model_id, chunk_range, max_bandwidth: 30%)
        handles.push(handle)
    
    // Wait for all chunks
    results = join_all(handles).await
    
    // Verify integrity
    assembled_hash = compute_sha256(assembled_file)
    expected_hash = catalog.get(model_id).checksum_sha256
    
    if assembled_hash != expected_hash:
        delete(assembled_file)
        return Error(TransferCorrupted)
    
    // Record accounting: each source gets credit for bytes sent
    for (source, bytes_sent) in transfer_stats:
        record_contribution(source, target, bandwidth_bytes: bytes_sent)
    
    return Ok(())
```

## 6. Error Handling

### 6.1 Leader Failure

```pseudocode
function handle_leader_failure(mesh_id):
    // Detected when leader misses 2 consecutive heartbeats (10 minutes)
    
    // Deterministic leader election: highest reputation + longest uptime among tier-3
    tier3_nodes = get_tier3_nodes(mesh_id).filter(|n| n.is_online)
    
    new_leader = tier3_nodes.max_by(|n| {
        n.reputation_score * 0.7 + n.uptime_days / 365.0 * 0.3
    })
    
    if new_leader == my_node_id:
        // I am the new leader
        log("Taking over as mesh optimizer leader for mesh {}", mesh_id)
        start_mesh_optimizer_loop(mesh_id)
        broadcast(LeaderAnnouncement { new_leader: my_node_id, mesh_id })
    
    // No election needed - it's deterministic, all nodes compute the same result
```

### 6.2 Plan Rejection Handling

```pseudocode
function handle_plan_rejections(plan, acknowledgments):
    rejections = acknowledgments.filter(|a| !a.accepted)
    
    if rejections.is_empty():
        return plan  // All accepted
    
    // Re-solve without the rejected placements
    for rejection in rejections:
        affected_placements = plan.placements.filter(|p| p.assigned_nodes.contains(rejection.node_id))
        for placement in affected_placements:
            plan.remove(placement)
            // Try to place on alternative nodes
            alternative = find_mesh_placement(placement.model_id, inputs_excluding(rejection.node_id))
            if alternative.is_some():
                plan.add(alternative)
            else:
                log("Could not find alternative placement for {} after rejection by {}",
                    placement.model_id, rejection.node_id)
    
    // Re-broadcast updated plan
    broadcast(MeshPlanProposal { plan })
```

### 6.3 Accounting Dispute Resolution

```pseudocode
function handle_accounting_dispute(record, disputing_node):
    // A node claims a record is incorrect
    // Resolution: majority of tier-3 nodes validate
    
    // Check signatures
    if !verify_signature(record.contributor_signature, record.contributor_node):
        // Invalid signature - record is fraudulent
        remove_record(record)
        penalize_reputation(record.contributor_node, -0.05)
        return
    
    if !verify_signature(record.consumer_signature, record.consumer_node):
        remove_record(record)
        penalize_reputation(record.consumer_node, -0.05)
        return
    
    // Both signatures valid - record stands
    // Disputes about amounts are resolved by the signed data
    log("Dispute rejected: record {} has valid dual signatures", record.record_id)
```

## 7. State Persistence

```rust
/// Mesh-specific persisted state (in addition to Phase 9A state)
pub struct PersistedMeshState {
    // Identity
    pub identity: MeshIdentity,
    
    // Memberships (can be in multiple meshes)
    pub memberships: Vec<MeshMembership>,
    
    // Accounting ledger (append-only)
    pub accounting_ledger: Vec<AccountingRecord>,
    
    // Reputation snapshots
    pub reputations: HashMap<(MeshId, NodeId), NodeReputation>,
    
    // Active proposals
    pub proposals: Vec<Proposal>,
    
    // Sensitivity config
    pub sensitivity_config: SensitivityConfig,
    
    // Capacity offer (what we're willing to share)
    pub capacity_offer: HashMap<MeshId, CapacityOffer>,
    
    // Known mesh members (for reconnection)
    pub known_members: HashMap<MeshId, Vec<MeshMember>>,
}
```

## 8. Configuration

```rust
pub struct MeshOptimizerConfig {
    // Timing
    pub optimization_interval_secs: u64,        // Default: 900 (15 min)
    pub heartbeat_interval_secs: u64,           // Default: 60
    pub heartbeat_timeout_secs: u64,            // Default: 300 (5 min)
    pub solver_timeout_ms: u64,                 // Default: 5000
    pub plan_ack_timeout_secs: u64,             // Default: 30
    
    // Scale
    pub max_nodes_per_mesh: u32,                // Default: 100
    pub max_model_candidates: u32,              // Default: 50
    
    // Trust
    pub trust_promotion_min_days: u32,          // Default: 7
    pub trust_promotion_min_reputation: f64,    // Default: 0.7
    
    // Incentives
    pub free_rider_grace_cycles: u32,           // Default: 2
    pub free_rider_warning_cycle: u32,          // Default: 3
    pub free_rider_deprioritize_cycle: u32,     // Default: 4
    pub free_rider_exclude_cycle: u32,          // Default: 6
    pub free_rider_recovery_cycles: u32,        // Default: 2
    pub reputation_max_change_per_cycle: f64,   // Default: 0.1
    pub reputation_initial: f64,                // Default: 0.5
    
    // Rate limiting
    pub rate_limit: RateLimitConfig,
    
    // Fairness
    pub max_owner_hosting_share: f64,           // Default: 0.60
    
    // Transfer
    pub max_transfer_bandwidth_percent: u8,     // Default: 30
    pub max_concurrent_transfers: u32,          // Default: 10
    pub transfer_chunk_size_mb: u32,            // Default: 64
    
    // Consensus
    pub proposal_timeout_hours: u32,            // Default: 24
    pub emergency_timeout_hours: u32,           // Default: 1
    pub quorum_threshold: f64,                  // Default: 0.50
    pub approval_threshold: f64,                // Default: 0.66
    pub emergency_approval_threshold: f64,      // Default: 0.50
    
    // Privacy
    pub default_sensitivity: PromptSensitivity, // Default: NonSensitive
    pub request_padding_enabled: bool,          // Default: true
    pub padding_block_size_bytes: u32,          // Default: 1024
    
    // Accounting
    pub accounting_retention_days: u32,         // Default: 90
    pub replication_target_nodes: u32,          // Default: 3 (or all tier-3 if fewer)
}
```

## 9. TypeScript Frontend Types

```typescript
interface MeshStatus {
  meshId: string;
  meshName: string;
  members: MeshMember[];
  currentPlan: MeshPlacementPlan | null;
  myReputation: number;
  myContribution: ContributionSummary;
  activeProposals: Proposal[];
  isLeader: boolean;
}

interface MeshMember {
  nodeId: string;
  hostname: string;
  trustTier: 'local_owned' | 'invited_friend' | 'public';
  isOnline: boolean;
  reputation: number;
  freeRiderStatus: 'good' | 'warning' | 'deprioritized' | 'excluded';
  joinedAt: string;
}

interface ContributionSummary {
  nodeId: string;
  period: string;
  gpuSecondsContributed: number;
  gpuSecondsConsumed: number;
  bandwidthContributed: number;
  bandwidthConsumed: number;
  requestsServed: number;
  requestsConsumed: number;
  balance: number;
  rankInMesh: number;
}

interface Proposal {
  proposalId: string;
  proposalType: string;
  description: string;
  proposer: string;
  createdAt: string;
  expiresAt: string;
  votes: { voter: string; decision: 'yes' | 'no' | 'abstain' }[];
  status: 'open' | 'passed' | 'rejected' | 'expired';
  quorumMet: boolean;
  currentApproval: number;
}

interface SensitivityOverride {
  sensitivity: 'sensitive' | 'non_sensitive';
}

interface InvitationLink {
  token: string;
  meshName: string;
  inviterName: string;
  offeredTier: string;
  expiresAt: string;
  qrCodeDataUrl: string;
}
```

## 10. Testing Strategy

### 10.1 Property-Based Tests

| Property | Description | Generator Strategy |
|----------|-------------|-------------------|
| Trust routing invariant | Sensitive prompts never reach tier < 3 | Random prompts + random node tiers |
| Pareto improvement | Every included node benefits | Random multi-owner networks |
| Free-rider escalation | Correct escalation ladder timing | Random contribution sequences |
| Accounting integrity | Dual-signed records match | Random request sequences |
| Rate limit enforcement | Never exceed limit beyond burst | Random request bursts |
| Consensus validity | Only valid votes pass proposals | Random vote sequences |
| Capacity offer honoring | Never over-allocate | Random offers + random placements |
| Reputation bounds | Always in [0.0, 1.0], max delta 0.1 | Random reputation histories |
| Transfer integrity | Corrupted transfers rejected | Inject corruption in chunks |
| Classification completeness | Every request classified before routing | Random prompts + configs |

### 10.2 Integration Tests

| Test | Scenario |
|------|----------|
| Mesh join flow | Create invitation, join, verify membership |
| Trust tier routing | Sensitive prompt stays local, non-sensitive routes to tier-2 |
| Free-rider detection | Simulate 6 cycles of consumption without contribution |
| Leader failover | Kill leader, verify new leader takes over |
| Plan rejection | Local optimizer rejects, verify re-solve |
| Consensus vote | Create proposal, collect votes, verify outcome |
| Cross-mesh transfer | Transfer model between mesh peers |
| Rate limit burst | Send burst, verify throttling after window |
| Multi-mesh | Node in 2 meshes, verify independent operation |
| Mesh + local independence | Kill mesh, verify local optimizer unaffected |

## 11. Migration and Dependencies

### 11.1 Dependencies

- **Phase 9A (Local Network Optimizer)**: Reuses solver algorithm, node registry, model catalog
- **Phase 10 (Unified Mesh Transport)**: Required for inter-node communication across mesh
- **Phase 6 (Reticulum Channel)**: One possible transport adapter for mesh communication
- **Phase 7 (Hardware Detection)**: Node capability reporting
- **Phase 4 (RL Policy)**: Demand signal input, model set change notification

### 11.2 Incremental Adoption

1. Phase 9A must be working first (local optimizer)
2. Phase 10 provides transport (can start with simple TCP for testing)
3. Mesh features activate only when user explicitly joins a mesh
4. All mesh code paths are behind feature flags
5. Local-only users never load mesh modules
