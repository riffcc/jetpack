# Shell vs Command Modules

Jetpack provides two modules for executing commands:

## Command Module (`!command`)

The `command` module executes commands directly without a shell. It's safer but more limited.

**Use when:**
- Running simple commands without shell features
- Security is a concern (prevents shell injection)
- You don't need pipes, redirections, or shell built-ins

**Example:**
```yaml
- !command
  name: List files
  cmd: ls -la /tmp
  
- !command
  name: Run a program with arguments
  cmd: myprogram --option value
```

**Restrictions:**
- No shell metacharacters allowed: `;`, `<`, `>`, `&`, `*`, `?`, `{`, `}`, `[`, `]`, `$`, `` ` ``
- No pipes or redirections
- No shell built-ins (like `cd`, `export`, etc.)

## Shell Module (`!shell`)

The `shell` module executes commands through a real shell (default: `/bin/bash`).

**Use when:**
- You need shell features (pipes, redirections, etc.)
- Running complex commands with shell syntax
- Using shell built-ins or environment manipulation

**Example:**
```yaml
- !shell
  name: Create config file with redirection
  cmd: echo "CONFIG=value" > /etc/myapp.conf
  
- !shell
  name: Complex pipeline
  cmd: ps aux | grep python | awk '{print $2}' | xargs kill
  
- !shell
  name: Use different shell
  cmd: echo $SHELL
  shell: /bin/zsh
```

**Options:**
- `shell`: Specify which shell to use (default: `/bin/bash`)

## Pull Mode

Both modules work in pull mode. When using pull mode with inventory files for variables:

```bash
# Run in pull mode with inventory
cargo run -- pull -p playbook.yml -i inventory_dir

# Run in pull mode without inventory
cargo run -- pull -p playbook.yml
```

Variables from inventory files are automatically available in templates:

```yaml
- !shell
  name: Create app config
  cmd: |
    cat > /etc/app.conf << EOF
    APP_NAME={{ app_name }}
    VERSION={{ app_version }}
    EOF
```