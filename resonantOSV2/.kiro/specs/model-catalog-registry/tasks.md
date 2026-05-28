# Implementation Plan: Model Catalog Registry

## Overview

Populate the model catalog with ~50 real model entries, support user-added models, integrate with Ollama for local discovery, and persist to disk. The optimizer queries this catalog for model selection.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [x] 1. Bundled catalog data
  - [x] 1.1 Create `assets/model_catalog.json` with ~50 model entries
    - Include all Qwen 2.5 variants (0.5B through 72B, Q4_K_M/Q5_K_M/Q8_0)
    - Include Llama 3.1 variants (1B, 3B, 8B, 70B)
    - Include DeepSeek V2 variants (1.3B, 6.7B, 16B, 33B)
    - Include Phi 3.5 (1.5B, 3.8B)
    - Include Mistral 7B, Mixtral 8x7B
    - Include CodeLlama (7B, 13B, 34B)
    - Include Gemma 2 (2B, 9B, 27B)
    - Include Whisper (tiny, base, small, medium)
    - Each entry: model_id, family, params, quantization, RAM requirements, performance estimates, task affinities, download URLs, checksums
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 2.1, 2.2, 3.1, 3.2, 3.3, 4.1, 4.2, 5.1, 5.2, 5.3_

- [x] 2. Catalog store
  - [x] 2.1 Create `network/catalog_store.rs` with persistence
    - `CatalogStore::load()` — load from $APPDATA JSON file
    - `CatalogStore::save()` — persist to JSON file (pretty-printed)
    - `CatalogStore::merge_bundled()` — merge bundled entries with persisted (add new, don't remove user)
    - Load bundled catalog from embedded asset on first run
    - _Requirements: 9.1, 9.2, 9.3, 9.4_

  - [x] 2.2 Implement catalog versioning
    - Version field in persisted catalog
    - Merge logic preserves user customizations during updates
    - _Requirements: 1.5, 8.3_

- [x] 3. Ollama integration
  - [x] 3.1 Create `network/catalog_ollama.rs` with Ollama discovery
    - Check `http://localhost:11434/api/tags` for running Ollama
    - Parse response, map model names to catalog entries
    - Mark discovered models as "locally available"
    - Handle Ollama not running gracefully (no error)
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

- [x] 4. User-added models
  - [x] 4.1 Implement user model addition
    - Accept local GGUF file path or download URL
    - Auto-detect metadata from GGUF header (params, context size, quantization)
    - Persist user models across restarts
    - Support removal of user-added models
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5_

- [x] 5. Background updates
  - [x] 5.1 Implement catalog update check
    - On startup (background): fetch version manifest from known URL
    - If newer version available: download, merge, save
    - Preserve user customizations during merge
    - Non-blocking (app starts immediately)
    - Configurable (user can disable)
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5_

- [x] 6. Integration with solver
  - [x] 6.1 Wire catalog into SolverInputs
    - On optimizer cycle: read current catalog as `model_catalog` field
    - Include performance estimates for hardware-aware placement
    - Include task affinities for demand-driven selection
    - _Requirements: 3.1, 3.2, 3.3, 4.1, 4.2_

- [x] 7. Final checkpoint
  - Verify compilation and catalog loads correctly.

## Notes

- The bundled catalog JSON is ~200KB (50 entries × ~4KB each)
- Download URLs point to HuggingFace CDN (reliable, fast)
- Performance estimates are conservative (real performance may be better)
- Ollama integration is read-only (never modifies Ollama's state)
- The catalog is the source of truth for what models the optimizer can consider
