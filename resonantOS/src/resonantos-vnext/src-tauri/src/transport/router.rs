// Intent citation: .kiro/specs/unified-mesh-transport/design.md Section 3.5
// Message Router — multi-hop routing, loop detection, relay, TTL enforcement
//
// Phase 15 extension (Task 11.1): Agent message type registration.
// Incoming messages with RequestType AgentStepDispatch/AgentStepResult/AgentStepData
// are classified as agent messages and routed to the agent worker handler.
// Outgoing agent messages are routed through the transport selector with
// appropriate priority and path scoring.

use super::registry::{PathStatus, UnifiedTopology};
use super::trait_def::{NodeId, RequestType, TransportError, TransportMessage};
use std::collections::{HashSet, VecDeque};

/// Maximum number of hops allowed.
pub const MAX_HOPS: u8 = 5;

/// Result of a routing decision.
#[derive(Debug, Clone, PartialEq)]
pub enum RoutingDecision {
    /// Target is directly reachable — send via this transport.
    Direct { transport_id: String },
    /// Target requires multi-hop — forward to next_hop.
    MultiHop { next_hop: NodeId, via_transport: String },
    /// Target is unreachable from current node.
    Unreachable,
}

/// Check if a message has a routing loop (current node already visited).
pub fn has_loop(message: &TransportMessage, current_node: &NodeId) -> bool {
    message.visited_nodes.contains(current_node)
}

/// Check if TTL has expired.
pub fn ttl_expired(message: &TransportMessage) -> bool {
    message.ttl_hops == 0
}

/// Prepare a message for forwarding: decrement TTL, add current node to visited list.
pub fn prepare_for_forward(message: &mut TransportMessage, current_node: NodeId) {
    message.ttl_hops = message.ttl_hops.saturating_sub(1);
    message.visited_nodes.push(current_node);
}

/// Find the next hop toward a target node using BFS on the topology graph.
/// Returns the immediate neighbor to forward to, and which transport to use.
pub fn find_next_hop(
    current_node: &NodeId,
    target: &NodeId,
    topology: &UnifiedTopology,
) -> Option<(NodeId, String)> {
    // If target is a direct neighbor, return it
    let direct = topology.paths.iter().find(|p| {
        p.source == *current_node
            && p.destination == *target
            && p.status == PathStatus::Active
    });

    if let Some(path) = direct {
        return Some((*target, path.transport_id.clone()));
    }

    // BFS to find shortest path
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut queue: VecDeque<(NodeId, NodeId, String)> = VecDeque::new(); // (current, first_hop, transport)

    visited.insert(*current_node);

    // Seed with direct neighbors
    for path in topology.paths.iter().filter(|p| p.source == *current_node && p.status == PathStatus::Active) {
        if !visited.contains(&path.destination) {
            visited.insert(path.destination);
            queue.push_back((path.destination, path.destination, path.transport_id.clone()));
        }
    }

    // BFS
    while let Some((current, first_hop, transport)) = queue.pop_front() {
        if current == *target {
            return Some((first_hop, transport));
        }

        // Explore neighbors of current
        for path in topology.paths.iter().filter(|p| p.source == current && p.status == PathStatus::Active) {
            if !visited.contains(&path.destination) {
                visited.insert(path.destination);
                queue.push_back((path.destination, first_hop, transport.clone()));
            }
        }
    }

    None // Target unreachable
}

/// Compute the hop distance between two nodes.
/// Returns None if unreachable.
pub fn hop_distance(
    from: &NodeId,
    to: &NodeId,
    topology: &UnifiedTopology,
) -> Option<u32> {
    if from == to {
        return Some(0);
    }

    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut queue: VecDeque<(NodeId, u32)> = VecDeque::new();

    visited.insert(*from);
    queue.push_back((*from, 0));

    while let Some((current, distance)) = queue.pop_front() {
        for path in topology.paths.iter().filter(|p| p.source == current && p.status == PathStatus::Active) {
            if path.destination == *to {
                return Some(distance + 1);
            }
            if !visited.contains(&path.destination) {
                visited.insert(path.destination);
                queue.push_back((path.destination, distance + 1));
            }
        }
    }

    None
}

/// Make a routing decision for a message.
pub fn route(
    current_node: &NodeId,
    target: &NodeId,
    message: &TransportMessage,
    topology: &UnifiedTopology,
) -> Result<RoutingDecision, TransportError> {
    // Check TTL
    if ttl_expired(message) {
        return Err(TransportError::TtlExpired {
            message_id: message.message_id,
        });
    }

    // Check loop
    if has_loop(message, current_node) {
        return Err(TransportError::RoutingLoop {
            node: *current_node,
        });
    }

    // Check direct path
    let direct = topology.paths.iter().find(|p| {
        p.source == *current_node
            && p.destination == *target
            && p.status == PathStatus::Active
    });

    if let Some(path) = direct {
        return Ok(RoutingDecision::Direct {
            transport_id: path.transport_id.clone(),
        });
    }

    // Find multi-hop route
    match find_next_hop(current_node, target, topology) {
        Some((next_hop, transport)) => Ok(RoutingDecision::MultiHop {
            next_hop,
            via_transport: transport,
        }),
        None => Ok(RoutingDecision::Unreachable),
    }
}

// ===========================================================================
// Phase 15 (Task 11.1): Agent Message Type Registration
// ===========================================================================

/// Classification of an incoming message for dispatch purposes.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageClass {
    /// Agent step dispatch: orchestrator → worker (execute/cancel step).
    AgentStepDispatch,
    /// Agent step result: worker → orchestrator (started/completed/failed/progress).
    AgentStepResult,
    /// Agent inter-step data transfer between nodes.
    AgentStepData,
    /// Non-agent message (inference, heartbeat, model transfer, etc.).
    Other,
}

/// Classify an incoming message by its request type.
///
/// Agent messages (AgentStepDispatch, AgentStepResult, AgentStepData) are
/// identified so the transport layer can route them to the agent worker handler
/// rather than the inference pipeline.
///
/// Satisfies FR-8.4: Orchestrator communicates with worker nodes via Phase 10 transport.
pub fn classify_message(message: &TransportMessage) -> MessageClass {
    match &message.request_type {
        RequestType::AgentStepDispatch => MessageClass::AgentStepDispatch,
        RequestType::AgentStepResult => MessageClass::AgentStepResult,
        RequestType::AgentStepData => MessageClass::AgentStepData,
        _ => MessageClass::Other,
    }
}

/// Check whether an incoming message is an agent message that should be
/// routed to the agent execution subsystem.
///
/// Returns true for AgentStepDispatch, AgentStepResult, and AgentStepData.
/// The caller should forward these to `agents::integration::handle_agent_message`.
///
/// Satisfies FR-8.4, FR-5.1.
pub fn is_agent_message(message: &TransportMessage) -> bool {
    matches!(
        message.request_type,
        RequestType::AgentStepDispatch
            | RequestType::AgentStepResult
            | RequestType::AgentStepData
    )
}

/// Dispatch result for an incoming message after routing classification.
///
/// When the transport layer receives a message destined for this node,
/// it uses this enum to determine which subsystem should handle it.
#[derive(Debug, Clone, PartialEq)]
pub enum IncomingDispatch {
    /// Route to the agent worker/orchestrator handler.
    AgentHandler,
    /// Route to the inference pipeline.
    InferencePipeline,
    /// Route to the model transfer handler.
    ModelTransfer,
    /// Route to the control plane (heartbeat, metrics, announcements).
    ControlPlane,
}

/// Determine the dispatch target for an incoming message that has arrived
/// at its final destination (this node).
///
/// This is the top-level routing decision for incoming messages:
/// - Agent messages → AgentHandler (agents::integration::handle_agent_message)
/// - Inference messages → InferencePipeline
/// - Model/KV-cache transfers → ModelTransfer
/// - Heartbeat/metrics/announcements → ControlPlane
///
/// Satisfies FR-8.4: Agent messages routed to worker handler.
pub fn dispatch_incoming(message: &TransportMessage) -> IncomingDispatch {
    match &message.request_type {
        RequestType::AgentStepDispatch
        | RequestType::AgentStepResult
        | RequestType::AgentStepData => IncomingDispatch::AgentHandler,

        RequestType::InferenceActivation
        | RequestType::InferenceRequest
        | RequestType::InferenceResponse => IncomingDispatch::InferencePipeline,

        RequestType::ModelTransfer | RequestType::KvCacheData => IncomingDispatch::ModelTransfer,

        RequestType::Heartbeat | RequestType::MetricProbe | RequestType::Announcement => {
            IncomingDispatch::ControlPlane
        }
    }
}

/// Determine the outgoing request type for an agent message based on direction.
///
/// - Orchestrator → Worker: use AgentStepDispatch
/// - Worker → Orchestrator: use AgentStepResult
/// - Inter-step data transfer: use AgentStepData
///
/// This is used by the executor/worker when preparing outgoing messages
/// to ensure the transport selector applies the correct path scoring.
///
/// Satisfies FR-8.4: Communication via Phase 10 transport.
/// Satisfies FR-5.1: Data transfer between steps on different nodes.
pub fn outgoing_request_type_for_agent(direction: AgentMessageDirection) -> RequestType {
    match direction {
        AgentMessageDirection::OrchestratorToWorker => RequestType::AgentStepDispatch,
        AgentMessageDirection::WorkerToOrchestrator => RequestType::AgentStepResult,
        AgentMessageDirection::InterStepData => RequestType::AgentStepData,
    }
}

/// Direction of an agent message for outgoing routing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentMessageDirection {
    /// Orchestrator dispatching a step to a worker node.
    OrchestratorToWorker,
    /// Worker sending results back to the orchestrator.
    WorkerToOrchestrator,
    /// Transferring intermediate data between steps on different nodes.
    InterStepData,
}

/// Route an incoming message: if it's for this node, classify for dispatch;
/// if it needs forwarding, compute the next hop.
///
/// This combines the existing multi-hop routing logic with the new agent
/// message classification. The caller uses the result to either:
/// - Forward the message to the next hop (if not destined for this node)
/// - Dispatch locally via `dispatch_incoming` (if destined for this node)
///
/// Satisfies FR-8.4: Agent messages routed through transport to correct handler.
pub fn route_and_classify(
    current_node: &NodeId,
    target: &NodeId,
    message: &TransportMessage,
    topology: &UnifiedTopology,
) -> Result<RouteAndClassifyResult, TransportError> {
    if current_node == target {
        // Message is for this node — classify for local dispatch
        let dispatch = dispatch_incoming(message);
        return Ok(RouteAndClassifyResult::LocalDispatch(dispatch));
    }

    // Message needs forwarding — use existing routing logic
    let decision = route(current_node, target, message, topology)?;
    Ok(RouteAndClassifyResult::Forward(decision))
}

/// Result of route_and_classify: either dispatch locally or forward.
#[derive(Debug, Clone, PartialEq)]
pub enum RouteAndClassifyResult {
    /// Message is for this node; dispatch to the indicated handler.
    LocalDispatch(IncomingDispatch),
    /// Message needs forwarding; use the routing decision.
    Forward(RoutingDecision),
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::registry::{PathMetrics, TransportPath};
    use super::super::trait_def::MessagePriority;
    use super::super::trait_def::RequestType;

    fn make_topology_path(src: NodeId, dst: NodeId, transport: &str) -> TransportPath {
        TransportPath {
            path_id: uuid::Uuid::new_v4(),
            source: src,
            destination: dst,
            transport_id: transport.to_string(),
            hops: vec![],
            metrics: PathMetrics {
                latency_ms: 5.0,
                bandwidth_mbps: 1000.0,
                reliability: 0.99,
                jitter_ms: 1.0,
                last_measured_ms: 1000,
                measurement_count: 10,
            },
            status: PathStatus::Active,
        }
    }

    fn make_message() -> TransportMessage {
        TransportMessage::new(vec![1, 2, 3], MessagePriority::Normal, RequestType::InferenceRequest)
    }

    #[test]
    fn test_direct_route() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();

        let mut topology = UnifiedTopology::new();
        topology.paths.push(make_topology_path(a, b, "lan"));

        let msg = make_message();
        let decision = route(&a, &b, &msg, &topology).unwrap();

        assert!(matches!(decision, RoutingDecision::Direct { transport_id } if transport_id == "lan"));
    }

    #[test]
    fn test_multi_hop_route() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();

        let mut topology = UnifiedTopology::new();
        topology.paths.push(make_topology_path(a, b, "lan")); // A → B
        topology.paths.push(make_topology_path(b, c, "lan")); // B → C
        // No direct A → C

        let msg = make_message();
        let decision = route(&a, &c, &msg, &topology).unwrap();

        // Should route through B
        assert!(matches!(decision, RoutingDecision::MultiHop { next_hop, .. } if next_hop == b));
    }

    #[test]
    fn test_unreachable() {
        let a = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();

        let topology = UnifiedTopology::new(); // Empty — no paths

        let msg = make_message();
        let decision = route(&a, &c, &msg, &topology).unwrap();

        assert!(matches!(decision, RoutingDecision::Unreachable));
    }

    #[test]
    fn test_ttl_expired() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();

        let topology = UnifiedTopology::new();
        let mut msg = make_message();
        msg.ttl_hops = 0; // Expired

        let result = route(&a, &b, &msg, &topology);
        assert!(matches!(result, Err(TransportError::TtlExpired { .. })));
    }

    #[test]
    fn test_loop_detection() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();

        let topology = UnifiedTopology::new();
        let mut msg = make_message();
        msg.visited_nodes.push(a); // Already visited A

        let result = route(&a, &b, &msg, &topology);
        assert!(matches!(result, Err(TransportError::RoutingLoop { .. })));
    }

    #[test]
    fn test_prepare_for_forward() {
        let node = uuid::Uuid::new_v4();
        let mut msg = make_message();
        assert_eq!(msg.ttl_hops, 5);
        assert!(msg.visited_nodes.is_empty());

        prepare_for_forward(&mut msg, node);
        assert_eq!(msg.ttl_hops, 4);
        assert_eq!(msg.visited_nodes.len(), 1);
        assert_eq!(msg.visited_nodes[0], node);
    }

    #[test]
    fn test_hop_distance() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();

        let mut topology = UnifiedTopology::new();
        topology.paths.push(make_topology_path(a, b, "lan"));
        topology.paths.push(make_topology_path(b, c, "lan"));

        assert_eq!(hop_distance(&a, &a, &topology), Some(0)); // Self
        assert_eq!(hop_distance(&a, &b, &topology), Some(1)); // Direct
        assert_eq!(hop_distance(&a, &c, &topology), Some(2)); // Via B

        let d = uuid::Uuid::new_v4();
        assert_eq!(hop_distance(&a, &d, &topology), None); // Unreachable
    }

    #[test]
    fn test_failed_path_not_used() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();

        let mut topology = UnifiedTopology::new();
        let mut path = make_topology_path(a, b, "lan");
        path.status = PathStatus::Failed { since_ms: 1000 }; // Failed!
        topology.paths.push(path);

        let msg = make_message();
        let decision = route(&a, &b, &msg, &topology).unwrap();

        // Should be unreachable (only path is failed)
        assert!(matches!(decision, RoutingDecision::Unreachable));
    }

    // ─── Phase 15 (Task 11.1): Agent message routing tests ──────────────────

    #[test]
    fn test_classify_agent_step_dispatch() {
        let msg = TransportMessage::new(
            vec![1, 2, 3],
            MessagePriority::Normal,
            RequestType::AgentStepDispatch,
        );
        assert_eq!(classify_message(&msg), MessageClass::AgentStepDispatch);
    }

    #[test]
    fn test_classify_agent_step_result() {
        let msg = TransportMessage::new(
            vec![1, 2, 3],
            MessagePriority::Normal,
            RequestType::AgentStepResult,
        );
        assert_eq!(classify_message(&msg), MessageClass::AgentStepResult);
    }

    #[test]
    fn test_classify_agent_step_data() {
        let msg = TransportMessage::new(
            vec![1, 2, 3],
            MessagePriority::Normal,
            RequestType::AgentStepData,
        );
        assert_eq!(classify_message(&msg), MessageClass::AgentStepData);
    }

    #[test]
    fn test_classify_non_agent_messages() {
        let types = vec![
            RequestType::InferenceActivation,
            RequestType::InferenceRequest,
            RequestType::InferenceResponse,
            RequestType::ModelTransfer,
            RequestType::Heartbeat,
            RequestType::MetricProbe,
            RequestType::KvCacheData,
            RequestType::Announcement,
        ];

        for rt in types {
            let msg = TransportMessage::new(vec![], MessagePriority::Normal, rt);
            assert_eq!(classify_message(&msg), MessageClass::Other);
        }
    }

    #[test]
    fn test_is_agent_message_true_for_agent_types() {
        let dispatch = TransportMessage::new(vec![], MessagePriority::Normal, RequestType::AgentStepDispatch);
        let result = TransportMessage::new(vec![], MessagePriority::Normal, RequestType::AgentStepResult);
        let data = TransportMessage::new(vec![], MessagePriority::Normal, RequestType::AgentStepData);

        assert!(is_agent_message(&dispatch));
        assert!(is_agent_message(&result));
        assert!(is_agent_message(&data));
    }

    #[test]
    fn test_is_agent_message_false_for_non_agent_types() {
        let inference = TransportMessage::new(vec![], MessagePriority::Normal, RequestType::InferenceRequest);
        let heartbeat = TransportMessage::new(vec![], MessagePriority::Low, RequestType::Heartbeat);

        assert!(!is_agent_message(&inference));
        assert!(!is_agent_message(&heartbeat));
    }

    #[test]
    fn test_dispatch_incoming_agent_messages_to_agent_handler() {
        let dispatch = TransportMessage::new(vec![], MessagePriority::Normal, RequestType::AgentStepDispatch);
        let result = TransportMessage::new(vec![], MessagePriority::Normal, RequestType::AgentStepResult);
        let data = TransportMessage::new(vec![], MessagePriority::Normal, RequestType::AgentStepData);

        assert_eq!(dispatch_incoming(&dispatch), IncomingDispatch::AgentHandler);
        assert_eq!(dispatch_incoming(&result), IncomingDispatch::AgentHandler);
        assert_eq!(dispatch_incoming(&data), IncomingDispatch::AgentHandler);
    }

    #[test]
    fn test_dispatch_incoming_inference_to_pipeline() {
        let activation = TransportMessage::new(vec![], MessagePriority::Critical, RequestType::InferenceActivation);
        let request = TransportMessage::new(vec![], MessagePriority::Normal, RequestType::InferenceRequest);
        let response = TransportMessage::new(vec![], MessagePriority::Normal, RequestType::InferenceResponse);

        assert_eq!(dispatch_incoming(&activation), IncomingDispatch::InferencePipeline);
        assert_eq!(dispatch_incoming(&request), IncomingDispatch::InferencePipeline);
        assert_eq!(dispatch_incoming(&response), IncomingDispatch::InferencePipeline);
    }

    #[test]
    fn test_dispatch_incoming_model_transfer() {
        let model = TransportMessage::new(vec![], MessagePriority::Normal, RequestType::ModelTransfer);
        let kv = TransportMessage::new(vec![], MessagePriority::Normal, RequestType::KvCacheData);

        assert_eq!(dispatch_incoming(&model), IncomingDispatch::ModelTransfer);
        assert_eq!(dispatch_incoming(&kv), IncomingDispatch::ModelTransfer);
    }

    #[test]
    fn test_dispatch_incoming_control_plane() {
        let heartbeat = TransportMessage::new(vec![], MessagePriority::Low, RequestType::Heartbeat);
        let metric = TransportMessage::new(vec![], MessagePriority::Low, RequestType::MetricProbe);
        let announce = TransportMessage::new(vec![], MessagePriority::Low, RequestType::Announcement);

        assert_eq!(dispatch_incoming(&heartbeat), IncomingDispatch::ControlPlane);
        assert_eq!(dispatch_incoming(&metric), IncomingDispatch::ControlPlane);
        assert_eq!(dispatch_incoming(&announce), IncomingDispatch::ControlPlane);
    }

    #[test]
    fn test_outgoing_request_type_for_agent_directions() {
        assert_eq!(
            outgoing_request_type_for_agent(AgentMessageDirection::OrchestratorToWorker),
            RequestType::AgentStepDispatch
        );
        assert_eq!(
            outgoing_request_type_for_agent(AgentMessageDirection::WorkerToOrchestrator),
            RequestType::AgentStepResult
        );
        assert_eq!(
            outgoing_request_type_for_agent(AgentMessageDirection::InterStepData),
            RequestType::AgentStepData
        );
    }

    #[test]
    fn test_route_and_classify_local_dispatch_agent() {
        let node = uuid::Uuid::new_v4();
        let topology = UnifiedTopology::new();

        let msg = TransportMessage::new(
            vec![1, 2, 3],
            MessagePriority::Normal,
            RequestType::AgentStepDispatch,
        );

        let result = route_and_classify(&node, &node, &msg, &topology).unwrap();
        assert_eq!(
            result,
            RouteAndClassifyResult::LocalDispatch(IncomingDispatch::AgentHandler)
        );
    }

    #[test]
    fn test_route_and_classify_local_dispatch_inference() {
        let node = uuid::Uuid::new_v4();
        let topology = UnifiedTopology::new();

        let msg = TransportMessage::new(
            vec![1, 2, 3],
            MessagePriority::Normal,
            RequestType::InferenceRequest,
        );

        let result = route_and_classify(&node, &node, &msg, &topology).unwrap();
        assert_eq!(
            result,
            RouteAndClassifyResult::LocalDispatch(IncomingDispatch::InferencePipeline)
        );
    }

    #[test]
    fn test_route_and_classify_forward_to_next_hop() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();

        let mut topology = UnifiedTopology::new();
        topology.paths.push(make_topology_path(a, b, "lan"));

        let msg = TransportMessage::new(
            vec![1, 2, 3],
            MessagePriority::Normal,
            RequestType::AgentStepDispatch,
        );

        let result = route_and_classify(&a, &b, &msg, &topology).unwrap();
        match result {
            RouteAndClassifyResult::Forward(RoutingDecision::Direct { transport_id }) => {
                assert_eq!(transport_id, "lan");
            }
            _ => panic!("Expected Forward(Direct), got {:?}", result),
        }
    }

    #[test]
    fn test_route_and_classify_forward_agent_data_multi_hop() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();

        let mut topology = UnifiedTopology::new();
        topology.paths.push(make_topology_path(a, b, "lan"));
        topology.paths.push(make_topology_path(b, c, "lan"));

        let msg = TransportMessage::new(
            vec![1, 2, 3],
            MessagePriority::Normal,
            RequestType::AgentStepData,
        );

        let result = route_and_classify(&a, &c, &msg, &topology).unwrap();
        match result {
            RouteAndClassifyResult::Forward(RoutingDecision::MultiHop { next_hop, .. }) => {
                assert_eq!(next_hop, b);
            }
            _ => panic!("Expected Forward(MultiHop), got {:?}", result),
        }
    }
}
