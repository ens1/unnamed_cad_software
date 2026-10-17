use super::CancelToken;
use crate::{CadError, Result};
use std::{
    io::Read,
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

pub(super) struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Never formats a command, arguments or raw process output in an error.
pub(super) fn run(
    command: &mut Command,
    cancel: &CancelToken,
    timeout: Duration,
    mut stderr_line: impl FnMut(&str),
) -> Result<Output> {
    cancel.check()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| {
        CadError::Invalid(
            "Could not start the GitHub helper. Reinstall TCAD or check TCAD_GH.".into(),
        )
    })?;
    let (sender, receiver) = mpsc::sync_channel::<(bool, std::io::Result<Vec<u8>>)>(16);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CadError::Invalid("Could not read GitHub helper output".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CadError::Invalid("Could not read GitHub helper status".into()))?;
    fn reader(
        mut stream: impl Read + Send + 'static,
        error: bool,
        sender: mpsc::SyncSender<(bool, std::io::Result<Vec<u8>>)>,
    ) {
        thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        if sender.send((error, Ok(buffer[..count].to_vec()))).is_err() {
                            break;
                        }
                    }
                    Err(error_value) => {
                        let _ = sender.send((error, Err(error_value)));
                        break;
                    }
                }
            }
        });
    }
    reader(stdout, false, sender.clone());
    reader(stderr, true, sender);
    let started = Instant::now();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut line = Vec::new();
    let mut status = None;
    let result = loop {
        if cancel.is_cancelled() {
            break Err(CadError::Invalid("Repository operation cancelled. Refresh before retrying a repository creation or sign-in.".into()));
        }
        if started.elapsed() >= timeout {
            break Err(CadError::Invalid("GitHub did not finish in time. Check your connection and refresh before retrying a repository creation or sign-in.".into()));
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok((is_error, Ok(bytes))) => {
                if out.len() + err.len() + bytes.len() > OUTPUT_LIMIT {
                    break Err(CadError::Invalid(
                        "GitHub returned too much data. Narrow the repository search.".into(),
                    ));
                }
                if is_error {
                    err.extend_from_slice(&bytes);
                    for byte in bytes {
                        if byte == b'\n' || byte == b'\r' {
                            stderr_line(&String::from_utf8_lossy(&line));
                            line.clear();
                        } else if line.len() < 16_384 {
                            line.push(byte);
                        }
                    }
                } else {
                    out.extend_from_slice(&bytes);
                }
            }
            Ok((_, Err(_))) => {
                break Err(CadError::Invalid(
                    "Could not read the GitHub helper response".into(),
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if !line.is_empty() {
                    stderr_line(&String::from_utf8_lossy(&line));
                    line.clear();
                }
                thread::sleep(Duration::from_millis(20));
                if let Some(status) = status {
                    break Ok(Output {
                        status,
                        stdout: out,
                        stderr: err,
                    });
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(_) => {
                    break Err(CadError::Invalid(
                        "Could not read GitHub helper completion".into(),
                    ));
                }
            }
        }
    };
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

/// Classify failures without forwarding output that could contain credentials or URLs with secrets.
pub(super) fn failure(output: &Output, operation: &str) -> CadError {
    let error = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    let message = if error.contains("environment variable") {
        "A GitHub token supplied by the computer environment controls this account. Remove that override before changing sign-in."
    } else if error.contains("http 401")
        || error.contains("not logged")
        || error.contains("authentication")
        || error.contains("bad credentials")
    {
        "Sign in again to GitHub, then retry."
    } else if error.contains("rate limit") {
        "GitHub's request limit was reached. Wait and refresh."
    } else if error.contains("http 403") || error.contains("forbidden") {
        "GitHub denied access. Check repository permissions and any organization SSO authorization."
    } else if error.contains("http 404")
        || error.contains("could not resolve to a repository")
        || error.contains("not found")
    {
        "The repository was not found or this account cannot access it."
    } else if error.contains("already exists") || error.contains("name already exists") {
        "A repository with that name already exists. Refresh the list and select it."
    } else if error.contains("http 422") || error.contains("validation failed") {
        "GitHub rejected these repository details. Check the owner and name; the repository may already exist."
    } else if error.contains("unknown flag") || error.contains("unknown command") {
        "The GitHub helper is too old. Reinstall the current TCAD package or update GitHub CLI."
    } else {
        "GitHub did not complete the request. Check your connection, sign-in and permissions."
    };
    CadError::Invalid(format!("{operation}: {message}"))
}
