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

`yes` requires a pretrusted key. `ask` also rejects an unknown key in Jetpack’s noninteractive native client; approve the key separately or select `accept-new`. An explicit `no`/`off` policy permits a changed key without replacing its stored record or enrolling any other hostname/IP for that connection. Password authentication and requested agent forwarding are blocked for that connection because they expose credentials to an unverified server.

Jetpack reads `~/.ssh/config` followed by `/etc/ssh/ssh_config` natively. The first applicable value wins. It supports `Host` patterns (including negation), `Include` files with `*`/`?` globs, and `Match all`, `final`, `host`, `originalhost`, `user`, and `localuser` criteria. Explicit Jetpack play/inventory/CLI connection settings take precedence; implicit defaults do not. Custom configuration via the connection API replaces these default config sources.

Supported connection and trust settings are `HostName`, `User`, `Port`, `StrictHostKeyChecking`, `HostKeyAlias`, `UserKnownHostsFile`, `GlobalKnownHostsFile`, `HashKnownHosts`, `CheckHostIP`, `HostKeyAlgorithms`, and `RevokedHostKeys`. Quoted paths, `~/`, `${ENVIRONMENT_VARIABLE}`, and path tokens `%%`, `%d`, `%h`, `%n`, `%p`, `%r`, and `%u` are supported. Relative known-host and revocation paths resolve against the controller's working directory. Relative Include paths resolve against `~/.ssh` in user/custom config or `/etc/ssh` in system config. Include depth is limited to 16. Use absolute paths for alternate system configuration locations.

Trust lookup supports OpenSSH hostname patterns, negation, hashed entries, aliases, user/global trust files, multiple keys, revoked markers, host certificate authorities, and binary OpenSSH KRLs. Revocation checks are native, including explicit keys, SHA-1/SHA-256 fingerprints, certificate serial lists/ranges/bitmaps, and key IDs. Malformed or unsupported revocation formats fail closed.

The native configuration reader implements this documented subset. Unsupported `Match` criteria (including `exec`), DNS canonicalization, explicit `CASignatureAlgorithms`/`RequiredRSASize` restrictions, `ProxyCommand`, `ProxyJump`, `KnownHostsCommand`, and `VerifyHostKeyDNS` produce explicit errors. These are Jetpack integration limits, not fundamental russh restrictions. `HostKeyAlgorithms` currently requires an explicit list; list modifiers are rejected. `~user` paths, Include character classes, and unlisted path tokens are unsupported. Identity selection remains controlled by Jetpack's existing native authentication options, including SSH-agent key-comment selection.

**Ordinary SSH/SFTP connections require neither `ssh` nor `ssh-keygen` on the controller.** The pre-existing, explicitly requested agent-forwarding command path uses `ssh -A` and requires `ssh`. Only that path creates a temporary file containing resolved configuration directives, never host keys or credentials. Interoperability tests use OpenSSH tools as test oracles; the runtime trust implementation does not.
