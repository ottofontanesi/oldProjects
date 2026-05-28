# Requirements: Distributed Agent Execution (Phase 15)

## Overview

Distributed Agent Execution enables agentic workloads (multi-step AI workflows with tool calls) to execute across multiple network nodes. Instead of an agent running entirely on one machine, its execution plan is decomposed into a dependency graph where independent steps run in parallel on different nodes, each step routed to the node that has the required model AND tools.

This extends the optimizer's scope from "place models on nodes" to "place models + agents + tools on nodes" — the same optimization problem P with an expanded resource vocabulary.

## Key Design Decisions

- Agent steps form a DAG (directed acyclic graph), not a linear pipeline
- Independent steps execute in parallel on different nodes
- Each step is routed to the best node for that specific operation (model + tool availability)
- Data locality and privacy: sensitive intermediate results stay on trusted nodes
- Tools are declared per-node capabilities (like hardware), registered in the node registry
- The optimizer extends to place tools alongside models (same algorithm, expanded resource types)
- Agents themselves are lightweight (orchestration logic) — they don't need "placement" like models do. The orchestrator runs on the requesting node.

## User Stories

### US-1: Parallel Research Agent
As a user running a research agent that needs to search the web, read documents, and synthesize with a large model, I want the web search to run on a node with browser tools while the document reading runs on a node with filesystem access, both in parallel, then synthesis on the node with the 14B model — completing in 30% of the time vs sequential.

### US-2: Multi-Model Agent
As a user running an agent that uses CodeLlama for code generation and Qwen for code review, I want the code generation and review to happen on different nodes simultaneously (when reviewing previous code while generating new code), utilizing both GPUs at once.

### US-3: Privacy-Aware Agent Steps
As a user running an agent that processes my private documents, I want document-reading steps to execute only on my local nodes (tier 3), while non-sensitive web search steps can execute on mesh nodes (tier 2), respecting the same trust model as inference routing.

### US-4: Tool-Aware Routing
As a user whose desktop has a browser and code execution tools but my laptop only has filesystem access, I want the agent to automatically route browser-requiring steps to my desktop and file-reading steps to whichever node has the relevant files.

### US-5: Fault-Tolerant Execution
As a user running a long agent workflow (10+ steps), I want the system to handle node failures mid-execution gracefully — retry failed steps on alternative nodes, preserve completed step results, and only fail the overall workflow if no alternative exists.

## Functional Requirements

### FR-1: Tool Registry (extends Node Capabilities)
- FR-1.1: Each node declares its available tools as part of capability reporting: `available_tools: Vec<ToolCapability>`
- FR-1.2: Tool capability includes: tool_id, tool_name, resource_requirements (CPU, GPU, memory, network), availability_status
- FR-1.3: Tools are categorized: filesystem, web_search, browser, code_execution, gpu_compute, database, custom
- FR-1.4: Tool availability is dynamic: tools can become available/unavailable at runtime (e.g., browser tool unavailable if browser process crashed)
- FR-1.5: Tool declarations propagate to all nodes via the same mechanism as hardware capabilities (Phase 9A node registry)
- FR-1.6: The optimizer considers tool placement when making model placement decisions (co-locate models with their commonly-needed tools when possible)

### FR-2: Agent Execution Plan Decomposition
- FR-2.1: An agent's execution plan is represented as a DAG (directed acyclic graph) of steps
- FR-2.2: Each step declares: required_model (optional), required_tools (list), input_dependencies (which previous steps' outputs it needs), estimated_compute_time, sensitivity_level
- FR-2.3: Steps with no mutual dependencies can execute in parallel
- FR-2.4: The orchestrator analyzes the DAG to identify maximum parallelism
- FR-2.5: Steps can be dynamically added during execution (agent decides next step based on previous results)

### FR-3: Step Routing
- FR-3.1: Each step is routed to the best node that satisfies ALL requirements: has the required model loaded AND has the required tools available AND meets trust tier for the step's sensitivity level
- FR-3.2: If no single node satisfies all requirements, the step is decomposed further (model inference on node A, tool execution on node B, with data transfer between them)
- FR-3.3: Routing considers: model availability, tool availability, current load (queue depth), latency to requesting node, trust tier, data locality (prefer nodes that already have the step's input data)
- FR-3.4: Routing reuses the Phase 9A/10 path selection infrastructure (same scoring, same failover)

### FR-4: Parallel Execution
- FR-4.1: Independent inference: "Ask model X about topic A" AND "Ask model Y about topic B" run simultaneously on different nodes
- FR-4.2: Map-reduce: "Process these N items" splits across N nodes (or fewer, with batching), each processes a subset, results merged
- FR-4.3: Speculative execution: "Try approach A on node X and approach B on node Y, use whichever finishes first or is better quality" — cancel the slower/worse one
- FR-4.4: Pipeline: step N+1 starts as soon as step N produces partial output (streaming results between steps)

### FR-5: Data Transfer Between Steps
- FR-5.1: When step B depends on step A's output and they run on different nodes, transfer the output via Phase 10 transport
- FR-5.2: Transfer uses appropriate priority: Critical for blocking dependencies, Normal for prefetch of likely-needed data
- FR-5.3: Data locality optimization: if multiple subsequent steps need the same data, prefer routing them to the same node (avoid repeated transfers)
- FR-5.4: Large intermediate results (>10MB) are transferred with bandwidth throttling (same as model transfers — don't impact active inference)
- FR-5.5: Intermediate results are ephemeral — deleted after the dependent steps complete (no permanent storage of inter-step data)

### FR-6: Privacy and Trust for Agent Steps
- FR-6.1: Each step has a sensitivity level (same classification as inference requests: sensitive/non-sensitive)
- FR-6.2: Sensitive steps execute only on tier-3 (local-owned) nodes — same trust model as Phase 9B
- FR-6.3: Intermediate results from sensitive steps are never transferred to lower-trust nodes
- FR-6.4: If a step requires a tool only available on a lower-trust node but the step is sensitive, the step FAILS (not silently downgraded)
- FR-6.5: The orchestrator can split a step into sensitive and non-sensitive sub-steps when possible (e.g., "search the web" is non-sensitive, "apply results to my private document" is sensitive)

### FR-7: Fault Tolerance
- FR-7.1: If a step fails (node timeout, tool error, inference error), retry on an alternative node (if one exists with the same capabilities)
- FR-7.2: Maximum 2 retries per step before declaring step failure
- FR-7.3: Completed step results are cached — if a later step fails and the workflow retries, don't re-execute already-completed steps
- FR-7.4: If a step has no alternative nodes and fails, the entire workflow fails with a clear error explaining which step failed and why
- FR-7.5: Long-running workflows (>5 minutes) checkpoint their progress — can resume after app restart

### FR-8: Orchestrator
- FR-8.1: The orchestrator runs on the requesting node (the node where the user initiated the agent)
- FR-8.2: Orchestrator responsibilities: decompose plan into DAG, route steps, manage parallel execution, collect results, handle failures, report progress
- FR-8.3: Orchestrator is lightweight (no GPU needed) — it's coordination logic, not computation
- FR-8.4: Orchestrator communicates with worker nodes via Phase 10 transport
- FR-8.5: Orchestrator exposes progress to the UI: which steps are running, which completed, which waiting, estimated time remaining

### FR-9: Optimizer Extension
- FR-9.1: The optimizer's Phase A (model selection) extends to consider tool co-location: if 80% of agent workflows need model X + tool Y together, prefer placing model X on a node that has tool Y
- FR-9.2: The optimizer's node capability reporting includes tool declarations
- FR-9.3: The optimizer does NOT place tools (tools are fixed per-node — you can't "move" a browser to another machine). It only considers tool presence when placing models.
- FR-9.4: New demand signal: "agent step demand" — which (model, tool) pairs are requested together, feeding into co-location decisions

## Non-Functional Requirements

### NFR-1: Performance
- NFR-1.1: Orchestrator overhead: <10ms per step routing decision
- NFR-1.2: Parallel speedup: N independent steps on N nodes complete in ~1x time (not N×)
- NFR-1.3: Data transfer between steps: uses Phase 10 transport, same latency characteristics
- NFR-1.4: Step routing decision: <5ms (reuses existing path selection)

### NFR-2: Scalability
- NFR-2.1: Support agent workflows with up to 50 steps
- NFR-2.2: Support up to 10 parallel steps executing simultaneously
- NFR-2.3: Support intermediate results up to 100MB per step

### NFR-3: Reliability
- NFR-3.1: Single step failure does not crash the orchestrator
- NFR-3.2: Completed step results survive step retries (cached)
- NFR-3.3: Orchestrator failure during long workflow is recoverable (checkpoint/resume)

### NFR-4: Privacy
- NFR-4.1: Sensitive step data never leaves tier-3 nodes
- NFR-4.2: Intermediate results encrypted in transit (Phase 10 transport encryption)
- NFR-4.3: Intermediate results deleted after workflow completion (no persistent storage of inter-step data)

## Correctness Properties

### Property 1: DAG execution order
Steps SHALL only execute after ALL their input dependencies have completed. No step SHALL receive stale or incomplete input data.

### Property 2: Parallel independence
Steps executing in parallel SHALL have no mutual dependencies. The orchestrator SHALL verify independence before parallel dispatch.

### Property 3: Trust enforcement
Sensitive steps SHALL NEVER execute on nodes with trust tier < 3. Sensitive intermediate results SHALL NEVER be transferred to nodes with trust tier < 3.

### Property 4: Tool requirement satisfaction
A step SHALL only execute on a node that has ALL its required tools available. If no such node exists, the step SHALL fail (not execute without the tool).

### Property 5: Fault isolation
Failure of one parallel step SHALL NOT affect other parallel steps. Only steps with a dependency on the failed step are affected.

### Property 6: Result caching correctness
Cached step results SHALL be invalidated if the step's inputs change (e.g., due to a retry of an upstream step producing different output).

### Property 7: No resource starvation
Parallel step execution SHALL NOT starve the network — total concurrent steps across all workflows bounded by available capacity. Excess steps queued, not rejected.

### Property 8: Completion guarantee
If all required nodes and tools are available, a workflow with no circular dependencies SHALL eventually complete (no deadlocks).

### Property 9: Privacy classification propagation
If step A is sensitive and step B depends on step A's output, step B SHALL be classified as at least as sensitive as step A (sensitivity propagates forward through the DAG).

### Property 10: Orchestrator locality
The orchestrator SHALL always run on the requesting node. It SHALL NOT be migrated to another node during workflow execution.
