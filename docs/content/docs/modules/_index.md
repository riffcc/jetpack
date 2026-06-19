---
title: Modules
weight: 6
---

Modules are the building blocks of a playbook — each one is a YAML task type that does one thing. Modules are organised into categories:

| Category | Examples |
|---|---|
| **files** | `file`, `copy`, `template`, `directory`, `unpack`, `download`, `move`, `git`, `stat`, `checksum` |
| **packages** | `package` (apt, pacman, yum/dnf, zypper, homebrew) |
| **services** | `service` (sd_service and others) |
| **commands** | `command`, `shell` |
| **access** | `user` |
| **control** | `assert`, `echo`, `debug`, `set`, `facts`, `fail` |

Module names map to the YAML task type, prefixed with `!`:

```yaml
- !file
  path: /etc/motd
  state: present

- !package
  name: htop
  state: present
```

## Documented modules

{{< cards >}}
  {{< card link="facts" title="facts" subtitle="Gather system facts as variables." >}}
{{< /cards >}}

> **Note:** Reference documentation for the full module set is being written. For now, the [module registry source](https://github.com/riffcc/jetpack/tree/main/src/modules) is the authoritative list.
