# Jetpack - the Jet Orchestrator

Jetpack (aka Jet) is a general-purpose, community-driven IT automation platform for configuration management, 
deployment, orchestration, patching, and arbitrary task execution workflows. 

Jetpack is a GPLv3 licensed project, based on the original [Jetporch](https://github.com/jetporch/jetporch) project created and run by [Michael DeHaan](https://home.laserllama.net). [(<michael@michaeldehaan.net>)](mailto:michael@michaeldehaan.net).

Links (currently outdated, will be updated soon)

* [All Documentation](https://www.jetporch.com/)
* [Installation](https://www.jetporch.com/basics/installing-from-source)
* [Blog and Announcements](https://jetporch.substack.com/)
* [Discord Chat](https://www.jetporch.com/community/discord-chat)
* [Contribution Guide](https://www.jetporch.com/community/contributing)

Please route all questions, help requests, and feature discussion to Discord. Thanks!

## Universal Task Modifiers (with parameters)

Jetpack now includes a universal `skip_if_exists` parameter that works with ALL modules:

```yaml
- !any_module
  # ... module parameters ...
  with:
    skip_if_exists: /path/to/file  # Skip task if this path exists
```

This is part of the `with` section that provides pre-execution modifiers:
- `condition`: Conditional execution based on expressions
- `skip_if_exists`: Skip task if specified path exists (NEW in Jetpack!)
- `sudo`: Execute with elevated privileges
- `items`: Loop over a list of items
- `delegate_to`: Execute on a different host
- `subscribe`: Handler subscription
- `tags`: Task categorization

Example:
```yaml
- !unpack
  src: /tmp/archive.tar.gz
  dest: /opt/app
  with:
    skip_if_exists: /opt/app/bin/app  # Don't extract if app already exists
    sudo: root
```

## New Modules

### download module
Downloads files from URLs with atomic writes and permission management:
- `url`: Source URL to download from
- `dest`: Local destination path
- `mode`, `owner`, `group`: Optional file permissions
- `force`: Re-download even if file exists

### unpack module
Extracts various archive formats:
- Supports: tar.gz, tar.bz2, tar.xz, zip, gz, bz2, xz
- `src`: Archive file path
- `dest`: Extraction destination directory
- `mode`, `owner`, `group`: Apply to extracted files

### move module
Moves files and directories with optional backup:
- `src`: Source file/directory path
- `dest`: Destination path
- `backup`: Create timestamped backup if destination exists
- `force`: Overwrite destination
- `mode`, `owner`, `group`: Apply permissions after move

## Technical Debt

### Proper file operations in Connection trait
The current architecture forces every module to use shell commands for basic file operations. The `cmd_library` is just a bash string factory that returns commands like `mv '{}' '{}'`. This is fundamentally broken.

**Current (bad) architecture:**
```rust
// Module calls:
handle.remote.rename(&src, &dest)
// Which calls:
cmd_library::get_rename_command() -> "mv 'src' 'dest'"
// Which gets executed as bash
```

**What it SHOULD be:**
```rust
trait Connection {
    fn rename(&self, src: &Path, dest: &Path) -> Result<()>;
    fn copy(&self, src: &Path, dest: &Path) -> Result<()>;
    fn remove(&self, path: &Path) -> Result<()>;
    fn exists(&self, path: &Path) -> Result<bool>;
    // etc...
}

// Local uses std::fs, SSH uses commands, but modules don't care!
```

Every file operation should be a proper method on the Connection trait, not a bash string generator. Modules shouldn't have to know about shell escaping or command formatting.

### Path handling limitations
- Tilde expansion (`~`) is NOT supported in paths. Use explicit paths like `/home/{{ username }}` instead of `~/`.
- This is intentional - shell expansions can be unpredictable across different execution contexts.
- Always use absolute paths or template variables for home directories.

## TODO Features

### github_release module enhancements
- **Improved channel semantics** (DONE):
  - `stable` / `latest`: Production - stable releases only (these are synonyms, default is `latest`)
  - `prerelease`: Smart mode - gets the absolute latest version, preferring stable over RC of same version
  - `unstable`: Testing only - stays on prereleases even if stable exists
  - `any`: Latest regardless of stability
- **ETag caching support**: Add optional `cache_etags: true` parameter to enable GitHub API ETag caching
  - Store ETags in `~/.local/share/jetpack/github_etags.json` 
  - Send `If-None-Match` header to receive 304 responses that don't count against rate limits
  - Include cache expiry timestamps
  - Privacy-focused: opt-in only feature to avoid tracking user lookups
- **GitHub token authentication**: Add optional `github_token` parameter for authenticated requests (5000 req/hour vs 60)

## Roadmap

### Secrets Management
- **rage/age encryption support**: Integrate [rage](https://github.com/str4d/rage) (Rust implementation of age) for encrypting sensitive values in inventory
  - Encrypt individual values or entire files
  - Support age identity files for decryption
  - Integrate with templating system for transparent decryption
- **Secrets directory**: Load variables from a separate secrets directory (e.g., `../infra-secrets/`) to cleanly separate sensitive data from version-controlled inventory

### Declarative Infrastructure Provisioning
- **provision: block in host_vars**: Define infrastructure declaratively in inventory - hosts are created automatically before playbook execution
- **Provisioner backends**: proxmox_lxc (done), proxmox_vm, docker, libvirt, cloud providers
- **Auto VMID assignment**: Query cluster for next available ID when vmid not specified
