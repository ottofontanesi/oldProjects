# ResonantOS vNext — Build Guide

## Prerequisites

- **Rust** 1.94+ via rustup (`C:\Users\fontanesio\.cargo\bin\`)
- **MinGW-w64** 14.2 portable (`C:\Users\fontanesio\Documents\mingw64\`)
- **Node.js** 26+ (`C:\Users\fontanesio\Documents\node-v26.1.0-win-x64\`)
- **No admin required** — uses GNU toolchain, no VS Build Tools needed

## Quick Start (PowerShell)

```powershell
# 1. Set environment
$env:PATH = "C:\Users\fontanesio\Documents\mingw64\bin;C:\Users\fontanesio\.cargo\bin;C:\Users\fontanesio\Documents\node-v26.1.0-win-x64;$env:PATH"
$env:CARGO_HTTP_CHECK_REVOKE = "false"

# 2. Check Rust backend compiles
cd src/resonantos-vnext/src-tauri
cargo check --lib

# 3. Check frontend compiles
cd ..
npx tsc --noEmit
```

## Rust Backend

```powershell
cd src/resonantos-vnext/src-tauri

# Type-check only (fast, no linking — ~20s incremental)
cargo check --lib

# Full debug build (produces .dll/.exe)
cargo build

# Release build (optimized)
cargo build --release

# Run tests (unit + proptest)
cargo test

# Run a specific test module
cargo test mesh::
cargo test network::
```

### Toolchain Details

| Component | Path / Value |
|-----------|-------------|
| Toolchain | `stable-x86_64-pc-windows-gnu` (rustup override) |
| Rust version | 1.94.1 (or newer stable) |
| Linker | MinGW `gcc` / `ld` |
| Target | `x86_64-pc-windows-gnu` |
| rust-toolchain.toml | channel 1.94.1, profile minimal |

The `rustup override` in `src-tauri/` takes precedence over `rust-toolchain.toml`.
This avoids needing VS Build Tools / `link.exe` entirely.

### Crate Features

- `tract-onnx` — enables ONNX RL policy inference. Build with:
  ```powershell
  cargo build --features tract-onnx
  ```
- `local-inference` — enables llama.cpp local inference backend (requires C++ build tools). Deferred.
- Without features: all modules compile with graceful fallbacks (mock inference, no model loading)

## Frontend (TypeScript/React)

```powershell
cd src/resonantos-vnext

# Install dependencies
npm install

# Type-check (no emit)
npx tsc --noEmit

# Dev server (Vite)
npm run dev

# Production build
npm run build

# Run tests (Vitest + fast-check)
npx vitest --run
```

## Tauri App (Full Stack)

```powershell
cd src/resonantos-vnext

# Development (hot-reload frontend + Rust rebuild)
npm run tauri dev

# Production build (installer output in src-tauri/target/release/bundle/)
npm run tauri build
```

## Project Structure

```
src/resonantos-vnext/
├── src-tauri/src/              # Rust backend (~120 source files)
│   ├── lib.rs                 # Crate root, Tauri command registration
│   ├── main.rs                # Tauri app entry point (desktop GUI)
│   ├── bin/node_daemon.rs     # Headless daemon entry point (no GUI)
│   ├── daemon/                # Headless node daemon (config, health, control API)
│   ├── agents/                # Distributed agent execution (DAG, router, executor, orchestrator)
│   ├── backends/              # Hardware Abstraction Layer (6 backends + sidecar + preparation)
│   ├── companion/             # Phone companion app (pairing, inference, lifecycle, NPU)
│   ├── mesh/                  # Mesh network optimizer (consensus, reputation, incentive)
│   ├── network/               # Local network optimizer + simulator + unified scheduler
│   │   ├── download/          # Model download engine (chunked, resumable)
│   │   ├── catalog_store.rs   # Model catalog registry (50 models, persistence)
│   │   └── solver*.rs         # Placement solver (agents, contention, Pareto)
│   ├── transport/             # Unified mesh transport layer
│   │   ├── adapters/          # LAN (mDNS/TCP), WireGuard (userspace), Reticulum
│   │   └── qos.rs             # QoS: priority queue, DSCP, token bucket, congestion
│   ├── inference/             # Inference engines
│   │   ├── split/             # Split inference + adaptive segment scheduling (CollaPipe)
│   │   └── local/             # Local llama.cpp backend (feature-gated)
│   ├── integration/           # RL-optimizer integration (ONNX + MARL decentralized)
│   ├── integration_tests/     # End-to-end cross-module tests (TestWorld harness)
│   ├── persistence/           # SQLite persistence (nodes, checkpoints, placements)
│   ├── ipc/                   # Tauri IPC commands + event emitters
│   ├── wizard/                # Onboarding wizard backend (state, discovery, pairing)
│   ├── service_registry.rs    # Service lifecycle management
│   ├── startup.rs             # App startup orchestrator (dependency-ordered init)
│   ├── optimizer_timer.rs     # 60-second optimizer cycle timer
│   ├── shutdown.rs            # Graceful shutdown (reverse-order, 5s budget)
│   └── *_service.rs           # Domain services (hardware, provider, cost, etc.)
├── src/                        # React/TypeScript frontend
│   ├── App.tsx                # Main app shell (2900 lines, lazy-loaded screens)
│   ├── hooks/                 # Live data hooks (Tauri events → React state)
│   ├── providers/             # DashboardProvider (context for all live data)
│   ├── components/dashboard/  # Network ops dashboard (6 panels, React.memo)
│   └── modules/               # Feature modules (archive, chat, settings, etc.)
├── .github/workflows/         # CI: typecheck.yml (manual/PR trigger)
├── assets/model_catalog.json  # Bundled model catalog (30 entries, 8 families)
├── Cargo.toml                 # Rust dependencies + [[bin]] targets
└── package.json               # Frontend dependencies
```

## Troubleshooting

| Issue | Fix |
|-------|-----|
| `link.exe not found` | Ensure MinGW `bin/` is first in PATH |
| `dlltool CreateProcess` error | Use full MinGW (not Rust self-contained) |
| `CARGO_HTTP_CHECK_REVOKE` errors | `$env:CARGO_HTTP_CHECK_REVOKE = "false"` |
| Toolchain override not active | `rustup override set stable-x86_64-pc-windows-gnu` in src-tauri/ |
| Crate download timeouts | Retry — corporate proxy drops long connections |
| `error[E0463]: can't find crate` | `cargo clean` then rebuild |
| 557 warnings after fresh clone | Normal on first build; `cargo fix --lib --allow-dirty` cleans most |

## Build Status (2026-05-28)

- Rust backend: **compiles clean** (0 errors, warnings only)
- All 14 integration specs + 4 architecture specs implemented
- Total Rust modules: 16 directories, 120+ source files
- Property-based tests: 90+ across all modules
- Feature gates: `tract-onnx`, `backend-llamacpp`, `backend-onnx`, `backend-tenstorrent`, `backend-ascend`
- CI: GitHub Actions workflow at `.github/workflows/typecheck.yml` (manual/PR trigger)
- Research integrations: CollaPipe (Lyapunov scheduling), MARL (decentralized policies)
- Hardware backends: 6 built-in (llama.cpp, Ollama, OpenAI API, ONNX, Tenstorrent, Ascend) + sidecar plugins
- **Two binary targets:** `resonantos-vnext` (desktop GUI) + `resonantos-node` (headless daemon)

---

## Notable Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `ed25519-dalek` | 2.x | Cryptographic identity (mesh identity, companion pairing) |
| `proptest` | 1.x | Property-based testing (dev-dependency) |
| `uuid` | 1.x | Node/agent/workflow identifiers |
| `dashmap` | 6.x | Concurrent hash maps for agent caches |
| `sha2` | 0.10 | Checksum verification (model/agent downloads) |
| `tract-onnx` | 0.21 | Optional ONNX inference for RL policy |
| `tokio` | 1.x | Async runtime (agent execution, transport) |

The `ed25519-dalek` crate with `rand_core` feature is required for mesh identity
generation in both the mesh module and the companion app. The `proptest` crate
is a dev-dependency only — it does not affect release binary size.

---

## Cross-Platform Installation

The app is fully cross-platform (Windows, macOS, Linux). Tauri + Rust + React
all support all three platforms natively.

### Windows

**Prerequisites:**
- [Rust](https://rustup.rs) with `stable-x86_64-pc-windows-msvc` toolchain
- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) (C++ workload)
- [Node.js](https://nodejs.org) 20+
- [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) (usually pre-installed on Windows 10/11)

```powershell
# Install Rust
winget install Rustlang.Rustup

# Install Node
winget install OpenJS.NodeJS.LTS

# Clone and build
git clone <repo-url>
cd resonantos-vnext
npm install
npx tauri dev
```

**Without admin (GNU toolchain — compile only, can't run):**
```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup override set stable-x86_64-pc-windows-gnu
# Requires MinGW-w64 in PATH (see BUILD.md main section)
cargo check --lib  # Compiles but binary won't run (WebView2 ABI mismatch)
```

### macOS

**Prerequisites:**
- Xcode Command Line Tools
- [Rust](https://rustup.rs)
- [Node.js](https://nodejs.org) 20+

```bash
# Install prerequisites
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install node

# Clone and build
git clone <repo-url>
cd resonantos-vnext
npm install
npx tauri dev
```

**Apple Silicon (M1/M2/M3):** Works natively. The hardware detection module
uses Metal APIs for GPU profiling. No Rosetta needed.

### Linux

**Prerequisites:**
- Build essentials, webkit2gtk, and related libraries
- [Rust](https://rustup.rs)
- [Node.js](https://nodejs.org) 20+

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install -y build-essential curl wget file \
  libssl-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libwebkit2gtk-4.1-dev

# Fedora
sudo dnf install -y gcc-c++ webkit2gtk4.1-devel openssl-devel \
  gtk3-devel libappindicator-gtk3-devel librsvg2-devel

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Node (via nvm recommended)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.0/install.sh | bash
nvm install 20

# Clone and build
git clone <repo-url>
cd resonantos-vnext
npm install
npx tauri dev
```

### Platform-Specific Features

| Feature | Windows | macOS | Linux |
|---------|---------|-------|-------|
| GPU Detection (NVIDIA/CUDA) | ✅ nvml.dll | ❌ | ✅ libnvidia-ml |
| GPU Detection (Metal) | ❌ | ✅ system_profiler | ❌ |
| Thermal Monitoring | ✅ WMI | ✅ powermetrics | ✅ /sys/class/thermal |
| Storage Type Detection | ✅ PowerShell | ✅ (assumes NVMe) | ✅ /sys/block |
| Native Browser Embed | ✅ WebView2 | ✅ WKWebView | ✅ WebKitGTK |
| Network Optimizer | ✅ | ✅ | ✅ |
| Mesh Networking | ✅ | ✅ | ✅ |
| Split Inference | ✅ | ✅ | ✅ |
| Reticulum Transport | ✅ | ✅ | ✅ |
| Phone Pairing | ✅ | ✅ | ✅ |

All networking, optimization, and inference modules are pure Rust with no
platform-specific code — they work identically on all platforms.
