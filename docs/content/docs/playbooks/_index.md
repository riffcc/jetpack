---
title: Playbooks
weight: 5
---

A playbook is a YAML file describing a list of tasks to run. Each task invokes a [module](../modules/) with parameters and an optional [`with:` block](../usage/task-modifiers/) of modifiers.

```yaml
---
- name: Set up a web server
  hosts: webservers
  tasks:
    - !package
      name: nginx
      state: present

    - !template
      src: nginx.conf.j2
      dest: /etc/nginx/nginx.conf

    - !service
      name: nginx
      state: started
      enabled: true
```

## Templating

Playbooks support `{{variable}}` substitution and conditionals throughout. Variables come from [inventory](../inventory/), `--extra-vars`, [facts](../modules/facts/), and registered task results.

```yaml
- !file
  path: "/home/{{ JET_USERNAME }}/.ssh/authorized_keys"
  state: present
```

## Validating playbooks

Before applying a playbook, validate it:

```bash
jetpack syntax-check --playbook site.yml
jetpack full-check --playbook site.yml --inventory production
```

See [Usage](../usage/#check-before-you-apply) for the full set of check modes.

## Local bootstrap

`jetpack local` supports a convention-based bootstrap flow from the current working directory. If you run it from a repository root, Jetpack first checks for `./.jetpack.yml`; if absent, it falls back to:

- `deploy/playbooks/bootstrap.yml`
- `deploy/roles`
- `deploy/inventory`

This lets a bootstrap repo expose a trivial install path:

```bash
#!/usr/bin/env bash
set -euo pipefail
jetpack local
```

### Optional `.jetpack.yml`

Override the local bootstrap defaults:

```yaml
local:
  playbook: deploy/playbooks/bootstrap.yml
  roles: deploy/roles
  inventory: deploy/inventory
```

Paths resolve relative to the repository root.

### Built-in bootstrap variables

When `jetpack local` runs, Jetpack injects these variables:

- `JET_CWD`
- `JET_REPO_ROOT`
- `JET_PLAYBOOK_DIR`
- `JET_ROLES_DIR`
- `JET_INVENTORY_DIR`
- `JET_USERNAME`
- `JET_USER_HOME` (when `HOME` is available)
