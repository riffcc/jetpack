//! Native OpenSSH PROTOCOL.krl reader. Unknown/critical formats fail closed.
use russh::keys::{PublicKeyOrCertificate, ssh_key};
use sha1::Digest;

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.0.len() {
            return Err("truncated SSH revocation list".into());
        }
        let (value, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<&'a [u8], String> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    fn end(&self) -> Result<(), String> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err("invalid SSH revocation section length".into())
        }
    }
}
fn extension(r: &mut Reader<'_>) -> Result<(), String> {
    r.string()?;
    let critical = r.byte()?;
    r.string()?;
    r.end()?;
    if critical != 0 {
        return Err("unsupported critical SSH revocation extension".into());
    }
    Ok(())
}
pub(super) fn is_revoked(data: &[u8], offered: &PublicKeyOrCertificate) -> Result<bool, String> {
    let mut r = Reader(data);
    if r.take(8)? != b"SSHKRL\n\0" || r.u32()? != 1 {
        return Err("unsupported SSH revocation list format".into());
    }
    r.u64()?;
    r.u64()?;
    if r.u64()? != 0 {
        return Err("unsupported SSH revocation list flags".into());
    }
    r.string()?;
    r.string()?;
    let mut keys = vec![offered.public_key().to_bytes().map_err(|e| e.to_string())?];
    if let Some(c) = offered.certificate() {
        keys.push(
            ssh_key::PublicKey::new(c.signature_key().clone(), "")
                .to_bytes()
                .map_err(|e| e.to_string())?,
        );
    }
    let mut revoked = false;
    while !r.0.is_empty() {
        let kind = r.byte()?;
        let mut section = Reader(r.string()?);
        match kind {
            1 => {
                let ca = section.string()?;
                if !ca.is_empty() {
                    ssh_key::PublicKey::from_bytes(ca)
                        .map_err(|_| "invalid KRL certificate authority")?;
                }
                section.string()?;
                let cert = offered
                    .certificate()
                    .filter(|_| ca.is_empty() || keys[1] == ca);
                while !section.0.is_empty() {
                    let kind = section.byte()?;
                    let mut sub = Reader(section.string()?);
                    match kind {
                        0x20 => {
                            if sub.0.is_empty() {
                                return Err("empty KRL serial list".into());
                            }
                            while !sub.0.is_empty() {
                                let serial = sub.u64()?;
                                revoked |= cert.is_some_and(|c| c.serial() == serial);
                            }
                        }
                        0x21 => {
                            let min = sub.u64()?;
                            let max = sub.u64()?;
                            if min > max {
                                return Err("invalid KRL serial range".into());
                            }
                            revoked |= cert.is_some_and(|c| (min..=max).contains(&c.serial()));
                        }
                        0x22 => {
                            let offset = sub.u64()?;
                            let bitmap = sub.string()?;
                            if bitmap.first().is_some_and(|b| b & 0x80 != 0) {
                                return Err("negative KRL serial bitmap".into());
                            }
                            if let Some(bit) = cert.and_then(|c| c.serial().checked_sub(offset))
                                && let Ok(byte) = usize::try_from(bit / 8)
                                && byte < bitmap.len()
                            {
                                revoked |= bitmap[bitmap.len() - 1 - byte] & (1 << (bit % 8)) != 0;
                            }
                        }
                        0x23 => {
                            if sub.0.is_empty() {
                                return Err("empty KRL key ID list".into());
                            }
                            while !sub.0.is_empty() {
                                let id = sub.string()?;
                                revoked |= cert.is_some_and(|c| c.key_id().as_bytes() == id);
                            }
                        }
                        0x39 => extension(&mut sub)?,
                        _ => {
                            return Err(format!(
                                "unsupported SSH certificate revocation section {kind}"
                            ));
                        }
                    }
                    sub.end()?;
                }
            }
            2 => {
                if section.0.is_empty() {
                    return Err("empty KRL key list".into());
                }
                while !section.0.is_empty() {
                    let key = section.string()?;
                    ssh_key::PublicKey::from_bytes(key).map_err(|_| "invalid KRL public key")?;
                    revoked |= keys.iter().any(|candidate| candidate == key);
                }
            }
            3 | 5 => {
                let size = if kind == 3 { 20 } else { 32 };
                if section.0.is_empty() {
                    return Err("empty KRL fingerprint section".into());
                }
                while !section.0.is_empty() {
                    let hash = section.string()?;
                    if hash.len() != size {
                        return Err("invalid KRL fingerprint length".into());
                    }
                    revoked |= keys.iter().any(|key| {
                        if kind == 3 {
                            sha1::Sha1::digest(key).as_slice() == hash
                        } else {
                            sha2::Sha256::digest(key).as_slice() == hash
                        }
                    });
                }
            }
            255 => extension(&mut section)?,
            _ => return Err(format!("unsupported SSH revocation section {kind}")),
        }
        section.end()?;
    }
    Ok(revoked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    fn key(seed: u8) -> ssh_key::PrivateKey {
        ssh_key::PrivateKey::new(
            ssh_key::private::Ed25519Keypair::from_seed(&[seed; 32]).into(),
            "",
        )
        .unwrap()
    }
    fn cert(ca: &ssh_key::PrivateKey, serial: u64, id: &str) -> PublicKeyOrCertificate {
        let mut b = ssh_key::certificate::Builder::new(
            vec![1; 32],
            key(2).public_key().key_data().clone(),
            0,
            u64::MAX,
        )
        .unwrap();
        b.serial(serial)
            .unwrap()
            .key_id(id)
            .unwrap()
            .cert_type(ssh_key::certificate::CertType::Host)
            .unwrap()
            .valid_principal("example.test")
            .unwrap();
        PublicKeyOrCertificate::Certificate(b.sign(ca).unwrap())
    }
    fn generate(root: &std::path::Path, spec: &str, ca: Option<&ssh_key::PrivateKey>) -> Vec<u8> {
        let spec_path = root.join("spec");
        let krl = root.join("revoked.krl");
        if krl.exists() {
            std::fs::remove_file(&krl).unwrap();
        }
        std::fs::write(&spec_path, spec).unwrap();
        let mut command = Command::new("ssh-keygen");
        command.args(["-k", "-f"]).arg(&krl);
        if let Some(ca) = ca {
            let path = root.join("ca.pub");
            std::fs::write(&path, ca.public_key().to_openssh().unwrap()).unwrap();
            command.arg("-s").arg(path);
        }
        let output = command.arg(spec_path).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(krl).unwrap()
    }
    #[test]
    fn native_krl_matches_openssh_keys_fingerprints_and_certificate_revocations() {
        let root = tempfile::tempdir().unwrap();
        let ca = key(1);
        let plain: PublicKeyOrCertificate = key(2).public_key().clone().into();
        for spec in [
            key(2).public_key().to_openssh().unwrap(),
            format!(
                "hash: {}",
                key(2).public_key().fingerprint(ssh_key::HashAlg::Sha256)
            ),
        ] {
            let data = generate(root.path(), &spec, None);
            assert!(is_revoked(&data, &plain).unwrap());
            assert!(!is_revoked(&data, &key(3).public_key().clone().into()).unwrap());
        }
        // OpenSSH chooses list, range or bitmap encodings according to density.
        for spec in [
            "serial: 7",
            "serial: 10-10000",
            "serial: 7\nserial: 9\nserial: 10\nserial: 12\nserial: 14\nserial: 15",
            "id: blocked",
        ] {
            let data = generate(root.path(), spec, Some(&ca));
            let offered = cert(&ca, if spec.contains("10000") { 500 } else { 7 }, "blocked");
            assert!(is_revoked(&data, &offered).unwrap(), "{spec}");
            assert!(
                !is_revoked(&data, &cert(&ca, 10001, "allowed")).unwrap(),
                "{spec}"
            );
            assert!(
                !is_revoked(&data, &cert(&key(4), 7, "blocked")).unwrap(),
                "CA isolation: {spec}"
            );
            assert!(!is_revoked(&data, &plain).unwrap());
            let public = root.path().join("offered.pub");
            std::fs::write(
                &public,
                offered.certificate().unwrap().to_openssh().unwrap(),
            )
            .unwrap();
            let result = Command::new("ssh-keygen")
                .args(["-Q", "-f"])
                .arg(root.path().join("revoked.krl"))
                .arg(public)
                .output()
                .unwrap();
            assert_eq!(result.status.code(), Some(1));
        }
        let data = generate(root.path(), &ca.public_key().to_openssh().unwrap(), None);
        assert!(is_revoked(&data, &cert(&ca, 1, "any")).unwrap());
    }
    #[test]
    fn malformed_krl_sections_never_authorize_a_key() {
        let root = tempfile::tempdir().unwrap();
        let key: PublicKeyOrCertificate = key(2).public_key().clone().into();
        let data = generate(root.path(), &key.public_key().to_openssh().unwrap(), None);
        for end in 0..data.len() {
            // A complete header without sections is a valid empty KRL.
            let result = is_revoked(&data[..end], &key);
            assert!(
                result.is_err() || end == 44,
                "accepted truncated KRL at {end}"
            );
        }
        let mut invalid = data.clone();
        invalid[8..12].copy_from_slice(&2u32.to_be_bytes());
        assert!(is_revoked(&invalid, &key).is_err());
        let mut invalid = data.clone();
        invalid[44] = 99;
        assert!(is_revoked(&invalid, &key).is_err());
        let mut invalid = data;
        invalid[45..49].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(is_revoked(&invalid, &key).is_err());
    }
}
