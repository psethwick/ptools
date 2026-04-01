# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                   # Build all crates
cargo build --release         # Release build
cargo check                   # Fast compilation check
cargo clippy                  # Lint
cargo test                    # Run tests
cargo test -p <crate>         # Test a single crate (e.g., -p ptime)
cargo run --bin <name>        # Run a specific binary (psync, ptime, ptasks, pterry)
```

## Architecture

This is a Rust workspace of 5 personal productivity tools that share a common data layer:

### `pstore` — shared data library
All other crates depend on this. It provides:
- SQLite persistence via sqlx (`~/.local/share/pstore/pstore.db`) with migrations in `pstore/migrations/`
- Core models: `Work`, `Person`, `Remote`, `Timesheet`, `Kind` (enum for Jira/AzureDevOps)
- System keyring integration for credential storage
- All DB access goes through `queries.rs`

### `psync` — work item sync
Pulls work items and time logs from Jira/Azure DevOps into pstore. Uses a `RemoteSync` trait (`remote.rs`) with implementations in `jira.rs` and `azure_devops.rs`. Parallel sync with tokio `JoinSet`. Can push timesheet entries back to Jira.

### `ptime` — timesheet management
Records time entries to text files, aggregates them by day/week/month/range. Entries use a `client::task::details` dot-path format. Can export to pstore for Jira sync via psync.

### `ptasks` — task runner
Reads JSON5 `tasks.json` files and runs tasks with dependency management (sequential/parallel). Supports shell commands, npm scripts, variable interpolation (`${workspaceFolder}`, `${input:*}`), and interactive prompts. Implemented as a single file (`ptasks/src/main.rs`).

### `pterry` — GUI launcher
An egui/eframe desktop launcher with a plugin system. Core abstraction is the `Picker` trait (`picker.rs`). Built-in pickers: desktop app launcher, emoji picker, calculator (numbat), notes, command language. Uses fuzzy matching (SkimMatcherV2). Runs as an always-on-top X11 overlay.

### Key cross-cutting patterns
- Async-first: tokio with full features throughout
- CLI parsing: clap with derive macros
- Credentials: stored in system keyring, never in config files
- Rust edition 2024, workspace-level dependency versions in root `Cargo.toml`
