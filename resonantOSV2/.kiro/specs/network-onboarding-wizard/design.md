# Technical Design: Network Onboarding Wizard (Phase 9C)

## 1. Architecture Overview

The Network Onboarding Wizard is a React-based multi-step UI component integrated into the ResonantOS frontend, backed by Tauri commands that orchestrate discovery, pairing, health checks, and optimizer preview. It provides three independent flows (local setup, mesh join, phone pairing) that share common sub-components.

### 1.1 System Context

```
┌─────────────────────────────────────────────────────────────────┐
│                    ResonantOS Frontend (React)                    │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                   Onboarding Wizard UI                        │ │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐ │ │
│  │  │ Local Setup  │ │ Mesh Join    │ │ Phone Pairing        │ │ │
│  │  │ Flow         │ │ Flow         │ │ Flow                 │ │ │
│  │  └──────┬───────┘ └──────┬───────┘ └──────────┬───────────┘ │ │
│  │         │                 │                     │             │ │
│  │  ┌──────▼─────────────────▼─────────────────────▼───────────┐ │ │
│  │  │              Shared Components                             │ │ │
│  │  │  HealthCheck │ CapacityPreview │ TrustExplainer │ QRCode │ │ │
│  │  └──────────────────────────┬────────────────────────────────┘ │ │
│  └─────────────────────────────┼────────────────────────────────┘ │
│                                │ Tauri invoke()                    │
├────────────────────────────────┼──────────────────────────────────┤
│                    ResonantOS Backend (Rust)                       │
│                                │                                   │
│  ┌─────────────────────────────▼────────────────────────────────┐ │
│  │              Wizard Backend Service                            │ │
│  │  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌───────────┐ │ │
│  │  │ Discovery  │ │ Health     │ │ Pairing    │ │ Preview   │ │ │
│  │  │ Scanner    │ │ Checker    │ │ Manager    │ │ Generator │ │ │
│  │  └────────────┘ └────────────┘ └────────────┘ └───────────┘ │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                                                                    │
│  Uses: Phase 9A (Node Registry, Optimizer) │ Phase 9B (Mesh)      │
└────────────────────────────────────────────────────────────────────┘
```

### 1.2 Module Decomposition

| Module | Responsibility | Path |
|--------|---------------|------|
| `WizardLocalSetup` | Local multi-machine setup flow UI | `src/components/wizard/LocalSetupWizard.tsx` |
| `WizardMeshJoin` | Mesh join flow UI | `src/components/wizard/MeshJoinWizard.tsx` |
| `WizardPhonePairing` | Phone pairing flow UI | `src/components/wizard/PhonePairingWizard.tsx` |
| `HealthCheckPanel` | Network health check shared component | `src/components/wizard/HealthCheckPanel.tsx` |
| `CapacityPreview` | Combined capacity visualization | `src/components/wizard/CapacityPreview.tsx` |
| `TrustExplainer` | Trust tier education component | `src/components/wizard/TrustExplainer.tsx` |
| `OptimizationPreview` | First-time optimizer explanation | `src/components/wizard/OptimizationPreview.tsx` |
| `QRCodeDisplay` | QR code generation for phone pairing | `src/components/wizard/QRCodeDisplay.tsx` |
| `wizard_backend` | Rust backend for wizard operations | `src-tauri/src/wizard/mod.rs` |
| `wizard_discovery` | Network scan and node detection | `src-tauri/src/wizard/discovery.rs` |
| `wizard_health` | Health check execution | `src-tauri/src/wizard/health.rs` |
| `wizard_pairing` | Phone pairing protocol | `src-tauri/src/wizard/pairing.rs` |
| `wizard_preview` | Optimization preview generation | `src-tauri/src/wizard/preview.rs` |
| `wizard_state` | Wizard progress persistence | `src-tauri/src/wizard/state.rs` |

## 2. Data Models

### 2.1 Wizard State

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WizardState {
    pub wizard_id: uuid::Uuid,
    pub wizard_type: WizardType,
    pub current_step: u32,
    pub total_steps: u32,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
    pub step_data: HashMap<u32, StepData>,
    pub status: WizardStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WizardType {
    LocalSetup,
    MeshJoin,
    PhonePairing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WizardStatus {
    InProgress,
    Completed,
    Cancelled,
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepData {
    NetworkScan(NetworkScanResult),
    NodeSelection(Vec<NodeId>),
    HealthCheck(HealthCheckResult),
    CapacityPreview(CapacityPreviewData),
    OptimizationPreview(OptimizationPreviewData),
    InvitationDecode(InvitationDecodeResult),
    TrustTierSelection(TrustTier),
    CapacityOffer(CapacityOfferData),
    PrivacySettings(PrivacySettingsData),
    PhonePairingInit(PairingInitData),
    PhoneSettings(PhoneSettingsData),
    Confirmation(bool),
}
```

### 2.2 Network Scan

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkScanResult {
    pub scan_duration_ms: u64,
    pub discovered_nodes: Vec<DiscoveredNode>,
    pub scan_method: ScanMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredNode {
    pub node_id: Option<NodeId>,        // None if not yet registered
    pub hostname: String,
    pub ip_address: String,
    pub has_resonantos: bool,
    pub resonantos_version: Option<String>,
    pub hardware_summary: Option<HardwareSummary>,
    pub is_reachable: bool,
    pub latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSummary {
    pub cpu_name: String,
    pub ram_gb: f64,
    pub gpu_name: Option<String>,
    pub vram_gb: Option<f64>,
    pub device_type: DeviceType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScanMethod {
    Mdns,
    ManualEntry,
    Both,
}
```

### 2.3 Health Check

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub overall_status: HealthStatus,
    pub checks: Vec<HealthCheckItem>,
    pub completed_at: chrono::DateTime<chrono::Utc>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HealthStatus {
    Green,      // All good
    Yellow,     // Some warnings, can proceed
    Red,        // Critical issues, should fix first
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckItem {
    pub check_type: HealthCheckType,
    pub status: HealthStatus,
    pub value: String,              // e.g., "12ms", "95 Mbps", "Open"
    pub description: String,        // Human-readable explanation
    pub fix_suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HealthCheckType {
    LanLatency { target_node: String },
    Bandwidth { target_node: String },
    PortOpen { port: u16 },
    MdnsResolution,
    InternetConnectivity,
    FirewallStatus,
}
```

### 2.4 Capacity Preview

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityPreviewData {
    pub single_machine: MachineCapacity,
    pub combined_network: NetworkCapacity,
    pub models_unlocked: Vec<ModelUnlocked>,
    pub improvement_summary: String,    // "2x smarter models, 3x more variety"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineCapacity {
    pub ram_gb: f64,
    pub vram_gb: f64,
    pub largest_model: Option<String>,  // "Qwen 7B" — what fits on this machine alone
    pub estimated_tok_s: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkCapacity {
    pub total_ram_gb: f64,
    pub total_vram_gb: f64,
    pub node_count: u32,
    pub largest_model: Option<String>,  // "Qwen 14B" — what fits on the network
    pub estimated_tok_s: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUnlocked {
    pub model_name: String,
    pub parameter_count_b: f64,
    pub why_unlocked: String,           // "Requires 16GB VRAM — split across Desktop + Laptop"
    pub quality_improvement: String,    // "2x smarter than your current 7B model"
}
```

### 2.5 Optimization Preview

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationPreviewData {
    pub proposed_plan: Vec<PlainLanguagePlacement>,
    pub utility_before: f64,
    pub utility_after: f64,
    pub improvement_percent: f64,
    pub per_node_benefits: Vec<NodeBenefitExplanation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlainLanguagePlacement {
    pub model_name: String,             // "Qwen 2.5 14B"
    pub placement_description: String,  // "Split across Desktop and Laptop"
    pub why_chosen: String,             // "Best model for your coding tasks"
    pub performance_note: String,       // "~25 tokens/second"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBenefitExplanation {
    pub node_name: String,              // "Desktop (RTX 4090)"
    pub benefit: String,                // "Gains access to 14B model by sharing GPU with Laptop"
    pub before: String,                 // "Could only run 7B models alone"
    pub after: String,                  // "Can now use 14B model split with Laptop"
}
```

### 2.6 Phone Pairing

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingInitData {
    pub pairing_token: String,          // 128-bit random, base64
    pub desktop_lan_address: String,
    pub network_id: uuid::Uuid,
    pub protocol_version: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,  // +5 minutes
    pub qr_code_data: String,           // Encoded QR content
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhoneSettingsData {
    pub battery_threshold: u8,          // Default: 20
    pub allow_cellular: bool,           // Default: false
    pub max_model_size_b: f64,          // Default: 3.0
    pub background_mode: BackgroundMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BackgroundMode {
    Aggressive,     // Keep alive as much as possible
    Balanced,       // Default — respect OS power management
    Conservative,   // Only run when app is in foreground
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingHandshake {
    pub phone_capabilities: PhoneCapabilities,
    pub pairing_token: String,
    pub phone_node_id: NodeId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhoneCapabilities {
    pub os: PhoneOs,
    pub os_version: String,
    pub npu: Option<NpuType>,
    pub ram_gb: f64,
    pub battery_percent: u8,
    pub is_charging: bool,
    pub connection_type: ConnectionType,
    pub app_version: String,
}
```

### 2.7 Mesh Join Data

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationDecodeResult {
    pub valid: bool,
    pub mesh_name: Option<String>,
    pub inviter_name: Option<String>,
    pub offered_tier: Option<TrustTier>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_expired: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityOfferData {
    pub spare_ram_percent: u8,          // How much RAM to share (0-80%)
    pub spare_vram_percent: u8,         // How much VRAM to share (0-80%)
    pub spare_gpu_percent: u8,          // How much GPU time to share (0-80%)
    pub available_hours: f64,           // Hours per day machine is available
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySettingsData {
    pub default_sensitivity: PromptSensitivity,
    pub sensitive_keywords: Vec<String>,
    pub allow_cellular_for_mesh: bool,
}
```

## 3. Wizard Flow Logic

### 3.1 Local Setup Flow

```pseudocode
// Total steps: 6 (scan, install, confirm, capacity, preview, activate)

function local_setup_wizard():
    state = load_or_create_wizard_state(LocalSetup)
    
    // Step 1: Network Scan
    step1:
        scan_result = discovery_scanner.scan_lan(timeout: 5.seconds())
        display_discovered_nodes(scan_result)
        
        if scan_result.discovered_nodes.is_empty():
            show_empty_state("No other ResonantOS nodes found on your network")
            show_suggestions([
                "Install ResonantOS on another machine",
                "Check that both machines are on the same WiFi/Ethernet",
                "Try manual entry if the machine is on a VPN",
            ])
            offer_manual_entry()  // User can type IP address
        
        save_step(1, NetworkScan(scan_result))
    
    // Step 2: Agent Installation (only if nodes without ResonantOS found)
    step2:
        nodes_without_agent = scan_result.nodes.filter(|n| !n.has_resonantos)
        if nodes_without_agent.is_empty():
            skip_to_step(3)
        
        for node in nodes_without_agent:
            show_install_instructions(node)
            // Platform-specific:
            // Windows: "Download installer from [link] and run on [hostname]"
            // macOS: "Download .dmg from [link] and install on [hostname]"
            // Linux: "Run: curl -fsSL https://... | sh on [hostname]"
        
        show_button("I've installed it — Rescan")
        on_rescan: goto step1
    
    // Step 3: Node Confirmation
    step3:
        eligible_nodes = scan_result.nodes.filter(|n| n.has_resonantos AND n.is_reachable)
        show_checkboxes(eligible_nodes, all_selected: true)
        
        selected = wait_for_user_selection()
        save_step(3, NodeSelection(selected))
    
    // Step 4: Capacity Preview
    step4:
        my_capacity = get_local_capacity()
        combined = compute_combined_capacity(selected_nodes)
        models_unlocked = compute_unlocked_models(combined, catalog)
        
        preview = CapacityPreviewData {
            single_machine: my_capacity,
            combined_network: combined,
            models_unlocked,
            improvement_summary: generate_improvement_text(my_capacity, combined),
        }
        
        display_capacity_comparison(preview)
        save_step(4, CapacityPreview(preview))
    
    // Step 5: Optimization Preview
    step5:
        // Run optimizer in preview mode (don't execute, just show plan)
        preview_plan = optimizer.solve_preview(selected_nodes, catalog, demand)
        
        plain_language = translate_plan_to_plain_language(preview_plan)
        display_optimization_preview(plain_language)
        
        // Allow preference adjustment
        show_preference_sliders(quality, speed, mass)
        if preferences_changed:
            recompute_preview()
        
        save_step(5, OptimizationPreview(plain_language))
    
    // Step 6: Activation
    step6:
        show_summary(all_steps)
        show_button("Start Optimizing")
        
        on_confirm:
            // Register all selected nodes in the node registry
            for node in selected_nodes:
                registry.register(node)
            
            // Trigger first optimization
            optimizer.trigger(ManualTrigger)
            
            state.status = Completed
            save_state()
            
            show_success("Your network is ready! The optimizer is now managing your models.")
            show_notification("Network Ready")
```

### 3.2 Mesh Join Flow

```pseudocode
// Total steps: 7 (decode, trust, health, capacity, privacy, confirm, post-join)

function mesh_join_wizard(invitation_input: String):
    state = load_or_create_wizard_state(MeshJoin)
    
    // Step 1: Invitation Decode
    step1:
        result = decode_invitation(invitation_input)
        
        if !result.valid:
            show_error(result.error)
            if result.is_expired:
                show_suggestion("This invitation has expired. Ask the sender for a new one.")
            return
        
        display_invitation_info(result)
        // "You've been invited to join [Mesh Name] by [Inviter Name]"
        // "Trust level offered: Invited Friend"
        // "Expires: in 23 hours"
        
        save_step(1, InvitationDecode(result))
    
    // Step 2: Trust Tier Education
    step2:
        offered_tier = result.offered_tier
        
        display_trust_explanation(offered_tier)
        // Show what this tier means in plain language
        // Show what data the mesh can see
        // Show what your machine will do
        
        // User can accept offered tier or request different one
        // (requesting different tier sends message to inviter)
        accepted_tier = wait_for_tier_acceptance()
        save_step(2, TrustTierSelection(accepted_tier))
    
    // Step 3: Network Health Check
    step3:
        // Test connectivity to mesh entry point
        health = run_health_check(mesh_entry_points: result.mesh_endpoints)
        display_health_results(health)
        
        if health.overall_status == Red:
            show_warning("Network conditions may prevent good mesh performance")
            show_fix_suggestions(health.checks.filter(|c| c.status == Red))
            show_buttons(["Fix and Retest", "Join Anyway"])
        
        save_step(3, HealthCheck(health))
    
    // Step 4: Capacity Offer
    step4:
        // Show sliders for how much to share
        show_capacity_sliders(
            ram: default_50_percent,
            vram: default_50_percent,
            gpu_time: default_50_percent,
            hours_per_day: default_16,
        )
        
        offer = wait_for_capacity_selection()
        save_step(4, CapacityOffer(offer))
    
    // Step 5: Privacy Settings
    step5:
        show_privacy_config(
            default_sensitivity: NonSensitive,
            keyword_list: default_sensitive_keywords(),
            cellular_opt_in: false,
        )
        
        // Explain: "Prompts classified as 'sensitive' will never leave your machine"
        // Show example: "Banking questions → stays local. General coding help → can go to mesh"
        
        privacy = wait_for_privacy_config()
        save_step(5, PrivacySettings(privacy))
    
    // Step 6: Confirmation
    step6:
        show_summary([
            "Joining: [Mesh Name]",
            "Trust level: [Tier]",
            "Sharing: [X]% RAM, [Y]% GPU",
            "Privacy: [default policy]",
            "Network health: [Green/Yellow]",
        ])
        
        show_button("Join Mesh")
        
        on_confirm:
            // Execute join
            membership = mesh_service.join(invitation_token, accepted_tier, offer, privacy)
            save_step(6, Confirmation(true))
    
    // Step 7: Post-Join
    step7:
        show_mesh_status(membership)
        // "Welcome to [Mesh Name]!"
        // "Members online: 5"
        // "Next optimization cycle: in 12 minutes"
        // "Your reputation: 0.5 (neutral — will grow as you contribute)"
        
        state.status = Completed
        save_state()
```

### 3.3 Phone Pairing Flow

```pseudocode
// Total steps: 4 (QR display, handshake, settings, confirmation)

function phone_pairing_wizard():
    state = load_or_create_wizard_state(PhonePairing)
    
    // Step 1: QR Code Display
    step1:
        pairing_data = generate_pairing_data()
        // pairing_data contains: token, LAN address, network ID, expiry
        
        qr_content = encode_pairing_qr(pairing_data)
        display_qr_code(qr_content)
        
        show_instructions([
            "1. Open the ResonantOS companion app on your phone",
            "2. Tap 'Pair with Desktop'",
            "3. Scan this QR code",
        ])
        
        // Start listening for pairing handshake
        start_pairing_listener(pairing_data.pairing_token)
        
        // Show countdown timer (5 minutes)
        show_expiry_countdown(pairing_data.expires_at)
        
        // Wait for phone to connect
        match wait_for_handshake(timeout: 5.minutes()):
            Ok(handshake) => goto step2(handshake)
            Err(Timeout) => {
                show_error("QR code expired. Generate a new one?")
                show_button("Generate New QR Code")
                on_click: goto step1  // Regenerate
            }
            Err(WrongNetwork) => {
                show_error("Phone is not on the same WiFi network as this computer")
                show_suggestion("Connect your phone to [WiFi Name] and try again")
            }
    
    // Step 2: Handshake Received
    step2(handshake):
        // Phone connected! Show its capabilities
        display_phone_info(handshake.phone_capabilities)
        // "iPhone 15 Pro connected!"
        // "NPU: Apple Neural Engine (Gen 5)"
        // "RAM: 8GB"
        // "Battery: 72%, charging"
        
        save_step(2, PhonePairingInit(pairing_data))
    
    // Step 3: Phone Settings
    step3:
        show_phone_settings(
            battery_threshold: 20,
            allow_cellular: false,
            max_model_size: "Small models only (3B)",
            background_mode: Balanced,
        )
        
        // Plain language explanations:
        // Battery: "Phone won't do AI work below 20% battery"
        // Cellular: "Only use WiFi for AI tasks (saves mobile data)"
        // Model size: "Only small, fast models — won't slow your phone"
        // Background: "Balanced — works when idle, respects battery saver"
        
        settings = wait_for_settings()
        save_step(3, PhoneSettings(settings))
    
    // Step 4: Confirmation
    step4:
        // Register phone in node registry
        registry.register_phone(handshake.phone_node_id, handshake.phone_capabilities, settings)
        
        // Trigger re-optimization to include phone
        optimizer.trigger(NodeJoined(handshake.phone_node_id))
        
        show_success([
            "Phone paired successfully!",
            "Your phone will handle simple AI tasks when idle and on WiFi.",
            "You can manage phone settings in Network → Devices.",
        ])
        
        state.status = Completed
        save_state()
```

## 4. Interface Design

### 4.1 Tauri Commands

```rust
/// Start a network scan for the local setup wizard
#[tauri::command]
pub async fn wizard_scan_network(
    timeout_ms: u64,
    state: State<'_, WizardBackendState>,
) -> Result<NetworkScanResult, String> {
    state.discovery.scan(Duration::from_millis(timeout_ms)).await.map_err(|e| e.to_string())
}

/// Run network health check
#[tauri::command]
pub async fn wizard_health_check(
    target_nodes: Vec<String>,  // IP addresses or hostnames
    state: State<'_, WizardBackendState>,
) -> Result<HealthCheckResult, String> {
    state.health_checker.run(target_nodes).await.map_err(|e| e.to_string())
}

/// Generate capacity preview for selected nodes
#[tauri::command]
pub async fn wizard_capacity_preview(
    selected_nodes: Vec<NodeId>,
    state: State<'_, WizardBackendState>,
) -> Result<CapacityPreviewData, String> {
    state.preview_generator.capacity_preview(selected_nodes).await.map_err(|e| e.to_string())
}

/// Generate optimization preview (dry-run solver)
#[tauri::command]
pub async fn wizard_optimization_preview(
    selected_nodes: Vec<NodeId>,
    preferences: Option<UserPreferences>,
    state: State<'_, WizardBackendState>,
) -> Result<OptimizationPreviewData, String> {
    state.preview_generator.optimization_preview(selected_nodes, preferences).await.map_err(|e| e.to_string())
}

/// Decode a mesh invitation token
#[tauri::command]
pub async fn wizard_decode_invitation(
    token: String,
    state: State<'_, WizardBackendState>,
) -> Result<InvitationDecodeResult, String> {
    state.mesh_service.decode_invitation(&token).await.map_err(|e| e.to_string())
}

/// Generate QR code for phone pairing
#[tauri::command]
pub async fn wizard_generate_pairing_qr(
    state: State<'_, WizardBackendState>,
) -> Result<PairingInitData, String> {
    state.pairing_manager.generate_qr().await.map_err(|e| e.to_string())
}

/// Check if phone has connected (poll during pairing)
#[tauri::command]
pub async fn wizard_check_pairing_status(
    pairing_token: String,
    state: State<'_, WizardBackendState>,
) -> Result<Option<PairingHandshake>, String> {
    state.pairing_manager.check_status(&pairing_token).await.map_err(|e| e.to_string())
}

/// Complete local setup (register nodes, activate optimizer)
#[tauri::command]
pub async fn wizard_activate_local_network(
    selected_nodes: Vec<NodeId>,
    preferences: UserPreferences,
    state: State<'_, WizardBackendState>,
) -> Result<(), String> {
    state.activator.activate_local(selected_nodes, preferences).await.map_err(|e| e.to_string())
}

/// Complete mesh join
#[tauri::command]
pub async fn wizard_join_mesh(
    invitation_token: String,
    tier: TrustTier,
    capacity_offer: CapacityOfferData,
    privacy_settings: PrivacySettingsData,
    state: State<'_, WizardBackendState>,
) -> Result<MeshMembership, String> {
    state.mesh_service.join(invitation_token, tier, capacity_offer, privacy_settings)
        .await.map_err(|e| e.to_string())
}

/// Complete phone pairing
#[tauri::command]
pub async fn wizard_complete_phone_pairing(
    pairing_token: String,
    settings: PhoneSettingsData,
    state: State<'_, WizardBackendState>,
) -> Result<NodeId, String> {
    state.pairing_manager.complete(pairing_token, settings).await.map_err(|e| e.to_string())
}

/// Save wizard state (for resume after interruption)
#[tauri::command]
pub async fn wizard_save_state(
    wizard_state: WizardState,
    state: State<'_, WizardBackendState>,
) -> Result<(), String> {
    state.state_store.save(wizard_state).await.map_err(|e| e.to_string())
}

/// Load wizard state (resume interrupted wizard)
#[tauri::command]
pub async fn wizard_load_state(
    wizard_type: WizardType,
    state: State<'_, WizardBackendState>,
) -> Result<Option<WizardState>, String> {
    Ok(state.state_store.load(wizard_type).await)
}
```

### 4.2 Health Check Implementation

```pseudocode
function run_health_check(targets):
    checks = []
    
    // 1. mDNS Resolution
    mdns_ok = test_mdns_resolution()
    checks.push(HealthCheckItem {
        check_type: MdnsResolution,
        status: if mdns_ok { Green } else { Red },
        value: if mdns_ok { "Working" } else { "Failed" },
        description: "mDNS allows automatic discovery of nodes on your network",
        fix_suggestion: if !mdns_ok { Some("Enable mDNS/Bonjour. Windows: ensure 'Bonjour' service is running. Linux: install avahi-daemon.") } else { None },
    })
    
    // 2. Port checks
    for port in [9741, 9742]:
        open = test_port_open(port)
        checks.push(HealthCheckItem {
            check_type: PortOpen { port },
            status: if open { Green } else { Red },
            value: if open { "Open" } else { "Blocked" },
            description: format!("Port {} is needed for {}", port, port_description(port)),
            fix_suggestion: if !open { Some(firewall_fix_instructions(port)) } else { None },
        })
    
    // 3. Latency to each target
    for target in targets:
        latency = ping(target, count: 5)
        status = match latency.avg_ms:
            0.0..=10.0 => Green,
            10.0..=100.0 => Yellow,
            _ => Red,
        checks.push(HealthCheckItem {
            check_type: LanLatency { target_node: target.hostname },
            status,
            value: format!("{:.1}ms", latency.avg_ms),
            description: format!("Latency to {}", target.hostname),
            fix_suggestion: match status:
                Yellow => Some("Consider using Ethernet instead of WiFi for better performance"),
                Red => Some("High latency will affect split inference. Check network congestion."),
                _ => None,
        })
    
    // 4. Bandwidth to each target
    for target in targets:
        bw = measure_bandwidth(target, test_size: 1_000_000)  // 1MB test
        status = match bw.mbps:
            100.0.. => Green,
            10.0..=100.0 => Yellow,
            _ => Red,
        checks.push(HealthCheckItem {
            check_type: Bandwidth { target_node: target.hostname },
            status,
            value: format!("{:.0} Mbps", bw.mbps),
            description: format!("Bandwidth to {}", target.hostname),
            fix_suggestion: match status:
                Yellow => Some("Bandwidth is moderate. Large model downloads may be slow."),
                Red => Some("Low bandwidth. Consider connecting both machines to the same switch."),
                _ => None,
        })
    
    // 5. Internet connectivity
    internet = test_internet()
    checks.push(HealthCheckItem {
        check_type: InternetConnectivity,
        status: if internet { Green } else { Yellow },  // Yellow not Red — offline-first
        value: if internet { "Connected" } else { "Offline" },
        description: "Internet is needed for model downloads but not for inference",
        fix_suggestion: if !internet { Some("Model downloads require internet. Local inference works offline.") } else { None },
    })
    
    // Compute overall status
    overall = if checks.any(|c| c.status == Red) { Red }
              else if checks.any(|c| c.status == Yellow) { Yellow }
              else { Green }
    
    return HealthCheckResult { overall_status: overall, checks, completed_at: now(), duration_ms: elapsed }
```

### 4.3 Plain Language Translation

```pseudocode
function translate_plan_to_plain_language(plan):
    placements = []
    
    for p in plan.placements:
        model_name = friendly_model_name(p.model_id)  // "Qwen 2.5 14B" not "qwen2.5:14b-q4_K_M"
        
        placement_desc = match p.protocol:
            SingleNode => format!("Running on {}", node_name(p.assigned_nodes[0]))
            TensorParallel { .. } => format!("Split across {} (fast parallel)", 
                p.assigned_nodes.map(node_name).join(" and "))
            PipelineParallel { .. } => format!("Split across {} (sequential)", 
                p.assigned_nodes.map(node_name).join(" and "))
        
        why = determine_why_chosen(p, plan.demand)
        // "Best model for your coding tasks (used 60% of the time)"
        // "Fast model for quick questions"
        // "Large model for complex reasoning"
        
        perf = format!("~{} tokens/second", p.estimated_tok_s as u32)
        
        placements.push(PlainLanguagePlacement {
            model_name,
            placement_description: placement_desc,
            why_chosen: why,
            performance_note: perf,
        })
    
    // Per-node benefits
    benefits = []
    for (node_id, incentive) in plan.node_incentives:
        benefits.push(NodeBenefitExplanation {
            node_name: friendly_node_name(node_id),
            benefit: incentive.explanation,
            before: format!("Could run up to {} alone", largest_model_alone(node_id)),
            after: format!("Now has access to {}", plan.largest_model().name),
        })
    
    return OptimizationPreviewData {
        proposed_plan: placements,
        utility_before: compute_single_node_utility(),
        utility_after: plan.utility_scores.total,
        improvement_percent: ((plan.utility_scores.total - single_utility) / single_utility * 100.0),
        per_node_benefits: benefits,
    }
```

## 5. React Component Structure

### 5.1 Component Hierarchy

```typescript
// Main wizard container
interface WizardProps {
  type: 'local_setup' | 'mesh_join' | 'phone_pairing';
  initialData?: string;  // e.g., invitation token for mesh join
  onComplete: () => void;
  onCancel: () => void;
}

// Wizard step wrapper (handles navigation, progress, persistence)
interface WizardStepProps {
  stepNumber: number;
  totalSteps: number;
  title: string;
  canGoBack: boolean;
  canSkip: boolean;
  onNext: () => void;
  onBack: () => void;
  onSkip: () => void;
  children: React.ReactNode;
}

// Shared components used across flows
interface HealthCheckPanelProps {
  targets: string[];
  onComplete: (result: HealthCheckResult) => void;
  allowProceedOnWarning: boolean;
}

interface CapacityPreviewProps {
  data: CapacityPreviewData;
}

interface TrustExplainerProps {
  tier: TrustTier;
  showAllTiers: boolean;
  onTierSelect?: (tier: TrustTier) => void;
}

interface OptimizationPreviewProps {
  data: OptimizationPreviewData;
  onPreferencesChange: (prefs: UserPreferences) => void;
}

interface QRCodeDisplayProps {
  data: string;
  expiresAt: string;
  onExpired: () => void;
  onScanned: () => void;
}
```

### 5.2 State Management

```typescript
// Wizard state hook
function useWizardState(type: WizardType) {
  const [state, setState] = useState<WizardState | null>(null);
  
  // Load persisted state on mount
  useEffect(() => {
    invoke('wizard_load_state', { wizardType: type }).then(saved => {
      if (saved) setState(saved);
      else setState(createInitialState(type));
    });
  }, [type]);
  
  // Auto-save on state change
  useEffect(() => {
    if (state) {
      invoke('wizard_save_state', { wizardState: state });
    }
  }, [state]);
  
  const goToStep = (step: number) => setState(s => ({ ...s!, currentStep: step }));
  const saveStepData = (step: number, data: StepData) => {
    setState(s => ({
      ...s!,
      stepData: { ...s!.stepData, [step]: data },
      lastUpdated: new Date().toISOString(),
    }));
  };
  
  return { state, goToStep, saveStepData };
}
```

## 6. Error Handling

### 6.1 Wizard-Level Error Recovery

```pseudocode
function handle_wizard_error(error, current_step, wizard_state):
    match error:
        NodeUnreachable { node } => {
            // Node went offline during wizard
            show_inline_warning(format!("{} is no longer reachable", node.hostname))
            // Remove from selection, don't fail entire wizard
            wizard_state.remove_node(node)
            if wizard_state.selected_nodes.is_empty():
                show_error("All nodes are offline. Please check your network.")
                offer_rescan()
        }
        
        InvitationExpired => {
            show_error("This invitation has expired")
            show_suggestion("Ask the sender for a new invitation link")
            wizard_state.status = Failed { error: "Invitation expired" }
        }
        
        PairingTimeout => {
            show_error("Phone didn't connect in time")
            show_buttons(["Generate New QR Code", "Cancel"])
        }
        
        HealthCheckFailed { check } => {
            // Don't fail wizard — show warning and let user decide
            show_warning(format!("Health check issue: {}", check.description))
            show_buttons(["Fix and Retry", "Continue Anyway"])
        }
        
        OptimizerError { reason } => {
            // Preview failed — show simplified version
            show_warning("Couldn't generate full optimization preview")
            show_simplified_preview(wizard_state.selected_nodes)
        }
        
        _ => {
            // Generic error — preserve state, offer retry
            show_error(format!("Something went wrong: {}", error))
            show_button("Retry This Step")
            // State is already saved — user can close and resume
        }
```

### 6.2 Partial Completion Safety

```pseudocode
function ensure_clean_state_on_cancel(wizard_state):
    match wizard_state.wizard_type:
        LocalSetup => {
            // If we registered nodes but didn't activate optimizer, unregister them
            if wizard_state.current_step > 3 AND wizard_state.status != Completed:
                for node in wizard_state.step_data[3].selected_nodes:
                    registry.unregister(node)
        }
        
        MeshJoin => {
            // If we started join but didn't complete, send leave notification
            if wizard_state.current_step > 6 AND wizard_state.status != Completed:
                mesh_service.abort_join()
        }
        
        PhonePairing => {
            // If pairing listener is active, stop it
            pairing_manager.cancel_active_pairing()
            // Phone node not registered until step 4 completes, so no cleanup needed
        }
    
    wizard_state.status = Cancelled
    save_state(wizard_state)
```

## 7. Configuration

```rust
pub struct WizardConfig {
    // Scan
    pub network_scan_timeout_ms: u64,       // Default: 5000
    pub mdns_service_name: String,          // Default: "_resonantos._tcp.local"
    
    // Pairing
    pub qr_code_validity_secs: u64,         // Default: 300 (5 min)
    pub pairing_token_bytes: u32,           // Default: 16 (128-bit)
    pub pairing_listen_port: u16,           // Default: 9743
    
    // Health check
    pub health_check_timeout_ms: u64,       // Default: 10000
    pub bandwidth_test_size_bytes: u64,     // Default: 1_000_000 (1MB)
    pub latency_ping_count: u32,            // Default: 5
    pub latency_green_threshold_ms: f64,    // Default: 10.0
    pub latency_yellow_threshold_ms: f64,   // Default: 100.0
    pub bandwidth_green_threshold_mbps: f64,// Default: 100.0
    pub bandwidth_yellow_threshold_mbps: f64,// Default: 10.0
    
    // UI
    pub auto_select_all_nodes: bool,        // Default: true
    pub show_advanced_settings: bool,       // Default: false (expandable)
}
```

## 8. Testing Strategy

### 8.1 Property-Based Tests

| Property | Description | Generator Strategy |
|----------|-------------|-------------------|
| State persistence | Save/load roundtrip preserves all data | Random wizard states |
| Invitation validation | Expired/malformed tokens always rejected | Random token mutations |
| QR code validity | Codes expire after configured duration | Time-based testing |
| Health check accuracy | Results match simulated network conditions | Mock network with known properties |
| Capacity preview correctness | Combined capacity = sum of individual nodes | Random node capabilities |
| Clean cancellation | Cancel at any step leaves no orphaned state | Random step + cancel |

### 8.2 Integration Tests

| Test | Scenario |
|------|----------|
| Full local setup | Scan → select → preview → activate, verify optimizer running |
| Mesh join happy path | Decode → trust → health → offer → privacy → join, verify membership |
| Phone pairing | Generate QR → simulate handshake → settings → confirm, verify registration |
| Resume after crash | Start wizard, kill app at step 3, restart, verify resume at step 3 |
| No nodes found | Scan empty network, verify helpful empty state |
| Expired invitation | Try to join with expired token, verify clear error |
| Health check failures | Simulate blocked port, verify red status + fix suggestion |
| Cancel mid-flow | Cancel at each step, verify no side effects |

### 8.3 UI Tests (Component)

| Test | Scenario |
|------|----------|
| Step navigation | Forward/back/skip buttons work correctly |
| Progress indicator | Shows correct step count and current position |
| QR countdown | Timer counts down and triggers expiry callback |
| Capacity comparison | Before/after visualization renders correctly |
| Trust explainer | All three tiers display with correct descriptions |
| Preference sliders | Adjusting sliders triggers preview recomputation |
| Responsive layout | Wizard renders correctly on different screen sizes |

## 9. Dependencies

- **Phase 9A (Local Network Optimizer)**: Node registry, optimizer solver (for preview), model catalog
- **Phase 9B (Mesh Network Optimizer)**: Mesh membership service, invitation handling
- **Phase 7 (Hardware Detection)**: Node capability reporting
- **Phase 8 (Onboarding Doctor)**: Extends existing onboarding with network flows
- **React + Tauri**: Frontend framework and IPC layer
