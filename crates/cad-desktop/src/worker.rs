use cad_core::protocol::*;
use std::{
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};
/// Process transport is replaceable; no kernel types enter the desktop.
pub struct WorkerClient {
    requests: Sender<Option<WorkerRequest>>,
    responses: Mutex<Receiver<WorkerResponse>>,
}
impl WorkerClient {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let (out_tx, out_rx) = mpsc::channel();
        thread::spawn(move || supervise(rx, out_tx));
        Self {
            requests: tx,
            responses: Mutex::new(out_rx),
        }
    }
    pub fn submit(&self, request: WorkerRequest) {
        let _ = self.requests.send(Some(request));
    }
    pub fn cancel(&self) {
        let _ = self.requests.send(None);
    }
    pub fn drain(&self) -> Vec<WorkerResponse> {
        self.responses
            .lock()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default()
    }
}
impl Drop for WorkerClient {
    fn drop(&mut self) {
        let _ = self.requests.send(Some(WorkerRequest::Shutdown));
    }
}
struct Process {
    child: Child,
    input: ChildStdin,
    replies: Receiver<cad_core::Result<WorkerResponse>>,
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn spawn() -> cad_core::Result<Process> {
    let exe = std::env::var_os("TCAD_WORKER")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap_or_default()
                .with_file_name(if cfg!(windows) {
                    "tcad-worker.exe"
                } else {
                    "tcad-worker"
                })
        });
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| cad_core::CadError::Invalid("Worker has no input".into()))?;
    let mut output = child
        .stdout
        .take()
        .ok_or_else(|| cad_core::CadError::Invalid("Worker has no output".into()))?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        loop {
            let result = read_frame(&mut output);
            let stop = result.is_err();
            if tx.send(result).is_err() || stop {
                break;
            }
        }
    });
    Ok(Process {
        child,
        input,
        replies: rx,
    })
}
fn request_id(r: &WorkerRequest) -> Option<RequestIdentity> {
    match r {
        WorkerRequest::Evaluate { identity, .. }
        | WorkerRequest::Export { identity, .. }
        | WorkerRequest::Release { identity, .. }
        | WorkerRequest::Import { identity, .. } => Some(identity.clone()),
        _ => None,
    }
}
fn supervise(requests: Receiver<Option<WorkerRequest>>, output: Sender<WorkerResponse>) {
    let mut process: Option<Process> = None;
    let mut active: Option<(RequestIdentity, Instant)> = None;
    let mut queue = std::collections::VecDeque::new();
    loop {
        match requests.recv_timeout(Duration::from_millis(8)) {
            Ok(r) => {
                let mut incoming = vec![r];
                incoming.extend(requests.try_iter());
                for r in incoming {
                    match r {
                        Some(WorkerRequest::Shutdown) => return,
                        None => {
                            process = None;
                            active = None;
                            queue.clear();
                        }
                        Some(r) => {
                            if matches!(r, WorkerRequest::Evaluate { .. }) {
                                queue.retain(|q| !matches!(q, WorkerRequest::Evaluate { .. }));
                                if active.is_some() {
                                    process = None;
                                    active = None;
                                }
                            }
                            queue.push_back(r);
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some(p) = &process {
            match p.replies.try_recv() {
                Ok(reply) => {
                    let identity = active.take().map(|a| a.0);
                    match reply {
                        Ok(r) => {
                            let _ = output.send(r);
                        }
                        Err(e) => {
                            let _ = output.send(WorkerResponse::Error {
                                identity,
                                message: format!("Worker disconnected: {e}"),
                            });
                            process = None;
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if let Some((identity, _)) = active.take() {
                        let _ = output.send(WorkerResponse::Error {
                            identity: Some(identity),
                            message: "Worker exited unexpectedly; retry to start a fresh process"
                                .into(),
                        });
                    }
                    process = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if active
            .as_ref()
            .is_some_and(|(_, started)| started.elapsed() > Duration::from_secs(120))
        {
            let identity = active.take().map(|a| a.0);
            process = None;
            let _=output.send(WorkerResponse::Error{identity,message:"Geometry exceeded the 120 second deadline. The worker was terminated; reduce complexity or retry.".into()});
        }
        if active.is_none()
            && let Some(r) = queue.pop_front()
        {
            let identity = request_id(&r);
            if process.is_none() {
                match spawn() {
                    Ok(p) => process = Some(p),
                    Err(e) => {
                        let _=output.send(WorkerResponse::Error{identity,message:format!("Cannot launch geometry worker: {e}. Build the workspace or set TCAD_WORKER.")});
                        continue;
                    }
                }
            }
            if let Some(p) = &mut process {
                match write_frame(&mut p.input, &r) {
                    Ok(()) => active = identity.map(|i| (i, Instant::now())),
                    Err(e) => {
                        let _ = output.send(WorkerResponse::Error {
                            identity,
                            message: e.to_string(),
                        });
                        process = None;
                    }
                }
            }
        }
    }
}
