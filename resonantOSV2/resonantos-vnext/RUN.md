# ResonantOS vNext — Run Guide

How to start, run, and use each component of the system.

---

## Environment (set once per terminal)

```powershell
$env:PATH = "C:\Users\fontanesio\Documents\mingw64\bin;C:\Users\fontanesio\.cargo\bin;C:\Users\fontanesio\Documents\node-v26.1.0-win-x64;$env:PATH"
$env:CARGO_HTTP_CHECK_REVOKE = "false"
```

---

## 1. Full App (Tauri Desktop)

The main way to run ResonantOS — launches the desktop shell with all services.

```powershell
cd src/resonantos-vnext
npx tauri dev
```

This starts:
- Vite dev server (frontend) on `127.0.0.1:1430`
- Rust backend compiled and launched as native window
- Hot-reload on frontend changes, auto-rebuild on Rust changes

**First launch** takes ~2-3 min (full Rust link). Subsequent launches ~20s.

To build a release binary:
```powershell
npx tauri build
# Output: src-tauri/target/release/resonantos-vnext.exe
```

---

## 2. Frontend Only (no Rust backend)

Useful for UI development when you don't need backend services.

```powershell
cd src/resonantos-vnext
npm run dev
```

Opens at `http://127.0.0.1:1430`. Tauri API calls will fail (no backend), but you can develop UI components, dashboard layouts, wizard flows.

---

## 3. Rust Backend Tests

The test suite compiles clean (0 errors). Running requires WebView2Loader.dll
compatibility with the GNU toolchain. Two approaches:

### Option A: Run via MSVC toolchain (if available)

If you have VS Build Tools installed on another machine or get admin access:
```powershell
rustup override set stable-x86_64-pc-windows-msvc
cargo test --lib
```

### Option B: Verify tests compile (current setup)

The GNU toolchain builds tests fine but the test binary can't load Tauri's
WebView2Loader.dll (MSVC/GNU ABI mismatch). To verify test logic compiles:

```powershell
cd src/resonantos-vnext/src-tauri

# Verify all tests compile (catches type errors, logic bugs)
cargo test --lib --no-run

# Once MSVC toolchain is available, run all tests:
cargo test mesh::              # Mesh network optimizer
cargo test network::           # Local network optimizer + simulator
cargo test transport::         # Unified mesh transport
cargo test inference::         # Split inference protocol
cargo test agents::            # Distributed agent execution (Phase 15)
cargo test companion::         # Phone companion app (Phase 16)
cargo test integration::       # RL-optimizer integration
cargo test schema_migration    # Schema versioning

# Run with output visible
cargo test -- --nocapture

# Run a single test by name
cargo test test_solver_pareto_improvement
```

### Phase 15: Distributed Agent Execution Tests

```powershell
cd src/resonantos-vnext/src-tauri

# All agent module tests (unit + property-based)
cargo test agents::

# Individual submodule tests
cargo test agents::dag          # DAG construction & validation
cargo test agents::router       # Step-to-node routing
cargo test agents::executor     # Parallel execution engine
cargo test agents::orchestrator # Workflow lifecycle
cargo test agents::worker       # Step worker execution
cargo test agents::cache        # Intermediate result caching
cargo test agents::checkpoint   # Checkpoint save/restore
cargo test agents::colocation   # Co-location scoring
cargo test agents::protocol     # Wire protocol
cargo test agents::tools        # Tool registry
cargo test agents::integration  # End-to-end integration
```

10 property-based tests covering: DAG acyclicity, step routing correctness,
parallel execution determinism, checkpoint idempotency, cache eviction bounds,
co-location scoring monotonicity, protocol serialization roundtrip, tool
capability matching, timeout enforcement, and retry convergence.

### Phase 16: Phone Companion App Tests

```powershell
cd src/resonantos-vnext/src-tauri

# All companion module tests (unit + property-based)
cargo test companion::

# Individual submodule tests
cargo test companion::identity          # Mesh identity generation
cargo test companion::health            # Health reporting
cargo test companion::inference_runtime # Inference runtime
cargo test companion::layer_worker      # Layer execution
cargo test companion::assignment        # Assignment logic
cargo test companion::lifecycle         # App lifecycle transitions
cargo test companion::npu              # NPU detection
cargo test companion::pairing          # Pairing protocol
cargo test companion::transport_bridge # Transport bridge
cargo test companion::property_tests   # All 15 property tests
```

15 property-based tests covering: identity uniqueness, health state transitions,
layer assignment validity, lifecycle state machine correctness, NPU capability
detection, pairing token expiry, transport bridge message integrity, battery
threshold enforcement, thermal throttling, assignment rejection on overload,
inference runtime memory bounds, and more.

### Unified Resource Scheduler Tests

```powershell
cd src/resonantos-vnext/src-tauri

# Solver agent selection + placement tests
cargo test network::solver_agents

# Contention computation tests
cargo test network::solver_contention
```

19 property-based tests covering: agent selection respects RAM budget, placement
satisfies tool requirements, contention cost is non-negative, unified objective
monotonicity, device-agnostic scheduling (no device-type branching), download
priority ordering, speed-weighted routing, proportional load assignment,
contention penalty weight sensitivity, and Pareto improvement preservation.

---

## 4. Frontend Tests (Vitest + fast-check)

```powershell
cd src/resonantos-vnext

# Run all frontend tests
npx vitest --run

# Watch mode (re-runs on file change)
npx vitest

# Run specific test file
npx vitest --run src/components/dashboard/
npx vitest --run src/components/wizard/
```

---

## 5. Network Simulator (standalone)

The network simulator can be exercised through Rust tests without the full app:

```powershell
cd src/resonantos-vnext/src-tauri

# Run simulator scenarios
cargo test network::simulator

# Run specific preset scenarios
cargo test simulator::presets
```

The simulator models virtual nodes, network failures, latency, and exercises the solver/optimizer logic in isolation.

---

## 6. RL Policy Training (Python)

Train the reinforcement learning model that optimizes network placement decisions.

**Prerequisites:**
```powershell
pip install torch numpy onnx
```

**Run training:**
```python
cd src/resonantos-vnext/training/unified_rl_policy

python -c "
from training_job import TrainingJob, TrainingJobConfig
config = TrainingJobConfig(
    experience_db_path='path/to/experience.db',
    output_dir='./output',
    episodes_required=100,
)
job = TrainingJob(config)
result = job.run()
print(f'Training complete: {result.model_version}')
"
```

**Run training tests:**
```powershell
cd src/resonantos-vnext/training/unified_rl_policy
python -m pytest tests/
```

The training pipeline: loads experience data → encodes states → computes rewards → trains DQN → exports ONNX model for Rust inference.

---

## 7. Reticulum Channel Sidecar (Python)

The mesh networking transport layer using Reticulum (LoRa/packet radio/TCP).

**Prerequisites:**
```powershell
pip install rns lxmf
```

**Run sidecar:**
```powershell
cd src/resonantos-vnext/addons/reticulum-channel/sidecar
python main.py
```

Communicates with the host app via stdio JSON-RPC. In production, the Tauri app spawns this as a child process.

**Run tests:**
```powershell
python -m pytest test_sidecar.py
```

---

## 8. Browser Host Addon (Node.js)

Controlled Chromium automation for AI-audited browser actions.

**Prerequisites:**
```powershell
cd src/resonantos-vnext/addons/resonant-browser-host
npm install
npx playwright install chromium
```

**Run headless browser host:**
```powershell
npm start
```

**Run with visible Electron window:**
```powershell
npm run start:electron
```

**Run tests:**
```powershell
npm test
```

---

## 9. Living Archive Memory Service (Node.js)

Standalone memory/knowledge service for the Living Archive addon.

```powershell
cd src/resonantos-vnext
npm run memory-service
```

**Run tests:**
```powershell
npm run test:living-archive-memory-service
npm run test:living-archive-mcp
```

---

## 10. Schema Migrations

The schema versioning system manages database migrations across updates.

Migrations run automatically on app startup. To test migration logic:

```powershell
cd src/resonantos-vnext/src-tauri
cargo test schema_migration
```

---

## 10.5. Node Persistence Layer

SQLite-backed durable storage for all node state (nodes, checkpoints, placements, settings, workflows).

```powershell
cd src/resonantos-vnext/src-tauri

# All persistence tests (unit + property-based + integration)
cargo test persistence::

# Property-based tests only (12 properties, 100+ iterations each)
cargo test persistence::property_tests

# Integration tests (concurrent access, lifecycle, error handling)
cargo test persistence::integration_tests

# Individual store tests
cargo test persistence::node_store
cargo test persistence::checkpoint_store
cargo test persistence::placement_store
cargo test persistence::settings_store
cargo test persistence::workflow_store
cargo test persistence::cleanup
cargo test persistence::migrations
```

12 property-based tests covering: node state round-trip, stale node cleanup,
unexpired checkpoint filtering, expired checkpoint cleanup, single active plan
invariant, plan retention bounds, settings round-trip, settings cache coherence,
workflow state round-trip, running workflow filtering, stale workflow timeout,
and JSON validation.

---

## 11. Hardware Detection (runs inside app)

Hardware profiling (GPU detection, VRAM management, thermal monitoring) runs automatically when the app starts. To test in isolation:

```powershell
cd src/resonantos-vnext/src-tauri
cargo test hardware
```

---

## 12. Backtest Suite

Run the engineer backtest mode to validate model routing decisions against historical data:

```powershell
cd src/resonantos-vnext/src-tauri
cargo test backtest
```

In the running app, backtest is triggered via the Tauri command `backtest_execute_suite`.

---

## Component Dependency Map

```
┌─────────────────────────────────────────────────────────┐
│                    Tauri Desktop App                      │
│  (npx tauri dev)                                         │
├─────────────────────────────────────────────────────────┤
│  Frontend (React/TS)          │  Backend (Rust)          │
│  - Dashboard UI               │  - Network optimizer     │
│  - Wizard flows                │  - Mesh coordinator      │
│  - Companion dashboard         │  - Transport layer       │
│  - Debug panels                │  - Split inference       │
│                                │  - Distributed agents    │
│                                │  - Phone companion       │
│                                │  - Unified scheduler     │
│                                │  - RL integration        │
│                                │  - Hardware services     │
├────────────────────────────────┼──────────────────────────┤
│  Addons (spawned by backend)                             │
│  - Browser Host (Node.js/Playwright)                     │
│  - Reticulum Sidecar (Python/RNS)                        │
│  - Living Archive Memory (Node.js)                       │
├─────────────────────────────────────────────────────────┤
│  Training (offline, produces ONNX models)                │
│  - RL Policy Training (Python/PyTorch)                   │
└─────────────────────────────────────────────────────────┘
```

---

## Ports & Paths

| Service | Port/Path | Notes |
|---------|-----------|-------|
| Vite dev server | `127.0.0.1:1430` | Frontend hot-reload |
| Tauri window | Native | Webview renders from Vite |
| Reticulum sidecar | stdio JSON-RPC | Spawned by backend |
| Browser host | stdio JSON-RPC | Spawned by backend |
| SQLite databases | `$APPDATA/resonantos-vnext/` | Auto-created on first run |

---

## Common Workflows

### "I want to work on the dashboard UI"
```powershell
npm run dev   # Frontend only, fast iteration
```

### "I want to test mesh networking logic"
```powershell
cargo test mesh:: -- --nocapture
```

### "I want to run the full app end-to-end"
```powershell
npx tauri dev
```

### "I want to train a new RL model"
```powershell
cd training/unified_rl_policy
python -c "from training_job import ..."
```

### "I want to test Reticulum mesh transport"
```powershell
cd addons/reticulum-channel/sidecar
python main.py
```

### "I want to test distributed agent execution"
```powershell
cd src-tauri
cargo test agents:: -- --nocapture
```

### "I want to test the phone companion module"
```powershell
cd src-tauri
cargo test companion:: -- --nocapture
```

### "I want to test the unified resource scheduler"
```powershell
cd src-tauri
cargo test network::solver_agents -- --nocapture
cargo test network::solver_contention -- --nocapture
```

### "I want to test the RL policy inference"
```powershell
cd src-tauri
cargo test integration::rl_ -- --nocapture
```

### "I want to test the WireGuard transport adapter"
```powershell
cd src-tauri
cargo test transport::adapters::wireguard -- --nocapture
```

### "I want to test the local inference engine"
```powershell
cd src-tauri
cargo test inference::local -- --nocapture
```

### "I want to run end-to-end integration tests"
```powershell
cd src-tauri
cargo test integration_tests:: -- --nocapture
```

### "I want to test the model catalog"
```powershell
cd src-tauri
cargo test network::catalog_store -- --nocapture
```

### "I want to test the adaptive segment scheduler (CollaPipe)"
```powershell
cd src-tauri
cargo test inference::split::segment -- --nocapture
cargo test inference::split::virtual_queue -- --nocapture
```

### "I want to test the MARL decentralized policies"
```powershell
cd src-tauri
cargo test integration::marl_ -- --nocapture
```

### "I want to test the hardware abstraction layer"
```powershell
cd src-tauri
cargo test backends:: -- --nocapture

# Individual backends
cargo test backends::llamacpp -- --nocapture
cargo test backends::ollama -- --nocapture
cargo test backends::tenstorrent -- --nocapture
cargo test backends::ascend -- --nocapture
cargo test backends::sidecar -- --nocapture
cargo test backends::registry -- --nocapture
cargo test backends::preparation -- --nocapture
```

### "I want to test the transport QoS layer"
```powershell
cd src-tauri
cargo test transport::qos -- --nocapture
```

### "I want to build and run the headless node daemon"
```powershell
cd src-tauri

# Build the daemon binary
cargo build --release --bin resonantos-node

# Run it (joins mesh on LAN)
./target/release/resonantos-node --join

# Run with manual peer
./target/release/resonantos-node --peer 192.168.1.10:9741

# Run in low-power mode (phones/laptops)
./target/release/resonantos-node --low-power

# Query running daemon status
./target/release/resonantos-node --status

# Test daemon module
cargo test daemon:: -- --nocapture
```

### "I want to run all property-based tests"
```powershell
cd src-tauri
cargo test property    # Matches all test modules with 'property' in the name
cargo test proptest    # Alternative: matches proptest-based tests
```

---

## CI: Type Check Workflow

A GitHub Actions workflow runs on every push and PR to `main`. It performs two parallel checks:

- **Rust**: `cargo check --lib` (catches type errors without full build)
- **TypeScript**: `npx tsc --noEmit` (catches type errors without emitting JS)

Workflow file: `.github/workflows/typecheck.yml`

### Run the same checks locally

```powershell
# Rust type check (from src-tauri/)
cd src/resonantos-vnext/src-tauri
cargo check --lib

# TypeScript type check (from project root)
cd src/resonantos-vnext
npx tsc --noEmit
```

Both should pass with zero errors. The CI workflow targets <2 minutes with warm caches.
