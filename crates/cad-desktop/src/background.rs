use cad_core::{
    persistence::atomic_write,
    version_control::{Git, VersionControl},
};
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        mpsc::{self, Receiver, Sender},
    },
};
pub enum Job {
    Recovery {
        path: PathBuf,
        data: Vec<u8>,
    },
    Git {
        root: PathBuf,
        args: Vec<String>,
    },
    Commit {
        root: PathBuf,
        manifest: String,
        message: String,
    },
}
pub struct Jobs {
    sender: Sender<Job>,
    recovery: Sender<(PathBuf, Vec<u8>)>,
    replies: Mutex<Receiver<(bool, String)>>,
}
impl Jobs {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let (out, replies) = mpsc::channel();
        let (recovery, recovery_rx) = mpsc::channel::<(PathBuf, Vec<u8>)>();
        let recovery_out = out.clone();
        std::thread::spawn(move || {
            while let Ok((path, data)) = recovery_rx.recv() {
                if let Err(e) = atomic_write(&path, &data) {
                    let _ = recovery_out.send((false, format!("Recovery save: {e}")));
                }
            }
        });
        std::thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                match job {
                    Job::Recovery { path, data } => {
                        if let Err(e) = atomic_write(&path, &data) {
                            let _ = out.send((false, format!("Recovery save: {e}")));
                        }
                    }
                    Job::Git { root, args } => {
                        let args = args.iter().map(String::as_str).collect::<Vec<_>>();
                        let result = Git.command(&root, &args);
                        let _ = out.send(match result {
                            Ok(text) => (true, text),
                            Err(e) => (false, e.to_string()),
                        });
                    }
                    Job::Commit {
                        root,
                        manifest,
                        message,
                    } => {
                        let result = (|| {
                            let mut paths = vec![manifest.as_str()];
                            for path in [
                                "parts",
                                "assemblies",
                                "imports",
                                "ui",
                                "libraries",
                                "graphs",
                                "standards",
                            ] {
                                if root.join(path).exists() {
                                    paths.push(path);
                                }
                            }
                            let mut args = vec!["add", "--all", "--"];
                            args.extend(paths);
                            Git.command(&root, &args)?;
                            Git.command(&root, &["commit", "-m", &message])
                        })();
                        let _ = out.send(match result {
                            Ok(text) => (true, text),
                            Err(e) => (false, e.to_string()),
                        });
                    }
                }
            }
        });
        Self {
            sender: tx,
            recovery,
            replies: Mutex::new(replies),
        }
    }
    pub fn submit(&self, job: Job) {
        if let Job::Recovery { path, data } = job {
            let _ = self.recovery.send((path, data));
            return;
        }
        let _ = self.sender.send(job);
    }
    pub fn drain(&self) -> Vec<(bool, String)> {
        self.replies
            .lock()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default()
    }
}
