//! Where a file of the project folder is on the website of the service that shares it, and a mail
//! that hands it on.
//!
//! Sharing with others, rights and the service's own version history stay with the service: Docs
//! opens the file's page there, or hands its address on. Nothing here asks the service anything;
//! the addresses follow from what the computer says about the folder.

use crate::{Service, Sync};
use std::path::Path;

/// A file's page on the website of the service that shares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Web {
    /// The file itself.
    pub file: String,
    /// The list of its versions, where the service has one at an address of its own.
    pub history: Option<String>,
}

/// Where the file at `path` of the project folder `project` is on the website of the service that
/// keeps it (`sync`). A Git checkout needs its `branch`. None for a folder no website shows.
pub fn web(sync: &Sync, project: &Path, path: &str, branch: Option<&str>) -> Option<Web> {
    // the file's path from the root of what the service keeps, `/` between the names
    let above = project.strip_prefix(&sync.root).ok()?.to_string_lossy().replace('\\', "/");
    let inside = [above.trim_matches('/'), path].iter().filter(|part| !part.is_empty()).copied().collect::<Vec<_>>().join("/");
    match sync.service {
        Service::GitHub => {
            let (site, branch) = (sync.site.as_deref()?.trim_end_matches('/'), encode(branch?));
            let inside = encode(&inside);
            Some(Web { file: format!("{site}/blob/{branch}/{inside}"), history: Some(format!("{site}/commits/{branch}/{inside}")) })
        }
        Service::SharePoint | Service::OneDrive => {
            // a SharePoint library or a OneDrive at work; a personal OneDrive gives no page
            let site = sync.site.as_deref().filter(|s| s.starts_with("https://") && s.contains(".sharepoint.com"))?;
            Some(Web { file: format!("{}/{}", site.trim_end_matches('/').replace(' ', "%20"), encode(&inside)), history: None })
        }
        Service::Dropbox => {
            let (folder, name) = inside.rsplit_once('/').unwrap_or(("", &inside));
            Some(Web { file: format!("https://www.dropbox.com/home/{}?preview={}", encode(folder), encode(name)), history: None })
        }
        Service::Synology | Service::Git | Service::Network | Service::Local => None,
    }
}

/// An address that opens a new mail with `subject` and `body` in the person's mail program.
pub fn mail(subject: &str, body: &str) -> String {
    let body = body.replace("\r\n", "\n").replace('\n', "\r\n");
    format!("mailto:?subject={}&body={}", escape(subject, false), escape(&body, false))
}

/// A path for an address: each name escaped, `/` kept between them.
fn encode(path: &str) -> String {
    escape(path, true)
}

/// `text` with everything but letters, digits and `-._~` (and `/` when `keep_slash`) written as
/// `%XX`, byte by byte of its UTF-8.
fn escape(text: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Service, Sync};
    use std::path::PathBuf;

    fn sync(service: Service, root: &str, site: Option<&str>) -> Sync {
        Sync { service, root: PathBuf::from(root), site: site.map(str::to_string) }
    }

    #[test]
    fn a_file_in_a_github_checkout_is_on_github_at_its_branch() {
        let s = sync(Service::GitHub, "/work/kade", Some("https://github.com/firm/kade-12"));
        let found = web(&s, &PathBuf::from("/work/kade/project"), "01 Models/Kade 12.ifc", Some("main")).expect("on GitHub");
        assert_eq!(found.file, "https://github.com/firm/kade-12/blob/main/project/01%20Models/Kade%2012.ifc");
        assert_eq!(found.history.as_deref(), Some("https://github.com/firm/kade-12/commits/main/project/01%20Models/Kade%2012.ifc"));
        // without a branch there is no address to give
        assert!(web(&s, &PathBuf::from("/work/kade"), "a.ifc", None).is_none());
    }

    #[test]
    fn a_file_in_a_sharepoint_library_is_at_the_library_s_address() {
        let s = sync(Service::SharePoint, "/Users/an/Firm/Kade 12 - Documents", Some("https://firm.sharepoint.com/sites/Kade12/Shared Documents/"));
        let found = web(&s, &PathBuf::from("/Users/an/Firm/Kade 12 - Documents/Design"), "A-101 #2.pdf", None).expect("in SharePoint");
        assert_eq!(found.file, "https://firm.sharepoint.com/sites/Kade12/Shared%20Documents/Design/A-101%20%232.pdf");
        assert!(found.history.is_none());
        // a person's own OneDrive at work is a SharePoint site too
        let own = sync(Service::OneDrive, "/Users/an/OneDrive - Firm", Some("https://firm-my.sharepoint.com/personal/an_firm_com/Documents"));
        assert_eq!(web(&own, &PathBuf::from("/Users/an/OneDrive - Firm"), "a.pdf", None).map(|w| w.file).as_deref(), Some("https://firm-my.sharepoint.com/personal/an_firm_com/Documents/a.pdf"));
        // the address Windows gives a personal OneDrive is no page
        let home = sync(Service::OneDrive, "/Users/an/OneDrive", Some("https://d.docs.live.net/1234abcd"));
        assert!(web(&home, &PathBuf::from("/Users/an/OneDrive"), "a.pdf", None).is_none());
    }

    #[test]
    fn a_file_in_dropbox_is_shown_in_its_folder_on_the_website() {
        let s = sync(Service::Dropbox, "/Users/an/Dropbox (Firm)", None);
        let found = web(&s, &PathBuf::from("/Users/an/Dropbox (Firm)/Kade 12"), "02 Drawings/A-101.pdf", None).expect("in Dropbox");
        assert_eq!(found.file, "https://www.dropbox.com/home/Kade%2012/02%20Drawings?preview=A-101.pdf");
    }

    #[test]
    fn a_local_or_network_folder_has_no_web_address() {
        for service in [Service::Local, Service::Network, Service::Synology, Service::Git] {
            assert!(web(&sync(service, "/work", None), &PathBuf::from("/work"), "a.pdf", Some("main")).is_none(), "{service:?}");
        }
    }

    #[test]
    fn a_mail_carries_its_subject_and_text_in_its_address() {
        assert_eq!(mail("A-101 & B", "See: https://x.example/a b\nThanks"), "mailto:?subject=A-101%20%26%20B&body=See%3A%20https%3A%2F%2Fx.example%2Fa%20b%0D%0AThanks");
    }
}
