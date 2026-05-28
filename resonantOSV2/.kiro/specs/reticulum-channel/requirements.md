# Requirements Document

## Introduction

Reticulum Channel is Phase 6 of the ResonantOS vNext improvement plan. It delivers a mesh network communication channel add-on that enables sending and receiving messages to and from the main Strategist chat via the Reticulum cryptographic networking stack. Reticulum provides end-to-end encrypted messaging over any transport (LoRa, packet radio, WiFi, serial, TCP, I2P) with multi-hop routing, delay tolerance, and initiator anonymity.

The channel is implemented as a pure add-on with `runtimeType: "channel-addon"` using a Python sidecar process that runs the Reticulum stack. Communication between the Rust host and the Python sidecar uses the `stdio-json-rpc` protocol defined in the Add-on SDK V0. Messages use the LXMF (Lightweight Extensible Message Format) standard for interoperability with MeshChat (desktop) and Sideband (mobile).

The Reticulum Channel operates fully offline when using LoRa or radio transport, does not require internet connectivity, and degrades gracefully if the sidecar process is unavailable. If disabled or removed, the shell operates identically to its current behavior.

## Glossary

- **Reticulum_Channel**: The channel add-on that bridges the Reticulum mesh network to the ResonantOS conversation system, registered as a ChannelDefinition with type "reticulum"
- **Reticulum_Sidecar**: The Python process that runs the Reticulum networking stack (via the `rns` library), announces a destination, and handles packet encoding/decoding
- **LXMF**: Lightweight Extensible Message Format, the standard message format used by Reticulum-compatible applications (MeshChat, Sideband) for interoperable messaging
- **Reticulum_Destination**: A cryptographic identity endpoint on the Reticulum network, identified by a hash derived from the destination's public keys
- **Mesh_Link**: An active Reticulum link between two destinations, providing reliable delivery with acknowledgements
- **Transport_Interface**: A configured Reticulum transport medium (TCP, LoRa via RNode, serial, I2P, or AutoInterface for LAN discovery)
- **Delivery_Receipt**: A cryptographic acknowledgement returned by Reticulum when a packet is successfully delivered to the destination
- **Message_Queue**: The local queue of outbound messages waiting to be transmitted when a Mesh_Link is available
- **Bandwidth_Profile**: A configuration set defining maximum message size, summarization behavior, and priority rules for a specific Transport_Interface type
- **Sidecar_Health_State**: The operational status of the Reticulum_Sidecar process: "running", "starting", "offline", or "crashed"
- **Behavioral_Contract**: A declarative specification of expected system behavior registered in the Phase 0 Contract_Registry
- **RNode**: A LoRa-capable hardware device that provides a Reticulum Transport_Interface for long-range radio communication

## Requirements

### Requirement 1: Channel Add-on Registration

**User Story:** As the system, I want the Reticulum channel registered as a standard channel add-on, so that it integrates with the existing multi-channel architecture without modifying core shell code.

#### Acceptance Criteria

1. THE Reticulum_Channel SHALL register a ChannelDefinition with type "reticulum", a unique channel identifier, and owningAgentId set to the Strategist agent
2. THE Reticulum_Channel SHALL declare runtimeType "channel-addon" in its AddOnManifest with category "channel"
3. THE Reticulum_Channel SHALL declare an AddOnLocalServiceDefinition with protocol "stdio-json-rpc" and entrypoint pointing to the Python sidecar launch command
4. THE Reticulum_Channel SHALL request capabilities: "chat-interface", "notifications", and "device-integration" in its manifest
5. WHEN the Reticulum_Channel add-on is installed and enabled, THE Reticulum_Channel SHALL create a ConversationThread linked to the "reticulum" channel for each active Reticulum peer
6. WHEN the Reticulum_Channel add-on is disabled or removed, THE shell SHALL operate identically to its behavior without the add-on, with no impact on desktop, telegram, or voice channels

### Requirement 2: Python Sidecar Lifecycle

**User Story:** As the system, I want the Python sidecar process managed reliably, so that the Reticulum stack starts, stops, and recovers without manual intervention.

#### Acceptance Criteria

1. WHEN the Reticulum_Channel is started, THE host SHALL spawn the Reticulum_Sidecar process using the stdio-json-rpc protocol with stdin/stdout as the communication channel
2. THE Reticulum_Sidecar SHALL load the user's existing Reticulum configuration from ~/.reticulum/config on startup
3. THE Reticulum_Sidecar SHALL announce a Reticulum_Destination and begin listening for incoming LXMF messages within 10 seconds of process start on TCP transport
4. WHEN the Reticulum_Sidecar process exits unexpectedly, THE Reticulum_Channel SHALL update the Sidecar_Health_State to "crashed" and emit a shell notification indicating the channel is offline
5. WHEN the Sidecar_Health_State transitions to "crashed", THE Reticulum_Channel SHALL attempt automatic restart after a 5-second delay, with exponential backoff up to 60 seconds for repeated failures
6. WHEN the Reticulum_Channel is stopped by the user, THE host SHALL send a shutdown command to the Reticulum_Sidecar and terminate the process within 5 seconds

### Requirement 3: Inbound Message Flow

**User Story:** As a user, I want messages from the Reticulum mesh to appear in my conversation thread, so that I can read and respond to mesh contacts from within ResonantOS.

#### Acceptance Criteria

1. WHEN the Reticulum_Sidecar receives an LXMF message from a remote Reticulum_Destination, THE Reticulum_Sidecar SHALL decode the message and emit a JSON-RPC notification to the host containing the sender destination hash, message content, and timestamp
2. WHEN the host receives an inbound message notification, THE Reticulum_Channel SHALL insert a ConversationMessage with role "user", the sender's display name or destination hash as author, and channelId set to the Reticulum channel identifier
3. WHEN an inbound message arrives for a sender that has no existing ConversationThread, THE Reticulum_Channel SHALL create a new ConversationThread with title containing the sender's identity label and channelId set to the Reticulum channel identifier
4. THE Reticulum_Channel SHALL process only text content from inbound LXMF messages, ignoring any binary attachments
5. WHEN an inbound message is inserted into a ConversationThread, THE Reticulum_Channel SHALL trigger the standard Strategist response flow using the configured provider route

### Requirement 4: Outbound Message Flow

**User Story:** As a user, I want AI responses sent back over the Reticulum mesh to my contact, so that the conversation is bidirectional.

#### Acceptance Criteria

1. WHEN the Strategist produces a response in a Reticulum-channel ConversationThread, THE Reticulum_Channel SHALL serialize the response text and send it to the Reticulum_Sidecar via a JSON-RPC request specifying the target Reticulum_Destination
2. THE Reticulum_Sidecar SHALL encode the outbound message as an LXMF message and transmit it to the target Reticulum_Destination
3. WHEN the outbound message exceeds the maximum single-packet size for the active Transport_Interface, THE Reticulum_Sidecar SHALL chunk the message into multiple LXMF packets and transmit them in sequence
4. WHEN the Reticulum_Sidecar successfully transmits an outbound message, THE Reticulum_Sidecar SHALL return a JSON-RPC response indicating success to the host
5. WHEN an outbound message transmission fails, THE Reticulum_Sidecar SHALL return a JSON-RPC error response containing the failure reason, and THE Reticulum_Channel SHALL update the ConversationMessage status to indicate delivery failure

### Requirement 5: Delivery Acknowledgement and Pending States

**User Story:** As a user, I want to see whether my messages were delivered over the mesh, so that I know if my contact received them despite network delays.

#### Acceptance Criteria

1. WHEN an outbound message is transmitted, THE Reticulum_Channel SHALL set the ConversationMessage status to "pending" until a Delivery_Receipt is received
2. WHEN the Reticulum_Sidecar receives a Delivery_Receipt for a previously sent message, THE Reticulum_Sidecar SHALL emit a JSON-RPC notification to the host containing the message identifier and delivery confirmation
3. WHEN the host receives a delivery confirmation notification, THE Reticulum_Channel SHALL update the corresponding ConversationMessage status to "complete"
4. IF a Delivery_Receipt is not received within a configurable timeout (defaulting to 300 seconds for LoRa, 30 seconds for TCP), THEN THE Reticulum_Channel SHALL update the ConversationMessage status to "delivery-unconfirmed" without treating it as a failure
5. THE Reticulum_Channel SHALL display the current delivery state (pending, complete, delivery-unconfirmed, failed) in the conversation thread metadata

### Requirement 6: Outbound Message Queuing

**User Story:** As a user, I want outbound messages queued when the mesh link is temporarily unavailable, so that messages are delivered when connectivity resumes.

#### Acceptance Criteria

1. WHEN the Reticulum_Sidecar cannot establish a Mesh_Link to the target Reticulum_Destination, THE Reticulum_Channel SHALL enqueue the outbound message in the Message_Queue
2. WHILE messages exist in the Message_Queue, THE Reticulum_Sidecar SHALL retry transmission at configurable intervals (defaulting to 30 seconds)
3. WHEN a Mesh_Link becomes available to a queued message's target destination, THE Reticulum_Sidecar SHALL transmit queued messages in FIFO order
4. THE Message_Queue SHALL persist queued messages to local storage so that messages survive sidecar restarts
5. IF a queued message remains undelivered for longer than a configurable maximum age (defaulting to 24 hours), THEN THE Reticulum_Channel SHALL mark the message as "expired" and notify the user

### Requirement 7: Bandwidth-Aware Response Handling

**User Story:** As a user, I want AI responses adapted to the available bandwidth, so that long responses do not overwhelm low-bandwidth LoRa links.

#### Acceptance Criteria

1. THE Reticulum_Channel SHALL maintain a Bandwidth_Profile for each configured Transport_Interface type, specifying maximum message size in bytes
2. WHEN the active Transport_Interface is LoRa or packet radio, THE Bandwidth_Profile SHALL default to a maximum message size of 500 bytes per chunk
3. WHEN the active Transport_Interface is TCP or I2P, THE Bandwidth_Profile SHALL default to a maximum message size of 32,000 bytes (no summarization required)
4. WHEN a Strategist response exceeds the Bandwidth_Profile maximum message size for a LoRa transport, THE Reticulum_Channel SHALL request a summarized version of the response from the provider, constrained to fit within the bandwidth limit
5. THE Reticulum_Channel SHALL prioritize user-originated messages over system notifications when the Message_Queue contains multiple pending items for a low-bandwidth transport
6. THE Bandwidth_Profile settings SHALL be configurable by the user through the add-on settings panel

### Requirement 8: LXMF Interoperability

**User Story:** As a user, I want to exchange messages with MeshChat and Sideband users, so that the Reticulum channel works with the existing mesh community.

#### Acceptance Criteria

1. THE Reticulum_Sidecar SHALL encode all outbound messages using the LXMF standard format so that MeshChat (desktop) and Sideband (mobile) applications can receive and display them
2. THE Reticulum_Sidecar SHALL decode inbound LXMF messages from MeshChat and Sideband applications and deliver them to the host as text content
3. THE Reticulum_Sidecar SHALL announce its Reticulum_Destination using standard LXMF propagation so that peer applications can discover and address it
4. THE Reticulum_Sidecar SHALL set the LXMF source display name to a configurable identity label (defaulting to "ResonantOS") so that peers see a recognizable sender name
5. WHEN an inbound LXMF message contains fields beyond plain text content (stamps, attachments, or extended fields), THE Reticulum_Sidecar SHALL extract only the text content and discard unsupported fields without error

### Requirement 9: Transport Configuration

**User Story:** As a user, I want to configure which Reticulum transports are active, so that I can use the channel over my available hardware (LoRa, TCP, serial).

#### Acceptance Criteria

1. THE Reticulum_Channel SHALL support configuration of multiple Transport_Interfaces: TCP, LoRa (via RNode), serial, I2P, and AutoInterface (LAN discovery)
2. WHEN the Reticulum_Sidecar starts, THE Reticulum_Sidecar SHALL auto-detect available Transport_Interfaces from the user's ~/.reticulum/config file
3. THE Reticulum_Channel SHALL expose transport configuration through the add-on settings panel, allowing the user to enable, disable, or modify Transport_Interface parameters
4. WHEN a configured Transport_Interface fails to initialize, THE Reticulum_Sidecar SHALL log the failure, skip the failed interface, and continue operating on remaining available interfaces
5. THE Reticulum_Channel SHALL report the list of active Transport_Interfaces and their status in the channel metadata visible to the user

### Requirement 10: Privacy and Offline Operation

**User Story:** As a user, I want all mesh communication to be end-to-end encrypted and fully offline-capable, so that no message content passes through cloud services.

#### Acceptance Criteria

1. THE Reticulum_Sidecar SHALL rely exclusively on Reticulum's built-in cryptographic layer for message encryption, with no additional cloud-based encryption or relay services
2. THE Reticulum_Channel SHALL not transmit any message content, metadata, or destination identities to any cloud service or internet endpoint
3. WHEN operating exclusively on LoRa or serial Transport_Interfaces, THE Reticulum_Channel SHALL function without any internet connectivity
4. THE Reticulum_Sidecar SHALL manage Reticulum_Destination identity keys locally within the ~/.reticulum/ directory, with no key material transmitted externally
5. THE Reticulum_Channel SHALL not consume LLM provider tokens for its own operation (message encoding, routing, queuing); LLM tokens are consumed only when generating AI responses to incoming messages

### Requirement 11: Graceful Degradation

**User Story:** As a user, I want the system to continue working normally if the Reticulum sidecar crashes, so that mesh failures never affect my other channels.

#### Acceptance Criteria

1. IF the Reticulum_Sidecar process crashes or becomes unresponsive, THEN THE Reticulum_Channel SHALL display "offline" status in the channel UI and emit a shell notification
2. IF the Reticulum_Sidecar is unavailable, THEN THE desktop, telegram, and voice channels SHALL continue operating without any degradation or error
3. WHEN the Reticulum_Sidecar recovers from a crash (manually or via automatic restart), THE Reticulum_Channel SHALL resume message processing without requiring user intervention or loss of queued messages
4. IF the Reticulum_Sidecar fails to start on initial launch (missing Python environment, missing rns package, or invalid configuration), THEN THE Reticulum_Channel SHALL display a diagnostic message identifying the failure cause and remain in "offline" state without affecting other system components
5. THE Reticulum_Channel SHALL implement a health check by sending a JSON-RPC ping to the sidecar every 30 seconds, transitioning Sidecar_Health_State to "offline" if three consecutive pings fail

### Requirement 12: Behavioral Contract Integration

**User Story:** As a developer, I want the Reticulum Channel to ship with behavioral contracts, so that the Engineer Backtest Mode can verify its correctness across future changes.

#### Acceptance Criteria

1. THE Reticulum_Channel SHALL register Behavioral_Contracts in the Phase 0 Contract_Registry covering: sidecar lifecycle transitions produce valid Sidecar_Health_State values, inbound messages are correctly inserted into ConversationThreads, and outbound messages are correctly serialized to JSON-RPC requests
2. THE Reticulum_Channel SHALL register Behavioral_Contracts covering: message queuing preserves FIFO order, delivery acknowledgements update ConversationMessage status correctly, and bandwidth-aware summarization produces responses within the configured size limit
3. THE Reticulum_Channel SHALL register Behavioral_Contracts covering: channel removal does not affect other channels, sidecar crash does not propagate errors to the host shell, and transport configuration changes are applied without restart
4. WHEN a Behavioral_Contract for the Reticulum_Channel fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report identifying the failing contract

### Requirement 13: JSON-RPC Protocol Contract

**User Story:** As a developer, I want a well-defined protocol between the Rust host and the Python sidecar, so that both sides can be developed and tested independently.

#### Acceptance Criteria

1. THE stdio-json-rpc protocol between the host and Reticulum_Sidecar SHALL support the following request methods: "start", "stop", "send_message", "get_status", "list_peers", and "ping"
2. THE stdio-json-rpc protocol SHALL support the following notification methods from sidecar to host: "message_received", "delivery_confirmed", "link_established", "link_lost", and "error"
3. THE "send_message" request SHALL accept parameters: destination_hash (string), content (string), and priority (string: "normal" or "high")
4. THE "message_received" notification SHALL include fields: source_hash (string), source_name (string or null), content (string), timestamp (ISO 8601 string), and lxmf_message_id (string)
5. THE Reticulum_Channel SHALL include a JSON Schema definition for all request and response types in the add-on package, enabling independent validation of both host and sidecar implementations
6. FOR ALL valid JSON-RPC requests sent by the host, parsing the request in the sidecar and serializing the response back SHALL produce a valid JSON-RPC response (round-trip property)

### Requirement 14: Performance and Resource Isolation

**User Story:** As a user, I want the Reticulum channel to operate without impacting shell responsiveness or other channels, so that mesh networking is invisible during normal desktop use.

#### Acceptance Criteria

1. THE Reticulum_Sidecar SHALL run as a separate OS process, isolated from the Tauri main thread and frontend render thread
2. THE Reticulum_Channel SHALL perform all JSON-RPC communication asynchronously without blocking the shell event loop
3. WHILE the Reticulum_Sidecar is processing messages, THE shell SHALL maintain sub-100-millisecond responsiveness for user interactions on other channels
4. THE Reticulum_Sidecar Python process SHALL consume no more than 50 MB of resident memory during normal operation with up to 10 active peers
5. THE Reticulum_Channel SHALL not increase the token count of any agent prompt beyond what is required for the active Reticulum conversation thread being responded to
