use super::*;
use crate::git_connections::{self, ConnectionProfile, RepositoryAddress};
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub(super) struct Git<'a> {
    pub root: &'a Path,
    pub index: Option<&'a Path>,
    pub address: Option<&'a RepositoryAddress>,
    pub connection: Option<&'a ConnectionProfile>,
}

impl<'a> Git<'a> {
    pub fn local(root: &'a Path) -> Self {
        Self {
            root,
            index: None,
            address: None,
            connection: None,
        }
    }

    fn command(&self) -> Result<Command> {
        let mut command = Command::new("git");
        for name in [
            "GIT_CONFIG",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_TEMPLATE_DIR",
            "GIT_SSH",
            "GIT_SSH_COMMAND",
            "GIT_ASKPASS",
            "SSH_ASKPASS",
            "GIT_TRACE",
            "GIT_TRACE_CURL",
            "GIT_CURL_VERBOSE",
            "GIT_TRACE_PACKET",
            "GIT_TRACE_SETUP",
            "GIT_TRACE_PERFORMANCE",
            "GIT_TRACE2",
            "GIT_TRACE2_EVENT",
            "GIT_TRACE2_PERF",
        ] {
            command.env_remove(name);
        }
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_LFS_SKIP_SMUDGE", "1");
        command.args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.symlinks=false",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "tag.gpgSign=false",
            "-c",
            "credential.interactive=false",
            "-c",
            "submodule.recurse=false",
            "-c",
            "fetch.recurseSubmodules=false",
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.https.allow=always",
            "-c",
            "protocol.ssh.allow=always",
            "-c",
            "protocol.file.allow=always",
            "-c",
            "http.followRedirects=false",
        ]);
        if let Some(address) = self.address {
            git_connections::configure_git(&mut command, address, self.connection)?;
        }
        if let Some(index) = self.index {
            command.env("GIT_INDEX_FILE", index);
        }
        command.arg("-C").arg(self.root);
        Ok(command)
    }

    pub fn run(
        &self,
        args: &[&str],
        cancel: &CancelToken,
        progress: &mut impl FnMut(&str),
    ) -> Result<String> {
        self.run_author(args, None, cancel, progress)
    }

    pub fn run_author(
        &self,
        args: &[&str],
        author: Option<&GitAuthor>,
        cancel: &CancelToken,
        progress: &mut impl FnMut(&str),
    ) -> Result<String> {
        let mut command = self.command()?;
        if let Some(author) = author {
            validate_author(author)?;
            command
                .env("GIT_AUTHOR_NAME", &author.name)
                .env("GIT_AUTHOR_EMAIL", &author.email)
                .env("GIT_COMMITTER_NAME", &author.name)
                .env("GIT_COMMITTER_EMAIL", &author.email);
        }
        command.args(args);
        let output = process(command, cancel, 8 * 1024 * 1024, progress)?;
        if output.success {
            String::from_utf8(output.stdout)
                .map_err(|_| invalid("Git returned a non-UTF-8 library path"))
        } else {
            Err(invalid(format!(
                "Git could not complete the library operation: {}",
                crate::forge::sanitize_diagnostic(&output.stderr)
            )))
        }
    }

    pub fn blob(
        &self,
        object: &str,
        cancel: &CancelToken,
        progress: &mut impl FnMut(&str),
    ) -> Result<Vec<u8>> {
        validate_commit(object)?;
        let mut command = self.command()?;
        command.args(["cat-file", "blob", object]);
        let output = process(command, cancel, storage::MAX_FILE as usize, progress)?;
        if output.success {
            Ok(output.stdout)
        } else {
            Err(invalid(
                "Could not verify the library blob prepared for commit",
            ))
        }
    }

    pub fn optional(&self, args: &[&str], cancel: &CancelToken) -> Result<Option<String>> {
        let mut command = self.command()?;
        command.args(args);
        let output = process(command, cancel, 8 * 1024 * 1024, &mut |_| {})?;
        if output.success {
            Ok(Some(
                String::from_utf8(output.stdout)
                    .map_err(|_| invalid("Git returned a non-UTF-8 value"))?
                    .trim()
                    .to_owned(),
            ))
        } else {
            Ok(None)
        }
    }

    pub fn head(&self, cancel: &CancelToken) -> Result<Option<String>> {
        self.optional(&["rev-parse", "--verify", "HEAD^{commit}"], cancel)
    }

    pub fn branch(&self, cancel: &CancelToken) -> Result<String> {
        self.optional(&["symbolic-ref", "--quiet", "--short", "HEAD"], cancel)?.ok_or_else(|| invalid("The library checkout has a detached version. Create a branch before editing or publishing"))
    }

    pub fn validate_root(&self, cancel: &CancelToken) -> Result<()> {
        let git = self.root.join(".git");
        let metadata = std::fs::symlink_metadata(&git).map_err(|_| {
            invalid("Choose a library with its own Git repository at the selected folder")
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid(
                "Library checkouts need their own .git folder; linked worktrees are not supported here",
            ));
        }
        let actual = self.run(&["rev-parse", "--show-toplevel"], cancel, &mut |_| {})?;
        if std::fs::canonicalize(actual.trim())? != self.root {
            return Err(invalid(
                "The library manifest must be at the Git repository root",
            ));
        }
        Ok(())
    }

    pub fn remote(&self, cancel: &CancelToken) -> Result<Option<String>> {
        let remote = self
            .optional(&["remote", "get-url", "origin"], cancel)?
            .or(self.optional(&["remote", "get-url", "upstream"], cancel)?);
        remote
            .map(|url| RepositoryAddress::parse(&url).map(|address| address.git_url))
            .transpose()
    }

    pub fn check_branch(&self, branch: &str, cancel: &CancelToken) -> Result<()> {
        if branch.is_empty()
            || branch.len() > 240
            || branch.starts_with('-')
            || branch.chars().any(char::is_control)
        {
            return Err(invalid("Choose a valid branch name"));
        }
        self.run(
            &["check-ref-format", "--branch", branch],
            cancel,
            &mut |_| {},
        )
        .map(|_| ())
    }

    pub fn check_reviewed_address(&self, push: bool, cancel: &CancelToken) -> Result<()> {
        let address = self
            .address
            .ok_or_else(|| invalid("A reviewed library repository address is required"))?;
        let args = if push {
            vec![
                "-c",
                "remote.tcad-review.url",
                "remote",
                "get-url",
                "--push",
                "--all",
                "tcad-review",
            ]
        } else {
            vec![
                "-c",
                "remote.tcad-review.url",
                "remote",
                "get-url",
                "--all",
                "tcad-review",
            ]
        };
        // -c is positioned before the subcommand, with an explicit unique remote.
        let setting = format!("remote.tcad-review.url={}", address.git_url);
        let mut args = args;
        args[1] = &setting;
        let actual = self.run(&args, cancel, &mut |_| {})?;
        let mut found = false;
        for line in actual.lines().filter(|line| !line.trim().is_empty()) {
            found = true;
            if RepositoryAddress::parse(line.trim())?.git_url != address.git_url {
                return Err(invalid(
                    "Git URL aliases change the reviewed library destination. Use the resolved address or adjust the alias before publishing",
                ));
            }
        }
        if !found {
            return Err(invalid(
                "Git did not resolve the reviewed library destination",
            ));
        }
        Ok(())
    }
}

pub(super) fn validate_commit(commit: &str) -> Result<()> {
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("A complete Git commit ID is required"));
    }
    Ok(())
}

pub(super) fn validate_author(author: &GitAuthor) -> Result<()> {
    if author.name.trim().is_empty()
        || author.name.len() > 256
        || author.email.trim().is_empty()
        || author.email.len() > 320
        || !author.email.contains('@')
        || author
            .name
            .chars()
            .chain(author.email.chars())
            .any(|character| character.is_control() || matches!(character, '<' | '>'))
    {
        return Err(invalid("Enter a commit author name and email address"));
    }
    Ok(())
}

struct Output {
    stdout: Vec<u8>,
    stderr: String,
    success: bool,
}

fn process(
    mut command: Command,
    cancel: &CancelToken,
    output_limit: usize,
    progress: &mut impl FnMut(&str),
) -> Result<Output> {
    cancel.check()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let (sender, receiver) = mpsc::channel();
    fn read(
        mut stream: impl Read + Send + 'static,
        sender: mpsc::Sender<(bool, Vec<u8>)>,
        stdout: bool,
        output_limit: usize,
    ) {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 8192];
            while let Ok(count) = stream.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                if bytes.len() <= output_limit {
                    bytes.extend_from_slice(&buffer[..count.min(output_limit + 1 - bytes.len())]);
                }
            }
            let _ = sender.send((stdout, bytes));
        });
    }
    read(
        child
            .stdout
            .take()
            .ok_or_else(|| invalid("Git output is unavailable"))?,
        sender.clone(),
        true,
        output_limit,
    );
    read(
        child
            .stderr
            .take()
            .ok_or_else(|| invalid("Git diagnostic output is unavailable"))?,
        sender,
        false,
        8 * 1024 * 1024,
    );
    let started = Instant::now();
    let mut announced = Instant::now();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut received = 0;
    let mut status = None;
    loop {
        if cancel.is_cancelled() || started.elapsed() > Duration::from_secs(300) {
            #[cfg(unix)]
            {
                let _ = Command::new("/bin/kill")
                    .args(["-TERM", &format!("-{}", child.id())])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            #[cfg(windows)]
            {
                let _ = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(invalid(if cancel.is_cancelled() {
                "Library operation cancelled; local files were retained"
            } else {
                "Git timed out; local files were retained"
            }));
        }
        while let Ok((is_stdout, bytes)) = receiver.try_recv() {
            if is_stdout {
                stdout = bytes;
            } else {
                stderr = bytes;
            }
            received += 1;
        }
        if status.is_none() {
            status = child.try_wait()?;
        }
        if let Some(status) = status
            && received == 2
        {
            if stdout.len() > output_limit || stderr.len() > 8 * 1024 * 1024 {
                return Err(invalid("Library Git output exceeds its bounded size limit"));
            }
            return Ok(Output {
                stdout,
                stderr: String::from_utf8_lossy(&stderr).into(),
                success: status.success(),
            });
        }
        if announced.elapsed() > Duration::from_secs(3) {
            progress("Git is working on the library. You can cancel this operation.");
            announced = Instant::now();
        }
        thread::sleep(Duration::from_millis(40));
    }
}
