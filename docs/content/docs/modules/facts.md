---
title: facts
weight: 1
---

The `facts` module gathers system information about a target host and exposes it as variables for subsequent tasks.

## Usage

```yaml
- !facts
  name: Gather system facts
```

With optional external fact gatherers:

```yaml
- !facts
  name: Gather all facts
  facter: yes   # enable facter integration (if installed)
  ohai: yes     # enable ohai integration (if installed)
```

## Variables provided

### Core (always available)

| Variable | Description | Examples |
|---|---|---|
| `jet_os_type` | Operating system type | `Linux`, `MacOS` |
| `jet_arch` | System architecture | `x86_64`, `arm64`, `aarch64` |

### Linux-specific

| Variable | Description | Examples |
|---|---|---|
| `jet_os_flavor` | Distribution family | `EL` (RHEL/CentOS/Rocky), `Debian`, `Arch`, `Unknown` |
| `jet_os_release_*` | All fields from `/etc/os-release` | see below |

Common `jet_os_release_*` variables (read from `/etc/os-release`):

- `jet_os_release_id` — distribution ID (`ubuntu`, `rocky`, `debian`)
- `jet_os_release_id_like` — similar distributions (`rhel centos fedora`)
- `jet_os_release_version_id` — version (`22.04`, `9.3`)
- `jet_os_release_pretty_name` — full name (`Ubuntu 22.04.3 LTS`)

### macOS-specific

| Variable | Description | Value |
|---|---|---|
| `jet_os_flavor` | OS flavor | `OSX` |

### External fact gathering

| Variable | Description | Requires |
|---|---|---|
| `facter` | JSON object of all facter data | `facter: yes` and facter installed |
| `ohai` | JSON object of all ohai data | `ohai: yes` and ohai installed |

## Example playbook

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

1. Run `facts` before using any `jet_*` variables.
2. Not all `/etc/os-release` fields exist on every distribution.
3. `jet_os_flavor` defaults to `Unknown` if the family can't be determined.
4. External gathering (facter/ohai) requires those tools on the target host.
