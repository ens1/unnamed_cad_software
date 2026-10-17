use super::{Result, invalid};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    Https,
    Ssh,
    Local,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryAddress {
    pub git_url: String,
    pub transport: Transport,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub path: String,
}
impl RepositoryAddress {
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if input.is_empty()
            || input.len() > 8192
            || input.chars().any(char::is_control)
            || input.starts_with('-')
        {
            return Err(invalid("Enter a repository URL or local folder path"));
        }
        if input.contains("::") && !input.contains("://[") {
            return Err(invalid("Git transport helper addresses are not supported"));
        }
        if let Some((scheme, tail)) = input.split_once("://") {
            if scheme == "file" {
                let path = tail
                    .strip_prefix("localhost/")
                    .map(|value| format!("/{value}"))
                    .unwrap_or_else(|| tail.to_owned());
                if !path.starts_with('/') {
                    return Err(invalid("A file URL must identify a local absolute path"));
                }
                return Self::local(&decode(&path)?);
            }
            let transport = match scheme {
                "https" => Transport::Https,
                "ssh" => Transport::Ssh,
                _ => {
                    return Err(invalid(
                        "Use HTTPS, SSH, or a local folder. Other Git transports are disabled.",
                    ));
                }
            };
            let (authority, path) = tail
                .split_once('/')
                .ok_or_else(|| invalid("The repository URL needs a path after its host"))?;
            if path.is_empty() || path.contains(['?', '#']) {
                return Err(invalid(
                    "Repository URLs cannot contain a query or fragment",
                ));
            }
            let (user, host_port) = match authority.rsplit_once('@') {
                Some((user, host)) if transport == Transport::Ssh && valid_user(user) => {
                    (Some(user.to_owned()), host)
                }
                Some(_) => {
                    return Err(invalid(
                        "Keep credentials out of repository URLs. Use the Connections panel.",
                    ));
                }
                None => (None, authority),
            };
            let (host, port) = parse_host(host_port)?;
            let path = decode(path)?;
            validate_remote_path(&path)?;
            let git_url = format!(
                "{scheme}://{}{}{}/{}",
                user.as_ref()
                    .map(|value| format!("{value}@"))
                    .unwrap_or_default(),
                host,
                port.map(|value| format!(":{value}")).unwrap_or_default(),
                encode_path(&path)
            );
            return Ok(Self {
                git_url,
                transport,
                host: Some(host),
                port,
                user,
                path,
            });
        }
        if !input.starts_with(['/', '.', '~'])
            && let Some((host_user, path)) = input.split_once(':')
        {
            if host_user.contains(['/', '\\']) || path.starts_with(':') {
                return Err(invalid("Invalid SSH repository address"));
            }
            let (user, host_text) = match host_user.rsplit_once('@') {
                Some((user, host)) if valid_user(user) => (Some(user.to_owned()), host),
                Some(_) => return Err(invalid("Invalid SSH user name")),
                None => (None, host_user),
            };
            let (host, port) = parse_host(host_text)?;
            if port.is_some() {
                return Err(invalid("Use ssh://user@host:port/path for an SSH port"));
            }
            validate_remote_path(path)?;
            return Ok(Self {
                git_url: input.to_owned(),
                transport: Transport::Ssh,
                host: Some(host),
                port: None,
                user,
                path: path.to_owned(),
            });
        }
        Self::local(input)
    }
    fn local(input: &str) -> Result<Self> {
        let path = if let Some(suffix) = input.strip_prefix("~/") {
            let home = std::env::var_os("HOME")
                .ok_or_else(|| invalid("The user home folder is unavailable"))?;
            PathBuf::from(home).join(suffix)
        } else {
            PathBuf::from(input)
        };
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()?.join(path)
        };
        let text = path
            .to_str()
            .ok_or_else(|| invalid("The repository path must be valid Unicode"))?
            .to_owned();
        Ok(Self {
            git_url: text.clone(),
            transport: Transport::Local,
            host: None,
            port: None,
            user: None,
            path: text,
        })
    }
    pub fn effective_port(&self) -> Option<u16> {
        self.port.or(match self.transport {
            Transport::Https => Some(443),
            Transport::Ssh => Some(22),
            Transport::Local => None,
        })
    }
    pub fn authority(&self) -> String {
        format!(
            "{}{}",
            self.host.as_deref().unwrap_or_default(),
            self.port
                .filter(|port| self.transport != Transport::Https || *port != 443)
                .map(|port| format!(":{port}"))
                .unwrap_or_default()
        )
    }
    pub fn credential_scope(&self) -> String {
        format!("https://{}", self.authority())
    }
}
fn parse_host(input: &str) -> Result<(String, Option<u16>)> {
    let (host, port) = if input.starts_with('[') {
        let end = input
            .find(']')
            .ok_or_else(|| invalid("Invalid IPv6 repository host"))?;
        let host = &input[..=end];
        let tail = &input[end + 1..];
        if host[1..host.len() - 1]
            .parse::<std::net::Ipv6Addr>()
            .is_err()
        {
            return Err(invalid("Invalid IPv6 repository host"));
        }
        (
            host,
            if tail.is_empty() {
                None
            } else {
                Some(
                    tail.strip_prefix(':')
                        .ok_or_else(|| invalid("Invalid repository port"))?,
                )
            },
        )
    } else if let Some((host, port)) = input.rsplit_once(':') {
        (host, Some(port))
    } else {
        (input, None)
    };
    if host.is_empty()
        || host.len() > 253
        || host.starts_with('-')
        || (!host.starts_with('[')
            && !host.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
            }))
    {
        return Err(invalid("Invalid repository host name"));
    }
    let port = port
        .map(|port| {
            port.parse::<u16>()
                .ok()
                .filter(|port| *port > 0)
                .ok_or_else(|| invalid("Repository ports must be between 1 and 65535"))
        })
        .transpose()?;
    Ok((host.to_ascii_lowercase(), port))
}
fn valid_user(user: &str) -> bool {
    !user.is_empty()
        && !user.starts_with('-')
        && user.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}
fn validate_remote_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.chars().any(char::is_control)
        || path.starts_with('-')
        || path.contains(['\\', '?', '#'])
        || path.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(invalid("Invalid repository path"));
    }
    Ok(())
}
fn decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position] == b'%' {
            if position + 2 >= bytes.len() {
                return Err(invalid("Invalid URL escape"));
            }
            let hex = std::str::from_utf8(&bytes[position + 1..position + 3])
                .map_err(|_| invalid("Invalid URL escape"))?;
            output.push(u8::from_str_radix(hex, 16).map_err(|_| invalid("Invalid URL escape"))?);
            position += 3;
        } else {
            output.push(bytes[position]);
            position += 1;
        }
    }
    let output = String::from_utf8(output)
        .map_err(|_| invalid("Repository URLs must contain valid Unicode"))?;
    if output.chars().any(char::is_control) {
        return Err(invalid("Repository URLs cannot contain control characters"));
    }
    Ok(output)
}
fn encode_path(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            output.push(char::from(byte));
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}
