---
title: Shell vs Command
weight: 1
---

Jetpack provides two modules for executing commands.

## Command module (`!command`)

Executes a command directly, **without** a shell. Safer, but more limited.

**Use when:**
- You're running a simple command without shell features.
- Security is a concern (no shell injection).
- You don't need pipes, redirections, or shell built-ins.

```yaml
- !command
  name: List files
  cmd: ls -la /tmp

- !command
  name: Run a program with arguments
  cmd: myprogram --option value
```

**Restrictions** — none of these are allowed:
`;` `<` `>` `&` `*` `?` `{` `}` `[` `]` `$` `` ` `` — and no pipes, redirections, or shell built-ins (like `cd`, `export`).

## Shell module (`!shell`)

Executes a command through a real shell (default `/bin/bash`).

**Use when:**
- You need shell features (pipes, redirections).
- You're running complex commands with shell syntax.
- You need shell built-ins or environment manipulation.

```yaml
- !shell
  name: Create config file with redirection
  cmd: echo "CONFIG=value" > /etc/myapp.conf

- !shell
  name: Complex pipeline
  cmd: ps aux | grep python | awk '{print $2}' | xargs kill

- !shell
  name: Use a different shell
  cmd: echo $SHELL
  shell: /bin/zsh
```

**Options:**
- `shell` — which shell to use (default `/bin/bash`).

Both modules support templating in `cmd` and work in [pull mode](../#pull-mode).
