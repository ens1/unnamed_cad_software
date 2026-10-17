//! Discoverable optional content, independent of installed catalogs or a network connection.
use super::*;
use crate::{
    git_connections::{self, ConnectionProfile, RepositoryAddress, Transport},
    repository_models::{self, CloneRequest},
};
use std::{fs, io::ErrorKind, path::Component};

/// A source recommendation, never an instruction to fetch or execute repository content.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Pack {
    pub version: u32,
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub clone_url: &'static str,
    pub web_url: &'static str,
    pub manifest_id: &'static str,
    pub directory_name: &'static str,
    pub button_label: &'static str,
    pub kinds: &'static [ContentKind],
}

const PACKS: &[Pack] = &[
    Pack {
        version: 1,
        id: "profiles",
        name: "Weldment profiles",
        description: "Nominal metric and imperial structural sections.",
        clone_url: "https://github.com/ens1/tcad-weldment-profiles.git",
        web_url: "https://github.com/ens1/tcad-weldment-profiles",
        manifest_id: "tcad-weldment-profiles",
        directory_name: "tcad-weldment-profiles",
        button_label: "Download weldment profile pack",
        kinds: &[ContentKind::WeldmentProfiles],
    },
    Pack {
        version: 1,
        id: "textures",
        name: "Textures and finishes",
        description: "Grayscale grain maps and surface finishes, separate from material properties.",
        clone_url: "https://github.com/ens1/tcad-textures.git",
        web_url: "https://github.com/ens1/tcad-textures",
        manifest_id: "tcad-textures",
        directory_name: "tcad-textures",
        button_label: "Download texture and finish pack",
        kinds: &[ContentKind::Textures, ContentKind::Finishes],
    },
    Pack {
        version: 1,
        id: "themes",
        name: "Themes",
        description: "Optional layouts and styles. Classic is always available without a download.",
        clone_url: "https://github.com/ens1/tcad-themes.git",
        web_url: "https://github.com/ens1/tcad-themes",
        manifest_id: "tcad-themes",
        directory_name: "tcad-themes",
        button_label: "Download theme pack",
        kinds: &[ContentKind::Themes],
    },
    Pack {
        version: 1,
        id: "materials",
        name: "Engineering materials",
        description: "Material properties for engineering calculations and part definitions.",
        clone_url: "https://github.com/ens1/tcad-materials.git",
        web_url: "https://github.com/ens1/tcad-materials",
        manifest_id: "tcad-materials",
        directory_name: "tcad-materials",
        button_label: "Download material pack",
        kinds: &[ContentKind::Materials],
    },
    Pack {
        version: 1,
        id: "standard-parts",
        name: "Standard parts",
        description: "Nominal metric hardware for reusable components.",
        clone_url: "https://github.com/ens1/tcad-standard-parts.git",
        web_url: "https://github.com/ens1/tcad-standard-parts",
        manifest_id: "tcad-standard-parts",
        directory_name: "tcad-standard-parts",
        button_label: "Download standard part pack",
        kinds: &[ContentKind::StandardParts],
    },
];

pub fn packs() -> &'static [Pack] {
    PACKS
}

pub fn pack(id: &str) -> Result<&'static Pack> {
    PACKS
        .iter()
        .find(|pack| pack.id == id)
        .ok_or_else(|| invalid("Unknown content pack; refresh the content library list"))
}

pub fn pack_for_kind(kind: ContentKind) -> &'static Pack {
    // The explicit match makes a future kind require a deliberate source decision.
    match kind {
        ContentKind::WeldmentProfiles => &PACKS[0],
        ContentKind::Textures | ContentKind::Finishes => &PACKS[1],
        ContentKind::Themes => &PACKS[2],
        ContentKind::Materials => &PACKS[3],
        ContentKind::StandardParts => &PACKS[4],
    }
}

fn same_source(pack: &Pack, remote: &str) -> bool {
    let Ok(address) = RepositoryAddress::parse(remote) else {
        return false;
    };
    let Ok(expected) = RepositoryAddress::parse(pack.clone_url) else {
        return false;
    };
    let standard_port = match address.transport {
        Transport::Https => address.port.is_none_or(|port| port == 443),
        Transport::Ssh => address.port.is_none_or(|port| port == 22),
        Transport::Local => false,
    };
    let path = |value: &str| {
        let value = value.trim_matches('/');
        value
            .strip_suffix(".git")
            .unwrap_or(value)
            .to_ascii_lowercase()
    };
    standard_port && address.host == expected.host && path(&address.path) == path(&expected.path)
}

fn download_address(source: &Pack, connection: Option<&ConnectionProfile>) -> Result<String> {
    let expected = RepositoryAddress::parse(source.clone_url)?;
    let clone_url =
        if connection.is_some_and(|connection| connection.scope.transport == Transport::Ssh) {
            format!(
                "git@{}:{}",
                expected
                    .host
                    .as_deref()
                    .ok_or_else(|| invalid("The content pack has no Git host"))?,
                expected.path
            )
        } else {
            source.clone_url.to_owned()
        };
    let address = RepositoryAddress::parse(&clone_url)?;
    if !same_source(source, &address.git_url) {
        return Err(invalid(
            "The content pack address does not match its known source",
        ));
    }
    if let Some(connection) = connection {
        connection.validate()?;
        if !connection.scope.matches(&address) {
            return Err(invalid(format!(
                "The selected Git connection does not match {clone_url}. Choose a connection for this GitHub host, port and repository path in Content Libraries"
            )));
        }
    }
    Ok(address.git_url)
}

/// Cached metadata only. A fork may have this source as its upstream; downloading
/// also checks the actual origin before deciding whether to reuse the checkout.
pub fn matching_pack_repository<'a>(
    pack: &Pack,
    registry: &'a Registry,
) -> Option<&'a InstalledRepository> {
    registry
        .repositories
        .iter()
        .filter(|repository| matches_pack_source(pack, repository))
        .max_by_key(|repository| repository.enabled)
}

fn matches_pack_source(pack: &Pack, repository: &InstalledRepository) -> bool {
    repository.manifest.id == pack.manifest_id
        && repository
            .remote
            .as_deref()
            .is_some_and(|remote| same_source(pack, remote))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LibraryPreferences {
    version: u32,
    directory: PathBuf,
}

fn preferences_path() -> Result<PathBuf> {
    Ok(registry_path()?.with_file_name("content-library-preferences.json"))
}

pub fn default_library_directory() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| invalid("The user home folder is unavailable"))?;
    validate_directory(&PathBuf::from(home).join("TCAD/Libraries"))
}

/// Does not create any directories or contact a repository.
pub fn library_directory() -> Result<PathBuf> {
    let path = preferences_path()?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => default_library_directory(),
        Err(error) => Err(error.into()),
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(invalid(
                    "The content library preferences must be a regular file",
                ));
            }
            let preferences: LibraryPreferences =
                serde_json::from_slice(&storage::read_bounded(&path, 16 * 1024)?)?;
            if preferences.version != 1 {
                return Err(invalid("Unsupported content library folder preferences"));
            }
            validate_directory(&preferences.directory)
        }
    }
}

/// Changes the destination of future downloads. Existing checkouts stay in place.
/// Replaces malformed preferences without needing to read them first.
pub fn set_library_directory(directory: &Path) -> Result<PathBuf> {
    let directory = validate_directory(directory)?;
    let path = preferences_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Invalid content library preferences location"))?;
    fs::create_dir_all(parent)?;
    storage::atomic_write(
        &path,
        &serde_json::to_vec_pretty(&LibraryPreferences {
            version: 1,
            directory: directory.clone(),
        })?,
    )?;
    Ok(directory)
}

fn validate_directory(path: &Path) -> Result<PathBuf> {
    let text = path
        .to_str()
        .ok_or_else(|| invalid("The library folder must use a readable Unicode path"))?;
    if !path.is_absolute()
        || path.parent().is_none()
        || text.len() > 4096
        || text.chars().any(char::is_control)
        || path.components().any(|component| {
            matches!(component, Component::CurDir | Component::ParentDir)
                || matches!(component, Component::Normal(name) if name.to_str().is_some_and(|name| name.eq_ignore_ascii_case(".git")))
        })
    {
        return Err(invalid("Choose an absolute library folder without parent-directory steps or a Git metadata folder"));
    }
    let mut ancestor = path;
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(invalid(format!(
                        "Choose a real library folder, not a file or symbolic link: {}",
                        ancestor.display()
                    )));
                }
                // Preserve the visible user choice; canonical paths are recorded per checkout.
                return Ok(path.to_path_buf());
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| invalid("The library folder has no accessible parent"))?;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

static DOWNLOAD_LOCK: Mutex<()> = Mutex::new(());

/// Explicitly download one known pack, preserving failed transfers and local edits.
/// No existing files are deleted, pulled over, or replaced. A repeat click enables
/// an existing matching origin; updates remain a separate review operation.
pub fn download_pack(
    id: &str,
    connection: Option<&ConnectionProfile>,
    cancel: &CancelToken,
    mut progress: impl FnMut(&str),
) -> Result<InstalledRepository> {
    let source = pack(id)?;
    let _download = DOWNLOAD_LOCK
        .lock()
        .map_err(|_| invalid("Content pack downloads are unavailable"))?;
    cancel.check()?;
    let registry = load_registry()?;
    // A user fork can carry the same catalog identities while deliberately
    // changing its source. Do not enable a second conflicting copy behind it.
    for repository in registry
        .repositories
        .iter()
        .filter(|repository| repository.enabled && repository.manifest.id == source.manifest_id)
    {
        match fs::symlink_metadata(&repository.root) {
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        if !matches_pack_source(source, repository)
            || !matching_origin(source, &repository.root, cancel)?
        {
            return Err(invalid(
                "An enabled custom or forked library already provides this pack. Keep it, or disable it in Content Libraries before downloading the standard pack",
            ));
        }
    }
    for repository in registry
        .repositories
        .iter()
        .filter(|repository| matches_pack_source(source, repository))
    {
        cancel.check()?;
        match fs::symlink_metadata(&repository.root) {
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        // Forks retain upstream identity but have a different (or no) origin.
        if !matching_origin(source, &repository.root, cancel)? {
            continue;
        }
        let ready = verify_checkout(source, &repository.root, cancel)?;
        progress("The pack is already downloaded; enabling its existing checkout");
        return install_ready(ready, cancel);
    }

    let directory = library_directory()?;
    cancel.check()?;
    fs::create_dir_all(&directory)?;
    let directory = storage::canonical_root(&directory)?;
    let destination = directory.join(source.directory_name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            let ready = verify_checkout(source, &destination, cancel).map_err(|error| {
                invalid(format!(
                    "The download folder {} is already in use and was kept: {error}. Choose another library folder or move the existing folder yourself",
                    destination.display()
                ))
            })?;
            progress("Registering the pack already present in the download folder");
            return install_ready(ready, cancel);
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    // Installed checkouts above remain usable offline regardless of the selected
    // download connection. Validate transport and scope only for a new transfer.
    let clone_url = download_address(source, connection)?;
    let downloads = directory.join(".downloads");
    fs::create_dir_all(&downloads)?;
    let downloads = storage::canonical_root(&downloads)?;
    let temporary = downloads.join(format!(
        "{}-{}",
        source.directory_name,
        uuid::Uuid::new_v4()
    ));
    progress(&format!(
        "Downloading {} to {}",
        source.name,
        destination.display()
    ));
    let checkout = repository_models::clone_repository(
        &CloneRequest {
            clone_url,
            destination: temporary.clone(),
            branch: None,
            expected_commit: None,
        },
        connection,
        cancel,
        &mut progress,
    )?;
    let mut retained = temporary.clone();
    let result = (|| {
        verify_checkout(source, &checkout.root, cancel)?;
        cancel.check()?;
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(invalid(
                "The pack destination appeared during download; it was kept unchanged",
            ));
        }
        fs::rename(&checkout.root, &destination)?;
        retained = destination.clone();
        git_connections::bind_reviewed_connection(&destination, connection)?;
        let ready = verify_checkout(source, &destination, cancel)?;
        progress("Enabling the downloaded pack");
        install_ready(ready, cancel)
    })();
    result.map_err(|error: CadError| {
        invalid(format!("{error}. Download files were retained at {}. Retry Download to register a complete pack; incomplete transfers remain available for recovery", retained.display()))
    })
}

fn matching_origin(source: &Pack, root: &Path, cancel: &CancelToken) -> Result<bool> {
    let root = storage::canonical_root(root)?;
    let git = git::Git::local(&root);
    git.validate_root(cancel)?;
    Ok(git
        .optional(&["remote", "get-url", "origin"], cancel)?
        .as_deref()
        .is_some_and(|remote| same_source(source, remote)))
}

fn verify_checkout(
    source: &Pack,
    root: &Path,
    cancel: &CancelToken,
) -> Result<InstalledRepository> {
    if !matching_origin(source, root, cancel)? {
        return Err(invalid(format!(
            "The folder is not a checkout of {}",
            source.web_url
        )));
    }
    let repository = operations::installation(root, true, cancel)?;
    if repository.manifest.id != source.manifest_id || repository.revision.is_none() {
        return Err(invalid(
            "The download does not contain the expected committed content pack",
        ));
    }
    Ok(repository)
}

fn install_ready(
    mut ready: InstalledRepository,
    cancel: &CancelToken,
) -> Result<InstalledRepository> {
    cancel.check()?;
    operations::mutate_registry(|registry| {
        if registry.repositories.iter().any(|entry| {
            entry.enabled
                && entry.manifest.id == ready.manifest.id
                && entry.root != ready.root
                && !fs::symlink_metadata(&entry.root)
                    .is_err_and(|error| error.kind() == ErrorKind::NotFound)
        }) {
            return Err(invalid(
                "Another enabled library already provides this pack. Keep it, or disable it before enabling this copy",
            ));
        }
        if let Some(existing) = registry
            .repositories
            .iter_mut()
            .find(|entry| entry.root == ready.root)
        {
            ready.id = existing.id.clone();
            *existing = ready.clone();
        } else {
            registry.repositories.push(ready.clone());
        }
        Ok(())
    })?;
    Ok(ready)
}
