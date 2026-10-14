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

The shared condition evaluator supports `and` and `or` with left-to-right,
short-circuit evaluation. This also applies to module condition fields such
as `changed_when` and `failed_when` in `!command`, `!shell`, and `!external`.
Ordinary comparison and string helpers retain their existing semantics.

```yaml
with:
  condition: '(and (isdefined resource) (eq resource.enabled true))'
```

`and` stops at its first false operand; `or` stops at its first true operand.
Skipped operands do not resolve variables or execute helpers, so a guard can
protect an otherwise invalid access. Errors from evaluated operands still fail
the condition. Logical results use the same truthiness as Handlebars `#if`,
including zero and empty strings being false. Missing variables keep their
existing behavior: `isdefined` can test them, while comparison helpers require
their parameters to exist.

```yaml
changed_when: '(or (eq rc 0) (contains out "updated"))'
```

Nested expressions and any number of operands are supported. `(and)` is true
and `(or)` is false; a single operand follows its normal truthiness. Logical
operators accept positional operands, not hash arguments. These lazy operators
are part of condition evaluation; ordinary text template rendering is unchanged.
