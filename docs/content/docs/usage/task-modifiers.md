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
| `skip_if_exists` | Skip the current item if the path exists on the execution target. |
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

`skip_if_exists` renders its path per host/item and probes the execution target
through the selected connection, including delegation. It uses the task's sudo
and working-directory context in both plan and apply. An existing path skips
only that item; connection or probe errors fail the task. Local execution probes
the local target through the same machinery.

## Retries

Every module accepts top-level `retry` and `delay`:

```yaml
- !http
  url: https://api.example.com/health
  retry: 15
  delay: 2
```

`retry` is the number of additional attempts after a task fails (default `0`).
`delay` is the number of seconds between those attempts (default `1`); `0`
retries immediately. Successful tasks are not retried. Both fields accept
nonnegative integers or templates, evaluated separately for each host/item after
`with.condition`. Retries run the task's normal query/apply cycle again.

The unreleased `and.retry` and `and.delay` forms are no longer accepted.
`and.notify` and `and.ignore_errors` keep their existing locations and behavior.
For `!wait_for_host` and `!wait_for_http`, `delay` also remains the polling
interval: omitted values preserve their five-second polling default, while an
explicit value sets both the polling interval and the pause between task retries.

## Working directory

`!command`, `!shell`, and `!external` accept a top-level `chdir` field:

```yaml
- !command
  cmd: git status --short
  chdir: /srv/projects/{{ item }}
  with:
    items: [app, worker]
    sudo: deploy
```

Use an absolute path in the execution target's filesystem. For example, `/app`
inside a container refers to that container's directory. Jetpack renders the
path per host/item after skip conditions and passes it literally; spaces and
shell metacharacters are allowed. Relative paths, empty paths, `~` shorthand,
and NUL bytes are rejected.

Directory entry happens inside the task's sudo context and affects only that
command. External-module uploads, staging, and cleanup keep their usual
directory. If the target directory is missing or inaccessible, the task fails
before running the requested command, even with `failed_when: "false"`.
Omitting `chdir` preserves the target's existing default directory.

`plan` validates the rendered path's syntax without entering or checking the
directory, except when `skip_if_exists` needs to probe inside it. Other modules
reject `chdir`, and `with.chdir` is not supported.

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
