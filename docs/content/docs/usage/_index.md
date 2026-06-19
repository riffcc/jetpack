---
title: Usage
weight: 3
---

Jetpack runs a playbook against a target: either the local machine or remote hosts over SSH.

## Local execution

Run a playbook against the machine you're on:

```bash
jetpack local --playbook bootstrap.yml
```

`jetpack local` also supports a convention-based bootstrap flow from the current working directory — see [Playbooks](../playbooks/#local-bootstrap).

## SSH execution

Run a playbook against hosts defined in an inventory, over SSH:

```bash
jetpack ssh --playbook site.yml --inventory production
```

## Pull mode

Fetch a playbook from a URL or git repository and run it, useful for bootstrapping:

```bash
jetpack pull --url https://example.com/playbook.tar.gz
jetpack pull --playbook playbook.yml -i inventory_dir
```

## Check before you apply

Validate a run without changing anything:

| Command | What it checks |
|---|---|
| `jetpack syntax-check --playbook site.yml` | The playbook parses and references are valid. |
| `jetpack inventory-check --inventory production` | The inventory tree loads and resolves. |
| `jetpack full-check --playbook site.yml --inventory production` | Both, together. |

## Inspect inventory

List the hosts and groups an inventory resolves to:

```bash
jetpack show-inventory --inventory production --show-hosts
```

## Common flags

- `-i` / `--inventory` — inventory path (may be given multiple times; later paths overlay earlier ones — see [Inventory](../inventory/)).
- `-p` / `--playbook` — playbook file.
- `--limit-hosts` / `--limit-groups` — restrict the run.
- `-v` / `-vv` / `-vvv` — increase verbosity.
- `--extra-vars` / `-e` — inject variables on the command line.

Run `jetpack --help` for the full list.

## Next

- [Shell vs Command](./shell-vs-command/) — the two ways to run commands.
- [Task modifiers](./task-modifiers/) — `with:` blocks: conditionals, loops, delegation, and more.
