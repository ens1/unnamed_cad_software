use super::{Result, Secret, invalid};
use crate::forge::CancelToken;
use std::{
    io::{Read, Write},
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
pub(super) struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
}
/// Bounded and cancellable. Command arguments and raw diagnostics never enter error messages.
pub(super) fn run(
    command: &mut Command,
    input: Option<Secret>,
    cancel: &CancelToken,
    timeout: Duration,
) -> Result<Output> {
    cancel.check()?;
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| invalid("Could not start the Git or SSH system helper"))?;
    if let Some(input) = input {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| invalid("Could not open the credential input channel"))?;
        thread::spawn(move || {
            let _ = stdin.write_all(&input.0);
        });
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| invalid("Could not read the system helper response"))?;
    let (sender, receiver) = mpsc::sync_channel(16);
    thread::spawn(move || {
        let mut stream = stdout;
        let mut buffer = [0_u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if sender.send(Ok(buffer[..count].to_vec())).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = sender.send(Err(()));
                    break;
                }
            }
        }
    });
    let start = Instant::now();
    let mut bytes = Vec::new();
    let mut status = None;
    let result = loop {
        if cancel.is_cancelled() {
            break Err(invalid(
                "The operation was cancelled. Completed credential changes may have been retained; refresh before retrying.",
            ));
        }
        if start.elapsed() > timeout {
            break Err(invalid(
                "The Git connection operation did not finish in time",
            ));
        }
        match receiver.recv_timeout(Duration::from_millis(30)) {
            Ok(Ok(chunk)) => {
                if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
                    break Err(invalid("The system helper returned too much data"));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(Err(())) => break Err(invalid("Could not read the system helper response")),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(status) = status {
                    break Ok(Output {
                        status,
                        stdout: bytes,
                    });
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => (),
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(_) => break Err(invalid("Could not read system helper completion")),
            }
        }
    };
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}
