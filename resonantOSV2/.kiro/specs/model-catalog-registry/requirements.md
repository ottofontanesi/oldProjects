# Requirements Document: Model Catalog Registry

## Introduction

This document specifies the requirements for populating the model catalog with real model entries (Qwen, Llama, DeepSeek, Phi, Mistral, etc.) and providing a registry that the optimizer can query. The existing `network/catalog.rs` defines the `ModelEntry` type but the catalog is empty at runtime. This feature provides a bundled default catalog, supports user-added models, and integrates with Ollama/HuggingFace for model discovery.

## Glossary

- **ModelCatalog**: The in-memory collection of all known models with their metadata (size, quantization, performance estimates, download URLs).
- **BundledCatalog**: A JSON file shipped with the app containing ~50 popular models pre-configured.
- **UserModel**: A model added by the user (custom GGUF file or custom registry URL).
- **OllamaIntegration**: Discovery of models already available via a local Ollama installation.
- **ModelMetadata**: The full description of a model: family, parameters, quantization, RAM requirements, performance estimates, task affinities, download sources.

## Requirements

### Requirement 1: Bundled Default Catalog

**User Story:** As a new ResonantOS user, I want a pre-populated model catalog out of the box, so that the optimizer has models to work with immediately.

#### Acceptance Criteria

1. THE app SHALL ship with a bundled catalog JSON file containing at least 50 model entries.
2. THE catalog SHALL include models from: Qwen 2.5 (0.5B, 1.5B, 3B, 7B, 14B, 32B, 72B), Llama 3.x (1B, 3B, 8B, 70B), DeepSeek (1.3B, 6.7B, 33B, V2), Phi (1.5B, 3B, 3.5B), Mistral (7B, 8x7B), CodeLlama (7B, 13B, 34B), Gemma (2B, 7B), and others.
3. EACH entry SHALL include: model_id, family, parameter_count_b, quantization variants (Q4_K_M, Q5_K_M, Q8_0, F16), RAM requirements per quantization, performance estimates per hardware class, task affinities, download URLs (HuggingFace).
4. THE bundled catalog SHALL be loaded on first startup and merged with any user additions.
5. THE catalog format SHALL be versioned so future updates can merge cleanly.

### Requirement 2: Quantization Variants

**User Story:** As a ResonantOS user with limited RAM, I want multiple quantization options per model, so that I can trade quality for size.

#### Acceptance Criteria

1. EACH model family SHALL have at least 3 quantization variants: Q4_K_M (smallest), Q5_K_M (balanced), Q8_0 (high quality).
2. EACH variant SHALL specify: file_size_mb, min_ram_mb, estimated_quality_score (0-1), download_url.
3. THE optimizer SHALL select the best quantization variant that fits within available resources.
4. THE user SHALL be able to pin a specific quantization (override optimizer choice).

### Requirement 3: Performance Estimates

**User Story:** As the optimizer, I want performance estimates per model per hardware class, so that I can predict inference speed on each node.

#### Acceptance Criteria

1. EACH model entry SHALL include performance estimates for: CpuOnly, CpuWithAvx2, GpuNvidia, GpuAppleMetal, NpuApple, NpuQualcomm.
2. EACH estimate SHALL include: estimated_tok_s (tokens/second), estimated_prefill_tok_s (prompt processing speed).
3. THE estimates SHALL be conservative (real performance may be better, never worse).
4. THE estimates SHALL be updatable based on actual measured performance (calibration).

### Requirement 4: Task Affinities

**User Story:** As the optimizer, I want to know which models are good at which tasks, so that I can match models to workload demand.

#### Acceptance Criteria

1. EACH model entry SHALL include task affinity scores (0.0-1.0) for: Chat, Coding, Reasoning, Creative, Translation, Summarization, ImageDescription.
2. THE optimizer SHALL prefer models with high affinity for the dominant task type in current demand.
3. TASK affinities SHALL be sourced from benchmark data (MMLU, HumanEval, etc.) normalized to [0,1].

### Requirement 5: Download Sources

**User Story:** As a ResonantOS node, I want reliable download URLs for each model, so that the download engine can fetch them.

#### Acceptance Criteria

1. EACH model variant SHALL have at least one download source URL (HuggingFace CDN preferred).
2. EACH download source SHALL include: url, file_size_mb, checksum_sha256.
3. THE catalog SHALL support multiple mirrors per model (fallback if primary is slow/down).
4. DOWNLOAD URLs SHALL point to GGUF format files (compatible with llama.cpp).

### Requirement 6: Ollama Integration

**User Story:** As a user with Ollama installed, I want ResonantOS to discover my existing Ollama models, so that I don't need to re-download them.

#### Acceptance Criteria

1. THE catalog SHALL detect a running Ollama instance (check `http://localhost:11434/api/tags`).
2. IF Ollama is running, THE catalog SHALL import its model list as available local models.
3. IMPORTED Ollama models SHALL be usable by the optimizer (mapped to catalog entries by name/size).
4. THE integration SHALL NOT modify Ollama's state (read-only discovery).
5. IF Ollama is not running, THE catalog SHALL proceed without it (no error).

### Requirement 7: User-Added Models

**User Story:** As a power user, I want to add custom models to the catalog, so that I can use models not in the default list.

#### Acceptance Criteria

1. THE user SHALL be able to add a model by providing: a local GGUF file path, or a download URL.
2. THE catalog SHALL auto-detect model metadata from the GGUF file header (parameter count, context size, quantization).
3. USER-ADDED models SHALL be persisted across restarts.
4. USER-ADDED models SHALL participate in optimizer placement like bundled models.
5. THE user SHALL be able to remove user-added models from the catalog.

### Requirement 8: Catalog Updates

**User Story:** As a ResonantOS user, I want the model catalog to stay current with new model releases, so that I have access to the latest models.

#### Acceptance Criteria

1. THE app SHALL check for catalog updates on startup (fetch a version manifest from a known URL).
2. IF a newer catalog version is available, THE app SHALL download and merge it with the local catalog.
3. USER customizations (pinned quantizations, user-added models) SHALL be preserved during updates.
4. THE update check SHALL be non-blocking (app starts immediately, update happens in background).
5. THE user SHALL be able to disable automatic catalog updates.

### Requirement 9: Catalog Persistence

**User Story:** As a ResonantOS node, I want the catalog persisted locally, so that it's available immediately on startup without network access.

#### Acceptance Criteria

1. THE catalog SHALL be persisted to a local JSON file in the app data directory.
2. ON startup, THE catalog SHALL load from the local file (fast, no network needed).
3. THE persisted catalog SHALL include: bundled entries + user additions + Ollama discoveries + calibrated performance data.
4. THE catalog file SHALL be human-readable (pretty-printed JSON) for debugging.
