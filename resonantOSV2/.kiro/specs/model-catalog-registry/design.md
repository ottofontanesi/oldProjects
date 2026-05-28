# Design Document: Model Catalog Registry

## Overview

Provides a populated model catalog with ~50 real model entries (Qwen, Llama, DeepSeek, Phi, Mistral, etc.), supports user-added models, integrates with Ollama for local model discovery, and persists the catalog to disk. The optimizer queries this catalog to know what models exist, their sizes, performance characteristics, and download URLs.

### Design Principles

1. **Bundled defaults**: Ship with a comprehensive catalog so the app works out of the box.
2. **Extensible**: Users can add custom models; Ollama models are auto-discovered.
3. **Versioned**: Catalog format has a version number for clean updates.
4. **Offline-first**: Catalog loads from local file; network updates happen in background.
5. **Optimizer-friendly**: Every entry has the fields the solver needs (RAM, performance, task affinity).

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      ModelCatalog                                 │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ BundledModels│  │ UserModels   │  │ OllamaDiscovery      │  │
│  │ (JSON file,  │  │ (user-added  │  │ (detect running      │  │
│  │  ~50 entries)│  │  GGUF files) │  │  Ollama, import)     │  │
│  └──────┬───────┘  └──────┬───────┘  └──────────┬───────────┘  │
│         │                  │                     │              │
│         └──────────────────┼─────────────────────┘              │
│                            ▼                                    │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Merged Catalog (in-memory)                    │   │
│  │  Vec<ModelEntry> — all models from all sources            │   │
│  │  Queryable by: model_id, family, task_type, size range    │   │
│  └──────────────────────────────────────────────────────────┘   │
│                            │                                    │
│                            ▼                                    │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Persisted Catalog (JSON file)                 │   │
│  │  $APPDATA/resonantos-vnext/catalog.json                   │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────┐
│ Network Solver  │
│ (queries catalog│
│  for model      │
│  selection)     │
└─────────────────┘
```

## Bundled Catalog Content

The bundled catalog ships as `assets/model_catalog.json` and includes:

### Model Families

| Family | Sizes | Quantizations | Primary Task |
|--------|-------|---------------|-------------|
| Qwen 2.5 | 0.5B, 1.5B, 3B, 7B, 14B, 32B, 72B | Q4_K_M, Q5_K_M, Q8_0 | Chat, Coding |
| Llama 3.1 | 1B, 3B, 8B, 70B | Q4_K_M, Q5_K_M, Q8_0 | Chat, Reasoning |
| DeepSeek V2 | 1.3B, 6.7B, 16B, 33B | Q4_K_M, Q5_K_M | Coding |
| Phi 3.5 | 1.5B, 3.8B | Q4_K_M, Q8_0 | Chat, Reasoning |
| Mistral | 7B | Q4_K_M, Q5_K_M, Q8_0 | Chat |
| Mixtral | 8x7B | Q4_K_M | Chat, Coding |
| CodeLlama | 7B, 13B, 34B | Q4_K_M, Q5_K_M | Coding |
| Gemma 2 | 2B, 9B, 27B | Q4_K_M, Q8_0 | Chat |
| Whisper | tiny, base, small, medium | F16 | Speech-to-Text |

### Entry Structure

```rust
// Each entry in the catalog:
{
    "model_id": "qwen2.5:7b-q4_k_m",
    "family": "qwen2.5",
    "parameter_count_b": 7.0,
    "quantization": "Q4_K_M",
    "requirements": {
        "min_ram_mb": 5200,
        "min_vram_mb": 0,
        "disk_size_mb": 4400
    },
    "performance": {
        "estimates": [
            { "hardware_class": "CpuOnly", "estimated_tok_s": 8.0, "estimated_prefill_tok_s": 25.0 },
            { "hardware_class": "CpuWithAvx2", "estimated_tok_s": 15.0, "estimated_prefill_tok_s": 45.0 },
            { "hardware_class": "GpuNvidia", "estimated_tok_s": 45.0, "estimated_prefill_tok_s": 120.0 },
            { "hardware_class": "GpuAppleMetal", "estimated_tok_s": 35.0, "estimated_prefill_tok_s": 90.0 }
        ]
    },
    "task_affinity": {
        "Chat": 0.85,
        "Coding": 0.90,
        "Reasoning": 0.75,
        "Creative": 0.70,
        "Translation": 0.60
    },
    "download_sources": [
        {
            "url": "https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF/resolve/main/qwen2.5-7b-instruct-q4_k_m.gguf",
            "size_mb": 4400,
            "checksum_sha256": "abc123..."
        }
    ],
    "supported_backends": ["Ollama", "LlamaCpp"],
    "context_length": 32768,
    "license": "Apache-2.0"
}
```

## Ollama Integration

```rust
pub struct OllamaDiscovery {
    endpoint: String,  // Default: http://localhost:11434
}

impl OllamaDiscovery {
    pub async fn discover() -> Result<Vec<OllamaModel>, CatalogError> {
        // GET http://localhost:11434/api/tags
        // Parse response into model list
        // Map Ollama model names to catalog entries
    }

    pub fn map_to_catalog_entry(ollama_model: &OllamaModel) -> Option<ModelEntry> {
        // Match by name pattern: "qwen2.5:7b" → find in bundled catalog
        // If found: mark as "locally available" (no download needed)
        // If not found: create a basic entry from Ollama metadata
    }
}
```

## Catalog Persistence

```rust
pub struct CatalogStore {
    catalog_path: PathBuf,  // $APPDATA/resonantos-vnext/catalog.json
}

impl CatalogStore {
    pub fn load(&self) -> Result<PersistedCatalog, CatalogError>;
    pub fn save(&self, catalog: &PersistedCatalog) -> Result<(), CatalogError>;
}

pub struct PersistedCatalog {
    pub version: u32,
    pub bundled_entries: Vec<ModelEntry>,
    pub user_entries: Vec<ModelEntry>,
    pub ollama_entries: Vec<ModelEntry>,
    pub calibrated_performance: HashMap<ModelId, CalibratedPerformance>,
    pub last_updated_ms: u64,
}
```

## Catalog Update Flow

```
On startup:
    1. Load persisted catalog from disk (fast, no network)
    2. Merge with bundled catalog (in case app was updated)
    3. Check Ollama (if running) for local models
    4. Background: check for catalog updates from remote manifest

Background update:
    1. Fetch version manifest from known URL
    2. If remote version > local version:
        a. Download new catalog JSON
        b. Merge with user customizations (preserve user entries, pinned quantizations)
        c. Save merged catalog to disk
    3. If fetch fails: continue with local catalog (no error to user)
```

## Correctness Properties

### Property 1: Catalog Completeness
The bundled catalog SHALL contain at least 50 model entries covering all listed families.

### Property 2: Entry Validity
Every catalog entry SHALL have: model_id, family, parameter_count_b > 0, at least one quantization, at least one download source with valid URL.

### Property 3: User Preservation
Catalog updates SHALL never remove or modify user-added entries.

### Property 4: Offline Availability
The catalog SHALL be fully functional without network access (loaded from local file).

## File Structure

```
src/resonantos-vnext/
├── assets/
│   └── model_catalog.json      # Bundled default catalog (~50 entries)
├── src-tauri/src/network/
│   ├── catalog.rs              # [EXISTING] ModelEntry types
│   ├── catalog_store.rs        # [NEW] Persistence, loading, merging
│   └── catalog_ollama.rs       # [NEW] Ollama discovery integration
```
