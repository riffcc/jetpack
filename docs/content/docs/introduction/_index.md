---
title: Introduction
weight: 1
---

Jetpack (sometimes "Jet") is a general-purpose, community-driven IT automation platform for configuration management, deployment, orchestration, patching, and arbitrary task execution workflows. It is written in Rust.

Jetpack is a GPLv3-licensed revival of [Jetporch](https://github.com/jetporch/jetporch), the original project created by [Michael DeHaan](https://home.laserllama.net).

## Why Jetpack?

- **Fast and self-contained** — a single Rust binary. No agent is required on target hosts.
- **Same playbooks, local or remote** — run against the local machine or over SSH with no changes to your code.
- **Templating first** — `{{variables}}` and conditionals throughout; [inventory](../inventory/) is the single source of truth.
- **Check before you apply** — `syntax-check`, `inventory-check`, and `full-check` validate a run before anything changes.
- **Instantiation** — provision VMs and LXCs on-demand, just-in-time, ahead of a playbook.

## At a glance

A playbook is a YAML list of tasks. Each task is a module (`!command`, `!package`, `!file`, …) with parameters and an optional `with:` block of modifiers:

```yaml
- !package
  name: nginx
  with:
    sudo: root
    condition: (eq jet_os_flavor "Debian")
```

## Where to next

- [Installation](../installation/) — build the binary.
- [Usage](../usage/) — run your first playbook.
- [Playbooks](../playbooks/) — the playbook format and modifiers.
- [Modules](../modules/) — the building blocks.

> **Note:** Jetpack is pre-alpha. Some areas of these docs are still being written — see the [GitHub repository](https://github.com/riffcc/jetpack) and the [DeepWiki](https://deepwiki.com/riffcc/jetpack) for more.
