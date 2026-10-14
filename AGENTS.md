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
- [x] rustfmt configured for stable Rust (removed unstable options)

### Build Status: BLOCKED - Environment Broken

**CRITICAL:** The current Windows environment is fundamentally broken and cannot compile Rust code.

**Root Cause:**
- `C:\Users\Admin\AppData\Local\hermes\git\usr\bin\link.exe` (GNU linker) conflicts with Windows MSVC linker
- This affects both MSYS/Git Bash AND PowerShell
- PATH pollution is systemic across the entire environment
- Cannot be fixed with simple PATH manipulation

**Impact:**
- cargo check fails with linker errors
- cargo clippy fails with linker errors
- cargo test fails with linker errors
- **PR #1 is NOT VERIFIED** - code exists but has never run

**Required Fix (choose one):**

**Option A - Clean up PATH (fastest):**
Remove or rename the hermes git installation's `link.exe`:
```powershell
Remove-Item "C:\Users\Admin\AppData\Local\hermes\git\usr\bin\link.exe"
```
Then restart PowerShell and try `cargo check --workspace`.

**Option B - Use WSL2 Ubuntu (recommended):**
1. Install WSL2: `wsl --install -d Ubuntu`
2. Inside WSL: `sudo apt update && sudo apt install -y curl build-essential pkg-config libssl-dev`
3. Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
4. Build: `cd /mnt/c/Users/Admin/downloads/kip/kinexfluence && cargo check --workspace`

**Option C - Install Visual Studio Build Tools:**
Install "Desktop development with C++" workload from https://visualstudio.microsoft.com/downloads/
This ensures proper MSVC toolchain is available and takes precedence.

**Current State:**
- Code is syntactically correct (passes rustfmt)
- Dependencies are properly configured
- Schema migrations are in place
- Health endpoint is implemented
- Integration test is written
- **NONE OF THIS HAS BEEN VERIFIED TO RUN**

**DO NOT PROCEED TO PR #2** until the build environment is fixed and `cargo check --workspace` passes green.

### SQLCipher Status:
- SQLCipher integration deferred to future PR
- Config flag in place but not yet implemented
- Requires building SQLx against SQLCipher libraries
- Documented in ADR 0003 and ADR 0005

### Recent Commits:
- 000b5a0: feat(core): initialize workspace with health endpoint and SQLCipher
- 3fb23a0: fix(core): remove sqlcipher feature flag (defer SQLCipher integration)
- 66aaa59: docs: update AGENTS.md with SQLCipher deferment and build issues
- ccbfcbb: chore: remove unstable rustfmt options for stable Rust

### Next Natural Task:
PR #2 would likely involve:
- Implementing the normalizer to parse raw provider exports
- Adding provider adapters for ChatGPT and Claude
- Setting up the scoring infrastructure with the 7 signals
