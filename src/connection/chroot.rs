// Jetpack
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
// Copyright (C) 2026 - Riff.CC <code@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// long with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Chroot connection — executes commands inside a chroot environment.
//!
//! All commands are wrapped with `chroot <path> sh -c "..."`.
//! File writes target `<chroot_path>/<remote_path>` on the host filesystem.
//! This is used by `jetp pull --chroot <path>` for provisioning
//! OS images before first boot.

use crate::connection::connection::Connection;
use crate::connection::command::{CommandResult, Forward};
use crate::connection::factory::ConnectionFactory;
use crate::connection::local::convert_out;
use crate::playbooks::context::PlaybookContext;
use crate::inventory::hosts::Host;
use crate::handle::response::Response;
use crate::tasks::{TaskRequest, TaskResponse};
use crate::Inventory;
use crate::util::io::jet_file_open;

use std::sync::{Arc, Mutex, RwLock};
use std::process::Command;
use std::path::Path;
use std::fs::File;
use std::io::Write;

/// Factory that creates ChrootConnection instances.
///
/// Like LocalFactory, a single connection is shared across all hosts
/// since everything runs on the local machine (inside the chroot).
pub struct ChrootFactory {
    local_connection: Arc<Mutex<dyn Connection>>,
    inventory: Arc<RwLock<Inventory>>,
}

impl ChrootFactory {
    pub fn new(inventory: &Arc<RwLock<Inventory>>, chroot_path: String) -> Self {
        let host = inventory.read().expect("inventory read").get_host(&String::from("localhost"));
        let mut cc = ChrootConnection::new(&host, chroot_path);
        cc.connect().expect("chroot connection ok");
        Self {
            inventory: Arc::clone(inventory),
            local_connection: Arc::new(Mutex::new(cc)),
        }
    }
}

impl ConnectionFactory for ChrootFactory {
    fn get_connection(
        &self,
        _context: &Arc<RwLock<PlaybookContext>>,
        host: &Arc<RwLock<Host>>,
    ) -> Result<Arc<Mutex<dyn Connection>>, String> {
        // Copy OS type from detected chroot OS to all hosts
        {
            let localhost = self.inventory.read().expect("inventory read").get_host(&String::from("localhost"));
            let localhost_os = localhost.read().expect("localhost read").os_type;
            if let Some(os_type) = localhost_os {
                let mut host_write = host.write().expect("host write");
                if host_write.os_type.is_none() {
                    host_write.os_type = Some(os_type);
                }
            }
        }
        Ok(Arc::clone(&self.local_connection))
    }

    fn get_local_connection(
        &self,
        _context: &Arc<RwLock<PlaybookContext>>,
    ) -> Result<Arc<Mutex<dyn Connection>>, String> {
        Ok(Arc::clone(&self.local_connection))
    }
}

/// Connection that executes commands inside a chroot.
pub struct ChrootConnection {
    host: Arc<RwLock<Host>>,
    chroot_path: String,
}

impl ChrootConnection {
    pub fn new(host: &Arc<RwLock<Host>>, chroot_path: String) -> Self {
        Self {
            host: Arc::clone(host),
            chroot_path,
        }
    }

    /// Translate a remote path to the actual host path inside the chroot.
    fn resolve_path(&self, remote_path: &str) -> String {
        if remote_path.starts_with('/') {
            format!("{}{}", self.chroot_path, remote_path)
        } else {
            format!("{}/{}", self.chroot_path, remote_path)
        }
    }

    fn trim_newlines(&self, s: &mut String) {
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
    }
}

impl Connection for ChrootConnection {
    fn whoami(&self) -> Result<String, String> {
        // Inside a chroot we're always root
        Ok(String::from("root"))
    }

    fn connect(&mut self) -> Result<(), String> {
        // Detect the OS inside the chroot
        let result = detect_chroot_os(&self.host, &self.chroot_path);
        match result {
            Ok(()) => Ok(()),
            Err((_rc, out)) => Err(out),
        }
    }

    fn run_command(
        &self,
        response: &Arc<Response>,
        request: &Arc<TaskRequest>,
        cmd: &String,
        _forward: Forward,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let mut base = Command::new("chroot");
        let cmd2 = format!("LANG=C {}", cmd);
        let command = base
            .arg(&self.chroot_path)
            .arg("sh")
            .arg("-c")
            .arg(&cmd2);

        match command.output() {
            Ok(x) => match x.status.code() {
                Some(rc) => {
                    let mut out = convert_out(&x.stdout, &x.stderr);
                    self.trim_newlines(&mut out);
                    Ok(response.command_ok(
                        request,
                        &Arc::new(Some(CommandResult {
                            cmd: cmd.clone(),
                            out: out.clone(),
                            rc,
                        })),
                    ))
                }
                None => Err(response.command_failed(
                    request,
                    &Arc::new(Some(CommandResult {
                        cmd: cmd.clone(),
                        out: String::from(""),
                        rc: 418,
                    })),
                )),
            },
            Err(_x) => Err(response.command_failed(
                request,
                &Arc::new(Some(CommandResult {
                    cmd: cmd.clone(),
                    out: String::from(""),
                    rc: 404,
                })),
            )),
        }
    }

    fn copy_file(
        &self,
        response: &Arc<Response>,
        request: &Arc<TaskRequest>,
        src: &Path,
        remote_path: &String,
    ) -> Result<(), Arc<TaskResponse>> {
        let actual_dest = self.resolve_path(remote_path);
        let dest_path = Path::new(&actual_dest);

        // Ensure parent directory exists
        if let Some(parent) = dest_path.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return Err(response.is_failed(
                        request,
                        &format!("mkdir failed for {}: {:?}", parent.display(), e),
                    ));
                }
            }
        }

        match std::fs::copy(src, dest_path) {
            Ok(_) => Ok(()),
            Err(e) => Err(response.is_failed(
                request,
                &format!("copy to chroot failed: {:?}", e),
            )),
        }
    }

    fn write_data(
        &self,
        response: &Arc<Response>,
        request: &Arc<TaskRequest>,
        data: &String,
        remote_path: &String,
    ) -> Result<(), Arc<TaskResponse>> {
        let actual_path = self.resolve_path(remote_path);
        let path = Path::new(&actual_path);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return Err(response.is_failed(
                        request,
                        &format!("mkdir failed for {}: {:?}", parent.display(), e),
                    ));
                }
            }
        }

        let mut file = if path.exists() {
            match jet_file_open(path) {
                Ok(x) => x,
                Err(y) => {
                    return Err(response.is_failed(
                        request,
                        &format!("failed to open: {}: {:?}", actual_path, y),
                    ))
                }
            }
        } else {
            match File::create(path) {
                Ok(x) => x,
                Err(y) => {
                    return Err(response.is_failed(
                        request,
                        &format!("failed to create: {}: {:?}", actual_path, y),
                    ))
                }
            }
        };

        match write!(file, "{}", data) {
            Ok(_) => Ok(()),
            Err(y) => Err(response.is_failed(
                request,
                &format!("failed to write: {}: {:?}", actual_path, y),
            )),
        }
    }
}

/// Detect OS inside the chroot by running `chroot <path> uname -a`.
fn detect_chroot_os(
    host: &Arc<RwLock<Host>>,
    chroot_path: &str,
) -> Result<(), (i32, String)> {
    let mut base = Command::new("chroot");
    let command = base.arg(chroot_path).arg("uname").arg("-a");

    match command.output() {
        Ok(x) => match x.status.code() {
            Some(0) => {
                let out = convert_out(&x.stdout, &x.stderr);
                match host.write().unwrap().set_os_info(&out) {
                    Ok(_) => Ok(()),
                    Err(_) => Err((500, String::from("failed to set OS info from chroot"))),
                }
            }
            Some(status) => Err((status, convert_out(&x.stdout, &x.stderr))),
            _ => Err((418, String::from("chroot uname -a failed without status code"))),
        },
        Err(_x) => Err((418, String::from("chroot uname -a failed"))),
    }
}
