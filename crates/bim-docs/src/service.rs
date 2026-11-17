//! Which sync tool keeps a project folder the same for everyone.
//!
//! Docs works on any folder; knowing the tool only lets the window say what it is, and what the
//! tool's marks on a file mean. The tool is known by what it leaves at the root of what it syncs,
//! and for OneDrive and SharePoint by the folders Windows says they sync.

use std::path::{Path, PathBuf};

/// What keeps a folder the same on everyone's machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    Dropbox,
    /// A document library of a SharePoint site, synced by OneDrive.
    SharePoint,
    /// A person's own OneDrive.
    OneDrive,
    /// Synology Drive.
    Synology,
    /// A folder on a network drive.
    Network,
    /// A folder on this computer that nothing is known to sync.
    Local,
}

impl Service {
    /// Its name: a product name, the same in every language (`None` for a network or local folder,
    /// which the window names in its own words).
    pub fn name(self) -> Option<&'static str> {
        match self {
            Service::Dropbox => Some("Dropbox"),
            Service::SharePoint => Some("SharePoint"),
            Service::OneDrive => Some("OneDrive"),
            Service::Synology => Some("Synology Drive"),
            Service::Network | Service::Local => None,
        }
    }

    /// Whether the folder reaches other people's machines.
    pub fn shared(self) -> bool {
        self != Service::Local
    }
}

/// The sync of a project folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sync {
    pub service: Service,
    /// The folder the tool syncs, which the project folder is or is in (the project folder itself
    /// for a network or local folder).
    pub root: PathBuf,
    /// The web address of the SharePoint library or OneDrive, where Windows gives it.
    pub site: Option<String>,
}

/// A folder OneDrive syncs, as Windows reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub folder: PathBuf,
    /// The web address of what is synced there.
    pub site: Option<String>,
    /// A library of a SharePoint site, not a person's own OneDrive.
    pub team: bool,
}

/// What this computer says about its sync tools, read once by [`known`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Known {
    pub mounts: Vec<Mount>,
}

/// The sync of `folder`. The nearest root above it stands: a Dropbox folder inside a network drive
/// is a Dropbox folder.
pub fn of(folder: &Path, known: &Known) -> Sync {
    for root in folder.ancestors() {
        if root.join(".dropbox").exists() || root.join(".dropbox.cache").exists() {
            return Sync { service: Service::Dropbox, root: root.to_path_buf(), site: None };
        }
        if root.join(".SynologyWorkingDirectory").exists() {
            return Sync { service: Service::Synology, root: root.to_path_buf(), site: None };
        }
        if let Some(mount) = known.mounts.iter().find(|m| same(&m.folder, root)) {
            let service = if mount.team { Service::SharePoint } else { Service::OneDrive };
            return Sync { service, root: root.to_path_buf(), site: mount.site.clone() };
        }
    }
    let service = if on_a_server(folder) { Service::Network } else { Service::Local };
    Sync { service, root: folder.to_path_buf(), site: None }
}

/// Whether two paths name the same folder: Windows does not tell capitals from small letters, and
/// a path may end in a separator.
fn same(a: &Path, b: &Path) -> bool {
    let key = |p: &Path| p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    key(a) == key(b)
}

/// A path on a server (`\\server\share\…`), not on a drive of this computer.
fn on_a_server(folder: &Path) -> bool {
    let text = folder.to_string_lossy();
    (text.starts_with(r"\\") || text.starts_with("//")) && !text.starts_with(r"\\?\") && !text.starts_with(r"\\.\")
}

/// The folders OneDrive syncs in the text `reg query … /s` prints of
/// `HKCU\Software\SyncEngines\Providers\OneDrive`.
pub fn mounts(registry: &str) -> Vec<Mount> {
    let mut found = Vec::new();
    // one key per synced folder: a line with the key's name, then its values indented
    let mut key: Option<(Option<PathBuf>, Option<String>, bool)> = None;
    let mut close = |key: Option<(Option<PathBuf>, Option<String>, bool)>| {
        if let Some((Some(folder), site, team)) = key {
            found.push(Mount { folder, site, team });
        }
    };
    for line in registry.lines() {
        if line.starts_with("HKEY_") {
            close(key.replace((None, None, false)));
            continue;
        }
        let Some(entry) = key.as_mut() else { continue };
        // "    MountPoint    REG_SZ    C:\Users\an\Firm": name, type and value four spaces apart
        let mut parts = line.trim().splitn(3, "    ");
        let (Some(name), Some(_kind), Some(value)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let value = value.trim();
        match name.trim() {
            "MountPoint" if !value.is_empty() => entry.0 = Some(PathBuf::from(value)),
            "UrlNamespace" if !value.is_empty() => entry.1 = Some(value.to_string()),
            "LibraryType" => entry.2 = value.eq_ignore_ascii_case("teamsite"),
            _ => {}
        }
    }
    close(key);
    found
}

/// What this computer says: the OneDrive folders of the environment and, on Windows, of the
/// registry.
pub fn known() -> Known {
    let mut mounts = Vec::new();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // no console window flashes up for the query
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let query = std::process::Command::new("reg").args(["query", r"HKCU\Software\SyncEngines\Providers\OneDrive", "/s"]).creation_flags(CREATE_NO_WINDOW).output();
        if let Ok(output) = query {
            mounts = self::mounts(&String::from_utf8_lossy(&output.stdout));
        }
    }
    for variable in ["OneDriveCommercial", "OneDriveConsumer", "OneDrive"] {
        if let Some(folder) = std::env::var_os(variable).map(PathBuf::from).filter(|f| !f.as_os_str().is_empty()) {
            if !mounts.iter().any(|m| m.folder == folder) {
                mounts.push(Mount { folder, site: None, team: false });
            }
        }
    }
    Known { mounts }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, inside: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bim-docs-service-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for path in inside {
            let path = root.join(path);
            if path.extension().is_some() || path.file_name().is_some_and(|n| n == ".dropbox") {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, b"").unwrap();
            } else {
                std::fs::create_dir_all(&path).unwrap();
            }
        }
        root
    }

    #[test]
    fn a_folder_in_a_dropbox_is_known_by_the_mark_at_its_root() {
        let root = folder("dropbox", &["Team/.dropbox", "Team/Projects/Kade 12/model"]);
        let sync = of(&root.join("Team/Projects/Kade 12"), &Known::default());
        assert_eq!((sync.service, sync.root.as_path(), sync.site.as_deref()), (Service::Dropbox, root.join("Team").as_path(), None));
        // the cache folder is a mark too (the file can be missing)
        let root = folder("dropbox-cache", &["Team/.dropbox.cache", "Team/Projects"]);
        assert_eq!(of(&root.join("Team/Projects"), &Known::default()).service, Service::Dropbox);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_folder_in_a_synology_drive_is_known_by_its_working_folder() {
        let root = folder("synology", &["Drive/.SynologyWorkingDirectory", "Drive/Kade 12"]);
        let sync = of(&root.join("Drive/Kade 12"), &Known::default());
        assert_eq!((sync.service, sync.root.as_path()), (Service::Synology, root.join("Drive").as_path()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sharepoint_and_onedrive_are_known_by_what_windows_says_they_sync() {
        let root = folder("onedrive", &["Firm/Kade 12 - Documents/model", "OneDrive - Firm/Kade 12"]);
        let known = Known {
            mounts: vec![
                Mount { folder: root.join("Firm/Kade 12 - Documents"), site: Some("https://firm.sharepoint.com/sites/Kade12/Shared Documents/".into()), team: true },
                Mount { folder: root.join("OneDrive - Firm"), site: None, team: false },
            ],
        };
        let library = of(&root.join("Firm/Kade 12 - Documents/model"), &known);
        assert_eq!(library.service, Service::SharePoint);
        assert_eq!(library.root, root.join("Firm/Kade 12 - Documents"));
        assert_eq!(library.site.as_deref(), Some("https://firm.sharepoint.com/sites/Kade12/Shared Documents/"));
        assert_eq!(of(&root.join("OneDrive - Firm/Kade 12"), &known).service, Service::OneDrive);
        // a folder whose name only starts the same is not in it
        let other = folder("onedrive-other", &["OneDrive - Firm 2/x"]);
        let known = Known { mounts: vec![Mount { folder: other.join("OneDrive - Firm"), site: None, team: false }] };
        assert_eq!(of(&other.join("OneDrive - Firm 2/x"), &known).service, Service::Local);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(other);
    }

    #[test]
    fn a_folder_nothing_syncs_is_local_and_its_own_root() {
        let root = folder("local", &["Kade 12/model"]);
        let sync = of(&root.join("Kade 12"), &Known::default());
        assert_eq!((sync.service, sync.root.as_path()), (Service::Local, root.join("Kade 12").as_path()));
        assert!(!sync.service.shared());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn a_folder_on_a_server_is_a_network_folder() {
        let sync = of(Path::new(r"\\server\projects\Kade 12"), &Known::default());
        assert_eq!(sync.service, Service::Network);
        assert!(sync.service.shared());
    }

    #[test]
    fn the_registry_text_gives_the_folders_onedrive_syncs() {
        let text = "\r\nHKEY_CURRENT_USER\\Software\\SyncEngines\\Providers\\OneDrive\\0f1e2d\r\n    LastModifiedTime    REG_SZ    2026-11-17T08:00:00\r\n    LibraryType    REG_SZ    teamsite\r\n    MountPoint    REG_SZ    C:\\Users\\an\\Firm\\Kade 12 - Documents\r\n    UrlNamespace    REG_SZ    https://firm.sharepoint.com/sites/Kade12/Shared Documents/\r\n\r\nHKEY_CURRENT_USER\\Software\\SyncEngines\\Providers\\OneDrive\\Personal\r\n    LibraryType    REG_SZ    personal\r\n    MountPoint    REG_SZ    C:\\Users\\an\\OneDrive\r\n    UrlNamespace    REG_SZ    https://d.docs.live.net/abc\r\n\r\nHKEY_CURRENT_USER\\Software\\SyncEngines\\Providers\\OneDrive\\broken\r\n    LibraryType    REG_SZ    teamsite\r\n";
        let found = mounts(text);
        assert_eq!(
            found,
            [
                Mount { folder: PathBuf::from(r"C:\Users\an\Firm\Kade 12 - Documents"), site: Some("https://firm.sharepoint.com/sites/Kade12/Shared Documents/".into()), team: true },
                Mount { folder: PathBuf::from(r"C:\Users\an\OneDrive"), site: Some("https://d.docs.live.net/abc".into()), team: false },
            ]
        );
        assert!(mounts("ERROR: The system was unable to find the specified registry key or value.").is_empty());
    }
}
