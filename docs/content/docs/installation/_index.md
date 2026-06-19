---
title: Installation
weight: 2
---

Jetpack is a single Rust binary. Until prebuilt binaries are published, build it from source.

## Prerequisites

- The Rust toolchain (install via [rustup](https://rustup.rs)). A recent stable compiler is required (edition 2024).

## Build from source

```bash
git clone https://github.com/riffcc/jetpack.git
cd jetpack
cargo build --release
```

The binary is produced at `target/release/jetpack`. Put it on your `PATH`:

```bash
# one option: install it into ~/.cargo/bin
cargo install --path .
```

Verify it runs:

```bash
jetpack --version
```

## Build the documentation locally

Jetpack can serve this documentation site from your working tree, so the docs always reflect the version you're running:

```bash
jetpack docs
```

See [Usage](../usage/) for everything else.
