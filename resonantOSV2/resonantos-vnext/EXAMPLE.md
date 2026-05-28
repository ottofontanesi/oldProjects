# ResonantOS vNext — Real-World Test Scenarios

Step-by-step guides to validate the system end-to-end on real hardware.
Each scenario builds on the previous one.

---

## Prerequisites (All Platforms)

### Hardware Requirements

| Scenario | Minimum | Recommended |
|----------|---------|-------------|
| Single PC | 8GB RAM, any CPU | 16GB RAM, GPU with 6GB+ VRAM |
| Local Network | 2 machines on same LAN | 3 machines (desktop+laptop+phone) |
| Mesh Network | 2 machines on different networks | 3+ machines with VPN or internet |

### Software Requirements

- Git
- Rust 1.78+ (via rustup)
- Node.js 20+ (LTS)
- A GGUF model file (see below)

### Download a Test Model

Pick one based on your RAM:

| RAM | Model | Size | Download |
|-----|-------|------|----------|
| 8GB | Qwen 2.5 0.5B Q4_K_M | 394MB | `wget https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf` |
| 16GB | Qwen 2.5 3B Q4_K_M | 1.9GB | `wget https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf` |
| 32GB+ | Qwen 2.5 7B Q4_K_M | 4.4GB | `wget https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF/resolve/main/qwen2.5-7b-instruct-q4_k_m.gguf` |

Place the downloaded file in `~/.resonantos/models/`.

---

## Platform Setup

### Windows (MSVC — recommended for running)

```powershell
# Install Rust
winget install Rustlang.Rustup

# Install Node.js
winget install OpenJS.NodeJS.LTS

# Install Visual Studio Build Tools (C++ workload)
winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools"

# Clone
git clone <repo-url> resonantos
cd resonantos/src/resonantos-vnext

# Install frontend dependencies
npm install

# Verify Rust compiles
cd src-tauri
cargo check --lib
cd ..

# Run the app
npx tauri dev
```

### Windows (GNU — compile-only, no runtime)

```powershell
# Set environment (portable MinGW)
$env:PATH = "C:\path\to\mingw64\bin;C:\Users\you\.cargo\bin;C:\path\to\node;$env:PATH"
$env:CARGO_HTTP_CHECK_REVOKE = "false"

# Verify compilation
cd src/resonantos-vnext/src-tauri
cargo test --lib --no-run

# Note: The binary won't run (WebView2 ABI mismatch with GNU toolchain)
# Use MSVC toolchain for actual execution
```

### macOS

```bash
# Install prerequisites
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install node

# Clone and build
git clone <repo-url> resonantos
cd resonantos/src/resonantos-vnext
npm install

# Run tests (Apple Silicon — native, no Rosetta needed)
cd src-tauri
cargo test --lib
cd ..

# Run the app
npx tauri dev
```

### Linux (Ubuntu/Debian)

```bash
# Install system dependencies
sudo apt update
sudo apt install -y build-essential curl wget \
  libssl-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libwebkit2gtk-4.1-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install Node.js
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt install -y nodejs

# Clone and build
git clone <repo-url> resonantos
cd resonantos/src/resonantos-vnext
npm install

# Run tests
cd src-tauri
cargo test --lib
cd ..

# Run the app
npx tauri dev
```

---

## Scenario 1: Single PC — Local Inference

**Goal:** Verify the app starts, detects hardware, loads a model, and generates tokens locally.

**Time:** ~10 minutes

### Steps

1. **Start the app:**
   ```bash
   cd src/resonantos-vnext
   npx tauri dev
   ```

2. **Complete the onboarding wizard:**
   - The wizard should detect your hardware (CPU, RAM, GPU)
   - It should recommend a model based on your specs
   - Accept the recommendation or select a model manually

3. **Download a model (if not pre-downloaded):**
   - The wizard will show download progress
   - Wait for completion (progress bar, speed, ETA visible)

4. **Test inference:**
   - After download, the wizard runs a test prompt: "Hello, I'm your local AI assistant"
   - You should see tokens streaming in real-time
   - Note the tokens/second displayed

5. **Verify the dashboard:**
   - After wizard completion, the Network Dashboard should show:
     - 1 node (your machine) with green status
     - CPU/RAM/VRAM utilization gauges
     - 1 model loaded
     - Utility score > 0

### Expected Results

| Check | Expected |
|-------|----------|
| Hardware detected | CPU name, RAM total, GPU (if present) |
| Model loads | No errors, status shows "loaded" |
| Inference works | Tokens stream, >5 tok/s on any modern CPU |
| Dashboard shows data | Node online, model listed, utility > 0 |
| No crashes | App remains responsive throughout |

### Troubleshooting

| Issue | Fix |
|-------|-----|
| "Model not found" | Check `~/.resonantos/models/` contains the .gguf file |
| Slow inference (<2 tok/s) | Model too large for your RAM — try smaller quantization |
| GPU not detected | Ensure CUDA drivers (NVIDIA) or Metal (macOS) are working |
| App won't start | Check `src-tauri/target/debug/` for crash logs |

---

## Scenario 2: Local Network — Two Nodes Collaborating

**Goal:** Two machines on the same LAN discover each other, form a network, and split a model across both.

**Time:** ~20 minutes

### Prerequisites

- Two machines on the same WiFi/LAN (can ping each other)
- Both have completed Scenario 1 (app installed, working)
- A model that's too large for one machine alone (e.g., 7B model with only 8GB RAM per machine)

### Steps

1. **Start the app on Machine A (desktop):**
   ```bash
   npx tauri dev
   ```
   - Complete wizard if first run
   - Note the node ID shown in the dashboard

2. **Start the app on Machine B (laptop):**
   ```bash
   npx tauri dev
   ```
   - Complete wizard if first run

3. **Verify mDNS discovery:**
   - Within 10 seconds, both dashboards should show 2 nodes
   - Machine A sees Machine B (and vice versa)
   - Transport Health panel shows "LAN/mDNS" adapter with 1 peer

4. **Trigger optimizer cycle:**
   - Wait 60 seconds (one optimizer cycle)
   - The placement plan should update to use both machines
   - Models may be redistributed across both nodes

5. **Test split inference (if model is large enough):**
   - Send a chat message that requires the large model
   - The system should split the model across both machines
   - Watch the Transport Health panel for activation traffic between nodes

6. **Test failover:**
   - Kill the app on Machine B (Ctrl+C)
   - Machine A should detect the loss within 10 seconds
   - Dashboard should show 1 node, model reassigned to Machine A only
   - Inference should still work (degraded but functional)

### Expected Results

| Check | Expected |
|-------|----------|
| Discovery | Both nodes visible within 10s |
| Optimizer | Plan uses both nodes' resources |
| Split inference | Tokens stream (possibly slower due to network) |
| Failover | Graceful degradation, no crash |
| Reconnection | If Machine B restarts, it rejoins within 10s |

### Network Debugging

```bash
# Verify mDNS works between machines
# On Machine A:
avahi-browse -a  # Linux
dns-sd -B _resonantos._tcp  # macOS

# Verify TCP connectivity (port 9741)
nc -zv <machine-b-ip> 9741

# Check firewall
sudo ufw allow 9741/tcp  # Linux
# Windows: allow through Windows Firewall
```

---

## Scenario 3: Phone Companion — Desktop + Phone Split Inference

**Goal:** Pair a phone with the desktop and use the phone's NPU for part of the inference workload.

**Time:** ~15 minutes

### Prerequisites

- Desktop running ResonantOS (Scenario 1 complete)
- iOS or Android phone on the same WiFi
- Phone has the ResonantOS Companion app installed (Tauri Mobile build)

### Steps

1. **Start desktop app:**
   ```bash
   npx tauri dev
   ```

2. **Open Companion settings on desktop:**
   - Navigate to Settings → Companion
   - Click "Pair New Phone"
   - A QR code appears (valid for 5 minutes)

3. **Scan QR code on phone:**
   - Open ResonantOS Companion app
   - Tap "Pair with Desktop"
   - Scan the QR code
   - Confirm pairing on both devices

4. **Verify pairing:**
   - Desktop dashboard shows the phone as a node
   - Phone shows "Connected to [desktop-name]"
   - Phone reports: battery %, NPU type, available RAM

5. **Test phone-assisted inference:**
   - Send a chat message on desktop
   - If the model is split, the phone should receive layer activations
   - Phone status shows "Processing layers 28-32" (or similar)
   - Result returns to desktop and displays

6. **Test battery protection:**
   - Drain phone battery below 20% (or simulate in settings)
   - The optimizer should remove the phone from the placement plan
   - Desktop handles inference alone (graceful degradation)

### Expected Results

| Check | Expected |
|-------|----------|
| QR pairing | Completes in <10 seconds |
| Phone appears in dashboard | Shows battery, NPU, RAM |
| Split inference uses phone | Phone processes assigned layers |
| Battery protection | Phone excluded when battery < 20% |
| Phone disconnect | Desktop continues alone, no crash |

---

## Scenario 4: Mesh Network — Cross-Network Collaboration via WireGuard

**Goal:** Two machines on different networks (different cities/ISPs) collaborate via encrypted WireGuard tunnel.

**Time:** ~30 minutes

### Prerequisites

- Two machines on different networks (not same LAN)
- WireGuard installed on both (`wg-quick`, or the WireGuard app)
- A WireGuard config connecting both machines (peer-to-peer)
- Both have completed Scenario 1

### Steps

1. **Set up WireGuard tunnel:**

   Machine A (`/etc/wireguard/wg0.conf`):
   ```ini
   [Interface]
   PrivateKey = <machine-a-private-key>
   Address = 10.0.0.1/24
   ListenPort = 51820

   [Peer]
   PublicKey = <machine-b-public-key>
   Endpoint = <machine-b-public-ip>:51820
   AllowedIPs = 10.0.0.2/32
   PersistentKeepalive = 25
   ```

   Machine B (`/etc/wireguard/wg0.conf`):
   ```ini
   [Interface]
   PrivateKey = <machine-b-private-key>
   Address = 10.0.0.2/24
   ListenPort = 51820

   [Peer]
   PublicKey = <machine-a-public-key>
   Endpoint = <machine-a-public-ip>:51820
   AllowedIPs = 10.0.0.1/32
   PersistentKeepalive = 25
   ```

   ```bash
   sudo wg-quick up wg0
   ping 10.0.0.2  # Verify tunnel works
   ```

2. **Start ResonantOS on both machines:**
   ```bash
   npx tauri dev
   ```

3. **Configure WireGuard adapter in ResonantOS:**
   - Settings → Transport → Add WireGuard Peer
   - Enter peer's WireGuard IP (10.0.0.2) and ResonantOS port (9741)
   - Or: the app auto-discovers peers on the WireGuard subnet

4. **Verify mesh connection:**
   - Both dashboards show 2 nodes
   - Transport Health shows "WireGuard" adapter with 1 peer
   - Latency displayed (typically 20-100ms for cross-internet)

5. **Test cross-network inference:**
   - Load a large model that benefits from both machines' RAM
   - Send a chat message
   - Observe split inference traffic flowing over WireGuard
   - Latency will be higher than LAN but still functional

6. **Test mesh resilience:**
   - Temporarily disconnect WireGuard (`sudo wg-quick down wg0`)
   - ResonantOS should detect the loss within 60 seconds (suspect) → 120 seconds (offline)
   - Reconnect: `sudo wg-quick up wg0`
   - Peer should rejoin automatically (re-handshake)

### Expected Results

| Check | Expected |
|-------|----------|
| WireGuard tunnel | Ping works between 10.0.0.x addresses |
| Peer discovery | Both nodes visible in dashboard |
| Cross-network inference | Works (higher latency than LAN) |
| Disconnect handling | Graceful degradation, auto-reconnect |
| Encryption | All traffic encrypted (WireGuard native) |

### Performance Expectations

| Metric | LAN (Scenario 2) | WireGuard (Scenario 4) |
|--------|-------------------|------------------------|
| Discovery time | <10s (mDNS) | <30s (manual/probe) |
| Latency | 1-5ms | 20-200ms |
| Bandwidth | 100-1000 Mbps | 10-100 Mbps |
| Split inference overhead | Negligible | Noticeable (activation transfer) |
| Recommended for | Large models, real-time chat | Background tasks, batch processing |

---

## Scenario 5: Stress Test — Optimizer Under Load

**Goal:** Verify the optimizer handles realistic workloads without degradation.

**Time:** ~15 minutes (automated)

### Steps

1. **Run the built-in stress test (from Rust tests):**
   ```bash
   cd src-tauri

   # Optimizer with 10 nodes, 20 models — should complete in <500ms
   cargo test integration_tests::test_errors::test_performance_optimizer -- --nocapture

   # Transport with 100 concurrent messages — should complete in <500ms
   cargo test integration_tests::test_errors::test_performance_transport -- --nocapture

   # Full property test suite (all 75+ properties)
   cargo test --lib -- --nocapture 2>&1 | grep "test result"
   ```

2. **Manual stress test (with running app):**
   - Open the app with 3+ nodes connected
   - Send 10 chat messages rapidly (within 5 seconds)
   - Watch the dashboard for:
     - Queue depth increasing then draining
     - No request timeouts
     - Utility score remains stable
   - Kill one node mid-inference
   - Verify: remaining nodes handle the load, no lost messages

### Expected Results

| Check | Expected |
|-------|----------|
| Optimizer <500ms for 10 nodes | ✓ (property test enforces this) |
| No message loss under load | ✓ (100 messages all delivered) |
| Graceful node removal | Plan updates within 60s, no crash |
| Queue doesn't overflow | Queue depth returns to 0 after burst |

---

## Common Issues Across All Scenarios

| Issue | Platform | Fix |
|-------|----------|-----|
| `WebView2Loader.dll` error | Windows (GNU) | Use MSVC toolchain for running |
| `webkit2gtk` not found | Linux | `sudo apt install libwebkit2gtk-4.1-dev` |
| Port 9741 blocked | All | Open in firewall |
| mDNS not working | Linux | `sudo apt install avahi-daemon` |
| Model download fails | All | Check disk space, try manual wget |
| GPU not detected | All | Update GPU drivers, check CUDA/Metal |
| App crashes on start | All | Delete `~/.resonantos/` and retry (fresh state) |
| Slow first build | All | Normal — full Rust link takes 2-3 min |

---

## Validation Checklist

After completing all scenarios, verify:

- [ ] Single PC inference works (Scenario 1)
- [ ] LAN discovery and split inference work (Scenario 2)
- [ ] Phone pairing and NPU offload work (Scenario 3)
- [ ] Cross-network mesh via WireGuard works (Scenario 4)
- [ ] Optimizer handles load without degradation (Scenario 5)
- [ ] Failover works in all scenarios (kill a node, verify recovery)
- [ ] Dashboard shows accurate real-time data in all scenarios
- [ ] No memory leaks after 1 hour of operation (check task manager)

---

## Reporting Issues

When reporting a bug from these scenarios, include:

1. Which scenario and step number
2. Platform (OS, architecture, GPU)
3. Model being used (name, size, quantization)
4. Network topology (how many nodes, what transport)
5. Error message or unexpected behavior
6. Contents of `~/.resonantos/logs/` (if exists)
7. Output of `cargo test --lib 2>&1 | tail -20`
