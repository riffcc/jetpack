//! Real loopback SSH/SFTP sessions: no external daemon and no operator key files.
use super::ssh::SshConnection;
use crate::connection::{command::Forward, connection::Connection};
use russh::keys::{PrivateKey, ssh_key::private::Ed25519Keypair};
use russh::{
    Channel, ChannelId,
    server::{self, ChannelOpenHandle, Msg, Session},
};
use russh_sftp::protocol::{Data, FileAttributes, Handle, OpenFlags, Status, StatusCode};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicUsize, Ordering},
};

type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;
struct Peer {
    attempts: Arc<AtomicUsize>,
    commands: Arc<AtomicUsize>,
    channels: HashMap<ChannelId, Channel<Msg>>,
    files: Files,
}
impl server::Handler for Peer {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        assert_eq!(user, "configured-user");
        assert_eq!(password, "test-only-password");
        Ok(server::Auth::Accept)
    }
    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.commands.fetch_add(1, Ordering::SeqCst);
        session.channel_success(channel)?;
        let output = if String::from_utf8_lossy(data).contains("uname") {
            b"Linux test 6.1.0 x86_64 GNU/Linux\n".to_vec()
        } else {
            b"command output\n".to_vec()
        };
        session.data(channel, output)?;
        session.exit_status_request(channel, 0)?;
        session.eof(channel)?;
        session.close(channel)?;
        self.channels.remove(&channel);
        Ok(())
    }
    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        assert_eq!(name, "sftp");
        session.channel_success(channel)?;
        let channel = self.channels.remove(&channel).unwrap();
        let files = self.files.clone();
        tokio::spawn(async move {
            russh_sftp::server::run(channel.into_stream(), FilePeer(files)).await;
        });
        Ok(())
    }
}
struct FilePeer(Files);
fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: String::new(),
        language_tag: String::new(),
    }
}
impl russh_sftp::server::Handler for FilePeer {
    type Error = StatusCode;
    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }
    async fn open(
        &mut self,
        id: u32,
        path: String,
        flags: OpenFlags,
        _: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let mut files = self.0.lock().unwrap();
        if flags.contains(OpenFlags::TRUNCATE) {
            files.insert(path.clone(), Vec::new());
        }
        if !files.contains_key(&path) {
            return Err(StatusCode::NoSuchFile);
        }
        Ok(Handle { id, handle: path })
    }
    async fn close(&mut self, id: u32, _: String) -> Result<Status, Self::Error> {
        Ok(ok(id))
    }
    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let mut files = self.0.lock().unwrap();
        let buffer = files.get_mut(&handle).unwrap();
        let start = offset as usize;
        buffer.resize(buffer.len().max(start + data.len()), 0);
        buffer[start..start + data.len()].copy_from_slice(&data);
        Ok(ok(id))
    }
    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let files = self.0.lock().unwrap();
        let buffer = files.get(&handle).unwrap();
        if offset as usize >= buffer.len() {
            return Err(StatusCode::Eof);
        }
        let start = offset as usize;
        Ok(Data {
            id,
            data: buffer[start..buffer.len().min(start + len as usize)].to_vec(),
        })
    }
}
struct TestServer {
    port: u16,
    key: Arc<Mutex<PrivateKey>>,
    attempts: Arc<AtomicUsize>,
    commands: Arc<AtomicUsize>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
fn key(seed: u8) -> PrivateKey {
    PrivateKey::new(Ed25519Keypair::from_seed(&[seed; 32]).into(), "").unwrap()
}
impl TestServer {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let key = Arc::new(Mutex::new(key(1)));
        let attempts = Arc::new(AtomicUsize::new(0));
        let commands = Arc::new(AtomicUsize::new(0));
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let (server_key, server_attempts, server_commands) =
            (key.clone(), attempts.clone(), commands.clone());
        let thread = std::thread::spawn(move || {
            tokio::runtime::Runtime::new().unwrap().block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let files: Files = Arc::default();
                let mut tasks = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        _ = &mut stopped => break,
                        socket = listener.accept() => {
                            let (socket, _) = socket.unwrap();
                            let config = Arc::new(server::Config { keys: vec![server_key.lock().unwrap().clone()], ..Default::default() });
                            let handler = Peer { attempts: server_attempts.clone(), commands: server_commands.clone(), files: files.clone(), channels: HashMap::new() };
                            tasks.spawn(async move { if let Ok(session) = server::run_stream(config, socket, handler).await { let _ = session.await; } });
                        }
                    }
                }
                tasks.abort_all();
            });
        });
        Self {
            port,
            key,
            attempts,
            commands,
            stop: Some(stop),
            thread: Some(thread),
        }
    }
    fn config(&self, root: &std::path::Path, strict: bool) -> std::path::PathBuf {
        let path = root.join("config");
        std::fs::write(&path, format!("Host test-alias\n HostName 127.0.0.1\n User configured-user\n Port {}\n UserKnownHostsFile \"{}\"\n GlobalKnownHostsFile none\n HashKnownHosts yes\n{}", self.port, root.join("known_hosts").display(), if strict { " StrictHostKeyChecking yes\n" } else { "" })).unwrap();
        path
    }
    fn connection(&self, config: &std::path::Path) -> SshConnection {
        let host = Arc::new(RwLock::new(crate::inventory::hosts::Host::new(
            "test-alias",
        )));
        SshConnection::new(
            host,
            "default-user",
            22,
            "test-alias".into(),
            false,
            Some("test-only-password".into()),
            None,
            None,
            None,
        )
        .with_ssh_config(config)
    }
}
impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap();
    }
}
#[test]
fn native_ssh_and_sftp_share_openssh_trust_and_reject_rotation_before_password() {
    let server = TestServer::start();
    let root = tempfile::tempdir().unwrap();
    let config = server.config(root.path(), false);
    let mut connection = server.connection(&config);
    connection.connect().unwrap();
    assert_eq!(connection.username, "configured-user");
    assert_eq!(connection.port, i64::from(server.port));
    assert_eq!(server.attempts.load(Ordering::SeqCst), 1);
    let before = std::fs::read(root.path().join("known_hosts")).unwrap();
    assert!(before.starts_with(b"|1|"));
    if std::env::var_os("JETPACK_TEST_NO_SSH_TOOLS").is_some() {
        assert_eq!(
            std::process::Command::new("ssh")
                .output()
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(
            std::process::Command::new("ssh-keygen")
                .output()
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
    } else {
        let openssh = std::process::Command::new("ssh")
            .arg("-F")
            .arg(&config)
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "PreferredAuthentications=none",
                "-o",
                "StrictHostKeyChecking=yes",
                "--",
                "test-alias",
                "true",
            ])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&openssh.stderr)
                .to_lowercase()
                .contains("permission denied"),
            "{}",
            String::from_utf8_lossy(&openssh.stderr)
        );
    }

    // Use the actual native command and SFTP implementations.
    let local = crate::test_support::test_handle(Arc::new(Mutex::new(
        crate::test_support::RecordingConnection::new(),
    )));
    let run = local.run_state.clone();
    let host = connection.host.clone();
    let response = Arc::new(crate::handle::response::Response::new(run, host));
    let request = crate::tasks::TaskRequest::validate();
    let result = connection
        .run_command(&response, &request, "echo hello", Forward::No)
        .unwrap();
    assert_eq!(
        result.command_result.as_ref().as_ref().unwrap().out,
        "command output"
    );
    connection
        .write_data(&response, &request, "test file bytes", "/remote/file")
        .unwrap();
    assert_eq!(
        connection
            .fetch_file(&response, &request, "/remote/file")
            .unwrap(),
        b"test file bytes"
    );
    let source = root.path().join("source");
    std::fs::write(&source, [0, 255, 128, 1]).unwrap();
    connection
        .copy_file(&response, &request, &source, "/remote/binary")
        .unwrap();
    assert_eq!(
        connection
            .fetch_file(&response, &request, "/remote/binary")
            .unwrap(),
        [0, 255, 128, 1]
    );
    drop(connection);
    let mut repeat = server.connection(&config);
    repeat.connect().unwrap();
    drop(repeat);
    assert_eq!(
        std::fs::read(root.path().join("known_hosts")).unwrap(),
        before
    );
    let attempts = server.attempts.load(Ordering::SeqCst);
    let commands = server.commands.load(Ordering::SeqCst);
    *server.key.lock().unwrap() = key(2);
    let mut rotated = server.connection(&config);
    assert!(rotated.connect().unwrap_err().contains("changed"));
    assert_eq!(server.attempts.load(Ordering::SeqCst), attempts);
    assert_eq!(server.commands.load(Ordering::SeqCst), commands);
    assert_eq!(
        std::fs::read(root.path().join("known_hosts")).unwrap(),
        before
    );
}
#[test]
fn strict_unknown_host_rejects_before_password_and_remote_execution() {
    let server = TestServer::start();
    let root = tempfile::tempdir().unwrap();
    let config = server.config(root.path(), true);
    let mut connection = server.connection(&config);
    assert!(connection.connect().unwrap_err().contains("strict"));
    assert_eq!(server.attempts.load(Ordering::SeqCst), 0);
    assert_eq!(server.commands.load(Ordering::SeqCst), 0);
    assert!(!root.path().join("known_hosts").exists());
}

#[test]
fn explicit_no_policy_still_blocks_password_exposure_when_a_key_changes() {
    let server = TestServer::start();
    let root = tempfile::tempdir().unwrap();
    let config = server.config(root.path(), false);
    let mut first = server.connection(&config);
    first.connect().unwrap();
    drop(first);
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str(" StrictHostKeyChecking no\n");
    std::fs::write(&config, text).unwrap();
    *server.key.lock().unwrap() = key(2);
    let mut changed = server.connection(&config);
    assert!(changed.connect().unwrap_err().contains("disabled"));
    assert_eq!(server.attempts.load(Ordering::SeqCst), 1);
    assert_eq!(server.commands.load(Ordering::SeqCst), 1);
}

#[test]
fn native_client_trusts_records_enrolled_by_openssh() {
    let server = TestServer::start();
    let root = tempfile::tempdir().unwrap();
    let path = server.config(root.path(), false);
    let config = super::ssh_config::SshConfig::resolve(
        "test-alias",
        "unused-default",
        22,
        false,
        false,
        Some(&path),
    )
    .unwrap();
    let forwarding = config.forwarding_file().unwrap();
    let result = std::process::Command::new("ssh")
        .arg("-F")
        .arg(forwarding.path())
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "PreferredAuthentications=none",
            "--",
            "test-alias",
            "true",
        ])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .to_lowercase()
            .contains("permission denied")
    );
    let before = std::fs::read(root.path().join("known_hosts")).unwrap();
    let mut native = server.connection(&path);
    native.connect().unwrap();
    assert_eq!(
        std::fs::read(root.path().join("known_hosts")).unwrap(),
        before
    );
}

#[test]
fn native_ssh_sftp_and_trust_work_without_openssh_executables() {
    let empty = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "connection::ssh_tests::native_ssh_and_sftp_share_openssh_trust_and_reject_rotation_before_password", "--nocapture"])
        .env("PATH", empty.path())
        .env("JETPACK_TEST_NO_SSH_TOOLS", "1")
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
