# ResonantOS vNext — Architecture Overview

Generated from source code analysis on 2026-05-24.

---

## 1. High-Level System Architecture

```mermaid
flowchart TB
  subgraph User["👤 Human Operator"]
    Desktop["Desktop UI"]
    Telegram["Telegram Client"]
  end

  subgraph Shell["ResonantOS Shell (Tauri v2 + React 19)"]
    direction TB
    subgraph Frontend["Frontend Layer (TypeScript/React)"]
      AppRoot["App.tsx — Shell Composition Root"]
      ChatRail["Chat Rail (right sidebar)"]
      Workspaces["Center Workspaces (lazy-loaded)"]
      NavDock["Left Navigation Dock"]
    end

    subgraph CoreTS["Core TypeScript Layer"]
      Contracts["contracts.ts — 2750 lines of typed interfaces"]
      Runtime["runtime.ts — Tauri IPC bridge"]
      Policies["policies.ts — Archive/provider guards"]
      ProviderSvc["provider-service.ts — Route resolution"]
      ContextMem["context-memory.ts — Budget & compaction"]
      Delegation["delegation.ts — Packet validation"]
      ModelStrategy["model-strategy.ts — Workload routing"]
      MemoryProvider["memory-provider.ts — Broker"]
    end

    subgraph SDK["Add-on SDK"]
      Validation["validation.ts — Manifest schema"]
      Registry["registry.ts — Catalog helpers"]
      AddonContracts["contracts.ts — SDK types"]
    end
  end

  subgraph TauriHost["Rust/Tauri Host Services (Privileged Boundary)"]
    HostState["host_state.rs — State, secrets, manifests"]
    ProviderRust["provider_service.rs — Chat completion, streaming, diagnostics"]
    ArchiveRust["archive_service.rs — Living Archive host ops"]
    BrowserRust["browser_service.rs — Chromium CDP"]
    BrowserNative["browser_native_service.rs — CEF/WebView"]
    BrowserHost["browser_host_service.rs — Host commands"]
    DelegationRust["delegation_service.rs — Task workspaces"]
    RecoveryRust["recovery_service.rs — Engineer tool loop"]
    ComputeRust["compute_service.rs — Safe commands, probes"]
    ObsidianRust["obsidian_service.rs — Vault bridge"]
    HermesRust["hermes_service.rs — Agent bridge"]
    OpenCodeRust["opencode_service.rs — Coding agent"]
    PaperclipRust["paperclip_service.rs — Project mgmt"]
    TelegramRust["telegram_service.rs — Channel service"]
    TerminalRust["terminal_service.rs — PTY sessions"]
    MemoryRust["memory_service.rs — MCP bridge"]
  end

  subgraph External["External Dependencies"]
    MiniMax["MiniMax Cloud API"]
    OpenAI["OpenAI API"]
    GX10["GX10 LAN Server\n(llama.cpp)"]
    LocalOllama["Local Ollama"]
    Chromium["Chromium Engine"]
    ObsidianApp["Obsidian App"]
    NAS["NAS Backup"]
    ArchiveFS["Living Archive\nFilesystem"]
    SQLite["SQLite DB"]
  end

  Desktop --> Shell
  Telegram --> TelegramRust

  AppRoot --> ChatRail
  AppRoot --> Workspaces
  AppRoot --> NavDock
  ChatRail --> ContextMem
  ChatRail --> ProviderSvc
  Workspaces --> Runtime
  Runtime --> TauriHost

  ProviderRust --> MiniMax
  ProviderRust --> OpenAI
  ProviderRust --> GX10
  ProviderRust --> LocalOllama
  ArchiveRust --> ArchiveFS
  ArchiveRust --> SQLite
  BrowserRust --> Chromium
  ObsidianRust --> ObsidianApp
  ComputeRust --> GX10
  ComputeRust --> NAS
```

---

## 2. Frontend Module Map

```mermaid
flowchart LR
  subgraph Modules["src/modules/ — Feature Modules"]
    direction TB
    Chat["chat/\n• controller.ts\n• thread-controller.ts\n• composer-controller.ts\n• StrategistChatRail.tsx\n• archive-context.ts\n• run-guard.ts"]
    Archive["archive/\n• controller.ts\n• ArchiveWorkspace.tsx\n• archive-action-center.ts\n• archive-ai-memory-jobs.ts\n• ArchiveSearchPanel.tsx\n• ArchiveReviewDesk.tsx"]
    Addons["addons/\n• controller.ts\n• AddOnsWorkspace.tsx\n• HermesAddonPanel.tsx\n• ObsidianAddonPanel.tsx"]
    Settings["settings/\n• controller.ts\n• SettingsWorkspace.tsx\n• provider-templates.ts"]
    Compute["compute/\n• controller.ts\n• ComputeFabricWorkspace.tsx"]
    Delegation2["delegation/\n• DelegationWorkspace.tsx"]
    Browser["browser/\n• BrowserWorkspace.tsx"]
    Recovery["recovery/\n• controller.ts\n• RecoveryWorkspace.tsx"]
    Shell["shell/\n• controller.ts\n• selectors.ts\n• system-slots.ts"]
    Strategist["strategist/\n• controller.ts\n• StrategistWorkspace.tsx"]
    Hermes["hermes/\n• HermesWorkspace.tsx"]
    Obsidian["obsidian/\n• ObsidianWorkspace.tsx\n• ObsidianEditor.tsx\n• ObsidianVaultTree.tsx"]
    OpenCode["opencode/\n• OpenCodeWorkspace.tsx"]
    Paperclip["paperclip/\n• PaperclipWorkspace.tsx"]
    Terminal["terminal/\n• TerminalWorkspace.tsx"]
    Overview["overview/\n• OverviewWorkspace.tsx"]
  end
```

---

## 3. Agent & Channel Architecture

```mermaid
flowchart TB
  subgraph Agents["Core Agents"]
    Augmentor["Augmentor (strategist.core)\nPrimary trusted AI identity\nProvider: MiniMax → OpenAI fallback"]
    Engineer["Resonant Engineer (setup.core)\nSetup, repair, recovery\nProvider: Local → MiniMax fallback"]
    Ingest["Ingest Agent (archive-ingest.core)\nArchive interpretation\nProvider: MiniMax → OpenAI fallback"]
    HermesAgent["Hermes (hermes.agent)\nAdd-on agent\nProvider: MiniMax → Local fallback"]
  end

  subgraph Channels["Communication Channels"]
    DesktopMain["desktop-main\n(Augmentor primary)"]
    DesktopSetup["desktop-setup\n(Engineer console)"]
    DesktopEngineer["desktop-engineer\n(Recovery console)"]
    TelegramPrimary["telegram-primary\n(Augmentor remote)"]
    TelegramField["telegram-field\n(Field capture)"]
    DesktopHermes["desktop-hermes\n(Hermes add-on)"]
  end

  subgraph Workspaces2["Workspaces"]
    WMain["workspace-main\nTrusted Main"]
    WSetup["workspace-setup\nEngineering"]
    WRecovery["workspace-recovery\nEmergency Recovery"]
    WField["workspace-field\nField Capture"]
    WHermes["workspace-hermes\nHermes Integration"]
  end

  Augmentor --> DesktopMain
  Augmentor --> TelegramPrimary
  Augmentor --> TelegramField
  Engineer --> DesktopSetup
  Engineer --> DesktopEngineer
  HermesAgent --> DesktopHermes

  DesktopMain --> WMain
  DesktopSetup --> WSetup
  DesktopEngineer --> WRecovery
  TelegramField --> WField
  DesktopHermes --> WHermes
```

---

## 4. Provider Fabric & Model Strategy

```mermaid
flowchart TB
  subgraph Strategy["Model Strategy: Personal Studio"]
    direction TB
    AugChat["Augmentor Primary Chat\n→ MiniMax M2.7-highspeed"]
    EngineerRec["Engineer Recovery\n→ Local Gemma4 (floor)"]
    ArchiveIngest["Archive Ingest\n→ OpenAI GPT-5.5 (premium)"]
    Routine["Routine Background\n→ MiniMax M2.7-highspeed"]
  end

  subgraph Providers["Provider Profiles"]
    MiniMaxP["Shared MiniMax\nM2.7 / M2.7-highspeed\nSubscription · Experimental"]
    OpenAIP["Shared OpenAI\nGPT-5.5 / GPT-5.4-mini\nSubscription · Experimental"]
    LocalP["Shared Local\nGemma4-E2B:Q4\nLocal-runtime · Supported"]
    GX10P["GX10 llama.cpp\nGemma-4-26B / Qwen3.6-27B\nLAN-remote · Supported"]
  end

  subgraph Nodes["Runtime Nodes"]
    CloudMM["MiniMax Cloud"]
    CloudOAI["OpenAI Cloud"]
    LocalRes["Local Resurrect\n(deployable on demand)"]
    GX10Gemma["GX10 Gemma\n192.168.1.77:30000"]
    GX10Qwen["GX10 Qwen\n192.168.1.77:30001"]
  end

  subgraph FallbackChain["Core Fast Fallback Chain"]
    direction LR
    F1["1. MiniMax Cloud"] --> F2["2. GX10 Gemma"] --> F3["3. GX10 Qwen"] --> F4["4. OpenAI Cloud"] --> F5["5. Local Floor"]
  end

  AugChat --> MiniMaxP
  EngineerRec --> LocalP
  ArchiveIngest --> OpenAIP
  Routine --> MiniMaxP

  MiniMaxP --> CloudMM
  OpenAIP --> CloudOAI
  LocalP --> LocalRes
  GX10P --> GX10Gemma
  GX10P --> GX10Qwen
```

---

## 5. Compute Fabric

```mermaid
flowchart LR
  subgraph ComputeNodes["Enrolled Compute Nodes"]
    Desktop["Desktop Local\nPassive · safe-command-runner\nartifact-store"]
    GX10Node["GX10 Inference Server\nSSH · aarch64 · 121GB RAM\nNVIDIA GB10 · model-host\nservice-host · artifact-store"]
    NASNode["NAS Backup Storage\nSSH · x86_64\nartifact-store only"]
  end

  subgraph Capabilities["Node Roles"]
    SafeCmd["safe-command-runner"]
    ContainerRun["container-runner"]
    ModelHost["model-host"]
    ArtifactStore["artifact-store"]
    ServiceHost["service-host"]
  end

  Desktop --> SafeCmd
  Desktop --> ArtifactStore
  GX10Node --> SafeCmd
  GX10Node --> ModelHost
  GX10Node --> ServiceHost
  GX10Node --> ArtifactStore
  NASNode --> ArtifactStore
```

---

## 6. Living Archive Data Flow

```mermaid
flowchart TD
  subgraph Sources["Intake Sources"]
    UserChat["Chat transcripts"]
    Audio2TOL["Audio2TOL recordings"]
    ExternalAgents["External agent outputs"]
    ObsidianNotes["Obsidian vault notes"]
    BrowserEvidence["Browser screenshots/pages"]
    LibraryImport["Library folder imports"]
  end

  subgraph Intake["INTAKE Zone (raw, untrusted)"]
    IntakeTranscripts["INTAKE/transcripts"]
    IntakeAudio["INTAKE/audio2tol"]
    IntakeExternal["INTAKE/external-agents"]
  end

  subgraph Review["REVIEW Zone (pending approval)"]
    ReviewQueue["Review artifacts\nStrategist-review default"]
  end

  subgraph Wiki["WIKI Zone (trusted knowledge)"]
    Summaries["WIKI/summaries"]
    Entities["WIKI/entities"]
    Concepts["WIKI/concepts"]
    Syntheses["WIKI/syntheses"]
  end

  subgraph Domains["Memory Domains"]
    HumanKnowledge["Human Knowledge"]
    ExternalKnowledge["External Knowledge"]
    AIMemory["AI Memory"]
  end

  Sources --> Intake
  Intake -->|"Ingest Agent\n(premium model)"| Review
  Review -->|"Strategist approval\nor auto-approve"| Wiki
  Wiki --> Domains

  style Intake fill:#fff2cc,stroke:#b78900
  style Review fill:#e8eefc,stroke:#4f6fb5
  style Wiki fill:#dff7e8,stroke:#2d7d46
```

---

## 7. Add-on SDK & Capability Model

```mermaid
flowchart TB
  subgraph ManifestStructure["Add-on Manifest (SDK V0)"]
    Identity["id, name, version, author, category"]
    RuntimeType["runtimeType:\nui-module | embedded-module |\nlocal-service | agent-addon | channel-addon"]
    Surfaces["surfaces:\npage | panel | rail | floating-window |\nembedded-pane | modal | channel"]
    Capabilities["requestedCapabilities:\nfilesystem | archive-read | archive-intake-write |\nchat-interface | memory-provider | providers |\nshell | network | ui-embedding | browser-control |\nagent-delegation | notifications"]
    Provenance["provenance:\nbundled-core | curated-signed |\nenterprise-signed | sideloaded-unverified"]
  end

  subgraph TrustModel["Trust & Isolation"]
    Core["Core (shell-ui)\nFull access"]
    Addon["Add-on (embedded-surface)\nCapability-gated"]
    External["External (host-mediated)\nStrictest isolation"]
  end

  subgraph SystemSlots["Replaceable System Slots"]
    PrimaryAgent["primary-agent"]
    ChatInterface["chat-interface"]
    MemorySystem["memory-system"]
    CommChannel["communication-channel"]
  end

  ManifestStructure --> TrustModel
  TrustModel --> SystemSlots
```

---

## 8. Recovery Mode Architecture

```mermaid
flowchart TD
  Trigger["System fault detected\nor manual panic button"]
  ActivateRecovery["Activate Recovery Mode\nSwitch to Engineer Agent"]
  LocalFloor["Start at local floor\n(Gemma4-E2B:Q4)"]

  subgraph Checklist["Recovery Checklist"]
    Facts["1. Establish facts"]
    BetterBrain["2. Restore better brain\n(probe routes)"]
    Promote["3. Promote runtime\n(validated stronger model)"]
    DeepDiag["4. Deep diagnosis"]
    Changes["5. Track changes"]
    Report["6. Write report"]
  end

  Resolve["System stable\nReturn to Augmentor"]

  Trigger --> ActivateRecovery
  ActivateRecovery --> LocalFloor
  LocalFloor --> Facts
  Facts --> BetterBrain
  BetterBrain --> Promote
  Promote --> DeepDiag
  DeepDiag --> Changes
  Changes --> Report
  Report --> Resolve

  style Trigger fill:#f7dede,stroke:#a33b3b
  style LocalFloor fill:#fff2cc,stroke:#b78900
  style Resolve fill:#dff7e8,stroke:#2d7d46
```

---

## 9. Technology Stack Summary

| Layer | Technology | Purpose |
|-------|-----------|---------|
| Desktop Shell | **Tauri v2** (Rust) | Native window, IPC, privileged ops |
| Frontend | **React 19** + TypeScript | UI rendering, state management |
| Build | **Vite 6.4** | Dev server, bundling |
| Editor | **CodeMirror 6** | Markdown editing (Obsidian) |
| Terminal | **xterm.js 6** | Embedded terminal |
| Markdown | **react-markdown 10** + remark-gfm | Chat message rendering |
| Testing | **Vitest 3.2** + Testing Library | Unit/integration tests |
| E2E | **Playwright 1.59** | Browser automation tests |
| Backend DB | **rusqlite** (SQLite) | Archive metadata |
| HTTP Client | **reqwest** (Rust) | Provider API calls |
| WebSocket | **tungstenite** | Real-time connections |
| PTY | **portable-pty** | Terminal sessions |
| Crypto | **sha2** | Artifact hashing |
| Native Browser | **libloading** + CEF | Embedded Chromium |

---

## 10. Key Architectural Principles

1. **Privileged boundary**: Secrets, provider execution, filesystem writes, browser control, and archive promotion stay in Rust host services. The React layer renders state and collects intent only.

2. **Capability-gated add-ons**: Every add-on declares required capabilities. The host enforces grants at the IPC boundary before executing any privileged operation.

3. **Replaceable defaults**: Core system slots (chat, memory, agent, channel) have default providers but are designed to be swappable via the add-on manifest system.

4. **Cost-aware routing**: The model strategy routes each workload class to the cheapest acceptable model first, escalating through fallback chains rather than always using the strongest model.

5. **Recovery-first design**: The system always maintains a local inference floor (Gemma4-E2B) that can operate without network access, enabling bounded self-repair.

6. **Intake-only add-on writes**: Add-ons can write raw intake artifacts but never trusted knowledge pages directly. Promotion requires Strategist review.

7. **Audited delegation**: Task delegation uses canonical packets with explicit approval gates for destructive, financial, or identity-sensitive operations.
