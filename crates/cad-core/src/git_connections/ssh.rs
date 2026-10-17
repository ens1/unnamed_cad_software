use super::{
    RepositoryAddress, Result, Secret, Transport, invalid, preferences_root, process,
    validate_key_path,
};
use crate::forge::CancelToken;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SshKeyInfo {
    pub private_key: PathBuf,
    pub public_key: String,
    pub fingerprint: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostKey {
    pub algorithm: String,
    pub public_key: String,
    pub fingerprint: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostKeyReview {
    pub host: String,
    pub port: u16,
    pub keys: Vec<HostKey>,
    pub known: bool,
}

pub fn known_hosts_path() -> Result<PathBuf> {
    Ok(preferences_root()?.join("ssh-known-hosts"))
}
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
pub(super) fn configure(command: &mut Command, private_key: Option<&Path>) -> Result<()> {
    let known = known_hosts_path()?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| invalid("The user home folder is unavailable"))?;
    let config_path = |path: &Path| -> Result<String> {
        let path = path
            .to_str()
            .ok_or_else(|| invalid("Invalid SSH known-hosts path"))?;
        if path.chars().any(char::is_control) {
            return Err(invalid("Invalid SSH known-hosts path"));
        }
        Ok(format!(
            "\"{}\"",
            path.replace('\\', "\\\\").replace('\"', "\\\"")
        ))
    };
    let files = format!(
        "{} {}",
        config_path(&home.join(".ssh/known_hosts"))?,
        config_path(&known)?
    );
    let mut ssh = format!(
        "ssh -o BatchMode=yes -o StrictHostKeyChecking=yes -o UpdateHostKeys=no -o {}",
        quote(&format!("UserKnownHostsFile={files}"))
    );
    if let Some(path) = private_key {
        validate_key_path(path)?;
        ssh.push_str(&format!(
            " -o IdentitiesOnly=yes -i {}",
            quote(
                path.to_str()
                    .ok_or_else(|| invalid("Invalid SSH key path"))?
            )
        ));
    }
    command
        .env("GIT_SSH_COMMAND", ssh)
        .env("GIT_SSH_VARIANT", "ssh");
    Ok(())
}
fn validate_private_key(path: &Path) -> Result<PathBuf> {
    validate_key_path(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 128 * 1024 {
        return Err(invalid("Choose a regular SSH private-key file, not a link"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(invalid(
                "The private key is accessible to other users. Restrict its permissions before importing it.",
            ));
        }
    }
    Ok(fs::canonicalize(path)?)
}
pub fn inspect_ssh_key(
    private_key: &Path,
    passphrase: Option<Secret>,
    cancel: &CancelToken,
) -> Result<SshKeyInfo> {
    let private_key = validate_private_key(private_key)?;
    let mut command = Command::new("ssh-keygen");
    command.args(["-y", "-f"]).arg(&private_key);
    let broker = passphrase.map(Askpass::new).transpose()?;
    if let Some(broker) = &broker {
        broker.configure(&mut command);
    } else {
        command.env("SSH_ASKPASS_REQUIRE", "never");
    }
    let output = process::run(&mut command, None, cancel, Duration::from_secs(30))?;
    if !output.status.success() {
        return Err(invalid(
            "Could not read the SSH key. Enter its passphrase if the key is encrypted.",
        ));
    }
    let public_key = String::from_utf8(output.stdout)
        .map_err(|_| invalid("Invalid SSH public key"))?
        .trim()
        .to_owned();
    let fingerprint = fingerprint(&public_key, cancel)?;
    Ok(SshKeyInfo {
        private_key,
        public_key,
        fingerprint,
    })
}
pub fn generate_ssh_key(
    destination: &Path,
    comment: &str,
    passphrase: Secret,
    cancel: &CancelToken,
) -> Result<SshKeyInfo> {
    validate_key_path(destination)?;
    if comment.len() > 300 || comment.chars().any(char::is_control) {
        return Err(invalid("Use a short, single-line SSH key label"));
    }
    let public_path = PathBuf::from(format!("{}.pub", destination.display()));
    if destination.exists() || public_path.exists() {
        return Err(invalid(
            "A key already exists at this path. Choose a new filename.",
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("Choose a folder for the SSH key"))?;
    fs::create_dir_all(parent)?;
    let staging = PrivateDirectory::new(parent, "tcad-key")?;
    let staged = staging.path.join("identity");
    let broker = Askpass::new(passphrase)?;
    let mut command = Command::new("ssh-keygen");
    command
        .args(["-q", "-t", "ed25519", "-a", "64", "-C", comment, "-f"])
        .arg(&staged);
    broker.configure(&mut command);
    let output = process::run(&mut command, None, cancel, Duration::from_secs(90))?;
    if !output.status.success() {
        return Err(invalid("Could not create the encrypted SSH key"));
    }
    cancel.check()?;
    validate_generated_encryption(&staged)?;
    let public_key = fs::read_to_string(staging.path.join("identity.pub"))?
        .trim()
        .to_owned();
    let fingerprint = fingerprint(&public_key, cancel)?;
    cancel.check()?;
    // Hard links publish without overwriting a file created while key generation was running.
    fs::hard_link(&staged, destination).map_err(|_| {
        invalid("Could not save the private key. The destination may already exist.")
    })?;
    if fs::hard_link(staging.path.join("identity.pub"), &public_path).is_err() {
        return Err(invalid(
            "The private key was saved, but its public-key file could not be saved. Import the private key to recover the public key.",
        ));
    }
    Ok(SshKeyInfo {
        private_key: destination.to_path_buf(),
        public_key,
        fingerprint,
    })
}
// Validate the output carrier before it leaves the private staging folder. OpenSSH can
// interpret a failed askpass helper as an empty passphrase, even when keygen exits successfully.
fn validate_generated_encryption(path: &Path) -> Result<()> {
    let pem = fs::read_to_string(path)?;
    let body = pem
        .strip_prefix("-----BEGIN OPENSSH PRIVATE KEY-----\n")
        .and_then(|value| {
            value
                .split_once("-----END OPENSSH PRIVATE KEY-----")
                .map(|(body, _)| body)
        })
        .ok_or_else(|| invalid("The generated SSH key has an unsupported format"))?;
    let mut decoded = Vec::new();
    let mut accumulator = 0_u32;
    let mut bits = 0_u32;
    for byte in body.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if byte == b'=' {
            break;
        }
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(invalid("The generated SSH key has invalid encoding")),
        };
        accumulator = (accumulator << 6) | u32::from(digit);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            decoded.push((accumulator >> bits) as u8);
        }
        if decoded.len() > 128 * 1024 {
            return Err(invalid("The generated SSH key is too large"));
        }
    }
    let mut bytes = decoded
        .strip_prefix(b"openssh-key-v1\0")
        .ok_or_else(|| invalid("The generated SSH key has an unsupported format"))?;
    fn field<'a>(bytes: &mut &'a [u8]) -> Option<&'a [u8]> {
        let size = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
        let field = bytes.get(4..4_usize.checked_add(size)?)?;
        *bytes = bytes.get(4 + size..)?;
        Some(field)
    }
    let cipher =
        field(&mut bytes).ok_or_else(|| invalid("The generated SSH key header is invalid"))?;
    let kdf =
        field(&mut bytes).ok_or_else(|| invalid("The generated SSH key header is invalid"))?;
    if cipher.is_empty() || cipher == b"none" || kdf != b"bcrypt" {
        return Err(invalid(
            "The SSH helper did not produce an encrypted key. No key was saved; check the passphrase helper and retry.",
        ));
    }
    Ok(())
}
pub fn unlock_ssh_key(private_key: &Path, passphrase: Secret, cancel: &CancelToken) -> Result<()> {
    let private_key = validate_private_key(private_key)?;
    if std::env::var_os("SSH_AUTH_SOCK").is_none() {
        return Err(invalid(
            "No SSH agent is available. Enable the system SSH agent before unlocking this key.",
        ));
    }
    let broker = Askpass::new(passphrase)?;
    let mut command = Command::new("ssh-add");
    command.args(["-t", "3600"]).arg(&private_key);
    broker.configure(&mut command);
    let output = process::run(&mut command, None, cancel, Duration::from_secs(30))?;
    if !output.status.success() {
        return Err(invalid(
            "Could not unlock the SSH key. Check its passphrase and the system SSH agent.",
        ));
    }
    Ok(())
}
fn fingerprint(public_key: &str, cancel: &CancelToken) -> Result<String> {
    let fields: Vec<_> = public_key.split_whitespace().collect();
    if fields.len() < 2
        || !valid_algorithm(fields[0])
        || !fields[1]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+/=".contains(&byte))
    {
        return Err(invalid("Invalid SSH public key"));
    }
    let mut command = Command::new("ssh-keygen");
    command.args(["-l", "-E", "sha256", "-f", "-"]);
    let output = process::run(
        &mut command,
        Some(Secret(format!("{public_key}\n").into_bytes())),
        cancel,
        Duration::from_secs(10),
    )?;
    if !output.status.success() {
        return Err(invalid("Could not calculate the SSH key fingerprint"));
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find(|field| field.starts_with("SHA256:"))
        .map(str::to_owned)
        .ok_or_else(|| invalid("SSH did not return a SHA-256 key fingerprint"))
}
fn valid_algorithm(value: &str) -> bool {
    matches!(
        value,
        "ssh-ed25519"
            | "ssh-rsa"
            | "ecdsa-sha2-nistp256"
            | "ecdsa-sha2-nistp384"
            | "ecdsa-sha2-nistp521"
            | "sk-ssh-ed25519@openssh.com"
            | "sk-ecdsa-sha2-nistp256@openssh.com"
    )
}
fn host_token(host: &str, port: u16) -> String {
    if port == 22 {
        host.trim_matches(['[', ']']).to_owned()
    } else {
        format!("[{}]:{port}", host.trim_matches(['[', ']']))
    }
}
pub fn scan_host_key(address: &RepositoryAddress, cancel: &CancelToken) -> Result<HostKeyReview> {
    if address.transport != Transport::Ssh {
        return Err(invalid(
            "Host-key review requires an SSH repository address",
        ));
    }
    let host = address
        .host
        .clone()
        .ok_or_else(|| invalid("Missing SSH host"))?;
    let port = address.effective_port().unwrap_or(22);
    let mut command = Command::new("ssh-keyscan");
    command
        .args([
            "-T",
            "8",
            "-p",
            &port.to_string(),
            "-t",
            "ed25519,ecdsa,rsa",
        ])
        .arg(host.trim_matches(['[', ']']));
    let output = process::run(&mut command, None, cancel, Duration::from_secs(35))?;
    let token = host_token(&host, port);
    let mut keys = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 || fields[0] != token || !valid_algorithm(fields[1]) {
            continue;
        }
        let public_key = format!("{} {}", fields[1], fields[2]);
        let key = HostKey {
            algorithm: fields[1].to_owned(),
            fingerprint: fingerprint(&public_key, cancel)?,
            public_key,
        };
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    if keys.is_empty() {
        return Err(invalid(
            "The SSH server returned no host keys. Check its address, port and connection.",
        ));
    }
    let known = fs::read_to_string(known_hosts_path()?)
        .unwrap_or_default()
        .lines()
        .any(|line| line.split_whitespace().next() == Some(token.as_str()));
    Ok(HostKeyReview {
        host,
        port,
        keys,
        known,
    })
}
/// Accept only fingerprints explicitly checked against the server owner's published fingerprint.
/// Existing keys are retained; a changed key is never silently replaced.
pub fn trust_host_key(review: &HostKeyReview, accepted_fingerprints: &[String]) -> Result<()> {
    trust_host_key_cancellable(review, accepted_fingerprints, &CancelToken::default())
}
pub fn trust_host_key_cancellable(
    review: &HostKeyReview,
    accepted_fingerprints: &[String],
    cancel: &CancelToken,
) -> Result<()> {
    cancel.check()?;
    static HOST_TRUST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = HOST_TRUST_LOCK
        .lock()
        .map_err(|_| invalid("SSH host-key preferences are unavailable"))?;
    let address =
        RepositoryAddress::parse(&format!("ssh://{}:{}/repository", review.host, review.port))?;
    if review.keys.is_empty()
        || review.keys.len() > 32
        || accepted_fingerprints.is_empty()
        || accepted_fingerprints.len() > review.keys.len()
    {
        return Err(invalid(
            "Select the SSH fingerprints you verified with the server owner",
        ));
    }
    let token = host_token(
        address
            .host
            .as_deref()
            .ok_or_else(|| invalid("Missing SSH host"))?,
        review.port,
    );
    let path = known_hosts_path()?;
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| invalid("Invalid host-key file path"))?,
    )?;
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err(invalid("The TCAD known-hosts path must be a regular file"));
    }
    let prior = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    if prior.len() > 4 * 1024 * 1024 {
        return Err(invalid("The SSH known-hosts file is too large"));
    }
    let mut added = String::new();
    for accepted in accepted_fingerprints {
        let key = review
            .keys
            .iter()
            .find(|key| &key.fingerprint == accepted)
            .ok_or_else(|| invalid("The selected SSH fingerprint is not in the current review"))?;
        if key.public_key.len() > 16_384
            || key.public_key.chars().any(char::is_control)
            || key.public_key.split_whitespace().count() != 2
            || key.public_key.split_whitespace().next() != Some(key.algorithm.as_str())
        {
            return Err(invalid(
                "The reviewed SSH key algorithm changed; scan it again",
            ));
        }
        if fingerprint(&key.public_key, cancel)? != *accepted {
            return Err(invalid("The reviewed SSH key changed; scan it again"));
        }
        for line in prior
            .lines()
            .filter(|line| line.split_whitespace().next() == Some(token.as_str()))
        {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.get(1) == Some(&key.algorithm.as_str())
                && fields.get(2).copied() != key.public_key.split_whitespace().nth(1)
            {
                return Err(invalid(
                    "A different key is already trusted for this host and algorithm. Resolve the key change before connecting.",
                ));
            }
        }
        let line = format!("{token} {}\n", key.public_key);
        if !prior.lines().any(|existing| existing == line.trim_end()) {
            added.push_str(&line);
        }
    }
    if !added.is_empty() {
        cancel.check()?;
        let mut options = fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        if !prior.is_empty() && !prior.ends_with('\n') {
            file.write_all(b"\n")?;
        }
        file.write_all(added.as_bytes())?;
        file.sync_all()?;
    }
    Ok(())
}

struct PrivateDirectory {
    path: PathBuf,
}
impl PrivateDirectory {
    fn new(parent: &Path, prefix: &str) -> Result<Self> {
        let path = parent.join(format!(".{prefix}-{}", uuid::Uuid::new_v4()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path)?;
        Ok(Self { path })
    }
}
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
struct Askpass {
    _directory: PrivateDirectory,
    socket: PathBuf,
    helper: PathBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
#[cfg(unix)]
impl Askpass {
    fn new(secret: Secret) -> Result<Self> {
        use std::{
            os::unix::{fs::PermissionsExt, net::UnixListener},
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            },
            thread,
        };
        if secret.0.len() > 1023 {
            return Err(invalid(
                "SSH passphrases must be at most 1023 bytes to fit OpenSSH's passphrase input",
            ));
        }
        let current = std::env::current_exe()?;
        let helper = std::env::var_os("TCAD_GIT_ASKPASS")
            .map(PathBuf::from)
            .unwrap_or_else(|| current.with_file_name("tcad-git-askpass"));
        if !helper.is_file() {
            return Err(invalid(
                "The SSH passphrase helper is missing. Reinstall the TCAD application.",
            ));
        }
        // Short temporary path also fits macOS's 104-byte Unix-socket limit.
        let directory = PrivateDirectory::new(Path::new("/tmp"), "tcad-askpass")?;
        let socket = directory.path.join("input");
        let listener = UnixListener::bind(&socket)?;
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            let mut replies = 0;
            while !worker_stop.load(Ordering::Relaxed) && replies < 4 {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                        let mut length = [0_u8; 4];
                        if stream.read_exact(&mut length).is_err() {
                            continue;
                        }
                        let length = u32::from_be_bytes(length) as usize;
                        if length > 4096 {
                            continue;
                        }
                        let mut prompt = vec![0; length];
                        if stream.read_exact(&mut prompt).is_err() {
                            continue;
                        }
                        let prompt = String::from_utf8_lossy(&prompt).to_ascii_lowercase();
                        if !prompt.contains("passphrase") {
                            continue;
                        }
                        replies += 1;
                        let _ = stream.write_all(&(secret.0.len() as u32).to_be_bytes());
                        let _ = stream.write_all(&secret.0);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            _directory: directory,
            socket,
            helper,
            stop,
            worker: Some(worker),
        })
    }
    fn configure(&self, command: &mut Command) {
        command
            .env("SSH_ASKPASS", &self.helper)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", "tcad:0")
            .env("TCAD_ASKPASS_SOCKET", &self.socket)
            .env("LC_ALL", "C");
    }
}
#[cfg(unix)]
impl Drop for Askpass {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[cfg(not(unix))]
struct Askpass;
#[cfg(not(unix))]
impl Askpass {
    fn new(_secret: Secret) -> Result<Self> {
        Err(invalid(
            "In-app SSH key passphrases currently require macOS or Linux",
        ))
    }
    fn configure(&self, _command: &mut Command) {}
}
/// Entry point for the small bundled helper. Its stdout goes only to OpenSSH, never to TCAD logs.
#[cfg(unix)]
pub fn askpass_main() -> Result<()> {
    use std::os::unix::net::UnixStream;
    let path = std::env::var_os("TCAD_ASKPASS_SOCKET")
        .ok_or_else(|| invalid("No SSH passphrase request is active"))?;
    let prompt = std::env::args()
        .nth(1)
        .ok_or_else(|| invalid("Missing SSH passphrase prompt"))?;
    if prompt.len() > 4096 || !prompt.to_ascii_lowercase().contains("passphrase") {
        return Err(invalid("Unrecognized SSH passphrase request"));
    }
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(&(prompt.len() as u32).to_be_bytes())?;
    stream.write_all(prompt.as_bytes())?;
    let mut length = [0_u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > 65_536 {
        return Err(invalid("Invalid SSH passphrase response"));
    }
    let mut secret = Secret(vec![0; length]);
    stream.read_exact(&mut secret.0)?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&secret.0)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}
#[cfg(not(unix))]
pub fn askpass_main() -> Result<()> {
    Err(invalid(
        "The SSH passphrase helper is unavailable on this platform",
    ))
}
