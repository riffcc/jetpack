//! Let OpenSSH resolve its configuration; the SSH/SFTP transport stays native.
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::NamedTempFile;

pub(super) struct SshConfig {
    // Retained for the existing agent-forwarding subprocess, which must see the
    // same default policy and operator configuration as the native connection.
    pub file: NamedTempFile,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub values: HashMap<String, Vec<String>>,
}

pub(super) fn quote(value: &str) -> Result<String, String> {
    if value.contains(['\n', '\r', '\0']) {
        return Err("invalid SSH configuration value".into());
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn dump(command: &mut Command) -> Result<HashMap<String, Vec<String>>, String> {
    let output = command
        .output()
        .map_err(|e| format!("cannot resolve OpenSSH configuration: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "OpenSSH configuration is invalid: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| "OpenSSH configuration output is not UTF-8")?;
    let mut values: HashMap<String, Vec<String>> = HashMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(' ') {
            values.entry(key.into()).or_default().push(value.into());
        }
    }
    Ok(values)
}

impl SshConfig {
    pub fn resolve(
        host: &str,
        user: &str,
        port: i64,
        override_user: bool,
        override_port: bool,
        custom: Option<&Path>,
    ) -> Result<Self, String> {
        if host.is_empty() || host.starts_with('-') || host.contains(['\n', '\r', '\0']) {
            return Err("invalid SSH hostname".into());
        }
        let port: u16 = port
            .try_into()
            .ok()
            .filter(|p| *p != 0)
            .ok_or("SSH port must be between 1 and 65535")?;
        let mut file =
            NamedTempFile::new().map_err(|e| format!("cannot prepare SSH configuration: {e}"))?;
        if let Some(path) = custom {
            writeln!(file, "Include {}", quote(&path.to_string_lossy())?)
                .map_err(|e| e.to_string())?;
        } else {
            // Probe compiled defaults, not user configuration, to find the system
            // config directory (also works with Homebrew's OpenSSH prefix).
            let defaults = dump(Command::new("ssh").args(["-G", "-F", "/dev/null", "--", host]))?;
            let global = defaults
                .get("globalknownhostsfile")
                .and_then(|v| v.first())
                .ok_or("OpenSSH did not report its system host files")?;
            let first = paths(global)?
                .into_iter()
                .next()
                .ok_or("OpenSSH did not report its system host files")?;
            let system = first
                .parent()
                .ok_or("invalid OpenSSH system host file")?
                .join("ssh_config");
            writeln!(
                file,
                "Include ~/.ssh/config\nInclude {}",
                quote(&system.to_string_lossy())?
            )
            .map_err(|e| e.to_string())?;
        }
        // Set defaults only after OpenSSH has evaluated user/system Host, Include,
        // canonicalization and Match final rules. Explicit configuration wins.
        writeln!(
            file,
            "Host *\nMatch final\n StrictHostKeyChecking accept-new\n User {}\n Port {port}",
            quote(user)?
        )
        .map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        let mut command = Command::new("ssh");
        command.arg("-G").arg("-F").arg(file.path());
        if override_user {
            command.arg("-l").arg(user);
        }
        if override_port {
            command.arg("-p").arg(port.to_string());
        }
        let values = dump(command.arg("--").arg(host))?;
        let scalar = |name: &str| {
            values
                .get(name)
                .and_then(|v| v.first())
                .cloned()
                .ok_or_else(|| format!("OpenSSH did not report {name}"))
        };
        let hostname = scalar("hostname")?;
        let user = scalar("user")?;
        let port = scalar("port")?
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or("invalid resolved SSH port")?;
        // These trust sources cannot be silently ignored by the native client.
        for name in ["knownhostscommand", "proxycommand", "proxyjump"] {
            if values
                .get(name)
                .is_some_and(|v| v.iter().any(|s| s != "none"))
            {
                return Err(format!(
                    "{name} is not supported by Jetpack's native SSH transport"
                ));
            }
        }
        if values
            .get("verifyhostkeydns")
            .and_then(|v| v.first())
            .is_some_and(|v| v != "false" && v != "no")
        {
            return Err(
                "VerifyHostKeyDNS is not supported by Jetpack's native SSH transport".into(),
            );
        }
        Ok(Self {
            file,
            hostname,
            user,
            port,
            values,
        })
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .and_then(|v| v.first())
            .map(String::as_str)
    }
    pub fn files(&self, key: &str) -> Result<Vec<PathBuf>, String> {
        self.get(key)
            .map(paths)
            .transpose()
            .map(Option::unwrap_or_default)
    }
}

// ssh -G expands known-host filename tokens but does not quote its filename-list
// output. Absolute paths are split at the next absolute path, preserving spaces
// inside a single path. Refuse relative/ambiguous values instead of writing keys
// into an unintended file.
fn paths(value: &str) -> Result<Vec<PathBuf>, String> {
    if value == "none" {
        return Ok(Vec::new());
    }
    if !value.starts_with('/') {
        return Err("known-hosts paths must resolve to absolute paths".into());
    }
    let full = PathBuf::from(value);
    if full.exists() || full.parent().is_some_and(Path::is_dir) {
        return Ok(vec![full]);
    }
    let mut result = Vec::new();
    for (i, part) in value.split(" /").enumerate() {
        result.push(PathBuf::from(if i == 0 {
            part.to_string()
        } else {
            format!("/{part}")
        }));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_defaults_and_explicit_openssh_host_and_final_policy() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config");
        std::fs::write(&path, format!("Host alias\n HostName 127.0.0.1\n Port 2222\n User configured\n UserKnownHostsFile {}\n GlobalKnownHostsFile none\n", quote(root.path().join("known hosts").to_str().unwrap()).unwrap())).unwrap();
        let config =
            SshConfig::resolve("alias", "default-user", 22, false, false, Some(&path)).unwrap();
        assert_eq!(config.hostname, "127.0.0.1");
        assert_eq!(config.port, 2222);
        assert_eq!(config.user, "configured");
        assert_eq!(config.get("stricthostkeychecking"), Some("accept-new"));
        assert_eq!(
            config.files("userknownhostsfile").unwrap(),
            vec![root.path().join("known hosts")]
        );
        let config =
            SshConfig::resolve("alias", "override-user", 2223, true, true, Some(&path)).unwrap();
        assert_eq!(config.user, "override-user");
        assert_eq!(config.port, 2223);
        std::fs::write(&path, "Host alias\n HostName 127.0.0.1\nMatch final host 127.0.0.1\n StrictHostKeyChecking yes\n").unwrap();
        let config =
            SshConfig::resolve("alias", "default-user", 22, false, false, Some(&path)).unwrap();
        assert_eq!(config.get("stricthostkeychecking"), Some("true"));
    }
}
