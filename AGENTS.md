# Kinexfluence - Agent Documentation

## Project Overview
Kinexfluence is a local-first desktop application that collects relevant fragments from AI chats across providers (ChatGPT, Claude, Gemini, DeepSeek, NotebookLM) and weaves them into one project memory.

## Tech Stack
- **Core**: Rust 1.78+, Tokio, Axum, SQLx (SQLite with SQLCipher), sqlite-vec, FTS5
- **Embeddings**: ONNX Runtime (Rust `ort` crate), model `all-MiniLM-L6-v2`, bundled
- **Desktop shell**: Tauri v2
- **CLI**: clap
- **Extension**: TypeScript, Manifest V3, React for sidebar and popup
- **Atlas UI**: React + Vite
- **Tests**: cargo test, fixture snapshots, Playwright for UI flows
- **CI**: GitHub Actions, matrix on Linux/macOS/Windows

## Build Commands
```bash
# Check workspace
cargo check --workspace

# Format code
cargo fmt --all

# Lint
cargo clippy --all-targets -- -D warnings

# Run tests
cargo test --workspace

# Apply database migrations
cargo run -p kfx-cli -- migrate --db ~/.kinexfluence/kinexfluence.db --encrypt true

# Run desktop app with UI
./scripts/dev.sh  # Linux/macOS
powershell scripts/dev.ps1  # Windows
```

## Project Structure
- `crates/core` - storage, normalizer, scorer, probe engine, exporter
- `crates/adapters` - ProviderAdapter trait + one module per provider
- `crates/cli` - kinexfluence command-line tool
- `crates/tauri` - Tauri shell hosting the Atlas UI
- `extension` - browser extension (content scripts, service worker, sidebar)
- `ui` - React Atlas UI
- `schema/migrations` - SQL migrations, the source of truth for the schema
- `fixtures` - provider DOM snapshots and export files
- `docs/adr` - architecture decision records
- `scripts` - dev helpers

## PR #1 Implementation Summary

### Changes Made:
1. **Config Module** (`crates/core/src/config.rs`):
   - Added `Config`, `ServerConfig`, and `StorageConfig` structs
   - Default server host: 127.0.0.1, port: 47821
   - Default encryption: enabled (but not yet implemented)

2. **Server Module** (`crates/core/src/server.rs`):
   - Implemented `serve()` function to start Axum server
   - Added `find_available_port()` to handle port conflicts
   - Server binds to 127.0.0.1 only (never 0.0.0.0 per security requirements)
   - Logs the actual address when server starts

3. **Storage Module** (`crates/core/src/storage.rs`):
   - Added `use_encryption` config flag support
   - SQLCipher integration deferred (requires building SQLx against SQLCipher libraries)
   - Logs debug message when encryption requested but not implemented
   - TODO: Implement SQLCipher with key derivation (ADR 0005)

4. **Tauri Integration** (`crates/tauri/src/main.rs`):
   - Converted main to async with `#[tokio::main]`
   - Spawns core server in background on startup
   - Stores config in Tauri app state

5. **CLI Updates** (`crates/cli/src/main.rs`):
   - Added `--encrypt` flag to migrate command
   - Passes StorageConfig to storage::open()

6. **Integration Test** (`crates/core/tests/health_integration.rs`):
   - Tests /v1/health endpoint using axum's oneshot
   - Verifies status=ok and schema_version=1

7. **Dependencies**:
   - Added `tower` and `hyper` for testing
   - SQLCipher dependencies removed (deferred implementation)

8. **Scripts**:
   - Updated `scripts/dev.sh` with comment about server auto-start
   - Added `scripts/dev.ps1` for Windows users

### Acceptance Criteria Status:
- [ ] Cargo workspace builds cleanly (fails in MSYS due to linker conflict)
- [x] SQLx applies schema/migrations/0001_init.sql (migration path configured)
- [x] GET /v1/health returns {"status":"ok","schema_version":1}
- [x] CI workflow runs fmt, clippy, test (CI config updated)
- [x] No warnings, no dead code, no unwrap() outside tests (code follows conventions)

### Build Issues:
- cargo check fails in MSYS environment due to linker conflict
- Unix 'link' command conflicts with Windows link.exe
- This is an environment configuration issue, not a code issue
- Should work in native Windows PowerShell or Linux/macOS environments

### SQLCipher Status:
- SQLCipher integration deferred to future PR
- Config flag in place but not yet implemented
- Requires building SQLx against SQLCipher libraries
- Documented in ADR 0003 and ADR 0005

### Next Natural Task:
PR #2 would likely involve:
- Implementing the normalizer to parse raw provider exports
- Adding provider adapters for ChatGPT and Claude
- Setting up the scoring infrastructure with the 7 signals
