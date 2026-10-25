//! Recovery writes run separately from Git operations and geometry workers.
use cad_core::persistence::atomic_write;
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        mpsc::{self, Receiver, Sender},
    },
};
pub enum Job {
    Recovery { path: PathBuf, data: Vec<u8> },
}
pub struct Jobs {
    sender: Sender<Job>,
    replies: Mutex<Receiver<(bool, String)>>,
}
impl Jobs {
    pub fn new() -> Self {
        let (sender, requests) = mpsc::channel::<Job>();
        let (out, replies) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(Job::Recovery { path, data }) = requests.recv() {
                if let Err(error) = atomic_write(&path, &data)
                    && out.send((false, format!("Recovery save: {error}"))).is_ok()
                {
                    crate::redraw::wake();
                }
            }
        });
        Self {
            sender,
            replies: Mutex::new(replies),
        }
    }
    pub fn submit(&self, job: Job) {
        let _ = self.sender.send(job);
    }
    pub fn drain(&self) -> Vec<(bool, String)> {
        self.replies
            .lock()
            .map(|receiver| receiver.try_iter().collect())
            .unwrap_or_default()
    }
}
