# Requirements: Network Onboarding Wizard (Phase 9C)

## Overview

The Network Onboarding Wizard provides guided flows for users to set up local multi-machine networks, join mesh networks, pair phones, and understand the optimization system. Phase 8's onboarding covers single-machine setup only — this phase extends it to multi-node scenarios where users need to discover other machines, pair devices, select trust tiers, and understand what the optimizer will do with their hardware.

The wizard must be approachable for non-technical users while providing enough detail for power users. It handles the "first 5 minutes" experience of going from a single-machine ResonantOS install to a functioning local network or mesh participant.

## User Stories

### US-1: Local Multi-Machine Setup
As a user with 3 home PCs, I want a step-by-step wizard that discovers my other machines on the LAN, helps me install the ResonantOS agent on them, and shows me the combined network capacity, so I can start using distributed AI without manual configuration.

### US-2: Mesh Join via Invitation
As a user who received an invitation link from a friend, I want to click the link, see what mesh I'm joining, choose my trust level, and complete the join in under 2 minutes, so I can start benefiting from the shared network immediately.

### US-3: Phone Pairing
As a user who wants to add my phone to my local network, I want to scan a QR code displayed on my desktop, install the companion app, and have the phone automatically join as a best-effort node, so my phone contributes its NPU when idle.

### US-4: First-Time Optimization Explanation
As a new user who just set up a multi-machine network, I want the wizard to explain what the optimizer will do (which models it will load, why, and what I gain), so I understand the system before it starts making changes to my machines.

### US-5: Network Health Pre-Check
As a user about to join a mesh, I want the wizard to check my network conditions (latency, bandwidth, firewall) and warn me about potential issues before I commit, so I don't join a mesh that won't work well for me.

### US-6: Trust Tier Education
As a user invited to a mesh, I want the wizard to clearly explain what each trust tier means (what data the mesh can see, what my machine will do), so I can make an informed decision about my privacy level.

## Functional Requirements

### FR-1: Local Network Setup Wizard
- FR-1.1: Wizard triggered from settings or first-time multi-machine detection (mDNS finds another ResonantOS node)
- FR-1.2: Step 1 — Network Scan: automatically discover other ResonantOS nodes on LAN via mDNS, display found nodes with hostname, IP, hardware summary
- FR-1.3: Step 2 — Agent Installation: for discovered machines without ResonantOS agent, provide one-click install instructions (platform-specific: Windows installer URL, macOS dmg, Linux apt/snap command)
- FR-1.4: Step 3 — Node Confirmation: user confirms which discovered nodes to include in their local network (checkboxes, all selected by default)
- FR-1.5: Step 4 — Capacity Preview: show combined network capacity (total RAM, total VRAM, total storage) and what models could be loaded that couldn't fit on any single machine
- FR-1.6: Step 5 — Optimization Preview: show what the optimizer WOULD do (proposed placement plan) before activating, with explanation of each decision
- FR-1.7: Step 6 — Activation: user confirms, optimizer activates, first plan executes
- FR-1.8: Support "Add another machine later" flow (abbreviated: scan → confirm → re-optimize)
- FR-1.9: Handle edge cases: no other nodes found (suggest installing on other machines), nodes found but unreachable (firewall guidance), nodes found but incompatible version

### FR-2: Mesh Join Wizard
- FR-2.1: Triggered by: clicking invitation link, pasting invitation token in UI, or scanning QR code
- FR-2.2: Step 1 — Invitation Decode: parse invitation token, display mesh name, inviter name, offered trust tier, expiry time
- FR-2.3: Step 2 — Trust Tier Education: explain what the offered tier means with clear language:
  - Tier 3 (Local-owned): "Full access — this mesh can use your hardware for any request and you can see all shared content"
  - Tier 2 (Invited-friend): "Shared inference — your machine will serve AI requests from mesh members. You'll see non-private prompts routed to you."
  - Tier 1 (Public/Relay): "Relay only — your machine helps route traffic but never sees prompt content. Minimal resource usage."
- FR-2.4: Step 3 — Network Health Check: test connectivity to mesh (latency, bandwidth), warn if conditions are poor (>200ms latency, <10Mbps bandwidth)
- FR-2.5: Step 4 — Capacity Offer: let user choose how much to share (slider: RAM, VRAM, GPU time, hours per day available)
- FR-2.6: Step 5 — Privacy Settings: configure sensitivity defaults (default policy, keyword list review, opt-in/out of cellular routing)
- FR-2.7: Step 6 — Confirmation: summary of what will happen, "Join Mesh" button
- FR-2.8: Step 7 — Post-Join: show mesh status, other members (anonymized by default), first optimization cycle ETA
- FR-2.9: Handle expired invitations gracefully (clear message, suggest requesting new one)
- FR-2.10: Handle already-a-member case (show current membership status instead)

### FR-3: Phone Pairing Flow
- FR-3.1: Desktop displays QR code containing: local network ID, pairing token, desktop's LAN address, protocol version
- FR-3.2: Phone companion app scans QR code and initiates pairing handshake over WiFi
- FR-3.3: Pairing handshake: phone sends capabilities (NPU type, RAM, battery, OS), desktop confirms and registers phone in node registry
- FR-3.4: Phone-specific settings during pairing:
  - Battery threshold for inference (default: 20%)
  - Allow cellular data (default: no)
  - Maximum model size (default: 3B, shown as "small models only")
  - Background execution preference (aggressive/balanced/conservative)
- FR-3.5: Post-pairing confirmation: show phone in network topology, explain what it will do ("Your phone will handle simple AI tasks when idle and on WiFi")
- FR-3.6: Handle pairing failures: phone not on same WiFi, QR code expired (5-minute validity), incompatible app version
- FR-3.7: Support re-pairing (phone was reset or app reinstalled) without losing history

### FR-4: Network Health Pre-Check
- FR-4.1: Run before joining any network (local or mesh):
  - LAN latency test: ping all discovered nodes, report RTT
  - Bandwidth test: small transfer test (1MB) to measure throughput
  - Firewall check: verify required ports are open (9741 for node protocol, 9742 for transfers)
  - DNS resolution: verify mDNS is working
  - Internet connectivity: check if model downloads will work
- FR-4.2: Display results as traffic-light indicators (green/yellow/red) with explanations
- FR-4.3: For yellow/red items, provide actionable fix suggestions:
  - Firewall: "Port 9741 is blocked. Open it in Windows Firewall → Advanced Settings → Inbound Rules"
  - High latency: "Latency to Node X is 150ms. This may affect split inference. Consider using Ethernet instead of WiFi."
  - Low bandwidth: "Bandwidth to Node X is 5Mbps. Model downloads will be slow. Consider connecting both machines to the same switch."
- FR-4.4: Allow user to proceed despite warnings (with acknowledgment) or fix and re-test
- FR-4.5: Save health check results for troubleshooting later

### FR-5: First-Time Optimization Explanation
- FR-5.1: After network setup completes, show an "Optimization Preview" panel explaining:
  - What models the optimizer plans to load and where
  - Why each model was chosen (workload demand, task affinity)
  - What each node gains from participating (incentive explanation)
  - Estimated performance improvement vs single-machine
- FR-5.2: Use plain language, not technical jargon:
  - Instead of "14B Q4_K_M tensor parallel across nodes A and B": "A large AI model (Qwen 14B) will be split across your Desktop and Laptop, giving you smarter responses than either machine could provide alone"
  - Instead of "Utility score: 0.73": "Your network is performing at 73% of its theoretical maximum"
- FR-5.3: Show before/after comparison: "Before: your desktop could run a 7B model. After: your network can run a 14B model (2x smarter)"
- FR-5.4: Allow user to adjust preferences before first optimization runs (model family preference, quality/speed balance)
- FR-5.5: "Start Optimizing" button that triggers first optimization cycle

### FR-6: Wizard State and Progress
- FR-6.1: Wizard state persisted — user can close and resume later without losing progress
- FR-6.2: Each step has a "Back" button (can revisit previous steps)
- FR-6.3: Progress indicator showing current step and total steps
- FR-6.4: Skip option for optional steps (e.g., phone pairing, privacy settings)
- FR-6.5: Wizard can be re-run from settings (e.g., to add another machine or re-pair a phone)
- FR-6.6: Wizard completion triggers a "Network Ready" notification

### FR-7: Error Recovery in Wizard
- FR-7.1: If a step fails (e.g., node unreachable during confirmation), show clear error with retry option
- FR-7.2: If network conditions change during wizard (node goes offline), update UI in real-time
- FR-7.3: If invitation expires during mesh join wizard, notify user and offer to request new invitation
- FR-7.4: If phone disconnects during pairing, offer retry without restarting entire flow
- FR-7.5: All errors include a "Help" link to relevant documentation

## Non-Functional Requirements

### NFR-1: Usability
- NFR-1.1: Complete local setup wizard in under 5 minutes (assuming nodes already have ResonantOS)
- NFR-1.2: Complete mesh join wizard in under 2 minutes
- NFR-1.3: Complete phone pairing in under 1 minute
- NFR-1.4: All text at 8th-grade reading level (no unexplained technical jargon)
- NFR-1.5: Wizard works on all supported platforms (Windows, macOS, Linux)
- NFR-1.6: Accessible: keyboard navigable, screen reader compatible, sufficient color contrast

### NFR-2: Reliability
- NFR-2.1: Wizard state survives app crash (persisted after each step)
- NFR-2.2: Network scan timeout: 5 seconds (don't block UI indefinitely)
- NFR-2.3: QR code valid for 5 minutes (security + usability balance)
- NFR-2.4: Health check completes within 10 seconds

### NFR-3: Security
- NFR-3.1: Pairing tokens are cryptographically random (128-bit minimum)
- NFR-3.2: QR codes contain no sensitive data beyond pairing token and LAN address
- NFR-3.3: Invitation tokens validated before any network action
- NFR-3.4: Phone pairing requires physical proximity (same WiFi network)

### NFR-4: Modularity
- NFR-4.1: Each wizard flow is independent (local setup, mesh join, phone pairing can be triggered separately)
- NFR-4.2: Wizard UI components reusable across flows (health check, capacity preview, trust explanation)
- NFR-4.3: Wizard logic separated from UI (testable without rendering)

## Correctness Properties

### Property 1: Wizard completeness
Every wizard flow SHALL have a defined terminal state (success or explicit cancellation). No flow SHALL leave the system in an intermediate/broken state.

### Property 2: State persistence
If the wizard is interrupted (app crash, user closes window) at any step, resuming SHALL restore the exact state of the last completed step.

### Property 3: Health check accuracy
Network health check results SHALL reflect actual measured values (latency, bandwidth, port status). No health check SHALL report "green" when the actual condition would prevent functionality.

### Property 4: Invitation validation
The mesh join wizard SHALL reject expired, malformed, already-consumed, or invalid-signature invitation tokens before any network action is taken.

### Property 5: Phone pairing security
Phone pairing SHALL only succeed when the phone is on the same local network as the desktop (verified by direct LAN communication, not relayed).

### Property 6: Trust tier transparency
The wizard SHALL display the exact capabilities and data access of each trust tier before the user commits. No capability SHALL be granted beyond what was displayed.

### Property 7: Capacity offer accuracy
The capacity preview shown in the wizard SHALL match the actual capacity that will be available after setup (within 10% margin for dynamic utilization).

### Property 8: Graceful degradation
If any wizard step fails, the system SHALL remain in its pre-wizard state. Partial wizard completion SHALL NOT leave orphaned configurations or broken network state.

### Property 9: Optimization preview fidelity
The optimization preview shown to the user SHALL be the actual plan that will execute (or a close approximation if conditions change between preview and execution).

### Property 10: QR code validity
QR codes generated for phone pairing SHALL be valid for exactly the configured duration (default 5 minutes) and SHALL be single-use (consumed on successful pairing).
