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

## Inspecting inventory

See exactly what an inventory resolves to before running anything:

```bash
jetpack inventory-check -i production
jetpack show-inventory -i production --show-hosts --show-groups
```

## Templating

Variables from inventory are available everywhere via `{{name}}` — in playbooks, templates, and the `with:` [condition](../usage/task-modifiers/#conditions) expressions.
