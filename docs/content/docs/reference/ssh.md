---
title: SSH host trust
weight: 140
---

Jetpack uses its native Rust SSH and SFTP clients. Host trust interoperates with OpenSSH: new hosts are recorded in the operator’s existing `known_hosts` file, and known hosts are checked before authentication. Jetpack creates no separate host-key database.

**The default behavior is `accept-new`.** A first connection records the host key; repeat connections accept that key; a changed key is rejected without replacing the stored key or sending credentials.

OpenSSH configuration controls the trust policy. For example:

```sshconfig
Host production
    HostName server.example.org
    Port 2222
    StrictHostKeyChecking yes
    UserKnownHostsFile ~/.ssh/known_hosts
```

`yes` requires a pretrusted key. `ask` also rejects an unknown key in Jetpack’s noninteractive native client; approve the key separately or select `accept-new`. An explicit `no`/`off` policy permits a changed key without replacing its stored record, but password authentication and requested agent forwarding are blocked for that connection, as they would expose credentials to an unverified server.

Jetpack asks `ssh -G` to resolve Host/Match/Include rules and uses `ssh-keygen -F` for OpenSSH hostname-pattern and hashed-entry lookup. It honors HostName, User, Port, HostKeyAlias, UserKnownHostsFile, GlobalKnownHostsFile, HashKnownHosts, CheckHostIP, HostKeyAlgorithms, RevokedHostKeys (including KRL files), and host-certificate authorities/revocation markers. Known-host paths must resolve to absolute paths. Existing Jetpack play/inventory and explicit CLI connection settings take precedence over SSH config; implicit defaults do not.

The transport and authentication implementation remain native, including password/private-key authentication, agent key-comment selection, and SFTP transfers. This does not implement every OpenSSH transport option: ProxyCommand, ProxyJump, KnownHostsCommand, and VerifyHostKeyDNS fail explicitly rather than being silently bypassed. Identity selection remains controlled by Jetpack’s existing authentication options.

The existing `ssh -A` command path uses the same resolved configuration and default trust policy. A temporary configuration file contains only configuration directives, never host keys or credentials, and is removed when the connection is dropped.

The controller needs OpenSSH’s `ssh` and `ssh-keygen` tools for configuration/trust interoperability. The Jetpack container includes `openssh-client`.
