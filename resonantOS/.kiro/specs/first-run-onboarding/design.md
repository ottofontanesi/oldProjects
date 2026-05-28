# Design Document: First-Run Onboarding

## Overview

The first-run experience guides new users through: hardware detection → model recommendation → first download → test inference → optional network discovery → transition to dashboard. It's a multi-step wizard that persists progress (resumable after interruption) and sets the `setup_complete` flag on completion.

### Design Principles

1. **One-click QuickStart**: Users who just want it to work can accept defaults with a single click.
2. **Resumable**: If the app closes mid-wizard, it resumes from the last completed step.
3. **Informative**: Each step explains what's happening and why.
4. **Skippable**: Power users can skip the wizard entirely.
5. **Progressive**: Start simple, offer advanced options for those who want them.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    OnboardingWizard                               │
│                                                                  │
│  Step 1: Welcome                                                 │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  "Welcome to ResonantOS"                                  │    │
│  │  [Quick Start] [Custom Setup] [Skip]                      │    │
│  └──────────────────────────────────────────────────────────┘    │
│                            │                                     │
│  Step 2: Hardware Detection                                      │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  Detecting hardware...                                    │    │
│  │  CPU: AMD Ryzen 9 5900X (12 cores, 4.8GHz)               │    │
│  │  RAM: 64 GB DDR4                                          │    │
│  │  GPU: NVIDIA RTX 4090 (24 GB VRAM)                        │    │
│  │  Storage: 2 TB NVMe (1.2 TB free)                         │    │
│  │  Classification: "High-end — can run models up to 33B"    │    │
│  └──────────────────────────────────────────────────────────┘    │
│                            │                                     │
│  Step 3: Model Recommendation                                    │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  Recommended for your hardware:                           │    │
│  │  ✅ Qwen 2.5 14B (Q4_K_M) — 9.2 GB — General chat       │    │
│  │  ✅ DeepSeek Coder 6.7B (Q5_K_M) — 5.1 GB — Coding      │    │
│  │  ✅ Phi 3.5 3.8B (Q4_K_M) — 2.3 GB — Fast tasks         │    │
│  │                                                           │    │
│  │  [Accept & Download] [Choose Different Models]            │    │
│  └──────────────────────────────────────────────────────────┘    │
│                            │                                     │
│  Step 4: Download & First Inference                              │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  Downloading Qwen 2.5 14B... [████████░░] 78% — 2:30 ETA │    │
│  │                                                           │    │
│  │  (After download completes:)                              │    │
│  │  Testing inference... "Hello! I'm your local AI..."       │    │
│  │  Speed: 32 tokens/second ✅                               │    │
│  └──────────────────────────────────────────────────────────┘    │
│                            │                                     │
│  Step 5: Network Discovery (Optional)                            │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  Scanning local network...                                │    │
│  │  Found: "Desktop-Office" (64GB RAM, RTX 4090)             │    │
│  │  [Join Network] [Skip — I'm the only node]               │    │
│  └──────────────────────────────────────────────────────────┘    │
│                            │                                     │
│  Step 6: Complete                                                │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  ✅ Setup Complete!                                       │    │
│  │  • Hardware: High-end (RTX 4090, 64GB)                    │    │
│  │  • Models: 3 downloaded (16.6 GB total)                   │    │
│  │  • Network: 1 peer discovered                             │    │
│  │  [Open Dashboard]                                         │    │
│  └──────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## Components

### WizardState (persisted)

```rust
pub struct WizardState {
    pub current_step: WizardStep,
    pub hardware_profile: Option<HardwareProfile>,
    pub selected_models: Vec<ModelSelection>,
    pub downloads_complete: Vec<String>,  // model_ids that finished downloading
    pub network_peers_found: Vec<NodeId>,
    pub started_at_ms: u64,
}

pub enum WizardStep {
    Welcome,
    HardwareDetection,
    ModelRecommendation,
    DownloadAndTest,
    NetworkDiscovery,
    Complete,
}
```

### ModelRecommender

```rust
pub struct ModelRecommender;

impl ModelRecommender {
    /// Given hardware profile, recommend 1-3 models from the catalog.
    pub fn recommend(
        hardware: &HardwareProfile,
        catalog: &[ModelEntry],
    ) -> Vec<ModelRecommendation>;
}

pub struct ModelRecommendation {
    pub model_entry: ModelEntry,
    pub reason: String,           // "Best general chat model for your hardware"
    pub download_size_gb: f64,
    pub estimated_tok_s: f64,
    pub estimated_download_time_secs: u64,
}
```

Recommendation logic:
1. Filter catalog to models that fit in available RAM (with headroom)
2. For "High-end" (≥32GB + GPU): recommend 14B chat + 7B coding + 3B fast
3. For "Mid-range" (16-32GB): recommend 7B chat + 3B coding
4. For "Basic" (≤16GB): recommend 3B chat only
5. Prefer models with highest task affinity for common tasks (chat, coding)

### QuickStart Flow

```
User clicks [Quick Start]
    │
    ├─ Auto-detect hardware (3 seconds)
    ├─ Auto-select recommended models
    ├─ Start downloading first model immediately
    ├─ Show progress
    ├─ Run test inference on completion
    ├─ Skip network discovery
    ├─ Set setup_complete = true
    └─ Navigate to dashboard
```

Total QuickStart time: hardware detection (3s) + download (varies) + test inference (5s)

## Progress Persistence

```rust
// After each step completes:
fn persist_wizard_progress(state: &WizardState, persistence: &PersistenceLayer) {
    persistence.set("wizard_state", serde_json::to_string(state).unwrap());
}

// On app restart during wizard:
fn resume_wizard(persistence: &PersistenceLayer) -> WizardState {
    match persistence.get("wizard_state") {
        Some(json) => serde_json::from_str(&json).unwrap_or(WizardState::new()),
        None => WizardState::new(),
    }
}
```

## Correctness Properties

### Property 1: Completion Flag
`setup_complete` SHALL be set to true if and only if the wizard reaches the Complete step.

### Property 2: Resume Accuracy
Resuming after interruption SHALL start from the last completed step (not repeat completed steps).

### Property 3: Recommendation Fit
Recommended models SHALL always fit within detected hardware (RAM + VRAM).

### Property 4: Skip Safety
Skipping the wizard SHALL result in a functional (but empty) app state.

## File Structure

### Backend
```
src/resonantos-vnext/src-tauri/src/
├── onboarding/
│   ├── mod.rs              # WizardState, step management
│   ├── hardware.rs         # Hardware detection + classification
│   ├── recommender.rs      # Model recommendation logic
│   └── commands.rs         # Tauri commands for wizard steps
```

### Frontend
```
src/resonantos-vnext/src/screens/
├── wizard/
│   ├── OnboardingWizard.tsx    # Main wizard container
│   ├── WelcomeStep.tsx         # Step 1
│   ├── HardwareStep.tsx        # Step 2
│   ├── ModelSelectStep.tsx     # Step 3
│   ├── DownloadStep.tsx        # Step 4
│   ├── NetworkStep.tsx         # Step 5
│   └── CompleteStep.tsx        # Step 6
```
