# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Common Development Commands

### Build and Run
- Build release: `cargo build --release`
- Run: `cargo run`
- Clean build artifacts: `make clean` or `cargo clean`

### Testing
- Run all tests: `cargo test`
- Run tests with output: `cargo test -- --nocapture`
- Run specific test: `cargo test test_name`
- Run tests in specific module: `cargo test module_name::`
- Run tests without building: `cargo test --no-run`

### Code Quality
- Format code: `cargo fmt`
- Run clippy linter: `cargo clippy`
- Check without building: `cargo check`

## Architecture Overview

JetPack (formerly JetPorch) is an IT automation platform written in Rust for configuration management, deployment, and orchestration. The codebase follows a modular architecture:

### Core Components

1. **CLI Layer** (`src/cli/`)
   - `parser.rs`: Command-line argument parsing
   - `playbooks.rs`: Playbook execution orchestration (ssh, local, check modes)
   - `show.rs`: Inventory display functionality

2. **Inventory System** (`src/inventory/`)
   - `hosts.rs`, `groups.rs`: Host and group management
   - `loading.rs`: Inventory file parsing
   - `inventory.rs`: Core inventory data structures

3. **Connection Layer** (`src/connection/`)
   - `connection.rs`: Abstract connection interface
   - `ssh.rs`: SSH connection implementation  
   - `local.rs`: Local execution implementation
   - `factory.rs`: Connection type instantiation

4. **Module System** (`src/modules/`)
   - Organized by category: `access/`, `commands/`, `control/`, `files/`, `packages/`, `services/`
   - Each module implements task execution logic
   - Modules are registered in the module registry

5. **Task Engine** (`src/tasks/`)
   - Task parsing and validation
   - Task execution workflow
   - Variable handling and templating

6. **Playbook Processing** (`src/playbooks/`)
   - YAML playbook parsing
   - Play and task orchestration
   - Template helpers (`t_helpers.rs` contains the only existing tests)

7. **Handle System** (`src/handle/`)
   - Abstracts local vs remote execution
   - Template processing
   - Command response handling

### Key Design Patterns

- **Factory Pattern**: Used for creating connections and modules
- **Registry Pattern**: Modules are registered and looked up dynamically
- **Arc<RwLock<T>>**: Used for thread-safe shared state (inventory, connections)
- **Result<T, String>**: Error handling uses String errors throughout

### Module Development

When adding new modules:
1. Create module in appropriate category under `src/modules/`
2. Implement required traits and handle methods
3. Register in the module's category `mod.rs`
4. Module names map to YAML task types in playbooks

### Testing Approach

Currently minimal test coverage - only helper functions in `src/playbooks/t_helpers.rs` have tests. When adding tests:
- Use `#[cfg(test)]` modules within source files
- Follow existing test patterns from `t_helpers.rs`
- Test modules should focus on unit testing individual components
- Integration tests can be added in `tests/` directory (currently none exist)