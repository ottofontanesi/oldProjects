# Resonant Browser Native Host

Intent citation: `docs/architecture/ADR-025-native-embedded-browser-host.md`

This add-on directory is reserved for the native embedded Chromium Browser host.

The goal is not to launch an external browser window. The host must attach a Chromium-class view inside the ResonantOS center workspace and expose a single live session shared by the human and Augmentor.

## Hard Requirements

- Embedded center-workspace rendering.
- Shared human/AI live session.
- Deterministic control tools: open, read, click, type, scroll, capture, close.
- Extension lifecycle tools: install, list, enable, disable, pin where supported.
- Phantom Wallet compatibility.
- Bitwarden compatibility.
- Human approval and audit for wallet/signing and credential-sensitive actions.
- Cross-platform packaging for macOS, Windows, and Linux.

## Candidate Engine

Initial candidate: CEF with Chrome Runtime enabled.

CEF is not accepted as final until tests prove Phantom Wallet and Bitwarden work. If CEF cannot satisfy those extensions, this add-on must move to a Chromium-source or Chrome-compatible host strategy rather than hiding the limitation behind UI.

## Current Status

The first macOS ARM64 CEF host binary now compiles locally and passes deterministic native smoke tests.

The external smoke test boots CEF Chrome Runtime, loads `https://example.com/`, observes a main-frame HTTP 200, and exits with status `0`.

The embedded smoke test loads `libResonantBrowserNativeBridgeShared.dylib`, creates a real macOS `NSWindow`/`NSView`, attaches CEF into that view, observes a main-frame HTTP 200, and exits with status `0`.

The extension-entrypoint smoke test opens `chrome://extensions` and the Chrome Web Store target. On the current test machine, `chrome://extensions` loads successfully, while the Chrome Web Store redirects to Google's consent gate before extension browsing.

The local extension smoke test loads a temporary unpacked Manifest V3 extension through Chromium's extension flags, opens `https://example.com`, and verifies that the extension content script executes by observing the page title change from the native host. This proves the CEF candidate can execute at least a basic unpacked Chrome extension.

This does not yet prove Chrome Web Store installation, Phantom Wallet, or Bitwarden. Those remain blocked until dedicated install, persistence, popup, wallet-connection, and credential-flow smokes pass.

It is still not product-ready because Phantom/Bitwarden compatibility has not been proven. The embedded CEF path currently logs that Chrome style is not supported for the child browser, so extension compatibility remains blocked until a dedicated extension smoke test passes.

The current attach smoke test intentionally blocks the external-host path on macOS. Tauri can expose the app `NSView`, but an external CEF executable cannot safely attach to that process-local view pointer. The next product implementation must move from an external executable to in-process CEF/native integration owned by the Tauri process.

This directory now contains the first CEF Chrome Runtime source scaffold:

- `native_host/CMakeLists.txt`
- `native_host/include/resonant_browser_native_bridge.h`
- `native_host/src/resonant_browser_native_bridge.cc`
- `native_host/src/resonant_browser_native_host.cc`
- `native_host/src/resonant_browser_native_host_mac.mm`
- `scripts/probe-native-host.mjs`
- `scripts/audit-browser-addon-drift.mjs`
- `test/native-cef-smoke.test.mjs`
- `test/native-cef-embed.test.mjs`
- `test/native-host-contract.test.mjs`

macOS note: the native host uses Chromium's `use-mock-keychain` and `password-store=basic` flags for deterministic smoke/probe boot. Without that guard, CEF can block during `CefInitialize` on macOS Keychain lookup, which makes the test non-deterministic. Production credential and wallet flows must still go through explicit ResonantOS capability gates.

The next accepted implementation step is validating the bridge inside the packaged ResonantOS/Tauri window, then proving Phantom/Bitwarden compatibility. This directory exists to prevent new Browser work from drifting back into Electron sidecar or Tauri WebView workarounds.

Run the deterministic source-contract check with:

```bash
npm run test:browser-native
```

Resolve the current CEF binary candidate without downloading it:

```bash
npm run browser-native:cef:plan
```

Download/extract CEF only when ready to build the native host:

```bash
node addons/resonant-browser-native/scripts/fetch-cef.mjs --download
```

Compile work requires the extracted CEF binary distribution and `CEF_ROOT`:

```bash
cmake -S addons/resonant-browser-native/native_host -B addons/resonant-browser-native/build -DCEF_ROOT=/path/to/cef_binary
cmake --build addons/resonant-browser-native/build
```

The build now creates both the external CEF probe app and the in-process bridge library:

Current local build output:

```text
addons/resonant-browser-native/build/ResonantBrowserNativeHost.app/Contents/MacOS/ResonantBrowserNativeHost
addons/resonant-browser-native/build/libResonantBrowserNativeBridge.a
addons/resonant-browser-native/build/libResonantBrowserNativeBridgeShared.dylib
```

Build and stage packaged assets with:

```bash
npm run browser-native:build
```

This command writes Tauri-ready generated artifacts into `build/native-browser/`:

```text
build/native-browser/libResonantBrowserNativeBridgeShared.dylib
build/native-browser/ResonantBrowserNativeHost.app.zip
```

The host app is intentionally zipped. Chromium's `.framework` bundle must stay structurally intact, and direct Tauri resource recursion can mis-handle framework internals. Rust unpacks the zip at runtime before initializing CEF.

Run the native smoke directly:

```bash
addons/resonant-browser-native/build/ResonantBrowserNativeHost.app/Contents/MacOS/ResonantBrowserNativeHost --resonantos-smoke --url=https://example.com
```
