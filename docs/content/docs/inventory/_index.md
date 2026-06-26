---
title: Inventory
weight: 4
---

Inventory is the single source of truth for everything Jetpack touches: which hosts exist, how they're grouped, and the variables attached to each. An inventory is a directory tree:

```
production/
├── groups/          # one file per group; lists hosts and subgroups
├── group_vars/      # one file per group; the group's variables
└── host_vars/       # one file per host; the host's variables
```

Point Jetpack at it with `-i` / `--inventory`:

```bash
jetpack ssh --playbook site.yml -i production
```

## Multiple inventory paths

You can pass `-i` more than once. Jetpack loads each path **in order** and blends them — this is how you keep public inventory and secrets separate:

```bash
jetpack ssh -i inventory/public -i ../infra-secrets/london -p site.yml
```

### Overlay semantics

When the same group or host appears in more than one path, variables are **merged**, with later paths winning on conflicts:

- `group_vars/<group>` from a later path overlays an earlier path's variables for that group.
- `host_vars/<host>` accumulates across paths.

So a secrets inventory loaded second can add (or override) variables without losing anything declared in the public inventory. "Order of operations saves us" — whichever you load last wins.

> [!NOTE]
> Each `-i` directory must contain a `groups/` subdirectory (it may be empty), or Jetpack will refuse to load it.

## Provision overlays

A `provision:` block in `group_vars/<group>` deep-merges onto each member host's
`host_vars` provision config — so you can set or override provision fields for a
whole fleet from one file. Host-specific fields win on conflict, exactly like
every other variable (more-specific wins).

The common use is toggling a fleet's lifecycle state. Put each host's real
provision spec in its `host_vars` (no `state`, so it defaults to `present`), then
drive the whole group from one `group_vars` file:

```yaml
# group_vars/test-k8s
provision:
  state: destroyed     # tear every member down (remove this block, or use
                       # `present`, to create them again)
```

Because the `host_vars` provision fields take precedence, only `state` is
overridden — `type`, `cluster`, `ip`, and the rest still come from each host.
This is how a repeatable test harness can reset and recreate a cluster by
flipping a single file between `destroyed` and `present`.

## Inspecting inventory

See exactly what an inventory resolves to before running anything:

```bash
jetpack inventory-check -i production
jetpack show-inventory -i production --show-hosts --show-groups
```

## Templating

Variables from inventory are available everywhere via `{{name}}` — in playbooks, templates, and the `with:` [condition](../usage/task-modifiers/#conditions) expressions.
