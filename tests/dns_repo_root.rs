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

//! Acceptance test for #46: DNS zone files must land under the automation-repo
//! root, not the playbook's directory. Reproduces the original bug — a playbook
//! in `playbooks/gravity/` declaring `dns.path: "dns/riff.cc"` used to write
//! zone YAML into `playbooks/gravity/dns/riff.cc/zones/` because every path was
//! resolved against the bare process working directory.

use serde_yaml::{Mapping, Value};
use std::fs;
use tempfile::TempDir;

#[test]
fn dns_record_lands_at_automation_root_not_playbook_dir() {
    let repo = TempDir::new().unwrap();
    // a marker so detection, started from a subdirectory, walks up to here
    fs::write(repo.path().join(".jetpack.yml"), "").unwrap();
    // the playbook lives in a nested directory; traversal starts detection here
    let playbook_dir = repo.path().join("playbooks").join("gravity");
    fs::create_dir_all(&playbook_dir).unwrap();

    // exactly what playbooks::traversal does: detect the repo root, then build
    // the DNS config from host vars anchored to that root.
    let automation_root = jetpack::util::repo::detect_automation_root(&playbook_dir);
    assert_eq!(
        automation_root.canonicalize().unwrap(),
        repo.path().canonicalize().unwrap(),
        "detection must walk up from the playbook dir to the repo root"
    );

    // host vars declare a *relative* dns path — the bug scenario
    let mut dns_block = Mapping::new();
    dns_block.insert(
        Value::String("path".to_string()),
        Value::String("dns/riff.cc".to_string()),
    );
    // disable provider sync so the test never shells out to octodns/gravity
    dns_block.insert(Value::String("auto_sync".to_string()), Value::Bool(false));
    let dns = jetpack::dns::dns_config_from_vars(&Value::Mapping(dns_block), &automation_root)
        .expect("dns block deserializes");

    jetpack::dns::add_host_record(&dns, "gravity01.riff.cc", "10.0.0.5")
        .expect("record write succeeds");

    // the zone file MUST be under the repo root …
    let zone_file = repo.path().join("dns/riff.cc/zones/riff.cc.yaml");
    assert!(
        zone_file.is_file(),
        "zone file should land at the repo root: {}",
        zone_file.display()
    );
    // … and MUST NOT silently land next to the playbook (the old bug)
    let wrong = playbook_dir.join("dns/riff.cc/zones/riff.cc.yaml");
    assert!(
        !wrong.exists(),
        "zone file must not land in the playbook directory: {}",
        wrong.display()
    );

    let content = fs::read_to_string(&zone_file).unwrap();
    assert!(
        content.contains("gravity01"),
        "hostname recorded: {content}"
    );
    assert!(content.contains("10.0.0.5"), "ip recorded: {content}");
}
