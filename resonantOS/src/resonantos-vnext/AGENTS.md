# ResonantOS vNext Agent Instructions

## Git Workflow

- Active development happens on `dev`.
- Codex must commit to `dev` by default.
- `main` is the stable preview/release branch.
- Do not commit directly to `main` unless the user explicitly instructs it.
- Merge or PR `dev` into `main` only after deterministic validation.
- Before committing, confirm the current branch with `git status --short --branch`.

## Validation

- Run deterministic checks before calling implementation work done.
- For TypeScript/UI changes, run `npm test -- --run` and `npm run build`.
- For Rust/Tauri host changes, run `cargo fmt --check && cargo test` from `src-tauri`.
- For alpha packaging changes, run `npm run tauri:build`.
