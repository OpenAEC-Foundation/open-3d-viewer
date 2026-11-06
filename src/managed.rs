//! OpenAEC Linux, the streamed SuperCloud environment (OpenAEC-Blueprint#61 and
//! `docs/supercloud-koppeling.md`, route B): the environment sets these variables and the desktop
//! app follows them. Outside it they are not set and nothing changes.
//!
//! | Variable | What |
//! |----------|------|
//! | `OPENAEC_BEHEERD=1` | Managed mode: no update notice; the AI connection only reaches files in the work folder. |
//! | `OPENAEC_MAP` | The work folder (`/openaec`): dialogs start there, relative paths resolve there. |
//! | `OPENAEC_TAAL` | The user's language (`nl`, `en`; other languages get English). |
//! | `OPENAEC_THEMA` | The theme (`light`, `dark`, or one of the viewer's own themes). |
//! | `OPENAEC_OPEN` | A file to open at the start, when the command line names none. |
//! | `OPENAEC_MCP_*` | The MCP endpoint over HTTP, see `mcp::HttpConfig`. |

use openaec_ui::design::ThemeId;
use openaec_ui::Language;
use std::path::{Component, Path, PathBuf};

/// `OPENAEC_BEHEERD=1`: the SuperCloud manages the version (it refreshes the image), so the app
/// shows no update notice.
pub fn managed() -> bool {
    std::env::var("OPENAEC_BEHEERD").as_deref() == Ok("1")
}

/// `OPENAEC_MAP`: the user's work folder (`/openaec` in the SuperCloud), where the open and save
/// dialogs start. Ignored when it is not a folder.
pub fn work_folder() -> Option<PathBuf> {
    std::env::var_os("OPENAEC_MAP").map(PathBuf::from).filter(|p| p.is_dir())
}

/// Where a dialog starts: the model's own folder, unless there is a work folder and the model
/// lies outside it.
pub fn dialog_folder(model_folder: Option<PathBuf>, work: Option<PathBuf>) -> Option<PathBuf> {
    match work {
        Some(work) if !model_folder.as_ref().is_some_and(|f| f.starts_with(&work)) => Some(work),
        _ => model_folder,
    }
}

/// `OPENAEC_TAAL`: the user's language in the SuperCloud. The viewer speaks Dutch and English;
/// every other language gets English.
pub fn language() -> Option<Language> {
    language_from(&std::env::var("OPENAEC_TAAL").ok()?)
}

fn language_from(value: &str) -> Option<Language> {
    let code = value.trim().to_ascii_lowercase();
    match code.get(..2) {
        _ if code.is_empty() => None,
        Some("nl") => Some(Language::Dutch),
        _ => Some(Language::English),
    }
}

/// `OPENAEC_THEMA`: `light`, `dark` or one of the viewer's own themes (as `--theme`).
pub fn theme() -> Option<ThemeId> {
    let value = std::env::var("OPENAEC_THEMA").ok().filter(|v| !v.trim().is_empty())?;
    let theme = crate::shell::theme_from_arg(value.trim());
    if theme.is_none() {
        eprintln!("OPENAEC_THEMA={value} is not a theme of the viewer; ignored");
    }
    theme
}

/// The file to open at the start: the command line's, else `OPENAEC_OPEN`. A relative path is
/// taken from the current folder when it is there, else from the work folder.
pub fn start_file(arg: Option<PathBuf>) -> Option<PathBuf> {
    let file = arg.or_else(|| std::env::var_os("OPENAEC_OPEN").filter(|v| !v.is_empty()).map(PathBuf::from))?;
    if file.is_absolute() {
        return Some(file);
    }
    // absolute, so it stays right when the app moves to the work folder
    if file.exists() {
        return Some(std::env::current_dir().map(|d| d.join(&file)).unwrap_or(file));
    }
    Some(work_folder().map(|work| work.join(&file)).unwrap_or(file))
}

/// Run in the work folder in managed mode, so a dialog that cannot be told where to start
/// (`zenity`, when there is no desktop portal) starts there too. Call it after [`start_file`].
pub fn enter_work_folder() {
    if let (true, Some(work)) = (managed(), work_folder()) {
        let _ = std::env::set_current_dir(work);
    }
}

/// A path given by the AI connection (the MCP tools): relative to the work folder when there is
/// one, and in managed mode only inside it, since the SuperCloud reaches the viewer from outside.
pub fn confine(path: &Path) -> Result<PathBuf, String> {
    let work = work_folder();
    let strict = managed() && work.is_some();
    let resolved = confine_in(path, work.as_deref(), strict)?;
    if strict {
        // a link inside the work folder must not lead out of it
        let work = work.and_then(|w| w.canonicalize().ok());
        let real = resolved.canonicalize().ok().or_else(|| {
            let parent = resolved.parent()?.canonicalize().ok()?;
            Some(parent.join(resolved.file_name()?))
        });
        if let (Some(work), Some(real)) = (work, real) {
            if !real.starts_with(&work) {
                return Err(format!("{} lies outside the work folder {}", path.display(), work.display()));
            }
        }
    }
    Ok(resolved)
}

/// [`confine`] without the file system: resolves `path` against `work` and, when `strict`,
/// refuses what lies outside it (also through `..`).
pub fn confine_in(path: &Path, work: Option<&Path>, strict: bool) -> Result<PathBuf, String> {
    let joined = match work {
        Some(work) if path.is_relative() => work.join(path),
        _ => path.to_path_buf(),
    };
    let resolved = normalize(&joined);
    match work {
        Some(work) if strict && !resolved.starts_with(normalize(work)) => {
            Err(format!("{} lies outside the work folder {}; give a path in it", path.display(), work.display()))
        }
        _ => Ok(resolved),
    }
}

/// Removes `.` and resolves `..` without asking the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogs_stay_in_the_work_folder() {
        let p = |s: &str| Some(PathBuf::from(s));
        // without a work folder, next to the model
        assert_eq!(dialog_folder(p("/home/a/models"), None), p("/home/a/models"));
        assert_eq!(dialog_folder(None, None), None);
        // with one, in it, or in the model's folder when that lies inside it
        assert_eq!(dialog_folder(p("/openaec/project/ifc"), p("/openaec")), p("/openaec/project/ifc"));
        assert_eq!(dialog_folder(p("/tmp"), p("/openaec")), p("/openaec"));
        assert_eq!(dialog_folder(None, p("/openaec")), p("/openaec"));
    }

    #[test]
    fn paths_of_the_ai_connection_stay_in_the_work_folder() {
        let work = Path::new("/openaec");
        let p = PathBuf::from;
        // relative paths are in the work folder
        assert_eq!(confine_in(Path::new("project/a.ifc"), Some(work), true), Ok(p("/openaec/project/a.ifc")));
        assert_eq!(confine_in(Path::new("./a.glb"), Some(work), false), Ok(p("/openaec/a.glb")));
        assert_eq!(confine_in(Path::new("/openaec/x/../a.ifc"), Some(work), true), Ok(p("/openaec/a.ifc")));
        // managed: nothing outside it, also not through ..
        assert!(confine_in(Path::new("/etc/passwd"), Some(work), true).is_err());
        assert!(confine_in(Path::new("../etc/passwd"), Some(work), true).is_err());
        assert!(confine_in(Path::new("/openaec-other/a.ifc"), Some(work), true).is_err());
        // not managed, or no work folder: as given
        assert_eq!(confine_in(Path::new("/tmp/a.ifc"), Some(work), false), Ok(p("/tmp/a.ifc")));
        assert_eq!(confine_in(Path::new("a.ifc"), None, true), Ok(p("a.ifc")));
    }

    #[test]
    fn the_language_follows_the_supercloud() {
        assert_eq!(language_from("nl"), Some(Language::Dutch));
        assert_eq!(language_from("nl-BE"), Some(Language::Dutch));
        assert_eq!(language_from("en"), Some(Language::English));
        assert_eq!(language_from("de"), Some(Language::English));
        assert_eq!(language_from(" "), None);
    }
}
