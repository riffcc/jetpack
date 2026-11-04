//! Shared OpenSSH host trust files; no Jetpack-specific host-key database.
use super::ssh_config::{SshConfig, patterns_match};
use data_encoding::BASE64;
use hmac::{Hmac, KeyInit, Mac};
use russh::keys::{
    PublicKeyOrCertificate,
    ssh_key::{
        self,
        known_hosts::{Entry, Marker},
    },
};
use sha1::Sha1;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
#[cfg(test)]
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum HostKeyPolicy {
    AcceptNew,
    Strict,
    Ask,
    Off,
}

#[derive(Clone)]
pub(super) struct HostTrust {
    pub host: String,
    pub port: u16,
    pub alias: bool,
    pub user_files: Vec<PathBuf>,
    pub global_files: Vec<PathBuf>,
    pub policy: HostKeyPolicy,
    pub changed: Arc<AtomicBool>,
    pub hash: bool,
    pub ip: Option<String>,
    pub revoked: Option<PathBuf>,
}

enum Status {
    Trusted,
    Unknown,
    Changed,
}

impl HostTrust {
    pub fn new(config: &SshConfig, ip: String) -> Result<Self, String> {
        let alias = config.get("hostkeyalias").filter(|s| *s != "none");
        let host = alias
            .map(str::to_string)
            .unwrap_or_else(|| config.hostname.to_lowercase());
        let policy = config.get("stricthostkeychecking").unwrap_or("accept-new");
        let policy = match policy {
            "yes" | "true" => HostKeyPolicy::Strict,
            "ask" => HostKeyPolicy::Ask,
            "accept-new" => HostKeyPolicy::AcceptNew,
            "no" | "false" | "off" => HostKeyPolicy::Off,
            _ => return Err(format!("unsupported StrictHostKeyChecking value: {policy}")),
        };
        Ok(Self {
            host,
            port: config.port,
            alias: alias.is_some(),
            user_files: config.files("userknownhostsfile")?,
            global_files: config.files("globalknownhostsfile")?,
            policy,
            changed: Arc::new(AtomicBool::new(false)),
            hash: config.get("hashknownhosts") == Some("yes"),
            ip: (alias.is_none() && config.get("checkhostip") == Some("yes")).then_some(ip),
            revoked: config
                .get("revokedhostkeys")
                .filter(|p| *p != "none")
                .map(PathBuf::from),
        })
    }
    pub fn has_authority(&self) -> Result<bool, String> {
        Ok(self
            .entries(&self.host)?
            .iter()
            .any(|entry| entry.marker() == Some(&Marker::CertAuthority)))
    }
    pub fn prefer_known_algorithms(
        &self,
        algorithms: &mut [ssh_key::Algorithm],
    ) -> Result<(), String> {
        let entries = self.entries(&self.host)?;
        algorithms.sort_by_key(|algorithm| {
            !entries.iter().any(|entry| {
                entry.marker().is_none()
                    && (entry.public_key().algorithm() == *algorithm
                        || (entry.public_key().algorithm().is_rsa() && algorithm.clone().is_rsa()))
            })
        });
        Ok(())
    }
    fn lookup_name(&self, host: &str) -> String {
        if self.port == 22 || (self.alias && host == self.host) {
            host.to_string()
        } else {
            format!("[{host}]:{}", self.port)
        }
    }
    fn entries(&self, host: &str) -> Result<Vec<Entry>, String> {
        let lookup = self.lookup_name(host);
        let mut entries = Vec::new();
        for path in self.user_files.iter().chain(&self.global_files) {
            let text = match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(format!(
                        "cannot read known-hosts file {}: {e}",
                        path.display()
                    ));
                }
            };
            for line in text
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty() && !s.starts_with('#'))
            {
                let fields: Vec<_> = line.split_whitespace().collect();
                let index = usize::from(fields[0].starts_with('@'));
                let Some(patterns) = fields.get(index) else {
                    continue;
                };
                let matched = if patterns.starts_with('|') {
                    let parts: Vec<_> = patterns.split('|').collect();
                    if parts.len() != 4 || parts[1] != "1" {
                        return Err("invalid hashed known-hosts entry".into());
                    }
                    let salt = BASE64
                        .decode(parts[2].as_bytes())
                        .map_err(|_| "invalid known-hosts salt")?;
                    let hash = BASE64
                        .decode(parts[3].as_bytes())
                        .map_err(|_| "invalid known-hosts hash")?;
                    if salt.len() != 20 || hash.len() != 20 {
                        return Err("invalid known-hosts hash length".into());
                    }
                    let mut mac = Hmac::<Sha1>::new_from_slice(&salt).map_err(|e| e.to_string())?;
                    mac.update(lookup.as_bytes());
                    mac.verify_slice(&hash).is_ok()
                } else {
                    patterns_match(patterns.split(','), &lookup)
                };
                if matched {
                    // Normalize field separators for ssh-key's OpenSSH parser;
                    // unrelated legacy entries cannot affect this connection.
                    let normalized = fields.join(" ");
                    entries.push(normalized.parse::<Entry>().map_err(|e| {
                        format!(
                            "invalid matching known-hosts entry in {}: {e}",
                            path.display()
                        )
                    })?);
                }
            }
        }
        Ok(entries)
    }
    fn certificate_valid(
        &self,
        certificate: &ssh_key::Certificate,
        ca: &ssh_key::PublicKey,
    ) -> bool {
        if certificate.cert_type() != ssh_key::certificate::CertType::Host
            || !certificate.critical_options().is_empty()
        {
            return false;
        }
        if !certificate.valid_principals().is_empty()
            && !certificate.valid_principals().iter().any(|p| {
                let pattern = format!(
                    "(?i)^{}$",
                    regex::escape(p).replace("\\*", ".*").replace("\\?", ".")
                );
                regex::Regex::new(&pattern).is_ok_and(|r| r.is_match(&self.host))
            })
        {
            return false;
        }
        certificate
            .validate([&ca.fingerprint(ssh_key::HashAlg::Sha256)])
            .is_ok()
    }
    fn status(&self, host: &str, offered: &PublicKeyOrCertificate) -> Result<Status, String> {
        let entries = self.entries(host)?;
        let key = offered.public_key();
        if entries.iter().any(|entry| {
            entry.marker() == Some(&Marker::Revoked)
                && (entry.public_key().key_data() == key.key_data()
                    || offered
                        .certificate()
                        .is_some_and(|c| entry.public_key().key_data() == c.signature_key()))
        }) {
            return Err(format!("SSH host key for {host} is revoked"));
        }
        if entries.iter().any(|entry| {
            entry.marker().is_none() && entry.public_key().key_data() == key.key_data()
        }) {
            return Ok(Status::Trusted);
        }
        if let Some(certificate) = offered.certificate() {
            for entry in &entries {
                if entry.marker() == Some(&Marker::CertAuthority)
                    && self.certificate_valid(certificate, entry.public_key())
                {
                    return Ok(Status::Trusted);
                }
            }
            // An unknown/invalid CA must not bypass an existing host record by
            // enrolling the certificate's leaf key as a newly trusted host.
            if !entries.is_empty() {
                return Ok(Status::Changed);
            }
            return Err(format!(
                "SSH host certificate for {host} has no trusted certificate authority"
            ));
        }
        Ok(
            if entries.iter().any(|entry| {
                entry.marker().is_none() || entry.marker() == Some(&Marker::CertAuthority)
            }) {
                Status::Changed
            } else {
                Status::Unknown
            },
        )
    }
    fn check_revocations(&self, offered: &PublicKeyOrCertificate) -> Result<(), String> {
        let Some(path) = &self.revoked else {
            return Ok(());
        };
        let data = std::fs::read(path)
            .map_err(|e| format!("cannot read RevokedHostKeys {}: {e}", path.display()))?;
        if data.starts_with(b"SSHKRL\n\0") {
            if super::ssh_krl::is_revoked(&data, offered)? {
                return Err("SSH host key is revoked".into());
            }
        } else {
            let text = std::str::from_utf8(&data).map_err(|_| "invalid RevokedHostKeys file")?;
            for line in text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
            {
                let key = ssh_key::PublicKey::from_openssh(line)
                    .map_err(|_| "invalid RevokedHostKeys entry")?;
                if key.key_data() == offered.public_key().key_data()
                    || offered
                        .certificate()
                        .is_some_and(|c| key.key_data() == c.signature_key())
                {
                    return Err("SSH host key is revoked".into());
                }
            }
        }
        Ok(())
    }
    fn unknown_hosts(&self, offered: &PublicKeyOrCertificate) -> Result<Vec<String>, String> {
        let mut unknown = Vec::new();
        let mut changed = false;
        for host in std::iter::once(&self.host).chain(self.ip.as_ref()) {
            match self.status(host, offered)? {
                Status::Trusted => {}
                Status::Changed if self.policy == HostKeyPolicy::Off => {
                    self.changed.store(true, Ordering::SeqCst);
                    changed = true;
                }
                Status::Changed => {
                    return Err(format!(
                        "SSH host key changed for {} (port {}); refusing authentication",
                        host, self.port
                    ));
                }
                Status::Unknown
                    if matches!(self.policy, HostKeyPolicy::Strict | HostKeyPolicy::Ask) =>
                {
                    return Err(format!(
                        "unknown SSH host {} (port {}); strict host-key checking forbids automatic enrollment",
                        host, self.port
                    ));
                }
                Status::Unknown => {
                    if !unknown.contains(host) {
                        unknown.push(host.clone());
                    }
                }
            }
        }
        if changed {
            return Ok(Vec::new());
        }
        Ok(unknown)
    }
    pub fn verify(&self, offered: &PublicKeyOrCertificate) -> Result<(), String> {
        self.check_revocations(offered)?;
        let unknown = self.unknown_hosts(offered)?;
        if unknown.is_empty() {
            return Ok(());
        }
        self.enroll(offered)
    }
    fn enroll(&self, offered: &PublicKeyOrCertificate) -> Result<(), String> {
        if self.policy == HostKeyPolicy::Off && self.user_files.is_empty() {
            return Ok(());
        }
        let path = self
            .user_files
            .first()
            .ok_or("cannot record a new SSH host: UserKnownHostsFile is disabled")?;
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(|e| e.to_string())?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("cannot record SSH host key in {}: {e}", path.display()))?;
        if file.metadata().map_err(|e| e.to_string())?.is_file() {
            file.lock()
                .map_err(|e| format!("cannot lock known-hosts file: {e}"))?;
        }
        // Revalidate every name under the lock, including records that were
        // trusted initially. A tolerated change must never authorize enrollment.
        self.check_revocations(offered)?;
        let mut records = Vec::new();
        for host in self.unknown_hosts(offered)? {
            let mut name = self.lookup_name(&host);
            if self.hash {
                let mut salt = [0u8; 20];
                File::open("/dev/urandom")
                    .and_then(|mut f| f.read_exact(&mut salt))
                    .map_err(|e| e.to_string())?;
                let mut mac = Hmac::<Sha1>::new_from_slice(&salt).map_err(|e| e.to_string())?;
                mac.update(name.as_bytes());
                name = format!(
                    "|1|{}|{}",
                    BASE64.encode(&salt),
                    BASE64.encode(&mac.finalize().into_bytes())
                );
            }
            let key = ssh_key::PublicKey::new(offered.public_key().key_data().clone(), "")
                .to_openssh()
                .map_err(|e| e.to_string())?;
            records.push(format!("{name} {key}\n"));
        }
        if !records.is_empty() {
            let mut existing = Vec::new();
            file.read_to_end(&mut existing).map_err(|e| e.to_string())?;
            if !existing.is_empty() && !existing.ends_with(b"\n") {
                file.write_all(b"\n").map_err(|e| e.to_string())?;
            }
            file.write_all(records.concat().as_bytes())
                .and_then(|_| file.flush())
                .map_err(|e| format!("cannot record SSH host key: {e}"))?;
        }
        Ok(())
    }
}
use std::os::unix::fs::DirBuilderExt;

#[cfg(test)]
mod tests {
    use super::*;
    fn key(seed: u8) -> PublicKeyOrCertificate {
        let pair = ssh_key::private::Ed25519Keypair::from_seed(&[seed; 32]);
        ssh_key::PrivateKey::new(pair.into(), "")
            .unwrap()
            .public_key()
            .clone()
            .into()
    }
    fn trust(root: &Path, strict: bool, hash: bool) -> HostTrust {
        HostTrust {
            host: "example.test".into(),
            port: 2222,
            alias: false,
            user_files: vec![root.join("known hosts")],
            global_files: Vec::new(),
            policy: if strict {
                HostKeyPolicy::Strict
            } else {
                HostKeyPolicy::AcceptNew
            },
            changed: Arc::new(AtomicBool::new(false)),
            hash,
            ip: None,
            revoked: None,
        }
    }
    use std::path::Path;
    #[test]
    fn accept_new_is_persistent_openssh_compatible_and_rejects_changed_keys() {
        for hash in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let trust = trust(root.path(), false, hash);
            trust.verify(&key(1)).unwrap();
            let before = std::fs::read(&trust.user_files[0]).unwrap();
            assert_eq!(before.starts_with(b"|1|"), hash);
            trust.verify(&key(1)).unwrap();
            assert_eq!(std::fs::read(&trust.user_files[0]).unwrap(), before);
            assert!(trust.verify(&key(2)).unwrap_err().contains("changed"));
            assert_eq!(std::fs::read(&trust.user_files[0]).unwrap(), before);
            let result = Command::new("ssh-keygen")
                .args(["-F", "[example.test]:2222", "-f"])
                .arg(&trust.user_files[0])
                .output()
                .unwrap();
            assert!(result.status.success());
        }
    }
    #[test]
    fn native_lookup_honors_patterns_negation_tabs_and_matching_parse_errors() {
        let root = tempfile::tempdir().unwrap();
        let trust = trust(root.path(), true, false);
        let public = key(1).public_key().to_openssh().unwrap();
        let path = &trust.user_files[0];
        std::fs::write(path, format!("# comment\n*.unrelated invalid data\n[*.test]:2222,![excluded.test]:2222\t{public}\n")).unwrap();
        trust.verify(&key(1)).unwrap();
        let mut excluded = trust.clone();
        excluded.host = "excluded.test".into();
        assert!(excluded.verify(&key(1)).unwrap_err().contains("strict"));
        std::fs::write(path, "[example.test]:2222 invalid data\n").unwrap();
        assert!(
            trust
                .verify(&key(1))
                .unwrap_err()
                .contains("invalid matching")
        );
    }
    #[test]
    fn strict_unknown_hosts_do_not_create_a_trust_file() {
        let root = tempfile::tempdir().unwrap();
        let trust = trust(root.path(), true, false);
        assert!(trust.verify(&key(1)).unwrap_err().contains("strict"));
        assert!(!trust.user_files[0].exists());
    }
    #[test]
    fn global_entries_multiple_keys_and_revocation_are_honored() {
        let root = tempfile::tempdir().unwrap();
        let mut trust = trust(root.path(), true, false);
        let first = key(1).public_key().to_openssh().unwrap();
        let second = key(2).public_key().to_openssh().unwrap();
        let path = root.path().join("global");
        std::fs::write(
            &path,
            format!("[example.test]:2222 {first}\n[example.test]:2222 {second}\n"),
        )
        .unwrap();
        trust.global_files.push(path.clone());
        trust.verify(&key(2)).unwrap();
        assert!(!trust.user_files[0].exists());
        std::fs::write(
            path,
            format!("[example.test]:2222 {second}\n@revoked [example.test]:2222 {second}\n"),
        )
        .unwrap();
        assert!(trust.verify(&key(2)).unwrap_err().contains("revoked"));
    }
    #[test]
    fn concurrent_first_connections_cannot_enroll_different_keys() {
        let root = tempfile::tempdir().unwrap();
        let trust = trust(root.path(), false, false);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                barrier.wait();
                trust.verify(&key(1))
            });
            let b = scope.spawn(|| {
                barrier.wait();
                trust.verify(&key(2))
            });
            [a.join().unwrap(), b.join().unwrap()]
        });
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            std::fs::read_to_string(&trust.user_files[0])
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
    #[test]
    fn configured_no_policy_accepts_change_without_replacing_the_known_key() {
        let root = tempfile::tempdir().unwrap();
        let mut trust = trust(root.path(), false, false);
        trust.verify(&key(1)).unwrap();
        let before = std::fs::read(&trust.user_files[0]).unwrap();
        trust.policy = HostKeyPolicy::Off;
        trust.verify(&key(2)).unwrap();
        assert!(trust.changed.load(Ordering::SeqCst));
        assert_eq!(std::fs::read(&trust.user_files[0]).unwrap(), before);
    }
    #[test]
    fn off_with_changed_hostname_and_unknown_ip_never_enrolls() {
        for changed_ip in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut trust = trust(root.path(), false, false);
            trust.verify(&key(1)).unwrap();
            if changed_ip {
                trust.ip = Some(trust.host.clone());
                trust.host = "unknown.test".into();
            } else {
                trust.ip = Some("192.0.2.1".into());
            }
            trust.policy = HostKeyPolicy::Off;
            let before = std::fs::read(&trust.user_files[0]).unwrap();
            trust.verify(&key(2)).unwrap();
            assert!(trust.changed.load(Ordering::SeqCst));
            assert_eq!(std::fs::read(&trust.user_files[0]).unwrap(), before);
        }
    }
    #[test]
    fn locked_revalidation_applies_off_policy_without_enrolling_other_names() {
        for policy in [HostKeyPolicy::Off, HostKeyPolicy::AcceptNew] {
            let root = tempfile::tempdir().unwrap();
            let mut trust = trust(root.path(), false, false);
            trust.policy = policy;
            trust.ip = Some("192.0.2.1".into());
            assert_eq!(trust.unknown_hosts(&key(2)).unwrap().len(), 2);
            // A competing connection enrolls a conflicting hostname after the
            // first pass but before this connection acquires the file lock.
            std::fs::write(
                &trust.user_files[0],
                format!(
                    "[example.test]:2222 {}\n",
                    key(1).public_key().to_openssh().unwrap()
                ),
            )
            .unwrap();
            let before = std::fs::read(&trust.user_files[0]).unwrap();
            let result = trust.enroll(&key(2));
            assert_eq!(result.is_ok(), policy == HostKeyPolicy::Off);
            assert_eq!(
                trust.changed.load(Ordering::SeqCst),
                policy == HostKeyPolicy::Off
            );
            assert_eq!(std::fs::read(&trust.user_files[0]).unwrap(), before);
        }
    }
    #[test]
    fn host_key_alias_is_not_decorated_with_a_port() {
        let root = tempfile::tempdir().unwrap();
        let mut trust = trust(root.path(), false, false);
        trust.host = "separate-trust-name".into();
        trust.alias = true;
        trust.verify(&key(1)).unwrap();
        let text = std::fs::read_to_string(&trust.user_files[0]).unwrap();
        assert!(text.starts_with("separate-trust-name ssh-ed25519 "));
        trust.verify(&key(1)).unwrap();
    }
    #[test]
    fn trusted_host_ca_checks_certificate_type_principal_validity_and_revocation() {
        let root = tempfile::tempdir().unwrap();
        let trust = trust(root.path(), true, false);
        let ca = ssh_key::PrivateKey::new(
            ssh_key::private::Ed25519Keypair::from_seed(&[3; 32]).into(),
            "",
        )
        .unwrap();
        std::fs::write(
            &trust.user_files[0],
            format!(
                "@cert-authority [example.test]:2222 {}\n",
                ca.public_key().to_openssh().unwrap()
            ),
        )
        .unwrap();
        let subject = key(1).public_key();
        let cert = |principal: &str, kind: ssh_key::certificate::CertType, before: u64| {
            let mut builder = ssh_key::certificate::Builder::new(
                vec![0u8; 32],
                subject.key_data().clone(),
                0,
                before,
            )
            .unwrap();
            builder
                .cert_type(kind)
                .unwrap()
                .valid_principal(principal)
                .unwrap();
            PublicKeyOrCertificate::Certificate(builder.sign(&ca).unwrap())
        };
        trust
            .verify(&cert(
                "example.test",
                ssh_key::certificate::CertType::Host,
                u64::MAX,
            ))
            .unwrap();
        assert!(
            trust
                .verify(&cert(
                    "other.test",
                    ssh_key::certificate::CertType::Host,
                    u64::MAX
                ))
                .is_err()
        );
        assert!(
            trust
                .verify(&cert(
                    "example.test",
                    ssh_key::certificate::CertType::User,
                    u64::MAX
                ))
                .is_err()
        );
        assert!(
            trust
                .verify(&cert(
                    "example.test",
                    ssh_key::certificate::CertType::Host,
                    1
                ))
                .is_err()
        );
        let mut revoked = trust.clone();
        let path = root.path().join("revoked");
        std::fs::write(&path, ca.public_key().to_openssh().unwrap()).unwrap();
        revoked.revoked = Some(path);
        assert!(
            revoked
                .verify(&cert(
                    "example.test",
                    ssh_key::certificate::CertType::Host,
                    u64::MAX
                ))
                .unwrap_err()
                .contains("revoked")
        );
    }
    #[test]
    fn openssh_krl_revocations_are_enforced() {
        let root = tempfile::tempdir().unwrap();
        let mut trust = trust(root.path(), false, false);
        let public = root.path().join("public_key");
        std::fs::write(&public, key(1).public_key().to_openssh().unwrap()).unwrap();
        let krl = root.path().join("revoked.krl");
        let output = Command::new("ssh-keygen")
            .args(["-k", "-f"])
            .arg(&krl)
            .arg(&public)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        trust.revoked = Some(krl);
        assert!(trust.verify(&key(1)).unwrap_err().contains("revoked"));
        trust.verify(&key(2)).unwrap();
    }
}
