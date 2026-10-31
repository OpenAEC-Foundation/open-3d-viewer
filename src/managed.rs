//! OpenAEC Linux, the streamed SuperCloud environment (OpenAEC-Blueprint#61): the environment
//! sets these variables and the desktop app follows them. Outside it they are not set and nothing
//! changes.

use std::path::PathBuf;

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
}
