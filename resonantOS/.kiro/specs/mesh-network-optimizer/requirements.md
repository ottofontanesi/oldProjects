# Requirements: Mesh Network Optimizer (Phase 9B)

## Overview

The Mesh Network Optimizer extends the Local Network Optimizer (Phase 9A) to multi-user mesh networks where nodes belong to different owners with varying trust levels. It solves the same Model Placement Problem P with additional constraints: tiered trust (local-owned/invited-friend/public), privacy-aware routing (sensitive prompts stay local), network economics/accounting (contribution tracking, reputation), stronger incentive enforcement, consensus protocol, rate limiting, and cross-network model transfer.

The Mesh Optimizer reuses the same solver algorithm as Phase 9A but operates at a different scale (up to 100 nodes), frequency (15-minute cycles), and trust model (earned trust vs implicit trust). It runs as an opt-in layer on top of the Local Optimizer — users who only want local multi-machine optimization never interact with mesh features.

## Key Design Decisions (from architecture discussions)

- Same algorithm as 9A, different constraint parameters (latency thresholds, trust tiers, optimization frequency)
- Two separate optimizers: Local (always runs, real-time) + Mesh (opt-in, periodic 15-min cycles)
- Interface between them: Local exports "capacity offer" + "demand request"; Mesh collects all and produces global plan
- Trust tiers: local-owned (full trust), invited-friend (sees prompts, reputation-staked), public (routing only, never sees plaintext)
- Pareto improvement enforced per-node: every participant must benefit or gets excluded
- Prompt sensitivity classification: sensitive=local-only, non-sensitive=mesh-eligible
- Network accounting: contribution tracking (GPU-hours given vs consumed) feeds reputation and priority routing
- Stronger incentive enforcement: auto-exclude nodes that free-ride for 3+ consecutive cycles
- Consensus: lightweight majority vote for mesh-wide decisions (model selection for shared capacity)

## User Stories

### US-1: Mesh Participant
As a user with a powerful desktop, I want to join a mesh network with friends so that I gain access to models running on their hardware (e.g., a 70B model split across 3 machines) while contributing my GPU for their requests, creating a cooperative AI network that benefits everyone.

### US-2: Privacy-Conscious User
As a mesh participant, I want my sensitive prompts (personal journals, financial questions, private code) to never leave my local network, while non-sensitive requests (general knowledge, public code help) can be routed to mesh nodes for faster/better responses.

### US-3: Trust Management
As a mesh owner, I want to invite specific friends by sharing an invitation link, assign them trust tiers (full access vs routing-only), and revoke access if they misbehave, so I maintain control over who sees my prompts and uses my hardware.

### US-4: Fair Contribution
As a mesh participant who contributes significant GPU time, I want priority routing when the network is busy, and I want free-riders (who consume but never contribute) to be automatically deprioritized or excluded after repeated cycles of imbalance.

### US-5: Network Economics Transparency
As a mesh participant, I want to see my contribution balance (GPU-hours given vs consumed), my reputation score, and how the network values my node, so I understand the economics of my participation.

### US-6: Resilient Mesh
As a mesh participant, I want the network to handle node churn gracefully — when participants go offline, the mesh re-optimizes without disrupting my active inference, and when they come back, the mesh incorporates them smoothly.

### US-7: Cross-Network Model Sharing
As a mesh participant who has already downloaded a large model, I want other mesh nodes to be able to fetch it from me over the mesh transport (bandwidth permitting) rather than each downloading independently from the internet.

## Functional Requirements

### FR-1: Mesh Membership and Identity
- FR-1.1: Each mesh node has a persistent cryptographic identity (Ed25519 keypair) generated on first join
- FR-1.2: Mesh membership is invitation-based: existing members generate time-limited invitation tokens (default 24h expiry)
- FR-1.3: Invitation tokens encode: inviter's node ID, trust tier offered, mesh ID, expiry timestamp, signature
- FR-1.4: A node can belong to multiple meshes simultaneously (e.g., "family mesh" + "work mesh")
- FR-1.5: Mesh has a human-readable name and a unique mesh ID (UUID)
- FR-1.6: Node departure: voluntary (graceful leave with notification) or involuntary (heartbeat timeout after 5 minutes for mesh vs 30s for local)
- FR-1.7: Mesh size limit: up to 100 nodes per mesh (configurable)
- FR-1.8: Node retirement (permanent disappearance): a node can request full data removal from the mesh — all accounting records referencing it are anonymized (replaced with "retired-node-XXXX"), reputation history deleted, membership record purged. Other nodes' aggregate balances are recalculated without the retired node. This is irreversible.
- FR-1.9: Retirement propagation: all tier-3 nodes must acknowledge the retirement request. Retirement completes only when all replicas confirm data removal.

### FR-2: Trust Tiers
- FR-2.1: Three trust tiers with distinct capabilities:
  - **Local-owned** (tier 3): Full trust. Node owner's own machines. Can see all prompts, host any model, participate in all inference. Equivalent to Phase 9A local nodes.
  - **Invited-friend** (tier 2): Medium trust. Can see non-sensitive prompts routed to them. Can host models and serve inference. Reputation-staked (bad behavior = demotion).
  - **Public** (tier 1): Routing only. Never sees prompt plaintext. Can relay encrypted packets between trusted nodes. Cannot host inference models. Useful for extending mesh reach.
- FR-2.2: Trust tier assigned at invitation time, can be upgraded/downgraded by the inviter
- FR-2.3: Trust tier determines what the optimizer can route to that node:
  - Tier 3: any prompt
  - Tier 2: non-sensitive prompts only
  - Tier 1: no prompts (routing/relay only)
- FR-2.4: Trust demotion triggers: reputation score drops below threshold, manual demotion by inviter, consensus vote by mesh majority
- FR-2.5: Trust promotion requires: explicit action by a tier-3 node owner + minimum reputation score + minimum participation time (default 7 days)

### FR-3: Prompt Sensitivity Classification
- FR-3.1: Each inference request is classified as sensitive or non-sensitive before routing
- FR-3.2: Classification methods (configurable, multiple can be active):
  - **User-explicit**: user marks a conversation as "private" in UI
  - **Keyword-based**: configurable keyword list (default: personal names, financial terms, passwords, "secret", "private")
  - **Context-based**: conversations with personal assistant persona, journal entries, code from private repos
  - **Default policy**: configurable (default: "assume non-sensitive unless classified otherwise")
- FR-3.3: Sensitive prompts are NEVER routed to tier-1 or tier-2 nodes — local-only (tier 3)
- FR-3.4: Non-sensitive prompts can be routed to tier-2 and tier-3 nodes
- FR-3.5: Classification is done locally before any routing decision — the prompt never leaves the local node until classification is complete
- FR-3.6: Users can override classification per-message ("send this to mesh anyway" or "keep this local")

### FR-4: Network Economics and Accounting
- FR-4.1: Track per-node contribution metrics:
  - GPU-seconds contributed (inference time served for other nodes' requests)
  - RAM-seconds contributed (model hosting time for shared models)
  - Bandwidth contributed (bytes relayed/transferred for others)
  - Requests served (count of inference requests handled for others)
- FR-4.2: Track per-node consumption metrics:
  - GPU-seconds consumed (inference time used on other nodes)
  - Bandwidth consumed (model downloads from peers, inference traffic)
  - Requests consumed (count of requests routed to other nodes)
- FR-4.3: Compute contribution balance: contribution_score - consumption_score (normalized)
- FR-4.4: Contribution balance feeds into:
  - Reputation score (rolling 30-day weighted average)
  - Priority routing (higher reputation = lower latency queue position when network is busy)
  - Incentive enforcement (negative balance for 3+ cycles = warning, then deprioritization)
- FR-4.5: Accounting is append-only and cryptographically signed by both parties (contributor and consumer) to prevent disputes
- FR-4.6: Accounting data is replicated across all tier-3 nodes for auditability
- FR-4.7: No real money involved — purely reputation-based economy

### FR-5: Incentive Enforcement
- FR-5.1: Pareto improvement constraint (same as 9A): every participating node must have utility_with_mesh >= utility_alone
- FR-5.2: Free-rider detection: if a node's contribution_balance is negative for 3 consecutive optimization cycles (45 minutes), flag as potential free-rider
- FR-5.3: Free-rider response (escalating):
  - Cycle 1-2 negative: no action (grace period, node may be warming up)
  - Cycle 3 negative: warning notification sent to node owner
  - Cycle 4+ negative: deprioritized routing (requests from this node go to back of queue)
  - Cycle 6+ negative: auto-exclude from mesh optimization (node still connected but not allocated shared models)
- FR-5.4: Recovery: if a previously-flagged node's balance becomes positive for 2 consecutive cycles, restore full participation
- FR-5.5: Exemptions: nodes explicitly marked as "consumer-only" by a tier-3 owner (e.g., a phone that can't contribute much) are exempt from free-rider detection but get lowest priority
- FR-5.6: Incentive report: each node can see its own contribution/consumption breakdown and standing

### FR-6: Mesh Optimizer Solver (extends Phase 9A)
- FR-6.1: Same two-phase solver as 9A (Phase A: WHAT models + Phase B: WHERE to place)
- FR-6.2: Additional constraints beyond 9A:
  - Trust routing constraint: sensitive prompts only to tier-3 nodes
  - Reputation-weighted placement: prefer high-reputation nodes for critical model hosting
  - Cross-owner fairness: no single owner's nodes should host >60% of shared models (prevent single point of failure)
  - Geographic/latency awareness: group nodes by measured latency for split decisions
- FR-6.3: Optimization frequency: every 15 minutes (configurable) + event-triggered (node join/leave, significant demand shift)
- FR-6.4: Solver timeout: 5 seconds (mesh has more nodes, needs more time than local's 2s)
- FR-6.5: Scale target: produce valid plans for up to 100 nodes and 50 model candidates within timeout
- FR-6.6: The mesh optimizer's output is a SUGGESTION to each local optimizer — local optimizers can reject placements that violate their local constraints

### FR-7: Consensus Protocol
- FR-7.1: Lightweight majority vote for mesh-wide decisions that affect all participants:
  - Adding a new model to the shared catalog
  - Removing a node from the mesh (ban)
  - Changing mesh-wide configuration (optimization frequency, trust policies)
- FR-7.2: Voting eligibility: only tier-3 (local-owned) nodes can vote
- FR-7.3: Quorum: >50% of eligible voters must participate for vote to be valid
- FR-7.4: Approval threshold: >66% of votes must be "yes" for proposal to pass
- FR-7.5: Vote timeout: 24 hours from proposal creation (configurable)
- FR-7.6: Automatic proposals: optimizer can propose model additions based on demand analysis (still requires vote)
- FR-7.7: Emergency actions (node ban for malicious behavior) require only >50% approval and 1-hour timeout

### FR-8: Rate Limiting and DDoS Protection
- FR-8.1: Per-node request rate limit: configurable max requests per minute from any single node (default: 60)
- FR-8.2: Per-mesh aggregate rate limit: total requests per minute across all nodes (default: 1000)
- FR-8.3: Burst allowance: 2x rate limit for 30 seconds, then enforce strictly
- FR-8.4: Rate limit response: HTTP 429 equivalent with retry-after hint
- FR-8.5: Reputation-based rate limits: higher reputation nodes get higher limits (up to 2x base)
- FR-8.6: Anomaly detection: if a node suddenly sends 10x its normal request rate, temporarily throttle and alert mesh admins (tier-3 nodes)
- FR-8.7: Connection limits: max 5 concurrent inference requests per node (configurable)
- FR-8.8: Token-weighted rate limiting: in addition to request count, enforce per-node token budget (default: 30,000 tokens/minute) and compute budget (default: 45 compute-seconds/minute). Prevents resource exhaustion via few large requests.
- FR-8.9: Dual enforcement: a request is rejected if ANY limit is exceeded (request count OR token budget OR compute budget OR concurrent limit)

### FR-9: Cross-Network Model Transfer
- FR-9.1: When mesh optimizer decides to place a model on a node that doesn't have it, check if any mesh peer already has it downloaded
- FR-9.2: Peer-to-mesh transfer: if a peer has the model, transfer via mesh transport (encrypted, bandwidth-aware)
- FR-9.3: Transfer priority: local peer (LAN) > mesh peer (WAN) > internet source (HuggingFace/Ollama)
- FR-9.4: Bandwidth fairness: model transfers between mesh nodes are throttled to not impact active inference (max 30% of available bandwidth)
- FR-9.5: Transfer integrity: SHA-256 verification after transfer (same as Phase 9A downloads)
- FR-9.6: Transfer accounting: bandwidth used for model transfer counts toward the receiver's consumption and sender's contribution
- FR-9.7: Parallel chunk transfer: large models can be fetched from multiple peers simultaneously (different chunks from different sources)

### FR-10: Mesh-Local Optimizer Interface
- FR-10.1: Local optimizer exports to mesh: "capacity offer" — spare RAM/VRAM/GPU-time willing to share with mesh
- FR-10.2: Local optimizer exports to mesh: "demand request" — models/capabilities the local network wants access to but can't provide alone
- FR-10.3: Mesh optimizer produces: "global placement plan" — suggested model placements across all mesh nodes
- FR-10.4: Local optimizer can REJECT mesh suggestions that would violate local constraints (memory headroom, stability, user preferences)
- FR-10.5: Rejection feedback: local optimizer reports WHY it rejected, so mesh optimizer can find alternatives
- FR-10.6: Capacity offer is dynamic: changes based on local utilization (busy local network offers less to mesh)
- FR-10.7: Mesh plan execution requires local optimizer acknowledgment before proceeding

### FR-11: Mesh Observability
- FR-11.1: Network-wide metrics: total nodes, total capacity, total loaded models, aggregate utility
- FR-11.2: Per-node metrics visible to that node's owner: contribution balance, reputation, requests served/consumed
- FR-11.3: Per-node metrics visible to mesh admins (tier-3): all of the above + trust tier + free-rider status
- FR-11.4: Mesh health indicators: average latency between nodes, node churn rate, optimization success rate
- FR-11.5: Economic dashboard: contribution leaderboard (anonymized unless opted-in), network-wide balance distribution
- FR-11.6: Audit trail: all mesh-wide decisions (votes, bans, trust changes) logged with timestamps and signatures
- FR-11.7: Privacy: no node can see another node's prompt content or inference results — only metadata (request count, model used, latency)

## Non-Functional Requirements

### NFR-1: Performance
- NFR-1.1: Mesh optimization solve time < 5 seconds for up to 100 nodes and 50 model candidates
- NFR-1.2: Mesh plan propagation to all nodes within 10 seconds of solve completion
- NFR-1.3: Prompt sensitivity classification adds < 5ms to routing decision
- NFR-1.4: Rate limiting check adds < 1ms per request
- NFR-1.5: Accounting update adds < 2ms per completed request

### NFR-2: Scalability
- NFR-2.1: Support up to 100 nodes per mesh
- NFR-2.2: Support up to 50 model candidates in catalog
- NFR-2.3: Support up to 10 concurrent model transfers across mesh
- NFR-2.4: Accounting storage grows linearly: ~1KB per node per day

### NFR-3: Security
- NFR-3.1: All mesh communication encrypted end-to-end (TLS 1.3 minimum)
- NFR-3.2: Node identity verified via Ed25519 signatures on every message
- NFR-3.3: Invitation tokens are single-use and time-limited
- NFR-3.4: Prompt content never visible to tier-1 (public) nodes under any circumstance
- NFR-3.5: Accounting records tamper-evident (signed by both parties)
- NFR-3.6: No single node can forge another node's contribution records

### NFR-4: Reliability
- NFR-4.1: Mesh continues operating when up to 30% of nodes are offline
- NFR-4.2: No single point of failure — mesh optimizer can run on any tier-3 node
- NFR-4.3: Node departure handled within 5 minutes (mesh heartbeat timeout)
- NFR-4.4: Consensus votes survive node restarts (persisted locally)
- NFR-4.5: Accounting data replicated across all tier-3 nodes (survives any single node loss)

### NFR-5: Privacy
- NFR-5.1: Sensitive prompt content never leaves the local network
- NFR-5.2: Mesh nodes cannot infer prompt content from metadata (request size, timing) — padding applied
- NFR-5.3: Contribution accounting tracks only aggregate metrics, not individual prompt content
- NFR-5.4: Node owners can delete their accounting history (right to be forgotten) — affects only their own records

### NFR-6: Modularity
- NFR-6.1: Mesh optimizer reuses Phase 9A solver with different constraint parameters
- NFR-6.2: Trust model is pluggable (can swap tier definitions without changing solver)
- NFR-6.3: Accounting system is independent of optimizer (can be disabled without affecting placement)
- NFR-6.4: Consensus protocol is independent (can be disabled for small trusted meshes)
- NFR-6.5: Mesh features are entirely opt-in — local-only users never see mesh code paths

## Correctness Properties

### Property 1: Trust routing invariant
Sensitive prompts SHALL NEVER be routed to nodes with trust tier < 3 (local-owned). This is a hard security constraint that cannot be overridden by the optimizer.

### Property 2: Pareto improvement (mesh-extended)
For every node included in the mesh placement plan, utility_with_mesh >= utility_alone. Nodes that cannot benefit are excluded from the plan.

### Property 3: Free-rider enforcement
A node with negative contribution balance for 3+ consecutive cycles SHALL be flagged. A node flagged for 6+ cycles SHALL be excluded from shared model allocation.

### Property 4: Accounting integrity
For every completed inference request routed across mesh nodes, both the contributor (server) and consumer (requester) SHALL record matching accounting entries signed by both parties.

### Property 5: Rate limit enforcement
No node SHALL exceed its configured rate limit for more than the burst allowance window (30 seconds). After burst, requests SHALL be rejected until the rate drops below the limit.

### Property 6: Consensus validity
A mesh-wide decision SHALL only be enacted if: quorum is met (>50% participation) AND approval threshold is met (>66% yes votes) AND vote timeout has not expired.

### Property 7: Capacity offer honoring
The mesh optimizer SHALL NOT allocate more resources on a node than that node's local optimizer has offered as spare capacity. Local optimizer rejections are final.

### Property 8: Transfer integrity
Every model transferred between mesh nodes SHALL pass SHA-256 integrity verification. Corrupted transfers SHALL be rejected and retried from an alternative source.

### Property 9: Privacy classification completeness
Every inference request SHALL be classified (sensitive or non-sensitive) BEFORE any routing decision is made. Unclassified requests SHALL be treated as sensitive (fail-safe).

### Property 10: Invitation security
Invitation tokens SHALL be single-use (consumed on acceptance), time-limited (rejected after expiry), and cryptographically bound to the inviter's identity (cannot be forged).

### Property 11: Mesh independence from local
The mesh optimizer's failure or unavailability SHALL NOT affect the local optimizer's operation. Local inference continues uninterrupted if mesh is down.

### Property 12: Reputation bounds
Reputation scores SHALL be bounded in [0.0, 1.0]. New nodes start at 0.5 (neutral). Reputation changes are capped at +/- 0.1 per cycle to prevent manipulation.

### Property 13: No single point of failure
The mesh SHALL continue operating (accepting requests, serving inference) when any single node (including the current optimizer leader) goes offline, within the 5-minute heartbeat timeout.

### Property 14: Bandwidth fairness
Cross-mesh model transfers SHALL NOT consume more than 30% of available bandwidth on any link, ensuring active inference is not degraded.

### Property 15: Deterministic accounting
Given the same sequence of inference requests and their outcomes, all tier-3 nodes SHALL compute identical contribution balances for every node in the mesh.
