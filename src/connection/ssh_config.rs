//! Native interpretation of the supported OpenSSH configuration subset.
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

pub(super) struct SshConfig {
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub values: HashMap<String, Vec<String>>,
    sources: Vec<PathBuf>,
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

// OpenSSH patterns use only * and ?, not regex or filesystem character classes.
fn pattern_matches(pattern: &str, value: &str, insensitive: bool) -> bool {
    let pattern = format!(
        "{}^{}$",
        if insensitive { "(?i)" } else { "" },
        regex::escape(pattern)
            .replace("\\*", ".*")
            .replace("\\?", ".")
    );
    regex::Regex::new(&pattern).is_ok_and(|r| r.is_match(value))
}
pub(super) fn patterns_match(
    patterns: impl IntoIterator<Item = impl AsRef<str>>,
    value: &str,
) -> bool {
    patterns_match_case(patterns, value, true)
}
fn patterns_match_case(
    patterns: impl IntoIterator<Item = impl AsRef<str>>,
    value: &str,
    insensitive: bool,
) -> bool {
    let mut positive = false;
    for pattern in patterns {
        let pattern = pattern.as_ref();
        if let Some(negative) = pattern.strip_prefix('!') {
            if pattern_matches(negative, value, insensitive) {
                return false;
            }
        } else if pattern_matches(pattern, value, insensitive) {
            positive = true;
        }
    }
    positive
}

// Preserve quoted whitespace and comments without invoking a shell. OpenSSH
// accepts keyword=value as well as keyword value.
fn tokens(line: &str) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                word.push(chars.next().ok_or("trailing escape in SSH config")?);
                started = true;
            }
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '#' if !quoted => break,
            c if !quoted && (c.is_whitespace() || (c == '=' && result.is_empty())) => {
                if started {
                    result.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            _ => {
                word.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return Err("unterminated quote in SSH config".into());
    }
    if started {
        result.push(word);
    }
    Ok(result)
}

fn home() -> Result<PathBuf, String> {
    std::env::home_dir().ok_or("cannot locate the controller home directory".into())
}
fn tilde(value: &str) -> Result<PathBuf, String> {
    if value == "~" {
        return home();
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return Ok(home()?.join(rest));
    }
    if value.starts_with('~') {
        return Err(
            "~user paths are not supported in native SSH config; use an absolute path".into(),
        );
    }
    Ok(PathBuf::from(value))
}
fn include_paths(pattern: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = vec![PathBuf::new()];
    for component in pattern.components() {
        let part = component.as_os_str().to_string_lossy();
        if part.contains(['*', '?']) {
            let mut next = Vec::new();
            for parent in &paths {
                match std::fs::read_dir(parent) {
                    Ok(entries) => {
                        for entry in entries {
                            let entry = entry.map_err(|e| e.to_string())?;
                            // Filesystem globbing is case-sensitive and omits dotfiles
                            // unless the pattern explicitly begins with a dot.
                            let name = entry.file_name().to_string_lossy().into_owned();
                            let regex = format!(
                                "^{}$",
                                regex::escape(&part)
                                    .replace("\\*", ".*")
                                    .replace("\\?", ".")
                            );
                            if (!name.starts_with('.') || part.starts_with('.'))
                                && regex::Regex::new(&regex).unwrap().is_match(&name)
                            {
                                next.push(entry.path());
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
            paths = next;
        } else if part.contains(['[', ']']) {
            return Err("character classes in SSH Include paths are unsupported".into());
        } else {
            for path in &mut paths {
                path.push(component);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

struct Evaluation<'a> {
    original: &'a str,
    host: &'a str,
    user: &'a str,
    local_user: &'a str,
    final_pass: bool,
    wants_final: bool,
    active: bool,
}
impl Evaluation<'_> {
    fn matches(
        &mut self,
        args: &[String],
        values: &HashMap<String, Vec<String>>,
    ) -> Result<bool, String> {
        let mut matched = true;
        let mut i = 0;
        while i < args.len() {
            let (negate, criterion) = args[i]
                .strip_prefix('!')
                .map_or((false, args[i].as_str()), |v| (true, v));
            let criterion = criterion.to_lowercase();
            i += 1;
            let value = match criterion.as_str() {
                "all" => true,
                "final" => {
                    self.wants_final = true;
                    self.final_pass
                }
                "host" | "originalhost" | "user" | "localuser" => {
                    let patterns = args.get(i).ok_or("missing SSH Match argument")?;
                    i += 1;
                    let value = match criterion.as_str() {
                        "host" => values
                            .get("hostname")
                            .and_then(|v| v.first())
                            .map(String::as_str)
                            .unwrap_or(self.host),
                        "originalhost" => self.original,
                        "user" => values
                            .get("user")
                            .and_then(|v| v.first())
                            .map(String::as_str)
                            .unwrap_or(self.user),
                        _ => self.local_user,
                    };
                    patterns_match_case(
                        patterns.split(','),
                        value,
                        matches!(criterion.as_str(), "host" | "originalhost"),
                    )
                }
                _ => {
                    return Err(format!(
                        "SSH Match {criterion} is not supported by native configuration resolution"
                    ));
                }
            };
            matched &= value != negate;
        }
        if args.is_empty() {
            return Err("empty SSH Match rule".into());
        }
        Ok(matched)
    }
    fn read(
        &mut self,
        path: &Path,
        base: &Path,
        depth: usize,
        values: &mut HashMap<String, Vec<String>>,
        required: bool,
    ) -> Result<(), String> {
        if depth > 16 {
            return Err("SSH Include nesting exceeds 16 levels".into());
        }
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if !required && e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(format!("cannot read SSH config {}: {e}", path.display())),
        };
        for (index, line) in text.lines().enumerate() {
            let words =
                tokens(line).map_err(|e| format!("{}:{}: {e}", path.display(), index + 1))?;
            if words.is_empty() {
                continue;
            }
            let key = words[0].to_lowercase();
            let args = &words[1..];
            if args.is_empty() {
                return Err(format!(
                    "{}:{}: missing {key} value",
                    path.display(),
                    index + 1
                ));
            }
            match key.as_str() {
                "host" => self.active = patterns_match(args, self.host),
                "match" => self.active = self.matches(args, values)?,
                "include" if self.active => {
                    for arg in args {
                        let expanded = tilde(arg)?;
                        let expanded = if expanded.is_absolute() {
                            expanded
                        } else {
                            base.join(expanded)
                        };
                        for path in include_paths(&expanded)? {
                            self.read(&path, base, depth + 1, values, false)?;
                        }
                    }
                }
                "include" => {}
                _ if self.active => {
                    values.entry(key).or_insert_with(|| args.to_vec());
                }
                _ => {}
            }
        }
        Ok(())
    }
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
        let user_base = home()?.join(".ssh");
        let sources = custom.map(|p| vec![p.to_path_buf()]).unwrap_or_else(|| {
            vec![
                user_base.join("config"),
                PathBuf::from("/etc/ssh/ssh_config"),
            ]
        });
        let mut values = HashMap::new();
        if override_user {
            values.insert("user".into(), vec![user.into()]);
        }
        if override_port {
            values.insert("port".into(), vec![port.to_string()]);
        }
        let local_user = std::env::var("USER").unwrap_or_else(|_| user.into());
        let mut wants_final = false;
        for final_pass in [false, true] {
            if final_pass && !wants_final {
                break;
            }
            let resolved = values
                .get("hostname")
                .and_then(|v| v.first())
                .cloned()
                .unwrap_or_else(|| host.into());
            let resolved_user = values
                .get("user")
                .and_then(|v| v.first())
                .cloned()
                .unwrap_or_else(|| user.into());
            let mut eval = Evaluation {
                original: host,
                host: &resolved,
                user: &resolved_user,
                local_user: &local_user,
                final_pass,
                wants_final: false,
                active: true,
            };
            for (index, source) in sources.iter().enumerate() {
                eval.active = true;
                let base = if custom.is_none() && index == 1 {
                    Path::new("/etc/ssh")
                } else {
                    &user_base
                };
                eval.read(source, base, 0, &mut values, custom.is_some())?;
            }
            wants_final |= eval.wants_final;
        }
        for (key, default) in [
            ("hostname", host.to_string()),
            ("user", user.to_string()),
            ("port", port.to_string()),
            ("stricthostkeychecking", "accept-new".into()),
            ("hashknownhosts", "no".into()),
            ("checkhostip", "no".into()),
        ] {
            values.entry(key.into()).or_insert(vec![default]);
        }
        values.entry("userknownhostsfile".into()).or_insert(vec![
            "~/.ssh/known_hosts".into(),
            "~/.ssh/known_hosts2".into(),
        ]);
        values.entry("globalknownhostsfile".into()).or_insert(vec![
            "/etc/ssh/ssh_known_hosts".into(),
            "/etc/ssh/ssh_known_hosts2".into(),
        ]);
        for key in [
            "hostname",
            "user",
            "port",
            "stricthostkeychecking",
            "hashknownhosts",
            "checkhostip",
            "hostkeyalias",
            "hostkeyalgorithms",
            "revokedhostkeys",
            "verifyhostkeydns",
            "canonicalizehostname",
        ] {
            if values.get(key).is_some_and(|v| v.len() != 1) {
                return Err(format!("SSH {key} requires exactly one value"));
            }
        }
        for key in ["hashknownhosts", "checkhostip"] {
            let value = &mut values.get_mut(key).unwrap()[0];
            *value = match value.to_lowercase().as_str() {
                "yes" | "true" => "yes",
                "no" | "false" => "no",
                _ => return Err(format!("invalid SSH {key} value")),
            }
            .into();
        }
        let policy = &mut values.get_mut("stricthostkeychecking").unwrap()[0];
        *policy = policy.to_lowercase();
        if !matches!(
            policy.as_str(),
            "yes" | "true" | "ask" | "accept-new" | "no" | "false" | "off"
        ) {
            return Err(format!("invalid StrictHostKeyChecking value: {policy}"));
        }
        // Never silently bypass explicitly configured trust restrictions.
        for key in ["casignaturealgorithms", "requiredrsasize"] {
            if values.contains_key(key) {
                return Err(format!(
                    "SSH {key} integration is not implemented by Jetpack"
                ));
            }
        }
        for key in ["proxycommand", "proxyjump", "knownhostscommand"] {
            if values
                .get(key)
                .is_some_and(|v| v.iter().any(|s| s != "none"))
            {
                return Err(format!(
                    "SSH {key} integration is not implemented by Jetpack"
                ));
            }
        }
        for key in ["verifyhostkeydns", "canonicalizehostname"] {
            if values
                .get(key)
                .is_some_and(|v| !matches!(v[0].as_str(), "no" | "false"))
            {
                return Err(format!(
                    "SSH {key} integration is not implemented by Jetpack"
                ));
            }
        }
        let hostname = values["hostname"][0].replace("%h", host).replace("%%", "%");
        let user = values["user"][0].clone();
        let port = values["port"][0]
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or("invalid resolved SSH port")?;
        values.insert("hostname".into(), vec![hostname.clone()]);
        for key in [
            "userknownhostsfile",
            "globalknownhostsfile",
            "revokedhostkeys",
        ] {
            if let Some(paths) = values.get_mut(key) {
                for path in paths {
                    if path == "none" {
                        continue;
                    }
                    let mut expanded = String::new();
                    let mut chars = path.chars();
                    while let Some(c) = chars.next() {
                        if c == '%' {
                            expanded.push_str(&match chars
                                .next()
                                .ok_or("incomplete SSH path token")?
                            {
                                '%' => "%".into(),
                                'h' => hostname.clone(),
                                'n' => host.into(),
                                'p' => port.to_string(),
                                'r' => user.clone(),
                                'u' => local_user.clone(),
                                'd' => home()?.to_string_lossy().into_owned(),
                                token => {
                                    return Err(format!("unsupported SSH path token %{token}"));
                                }
                            });
                        } else if c == '$' && chars.clone().next() == Some('{') {
                            chars.next();
                            let mut variable = String::new();
                            loop {
                                match chars.next() {
                                    Some('}') => break,
                                    Some(c) => variable.push(c),
                                    None => {
                                        return Err(
                                            "unterminated SSH path environment variable".into()
                                        );
                                    }
                                }
                            }
                            expanded.push_str(&std::env::var(&variable).map_err(|_| {
                                format!("SSH path environment variable {variable} is unset")
                            })?);
                        } else {
                            expanded.push(c);
                        }
                    }
                    let expanded = tilde(&expanded)?;
                    *path = if expanded.is_absolute() {
                        expanded
                    } else {
                        std::env::current_dir()
                            .map_err(|e| e.to_string())?
                            .join(expanded)
                    }
                    .to_string_lossy()
                    .into_owned();
                }
            }
        }
        Ok(Self {
            hostname,
            user,
            port,
            values,
            sources,
        })
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .and_then(|v| v.first())
            .map(String::as_str)
    }
    pub fn files(&self, key: &str) -> Result<Vec<PathBuf>, String> {
        let values = self.values.get(key).cloned().unwrap_or_default();
        if values.iter().any(|v| v == "none") {
            if values.len() != 1 {
                return Err(format!("SSH {key}: none cannot be combined with filenames"));
            }
            return Ok(Vec::new());
        }
        Ok(values.into_iter().map(PathBuf::from).collect())
    }
    // Only the pre-existing, explicitly requested ssh -A path needs a helper.
    pub fn forwarding_file(&self) -> Result<NamedTempFile, String> {
        let mut file = NamedTempFile::new().map_err(|e| e.to_string())?;
        writeln!(file, "Host *").map_err(|e| e.to_string())?;
        for key in [
            "hostname",
            "user",
            "port",
            "stricthostkeychecking",
            "userknownhostsfile",
            "globalknownhostsfile",
            "hostkeyalias",
            "hashknownhosts",
            "checkhostip",
            "hostkeyalgorithms",
            "revokedhostkeys",
        ] {
            if let Some(values) = self.values.get(key) {
                let values = values
                    .iter()
                    .map(|v| quote(v))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(" ");
                writeln!(file, " {key} {values}").map_err(|e| e.to_string())?;
            }
        }
        for path in &self.sources {
            if path.exists() {
                writeln!(file, "Include {}", quote(&path.to_string_lossy())?)
                    .map_err(|e| e.to_string())?;
            }
        }
        file.flush().map_err(|e| e.to_string())?;
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn resolve(path: &Path) -> Result<SshConfig, String> {
        SshConfig::resolve("alias", "default-user", 22, false, false, Some(path))
    }
    #[test]
    fn native_config_respects_include_first_value_final_match_and_overrides() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config");
        let includes = root.path().join("parts");
        std::fs::create_dir(&includes).unwrap();
        std::fs::write(includes.join("a.conf"), "Host alias !excluded\n HostName 127.0.0.1\n Port=2222\n User configured\nHost other\n StrictHostKeyChecking no\n").unwrap();
        std::fs::write(includes.join("b.conf"), "Host alias\n Port 9999\n").unwrap();
        std::fs::write(&path, format!("Include {}/*.conf\nHost alias\n UserKnownHostsFile {} {}\n GlobalKnownHostsFile none\nMatch final host 127.0.0.1 originalhost alias user configured\n StrictHostKeyChecking yes\n", includes.display(), quote(root.path().join("known hosts").to_str().unwrap()).unwrap(), quote(root.path().join("%n-%p-%r").to_str().unwrap()).unwrap())).unwrap();
        let config = resolve(&path).unwrap();
        assert_eq!(config.hostname, "127.0.0.1");
        assert_eq!(config.user, "configured");
        assert_eq!(config.port, 2222);
        assert_eq!(config.get("stricthostkeychecking"), Some("yes"));
        assert_eq!(
            config.files("userknownhostsfile").unwrap(),
            vec![
                root.path().join("known hosts"),
                root.path().join("alias-2222-configured")
            ]
        );
        assert!(config.files("globalknownhostsfile").unwrap().is_empty());
        let overridden =
            SshConfig::resolve("alias", "explicit", 2223, true, true, Some(&path)).unwrap();
        assert_eq!(overridden.user, "explicit");
        assert_eq!(overridden.port, 2223);
        assert_eq!(overridden.get("stricthostkeychecking"), Some("accept-new"));
    }
    #[test]
    fn config_defaults_quoting_negation_and_invalid_trust_options() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config");
        std::fs::write(&path, "Host * !alias\n StrictHostKeyChecking no\nHost alias\n HashKnownHosts YES\n CheckHostIP true\n UserKnownHostsFile \"relative path\" # comment\n").unwrap();
        let config = resolve(&path).unwrap();
        assert_eq!(config.get("stricthostkeychecking"), Some("accept-new"));
        assert_eq!(config.get("hashknownhosts"), Some("yes"));
        assert_eq!(config.get("checkhostip"), Some("yes"));
        assert_eq!(
            config.files("userknownhostsfile").unwrap(),
            vec![std::env::current_dir().unwrap().join("relative path")]
        );
        for line in [
            "Match exec true",
            "Match localnetwork 192.0.2.0/24",
            "Host *\n CheckHostIP perhaps",
            "Host *\n ProxyJump bastion",
            "Host *\n KnownHostsCommand helper",
            "Host *\n VerifyHostKeyDNS yes",
            "Host *\n StrictHostKeyChecking yes no",
            "Host *\n UserKnownHostsFile \"unterminated",
            "Host *\n UserKnownHostsFile %x/hosts",
            "Host *\n CanonicalizeHostname yes",
        ] {
            std::fs::write(&path, line).unwrap();
            assert!(resolve(&path).is_err(), "{line}");
        }
        std::fs::write(&path, format!("Include {}", path.display())).unwrap();
        assert!(resolve(&path).err().unwrap().contains("nesting"));
    }
    #[test]
    fn supported_resolution_agrees_with_openssh() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config");
        std::fs::write(&path, "Host * !excluded\n HostName example.test\n User configured\n Port 2200\n HashKnownHosts yes\n CheckHostIP yes\n HostKeyAlias trust-name\nMatch user CONFIGURED\n StrictHostKeyChecking no\nMatch final originalhost alias\n StrictHostKeyChecking yes\n").unwrap();
        let native = resolve(&path).unwrap();
        let output = std::process::Command::new("ssh")
            .arg("-G")
            .arg("-F")
            .arg(&path)
            .arg("alias")
            .output()
            .unwrap();
        assert!(output.status.success());
        let output = String::from_utf8(output.stdout).unwrap();
        for key in [
            "hostname",
            "user",
            "port",
            "hashknownhosts",
            "checkhostip",
            "hostkeyalias",
        ] {
            let line = output
                .lines()
                .find(|line| line.starts_with(&format!("{key} ")))
                .unwrap();
            assert_eq!(
                native.get(key),
                line.split_once(' ').map(|(_, value)| value),
                "{key}"
            );
        }
        assert_eq!(native.get("stricthostkeychecking"), Some("yes"));
        assert!(output.lines().any(|s| s == "stricthostkeychecking true"));
    }
}
