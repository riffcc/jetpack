---
title: Task Modifiers
weight: 2
---

Every task accepts an optional `with:` block of pre-execution modifiers. These work with **all** modules.

```yaml
- !any_module
  # ... module parameters ...
  with:
    skip_if_exists: /path/to/file   # skip the task if this path exists
```

## Available modifiers

| Modifier | Purpose |
|---|---|
| `condition` | Run the task only when an expression is true. |
| `skip_if_exists` | Skip the task if the given path already exists. |
| `sudo` | Execute with elevated privileges (`sudo: root`). |
| `items` | Loop the task over a list of items. |
| `delegate_to` | Execute on a different host than the current target. |
| `subscribe` | Subscribe this task to a handler. |
| `tags` | Categorise the task for selective runs. |

## Example

```yaml
- !unpack
  src: /tmp/archive.tar.gz
  dest: /opt/app
  with:
    skip_if_exists: /opt/app/bin/app   # don't re-extract if already present
    sudo: root
```

## Conditions

Conditions are expressions evaluated against the current variables (including [facts](../modules/facts/)):

```yaml
- !package
  name: apache2
  with:
    condition: (eq jet_os_flavor "Debian")
```
