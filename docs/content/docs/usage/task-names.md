---
title: Task Names
weight: 3
---

Jetpack generates task descriptions when `name:` is omitted. Describe the
operation once; Jetpack uses its module and public resource fields to label it.
An explicit `name:` takes precedence and keeps its existing behavior.

| Task | Generated description |
|---|---|
| `!apt` with `package: [nginx, curl]` | `Install nginx, curl` |
| `!apt` with `remove: true` | `Remove nginx, curl` |
| `!template` targeting `/etc/resolv.conf` | `Render /etc/resolv.conf` |
| `!directory` targeting `/opt/app` | `Ensure directory /opt/app` |
| `!sd_service` restarting nginx | `Restart nginx` |

Use a manual name when you want to explain the purpose:

```yaml
- !apt
  name: Install dependencies for Jellyfin transcoding
  package: [intel-media-va-driver, vainfo]
```

## Lists and loops

One declaration receives one stable description, including item loops:

```yaml
- !apt
  package: '{{ item }}'
  with:
    items: [nginx, curl]
```

The description is `Install nginx, curl`, just like `package: [nginx, curl]`.
The resource list still uses a native package-manager batch; the loop still
runs separate iterations. Combinations of resource lists and loops also retain
their execution boundaries. Names follow the package batch's existing ordering
and duplicate handling within each batch; deliberate loop repetitions remain
visible in the resource summary.

Descriptions show up to four resources, with a count for additional resources,
and generated labels are limited to 120 characters. Whitespace is collapsed and
long labels end with an ellipsis. Explicit names are preserved as written.

## Available values and fallbacks

Literal resources and plain references to already available values can appear
in generated names. Only values equal across all targeted hosts are eligible;
otherwise Jetpack uses a common description such as `Install packages`.

Naming does not render helpers, resolve nested template aliases, load files,
make network requests, or evaluate a task. Complex, missing, or unresolved
values use a generic description and do not introduce new failures for skipped
tasks. Modules without a description implementation fall back to their module
name.

In normal execution a label is prepared before each task starts. In host-parallel
`--async` execution labels are frozen before workers start the task sequence;
values that earlier tasks may replace use a generic label. Output metadata
invalidates potentially changed values without running those tasks or predicting
their results. Dynamic save names and opaque writers conservatively invalidate
the snapshot. Workers and
item iterations cannot overwrite the declaration's description. Multi-batch plays
use a common snapshot from the start of the play, so host-specific values cannot
change a declaration's name between execution batches. Role or runtime values
unavailable in that snapshot use generic descriptions.

## Commands and secrets

Command and shell names display their **original command templates**, with
variable references preserved:

```yaml
- !command
  cmd: examplectl login --token '{{ api_token }}'
```

Generated description:

```text
Run examplectl login --token '{{ api_token }}'
```

Store secrets in variables and reference those variables from commands. Literal
credentials embedded in command text appear in generated labels. Jetpack does
not attempt to identify or sanitize arbitrary credentials. A command supplied
as `cmd: '{{ login_command }}'` is described as `Run {{ login_command }}`;
references nested inside that variable are never expanded for naming.

HTTP descriptions use only a literal standard method and, when available, the
literal endpoint's hostname. They exclude URL credentials, paths, query strings,
fragments, request headers, and bodies. Templated endpoints use a generic HTTP
label. Download and checkout tasks describe their destinations rather than
source URLs.

These rules govern task labels. Verbose execution logs and other output channels
retain their existing behavior.
