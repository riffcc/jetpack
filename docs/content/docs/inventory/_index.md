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

You can pass `-i` more than once to compose one inventory from multiple sources.
Each directory can contribute `groups/`, `group_vars/`, `host_vars/`, or any
combination of them. Executable dynamic inventory sources are also supported.
Jetpack combines topology from all sources, then applies variables in the
supplied order, with later sources winning on conflicting declarations.

For example, separate public inventory and secrets:

```bash
jetpack ssh -i inventory/public -i ../infra-secrets/london -p site.yml
```

### Combining sources

When the same group or host appears in more than one path, variables are **merged**, with later paths winning on conflicts:

- `group_vars/<group>` from a later path overlays an earlier path's variables for that group.
- `host_vars/<host>` accumulates across paths.

Sources can add hosts, groups, memberships, and variables. Using a later source
to override selected values gives you an overlay; it uses the same composition
rules as every other source. A variables-only source can refer to hosts and
groups declared elsewhere in the combined inventory, including a later source.

### Variable resolution

Inherited group declarations remain separate from host declarations. Each
ancestor group contributes once, including when multiple membership paths reach
the same ancestor. Parents are applied before descendants; when multiple groups
are eligible, alphabetical group-name order makes their precedence deterministic.
Host variables apply after groups, and CLI extra variables have highest
precedence. Later inventory paths still replace conflicting keys within the same
group or host declaration.

When resolving different declaration layers, mappings merge recursively and
lists append in layer order. Intentional repeated list entries are preserved.
Reading resolved variables repeatedly does not mutate declarations or grow lists.

For Rust consumers, `Host::get_variables()` returns host declarations and runtime
updates. `get_inventory_variables()` resolves groups, host values, and CLI
overrides for inventory consumers such as DNS and provisioning. Templates use
`PlaybookContext`, which adds facts, play/role variables, and CLI overrides in
their existing order. Standalone `get_blended_variables()` includes facts and
CLI overrides; the context uses a view without those CLI overrides so they enter
only at its final precedence layer. Never save a resolved view back into the
host declaration map.

## Provision overlays

A `provision:` block in `group_vars/<group>` deep-merges onto each member host's
`host_vars` provision config — so you can set or override provision fields for a
whole fleet from one file. Host-specific fields win on conflict, exactly like
every other variable (more-specific wins). The same works ad-hoc from the CLI
with `-e` / `--extra-vars` (highest precedence): `-e provision.state=destroyed`
overrides every host's lifecycle state for that run, no inventory file touched.
(Values take the convenient `key=value` form, with dotted keys for nesting; JSON
and `@file` are still accepted for typed/structured values.)

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
