---
title: Development
weight: 7
---

Jetpack is developed in the open at [github.com/riffcc/jetpack](https://github.com/riffcc/jetpack). Contributions are welcome.

## Getting started

```bash
git clone https://github.com/riffcc/jetpack.git
cd jetpack
cargo build
```

## Common commands

| Command | Purpose |
|---|---|
| `cargo build` | Compile. |
| `cargo test` | Run the test suite. |
| `cargo clippy -- -W clippy::all` | Lint (the project keeps this warning-free). |
| `cargo fmt` | Format the code (enforced in CI). |
| `jetpack docs` | Serve this documentation site locally. |

## Architecture

Jetpack is organised into:

- **CLI** (`src/cli/`) — argument parsing and command dispatch.
- **Inventory** (`src/inventory/`) — hosts, groups, variable blending.
- **Connection** (`src/connection/`) — local, SSH, and chroot execution.
- **Modules** (`src/modules/`) — task types, grouped by category.
- **Task engine** (`src/tasks/`) — task parsing and execution.
- **Playbooks** (`src/playbooks/`) — YAML parsing and templating.

See the repository's `CLAUDE.md` and `CONTRIBUTING.md` for more detail, and the [DeepWiki](https://deepwiki.com/riffcc/jetpack) for a generated architectural overview.
