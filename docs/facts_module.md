# Facts Module Documentation

The `facts` module gathers system information about target hosts and makes it available as variables for use in subsequent tasks.

## Usage

```yaml
- !facts
  name: Gather system facts
```

With optional external fact gathering tools:

```yaml
- !facts
  name: Gather all facts
  facter: yes  # Enable facter integration (if installed)
  ohai: yes    # Enable ohai integration (if installed)
```

## Variables Provided

### Core Variables (Always Available)

| Variable | Description | Example Values |
|----------|-------------|----------------|
| `jet_os_type` | Operating system type | `Linux`, `MacOS` |
| `jet_arch` | System architecture | `x86_64`, `arm64`, `aarch64` |

### Linux-Specific Variables

| Variable | Description | Example Values |
|----------|-------------|----------------|
| `jet_os_flavor` | Linux distribution family | `EL` (RHEL/CentOS/Rocky), `Debian`, `Arch`, `Unknown` |
| `jet_os_release_*` | All fields from `/etc/os-release` | See below |

#### Common `jet_os_release_*` Variables

The facts module reads `/etc/os-release` and creates variables with the prefix `jet_os_release_`. Common ones include:

- `jet_os_release_id` - Distribution ID (e.g., `ubuntu`, `rocky`, `debian`)
- `jet_os_release_id_like` - Similar distributions (e.g., `rhel centos fedora`)
- `jet_os_release_version_id` - Version number (e.g., `22.04`, `9.3`)
- `jet_os_release_pretty_name` - Full name (e.g., `Ubuntu 22.04.3 LTS`)
- `jet_os_release_name` - Release name (e.g., `Jammy Jellyfish`)
- `jet_os_release_version_codename` - Codename (e.g., `jammy`, `bookworm`)
- `jet_os_release_platform_id` - Platform ID (e.g., `platform:el9`)

### macOS-Specific Variables

| Variable | Description | Value |
|----------|-------------|-------|
| `jet_os_flavor` | OS flavor | `OSX` |

### External Fact Gathering

When enabled, these tools provide additional facts:

| Variable | Description | Requires |
|----------|-------------|----------|
| `facter` | JSON object with all facter data | `facter: yes` and facter installed |
| `ohai` | JSON object with all ohai data | `ohai: yes` and ohai installed |

## Example Playbook

```yaml
---
- name: Gather and use system facts
  hosts: all
  tasks:
    - !facts
      name: Gather system facts

    - !assert
      name: Ensure we're on Linux
      true: (eq jet_os_type "Linux")

    - !echo
      msg: "Running on {{ jet_os_release_pretty_name }} ({{ jet_arch }})"

    - !package
      name: httpd
      with:
        condition: (eq jet_os_flavor "EL")

    - !package
      name: apache2
      with:
        condition: (eq jet_os_flavor "Debian")
```

## Notes

1. The facts module must be run before using any `jet_*` variables
2. Not all `/etc/os-release` fields are available on all distributions
3. `jet_os_flavor` defaults to `Unknown` if the distribution family cannot be determined
4. External fact gathering (facter/ohai) requires those tools to be installed on target hosts