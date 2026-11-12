//! Helpers for tests that run the node binaries.

#![allow(dead_code)]

use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// A fresh directory of this test process, removed when dropped.
pub struct Temp(pub PathBuf);

impl Temp {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A child process, killed when dropped.
pub struct Child(pub std::process::Child);

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A local port that was free a moment ago.
pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The JSON a node on local `port` answers one request with, or `None`
/// when it does not answer with JSON.
pub fn http(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&str>,
    token: Option<&str>,
) -> Option<serde_json::Value> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    let body = body.unwrap_or("");
    let auth = token.map_or(String::new(), |t| format!("Authorization: Bearer {t}\r\n"));
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\n\
         Host: 127.0.0.1\r\n\
         Connection: close\r\n\
         {auth}Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {body}",
        body.len()
    )
    .ok()?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).ok()?;
    let split = response.windows(4).position(|w| w == b"\r\n\r\n")?;
    serde_json::from_slice(&response[split + 4..]).ok()
}

/// Waits for a started node to serve: the JSON it answers `GET path` with,
/// or its exit status if it stopped first. Panics with the node's `log`
/// after two minutes of neither.
pub fn ready(
    child: &mut Child,
    port: u16,
    path: &str,
    log: &Path,
) -> Result<serde_json::Value, ExitStatus> {
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        if let Some(code) = child.0.try_wait().unwrap() {
            return Err(code);
        }
        if let Some(answer) = http(port, "GET", path, None, None) {
            return Ok(answer);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "node readiness timed out: {}",
        std::fs::read_to_string(log).unwrap_or_default()
    );
}

/// Runs `binary` with a command line it must refuse and returns its stderr.
///
/// Such command lines are often otherwise complete, so a lost guard would
/// start a node: the process is killed and the test fails after a deadline
/// instead of waiting for it.
pub fn refused(binary: &str, args: &[String], env: &[(&str, &str)]) -> String {
    let mut child = Command::new(binary)
        .args(args)
        .envs(env.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let code = loop {
        if let Some(code) = child.try_wait().unwrap() {
            break code;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{args:?} was accepted");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(!code.success(), "{args:?}");
    stderr
}
