// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! `.jetpack.yml` / `.jetpack.yaml` — the versioned automation contract.
//!
//! A contract at the automation root is `Cargo.toml` for ops: it declares the
//! playbook, inventory, and role paths the root runs by default, so `jetp ssh`
//! needs no flags. This module owns the schema, the legacy `local:` back-compat
//! alias, the schema-version gate, and the search order that locates the file.
//!
//! `version:` is a **schema version, not a file-format tag**: it gates the body
//! parse. A future-version file is rejected up front ("upgrade Jetpack") by the
//! lenient [`probe_version`] probe *before* the strict within-version
//! [`deserialize`] runs — so `deny_unknown_fields` catches v1 typos without ever
//! being exposed to a v2 file's as-yet-unknown keys. That keeps the contract
//! forward-compatible: a vendored, pinned contract never trips a confusing
//! "unknown field" on a newer schema it simply doesn't implement yet.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The schema version this Jetpack implements. A contract may omit `version:`
/// (treated as this version, for back-compat with marker-only files); any other
/// value is rejected by [`validate_version`] before the body is parsed.
pub(super) const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// The strict, within-version contract. `deny_unknown_fields` turns typos into
/// clear errors — but it only ever sees files that already passed the version
/// gate, so it is never a forward-compatibility hazard.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct JetpackFileConfig {
    pub(super) version: Option<u32>,
    pub(super) defaults: Option<JetpackDefaults>,
    /// Legacy back-compat alias (scalar values). Promoted into the defaults
    /// shape by [`JetpackFileConfig::effective`] when `defaults` is absent.
    pub(super) local: Option<JetpackLocalConfig>,
    /// Named operator-truth presets. A profile (selected via `--profile` or
    /// `defaults.profile`) overrides `inventory` + `secrets_inventory` only.
    pub(super) profiles: Option<BTreeMap<String, JetpackProfile>>,
    /// External automation source. **Informational only** for now — parsed so
    /// the contract validates, but Jetpack does not fetch it yet.
    pub(super) automation: Option<JetpackAutomation>,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct JetpackDefaults {
    pub(super) playbook: Option<String>,
    pub(super) inventory: Option<Vec<String>>,
    /// Operator-supplied secrets overlay (often gitignored / a sibling repo),
    /// layered on top of `inventory` at load time.
    pub(super) secrets_inventory: Option<Vec<String>>,
    pub(super) roles: Option<Vec<String>>,
    /// The default profile to activate when `--profile` is not given.
    pub(super) profile: Option<String>,
}

/// A named operator-truth preset. When active, a profile overrides only
/// `inventory` and `secrets_inventory` (playbook/roles stay from `defaults`).
/// `None` means "inherit defaults"; `Some(vec![])` means "set to none".
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct JetpackProfile {
    pub(super) inventory: Option<Vec<String>>,
    pub(super) secrets_inventory: Option<Vec<String>>,
}

/// External automation source declaration. **Informational**: parsed and
/// surfaced in the resolution summary, but Jetpack does not yet clone/fetch it.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct JetpackAutomation {
    pub(super) source: Option<String>,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct JetpackLocalConfig {
    pub(super) playbook: Option<String>,
    pub(super) roles: Option<String>,
    pub(super) inventory: Option<String>,
}

/// Defaults after collapsing `defaults` and the legacy `local:` alias into one
/// list-typed shape. Consumed by the file-defaults step (and the local
/// convention fallback) so they share one notion of "what the contract asked for".
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct EffectiveDefaults {
    pub(super) playbook: Option<String>,
    pub(super) inventory: Vec<String>,
    pub(super) secrets_inventory: Vec<String>,
    pub(super) roles: Vec<String>,
    /// The default profile name from `defaults.profile` (the CLI `--profile`
    /// still wins over this; resolution happens in the consumer).
    pub(super) profile: Option<String>,
}

impl JetpackFileConfig {
    /// `defaults` wins outright when present; otherwise `local:` is normalized
    /// (scalars → single-element lists). Returns empty defaults when neither is
    /// set (a marker-only contract). Profile override (applying a selected
    /// profile's inventory/secrets) is the consumer's job, since it depends on
    /// the CLI `--profile` flag.
    pub(super) fn effective(&self) -> EffectiveDefaults {
        if let Some(defaults) = &self.defaults {
            return EffectiveDefaults {
                playbook: defaults.playbook.clone(),
                inventory: defaults.inventory.clone().unwrap_or_default(),
                secrets_inventory: defaults.secrets_inventory.clone().unwrap_or_default(),
                roles: defaults.roles.clone().unwrap_or_default(),
                profile: defaults.profile.clone(),
            };
        }
        match &self.local {
            Some(local) => EffectiveDefaults {
                playbook: local.playbook.clone(),
                inventory: local.inventory.clone().into_iter().collect(),
                secrets_inventory: Vec::new(),
                roles: local.roles.clone().into_iter().collect(),
                profile: None,
            },
            None => EffectiveDefaults::default(),
        }
    }
}

/// Phase 1 of the gate: read ONLY the schema version, leniently. Unknown keys
/// (from a future schema) are deliberately ignored here so they never surface as
/// a parse error — the decision to reject is made on the version, in
/// [`validate_version`].
pub(super) fn probe_version(raw: &str) -> Result<Option<u32>, serde_yaml::Error> {
    #[derive(Deserialize)]
    struct Probe {
        version: Option<u32>,
    }
    Ok(serde_yaml::from_str::<Probe>(raw)?.version)
}

/// Reject unknown schema versions with a forward-looking message before the
/// strict body parse runs. `None` (version omitted) is permitted for back-compat
/// with marker-only contracts.
pub(super) fn validate_version(version: Option<u32>) -> Result<(), String> {
    match version {
        None | Some(SUPPORTED_SCHEMA_VERSION) => Ok(()),
        Some(v) => Err(format!(
            "unsupported .jetpack schema version {v}: this Jetpack implements version \
             {SUPPORTED_SCHEMA_VERSION}. Upgrade Jetpack to read a version {v} contract, or pin \
             the file to version {SUPPORTED_SCHEMA_VERSION}."
        )),
    }
}

/// Phase 2 of the gate: strict within-version parse. Call only after
/// [`validate_version`] has accepted the probed version, so `deny_unknown_fields`
/// catches typos against the pinned schema rather than tripping on a newer file.
pub(super) fn deserialize(raw: &str) -> Result<JetpackFileConfig, serde_yaml::Error> {
    serde_yaml::from_str(raw)
}

/// Resolve the contract file for a run.
///
/// Precedence:
/// 1. An explicit `--config PATH` (authoritative — returned as-is; existence is
///    checked by the caller so a typo surfaces as a clear error, not a silent
///    fall-through to a different contract).
/// 2. An upward walk from `start`, bounded by the automation root, returning the
///    first `.jetpack.yml` and then `.jetpack.yaml` in each directory — so `.yml`
///    wins over `.yaml` when both sit in the same directory.
/// 3. `None` when nothing is found; callers fall back to conventions / CLI-only.
pub(super) fn locate_config(start: &Path, override_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(explicit) = override_path {
        return Some(explicit.to_path_buf());
    }
    let bound = crate::util::repo::detect_automation_root(start);
    for ancestor in start.ancestors() {
        let yml = ancestor.join(".jetpack.yml");
        if yml.is_file() {
            return Some(yml);
        }
        let yaml = ancestor.join(".jetpack.yaml");
        if yaml.is_file() {
            return Some(yaml);
        }
        if ancestor == bound {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn defaults_block_parses_with_lists() {
        let raw = "\
version: 1
defaults:
  playbook: playbooks/install.yml
  inventory:
    - labs/london
    - labs/paris
  roles:
    - roles
";
        let cfg = deserialize(raw).expect("parses");
        assert_eq!(cfg.version, Some(1));
        let eff = cfg.effective();
        assert_eq!(eff.playbook.as_deref(), Some("playbooks/install.yml"));
        assert_eq!(eff.inventory, ["labs/london", "labs/paris"]);
        assert_eq!(eff.roles, ["roles"]);
    }

    #[test]
    fn local_alias_promotes_scalars_to_lists() {
        let raw = "\
local:
  playbook: pb/install.yml
  roles: roles
  inventory: inv
";
        let cfg = deserialize(raw).expect("parses");
        let eff = cfg.effective();
        assert_eq!(eff.playbook.as_deref(), Some("pb/install.yml"));
        assert_eq!(eff.roles, ["roles"]);
        assert_eq!(eff.inventory, ["inv"]);
    }

    #[test]
    fn defaults_wins_over_local_when_both_present() {
        let raw = "\
defaults:
  playbook: from-defaults
local:
  playbook: from-local
";
        let cfg = deserialize(raw).expect("parses");
        assert_eq!(cfg.effective().playbook.as_deref(), Some("from-defaults"));
    }

    #[test]
    fn defaults_secrets_inventory_and_profile_parse_and_effective() {
        let raw = "\
version: 1
defaults:
  playbook: pb/install.yml
  inventory: [labs/london]
  secrets_inventory:
    - ../infra-secrets/london
  roles: [roles]
  profile: london
";
        let cfg = deserialize(raw).expect("parses");
        let eff = cfg.effective();
        assert_eq!(
            eff.secrets_inventory,
            vec!["../infra-secrets/london".to_string()]
        );
        assert_eq!(eff.profile.as_deref(), Some("london"));
    }

    #[test]
    fn profiles_map_parses() {
        let raw = "\
version: 1
profiles:
  london:
    inventory: [labs/london]
    secrets_inventory: [../infra-secrets/london]
  ci:
    inventory: [inventory/ephemeral]
";
        let cfg = deserialize(raw).expect("parses");
        let profiles = cfg.profiles.expect("profiles present");
        let london = profiles.get("london").expect("london profile");
        assert_eq!(
            london.inventory.clone(),
            Some(vec!["labs/london".to_string()])
        );
        assert_eq!(
            london.secrets_inventory.clone(),
            Some(vec!["../infra-secrets/london".to_string()])
        );
        let ci = profiles.get("ci").expect("ci profile");
        assert_eq!(
            ci.inventory.clone(),
            Some(vec!["inventory/ephemeral".to_string()])
        );
        assert!(
            ci.secrets_inventory.is_none(),
            "ci has no secrets (inherits)"
        );
    }

    #[test]
    fn profile_secrets_empty_vec_is_distinct_from_absent() {
        // Some([]) means "clear secrets"; None means "inherit defaults". The
        // schema must preserve the distinction so a profile can opt out.
        let with_empty = "\
profiles:
  clean:
    secrets_inventory: []
";
        let cfg = deserialize(with_empty).expect("parses");
        let profiles = cfg.profiles.unwrap();
        let clean = profiles.get("clean").unwrap();
        assert_eq!(clean.secrets_inventory.clone(), Some(vec![]));

        let without = "\
profiles:
  inherit:
    inventory: [x]
";
        let cfg2 = deserialize(without).expect("parses");
        let profiles2 = cfg2.profiles.unwrap();
        let inherit = profiles2.get("inherit").unwrap();
        assert!(inherit.secrets_inventory.is_none());
    }

    #[test]
    fn automation_source_parses() {
        let raw = "\
version: 1
automation:
  source: https://github.com/riffcc/moosefs-automation
";
        let cfg = deserialize(raw).expect("parses");
        assert_eq!(
            cfg.automation.unwrap().source.as_deref(),
            Some("https://github.com/riffcc/moosefs-automation")
        );
    }

    #[test]
    fn deny_unknown_fields_still_rejects_typos_with_new_fields() {
        // Adding new v1 fields must not weaken typo detection.
        assert!(deserialize("version: 1\nplayboook: oops\n").is_err());
        assert!(deserialize("version: 1\ndefaults:\n  inventroy: [x]\n").is_err());
        assert!(deserialize("version: 1\nprofiles:\n  x:\n    inventry: [y]\n").is_err());
    }

    #[test]
    fn version_only_contract_yields_empty_defaults() {
        let cfg = deserialize("version: 1\n").expect("parses");
        assert_eq!(cfg.version, Some(1));
        assert_eq!(cfg.effective(), EffectiveDefaults::default());
    }

    #[test]
    fn probe_reads_version_leniently_ignoring_unknown_keys() {
        // a v2 file carrying an as-yet-unknown `profiles:` key must probe cleanly
        // so the version gate — not deny_unknown_fields — decides its fate
        assert_eq!(
            probe_version("version: 2\nprofiles: [prod]\n").unwrap(),
            Some(2)
        );
        assert_eq!(probe_version("defaults:\n  playbook: x\n").unwrap(), None);
    }

    #[test]
    fn validate_version_accepts_omitted_and_one() {
        assert!(validate_version(None).is_ok());
        assert!(validate_version(Some(1)).is_ok());
    }

    #[test]
    fn validate_version_rejects_future_schema() {
        let err = validate_version(Some(2)).unwrap_err();
        assert!(
            err.contains("version 2"),
            "message names the version: {err}"
        );
        assert!(
            err.contains("Upgrade Jetpack"),
            "message points to upgrade: {err}"
        );
    }

    #[test]
    fn deserialize_rejects_unknown_field_within_version() {
        // a v1 file with a typo'd key → strict parse catches it (typo detection)
        assert!(deserialize("version: 1\nplayboook: oops\n").is_err());
    }

    #[test]
    fn locate_config_honors_explicit_override() {
        let dir = TempDir::new().unwrap();
        // override is authoritative even when it doesn't exist — existence is the
        // caller's concern, so a typo surfaces as "file not found", not a silent
        // fall-through to an auto-discovered contract
        let explicit = dir.path().join("elsewhere.yml");
        assert_eq!(
            locate_config(dir.path(), Some(&explicit)),
            Some(explicit.clone())
        );
    }

    #[test]
    fn locate_config_walks_up_to_jetpack_yml() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(".jetpack.yml"), "").unwrap();
        let deep = root.path().join("a").join("b");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            locate_config(&deep, None),
            Some(root.path().join(".jetpack.yml"))
        );
    }

    #[test]
    fn locate_config_finds_jetpack_yaml() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(".jetpack.yaml"), "").unwrap();
        let deep = root.path().join("sub");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            locate_config(&deep, None),
            Some(root.path().join(".jetpack.yaml"))
        );
    }

    #[test]
    fn locate_config_prefers_yml_over_yaml_in_same_dir() {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(".jetpack.yml"), "").unwrap();
        fs::write(root.path().join(".jetpack.yaml"), "").unwrap();
        assert_eq!(
            locate_config(root.path(), None),
            Some(root.path().join(".jetpack.yml"))
        );
    }

    #[test]
    fn locate_config_returns_none_when_absent() {
        let root = TempDir::new().unwrap();
        assert_eq!(locate_config(root.path(), None), None);
    }
}
