//! A project folder that is a Git checkout: where it comes from, and the versions Git keeps of a
//! file.
//!
//! A team that shares its project through GitHub (or any Git server) has the folder on every
//! machine as a checkout. Docs works on it as on any folder; Git adds what the sync tools keep to
//! themselves: every version of a file, with who made it and why.
//!
//! Where the checkout comes from is read from its own files. The versions are asked of the `git`
//! program, for one file at a time and only when a person picks it. Nothing is ever committed,
//! fetched or pushed here.

use std::path::{Path, PathBuf};

/// A Git checkout a folder is (in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    /// The folder that holds `.git`.
    pub root: PathBuf,
    /// The web address of the repository it was cloned from (`origin`), without any name or
    /// password that stood in it.
    pub web: Option<String>,
}

impl Checkout {
    /// Whether it comes from GitHub.
    pub fn github(&self) -> bool {
        self.web.as_deref().is_some_and(|web| web.starts_with("https://github.com/"))
    }
}

/// A version of a file that Git keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub by: String,
    /// When, as Git writes it (`2026-11-17T14:05:13+01:00`).
    pub date: String,
    /// The first line of what was written about it.
    pub subject: String,
}

impl Commit {
    /// The hash as people quote it: its first seven characters.
    pub fn short(&self) -> &str {
        self.hash.get(..7).unwrap_or(&self.hash)
    }
}

/// How a file stands against the last version Git has of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// As Git has it.
    Same,
    /// Changed since the last version.
    Changed,
    /// Not in Git yet.
    New,
}

/// The checkout whose root is `folder` itself, if it is one.
pub fn at(folder: &Path) -> Option<Checkout> {
    let mark = folder.join(".git");
    let home = if mark.is_dir() {
        mark
    } else {
        // a worktree or submodule: `.git` is a file that says where the real folder is
        let said = std::fs::read_to_string(&mark).ok()?;
        let path = said.lines().find_map(|line| line.strip_prefix("gitdir:"))?.trim().to_string();
        let home = if Path::new(&path).is_absolute() { PathBuf::from(path) } else { folder.join(path) };
        // a worktree's own folder points at the one all worktrees share, which has the config
        match std::fs::read_to_string(home.join("commondir")) {
            Ok(common) => {
                let common = common.trim();
                if Path::new(common).is_absolute() {
                    PathBuf::from(common)
                } else {
                    home.join(common)
                }
            }
            Err(_) => home,
        }
    };
    let config = std::fs::read_to_string(home.join("config")).unwrap_or_default();
    Some(Checkout { root: folder.to_path_buf(), web: origin(&config).and_then(|url| web(&url)) })
}

/// The address of the remote called `origin` in the text of a Git config file.
pub fn origin(config: &str) -> Option<String> {
    let mut inside = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line.replace(' ', "").eq_ignore_ascii_case("[remote\"origin\"]");
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim().eq_ignore_ascii_case("url") && !value.trim().is_empty() {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// The web address of a repository from the address Git fetches it at: `https://host/owner/name`.
/// A name or password in the address is left out; an address on this computer gives none.
pub fn web(remote: &str) -> Option<String> {
    let remote = remote.trim();
    let (host, path) = if let Some(rest) = remote.strip_prefix("https://").or_else(|| remote.strip_prefix("http://")).or_else(|| remote.strip_prefix("ssh://")).or_else(|| remote.strip_prefix("git://")) {
        let (host, path) = rest.split_once('/')?;
        // without who logs in, and without the port
        let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
        (host.split_once(':').map_or(host, |(host, _)| host), path)
    } else {
        // git@host:owner/name.git
        let (host, path) = remote.split_once(':')?;
        let host = host.rsplit_once('@').map(|(_, host)| host)?;
        (host, path)
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if host.is_empty() || path.is_empty() || !host.contains('.') {
        return None;
    }
    Some(format!("https://{host}/{path}"))
}

/// The separator between the fields of a line of [`history`]: no name or subject holds it.
const FIELD: char = '\u{1f}';

/// The versions in the text `git log` prints with the format of [`history`], newest first.
pub fn commits(log: &str) -> Vec<Commit> {
    log.lines()
        .filter_map(|line| {
            let mut fields = line.split(FIELD);
            let (hash, by, date) = (fields.next()?, fields.next()?, fields.next()?);
            if hash.len() < 7 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            Some(Commit { hash: hash.to_string(), by: by.to_string(), date: date.to_string(), subject: fields.next().unwrap_or("").to_string() })
        })
        .collect()
}

/// How a file stands, from what `git status --porcelain` prints for it.
pub fn standing(status: &str) -> Standing {
    match status.lines().find(|line| !line.trim().is_empty()) {
        None => Standing::Same,
        Some(line) if line.starts_with("??") => Standing::New,
        Some(_) => Standing::Changed,
    }
}

/// Runs `git` in `root` and gives what it printed.
fn git(root: &Path, arguments: &[&str]) -> Result<Vec<u8>, String> {
    let mut command = std::process::Command::new("git");
    command.arg("-C").arg(root).args(arguments);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // no console window flashes up
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().map_err(|e| format!("git: {e}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// The last `limit` versions Git keeps of the file at `path` (in the checkout, `/` between
/// names), newest first, through its renames.
pub fn history(root: &Path, path: &str, limit: usize) -> Result<Vec<Commit>, String> {
    let format = format!("--format=%H{FIELD}%an{FIELD}%ad{FIELD}%s");
    let out = git(root, &["log", "--follow", "-n", &limit.to_string(), "--date=iso-strict", &format, "--", path])?;
    Ok(commits(&String::from_utf8_lossy(&out)))
}

/// How the file at `path` stands against the last version Git has of it.
pub fn standing_of(root: &Path, path: &str) -> Result<Standing, String> {
    Ok(standing(&String::from_utf8_lossy(&git(root, &["status", "--porcelain", "--", path])?)))
}

/// The file at `path` as it was in the version `hash`.
pub fn content(root: &Path, path: &str, hash: &str) -> Result<Vec<u8>, String> {
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("not a version: {hash}"));
    }
    git(root, &["show", &format!("{hash}:{path}")])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bim-docs-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn the_web_address_follows_from_the_address_git_fetches_at() {
        for remote in ["git@github.com:firm/kade-12.git", "https://github.com/firm/kade-12.git", "https://github.com/firm/kade-12", "ssh://git@github.com/firm/kade-12.git", "https://github.com/firm/kade-12/"] {
            assert_eq!(web(remote).as_deref(), Some("https://github.com/firm/kade-12"), "{remote}");
        }
        // who logs in, a password or token and a port are not part of it
        assert_eq!(web("https://an:secret-token@git.firm.example:8443/team/kade.git").as_deref(), Some("https://git.firm.example/team/kade"));
        // a folder on this computer or a share has no web address
        assert_eq!(web(r"C:\repos\kade.git"), None);
        assert_eq!(web("/srv/git/kade.git"), None);
        assert_eq!(web(""), None);
    }

    #[test]
    fn origin_is_read_from_the_config_and_no_other_remote() {
        let config = "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = git@github.com:other/kade.git\n[remote \"origin\"]\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n\turl = git@github.com:firm/kade-12.git\n[branch \"main\"]\n\tremote = origin\n";
        assert_eq!(origin(config).as_deref(), Some("git@github.com:firm/kade-12.git"));
        assert_eq!(origin("[core]\n\tbare = false\n"), None);
    }

    #[test]
    fn a_checkout_is_known_by_its_git_folder_and_says_where_it_comes_from() {
        let root = folder("checkout");
        assert_eq!(at(&root), None);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config"), "[remote \"origin\"]\n\turl = https://github.com/firm/kade-12.git\n").unwrap();
        let checkout = at(&root).expect("a checkout");
        assert_eq!((checkout.root.as_path(), checkout.web.as_deref()), (root.as_path(), Some("https://github.com/firm/kade-12")));
        assert!(checkout.github());
        // a checkout that was never cloned from anywhere is one too, without an address
        std::fs::write(root.join(".git/config"), "[core]\n").unwrap();
        let own = at(&root).expect("a checkout");
        assert!(own.web.is_none() && !own.github());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_worktree_finds_the_config_of_the_checkout_it_belongs_to() {
        let root = folder("worktree");
        std::fs::create_dir_all(root.join("main/.git/worktrees/side")).unwrap();
        std::fs::write(root.join("main/.git/config"), "[remote \"origin\"]\n\turl = git@gitlab.com:firm/kade.git\n").unwrap();
        std::fs::write(root.join("main/.git/worktrees/side/commondir"), "../..\n").unwrap();
        std::fs::create_dir_all(root.join("side")).unwrap();
        std::fs::write(root.join("side/.git"), format!("gitdir: {}\n", root.join("main/.git/worktrees/side").display())).unwrap();
        let checkout = at(&root.join("side")).expect("a checkout");
        assert_eq!(checkout.web.as_deref(), Some("https://gitlab.com/firm/kade"));
        assert!(!checkout.github());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_log_gives_the_versions_newest_first() {
        let log = "0123456789abcdef0123456789abcdef01234567\u{1f}An de Vries\u{1f}2026-11-17T14:05:13+01:00\u{1f}Kozijnen op de eerste verdieping\n89abcdef0123456789abcdef0123456789abcdef\u{1f}Bo\u{1f}2026-11-10T09:00:00+01:00\u{1f}\nnot a line of the log\n";
        let found = commits(log);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].short(), found[0].by.as_str(), found[0].subject.as_str()), ("0123456", "An de Vries", "Kozijnen op de eerste verdieping"));
        assert_eq!((found[1].date.as_str(), found[1].subject.as_str()), ("2026-11-10T09:00:00+01:00", ""));
    }

    #[test]
    fn the_status_says_how_a_file_stands() {
        assert_eq!(standing(""), Standing::Same);
        assert_eq!(standing("?? model/new.ifc\n"), Standing::New);
        assert_eq!(standing(" M model/kade.ifc\n"), Standing::Changed);
        assert_eq!(standing("A  model/kade.ifc\n"), Standing::Changed);
    }

    /// Against the `git` program itself; a computer without it has nothing to check.
    #[test]
    fn git_gives_the_versions_of_a_file_how_it_stands_and_an_older_version() {
        let root = folder("real");
        let run = |arguments: &[&str]| git(&root, arguments);
        if run(&["init", "-q"]).is_err() {
            return;
        }
        let commit = |message: &str| {
            run(&["add", "-A"]).unwrap();
            run(&["-c", "user.name=An", "-c", "user.email=an@firm.example", "-c", "commit.gpgsign=false", "commit", "-q", "-m", message]).unwrap();
        };
        std::fs::create_dir_all(root.join("model")).unwrap();
        std::fs::write(root.join("model/kade.ifc"), b"first").unwrap();
        commit("The first model");
        std::fs::write(root.join("model/kade.ifc"), b"second").unwrap();
        commit("Windows on the first floor");

        let versions = history(&root, "model/kade.ifc", 20).unwrap();
        assert_eq!(versions.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(), ["Windows on the first floor", "The first model"]);
        assert_eq!(versions[0].by, "An");
        assert_eq!(content(&root, "model/kade.ifc", &versions[1].hash).unwrap(), b"first");
        assert!(content(&root, "model/kade.ifc", "HEAD; rm").is_err(), "only a hash names a version");

        assert_eq!(standing_of(&root, "model/kade.ifc").unwrap(), Standing::Same);
        std::fs::write(root.join("model/kade.ifc"), b"third").unwrap();
        assert_eq!(standing_of(&root, "model/kade.ifc").unwrap(), Standing::Changed);
        std::fs::write(root.join("model/new.ifc"), b"new").unwrap();
        assert_eq!(standing_of(&root, "model/new.ifc").unwrap(), Standing::New);
        assert!(history(&root, "model/new.ifc", 20).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
