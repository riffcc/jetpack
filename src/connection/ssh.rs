// Jetpack
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
// Copyright (C) 2025 - Riff.CC <https://riff.cc>
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
use crate::connection::local::convert_out;
use crate::connection::local::LocalFactory;
use crate::handle::response::Response;
use crate::inventory::hosts::Host;
use crate::playbooks::context::PlaybookContext;
use crate::tasks::*;
use crate::Inventory;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use russh::client::{self, AuthResult};
use russh::ChannelMsg;
use tokio::io::AsyncWriteExt;
use tokio::runtime::Runtime;

// Minimal handler for russh client — accepts all host keys (same as ssh2 default)
struct SshHandler;

impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

// SSH connection factory — creates and caches SSH connections

pub struct SshFactory {
    local_factory: LocalFactory,
    localhost: Arc<RwLock<Host>>,
    forward_agent: bool,
    login_password: Option<String>,
}

impl SshFactory {
    pub fn new(
        inventory: &Arc<RwLock<Inventory>>,
        forward_agent: bool,
        login_password: Option<String>,
    ) -> Self {
        Self {
            localhost: inventory
                .read()
                .expect("inventory read")
                .get_host(&String::from("localhost")),
            local_factory: LocalFactory::new(inventory),
            forward_agent,
            login_password,
        }
    }
}

impl ConnectionFactory for SshFactory {
    fn get_local_connection(
        &self,
        context: &Arc<RwLock<PlaybookContext>>,
    ) -> Result<Arc<Mutex<dyn Connection>>, String> {
        return Ok(self
            .local_factory
            .get_connection(context, &self.localhost)?);
    }

    fn get_connection(
        &self,
        context: &Arc<RwLock<PlaybookContext>>,
        host: &Arc<RwLock<Host>>,
    ) -> Result<Arc<Mutex<dyn Connection>>, String> {
        let ctx = context.read().expect("context read");
        let hostname1 = host.read().expect("host read").name.clone();
        if hostname1.eq("localhost") {
            let conn: Arc<Mutex<dyn Connection>> =
                self.local_factory.get_connection(context, &self.localhost)?;
            return Ok(conn);
        }

        {
            let cache = ctx.connection_cache.read().unwrap();
            if cache.has_connection(host) {
                let conn = cache.get_connection(host);
                return Ok(conn);
            }
        }

        let (hostname2, user, port, key, passphrase, key_comment) =
            ctx.get_ssh_connection_details(host);
        if hostname2.eq("localhost") {
            let conn: Arc<Mutex<dyn Connection>> =
                self.local_factory.get_connection(context, &self.localhost)?;
            return Ok(conn);
        }

        let mut conn = SshConnection::new(
            Arc::clone(host),
            &user,
            port,
            hostname2,
            self.forward_agent,
            self.login_password.clone(),
            key,
            passphrase,
            key_comment,
        );
        return match conn.connect() {
            Ok(_) => {
                let conn2: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(conn));
                ctx.connection_cache
                    .write()
                    .expect("connection cache write")
                    .add_connection(&Arc::clone(host), &Arc::clone(&conn2));
                Ok(conn2)
            }
            Err(x) => Err(x),
        };
    }
}

// SSH connection implementation using russh (async internals, sync interface)

pub struct SshConnection {
    pub host: Arc<RwLock<Host>>,
    pub username: String,
    pub port: i64,
    pub hostname: String,
    pub connected: bool,
    pub forward_agent: bool,
    pub login_password: Option<String>,
    pub key: Option<String>,
    pub passphrase: Option<String>,
    pub key_comment: Option<String>,
    // Async runtime bridge — Mutex for Sync compliance (outer Arc<Mutex<Connection>> prevents contention)
    runtime: Mutex<Runtime>,
    handle: Option<client::Handle<SshHandler>>,
}

impl SshConnection {
    pub fn new(
        host: Arc<RwLock<Host>>,
        username: &String,
        port: i64,
        hostname: String,
        forward_agent: bool,
        login_password: Option<String>,
        key: Option<String>,
        passphrase: Option<String>,
        key_comment: Option<String>,
    ) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime");
        Self {
            host: Arc::clone(&host),
            username: username.clone(),
            port,
            hostname,
            connected: false,
            forward_agent,
            login_password,
            key,
            passphrase,
            key_comment,
            runtime: Mutex::new(runtime),
            handle: None,
        }
    }
}

impl Connection for SshConnection {
    fn whoami(&self) -> Result<String, String> {
        return Ok(self.username.clone());
    }

    fn connect(&mut self) -> Result<(), String> {
        if self.connected {
            return Ok(());
        }

        assert!(!self.host.read().expect("host read").name.eq("localhost"));

        // Resolve address synchronously
        let connect_str = format!("{}:{}", self.hostname, self.port);
        let mut addrs = connect_str
            .to_socket_addrs()
            .map_err(|_| String::from("unable to resolve"))?;
        let addr = addrs
            .next()
            .ok_or_else(|| String::from("unable to resolve(2)"))?;

        // Capture fields for the async block
        let username = self.username.clone();
        let login_password = self.login_password.clone();
        let key_path = self.key.clone();
        let passphrase = self.passphrase.clone();
        let key_comment = self.key_comment.clone();
        let connect_str_err = connect_str.clone();

        let handle = self.runtime.lock().unwrap().block_on(async {
            let config = Arc::new(client::Config::default());

            let mut handle = tokio::time::timeout(
                Duration::from_secs(10),
                client::connect(config, addr, SshHandler),
            )
            .await
            .map_err(|_| {
                format!(
                    "SSH connection attempt failed for {}: timeout",
                    connect_str_err
                )
            })?
            .map_err(|e| {
                format!(
                    "SSH connection attempt failed for {}: {}",
                    connect_str_err, e
                )
            })?;

            // Authenticate — try methods in priority order
            if let Some(ref password) = login_password {
                let result = handle
                    .authenticate_password(&username, password)
                    .await
                    .map_err(|e| {
                        format!(
                            "SSH password authentication failed for user {}: {}",
                            username, e
                        )
                    })?;
                if !matches!(result, AuthResult::Success) {
                    return Err(format!(
                        "SSH password authentication failed for user {}",
                        username
                    ));
                }
            } else if let Some(ref key_file) = key_path {
                let path = Path::new(key_file);
                if !path.exists() {
                    return Err(format!("cannot find designated keyfile {}", key_file));
                }
                let secret_key =
                    russh::keys::load_secret_key(path, passphrase.as_deref()).map_err(|e| {
                        format!("SSH key load failed for {}: {}", key_file, e)
                    })?;
                let key_with_alg =
                    russh::keys::PrivateKeyWithHashAlg::new(Arc::new(secret_key), None);
                let result = handle
                    .authenticate_publickey(&username, key_with_alg)
                    .await
                    .map_err(|e| {
                        format!(
                            "SSH key authentication failed for user {} with key {:?}: {}",
                            username, key_file, e
                        )
                    })?;
                if !matches!(result, AuthResult::Success) {
                    return Err(format!(
                        "SSH key authentication failed for user {} with key {}",
                        username, key_file
                    ));
                }
            } else if let Some(ref comment) = key_comment {
                // Use specific key from SSH agent by comment
                let ssh_auth_sock = std::env::var("SSH_AUTH_SOCK")
                    .map_err(|_| format!("SSH cannot connect to agent: SSH_AUTH_SOCK not set"))?;
                let stream = tokio::net::UnixStream::connect(&ssh_auth_sock)
                    .await
                    .map_err(|e| format!("SSH cannot connect to agent: {}", e))?;
                let mut agent = russh::keys::agent::client::AgentClient::connect(stream);
                let identities = agent.request_identities().await.map_err(|e| {
                    format!(
                        "SSH list_identities returned an error, please check whether agent is running: {}",
                        e
                    )
                })?;

                let mut found = false;
                for identity in &identities {
                    if identity.comment() == comment {
                        let result = handle
                            .authenticate_publickey_with(&username, identity.clone(), None, &mut agent)
                            .await
                            .map_err(|e| {
                                format!(
                                    "SSH Key authentication failed for user {} with key {}: {}",
                                    username, comment, e
                                )
                            })?;
                        if matches!(result, AuthResult::Success) {
                            found = true;
                            break;
                        }
                    }
                }
                if !found {
                    return Err(format!(
                        "specified SSH key not found with comment {}",
                        comment
                    ));
                }
            } else {
                // Use any key from SSH agent
                let ssh_auth_sock = std::env::var("SSH_AUTH_SOCK")
                    .map_err(|_| format!("SSH cannot connect to agent: SSH_AUTH_SOCK not set"))?;
                let stream = tokio::net::UnixStream::connect(&ssh_auth_sock)
                    .await
                    .map_err(|e| format!("SSH cannot connect to agent: {}", e))?;
                let mut agent = russh::keys::agent::client::AgentClient::connect(stream);
                let identities = agent
                    .request_identities()
                    .await
                    .map_err(|e| format!("SSH agent failed to list identities: {}", e))?;

                let mut authenticated = false;
                for identity in &identities {
                    match handle
                        .authenticate_publickey_with(
                            &username,
                            identity.clone(),
                            None,
                            &mut agent,
                        )
                        .await
                    {
                        Ok(AuthResult::Success) => {
                            authenticated = true;
                            break;
                        }
                        _ => continue,
                    }
                }
                if !authenticated {
                    return Err(format!(
                        "SSH agent authentication failed for user {}",
                        username
                    ));
                }
            }

            Ok::<_, String>(handle)
        })?;

        self.handle = Some(handle);
        self.connected = true;

        // OS detection — run uname on first connect
        let uname_result = self.run_command_low_level(&String::from("uname -a"));
        match uname_result {
            Ok((_rc, out)) => {
                match self.host.write().unwrap().set_os_info(&out.clone()) {
                    Ok(_x) => {}
                    Err(_y) => return Err(format!("failed to set OS info")),
                }
            }
            Err((rc, out)) => {
                return Err(format!(
                    "uname -a command failed: rc={}, out={}",
                    rc, out
                ))
            }
        }

        return Ok(());
    }

    fn run_command(
        &self,
        response: &Arc<Response>,
        request: &Arc<TaskRequest>,
        cmd: &String,
        forward: Forward,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let result = match forward {
            Forward::Yes => match self.forward_agent {
                false => self.run_command_low_level(cmd),
                true => self.run_command_with_ssh_a(cmd),
            },
            Forward::No => self.run_command_low_level(cmd),
        };

        match result {
            Ok((rc, s)) => {
                return Ok(response.command_ok(
                    request,
                    &Arc::new(Some(CommandResult {
                        cmd: cmd.clone(),
                        out: s.clone(),
                        rc: rc,
                    })),
                ));
            }
            Err((rc, s)) => {
                return Err(response.command_failed(
                    request,
                    &Arc::new(Some(CommandResult {
                        cmd: cmd.clone(),
                        out: s.clone(),
                        rc: rc,
                    })),
                ));
            }
        }
    }

    fn write_data(
        &self,
        response: &Arc<Response>,
        request: &Arc<TaskRequest>,
        data: &String,
        remote_path: &String,
    ) -> Result<(), Arc<TaskResponse>> {
        let handle = self.handle.as_ref().expect("session not established");
        let remote_path = remote_path.clone();
        let data_bytes = data.as_bytes().to_vec();

        self.runtime.lock().unwrap().block_on(async {
            let channel = handle
                .channel_open_session()
                .await
                .map_err(|e| response.is_failed(request, &format!("sftp connection failed: {}", e)))?;
            channel
                .request_subsystem(true, "sftp")
                .await
                .map_err(|e| {
                    response.is_failed(request, &format!("sftp subsystem request failed: {}", e))
                })?;
            let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
                .await
                .map_err(|e| {
                    response.is_failed(request, &format!("sftp session failed: {}", e))
                })?;

            let mut file = sftp
                .create(&remote_path)
                .await
                .map_err(|e| response.is_failed(request, &format!("sftp open failed: {}", e)))?;
            file.write_all(&data_bytes)
                .await
                .map_err(|e| response.is_failed(request, &format!("sftp write failed: {}", e)))?;
            file.shutdown()
                .await
                .map_err(|e| response.is_failed(request, &format!("sftp flush failed: {}", e)))?;

            Ok(())
        })
    }

    fn copy_file(
        &self,
        response: &Arc<Response>,
        request: &Arc<TaskRequest>,
        src: &Path,
        remote_path: &String,
    ) -> Result<(), Arc<TaskResponse>> {
        // Read source file into memory (fine for config files, templates, scripts)
        let src_data = std::fs::read(src).map_err(|e| {
            response.is_failed(request, &format!("failed to open source file: {}", e))
        })?;

        let handle = self.handle.as_ref().expect("session not established");
        let remote_path = remote_path.clone();

        self.runtime.lock().unwrap().block_on(async {
            let channel = handle
                .channel_open_session()
                .await
                .map_err(|e| response.is_failed(request, &format!("sftp connection failed: {}", e)))?;
            channel
                .request_subsystem(true, "sftp")
                .await
                .map_err(|e| {
                    response.is_failed(request, &format!("sftp subsystem request failed: {}", e))
                })?;
            let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
                .await
                .map_err(|e| {
                    response.is_failed(request, &format!("sftp session failed: {}", e))
                })?;

            let mut file = sftp
                .create(&remote_path)
                .await
                .map_err(|e| {
                    response.is_failed(request, &format!("sftp write failed (1): {}", e))
                })?;
            file.write_all(&src_data)
                .await
                .map_err(|e| {
                    response.is_failed(request, &format!("sftp copy failed (1): {}", e))
                })?;
            file.shutdown()
                .await
                .map_err(|e| {
                    response.is_failed(request, &format!("sftp close failed: {}", e))
                })?;

            Ok(())
        })
    }
}

impl SshConnection {
    fn trim_newlines(&self, s: &mut String) {
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
    }

    fn run_command_low_level(&self, cmd: &String) -> Result<(i32, String), (i32, String)> {
        let handle = self.handle.as_ref().unwrap();
        let actual_cmd = format!("LANG=C {} 2>&1", cmd);

        self.runtime.lock().unwrap().block_on(async {
            let mut channel = handle
                .channel_open_session()
                .await
                .map_err(|e| (500, format!("channel session failed: {:?}", e)))?;
            channel
                .exec(true, actual_cmd.as_bytes())
                .await
                .map_err(|e| (500, e.to_string()))?;

            let mut output = Vec::new();
            let mut exit_code: Option<u32> = None;

            loop {
                match channel.wait().await {
                    Some(ChannelMsg::Data { ref data }) => {
                        output.extend_from_slice(data);
                    }
                    Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                        output.extend_from_slice(data);
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = Some(exit_status);
                    }
                    Some(ChannelMsg::Eof) => {}
                    None => break,
                    _ => {}
                }
            }

            let mut s = String::from_utf8_lossy(&output).to_string();
            self.trim_newlines(&mut s);
            let rc = exit_code.unwrap_or(0) as i32;
            Ok((rc, s))
        })
    }

    fn run_command_with_ssh_a(&self, cmd: &String) -> Result<(i32, String), (i32, String)> {
        // libssh2/russh agent forwarding is unreliable, so shell out to ssh -A
        let mut base = Command::new("ssh");
        let hostname = &self.host.read().unwrap().name;
        let port = format!("{}", self.port);
        let cmd2 = format!("LANG=C {} 2>&1", cmd);
        let command = base
            .arg(hostname)
            .arg("-p")
            .arg(port)
            .arg("-l")
            .arg(self.username.clone())
            .arg("-A")
            .arg(cmd2);
        match command.output() {
            Ok(x) => match x.status.code() {
                Some(rc) => {
                    let mut out = convert_out(&x.stdout, &x.stderr);
                    self.trim_newlines(&mut out);
                    return Ok((rc, out.clone()));
                }
                None => {
                    return Ok((418, String::from("")));
                }
            },
            Err(_x) => {
                return Err((404, String::from("")));
            }
        };
    }
}
