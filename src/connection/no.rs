// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
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

use crate::connection::command::CommandResult;
use crate::connection::command::Forward;
use crate::connection::connection::Connection;
use crate::connection::factory::ConnectionFactory;
use crate::handle::response::Response;
use crate::inventory::hosts::{Host, HostOSType};
use crate::playbooks::context::PlaybookContext;
use crate::tasks::request::TaskRequest;
use crate::tasks::response::TaskResponse;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;

// the noconnection and nofactory are not really used in normal execution of jet, but are around in the "__simulate" hidden
// suboption, as this is occasionally useful for testing certain jet internals.  This is not meant for serious work.

pub struct NoFactory {}

impl Default for NoFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl NoFactory {
    pub fn new() -> Self {
        Self {}
    }
}

impl ConnectionFactory for NoFactory {
    fn get_connection(
        &self,
        _context: &Arc<RwLock<PlaybookContext>>,
        host: &Arc<RwLock<Host>>,
    ) -> Result<Arc<Mutex<dyn Connection>>, String> {
        // we just pretend everything is Linux for now
        host.write().unwrap().os_type = Some(HostOSType::Linux);
        let conn: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(NoConnection::new()));
        Ok(conn)
    }
    fn get_local_connection(
        &self,
        _context: &Arc<RwLock<PlaybookContext>>,
    ) -> Result<Arc<Mutex<dyn Connection>>, String> {
        let conn: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(NoConnection::new()));
        Ok(conn)
    }
}

pub struct NoConnection {}

impl Default for NoConnection {
    fn default() -> Self {
        Self::new()
    }
}

impl NoConnection {
    pub fn new() -> Self {
        Self {}
    }
}

impl Connection for NoConnection {
    fn whoami(&self) -> Result<String, String> {
        // we don't really bother with saying what username is connected
        Ok(String::from("root"))
    }

    fn connect(&mut self) -> Result<(), String> {
        // all connections are imaginary so there's nothing to do
        Ok(())
    }

    fn run_command(
        &self,
        response: &Arc<Response>,
        request: &Arc<TaskRequest>,
        cmd: &str,
        _forward: Forward,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        // all commands return junk output pretending they were successful
        Ok(response.command_ok(
            request,
            &Arc::new(Some(CommandResult {
                cmd: cmd.to_string(),
                out: String::from("__simulated__"),
                rc: 0,
            })),
        ))
    }

    fn write_data(
        &self,
        _response: &Arc<Response>,
        _request: &Arc<TaskRequest>,
        _data: &str,
        _remote_path: &str,
    ) -> Result<(), Arc<TaskResponse>> {
        // no data is transferred, we just pretend things were successful
        Ok(())
    }

    fn copy_file(
        &self,
        _response: &Arc<Response>,
        _request: &Arc<TaskRequest>,
        _src: &Path,
        _dest: &str,
    ) -> Result<(), Arc<TaskResponse>> {
        // no data is transferred, as per above
        Ok(())
    }

    fn fetch_file(
        &self,
        _response: &Arc<Response>,
        _request: &Arc<TaskRequest>,
        _remote_path: &str,
    ) -> Result<Vec<u8>, Arc<TaskResponse>> {
        // simulation mode — return empty bytes
        Ok(Vec::new())
    }
}
